// End-to-end scenario: spawn demo → tap each Activatable-equivalent widget
// (Switch / CheckButton / ToggleButton) → verify state via `app_state_eq`.
//
// T019 §3.B: tap dispatch + state push for toggleable widgets. Skipped
// without a GUI display.

import { describe, expect, test } from "bun:test";

import type { WaitCondition } from "../../client/src/types.gen.ts";

import { hasDisplay, spawnDemo } from "./_setup.ts";

const haveDisplay = hasDisplay();

describe.skipIf(!haveDisplay)("scenarios/activatable", () => {
  test("switch tap toggles active and pushes /switch1/active = true", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await client.tap({ selector: "#switch1" });
      const cond: WaitCondition = {
        kind: "app_state_eq",
        path: "/switch1/active",
        value: true,
      };
      const r = await client.wait(cond, { timeoutMs: 3000 });
      expect(r.elapsed_ms).toBeLessThan(3000);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("check button tap toggles active", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await client.tap({ selector: "#check1" });
      const cond: WaitCondition = {
        kind: "app_state_eq",
        path: "/check1/active",
        value: true,
      };
      const r = await client.wait(cond, { timeoutMs: 3000 });
      expect(r.elapsed_ms).toBeLessThan(3000);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("toggle button tap toggles active", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await client.tap({ selector: "#toggle1" });
      const cond: WaitCondition = {
        kind: "app_state_eq",
        path: "/toggle1/active",
        value: true,
      };
      const r = await client.wait(cond, { timeoutMs: 3000 });
      expect(r.elapsed_ms).toBeLessThan(3000);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("multi-widget operations accumulate in the snapshot", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await client.tap({ selector: "#switch1" });
      await client.wait(
        { kind: "app_state_eq", path: "/switch1/active", value: true },
        { timeoutMs: 3000 },
      );
      await client.tap({ selector: "#check1" });
      await client.wait(
        { kind: "app_state_eq", path: "/check1/active", value: true },
        { timeoutMs: 3000 },
      );
      // The snapshot should still contain `/switch1/active` even after the
      // CheckButton callback overwrote part of the state via set_state. This
      // is the regression that the demo's accumulator pattern guards against.
      const snapshot = (await client.state()) as Record<string, unknown>;
      expect(snapshot).toMatchObject({
        switch1: { active: true },
        check1: { active: true },
      });
    } finally {
      await teardown();
    }
  }, 30_000);
});
