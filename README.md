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

## License

MIT — see [LICENSE](LICENSE).
