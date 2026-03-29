# MusicAnalyzer WebUI — 実装ルール

## 1. スタック概要

| レイヤー | 技術 | 役割 |
|---------|------|------|
| フロントエンド | Rust + Leptos 0.6 (CSR/WASM) | UI・再生・ビジュアライズ |
| スタイル | Tailwind CSS (Play CDN) | ユーティリティ CSS |
| バックエンド | Python + FastAPI | 音声・メタデータ配信 |
| ビルド | trunk | WASM バンドル |

---

## 2. ディレクトリ構成

```
webui/
├── backend/
│   └── main.py              # FastAPI サーバー
└── frontend/
    ├── Trunk.toml
    ├── Cargo.toml
    └── src/
        ├── main.rs           # App エントリ・ルーティング・GlobalPlayback 初期化
        ├── state.rs          # GlobalPlayback（アプリ全体の再生状態）
        ├── api.rs            # バックエンド API クライアント
        ├── types.rs          # 共有データ型
        ├── components/
        │   ├── player.rs     # AudioEngine / PlaybackContext / Player コンポーネント
        │   ├── timeline.rs   # Timeline コンポーネント
        │   ├── section_card.rs
        │   ├── track_list.rs
        │   ├── viz_canvas.rs # Canvas 2D アニメーション
        │   └── stem_mixer.rs # ステムミキサー UI
        └── pages/
            ├── home.rs       # トラック一覧
            ├── analysis.rs   # 解析ページ (/analysis/:stem)
            └── visualization.rs  # ビジュアライズページ (/visualization/:stem)
```

---

## 3. ルーティング規則

```
/                       → pages::home::Home
/analysis/:stem         → pages::analysis::Analysis
/visualization/:stem    → pages::visualization::Visualization
```

- `:stem` はトラックのファイル名（拡張子なし）を URL エンコードしたもの
- `js_sys::encode_uri_component` でエンコードし、バックエンド側で自動デコード

---

## 4. 状態管理

### 4-0. 状態の分類戦略

状態は3層に分類し、それぞれ適切な場所に置く。

| 分類 | 型 | 置き場 | 寿命 | 例 |
|------|---|--------|------|---|
| 再生状態 | `GlobalPlayback` | `state.rs` + `main.rs` | App 全体 | current_time, is_playing, volume, 音声エンジン |
| ページUI設定 | `XxxPageState` | `state.rs` + `main.rs` | App 全体 | ステムミキサー音量, ビートオフセット, ループ設定 |
| エフェメラル描画値 | ローカルシグナル / `XxxContext` | ページ/コンポーネント内 | マウント中のみ | energy, current_hue, beat_trigger |

**ページUI設定の追加手順（新ページが設定永続化を必要とする場合）**:
1. `state.rs` に `FooPageState` struct を定義（`RwSignal<T>` フィールド + `new()` + 必要なリセットメソッド）
2. `main.rs` の `App` コンポーネントに `provide_context(FooPageState::new())` を追加
3. 対象ページの先頭で `use_context::<FooPageState>()` を取得
4. `.read_only()` / `.write_only()` でシグナルを分割し、ページ内 Context struct に詰めて `provide_context`

**リセットのルール**:
- トラック非依存の設定（音量・タイミング）: 保持。リセット不要
- トラック依存の設定（ループゾーン・選択状態）: 別ステムロード時のみリセット
  - `create_effect` 内で `global.clear()` を呼ぶ直前に `page_state.reset_xxx()` を呼ぶ
  - `main_audio_res` が `Ok(None)` を返すとき（同一ステム再訪）はエフェクト本体に到達しないのでリセットされない

### 4-1. GlobalPlayback（`src/state.rs`）

アプリ生存期間中に `App` コンポーネントで生成し `provide_context` で提供する。
ページ遷移をまたいで再生エンジンとシグナルを保持する唯一のオブジェクト。

```
App
├── provide_context(GlobalPlayback)       # 再生状態（全ページ共通）
│   ├── loaded_stem: RwSignal<String>   # 現在ロード済みの stem 名
│   ├── current_time / is_playing / duration / volume: RwSignal<f64|bool>
│   ├── engine: StoredValue<Option<AudioEngine>>      # メイン AudioEngine
│   ├── stem_engine: StoredValue<Option<StemAudioEngine>>
│   ├── stem_gains: StoredValue<Option<StemGains>>
│   └── stems_available: RwSignal<bool>
│   └── stems_loading: RwSignal<bool>          # ステムロード中フラグ（UIロック用）
└── provide_context(VisualizationPageState)  # Analysis / Visualization 共通 UI 設定
    ├── stem_volumes: RwSignal<StemVolumes>  # ステムミキサー音量（トラック非依存）
    ├── beat_offset: RwSignal<f64>           # ビートオフセット（トラック非依存）
    ├── loop_start / loop_end: RwSignal<Option<f64>>  # ループゾーン（トラック依存）
    ├── loop_active: RwSignal<bool>
    └── selected_segment_indices: RwSignal<Vec<u32>>
```

**ルール**:
- 再生機能を持つページ（Analysis / Visualization）は必ずサイドバー（`StemMixer`）を表示する
- 再生機能を持つページは必ず `VizContext` を `provide_context` し、`StemMixer` が `use_context` できるようにする
- 再生機能を持つページはステムロード中に `global.stems_loading = true` となり、UIロックオーバーレイを表示する
- ページコンポーネントは `GlobalPlayback` を `use_context` で取得する
- 再生シグナルは `global.xxx.read_only()` / `write_only()` で分割して `PlaybackContext` に渡す
- 同じ stem に遷移した場合は `is_loaded()` で判定し、音声の再フェッチを行わない
- 別 stem に遷移する場合は `viz_page_state.reset_loop()` を呼んでから `global.clear()` を呼ぶ
- ホームへ戻る場合は `global.stop_playback()` を呼ぶ（`home.rs` で実施済み）

### 4-2. PlaybackContext（`src/components/player.rs`）

ページ内で `provide_context` し、Player / Timeline / SectionCard が `use_context` で読む。
`GlobalPlayback` のシグナルを分割して渡すため、実体は GlobalPlayback と同一ノードを参照する。

### 4-3. VizContext（`src/pages/visualization.rs`）

再生機能を持つ全ページ（Analysis / Visualization）で `provide_context` するコンテキスト。
ビート・コード・エネルギー・ループ・ステム音量など、ビジュアル制御シグナルを保持する。
StemMixer / VizCanvas / VizTimeline が `use_context` で読む。

- Analysis ページでは VizCanvas を持たないため、エフェメラルシグナル（energy 等）は初期値のまま更新しない。
- VisualizationPageState から取得した永続シグナル（stem_volumes / beat_offset / loop_* など）は Analysis でも共有されるため、Visualization に遷移したときにそのまま引き継がれる。

各シグナルの出所:
- `stem_volumes` / `beat_offset` / `loop_*` / `selected_segment_indices`
  → `VisualizationPageState` から `.read_only()` / `.write_only()` で分割（永続）
- `energy` / `density` / `current_hue` / `beat_trigger` / `downbeat_trigger`
  → `Visualization()` 内でローカル `create_signal`（エフェメラル、再計算可能）
- `stems_available` / `stem_gains` / `stem_engine`
  → `GlobalPlayback` から参照

---

## 5. 音声エンジン

### AudioEngine（`src/components/player.rs`）

- Web Audio API の `AudioContext` + `AudioBufferSourceNode` をラップ
- シーク時は既存 source を stop して新しい source を作り直す（Web Audio の仕様）
- `StoredValue<Option<AudioEngine>>` として `GlobalPlayback.engine` に格納
- Analysis / Visualization どちらのページでも使用（stems がない場合のフォールバックを含む）

### load_stems（`src/pages/visualization.rs`）

`pub async fn load_stems(global: GlobalPlayback, stem_key: String)` — Analysis / Visualization 両ページから `spawn_local` で呼ぶ共有ヘルパー。

- `stem_engine` が既に Some ならスキップ（再ロード不要）
- 実行中は `global.stems_loading = true`（UI ロックオーバーレイが表示される）
- メイン AudioEngine のミュートはここでは行わない（Visualization 側の `create_effect` が `stems_available` を監視してミュートする）

### StemAudioEngine（`src/pages/visualization.rs`）

- 4 本（vocals / drums / bass / other）の `AudioBuffer` を同一 `AudioContext` 上で同期再生
- 同期の保証: すべての source を同一の `ctx_time_at_start` で `start_with_when_and_grain_offset` する
- `StoredValue<Option<StemAudioEngine>>` として `GlobalPlayback.stem_engine` に格納
- stems が利用可能な場合にメイン AudioEngine を mute(volume=0) して切り替える

**重要**: `AudioBuffer` はデコードに使用した `AudioContext` 上でのみ再生可能。
必ず `stem_ctx` を先に作成し、全バッファを同じ `stem_ctx` でデコードすること。

---

## 6. API クライアント規則（`src/api.rs`）

- バックエンドとの通信はすべてこのモジュールに集約する
- `gloo_net::http::Request` を使用
- stem 名は必ず `encode(s)` でエンコードしてから URL に埋め込む
- バイナリ取得（音声）は `.binary()` → `Uint8Array::from` → `.buffer()` の順で `ArrayBuffer` に変換

---

## 7. バックエンド API 規則（`webui/backend/main.py`）

| エンドポイント | 説明 |
|-------------|------|
| `GET /api/tracks` | トラック一覧 |
| `GET /api/tracks/{stem}` | 1 トラックのメタデータ（JSON） |
| `GET /api/audio/{stem}` | 音声ファイル（Range リクエスト対応） |
| `GET /api/stems/{stem}/{track_name}` | ステムファイル（vocals/drums/bass/other） |
| `GET /api/stems/{stem}` | ステム利用可能フラグ（JSON） |

**ルート定義順序の規則**:
`{stem:path}/{track_name}` は `{stem:path}` より**前**に定義すること。
FastAPI の `{path}` 型パラメータはグリーディマッチするため、後から定義したルートは到達されない。

---

## 8. コンポーネント設計規則

- コンポーネントは Props を所有権ごと受け取る（`&str` ではなく `String` / `TrackDataset` など）
- `use_context` で取得した値は `.clone()` して各クロージャに移動させる
- 複数のクロージャで同じ関数型ジェネリクス（`Fn()` など）を使う場合は必ず `Clone` バウンドを追加し、クロージャごとに `.clone()` する
- `NodeRef` を使ったDOM要素アクセスは `ev.current_target()` より信頼性が高い（Leptos のイベント委譲と相性が悪いため）
- Canvas 要素への参照は `unchecked_ref::<HtmlCanvasElement>().clone()` で取得する

---

## 9. イベントハンドリング規則

- タイムライン上のクリック処理は `create_node_ref` + コンテナ側ハンドラで一本化する
- セグメントバーが `z-index` を持つオーバーレイ div と重なるとクリックが遮断される。`z-0` の overlay div は絶対に置かない
- Shift+click のシーク: セグメントバーではバブリングに任せ、コンテナ側の `on:click` で `shift_key()` チェックして処理する

---

## 10. Web Audio API 規則

- `AudioBufferSourceNode` は **一度しか再生できない**。シーク・再開のたびに新しいノードを作成する
- `AudioContext` は必ずユーザー操作（click 等）の後に `.resume()` すること（ブラウザの Autoplay Policy）
- `resume()` は `async fn resume_ctx()` 経由で `spawn_local` + `await` する
- 音量制御は `GainNode.gain().set_value(v as f32)` で行う（`f64` ではなく `f32`）

---

## 11. CSS / スタイル規則

- Tailwind CSS のユーティリティクラスのみ使用（インライン `style=` はアニメーション計算値のみ許可）
- カラーパレット: グレー系 `gray-950 / 900 / 800 / 700`、アクセント `orange-500 / 600`
- StemMixer パネル幅は `w-64`（256px）固定。スライダーを含む行は **2行構成**（1行目: ラベル・値・ボタン / 2行目: `w-full` スライダー）にすること
- 固定幅要素の合計がパネル幅を超えるとスライダーが画面外にはみ出すため注意

---

## 12. ビルド・起動手順

### クライアント単体での起動

#### Tauriビルド(開発ビルド)

```bash
cd webui/backend/src_tauri
cargo tauri dev
```

### Webベースでの起動

#### バックエンド
```bash
cd webui/backend
uv run uvicorn main:app --host 127.0.0.1 --port 7777 --reload
```

#### フロントエンド 
```
cd webui/frontend
trunk serve

# ブラウザ
http://localhost:8080
```
