# MusicAnalyzer WebUI — コンポーネント・モジュール説明

---

## state.rs — グローバル再生状態

### `GlobalPlayback`

アプリ全体で共有する再生状態。`App` コンポーネントで生成し `provide_context` で提供する。
ページ遷移（Analysis ↔ Visualization）をまたいでエンジンとシグナルが生き続ける。

| フィールド | 型 | 説明 |
|-----------|---|------|
| `loaded_stem` | `RwSignal<String>` | 現在ロード中の stem 名。空文字 = 未ロード |
| `current_time` | `RwSignal<f64>` | 再生位置（秒） |
| `is_playing` | `RwSignal<bool>` | 再生中フラグ |
| `duration` | `RwSignal<f64>` | トラック長（秒） |
| `volume` | `RwSignal<f64>` | 音量 (0.0–1.0) |
| `engine` | `StoredValue<Option<AudioEngine>>` | メイン AudioEngine |
| `stem_engine` | `StoredValue<Option<StemAudioEngine>>` | ステム AudioEngine |
| `stem_gains` | `StoredValue<Option<StemGains>>` | ステム別 GainNode |
| `stems_available` | `RwSignal<bool>` | ステムファイルの利用可否 |

**主要メソッド**:
- `is_loaded(stem)` — 指定 stem が既にロード済みか（再フェッチスキップ判定）
- `stop_playback()` — 停止・位置リセット（エンジンは保持）
- `clear()` — 完全クリア（別 stem ロード前に呼ぶ）

---

## api.rs — バックエンド API クライアント

すべてのネットワーク通信をこのモジュールに集約。

| 関数 | 説明 |
|-----|------|
| `fetch_tracks()` | トラック一覧取得 |
| `fetch_track(stem)` | 1 トラックのメタデータ取得 |
| `audio_url(stem)` | 音声ファイルの URL を返す（`<audio>` タグ用） |
| `fetch_audio_array_buffer(stem)` | 音声を ArrayBuffer で取得（Web Audio デコード用） |
| `fetch_stem_availability(stem)` | ステムファイルの存在確認 |
| `fetch_stem_array_buffer(stem, track)` | ステム音声を ArrayBuffer で取得 |

---

## types.rs — 共有データ型

### データ構造

| 構造体 | 説明 |
|-------|------|
| `TrackDataset` | 1 曲のフル解析データ（セグメント・BPM・ビート・コード等） |
| `TrackSummary` | 一覧表示用の要約データ |
| `SegmentResult` | 1 セクション（Verse/Chorus 等）の情報 |
| `ChordResult` | 1 コード区間の情報 |
| `StemAvailability` | ステムファイルの存在フラグ（vocals/drums/bass/other） |
| `SubCaption` | セクション内の字幕単位 |
| `OverallDescription` | 楽曲全体の説明文 |

### ユーティリティ関数

| 関数 | 説明 |
|-----|------|
| `segment_color(label)` | セクションラベル → Tailwind BG クラス |
| `chord_hue(label)` | コードラベル → HSL の Hue 値 (0–360) |
| `format_time(secs)` | 秒数 → `"m:ss"` 形式の文字列 |

---

## components/player.rs — 音声エンジン・プレイヤー

### `AudioEngine`

Web Audio API をラップした再生エンジン。

| メソッド | 説明 |
|---------|------|
| `new(ctx, buffer)` | AudioContext と AudioBuffer からエンジンを生成 |
| `play()` | 現在位置から再生開始 |
| `pause()` | 一時停止 |
| `seek(time)` | 指定秒数にシーク（再生中なら即座に切り替え） |
| `current_time()` | 現在の再生位置（秒） |
| `duration()` | トラック長（秒） |
| `is_playing()` | 再生中かどうか |
| `set_volume(v)` | 音量設定（GainNode 経由） |
| `resume_ctx()` | AudioContext を Resume（ブラウザの Autoplay Policy 対策） |

### `PlaybackContext`

Player / Timeline / SectionCard が共有する再生状態コンテキスト。
`provide_context` / `use_context` で受け渡す。

| フィールド | 説明 |
|-----------|------|
| `current_time / set_current_time` | 再生位置シグナル |
| `is_playing / set_is_playing` | 再生状態シグナル |
| `duration / set_duration` | トラック長シグナル |
| `volume / set_volume` | 音量シグナル |
| `current_segment_idx` | 現在再生中のセクションインデックス（Memo） |
| `engine` | AudioEngine への StoredValue |

### `Player` コンポーネント

Analysis ページ用のプレイヤーバー。

- 再生/一時停止ボタン（`PlaybackContext.engine` を制御）
- シークバー（クリックで任意位置へ）
- 時刻表示、音量スライダー
- トラック名・BPM 表示

---

## components/timeline.rs — タイムライン

### `Timeline` コンポーネント

Analysis ページ用のセクションタイムライン。

**操作**:
- **通常クリック（セグメントバー）**: そのセクションの先頭にジャンプ
- **Shift+クリック（任意の場所）**: クリックした正確な時刻にシーク

`create_node_ref` でコンテナ要素を参照し、Shift+click 処理はコンテナ側に一本化している（セグメントバーのクロージャでは `ev.current_target()` が信頼できないため）。

---

## components/section_card.rs — セクションカード

再生中のセクション情報をフローティングカードとして表示。
`PlaybackContext.current_segment_idx` を監視して自動更新。

---

## components/track_list.rs — トラックカード

### `TrackCard` コンポーネント

ホーム画面の1曲分のカード。BPM・セクション数・解析日時を表示し、Analysis ページへのリンクを持つ。

---

## components/viz_canvas.rs — Canvas 2D アニメーション

### `VizCanvas` コンポーネント

ビジュアライズページの Canvas 2D アニメーション。`requestAnimationFrame` ループで毎フレーム描画。

**描画要素**:

| 要素 | 制御シグナル |
|-----|------------|
| 背景グラデーション | `current_hue`（コード → 色相） |
| 同心リング（2本） | `downbeat_trigger`（ダウンビート） × `stem_volumes.bass` |
| 中心多角形（8角形） | `beat_trigger`（ビート） × `stem_volumes.drums` |
| 頂点の歪み | `distortion_seed` × `stem_volumes.vocals` |
| パーティクル | ビートバースト + `stem_volumes.others` + `energy` / `density` |

**パーティクル仕様**:
- 生成: ビート時にバースト（`beat_pulse × 20 × drums`）、ダウンビートで追加（`downbeat_pulse × 12 × bass`）、常時少量（`density × others × energy × 1.5`）
- スポーン位置: 多角形縁（`base_r × 0.9–1.1`）から外側へ放射
- 速度: ビートバースト時 4–8 px/frame、常時 1.5–3 px/frame
- 摩擦: 0.91–0.97 / フレームで指数減速
- 寿命: 粒子ごとにランダム（約 0.7–1.3 秒）

**内部状態** (`AnimState`):
- `beat_pulse` / `downbeat_pulse` — パルス値（毎フレーム × 0.85/0.88 で減衰）
- `distortion_seed` — 多角形頂点の揺れ量
- `hue` / `energy` / `density` — ターゲット値に向けて毎フレーム lerp
- `rng` — LCG 擬似乱数

---

## components/stem_mixer.rs — ステムミキサー

### `StemMixer` コンポーネント

Visualization ページ右サイドパネル。4 本のステムスライダー + Beat Offset + ループコントロールを含む。

### `StemSlider` コンポーネント（内部）

各ステムの音量スライダー行。2行構成:
- 1行目: アイコン・ラベル・音量値・MUTE ボタン
- 2行目: `w-full` スライダー

`stems_available = false` 時はスライダーを動かしてもビジュアル効果のみ（GainNode は操作されない）。

### `BeatOffsetControl` コンポーネント（内部）

ビートイベントの発火タイミングをずらすスライダー。
範囲: -0.5s ～ +0.5s（0.01s 刻み）。
`VizContext.beat_offset` を更新し、ポーリングループ内のビート判定時刻に加算される。

### `LoopControls` コンポーネント（内部）

ループ区間の設定 UI。
- **◀ Loop Start**: 現在位置をループ開始点に設定
- **Loop End ▶**: 現在位置をループ終了点に設定
- **Loop ON/OFF**: トグル
- **✕ Clear**: ループ解除・セクション選択もクリア

---

## pages/home.rs — ホームページ

トラック一覧表示。BPM・ファイル名でのソート機能あり。
マウント時に `GlobalPlayback.stop_playback()` を呼び、再生を停止する。

---

## pages/analysis.rs — 解析ページ

**URL**: `/analysis/:stem`

**構成コンポーネント**:
```
Analysis
├── Player            ← 上部プレイヤーバー
├── Timeline          ← セクションタイムライン
├── AnalysisContent   ← メタデータ・セクション一覧
└── SectionCard       ← 右下フローティングカード
```

**初期化フロー**:
1. `GlobalPlayback.is_loaded(stem)` チェック
2. 未ロードの場合のみ音声をフェッチ・デコードし `global.engine` に保存
3. `global.xxx.read_only()` / `write_only()` から `PlaybackContext` を構築
4. 100ms ポーリングで `current_time` 更新・トラック終端検出

---

## pages/visualization.rs — ビジュアライズページ

**URL**: `/visualization/:stem`

**構成コンポーネント**:
```
Visualization
├── VizPlayer         ← 上部プレイヤーバー（オレンジテーマ）
├── VizTimeline       ← ループオーバーレイ付きタイムライン
├── VizCanvas         ← Canvas 2D アニメーション（左側メインエリア）
└── StemMixer         ← 右サイドパネル
```

### 内部コンポーネント

#### `VizPlayer`

Analysis の `Player` と同等だが Visualization 専用のデザイン（オレンジテーマ）。
StemAudioEngine と AudioEngine の両方を制御。
「Analysis」「Visualize●」のページ切り替えボタンを内包。

#### `VizTimeline`

`Timeline` の拡張版。追加機能:
- **Ctrl+クリック**: セクション選択（複数選択可）→ 自動でループ区間に設定
- **ループオーバーレイ**: 橙色の半透明帯でループ区間を表示
- `create_node_ref` によるコンテナ参照でシークを確実に動作させる

**セクション選択 → ループ連動**:
`create_effect` が `selected_segment_indices` を監視し、選択セクション群の min(start)〜max(end) を自動的にループ区間に設定する。

### データ型

| 型 | 説明 |
|---|------|
| `StemVolumes` | 各ステムのビジュアル強度（0.0–1.0）。stems 非利用時もビジュアルのみ制御 |
| `StemGains` | 各ステムの GainNode（stems 利用可能時のみ Some） |
| `StemAudioEngine` | 4 ステム同期再生エンジン |
| `VizContext` | Visualization ページ専用コンテキスト |

### `VizContext` フィールド一覧

| フィールド | 説明 |
|-----------|------|
| `energy / density` | セクション由来のビジュアル強度（lerp でスムーズ遷移） |
| `current_hue` | コード由来の色相（0–360） |
| `beat_trigger` | ビートのたびにインクリメント（u32、折り返しあり） |
| `downbeat_trigger` | ダウンビートのたびにインクリメント |
| `stem_volumes` | ステム別ビジュアル強度 |
| `stems_available` | ステムファイルが利用可能か |
| `loop_start / loop_end` | ループ区間（None = 未設定） |
| `loop_active` | ループ有効フラグ |
| `selected_segment_indices` | Ctrl+クリックで選択中のセクションインデックス一覧 |
| `stem_gains` | ステム別 GainNode（`GlobalPlayback.stem_gains` と同一） |
| `stem_engine` | StemAudioEngine（`GlobalPlayback.stem_engine` と同一） |
| `beat_offset` | ビート発火タイミングオフセット（秒） |

### ポーリングループ（100ms）

毎 100ms に以下を実行:
1. `current_time` 更新（StemAudioEngine または AudioEngine から取得）
2. トラック終端検出 → 停止
3. ビート/ダウンビート検出 → `beat_trigger` / `downbeat_trigger` インクリメント
4. セクション変化検出 → `energy` / `density` のターゲット値更新
5. コード変化検出 → `current_hue` のターゲット値更新
6. ループ制御 → 終端到達時に `loop_start` にシーク

### セクションエネルギーマッピング

| セクション | energy | density |
|-----------|--------|---------|
| intro | 0.2 | 0.2 |
| verse | 0.6 | 0.5 |
| chorus / refrain | 1.2 | 1.0 |
| bridge | 0.7 | 0.4 |
| break | 0.5 | 0.3 |
| solo | 0.9 | 0.7 |
| pre-chorus | 0.8 | 0.6 |
| その他 | 0.8 | 0.6 |
| outro | 0.3 | 0.2 |

---

## backend/main.py — FastAPI サーバー

### 定数・設定

| 定数 | 説明 |
|-----|------|
| `MUSIC_DIR` | 音声ファイルのルートディレクトリ |
| `OUTPUT_DIR` | 解析 JSON の出力ディレクトリ |
| `STEMS_DIR` | ステムファイルのルートディレクトリ（`MUSIC_DIR/stems/`） |
| `AUDIO_EXTS` | 対応音声拡張子（.mp3/.wav/.flac/.ogg） |
| `STEM_TRACKS` | ステムトラック名一覧（vocals/drums/bass/other） |

### エンドポイント

#### `GET /api/tracks`
全トラック一覧を JSON で返す。`OUTPUT_DIR` の `*.json` を検索。

#### `GET /api/tracks/{stem}`
指定 stem の解析 JSON を返す。

#### `GET /api/audio/{stem}`
音声ファイルをストリーミング返却。HTTP Range リクエスト対応（`206 Partial Content`）。

#### `GET /api/stems/{stem}/{track_name}` ★先に定義
ステムファイルをストリーミング返却。`track_name` は vocals/drums/bass/other のいずれか。
`{stem:path}` のグリーディマッチ問題を避けるため、このルートを availability ルートより**先に**定義すること。

#### `GET /api/stems/{stem}`
ステムファイルの存在確認。`{ vocals: bool, drums: bool, bass: bool, other: bool }` を返す。
