# gtk4-e2e

E2E test framework for **GTK4 + Rust** applications, providing Playwright-equivalent capabilities for Native GUI apps where browser-based test tools are unsuitable (GPU-accelerated camera pipelines, AI inference, kiosk-mode rendering, etc.).

## Architecture (high level)

Three packages, two languages:

| Package | Language | Role |
|---|---|---|
| `packages/server` | Rust crate | HTTP / WebSocket server, embedded **in-process** in the GTK4 app via Cargo feature flag (debug builds only) |
| `packages/client` | Bun / TypeScript | SDK + CLI + recorder + Claude Code plugin, runs as an external test client |
| `packages/demo` | Rust binary | Minimal GTK4 app embedding the server, used for development and CI regression testing without depending on any specific consumer |

Rust → JSON Schema → TypeScript codegen pipeline keeps protocol types in sync (Rust is the SSOT, TS types are auto-generated, `.gen.ts` files are gitignored).

## Why a separate repo

Originally proposed inside the Brainship project (private), this framework was extracted as an independent OSS effort because:

- Largely independent from any single consumer (designed against the demo, integrated into consumers later)
- Reusable for any GTK4+Rust application
- Independent CI / Issue board / release cadence

## Status

**Bootstrap phase**. See [`docs/seed.md`](docs/seed.md) for the initial Claude Code instructions used to scaffold the project, and [`docs/adr/`](docs/adr/) for architectural decisions.

## Quick start

Launch the demo GTK4 app with the in-process e2e server enabled:

```bash
cargo run -p gtk4-e2e-demo --features e2e
```

The `e2e` feature is intended for **debug / CI builds only**. Production consumer builds should leave it off (default), so the server crate is fully excluded from the dependency graph.

### System dependencies

`gtk4-rs` links against system `libgtk-4` via `pkg-config`.

- macOS: `brew install gtk4`
- Ubuntu/Debian: `sudo apt install libgtk-4-dev pkg-config`

### Verify the server (separate terminal)

```bash
# Pick the registry directory for the current platform
#   macOS (XDG_RUNTIME_DIR unset): "$TMPDIR/gtk4-e2e/" (e.g. /var/folders/.../T/gtk4-e2e/)
#   Linux:                         "$XDG_RUNTIME_DIR/gtk4-e2e/"
REG_DIR="${XDG_RUNTIME_DIR:-${TMPDIR%/}}/gtk4-e2e"

# 1. Confirm the registry file is present while the demo runs
ls "$REG_DIR" | grep '^instance-[0-9]\+\.json$'

# 2. Pull the bound port out of the registry file and hit /test/info
PORT=$(jq -r .port "$REG_DIR"/instance-*.json | head -1)
curl -sf "http://127.0.0.1:$PORT/test/info" | jq .

# 3. After closing the window, the registry file should be gone
ls "$REG_DIR" 2>/dev/null | grep '^instance-' || echo "OK: cleaned up"
```

The demo also prints the URL to stderr at startup:

```
[gtk4-e2e-demo] server up on http://127.0.0.1:<port>/test/info
```

## Recording (MVP: X11)

Local screen recording is driven by `ffmpeg` and tracked via a single PID
file under the runtime directory (one host = one recording in MVP). It is
client-side only — no protocol endpoint is involved.

```bash
# Requires `ffmpeg` on PATH and an X11 server (`$DISPLAY` set).
bunx gtk4-e2e record start --output run.mp4
# ... drive the app via tap / scenarios ...
bunx gtk4-e2e record stop
bunx gtk4-e2e record status   # JSON: { running, output, pid, startedAt, elapsedMs }
```

Wayland sessions, macOS, and headless environments are out of MVP scope and
exit with code 6 (`RecorderError`). Install ffmpeg via `apt install ffmpeg` /
`brew install ffmpeg` / `dnf install ffmpeg`.

For a full-run recording that drives all demo scenarios end-to-end, see
the [Recorded demo run](#recorded-demo-run) section below.

The `Recorder` class is also exported from the SDK:

```ts
import { Recorder } from "gtk4-e2e";
const r = new Recorder({ output: "run.mp4" });
await r.start();
// ...
await r.stop();
```

## Recorded demo run

The CI job `record-demo` (`.github/workflows/ci.yml`) drives all scenarios
under Xvfb while ffmpeg captures the X display, and uploads the resulting
mp4 as a workflow artifact named `demo-run-mp4` (retention 30 days).

To download the latest recording:

1. Open the most recent CI run on `main` from the Actions tab
2. Scroll to **Artifacts** → `demo-run-mp4` → download → unzip → `demo-run.mp4`

To regenerate locally (Linux X11 + `ffmpeg` on PATH required):

```bash
bash packages/demo/scripts/record-run.sh
# → artifacts/demo-run.mp4
```

macOS and Wayland sessions are not supported by the recorder (T009 MVP)
and will exit with code 6 (`RecorderError`).

## Claude Code integration

A Claude Code plugin lives under `packages/client/claude-plugin/` with
three slash commands and an auto-triggering SKILL:

| Slash command | Purpose |
|---|---|
| `/gtk4-e2e:e2e-tap`      | Tap a widget by selector or coordinates |
| `/gtk4-e2e:e2e-record`   | Manage screen recording (start / stop / status) |
| `/gtk4-e2e:e2e-scenario` | Run a `bun test` scenario file |

The SKILL (`skills/gtk4-e2e/SKILL.md`) auto-triggers on phrases like
"gtk4-e2e", "demo を tap", "画面を録画", "scenario を流す" and routes them
to the underlying `bunx gtk4-e2e ...` calls.

Local install (linked from this checkout — recommended for development):

```bash
# from the repo root
mkdir -p ~/.claude/plugins/local
ln -snf "$(pwd)/packages/client/claude-plugin" ~/.claude/plugins/local/gtk4-e2e
```

Then enable it via `/plugin` inside Claude Code. The plugin manifest
follows the current `.claude-plugin/plugin.json` convention; the older
`manifest.json` filename is no longer used.

## License

MIT — see [LICENSE](LICENSE).
