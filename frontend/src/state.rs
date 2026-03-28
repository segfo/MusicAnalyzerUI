use crate::audio::{AudioEngine, StemAudioEngine, StemGains, StemVolumes};
use leptos::*;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// GlobalPlayback — アプリ全体で共有する再生状態
// App コンポーネントで生成し provide_context で提供する。
// ページをまたいでエンジン・シグナルが生き続けることで、
// Analysis ↔ Visualization 間の再生継続を実現する。
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct GlobalPlayback {
    /// 現在ロード済みの stem 名（空文字 = 未ロード）
    pub loaded_stem: RwSignal<String>,

    // --- 再生シグナル (ReadSignal/WriteSignal として渡すため RwSignal で保持) ---
    pub current_time:  RwSignal<f64>,
    pub is_playing:    RwSignal<bool>,
    pub duration:      RwSignal<f64>,
    pub volume:        RwSignal<f64>,

    // --- エンジン (ページをまたいで生存させるため StoredValue) ---
    pub engine:        StoredValue<Option<AudioEngine>>,
    pub stem_engine:   StoredValue<Option<StemAudioEngine>>,
    pub stem_gains:    StoredValue<Option<StemGains>>,
    pub stems_available: RwSignal<bool>,
    pub stems_loading:   RwSignal<bool>,
    /// ステムロード失敗時のエラーメッセージ（None = エラーなし）
    pub stems_error:     RwSignal<Option<String>>,
    /// 現在ロード中のステムキー。高速切り替え時の競合防止に使用。
    pub loading_stem_key: StoredValue<String>,
}

impl GlobalPlayback {
    pub fn new() -> Self {
        Self {
            loaded_stem:     create_rw_signal(String::new()),
            current_time:    create_rw_signal(0.0),
            is_playing:      create_rw_signal(false),
            duration:        create_rw_signal(0.0),
            volume:          create_rw_signal(1.0),
            engine:           store_value(None),
            stem_engine:      store_value(None),
            stem_gains:       store_value(None),
            stems_available:  create_rw_signal(false),
            stems_loading:    create_rw_signal(false),
            stems_error:      create_rw_signal(None),
            loading_stem_key: store_value(String::new()),
        }
    }

    /// 指定 stem が既にエンジンつきでロード済みか
    pub fn is_loaded(&self, stem: &str) -> bool {
        self.loaded_stem.get_untracked() == stem
            && self.engine.get_value().is_some()
    }

    /// 再生を停止し、位置を 0 に戻す（エンジンは保持）
    pub fn stop_playback(&self) {
        if let Some(s) = self.stem_engine.get_value() { s.pause(); }
        if let Some(e) = self.engine.get_value()      { e.pause(); }
        self.is_playing.set(false);
        self.current_time.set(0.0);
    }

    /// エンジンを含め完全クリア（別の stem をロードする前に呼ぶ）
    pub fn clear(&self) {
        self.stop_playback();
        self.engine.set_value(None);
        self.stem_engine.set_value(None);
        self.stem_gains.set_value(None);
        self.loaded_stem.set(String::new());
        self.duration.set(0.0);
        self.stems_available.set(false);
        self.stems_loading.set(false);
        self.stems_error.set(None);
    }
}

// ---------------------------------------------------------------------------
// SongAudioState — 楽曲ごとに保存する音量状態
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SongAudioState {
    pub master_volume: f64,
    pub stem_volumes: StemVolumes,
}

impl Default for SongAudioState {
    fn default() -> Self {
        Self { master_volume: 1.0, stem_volumes: StemVolumes::default() }
    }
}

// ---------------------------------------------------------------------------
// VisualizationPageState — Visualization ページ固有の永続 UI 設定
// App コンポーネントで生成し provide_context で提供する。
// ページを離れても値が保持され、戻ったときに復元される。
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct VisualizationPageState {
    /// 楽曲ごとの音量キャッシュ（stem名 → SongAudioState）
    pub per_song_audio: StoredValue<HashMap<String, SongAudioState>>,
    /// 4 チャンネルステムミキサー音量（ライブ値 — per_song_audio から復元される）
    pub stem_volumes: RwSignal<StemVolumes>,
    /// ビートタイミングオフセット in seconds（トラック非依存）
    pub beat_offset: RwSignal<f64>,
    /// ループ開始位置（トラック依存 → 新規ステムロード時にリセット）
    pub loop_start: RwSignal<Option<f64>>,
    /// ループ終了位置（トラック依存）
    pub loop_end: RwSignal<Option<f64>>,
    /// ループ有効フラグ（トラック依存）
    pub loop_active: RwSignal<bool>,
    /// Ctrl+クリックで選択されたセグメントインデックス（トラック依存）
    pub selected_segment_indices: RwSignal<Vec<u32>>,
    /// セクションリストのハイライト表示フラグ
    pub highlight_enabled: RwSignal<bool>,
    /// SectionCard 表示フラグ
    pub section_card_enabled: RwSignal<bool>,
}

impl VisualizationPageState {
    pub fn new() -> Self {
        Self {
            per_song_audio:           store_value(HashMap::new()),
            stem_volumes:             create_rw_signal(StemVolumes::default()),
            beat_offset:              create_rw_signal(0.0_f64),
            loop_start:               create_rw_signal(None::<f64>),
            loop_end:                 create_rw_signal(None::<f64>),
            loop_active:              create_rw_signal(false),
            selected_segment_indices: create_rw_signal(Vec::<u32>::new()),
            highlight_enabled:        create_rw_signal(true),
            section_card_enabled:     create_rw_signal(false),
        }
    }

    /// 楽曲切り替え時に呼ぶ。
    /// 旧楽曲の音量をキャッシュに保存し、新楽曲の保存値（なければデフォルト1.0）をシグナルに反映。
    /// 戻り値の master_volume を GlobalPlayback.volume に適用すること。
    pub fn switch_song(&self, old_stem: &str, new_stem: &str, current_master: f64) -> SongAudioState {
        if !old_stem.is_empty() {
            let current_vols = self.stem_volumes.get_untracked();
            self.per_song_audio.update_value(|map| {
                map.insert(old_stem.to_string(), SongAudioState {
                    master_volume: current_master,
                    stem_volumes: current_vols,
                });
            });
        }
        let restored = self.per_song_audio.with_value(|map| {
            map.get(new_stem).cloned().unwrap_or_default()
        });
        self.stem_volumes.set(restored.stem_volumes);
        restored
    }

    /// 新しいステムをロードするときに呼ぶ。
    /// stem_volumes / beat_offset は保持し、ループとセクション選択のみリセット。
    pub fn reset_loop(&self) {
        self.loop_start.set(None);
        self.loop_end.set(None);
        self.loop_active.set(false);
        self.selected_segment_indices.set(Vec::new());
    }
}
