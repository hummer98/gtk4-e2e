# ADR-0004: `GET /test/elements` の popover (cross-surface) bounds 合成

- **Status**: Accepted
- **Date**: 2026-07-21 (実装は 2026-06-16 `fe8eba3`、本 ADR は事後記録)
- **Confidence**: 中〜高 (75%) — 合成式は macOS quartz + Linux/xvfb (X11) の 2 backend で数値検証済み、X11 は CI の `scenarios` job で継続検証される (§Verification)。100% にしないのは (a) **Wayland 未検証** (CI は X11、本番 consumer は Wayland/AGX、`surface_transform` の符号は backend 依存)、(b) HiDPI (scale>1) 未検証、(c) 多段 popover 未対応のため。

## Context

`GET /test/elements` は widget tree を返し、各ノードの `bounds` (親 `GtkWindow` の root widget 原点相対 px) で consumer が popover の位置を機械検証できる。issue #5 の依頼元 (Brainship prototype) は「SideBar の popover がサイドバー帯 (左 160px) に被らない」ことを screenshot 目視ではなく数値で assert したかった。

しかし `GtkPopover` は `GtkNative` であり、**自前の `GdkSurface`** (Wayland では `xdg_popup`) を持つ。bounds 計算に使う `widget.compute_bounds(window_root)` は 2 widget が同一 surface 上にある (共通座標系に変換できる) ことを前提とするため、popover とその配下では `None` を返し `bounds: {}` になっていた。検証したい対象がちょうど見えない状態だった。

確定事実:

- `GtkPopover` は `set_parent()` / `MenuButton::set_popover()` で親 widget の子になり、`first_child()`/`next_sibling()` の DFS walk に **popover とその content が含まれる**。よって popover を別 root として再列挙する必要はなく、`to_element_info` の bounds 計算だけを直せばよい。
- `compute_point` / `translate_coordinates` も内部で同じ `gtk_widget_compute_transform` を使うため、別 surface 越えでは同様に失敗する。**より単純な API での代替は不可**で、手動合成が必要。

## Decision

`elements.rs::to_element_info` の bounds 計算を **4 分岐**にし、`compute_bounds(window_root)` が `None` を返した場合にだけ GdkPopup geometry からの合成にフォールバックする。

### 1. 4 分岐

1. `compute_bounds(window_root)` が `Some(r)` → **従来どおり採用**。非 popover widget の挙動は完全に不変。
2. `None` かつ自身が `GtkPopover` (`dynamic_cast_ref::<gtk::Popover>()`) → **popover root**。§2 で frame を合成し、子孫へ伝播。
3. `None` かつ検出済み popover の子孫 (`popover_origin = Some(frame)`) → popover root 相対の `compute_bounds(&frame.root)` (同一 surface なので成功する) を root 原点でオフセット。
4. それ以外 → `None` (未 realize 等、従来どおり)。

分岐 2 を分岐 3 の入れ子にしてはならない。popover root が DFS に到達した時点では `popover_origin` はまだ `None` (その root こそが origin の*設定者*) なので、入れ子にすると最初の root が分岐 4 に落ちる。

### 2. 合成式

**popover root** (分岐 2) — `native → surface → gdk::Popup` から:

```text
x = popup.position_x() + toplevel.surface_transform().0
y = popup.position_y() + toplevel.surface_transform().1
w = surface.width()
h = surface.height()
```

`position_*` は親 (toplevel) *surface* 内での popup のオフセットで、windowing system が negotiate 済みの最終位置 (端での flip/slide 後の値)。`surface_transform` は surface → widget 座標への並進なので `widget = surface + transform`、すなわち**加算**。

**popover 子孫** (分岐 3) — root 相対の局所矩形を root 原点に足すだけ:

```text
x = origin.0 + local.x     w = local.width
y = origin.1 + local.y     h = local.height
```

`native()→surface()→popup→surface_transform` の重い解決は **popover root で 1 回だけ**起き、子孫は軽量な `compute_bounds(root)` で済む。root の frame は `PopoverFrame { origin, root }` として DFS に伝播する。

### 3. 純関数への切り出し

算術は `compose_popover_root_bounds` / `compose_child_bounds` に切り出し、GTK 依存の「採取」層 (`popover_root_frame`) と分離する。オフセット計算を実機なしで unit test できる。

### 4. response 表現は変更しない

`Bounds` の形は不変で、popover ノードも通常 widget と同じ `{x, y, width, height}` を返す。座標基準はどちらも親 `GtkWindow` root widget 原点相対。**既存 payload・schema・型は 1 bit も変わらない。**

## Alternatives considered

- **surface 同一性 (`widget.native()` の同値) で分岐する (不採用)**: PR #4 が提案した方式。`compute_bounds` が backend 差で `None` ではなく「誤った `Some(rect)`」を返した場合に分岐が発火せず、fix がサイレントに無効化されるリスクを避けられる利点があった。採用しなかったのは型判定 (`GtkPopover`) のほうが単純で、MVP の完了条件を満たすため。**このリスクは残存する** (§Limitations m5)。
- **`Bounds.basis` で出自 (provenance) を示す (不採用)**: PR #4 が提案した optional な `BoundsBasis` enum。consumer が「同一 surface 由来」と「popup 合成由来」を判別できる。schema 追加を避けるため見送ったが、後方互換 (optional field) で後から追加できる。
- **popover 側 `surface_transform` を差し引いて厳密な content 矩形にする (不採用)**: PR #4 の合成式にあった補正項。macOS quartz の実測ではこれを適用すると popover content が 3px 右にずれ、アンカーとのセンタリング誤差が 0.5px → 3.5px に**悪化**した (§Verification)。CSD shadow margin が実質ゼロの環境では不要。shadow を持つテーマでは再評価が必要。
- **`compute_point` / `translate_coordinates` (代替不可)**: 内部で同じ変換を使うため別 surface 越えでは同様に失敗する。
- **popover を別 root として再列挙 (不要)**: walk に既に含まれる。

## Verification

**macOS / quartz 実機 (2026-07-19、PR #21 で計測)** — demo の `#open-popover` / `#confirm-popover` fixture を開き、合成式の自己整合ではなく**アンカー widget との幾何整合**を独立オラクルとして検証した:

```
window = 360x1051
anchor  #open-popover    x=12  y=1005 w=336 h=34   → 中心 x = 180.0
content #popover-confirm x=133 y=908  w=95  h=34   → 中心 x = 180.5
```

GTK は popover をアンカーに水平センタリングするので、x の符号が反転していれば約 `2*position_x` 横にずれる。**0.5px 一致**により符号は正しいと確認した。popover subtree の実測でも root は content より左右 3px・上 2px 大きいだけで、CSD shadow margin (m2) 由来のずれは観測されなかった。

**CI (xvfb / X11) — 本 ADR と同 PR で追加** — demo に窓上部アンカーの popover fixture (`#bounds-popover-btn` / `#bounds-popover-content`) と scenario `popover-bounds.spec.ts` を追加し、xvfb/X11 の `scenarios` job で **skip せず pass** させた。同じアンカー幾何オラクルでの実測:

```
anchor  #bounds-popover-btn     x=12  y=12 w=336 h=34   → 中心 x = 180.0
content #bounds-popover-content x=136 y=67 w=88  h=17   → 中心 x = 180.0
```

macOS quartz (中心 x=180.0 / 180.5) と X11 (180.0 / 180.0) の**両 backend で符号・センタリング・アンカー隣接 (gapBelow=21) が一致**した。これで合成式は 2 backend で数値検証済みとなった。

**残る CI の空白** — Rust 統合テスト `packages/server/tests/elements_popover_bounds.rs` は依然 popup surface を realize しない環境で skip する (scenario が代替カバーする)。**Wayland は未検証** — CI は X11 (xvfb) であり、本番 consumer の Wayland/AGX とはコンポジタが異なる。X11 で符号が正しいことは Wayland での正しさを保証しない (surface_transform の符号は backend 依存 §Confidence(c))。consumer 側で実機の値を一度確認することを推奨する。

**この過程で判明した backend 依存の 2 点** — (1) 非 autohide popover は X11 で GdkPopup にならず合成されない (§m7)。(2) `elements({selector})` は X11 で open popover 内の widget を返さない (full-tree walk は返す) — selector 到達性の別問題で issue #20 と近縁。scenario は full-tree walk で回避している。どちらも quartz だけ見ていると気づけなかった差で、CI 化の実利だった。

## Limitations / 前提

- **(m1) 単段 popover 前提**: `position_x/y` は直近の親 surface 相対なので、popover-in-popover では式が崩れる。多段対応は親 popup chain を辿って position を累積する必要がある。
- **(m2) root のサイズは surface 全体**: `surface.width()/height()` をそのまま使うため CSD shadow 余白を含みうる。「帯に被らない」系の判定では矩形が大きめ = 安全側 (過検出)。quartz では実質ゼロだった。
- **(m3) scale=1 のみ検証**: HiDPI では `position_{x,y}` と widget 座標の単位ズレリスクが残る。
- **(m4) popover は「画面」に拘束され「窓」には拘束されない**: コンポジタは `xdg_popup` を画面内に flip/slide するが、**窓の矩形内に収まる保証はない**。窓下端近くのアンカーでは popover が正当に窓外へはみ出す。consumer が「窓内に収まる」を assert すると窓の画面上の位置次第で結果が変わる (PR #21 で実際に flip の有無により pass/fail が揺れた)。判定には窓ではなく対象領域の矩形を使うこと。
- **(m5) 分岐が backend 依存**: 分岐 1 の `compute_bounds` が `None` を返すことが合成のトリガなので、将来 GTK/backend が別 surface でも `Some` (親内 allocation 相対等) を返すようになると、合成が発火せず誤った値が静かに返る。§Alternatives の surface 同一性判定はこれを構造的に防げる。
- **(m6) overflow 拘束の現実**: コンポジタが `xdg_popup` を自動で画面内に拘束するため、実機で popover が画面外に出ることは通常起きない。本 API の契約は「**正確な数値を返す**」ことまでで、はみ出し判定の述語とその test は consumer 側が持つ。
- **(m7) 非 autohide popover は合成されない (backend 依存)**: X11/xvfb では `autohide(false)` の非モーダル popover が `GdkPopup` サーフェスとして realize されず、`popover_root_frame` の `dynamic_cast::<gdk::Popup>()` が失敗して `bounds` が null になる (macOS/quartz では GdkPopup になり合成が成功するため、この差は quartz だけ見ていると気づけない)。モーダル (autohide) popover は両 backend で popup サーフェスとして realize される。合成対象にしたい popover は autohide を有効にすること。CI scenario の fixture (`#bounds-popover-btn`) は autohide 有効。

## 関連

- 実装: `packages/server/src/elements.rs` (`compose_popover_root_bounds` / `compose_child_bounds` / `popover_root_frame` / `to_element_info`)
- 依拠 API: `NativeExt::surface_transform`/`surface`、`PopupExt::position_x`/`position_y`、`WidgetExt::native` (すべて pinned `gtk4 0.9 / v4_6`)
- issue #5 (依頼元)、PR #4 (別アプローチ実装、close 済み — `basis` フィールドと CI scenario は同ブランチに残存)、PR #21 (検証、取り下げ済み)
- [Gdk.Popup](https://docs.gtk.org/gdk4/iface.Popup.html) / [Gtk.Native](https://docs.gtk.org/gtk4/iface.Native.html) / [Gtk.Popover](https://docs.gtk.org/gtk4/class.Popover.html)
