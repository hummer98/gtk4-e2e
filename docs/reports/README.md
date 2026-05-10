# Project Status Reports

完了したラウンドごとの status report を時系列で集約。各ファイルは `YYYY-MM-DD-<概要>.md` 形式で命名。

## Index (新しい順)

| 日付 | レポート | 対象 | 主な成果 |
|------|---------|------|---------|
| 2026-05-11 | [Round 4 — Quick Win Cleanup](./2026-05-11-round4-quickwin.md) | ADR-0002 + ADR README | ADR-0002 を Accepted 昇格、ADR 運用ルールに Status 昇格手順追記、既存 1 fail (wait.test.ts) の素性確認 |
| 2026-05-10 | [Round 3 — Visual Regression Diff](./2026-05-10-round3-visual-regression.md) | T020 (ADR), T021〜T025 (T020-A〜E) | ADR-0003 採用 (pixelmatch)、SDK `expectScreenshot`、CLI `screenshot --baseline`、baseline storage 規約、CI diff artifact、demo visual-regression scenario |
| 2026-05-10 | [Round 2 — Stabilization](./2026-05-10-round2-stabilization.md) | T013〜T019 + T020 (draft) | type/swipe/pinch capability、`/test/elements`、`/test/state`、Biome+tsc CI 連携、record-demo CI 削除、T006 申し送り総括 |
| 2026-05-10 | [Round 1 — Bootstrap](./2026-05-10-bootstrap.md) | T001〜T012 (Step 0〜11) | workspace + CI、in-process server、demo、codegen、TS SDK、tap/wait、snapshot、events/WS、recorder + Claude plugin、録画ラン、最終レポート |

## 命名規約

- ファイル名: `YYYY-MM-DD-<概要>.md` (kebab-case の概要)
- 同日複数ラウンドあり得るため、概要で区別 (`-bootstrap`, `-round2-stabilization` 等)
- 内容は **その時点での snapshot**。後追いで更新せず、新ラウンドは新ファイルで追記
