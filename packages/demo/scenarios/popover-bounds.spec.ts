// End-to-end scenario: open a top-anchored Popover (separate GdkSurface /
// xdg_popup) and check that `GET /test/elements` composes its content widget's
// bounds back into the parent-window coordinate space (ADR-0004).
//
// This is the CI green-source for the cross-surface bounds composition. Before
// this scenario the composition (fe8eba3, issue #5) was only ever verified on
// macOS/quartz: its Rust integration test skips whenever the host does not
// realize a popup surface, which is exactly the headless CI case. Under xvfb
// this popover DOES map (it is anchored near the top of the window, above the
// 720px screen fold), so the test MUST run — a popover that fails to open is a
// FAILURE, never a skip.
//
// The assertions check the composed bounds *against the anchor widget*, not
// against the composition formula, so a sign error cannot pass by
// self-consistency:
//   - GTK centres the popover horizontally on its anchor, so a flipped x sign
//     offsets the content sideways by ~2*position_x.
//   - a flipped y sign pushes the content hundreds of px off the anchor.
//
// We deliberately do NOT assert window containment: a popover is constrained to
// the *screen*, not the window, so a correctly composed popover can legitimately
// fall outside the window rect near an edge (ADR-0004 m4).

import { describe, expect, test } from "bun:test";

import type { ElementInfo, WaitCondition } from "../../client/src/types.gen.ts";

import { hasDisplay, spawnDemo } from "./_setup.ts";

const haveDisplay = hasDisplay();

type Bounds = NonNullable<ElementInfo["bounds"]>;

function findNode(roots: ElementInfo[], name: string): ElementInfo | undefined {
  for (const r of roots) {
    if (r.widget_name === name) return r;
    const hit = findNode(r.children ?? [], name);
    if (hit) return hit;
  }
  return undefined;
}

async function waitVisible(
  client: Awaited<ReturnType<typeof spawnDemo>>["client"],
  selector: string,
  timeoutMs: number,
): Promise<void> {
  const cond: WaitCondition = { kind: "selector_visible", selector };
  await client.wait(cond, { timeoutMs });
}

/**
 * Wait for the window's first layout pass. Right after a widget maps, the
 * window can still be resizing to its content, and widgets near the top read
 * transient pre-allocation bounds (negative x, minimum size). Poll the root's
 * self-relative bounds until it has a real allocation so anchor reads below are
 * meaningful.
 */
async function waitRootSettled(
  client: Awaited<ReturnType<typeof spawnDemo>>["client"],
  timeoutMs: number,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const resp = await client.elements({ maxDepth: 0 });
    const b = resp.roots[0]?.bounds as Bounds | undefined;
    if (b && b.width > 0 && b.height > 0) return;
    if (Date.now() >= deadline) {
      throw new Error(`window root bounds did not settle within ${timeoutMs}ms`);
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}

/**
 * Fetch a widget's bounds, polling until the allocation is non-degenerate and
 * stable across two consecutive reads — a popup's position is negotiated a
 * frame or two after it maps, so a single read can catch a transient. Failure
 * to find the node or to settle within the deadline throws (→ test FAILS,
 * never a skip).
 *
 * Walks the FULL tree (`elements({})`) and finds the node by name rather than
 * scoping with `elements({selector})`. On X11/xvfb a selector-scoped query
 * returns a widget that lives inside an open popover but with `bounds` null:
 * the selector path starts the matched widget with no popover frame, so its
 * cross-surface bounds are not composed (issue #23). The full unfiltered walk
 * composes them correctly. This scenario is about the bounds composition, so it
 * sidesteps that bug by walking the whole tree.
 */
async function settledBounds(
  client: Awaited<ReturnType<typeof spawnDemo>>["client"],
  name: string,
  timeoutMs: number,
): Promise<Bounds> {
  const deadline = Date.now() + timeoutMs;
  const eq = (a?: Bounds, b?: Bounds) =>
    !!a && !!b && a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
  let prev: Bounds | undefined;
  for (;;) {
    const resp = await client.elements({});
    const cur = findNode(resp.roots, name)?.bounds as Bounds | undefined;
    if (cur && cur.width > 0 && cur.height > 0 && eq(prev, cur)) return cur;
    prev = cur;
    if (Date.now() >= deadline) {
      throw new Error(
        `bounds for ${name} did not settle within ${timeoutMs}ms (last=${JSON.stringify(cur)})`,
      );
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}

describe.skipIf(!haveDisplay)("scenarios/popover-bounds", () => {
  test("popover content bounds compose into window space and hug the anchor", async () => {
    const { client, teardown } = await spawnDemo(20_000);
    try {
      await waitVisible(client, "#bounds-popover-btn", 5_000);
      await waitRootSettled(client, 5_000);

      // Anchor bounds (same surface, ordinary compute_bounds) as an
      // independent geometric reference.
      const anchor = await settledBounds(client, "bounds-popover-btn", 5_000);

      await client.tap("#bounds-popover-btn");
      await waitVisible(client, "#bounds-popover-content", 5_000);

      let b: Bounds;
      try {
        b = await settledBounds(client, "bounds-popover-content", 5_000);
      } catch (err) {
        // On failure, dump the popover subtree so a CI triage sees whether the
        // node is absent (never mapped) or present with null bounds (the
        // composition declined on this backend).
        const full = await client.elements({});
        const dump = (n: ElementInfo, d: number): string =>
          [
            `${"  ".repeat(d)}${n.kind} ${n.widget_name ?? "-"} ${JSON.stringify(n.bounds ?? null)}`,
            ...(n.children ?? []).map((c) => dump(c, d + 1)),
          ].join("\n");
        const node = findNode(full.roots, "bounds-popover-content");
        console.log(
          `[popover][diag] bounds-popover-content in tree: ${node ? "YES" : "NO"}` +
            (node ? ` bounds=${JSON.stringify(node.bounds ?? null)}` : ""),
        );
        console.log(`[popover][diag] tree:\n${full.roots.map((r) => dump(r, 0)).join("\n")}`);
        throw err;
      }

      // Numeric, non-degenerate bounds (the composition produced a rect).
      expect(Number.isFinite(b.x) && Number.isFinite(b.y)).toBe(true);
      expect(b.width).toBeGreaterThan(0);
      expect(b.height).toBeGreaterThan(0);

      // Diagnostic breadcrumb — the actual numbers are the point of this run
      // (xvfb vs quartz composition can be compared from CI logs).
      console.log(`[popover] anchor=${JSON.stringify(anchor)} content=${JSON.stringify(b)}`);

      // Independent oracle 1: horizontal centring. A flipped x sign offsets the
      // content sideways by ~2*position_x, breaking this bound.
      const anchorCx = anchor.x + anchor.width / 2;
      const contentCx = b.x + b.width / 2;
      expect(Math.abs(anchorCx - contentCx)).toBeLessThanOrEqual(24);

      // Independent oracle 2: the content hugs the anchor vertically (just
      // below, or flipped just above). A flipped y sign pushes it ~2*position_y
      // (hundreds of px) away, breaking this bound.
      const gapBelow = b.y - (anchor.y + anchor.height);
      const gapAbove = anchor.y - (b.y + b.height);
      const adjacent = (gapBelow >= -2 && gapBelow <= 64) || (gapAbove >= -2 && gapAbove <= 64);
      expect(
        adjacent,
        `popover must hug anchor vertically (gapBelow=${gapBelow}, gapAbove=${gapAbove})`,
      ).toBe(true);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("same-surface widget keeps legacy bounds shape", async () => {
    const { client, teardown } = await spawnDemo(20_000);
    try {
      await waitVisible(client, "#entry1", 5_000);
      // Regression: an ordinary widget's bounds are unchanged by the popover
      // composition path.
      const b = await settledBounds(client, "entry1", 5_000);
      expect(b.width).toBeGreaterThan(0);
      expect(b.height).toBeGreaterThan(0);
    } finally {
      await teardown();
    }
  }, 30_000);
});
