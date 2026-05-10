// End-to-end scenario helpers: spawn the demo binary, wait for the server
// banner on stderr, hand back an `E2EClient`. Plan §Q13 / §Open Q-F.
//
// We pre-build the demo with `cargo build` (sync, blocking) before spawning
// to avoid the cold-start cost on each scenario run. Spawn uses the produced
// binary at `target/debug/gtk4-e2e-demo` directly so subsequent runs skip
// `cargo`.

import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { resolve } from "node:path";

import { E2EClient } from "../../client/src/client.ts";

export const REPO_ROOT = resolve(import.meta.dir, "..", "..", "..");

/** Detect whether a GUI display is reachable for GTK. */
export function hasDisplay(): boolean {
  if (process.platform === "darwin") {
    // macOS Quartz backend doesn't need DISPLAY/WAYLAND_DISPLAY, but the
    // user must be in a graphical session for the demo window to map.
    // We're conservative: rely on the demo emitting "server up on" on
    // stderr; if it doesn't, the scenario times out and we surface that.
    return true;
  }
  return Boolean(process.env["DISPLAY"] ?? process.env["WAYLAND_DISPLAY"]);
}

interface DemoHandle {
  client: E2EClient;
  proc: ChildProcess;
  teardown: () => Promise<void>;
}

/**
 * Build (if necessary) and spawn the demo binary. Resolves once the server
 * has emitted its `server up on http://127.0.0.1:NNNN` line on stderr.
 */
export async function spawnDemo(timeoutMs = 10_000): Promise<DemoHandle> {
  // Build first to keep spawn fast and predictable. `--features e2e` is
  // mandatory: the no-feature build doesn't include the server.
  const built = spawnSync("cargo", ["build", "-p", "gtk4-e2e-demo", "--features", "e2e"], {
    cwd: REPO_ROOT,
    stdio: "inherit",
    env: { ...process.env, GTK4_E2E_TOKEN: "" },
  });
  if (built.status !== 0) {
    throw new Error(`cargo build failed (status ${built.status})`);
  }

  const binary = resolve(REPO_ROOT, "target", "debug", "gtk4-e2e-demo");
  const proc = spawn(binary, [], {
    cwd: REPO_ROOT,
    env: { ...process.env, GTK4_E2E_TOKEN: "" },
    stdio: ["ignore", "pipe", "pipe"],
  });

  const port = await waitForServerLog(proc, timeoutMs);
  // Pass `token: ""` explicitly so the test process's GTK4_E2E_TOKEN doesn't
  // leak in. Plan §Open Q-H.
  const client = new E2EClient({ baseUrl: `http://127.0.0.1:${port}`, token: "" });
  return {
    client,
    proc,
    async teardown() {
      proc.kill("SIGTERM");
      await new Promise<void>((res) => {
        if (proc.exitCode !== null) {
          res();
          return;
        }
        proc.once("exit", () => res());
        // Hard fallback after 2s.
        setTimeout(() => {
          if (proc.exitCode === null) {
            try {
              proc.kill("SIGKILL");
            } catch {
              // already dead
            }
          }
          res();
        }, 2000);
      });
    },
  };
}

function waitForServerLog(proc: ChildProcess, timeoutMs: number): Promise<number> {
  return new Promise((resolve, reject) => {
    let buf = "";
    const timer = setTimeout(() => {
      reject(new Error(`demo never emitted server-ready banner within ${timeoutMs}ms`));
    }, timeoutMs);

    proc.stderr?.on("data", (chunk: Buffer) => {
      buf += chunk.toString("utf8");
      const m = buf.match(/server up on http:\/\/127\.0\.0\.1:(\d+)/);
      if (m) {
        clearTimeout(timer);
        resolve(Number.parseInt(m[1], 10));
      }
    });

    proc.on("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`demo exited (code ${code}) before becoming ready: ${buf}`));
    });
  });
}
