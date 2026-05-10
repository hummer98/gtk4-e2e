# ADR-0002: Codegen pipeline — schemars → JSON Schema (committed) → json-schema-to-typescript

- **Status**: Accepted (2026-05-11, after Round 1–3 production use across T004 / T013 / T014 / T015 / T018 / T019)
- **Date**: 2026-05-10
- **Confidence**: 高 (90%) — Round 1 で実装後、Round 2/3 で 6 タスク (type / swipe / pinch / elements / state / app_state_eq) が同パイプラインで proto 拡張に成功、stale check も期待通り作動。`InstanceFile` の SSOT 化のみ未解決として残置。

## Context

ADR-0001 §langmap は「TS 型は Rust から自動生成、手書き禁止 (`build.rs` で〜)」を確定している。Step 1 で `packages/server/src/proto.rs` の `Info` / `Capability` に `JsonSchema` derive 済みだが、schema 出力経路も TS 生成経路もまだ存在しない (ルート `Taskfile.yml` の `gen:types` task は `cargo build` を呼ぶだけのスタブ)。

Step 4 で `packages/client` が SDK 実装に着手する前に、proto.rs を SSOT として TS 型を機械生成する pipeline を成立させる必要がある。

加えて、現 `.gitignore` が `**/*.gen.ts` と `packages/server/proto/schemas/` の双方を ignore しているため、`git diff --exit-code` ベースの stale 検出が**自動的に常に空 diff になる**問題があり、CI の「proto.rs を変えたら必ず fail」という規律を成立させる構造を選び直す必要がある。

## Decision

### D1. TS 型生成ツール: `json-schema-to-typescript` (npm/bun)

- schemars 0.8 の draft-07 JSON Schema をそのまま入力にできる。
- Rust toolchain を呼ばずに TS 生成完結 (= bun job 単独で smoke を回せる)。
- 将来 OpenAPI / AsyncAPI / 他言語 SDK へ展開する際、JSON Schema を共通中間表現として転用できる。
- 依存数最小 (`@apidevtools/json-schema-ref-parser` 系のみ)。

### D2. Schema 出力: `examples/gen-schemas.rs` 経由 (build.rs を使わない)

- `packages/server/src/schema_export.rs::write_schemas(out_dir)` が `schemars::schema_for!(Info)` / `schema_for!(Capability)` を回し、`out_dir/{Info,Capability}.schema.json` を pretty-printed JSON + 末尾 LF で書き出す。
- top-level に `"$comment": "AUTO-GENERATED FROM packages/server/src/proto.rs — do not edit by hand"` を `serde_json::to_value` 経由で挿入。
- `packages/server/examples/gen-schemas.rs` は `gtk4_e2e_server::write_schemas` を呼ぶだけの薄い entry。出力ディレクトリは `CARGO_MANIFEST_DIR` 起点の絶対パスで解決し、cwd 非依存。
- Cargo.toml の `[[example]]` セクションで `required-features = ["e2e"]` を強制 (= `--features e2e` 指定漏れに対する親切な diagnostic)。
- すべて `#[cfg(feature = "e2e")]` gate (default build に schemars / serde_json を引き込まない)。

`build.rs` を採用しない理由:

1. 案A (build.rs に struct を再定義) は SSOT 原則違反。
2. 案B (test 内で schema を書く) は test 副作用が原則違反 + race。
3. 案C (`include_bytes!` でテキスト inclusion) は型情報が手に入らない。
4. 案E (build.rs から再帰的に `cargo run`) は reproducible build を壊す。

### D3. TS 生成: `packages/client/scripts/gen-types.ts`

- 入力: `packages/server/proto/schemas/*.schema.json`
- `json-schema-to-typescript` の `compileFromFile` を全 schema に適用、出力された TS を top-level 宣言ブロック単位で deduplicate (Info schema の `definitions.Capability` と Capability schema 単独出力で同名 export が重複するため) し、ヘッダコメント付きで `packages/client/src/types.gen.ts` に書き出す。
- 設定: `additionalProperties: false`, `bannerComment: ""` (連結時の二重ヘッダ抑制), `format: false` (Biome は build artifact を fmt しない)。
- `if (import.meta.main)` で CLI 実行可能。

### D4. Stale check 戦略: schema を commit、TS は build artifact

- `packages/server/proto/schemas/*.schema.json` は **commit 対象 (SSOT artifact)**。`.gitignore` から `packages/server/proto/schemas/` 行を削除済み。
- `packages/client/src/types.gen.ts` は **commit しない build artifact**。`.gitignore` の `**/*.gen.ts` で従来どおり ignore。
- CI rust job: `cargo run -p gtk4-e2e-server --example gen-schemas --features e2e` 実行 → `git diff --exit-code packages/server/proto/schemas/` が non-zero なら fail。
- CI bun job: rust toolchain なしで動くので、commit 済みの schema を入力に `bun packages/client/scripts/gen-types.ts` を smoke 実行 + `grep -q` で `Info` / `Capability` 名の出力を最低限アサーション。

### D5. Pipeline flow

```
proto.rs (SSOT, JsonSchema derive)
    │
    │  cargo run -p gtk4-e2e-server --example gen-schemas --features e2e
    ▼
packages/server/proto/schemas/{Info,Capability}.schema.json  ← committed
    │
    │  bun packages/client/scripts/gen-types.ts
    ▼
packages/client/src/types.gen.ts                              ← .gitignored
```

ルート `task gen:types` (= `bun run gen:types` = `bun run --filter 'gtk4-e2e' gen:types`) が上 2 段を一括で回す。CI rust job は段 1 の stale を、bun job は段 2 が壊れていないことを確認する。

## Consequences

### Positive

- **SSOT 担保**: `proto.rs` が唯一の出発点。`build.rs` 二重定義のような fragile pattern を避ける。
- **stale を物理的に検出**: schema diff が 1 ファイルでも残れば CI が落ちる。`*.gen.ts` の formatter / json-schema-to-typescript バージョン差は stale 判定から除外され、ノイズが少ない。
- **bun job 単独で TS smoke**: rust toolchain 非依存なので、bun の codegen 退行は早く検出できる。
- **将来拡張の余地**: JSON Schema 中間表現を保つことで OpenAPI / AsyncAPI / 他言語 SDK への分岐を残す。

### Negative / Trade-offs

- **schemars 0.8 → 1.0 移行で breaking**: 1.0 は draft-2020-12 出力で json-schema-to-typescript の対応状況が変わる (別 ADR で対応予定)。
- **commit 対象が増える**: schema 2 ファイルが repo に永続化される。proto.rs を変えるたび schema diff も commit する必要がある (= レビュー対象が増える)。これは「stale 検出を成立させるコスト」として受容。
- **examples の `--features e2e` 強制**: `cargo run --example gen-schemas` 単体で fail する。`required-features` で diagnostic を出す形にして UX を緩和したが、知らずに叩くと一瞬戸惑う。

## Alternatives Considered

### TS 型生成ツール (D1)

| 案 | 却下理由 |
|---|---|
| `typeshare` (Rust + CLI) | `cargo install typeshare-cli` を CI に追加 / `#[typeshare]` attribute を proto.rs に振る必要 / JSON Schema を SSOT にする ADR-0001 と二重路線になる |
| `ts-rs` (Rust derive) | TS 専用、JSON Schema を出さないので OpenAPI/AsyncAPI 等への展開時に再 codegen 必要 / `cargo test` 時に副作用書込みする pattern が schemars 派生と噛み合わない / `JsonSchema` derive と独立に `TS` derive を並べる重複 |

### Schema 出力タイミング (D2)

| 案 | 却下理由 |
|---|---|
| 案A: `[build-dependencies]` に schemars を入れ、struct を build.rs に再定義 | 同じ struct を 2 か所に書くため SSOT 原則違反 |
| 案B: test 内で schema 出力 | test 副作用は原則違反 / 並列実行時 race / 「test を回さないと schema が更新されない」依存も筋悪 |
| 案C: build.rs から `include_bytes!("src/proto.rs")` | テキスト inclusion で型情報は手に入らず、現実的でない |
| 案E: build.rs から再帰的に `cargo run --example` 起動 | reproducible build を壊しやすく Cargo lock の deadlock 風挙動が知られる |

### Stale check 戦略 (D4)

| 案 | 却下理由 |
|---|---|
| 案A: `*.gen.ts` を commit (`.gitignore` から外す) | seed.md §5 / ADR-0001 §langmap が「`types.gen.ts` は手書き禁止、`.gitignore` 対象」と明文化 / commit 物が大きく / TS 生成パラメータの差で diff ノイズが増える |
| 案C: schema fingerprint hash のみ commit | hash はレビュー時に内容を読めない / 生成ツールバージョン差で hash がぶれ false positive 多発 |

## Verification

### 自動 (CI で毎回)

- rust job: `cargo run -p gtk4-e2e-server --example gen-schemas --features e2e` 実行後 `git diff --exit-code packages/server/proto/schemas/` が exit 0。
- bun job: `bun packages/client/scripts/gen-types.ts` が exit 0 で完走し、`grep -q "export interface Info" packages/client/src/types.gen.ts` と `grep -q "Capability" packages/client/src/types.gen.ts` が pass。
- platform: Linux (ubuntu-latest) / macOS。Windows は対象外。bun の `&&` chain は sh 経由に依存するため Windows は意識しない。

### 手動 simulate (PR description に結果を残す)

1. clean な worktree で `task gen:types` を 1 回実行 → schemas + `types.gen.ts` を生成。
2. `proto.rs` の `Info` に dummy field (例: `pub debug_marker: bool`) を一時追加。
3. `cargo run -p gtk4-e2e-server --example gen-schemas --features e2e` 実行。
4. `git diff packages/server/proto/schemas/Info.schema.json` で `debug_marker` が現れること、`git diff --exit-code packages/server/proto/schemas/` が non-zero exit すること。
5. dummy field を revert し再度 `task gen:types`、diff が空に戻ること。

### Rust integration tests (`packages/server/tests/schema_export.rs`, feature-gated)

- `writes_info_and_capability` — tempdir に 2 ファイル生成。
- `info_schema_has_instance_id` — `Info.schema.json` の `properties.instance_id.type == "string"`。
- `capability_schema_has_snake_case_variant` — `Capability.schema.json` の `enum` に `"info"` を含む。
- `schema_carries_provenance_comment` — top-level `$comment` が `proto.rs` を参照。
- `output_is_deterministic` — 連続 2 回呼んで bytes 完全一致 + 末尾 LF。

## Open Questions

- **`InstanceFile` の SSOT 化**: `packages/server/src/registry.rs::InstanceFile` は registry file format として protocol 表面に出ているが、現 Step 3 のスコープ外。Step 4 で SDK 着手時に `JsonSchema` derive を追加し `schema_export` を拡張するか、SDK 側で TS 型を別途用意する (= SSOT を割る) か、Step 4 着手時に再評価する。
- **`types.gen.ts` の export エントリ点**: Step 4 で `packages/client/src/index.ts` から `types.gen.ts` の型をどう re-export するか。`bun build` 時に `types.gen.ts` が未生成だと壊れる可能性があり、初回の `task gen:types` をどう CI / 開発者導線に組み込むかを Step 4 で決める。
- **schemars 0.8 → 1.0 移行 timing**: 1.0 は draft-2020-12 出力。json-schema-to-typescript の対応状況を確認したうえで、Step 9 以降に別 ADR で扱う。
- **将来の typeshare / ts-rs 移行余地**: 複数言語 SDK の必要性が顕在化した場合、JSON Schema → 言語別 codegen に分岐する形で吸収する。typeshare へ全面移行する判断は当面不要。

## References

- ADR-0001 §langmap (本 ADR で「`build.rs` で〜」の実装手段を `examples/gen-schemas.rs` 経由へ変更している点を明記)。
- [`schemars`](https://crates.io/crates/schemars) — Rust → JSON Schema。
- [`json-schema-to-typescript`](https://www.npmjs.com/package/json-schema-to-typescript) — JSON Schema → TS。
- `packages/server/src/schema_export.rs` — 出力ロジック。
- `packages/server/examples/gen-schemas.rs` — codegen entry。
- `packages/client/scripts/gen-types.ts` — TS codegen entry。
