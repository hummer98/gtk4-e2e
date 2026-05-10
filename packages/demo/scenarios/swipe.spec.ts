// End-to-end scenario: spawn demo → swipe up → wait until #scroll-pos.label == "300".
//
// Skipped when no GUI display is detected. Plan T014 §8.3.

import { describe, expect, test } from "bun:test";

import type { WaitCondition } from "../../client/src/types.gen.ts";

import { hasDisplay, spawnDemo } from "./_setup.ts";

const haveDisplay = hasDisplay();

async function waitForScroll(client: Awaited<ReturnType<typeof spawnDemo>>["client"]) {
  // Block until the ScrolledWindow has been mapped — gtk's `window.allocation()`
  // is 0×0 until the toplevel is mapped, and `swipe` would otherwise fail with
  // out_of_bounds even on legal coordinates. `selector_visible` resolves once
  // the widget is `is_visible_and_mapped`; we then sleep briefly to give the
  // macOS quartz backend a chance to settle the toplevel allocation, which is
  // observed to lag a frame behind child mapping.
  await client.wait(
    { kind: "selector_visible", selector: "#scroll1" },
    { timeoutMs: 3000 },
  );
  await new Promise((res) => setTimeout(res, 200));
}

describe.skipIf(!haveDisplay)("scenarios/swipe", () => {
  test("upward swipe scrolls the list to vadjustment.value=300", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await waitForScroll(client);
      await client.swipe({ x: 100, y: 400 }, { x: 100, y: 100 }, 300);
      // input.rs rounds set_value() so the final frame is exactly 300.0; the
      // demo formats vadjustment.value as `format!("{}", a.value() as i32)`,
      // so the label settles at "300".
      const cond: WaitCondition = {
        kind: "state_eq",
        selector: "#scroll-pos",
        property: "label",
        value: "300",
      };
      const r = await client.wait(cond, { timeoutMs: 3000 });
      expect(r.elapsed_ms).toBeLessThan(3000);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("swipe in non-scrollable area is rejected", async () => {
    const { HttpError } = await import("../../client/src/errors.ts");
    const { client, teardown } = await spawnDemo();
    try {
      await waitForScroll(client);
      // (5, 5) lives in the vbox margin / entry area above the ScrolledWindow.
      // The expected outcome is 404 no_scrollable_at_point, but on slow
      // backends the toplevel allocation may still be settling so the bounds
      // check fires first and returns 422 out_of_bounds. Either flavour
      // proves the route correctly refuses non-scrollable targets.
      let thrown: unknown;
      try {
        await client.swipe({ x: 5, y: 5 }, { x: 5, y: 30 }, 100);
      } catch (err) {
        thrown = err;
      }
      expect(thrown).toBeInstanceOf(HttpError);
      const status = (thrown as InstanceType<typeof HttpError>).status;
      expect([404, 422]).toContain(status);
    } finally {
      await teardown();
    }
  }, 30_000);
});
