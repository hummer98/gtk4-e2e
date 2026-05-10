export { E2EClient } from "./client.ts";
export { discover, runtimeDir } from "./discover.ts";
export type { InstanceFile, DiscoverFilter } from "./discover.ts";
export {
  DiscoveryError,
  E2EError,
  EventStreamError,
  HttpError,
  NotImplementedError,
  WaitTimeoutError,
} from "./errors.ts";
export { openEventStream } from "./events.ts";
export type { EventStream, EventsOptions } from "./events.ts";
export type {
  Capability,
  EventEnvelope,
  EventKind,
  Info,
  TapTarget,
  WaitCondition,
  WaitRequest,
  WaitResult,
} from "./types.gen.ts";
