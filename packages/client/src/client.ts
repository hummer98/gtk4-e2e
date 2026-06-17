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

import { fileURLToPath } from "node:url";

import { resolveBaselineDir, resolveScenarioBasename } from "./baselineResolver.ts";
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
  FocusRequest,
  Info,
  PinchRequest,
  PressRequest,
  TapTarget,
  TypeRequest,
  WaitCondition,
  WaitResult,
} from "./types.gen.ts";
import {
  type ExpectScreenshotOptions,
  expectScreenshot,
  type VisualDiffResult,
} from "./visualDiff.ts";

// Stack-based caller 推定で resolver / wrapper 自身のフレームを skip するための
// 絶対パス (Step 18 / T020-B)。`import.meta.url` で実行時に取り直すので、
// 将来 src/ 内のレイアウトが変わっても破綻しない。
const CLIENT_FILE = fileURLToPath(import.meta.url);
const RESOLVER_FILE = CLIENT_FILE.replace(/client\.ts$/, "baselineResolver.ts");
const SKIP_FILES: readonly string[] = [CLIENT_FILE, RESOLVER_FILE];

interface ClientOptions {
  baseUrl: string;
  token?: string;
}

/**
 * Picks what `screenshot()` / `expectScreenshot()` capture (issue #7).
 * Both fields omitted → the active window (historical default).
 */
export interface ScreenshotTarget {
  /**
   * Capture the first widget matching this selector (`#name` / `.class`),
   * resolved across **all** toplevel windows — including non-active windows
   * and open popovers (separate surfaces the active-window path can't reach).
   */
  selector?: string;
  /**
   * Capture a specific toplevel by index into the app's window list
   * (creation order). Ignored when `selector` is set.
   */
  window?: number;
}

export interface ClientExpectScreenshotOptions extends ExpectScreenshotOptions, ScreenshotTarget {
  /**
   * Absolute path to the calling test file. When provided, the wrapper
   * skips stack inspection and resolves baselines under
   * `<dirname(testFile)>/__screenshots__/`. Useful for harnesses where
   * stack walking is fragile (bundlers, dynamic imports).
   */
  testFile?: string;
  /**
   * Inject the env subset that controls baseline resolution. Defaults to
   * `process.env`. Tests should pass an explicit object to avoid leaking
   * `process.env.CI` / `process.env.GTK4_E2E_BASELINE_DIR` across the
   * bun:test parallel runner.
   */
  env?: { CI?: string; GTK4_E2E_BASELINE_DIR?: string };
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
   * Move keyboard focus to the widget matched by `selector` via `grab_focus()`,
   * so `:focus` / `:focus-within` dependent CSS (focus ring, accent border)
   * renders for deterministic screenshot verification (issue #3).
   *
   * Errors:
   *   - 404 selector_not_found    — no widget matches `selector`
   *   - 422 focus_rejected        — the widget cannot take focus (e.g. a Label)
   *   - 422 widget_not_visible / widget_disabled / invalid_selector / no_active_window
   *   - 501 NotImplementedError if the capability is missing on the server
   */
  async focus(selector: string): Promise<void> {
    const body: FocusRequest = { selector };
    await this._request<void>({
      method: "POST",
      path: "/test/focus",
      body,
      capability: "focus",
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
   * Synthesize a pinch (zoom) gesture centred at `center` over `durationMs`,
   * targeting any `gtk::GestureZoom` in the widget tree at that point.
   *
   * `scale > 1.0` zooms in, `scale < 1.0` zooms out, `scale = 1.0` is a no-op.
   *
   * Errors:
   *   - 404 no_pinchable_at_point  — no `GestureZoom` ancestor at `center`
   *   - 422 invalid_scale / invalid_duration / out_of_bounds / no_active_window
   *   - 501 NotImplementedError if the capability is missing on the server
   */
  async pinch(center: { x: number; y: number }, scale: number, durationMs: number): Promise<void> {
    const body: PinchRequest = { center, scale, duration_ms: durationMs };
    await this._request<void>({
      method: "POST",
      path: "/test/pinch",
      body,
      capability: "pinch",
      expect: "void",
    });
  }

  /**
   * Inject a press → hold → release sequence to fire a `GestureLongPress`
   * (`pressed` signal) on the widget matched by `selector`, or at window-local
   * `xy`. Exactly one of `selector` / `xy` must be provided. `hold_ms` is the
   * press-to-recognition delay (1..=10000ms); the request resolves once the
   * long-press has fired server-side.
   *
   * Errors:
   *   - 404 selector_not_found / no_long_pressable_at_point /
   *         no_long_pressable_for_selector
   *   - 422 invalid_target / invalid_hold / invalid_selector /
   *         out_of_bounds / no_active_window
   *   - 501 NotImplementedError if the capability is missing on the server
   */
  async press(opts: {
    selector?: string;
    xy?: { x: number; y: number };
    hold_ms: number;
  }): Promise<void> {
    const body: PressRequest = {
      selector: opts.selector,
      xy: opts.xy,
      hold_ms: opts.hold_ms,
    };
    await this._request<void>({
      method: "POST",
      path: "/test/press",
      body,
      capability: "press",
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
   *
   * Pass `props` to opt-in per-widget GObject property reads — each name
   * is looked up against the matched widget and surfaced under
   * `node.properties[name]`. Sentinels distinguish failure modes:
   *
   * - `{ "$missing": true }` — widget exposes no such property.
   * - `{ "$unsupported": "GTypeName" }` — type outside MVP support
   *   (server currently maps String / bool / i32 / f64; other types
   *   round-trip as this sentinel).
   *
   * The literal token `"*"` in `props` is the wildcard: the server
   * expands it to every readable GObject property advertised by the
   * matched widget's class (`list_properties()`). Mixing `"*"` with
   * specific names is allowed; explicitly-named values always win
   * over the wildcard expansion.
   *
   * Without `props`, `node.properties` is `undefined` (legacy shape).
   */
  async elements(opts?: {
    selector?: string;
    maxDepth?: number;
    props?: string[];
  }): Promise<ElementsResponse> {
    const query: Record<string, string> = {};
    if (opts?.selector !== undefined) query.selector = opts.selector;
    if (opts?.maxDepth !== undefined) query.max_depth = String(opts.maxDepth);
    if (opts?.props !== undefined && opts.props.length > 0) {
      query.props = opts.props.join(",");
    }
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

  async screenshot(opts?: ScreenshotTarget): Promise<Uint8Array>;
  async screenshot(path: string, opts?: ScreenshotTarget): Promise<string>;
  async screenshot(
    pathOrOpts?: string | ScreenshotTarget,
    maybeOpts?: ScreenshotTarget,
  ): Promise<Uint8Array | string> {
    // Overload resolution: a leading string is the output path (write mode);
    // otherwise the first arg is the target options (return-bytes mode).
    const path = typeof pathOrOpts === "string" ? pathOrOpts : undefined;
    const target = typeof pathOrOpts === "string" ? maybeOpts : pathOrOpts;

    // issue #7: `selector` / `window` pick a non-active window or popover
    // surface. Omitted → server captures the active window (historical default).
    const query: Record<string, string> = {};
    if (target?.selector !== undefined) query.selector = target.selector;
    if (target?.window !== undefined) query.window = String(target.window);

    const bytes = await this._request<Uint8Array>({
      method: "GET",
      path: "/test/screenshot",
      capability: "screenshot",
      expect: "bytes",
      query,
    });
    if (path === undefined) return bytes;
    await Bun.write(path, bytes);
    return path;
  }

  /**
   * Capture a screenshot and compare against a baseline PNG. Convenience
   * wrapper around `screenshot()` + `expectScreenshot()` (plan §Q2 / §Q5,
   * Step 18 / T020-B baseline-resolution rules).
   *
   * Resolution order for `baselineDir` (high → low):
   *   1. `opts.baselineDir` (absolute, or relative to cwd)
   *   2. `<dirname(opts.testFile)>/__screenshots__/`
   *   3. `env.GTK4_E2E_BASELINE_DIR`
   *   4. `<dirname(callerFile)>/__screenshots__/` from `Error().stack`
   *   5. `<process.cwd()>/__screenshots__/` fallback
   *
   * `failOnMissing` defaults to `env.CI === "true"` unless caller overrides.
   * The pure function `expectScreenshot()` stays env-agnostic.
   *
   * When `opts.baselineDir` is explicit, the `<scenario_basename>-` prefix
   * is suppressed and the file is read/written as `<baselineDir>/<name>.png`.
   * Rationale: a caller that pins `baselineDir` (CLI `--baseline <path>`,
   * tools that compose paths externally) is asserting full control over
   * the filename; prefix derivation would silently rewrite their path.
   */
  async expectScreenshot(
    name: string,
    opts: ClientExpectScreenshotOptions = {},
  ): Promise<VisualDiffResult> {
    // env / process.env 参照は wrapper エントリポイント 1 箇所に閉じる。
    // 以降の resolver / 内部判定はここで inject された値だけを参照する
    // (bun:test の並行実行で leak しないようにするための原則; design-review
    // Recommendation 2)。
    const env = opts.env ?? process.env;
    const callerStack = new Error().stack;

    const baselineDir = resolveBaselineDir({
      optsBaselineDir: opts.baselineDir,
      optsTestFile: opts.testFile,
      env: { GTK4_E2E_BASELINE_DIR: env.GTK4_E2E_BASELINE_DIR },
      callerStack,
      skipFiles: [...SKIP_FILES],
    });

    // 「明示 > 暗黙」(plan §1.1): opts.baselineDir を渡した呼び出し側は
    // ファイル名も自前管理しているとみなし、scenario_basename prefix は付け
    // ない (CLI `--baseline <path>` のようにファイル名まで完全に決め打って
    // いるケースに対応; rev2 fix)。
    const scenarioBasename =
      opts.baselineDir !== undefined
        ? null
        : resolveScenarioBasename({
            optsTestFile: opts.testFile,
            callerStack,
            skipFiles: [...SKIP_FILES],
          });
    const fullName = scenarioBasename === null ? name : `${scenarioBasename}-${name}`;

    const failOnMissing = opts.failOnMissing ?? env.CI === "true";

    // issue #7: forward selector/window so visual diffs can target a
    // non-active window or popover surface, not just the active window.
    const actual = await this.screenshot({ selector: opts.selector, window: opts.window });
    return expectScreenshot(actual, fullName, {
      threshold: opts.threshold,
      includeAA: opts.includeAA,
      updateBaseline: opts.updateBaseline,
      failOnMissing,
      baselineDir,
    });
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
