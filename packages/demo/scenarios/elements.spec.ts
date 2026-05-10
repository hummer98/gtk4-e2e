// End-to-end scenario: spawn demo → query widget tree via /test/elements.
//
// Skipped when no GUI display is detected. Step 14 plan §11.3.

import { describe, expect, test } from "bun:test";

import type {
  ElementInfo,
  WaitCondition,
} from "../../client/src/types.gen.ts";

import { hasDisplay, spawnDemo } from "./_setup.ts";

const haveDisplay = hasDisplay();

function recursiveCount(info: ElementInfo): number {
  const kids = info.children ?? [];
  return 1 + kids.reduce((acc, c) => acc + recursiveCount(c), 0);
}

async function waitForEntryVisible(
  client: Awaited<ReturnType<typeof spawnDemo>>["client"],
): Promise<void> {
  const cond: WaitCondition = {
    kind: "selector_visible",
    selector: "#entry1",
  };
  await client.wait(cond, { timeoutMs: 3000 });
}

describe.skipIf(!haveDisplay)("scenarios/elements", () => {
  test("dumps the full window tree when called without arguments", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await waitForEntryVisible(client);
      const resp = await client.elements();
      expect(resp.roots.length).toBe(1);
      expect(resp.roots[0].kind).toBe("GtkApplicationWindow");
      expect(resp.count).toBeGreaterThan(1);
      const computed = resp.roots.reduce((a, r) => a + recursiveCount(r), 0);
      expect(resp.count).toBe(computed);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("returns the matching subtree for a name selector", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await waitForEntryVisible(client);
      const resp = await client.elements({ selector: "#input1" });
      expect(resp.roots.length).toBe(1);
      expect(resp.roots[0].kind).toBe("GtkEntry");
      expect(resp.roots[0].widget_name).toBe("input1");
    } finally {
      await teardown();
    }
  }, 30_000);

  test("returns empty roots on selector miss (HTTP 200)", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await waitForEntryVisible(client);
      const resp = await client.elements({ selector: "#nosuch" });
      expect(resp.roots).toEqual([]);
      expect(resp.count).toBe(0);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("max_depth=0 returns root only", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await waitForEntryVisible(client);
      const resp = await client.elements({ maxDepth: 0 });
      expect(resp.roots.length).toBe(1);
      expect(resp.roots[0].children ?? []).toEqual([]);
      expect(resp.count).toBe(1);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("class selector .primary matches entry1", async () => {
    const { client, teardown } = await spawnDemo();
    try {
      await waitForEntryVisible(client);
      const resp = await client.elements({ selector: ".primary" });
      expect(resp.roots.length).toBe(1);
      expect(resp.roots[0].widget_name).toBe("entry1");
    } finally {
      await teardown();
    }
  }, 30_000);
});
