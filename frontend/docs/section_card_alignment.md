# SectionCard 水平位置合わせ — ズレの原因と設計判断

## 問題の構造

SectionCard を「後ろのセクションリストアイテムと同じ幅・位置に合わせる」ことは、
一見シンプルに見えるが、以下の2つの独立した原因が重なっているため CSS 固定値では解決できない。

---

## 原因 1: `max-w-4xl mx-auto` による中央寄せ

`AnalysisContent` のルート div（`src/pages/analysis.rs:204`）は `max-w-4xl mx-auto` でラップされており、
コンテンツ幅が 896px を超えるウィンドウでは**セクションリストが中央寄せになる**。

```
┌──────────────────────────────────────────────────────┐  ← flex-1（サイドバー分を除いた残り幅）
│           ┌────────────── 896px ──────────────┐      │
│   空白    │  セクションリストアイテム          │  空白 │  ← max-w-4xl mx-auto が中央寄せ
│           └──────────────────────────────────┘      │
└──────────────────────────────────────────────────────┘
```

このとき `left-6`（24px 固定）や `right-[280px]`（固定値）は常に **flex-1 の左右端** を基準に
置かれるため、中央寄せされたリストアイテムとはズレる。

---

## 原因 2: スクロールバーによる幅の縮小

セクションリストは `overflow-y-auto` のスクロール可能な div の中にある。
コンテンツが overflow するとスクロールバーが現れ、**コンテンツ幅が scrollbar_width（Windows: ~17px）分だけ縮む**。

```
┌─────────────────────── scrollable div ───────────────┐
│  p-6(24px) │ ← コンテンツ幅（スクロールバーで縮む）　　  │▐▐│
│            │  セクションリストアイテム               　 │  │ ← スクロールバー
│  p-6(24px) │                                         │▐▐│
└──────────────────────────────────────────────────────┘
```

SectionCard は `position: absolute` でスクロール div の**外側**に配置されているため、
スクロールバーの有無・幅を CSS レベルで知る方法がない。

---

## なぜ CSS 構造変更（layout restructuring）では解決しなかったか

「SectionCard を `max-w-4xl mx-auto` ラッパーの **内側**に移し、`left-6 right-6` で合わせる」
というアプローチを試みた。原因 1 は解決するが、原因 2 が残る。

```
┌────── max-w-4xl wrapper (relative) ──────┐
│  ┌── overflow-y-auto (scrollable) ─────┐ │
│  │  p-6 │ content │ p-6 │ scrollbar │  │ │  ← スクロールバーは content を縮める
│  └──────────────────────────────────────┘ │
│  SectionCard: left-6(24px) ... right-6(24px)│  ← scrollbar 分だけ広い
└──────────────────────────────────────────┘
```

SectionCard の `right-6`（24px）は wrapper の右端から 24px の位置だが、
セクションリストアイテムの右端は `24px + scrollbar_width` の位置になる。
→ CSS の `right` に固定値を使う限り、scrollbar_width 分のズレは回避できない。

---

## `scrollbar-gutter: stable` でも解決しない理由

`scrollbar-gutter: stable` を使うと、スクロールバーの有無にかかわらずガター幅が
常に予約される。これで**幅は安定する**が、ガター幅の実際の px 値を CSS 時点では知れないため、
SectionCard の `right` に足すべき値が不明のまま。

---

## 現在の解決策: `measure_viewport_x` による動的実測

```rust
fn measure_viewport_x(seg_id: &str) -> (f64, f64) {
    // seg-item の getBoundingClientRect() でビューポート座標を取得し、
    // left はそのまま、right は window.innerWidth - rect.right() として返す
}
```

- `seg-item-{index}` の実際のビューポート座標を計測することで、
  **max-w-4xl 中央寄せ** と **スクロールバー幅** の両方が自動的に反映される
- セグメント切替・トグル・再生開始の各 Enter アニメーション前に呼ばれ、
  取得した `(left, right)` を `pos_left` / `pos_right` シグナルに反映する

### ウィンドウリサイズへの対応: ResizeObserver

カード表示中にウィンドウリサイズが起きた場合も追従できるよう、
`section-card-container` 要素を `ResizeObserver` で監視している（`src/components/section_card.rs:359-385`）。

```rust
// 初回レンダリング後に一度だけ ResizeObserver を設定
create_effect(move |prev| {
    if prev.is_some() { return; }
    // section-card-container を observe し、
    // リサイズ時に card_showing 中なら measure_viewport_x を再実行
});
```

- クロージャは `resize_closure: StoredValue` で保持（`drop` すると JS 関数ポインタが無効になるため）
- オブザーバーは `resize_observer: StoredValue` で保持し、`on_cleanup` で `disconnect()` する

---

## 参照コード

| ファイル | 関係箇所 |
|----------|----------|
| [src/components/section_card.rs](../src/components/section_card.rs) | `measure_viewport_x()`（line 24）、`pos_left`/`pos_right` シグナル（line 73-74）、Enter アニメーション前の計測呼び出し、ResizeObserver（line 359-385） |
| [src/pages/analysis.rs:136](../src/pages/analysis.rs#L136) | `id="section-card-container"` — ResizeObserver の観測対象 |
| [src/pages/analysis.rs:204](../src/pages/analysis.rs#L204) | `max-w-4xl mx-auto` — セクションリストの中央寄せ原因 |
