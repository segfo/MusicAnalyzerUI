# MusicAnalyzer WebUI

楽曲解析結果を可視化・試聴するための Web フロントエンドです。

![](usage.png)


## 技術スタック

| 分類 | 技術 |
|------|------|
| UI フレームワーク | [Leptos](https://leptos.dev/) 0.6 (Rust / WebAssembly・CSR モード) |
| デスクトップシェル | [Tauri](https://tauri.app/) 2.x |
| スタイリング | Tailwind CSS (JIT) |
| オーディオ | Web Audio API (`web-sys` 経由) |
| バックエンド通信 | Tauri IPC (デスクトップ) / HTTP REST (Web) を自動切り替え |

### 実行モード

- **Tauri デスクトップ版** — `cargo tauri dev` で起動。ファイルシステムへの直接アクセスが可能
- **Web 版** — `trunk serve` でフロントエンドを起動し、Python FastAPI バックエンド (`uvicorn main:app --port 7777`) と組み合わせて利用

---

## アプリケーションの特徴

### 楽曲解析の表示
BPM・ビート/ダウンビート・セクション（イントロ、Aメロ、サビ等）・コード進行など、バックエンドで解析された情報を視覚的に表示します。各セクションはカラーコードで区別され、キャプションや拍数も確認できます。

### インタラクティブ再生
- シークバー・音量スライダー・再生/一時停止ボタン
- **スペースキー**でトグル再生
- タイムライン上の**クリック**でセクション先頭にジャンプ、**Shift+クリック**で任意位置にシーク

### ステム分離ミキサー
ボーカル・ドラム・ベース・その他の 4 チャンネルを個別に音量調整できます。ステムファイルが利用可能になると、メインオーディオから自動的に切り替わりシームレスに再生を継続します。

### リアルタイムビジュアライゼーション
Canvas 2D アニメーションがビート・コード・ステム音量にリアルタイムで反応します。
- ビート/ダウンビートに合わせてリング・ポリゴンがパルス
- コード進行に連動した色相変化
- ステム音量で粒子の密度・速度が変動

### ループゾーン設定
- タイムラインで **Ctrl+クリック** するとセクションを複数選択でき、自動的にループ範囲を設定
- ステムミキサーパネルから手動でループ開始/終了位置を現在時刻に設定することも可能

---

## ディレクトリ構造

```
webui/
├── frontend/                # Leptos フロントエンド (Rust/WASM)
│   ├── src/
│   │   ├── main.rs          # アプリエントリ・ルーティング
│   │   ├── state.rs         # グローバル状態管理
│   │   ├── api.rs           # バックエンド通信
│   │   ├── types.rs         # 共有データ型
│   │   ├── audio_setup.rs   # 音声初期化ユーティリティ
│   │   ├── config.rs        # 接続モード設定
│   │   ├── audio/           # Web Audio API ラッパー
│   │   ├── components/      # 再利用可能 UI コンポーネント
│   │   └── pages/           # ページコンポーネント
│   ├── Cargo.toml
│   ├── Trunk.toml           # WASM バンドラ設定
│   └── tailwind.config.js
├── backend/
│   └── src-tauri/           # Tauri バックエンド (Rust)
└── doc/                     # 技術ドキュメント（日本語）
```

---

## コンポーネント一覧

### ページ (`pages/`)

| ファイル | ルート | 概要 |
|----------|--------|------|
| [home.rs](frontend/src/pages/home.rs) | `/` | トラック一覧。BPM・ファイル名でのソート機能付き |
| [analysis.rs](frontend/src/pages/analysis.rs) | `/analysis/:stem` | タイムライン・セクション一覧・コード進行の表示。音声再生・ステム読み込みを管理 |
| [visualization.rs](frontend/src/pages/visualization.rs) | `/visualization/:stem` | Canvas アニメーション＋ステムミキサーを統合したビジュアライゼーション画面 |

### UI コンポーネント (`components/`)

| ファイル | 概要 |
|----------|------|
| [player.rs](frontend/src/components/player.rs) | 再生コントロールバー。再生/一時停止・シークバー・音量・BPM 表示。`PlaybackContext` を提供 |
| [timeline.rs](frontend/src/components/timeline.rs) | セクションタイムライン。クリックでシーク、Shift+クリックで精密シーク、Ctrl+クリックでループ範囲選択 |
| [viz_canvas.rs](frontend/src/components/viz_canvas.rs) | Canvas 2D アニメーション。ビート・コード・ステム音量に連動したパーティクル/リング描画 |
| [stem_mixer.rs](frontend/src/components/stem_mixer.rs) | 4ch ステムスライダー・ビートオフセット調整・ループコントロール |
| [section_card.rs](frontend/src/components/section_card.rs) | 現在再生中のセクション情報をフローティング表示。セクション切り替え時にフェードアニメーション |
| [track_list.rs](frontend/src/components/track_list.rs) | ホームページ用トラックカード。ファイル名・BPM・セクション数・音声有無を表示 |
| [settings.rs](frontend/src/components/settings.rs) | バックエンド接続設定パネル。HTTP ベース URL を入力・localStorage に永続化 |
| [error_display.rs](frontend/src/components/error_display.rs) | エラー表示パネル。リトライコールバックを受け取り再試行ボタンを表示 |

### 状態管理 (`state.rs`)

| 構造体 | 概要 |
|--------|------|
| `GlobalPlayback` | ページをまたいで再生状態を保持。エンジン参照・ステムゲイン・再生位置・音量などを管理 |
| `VisualizationPageState` | 曲ごとのステム音量キャッシュ・ループ設定・セクション選択・ビートオフセットを永続化 |

### API / オーディオ

| ファイル | 概要 |
|----------|------|
| [api.rs](frontend/src/api.rs) | Tauri IPC または HTTP REST にディスパッチするバックエンド通信クライアント |
| [audio/engine.rs](frontend/src/audio/engine.rs) | `AudioEngine`（単一トラック）と `StemAudioEngine`（4ch 同期再生）の Web Audio API ラッパー |
| [audio_setup.rs](frontend/src/audio_setup.rs) | 曲切り替え時の音量復元・ステム準備完了時の AudioEngine → StemAudioEngine ハンドオフ処理 |
| [config.rs](frontend/src/config.rs) | バックエンド接続モード（Auto / Tauri / HTTP）の判定と localStorage への永続化 |
| [types.rs](frontend/src/types.rs) | `TrackDataset`・`SegmentResult`・`ChordResult` など共有データ型の定義 |

---

## 起動方法

### Web 版

```bash
# バックエンド (別ターミナル)
uvicorn main:app --port 7777
# または uv runを使える場合
uv run ./main.py

# フロントエンド
cd frontend
trunk serve
# → http://localhost:8080 でアクセス
```

### Tauri デスクトップ版

```bash
cd backend
cargo tauri dev
```

---

## 詳細ドキュメント

- [doc/Components.md](doc/Components.md) — コンポーネント仕様・状態図
- [doc/ImplementsRule.md](doc/ImplementsRule.md) — アーキテクチャ・実装ルール
- [doc/CodingRule.md](doc/CodingRule.md) — CSS/Tailwind コーディング規約
