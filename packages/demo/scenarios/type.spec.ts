// End-to-end scenario: spawn demo → type into #input1 → wait until #input1.text == "hello".
//
// Skipped when no GUI display is detected. Step 9 plan §3.11.

import { describe, expect, test } from "bun:test";

import type { WaitCondition } from "../../client/src/types.gen.ts";

import { hasDisplay, spawnDemo } from "./_setup.ts";

const haveDisplay = hasDisplay();

describe.skipIf(!haveDisplay)("scenarios/type", () => {
  test("type fills entry text", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await client.type("#input1", "hello");
      const cond: WaitCondition = {
        kind: "state_eq",
        selector: "#input1",
        property: "text",
        value: "hello",
      };
      const r = await client.wait(cond, { timeoutMs: 3000 });
      expect(r.elapsed_ms).toBeLessThan(3000);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("type on missing selector returns 404", async () => {
    const { HttpError } = await import("../../client/src/errors.ts");
    const { client, teardown } = await spawnDemo();
    try {
      let thrown: unknown;
      try {
        await client.type("#nosuch", "x");
      } catch (err) {
        thrown = err;
      }
      expect(thrown).toBeInstanceOf(HttpError);
      expect((thrown as InstanceType<typeof HttpError>).status).toBe(404);
    } finally {
      await teardown();
    }
  }, 30_000);
});
