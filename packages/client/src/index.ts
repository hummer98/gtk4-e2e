export { E2EClient } from "./client.ts";
export { discover, runtimeDir } from "./discover.ts";
export type { InstanceFile, DiscoverFilter } from "./discover.ts";
export {
  DiscoveryError,
  E2EError,
  HttpError,
  NotImplementedError,
  WaitTimeoutError,
} from "./errors.ts";
export type {
  Capability,
  Info,
  TapTarget,
  WaitCondition,
  WaitRequest,
  WaitResult,
} from "./types.gen.ts";
