// End-to-end scenario: spawn demo → tap #btn1 → verify the snapshot at
// `GET /test/state` and `WaitCondition::AppStateEq` reflects the click.
//
// T019 §3.A: app-defined state schema. Skipped without a GUI display.

import { describe, expect, test } from "bun:test";

import type { WaitCondition } from "../../client/src/types.gen.ts";

import { hasDisplay, spawnDemo } from "./_setup.ts";

const haveDisplay = hasDisplay();

describe.skipIf(!haveDisplay)("scenarios/state", () => {
  test("apply button push updates /test/state snapshot", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      // Pre-tap: server has not received any set_state yet → null.
      const before = await client.state();
      expect(before).toBeNull();

      await client.tap({ selector: "#btn1" });
      // Wait until the demo has pushed `/session/mode = "applied"`.
      const cond: WaitCondition = {
        kind: "app_state_eq",
        path: "/session/mode",
        value: "applied",
      };
      const r = await client.wait(cond, { timeoutMs: 3000 });
      expect(r.elapsed_ms).toBeLessThan(3000);

      const after = (await client.state()) as Record<string, unknown>;
      expect(after).toMatchObject({
        session: { mode: "applied" },
        label1: { text: "hello" },
        click_count: 1,
      });
    } finally {
      await teardown();
    }
  }, 30_000);

  test("app_state_eq waits for click_count to reach 2", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await client.tap({ selector: "#btn1" });
      await client.wait(
        { kind: "app_state_eq", path: "/click_count", value: 1 },
        { timeoutMs: 3000 },
      );
      await client.tap({ selector: "#btn1" });
      const r = await client.wait(
        { kind: "app_state_eq", path: "/click_count", value: 2 },
        { timeoutMs: 3000 },
      );
      expect(r.elapsed_ms).toBeLessThan(3000);
    } finally {
      await teardown();
    }
  }, 30_000);
});
