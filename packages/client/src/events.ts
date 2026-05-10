// `WS /test/events` consumer (plan §5).
//
// `openEventStream` returns a Promise that resolves once the WebSocket has
// successfully opened, so callers can `await client.events(...)` and be sure
// the server-side subscriber is live before they trigger the events they want
// to observe (e.g. a `tap` that produces a `state_change`). The returned
// object satisfies both `AsyncIterable<EventEnvelope>` and
// `AsyncIterator<EventEnvelope>` so it works with `for await` and explicit
// `stream.next()` alike.
//
// Reconnect: on disconnect after the initial open, the stream sleeps using
// full-jitter exponential backoff (default base 100 ms / cap 5 s / factor 2)
// and reconnects up to `maxRetries` (default 10) times before throwing
// `EventStreamError` from the next `next()`. Pass `maxRetries: Infinity`
// to opt out — the caller then owns termination via AbortSignal.

import { EventStreamError } from "./errors.ts";
import type { EventEnvelope, EventKind } from "./types.gen.ts";

interface BaseUrlSource {
  readonly baseUrl: string;
}

interface ReconnectConfig {
  baseMs?: number;
  maxMs?: number;
  factor?: number;
  maxRetries?: number;
}

interface ResolvedReconnectConfig {
  baseMs: number;
  maxMs: number;
  factor: number;
  maxRetries: number;
}

export interface EventsOptions {
  kinds?: EventKind[];
  signal?: AbortSignal;
  reconnect?: ReconnectConfig;
  /** Override URL builder for tests; defaults to `client.baseUrl` → `ws://`. */
  urlBuilder?: () => URL;
}

export type EventStream = AsyncIterable<EventEnvelope> &
  AsyncIterator<EventEnvelope>;

const DEFAULT_RECONNECT: ResolvedReconnectConfig = {
  baseMs: 100,
  maxMs: 5_000,
  factor: 2,
  maxRetries: 10,
};

/**
 * Open a subscriber to `WS /test/events` and return an async iterable.
 *
 * Resolves only after the initial WebSocket handshake completes. The first
 * yield is therefore guaranteed to capture any events emitted *after* the
 * caller awaits this function.
 */
export async function openEventStream(
  client: BaseUrlSource,
  opts: EventsOptions = {},
): Promise<EventStream> {
  const url = opts.urlBuilder ? opts.urlBuilder() : defaultUrl(client, opts.kinds);
  const cfg = resolveReconnect(opts.reconnect);
  const stream = new EventStreamImpl(url, opts.signal, cfg);
  await stream.openInitial();
  return stream;
}

function defaultUrl(client: BaseUrlSource, kinds: EventKind[] | undefined): URL {
  const url = new URL(client.baseUrl);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = "/test/events";
  if (kinds && kinds.length > 0) {
    url.searchParams.set("kinds", kinds.join(","));
  }
  return url;
}

function resolveReconnect(input: ReconnectConfig | undefined): ResolvedReconnectConfig {
  return {
    baseMs: input?.baseMs ?? DEFAULT_RECONNECT.baseMs,
    maxMs: input?.maxMs ?? DEFAULT_RECONNECT.maxMs,
    factor: input?.factor ?? DEFAULT_RECONNECT.factor,
    maxRetries: input?.maxRetries ?? DEFAULT_RECONNECT.maxRetries,
  };
}

interface PendingPull {
  resolve: (r: IteratorResult<EventEnvelope>) => void;
  reject: (e: unknown) => void;
}

class EventStreamImpl implements AsyncIterable<EventEnvelope>, AsyncIterator<EventEnvelope> {
  private readonly queue: EventEnvelope[] = [];
  private readonly pulls: PendingPull[] = [];
  private ws: WebSocket | null = null;
  private done = false;
  private terminalError: Error | null = null;
  private retries = 0;
  private reconnecting = false;

  constructor(
    private readonly url: URL,
    private readonly signal: AbortSignal | undefined,
    private readonly cfg: ResolvedReconnectConfig,
  ) {}

  async openInitial(): Promise<void> {
    if (this.signal?.aborted) {
      this.endWithDone();
      return;
    }
    const ws = await connectWebSocket(this.url);
    this.attachSocket(ws);
    if (this.signal) {
      if (this.signal.aborted) {
        this.endWithDone();
        return;
      }
      this.signal.addEventListener("abort", this.handleAbort, { once: true });
    }
  }

  [Symbol.asyncIterator](): AsyncIterator<EventEnvelope> {
    return this;
  }

  next(): Promise<IteratorResult<EventEnvelope>> {
    if (this.terminalError) {
      const err = this.terminalError;
      this.terminalError = null;
      return Promise.reject(err);
    }
    if (this.queue.length > 0) {
      const env = this.queue.shift() as EventEnvelope;
      return Promise.resolve({ value: env, done: false });
    }
    if (this.done) {
      return Promise.resolve({ value: undefined, done: true });
    }
    return new Promise<IteratorResult<EventEnvelope>>((resolve, reject) => {
      this.pulls.push({ resolve, reject });
    });
  }

  async return(): Promise<IteratorResult<EventEnvelope>> {
    this.endWithDone();
    return { value: undefined, done: true };
  }

  private readonly handleAbort = () => {
    this.endWithDone();
  };

  private attachSocket(ws: WebSocket): void {
    this.ws = ws;
    ws.onmessage = (ev) => {
      if (this.done) return;
      let env: EventEnvelope;
      try {
        env = JSON.parse(typeof ev.data === "string" ? ev.data : "") as EventEnvelope;
      } catch {
        return;
      }
      this.push(env);
    };
    const onLost = () => {
      if (ws !== this.ws) return; // already replaced
      this.ws = null;
      this.scheduleReconnect();
    };
    ws.onclose = onLost;
    ws.onerror = onLost;
  }

  private push(env: EventEnvelope): void {
    if (this.done) return;
    const pull = this.pulls.shift();
    if (pull) {
      pull.resolve({ value: env, done: false });
    } else {
      this.queue.push(env);
    }
  }

  private scheduleReconnect(): void {
    if (this.done || this.reconnecting) return;
    if (this.signal?.aborted) {
      this.endWithDone();
      return;
    }
    this.reconnecting = true;
    void this.runReconnect();
  }

  private async runReconnect(): Promise<void> {
    while (!this.done && !this.signal?.aborted) {
      if (this.retries >= this.cfg.maxRetries) {
        this.endWithError(
          new EventStreamError(
            `event stream lost connection after ${this.retries} retries`,
          ),
        );
        return;
      }
      const delay = this.computeBackoff(this.retries);
      this.retries += 1;
      try {
        await sleep(delay, this.signal);
      } catch {
        // aborted
        this.endWithDone();
        return;
      }
      if (this.done || this.signal?.aborted) {
        this.endWithDone();
        return;
      }
      try {
        const ws = await connectWebSocket(this.url);
        this.retries = 0;
        this.attachSocket(ws);
        this.reconnecting = false;
        return;
      } catch {
        // try again with the next backoff slot
      }
    }
    this.reconnecting = false;
    if (this.signal?.aborted) {
      this.endWithDone();
    }
  }

  private computeBackoff(attempt: number): number {
    const exp = Math.min(this.cfg.maxMs, this.cfg.baseMs * Math.pow(this.cfg.factor, attempt));
    return Math.random() * exp;
  }

  private endWithDone(): void {
    if (this.done) return;
    this.done = true;
    this.signal?.removeEventListener("abort", this.handleAbort);
    this.closeSocketSafely();
    while (this.pulls.length > 0) {
      const p = this.pulls.shift() as PendingPull;
      p.resolve({ value: undefined, done: true });
    }
  }

  private endWithError(err: Error): void {
    if (this.done) return;
    this.done = true;
    this.terminalError = err;
    this.signal?.removeEventListener("abort", this.handleAbort);
    this.closeSocketSafely();
    if (this.pulls.length > 0) {
      // Drain the first pending pull with the error; the rest get DONE so the
      // iterator stays well-behaved if multiple consumers are awaiting.
      const first = this.pulls.shift() as PendingPull;
      first.reject(err);
      while (this.pulls.length > 0) {
        const p = this.pulls.shift() as PendingPull;
        p.resolve({ value: undefined, done: true });
      }
      this.terminalError = null;
    }
  }

  private closeSocketSafely(): void {
    const ws = this.ws;
    this.ws = null;
    if (!ws) return;
    try {
      ws.onopen = null;
      ws.onmessage = null;
      ws.onerror = null;
      ws.onclose = null;
      ws.close();
    } catch {
      // best effort
    }
  }
}

function connectWebSocket(url: URL): Promise<WebSocket> {
  return new Promise((resolve, reject) => {
    let settled = false;
    let ws: WebSocket;
    try {
      ws = new WebSocket(url.toString());
    } catch (err) {
      reject(new EventStreamError(`failed to construct WebSocket to ${url}`, { cause: err }));
      return;
    }
    const cleanup = () => {
      ws.onopen = null;
      ws.onerror = null;
      ws.onclose = null;
    };
    ws.onopen = () => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(ws);
    };
    ws.onerror = (ev) => {
      if (settled) return;
      settled = true;
      cleanup();
      try {
        ws.close();
      } catch {
        // already closing
      }
      reject(new EventStreamError(`WebSocket error before open at ${url}`, { cause: ev }));
    };
    ws.onclose = (ev) => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(new EventStreamError(`WebSocket closed before open at ${url}`, { cause: ev }));
    };
  });
}

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new Error("aborted"));
      return;
    }
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    const onAbort = () => {
      clearTimeout(timer);
      reject(new Error("aborted"));
    };
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}
