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

**Round 2 stabilization complete** (Step 0–15 + Step 9 a/b/c shipped). Per-round status reports live under [`docs/reports/`](docs/reports/) — see [`docs/reports/README.md`](docs/reports/README.md) for the index, [`docs/seed.md`](docs/seed.md) for the initial Claude Code scaffolding instructions, and [`docs/adr/`](docs/adr/) for architectural decisions.

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

### Lint / Format / Type-check (TS side)

```bash
bun install                                  # devDeps (biome, typescript)
bun run lint                                 # biome lint  → packages/client 配下
bun run fmt:check                            # biome format (差分があれば fail)
(cd packages/client && bunx tsc --noEmit)    # 型チェック (要 types.gen.ts 生成)
```

整形を当てるにはリポジトリ root で:

```bash
bunx biome format --write .
bunx biome lint --write .
```

`types.gen.ts` は gitignored なので、ローカルで `tsc --noEmit` を初めて走らせるときは
先に `bun packages/client/scripts/gen-types.ts` で生成しておく
(Rust toolchain 不要、committed JSON Schema から生成される)。
CI は同じ手順を `tsc --noEmit` の前段に組み込み済み。

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

## Visual regression (screenshot diff)

Compare a freshly-captured screenshot against a baseline PNG using pixelmatch
(see ADR-0003). Threshold defaults to 0.1 (pixelmatch native).

```bash
# Diff against a baseline. Exits 0 on match, 1 on mismatch.
# The baseline file actually consulted is `<dirname(<path>)>/<name>.png`.
bunx gtk4-e2e screenshot main-window --baseline ./baselines/main-window.png

# Override the per-pixel YIQ threshold (0.0 = strict, 1.0 = lenient; both bounds inclusive).
bunx gtk4-e2e screenshot main-window --baseline ./baselines/x.png --threshold 0.2

# Create or overwrite the baseline with the current screenshot.
bunx gtk4-e2e screenshot main-window --baseline ./baselines/x.png --update-baseline
```

The `<path>` argument supplies **only the directory** that holds baselines.
The actual file consulted is `<dirname(<path>)>/<name>.png`, where `<name>` is
the positional. So `--baseline ./baselines/foo.png main-window` looks for
`./baselines/main-window.png` and ignores the `foo.png` part. If you don't have
a strong opinion on the path, pass `./baselines/<name>.png` (or any sibling
filename in the right directory).

The CLI prints a JSON report (the SDK's `VisualDiffResult` plus the `name` you
passed) to stdout. On mismatch, `<dir>/<name>.actual.png` and (when sizes
match) `<dir>/<name>.diff.png` are written next to the baseline.

Exit codes specific to this subcommand:

- `0` match
- `1` mismatch
- `2` invalid argv (missing `--baseline` value, `--threshold` out of range, `--update-baseline` without `--baseline`, ...)
- `5` HttpError (server failed to capture a screenshot)
- `7` VisualDiffError (baseline missing without `--update-baseline`, or PNG decode failure)

## Recorded demo run

A full-run recording drives all demo scenarios end-to-end and writes the
result to `artifacts/demo-run.mp4` (gitignored). Recording is **local-only**
— there is no CI job that publishes this mp4 as a workflow artifact, so
generate it on demand when you need to share or review behavior.

### Requirements

- Linux X11 session (`$DISPLAY` set; Xvfb is fine — see the example below)
- `ffmpeg` on PATH — `apt install ffmpeg` / `dnf install ffmpeg` / `brew install ffmpeg`
- `xvfb-run` if you want a headless run on Linux (`apt install xvfb`)
- A working build environment for the demo (GTK4 system deps, Rust toolchain, Bun)

### Generate the recording

```bash
# On a graphical Linux X11 host (uses your active display)
bash packages/demo/scripts/record-run.sh

# Or headless via Xvfb (matches the historical CI resolution of 1280x720)
xvfb-run -a --server-args="-screen 0 1280x720x24" \
  bash packages/demo/scripts/record-run.sh

# → artifacts/demo-run.mp4
```

### Platform support

macOS and Wayland sessions are not supported by the recorder (T009 MVP)
and will exit with code 6 (`RecorderError`). Run on a Linux X11 host (or
Xvfb) to produce the mp4.

## Visual regression baseline (`__screenshots__/` 規約)

`E2EClient.expectScreenshot(name, opts?)` で取得した PNG は scenario と同階層の `__screenshots__/<scenario_basename>-<name>.png` に baseline として配置されます (例: `packages/demo/scenarios/__screenshots__/screenshot.spec.ts-main-window.png`)。詳細は [ADR-0003](docs/adr/0003-visual-regression-engine.md) を参照。

- **初回実行**: baseline 不在時は default で **auto-save** され `match: true` を返します (Playwright 同等)。`process.env.CI === "true"` の場合のみ `VisualDiffError("baseline_missing")` を throw します。
- **commit 推奨**: ローカルで auto-save された baseline は **必ず commit** してください。commit せずに push すると CI で baseline 不在 → fail のチェーンが起きます。`__screenshots__/` は **gitignore に入れない** こと。
- **更新**: 意図的に baseline を更新する場合は `expectScreenshot(name, { updateBaseline: true })` (CI / env を問わず最優先で上書き) を使い、PR に baseline diff を含めてレビューします。
- **env override**: `GTK4_E2E_BASELINE_DIR` を export すると baseline ディレクトリ全体を別パスへ切り替えられます (CI で OS/matrix 別に baseline 群を集約したい場合の一時的 override 用)。

`process.env.CI` は `"true"` (文字列一致) のみ判定。Travis 旧設定の `CI=1` は意図的に取りこぼします — CI 側で `CI=true` を export するか `opts.failOnMissing` で明示してください。

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
