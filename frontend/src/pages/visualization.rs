use crate::{
    api,
    audio_setup::{setup_stem_handoff_effect, setup_switch_song_effect, use_playback_polling},
    components::{
        error_display::ErrorPanel,
        player::{AudioEngine, PlaybackContext, Player},
        stem_mixer::StemMixer,
        timeline::Timeline,
        viz_canvas::VizCanvas,
    },
    state::{GlobalPlayback, VisualizationPageState},
    types::chord_hue,
};
use leptos::*;
use leptos_router::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioBuffer, AudioContext, GainNode};

// ─── StemAudioEngine 等は audio/engine.rs で定義 → 再エクスポート ─────────────
pub use crate::audio::{StemAudioEngine, StemGains};

// ---------------------------------------------------------------------------
// load_stems — ステムをバックグラウンドでロードする共有ヘルパー
// Analysis / Visualization 両ページから呼び出す。
// メイン AudioEngine のミュートはここでは行わない（Visualization 側の責務）。
// ---------------------------------------------------------------------------

pub async fn load_stems(global: crate::state::GlobalPlayback, stem_key: String) {
    if global.stem_engine.get_value().is_some() { return; }
    // このロードが最新であることを記録（古いロードタスクが自己中断できるように）
    global.loading_stem_key.set_value(stem_key.clone());
    global.stems_loading.set(true);
    global.stems_error.set(None);
    if let Err(e) = load_stems_inner(&global, &stem_key).await {
        // キャンセル（中断）以外のエラーだけ表示する
        if global.loading_stem_key.get_value() == stem_key {
            global.stems_error.set(Some(e));
        }
    }
    global.stems_loading.set(false);
}

/// Ok(()) = 成功または正常キャンセル、Err(msg) = ユーザーに見せるエラー
async fn load_stems_inner(global: &crate::state::GlobalPlayback, stem_key: &str) -> Result<(), String> {
    let avail = match api::fetch_stem_availability(stem_key).await {
        Ok(a) if a.any_available() => a,
        Ok(_) => return Err("No stems available for this track".to_string()),
        Err(e) => return Err(format!("Failed to fetch stem availability: {e}")),
    };
    // ステム切り替えで古いタスクを中断（エラーとして扱わない）
    if global.loading_stem_key.get_value() != stem_key { return Ok(()); }

    let stem_ctx = AudioContext::new().map_err(|e| format!("AudioContext error: {e:?}"))?;

    let mut gain_opts: [Option<GainNode>; 4] = [None, None, None, None];
    for i in 0..4 {
        let g = stem_ctx.create_gain().map_err(|e| format!("GainNode error: {e:?}"))?;
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
        if global.loading_stem_key.get_value() != stem_key { return Ok(()); }
        let Ok(decode_promise) = stem_ctx.decode_audio_data(&arr_buf) else { continue };
        let Ok(decoded) = JsFuture::from(decode_promise).await else { continue };
        if global.loading_stem_key.get_value() != stem_key { return Ok(()); }
        let Ok(buf) = decoded.dyn_into::<AudioBuffer>() else { continue };
        buffers[i] = Some(buf);
    }

    if buffers.iter().all(|b| b.is_none()) {
        return Err("All stem audio files failed to decode".to_string());
    }

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
    Ok(())
}

// ---------------------------------------------------------------------------
// VizContext — shared state for all visualization components
// ---------------------------------------------------------------------------

/// エフェメラルなビジュアライゼーション専用シグナル。
/// GlobalPlayback / VisualizationPageState と重複するフィールドは保持しない。
/// コンポーネントは必要に応じて use_context::<GlobalPlayback>() /
/// use_context::<VisualizationPageState>() を直接呼ぶこと。
#[derive(Clone)]
pub struct VizContext {
    pub energy: ReadSignal<f64>,
    pub set_energy: WriteSignal<f64>,
    pub density: ReadSignal<f64>,
    pub set_density: WriteSignal<f64>,
    pub current_hue: ReadSignal<f64>,
    pub set_current_hue: WriteSignal<f64>,
    pub current_chord: ReadSignal<String>,
    pub set_current_chord: WriteSignal<String>,
    pub beat_trigger: ReadSignal<u32>,
    pub set_beat_trigger: WriteSignal<u32>,
    pub downbeat_trigger: ReadSignal<u32>,
    pub set_downbeat_trigger: WriteSignal<u32>,
}

impl VizContext {
    /// ビジュアライゼーションシグナルをデフォルト値で生成する。
    /// Analysis ページのように VizCanvas を持たないページでは
    /// `provide_context(VizContext::new_dummy())` の1行で済む。
    pub fn new_dummy() -> Self {
        let (energy,          set_energy)          = create_signal(0.5_f64);
        let (density,         set_density)         = create_signal(0.5_f64);
        let (current_hue,     set_current_hue)     = create_signal(220.0_f64);
        let (current_chord,   set_current_chord)   = create_signal(String::new());
        let (beat_trigger,    set_beat_trigger)    = create_signal(0u32);
        let (downbeat_trigger, set_downbeat_trigger) = create_signal(0u32);
        Self {
            energy, set_energy, density, set_density,
            current_hue, set_current_hue,
            current_chord, set_current_chord,
            beat_trigger, set_beat_trigger,
            downbeat_trigger, set_downbeat_trigger,
        }
    }
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

    // --- VizContext 生成（WriteSignal は Copy なので先に退避してから provide）---
    let viz_ctx = VizContext::new_dummy();
    let (set_energy, set_density, set_current_hue, set_current_chord, set_beat_trigger, set_downbeat_trigger) = (
        viz_ctx.set_energy, viz_ctx.set_density, viz_ctx.set_current_hue,
        viz_ctx.set_current_chord, viz_ctx.set_beat_trigger, viz_ctx.set_downbeat_trigger,
    );

    // --- Beat/section/chord 変化追跡用（tick 間の状態保持）---
    let prev_beat_idx:      StoredValue<usize>  = store_value(0);
    let prev_downbeat_idx:  StoredValue<usize>  = store_value(0);
    let prev_segment_label: StoredValue<String> = store_value(String::new());
    let prev_chord_label:   StoredValue<String> = store_value(String::new());
    let beat_offset = viz_page_state.beat_offset.read_only();

    // --- 楽曲切り替えを stem() の変化から即座に検出して音量を UI に反映 ---
    setup_switch_song_effect(global.clone(), viz_page_state.clone(), stem);

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
    setup_stem_handoff_effect(global.clone(), viz_page_state.clone());

    // --- current_segment_idx ---
    let ct = global.current_time;
    let current_segment_idx = create_memo(move |_| {
        let t = ct.get();
        track_data.get().and_then(|r| r.ok()).and_then(|track| {
            track.segments.iter().position(|seg| t >= seg.start && t < seg.end)
        })
    });

    provide_context(PlaybackContext::new(&global, current_segment_idx));
    provide_context(viz_ctx);

    // --- Visualization 固有のポーリング: ビート / セクション / コード検出 ---
    use_playback_polling(global.clone(), viz_page_state.clone(), move |t| {
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
                set_current_chord.set(if label == "N" { String::new() } else { label });
            }
        }
    });

    let stems_loading = global.stems_loading;
    let stems_error   = global.stems_error;

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
            // ステムロードエラー表示
            {move || stems_error.get().map(|e| view! {
                <div class="absolute inset-x-0 top-0 z-40 mx-auto max-w-lg mt-4 px-4">
                    <div class="bg-red-900/80 border border-red-700 rounded-xl p-4 flex items-start gap-3">
                        <p class="text-red-300 text-sm flex-1">
                            <span class="font-medium text-red-200">"Stem load failed: "</span>
                            {e}
                        </p>
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
                        <ErrorPanel title="Failed to load track" message=e />
                    }.into_view(),
                })}
            </Suspense>
        </div>
    }
}

// VizPlayer は components/player::Player に統合されました


