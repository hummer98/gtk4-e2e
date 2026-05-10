import "./_setup.ts";

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { E2EError, RecorderError } from "../src/errors.ts";
import { buildFfmpegArgs, detectDisplayKind, Recorder } from "../src/recorder.ts";

describe("buildFfmpegArgs", () => {
  test("X11 default args (display from env)", () => {
    const args = buildFfmpegArgs({
      output: "/tmp/run.mp4",
      env: { DISPLAY: ":0" },
    });
    expect(args).toEqual([
      "-y",
      "-f",
      "x11grab",
      "-framerate",
      "30",
      "-i",
      ":0",
      "-c:v",
      "libx264",
      "-pix_fmt",
      "yuv420p",
      "-preset",
      "veryfast",
      "/tmp/run.mp4",
    ]);
  });

  test("explicit display overrides env", () => {
    const args = buildFfmpegArgs({
      output: "/tmp/x.mp4",
      display: ":1.0",
      env: { DISPLAY: ":0" },
    });
    expect(args).toContain(":1.0");
    expect(args).not.toContain(":0");
  });

  test("custom fps", () => {
    const args = buildFfmpegArgs({
      output: "/tmp/x.mp4",
      fps: 15,
      env: { DISPLAY: ":0" },
    });
    expect(args).toContain("15");
    const i = args.indexOf("-framerate");
    expect(args[i + 1]).toBe("15");
  });

  test("size adds -video_size flag", () => {
    const args = buildFfmpegArgs({
      output: "/tmp/x.mp4",
      size: { w: 1280, h: 720 },
      env: { DISPLAY: ":0" },
    });
    const i = args.indexOf("-video_size");
    expect(i).toBeGreaterThan(-1);
    expect(args[i + 1]).toBe("1280x720");
  });

  test("output is the last arg", () => {
    const args = buildFfmpegArgs({
      output: "/tmp/run.mp4",
      env: { DISPLAY: ":0" },
    });
    expect(args.at(-1)).toBe("/tmp/run.mp4");
  });
});

describe("detectDisplayKind", () => {
  test("WAYLAND_DISPLAY → wayland", () => {
    expect(detectDisplayKind({ WAYLAND_DISPLAY: "wayland-0" })).toBe("wayland");
  });

  test("DISPLAY only → x11", () => {
    expect(detectDisplayKind({ DISPLAY: ":0" })).toBe("x11");
  });

  test("WAYLAND wins over DISPLAY (Xwayland scenario)", () => {
    expect(detectDisplayKind({ WAYLAND_DISPLAY: "wayland-0", DISPLAY: ":0" })).toBe("wayland");
  });

  test("neither → unknown", () => {
    expect(detectDisplayKind({})).toBe("unknown");
  });

  test("empty string treated as absent", () => {
    expect(detectDisplayKind({ DISPLAY: "", WAYLAND_DISPLAY: "" })).toBe("unknown");
  });
});

describe("RecorderError", () => {
  test("instanceof E2EError", () => {
    const e = new RecorderError("boom");
    expect(e).toBeInstanceOf(E2EError);
    expect(e).toBeInstanceOf(RecorderError);
    expect(e.name).toBe("RecorderError");
    expect(e.message).toBe("boom");
  });

  test("kind is preserved", () => {
    const e = new RecorderError("wayland not supported", {
      kind: "not_implemented_wayland",
    });
    expect(e.kind).toBe("not_implemented_wayland");
  });
});

// ---- Phase B tests below ----

// Tests use a *real* `sleep 60` subprocess as the ffmpeg stand-in. Plan §2.2
// recommends DI'ing a fake spawn, but we still want a real PID so that
// `process.kill(pid, 0)` (the alive check) returns true. `sleep` is portable
// across Linux + macOS and exits on SIGTERM in <100ms.
function spawnSleep(): Bun.Subprocess {
  return Bun.spawn(["sleep", "60"], { stdout: "ignore", stderr: "ignore" });
}

describe("Recorder lifecycle (real sleep as fake ffmpeg)", () => {
  let scratch: string;
  const liveProcs: Bun.Subprocess[] = [];

  beforeEach(() => {
    scratch = mkdtempSync(join(tmpdir(), "gtk4-e2e-rec-"));
  });

  afterEach(async () => {
    // Best-effort cleanup of any stray `sleep 60` started during the test.
    for (const p of liveProcs.splice(0)) {
      try {
        p.kill();
      } catch {
        /* already exited */
      }
    }
    rmSync(scratch, { recursive: true, force: true });
  });

  test("start writes recorder.json, status reports running, stop deletes file", async () => {
    let captured: Bun.Subprocess | null = null;
    const r = new Recorder({
      output: join(scratch, "out.mp4"),
      registryDir: scratch,
      env: { DISPLAY: ":0" },
      ffmpegBin: "sleep",
      spawn: () => {
        const p = spawnSleep();
        liveProcs.push(p);
        captured = p;
        return p;
      },
    });

    await r.start();
    const pidFile = join(scratch, "recorder.json");
    expect(existsSync(pidFile)).toBe(true);
    const written = JSON.parse(readFileSync(pidFile, "utf8"));
    expect(captured).not.toBeNull();
    expect(written.pid).toBe((captured as unknown as Bun.Subprocess).pid);
    expect(written.output).toBe(join(scratch, "out.mp4"));

    const s = r.status();
    expect(s.running).toBe(true);
    expect(s.pid).toBe((captured as unknown as Bun.Subprocess).pid);
    expect(s.output).toBe(join(scratch, "out.mp4"));
    expect(typeof s.startedAt).toBe("string");
    expect(typeof s.elapsedMs).toBe("number");

    await r.stop();
    expect(existsSync(pidFile)).toBe(false);
  });

  test("start fails when an earlier recording is still running", async () => {
    const livePid = process.pid; // current process is definitely alive
    writeFileSync(
      join(scratch, "recorder.json"),
      JSON.stringify({
        pid: livePid,
        output: "/tmp/other.mp4",
        started_at: new Date().toISOString(),
        display: ":0",
      }),
    );
    const r = new Recorder({
      output: join(scratch, "out.mp4"),
      registryDir: scratch,
      env: { DISPLAY: ":0" },
      ffmpegBin: "sleep",
      spawn: () => {
        const p = spawnSleep();
        liveProcs.push(p);
        return p;
      },
    });
    await expect(r.start()).rejects.toThrow(RecorderError);
  });

  test("stale PID file is cleaned up automatically", async () => {
    // Use a pid that is essentially guaranteed to be dead.
    const stalePid = 999999;
    writeFileSync(
      join(scratch, "recorder.json"),
      JSON.stringify({
        pid: stalePid,
        output: "/tmp/old.mp4",
        started_at: new Date().toISOString(),
        display: ":0",
      }),
    );
    const r = new Recorder({
      output: join(scratch, "out.mp4"),
      registryDir: scratch,
      env: { DISPLAY: ":0" },
      ffmpegBin: "sleep",
      spawn: () => {
        const p = spawnSleep();
        liveProcs.push(p);
        return p;
      },
    });
    await r.start();
    expect(r.status().running).toBe(true);
    await r.stop();
  });

  test("Wayland env throws RecorderError without spawning", async () => {
    let spawnCalled = false;
    const r = new Recorder({
      output: join(scratch, "out.mp4"),
      registryDir: scratch,
      env: { WAYLAND_DISPLAY: "wayland-0" },
      ffmpegBin: "sleep",
      spawn: () => {
        spawnCalled = true;
        const p = spawnSleep();
        liveProcs.push(p);
        return p;
      },
    });
    await expect(r.start()).rejects.toThrow(RecorderError);
    expect(spawnCalled).toBe(false);
  });

  test("unknown display env throws RecorderError", async () => {
    const r = new Recorder({
      output: join(scratch, "out.mp4"),
      registryDir: scratch,
      env: {},
      ffmpegBin: "sleep",
      spawn: () => {
        const p = spawnSleep();
        liveProcs.push(p);
        return p;
      },
    });
    await expect(r.start()).rejects.toThrow(RecorderError);
  });

  test("missing ffmpeg binary throws (which returns null)", async () => {
    // Pass a non-existent ffmpegBin and let the default Bun.which run.
    const r = new Recorder({
      output: join(scratch, "out.mp4"),
      registryDir: scratch,
      env: { DISPLAY: ":0" },
      ffmpegBin: "this-binary-does-not-exist-zzz-9876",
      // No spawn DI: precheck should fire before spawn.
    });
    await expect(r.start()).rejects.toThrow(RecorderError);
  });

  test("stop without start throws RecorderError", async () => {
    const r = new Recorder({
      output: join(scratch, "out.mp4"),
      registryDir: scratch,
      env: { DISPLAY: ":0" },
      ffmpegBin: "sleep",
    });
    await expect(r.stop()).rejects.toThrow(RecorderError);
  });

  test("status() returns running:false when no PID file (no throw)", () => {
    const r = new Recorder({
      output: join(scratch, "out.mp4"),
      registryDir: scratch,
      env: { DISPLAY: ":0" },
      ffmpegBin: "sleep",
    });
    const s = r.status();
    expect(s.running).toBe(false);
    expect(s.pid).toBeNull();
  });

  test("status() handles broken JSON gracefully (running:false)", () => {
    writeFileSync(join(scratch, "recorder.json"), "not-json{");
    const r = new Recorder({
      output: join(scratch, "out.mp4"),
      registryDir: scratch,
      env: { DISPLAY: ":0" },
      ffmpegBin: "sleep",
    });
    expect(() => r.status()).not.toThrow();
    expect(r.status().running).toBe(false);
  });
});
