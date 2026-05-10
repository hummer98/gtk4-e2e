---
description: "Tap a widget in the running gtk4-e2e demo (or any consumer with the e2e feature on)."
argument-hint: "<selector|x,y>"
allowed-tools: Bash
---

# /gtk4-e2e:e2e-tap

Tap a widget by selector or absolute window coordinates against the running
gtk4-e2e instance.

## Usage

- `/gtk4-e2e:e2e-tap "#submit"` — tap the widget matching the selector
- `/gtk4-e2e:e2e-tap 100,200` — tap at window-local (x,y)

## Action

The argument from the user is `$ARGUMENTS`. Run the following Bash command,
substituting the argument verbatim (quote it if it contains spaces):

```bash
bunx gtk4-e2e tap $ARGUMENTS
```

If `$ARGUMENTS` is empty, ask the user for a selector or coordinates first.

## Prerequisites

- A discoverable instance under `${XDG_RUNTIME_DIR}/gtk4-e2e/instance-*.json`
  (Linux) or `${TMPDIR}/gtk4-e2e/instance-*.json` (macOS). Use
  `bunx gtk4-e2e info` first if unsure.
- `--token` / env `GTK4_E2E_TOKEN` if the instance was launched with a token.

## Exit codes

| code | meaning |
|------|---------|
| 0    | tap accepted (HTTP 204) |
| 3    | server returned 501 (capability missing) |
| 4    | no instance reachable via discover() |
| 5    | HTTP error from the server (e.g. 404 selector_not_found) |
