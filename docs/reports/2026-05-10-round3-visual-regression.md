# Round 3 — Visual Regression Diff (ADR-0003 + T020-A〜E)

**作成日**: 2026-05-10
**対象タスク**: T020 (ADR 草案), T021〜T025 (T020-A〜E 実装) — 全 closed
**前段**: [`2026-05-10-round2-stabilization.md`](./2026-05-10-round2-stabilization.md)

---

## A. 概要

Round 2 完遂後、ユーザー判断で `T020` (visual regression diff) を `low priority + draft` から **`ready` に昇格**。Conductor は最初の T020 run で「draft 温存方針継続」と解釈してフル実装せず ADR-0003 草案 + subtask 分割計画 (T020-A〜F) のみ納品。Master が ADR-0003 を `docs/adr/` に正式採用 + subtask T021〜T025 を ready 起票することで Round 3 が正式始動。

最終的に **pixelmatch + pngjs ベースの SDK + CLI + CI + demo scenarios + baseline 規約** が一気通貫で整い、ADR-0003 は `Accepted` に昇格。

---

## B. 各タスクの完了状況

| ID | タイトル | 主な成果 |
|----|---------|---------|
| **T020** | visual regression diff — engine selection | ADR-0003 草案 (pixelmatch 推奨)、subtask 分割計画 |
| **T021** (T020-A) | SDK `expectScreenshot` 実装 + unit test | pure function + Client method 二段、`VisualDiffError` 追加、unit test 8 件 green |
| **T022** (T020-B) | baseline storage 規約 + 初回挙動 | `<scenario_basename>-<name>.png` 規約、Playwright 同等 auto-save (CI=true 時 fail)、rev2 で T020-C 互換 fix |
| **T023** (T020-C) | CLI `screenshot --baseline` フラグ | `<name> --baseline <path>` ディスパッチ、`--threshold` / `--update-baseline`、exit code 0/1/2/5/7、test 18 件 |
| **T024** (T020-D) | CI 連携 + diff artifact | scenarios job に `visual-regression-diff` artifact upload step + env `CI=true` 追加 |
| **T025** (T020-E) | demo scenarios visual regression assertion | `visual-regression.spec.ts` + Linux+xvfb baseline (16757B / 360x700) + `gen-visual-baseline.sh` 補助スクリプト |

---

## C. 解消された Bootstrap §E / Round 2 §E 項目

| 項目 | 解消手段 |
|------|---------|
| **Visual regression diff (T007 申し送り)**: pixel diff / SSIM、baseline 管理 | ✅ T020〜T025 で完全実装、ADR-0003 Accepted |

---

## D. 判断ポイント (Round 3)

- **Conductor の draft シグナル解釈** (T020 1st run): Conductor は親タスク本文の `priority: low` + `status: draft 由来` + 「着手時に再分割推奨」二重明示から「フル実装せず ADR + 分割計画」とスコープを絞った。これは妥当な判断だが、ユーザー意図 (`ready 化 = 実装してほしい`) との解釈ギャップが発生した。
- **Master の介入**: ADR 草案を `docs/adr/0003-visual-regression-engine.md` として正式採用 + subtask T021〜T025 (T020-A〜E) を ready 起票で実装続行。T020-F (ADR Accepted 昇格) は別タスク化せず、本ラウンド完遂時に Master 直接で実施。
- **engine 選定 (ADR-0003)**: **pixelmatch** 採用。代替案不採用理由:
  - **odiff** (Reason ML): postinstall 必要 → Bun の `trustedDependencies` 強要が SDK consumer 側に伝播するリスク
  - **dssim** (SSIM ベース): AGPL-3.0 → SDK + Claude plugin の配布物に AGPL を持ち込めない
  - **image-diff**: uber-archive で deprecated、公式が pixelmatch を推奨
- **PNG decode**: `pngjs` (純 JS、Bun で postinstall 不要)
- **baseline storage 規約** (T022): `<scenario_dir>/__screenshots__/<scenario_basename>-<name>.png`、ユーザー override (env / opts.baselineDir) も許容
- **初回挙動** (T022): non-CI で auto-save (Playwright 同等)、`CI=true` で fail (CI で不意に baseline を書かない)
- **CLI 仕様** (T023): exit code は既存規約 (0=ok / 1=mismatch / 2=argv / 5=HttpError) + 新設 **7=VisualDiffError** (baseline missing 等)
- **T025 の `task_aborted`**: Manager 再起動 (PID 10152→22034) のタイミングで `resume_no_session_id` で abort。`elevens restart-task --task-id 025` (専用 CLI、`update-task --status ready` ではなく) で再 ready 化 → Conductor が preserved `plan.md` を引き継いで完走。Master が初回 `update-task --status ready` を試したが反映されず、`restart-task` が aborted → ready 専用と判明。

---

## E. ADR-0003 Open Questions の決定状況

ADR 本文には 6 件の Open Questions が残っていたが、本ラウンドで以下のとおり決定:

| # | Open Question | 決定 |
|---|---|------|
| 1 | SDK API 命名 | `expectScreenshot(name, opts?)` (Playwright 流) |
| 2 | baseline 格納先 | `__screenshots__/<scenario_basename>-<name>.png` (T022) |
| 3 | threshold default | `0.1` (pixelmatch native) |
| 4 | 初回挙動 | non-CI: auto-save / CI: fail (T022) |
| 5 | AA 対策 | `opts.includeAA` (default false) — pixelmatch native option (T021) |
| 6 | CI artifact 保持期間 | 7 days (`actions/upload-artifact@v4`, T024) |

---

## F. 残課題 (Round 4 以降)

Round 2 §E の残項目から visual regression を除いたもの:

- **`POST /test/eval` capability** — `mlua` sandbox / 専用 mutator endpoint の決定 (ADR 化)
- **`WS /test/video/stream` capability** — kmsgrab / EGLFS 環境向け
- **`InstanceFile` の SSOT 化** — ADR-0002 Open Question 解消
- **複数言語 SDK (Python / Rust)** — JSON Schema 経由で codegen
- **schemars 0.8 → 1.0 移行**
- **macOS / Wayland 録画 backend**
- **ADR-0002 を `Accepted` 昇格** + ADR README に運用ルール追記
- **`plugin.json` filename** に合わせて `seed.md §5` の `manifest.json` 記述更新
- **Brainship 等 consumer への接続** (別 repo / 別運用)
- **swipe / pinch のハードウェア touch event 合成** — 現状は ScrolledWindow / GestureZoom 経由の代替実装

---

## G. メトリクス

| 指標 | Round 2 完了時 | Round 3 完了時 |
|------|----------------|----------------|
| クローズ済タスク | 19 | 25 |
| ADR | 0001 (Accepted), 0002 (Proposed) | + **0003 (Accepted)** |
| `Capability` enum メンバ | 10 | 10 (visual diff は SDK 層、proto には乗らない) |
| TypeScript test 件数 | 132 (T015 時点) | 161+ (T022 申告) |
| CI artifact | (なし) | `visual-regression-diff` (失敗時のみ、retention 7 日) |

---

## H. ファイル変更サマリー

Round 3 で main に積んだ commit (6 + 整備 ?):

```
61a59bf feat(demo): visual regression scenario with committed baseline (T020-E)
c00aac4 feat(ci): upload visual regression diff as artifact on scenarios fail (T020-D)
bef7bb2 fix(client): opts.baselineDir 明示時は scenario prefix skip + CLI は failOnMissing 強制 (T020-B rev2)
0110120 feat(client): baseline storage 規約 + 初回挙動 (T020-B)
9b494f1 feat(client): CLI screenshot --baseline flag for visual diff (T020-C)
02bb199 feat(client): expectScreenshot SDK API on pixelmatch + pngjs (T020-A)
```

加えて Round 3 整備として:
- `docs/adr/0003-visual-regression-engine.md` を Master が `.team/artifacts/A001-research.md` から抽出して採用 (Round 3 開始時)
- ADR-0003 を `Accepted` に昇格 (Round 3 完遂時、本ファイルと同一 commit)

---

## I. 関連ドキュメント

- [`2026-05-10-round2-stabilization.md`](./2026-05-10-round2-stabilization.md) — Round 2 (T013〜T019)
- [`2026-05-10-bootstrap.md`](./2026-05-10-bootstrap.md) — Round 1 (T001〜T012)
- [`README.md`](./README.md) — 本ディレクトリ索引
- [`../adr/0003-visual-regression-engine.md`](../adr/0003-visual-regression-engine.md) — engine 選定 ADR (Accepted)
- [`../seed.md`](../seed.md)
