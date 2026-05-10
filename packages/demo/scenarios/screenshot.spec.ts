// End-to-end scenario: spawn demo → client.screenshot() → assert valid PNG
// bytes + IHDR. Always saves /tmp/scenario-artifacts/screenshot.png so CI
// can upload it on failure (plan §Q11 / §Q13).
//
// Skipped when no GUI display is detected. CI runs this under xvfb so the
// `DISPLAY` env makes `hasDisplay()` true.

import { describe, expect, test } from "bun:test";
import { existsSync, mkdirSync } from "node:fs";
import type { E2EClient } from "../../client/src/client.ts";
import { HttpError } from "../../client/src/errors.ts";

import { hasDisplay, spawnDemo } from "./_setup.ts";

const ARTIFACT_DIR = "/tmp/scenario-artifacts";
const haveDisplay = hasDisplay();

const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/**
 * The demo emits "server up on" while `connect_activate` is still on the
 * stack, so `window.width()` can be 0 the first time a tokio-side handler
 * inspects it. Retry briefly so the GTK frame clock has a chance to run
 * `measure → allocate → map` before we hard-fail.
 */
async function fetchScreenshotWithRetry(client: E2EClient): Promise<Uint8Array> {
  const deadline = Date.now() + 5_000;
  for (;;) {
    try {
      return await client.screenshot();
    } catch (err) {
      const isLayoutNotReady =
        err instanceof HttpError &&
        err.status === 422 &&
        typeof err.body === "object" &&
        err.body !== null &&
        "error" in err.body &&
        ((err.body as { error?: string }).error === "zero_size" ||
          (err.body as { error?: string }).error === "empty_node");
      if (!isLayoutNotReady || Date.now() >= deadline) throw err;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }
}

function assertValidPng(bytes: Uint8Array, ctx: string): void {
  expect(bytes.length, `${ctx} bytes length`).toBeGreaterThan(100);
  expect(Array.from(bytes.slice(0, 8)), `${ctx} PNG signature`).toEqual(PNG_SIGNATURE);
  // IHDR: bytes 8-15 = chunk length(4) + "IHDR"(4)
  // bytes 16-19 = width BE u32, bytes 20-23 = height BE u32.
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const width = view.getUint32(16, false);
  const height = view.getUint32(20, false);
  expect(width, `${ctx} IHDR width`).toBeGreaterThan(0);
  expect(height, `${ctx} IHDR height`).toBeGreaterThan(0);
}

describe.skipIf(!haveDisplay)("scenarios/screenshot", () => {
  test("returns valid PNG bytes", async () => {
    if (!existsSync(ARTIFACT_DIR)) mkdirSync(ARTIFACT_DIR, { recursive: true });

    // Plan rev2 / M2: 20 s timeout absorbs xvfb cold-start so the run
    // doesn't go flaky on CI. tap-wait.spec.ts keeps the default 10 s.
    const { client, teardown } = await spawnDemo(20_000);
    try {
      // The "server up on" banner fires from inside `connect_activate` before
      // the first frame clock pass, so `window.width()` can still be 0 here.
      // Retry the call across a few short waits so the layout has time to
      // settle. ~half a second is plenty in practice.
      const bytes = await fetchScreenshotWithRetry(client);
      assertValidPng(bytes, "screenshot()");

      // Always save so artifact upload (CI failure) and local debug both work.
      await Bun.write(`${ARTIFACT_DIR}/screenshot.png`, bytes);

      // Path overload exercises the SDK's `Bun.write` branch end-to-end.
      const savedPath = await client.screenshot(`${ARTIFACT_DIR}/screenshot-via-path.png`);
      expect(savedPath).toBe(`${ARTIFACT_DIR}/screenshot-via-path.png`);
    } finally {
      await teardown();
    }
  }, 30_000);
});
