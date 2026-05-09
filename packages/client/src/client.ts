// HTTP client surface — see plan §Q2/Q3/Q4/Q6 for the design rationale.
//
// `_request<T>` centralises the response triage:
//   * 501 / 404  → NotImplementedError (server treats the capability as absent)
//   * 4xx / 5xx  → HttpError (other failures)
//   * ok + json  → parsed JSON
//   * ok + bytes → Uint8Array via `res.bytes()`
//
// `tap` and `screenshot` wire formats are provisional (Step 5 will land
// `TapRequest` in proto.rs and re-align here). The shapes below are deliberately
// matched to the SDK tests so the eventual proto.rs change can adjust both at
// once.

import { discover, type DiscoverFilter, type InstanceFile } from "./discover.ts";
import { DiscoveryError, E2EError, HttpError, NotImplementedError } from "./errors.ts";
import type { Info } from "./types.gen.ts";

interface ClientOptions {
  baseUrl: string;
  token?: string;
}

export type TapTarget = string | { x: number; y: number };

type ResponseKind = "json" | "bytes" | "void";

interface RequestOptions {
  method: "GET" | "POST";
  path: string;
  body?: unknown;
  capability: string;
  expect: ResponseKind;
}

export class E2EClient {
  readonly baseUrl: string;
  readonly token?: string;

  constructor(opts: ClientOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/$/, "");
    this.token = opts.token ?? process.env["GTK4_E2E_TOKEN"];
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

  async tap(target: TapTarget): Promise<void> {
    const body =
      typeof target === "string"
        ? { selector: target }
        : { xy: { x: target.x, y: target.y } };
    await this._request<void>({
      method: "POST",
      path: "/test/tap",
      body,
      capability: "tap",
      expect: "void",
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

    let res: Response;
    try {
      res = await fetch(`${this.baseUrl}${opts.path}`, init);
    } catch (err) {
      throw new E2EError(
        `network error contacting ${this.baseUrl}${opts.path}`,
        { cause: err },
      );
    }

    if (res.status === 501 || res.status === 404) {
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

async function safeReadBody(res: Response): Promise<unknown> {
  const ct = res.headers.get("content-type") ?? "";
  try {
    if (ct.includes("application/json")) return await res.json();
    return await res.text();
  } catch {
    return null;
  }
}
