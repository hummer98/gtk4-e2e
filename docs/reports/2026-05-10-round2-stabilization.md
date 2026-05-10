# Round 2 — Stabilization & Step 9 Capability Expansion

**作成日**: 2026-05-10
**対象タスク**: T013〜T019 (closed) + T020 (draft 温存)
**前段**: [`2026-05-10-bootstrap.md`](./2026-05-10-bootstrap.md) (T001〜T012 / Step 0〜11)

---

## A. 概要

Bootstrap (Round 1) 完遂後の周辺整備ラウンド。bootstrap report §E "残課題・既知の制約" / §G "次にやるべきこと" の短期項目を網羅的に消化し、Step 9 の 3 つの capability 拡張も同ラウンドで完走。`T020` (visual regression diff) のみユーザー判断で `low priority + draft` 温存。

---

## B. 各タスクの完了状況

| ID | タイトル | 主な成果 | 検証 |
|----|---------|---------|------|
| **T013** | Step 9(a) type capability | `POST /test/type` + SDK + CLI + demo scenario、`Capability::Type` 末尾追加 | cargo 91 / bun 94 |
| **T014** | Step 9(b) swipe capability | `POST /test/swipe`、ScrolledWindow vadj/hadj 線形アニメで代替実装 | cargo 94 / bun 104 |
| **T015** | Step 9(c) pinch capability | `POST /test/pinch`、`GestureZoom::scale-changed` の線形補間多段 emit | cargo 200+ / bun 132 / tsc 0 |
| **T016** | record-demo CI job 削除 | 動画はローカル運用 (`record-run.sh`) に降格、3 files +32/-58 | ci.yml は rust/bun/scenarios の 3 job のみ |
| **T017** | Biome 導入 + tsc --noEmit baseline 解消 + CI 連携 | `biome.json` 追加、lint/fmt:check 実体化、TS2322 baseline 解消 (server.port! narrowing)、bun job に fmt:check / tsc --noEmit step 追加 | Inspector GO |
| **T018** | `/test/elements` widget tree query | proto/tree/http/SDK/CLI/demo/tests 一式、`Capability::Elements` 追加 | cargo 113 / bun 104 / scenarios 15 |
| **T019** | T006 申し送り総括 | A: `/test/state` + state push、B: Activatable/Switch/CheckButton tap 拡張、C: `real_widget_visible_after_present` integration test、D: `cli.ts` exec bit 決着 | cargo 135 / bun 108 / 0 error |
| T020 | visual regression diff (pixel diff/SSIM) | **draft 温存** (ユーザー判断) | — |

---

## C. Bootstrap Report §E の解消状況

| 項目 | bootstrap 時点 | Round 2 後 |
|------|---------------|-----------|
| MVP capability の未実装 (type/swipe/pinch) | T013/T014/T015 が draft | ✅ 全 closed |
| `/test/state` endpoint と app-defined state schema | 未実装 | ✅ T019-A |
| `/test/elements` widget tree query | 未実装 | ✅ T018 |
| Activatable / Switch / CheckButton tap 対応 | Button のみ | ✅ T019-B |
| `real_widget_visible_after_present` integration test (Open Q-K) | 未着手 | ✅ T019-C |
| `packages/server/src/cli.ts` の executable bit | `100755` のまま保留 | ✅ T019-D で決着 |
| Biome 未導入 (`bun run lint` / `fmt:check` が `exit 0` placeholder) | placeholder | ✅ T017 で実体化 |
| `tsc --noEmit` を CI に未連携 (TS2322 baseline 残存) | baseline 残存 | ✅ T017 で解消 + CI 連携 |
| `record-demo` CI コスト | PR + push 双方で約 2 分 | ✅ T016 で job 削除 (ローカル運用に降格) |
| Visual regression diff (pixel diff / SSIM) | T007 申し送り、未配線 | ⏸ T020 として draft 温存 (ユーザー判断: low priority) |
| `eval` (`POST /test/eval`) | optional / future | ⏸ ADR 化が前提、未着手 |
| `Capability::VideoStream` (`WS /test/video/stream`) | 未実装 | ⏸ 必要顕在化後 |
| `InstanceFile` の SSOT 化 (TS と Rust で別記述) | ADR-0002 Open Question | ⏸ Round 3 候補 |
| EGLFS / KMS 環境 (`kmsgrab`) 録画 | ADR-0001 Open Question | ⏸ long-term |
| macOS / Wayland 録画 backend | exit 6 で skip | ⏸ long-term |
| ADR-0002 の `Accepted` 昇格 | `Proposed` のまま | ⏸ Round 3 候補 |

---

## D. 判断ポイント (Round 2)

- **T014 swipe 実装方針**: gtk4-rs API の制約で連続 motion event 合成不可 → ScrolledWindow `vadj` / `hadj` の線形アニメで代替。consumer 用途の中心である「スクロール検証」には十分だが、ポインタドラッグ系 widget (ListView の reorder, DrawingArea 上の selection 等) の swipe は別設計が必要 (Round 3 以降の課題)。
- **T015 pinch 実装方針**: ハードウェア touch event 合成は同様に不可 → `GestureZoom::scale-changed` の線形補間多段 emit で代替。`gtk::ScrolledWindow` の zoom や `gtk::Picture` の scale テストには有効。
- **rebase conflict の semantic resolution**: T013/T014/T015/T017/T018/T019 間で 8〜十数ファイルの conflict が発生したが、すべて (1)`Capability` enum 末尾追加 (append-only 規律) と (2)Biome formatter の import 整列 由来。両ルールの組み合わせで Conductor 側で機械的に resolve 可能だった。
- **T019 を分割せず巨大タスクで起票**: タスク本文に「Conductor 判断で分割可」と明記したが、Conductor は分割せず A〜D を一体実装。結果 cargo 135 / bun 108 全 pass で完走、T006 申し送りを 1 PR で総括できた。
- **T020 を draft + low で温存**: ユーザー判断「pixeldiff はあまり使わない」を反映。起票だけは済ませており、consumer ニーズが顕在化したら `elevens update-task --task-id 020 --status ready` で即昇格可能。
- **T016 で CI 動画ジョブ撤回**: bootstrap report §F「録画保存方法: GH Actions artifact 採用」をユーザー判断で撤回、ローカル運用に降格。`record-run.sh` はそのまま残置。
- **`.team/` の git 扱い**: 未だ untracked。bootstrap 時の判断「`.team/` を main に push しない」を継続中。

---

## E. 残課題 (Round 3 以降)

### draft 温存
- **T020 visual regression diff** — consumer ニーズ顕在化後に ready 化

### bootstrap report §G の未対応項目
- **`POST /test/eval` capability** — `mlua` sandbox / 専用 mutator endpoint の決定 (ADR 化)
- **`WS /test/video/stream` capability** — kmsgrab / EGLFS 環境向け
- **`InstanceFile` の SSOT 化** — ADR-0002 Open Question 解消
- **複数言語 SDK (Python / Rust)** — JSON Schema 経由で codegen target 追加
- **schemars 0.8 → 1.0 移行** (draft-2020-12 出力)
- **macOS / Wayland 録画 backend**
- **ADR-0002 を `Accepted` 昇格** + ADR README に運用ルール追記
- **`plugin.json` filename** に合わせて `seed.md §5` の `manifest.json` 記述更新
- **Brainship 等 consumer への接続** (別 repo / 別運用)
- **swipe / pinch のハードウェア touch event 合成** — 現状は ScrolledWindow / GestureZoom 経由の代替実装

---

## F. メトリクス

| 指標 | bootstrap 完了時 | Round 2 完了時 |
|------|-----------------|----------------|
| cargo test | 80 (T008 時点) | 200+ (T015 時点) |
| bun test | 81 (T009 時点) | 132 (T015 時点) |
| scenarios | 2 (display-bound) | 15+ (T018) |
| `Capability` enum メンバ | 5 (Info, Tap, Wait, Screenshot, Events) | 10 (+ Type, Swipe, Pinch, Elements, State) |
| CI job 数 | 4 (rust/bun/scenarios/record-demo) | 3 (rust/bun/scenarios) |
| TS error baseline | 2 件 (TS2322 × 2) | 0 |

---

## G. ファイル変更サマリー

Round 2 で local main に積み上げた commit (T013〜T019 + 関連 fix):

```
bcabc00 feat(pinch): pinch capability — POST /test/pinch (T015)
6735eab feat(server,client,demo): T006 申し送り総括 (T019)
b269576 feat(elements): widget tree query endpoint — GET /test/elements (T018)
b9e435f feat(tooling): Biome 導入 + tsc --noEmit baseline 解消 + CI 連携 (T017)
db430e9 test(http_routes): include swipe in type_capability_in_info expectation
b91d718 feat(swipe): swipe capability — POST /test/swipe (T014)
77afa84 chore(ci): drop record-demo job; demote demo recording to local-only (T016)
0d4cb88 feat(server,client,demo): add type capability — POST /test/type (T013)
```

---

## H. 関連ドキュメント

- [`2026-05-10-bootstrap.md`](./2026-05-10-bootstrap.md) — Round 1 (T001〜T012 / Step 0〜11)
- [`README.md`](./README.md) — 本ディレクトリの索引
- [`../seed.md`](../seed.md) — 初期スキャフォールディング指示
- [`../adr/0001-architecture.md`](../adr/0001-architecture.md)
- [`../adr/0002-codegen-pipeline.md`](../adr/0002-codegen-pipeline.md) (Proposed)
