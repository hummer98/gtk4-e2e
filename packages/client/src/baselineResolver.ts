// baseline directory / scenario basename を「明示 opts > env > stack > cwd」の
// 優先順位で解決する pure function 群 (Step 18 / T020-B)。
//
// 設計判断は plan §1.1 / §1.2 と ADR-0003 §"Resolved Decisions" を参照。
//
//   * env と callerStack を引数で受け取り、`process.env` を一切読まない。
//     wrapper エントリポイント (`E2EClient.expectScreenshot` in `client.ts`)
//     が `process.env` を 1 度だけ読み、ここに inject する。
//   * stack 解析失敗時は静かに `<cwd>/__screenshots__` へフォールバックする。
//     fail はしない (壊れた stack で本番 assert が落ちる方が筋悪)。

import { dirname, isAbsolute, resolve } from "node:path";

const BASELINE_SUBDIR = "__screenshots__";

export interface ResolveBaselineDirInput {
  /** Wrapper user が直接渡した `opts.baselineDir`. 最優先。 */
  optsBaselineDir?: string;
  /** Wrapper user が渡した絶対 path 形式の test ファイル. opts > env > stack の中位。 */
  optsTestFile?: string;
  /** wrapper が `process.env` から inject した env subset. */
  env?: { GTK4_E2E_BASELINE_DIR?: string };
  /** `new Error().stack` (wrapper 内で取って渡す)。stack-based caller 推定の入力。 */
  callerStack?: string;
  /** stack 解析時に skip する自身の絶対パス (resolver / wrapper / SDK 内部 file)。 */
  skipFiles?: string[];
  /** 相対 path 解決と最終フォールバックの基点。default `process.cwd()`. */
  cwd?: string;
}

export interface ResolveScenarioBasenameInput {
  optsTestFile?: string;
  callerStack?: string;
  skipFiles?: string[];
}

export function resolveBaselineDir(input: ResolveBaselineDirInput): string {
  const cwd = input.cwd ?? process.cwd();

  if (input.optsBaselineDir !== undefined) {
    return absolutise(input.optsBaselineDir, cwd);
  }

  if (input.optsTestFile !== undefined) {
    return resolve(dirname(absolutise(input.optsTestFile, cwd)), BASELINE_SUBDIR);
  }

  const envDir = input.env?.GTK4_E2E_BASELINE_DIR;
  if (envDir !== undefined && envDir !== "") {
    return absolutise(envDir, cwd);
  }

  const callerFile = parseCallerFile(input.callerStack, input.skipFiles);
  if (callerFile !== null) {
    return resolve(dirname(callerFile), BASELINE_SUBDIR);
  }

  return resolve(cwd, BASELINE_SUBDIR);
}

export function resolveScenarioBasename(input: ResolveScenarioBasenameInput): string | null {
  if (input.optsTestFile !== undefined) {
    return basenameOf(input.optsTestFile);
  }
  const callerFile = parseCallerFile(input.callerStack, input.skipFiles);
  if (callerFile !== null) {
    return basenameOf(callerFile);
  }
  return null;
}

function absolutise(p: string, cwd: string): string {
  return isAbsolute(p) ? p : resolve(cwd, p);
}

function basenameOf(p: string): string {
  // node:path.basename を使うと OS によって挙動が変わる懸念があるので、
  // 受け取った path を `/` 区切りで自前 split する (POSIX のみ対応)。
  const idx = p.lastIndexOf("/");
  return idx >= 0 ? p.slice(idx + 1) : p;
}

// V8 互換 stack の各 frame から「最初の非 skip 絶対 path」を返す。
// Bun は `at <fn> (/abs/path:L:C)` または `at /abs/path:L:C` の 2 形式を出す。
const FRAME_RE =
  /\((?:file:\/\/)?(\/[^\s)]+):(\d+):(\d+)\)|at (?:file:\/\/)?(\/[^\s)]+):(\d+):(\d+)/;

function parseCallerFile(
  stack: string | undefined,
  skipFiles: string[] | undefined,
): string | null {
  if (stack === undefined) return null;
  const skip = new Set(skipFiles ?? []);
  for (const line of stack.split("\n")) {
    const m = FRAME_RE.exec(line);
    if (!m) continue;
    const file = m[1] ?? m[4];
    if (file === undefined) continue;
    if (skip.has(file)) continue;
    return file;
  }
  return null;
}
