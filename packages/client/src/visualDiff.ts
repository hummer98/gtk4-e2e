// Visual regression diff for the SDK (Step 17 / T020-A, Step 18 / T020-B).
//
// Pure-function `expectScreenshot(actual, name, opts?)` compares PNG bytes
// against `<baselineDir>/<name>.png` using pixelmatch + pngjs. The
// `E2EClient.expectScreenshot()` wrapper in `client.ts` is the convenience
// entry point that captures via `screenshot()` and resolves baseline path
// conventions before delegating here.
//
// Design notes (plan §1, ADR-0003 §"Resolved Decisions"):
//   * Q1 size mismatch → `match: false`, `diffPixels = totalPixels` (actual
//     dims). No diff PNG is written; `actualPath` is set so callers can
//     inspect.
//   * Q4 mismatch-only file output: `<name>.actual.png` and `<name>.diff.png`
//     land in the same `baselineDir`; matched runs leave the directory
//     untouched.
//   * T020-B baseline-missing branching:
//       - `updateBaseline: true`            → write baseline, match=true.
//       - `failOnMissing: true` && !exists → throw `VisualDiffError("baseline_missing")`.
//       - !exists (default)                → auto-save baseline, match=true.
//     CI-vs-local detection lives in the wrapper (`client.ts`); the pure
//     function only honours the booleans it receives.
//
// path traversal (`name = "../foo"`): unsanitized in this MVP scope. The
// decision is deferred to a follow-up task that will pick between rejection,
// slugify, or `/` namespace support.

import { mkdir } from "node:fs/promises";
import { isAbsolute, join, resolve } from "node:path";

import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

import { VisualDiffError } from "./errors.ts";

export interface ExpectScreenshotOptions {
  /** Per-pixel YIQ threshold (0-1). Lower = stricter. Default 0.1 (pixelmatch native). */
  threshold?: number;

  /** Treat anti-aliased pixels as differences. Default false (= AA detected and ignored). */
  includeAA?: boolean;

  /**
   * If true, overwrite (or create) the baseline with the current actual bytes
   * and return `match: true`. Takes priority over `failOnMissing`. Default false.
   */
  updateBaseline?: boolean;

  /**
   * If true and the baseline does not exist, throw `VisualDiffError("baseline_missing")`
   * instead of auto-saving. The wrapper sets this from `process.env.CI === "true"`
   * unless caller overrides. Ignored when `updateBaseline: true`. Default false.
   */
  failOnMissing?: boolean;

  /**
   * Directory containing baseline PNGs. Resolved against `process.cwd()` if
   * relative. Default `"__screenshots__"`.
   */
  baselineDir?: string;
}

export interface VisualDiffResult {
  /** True iff `diffPixels === 0` after pixelmatch (or after updateBaseline / auto-save). */
  match: boolean;
  /** Number of pixels different per pixelmatch. For size mismatch, equals totalPixels. */
  diffPixels: number;
  /** width * height of the actual image. (Always actual-derived, never baseline.) */
  totalPixels: number;
  /** Absolute path to the baseline PNG (existing or newly written). */
  baselinePath: string;
  /** Absolute path to the actual PNG. Set only on mismatch. */
  actualPath?: string;
  /** Absolute path to the diff PNG. Set only on mismatch when sizes match. */
  diffPath?: string;
}

const DEFAULT_BASELINE_DIR = "__screenshots__";
const DEFAULT_THRESHOLD = 0.1;

/**
 * Compare `actual` (PNG bytes) against the baseline at `<baselineDir>/<name>.png`.
 * Throws `VisualDiffError` when the baseline is missing and `failOnMissing: true`,
 * or when PNG decoding fails. Default behaviour on missing baseline is to
 * auto-save the actual bytes and report `match: true` (Playwright-style first-run).
 */
export async function expectScreenshot(
  actual: Uint8Array,
  name: string,
  opts: ExpectScreenshotOptions = {},
): Promise<VisualDiffResult> {
  const baselineDir = opts.baselineDir ?? DEFAULT_BASELINE_DIR;
  const absDir = isAbsolute(baselineDir) ? baselineDir : resolve(process.cwd(), baselineDir);
  const baselinePath = join(absDir, `${name}.png`);
  const actualPath = join(absDir, `${name}.actual.png`);
  const diffPath = join(absDir, `${name}.diff.png`);

  if (opts.updateBaseline === true) {
    return writeBaseline(absDir, baselinePath, actual);
  }

  const baselineExists = await Bun.file(baselinePath).exists();

  if (!baselineExists) {
    if (opts.failOnMissing === true) {
      throw new VisualDiffError(
        `baseline PNG not found at ${baselinePath} — run with { updateBaseline: true } to create it`,
        "baseline_missing",
      );
    }
    return writeBaseline(absDir, baselinePath, actual);
  }

  const baselineBytes = await Bun.file(baselinePath).bytes();
  const baselinePng = decodePng(baselineBytes);
  const actualPng = decodePng(actual);
  const totalPixels = actualPng.width * actualPng.height;

  // Size mismatch: return match=false without invoking pixelmatch (plan Q1).
  if (baselinePng.width !== actualPng.width || baselinePng.height !== actualPng.height) {
    await mkdir(absDir, { recursive: true });
    await Bun.write(actualPath, actual);
    return {
      match: false,
      diffPixels: totalPixels,
      totalPixels,
      baselinePath,
      actualPath,
    };
  }

  const diff = new PNG({ width: actualPng.width, height: actualPng.height });
  const diffPixels = pixelmatch(
    baselinePng.data,
    actualPng.data,
    diff.data,
    actualPng.width,
    actualPng.height,
    {
      threshold: opts.threshold ?? DEFAULT_THRESHOLD,
      includeAA: opts.includeAA ?? false,
    },
  );

  if (diffPixels === 0) {
    return {
      match: true,
      diffPixels: 0,
      totalPixels,
      baselinePath,
    };
  }

  await mkdir(absDir, { recursive: true });
  await Bun.write(actualPath, actual);
  await Bun.write(diffPath, new Uint8Array(PNG.sync.write(diff)));

  return {
    match: false,
    diffPixels,
    totalPixels,
    baselinePath,
    actualPath,
    diffPath,
  };
}

interface DecodedPng {
  width: number;
  height: number;
  data: Buffer;
}

// auto-save (baseline 不在 + !failOnMissing) と explicit `updateBaseline: true`
// の双方で使う共通の baseline 書き込み経路。decodePng で width/height を確定し、
// match=true / diffPixels=0 を返す。
async function writeBaseline(
  absDir: string,
  baselinePath: string,
  actual: Uint8Array,
): Promise<VisualDiffResult> {
  await mkdir(absDir, { recursive: true });
  await Bun.write(baselinePath, actual);
  const actualPng = decodePng(actual);
  return {
    match: true,
    diffPixels: 0,
    totalPixels: actualPng.width * actualPng.height,
    baselinePath,
  };
}

function decodePng(bytes: Uint8Array): DecodedPng {
  try {
    const png = PNG.sync.read(Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength));
    return { width: png.width, height: png.height, data: png.data };
  } catch (err) {
    throw new VisualDiffError("failed to decode PNG", "decode_failed", { cause: err });
  }
}
