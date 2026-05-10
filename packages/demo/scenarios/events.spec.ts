// End-to-end scenario: spawn demo → subscribe to /test/events → tap → assert
// the state_change envelope arrives. Filter test confirms server-side
// filtering by asking for the reserved `log_line` kind, which the server
// never produces (plan §3.2 / §10.9).

import { describe, expect, test } from "bun:test";

import type { EventEnvelope } from "../../client/src/types.gen.ts";

import { hasDisplay, spawnDemo } from "./_setup.ts";

const haveDisplay = hasDisplay();

describe.skipIf(!haveDisplay)("scenarios/events", () => {
  test("state_change event arrives after tap", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      const ac = new AbortController();
      // `await` here synchronizes against the WebSocket open, so the tap
      // below cannot race the server-side broadcast subscription.
      const stream = await client.events({
        kinds: ["state_change"],
        signal: ac.signal,
      });

      await client.tap({ selector: "#btn1" });

      const { value, done } = await stream.next();
      expect(done).toBe(false);
      const env = value as EventEnvelope;
      expect(env.kind).toBe("state_change");
      expect(env.data).toMatchObject({
        selector: "#label1",
        property: "label",
      });

      ac.abort();
    } finally {
      await teardown();
    }
  }, 30_000);

  test("filter excludes log_line", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      const ac = new AbortController();
      // log_line is reserved and never produced; subscribing to it alone
      // means a state_change event must be filtered out server-side.
      const stream = await client.events({
        kinds: ["log_line"],
        signal: ac.signal,
      });

      // Pull first, *then* trigger an unrelated state_change. If the filter
      // is broken, the pending pull resolves with the state_change envelope.
      const firstP = stream.next();
      // brief grace for the server-side filter to be in place
      await new Promise((r) => setTimeout(r, 100));
      await client.tap({ selector: "#btn1" });

      const result = await Promise.race([
        firstP.then(() => "received" as const),
        new Promise<"timeout">((r) => setTimeout(() => r("timeout"), 800)),
      ]);
      expect(result).toBe("timeout");

      ac.abort();
    } finally {
      await teardown();
    }
  }, 30_000);
});
