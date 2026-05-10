import "./_setup.ts";

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { discover, type InstanceFile, listInstances, runtimeDir } from "../src/discover.ts";

// Pid that is extremely unlikely to exist as a live process. `process.kill`
// with sig 0 returns ESRCH for unused pids on macOS / Linux.
const DEAD_PID = 0x7fffffff;

function makeInstance(overrides: Partial<InstanceFile> & { pid: number }): InstanceFile {
  return {
    pid: overrides.pid,
    port: overrides.port ?? 19042,
    app_name: overrides.app_name ?? "gtk4-e2e-app",
    app_version: overrides.app_version ?? "0.1.0",
    started_at: overrides.started_at ?? "2026-05-10T00:00:00Z",
    ...(overrides.token !== undefined ? { token: overrides.token } : {}),
  };
}

function writeInstanceFile(dir: string, file: InstanceFile, fileName?: string): string {
  const path = join(dir, fileName ?? `instance-${file.pid}.json`);
  writeFileSync(path, JSON.stringify(file));
  return path;
}

describe("runtimeDir", () => {
  let scratch: string;

  beforeEach(() => {
    scratch = mkdtempSync(join(tmpdir(), "gtk4-e2e-discover-"));
  });

  afterEach(() => {
    rmSync(scratch, { recursive: true, force: true });
  });

  test("uses XDG_RUNTIME_DIR when it exists as a directory", () => {
    const dir = runtimeDir({ XDG_RUNTIME_DIR: scratch });
    expect(dir).toBe(join(scratch, "gtk4-e2e"));
  });

  test("falls back to tmpdir when XDG_RUNTIME_DIR is unset", () => {
    const dir = runtimeDir({});
    expect(dir).toBe(join(tmpdir(), "gtk4-e2e"));
  });

  test("falls back to tmpdir when XDG_RUNTIME_DIR points at a file", () => {
    const filePath = join(scratch, "not-a-dir");
    writeFileSync(filePath, "");
    const dir = runtimeDir({ XDG_RUNTIME_DIR: filePath });
    expect(dir).toBe(join(tmpdir(), "gtk4-e2e"));
  });

  test("falls back to tmpdir when XDG_RUNTIME_DIR is empty string", () => {
    const dir = runtimeDir({ XDG_RUNTIME_DIR: "" });
    expect(dir).toBe(join(tmpdir(), "gtk4-e2e"));
  });
});

describe("listInstances / discover", () => {
  let registryDir: string;

  beforeEach(() => {
    const root = mkdtempSync(join(tmpdir(), "gtk4-e2e-registry-"));
    registryDir = join(root, "gtk4-e2e");
    mkdirSync(registryDir, { recursive: true });
  });

  afterEach(() => {
    rmSync(registryDir, { recursive: true, force: true });
  });

  test("returns every instance whose pid is alive (smoke)", async () => {
    // pid 1 (init / launchd) is always alive on POSIX systems and gives us a
    // second canonically-distinct file name without having to spawn anything.
    const a = makeInstance({ pid: process.pid, port: 19001 });
    const b = makeInstance({
      pid: 1,
      port: 19002,
      app_name: "other-app",
      started_at: "2026-05-10T00:00:01Z",
    });
    // Names that don't match JSON pid get skipped by the integrity rule.
    writeInstanceFile(registryDir, a, "instance-aaa.json");
    writeInstanceFile(registryDir, a);
    writeInstanceFile(registryDir, b);

    const all = await listInstances({ dir: registryDir });
    expect(all).toHaveLength(2);
    expect(all.map((i) => i.port).sort()).toEqual([19001, 19002]);
  });

  test("filters by port", async () => {
    writeInstanceFile(registryDir, makeInstance({ pid: process.pid, port: 19010 }));
    const second = makeInstance({ pid: process.pid + 0, port: 19011 });
    // distinct file name to avoid clobbering the first
    writeInstanceFile(registryDir, second, `instance-${second.pid}-b.json`);

    // file name pid mismatch — that one is filtered out by the integrity rule.
    const matches = await discover({ port: 19010 }, { dir: registryDir });
    expect(matches).toHaveLength(1);
    expect(matches[0].port).toBe(19010);
  });

  test("filters by appName (camelCase API → snake_case JSON)", async () => {
    const alpha = makeInstance({ pid: process.pid, port: 19020, app_name: "alpha" });
    writeInstanceFile(registryDir, alpha);

    const matches = await discover({ appName: "alpha" }, { dir: registryDir });
    expect(matches).toHaveLength(1);
    expect(matches[0].app_name).toBe("alpha");

    const empty = await discover({ appName: "beta" }, { dir: registryDir });
    expect(empty).toHaveLength(0);
  });

  test("filters by pid", async () => {
    writeInstanceFile(registryDir, makeInstance({ pid: process.pid, port: 19030 }));

    const matches = await discover({ pid: process.pid }, { dir: registryDir });
    expect(matches).toHaveLength(1);

    const none = await discover({ pid: process.pid + 1 }, { dir: registryDir });
    expect(none).toHaveLength(0);
  });

  test("excludes instances whose pid is dead", async () => {
    writeInstanceFile(registryDir, makeInstance({ pid: process.pid, port: 19040 }));
    writeInstanceFile(registryDir, makeInstance({ pid: DEAD_PID, port: 19041 }));

    const all = await listInstances({ dir: registryDir });
    expect(all).toHaveLength(1);
    expect(all[0].pid).toBe(process.pid);
  });

  test("excludes instances where filename pid does not match JSON pid", async () => {
    const skewed = makeInstance({ pid: process.pid, port: 19050 });
    // intentionally write under a wrong file name — file name says 999 but JSON says process.pid
    writeFileSync(join(registryDir, "instance-999.json"), JSON.stringify(skewed));

    // also put a clean one to ensure listing still works
    writeInstanceFile(registryDir, makeInstance({ pid: process.pid, port: 19051 }));

    const all = await listInstances({ dir: registryDir });
    expect(all).toHaveLength(1);
    expect(all[0].port).toBe(19051);
  });

  test("ignores files that are not instance-*.json", async () => {
    writeFileSync(join(registryDir, "README.md"), "# noise");
    writeFileSync(join(registryDir, "instance-abc.json"), "{}");
    writeFileSync(join(registryDir, "garbage.json"), "not json at all");
    writeInstanceFile(registryDir, makeInstance({ pid: process.pid, port: 19060 }));

    const all = await listInstances({ dir: registryDir });
    expect(all).toHaveLength(1);
    expect(all[0].port).toBe(19060);
  });

  test("returns [] when registry directory is missing", async () => {
    const all = await listInstances({ dir: join(registryDir, "does-not-exist") });
    expect(all).toEqual([]);
  });

  test("uses env-injected runtimeDir when no dir override is provided", async () => {
    const xdgRoot = registryDir.replace(/\/gtk4-e2e$/, "");
    writeInstanceFile(registryDir, makeInstance({ pid: process.pid, port: 19070 }));

    const all = await listInstances({ env: { XDG_RUNTIME_DIR: xdgRoot } });
    expect(all).toHaveLength(1);
    expect(all[0].port).toBe(19070);
  });
});
