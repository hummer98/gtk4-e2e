# gtk4-e2e — Project Status Report

> 本レポートは gtk4-e2e フレームワーク (T001-T011 の bootstrap step 0-10) の完成度を 1 ファイルで判断するためのスナップショットである。詳細な意思決定背景は ADR、step ごとの実装ログは `.team/tasks/*/runs/*/summary.md` を参照。

- 対象ブランチ: `task-012-1778385764/task` (worktree: `.worktrees/task-012-1778385764/`)
- main HEAD 基準: `b1ad0b0` (`feat(demo+ci): record-run script + CI mp4 artifact (Step 10)`)
- 作成日: 2026-05-10

---

## A. プロジェクト概要

GTK4 + Rust で書かれた Native GUI アプリケーションに対し、Playwright 同等の e2e テスト能力を提供することがゴール。具体能力は (1) 画面要素取得 (widget tree + screenshot)、(2) 操作エミュレーション (tap / swipe / pinch / type)、(3) wait 系 (long-polling での selector visibility / state 変化 / log マッチ)、(4) インスタンス分離 (port 自動払い出し + registry file での discovery)、(5) 動画録画 (ffmpeg で X11/Wayland capture)、(6) Claude Code 統合 (slash command + SKILL + SDK)。

ブラウザ自動化ツール (Playwright / WebDriver / Cypress) は対象外。GPU 加速カメラ + AI 推論 + キオスク描画の Native アプリでは適用できないため、独自に書く。元は Brainship プロジェクトで必要とされ、検討の中で独立 repo に切り出された経緯がある (Brainship 内部仕様には依存しない、consumer-agnostic に保つ)。

### アーキテクチャ要約

3 パッケージ、2 言語の構成を採る。

| Package | 言語 | 役割 |
|---|---|---|
| `packages/server` | Rust crate | consumer GTK4 app に **in-process** 組込み (debug build only)。axum + tokio で `127.0.0.1:1900x` を bind |
| `packages/client` | Bun / TypeScript | SDK + CLI + recorder + Claude Code plugin。外部プロセス |
| `packages/demo` | Rust binary | server を組込んだ最小 GTK4 app。framework 単独で develop / CI 回す |

```
┌─────────────────────────────────────────────┐
│  Consumer GTK4 App (Rust, debug build)      │
│  ┌──────────┐    ┌─────────────────────┐    │
│  │ GTK4 main│ ←→ │ gtk4-e2e-server     │    │
│  │ loop     │    │ (axum on tokio,     │    │
│  │ (UI/...) │    │  127.0.0.1:1900x)   │    │
│  └──────────┘    └──────────┬──────────┘    │
└─────────────────────────────┼───────────────┘
                              │ HTTP / WS
                              ▼
                  ┌──────────────────┐
                  │ gtk4-e2e (Bun/TS)│  ← 別プロセス
                  │ SDK / CLI / Plugin│
                  └──────────────────┘
```

**Codegen (ADR-0002)**: `proto.rs` (Rust SSOT) → schemars 経由で `packages/server/proto/schemas/*.schema.json` を生成 (commit 対象) → bun の `json-schema-to-typescript` で `packages/client/src/types.gen.ts` を生成 (`.gitignore` 対象、build artifact)。CI は schema diff で stale 検出。

**詳細**: [`./adr/0001-architecture.md`](./adr/0001-architecture.md)、[`./adr/0002-codegen-pipeline.md`](./adr/0002-codegen-pipeline.md)、[`./seed.md`](./seed.md)、[`./adr/README.md`](./adr/README.md)。Quick start / Recording の運用は [`../README.md`](../README.md) を参照。

---

## B. 各 Step の完了状況

| Step | Task | 状態 | 主な成果物 | 備考 / commit |
|---|---|---|---|---|
| 0 | T001 | ✅完了 | workspace `Cargo.toml`、Bun workspaces `package.json`、`Taskfile.yml`、`.github/workflows/ci.yml` (rust + bun 並列 job)、placeholder server / client / demo crate | `4ba23f3` / Bun pin `1.3.13` / `bun test --pass-with-no-tests` で初回 CI green |
| 1 | T002 | ✅完了 | `packages/server` skeleton:<br>・`port::pick_free_listener` (19000-19999 shuffle bind)<br>・`registry::write_instance_file` (uuid + rename atomic)<br>・`proto::{Info, Capability}`<br>・`GET /test/info`<br>・`start(&Application) -> Handle`<br>・`#[cfg(feature = "e2e")]` 全 gate | `cd3680e` / 8 unit tests / `cargo tree --no-default-features` で feature OFF 時に依存空を確認 |
| 2 | T003 | ✅完了 | `packages/demo` skeleton: feature propagation で `--features e2e` → server 自動有効化、Entry + Apply Button + Label の最小 UI、`Rc<RefCell<Option<Handle>>>` で Handle root 保持、README "Quick start" 節 | `1c09909` / 実機 (macOS) で `[gtk4-e2e-demo] server up on http://127.0.0.1:19673/test/info` 確認 |
| 3 | T004 | ✅完了 | codegen pipeline:<br>・`schema_export.rs` + `examples/gen-schemas.rs`<br>・`proto/schemas/{Info,Capability}.schema.json` (commit 対象)<br>・`scripts/gen-types.ts`<br>・CI stale check (`git diff --exit-code packages/server/proto/schemas/`)<br>・`docs/adr/0002-codegen-pipeline.md` (Status: Proposed) | `857a248` / `cargo test 5` schema_export integration / SSOT idempotent (2 回連続実行で diff 0) |
| 4 | T005 | ✅完了 | TS SDK + CLI:<br>・`errors.ts` (`E2EError` / `DiscoveryError` / `HttpError` / `NotImplementedError`)<br>・`discover.ts` (registry 読み + pid liveness + env injection)<br>・`client.ts` (`E2EClient.discover/getInfo/tap/screenshot`)<br>・`cli.ts` (subcommand `info` / `tap` / `screenshot`、global flag `--port` / `--pid` / `--app` / `--token`) | `a949001` / **33 bun tests** / exit code 規約: 0=ok / 2=argv / 3=NotImplementedError / 4=DiscoveryError / 5=HttpError / 1=other |
| 5 | T006 | ✅完了 | `tree.rs` / `input.rs` / `wait.rs` / `main_thread.rs` を新設:<br>・selector parser `#name` + `WidgetTree` trait<br>・tap Button MVP + `resolve_xy` bbox hit-test、6 種 `TapError`<br>・long-polling 100ms tick + `eval_condition` 純関数<br>・越境 smoke `oneshot_roundtrip_under_glib_and_tokio` pass (Open Q-G close)<br>・`/test/tap` 200/400/422/404/500、`/test/wait` 408 timeout、未登録 endpoint 501 fallback<br>・SDK `client.wait()` + `WaitTimeoutError`、demo widget rename (`entry1` / `btn1` / `label1`)<br>・proto 拡張 (`TapTarget` untagged / `WaitRequest` / `WaitCondition` tagged on `kind` / `WaitResult { elapsed_ms }`)<br>・CI に `cargo test --all --features e2e` 追加 | `a1163e8` / **61 cargo + 41 bun tests** / Activatable / Switch / CheckButton 系 tap は Step 9 申し送り |
| 6 | T007 | ✅完了 | `snapshot.rs` (`WidgetPaintable` + `gtk::Snapshot` + `gsk::CairoRenderer` + `gdk::Texture::save_to_png_bytes`、gtk4 `v4_6` feature 追加)、`GET /test/screenshot` 200 PNG、`Capability::Screenshot` 末尾追加、`packages/demo/scenarios/screenshot.spec.ts` (`fetchScreenshotWithRetry` で frame clock タイミング吸収)、CI `scenarios` ジョブ追加 (xvfb-run `1280x720x24`、失敗時 `/tmp/scenario-artifacts/` upload retention 7 日) | `89c3879` / **66 cargo + 42 bun tests** / PNG = 360x200 RGBA (demo default サイズ一致) / visual regression diff は MVP 範囲外 |
| 7 | T008 | ✅完了 | `ws.rs` (`WS /test/events`、`parse_kinds` / `should_forward` 純関数)、`tokio::sync::broadcast::channel(256)`、`proto::EventEnvelope` + `EventKind { StateChange, LogLine }` + `Capability::Events`、SDK `events.ts` `openEventStream(client, opts)` (full-jitter exponential backoff、default `maxRetries: 10`)、demo on-click が `state_change` envelope を push、`proto/asyncapi.yaml` 追加 | `e97dd01` / **80 cargo + 48 bun tests** / `EventKind::LogLine` は forward-compat 用 (MVP では emit せず) |
| 8 | T009 | ✅完了 | `recorder.ts` (`Bun.spawn(["ffmpeg", ...])` X11 capture、SIGTERM→3s polling→SIGKILL、PID file `recorder.json` atomic write、stale PID 自掃除)、`record start/stop/status` CLI (exit code **6** = `RecorderError`)、`claude-plugin/.claude-plugin/plugin.json` (canonical filename — task 本文の `manifest.json` は不採用、`~/.claude/plugins/cache/` 慣行に従う)、`commands/{e2e-tap,e2e-record,e2e-scenario}.md` (`$ARGUMENTS` テンプレ)、`skills/gtk4-e2e/SKILL.md` (auto-trigger)、README に `Recording (MVP: X11)` + `Claude Code integration` 節追加 | `d9dc831` / `6c781d5` / `f0d718c` / `37eb247` / **81 bun tests** / Wayland / no-DISPLAY / macOS は即時 fail (kind discriminator) |
| 9 | T010 | ⚠️ subtask 化 (コード変更ゼロ) | T013 (type) / T014 (swipe) / T015 (pinch) を **draft / priority low** で起票。共通基盤 (Step 5 input.rs / proto.rs) は完成済み。consumer ニーズが固まった subtask から `ready` 昇格させる運用を採用 | commit なし / `seed.md §6 Step 9` 末尾「pinch の必要性は consumer 側ニーズ次第」と整合 |
| 10 | T011 | ✅完了 | `packages/demo/scripts/record-run.sh` (cargo build warm-up + `bunx record start` + `bun test scenarios/` + `bunx record stop` + `trap cleanup EXIT` + `[ -s "$OUTPUT" ]` ガード、executable bit)、`.gitignore` に `artifacts/` 追加 (mp4 を repo に持たない)、CI `record-demo` ジョブ (Ubuntu + GTK4 + xvfb + ffmpeg、`bunx gtk4-e2e --help` smoke、`actions/upload-artifact@v4` `demo-run-mp4` `if-no-files-found: error` `retention-days: 30`)、README に `Recorded demo run` 節 | `b1ad0b0` / 録画品質: fps=15、libx264/yuv420p/preset veryfast、解像度 = display 依存 (CI は 1280x720)、scenarios 全 3 spec を 1 ラン |

---

## C. デモ録画

### 入手方法

- **ローカル生成 (唯一の入手手段)**: Linux + X11 + ffmpeg + GTK4 環境で `bash packages/demo/scripts/record-run.sh` ([`../packages/demo/scripts/record-run.sh`](../packages/demo/scripts/record-run.sh)) を実行 → [`../artifacts/demo-run.mp4`](../artifacts/demo-run.mp4) に出力 (`.gitignore` 対象なので tracked にはならない)。headless で回したいときは `xvfb-run -a --server-args="-screen 0 1280x720x24" bash packages/demo/scripts/record-run.sh`。
- **macOS / Wayland**: 録画 backend が X11 only のため exit code 6 (`RecorderError`) で即時失敗 (T009 MVP 範囲)。
- **CI artifact は提供しない (T016)**: かつて `record-demo` ジョブが mp4 を upload していたが、PR / push 双方で xvfb + ffmpeg + cargo build + scenarios に約 2 分の恒常コストがかかる割に「report 時にローカルで見られれば十分」と判断され、T016 で job ごと削除した。録画は report / レビューで必要になったときに上記コマンドで都度生成する運用とする。

ローカルでは記録するごとに `artifacts/demo-run.mp4` を上書きする運用とし、**ローカル実行が唯一の source of truth**（共有が必要なら手元の mp4 を PR コメント / Slack 等で添付する）。本 worktree のチェックアウト時点では `artifacts/` ディレクトリは存在しない (`.gitignore` + `record-run.sh` 未実行)。

### 録画保存方法の判断 (T011 → T016 で再判断)

**現行: (d) ローカル運用 (T016 採択)**。CI には `record-demo` ジョブを持たず、必要な人が `bash packages/demo/scripts/record-run.sh` を都度実行する。

判断履歴:

- T011 で `(b) GH Actions artifact` を採用 (再現可能・retention で容量管理可能・run 単位 URL で共有可能、`(a) repo commit / git lfs` はサイズ未測定で却下、`(c) Release` は手動ステップ多く却下)。
- T016 で再判断: 動画は report / レビューでたまに見られれば十分で、PR / push 双方の CI で xvfb + ffmpeg + cargo build + scenarios を毎回回す約 2 分のコストに見合わない。`(b)` から `(d) ローカル運用` に降格。
- `(a) repo commit / git lfs` および `(c) Release` の却下理由は再判断後も有効。

### 録画内容

既存 3 spec (`screenshot.spec.ts` / `events.spec.ts` / `tap-wait.spec.ts`) を `bun test packages/demo/scenarios/` で一括実行する流れを xvfb (`1280x720x24`) で 1 ラン録画。CI artifact 化はせず、ローカル生成 (上記コマンド) のみで提供する。GitHub README は mp4 inline 再生不可なので、共有が必要なときは手元の mp4 を PR コメント等に添付する運用とする。

---

## D. MVP 完走条件チェック

seed.md §8 と ADR-0001 §Verification Phase 2 の 10 項目を T001-T011 の実装結果から判定。

| # | 条件 | 状態 | 達成 evidence |
|---|---|---|---|
| 1 | `packages/server` を `packages/demo` に組込み、外部 HTTP からテストボタンタップ | ✅ | T003 で demo 起動確認 + T006 で `client.tap("#btn1")` → label 変化を long-poll で観測 (scenarios `tap-wait.spec.ts`) |
| 2 | 同一マシン上で 2 インスタンス起動、port 衝突せず両方独立に操作可能 | ✅ | T002 `port::pick_free_listener` で 19000-19999 shuffle bind + `registry::write_instance_file` 1 ファイル/インスタンス。`SDK.discover()` (T005) で `pid` / `app` フィルタ可 |
| 3 | `screenshot` 出力の visual diff threshold で safelist 通過 | ⚠️ 部分 | T007 で PNG 取得は実装済 + `screenshot.spec.ts` で smoke pass。**visual regression diff は MVP 範囲外**、保存のみ (§E に再掲) |
| 4 | `wait` long-polling で `state_eq` 条件が 5 秒以内に発火 | ✅ | T006 `wait.rs::poll_until` (100ms tick) + `tap-wait.spec.ts` で実測 (`elapsed_ms < 3000`) |
| 5 | WebSocket `/test/events` で `frame_ready` を 5 秒間購読し loss なし | ⚠️ 部分 | T008 `WS /test/events` 実装済 + `events.spec.ts` で `state_change` 受信を実測。**`frame_ready` event 種別は未実装** (`EventKind::StateChange` / `LogLine` のみ、`LogLine` も emit 側未実装、§E に再掲) |
| 6 | TS SDK 経由で 1 シナリオが `bun test` で完走 | ✅ | T006 / T007 / T008 の `packages/demo/scenarios/*.spec.ts` を CI `scenarios` ジョブ (xvfb) で実走 |
| 7 | Claude Code plugin (slash command + SKILL) 経由で同シナリオが起動できる | ✅ structural | T009 で `claude-plugin/.claude-plugin/plugin.json` + `commands/{e2e-tap,e2e-record,e2e-scenario}.md` + `skills/gtk4-e2e/SKILL.md` 配置。**Claude Code 上での実機動作確認は手動 (T009 plan §7.3) で skip**、構造的には完備 (`claude-plugin.test.ts` 12 件 pass) |
| 8 | `recorder.ts` で 30 秒の操作録画 → mp4 出力 | ✅ | T009 + T011 で `record-run.sh` + CI `record-demo` ジョブで mp4 artifact 化 (`demo-run-mp4`、retention 30 日) |
| 9 | codegen の整合: CI で `bun run gen:types && git diff --exit-code` で stale 検出 | ✅ | T004 ADR-0002 採択、CI rust ジョブで `git diff --exit-code packages/server/proto/schemas/` を毎回実行 |
| 10 | feature flag `e2e` での conditional compile (本番除外) | ✅ | T002 で `cargo tree --no-default-features` で production 依存空を確認、T003 で demo `cargo build`(features なし) も同様確認 |

ADR-0001 §Verification Phase 1 (本 ADR の妥当性検証 5 項目) は Step 1〜3 までで全て満たされている (gtk4-rs `EventController` / GSK Renderer / axum + tokio 並行 / feature flag conditional / schemars + JSON Schema → TS pipeline)。本節は Phase 2 のみ転記する。

---

## E. 残課題・既知の制約

### MVP capability の未実装

- **type / swipe / pinch** (Step 9): T010 でメタ計画化、T013 / T014 / T015 を draft / low priority で起票済。consumer ニーズが固まった subtask から `ready` 昇格させる運用。
- **eval** (`POST /test/eval`): seed.md §4 で「optional / future」、ADR-0001 で「専用 mutator endpoint で代替か Lua sandbox か」未確定。subtask 起票なし、ADR 化が前提。
- ~~**`/test/state`** endpoint と `state_eq { path: "session.mode" }` の app-defined state schema 設計 (T006 Open Q-C)~~ → **完了 (T019)**: `Capability::State` 追加、`Handle::set_state(json)` API、`WaitCondition::AppStateEq { path, value }` (JSON Pointer / RFC 6901)、SDK `client.state()` を整備。
- ~~**Activatable / Switch / CheckButton tap 対応**~~ → **完了 (T019)**: `tap_widget` を派生クラス → 基底クラスの順で downcast する dispatch 構造に変更し、`Switch` / `CheckButton` / `ToggleButton` で `set_active(!active)` を発火。GTK4 では `Activatable` interface は削除済みのため `gtk::Widget` の派生別 toggle に統一。
- **`/test/elements`** widget tree query endpoint は未実装 (現状 server 側で walk するのは `find_first` のみ)。
- **`Capability::VideoStream`** (`WS /test/video/stream`): 現 ffmpeg X11 capture で十分なので未実装、必要が顕在化したら追加。

### Visual regression diff (T007 申し送り)

screenshot は PNG 保存のみ。pixel diff / SSIM などの diff engine は未配線。Step 10 以降の別タスク。

### 録画 backend の制約 (T009 / T011)

- **X11 のみ MVP 対象**。Wayland (`WAYLAND_DISPLAY`) / macOS / no-DISPLAY (headless) は exit code 6 で即時 fail。
- EGLFS / KMS 環境 (`kmsgrab`) 対応は ADR-0001 Open Question 残置。
- README で mp4 を inline 再生不可 (link only)。

### 周辺整備の未完

- ~~**Biome 未導入**~~ → **完了 (T017)**: ルートに `biome.json` を追加、`packages/client/package.json` の `lint` / `fmt:check` を biome 実体化、CI bun job で `bun run lint` / `bun run fmt:check` を実行。
- ~~**`tsc --noEmit` を CI に未連携**~~ → **完了 (T017)**: TS2322 baseline 2 件 (`cli.test.ts:67` / `events.test.ts:49` の `Bun.serve(...).port` narrowing) を非 null assertion で解消、CI bun job に `tsc --noEmit (packages/client)` step を追加。
- **`InstanceFile` の SSOT 化**: registry file format `InstanceFile` は現在 SDK 側 (TS) と server 側 (Rust) で別個に書かれている (ADR-0002 Open Question)。
- ~~**`packages/server/src/cli.ts` の executable bit 変更** (T006)~~ → **完了 (T019)**: 実体は `packages/client/src/cli.ts` (server 配下には存在しない、prompt 表記の誤記) で、`100755` で commit 済み (`b30903e7`) を再確認し追認。shebang `#!/usr/bin/env bun` 付きで `bunx gtk4-e2e` / `bun run …/cli.ts` 双方から呼べる。
- **手動検証 skip 項目**: T003 の window close → registry cleanup、T009 の Claude Code 上 plugin install、T011 の Linux X11 golden path は CI / display 持ちレビュアーに委譲。

### ADR-0002 の status

`Proposed` のまま。`Accepted` 昇格運用は ADR README に未明文化、別 PR で運用ルール明示が必要。

---

## F. 判断ポイント

各 task の summary から「Master 判断ポイント」相当を集約。決定事項 + 採用理由の形で列挙する。

- **`.team/` ディレクトリの git 扱い**: 本 repo では `.team/tasks/*/runs/*/` 以下のすべての成果物 (plan / summary / inspection) は repo に commit せず別管理 (worktree limited)。`.gitignore` には `.team/` の明示記述なし、`.team/` 配下を main に push しない運用とする。
- **録画保存方法**: T011 で GH Actions artifact (`(b)`) を採用したが、T016 でローカル運用 (`(d)`) に再降格。理由は §C 参照 (CI コストに見合わず、report 時にローカル生成すれば十分)。
- **TS 型生成ツール**: `json-schema-to-typescript` (npm v15) を採用 (ADR-0002 D1)。`typeshare` / `ts-rs` を却下した理由: SSOT 二重化を避け JSON Schema 中間表現で OpenAPI / 他言語 SDK への展開余地を残すため。
- **schema 出力経路**: `examples/gen-schemas.rs` 経由 (build.rs 不採用、ADR-0002 D2)。理由: build.rs に struct 再定義する案は SSOT 違反、test 副作用案は race、build.rs から `cargo run` する案は reproducible build 破壊。
- **stale check 戦略**: schema を commit、`*.gen.ts` は build artifact (ADR-0002 D4)。理由: schema diff 1 ファイルでも残れば CI が落ちる、TS formatter / 生成ツール version 差での false positive を回避。
- **Bun pin**: `1.3.13` (T001)。`packageManager` フィールドと CI `bun-version` 双方で同一値固定。
- **Plugin manifest filename**: `plugin.json` を採用 (T009)。task 本文・seed.md §5 の `manifest.json` 表記は不採用。理由: `~/.claude/plugins/cache/` 観測上、複数の既存 plugin (hookify / cmux-team) が `plugin.json` を採用。
- **CLI exit code 規約**: `0`=ok / `1`=other / `2`=argv / `3`=NotImplementedError / `4`=DiscoveryError / `5`=HttpError / `6`=RecorderError (T005 + T009)。
- **`TapTarget` の wire 形式**: untagged enum (`{selector}` または `{xy:{x,y}}`)。SDK 側手書き型は削除、generated `types.gen.ts` から re-import (T006)。
- **proto Capability ordering**: append-only 規律 (`Info, Tap, Wait, Screenshot, Events`)。`schema_export` ordering anchor test で固定 (T004 / T006 / T007 / T008)。
- **Wait condition の `kind` tagged union**: `{kind: "selector_visible", ...}` / `{kind: "state_eq", ...}` / `{kind: "log_match", ...}` (T006)。
- **Reconnect 戦略 (events)**: full-jitter exponential backoff、default `maxRetries: 10` (有限)、`Infinity` は opt-in (T008)。
- **broadcast channel 容量**: 256 (T008)。`RecvError::Lagged(n)` は warn log + subscriber 継続。
- **501 統一**: 未登録 endpoint / 未実装 capability は `axum::Router::fallback` で 501 共通化 (T006)。SDK 側は 501 → `NotImplementedError`、404 → `HttpError` (selector_not_found 等)。
- **gtk4-rs version**: `0.9` 統一 (T002 / T003)。`v4_6` feature を T007 で screenshot 用に追加。
- **Step 9 を分割した理由**: capability 3 種は独立加算機能なので PR 単位で個別に切れる (T010)。consumer ニーズで起動 timing を選ぶため `draft` / `low` で温存。
- **README 導線の言語**: REPORT.md 本体は日本語、README の `## Status` 導線文は既存 README の英語トーンを維持 (T012 plan §2)。

---

## G. 次にやるべきこと

### 短期 (consumer 接続を見据えた整備)

- **Brainship 等の consumer への接続**: `gtk4-e2e-server` を consumer の `Cargo.toml` に `gtk4-e2e-server = { git = "...", features = ["e2e"] }` として追加 (debug build 時のみ)。consumer 側は `dev.foo.MyApp.start()` の中で `gtk4_e2e_server::start(&app)` を呼び、`Handle` を `Rc<RefCell<Option<Handle>>>` に root 保持する (T003 demo パターン参照)。scenario は別 repo / 別 directory に bun project として作る。
- **ADR-0002 を `Accepted` 昇格** + ADR README に運用ルール追記。
- **`plugin.json` filename** に合わせて `seed.md §5` の `manifest.json` 記述を更新 (T009 申し送り)。

### 中期 (MVP capability の埋め込み)

- **T013 type / T014 swipe / T015 pinch** を consumer ニーズに応じて順次 `ready` 化、実装。推奨着手順: type → swipe → pinch (T010)。
- **`/test/elements`** widget tree query endpoint。
- ~~**`/test/state`** + state_eq path schema 設計 (T006 Open Q-C)~~ → **完了 (T019)**。
- ~~**Activatable / Switch / CheckButton tap 対応** (T006 申し送り)~~ → **完了 (T019)**。
- **visual regression diff** (T007 申し送り): pixel diff / SSIM、baseline 管理。
- ~~**`real_widget_visible_after_present` integration test** (T006 Open Q-K)~~ → **完了 (T019)**。

### 長期 (capability roadmap)

- **`POST /test/eval`** capability — `mlua` sandbox か専用 mutator endpoint か決定 (ADR 化)。
- **`WS /test/video/stream`** capability — kmsgrab / EGLFS 環境向けに ffmpeg 不可な経路用。
- **複数言語 SDK** (Python / Rust) — JSON Schema を中間表現として codegen target 追加 (ADR-0002 で余地残置)。
- **schemars 0.8 → 1.0 移行** (draft-2020-12 出力対応、別 ADR)。
- **macOS / Wayland 録画 backend** — `screencapture` / `wayland-screencopy` 等の外部ツール調査。
- **メンテナ向け開発フロー**: Taskfile (`task dev:demo` / `task gen:types` / `task ci`) + CI matrix (現状 `ubuntu-latest` のみ、macOS は manual) を README で 1 節にまとめる。
