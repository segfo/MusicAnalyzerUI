# バグ修正記録: Visualize画面でパーティクル・コードが無反応になる

**日付:** 2026-04-03  
**修正ファイル:** `webui/frontend/src/components/player.rs`

---

## 症状

カナリィ戦.mp3・ユカリ戦.mp3（ステムファイルあり）を Visualize 画面で再生すると、パーティクルもコードも表示されず完全に無反応になる。

ブラウザコンソールに以下のエラーが出力されていた：

```
src\components\player.rs:113:30: could not get stored value
Uncaught RuntimeError: unreachable
```

---

## 根本原因

`Player` コンポーネントの keydown リスナー（Space キーによる再生/一時停止）において、**リスナーのライフタイムとリアクティブスコープのライフタイムが一致していなかった**。

### 問題のあったコード（修正前）

```rust
// player.rs L99-120
let do_toggle_sv = store_value(do_toggle.clone());  // Player スコープに束縛
let cb = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |ev| {
    if ev.code() == "Space" {
        ev.prevent_default();
        do_toggle_sv.with_value(|f| f());  // ← L113: パニック発生箇所
    }
});
web_sys::window().unwrap()
    .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
cb.forget();  // ← リスナーが window に永久に残り続ける
```

### クラッシュに至る流れ

```
1. Analysis ページで Track A を開く
   └─ Player コンポーネントがマウント
   └─ do_toggle_sv（StoredValue）が Player のリアクティブスコープに作成される
   └─ keydown リスナーが window に登録される
   └─ cb.forget() によりリスナーは永続化（クリーンアップなし）

2. Visualization ページへ移動
   └─ Player コンポーネントが再マウント（新しいスコープ）
   └─ 古い Player スコープが破棄される
   └─ 古い do_toggle_sv も破棄される  ← ここが鍵
   └─ ただし古い keydown リスナーは window に残り続ける  ← バグ

3. Space キーを押す / 再生操作をする
   └─ 古いリスナー（cb）が発火
   └─ 破棄済みの do_toggle_sv に with_value() でアクセス
   └─ Leptos が "could not get stored value" でパニック
   └─ WASM はパニックから回復不能
   └─ ビジュアライゼーション機能が完全停止
```

### なぜステムありの曲で再現しやすかったか

ステムファイルが存在するトラック（カナリィ戦・ユカリ戦など）では、ステムロード完了後に `setup_stem_handoff_effect` が `AudioEngine` から `StemAudioEngine` への切り替えを実行する。このタイミングでページ間のナビゲーションが発生しやすく、Player の再マウントが引き起こされていた。

ステムなしのトラックでも理論上は同じ問題が発生しうるが、切り替えの機会が少ないため気づきにくかった。

---

## 修正内容

`cb.forget()` をやめ、`on_cleanup` でコンポーネントのアンマウント時にリスナーを確実に除去するよう変更した。

### 修正後のコード

```rust
// player.rs
let do_toggle_sv = store_value(do_toggle.clone());
use wasm_bindgen::closure::Closure;
let cb = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
    if let Some(target) = ev.target() {
        if let Ok(el) = target.dyn_into::<web_sys::HtmlElement>() {
            let tag = el.tag_name().to_lowercase();
            if tag == "input" || tag == "textarea" { return; }
        }
    }
    if ev.code() == "Space" {
        ev.prevent_default();
        do_toggle_sv.with_value(|f| f());
    }
});
let win = web_sys::window().unwrap();
let cb_ref = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
let _ = win.add_event_listener_with_callback("keydown", &cb_ref);
// Player アンマウント時にリスナーを除去する。
// cb.forget() を使うと window にリスナーが残り続け、Player が再マウントされて
// do_toggle_sv のスコープが破棄された後に発火すると WASM パニックになる。
on_cleanup(move || {
    if let Some(w) = web_sys::window() {
        let _ = w.remove_event_listener_with_callback("keydown", &cb_ref);
    }
    drop(cb);
});
```

### 変更のポイント

| 変更前 | 変更後 | 理由 |
|---|---|---|
| `cb.forget()` | `on_cleanup(move \|\| { remove_event_listener...; drop(cb); })` | コンポーネント破棄と同時にリスナーを除去するため |
| `cb.as_ref().unchecked_ref()` を直接渡す | `js_sys::Function` のクローン `cb_ref` を使う | 追加・削除で同じ関数参照が必要なため |

---

## 教訓: Leptos/WASM における window イベントリスナーの正しい書き方

### やってはいけないパターン

```rust
// NG: cb.forget() はリスナーを永続化してしまう
let cb = Closure::new(move |ev| {
    some_stored_value.with_value(|f| f());  // StoredValue はコンポーネントスコープに依存
});
window.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
cb.forget();  // ← 危険！スコープ破棄後も残り続ける
```

### 正しいパターン

```rust
// OK: on_cleanup でライフタイムをコンポーネントに合わせる
let cb = Closure::new(move |ev| { ... });
let cb_ref = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
window.add_event_listener_with_callback("keydown", &cb_ref).unwrap();
on_cleanup(move || {
    if let Some(w) = web_sys::window() {
        let _ = w.remove_event_listener_with_callback("keydown", &cb_ref);
    }
    drop(cb);
});
```

`cb.forget()` は Rust の所有権管理から切り離してクロージャを永続させる手段だが、クロージャの中で **Leptos のリアクティブプリミティブ（`StoredValue`, `RwSignal` 等）を参照している場合は使ってはならない**。これらはコンポーネントのリアクティブスコープに束縛されており、スコープが破棄された後にアクセスすると WASM パニックになる。

`window` や `document` のような**グローバルオブジェクトへのイベントリスナー**は特に注意が必要。コンポーネントにスコープされた `create_effect` 内の `addEventListener` と異なり、コンポーネントが破棄されても自動的には除去されないため、必ず `on_cleanup` とセットで使うこと。
