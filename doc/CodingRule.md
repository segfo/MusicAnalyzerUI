---
name: CSS animation (fill-mode: both) overrides inline opacity styles
description: CSS animation with fill-mode:both locks the animated property and overrides inline styles. Separate animation and dynamic styling into different elements.
type: feedback
---

# MusicAnalyzer WebUI — Frontend

Tailwind JIT は `tailwind.config.js` の content に指定された `./src/**/*.rs` をスキャンする。`format!("{} opacity-25", base)` のようにクラス名が完全な文字列リテラルとして現れていれば JIT が検出して CSS を生成するため問題なく動作する。

**Why:** 以前 opacity-25 が効かないと思ったが、実際の原因は CSS animation の fill-mode による上書きだった（別メモリ参照）。Tailwind 動的クラス自体は正しく動作する。

**How to apply:** クラス名を完全な文字列として書く。途中で切ったり変数に分割するのはNG。

```rust
// OK: 完全なクラス名がリテラルとして存在する
format!("{} opacity-25", base)
format!("{} opacity-100", base)

// NG: Tailwind が "opacity-" しか見つけられない
format!("opacity-{}", value)
```
