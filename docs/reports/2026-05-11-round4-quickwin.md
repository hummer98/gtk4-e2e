# Round 4 — Quick Win Cleanup

**作成日**: 2026-05-11
**対象**: ADR-0002 Accepted 昇格 + ADR 運用ルール追記 + 既存「1 fail」の素性確認
**前段**: [`2026-05-10-round3-visual-regression.md`](./2026-05-10-round3-visual-regression.md)

---

## A. 概要

Round 3 完遂後の区切り作業。Master 直接で 30 分以内に完遂できる以下 3 項目を 1 PR でクリーンアップ:

1. **ADR-0002 (codegen pipeline) を `Proposed → Accepted`** — Round 1 で実装、Round 2/3 で 6 タスク (type / swipe / pinch / elements / state / app_state_eq) が同パイプラインで成功実績、stale check も期待通り作動。Confidence 75% → 90% に引き上げ。
2. **`docs/adr/README.md` の運用ルールに Status 昇格手順を追記** — `Proposed → Accepted` / `Accepted → Superseded` / `Open Questions の解消` の 3 ルール。Round 3 で ADR-0003 を Master 直接で昇格した経験を制度化。
3. **既存「1 fail」の素性確認** — Round 2/3 で Conductor 申告に出続けた `wait.test.ts` の TS2322 / `app_state_eq` typo 系の fail は **types.gen.ts の再生成だけで解消**し、CI 上ではそもそも green。本番影響なし、record として確認のみ。

加えて Round 4 で発見した別 fail (Master local 環境のみ、`cli.test.ts:184`) は本ラウンドのスコープ外として §F に残置。

---

## B. 各項目の完了状況

| 項目 | 状態 | 備考 |
|------|------|------|
| ADR-0002 Status を Accepted に更新 | ✅ | Confidence も 75% → 90% に引き上げ |
| ADR README の table を Accepted 反映 | ✅ | |
| ADR README に Status 昇格手順追記 | ✅ | 4 + 5 番目のルールとして追加 |
| `wait.test.ts` の typo fail 確認 | ✅ | `bun packages/client/scripts/gen-types.ts` で types.gen.ts に `app_state_eq` が反映されると消える。CI は元々 green |
| Round 4 レポート作成 | ✅ | 本ファイル |
| `docs/reports/README.md` 索引更新 | ✅ | |
| `README.md` Status 行更新 | ✅ | Round 4 表記に |

---

## C. 解消された Round 3 §F の項目

| 項目 | 解消手段 |
|------|---------|
| ADR-0002 を `Accepted` 昇格 + ADR README に運用ルール追記 | ✅ Round 4 (本ラウンド) |
| 既存 1 fail (`wait.test.ts:131` の `"app_state_eq"` typo) | ✅ types.gen.ts 再生成で解消、CI 上は元々 green と判明 |

---

## D. 判断ポイント (Round 4)

- **Master 直接で 1 PR**: Round 3 完遂後の「区切り」を最速で打つため、quick win 3 件を Conductor 経由ではなく Master 直接で commit。明示指示「Quick win セットのみ」を踏まえた判断。
- **ADR-0002 Confidence を 75% → 90% に引き上げ**: Round 1 着手時の Confidence は「Step 4 着手時の使用感で再評価」と書かれていた。Round 3 完了時点で 6 タスクが同パイプラインで proto 拡張に成功した実績から再評価。`InstanceFile` の SSOT 化のみ未解決として残置 (Confidence 100% にしない理由)。
- **wait.test.ts の typo は実態として bug ではなかった**: Round 2/3 で Conductor が「既存 1 fail」と申告し続けたが、CI ログ上は通っていた。Master の local 環境で `bun install` / `gen:types` を回さずに `tsc --noEmit` を試した結果、生成物の遅れが原因と確定。**TS 生成パイプラインの依存関係**を `package.json` の `scripts.test` に embed することは別途検討余地あり (Round 5 候補)。
- **ADR 運用ルール 4「Status 昇格」の文書化**: Round 3 で ADR-0003 を Master 直接で昇格したが、その時点では運用ルールが未明文化だった。今後の Conductor / Master 双方が同一手順に従えるように明示。

---

## E. 残課題 (Round 5 以降)

### Round 4 で新規発見
- **`cli.test.ts:184` の macOS-only fail**: `XDG_RUNTIME_DIR` を honor せず `TMPDIR` fallback して残骸 instance を拾う環境依存 bug。test 側で macOS の registry path 計算を真似る (or `XDG_RUNTIME_DIR` を強制 honor) 修正が必要。CI (Linux) では通っているため緊急性は低い。
- **`bun test` 前に `gen-types.ts` を自動実行する仕組み**: 現状 `tsc --noEmit` を local で初めて回すときに「`bun packages/client/scripts/gen-types.ts` を先に実行」を README に書いているが、`package.json` の `scripts.test` で前段に組み込めば手動忘れがなくなる。

### Round 3 から継続
- **Brainship 等 consumer への接続** — framework dogfooding
- **`POST /test/eval` capability** — `mlua` sandbox / 専用 mutator endpoint の決定 (ADR 化)
- **`InstanceFile` の SSOT 化** — ADR-0002 Open Question の最後の残り
- **`WS /test/video/stream` capability**
- **複数言語 SDK (Python / Rust)**
- **schemars 0.8 → 1.0 移行**
- **macOS / Wayland 録画 backend**
- **`plugin.json` filename** に合わせて `seed.md §5` の `manifest.json` 記述更新
- **swipe / pinch のハードウェア touch event 合成** (現状は ScrolledWindow / GestureZoom 経由の代替)
- **visual regression の region mask / per-CI-image rebaseline workflow** (Round 3 の 2% 緩和の置き換え)

---

## F. メトリクス

| 指標 | Round 3 完了時 | Round 4 完了時 |
|------|----------------|----------------|
| ADR | 0001 (Accepted), 0002 (Proposed), 0003 (Accepted) | 0001 (Accepted), **0002 (Accepted)**, 0003 (Accepted) |
| ADR 運用ルール | 3 項目 | 5 項目 (Status 昇格 + Open Questions 解消) |
| クローズ済タスク | 25 | 25 (Round 4 はタスク化せず Master 直接) |
| 残報告書（Round 5 候補項目） | 12 件 | 14 件 (Round 4 で 2 件新規発見、2 件解消差引で +2) |

---

## G. ファイル変更サマリー

Round 4 の commit (Master 直接、1 PR 想定):

```
docs/adr/0002-codegen-pipeline.md          Status / Confidence 更新
docs/adr/README.md                          0002 行を Accepted に + 運用ルール 4,5 追記
docs/reports/2026-05-11-round4-quickwin.md  新規 (本ファイル)
docs/reports/README.md                      Round 4 行追加
README.md                                   Status 行を Round 4 表記に
```

---

## H. 関連ドキュメント

- [`2026-05-10-round3-visual-regression.md`](./2026-05-10-round3-visual-regression.md) — Round 3
- [`2026-05-10-round2-stabilization.md`](./2026-05-10-round2-stabilization.md) — Round 2
- [`2026-05-10-bootstrap.md`](./2026-05-10-bootstrap.md) — Round 1
- [`README.md`](./README.md) — 本ディレクトリ索引
- [`../adr/0002-codegen-pipeline.md`](../adr/0002-codegen-pipeline.md) (Accepted)
- [`../adr/README.md`](../adr/README.md) — 運用ルール
