// End-to-end scenario: open a Popover (separate GdkSurface / xdg_popup) and
// assert `GET /test/elements` returns its content widget's bounds composed
// back into the parent-window coordinate space (basis = popup_composed).
// ADR-0004. This is the CI green-source for the cross-surface bounds contract
// (plan M2): under xvfb the popover maps, so it MUST run — a popover that
// fails to open or whose content is not found is a FAILURE, never a skip.

import { describe, expect, test } from "bun:test";

import type { Bounds, ElementInfo, WaitCondition } from "../../client/src/types.gen.ts";

import { hasDisplay, spawnDemo } from "./_setup.ts";

const haveDisplay = hasDisplay();

function findNode(roots: ElementInfo[], name: string): ElementInfo | undefined {
  for (const r of roots) {
    if (r.widget_name === name) return r;
    const hit = findNode(r.children ?? [], name);
    if (hit) return hit;
  }
  return undefined;
}

/** True iff all four corners of `b` lie within the [0,W]×[0,H] window rect. */
function fourCornersInWindow(b: Bounds, w: number, h: number): boolean {
  return b.x >= 0 && b.y >= 0 && b.x + b.width <= w && b.y + b.height <= h;
}

/**
 * Poll the root window's self-relative bounds until it has a real allocation.
 * The first frame after the entry maps can still read (0,0,0,0). Throws on
 * timeout (→ test FAILS, never a skip).
 */
async function settledRootBounds(
  client: Awaited<ReturnType<typeof spawnDemo>>["client"],
  timeoutMs: number,
): Promise<Bounds> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const resp = await client.elements({ maxDepth: 0 });
    const root = resp.roots[0];
    const b = root?.bounds as Bounds | undefined;
    if (b && b.width > 0 && b.height > 0) {
      expect(root.kind).toBe("GtkApplicationWindow");
      return b;
    }
    if (Date.now() >= deadline) {
      throw new Error(`window root bounds did not settle within ${timeoutMs}ms`);
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}

async function waitVisible(
  client: Awaited<ReturnType<typeof spawnDemo>>["client"],
  selector: string,
  timeoutMs: number,
): Promise<void> {
  const cond: WaitCondition = { kind: "selector_visible", selector };
  // On timeout this throws (HTTP 408) and the test FAILS — we deliberately do
  // not catch it into a skip (plan §5.3 / M2).
  await client.wait(cond, { timeoutMs });
}

/**
 * Fetch a widget's bounds, polling until it has a non-degenerate allocation.
 * `selector_visible` fires before the freshly-mapped popover content has been
 * sized in the layout pass, so the first read can show width/height 0; we wait
 * a few frames for it to settle. Failure to find the node or to settle within
 * the deadline throws (→ test FAILS, never a skip).
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
    const node = findNode(resp.roots, name);
    const cur = node?.bounds as Bounds | undefined;
    // Require a non-degenerate allocation AND a value that is stable across two
    // consecutive reads — the popup's position is negotiated a frame or two
    // after it maps, so a single read can catch an un-positioned transient.
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

describe.skipIf(!haveDisplay)("scenarios/popover", () => {
  test("popover content bounds are composed into window space (basis=popup_composed)", async () => {
    const { client, teardown } = await spawnDemo(20_000);
    try {
      await waitVisible(client, "#entry1", 5_000);

      // Wait for the window's first layout pass — right after the entry maps,
      // the root's self-relative bounds can still read (0,0,0,0). Poll until
      // it has a real size so W/H and the anchor reference are meaningful.
      const winB = await settledRootBounds(client, 5_000);
      const W = winB.width;
      const H = winB.height;

      // Anchor button bounds (same surface → basis omitted) as an independent
      // geometric reference. Settled, so its allocation is final.
      const anchor = await settledBounds(client, "#popover-btn", "popover-btn", 5_000);
      expect(anchor.basis).toBeUndefined();

      // Open the popover. Selector tap resolves the widget centre, so the
      // button's position in the (tall) window does not matter.
      await client.tap("#popover-btn");
      await waitVisible(client, "#popover-content", 5_000);

      // Read once the freshly-mapped content has a real allocation (AC1).
      const b = await settledBounds(client, "#popover-content", "popover-content", 5_000);

      // AC1: numeric bounds.
      expect(typeof b.x).toBe("number");
      expect(typeof b.y).toBe("number");
      expect(typeof b.width).toBe("number");
      expect(typeof b.height).toBe("number");
      expect(Number.isFinite(b.x) && Number.isFinite(b.y)).toBe(true);
      expect(b.width).toBeGreaterThan(0);
      expect(b.height).toBeGreaterThan(0);

      // The provenance marker that tells a consumer this is cross-surface.
      expect(b.basis).toBe("popup_composed");

      // Diagnostic breadcrumb for CI triage (xvfb shadow offsets differ).
      console.log(
        `[popover] window=${W}x${H} anchor=${JSON.stringify(anchor)} content=${JSON.stringify(b)}`,
      );

      // AC2: the 4-corners-in-window predicate is computable and holds for a
      // happy-path popover (the compositor constrains xdg_popup on-screen).
      expect(fourCornersInWindow(b, W, H)).toBe(true);

      // M3 independent oracle — geometry vs the anchor, NOT the composition
      // formula. GTK centres the popover horizontally on its anchor, so a
      // sign error on x would offset the content sideways by ~2*position_x.
      const anchorCx = anchor.x + anchor.width / 2;
      const contentCx = b.x + b.width / 2;
      expect(Math.abs(anchorCx - contentCx)).toBeLessThanOrEqual(8);

      // The content hugs the anchor vertically (just below, or flipped just
      // above near the bottom edge). A y-sign flip would push it ~2*position_y
      // (hundreds of px) away, breaking this bound.
      const gapBelow = b.y - (anchor.y + anchor.height);
      const gapAbove = anchor.y - (b.y + b.height);
      const adjacent =
        (gapBelow >= -2 && gapBelow <= 48) || (gapAbove >= -2 && gapAbove <= 48);
      expect(
        adjacent,
        `popover must hug anchor vertically (gapBelow=${gapBelow}, gapAbove=${gapAbove})`,
      ).toBe(true);
    } finally {
      await teardown();
    }
  }, 30_000);

  test("same-surface widget keeps legacy bounds shape (basis omitted)", async () => {
    const { client, teardown } = await spawnDemo(20_000);
    try {
      await waitVisible(client, "#entry1", 5_000);
      const resp = await client.elements({ selector: "#entry1" });
      expect(resp.roots.length).toBe(1);
      // AC3: no regression — ordinary widgets never carry a basis field.
      expect(resp.roots[0].bounds?.basis).toBeUndefined();
    } finally {
      await teardown();
    }
  }, 30_000);
});
