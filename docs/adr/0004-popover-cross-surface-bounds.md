
# ADR-0004: `GET /test/elements` cross-surface (popover) bounds composition

- **Status**: Proposed (実装後 Accepted へ昇格予定 — consumer = Brainship FE が submodule SHA bump 後に popover はみ出し検出で実利用したタイミング)
- **Date**: 2026-06-14
- **Confidence**: 中〜高 (75%) — 合成式と surface 同一性ディスパッチは macOS quartz 実機 + CI(xvfb/X11) scenarios で数値検証済み。100% にしないのは (a) HiDPI (scale>1) 未検証、(b) 多段 popover (popover-in-popover) 未対応、(c) X11/Wayland 以外のバックエンドでの surface_transform 符号を実測していないため。

## Context

`GET /test/elements` は widget tree を返し、各ノードの `bounds`(親 `GtkWindow` の root widget 原点相対 px)で consumer が「画面(=モニタ=全画面窓)からのはみ出し」を機械検証できる。これは Brainship FE の凡例 Popover などが端で画面外に切れていないかを CI で検出するための土台である(T329/T330 反証参照)。

しかし `GtkPopover` は `GtkNative` であり、**自前の `GdkSurface`(Wayland では `xdg_popup`)** を持つ。bounds 計算は `elements.rs` で `widget.compute_bounds(window_root)` を使うが、これは 2 widget が同一 surface 上にある(共通座標系に変換できる)ことを前提とするため、popover 内 widget では **`None`** を返し `bounds: null` になっていた。結果、はみ出し検証の対象として最も重要な popover をちょうど見られなかった。

確定事実:

- `GtkPopover` は `set_parent()` / `MenuButton::set_popover()` で親 widget の子になり、`first_child()`/`next_sibling()` の DFS walk に **popover とその content が含まれる**(その時の `window_root` は親 `GtkWindow`)。よって「popover を別 root として再発見する」必要はなく、`to_element_info` の bounds 計算だけを直せばよい。
- `compute_point` / `translate_coordinates` も内部で同じ `gtk_widget_compute_transform` を使うため、別 surface 越えでは同様に失敗する(**より単純な API での代替は不可**)。手動合成が必要。

## Decision

### 1. ディスパッチは surface (GtkNative) 同一性で決める

bounds 計算の分岐を「`compute_bounds` が `None` を返したか」ではなく、**widget が属する surface の同一性**で決める:

```rust
let same_surface = match (widget.native(), window_root.native()) {
    (Some(wn), Some(rn)) => wn.upcast::<glib::Object>() == rn.upcast::<glib::Object>(),
    _ => false,
};
```

- **同一 native(= 同一 surface)** → 従来どおり `compute_bounds(window_root)`(`basis = None`)。同一 surface 内の未 realize widget もここに落ち、従来どおり `None` を返す(**副作用なし**)。
- **別 native(= 別 surface = popover)** → §2 の合成(`basis = Some(PopupComposed)`)。
- **合成不能**(`gdk::Popup` downcast 失敗 / position 取得不可 = popover が閉じている等)→ `None`(従来の `bounds: null` に戻るだけ)。

**なぜ surface 同一性か**: `widget.native()` は「その widget をホストする `GtkNative`(= 1 surface)」を返す関数なので、別 surface かどうかは native の同一性で**一意に決まる**。`compute_bounds` が X11/Wayland でたまたま `None` を返すか「親内 allocation 相対の誤った `Some(rect)`」を返すかに**完全に非依存**になり、「分岐が発火せず緑のまま fix がサイレント無効化される」事故が原理的に起きない。CI は ubuntu + xvfb(X11)だが、本番 consumer は Wayland/AGX なので、この非依存性が重要。

### 2. 合成式(popover → 親窓 root 相対)

popover content widget `w` の親窓 root 相対原点は 4 項の和:

```text
origin = (A) w を popover widget 原点相対で測る      compute_bounds(&popover)
       − (B) popover widget → popover surface 原点    popover.surface_transform()
       + (C) popover surface → 親 surface 原点         popup.position_{x,y}()
       − (D) 親 surface → 親窓 root widget 原点         window.surface_transform()
```

- **(A)** `w.compute_bounds(&popover_widget)` → `graphene::Rect`。w と popover は同一 surface なので成功する。w/h(allocation サイズ)もここから取る。
- **(B)** `NativeExt::surface_transform(&popover)` → `(f64,f64)`。
- **(C)** popover の surface を `gdk::Popup` にダウンキャストし `PopupExt::position_x()/position_y()`。**親 surface 相対の、windowing system が negotiate 済みの最終位置**(端での flip/slide 後の値)。
- **(D)** `NativeExt::surface_transform(&window)`。

**符号の根拠(実測)**: `GtkNative` の surface transform は「surface 原点から widget 原点へのオフセット(CSS shadow/margin 由来)」であり、widget 座標を surface 座標へ移すには transform を**引く**(surface 内の widget 原点 = widget座標 − transform)。(C) は既に親 surface 相対かつ negotiate 後なので**そのまま足す**。macOS quartz 実機で計測:窓 360×762、アンカーボタン `#popover-btn` が y=716(最下部、中心 x≈180)のとき、popover content は **x=129, y=680, w=103, h=15** に合成された。これは「アンカー上に flip して水平センタリングし、4 隅すべて窓内」という物理的に正しい配置で、符号(−B, +C, −D)が正しいことを確認した。CI(xvfb/X11)scenarios でも同じ「4 隅 in 窓 + アンカー整合」が pass。

### 3. response 表現 — `Bounds` に optional な `basis` を足す

```rust
pub enum BoundsBasis { WindowRoot, PopupComposed }   // serde snake_case

pub struct Bounds {
    pub x: f64, pub y: f64, pub width: f64, pub height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<BoundsBasis>,
}
```

- **通常 widget**: `basis = None` → wire から省略 → **既存 payload は 1 byte も変わらない**(後方互換)。意味は「`window_root`(既定)」。
- **popover widget**: `basis = Some(PopupComposed)` → consumer は「別 surface 由来だが親窓相対の合成値」と判別できる。「取得不能で null」ではなく解決手段と provenance を response で示す。

座標基準は両 variant とも**同一**(親 `GtkWindow` root widget 原点相対)。`basis` は値の出自(provenance)だけを表す。

## Alternatives considered

- **`compute_bounds` の `None` 判定でフォールバック(不採用)**: §1 のとおり X11/Wayland 差で誤 `Some` が返ると fix がサイレント無効化される。surface 同一性で代替。
- **`compute_point` / `translate_coordinates`(不採用・代替不可)**: 内部で同じ `gtk_widget_compute_transform` を使うため別 surface 越えで同様に失敗する。
- **`ElementInfo` 側に `bounds_basis` を置く(不採用)**: basis は論理的に bounds に属するので `Bounds` に置く。
- **popover を別 root として再列挙(不採用)**: walk に既に含まれるので不要(Context 参照)。

## Limitations / 前提

- **(m1) 単段 popover 前提**: `position_x/y` は直近の親 surface 相対なので、popover-in-popover(多段)では式が崩れる。多段対応は親 popup chain を辿って position を累積する必要がある(scope 外)。Brainship 凡例 popover は単段で AC を満たす。
- **(m2) `compute_point`/`translate_coordinates` は代替不可**(上述)。手動合成が必須。
- **(m3) scale=1 のみ検証**: CI(xvfb)・macOS quartz とも scale=1。HiDPI(scale>1)では `position_{x,y}`(device px 相当)と widget 座標(論理 px)の単位ズレリスクが残り**未検証**。
- **(m4) overflow 拘束の現実**: 実 Wayland コンポジタ(及び多くの環境)は `xdg_popup` を自動で画面内に拘束(flip/slide)し、`position_x/y` は拘束後の値を返す。よって**実機で popover が実際にモニタ外へ出ることは通常起きない**。本 API の契約は「**正確な数値を返す**こと」までで、はみ出し判定(4 隅 ∈ 矩形)の純粋関数とその unit test は **consumer 側(Brainship FE / AGX)** が持つ。本当にはみ出すケース(FE の開く方向ロジックがバグって拘束しきれないサイズの popover を出した場合)の end-to-end 回帰検出に、本 API の bounds が使える。framework は座標を返すだけで判定ロジックは持たない。
- **(m5) demo の visual baseline 影響**: demo に `#popover-btn` ボタンを 1 個追加したため、`visual-regression.spec.ts` の `main-window` baseline を再生成した(`packages/demo/scripts/gen-visual-baseline.sh`)。popover は既定で閉じているので baseline フレームには現れない。

## Verification

主ゲートは CI(`.github/workflows/ci.yml`):

- **`rust` job**: build / clippy / fmt / `cargo test --all --features e2e` / schema stale check。新しい Rust 統合テスト(`tests/elements_walk.rs` の popover 系)は `rust` job に display が無いため **常に skip**(`require_display()` が false)。CI 緑の根拠にはしない。
- **`bun` job**: `BoundsBasis` 型 + `basis?` フィールドが `types.gen.ts` に出て `tsc --noEmit` が通る。
- **`scenarios` job(CI 緑の根拠)**: xvfb/X11 下で `popover.spec.ts` が **skip でなく実行され pass**。popover を開き、`#popover-content` の bounds が数値で返り `basis="popup_composed"`、4 隅 in 窓、かつ**アンカーボタンとの幾何整合**(中心 x 一致 + 垂直隣接)を独立オラクルとして検証する(合成式の自己整合に依存しない = 符号ミスを happy path で取りこぼさない)。popover 不開 / content 未検出は skip でなく fail。

## 関連

- 実装: `packages/server/src/elements.rs`(`compute_widget_bounds` / `compose_popover_bounds`)、`packages/server/src/proto.rs`(`BoundsBasis` / `Bounds.basis`)。
- 依拠 API: `NativeExt::surface_transform`/`surface`、`PopupExt::position_x/position_y`、`WidgetExt::native`(すべて pinned `gtk4 0.9 / v4_6`、Cargo.toml 変更不要)。
- [Gdk.Popup](https://docs.gtk.org/gdk4/iface.Popup.html) / [Gtk.Native](https://docs.gtk.org/gtk4/iface.Native.html) / [Gtk.Popover](https://docs.gtk.org/gtk4/class.Popover.html)。
