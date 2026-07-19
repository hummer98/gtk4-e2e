// End-to-end scenario: open a Popover (separate GdkSurface / xdg_popup) and
// check that `GET /test/elements` returns its content widget's bounds composed
// back into the parent-window coordinate space.
//
// The composition landed in `fe8eba3` (issue #5) but shipped without an
// executable oracle: its Rust integration test skips whenever the host does not
// realize a popup surface, which is exactly the headless CI case. So the sign
// of the toplevel `surface_transform` — flagged as "confirm on a real display"
// in that change's own plan — was never confirmed by CI. This spec closes that
// gap and is the green-source for the cross-surface bounds contract.
//
// The assertions deliberately check geometry *against the anchor*, not against
// the composition formula, so a sign error cannot pass by self-consistency: a
// flipped x offsets the content sideways by ~2*position_x, a flipped y pushes
// it hundreds of px off the anchor. Measured on macOS/quartz the anchor and
// content centres agree to 0.5px.
//
// Reuses the existing `#open-popover` / `#confirm-popover` fixture (issue #10)
// so the demo UI — and the visual-regression baseline — stay untouched.

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

/** True iff all four corners of `b` lie within the [0,W]x[0,H] window rect. */
function fourCornersInWindow(b: Bounds, w: number, h: number): boolean {
  return b.x >= 0 && b.y >= 0 && b.x + b.width <= w && b.y + b.height <= h;
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
 * Poll the root window's self-relative bounds until it has a real allocation:
 * the first frame after the entry maps can still read (0,0,0,0).
 */
async function settledRootBounds(
  client: Awaited<ReturnType<typeof spawnDemo>>["client"],
  timeoutMs: number,
): Promise<Bounds> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const resp = await client.elements({ maxDepth: 0 });
    const b = resp.roots[0]?.bounds as Bounds | undefined;
    if (b && b.width > 0 && b.height > 0) return b;
    if (Date.now() >= deadline) {
      throw new Error(`window root bounds did not settle within ${timeoutMs}ms`);
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}

/**
 * Fetch a widget's bounds, polling until the allocation is non-degenerate and
 * stable across two consecutive reads — a popup's position is negotiated a
 * frame or two after it maps, so a single read can catch a transient.
 */
async function settledBounds(
  client: Awaited<ReturnType<typeof spawnDemo>>["client"],
  selector: string,
  name: string,
  timeoutMs: number,
): Promise<Bounds> {
  const deadline = Date.now() + timeoutMs;
  const eq = (a?: Bounds, b?: Bounds) =>
    !!a && !!b && a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
  let prev: Bounds | undefined;
  for (;;) {
    const resp = await client.elements({ selector });
    const cur = findNode(resp.roots, name)?.bounds as Bounds | undefined;
    if (cur && cur.width > 0 && cur.height > 0 && eq(prev, cur)) return cur;
    prev = cur;
    if (Date.now() >= deadline) {
      throw new Error(
        `bounds for ${selector} did not settle within ${timeoutMs}ms (last=${JSON.stringify(cur)})`,
      );
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}

describe.skipIf(!haveDisplay)("scenarios/popover-bounds", () => {
  test("popover content bounds compose into window space and hug the anchor", async () => {
    const { client, teardown } = await spawnDemo(20_000);
    try {
      await waitVisible(client, "#entry1", 5_000);

      const winB = await settledRootBounds(client, 5_000);
      const W = winB.width;
      const H = winB.height;

      // Anchor bounds (same surface, ordinary compute_bounds) as an
      // independent geometric reference.
      const anchor = await settledBounds(client, "#open-popover", "open-popover", 5_000);

      await client.tap("#open-popover");
      await waitVisible(client, "#popover-confirm", 5_000);

      const b = await settledBounds(client, "#popover-confirm", "popover-confirm", 5_000);

      // Numeric, non-degenerate bounds.
      expect(Number.isFinite(b.x) && Number.isFinite(b.y)).toBe(true);
      expect(b.width).toBeGreaterThan(0);
      expect(b.height).toBeGreaterThan(0);

      // Diagnostic breadcrumb — the actual numbers are the point of this run.
      console.log(
        `[popover] window=${W}x${H} anchor=${JSON.stringify(anchor)} content=${JSON.stringify(b)}`,
      );

      // The compositor constrains xdg_popup on-screen, so a correctly composed
      // popover sits inside the window rect.
      expect(fourCornersInWindow(b, W, H)).toBe(true);

      // Independent oracle: GTK centres the popover horizontally on its anchor,
      // so a sign error on x offsets the content sideways by ~2*position_x.
      const anchorCx = anchor.x + anchor.width / 2;
      const contentCx = b.x + b.width / 2;
      expect(Math.abs(anchorCx - contentCx)).toBeLessThanOrEqual(24);

      // The content hugs the anchor vertically (just below, or flipped just
      // above near the bottom edge). A y-sign flip pushes it ~2*position_y
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
});
