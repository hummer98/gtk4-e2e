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
