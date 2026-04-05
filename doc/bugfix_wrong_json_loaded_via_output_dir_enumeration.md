# バグ修正記録: ユカリ戦で八角形・コード・外周リングが表示されない

**日付:** 2026-04-05  
**修正ファイル:**
- `webui/backend/src-tauri/src/lib.rs`
- `webui/backend/main.py`

---

## 症状

ユカリ戦.mp3（ステムあり）を Visualize 画面で再生すると、パーティクルと Chorus フラッシュは動いているのに、八角形・コードテキスト・外周リングが最初から一切動かない。カナリィ戦.mp3（同じくステムあり）では正常に動く。DevConsole にエラーなし。

---

## 根本原因

仕様は「**音声ファイル名（拡張子なし）を stem キーとして扱う**」であるが、バックエンドのトラック一覧取得処理が `output/` ディレクトリの `.json` ファイルを直接列挙していた。このため `output/` に置かれた `ユカリ戦 copy.json`（beats・chords が空のバックアップファイル）も正規のトラックと同等に扱われ、ソート順で先に来た copy 版が読み込まれた。

---

## 問題のあったコード（Tauri バックエンド）

```rust
// lib.rs — list_tracks コマンド（修正前）

let output_dir = base_dir.join("output");
if !output_dir.exists() {
    return Ok(vec![]);
}

// ❌ output/ 以下の .json をすべて列挙している
// → "ユカリ戦 copy.json" のようなバックアップや一時ファイルも
//   正規トラックと同列に扱われてしまう
let mut stems: Vec<String> = std::fs::read_dir(&output_dir)
    .map_err(|e| e.to_string())?
    .filter_map(|e| e.ok())
    .filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        // 拡張子 ".json" を除いた文字列をそのまま stem とする
        // "ユカリ戦 copy.json" → stem = "ユカリ戦 copy"
        name.strip_suffix(".json").map(|s| s.to_string())
    })
    .collect();
stems.sort(); // アルファベット順ソート
```

## 問題のあったコード（HTTP バックエンド）

```python
# main.py — list_tracks エンドポイント（修正前）

# ❌ output/ ディレクトリの .json を直接 glob している
# → "ユカリ戦 copy.json" も列挙対象になる
stems = sorted(p.stem for p in OUTPUT_DIR.glob("*.json"))
```

---

## なぜ ユカリ戦 だけ壊れたか

ソートを行うと辞書順で `"ユカリ戦 copy"` は `"ユカリ戦"` より **前** に来る（スペースの ASCII コードが日本語文字より小さいため）。

```
ソート結果（抜粋）:
  "ユカリ戦"         ← 正規ファイル（beats=623, chords=162）
  "ユカリ戦 copy"    ← ← バックアップ（beats=0, chords=0）
  ↑ "ユカリ戦 copy" の方が辞書順で先
```

カナリィ戦でも `カナリィ戦 copy.json` は存在したが、copy 版の `track_filename` も `カナリィ戦.mp3` のままであるため、UI から見ると同名エントリが2件あることになる。ただし UI のトラック一覧は先頭から順に表示するため、ソート順で `カナリィ戦 copy` が先に並んでいてもたまたまユーザーが正規版を選択していた（あるいは copy 版が正常データを持っていた）ため症状が出なかった。

---

## バグに至る経緯まとめ

```
1. output/ に "ユカリ戦 copy.json"（beats=0, chords=0）が存在していた

2. list_tracks が output/*.json を全列挙
   → stems = ["ユカリ戦", "ユカリ戦 copy", ...]

3. ソート後、"ユカリ戦 copy" が "ユカリ戦" より前に来る

4. UI は最初にマッチしたエントリを使うため
   "ユカリ戦.mp3" を開こうとすると stem="ユカリ戦 copy" が渡される

5. get_track("ユカリ戦 copy") → output/ユカリ戦 copy.json を読み込む

6. beats=[] chords=[] → ビート/コード検出が一切発火しない

7. パーティクルは RMS ベースで動くため影響を受けず、
   八角形・コード・外周リング（beat/downbeat トリガー依存）だけが動かない
```

---

## 修正内容

**仕様通り「music/ の音声ファイルを正とする」に変更。**  
`output/` の `.json` 列挙ではなく、`music/` の音声ファイルを列挙し、対応する `.json` が存在するものだけをトラックとして返す。

### Tauri バックエンド（修正後）

```rust
// lib.rs — list_tracks コマンド（修正後）

// ✅ music/ ディレクトリの音声ファイルをステムの起点とする
// → output/ に余分な .json があっても列挙されない
let music_dir = base_dir.join("music");
if !music_dir.exists() {
    return Ok(vec![]);
}

let mut stems: Vec<String> = std::fs::read_dir(&music_dir)
    .map_err(|e| e.to_string())?
    .filter_map(|e| e.ok())
    .filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        // 音声ファイルの拡張子を除いた名前を stem とする
        // "ユカリ戦.mp3" → stem = "ユカリ戦"
        for ext in AUDIO_EXTS {
            if name.to_lowercase().ends_with(ext) {
                let stem = name[..name.len() - ext.len()].to_string();
                return Some(stem);
            }
        }
        None
    })
    // 対応する .json が存在しないトラックは除外
    .filter(|stem| resolve_json(base_dir, stem).is_some())
    .collect();
stems.sort();
```

### HTTP バックエンド（修正後）

```python
# main.py — list_tracks エンドポイント（修正後）

# ✅ music/ の音声ファイルを起点として列挙する
# → output/ に余分な .json があっても混入しない
stems = sorted(
    p.stem
    for p in MUSIC_DIR.iterdir()
    if p.suffix.lower() in AUDIO_EXTS and _resolve_json(p.stem) is not None
)
```

---

## 教訓

- **列挙の起点は「正とするデータ」に合わせる**。本システムでは音声ファイル（`music/`）が正であり、JSON（`output/`）は派生物。列挙方向を逆にすると、バックアップや一時ファイルが混入する。
- `output/` への書き込み権限を持つ操作（手動コピー、バックアップスクリプト等）が行われた場合、`*.json` の全列挙は即座に壊れる。
- ファイル名のソートは言語・文化によって直感と異なる順序になりえる（スペースが漢字より前に来るなど）。ソート依存のロジックは最終手段とし、正規のキー（音声ファイル名）で一意に引くこと。
