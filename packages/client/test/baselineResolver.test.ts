import "./_setup.ts";

import { describe, expect, test } from "bun:test";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { resolveBaselineDir, resolveScenarioBasename } from "../src/baselineResolver.ts";

const HERE = fileURLToPath(import.meta.url);
const HERE_DIR = dirname(HERE);

describe("resolveBaselineDir", () => {
  test("opts.baselineDir wins over opts.testFile, env, and stack", () => {
    const result = resolveBaselineDir({
      optsBaselineDir: "/abs/baselines",
      optsTestFile: "/abs/tests/foo.spec.ts",
      env: { GTK4_E2E_BASELINE_DIR: "/abs/env-base" },
      callerStack: "Error\n    at fn (/abs/caller/bar.spec.ts:1:1)",
      cwd: "/fake/cwd",
    });
    expect(result).toBe("/abs/baselines");
  });

  test("opts.testFile (= absolute) wins over env and stack — yields <dirname(testFile)>/__screenshots__", () => {
    const result = resolveBaselineDir({
      optsTestFile: "/abs/tests/foo.spec.ts",
      env: { GTK4_E2E_BASELINE_DIR: "/abs/env-base" },
      callerStack: "Error\n    at fn (/abs/caller/bar.spec.ts:1:1)",
      cwd: "/fake/cwd",
    });
    expect(result).toBe("/abs/tests/__screenshots__");
  });

  test("env GTK4_E2E_BASELINE_DIR wins over stack", () => {
    const result = resolveBaselineDir({
      env: { GTK4_E2E_BASELINE_DIR: "/abs/env-base" },
      callerStack: "Error\n    at fn (/abs/caller/bar.spec.ts:1:1)",
      cwd: "/fake/cwd",
    });
    expect(result).toBe("/abs/env-base");
  });

  test("env (relative) is resolved against cwd", () => {
    const result = resolveBaselineDir({
      env: { GTK4_E2E_BASELINE_DIR: "rel/env-base" },
      cwd: "/fake/cwd",
    });
    expect(result).toBe("/fake/cwd/rel/env-base");
  });

  test("stack frame yields <dirname(callerFile)>/__screenshots__ when no opts/env given", () => {
    const result = resolveBaselineDir({
      callerStack: "Error\n    at fn (/abs/caller/bar.spec.ts:1:1)",
      cwd: "/fake/cwd",
    });
    expect(result).toBe("/abs/caller/__screenshots__");
  });

  test("stack parse failure falls back to <cwd>/__screenshots__", () => {
    const result = resolveBaselineDir({
      callerStack: "Error\n    at <anonymous>",
      cwd: "/fake/cwd",
    });
    expect(result).toBe("/fake/cwd/__screenshots__");
  });

  test("skipFiles filter removes resolver/wrapper frames before picking caller", () => {
    const stack = [
      "Error",
      "    at resolveBaselineDir (/abs/src/baselineResolver.ts:10:5)",
      "    at E2EClient.expectScreenshot (/abs/src/client.ts:20:5)",
      "    at scenario (/abs/caller/bar.spec.ts:1:1)",
    ].join("\n");
    const result = resolveBaselineDir({
      callerStack: stack,
      skipFiles: ["/abs/src/baselineResolver.ts", "/abs/src/client.ts"],
      cwd: "/fake/cwd",
    });
    expect(result).toBe("/abs/caller/__screenshots__");
  });

  test("relative opts.baselineDir is resolved against cwd", () => {
    const result = resolveBaselineDir({
      optsBaselineDir: "rel/baselines",
      cwd: "/fake/cwd",
    });
    expect(result).toBe("/fake/cwd/rel/baselines");
  });

  test("env arg is honoured even when process.env (real) lacks the variable (smoke)", () => {
    // resolver must rely on the injected `env` param exclusively. We cannot
    // assume anything about the live `process.env`, so this smoke just
    // demonstrates that an injected `{}` env triggers the stack/cwd fallback
    // even if the developer happens to have GTK4_E2E_BASELINE_DIR exported.
    const result = resolveBaselineDir({
      env: {},
      cwd: "/fake/cwd",
    });
    expect(result).toBe("/fake/cwd/__screenshots__");
  });
});

describe("resolveScenarioBasename", () => {
  test("returns testFile basename when explicit", () => {
    const result = resolveScenarioBasename({
      optsTestFile: "/abs/tests/foo.spec.ts",
    });
    expect(result).toBe("foo.spec.ts");
  });

  test("returns parsed stack basename when stack-derived", () => {
    const result = resolveScenarioBasename({
      callerStack: "Error\n    at fn (/abs/caller/bar.spec.ts:1:1)",
    });
    expect(result).toBe("bar.spec.ts");
  });

  test("returns null when both are absent", () => {
    const result = resolveScenarioBasename({});
    expect(result).toBe(null);
  });

  test("skipFiles filter removes resolver/wrapper frames before picking caller", () => {
    const stack = [
      "Error",
      "    at resolveBaselineDir (/abs/src/baselineResolver.ts:10:5)",
      "    at E2EClient.expectScreenshot (/abs/src/client.ts:20:5)",
      "    at scenario (/abs/caller/bar.spec.ts:1:1)",
    ].join("\n");
    const result = resolveScenarioBasename({
      callerStack: stack,
      skipFiles: ["/abs/src/baselineResolver.ts", "/abs/src/client.ts"],
    });
    expect(result).toBe("bar.spec.ts");
  });
});

// Bun の version up で stack format が変わった時、本番ロジックの破綻を smoke test で検知する。
describe("stack parsing (smoke against actual Bun runtime)", () => {
  test("`new Error().stack` から本テストファイルの絶対パスが拾えること", () => {
    const stack = new Error().stack;
    expect(stack).toBeDefined();
    if (stack === undefined) return;

    expect(stack).toContain(HERE);

    const dir = resolveBaselineDir({ callerStack: stack, cwd: "/never-used" });
    expect(dir).toBe(join(HERE_DIR, "__screenshots__"));

    const basename = resolveScenarioBasename({ callerStack: stack });
    expect(basename).toBe("baselineResolver.test.ts");
  });
});
