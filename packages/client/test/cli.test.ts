import "./_setup.ts";

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import type { Info } from "../src/types.gen.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, "..", "..", "..");
const CLI_PATH = resolve(HERE, "..", "src", "cli.ts");

const sampleInfo: Info = {
  instance_id: "abc",
  pid: 4242,
  port: 19042,
  app_name: "gtk4-e2e-app",
  app_version: "0.1.0",
  capabilities: ["info"],
};

interface MockServer {
  port: number;
  receivedAuth: string[];
  receivedBodies: Array<{ path: string; method: string; body: unknown }>;
  stop(): Promise<void>;
}

interface RouteHandlers {
  info?: () => Response | Promise<Response>;
  tap?: (body: unknown) => Response | Promise<Response>;
  screenshot?: () => Response | Promise<Response>;
}

function startMock(handlers: RouteHandlers): MockServer {
  const receivedAuth: string[] = [];
  const receivedBodies: Array<{ path: string; method: string; body: unknown }> = [];

  const server = Bun.serve({
    port: 0,
    async fetch(req) {
      const url = new URL(req.url);
      receivedAuth.push(req.headers.get("authorization") ?? "");

      let body: unknown = null;
      if (req.method !== "GET" && req.headers.get("content-type")?.includes("application/json")) {
        try {
          body = await req.json();
        } catch {
          body = null;
        }
      }
      receivedBodies.push({ path: url.pathname, method: req.method, body });

      if (url.pathname === "/test/info" && handlers.info) return handlers.info();
      if (url.pathname === "/test/tap" && handlers.tap) return handlers.tap(body);
      if (url.pathname === "/test/screenshot" && handlers.screenshot) return handlers.screenshot();
      return new Response("not found", { status: 404 });
    },
  });

  return {
    port: server.port,
    receivedAuth,
    receivedBodies,
    async stop() {
      await server.stop(true);
    },
  };
}

interface SpawnResult {
  exitCode: number;
  stdout: string;
  stderr: string;
}

async function runCli(args: string[]): Promise<SpawnResult> {
  const proc = Bun.spawn(["bun", "run", CLI_PATH, ...args], {
    cwd: REPO_ROOT,
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, stderr] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
  ]);
  const exitCode = await proc.exited;
  return { exitCode, stdout, stderr };
}

describe("cli info", () => {
  let mock: MockServer;

  afterEach(async () => {
    await mock.stop();
  });

  test("prints JSON info to stdout", async () => {
    mock = startMock({ info: () => Response.json(sampleInfo) });

    const result = await runCli(["info", "--port", String(mock.port)]);
    expect(result.exitCode).toBe(0);

    const parsed = JSON.parse(result.stdout) as Info;
    expect(parsed).toEqual(sampleInfo);
  });

  test("forwards --token as Authorization: Bearer", async () => {
    mock = startMock({ info: () => Response.json(sampleInfo) });

    const result = await runCli([
      "info",
      "--port",
      String(mock.port),
      "--token",
      "foo",
    ]);
    expect(result.exitCode).toBe(0);
    expect(mock.receivedAuth.at(-1)).toBe("Bearer foo");
  });

  test("exits 4 (DiscoveryError) when no instance is reachable and no --port given", async () => {
    mock = startMock({ info: () => Response.json(sampleInfo) });
    // Don't pass --port; the CLI will call discover() which won't find this mock.
    // To make discover() come back empty deterministically, point at a tmp registry dir
    // via env so it doesn't pick up other developers' real instances.
    const env = {
      ...process.env,
      XDG_RUNTIME_DIR: "/tmp/this-path-deliberately-does-not-exist-gtk4e2e",
    };
    const proc = Bun.spawn(["bun", "run", CLI_PATH, "info"], {
      cwd: REPO_ROOT,
      env,
      stdout: "pipe",
      stderr: "pipe",
    });
    const stderr = await new Response(proc.stderr).text();
    const exitCode = await proc.exited;
    expect(exitCode).toBe(4);
    expect(stderr.length).toBeGreaterThan(0);
  });
});

describe("cli tap", () => {
  let mock: MockServer;

  afterEach(async () => {
    await mock.stop();
  });

  test("xy form sends {xy:{x,y}}", async () => {
    mock = startMock({
      tap: () => new Response(null, { status: 204 }),
    });

    const result = await runCli([
      "tap",
      "100,200",
      "--port",
      String(mock.port),
    ]);
    expect(result.exitCode).toBe(0);
    expect(mock.receivedBodies.at(-1)?.body).toEqual({ xy: { x: 100, y: 200 } });
  });

  test("selector form sends {selector}", async () => {
    mock = startMock({
      tap: () => new Response(null, { status: 204 }),
    });

    const result = await runCli([
      "tap",
      "#submit",
      "--port",
      String(mock.port),
    ]);
    expect(result.exitCode).toBe(0);
    expect(mock.receivedBodies.at(-1)?.body).toEqual({ selector: "#submit" });
  });

  test("exits 3 on NotImplementedError (404)", async () => {
    mock = startMock({});

    const result = await runCli([
      "tap",
      "#submit",
      "--port",
      String(mock.port),
    ]);
    expect(result.exitCode).toBe(3);
  });
});

describe("cli screenshot", () => {
  let mock: MockServer;
  let scratch: string;

  beforeEach(() => {
    scratch = mkdtempSync(join(tmpdir(), "gtk4-e2e-cli-shot-"));
  });

  afterEach(async () => {
    await mock.stop();
    rmSync(scratch, { recursive: true, force: true });
  });

  test("writes PNG bytes to the given path", async () => {
    const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    mock = startMock({
      screenshot: () =>
        new Response(png, {
          status: 200,
          headers: { "content-type": "image/png" },
        }),
    });

    const out = join(scratch, "out.png");
    const result = await runCli([
      "screenshot",
      out,
      "--port",
      String(mock.port),
    ]);
    expect(result.exitCode).toBe(0);
    const bytes = await Bun.file(out).bytes();
    expect(bytes[0]).toBe(0x89);
  });
});

describe("cli error handling", () => {
  test("unknown subcommand exits 2", async () => {
    const result = await runCli(["frobnicate"]);
    expect(result.exitCode).toBe(2);
    expect(result.stderr.length).toBeGreaterThan(0);
  });

  test("--help exits 0 and prints usage", async () => {
    const result = await runCli(["--help"]);
    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("gtk4-e2e");
  });
});
