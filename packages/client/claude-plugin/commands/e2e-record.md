---
description: "Start / stop / status of ffmpeg-based screen recording (X11 only in MVP)."
argument-hint: "start <path.mp4> | stop | status"
allowed-tools: Bash
---

# /gtk4-e2e:e2e-record

Manage the local screen recorder. The recorder spawns `ffmpeg` against the
X11 display and tracks lifecycle via a single PID file under the runtime
directory (one host = one recording in MVP).

## Usage

- `/gtk4-e2e:e2e-record start <path.mp4>` — begin recording
- `/gtk4-e2e:e2e-record stop` — gracefully stop, ffmpeg flushes the moov atom
- `/gtk4-e2e:e2e-record status` — print JSON status (running, output, pid, elapsedMs)

## Action

Parse `$ARGUMENTS` to determine the sub-action. The first whitespace-separated
token is the action; remaining tokens are arguments.

| `$ARGUMENTS` | Bash to run |
|---|---|
| `start <path>` | `bunx gtk4-e2e record start --output <path>` |
| `stop` | `bunx gtk4-e2e record stop` |
| `status` | `bunx gtk4-e2e record status` |

If `$ARGUMENTS` is empty or doesn't start with a known action, ask the user
which sub-action to run and what output path to use.

## Prerequisites

- `ffmpeg` on `$PATH` (`apt install ffmpeg` / `brew install ffmpeg` / `dnf install ffmpeg`)
- An X11 server (`$DISPLAY` set). Wayland sessions, macOS, and headless
  environments will fail with exit code 6.
- Parent directory of the output file must exist (the recorder does not
  `mkdir -p`).

## Exit codes

| code | meaning |
|------|---------|
| 0    | success |
| 2    | bad argv (missing `--output`, unknown sub-action) |
| 6    | RecorderError (already running / ffmpeg not found / Wayland / no DISPLAY) |
