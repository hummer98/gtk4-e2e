// End-to-end scenario: spawn demo → expectScreenshot("main-window") against
// a repo-committed baseline. Baseline is pixel-exact for the CI Linux+xvfb
// 1280x720x24 environment; macOS Quartz backend renders differently, so we
// skip on non-Linux. To regenerate the baseline run
// `bash packages/demo/scripts/gen-visual-baseline.sh` (Docker required).

import { describe, expect, test } from "bun:test";

import type { E2EClient } from "../../client/src/client.ts";
import { HttpError } from "../../client/src/errors.ts";

import { hasDisplay, spawnDemo } from "./_setup.ts";

const haveDisplay = hasDisplay();
const isLinux = process.platform === "linux";

describe.skipIf(!haveDisplay || !isLinux)("scenarios/visual-regression", () => {
  test("matches main-window baseline", async () => {
    const updateBaseline = process.env["GTK4_E2E_UPDATE_BASELINE"] === "1";
    // Plan rev2 / M2: 20 s spawn timeout absorbs xvfb cold-start so the run
    // doesn't go flaky on CI (matches screenshot.spec.ts).
    const { client, teardown } = await spawnDemo(20_000);
    try {
      await warmupLayout(client);
      // threshold=0.5 absorbs per-pixel YIQ noise from text rendering /
      // border AA. Round 4 may introduce a region mask or per-CI-image
      // rebaseline workflow.
      const result = await client.expectScreenshot("main-window", {
        updateBaseline,
        threshold: 0.5,
      });
      if (!result.match) {
        // Surface diff/actual paths so the CI log lets us locate the artifacts
        // even if scenarios job's artifact upload doesn't include __screenshots__/.
        console.log(
          `actual=${result.actualPath} diff=${result.diffPath} diffPixels=${result.diffPixels}/${result.totalPixels}`,
        );
      }
      // GTK Entry focus ring + cursor blink are non-deterministic between the
      // baseline-capture frame and the CI-replay frame. Allow up to ~2% diff
      // to absorb that without missing real structural regressions (a layout
      // shift would dwarf this budget).
      const MAX_DIFF_PIXELS = Math.floor(result.totalPixels * 0.02);
      expect(result.diffPixels).toBeLessThanOrEqual(MAX_DIFF_PIXELS);
    } finally {
      await teardown();
    }
  }, 30_000);
});

// connect_activate fires the server-up banner before the first frame clock
// pass, so the very first screenshot can still hit zero_size / empty_node.
// Mirrors screenshot.spec.ts:fetchScreenshotWithRetry. Plan §"Step 1" leaves
// dedup to a follow-up; here we keep the helper inline to avoid touching
// _setup.ts (completion criterion §3).
async function warmupLayout(client: E2EClient): Promise<void> {
  const deadline = Date.now() + 5_000;
  for (;;) {
    try {
      await client.screenshot();
      return;
    } catch (err) {
      const ready =
        err instanceof HttpError &&
        err.status === 422 &&
        typeof err.body === "object" &&
        err.body !== null &&
        "error" in err.body &&
        ((err.body as { error?: string }).error === "zero_size" ||
          (err.body as { error?: string }).error === "empty_node");
      if (ready && Date.now() < deadline) {
        await new Promise((r) => setTimeout(r, 100));
        continue;
      }
      throw err;
    }
  }
}
