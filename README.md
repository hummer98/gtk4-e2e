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

## License

MIT — see [LICENSE](LICENSE).
