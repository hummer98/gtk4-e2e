---
description: "Run a bun test scenario file against the running gtk4-e2e demo (or any consumer)."
argument-hint: "<path/to/spec.ts>"
allowed-tools: Bash
---

# /gtk4-e2e:e2e-scenario

Run a single scenario spec via `bun test`. Scenarios import the SDK
(`E2EClient`, `discover`, `Recorder`) and exercise the running instance.

## Usage

- `/gtk4-e2e:e2e-scenario packages/demo/scenarios/tap.spec.ts`

## Action

The argument from the user is `$ARGUMENTS` (a path to a spec file relative to
the repository root, or absolute). Run:

```bash
bun test $ARGUMENTS
```

If `$ARGUMENTS` is empty, ask the user which scenario file to run, or list
candidates under `packages/demo/scenarios/` first with `ls`.

## Prerequisites

- A running gtk4-e2e instance (e.g. `cargo run -p gtk4-e2e-demo --features e2e`)
  reachable via the local registry.
- Bun installed (`curl -fsSL https://bun.sh/install | bash`).
