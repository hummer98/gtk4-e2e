# Architecture Decision Records (ADR)

`gtk4-e2e` の主要意思決定を記録する。

## Status の意味

| Status | 意味 |
|---|---|
| **Proposed** | 提案段階。実装または spike で確定予定 |
| **Accepted** | 確定。実装フェーズへ |
| **Deferred** | 採用は将来再検討 |
| **Superseded by ADR-XXXX** | 後続 ADR で置き換え |
| **Deprecated** | 不採用 |

## ADR 一覧

| # | タイトル | Status | 信頼度 |
|---|---|---|---|
| [0001](./0001-architecture.md) | Architecture — In-process Rust server + Bun/TypeScript client + demo app | Accepted | 中 (70%) |
| [0002](./0002-codegen-pipeline.md) | Codegen pipeline — schemars → JSON Schema (committed) → json-schema-to-typescript | Proposed | 中 (75%) |
| [0003](./0003-visual-regression-engine.md) | Visual regression diff — engine selection (pixelmatch) | Accepted | 中〜高 (75%) |

## ADR 運用ルール

1. **新規 ADR**: 番号は連番、ファイル名は `NNNN-kebab-case-title.md`
2. **既存 ADR の修正**: 軽微な誤記訂正のみ。本質的な変更は新規 ADR で Supersede
3. **テンプレート**: `0001-architecture.md` を参考に新規作成

## 関連

- [`docs/seed.md`](../seed.md) — Claude Code 初期指示 (bootstrap 用)
- [README.md](../../README.md) — プロジェクト概要
