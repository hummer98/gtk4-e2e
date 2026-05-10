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

import { type DiscoverFilter, discover, type InstanceFile } from "./discover.ts";
import {
  DiscoveryError,
  E2EError,
  HttpError,
  NotImplementedError,
  WaitTimeoutError,
} from "./errors.ts";
import { type EventStream, type EventsOptions, openEventStream } from "./events.ts";
import type {
  ElementsResponse,
  Info,
  TapTarget,
  TypeRequest,
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
  /** Optional query string params; appended as `?k=v&k=v`. */
  query?: Record<string, string>;
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
      throw new DiscoveryError("no gtk4-e2e instance matched the given filter");
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
   * Replace the text content of an `Entry` / `Editable` / `TextView` widget.
   *
   * MVP semantics (Step 9 plan §2.2): full replacement, not "insert at
   * cursor". Empty `text` is allowed and clears the widget.
   */
  async type(selector: string, text: string): Promise<void> {
    const body: TypeRequest = { selector, text };
    await this._request<void>({
      method: "POST",
      path: "/test/type",
      body,
      capability: "type",
      expect: "void",
    });
  }

  /**
   * Synthesize a swipe gesture from `from` to `to` over `durationMs`. Both
   * endpoints are window-local pixel coords (top-left origin). The request
   * resolves once the animation completes server-side.
   *
   * Errors:
   *   - 404 no_scrollable_at_point  — `from` is not inside a ScrolledWindow
   *   - 422 out_of_bounds / invalid_duration / no_active_window
   *   - 501 NotImplementedError if the capability is missing on the server
   */
  async swipe(
    from: { x: number; y: number },
    to: { x: number; y: number },
    durationMs: number,
  ): Promise<void> {
    await this._request<void>({
      method: "POST",
      path: "/test/swipe",
      body: { from, to, duration_ms: durationMs },
      capability: "swipe",
      expect: "void",
    });
  }

  /**
   * Long-poll until `condition` matches or the server deadline elapses.
   * On 408, throws `WaitTimeoutError`.
   */
  async wait(condition: WaitCondition, options?: { timeoutMs?: number }): Promise<WaitResult> {
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

  /**
   * Walk the widget tree (Step 14, T018).
   *
   * Without options, returns one root per active window with the full
   * subtree. With `selector` (`#name` or `.class`), returns one root per
   * matching widget — `roots: []` is a clean miss (HTTP 200, not 404).
   * `maxDepth` caps the depth of each returned subtree (`0` = root only).
   */
  async elements(opts?: { selector?: string; maxDepth?: number }): Promise<ElementsResponse> {
    const query: Record<string, string> = {};
    if (opts?.selector !== undefined) query.selector = opts.selector;
    if (opts?.maxDepth !== undefined) query.max_depth = String(opts.maxDepth);
    return this._request<ElementsResponse>({
      method: "GET",
      path: "/test/elements",
      query,
      capability: "elements",
      expect: "json",
    });
  }

  /**
   * Fetch the current app-defined state snapshot exposed at `GET /test/state`.
   *
   * Returns whatever JSON the demo / consumer last pushed via
   * `Handle::set_state`. Defaults to `null` before the first push, so callers
   * should narrow with the discriminator they expect rather than assume the
   * shape.
   */
  async state(): Promise<unknown> {
    return this._request<unknown>({
      method: "GET",
      path: "/test/state",
      capability: "state",
      expect: "json",
    });
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

    let url = `${this.baseUrl}${opts.path}`;
    if (opts.query) {
      const entries = Object.entries(opts.query);
      if (entries.length > 0) {
        const qs = new URLSearchParams();
        for (const [k, v] of entries) qs.set(k, v);
        const sep = opts.path.includes("?") ? "&" : "?";
        url = `${this.baseUrl}${opts.path}${sep}${qs.toString()}`;
      }
    }

    let res: Response;
    try {
      res = await fetch(url, init);
    } catch (err) {
      throw new E2EError(`network error contacting ${this.baseUrl}${opts.path}`, { cause: err });
    }

    if (res.status === 408) {
      throw new WaitTimeoutError(opts.waitTimeoutMs ?? DEFAULT_WAIT_TIMEOUT_MS);
    }
    if (res.status === 501) {
      throw new NotImplementedError(opts.capability, res.status);
    }
    if (res.status >= 400) {
      const body = await safeReadBody(res);
      throw new HttpError(res.status, body, `HTTP ${res.status} from ${opts.method} ${opts.path}`);
    }

    if (opts.expect === "void") return undefined as T;
    if (opts.expect === "bytes") return (await res.bytes()) as T;
    return (await res.json()) as T;
  }
}

function normaliseTapTarget(target: TapTarget | { x: number; y: number } | string): TapTarget {
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
