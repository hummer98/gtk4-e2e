#!/usr/bin/env bun

// Minimal CLI for `gtk4-e2e`. Exit code conventions (plan §Q5):
//   0  success
//   1  unexpected error
//   2  invalid argv (unknown subcommand / missing arg / unparsable flag)
//   3  NotImplementedError
//   4  DiscoveryError
//   5  HttpError
//   6  RecorderError (Step 8)
//   7  VisualDiffError (Step 19; baseline_missing or decode_failed)
//
// The argv parser is hand-rolled to avoid a dependency.

import { dirname, resolve } from "node:path";

import { E2EClient } from "./client.ts";
import { discover } from "./discover.ts";
import {
  DiscoveryError,
  E2EError,
  HttpError,
  NotImplementedError,
  RecorderError,
  VisualDiffError,
} from "./errors.ts";
import { Recorder } from "./recorder.ts";

const USAGE = `gtk4-e2e <subcommand> [args] [flags]

Subcommands:
  info                              GET /test/info → JSON to stdout
  tap <selector|x,y>                POST /test/tap
  type <selector> <text>            POST /test/type
  swipe <x1,y1> <x2,y2>             POST /test/swipe (default duration 300ms)
  pinch <x,y> <scale>               POST /test/pinch (default duration 300ms)
  screenshot <out.png>                                 GET /test/screenshot → save to file
  screenshot <name> --baseline <path>                  diff against baseline (exit 1 on mismatch)
                  [--threshold <0.0-1.0>] [--update-baseline]
  elements [--selector <s>] [--max-depth <n>]
                                    GET /test/elements → JSON tree to stdout
  record start --output <path>      start ffmpeg recording (X11 only in MVP)
  record stop                       stop the running recorder
  record status                     print recorder status as JSON

Flags (apply to all subcommands):
  --port <p>      filter discover() to a specific port
  --pid <pid>     filter discover() to a specific pid
  --app <name>    filter discover() to a specific app_name
  --token <t>     Authorization: Bearer <t> (env GTK4_E2E_TOKEN also honored)
  --output <path> recorder output file (record start)
  --display <:N>  X11 display for record start (default: $DISPLAY)
  --fps <n>       recorder framerate (default: 30)
  --duration <ms> swipe gesture duration in ms (default: 300)
  --selector <s>  selector for elements (e.g. #input1 or .primary)
  --max-depth <n> cap subtree depth for elements (0 = root only)
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
    duration?: number;
    selector?: string;
    maxDepth?: number;
    baseline?: string;
    updateBaseline: boolean;
    threshold?: number;
    verbose: boolean;
    help: boolean;
  };
}

function parseArgs(argv: string[]): ParsedArgs {
  const result: ParsedArgs = {
    subcommand: null,
    positional: [],
    flags: { help: false, verbose: false, updateBaseline: false },
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
      if (!Number.isFinite(n) || n <= 0) throw new ArgvError(`--pid: not a positive integer: ${v}`);
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
      if (!Number.isFinite(n) || n <= 0) throw new ArgvError(`--fps: not a positive integer: ${v}`);
      result.flags.fps = n;
      continue;
    }
    if (a === "--duration") {
      const v = argv[++i];
      if (v === undefined) throw new ArgvError("--duration requires a value");
      const n = Number.parseInt(v, 10);
      if (!Number.isFinite(n) || n <= 0)
        throw new ArgvError(`--duration: not a positive integer: ${v}`);
      result.flags.duration = n;
      continue;
    }
    if (a === "--selector") {
      const v = argv[++i];
      if (v === undefined) throw new ArgvError("--selector requires a value");
      result.flags.selector = v;
      continue;
    }
    if (a === "--max-depth") {
      const v = argv[++i];
      if (v === undefined) throw new ArgvError("--max-depth requires a value");
      const n = Number.parseInt(v, 10);
      if (!Number.isFinite(n) || n < 0)
        throw new ArgvError(`--max-depth: not a non-negative integer: ${v}`);
      result.flags.maxDepth = n;
      continue;
    }
    if (a === "--baseline") {
      const v = argv[++i];
      if (v === undefined) throw new ArgvError("--baseline requires a value");
      result.flags.baseline = v;
      continue;
    }
    if (a === "--update-baseline") {
      result.flags.updateBaseline = true;
      continue;
    }
    if (a === "--threshold") {
      const v = argv[++i];
      if (v === undefined) throw new ArgvError("--threshold requires a value");
      const n = Number.parseFloat(v);
      if (!Number.isFinite(n) || n < 0 || n > 1)
        throw new ArgvError(`--threshold: not in [0.0, 1.0]: ${v}`);
      result.flags.threshold = n;
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

  // Cross-flag validation (centralized here to keep `runScreenshot` from
  // re-checking; see plan §2.1.5). `--update-baseline` only makes sense in
  // diff mode, which requires `--baseline`. Guarded by subcommand so other
  // subcommands ignore the flag like they ignore other unrelated flags.
  if (
    result.subcommand === "screenshot" &&
    result.flags.updateBaseline === true &&
    result.flags.baseline === undefined
  ) {
    throw new ArgvError("--update-baseline requires --baseline <path>");
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
    throw new DiscoveryError("no gtk4-e2e instance found via local registry; try --port");
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

async function runType(parsed: ParsedArgs): Promise<void> {
  if (parsed.positional.length < 2) {
    throw new ArgvError("type requires <selector> <text>");
  }
  const [selector, text] = parsed.positional;
  const client = await buildClient(parsed);
  await client.type(selector, text);
}

function parseSwipeXY(arg: string): { x: number; y: number } {
  if (!/^\d+,\d+$/.test(arg)) {
    throw new ArgvError(`swipe: expected non-negative "x,y", got: ${arg}`);
  }
  const [x, y] = arg.split(",").map((s) => Number.parseInt(s, 10));
  return { x, y };
}

async function runSwipe(parsed: ParsedArgs): Promise<void> {
  if (parsed.positional.length < 2) {
    throw new ArgvError("swipe requires <x1,y1> <x2,y2>");
  }
  const from = parseSwipeXY(parsed.positional[0]);
  const to = parseSwipeXY(parsed.positional[1]);
  const durationMs = parsed.flags.duration ?? 300;
  const client = await buildClient(parsed);
  await client.swipe(from, to, durationMs);
}

function parsePinchScale(arg: string): number {
  const n = Number.parseFloat(arg);
  if (!Number.isFinite(n) || n <= 0) {
    throw new ArgvError(`pinch: expected positive scale, got: ${arg}`);
  }
  return n;
}

async function runPinch(parsed: ParsedArgs): Promise<void> {
  if (parsed.positional.length < 2) {
    throw new ArgvError("pinch requires <x,y> <scale>");
  }
  const center = parseSwipeXY(parsed.positional[0]);
  const scale = parsePinchScale(parsed.positional[1]);
  const durationMs = parsed.flags.duration ?? 300;
  const client = await buildClient(parsed);
  await client.pinch(center, scale, durationMs);
}

async function runElements(parsed: ParsedArgs): Promise<void> {
  const client = await buildClient(parsed);
  const resp = await client.elements({
    selector: parsed.flags.selector,
    maxDepth: parsed.flags.maxDepth,
  });
  process.stdout.write(`${JSON.stringify(resp, null, 2)}\n`);
}

async function runScreenshot(parsed: ParsedArgs): Promise<number> {
  if (parsed.positional.length === 0) {
    throw new ArgvError("screenshot requires <out.png> or <name> --baseline <path>");
  }
  const positional = parsed.positional[0];
  const client = await buildClient(parsed);

  // Save mode: backward-compatible (positional is treated as the output path).
  const baseline = parsed.flags.baseline;
  if (baseline === undefined) {
    const path = await client.screenshot(positional);
    process.stdout.write(`${path}\n`);
    return 0;
  }

  // Diff mode: positional is the SDK `name`; `--baseline <path>` supplies
  // only the directory (its basename / suffix is intentionally ignored — see
  // plan §Q1 sub-decision).
  const baselineDir = dirname(resolve(process.cwd(), baseline));
  const result = await client.expectScreenshot(positional, {
    baselineDir,
    threshold: parsed.flags.threshold,
    updateBaseline: parsed.flags.updateBaseline,
  });
  process.stdout.write(`${JSON.stringify({ name: positional, ...result }, null, 2)}\n`);
  return result.match ? 0 : 1;
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
      if (out === undefined) throw new ArgvError("record start requires --output <path>");
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
      throw new ArgvError(`unknown record sub-action: ${action} (expected start | stop | status)`);
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
      case "type":
        await runType(parsed);
        return 0;
      case "swipe":
        await runSwipe(parsed);
        return 0;
      case "pinch":
        await runPinch(parsed);
        return 0;
      case "screenshot":
        return await runScreenshot(parsed);
      case "elements":
        await runElements(parsed);
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
    if (err instanceof VisualDiffError) {
      process.stderr.write(`${err.message}\n`);
      return 7;
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
