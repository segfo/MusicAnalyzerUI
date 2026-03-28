use crate::state::{GlobalPlayback, VisualizationPageState};
use leptos::*;
use leptos_use::use_interval_fn;

/// 楽曲切り替え時に stem volumes / master volume をリストアする effect。
/// Analysis・Visualization 両ページで同一のため共通化。
pub fn setup_switch_song_effect(
    global: GlobalPlayback,
    viz_state: VisualizationPageState,
    stem: impl Fn() -> String + 'static,
) {
    create_effect(move |_| {
        let s = stem();
        let old = global.loaded_stem.get_untracked();
        if s == old { return; }
        let audio_state = viz_state.switch_song(&old, &s, global.volume.get_untracked());
        global.volume.set(audio_state.master_volume);
    });
}

/// 再生位置の 100ms ポーリング + 曲末検出 + ループ制御の共通フック。
///
/// Analysis・Visualization 両ページで共有するポーリング処理をここに集約。
/// ページ固有のビート / コード / セクション検出は `on_tick(t)` コールバックで行う。
/// Analysis では `|_| {}` を渡せばよい。
pub fn use_playback_polling(
    global: GlobalPlayback,
    viz_state: VisualizationPageState,
    on_tick: impl Fn(f64) + Clone + 'static,
) {
    let set_current_time = global.current_time.write_only();
    let set_is_playing   = global.is_playing.write_only();
    let stem_engine_sv   = global.stem_engine;
    let engine           = global.engine;
    let loop_start       = viz_state.loop_start.read_only();
    let loop_end         = viz_state.loop_end.read_only();
    let loop_active      = viz_state.loop_active.read_only();

    use_interval_fn(move || {
        let (t, eng_is_playing, eng_dur) = if let Some(s) = stem_engine_sv.get_value() {
            (s.current_time(), s.is_playing(), s.duration())
        } else if let Some(e) = engine.get_value() {
            (e.current_time(), e.is_playing(), e.duration())
        } else {
            return;
        };
        set_current_time.set(t);
        if eng_is_playing && t >= eng_dur - 0.05 {
            if let Some(s) = stem_engine_sv.get_value() { s.pause(); }
            if let Some(e) = engine.get_value() { e.pause(); }
            set_is_playing.set(false);
            set_current_time.set(0.0);
        } else if loop_active.get() {
            if let (Some(ls), Some(le)) = (loop_start.get(), loop_end.get()) {
                if ls < le && t >= le {
                    if let Some(s) = stem_engine_sv.get_value() { s.seek(ls); }
                    else if let Some(e) = engine.get_value() { e.seek(ls); }
                    set_current_time.set(ls);
                }
            }
        }
        on_tick(t);
    }, 100);
}

/// stems_available が true になったら StemAudioEngine に再生を引き継ぐ effect。
/// Analysis・Visualization 両ページで逐語的に重複していたため共通化。
pub fn setup_stem_handoff_effect(
    global: GlobalPlayback,
    viz_state: VisualizationPageState,
) {
    create_effect(move |_| {
        if !global.stems_available.get() { return; }

        // 復元された stem volumes を新しい GainNode に適用（UI と音声を同期）
        let vols = viz_state.stem_volumes.get_untracked();
        if let Some(gains) = global.stem_gains.get_value() {
            gains.vocals.gain().set_value(vols.vocals as f32);
            gains.drums.gain().set_value(vols.drums as f32);
            gains.bass.gain().set_value(vols.bass as f32);
            gains.others.gain().set_value(vols.others as f32);
        }

        // engine がまだ再生中の場合のみ引き継ぎ（ナビゲーション時の二重実行防止）
        let eng_playing = global.engine.get_value().map(|e| e.is_playing()).unwrap_or(false);
        if !eng_playing {
            if let Some(eng) = global.engine.get_value() { eng.set_volume(0.0); }
            return;
        }

        let was_playing = global.is_playing.get_untracked();
        let t = global.engine.get_value()
            .map(|e| e.current_time())
            .unwrap_or_else(|| global.current_time.get_untracked());

        if let Some(eng) = global.engine.get_value() {
            eng.set_volume(0.0);
            eng.pause();
        }

        if let Some(stem_eng) = global.stem_engine.get_value() {
            stem_eng.seek(t);
            if was_playing {
                spawn_local(async move {
                    stem_eng.resume_ctx().await;
                    stem_eng.play();
                });
            }
        }
    });
}
