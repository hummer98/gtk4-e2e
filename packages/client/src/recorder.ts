// Local screen recorder built on top of `ffmpeg`. The ffmpeg process is
// spawned via `Bun.spawn` (or a DI seam in tests), and a single PID file at
// `${registryDir}/recorder.json` tracks lifecycle so `record start/stop/status`
// from a different CLI invocation can observe each other.
//
// Plan §3.1 / §4.1 / §4.6: PID file, no IPC, X11-only MVP, ffmpeg start
// fails fast when display kind is unknown / Wayland / ffmpeg missing.

import { existsSync, readFileSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { runtimeDir } from "./discover.ts";
import { RecorderError } from "./errors.ts";

type EnvLike = Record<string, string | undefined>;

export type DisplayKind = "x11" | "wayland" | "unknown";

export interface RecorderOptions {
  output: string;
  display?: string;
  fps?: number;
  size?: { w: number; h: number };
  ffmpegBin?: string;
  registryDir?: string;
  env?: EnvLike;
  spawn?: (cmd: string[]) => Bun.Subprocess;
  verbose?: boolean;
}

export interface RecorderStatus {
  running: boolean;
  output: string | null;
  pid: number | null;
  startedAt: string | null;
  elapsedMs: number | null;
}

interface PidFile {
  pid: number;
  output: string;
  started_at: string;
  display: string | null;
}

const PID_FILE_NAME = "recorder.json";

export function detectDisplayKind(env: EnvLike): DisplayKind {
  const wayland = env.WAYLAND_DISPLAY;
  if (typeof wayland === "string" && wayland.length > 0) return "wayland";
  const display = env.DISPLAY;
  if (typeof display === "string" && display.length > 0) return "x11";
  return "unknown";
}

function defaultDisplay(env: EnvLike): string {
  const d = env.DISPLAY;
  if (typeof d === "string" && d.length > 0) return d;
  return ":0";
}

export function buildFfmpegArgs(opts: {
  output: string;
  display?: string;
  fps?: number;
  size?: { w: number; h: number };
  env?: EnvLike;
}): string[] {
  const env = opts.env ?? {};
  const display = opts.display ?? defaultDisplay(env);
  const args = [
    "-y",
    "-f",
    "x11grab",
    "-framerate",
    String(opts.fps ?? 30),
  ];
  if (opts.size) {
    args.push("-video_size", `${opts.size.w}x${opts.size.h}`);
  }
  args.push(
    "-i",
    display,
    "-c:v",
    "libx264",
    "-pix_fmt",
    "yuv420p",
    "-preset",
    "veryfast",
    opts.output,
  );
  return args;
}

function pidFilePath(registryDir: string): string {
  return join(registryDir, PID_FILE_NAME);
}

function isPidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === "ESRCH") return false;
    return true;
  }
}

function readPidFile(path: string): PidFile | null {
  try {
    const raw = readFileSync(path, "utf8");
    const parsed = JSON.parse(raw) as unknown;
    if (typeof parsed !== "object" || parsed === null) return null;
    const o = parsed as Record<string, unknown>;
    if (
      typeof o.pid !== "number" ||
      typeof o.output !== "string" ||
      typeof o.started_at !== "string"
    )
      return null;
    return {
      pid: o.pid,
      output: o.output,
      started_at: o.started_at,
      display: typeof o.display === "string" ? o.display : null,
    };
  } catch {
    return null;
  }
}

function atomicWritePidFile(path: string, data: PidFile): void {
  const tmp = `${path}.tmp.${process.pid}`;
  writeFileSync(tmp, JSON.stringify(data));
  renameSync(tmp, path);
}

function removePidFile(path: string): void {
  try {
    unlinkSync(path);
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== "ENOENT") throw err;
  }
}

async function sleepMs(ms: number): Promise<void> {
  await new Promise<void>((r) => setTimeout(r, ms));
}

export class Recorder {
  private readonly opts: RecorderOptions;
  private readonly env: EnvLike;
  private readonly registryDir: string;
  private proc: Bun.Subprocess | null = null;

  constructor(opts: RecorderOptions) {
    this.opts = opts;
    this.env = opts.env ?? (process.env as EnvLike);
    this.registryDir = opts.registryDir ?? runtimeDir(this.env);
  }

  async start(): Promise<void> {
    const kind = detectDisplayKind(this.env);
    if (kind === "wayland") {
      throw new RecorderError(
        "wayland capture is not supported in MVP; export DISPLAY for X11 (or unset WAYLAND_DISPLAY)",
        { kind: "not_implemented_wayland" },
      );
    }
    if (kind === "unknown") {
      throw new RecorderError(
        "no DISPLAY/WAYLAND_DISPLAY in env; X11 capture requires a running X server",
        { kind: "no_display" },
      );
    }

    await mkdir(this.registryDir, { recursive: true });
    const pidPath = pidFilePath(this.registryDir);
    const existing = readPidFile(pidPath);
    if (existing !== null) {
      if (isPidAlive(existing.pid)) {
        throw new RecorderError(
          `recorder already running (pid=${existing.pid}, output=${existing.output})`,
          { kind: "already_running" },
        );
      }
      // Stale: clean up.
      removePidFile(pidPath);
    }

    const ffmpegBin = this.opts.ffmpegBin ?? "ffmpeg";
    // Skip the `which` precheck when the caller injected a fake spawn. The
    // CLI / SDK path uses Bun.which to fail fast with a clearer message than
    // a raw ENOENT from spawn.
    if (this.opts.spawn === undefined) {
      const resolved = Bun.which(ffmpegBin);
      if (resolved === null) {
        throw new RecorderError(
          `ffmpeg binary not found in PATH (looked for "${ffmpegBin}"); install via apt/brew/dnf or pass ffmpegBin`,
          { kind: "ffmpeg_not_found" },
        );
      }
    }

    const args = buildFfmpegArgs({
      output: this.opts.output,
      display: this.opts.display,
      fps: this.opts.fps,
      size: this.opts.size,
      env: this.env,
    });
    const cmd = [ffmpegBin, ...args];

    let proc: Bun.Subprocess;
    try {
      if (this.opts.spawn) {
        proc = this.opts.spawn(cmd);
      } else {
        proc = Bun.spawn(cmd, {
          stdout: "ignore",
          stderr: this.opts.verbose ? "inherit" : "ignore",
        });
      }
    } catch (err) {
      throw new RecorderError(`failed to spawn ffmpeg: ${String(err)}`, {
        cause: err,
        kind: "spawn_failed",
      });
    }

    this.proc = proc;
    const data: PidFile = {
      pid: proc.pid,
      output: this.opts.output,
      started_at: new Date().toISOString(),
      display: this.opts.display ?? this.env.DISPLAY ?? null,
    };
    atomicWritePidFile(pidPath, data);
  }

  async stop(): Promise<void> {
    const pidPath = pidFilePath(this.registryDir);
    const existing = readPidFile(pidPath);
    if (existing === null) {
      throw new RecorderError("no recorder is running (no recorder.json found)", {
        kind: "not_running",
      });
    }

    // Prefer the in-process Subprocess handle when we own it (fewer races,
    // and tests using a fake handle don't need a real PID). Otherwise fall
    // back to signaling the PID directly.
    const proc = this.proc;
    try {
      if (proc !== null && proc.pid === existing.pid) {
        proc.kill("SIGTERM");
      } else {
        process.kill(existing.pid, "SIGTERM");
      }
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code !== "ESRCH") {
        // Non-ESRCH (e.g. EPERM) is unexpected; surface it.
        removePidFile(pidPath);
        this.proc = null;
        throw new RecorderError(`failed to send SIGTERM: ${String(err)}`, {
          cause: err,
          kind: "signal_failed",
        });
      }
    }

    // Poll up to 3s for graceful exit, then SIGKILL.
    const deadline = Date.now() + 3000;
    while (Date.now() < deadline) {
      if (!isPidAlive(existing.pid)) break;
      await sleepMs(50);
    }
    if (isPidAlive(existing.pid)) {
      try {
        if (proc !== null && proc.pid === existing.pid) {
          proc.kill("SIGKILL");
        } else {
          process.kill(existing.pid, "SIGKILL");
        }
      } catch {
        // Best effort.
      }
    }

    removePidFile(pidPath);
    this.proc = null;
  }

  status(): RecorderStatus {
    const pidPath = pidFilePath(this.registryDir);
    if (!existsSync(pidPath)) {
      return {
        running: false,
        output: null,
        pid: null,
        startedAt: null,
        elapsedMs: null,
      };
    }
    const parsed = readPidFile(pidPath);
    if (parsed === null) {
      return {
        running: false,
        output: null,
        pid: null,
        startedAt: null,
        elapsedMs: null,
      };
    }
    const alive = isPidAlive(parsed.pid);
    if (!alive) {
      return {
        running: false,
        output: parsed.output,
        pid: parsed.pid,
        startedAt: parsed.started_at,
        elapsedMs: null,
      };
    }
    const startedMs = Date.parse(parsed.started_at);
    const elapsedMs = Number.isFinite(startedMs)
      ? Math.max(0, Date.now() - startedMs)
      : null;
    return {
      running: true,
      output: parsed.output,
      pid: parsed.pid,
      startedAt: parsed.started_at,
      elapsedMs,
    };
  }
}
