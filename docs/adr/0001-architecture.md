# ADR-0001: Architecture — In-process Rust server + Bun/TypeScript client + demo app

- **Status**: Accepted
- **Date**: 2026-05-10
- **Confidence**: 中 (70%) — 構造は確定、capability の MVP 範囲は実装で詰める

## Context

GTK4 + Rust で書かれた Native GUI アプリケーションに対し、Playwright 同等の e2e テスト能力を提供することがゴール。Browser ベースの自動化ツール (Playwright / WebDriver / Cypress) は対象外:

- GPU 加速カメラパイプライン (DeepStream / nvinfer / nvjpegdec) と HTML レンダリングの共存が困難
- ARM64 + NVIDIA GPU 環境で Playwright/CDP の制御が実用にならない
- AI 推論結果やパイプライン性能をテストから直接検証する必要がある場合、ブラウザ DOM 抽象では不足

GTK4 ネイティブ向けの既存ツール (Squish, dogtail, AT-SPI, GTK Test) も以下の理由で不採用:

- Squish: 商用 ($$$)、GTK4 サポートなし、AI 結果検証不可
- dogtail / AT-SPI: 機能不足、Playwright 同等の操作・wait 体系を持たない
- GTK Test: in-process unit test のみ、外部 orchestration 不可
- xdotool / PyAutoGUI: OS レベル汎用、widget 論理構造が取れない

→ 自作の正当性あり。

### 解決すべき要求

1. **画面要素取得** — 論理構造 (widget tree) または画像 (screenshot)
2. **操作エミュレーション** — tap, swipe (slide), pinch in/out, type
3. **内部 state 書換** — `eval` 相当 (optional capability)
4. **インスタンス分離** — 同時起動でコマンド衝突しない (port / token 払い出し)
5. **screenshot / 動画録画** — テスト assert + 解説動画 + デモ素材 + UX 検証用
6. **Claude Code plugin 配布** — slash command + SKILL
7. **SDK 提供** — コードベースで自動化記述可能 (テストスクリプト言語)

### 中核論点 1: 内包 vs 独立パッケージ

機能ごとに**置き場所が物理的に決まる**:

| 機能 | 配置制約 | 理由 |
|---|---|---|
| widget tree 取得 | **内包必須** | gtk4-rs の `Widget` ツリーは app プロセス内 |
| event 合成 (tap/swipe/pinch) | **内包必須** | `gdk::EventController` / `gdk::EventTouch` 系は in-process |
| screenshot | **内包必須** | `gtk::WidgetExt::snapshot` (GSK) は app プロセス内 |
| 内部 state 書換 | **内包必須** | Rust 構造体への直接アクセス |
| 動画録画 | 外部 | ffmpeg で X11/Wayland capture |
| SDK | 独立 | HTTP/WS client、app に同梱する理由なし |
| CLI | 独立 | SDK のラッパ |
| Claude Code plugin | 独立 | 配布物として独立リリースが自然 |
| protocol 仕様 | server crate に同梱 | Rust 起点で生成し他言語へ codegen |

→ 「ハイブリッド」が物理的に唯一解。

### 中核論点 2: SDK の言語選定 — Bun/TypeScript

server は構造的に Rust 必須。SDK / CLI / Claude plugin / scenario は別言語が選択可能。

| 言語 | 利点 | 欠点 |
|---|---|---|
| **Bun / TypeScript** | 起動 ~10ms、型を Rust から自動生成可、Claude Code 親和性高、Web 系エコシステム | TS↔Rust codegen pipeline 保守 |
| Rust | server crate と型共有が無料、CLI 単一バイナリ | テスト書換ごとに compile、scenario verbose、Claude Code が ad-hoc スクリプトを書きにくい |
| Python | pytest 文化、async / REPL デバッグ最強 | Brainship をはじめ多くの consumer 環境にとって追加言語負担 |
| Lua (mlua sandbox) | 軽量、hot-reload 可 | Claude Code の Lua 生成精度低、エコシステム弱 |

**結論: Bun / TypeScript を採用**。Cypress / Playwright / Selenium が「コア = compiled / テスト = scripting」分業を採るのと同じ構造。

## Decision

**3 パッケージ構成、2 言語。**

```
packages/
├── server/                     # Rust crate (consumer app に in-process 組込)
│   ├── Cargo.toml              # crate name = "gtk4-e2e-server"、feature `e2e` で有効化
│   ├── build.rs                # schemars で JSON Schema 生成 → ../client/src/types.gen.ts へ出力
│   ├── src/
│   │   ├── lib.rs              # pub fn start(app: &gtk::Application) -> Handle
│   │   ├── http.rs             # axum REST handlers
│   │   ├── ws.rs               # WebSocket events / streams
│   │   ├── tree.rs             # widget tree シリアライズ
│   │   ├── input.rs            # tap / swipe / pinch event 合成
│   │   ├── snapshot.rs         # screenshot (GSK Renderer)
│   │   ├── wait.rs             # long-polling wait endpoint
│   │   ├── registry.rs         # instance ID/port 払い出し
│   │   └── proto.rs            # Rust 型定義 (serde + schemars)、SSOT
│   ├── proto/                  # 仕様書 (人間向け)
│   │   ├── openapi.yaml
│   │   └── asyncapi.yaml
│   └── tests/
│
├── client/                     # Bun/TypeScript パッケージ
│   ├── package.json            # npm name = "gtk4-e2e"
│   ├── src/
│   │   ├── client.ts           # SDK 本体: await using c = await E2EClient.discover()
│   │   ├── selectors.ts
│   │   ├── recorder.ts         # Bun.spawn(["ffmpeg", ...]) で X11/Wayland capture
│   │   ├── types.gen.ts        # ★ server crate から自動生成 (.gitignore 対象)
│   │   ├── cli.ts              # bunx gtk4-e2e tap 100 200
│   │   └── index.ts
│   └── claude-plugin/
│       ├── .claude-plugin/manifest.json
│       ├── commands/           # /e2e-tap, /e2e-record, /e2e-scenario
│       └── skills/gtk4-e2e/SKILL.md
│
└── demo/                       # Rust binary、framework 単独で develop / CI 回すための参照実装
    ├── Cargo.toml              # bin name = "gtk4-e2e-demo"、server を path 依存
    ├── src/main.rs             # 最小 GTK4 app、各 capability を exercise する widget セット
    ├── assets/
    └── scenarios/              # demo に対する TS テストスクリプト (CI で実行)
```

### 言語マップ

| レイヤ | 言語 |
|---|---|
| consumer app (本フレームワークの利用先) | Rust + gtk4-rs |
| └ in-process e2e server | **Rust** crate (本 repo `packages/server`) |
| test SDK / CLI / recorder / Claude plugin | **Bun / TypeScript** (本 repo `packages/client`) |
| demo app (framework 単独 develop / CI) | Rust + gtk4-rs (本 repo `packages/demo`) |
| protocol 型 SSOT | Rust struct + serde + schemars |
| TS 型 | `build.rs` で Rust から自動生成、手書き禁止 |

### プロトコル: REST + WebSocket

#### REST (operations + long-polling wait)

```
GET    /test/info                             # version, instance_id, port, capabilities
GET    /test/elements?selector=...            # widget tree query
GET    /test/element/:id                      # 単体詳細
POST   /test/tap         { selector|xy }
POST   /test/swipe       { from, to, duration_ms }
POST   /test/pinch       { center, scale, duration_ms }
POST   /test/type        { selector, text }
POST   /test/wait        { condition, timeout_ms }   # long-polling
GET    /test/screenshot                      # PNG
GET    /test/state                           # app state snapshot
POST   /test/eval        { script }          # optional capability (debug only)
```

`POST /test/wait` の `condition` 例:

- `{ kind: "selector_visible", selector: "#mode-btn" }`
- `{ kind: "state_eq", path: "session.mode", value: "navigating" }`
- `{ kind: "log_match", pattern: "fps=\\d{2}" }`

server 側で gtk4-rs signal / `glib::timeout_add` を購読、条件成立まで block。タイムアウトで 408。SDK 側は `await client.wait(...)` で素直に記述。

#### WebSocket (events + streams)

```
WS /test/events            # state change / log line / render frame ready
WS /test/video/stream      # frame 列 (optional, ffmpeg 不可な経路用)
```

WebSocket は**真にストリームが必要な経路に限定**。`waitForSelector` 等は REST long-polling で十分。

### Capability negotiation

すべての endpoint は `/test/info` の `capabilities` 配列で公開、未実装は **501 Not Implemented** を返却:

```json
{
  "instance_id": "01HW9...",
  "port": 19042,
  "app_name": "demo",
  "app_version": "0.1.0",
  "capabilities": [
    "elements", "tap", "swipe", "pinch", "type", "wait",
    "screenshot", "state", "events"
  ]
}
```

MVP で `eval` / `video_stream` は未実装で出し、必要が顕在化したら段階的に追加。

### インスタンス分離 (port + registry)

- server は **127.0.0.1 のみ bind**、port は **19000-19999 のランダム**
- 起動時に `$XDG_RUNTIME_DIR/gtk4-e2e/instance-${pid}.json` を書き出し、終了時に削除
- ファイル内容: `{ pid, port, app_name, app_version, started_at, token }`
- SDK は `E2EClient.discover()` で registry を読み列挙、複数あれば `app_name` / `pid` で絞込
- 認証は環境変数 `GTK4_E2E_TOKEN` (任意)

### Production 除外

- Cargo feature `e2e` は **default off**
- consumer app の本番ビルドは `--no-default-features` で server コードを完全除外
- → 本番バイナリにテスト API のコードが**1 byte も入らない**

### 動画録画

- `packages/client` の `recorder.ts` が `Bun.spawn(["ffmpeg", ...])` で X11/Wayland capture
- API: `await recorder.start({ windowId, output: "run.mp4", fps: 30 })` → `await recorder.stop()`
- EGLFS / KMS 環境は `kmsgrab` で対応 (Open Question)

## Consequences

### Positive

- **物理配置が機能制約と一致**: 内包必須なものだけ内包、それ以外は独立
- **型ずれなし**: Rust 起点 SSOT を schemars で JSON Schema 化 → TS 自動生成、drift 不可能
- **production 安全性**: feature flag で完全除外、本番に test API のコード混入なし
- **インスタンス並列**: 同一マシン上で複数インスタンス同時テストが容易
- **再利用可能性**: 任意の GTK4+Rust app に server crate を貼れる
- **demo による独立 develop**: consumer app 完成を待たず framework 単独で CI / regression 回せる
- **CLI 単一バイナリ**: `bun build --compile` で配布容易
- **debug 性**: REST 主体、`curl` で叩ける、ストリームは WebSocket と分離

### Negative / Trade-offs

- **言語境界の 2 package は確定的に必要**: Rust server と TS client を統合できる方法はない (構造的制約)
- **TS↔Rust の codegen pipeline 保守**: `build.rs` + JSON Schema → TS 変換が CI で stale check 必須、手書き禁止のルール徹底
- **server 実装は consumer app 寿命と密結合**: gtk4-rs 0.9 → 0.10 等の breaking change を直接被る
- **video 録画の EGLFS 対応未確定**: container 内 X11 で開発時は問題なし、kiosk モードでの録画は別解必要
- **触感系 (haptic / 圧力)** は GTK4 にネイティブな概念がなく対象外
- **MVP には `eval` 不実装**: 内部 state 操作は専用 endpoint を都度追加、ad-hoc になる可能性
- **複数言語 SDK が必要になった場合**: Rust SDK / Python SDK が後から欲しくなれば、JSON Schema からの codegen target を追加する形で対応

## Alternatives Considered

### 構造

| 選択肢 | 却下理由 |
|---|---|
| 全部 1 package に内包 | TS client を Rust app に統合不可、言語境界で物理的に分割必須 |
| server も別プロセス (sidecar) | gtk4-rs widget tree / event 合成が in-process でしか不可、致命的 |
| 7 パッケージ最大分割 | monorepo CI 7 倍、当面 use case なし、Playwright (1-2 package) と乖離 |
| `examples/` に demo を入れる | demo は永続的な regression test 用なので独立 package が自然 |

### SDK 言語

| 選択肢 | 却下理由 |
|---|---|
| Rust SDK | テスト書換ごとに compile 待ち、scenario verbose、テスト = scripting の一般則に逆行 |
| Python SDK | 多くの consumer 環境で primary stack 外、追加言語負担 |
| Lua DSL | Claude Code の Lua 生成精度低、エコシステム弱 |
| 複数言語同時提供 | 現時点で複数言語ニーズなし、保守コスト N 倍 |

### プロトコル

| 選択肢 | 却下理由 |
|---|---|
| WebDriver BiDi 準拠 | GTK4 用 BiDi server 一から自作必要、overkill |
| CDP (JSON-RPC over WS) のみ | curl debug 不可、SDK 書きにくい、protocol 設計コスト大 |

### 既存ツール

| 選択肢 | 却下理由 |
|---|---|
| Squish (Froglogic 商用) | $$$、GTK4 サポートなし |
| GTK Test (in-process unit test) | 外部からの操作不可 |
| xdotool / PyAutoGUI | widget 論理構造が取れない |

## Verification

### Phase 1 (本 ADR の妥当性検証)

- [ ] gtk4-rs の `gdk::EventController` 経路で touch event を合成できることを spike
- [ ] `gtk::WidgetExt::snapshot` (GSK Renderer) で容易に PNG 化できる手順を確認
- [ ] axum を gtk4-rs main loop と並行動作させる pattern を確立 (`tokio::spawn` + `glib::MainContext::default()` 混在)
- [ ] feature flag `e2e` での conditional compile を `cargo build --features e2e` で確認
- [ ] schemars + JSON Schema → TS 型生成の build.rs パイプラインが動く

### Phase 2 (MVP 完成基準)

- [ ] `packages/server` を `packages/demo` に組込み、外部 HTTP からテストボタンタップ
- [ ] 同一マシン上で 2 インスタンス起動、port 衝突せず両方独立に操作可能
- [ ] `screenshot` 出力の visual diff threshold で safelist 通過
- [ ] `wait` long-polling で `state_eq` 条件が 5 秒以内に発火
- [ ] WebSocket `/test/events` で `frame_ready` を 5 秒間購読し loss なし
- [ ] TS SDK 経由で 1 シナリオが `bun test` で完走
- [ ] Claude Code plugin (slash command + SKILL) 経由で同シナリオが起動できる
- [ ] `recorder.ts` で 30 秒の操作録画 → mp4 出力

## Open Questions

- [ ] **MVP capability 範囲の確定** — `pinch` を MVP に含めるか defer か
- [ ] **`eval` capability の安全性** — `mlua` 等で Lua sandbox を載せるか、専用 mutator endpoint で代替するか
- [ ] **EGLFS / KMS 環境での録画手段** — `kmsgrab` で frame 取れるか実機検証必要
- [ ] **selector 文法の決定** — CSS-like (`#id`, `.class`) / XPath / 独自 query DSL のどれか
- [ ] **state snapshot のスキーマ** — Rust struct → JSON 直列化時の private field の扱い
- [ ] **Cargo workspace 構成** — `[workspace] members = ["packages/*"]` で server / demo / build artifacts を統合
- [ ] **codegen ツール選定** — `schemars` + `json-schema-to-typescript` か、`typeshare` か、`ts-rs` か
- [ ] **codegen の trigger** — `cargo build` 内で完結 vs `bun run gen:types` で別タスク
- [ ] **将来の分割余地** — Python / Rust SDK ニーズが顕在化したら JSON Schema から codegen target 追加
- [ ] **plugin 名称** — 当面 `gtk4-e2e`、将来別フレームワークにも展開する場合 `native-e2e` への rename 余地
- [ ] **`bun build --compile` で CLI 単一バイナリ化するか** — kiosk 配布用途で必要なら採用

## References

- Playwright (https://playwright.dev/) — 機能要件のベンチマーク、1 package 構成の参考
- WebDriver BiDi (https://w3c.github.io/webdriver-bidi/) — protocol 設計の参考
- `schemars` (https://crates.io/crates/schemars) — Rust → JSON Schema 自動生成
- `json-schema-to-typescript` / `typeshare` / `ts-rs` — JSON Schema or Rust → TS 型生成 (codegen 候補)
- gtk4-rs (https://gtk-rs.org/gtk4-rs/) — GTK4 Rust bindings
- axum (https://docs.rs/axum/) — Rust HTTP framework, tokio エコシステム
