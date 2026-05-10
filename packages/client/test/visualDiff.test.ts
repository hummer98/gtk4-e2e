import "./_setup.ts";

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { PNG } from "pngjs";

import { VisualDiffError } from "../src/errors.ts";
import { expectScreenshot } from "../src/visualDiff.ts";

// Build a solid-color RGBA PNG of the given size.
function makePng(
  width: number,
  height: number,
  rgba: [number, number, number, number],
): Uint8Array {
  const png = new PNG({ width, height });
  for (let i = 0; i < width * height; i++) {
    png.data[i * 4 + 0] = rgba[0];
    png.data[i * 4 + 1] = rgba[1];
    png.data[i * 4 + 2] = rgba[2];
    png.data[i * 4 + 3] = rgba[3];
  }
  return new Uint8Array(PNG.sync.write(png));
}

// Solid-color PNG with one pixel at (px, py) replaced by `pixelRgba`.
function makePngWith1pxDiff(
  width: number,
  height: number,
  baseRgba: [number, number, number, number],
  px: number,
  py: number,
  pixelRgba: [number, number, number, number],
): Uint8Array {
  const png = new PNG({ width, height });
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const idx = (y * width + x) * 4;
      const rgba = x === px && y === py ? pixelRgba : baseRgba;
      png.data[idx + 0] = rgba[0];
      png.data[idx + 1] = rgba[1];
      png.data[idx + 2] = rgba[2];
      png.data[idx + 3] = rgba[3];
    }
  }
  return new Uint8Array(PNG.sync.write(png));
}

describe("expectScreenshot", () => {
  let scratch: string;

  beforeEach(() => {
    scratch = mkdtempSync(join(tmpdir(), "gtk4-e2e-vdiff-"));
  });

  afterEach(() => {
    rmSync(scratch, { recursive: true, force: true });
  });

  test("returns match=true with diffPixels=0 when actual === baseline", async () => {
    const png = makePng(10, 10, [255, 0, 0, 255]);
    await Bun.write(join(scratch, "main-window.png"), png);

    const result = await expectScreenshot(png, "main-window", { baselineDir: scratch });

    expect(result.match).toBe(true);
    expect(result.diffPixels).toBe(0);
    expect(result.totalPixels).toBe(100);
    expect(result.actualPath).toBeUndefined();
    expect(result.diffPath).toBeUndefined();
    expect(existsSync(join(scratch, "main-window.actual.png"))).toBe(false);
    expect(existsSync(join(scratch, "main-window.diff.png"))).toBe(false);
  });

  test("returns match=false with diffPixels=1 and writes actual/diff PNGs", async () => {
    const baseline = makePng(10, 10, [255, 0, 0, 255]);
    const actual = makePngWith1pxDiff(10, 10, [255, 0, 0, 255], 5, 5, [0, 255, 0, 255]);
    await Bun.write(join(scratch, "main-window.png"), baseline);

    const result = await expectScreenshot(actual, "main-window", { baselineDir: scratch });

    expect(result.match).toBe(false);
    expect(result.diffPixels).toBe(1);
    expect(result.totalPixels).toBe(100);
    expect(result.actualPath).toBe(join(scratch, "main-window.actual.png"));
    expect(result.diffPath).toBe(join(scratch, "main-window.diff.png"));
    expect(existsSync(result.actualPath as string)).toBe(true);
    expect(existsSync(result.diffPath as string)).toBe(true);
    // diff PNG signature byte (0x89) confirms a real PNG was written.
    const diffBytes = await Bun.file(result.diffPath as string).bytes();
    expect(diffBytes[0]).toBe(0x89);
  });

  test("returns match=false on size mismatch (no diff PNG, but actual written)", async () => {
    const baseline = makePng(10, 10, [255, 0, 0, 255]);
    const actual = makePng(20, 20, [255, 0, 0, 255]);
    await Bun.write(join(scratch, "main-window.png"), baseline);

    const result = await expectScreenshot(actual, "main-window", { baselineDir: scratch });

    expect(result.match).toBe(false);
    // totalPixels / diffPixels are derived from the *actual* dimensions
    // (size mismatch == "every actual pixel is incomparable").
    expect(result.totalPixels).toBe(400);
    expect(result.diffPixels).toBe(400);
    expect(result.actualPath).toBe(join(scratch, "main-window.actual.png"));
    expect(result.diffPath).toBeUndefined();
    expect(existsSync(join(scratch, "main-window.diff.png"))).toBe(false);
  });

  test("threshold=1.0 treats subtle RGB shift as match (diffPixels=0)", async () => {
    const baseline = makePng(2, 2, [200, 100, 100, 255]);
    const actual = makePngWith1pxDiff(2, 2, [200, 100, 100, 255], 0, 0, [210, 110, 110, 255]);
    await Bun.write(join(scratch, "x.png"), baseline);

    const lenient = await expectScreenshot(actual, "x", {
      baselineDir: scratch,
      threshold: 1.0,
    });
    expect(lenient.match).toBe(true);
    expect(lenient.diffPixels).toBe(0);
  });

  test("threshold=0.0 treats subtle RGB shift as mismatch", async () => {
    const baseline = makePng(2, 2, [200, 100, 100, 255]);
    const actual = makePngWith1pxDiff(2, 2, [200, 100, 100, 255], 0, 0, [210, 110, 110, 255]);
    await Bun.write(join(scratch, "y.png"), baseline);

    const strict = await expectScreenshot(actual, "y", {
      baselineDir: scratch,
      threshold: 0.0,
    });
    expect(strict.match).toBe(false);
    expect(strict.diffPixels).toBeGreaterThanOrEqual(1);
  });

  test("updateBaseline=true overwrites baseline with actual and returns match=true", async () => {
    const oldBaseline = makePng(10, 10, [255, 0, 0, 255]);
    const newActual = makePng(10, 10, [0, 0, 255, 255]);
    await Bun.write(join(scratch, "main-window.png"), oldBaseline);

    const result = await expectScreenshot(newActual, "main-window", {
      baselineDir: scratch,
      updateBaseline: true,
    });

    expect(result.match).toBe(true);
    expect(result.diffPixels).toBe(0);

    const written = await Bun.file(join(scratch, "main-window.png")).bytes();
    expect(written.byteLength).toBe(newActual.byteLength);
    expect(Buffer.from(written).equals(Buffer.from(newActual))).toBe(true);
  });

  test("throws VisualDiffError(baseline_missing) when baseline absent and failOnMissing=true", async () => {
    const actual = makePng(5, 5, [0, 255, 0, 255]);

    let thrown: unknown;
    try {
      await expectScreenshot(actual, "missing", {
        baselineDir: scratch,
        failOnMissing: true,
      });
    } catch (err) {
      thrown = err;
    }
    expect(thrown).toBeInstanceOf(VisualDiffError);
    expect((thrown as VisualDiffError).kind).toBe("baseline_missing");
    expect(existsSync(join(scratch, "missing.png"))).toBe(false);
  });

  test("auto-saves baseline on first run when failOnMissing is unset (= default)", async () => {
    const actual = makePng(5, 5, [0, 255, 0, 255]);

    const result = await expectScreenshot(actual, "first-run", { baselineDir: scratch });

    expect(result.match).toBe(true);
    expect(result.diffPixels).toBe(0);
    expect(result.totalPixels).toBe(25);
    expect(result.actualPath).toBeUndefined();
    expect(result.diffPath).toBeUndefined();
    expect(existsSync(join(scratch, "first-run.png"))).toBe(true);
  });

  test("auto-saved baseline content equals the actual bytes", async () => {
    const actual = makePng(8, 6, [10, 20, 30, 255]);

    await expectScreenshot(actual, "auto", { baselineDir: scratch });

    const written = await Bun.file(join(scratch, "auto.png")).bytes();
    expect(written.byteLength).toBe(actual.byteLength);
    expect(Buffer.from(written).equals(Buffer.from(actual))).toBe(true);
  });

  test("does not write baseline when failOnMissing=true and baseline absent", async () => {
    const actual = makePng(5, 5, [0, 255, 0, 255]);

    await expect(
      expectScreenshot(actual, "no-write", {
        baselineDir: scratch,
        failOnMissing: true,
      }),
    ).rejects.toBeInstanceOf(VisualDiffError);

    expect(existsSync(join(scratch, "no-write.png"))).toBe(false);
    expect(existsSync(join(scratch, "no-write.actual.png"))).toBe(false);
    expect(existsSync(join(scratch, "no-write.diff.png"))).toBe(false);
  });

  test("updateBaseline=true overrides failOnMissing=true (still writes)", async () => {
    const actual = makePng(5, 5, [0, 255, 0, 255]);

    const result = await expectScreenshot(actual, "override", {
      baselineDir: scratch,
      failOnMissing: true,
      updateBaseline: true,
    });

    expect(result.match).toBe(true);
    expect(result.diffPixels).toBe(0);
    expect(existsSync(join(scratch, "override.png"))).toBe(true);
  });

  test("creates baseline on first run when updateBaseline=true and baseline absent", async () => {
    const actual = makePng(5, 5, [0, 255, 0, 255]);

    const result = await expectScreenshot(actual, "first-run-update", {
      baselineDir: scratch,
      updateBaseline: true,
    });

    expect(result.match).toBe(true);
    expect(result.diffPixels).toBe(0);
    expect(existsSync(join(scratch, "first-run-update.png"))).toBe(true);
  });
});
