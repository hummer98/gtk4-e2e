export { E2EClient } from "./client.ts";
export type { DiscoverFilter, InstanceFile } from "./discover.ts";
export { discover, runtimeDir } from "./discover.ts";
export {
  DiscoveryError,
  E2EError,
  EventStreamError,
  HttpError,
  NotImplementedError,
  RecorderError,
  VisualDiffError,
  WaitTimeoutError,
} from "./errors.ts";
export type { EventStream, EventsOptions } from "./events.ts";
export { openEventStream } from "./events.ts";
export type {
  DisplayKind,
  RecorderOptions,
  RecorderStatus,
} from "./recorder.ts";
export { buildFfmpegArgs, detectDisplayKind, Recorder } from "./recorder.ts";
export type {
  Capability,
  EventEnvelope,
  EventKind,
  Info,
  PinchRequest,
  SwipeRequest,
  TapTarget,
  TypeRequest,
  WaitCondition,
  WaitRequest,
  WaitResult,
} from "./types.gen.ts";
export type { ExpectScreenshotOptions, VisualDiffResult } from "./visualDiff.ts";
export { expectScreenshot } from "./visualDiff.ts";
