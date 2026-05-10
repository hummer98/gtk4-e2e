// End-to-end scenario: spawn demo → tap #btn1 → wait until #label1.label == "hello".
//
// Skipped when no GUI display is detected. Plan §Q14: this is *not* run in CI
// during Step 5; it must pass locally and informs the Step 6 CI wiring.

import { describe, expect, test } from "bun:test";

import { WaitTimeoutError } from "../../client/src/errors.ts";
import type { WaitCondition } from "../../client/src/types.gen.ts";

import { hasDisplay, spawnDemo } from "./_setup.ts";

const haveDisplay = hasDisplay();

describe.skipIf(!haveDisplay)("scenarios/tap-wait", () => {
  test("button tap changes label", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await client.tap({ selector: "#btn1" });
      const cond: WaitCondition = {
        kind: "state_eq",
        selector: "#label1",
        property: "label",
        value: "hello",
      };
      const r = await client.wait(cond, { timeoutMs: 3000 });
      expect(r.elapsed_ms).toBeLessThan(3000);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("wait times out when no change happens", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      const cond: WaitCondition = {
        kind: "state_eq",
        selector: "#label1",
        property: "label",
        value: "hello",
      };
      // No tap, so the label stays at "waiting...". 500 ms is plenty for
      // multiple poll ticks but well below the 30 s test wrap.
      await expect(client.wait(cond, { timeoutMs: 500 })).rejects.toBeInstanceOf(WaitTimeoutError);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("xy tap path is reachable", async () => {
    // The xy path validates that the SDK → handler → resolve_xy chain runs;
    // we don't assert which widget is hit (theme / window size dependent).
    // Acceptable outcomes:
    //   * tap succeeds (we caught a real widget, optionally followed by wait)
    //   * tap returns 422 out_of_bounds / no_widget_at_point / unsupported
    //     (coordinate missed, but the route was exercised)
    // Anything else (network error, 500, 501) is a regression.
    const { HttpError } = await import("../../client/src/errors.ts");
    const { client, teardown } = await spawnDemo();
    try {
      try {
        await client.tap({ xy: { x: 180, y: 64 } });
      } catch (err) {
        if (!(err instanceof HttpError) || err.status >= 500 || err.status === 404) {
          throw err;
        }
      }
    } finally {
      await teardown();
    }
  }, 30_000);
});
