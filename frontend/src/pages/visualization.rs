use crate::{
    api,
    components::{
        player::Player,
        stem_mixer::StemMixer,
        timeline::Timeline,
        viz_canvas::VizCanvas,
    },
    state::{GlobalPlayback, VisualizationPageState},
    types::chord_hue,
};
use leptos::*;
use leptos_router::*;
use leptos_use::use_interval_fn;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioBuffer, AudioContext, GainNode};

use crate::components::player::{AudioEngine, PlaybackContext};

// ─── StemAudioEngine 等は components/player.rs で定義 → 再エクスポート ─────────
pub use crate::components::player::{StemAudioEngine, StemGains, StemVolumes};

// ---------------------------------------------------------------------------
// load_stems — ステムをバックグラウンドでロードする共有ヘルパー
// Analysis / Visualization 両ページから呼び出す。
// メイン AudioEngine のミュートはここでは行わない（Visualization 側の責務）。
// ---------------------------------------------------------------------------

pub async fn load_stems(global: crate::state::GlobalPlayback, stem_key: String) {
    if global.stem_engine.get_value().is_some() { return; }
    global.stems_loading.set(true);
    load_stems_inner(&global, &stem_key).await;
    global.stems_loading.set(false);
}

async fn load_stems_inner(global: &crate::state::GlobalPlayback, stem_key: &str) {
    let avail = match api::fetch_stem_availability(stem_key).await {
        Ok(a) if a.any_available() => a,
        _ => return,
    };

    let Ok(stem_ctx) = AudioContext::new() else { return };

    let mut gain_opts: [Option<GainNode>; 4] = [None, None, None, None];
    for i in 0..4 {
        let Ok(g) = stem_ctx.create_gain() else { return };
        let _ = g.connect_with_audio_node(&stem_ctx.destination());
        gain_opts[i] = Some(g);
    }
    let gains_arr: [GainNode; 4] = [
        gain_opts[0].take().unwrap(),
        gain_opts[1].take().unwrap(),
        gain_opts[2].take().unwrap(),
        gain_opts[3].take().unwrap(),
    ];

    let stem_names = ["vocals", "drums", "bass", "other"];
    let available_flags = [avail.vocals, avail.drums, avail.bass, avail.other];
    let mut buffers: [Option<AudioBuffer>; 4] = [None, None, None, None];

    for (i, name) in stem_names.iter().enumerate() {
        if !available_flags[i] { continue; }
        let Ok(arr_buf) = api::fetch_stem_array_buffer(stem_key, name).await else { continue };
        let Ok(decode_promise) = stem_ctx.decode_audio_data(&arr_buf) else { continue };
        let Ok(decoded) = JsFuture::from(decode_promise).await else { continue };
        let Ok(buf) = decoded.dyn_into::<AudioBuffer>() else { continue };
        buffers[i] = Some(buf);
    }

    if buffers.iter().all(|b| b.is_none()) { return; }

    global.stem_gains.set_value(Some(StemGains {
        vocals: gains_arr[0].clone(),
        drums:  gains_arr[1].clone(),
        bass:   gains_arr[2].clone(),
        others: gains_arr[3].clone(),
    }));

    let stem_eng = StemAudioEngine::new(stem_ctx, buffers, gains_arr);
    let stem_dur = stem_eng.duration();
    global.stem_engine.set_value(Some(stem_eng));
    global.duration.set(stem_dur);
    global.stems_available.set(true);
}

// ---------------------------------------------------------------------------
// VizContext — shared state for all visualization components
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct VizContext {
    pub energy: ReadSignal<f64>,
    pub set_energy: WriteSignal<f64>,
    pub density: ReadSignal<f64>,
    pub set_density: WriteSignal<f64>,
    pub current_hue: ReadSignal<f64>,
    pub set_current_hue: WriteSignal<f64>,
    pub beat_trigger: ReadSignal<u32>,
    pub set_beat_trigger: WriteSignal<u32>,
    pub downbeat_trigger: ReadSignal<u32>,
    pub set_downbeat_trigger: WriteSignal<u32>,
    pub stem_volumes: ReadSignal<StemVolumes>,
    pub set_stem_volumes: WriteSignal<StemVolumes>,
    pub stems_available: ReadSignal<bool>,
    pub set_stems_available: WriteSignal<bool>,
    pub loop_start: ReadSignal<Option<f64>>,
    pub set_loop_start: WriteSignal<Option<f64>>,
    pub loop_end: ReadSignal<Option<f64>>,
    pub set_loop_end: WriteSignal<Option<f64>>,
    pub loop_active: ReadSignal<bool>,
    pub set_loop_active: WriteSignal<bool>,
    pub selected_segment_indices: ReadSignal<Vec<u32>>,
    pub set_selected_segment_indices: WriteSignal<Vec<u32>>,
    pub stem_gains: StoredValue<Option<StemGains>>,
    /// Stem engine — Some when stems are loaded and usable
    pub stem_engine: StoredValue<Option<StemAudioEngine>>,
    /// Beat timing offset in seconds (negative = fire earlier, positive = later)
    pub beat_offset: ReadSignal<f64>,
    pub set_beat_offset: WriteSignal<f64>,
}

// ---------------------------------------------------------------------------
// section_energy: section label → (energy, density)
// ---------------------------------------------------------------------------

fn section_energy(label: &str) -> (f64, f64) {
    match label.to_lowercase().as_str() {
        "intro"  => (0.2, 0.2),
        "verse"  => (0.6, 0.5),
        "chorus" | "refrain" => (1.2, 1.0),
        "bridge" => (0.7, 0.4),
        "outro"  => (0.3, 0.2),
        "break"  => (0.5, 0.3),
        "solo"   => (0.9, 0.7),
        "pre-chorus" | "prechorus" => (0.8, 0.6),
        _ => (0.8, 0.6),
    }
}

// ---------------------------------------------------------------------------
// Visualization page
// ---------------------------------------------------------------------------

#[component]
pub fn Visualization() -> impl IntoView {
    let global = use_context::<GlobalPlayback>().expect("GlobalPlayback missing");
    let viz_page_state = use_context::<VisualizationPageState>()
        .expect("VisualizationPageState missing");
    let params = use_params_map();
    let stem = move || params.with(|p| p.get("stem").cloned().unwrap_or_default());

    // --- Metadata ---
    let track_data = create_resource(stem, |s| async move { api::fetch_track(&s).await });

    // --- Main audio: 同じ stem がロード済みならスキップ ---
    let main_audio_res = create_local_resource(stem, {
        let global = global.clone();
        move |s: String| {
            let global = global.clone();
            async move {
                if global.is_loaded(&s) {
                    return Ok(None);
                }
                let array_buf = api::fetch_audio_array_buffer(&s).await?;
                let ctx = AudioContext::new().map_err(|e| format!("{e:?}"))?;
                let decoded = JsFuture::from(ctx.decode_audio_data(&array_buf).map_err(|e| format!("{e:?}"))?)
                    .await.map_err(|e| format!("{e:?}"))?;
                let buf = decoded.dyn_into::<AudioBuffer>().map_err(|e| format!("{e:?}"))?;
                Ok::<Option<(AudioContext, AudioBuffer)>, String>(Some((ctx, buf)))
            }
        }
    });

    // --- GlobalPlayback のシグナルを PlaybackContext 用に分割 ---
    let current_time     = global.current_time.read_only();
    let set_current_time = global.current_time.write_only();
    let is_playing       = global.is_playing.read_only();
    let set_is_playing   = global.is_playing.write_only();
    let duration         = global.duration.read_only();
    let set_duration     = global.duration.write_only();
    let volume           = global.volume.read_only();
    let set_volume       = global.volume.write_only();
    let engine           = global.engine;

    // VizContext 固有シグナル
    // エフェメラル（再計算可能）なものはローカルシグナルのまま
    let (energy, set_energy) = create_signal(0.5_f64);
    let (density, set_density) = create_signal(0.5_f64);
    let (current_hue, set_current_hue) = create_signal(220.0_f64);
    let (beat_trigger, set_beat_trigger) = create_signal(0u32);
    let (downbeat_trigger, set_downbeat_trigger) = create_signal(0u32);
    // ページUI設定（VisualizationPageState から取得して永続化）
    let stem_volumes     = viz_page_state.stem_volumes.read_only();
    let set_stem_volumes = viz_page_state.stem_volumes.write_only();
    let loop_start       = viz_page_state.loop_start.read_only();
    let set_loop_start   = viz_page_state.loop_start.write_only();
    let loop_end         = viz_page_state.loop_end.read_only();
    let set_loop_end     = viz_page_state.loop_end.write_only();
    let loop_active      = viz_page_state.loop_active.read_only();
    let set_loop_active  = viz_page_state.loop_active.write_only();
    let selected_segment_indices     = viz_page_state.selected_segment_indices.read_only();
    let set_selected_segment_indices = viz_page_state.selected_segment_indices.write_only();
    let beat_offset      = viz_page_state.beat_offset.read_only();
    let set_beat_offset  = viz_page_state.beat_offset.write_only();

    // stems_available / stem_gains / stem_engine は GlobalPlayback から
    let stems_available     = global.stems_available.read_only();
    let set_stems_available = global.stems_available.write_only();
    let stem_gains_sv       = global.stem_gains;
    let stem_engine_sv      = global.stem_engine;

    // --- Beat/section/chord tracking ---
    let prev_beat_idx: StoredValue<usize> = store_value(0);
    let prev_downbeat_idx: StoredValue<usize> = store_value(0);
    let prev_segment_label: StoredValue<String> = store_value(String::new());
    let prev_chord_label: StoredValue<String> = store_value(String::new());

    // --- 楽曲切り替えを stem() の変化から即座に検出して音量を UI に反映 ---
    // audio ロードより先に同期的に実行されるため、UI がデフォルト値を表示するまでの遅延がない
    create_effect({
        let global = global.clone();
        let viz_page_state = viz_page_state.clone();
        move |_| {
            let s = stem(); // reactive dependency: stem が変わるとここが再実行される
            let old = global.loaded_stem.get_untracked();
            if s == old { return; } // 同一楽曲（または初回ロード済み）は何もしない
            let audio_state = viz_page_state.switch_song(&old, &s, global.volume.get_untracked());
            global.volume.set(audio_state.master_volume);
        }
    });

    // --- 新規ロード時のみエンジンを作成してグローバルに保存 ---
    create_effect({
        let global = global.clone();
        let viz_page_state = viz_page_state.clone();
        move |_| {
            let Some(Ok(Some((ctx, buf)))) = main_audio_res.get() else { return };
            let s = stem();
            viz_page_state.reset_loop();
            global.clear();
            let dur = buf.duration();
            if let Ok(eng) = AudioEngine::new(ctx.clone(), buf) {
                eng.set_volume(global.volume.get_untracked());
                global.engine.set_value(Some(eng));
            }
            global.duration.set(dur);
            global.loaded_stem.set(s.clone());

            // ステムを共有ヘルパーでバックグラウンドロード
            let global2 = global.clone();
            spawn_local(load_stems(global2, s));
        }
    });

    // stems が利用可能になったらメイン AudioEngine をミュート・停止し StemAudioEngine に引き継ぐ
    create_effect({
        let global = global.clone();
        let viz_page_state = viz_page_state.clone();
        move |_| {
            if !global.stems_available.get() { return; }
            // 復元された stem volumes を新しい GainNode に適用（UI と音声を同期）
            let vols = viz_page_state.stem_volumes.get_untracked();
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
        }
    });

    // --- current_segment_idx (for PlaybackContext) ---
    let current_segment_idx = create_memo(move |_| {
        let t = current_time.get();
        track_data.get().and_then(|r| r.ok()).and_then(|track| {
            track.segments.iter().position(|seg| t >= seg.start && t < seg.end)
        })
    });

    provide_context(PlaybackContext {
        current_time,
        set_current_time,
        is_playing,
        set_is_playing,
        duration,
        set_duration,
        volume,
        set_volume,
        current_segment_idx,
        engine,
        stem_engine: stem_engine_sv,
        stem_gains:  stem_gains_sv,
    });

    let viz_ctx = VizContext {
        energy, set_energy,
        density, set_density,
        current_hue, set_current_hue,
        beat_trigger, set_beat_trigger,
        downbeat_trigger, set_downbeat_trigger,
        stem_volumes, set_stem_volumes,
        stems_available, set_stems_available,
        loop_start, set_loop_start,
        loop_end, set_loop_end,
        loop_active, set_loop_active,
        selected_segment_indices, set_selected_segment_indices,
        stem_gains: stem_gains_sv,
        stem_engine: stem_engine_sv,
        beat_offset,
        set_beat_offset,
    };
    provide_context(viz_ctx);

    // --- Polling loop (100 ms) ---
    use_interval_fn(move || {
        // Get current time from stem engine (if available) or main engine
        let (t, eng_is_playing, eng_dur) = if let Some(s) = stem_engine_sv.get_value() {
            (s.current_time(), s.is_playing(), s.duration())
        } else if let Some(e) = engine.get_value() {
            (e.current_time(), e.is_playing(), e.duration())
        } else {
            return;
        };

        set_current_time.set(t);

        // End-of-track detection
        if eng_is_playing && t >= eng_dur - 0.05 {
            if let Some(s) = stem_engine_sv.get_value() { s.pause(); }
            if let Some(e) = engine.get_value() { e.pause(); }
            set_is_playing.set(false);
            set_current_time.set(0.0);
        }

        let Some(Ok(track)) = track_data.get() else { return };

        // Beat detection (apply offset: positive offset fires triggers earlier)
        let t_beat = t + beat_offset.get();
        let beat_idx = track.beats.partition_point(|&b| b <= t_beat);
        if beat_idx != prev_beat_idx.get_value() {
            prev_beat_idx.set_value(beat_idx);
            set_beat_trigger.update(|v| *v = v.wrapping_add(1));
        }

        // Downbeat detection (same offset)
        let db_idx = track.downbeats.partition_point(|&b| b <= t_beat);
        if db_idx != prev_downbeat_idx.get_value() {
            prev_downbeat_idx.set_value(db_idx);
            set_downbeat_trigger.update(|v| *v = v.wrapping_add(1));
        }

        // Section change → energy / density
        if let Some(seg) = track.segments.iter().find(|s| t >= s.start && t < s.end) {
            if seg.label != prev_segment_label.get_value() {
                prev_segment_label.set_value(seg.label.clone());
                let (e, d) = section_energy(&seg.label);
                set_energy.set(e.min(1.0));
                set_density.set(d);
            }
        }

        // Chord change → hue
        if let Some(chord) = track.chords.iter().find(|c| {
            c.start.map(|s| t >= s).unwrap_or(false) && c.end.map(|e| t < e).unwrap_or(false)
        }) {
            let label = chord.label.as_deref().unwrap_or("N").to_string();
            if label != prev_chord_label.get_value() {
                prev_chord_label.set_value(label.clone());
                set_current_hue.set(chord_hue(&label));
            }
        }

        // Loop control
        if loop_active.get() {
            if let (Some(ls), Some(le)) = (loop_start.get(), loop_end.get()) {
                if ls < le && t >= le {
                    if let Some(s) = stem_engine_sv.get_value() { s.seek(ls); }
                    else if let Some(e) = engine.get_value() { e.seek(ls); }
                    set_current_time.set(ls);
                }
            }
        }
    }, 100);

    let stems_loading = global.stems_loading;

    view! {
        <div class="flex flex-col h-screen bg-gray-950 overflow-hidden relative">
            // ステム読み込み中オーバーレイ（UIロック）
            {move || stems_loading.get().then(|| view! {
                <div class="absolute inset-0 bg-gray-950/80 flex items-center justify-center z-50 backdrop-blur-sm">
                    <div class="bg-gray-800 rounded-xl p-8 border border-gray-700 flex flex-col items-center gap-4">
                        <div class="w-10 h-10 border-4 border-orange-500 border-t-transparent rounded-full animate-spin" />
                        <p class="text-gray-200 font-medium">"Loading stems..."</p>
                    </div>
                </div>
            })}
            <Suspense fallback=|| view! {
                <div class="flex items-center justify-center h-full text-gray-400">"Loading..."</div>
            }>
                {move || track_data.get().map(|result| match result {
                    Ok(track) => {
                        let t2 = track.clone();
                        view! {
                            <Player track=track.clone() active_page="visualization" />
                            <Timeline track=t2 />
                            <div class="flex flex-1 min-h-0">
                                <div class="flex-1 min-w-0">
                                    <VizCanvas />
                                </div>
                                <div class="w-64 flex-shrink-0 bg-gray-900 border-l border-gray-800 p-3 overflow-y-auto">
                                    <StemMixer />
                                </div>
                            </div>
                        }.into_view()
                    },
                    Err(e) => view! {
                        <div class="flex items-center justify-center h-full">
                            <div class="bg-red-900/30 border border-red-700 rounded-xl p-6 max-w-md">
                                <p class="text-red-400 font-medium mb-1">"Failed to load track"</p>
                                <p class="text-red-300 text-sm">{e}</p>
                            </div>
                        </div>
                    }.into_view(),
                })}
            </Suspense>
        </div>
    }
}

// VizPlayer は components/player::Player に統合されました


