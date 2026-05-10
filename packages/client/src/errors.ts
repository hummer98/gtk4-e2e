// SDK 由来の例外階層。`instanceof E2EError` で全捕捉できるよう base を共有する。
// 設計判断は plan §Q6 を参照。

export class E2EError extends Error {
  constructor(message: string, options?: { cause?: unknown }) {
    super(message, options as ErrorOptions);
    this.name = "E2EError";
  }
}

export class DiscoveryError extends E2EError {
  constructor(message: string, options?: { cause?: unknown }) {
    super(message, options);
    this.name = "DiscoveryError";
  }
}

export class HttpError extends E2EError {
  readonly status: number;
  readonly body: unknown;

  constructor(status: number, body: unknown, message: string) {
    super(message);
    this.name = "HttpError";
    this.status = status;
    this.body = body;
  }
}

export class NotImplementedError extends E2EError {
  readonly capability: string;
  readonly status: number;

  constructor(capability: string, status: number) {
    super(`capability "${capability}" is not implemented (HTTP ${status})`);
    this.name = "NotImplementedError";
    this.capability = capability;
    this.status = status;
  }
}

// Long-polling deadline reached server-side (HTTP 408). Distinct from
// HttpError so callers can `expect(...).rejects.toThrow(WaitTimeoutError)`
// without conflating it with arbitrary 4xx failures. Plan §Q12.
export class WaitTimeoutError extends E2EError {
  readonly timeoutMs: number;

  constructor(timeoutMs: number) {
    super(`wait timed out after ${timeoutMs}ms`);
    this.name = "WaitTimeoutError";
    this.timeoutMs = timeoutMs;
  }
}

// Local recorder failures (Step 8). Distinct from server-origin
// `NotImplementedError` (HTTP 501) so the CLI can map them to a separate
// exit code (6). `kind` lets callers distinguish e.g. ffmpeg-not-found vs
// already-running vs Wayland-not-supported without string matching.
export class RecorderError extends E2EError {
  readonly kind?: string;

  constructor(message: string, options?: { cause?: unknown; kind?: string }) {
    super(message, options);
    this.name = "RecorderError";
    if (options?.kind) this.kind = options.kind;
  }
}

// Visual regression failures (Step 17 / T020-A). `kind` lets callers
// distinguish "baseline_missing" (a precondition error — caller may want to
// regenerate baselines) from "decode_failed" (the supplied bytes weren't a
// readable PNG) without string-matching the message. Threshold violations
// surface as `VisualDiffResult.match === false`, not as throws.
export class VisualDiffError extends E2EError {
  readonly kind: "baseline_missing" | "decode_failed";

  constructor(
    message: string,
    kind: "baseline_missing" | "decode_failed",
    options?: { cause?: unknown },
  ) {
    super(message, options);
    this.name = "VisualDiffError";
    this.kind = kind;
  }
}

// `WS /test/events` failures. Step 7 plan §5.4 / §10.2:
//   * thrown from `await client.events(...)` when the initial open fails
//     (host unreachable, capability missing, handshake rejected)
//   * thrown from the next `stream.next()` after `maxRetries` reconnect
//     attempts are exhausted. The default cap is 10; pass
//     `reconnect: { maxRetries: Infinity }` to opt out (caller owns
//     termination via AbortSignal).
export class EventStreamError extends E2EError {
  constructor(message: string, options?: { cause?: unknown }) {
    super(message, options);
    this.name = "EventStreamError";
  }
}
