import "./_setup.ts";

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { PNG } from "pngjs";

import { E2EClient } from "../src/client.ts";
import { VisualDiffError } from "../src/errors.ts";

const HERE = fileURLToPath(import.meta.url);
const HERE_DIR = dirname(HERE);

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

// `screenshot()` を spy / stub するための薄いヘルパ。HTTP 経路は触らない
// (baseline 解決 + first-run 挙動の確認だけが目的)。
function makeStubbedClient(bytes: Uint8Array): E2EClient {
  const client = new E2EClient({ baseUrl: "http://invalid.local.invalid", token: "" });
  // biome-ignore lint/suspicious/noExplicitAny: tests stub a public method on the prototype intentionally
  (client as any).screenshot = async (): Promise<Uint8Array> => bytes;
  return client;
}

describe("E2EClient.expectScreenshot wrapper", () => {
  let scratch: string;

  beforeEach(() => {
    scratch = mkdtempSync(join(tmpdir(), "gtk4-e2e-vdiff-client-"));
  });

  afterEach(() => {
    rmSync(scratch, { recursive: true, force: true });
  });

  test("opts.baselineDir wins over default resolution (no prefix)", async () => {
    const png = makePng(4, 4, [10, 20, 30, 255]);
    const client = makeStubbedClient(png);

    const result = await client.expectScreenshot("button", {
      baselineDir: scratch,
      env: {},
    });

    expect(result.match).toBe(true);
    // opts.baselineDir 明示時は <basename>-<name> prefix を skip する
    // (rev2 で確定した「呼び出し側がファイル名まで制御している」前提)。
    expect(existsSync(join(scratch, "button.png"))).toBe(true);
    expect(existsSync(join(scratch, "clientExpectScreenshot.test.ts-button.png"))).toBe(false);
  });

  test("opts.testFile が env GTK4_E2E_BASELINE_DIR より優先される (案 X)", async () => {
    const png = makePng(4, 4, [0, 0, 0, 255]);
    const client = makeStubbedClient(png);
    const fakeTestFile = join(scratch, "synth", "fake.spec.ts");

    const result = await client.expectScreenshot("hero", {
      testFile: fakeTestFile,
      env: { GTK4_E2E_BASELINE_DIR: join(scratch, "env-baselines") },
    });

    expect(result.match).toBe(true);
    // dirname(testFile) + /__screenshots__/ + fake.spec.ts-hero.png
    expect(existsSync(join(scratch, "synth", "__screenshots__", "fake.spec.ts-hero.png"))).toBe(
      true,
    );
    // env 側には書かれていない (= opts.testFile が勝った証拠)
    expect(existsSync(join(scratch, "env-baselines", "fake.spec.ts-hero.png"))).toBe(false);
  });

  test("env GTK4_E2E_BASELINE_DIR wins over stack-derived default", async () => {
    const png = makePng(4, 4, [50, 50, 50, 255]);
    const client = makeStubbedClient(png);

    const result = await client.expectScreenshot("hero", {
      env: { GTK4_E2E_BASELINE_DIR: scratch },
    });

    expect(result.match).toBe(true);
    // stack-derived basename (clientExpectScreenshot.test.ts) は prefix に残る。
    expect(existsSync(join(scratch, "clientExpectScreenshot.test.ts-hero.png"))).toBe(true);
  });

  test("CI=true + baseline missing → throws VisualDiffError(baseline_missing)", async () => {
    const png = makePng(4, 4, [255, 0, 0, 255]);
    const client = makeStubbedClient(png);

    let thrown: unknown;
    try {
      await client.expectScreenshot("ci-missing", {
        baselineDir: scratch,
        env: { CI: "true" },
      });
    } catch (err) {
      thrown = err;
    }
    expect(thrown).toBeInstanceOf(VisualDiffError);
    expect((thrown as VisualDiffError).kind).toBe("baseline_missing");
    // opts.baselineDir 明示時は prefix が付かないため、<scratch>/<name>.png を確認。
    expect(existsSync(join(scratch, "ci-missing.png"))).toBe(false);
  });

  test("CI unset + baseline missing → auto-saves and returns match=true", async () => {
    const png = makePng(4, 4, [0, 255, 0, 255]);
    const client = makeStubbedClient(png);

    const result = await client.expectScreenshot("local-first-run", {
      baselineDir: scratch,
      env: {},
    });

    expect(result.match).toBe(true);
    expect(result.diffPixels).toBe(0);
    expect(existsSync(join(scratch, "local-first-run.png"))).toBe(true);
  });

  test("opts.failOnMissing=false overrides CI=true (escape hatch)", async () => {
    const png = makePng(4, 4, [0, 0, 255, 255]);
    const client = makeStubbedClient(png);

    const result = await client.expectScreenshot("escape", {
      baselineDir: scratch,
      env: { CI: "true" },
      failOnMissing: false,
    });

    expect(result.match).toBe(true);
    expect(existsSync(join(scratch, "escape.png"))).toBe(true);
  });

  test('CI other than "true" (e.g. "1") does not trigger fail mode', async () => {
    // Plan §5 / ADR-0003 §"Resolved Decisions": `process.env.CI === "true"` 文字列
    // 一致のみ。Travis 旧設定 `CI=1` 等は意図的に取りこぼし、運用側で `CI=true`
    // を export することで揃える。
    const png = makePng(4, 4, [128, 128, 128, 255]);
    const client = makeStubbedClient(png);

    const result = await client.expectScreenshot("ci-1", {
      baselineDir: scratch,
      env: { CI: "1" },
    });

    expect(result.match).toBe(true);
    expect(existsSync(join(scratch, "ci-1.png"))).toBe(true);
  });

  test("opts.baselineDir + opts.testFile both → baselineDir wins, no prefix (rev2)", async () => {
    // baselineDir を渡した時点で「呼び出し側がファイル名まで制御している」と
    // 解釈する規約 (rev2 fix)。testFile が同時に与えられても prefix は付かない。
    const png = makePng(4, 4, [33, 99, 33, 255]);
    const client = makeStubbedClient(png);
    const fakeTestFile = join(scratch, "fake.spec.ts");

    const result = await client.expectScreenshot("explicit", {
      baselineDir: scratch,
      testFile: fakeTestFile,
      env: {},
    });

    expect(result.match).toBe(true);
    expect(existsSync(join(scratch, "explicit.png"))).toBe(true);
    expect(existsSync(join(scratch, "fake.spec.ts-explicit.png"))).toBe(false);
  });

  test("default baselineDir resolves to <caller_dir>/__screenshots__", async () => {
    // env / opts.testFile / opts.baselineDir すべて未指定で stack-based 推定が
    // 効くこと、prefix に本テストファイル basename が入ることを確認。
    const png = makePng(4, 4, [200, 0, 200, 255]);
    const client = makeStubbedClient(png);

    const result = await client.expectScreenshot("default-resolve", { env: {} });

    const expectedDir = join(HERE_DIR, "__screenshots__");
    const expectedFile = join(expectedDir, "clientExpectScreenshot.test.ts-default-resolve.png");
    try {
      expect(result.match).toBe(true);
      expect(result.baselinePath).toBe(expectedFile);
      expect(existsSync(expectedFile)).toBe(true);
    } finally {
      // 副作用クリーンアップ: 本テストが書いた baseline を消す。残しておくと
      // git status が dirty になり他テストの先頭ケースを汚染する。
      rmSync(expectedDir, { recursive: true, force: true });
    }
  });
});
