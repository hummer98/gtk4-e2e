// Local registry discovery — mirrors `packages/server/src/registry.rs`.
//
// `runtimeDir` resolves to `${XDG_RUNTIME_DIR}/gtk4-e2e` when XDG points at an
// existing directory, otherwise falls back to `os.tmpdir() + "/gtk4-e2e"`. The
// env is injected (defaulting to `process.env`) so tests can exercise each
// branch without mutating shared process state.
//
// `listInstances` enumerates `instance-*.json` files, validates that the file
// name pid matches the JSON `pid` (cleans up corrupt or stale entries), and
// filters out pids that no longer exist. `discover` layers a camelCase filter
// API on top — translation to the registry's snake_case keys is centralised
// here so callers never see `app_name` etc.

import { statSync } from "node:fs";
import { readdir, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";

const REGISTRY_SUBDIR = "gtk4-e2e";

export interface InstanceFile {
  pid: number;
  port: number;
  app_name: string;
  app_version: string;
  started_at: string;
  token?: string;
}

export interface DiscoverFilter {
  port?: number;
  pid?: number;
  appName?: string;
}

type EnvLike = Record<string, string | undefined>;

interface ListOptions {
  dir?: string;
  env?: EnvLike;
}

export function runtimeDir(env: EnvLike = process.env): string {
  const xdg = env["XDG_RUNTIME_DIR"];
  if (xdg && xdg.length > 0 && isDirectorySync(xdg)) {
    return join(xdg, REGISTRY_SUBDIR);
  }
  return join(tmpdir(), REGISTRY_SUBDIR);
}

export async function listInstances(opts: ListOptions = {}): Promise<InstanceFile[]> {
  const dir = opts.dir ?? runtimeDir(opts.env);

  let entries: string[];
  try {
    entries = await readdir(dir);
  } catch (err) {
    if (isNodeError(err) && err.code === "ENOENT") return [];
    throw err;
  }

  const out: InstanceFile[] = [];
  for (const name of entries) {
    const fileNamePid = parseInstanceFileName(name);
    if (fileNamePid === null) continue;

    const path = join(dir, name);
    const parsed = await readInstanceFile(path);
    if (parsed === null) continue;
    if (parsed.pid !== fileNamePid) continue;
    if (!isPidAlive(parsed.pid)) continue;

    out.push(parsed);
  }
  return out;
}

export async function discover(
  filter: DiscoverFilter = {},
  opts: ListOptions = {},
): Promise<InstanceFile[]> {
  const all = await listInstances(opts);
  return all.filter((entry) => {
    if (filter.port !== undefined && entry.port !== filter.port) return false;
    if (filter.pid !== undefined && entry.pid !== filter.pid) return false;
    if (filter.appName !== undefined && entry.app_name !== filter.appName) return false;
    return true;
  });
}

function isDirectorySync(p: string): boolean {
  try {
    return statSync(p).isDirectory();
  } catch {
    return false;
  }
}

function parseInstanceFileName(name: string): number | null {
  const m = basename(name).match(/^instance-(\d+)\.json$/);
  if (!m) return null;
  const pid = Number.parseInt(m[1], 10);
  return Number.isFinite(pid) ? pid : null;
}

async function readInstanceFile(path: string): Promise<InstanceFile | null> {
  try {
    const raw = await readFile(path, "utf8");
    const parsed = JSON.parse(raw) as unknown;
    if (!isInstanceFile(parsed)) return null;
    return parsed;
  } catch {
    return null;
  }
}

function isInstanceFile(v: unknown): v is InstanceFile {
  if (typeof v !== "object" || v === null) return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.pid === "number" &&
    typeof o.port === "number" &&
    typeof o.app_name === "string" &&
    typeof o.app_version === "string" &&
    typeof o.started_at === "string" &&
    (o.token === undefined || typeof o.token === "string")
  );
}

function isPidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (err) {
    if (isNodeError(err) && err.code === "ESRCH") return false;
    return true;
  }
}

function isNodeError(err: unknown): err is NodeJS.ErrnoException {
  return err instanceof Error && typeof (err as NodeJS.ErrnoException).code === "string";
}
