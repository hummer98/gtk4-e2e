
# ADR-0003 (Draft): Visual regression diff — engine selection

- **Status**: Proposed
- **Date**: 2026-05-10
- **Confidence**: 中〜高 (75%) — Bun 単体ランタイム制約・ライセンス・Playwright 先行事例が一致して候補を絞っているため engine 選定そのものはほぼ確定的に書ける。確信度を 100% にしていないのは、(a) gtk4-e2e の screenshot サイズ実測値 (cargo demo の typical 出力 px 数) を CI で取っていないため pixelmatch の処理時間 budget が現物では未検証、(b) baseline storage / SDK API 名 / threshold default の 4 つの Open Questions が ready 化時に決定すべき項目として残るため。

## Context

T007 申し送り (`docs/reports/2026-05-10-bootstrap.md` §E "Visual regression diff (T007 申し送り)") で挙がった「screenshot は PNG バイナリとして取れるが baseline 比較が未配線」問題に対し、診断手段としての visual regression を整備する基盤調査。

現状把握:

- `GET /test/screenshot` は `packages/server/src/snapshot.rs` の `gsk::CairoRenderer` + `gdk::Texture::save_to_png_bytes()` で PNG bytes を返す (T007 で実装済み)。
- TS SDK 側 `client.screenshot()` は PNG `Uint8Array` を返すか、引数のパスへ `Bun.write` するだけ (`packages/client/src/client.ts`)。
- `packages/demo/scenarios/screenshot.spec.ts` は PNG signature と IHDR のチェックのみで、baseline 比較は未実装。
- 親タスクは ADR-0001 §Open Questions 「screenshot 出力の visual diff threshold で safelist 通過」(MVP Phase 2 verification) の継承。
- ユーザー判断: **「pixeldiff はあまり使わない」**ため `low priority + draft 温存`、本タスクは ready 化前段の選定 + ADR 化に **スコープを絞っている**。フル実装には踏み込まない。
- ランタイム/言語: TS SDK 側は Bun 1.x、TypeScript strict、ESM (`packages/client/package.json`)。devDependencies に既に `json-schema-to-typescript@^15` があるため "TS だけで完結する追加依存" は許容文化。
- gtk4-e2e の screenshot 特性: GTK4 theme は CI 上で OS テーマを `Adwaita` 固定にすれば決定論的にレンダリングされる蓋然性が高い (xvfb 経由の CairoRenderer) が、widget アンチエイリアス (text rendering, rounded corners) のサブピクセル差は環境依存で出やすい。

## Decision Drivers

選定軸:

1. **Bun 互換性 (最重要)**: SDK は Bun ランタイム必須。Bun は postinstall を default で実行しないため、native binary 同梱で postinstall に依存する package は `trustedDependencies` 経由か CI 側の install 設計を強要する。
2. **ライセンス**: 配布物 (TS SDK + Claude plugin) に組み込むため AGPL は実質的に不可。MIT/ISC が望ましい。
3. **アルゴリズム**: ピクセル単位 / SSIM / perceptual のどれが GTK4 native screenshot に適するか。CI が固定 (xvfb + Adwaita) なので**過剰に perceptual である必要はなく、サブピクセル AA だけ無視できれば十分**。
4. **メンテナンス状態**: 直近 1 年以内の release があり、issue 応答が生きていること。
5. **diff 画像出力**: 失敗時に「どこが変わったか」を artifact 化できないと CI レビューが詰まる。
6. **threshold 粒度**: per-pixel しきい値 + 全体 diff pixel 数の 2 段階 (Playwright 互換) を取れること。
7. **ARM64 macOS / x86_64 Linux 双方**: yamamoto のローカル M-series Mac と GitHub Actions ubuntu-latest の両方で動くこと。
8. **TS SDK 統合容易性**: 現在の `packages/client/src/` の構造 (ESM, raw `Uint8Array` を返す `client.screenshot()`) に対し追加依存を最小に取り込めること。
9. **Playwright 先行事例**: gtk4-e2e は ADR-0001 で「Playwright 同等」を掲げている。ユーザーが既に Playwright を触っている前提では、threshold パラメータの語彙が一致するほど学習コストが低い。

## Options Considered

### Option A: pixelmatch (Mapbox)

- **公式リポジトリ**: <https://github.com/mapbox/pixelmatch>
- **npm**: <https://www.npmjs.com/package/pixelmatch>
- **直近 release**: v7.2.0 (2025-04-29) — 最近活発に更新されている (v7.1.0 で TS 型定義追加・性能 +22%、v7.1.1 で +8%)。
- **License**: ISC
- **アルゴリズム**: ピクセル単位 + YIQ NTSC color space で「人間の目に近い」色差。
- **TS/Bun 統合**: 純 JS、依存ゼロ、ESM (v6+)、型定義同梱 (v7.1.0+)。Bun で `bun add pixelmatch` だけで動く。**postinstall なし**。
- **入出力 format**: 引数は **生 RGBA buffer** (`Uint8Array` or `Buffer`)。PNG decode は呼び出し側が `pngjs` などで行う必要がある。出力 diff も RGBA buffer に書く。
- **threshold**: `threshold` (0-1, default `0.1`、`includeAA`、`alpha`、`aaColor`、`diffColor`、`diffColorAlt`、`diffMask`。anti-aliasing は default で「検出して無視」する設定 (`includeAA: false`)。
- **anti-aliasing 対策**: ✅ 内蔵。AA pixel を検出してマスクするロジックがある。
- **diff 画像**: ✅ 出力 buffer に書き、別途 `pngjs` で PNG エンコード。
- **ARM64 mac / x86_64 Linux**: ✅ JS のみなので 100% 動く。
- **Performance**: 1 thread、SIMD 等は使わない。8K 画像で odiff の 5x 遅い (10s vs 2s) というベンチがあるが、gtk4-e2e の typical screenshot はせいぜい 1024×768〜1920×1080 (≦ 2 MP) なので**サブ秒で済む見込み**。
- **README quote**: "The smallest, simplest and fastest JavaScript pixel-level image comparison library, originally created to compare screenshots in tests."
- **pros**:
  - Bun で postinstall を回避できる (純 JS)。
  - Playwright と同じ engine で、threshold 0.2 等の語彙が直接使える。
  - 依存ゼロで supply chain 影響面が極小。
  - ISC は AGPL より緩く、商用配布も問題ない。
  - 型定義同梱で TS strict モードと相性良。
- **cons**:
  - PNG decode を別パッケージ (`pngjs` or Bun の `sharp`) で行う必要があり、追加依存が 1 つ増える。
  - 8K 級の大画像では SIMD 化された odiff より 5x 遅い (gtk4-e2e の用途では問題にならない見込み)。

### Option B: odiff (`odiff-bin`)

- **公式リポジトリ**: <https://github.com/dmtrKovalenko/odiff>
- **npm**: <https://www.npmjs.com/package/odiff-bin>
- **直近 release**: v4.3.8 (2024-04-17) — 1 年強更新が止まっているが、安定運用に入った status と読める。
- **License**: MIT
- **アルゴリズム**: ピクセル単位 + SIMD (Zig)。
- **TS/Bun 統合**: ❌ **postinstall script が前提**。Bun は default で postinstall を走らせないため `trustedDependencies` に明示する必要がある (Bun docs)。
- **入出力**: ファイルパス前提 (PNG/JPG ファイル ⇄ ファイル)。in-memory buffer も server mode では受け取れる。
- **threshold**: `threshold` (0-1)、`antialiasing`、`captureDiffLines`、`ignoreRegions`、`diffColor`、`diffOverlay`、`outputDiffMask`、`reduceRamUsage`。
- **anti-aliasing 対策**: ✅ option として持っている。
- **diff 画像**: ✅ ファイルとして直接書ける。
- **ARM64 mac / x86_64 Linux**: ✅ prebuilt binaries 同梱。Apple Silicon / Linux x64 / RISC-V までカバーしている。
- **Performance**: 1 thread で SIMD 駆使、8K で pixelmatch の 5–7x 速い (Cypress full-page で 1.17s vs 7.71s)。
- **採用例**: Argos visual testing platform、LostPixel。
- **README quote**: "The fastest (one-thread) pixel-by-pixel image difference tool in the world."
- **pros**:
  - 速い (large screenshot で 5x+)。
  - server mode で複数比較を 1 process に集約可能。
  - 採用実績 (Argos / LostPixel) が確かで、CI 用途で枯れている。
- **cons**:
  - **Bun での postinstall 問題**: `trustedDependencies` 追加が SDK consumer 側に伝播する (consumer が `bun install gtk4-e2e` した時、odiff-bin の postinstall が走らないと binary が不在になる)。SDK が破綻なく配布できるかは未検証。
  - native binary 同梱で npm tarball サイズが大きい (issue #50)。
  - macOS / Linux / Windows / RISC-V 全部入りの tarball で download/cache 時間がかさむ。
  - サブプロセス起動コストがあり、screenshot 1 回あたり pixelmatch 比で fixed cost が乗る (small screenshot では逆転する余地)。

### Option C: dssim (Kornel Lesiński)

- **公式リポジトリ**: <https://github.com/kornelski/dssim>
- **直近 release**: v3.4.0 (2025-03-05) — 活発。
- **License**: ❌ **AGPL-3.0 or 商用ライセンス (dual)**。
- **アルゴリズム**: SSIM (multi-scale)、L*a*b* color space。人間の知覚に最も近い結果を出す。
- **TS/Bun 統合**: ❌ **公式 Node bindings なし**。Rust / C / WASM のみ。Wrapping するなら subprocess を spawn するか、自分で WASM 経由を書く必要がある。
- **入出力**: ファイル → 数値 (CLI のみ、stdout に dissimilarity score)。
- **threshold**: 数値出力なので呼び出し側で自前比較。
- **anti-aliasing 対策**: SSIM ベースなので暗黙的にロバスト。
- **diff 画像**: ✅ CLI で生成可能。
- **ARM64 mac / x86_64 Linux**: ✅ Rust なら両方ビルド可能。Homebrew / Snap で配布あり。
- **Performance**: SSIM としては高速だが pixelmatch 純 JS よりは遅い (実測値はベンチによる)。
- **README quote**: "Image similarity comparison simulating human perception (multiscale SSIM in Rust)"
- **pros**:
  - 知覚的に最も正確。GTK theme/AA の noise を SSIM で吸収する強さは最大。
  - Rust 製で安全。
- **cons**:
  - **AGPL は SDK 配布で実質採用不可**。Claude plugin / `bunx gtk4-e2e` の同梱物として配ると配布物全体が AGPL 化リスク (商用ライセンス購入で回避可能だが運用負荷高)。
  - 公式 Node bindings がなく、wrapping コストを丸抱え。
  - CI 環境に Rust toolchain or 別途 dssim binary を install する手数。
  - 数値出力のみで diff 画像と pass/fail の判定は呼び出し側に委ねられ、glue code が増える。

### Option D: image-diff (uber-archive) — 参考のみ

- **公式リポジトリ**: <https://github.com/uber-archive/image-diff>
- **npm**: <https://www.npmjs.com/package/image-diff>
- **状態**: ❌ **DEPRECATED**。`uber-archive` 配下、Uber が "halted maintenance" を明言。
- **公式推奨代替**: pixelmatch、looks-same。
- **結論**: **不採用**。維持されていない package を新規導入する理由がない。

### Option E: 補足 — Playwright の `toHaveScreenshot()` がどう動くか

公式 docs (<https://playwright.dev/docs/test-snapshots>) と PR ベースで確認:

- **engine**: pixelmatch (default 設定で `_comparator: "pixelmatch"`)。
- **threshold default**: **0.2** (pixelmatch の素の default 0.1 より緩く設定し、Playwright のテストが安定するよう調整している)。
- **その他のしきい値**: `maxDiffPixels` (絶対) と `maxDiffPixelRatio` (0-1) を threshold と AND で評価。
- **anti-aliasing**: pixelmatch の `includeAA: false` (= AA 検出して無視) の default を踏襲。
- **baseline 戦略**: 初回実行時 baseline を自動作成 (`--update-snapshots` でも明示更新可能)。
- **diff artifact**: 失敗時に baseline / actual / diff の 3 枚を test report に attach。
- **gtk4-e2e への含意**: SDK API 設計時、threshold default を **0.1 (pixelmatch native) ではなく 0.2 (Playwright 慣例)** に揃える方が、Playwright を触ってきた consumer の expectation と一致する。

## Comparison Table

| 軸 | pixelmatch | odiff (`odiff-bin`) | dssim | image-diff |
|---|---|---|---|---|
| License | **ISC** | MIT | ❌ AGPL/商用 | MIT (但し deprecated) |
| 直近 release | 2025-04-29 (v7.2.0) | 2024-04-17 (v4.3.8) | 2025-03-05 (v3.4.0) | (deprecated) |
| アルゴリズム | pixel + YIQ | pixel + SIMD | multi-scale SSIM (L*a*b*) | pixel (古典) |
| Bun 互換 | ✅ 純 JS、postinstall なし | ⚠️ postinstall 必須 (trustedDependencies) | ❌ Node bindings なし | ✅ 但し deprecated |
| 公式 Node API | ✅ | ✅ | ❌ | ✅ |
| 入出力 | RGBA buffer ⇄ buffer | ファイル ⇄ ファイル (server mode で buffer) | ファイル → score | ファイル ⇄ ファイル |
| TypeScript types | ✅ 同梱 (v7.1.0+) | DefinitelyTyped 由来 | ❌ | ⚠️ |
| anti-aliasing 対策 | ✅ `includeAA` | ✅ `antialiasing` | 暗黙的 (SSIM) | ⚠️ 弱 |
| diff 画像出力 | ✅ buffer | ✅ ファイル | ✅ CLI | ✅ |
| threshold 粒度 | per-pixel (0-1) | per-pixel (0-1) | 数値、後段で判定 | 0/1 真偽 |
| ARM64 mac | ✅ | ✅ | ✅ | ✅ |
| x86_64 Linux | ✅ | ✅ | ✅ | ✅ |
| 8K perf | 〜10s | 〜2s (5x) | 〜数秒 | 不明 (deprecated) |
| 1〜2 MP perf (gtk4-e2e の現実的サイズ) | サブ秒 | サブ秒 | 〜1s | 不明 |
| Playwright 慣例との一致 | ✅ (同 engine) | △ | ❌ | ❌ |
| 配布同梱の容易さ | ✅ | △ (binary 同梱で大きい) | ❌ (AGPL) | ❌ (deprecated) |

## Recommendation

**Option A (pixelmatch) を採用する。**

### 理由

1. **Bun ランタイム整合性**: gtk4-e2e SDK は Bun を前提にしている (ADR-0001)。pixelmatch は純 JS / 依存ゼロ / ESM で `bun add` 一発、postinstall 問題に巻き込まれない。`odiff-bin` の `trustedDependencies` 強要は、SDK を `bun add gtk4-e2e` で取り込む consumer の install 体験を壊す risk がある。
2. **ライセンス**: ISC は MIT/BSD と並んで配布物に組込み自由。dssim の AGPL は SDK + Claude plugin の配布物に持ち込めない (商用ライセンス購入で回避可能だが運用負荷)。
3. **Playwright 整合性**: ADR-0001 が「Playwright 同等」を掲げ、本 framework の SDK 利用者は Playwright 経験者が多いと想定できる。pixelmatch を採れば threshold default を 0.2 にする等、語彙とパラメータが直接転用できる。学習コストが最も低い。
4. **gtk4-e2e の用途と一致**: typical な GTK4 window の screenshot は 1〜2 MP で、odiff の SIMD アドバンテージ (8K で 5x) が顕在化しない。pixelmatch のサブ秒処理で十分。
5. **依存リスク最小**: pixelmatch は依存ゼロ、数百行規模のコードベースで監査可能。odiff の native binary tarball / dssim の Rust toolchain 依存より supply chain risk が低い。
6. **メンテナンス活発度**: 2025-04-29 の最近 release があり、TS 型定義の改善 (v7.1.0) も 2025-02 に入っている。

### 代替案を採るべき条件 (= pixelmatch から乗り換える signal)

- 後で screenshot サイズが恒常的に 8K 級になる (cargo demo を 4K display で動かす consumer が出る) 場合 → **odiff** に乗り換え。Bun の `trustedDependencies` で解決可能と確認済み。
- GTK theme / AA noise が CI 上で flake を起こし、pixel-level しきい値ではマスクしきれないと判明した場合 → **dssim** を CLI として subprocess 起動する経路で導入 (商用ライセンスが必要な場合は別途判断)。ただし AGPL を avoid するため SDK には bundle せず、CI 上の verification step として呼ぶに留める。
- pixelmatch v7 が ESM のみであることが consumer の CommonJS 環境で問題になった場合 → 当面 SDK は ESM 確定なので非該当。

### PNG decode 補助の選択

pixelmatch は raw RGBA を要求するので、PNG decode を別途行う必要がある。選択肢:

- **`pngjs`** (純 JS、依存ゼロ、ESM 対応): pixelmatch 同様 Bun で postinstall 不要。**第一候補**。
- **`sharp`** (libvips bind、native): postinstall 経路。Bun の trustedDependencies 必須化は pixelmatch を選ぶ前提に反するので非採用。
- **Bun 内蔵 image API**: 2026-05 時点で stable PNG decode は提供されていない (Bun.image は WIP)。将来的に置換可能。

→ MVP は **pixelmatch + pngjs** の 2 package セットで進める。

## Open Questions

ready 化時に決定する項目:

1. **SDK API 命名**: 候補 3 案。
   - 案 A. **`client.expectScreenshot(name, opts?)`** (Playwright 風) — Playwright 経験者には自然、SDK の `client` 文脈と整合。
   - 案 B. `expect(client.screenshot()).toMatchScreenshot(name)` (Jest snapshot 風) — bun:test との結合度が高い。
   - 案 C. `compareScreenshot(client, name, opts?)` (関数型) — テストランナー非依存。
   - **推奨初期スタンス**: 案 A。`expect()` を bun:test の matcher 拡張で書くと runtime 結合が深くなり SDK 単独利用 (`bunx gtk4-e2e ...`) で扱いづらいため。
2. **baseline 格納先**: 親タスク文では `packages/demo/scenarios/__screenshots__/<scenario>-<name>.png`。**commit 対象** にすべきか別 storage (Git LFS / CI artifact + ハッシュ参照) か。
   - 推奨初期スタンス: **commit 対象**。screenshot サイズが小さい (<200 KB/枚 想定) うちは git で扱う方が CI 設定が単純。LFS は consumer ニーズで重くなったタイミングで再評価。
   - **決定 (T020-B 2026-05-10)**: **commit 対象** で確定。`__screenshots__/` ディレクトリは scenario と同階層に置き、`.gitignore` に追加しない。SDK 側は wrapper (`E2EClient.expectScreenshot`) で「`opts.baselineDir > opts.testFile > env GTK4_E2E_BASELINE_DIR > Error().stack 推定 > <cwd>/__screenshots__`」の優先順位で baseline ディレクトリを解決する (実装 §"Resolved Decisions" 参照)。Git LFS は当面採用せず、200 KB/枚 を有意に超えた段階で再評価する未決事項として残す。
3. **threshold default**: 0.1 (pixelmatch native) / 0.2 (Playwright 慣例) / それ以外。
   - 推奨初期スタンス: **0.2** (Playwright 互換)。GTK theme + xvfb の AA noise を吸収する余裕として 0.1 は厳しめ。
4. **初回実行時 (baseline 不在) の挙動**: 自動生成 / `--update-baseline` 明示まで fail のどちらか。
   - 推奨初期スタンス: **明示まで fail + わかりやすい error message**。Playwright は default で auto-create するが、CI で baseline drift を見落とす事故が起きやすい。`bunx gtk4-e2e screenshot --update-baseline <name>` の明示更新を推す。
   - **変更 (T020-B 2026-05-10)**: 元推奨「明示まで fail」を**撤回**し「auto-save + `process.env.CI === "true"` 検出時のみ fail」を採る (Playwright 慣例と整合)。元懸念 (CI で baseline drift を見落とす事故) は CI 検出により完全にカバーされるため。`opts.failOnMissing` を pure function (`expectScreenshot`) の API に追加し、wrapper (`E2EClient.expectScreenshot`) が CI 判定 → `failOnMissing` を導出する。`opts.updateBaseline=true` は引き続き「無条件で baseline を上書きする」最優先パスとして残る。
5. **anti-aliasing 対策の方針**: pixelmatch の `includeAA: false` (= AA 検出して無視) を採るか、CI が決定論的なので `includeAA: true` (= AA 込みで厳密判定) を採るか。
   - 推奨初期スタンス: **`includeAA: false` (default)**。GTK theme は CI で固定だが、xvfb のサブピクセル AA 差は OS / Cairo バージョンで微妙にぶれ得るので緩めに開始し、flake が出ない範囲で締める。
6. **diff artifact の CI 保持期間**: GitHub Actions の `actions/upload-artifact` で baseline / actual / diff の 3 枚を残す保持期間 (default 90 日 / 短縮するか) と、failure 時のみ upload するか毎回 upload するか。
   - 推奨初期スタンス: **failure 時のみ upload、保持 30 日**。
7. **filename 規約の見直し** (新規、T020-B 2026-05-10 追加): 現規約 `<scenario_basename including ext>-<name>.png` (例 `foo.spec.ts-button.png`) は親タスク本文に従って採用したが、拡張子重複 (`.spec.ts-`) の見た目が独特。将来 `<basename without ext>-<name>.png` (例 `foo-button.png`) 案で再議論する余地を残す。判断保留 (再議論時期未定)。
8. **path traversal 安全性** (新規、T020-B 2026-05-10 追加): `expectScreenshot(actual, name)` の `name` に `../` を含めた場合の挙動 (rejection / slugify / `/` namespace) は別タスクで扱う。本 ADR / T020-B のスコープ外。

## Resolved Decisions (T020-B 2026-05-10)

§Open Questions §2 / §4 の決定 block と並んで、実装で確定した補助判断を以下に記録する (README は寿命短、ADR を正本とする)。

- **env 名**: `GTK4_E2E_BASELINE_DIR` を正式採用。既存 `GTK4_E2E_TOKEN` (`packages/client/src/client.ts`) と prefix を揃え、プロジェクト全体での grep 容易性・名前空間衝突回避を優先。`PWTEST_BASELINE_DIR` 等の Playwright 互換 env と共用する実利は乏しいため不採用。
- **filename 規約**: `<scenario_basename>-<name>.png`。`<scenario_basename>` はテストファイルの **拡張子を含む** basename (例 `foo.spec.ts`)。最終的なファイル名は `foo.spec.ts-button.png` の形を取る。`.spec.ts-` の重複ドットは見た目が独特だが、OS / Git / Bun 上の動作には支障なし。タスク本文の確定要件に従う。将来 `<basename without ext>-<name>.png` 案で再議論する余地は §Open Questions §7 で残してある。
  - **rev2 補足 (T020-B 2026-05-10, T020-C 互換)**: `opts.baselineDir` を**明示**した呼び出しでは `<scenario_basename>-` prefix を**付けない** (ファイル名は `<name>.png` のみ)。論拠: 呼び出し側が baseline ディレクトリを完全に明示しているケース (CLI `--baseline <path>` 経由など) では、ファイル名規約も呼び出し側が制御していると解釈し、SDK 側で暗黙の prefix 化を行うとパスがずれて baseline が見つからなくなるため。stack 推定 / `opts.testFile` / `env GTK4_E2E_BASELINE_DIR` 経由の resolve では従来どおり prefix する (= `opts.baselineDir` のみが特別扱い)。
- **API 命名**: `ExpectScreenshotOptions.failOnMissing?: boolean` を採用 (旧 sketch の `createBaselineIfMissing?: boolean` は不採用)。既存の `updateBaseline` と語彙を揃え、「失敗側を明示する boolean は他にも増えうる」運用を意識した命名。
- **CI 検出**: `process.env.CI === "true"` (文字列一致) のみ。GitHub Actions / GitLab CI / CircleCI / Bitbucket Pipelines / Buildkite はいずれも `CI=true` を default で export するため、現運用下では取りこぼしなし。Travis 旧設定 (`CI=1`) や Jenkins ジョブ依存設定は **意図的に取りこぼす**。必要が生じれば `["true", "1"].includes(...)` 等への拡張は容易だが、「`CI=true` で揃える」運用統一を当面の正と扱う。
- **env 参照の局所化**: pure function (`expectScreenshot`) は env を一切読まない。stack-based caller 推定 + `process.env.CI` / `process.env.GTK4_E2E_BASELINE_DIR` の取得は wrapper (`E2EClient.expectScreenshot`) のエントリポイント 1 箇所に閉じ、内部 helper には引数として inject する。bun:test のファイル間並行実行で `process.env` を書き換えるテストが他 file に leak しないようにするための原則。
- **path traversal 安全性**: `name` のサニタイズは本 ADR / T020-B のスコープ外。後続タスクで decision を取る (§Open Questions §8)。pure function (`visualDiff.ts`) header コメントの "T022 で決定" 注記は誤誘導なので「後続タスクで決定」に書き換える。
- **Status**: 本 ADR は引き続き **Proposed** のまま。Accepted への昇格は T020-F (実装 + demo 統合 + 親タスク完走後にまとめて) で行う。本タスクは §Open Questions §2 / §4 の方針確定に留める。

## Implementation sketch (high level)

詳細実装は ready 化後の別タスクで分割すること。本 ADR は方向性の素描まで。

### SDK API (TS, `packages/client/src/visual.ts` 仮称)

```typescript
// 実装は ready 化後タスクで詰める。型は Playwright を参考にした素案。
export interface ExpectScreenshotOptions {
  /** Per-pixel YIQ threshold (0-1). Default tentatively 0.2 (Playwright 慣例; finalize at ready). */
  threshold?: number;
  /** Absolute max diff pixels. Default unset. */
  maxDiffPixels?: number;
  /** Max diff pixel ratio (0-1). Default unset. */
  maxDiffPixelRatio?: number;
  /** Include anti-aliased pixels in diff. Default tentatively false (= AA を無視; finalize at ready). */
  includeAA?: boolean;
  /**
   * baseline 不在時、throw するか。Default false (= auto-save + match=true)。
   * wrapper (`E2EClient.expectScreenshot`) は `process.env.CI === "true"` を
   * 検出した場合 default を true にする (T020-B 決定)。
   * 旧 sketch にあった `createBaselineIfMissing?: boolean` は不採用。
   */
  failOnMissing?: boolean;
  /** baseline ディレクトリの override。Default は scenario 起点の `__screenshots__/`. */
  baselineDir?: string;
  /** baseline を強制的に上書きする (CI/env を問わず最優先)。Default false. */
  updateBaseline?: boolean;
}

export class E2EClient {
  // ... 既存 methods
  async expectScreenshot(name: string, opts?: ExpectScreenshotOptions): Promise<void>;
}
```

### baseline directory layout

```
packages/demo/scenarios/
├── __screenshots__/
│   ├── screenshot.spec.ts-main-window.png   # baseline (commit 対象)
│   └── ...
├── screenshot.spec.ts
└── ...
```

`<scenario_basename>-<name>.png` の命名は parent task に従う。`<scenario_basename>` は spec ファイルの **拡張子を含む** basename (例 `screenshot.spec.ts`)、`<name>` は `expectScreenshot` 第一引数。`.spec.ts-` の重複ドットは見た目が独特だが、当面はタスク本文の確定要件に従う (§Open Questions §7 で再議論余地)。

### CLI flag (`packages/client/src/cli.ts`)

```bash
bunx gtk4-e2e screenshot --baseline <name>     # 比較モード (差分があれば exit 1)
bunx gtk4-e2e screenshot --update-baseline <name>  # baseline 更新
bunx gtk4-e2e screenshot --baseline <name> --threshold 0.1  # threshold 上書き
```

### CI artifact

```yaml
# .github/workflows/ci.yml の demo job 末尾、failure 時のみ
- if: failure()
  uses: actions/upload-artifact@v4
  with:
    name: visual-regression-${{ matrix.os }}
    path: |
      packages/demo/scenarios/__screenshots__/**/*.png
      /tmp/scenario-artifacts/visual-diff/**/*.png
    retention-days: 30
```

### baseline / actual / diff の出力規約

- **baseline**: `packages/demo/scenarios/__screenshots__/<scenario>-<name>.png` (commit 対象)
- **actual** (実行時取得): `/tmp/scenario-artifacts/visual-diff/<scenario>-<name>.actual.png`
- **diff** (pixelmatch 出力): `/tmp/scenario-artifacts/visual-diff/<scenario>-<name>.diff.png`

CI artifact は actual / diff のみ (baseline は repo に commit 済みなので artifact 化不要)。

### 依存追加

`packages/client/package.json`:

```json
{
  "dependencies": {
    "pixelmatch": "^7.2.0",
    "pngjs": "^7"
  }
}
```

`pngjs` の最新 stable は v7 系。Bun で動作確認は ready 化時に取る。

### 実装分割 (ready 化時にここから細粒度 task を切る想定)

T020-A. SDK 層 — `expectScreenshot` 実装 + unit test (mock screenshot bytes)
T020-B. baseline storage — `__screenshots__/` ディレクトリ規約 + 初回実行時の挙動
T020-C. CLI 統合 — `bunx gtk4-e2e screenshot --baseline` フラグ
T020-D. CI 連携 — `actions/upload-artifact` で diff 配信
T020-E. demo scenarios — `screenshot.spec.ts` に visual regression assertion 追加
T020-F. ADR-0003 確定化 (本 draft → Accepted への昇格)

## References

### 一次情報

- Mapbox pixelmatch リポジトリ: <https://github.com/mapbox/pixelmatch>
- pixelmatch npm: <https://www.npmjs.com/package/pixelmatch>
- pixelmatch releases (v7.2.0 = 2025-04-29、v7.1.0 = 2025-02-21): <https://github.com/mapbox/pixelmatch/releases>
  - README quote: "The smallest, simplest and fastest JavaScript pixel-level image comparison library, originally created to compare screenshots in tests." (License: ISC)
- dmtrKovalenko odiff リポジトリ: <https://github.com/dmtrKovalenko/odiff>
- odiff-bin npm: <https://www.npmjs.com/package/odiff-bin>
- odiff releases (v4.3.8 = 2024-04-17): <https://github.com/dmtrKovalenko/odiff/releases>
  - README quote: "The fastest (one-thread) pixel-by-pixel image difference tool in the world." (License: MIT)
  - 性能比較 (Cypress full-page): odiff 1.168s / pixelmatch 7.712s / ImageMagick 8.881s
- kornelski dssim リポジトリ: <https://github.com/kornelski/dssim>
  - License: AGPL-3.0 or commercial (dual-licensed)
  - 直近 release: v3.4.0 (2025-03-05)
- uber-archive/image-diff: <https://github.com/uber-archive/image-diff>
  - 状態: deprecated、Uber が halted maintenance を明言
- Playwright "Visual comparisons" docs: <https://playwright.dev/docs/test-snapshots>
- Playwright SnapshotAssertions API: <https://playwright.dev/docs/api/class-snapshotassertions>
  - "The "pixelmatch" comparator computes color difference in YIQ color space and defaults threshold value to 0.2."

### 二次情報 / 議論

- "Comparing odiff with pixelmatch and jimp" (odiff Discussion #82): <https://github.com/dmtrKovalenko/odiff/discussions/82>
- "Why our visual regression is so slow?" (odiff 著者 blog): <https://dev.to/dmtrkovalenko/why-our-visual-regression-is-so-slow-33dn>
- Bun postinstall / trustedDependencies 仕様: <https://bun.sh/package-manager> および Bun issue #4959

### 内部参照

- ADR-0001: `docs/adr/0001-architecture.md` — Bun/TS SDK 採用、Playwright 同等の能力という設計指針
- ADR-0002: `docs/adr/0002-codegen-pipeline.md` — `packages/client` の依存追加先 (devDependencies に json-schema-to-typescript の前例あり)
- T007 申し送り: `docs/reports/2026-05-10-bootstrap.md` §E "Visual regression diff (T007 申し送り)" (round2 後の経緯は `docs/reports/2026-05-10-round2-stabilization.md` §C / §E)
- 既存実装: `packages/server/src/snapshot.rs` (PNG bytes 出力), `packages/client/src/client.ts` の `screenshot()`, `packages/demo/scenarios/screenshot.spec.ts`
