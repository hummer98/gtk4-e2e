// End-to-end scenario: spawn demo → query widget tree via /test/elements.
//
// Skipped when no GUI display is detected. Step 14 plan §11.3.

import { describe, expect, test } from "bun:test";

import type { ElementInfo, WaitCondition } from "../../client/src/types.gen.ts";

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

  test("props= reads GObject properties per matched widget", async () => {
    // Verifies the opt-in property read-through end-to-end:
    //   - typing into #entry1 changes the GtkEntry `text` value
    //   - elements({ selector, props: ["text"] }) returns the live value
    //   - an unknown property surfaces the $missing sentinel
    //   - omitting props leaves `properties` undefined (legacy shape)
    const { client, teardown } = await spawnDemo();
    try {
      await waitForEntryVisible(client);

      // Drive the live text via the public type() helper so the value
      // we read back is provably the GTK-computed state, not the static
      // initial.
      await client.type("#entry1", "hello-props");

      const resp = await client.elements({
        selector: "#entry1",
        props: ["text", "no-such-property"],
      });
      expect(resp.roots.length).toBe(1);
      const node = resp.roots[0];
      expect(node.kind).toBe("GtkEntry");
      expect(node.properties).toBeDefined();
      const map = node.properties as Record<string, unknown>;
      expect(map.text).toBe("hello-props");
      expect(map["no-such-property"]).toEqual({ $missing: true });

      const legacy = await client.elements({ selector: "#entry1" });
      expect(legacy.roots[0].properties).toBeUndefined();
    } finally {
      await teardown();
    }
  }, 30_000);

  test("text field carries display text for Label / Entry (issue #17)", async () => {
    // Text-bearing widgets expose their human-visible text as a stable
    // `text` field without opting into props=; other widgets omit it.
    const { client, teardown } = await spawnDemo();
    try {
      await waitForEntryVisible(client);

      const entry = await client.elements({ selector: "#entry1", maxDepth: 0 });
      expect(entry.roots[0].text).toBe("hello"); // demo's initial Entry text

      const label = await client.elements({ selector: "#label1", maxDepth: 0 });
      expect(label.roots[0].text).toBe("waiting..."); // initial Label text

      // Non-text widgets (the ApplicationWindow root) carry no text field.
      const win = await client.elements({ maxDepth: 0 });
      expect(win.roots[0].kind).toBe("GtkApplicationWindow");
      expect(win.roots[0].text ?? null).toBeNull();
    } finally {
      await teardown();
    }
  }, 30_000);

  test("props= stringifies GEnum / GFlags values (issue #17)", async () => {
    // GEnum properties serialize as their nick string, GFlags as an array
    // of nick strings; boxed types keep the $unsupported sentinel.
    const { client, teardown } = await spawnDemo();
    try {
      await waitForEntryVisible(client);

      const sw = await client.elements({
        selector: "#scroll1",
        props: ["vscrollbar-policy"],
        maxDepth: 0,
      });
      const swMap = sw.roots[0].properties as Record<string, unknown>;
      expect(swMap["vscrollbar-policy"]).toBe("automatic");

      const entry = await client.elements({
        selector: "#entry1",
        props: ["input-purpose", "input-hints", "attributes"],
        maxDepth: 0,
      });
      const map = entry.roots[0].properties as Record<string, unknown>;
      expect(map["input-purpose"]).toBe("free-form"); // GEnum → nick
      expect(Array.isArray(map["input-hints"])).toBe(true); // GFlags → array
      expect(map.attributes).toEqual({ $unsupported: "PangoAttrList" }); // boxed stays sentinel
    } finally {
      await teardown();
    }
  }, 30_000);

  test("props=['*'] enumerates every readable GObject property", async () => {
    // Wildcard expansion at the server side: every GObject property
    // advertised by the matched widget's class shows up in `properties`,
    // with unsupported types degrading to the `$unsupported` sentinel.
    const { client, teardown } = await spawnDemo();
    try {
      await waitForEntryVisible(client);
      const resp = await client.elements({
        selector: "#entry1",
        props: ["*"],
      });
      expect(resp.roots.length).toBe(1);
      const node = resp.roots[0];
      expect(node.kind).toBe("GtkEntry");
      const map = node.properties as Record<string, unknown>;
      expect(map).toBeDefined();
      // A representative slice: every GtkWidget exposes these, and
      // GtkEntry adds `text` on top.
      for (const required of [
        "name",
        "visible",
        "sensitive",
        "width-request",
        "height-request",
        "text",
      ]) {
        expect(map[required], `wildcard should include ${required}`).toBeDefined();
      }
    } finally {
      await teardown();
    }
  }, 30_000);
});
