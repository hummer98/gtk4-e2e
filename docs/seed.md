# gtk4-e2e — Seed Instructions for Claude Code

このドキュメントは、まっさらな Claude Code セッションが本リポジトリで作業を開始するための**最小かつ十分な前提**をまとめたもの。詳細な意思決定背景は [`docs/adr/0001-architecture.md`](./adr/0001-architecture.md) を参照。本ファイルは bootstrap が完了してアーキテクチャが安定したら、`README.md` と各 package の `README.md` に役割を委譲して短縮してよい。

---

## 1. プロジェクトのゴール

GTK4 + Rust で書かれた Native GUI アプリケーション向けの **Playwright 同等の e2e テスト基盤**を作る。具体能力:

- **画面要素取得** (widget tree の論理構造 + screenshot)
- **操作エミュレーション** (tap / swipe / pinch / type)
- **wait 系** (long-polling で selector visibility / state 変化 / log マッチ)
- **インスタンス分離** (port 自動払い出し + registry file での discovery、複数アプリ並列テスト可)
- **動画録画** (ffmpeg で X11/Wayland capture、解説動画・デモ素材・UX 検証用途)
- **Claude Code 統合** (slash command + SKILL + SDK)
- **AI / GPU パイプライン検証** (将来の capability、optional)

ブラウザ自動化ツール (Playwright / WebDriver / Cypress) は **対象外**。GPU 加速カメラ + AI 推論 + キオスク描画の Native アプリでは適用できないため、独自に書く。

## 2. 非ゴール (やらないこと)

- ブラウザ向けテストツールの再発明 (DOM / iframe / cookies は対象外)
- Qt / Flutter / Web 等の他フレームワーク対応 (将来の余地は残すが MVP では GTK4+Rust に集中)
- 触感 (haptic feedback) / 圧力センサ / 3D touch のテスト
- 本番バイナリへの組込み (server コードは debug build のみ、Cargo feature flag で完全除外)
- Brainship 内部仕様 (Envelope schema, helva-frontend 構造, etc.) への依存。本 repo は単独で完結する

## 3. アーキテクチャ要約

### 3 パッケージ、2 言語

```
packages/
├── server/   # Rust crate: consumer app に in-process 組込み (debug build のみ)
├── client/   # Bun/TypeScript: SDK + CLI + recorder + Claude Code plugin (外部プロセス)
└── demo/     # Rust binary: server を組込んだ最小 GTK4 app、framework 単独で develop / CI 回す
```

### 言語マップ

| レイヤ | 言語 |
|---|---|
| in-process e2e server | **Rust** (`packages/server`) |
| consumer app (利用先) | Rust + gtk4-rs (本 repo の対象は demo) |
| test SDK / CLI / recorder / Claude plugin | **Bun / TypeScript** (`packages/client`) |
| protocol 型 SSOT | Rust struct + serde + schemars |
| TS 型 | `build.rs` で Rust から自動生成 (`*.gen.ts`)、**手書き禁止**、`.gitignore` 対象 |

### server は「途中のサーバー」ではなく in-process

`packages/server` は **consumer app と同一プロセス内のスレッドで HTTP/WS port を開く** Rust crate。別プロセスのサイドカーではない。Playwright で browser が CDP port を開くのと同じ構造。

理由:
- gtk4-rs の widget tree / event 合成 / snapshot は app プロセス内でしか触れない
- 別プロセスにすると IPC 越しになり致命的に重く・触れない API も増える
- 本番除外は「別プロセスを停止」ではなく「Cargo feature flag で server コード自体を build しない」で実現する

```
┌─────────────────────────────────────────────┐
│  Consumer GTK4 App (Rust, debug build)     │
│  ┌──────────┐    ┌─────────────────────┐   │
│  │ GTK4 main│ ←→ │ gtk4-e2e-server     │   │
│  │ loop     │    │ (axum on tokio,     │   │
│  │ (UI/...) │    │  127.0.0.1:1900x)   │   │
│  └──────────┘    └──────────┬──────────┘   │
└─────────────────────────────┼───────────────┘
                              │ HTTP / WS
                              ▼
                  ┌──────────────────┐
                  │ gtk4-e2e (Bun/TS)│  ← 別プロセス
                  │ SDK / CLI / Plugin│
                  └──────────────────┘
```

## 4. プロトコル概要

### REST endpoints

```
GET    /test/info                             # version, instance_id, port, capabilities
GET    /test/elements?selector=...            # widget tree query
POST   /test/tap         { selector|xy }
POST   /test/swipe       { from, to, duration_ms }
POST   /test/pinch       { center, scale, duration_ms }
POST   /test/type        { selector, text }
POST   /test/wait        { condition, timeout_ms }   # long-polling、Playwright の waitFor 相当
GET    /test/screenshot                              # PNG
GET    /test/state                                   # app state snapshot
POST   /test/eval        { script }                  # optional / future
```

### WebSocket

```
WS /test/events            # state change / log line / render frame ready (event subscription)
WS /test/video/stream      # frame 列 (optional, ffmpeg 不可な経路用)
```

WebSocket は**真にストリームが必要な経路に限定**する。`waitForSelector` 相当は REST `POST /test/wait` の long-polling で実装すること (server 側で gtk4-rs signal / `glib::timeout_add` を購読、条件成立で 200、タイムアウトで 408)。

### Capability negotiation

すべての endpoint は `/test/info` の `capabilities` 配列で公開、未実装は **501 Not Implemented**:

```json
{ "error": "not_implemented", "capability": "video_stream" }
```

MVP では `eval` / `video_stream` を未実装で出して OK。

### インスタンス分離

- server は **127.0.0.1 のみ bind**、port は **19000-19999 のランダム**
- 起動時に `$XDG_RUNTIME_DIR/gtk4-e2e/instance-${pid}.json` を書き出し、終了時に削除
- ファイル内容: `{ pid, port, app_name, app_version, started_at, token }`
- SDK の `E2EClient.discover()` で registry を読みアクティブ instance を列挙
- 認証: 環境変数 `GTK4_E2E_TOKEN` (任意)

### 本番除外

- Cargo feature `e2e` は **default off**
- consumer は debug 用に `cargo build --features e2e` で組込
- 本番ビルド (`--no-default-features`) では server コードが**1 byte も含まれない**

## 5. リポジトリ構成 (期待形)

```
gtk4-e2e/
├── Cargo.toml                  # workspace = ["packages/server", "packages/demo"]
├── package.json                # workspaces = ["packages/client"]
├── README.md
├── LICENSE                     # MIT
├── .gitignore
├── docs/
│   ├── seed.md                 # ★ this file
│   └── adr/
│       ├── README.md
│       └── 0001-architecture.md
├── packages/
│   ├── server/
│   │   ├── Cargo.toml
│   │   ├── build.rs            # schemars で JSON Schema → ../client/src/types.gen.ts
│   │   ├── src/
│   │   │   ├── lib.rs          # pub fn start(app: &gtk::Application) -> Handle
│   │   │   ├── http.rs
│   │   │   ├── ws.rs
│   │   │   ├── tree.rs
│   │   │   ├── input.rs
│   │   │   ├── snapshot.rs
│   │   │   ├── wait.rs
│   │   │   ├── registry.rs
│   │   │   └── proto.rs        # SSOT 型定義 (serde + schemars)
│   │   ├── proto/
│   │   │   ├── openapi.yaml
│   │   │   └── asyncapi.yaml
│   │   └── tests/
│   ├── client/
│   │   ├── package.json
│   │   ├── tsconfig.json
│   │   ├── src/
│   │   │   ├── index.ts
│   │   │   ├── client.ts
│   │   │   ├── selectors.ts
│   │   │   ├── recorder.ts
│   │   │   ├── cli.ts
│   │   │   └── types.gen.ts    # 自動生成、手書き禁止、.gitignore
│   │   └── claude-plugin/
│   │       ├── .claude-plugin/manifest.json
│   │       ├── commands/
│   │       └── skills/gtk4-e2e/SKILL.md
│   └── demo/
│       ├── Cargo.toml
│       ├── src/main.rs
│       ├── assets/
│       └── scenarios/          # TS テストスクリプト
└── .github/workflows/
    └── ci.yml                  # build (cargo + bun) + lint + test
```

## 6. 初期 bootstrap タスク (推奨順序)

依存関係順。各タスクは小さくまとまり、PR にしやすい単位で切る。

### Step 0: プロジェクト骨格

1. ルート `Cargo.toml` を Cargo workspace として作成 (`members = ["packages/server", "packages/demo"]`)
2. ルート `package.json` を Bun workspaces として作成 (`workspaces = ["packages/client"]`)
3. `.github/workflows/ci.yml` を作成 (Rust + Bun の lint/test/build を回す)
4. ルート `Taskfile.yml` (or scripts) で `task dev`, `task gen:types`, `task test:demo` 等のショートカット定義

### Step 1: server crate スパイク

1. `packages/server/Cargo.toml` 作成、`gtk4`, `tokio`, `axum`, `serde`, `schemars`, `tower-http` 等を追加
2. `lib.rs` で `pub fn start(app: &gtk::Application) -> Handle` を提供 (axum を `tokio::spawn`、glib main loop と並行動作)
3. `proto.rs` に最小型 (`Info`, `TapRequest`, `WaitRequest`, etc.) を `#[derive(Serialize, Deserialize, JsonSchema)]` 付きで定義
4. `/test/info` だけ動かす (instance_id, port, capabilities)
5. `registry.rs` で `$XDG_RUNTIME_DIR/gtk4-e2e/instance-${pid}.json` 書き出し
6. `feature = "e2e"` で全コードを gate、デフォルト OFF

### Step 2: demo crate スパイク

1. `packages/demo/Cargo.toml` 作成、`gtk4`, `gtk4-e2e-server = { path = "../server", features = ["e2e"] }` 依存
2. `main.rs` で gtk::Application を起動、Button + Entry + Label の最小 UI
3. デバッグビルドで起動すると server が in-process で port を開いていることを `curl localhost:1904x/test/info` で確認

### Step 3: codegen pipeline

1. `packages/server/build.rs` で `schemars::schema_for!(...)` を全型に対し回し JSON Schema を `proto/schemas/` に出力
2. `json-schema-to-typescript` (or `typeshare` or `ts-rs`) を選定し、`packages/client/src/types.gen.ts` を生成
3. CI に "stale check" を追加 (`bun run gen:types && git diff --exit-code`)
4. `.gitignore` に `*.gen.ts` を確認、PR で偶発的に commit されないこと

### Step 4: client (TS SDK) スパイク

1. `packages/client/package.json` 作成、`bun init` 相当
2. `client.ts` に `E2EClient.discover()` を実装 (`fs.readdir($XDG_RUNTIME_DIR/gtk4-e2e/)`、active instance 選択)
3. `getInfo()` / `tap()` / `screenshot()` の最小実装
4. `cli.ts` で `bunx gtk4-e2e info`, `bunx gtk4-e2e tap 100 200`, `bunx gtk4-e2e screenshot out.png`
5. demo を起動して CLI から疎通

### Step 5: 操作 + wait

1. `packages/server/src/input.rs` で tap (selector or xy) を `gdk::EventController` 経由で合成
2. `wait.rs` で `selector_visible` / `state_eq` を long-polling 実装、`glib::timeout_add` で監視
3. SDK 側で `client.wait({ kind: "selector_visible", selector: "#btn1" })` を提供
4. demo の Button を SDK から tap → label 変化を wait で確認

### Step 6: snapshot + scenarios

1. `snapshot.rs` で `WidgetExt::snapshot` (GSK Renderer) → PNG 化
2. SDK `client.screenshot()` 実装
3. `packages/demo/scenarios/tap.spec.ts` 等を bun test で記述、CI で回す
4. visual regression は MVP では PNG 保存のみ、diff は後続

### Step 7: events / WebSocket

1. `ws.rs` で `/test/events` endpoint を実装、glib signal の subset を購読
2. SDK 側で `client.events()` async iterator を提供
3. demo の progress bar tick を購読

### Step 8: recorder + Claude plugin

1. `recorder.ts` を `Bun.spawn(["ffmpeg", ...])` で実装、X11 を MVP 対象とする
2. `bunx gtk4-e2e record start/stop` CLI を提供
3. `claude-plugin/` に slash command (`/e2e-tap`, `/e2e-record`, `/e2e-scenario`) と SKILL.md を作成
4. SKILL は CLI を呼ぶ薄い shell に留める (Claude Code が SKILL 経由で `bunx gtk4-e2e ...` を起動)

### Step 9: pinch / swipe / その他 capability 拡張

順次。`pinch` の必要性は consumer 側ニーズ次第。

## 7. 開発ワークフロー

```bash
# demo 起動 (1 ターミナル目)
cargo run -p gtk4-e2e-demo --features e2e

# 別ターミナルで scenario 実行
cd packages/demo
bun test scenarios/

# CLI 単発操作 (デバッグ用)
bunx gtk4-e2e info
bunx gtk4-e2e tap '#btn1'
bunx gtk4-e2e screenshot /tmp/now.png

# 動画録画
bunx gtk4-e2e record start --output run.mp4
# ... 操作 ...
bunx gtk4-e2e record stop
```

ルートに `Taskfile.yml` を置いて `task dev:demo`, `task test:e2e` などを定義することを推奨 (使用は任意)。

## 8. テスト方針

- **server crate のユニットテスト**: `packages/server/tests/` で mock GTK app に対する integration test (`gtk4` を test mode で起動)
- **client SDK のユニットテスト**: `packages/client/test/` で server に対する mock or 実 demo 起動でテスト
- **demo の e2e**: `packages/demo/scenarios/` を bun test で実行、CI で必須化
- **codegen の整合**: CI で `bun run gen:types && git diff --exit-code` で stale check
- **MVP の必須完走条件**: ADR-0001 §Verification Phase 2 の項目すべて

## 9. CI (GitHub Actions)

`.github/workflows/ci.yml` で以下を回す:

- Rust: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --all`, `cargo build --features e2e`
- Bun/TS: `bun install --frozen-lockfile`, `bun run lint`, `bun test`
- Codegen stale check: `bun run gen:types` 後 git diff が空であること
- (将来) demo を起動して shell スクリプトから 1 シナリオ完走

OS マトリクスは MVP では `ubuntu-latest` のみで OK。`gtk4-dev` 等のシステム依存は apt で入れる。

## 10. コーディング規約

### Rust (`packages/server`, `packages/demo`)

- `cargo fmt` / `cargo clippy -- -D warnings` を CI 必須
- `unsafe` は server / demo では使わない方針 (gtk4-rs の API を経由)
- すべての pub API に doc コメント (`///`)
- proto 型は `#[derive(Serialize, Deserialize, JsonSchema, Debug)]` を付与
- `Result<T, E>` を返す、panic は server boot 時のみ許容

### TypeScript (`packages/client`)

- Bun のネイティブ TypeScript で実装、tsc は使わない方針 (`bun run` 直接)
- `bun fmt` (Biome) で format、`biome lint` で lint
- 型は `types.gen.ts` から import、自分で書かない
- async 一貫、`async function` で書く、callback hell は避ける
- public API は `index.ts` から export

### コミット規約

- Conventional Commits: `feat(server): ...`, `fix(client): ...`, `docs: ...`
- 1 コミットは 1 論理単位、PR は 1 つのトピックにまとめる
- breaking change は `feat(server)!: ...` で明示

### PR 規約

- PR 説明に **どの ADR / どの bootstrap step に対応するか** 明記
- CI が緑になるまで merge 不可
- 別 step を一気にやらない (Step 1 PR / Step 2 PR を分ける)

## 11. 範囲外 / してはいけないこと

- **特定 consumer (Brainship 等) の内部仕様への依存** — 本 repo は consumer-agnostic に保つ。Envelope schema, 特定 Subject 命名, 特定 frontend の widget 命名規則などは持ち込まない
- **TS 型を手書き** — `types.gen.ts` は自動生成のみ。Rust の `proto.rs` を SSOT とする
- **server を別プロセス化** — in-process が物理制約 (gtk4-rs の widget アクセス)
- **本番ビルドへの test API 混入** — Cargo feature gate を確実に
- **WebSocket-only にする** — REST + WS の 2 方式を堅持、`waitFor` は long-polling REST
- **Squish / dogtail / xdotool 等の依存** — gtk4-rs を直接使うことが性能・正確性両面で正解

## 12. 関連ドキュメント

- [`docs/adr/0001-architecture.md`](./adr/0001-architecture.md) — 本アーキテクチャの完全な意思決定背景・代替案検討・Open Questions
- [`docs/adr/README.md`](./adr/README.md) — ADR 運用ルール
- [`README.md`](../README.md) — プロジェクト概要

## 13. 外部参照 (read-only context)

本 framework は元々 **Brainship プロジェクト** (`hummer98/Brainship` private) で必要とされ、検討の中で独立 repo に切り出された。Brainship 側には以下の関連物があるが、**本 repo はそれらに依存しない** (depend on it != reference it):

- Brainship の元 Issue で Native アプリ向け e2e の方針が決定された
- Brainship の元 ADR-0023 はスタブ化され、本 repo を参照する形になっている

Brainship の内部仕様 (Envelope, helva-frontend の widget 設計, etc.) は本 repo の consumer の一例としては想定するが、**API 契約には現れない**。`/test/state` 等が返す内容は consumer 自身が定義する自由形式 JSON とする。

## 14. 困ったら

- 設計判断の経緯: `docs/adr/0001-architecture.md` の Alternatives Considered / Open Questions
- 実装に着手する順序: §6 の bootstrap step を 0 → 9 順に
- 言語選定の理由: ADR §Decision の "言語マップ" と "中核論点 2"
- protocol 仕様: ADR §Decision の "プロトコル: REST + WebSocket"
- 「server は別プロセスでは？」: §3 の図と説明 (in-process が物理制約)
- 「TS 型を書きたい」: §11 で禁止、`types.gen.ts` は自動生成
