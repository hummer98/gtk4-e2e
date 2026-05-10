---
name: gtk4-e2e
description: GTK4 + Rust アプリの e2e 自動操作・録画・scenario 実行スキル。Triggers - 「gtk4-e2e」「demo を tap」「画面を録画」「scenario を流す」「e2e で wait」「screenshot を保存」等の発言、または $XDG_RUNTIME_DIR/gtk4-e2e/instance-*.json が存在する前提の作業時。非対応 - Wayland 上での録画 (MVP では X11 のみ) / macOS でのキャプチャ。
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
- `bunx gtk4-e2e info | tap | screenshot | wait | record (start|stop|status) | events`
  を呼ぶとき

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

### screenshot

```bash
bunx gtk4-e2e screenshot /tmp/now.png
```

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
| 0 | 成功 | — |
| 2 | argv エラー (missing flag / unknown subcommand) | usage を確認 |
| 3 | NotImplementedError (server が capability を持たない / 501) | server 側 build feature を確認 |
| 4 | DiscoveryError (起動中の instance が見つからない) | `cargo run -p gtk4-e2e-demo --features e2e` を案内 |
| 5 | HttpError (例: selector_not_found / 404 / 4xx-5xx) | エラー本文を読み selector を訂正 |
| 6 | RecorderError (ffmpeg 未インストール / 既に録画中 / Wayland / DISPLAY なし) | エラーメッセージ参照 |

## 注意

- 本 SKILL は「CLI を呼ぶ薄い shell」に徹する。recorder の state は PID file
  (`$XDG_RUNTIME_DIR/gtk4-e2e/recorder.json`) で管理されており、Claude が
  独自にプロセスを kill するのは避けること (常に `record stop` を経由する)。
- token が必要な場合は `GTK4_E2E_TOKEN=...` を env で渡すか `--token` を使う。
- 録画の出力先は親ディレクトリが事前に存在している必要がある (recorder は
  自動 `mkdir -p` しない)。
- Wayland セッション (`$WAYLAND_DISPLAY` が set) で `record start` すると
  即時 exit 6。X11 を立てるか、Xwayland 経由で `$DISPLAY` を見せること。
