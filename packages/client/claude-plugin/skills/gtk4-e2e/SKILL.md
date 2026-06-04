---
name: gtk4-e2e
description: GTK4 + Rust アプリの e2e 自動操作・録画・scenario 実行スキル。Triggers - 「gtk4-e2e」「demo を tap」「画面を録画」「scenario を流す」「e2e で wait」「event を観測する」「screenshot を保存」「widget の property を読む」「elements ツリーを取る」等の発言、または $XDG_RUNTIME_DIR/gtk4-e2e/instance-*.json が存在する前提の作業時。非対応 - Wayland 上での録画 (MVP では X11 のみ) / macOS でのキャプチャ。
---

# gtk4-e2e: GTK4+Rust e2e 操作スキル

Claude Code 上で `bunx gtk4-e2e` を呼んで GTK4 アプリを操作するためのスキル。
SDK は client-side で完結し、`packages/server` (Rust) が公開する
`/test/*` HTTP/WebSocket エンドポイントを叩く。

## いつ使うか

- ユーザーが「demo を起動して #btn1 を tap して」「録画しながら scenario を流して」
  など、実機 GUI への自動操作を求めたとき
- `$XDG_RUNTIME_DIR/gtk4-e2e/instance-*.json` (Linux) /
  `$TMPDIR/gtk4-e2e/instance-*.json` (macOS) から利用可能な instance を選ぶとき
- `bunx gtk4-e2e info | tap | type | swipe | pinch | screenshot | elements | wait | events | record (start|stop|status)`
  を呼ぶとき
- widget の現在値 (`Entry.text` / `Switch.active` 等) を読み取って assertion
  したい / アプリの現状を把握したいとき (→ `elements --props ...`)
- screenshot をベースラインと突き合わせて視覚回帰を検出したいとき
  (→ `screenshot <name> --baseline ...`)
- 状態が整うまで待ってから次の操作をしたいとき (→ `wait ...`)、
  または `state_change` などの event を観測したいとき (→ `events ...`)

## トリガー条件 (frontmatter description と重複)

- 「gtk4-e2e」「e2e」「scenario」「tap」「record」「screenshot」を含む発言
- demo / consumer app が起動している前提で「画面を確認して」「ボタンを押して」という指示
- registry ファイル (`instance-*.json`) を直接読まずに高レベルで操作したい場合

## 典型コマンド集

### instance 探索

```bash
bunx gtk4-e2e info
```

### tap

```bash
# selector
bunx gtk4-e2e tap "#submit"
# coords (window-local)
bunx gtk4-e2e tap 100,200
```

### type / swipe / pinch

```bash
# selector に文字入力
bunx gtk4-e2e type "#entry1" "hello"
# swipe (window-local 座標, 既定 duration 300ms / --duration で変更)
bunx gtk4-e2e swipe 100,400 100,100
bunx gtk4-e2e swipe 100,400 100,100 --duration 500
# pinch (中心座標 + scale, 既定 duration 300ms)
bunx gtk4-e2e pinch 200,200 2.0
```

### screenshot (保存)

```bash
bunx gtk4-e2e screenshot /tmp/now.png
```

### screenshot (視覚回帰 diff)

```bash
# <name> を baseline ディレクトリ内の基準画像と突き合わせる。
# --baseline はディレクトリ指定 (basename は無視される)。
bunx gtk4-e2e screenshot home --baseline packages/demo/baselines/
# 一致許容率を変える (0.0-1.0)
bunx gtk4-e2e screenshot home --baseline packages/demo/baselines/ --threshold 0.02
# 基準画像を意図的に作成 / 更新する (--baseline 必須)
bunx gtk4-e2e screenshot home --baseline packages/demo/baselines/ --update-baseline
```

diff モードでは **一致=exit 0 / 不一致=exit 1**。baseline 不在は
`--update-baseline` を付けない限り **exit 7 (VisualDiffError)**。結果 JSON
(`{ name, match, ... }`) を stdout に出すので `jq` で判定できる。

### elements (widget tree query)

```bash
# 全ウィンドウのツリーを JSON で
bunx gtk4-e2e elements

# 個別 widget に絞る
bunx gtk4-e2e elements --selector "#entry1"

# 各ノードの GObject property を opt-in で取得 (カンマ区切り)
bunx gtk4-e2e elements --selector "#entry1" --props text,placeholder-text

# 当該 widget が公開する全 readable property をダンプ
bunx gtk4-e2e elements --selector "#entry1" --props '*'
```

`--props` で取れる値の型は MVP で `String / bool / i32 / f64`。それ以外
(`GdkRGBA` 等 boxed や enum / flags) は `{"$unsupported": "GTypeName"}`、
そもそも widget が公開していない property は `{"$missing": true}` という
sentinel で返るので、`jq` で振り分けられる。GTK4 が public API を持たない
CSS computed style と state flags は取れない (`css_classes` までは出る)。

```bash
# 例: Entry.text が "" でないことを確認
bunx gtk4-e2e elements --selector "#entry1" --props text \
  | jq -e '.roots[0].properties.text != ""'
```

### wait (条件が整うまで long-poll)

```bash
# selector が可視になるまで待つ
bunx gtk4-e2e wait visible "#dialog"
# widget の property が指定値になるまで待つ (value は JSON、失敗時は文字列)
bunx gtk4-e2e wait state-eq "#sw" active true
bunx gtk4-e2e wait state-eq "#entry1" text "done"
# app が push した state (JSON Pointer) が指定値になるまで待つ
bunx gtk4-e2e wait app-state-eq /counter 42
# deadline は --timeout ms (既定 5000)。超過すると exit 8
bunx gtk4-e2e wait visible "#slow" --timeout 10000
```

成功時は `{ "elapsed_ms": N }` を stdout。条件未達でタイムアウト (HTTP 408)
は **exit 8 (WaitTimeoutError)**。

### events (WS /test/events を購読 → NDJSON)

```bash
# 次の 1 件を受け取って終了 (jq で取り出す)
bunx gtk4-e2e events --count 1 | jq -c .
# state_change だけを最大 5 件
bunx gtk4-e2e events --kinds state_change --count 5
# 3 秒間だけ観測 (--timeout 経過で exit 0)
bunx gtk4-e2e events --timeout 3000
```

各 event は **1 行 1 JSON (NDJSON)** で逐次出力。終了条件は `--count N` 到達 /
`--timeout ms` 経過 / SIGINT のいずれかで、いずれも exit 0。接続失敗・再接続
上限超過は **exit 9 (EventStreamError)**。`--count` も `--timeout` も付けない
場合は kill されるまでストリームし続ける。

### record (X11 only / MVP)

```bash
bunx gtk4-e2e record start --output /tmp/run.mp4
# ... ユーザー操作 / scenario 実行 ...
bunx gtk4-e2e record stop
bunx gtk4-e2e record status
```

### scenario 実行 (bun test)

```bash
bun test packages/demo/scenarios/tap.spec.ts
```

## 失敗時の挙動 (CLI 終了コード)

| code | 意味 | 対処 |
|------|------|------|
| 0 | 成功 (diff モードは「一致」も含む) | — |
| 1 | 予期しないエラー (E2EError) / screenshot diff の **不一致** | 本文を確認 (diff 不一致は意図通りなら無視可) |
| 2 | argv エラー (missing flag / unknown subcommand) | usage を確認 |
| 3 | NotImplementedError (server が capability を持たない / 501) | server 側 build feature を確認 |
| 4 | DiscoveryError (起動中の instance が見つからない) | `cargo run -p gtk4-e2e-demo --features e2e` を案内 |
| 5 | HttpError (例: selector_not_found / 404 / 4xx-5xx) | エラー本文を読み selector を訂正 |
| 6 | RecorderError (ffmpeg 未インストール / 既に録画中 / Wayland / DISPLAY なし) | エラーメッセージ参照 |
| 7 | VisualDiffError (baseline_missing / decode_failed) | `--update-baseline` で基準作成、または PNG を確認 |
| 8 | WaitTimeoutError (wait: 条件未達で deadline 超過 / HTTP 408) | `--timeout` を延ばす、または条件/selector を見直す |
| 9 | EventStreamError (events: 接続失敗 / 再接続上限超過) | instance 稼働と capability を確認 |

## 注意

- 本 SKILL は「CLI を呼ぶ薄い shell」に徹する。recorder の state は PID file
  (`$XDG_RUNTIME_DIR/gtk4-e2e/recorder.json`) で管理されており、Claude が
  独自にプロセスを kill するのは避けること (常に `record stop` を経由する)。
- token が必要な場合は `GTK4_E2E_TOKEN=...` を env で渡すか `--token` を使う。
- 録画の出力先は親ディレクトリが事前に存在している必要がある (recorder は
  自動 `mkdir -p` しない)。
- Wayland セッション (`$WAYLAND_DISPLAY` が set) で `record start` すると
  即時 exit 6。X11 を立てるか、Xwayland 経由で `$DISPLAY` を見せること。
