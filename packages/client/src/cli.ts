#!/usr/bin/env bun

// Minimal CLI for `gtk4-e2e`. Exit code conventions (plan §Q5):
//   0  success
//   1  unexpected error
//   2  invalid argv (unknown subcommand / missing arg / unparsable flag)
//   3  NotImplementedError
//   4  DiscoveryError
//   5  HttpError
//   6  RecorderError (Step 8)
//
// The argv parser is hand-rolled to avoid a dependency.

import { E2EClient } from "./client.ts";
import { discover } from "./discover.ts";
import {
  DiscoveryError,
  E2EError,
  HttpError,
  NotImplementedError,
  RecorderError,
} from "./errors.ts";
import { Recorder } from "./recorder.ts";

const USAGE = `gtk4-e2e <subcommand> [args] [flags]

Subcommands:
  info                              GET /test/info → JSON to stdout
  tap <selector|x,y>                POST /test/tap
  screenshot <out.png>              GET /test/screenshot → save to file
  record start --output <path>      start ffmpeg recording (X11 only in MVP)
  record stop                       stop the running recorder
  record status                     print recorder status as JSON

Flags (apply to all subcommands):
  --port <p>      filter discover() to a specific port
  --pid <pid>     filter discover() to a specific pid
  --app <name>    filter discover() to a specific app_name
  --token <t>     Authorization: Bearer <t> (env GTK4_E2E_TOKEN also honored)
  --output <path> recorder output file (record start)
  --display <:N>  X11 display for record start (default: \$DISPLAY)
  --fps <n>       recorder framerate (default: 30)
  --verbose       inherit recorder stderr (record start)
  --help, -h      show this message
`;

interface ParsedArgs {
  subcommand: string | null;
  positional: string[];
  flags: {
    port?: number;
    pid?: number;
    app?: string;
    token?: string;
    output?: string;
    display?: string;
    fps?: number;
    verbose: boolean;
    help: boolean;
  };
}

function parseArgs(argv: string[]): ParsedArgs {
  const result: ParsedArgs = {
    subcommand: null,
    positional: [],
    flags: { help: false, verbose: false },
  };

  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--help" || a === "-h") {
      result.flags.help = true;
      continue;
    }
    if (a === "--verbose") {
      result.flags.verbose = true;
      continue;
    }
    if (a === "--port") {
      const v = argv[++i];
      if (v === undefined) throw new ArgvError("--port requires a value");
      const n = Number.parseInt(v, 10);
      if (!Number.isFinite(n) || n <= 0)
        throw new ArgvError(`--port: not a positive integer: ${v}`);
      result.flags.port = n;
      continue;
    }
    if (a === "--pid") {
      const v = argv[++i];
      if (v === undefined) throw new ArgvError("--pid requires a value");
      const n = Number.parseInt(v, 10);
      if (!Number.isFinite(n) || n <= 0)
        throw new ArgvError(`--pid: not a positive integer: ${v}`);
      result.flags.pid = n;
      continue;
    }
    if (a === "--app") {
      const v = argv[++i];
      if (v === undefined) throw new ArgvError("--app requires a value");
      result.flags.app = v;
      continue;
    }
    if (a === "--token") {
      const v = argv[++i];
      if (v === undefined) throw new ArgvError("--token requires a value");
      result.flags.token = v;
      continue;
    }
    if (a === "--output") {
      const v = argv[++i];
      if (v === undefined) throw new ArgvError("--output requires a value");
      result.flags.output = v;
      continue;
    }
    if (a === "--display") {
      const v = argv[++i];
      if (v === undefined) throw new ArgvError("--display requires a value");
      result.flags.display = v;
      continue;
    }
    if (a === "--fps") {
      const v = argv[++i];
      if (v === undefined) throw new ArgvError("--fps requires a value");
      const n = Number.parseInt(v, 10);
      if (!Number.isFinite(n) || n <= 0)
        throw new ArgvError(`--fps: not a positive integer: ${v}`);
      result.flags.fps = n;
      continue;
    }
    if (a.startsWith("--")) {
      throw new ArgvError(`unknown flag: ${a}`);
    }
    if (result.subcommand === null) {
      result.subcommand = a;
    } else {
      result.positional.push(a);
    }
  }

  return result;
}

class ArgvError extends Error {}

async function buildClient(parsed: ParsedArgs): Promise<E2EClient> {
  if (parsed.flags.port !== undefined) {
    return new E2EClient({
      baseUrl: `http://127.0.0.1:${parsed.flags.port}`,
      token: parsed.flags.token,
    });
  }
  // Fall back to discover().
  const matches = await discover({
    pid: parsed.flags.pid,
    appName: parsed.flags.app,
  });
  if (matches.length === 0) {
    throw new DiscoveryError(
      "no gtk4-e2e instance found via local registry; try --port",
    );
  }
  const sorted = [...matches].sort((a, b) =>
    a.started_at < b.started_at ? 1 : a.started_at > b.started_at ? -1 : 0,
  );
  const newest = sorted[0];
  return new E2EClient({
    baseUrl: `http://127.0.0.1:${newest.port}`,
    token: parsed.flags.token,
  });
}

function parseTapTarget(arg: string): string | { x: number; y: number } {
  if (/^\d+,\d+$/.test(arg)) {
    const [x, y] = arg.split(",").map((s) => Number.parseInt(s, 10));
    return { x, y };
  }
  return arg;
}

async function runInfo(parsed: ParsedArgs): Promise<void> {
  const client = await buildClient(parsed);
  const info = await client.getInfo();
  process.stdout.write(`${JSON.stringify(info, null, 2)}\n`);
}

async function runTap(parsed: ParsedArgs): Promise<void> {
  if (parsed.positional.length === 0) {
    throw new ArgvError("tap requires a target (<selector> or <x,y>)");
  }
  const target = parseTapTarget(parsed.positional[0]);
  const client = await buildClient(parsed);
  await client.tap(target);
}

async function runScreenshot(parsed: ParsedArgs): Promise<void> {
  if (parsed.positional.length === 0) {
    throw new ArgvError("screenshot requires an output path");
  }
  const out = parsed.positional[0];
  const client = await buildClient(parsed);
  const path = await client.screenshot(out);
  process.stdout.write(`${path}\n`);
}

function buildRecorderFromEnv(parsed: ParsedArgs, output: string): Recorder {
  // GTK4_E2E_RECORDER_BIN lets tests stand in `sleep` for ffmpeg without
  // touching X11 or installing ffmpeg. In production the env is unset and
  // the default ("ffmpeg") is used.
  const ffmpegBin = process.env.GTK4_E2E_RECORDER_BIN;
  // GTK4_E2E_RECORDER_FAKE_ARGS short-circuits ffmpeg-style args (which
  // sleep wouldn't accept). When the test env asks for a fake spawn, build
  // an args list the fake bin can actually run.
  const fakeArgs = process.env.GTK4_E2E_RECORDER_FAKE_ARGS === "1";
  const spawnFn = fakeArgs
    ? (_cmd: string[]) =>
        Bun.spawn([ffmpegBin ?? "sleep", "60"], {
          stdout: "ignore",
          stderr: "ignore",
        })
    : undefined;
  return new Recorder({
    output,
    display: parsed.flags.display,
    fps: parsed.flags.fps,
    ffmpegBin,
    spawn: spawnFn,
    verbose: parsed.flags.verbose,
  });
}

async function runRecord(parsed: ParsedArgs): Promise<number> {
  const action = parsed.positional[0];
  if (action === undefined) {
    throw new ArgvError("record requires a sub-action: start | stop | status");
  }
  switch (action) {
    case "start": {
      const out = parsed.flags.output;
      if (out === undefined)
        throw new ArgvError("record start requires --output <path>");
      const r = buildRecorderFromEnv(parsed, out);
      await r.start();
      return 0;
    }
    case "stop": {
      // The output isn't needed at stop time — Recorder reads the PID file.
      // Pass a placeholder.
      const r = buildRecorderFromEnv(parsed, parsed.flags.output ?? "");
      await r.stop();
      return 0;
    }
    case "status": {
      const r = buildRecorderFromEnv(parsed, parsed.flags.output ?? "");
      const s = r.status();
      process.stdout.write(`${JSON.stringify(s, null, 2)}\n`);
      return 0;
    }
    default:
      throw new ArgvError(
        `unknown record sub-action: ${action} (expected start | stop | status)`,
      );
  }
}

async function main(argv: string[]): Promise<number> {
  let parsed: ParsedArgs;
  try {
    parsed = parseArgs(argv);
  } catch (err) {
    if (err instanceof ArgvError) {
      process.stderr.write(`${err.message}\n\n${USAGE}`);
      return 2;
    }
    throw err;
  }

  if (parsed.flags.help) {
    process.stdout.write(USAGE);
    return 0;
  }

  if (parsed.subcommand === null) {
    process.stderr.write(`missing subcommand\n\n${USAGE}`);
    return 2;
  }

  try {
    switch (parsed.subcommand) {
      case "info":
        await runInfo(parsed);
        return 0;
      case "tap":
        await runTap(parsed);
        return 0;
      case "screenshot":
        await runScreenshot(parsed);
        return 0;
      case "record":
        return await runRecord(parsed);
      default:
        process.stderr.write(`unknown subcommand: ${parsed.subcommand}\n\n${USAGE}`);
        return 2;
    }
  } catch (err) {
    if (err instanceof ArgvError) {
      process.stderr.write(`${err.message}\n\n${USAGE}`);
      return 2;
    }
    if (err instanceof NotImplementedError) {
      process.stderr.write(`${err.message}\n`);
      return 3;
    }
    if (err instanceof DiscoveryError) {
      process.stderr.write(`${err.message}\n`);
      return 4;
    }
    if (err instanceof HttpError) {
      process.stderr.write(`${err.message}\n`);
      return 5;
    }
    if (err instanceof RecorderError) {
      process.stderr.write(`${err.message}\n`);
      return 6;
    }
    if (err instanceof E2EError) {
      process.stderr.write(`${err.message}\n`);
      return 1;
    }
    process.stderr.write(`unexpected error: ${String(err)}\n`);
    return 1;
  }
}

if (import.meta.main) {
  const code = await main(process.argv.slice(2));
  process.exit(code);
}
