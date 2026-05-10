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
  type?: (body: unknown) => Response | Promise<Response>;
  screenshot?: () => Response | Promise<Response>;
  swipe?: (body: unknown) => Response | Promise<Response>;
  elements?: (url: URL) => Response | Promise<Response>;
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
      if (url.pathname === "/test/type" && handlers.type) return handlers.type(body);
      if (url.pathname === "/test/screenshot" && handlers.screenshot) return handlers.screenshot();
      if (url.pathname === "/test/swipe" && handlers.swipe) return handlers.swipe(body);
      if (url.pathname === "/test/elements" && handlers.elements)
        return handlers.elements(url);
      return new Response("not found", { status: 404 });
    },
  });

  // Bun.serve(...).port is `number | undefined` for UDS support; we always
  // bind a TCP port (port: 0), so it is guaranteed non-null at runtime.
  return {
    port: server.port!,
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

    const result = await runCli(["info", "--port", String(mock.port), "--token", "foo"]);
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

    const result = await runCli(["tap", "100,200", "--port", String(mock.port)]);
    expect(result.exitCode).toBe(0);
    expect(mock.receivedBodies.at(-1)?.body).toEqual({ xy: { x: 100, y: 200 } });
  });

  test("selector form sends {selector}", async () => {
    mock = startMock({
      tap: () => new Response(null, { status: 204 }),
    });

    const result = await runCli(["tap", "#submit", "--port", String(mock.port)]);
    expect(result.exitCode).toBe(0);
    expect(mock.receivedBodies.at(-1)?.body).toEqual({ selector: "#submit" });
  });

  test("exits 5 on HttpError (404 selector_not_found)", async () => {
    // Plan Review M2: server 404 is now domain not-found (e.g.
    // selector_not_found), not capability missing. Capability missing
    // surfaces as 501 → exit 3.
    mock = startMock({
      tap: () =>
        new Response(JSON.stringify({ error: "selector_not_found", selector: "#submit" }), {
          status: 404,
          headers: { "content-type": "application/json" },
        }),
    });

    const result = await runCli(["tap", "#submit", "--port", String(mock.port)]);
    expect(result.exitCode).toBe(5);
  });

  test("exits 3 on NotImplementedError (501)", async () => {
    mock = startMock({
      tap: () =>
        new Response(JSON.stringify({ error: "not_implemented", capability: "tap" }), {
          status: 501,
          headers: { "content-type": "application/json" },
        }),
    });

    const result = await runCli(["tap", "#submit", "--port", String(mock.port)]);
    expect(result.exitCode).toBe(3);
  });
});

describe("cli type", () => {
  let mock: MockServer;

  afterEach(async () => {
    await mock.stop();
  });

  test("sends {selector,text} as POST /test/type body", async () => {
    mock = startMock({
      type: () => new Response(null, { status: 200 }),
    });

    const result = await runCli(["type", "#input1", "hello", "--port", String(mock.port)]);
    expect(result.exitCode).toBe(0);
    expect(mock.receivedBodies.at(-1)?.body).toEqual({
      selector: "#input1",
      text: "hello",
    });
  });

  test("missing text argument exits 2", async () => {
    mock = startMock({
      type: () => new Response(null, { status: 200 }),
    });

    const result = await runCli(["type", "#input1", "--port", String(mock.port)]);
    expect(result.exitCode).toBe(2);
    expect(result.stderr.length).toBeGreaterThan(0);
  });
});

describe("cli swipe", () => {
  let mock: MockServer;

  afterEach(async () => {
    await mock.stop();
  });

  test("sends from / to / duration_ms with explicit --duration", async () => {
    mock = startMock({ swipe: () => new Response(null, { status: 200 }) });

    const result = await runCli([
      "swipe",
      "100,400",
      "100,100",
      "--duration",
      "300",
      "--port",
      String(mock.port),
    ]);
    expect(result.exitCode).toBe(0);
    expect(mock.receivedBodies.at(-1)?.body).toEqual({
      from: { x: 100, y: 400 },
      to: { x: 100, y: 100 },
      duration_ms: 300,
    });
  });

  test("default duration is 300ms when --duration omitted", async () => {
    mock = startMock({ swipe: () => new Response(null, { status: 200 }) });

    const result = await runCli([
      "swipe",
      "10,20",
      "30,40",
      "--port",
      String(mock.port),
    ]);
    expect(result.exitCode).toBe(0);
    expect(mock.receivedBodies.at(-1)?.body).toEqual({
      from: { x: 10, y: 20 },
      to: { x: 30, y: 40 },
      duration_ms: 300,
    });
  });

  test("missing positional args exits 2", async () => {
    const result = await runCli(["swipe", "10,20"]);
    expect(result.exitCode).toBe(2);
  });

  test("negative coordinates exit 2 (parseSwipeXY non-negative only)", async () => {
    mock = startMock({ swipe: () => new Response(null, { status: 200 }) });

    const result = await runCli([
      "swipe",
      "-100,100",
      "100,100",
      "--port",
      String(mock.port),
    ]);
    expect(result.exitCode).toBe(2);
    expect(result.stderr).toContain("non-negative");
  });

  test("USAGE mentions the swipe subcommand", async () => {
    const result = await runCli(["--help"]);
    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("swipe");
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
    const result = await runCli(["screenshot", out, "--port", String(mock.port)]);
    expect(result.exitCode).toBe(0);
    const bytes = await Bun.file(out).bytes();
    expect(bytes[0]).toBe(0x89);
  });
});

describe("cli elements", () => {
  let mock: MockServer;
  let lastUrl: URL | null = null;

  afterEach(async () => {
    await mock.stop();
    lastUrl = null;
  });

  const sampleResp = {
    roots: [
      {
        id: "e0",
        kind: "GtkApplicationWindow",
        widget_name: "win1",
        css_classes: [],
        visible: true,
        sensitive: true,
        children: [],
      },
    ],
    count: 1,
  };

  test("prints pretty JSON tree to stdout (no flags)", async () => {
    mock = startMock({
      elements: (url) => {
        lastUrl = url;
        return Response.json(sampleResp);
      },
    });

    const result = await runCli(["elements", "--port", String(mock.port)]);
    expect(result.exitCode).toBe(0);
    const parsed = JSON.parse(result.stdout) as typeof sampleResp;
    expect(parsed).toEqual(sampleResp);
    expect(lastUrl?.search).toBe("");
  });

  test("forwards --selector and --max-depth as query params", async () => {
    mock = startMock({
      elements: (url) => {
        lastUrl = url;
        return Response.json(sampleResp);
      },
    });

    const result = await runCli([
      "elements",
      "--selector",
      "#input1",
      "--max-depth",
      "2",
      "--port",
      String(mock.port),
    ]);
    expect(result.exitCode).toBe(0);
    expect(lastUrl?.searchParams.get("selector")).toBe("#input1");
    expect(lastUrl?.searchParams.get("max_depth")).toBe("2");
  });

  test("--max-depth with negative value exits 2", async () => {
    mock = startMock({ elements: () => Response.json(sampleResp) });

    const result = await runCli([
      "elements",
      "--max-depth",
      "-1",
      "--port",
      String(mock.port),
    ]);
    expect(result.exitCode).toBe(2);
  });

  test("--max-depth with non-integer value exits 2", async () => {
    mock = startMock({ elements: () => Response.json(sampleResp) });

    const result = await runCli([
      "elements",
      "--max-depth",
      "abc",
      "--port",
      String(mock.port),
    ]);
    expect(result.exitCode).toBe(2);
  });

  test("501 from server exits 3 (NotImplementedError)", async () => {
    mock = startMock({
      elements: () =>
        new Response(
          JSON.stringify({ error: "not_implemented", capability: "elements" }),
          {
            status: 501,
            headers: { "content-type": "application/json" },
          },
        ),
    });

    const result = await runCli(["elements", "--port", String(mock.port)]);
    expect(result.exitCode).toBe(3);
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

  test("USAGE mentions the record subcommand", async () => {
    const result = await runCli(["--help"]);
    expect(result.exitCode).toBe(0);
    expect(result.stdout).toContain("record");
  });
});

describe("cli record", () => {
  let scratch: string;

  beforeEach(() => {
    scratch = mkdtempSync(join(tmpdir(), "gtk4-e2e-cli-rec-"));
  });

  afterEach(() => {
    rmSync(scratch, { recursive: true, force: true });
  });

  // The CLI uses GTK4_E2E_RECORDER_BIN as a back-door so tests can stand a
  // real `sleep` in for ffmpeg without touching X11. XDG_RUNTIME_DIR pins
  // the PID file location to a tmpdir so concurrent test runs don't trip
  // over each other's recorder.json.
  async function runRecord(
    args: string[],
    extraEnv: Record<string, string> = {},
  ): Promise<SpawnResult> {
    const env: Record<string, string> = {
      ...(process.env as Record<string, string>),
      XDG_RUNTIME_DIR: scratch,
      DISPLAY: ":0",
      GTK4_E2E_RECORDER_BIN: "sleep",
      GTK4_E2E_RECORDER_FAKE_ARGS: "1",
      ...extraEnv,
    };
    const proc = Bun.spawn(["bun", "run", CLI_PATH, ...args], {
      cwd: REPO_ROOT,
      env,
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

  test("start --output starts the recorder, status says running, stop ends it", async () => {
    const out = join(scratch, "run.mp4");
    const start = await runRecord(["record", "start", "--output", out]);
    expect(start.exitCode).toBe(0);

    const status = await runRecord(["record", "status"]);
    expect(status.exitCode).toBe(0);
    const parsed = JSON.parse(status.stdout) as {
      running: boolean;
      output: string;
    };
    expect(parsed.running).toBe(true);
    expect(parsed.output).toBe(out);

    const stop = await runRecord(["record", "stop"]);
    expect(stop.exitCode).toBe(0);

    const after = await runRecord(["record", "status"]);
    expect(after.exitCode).toBe(0);
    const afterParsed = JSON.parse(after.stdout) as { running: boolean };
    expect(afterParsed.running).toBe(false);
  });

  test("start without --output exits 2", async () => {
    const result = await runRecord(["record", "start"]);
    expect(result.exitCode).toBe(2);
    expect(result.stderr.length).toBeGreaterThan(0);
  });

  test("starting twice without stop exits 6 (RecorderError)", async () => {
    const out = join(scratch, "run.mp4");
    const first = await runRecord(["record", "start", "--output", out]);
    expect(first.exitCode).toBe(0);

    const second = await runRecord(["record", "start", "--output", out]);
    expect(second.exitCode).toBe(6);

    await runRecord(["record", "stop"]);
  });

  test("missing record sub-action exits 2", async () => {
    const result = await runRecord(["record"]);
    expect(result.exitCode).toBe(2);
  });

  test("unknown record sub-action exits 2", async () => {
    const result = await runRecord(["record", "frobnicate"]);
    expect(result.exitCode).toBe(2);
  });
});
