// End-to-end scenario: spawn demo → pinch the DrawingArea → wait until
// `#zoom-pos.label == "1.50"`.
//
// Skipped when no GUI display is detected. Plan T015 §10.2.

import { describe, expect, test } from "bun:test";

import type { WaitCondition } from "../../client/src/types.gen.ts";

import { hasDisplay, spawnDemo } from "./_setup.ts";

const haveDisplay = hasDisplay();

async function waitForZoom(client: Awaited<ReturnType<typeof spawnDemo>>["client"]) {
  // Block until the DrawingArea has been mapped — `gtk::Widget::allocation()`
  // is 0×0 until the toplevel is mapped, and `pinch` would otherwise fail
  // with out_of_bounds even on legal coordinates. Same trick as
  // swipe.spec.ts's `waitForScroll`.
  await client.wait(
    { kind: "selector_visible", selector: "#zoom1" },
    { timeoutMs: 3000 },
  );
  await new Promise((res) => setTimeout(res, 200));
}

describe.skipIf(!haveDisplay)("scenarios/pinch", () => {
  test("pinch with scale=1.5 updates #zoom-pos.label to '1.50'", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await waitForZoom(client);
      // The DrawingArea sits below the ScrolledWindow. Use elements() to find
      // the centre robustly: hard-coding coords would couple this test to
      // demo layout choices that are likely to drift.
      const els = await client.elements({ selector: "#zoom1" });
      const root = els.roots[0];
      if (!root?.bounds) {
        throw new Error("expected #zoom1 to expose bounds");
      }
      const cx = Math.round(root.bounds.x + root.bounds.width / 2);
      const cy = Math.round(root.bounds.y + root.bounds.height / 2);
      await client.pinch({ x: cx, y: cy }, 1.5, 200);
      const cond: WaitCondition = {
        kind: "state_eq",
        selector: "#zoom-pos",
        property: "label",
        value: "1.50",
      };
      const r = await client.wait(cond, { timeoutMs: 3000 });
      expect(r.elapsed_ms).toBeLessThan(3000);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("pinch on non-zoomable area is rejected", async () => {
    const { HttpError } = await import("../../client/src/errors.ts");
    const { client, teardown } = await spawnDemo();
    try {
      await waitForZoom(client);
      // (5, 5) lives in the entry / button area above the DrawingArea — no
      // `GestureZoom` ancestor at that point. Race-tolerant in the same
      // shape as swipe.spec.ts: 404 on settled hosts, 422 out_of_bounds when
      // the toplevel allocation is still settling.
      let thrown: unknown;
      try {
        await client.pinch({ x: 5, y: 5 }, 1.5, 100);
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

  test("pinch with scale=0 returns 422 invalid_scale", async () => {
    const { HttpError } = await import("../../client/src/errors.ts");
    const { client, teardown } = await spawnDemo();
    try {
      await waitForZoom(client);
      let thrown: unknown;
      try {
        await client.pinch({ x: 100, y: 100 }, 0, 100);
      } catch (err) {
        thrown = err;
      }
      expect(thrown).toBeInstanceOf(HttpError);
      const httpErr = thrown as InstanceType<typeof HttpError>;
      expect(httpErr.status).toBe(422);
      // body shape mirrors swipe_zero_duration_422 — server emits a JSON
      // object with `error: "invalid_scale", reason: "non_positive"`.
      const body = httpErr.body as { error?: string; reason?: string };
      expect(body?.error).toBe("invalid_scale");
      expect(body?.reason).toBe("non_positive");
    } finally {
      await teardown();
    }
  }, 30_000);
});
