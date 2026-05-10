// HTTP client surface — see plan §Q11 / §Q12 for the response triage rules.
//
//   * 501       → NotImplementedError (capability missing — plan Review M2)
//   * 408       → WaitTimeoutError (long-poll deadline reached)
//   * 4xx / 5xx → HttpError (everything else, including 404 selector_not_found)
//   * ok + json → parsed JSON
//   * ok + bytes → Uint8Array via `res.bytes()`
//
// Type imports come from `./types.gen.ts` (SSOT: server proto.rs → JSON Schema
// → TS). The hand-written `TapTarget` type that used to live here was removed
// in plan Review C3.

import { discover, type DiscoverFilter, type InstanceFile } from "./discover.ts";
import {
  DiscoveryError,
  E2EError,
  HttpError,
  NotImplementedError,
  WaitTimeoutError,
} from "./errors.ts";
import {
  openEventStream,
  type EventStream,
  type EventsOptions,
} from "./events.ts";
import type {
  Info,
  TapTarget,
  WaitCondition,
  WaitResult,
} from "./types.gen.ts";

interface ClientOptions {
  baseUrl: string;
  token?: string;
}

type ResponseKind = "json" | "bytes" | "void";

interface RequestOptions {
  method: "GET" | "POST";
  path: string;
  body?: unknown;
  capability: string;
  expect: ResponseKind;
  /** Server-side deadline for long-poll endpoints; surfaced via WaitTimeoutError. */
  waitTimeoutMs?: number;
}

const DEFAULT_WAIT_TIMEOUT_MS = 5_000;

export class E2EClient {
  readonly baseUrl: string;
  readonly token?: string;

  constructor(opts: ClientOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/$/, "");
    // Only fall back to the env var when the caller did not pass `token`
    // explicitly. Passing `token: ""` opts out (used by demo scenarios that
    // don't want to inherit the developer's local `GTK4_E2E_TOKEN`).
    if (opts.token !== undefined) {
      this.token = opts.token === "" ? undefined : opts.token;
    } else {
      this.token = process.env["GTK4_E2E_TOKEN"];
    }
  }

  static async discover(filter: DiscoverFilter = {}): Promise<InstanceFile[]> {
    return discover(filter);
  }

  static async connect(filter: DiscoverFilter = {}): Promise<E2EClient> {
    const matches = await discover(filter);
    if (matches.length === 0) {
      throw new DiscoveryError(
        "no gtk4-e2e instance matched the given filter",
      );
    }
    // Multiple hits: pick the most recently started. RFC3339 strings sort
    // lexicographically as long as the offset is consistent (the server emits
    // UTC `Z` suffixes), which is good enough for "newest wins" tie-breaking.
    const sorted = [...matches].sort((a, b) =>
      a.started_at < b.started_at ? 1 : a.started_at > b.started_at ? -1 : 0,
    );
    const newest = sorted[0];
    return new E2EClient({
      baseUrl: `http://127.0.0.1:${newest.port}`,
    });
  }

  async getInfo(): Promise<Info> {
    return this._request<Info>({
      method: "GET",
      path: "/test/info",
      capability: "info",
      expect: "json",
    });
  }

  /**
   * Synthesize a tap. Coordinates passed via `{ x, y }` are window-local with
   * a top-left origin (px), targeting the application's active window.
   */
  async tap(target: TapTarget | { x: number; y: number } | string): Promise<void> {
    const body = normaliseTapTarget(target);
    await this._request<void>({
      method: "POST",
      path: "/test/tap",
      body,
      capability: "tap",
      expect: "void",
    });
  }

  /**
   * Long-poll until `condition` matches or the server deadline elapses.
   * On 408, throws `WaitTimeoutError`.
   */
  async wait(
    condition: WaitCondition,
    options?: { timeoutMs?: number },
  ): Promise<WaitResult> {
    const timeoutMs = options?.timeoutMs ?? DEFAULT_WAIT_TIMEOUT_MS;
    return this._request<WaitResult>({
      method: "POST",
      path: "/test/wait",
      body: { condition, timeout_ms: timeoutMs },
      capability: "wait",
      expect: "json",
      waitTimeoutMs: timeoutMs,
    });
  }

  /**
   * Subscribe to `WS /test/events` and return an async iterable of envelopes.
   *
   * Resolves once the WebSocket has opened and the server-side subscriber is
   * live, so callers can chain a triggering action (e.g. `await client.tap(...)`)
   * immediately after the await without racing the broadcaster.
   */
  async events(opts?: EventsOptions): Promise<EventStream> {
    return openEventStream(this, opts);
  }

  async screenshot(): Promise<Uint8Array>;
  async screenshot(path: string): Promise<string>;
  async screenshot(path?: string): Promise<Uint8Array | string> {
    const bytes = await this._request<Uint8Array>({
      method: "GET",
      path: "/test/screenshot",
      capability: "screenshot",
      expect: "bytes",
    });
    if (path === undefined) return bytes;
    await Bun.write(path, bytes);
    return path;
  }

  private async _request<T>(opts: RequestOptions): Promise<T> {
    const headers: Record<string, string> = {};
    if (this.token) headers["authorization"] = `Bearer ${this.token}`;
    if (opts.body !== undefined) headers["content-type"] = "application/json";

    const init: RequestInit = {
      method: opts.method,
      headers,
    };
    if (opts.body !== undefined) {
      init.body = JSON.stringify(opts.body);
    }

    let res: Response;
    try {
      res = await fetch(`${this.baseUrl}${opts.path}`, init);
    } catch (err) {
      throw new E2EError(
        `network error contacting ${this.baseUrl}${opts.path}`,
        { cause: err },
      );
    }

    if (res.status === 408) {
      throw new WaitTimeoutError(opts.waitTimeoutMs ?? DEFAULT_WAIT_TIMEOUT_MS);
    }
    if (res.status === 501) {
      throw new NotImplementedError(opts.capability, res.status);
    }
    if (res.status >= 400) {
      const body = await safeReadBody(res);
      throw new HttpError(
        res.status,
        body,
        `HTTP ${res.status} from ${opts.method} ${opts.path}`,
      );
    }

    if (opts.expect === "void") return undefined as T;
    if (opts.expect === "bytes") return (await res.bytes()) as T;
    return (await res.json()) as T;
  }
}

function normaliseTapTarget(
  target: TapTarget | { x: number; y: number } | string,
): TapTarget {
  if (typeof target === "string") return { selector: target };
  if ("selector" in target) return target;
  if ("xy" in target) return target;
  return { xy: { x: target.x, y: target.y } };
}

async function safeReadBody(res: Response): Promise<unknown> {
  const ct = res.headers.get("content-type") ?? "";
  try {
    if (ct.includes("application/json")) return await res.json();
    return await res.text();
  } catch {
    return null;
  }
}
