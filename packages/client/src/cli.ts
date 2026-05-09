#!/usr/bin/env bun

// Minimal CLI for `gtk4-e2e`. Exit code conventions (plan §Q5):
//   0  success
//   1  unexpected error
//   2  invalid argv (unknown subcommand / missing arg / unparsable flag)
//   3  NotImplementedError
//   4  DiscoveryError
//   5  HttpError
//
// The argv parser is hand-rolled to avoid a dependency. Step 8 (recorder /
// plugin) will likely revisit this.

import { E2EClient } from "./client.ts";
import { discover } from "./discover.ts";
import {
  DiscoveryError,
  E2EError,
  HttpError,
  NotImplementedError,
} from "./errors.ts";

const USAGE = `gtk4-e2e <subcommand> [args] [flags]

Subcommands:
  info                              GET /test/info → JSON to stdout
  tap <selector|x,y>                POST /test/tap
  screenshot <out.png>              GET /test/screenshot → save to file

Flags (apply to all subcommands):
  --port <p>     filter discover() to a specific port
  --pid <pid>    filter discover() to a specific pid
  --app <name>   filter discover() to a specific app_name
  --token <t>    Authorization: Bearer <t> (env GTK4_E2E_TOKEN also honored)
  --help, -h     show this message
`;

interface ParsedArgs {
  subcommand: string | null;
  positional: string[];
  flags: {
    port?: number;
    pid?: number;
    app?: string;
    token?: string;
    help: boolean;
  };
}

function parseArgs(argv: string[]): ParsedArgs {
  const result: ParsedArgs = {
    subcommand: null,
    positional: [],
    flags: { help: false },
  };

  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--help" || a === "-h") {
      result.flags.help = true;
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
