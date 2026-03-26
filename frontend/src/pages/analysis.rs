use crate::{
    api,
    components::{
        player::{AudioEngine, PlaybackContext, Player},
        section_card::SectionCard,
        stem_mixer::StemMixer,
        timeline::Timeline,
    },
    pages::visualization::{load_stems, VizContext},
    state::{GlobalPlayback, VisualizationPageState},
    types::{SegmentResult, TrackDataset},
};
use leptos::*;
use leptos_router::*;

// Helper: build the /visualization/:stem URL from current route params
fn visualization_href(stem: &str) -> String {
    format!("/visualization/{}", js_sys::encode_uri_component(stem).as_string().unwrap_or_default())
}
use leptos_use::use_interval_fn;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

#[component]
pub fn Analysis() -> impl IntoView {
    let global = use_context::<GlobalPlayback>().expect("GlobalPlayback missing");
    let viz_page_state = use_context::<VisualizationPageState>()
        .expect("VisualizationPageState missing");
    let params = use_params_map();
    let stem = move || params.with(|p| p.get("stem").cloned().unwrap_or_default());

    // Fetch track metadata (segments, BPM, etc.)
    let track_data = create_resource(stem, |s: String| async move { api::fetch_track(&s).await });

    // 同じ stem がロード済みならスキップ、異なる場合のみフェッチ
    let audio_buffer_res = create_local_resource(stem, {
        let global = global.clone();
        move |s: String| {
            let global = global.clone();
            async move {
                if global.is_loaded(&s) {
                    return Ok(None); // 再利用
                }
                let array_buffer = api::fetch_audio_array_buffer(&s).await?;
                let ctx = web_sys::AudioContext::new().map_err(|e| format!("{e:?}"))?;
                let decoded = JsFuture::from(
                    ctx.decode_audio_data(&array_buffer).map_err(|e| format!("{e:?}"))?,
                ).await.map_err(|e| format!("{e:?}"))?;
                let buf = decoded.dyn_into::<web_sys::AudioBuffer>().map_err(|e| format!("{e:?}"))?;
                Ok::<Option<(web_sys::AudioContext, web_sys::AudioBuffer)>, String>(Some((ctx, buf)))
            }
        }
    });

    // GlobalPlayback のシグナルを PlaybackContext 用に read/write 分割
    let current_time  = global.current_time.read_only();
    let set_current_time = global.current_time.write_only();
    let is_playing    = global.is_playing.read_only();
    let set_is_playing = global.is_playing.write_only();
    let duration      = global.duration.read_only();
    let set_duration  = global.duration.write_only();
    let volume        = global.volume.read_only();
    let set_volume    = global.volume.write_only();
    let engine        = global.engine;

    // VizContext 用シグナル（エフェメラル — AnalysisページではVizCanvasを使わないため未更新）
    let (energy, set_energy) = create_signal(0.5_f64);
    let (density, set_density) = create_signal(0.5_f64);
    let (current_hue, set_current_hue) = create_signal(220.0_f64);
    let (beat_trigger, set_beat_trigger) = create_signal(0u32);
    let (downbeat_trigger, set_downbeat_trigger) = create_signal(0u32);
    // VisualizationPageState から永続設定を取得
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
    // GlobalPlayback から stems シグナル
    let stems_available     = global.stems_available.read_only();
    let set_stems_available = global.stems_available.write_only();
    let stems_loading       = global.stems_loading;

    // ローディング状態（楽曲データ or ステム）
    let any_loading = move || {
        track_data.loading().get() || audio_buffer_res.loading().get() || stems_loading.get()
    };
    let loading_message = move || {
        if stems_loading.get() {
            "ステム読み込み中..."
        } else {
            "楽曲データ読み込み中..."
        }
    };

    // 楽曲切り替えを stem() の変化から即座に検出して音量を UI に反映
    create_effect({
        let global = global.clone();
        let viz_page_state = viz_page_state.clone();
        move |_| {
            let s = stem();
            let old = global.loaded_stem.get_untracked();
            if s == old { return; }
            let audio_state = viz_page_state.switch_song(&old, &s, global.volume.get_untracked());
            global.volume.set(audio_state.master_volume);
        }
    });

    // 新規ロード時のみエンジンを作成してグローバルに保存し、ステムをバックグラウンドロード
    create_effect({
        let global = global.clone();
        let viz_page_state = viz_page_state.clone();
        move |_| {
            if let Some(Ok(Some((ctx, buf)))) = audio_buffer_res.get() {
                let s = stem();
                viz_page_state.reset_loop();
                global.clear();
                let dur = buf.duration();
                if let Ok(eng) = AudioEngine::new(ctx, buf) {
                    eng.set_volume(global.volume.get_untracked());
                    global.engine.set_value(Some(eng));
                }
                global.duration.set(dur);
                global.loaded_stem.set(s.clone());

                // ステムをバックグラウンドでロード
                let global2 = global.clone();
                spawn_local(load_stems(global2, s));
            }
        }
    });

    // ステムが利用可能になったら AudioEngine をミュート・停止し StemAudioEngine に引き継ぐ
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

    // 100ms ポーリング（再生位置更新 + ループ制御）
    // stem_engine が利用可能な場合はそちらを優先して参照する
    let stem_engine_sv = global.stem_engine;
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
    }, 100);

    // current_segment_idx derived from current_time + segment list
    let current_segment_idx = create_memo(move |_| {
        let t = current_time.get();
        track_data
            .get()
            .and_then(|r| r.ok())
            .and_then(|track| {
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
        stem_engine: global.stem_engine,
        stem_gains:  global.stem_gains,
    });

    provide_context(VizContext {
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
        stem_gains: global.stem_gains,
        stem_engine: global.stem_engine,
        beat_offset, set_beat_offset,
    });

    view! {
        <div class="flex flex-col h-screen bg-gray-950 overflow-hidden relative">
            // 読み込み中オーバーレイ（楽曲データ・ステム共通）
            {move || any_loading().then(|| view! {
                <div class="absolute inset-0 bg-gray-950/80 flex items-center justify-center z-50 backdrop-blur-sm">
                    <div class="bg-gray-800 rounded-xl p-8 border border-gray-700 flex flex-col items-center gap-4">
                        <div class="w-10 h-10 border-4 border-orange-500 border-t-transparent rounded-full animate-spin" />
                        <p class="text-gray-200 font-medium">{loading_message()}</p>
                    </div>
                </div>
            })}
            <Suspense fallback=|| ()>
                {move || track_data.get().map(|result| {
                    match result {
                        Ok(track) => {
                            let track_for_player = track.clone();
                            let track_for_timeline = track.clone();
                            let track_for_card = track.clone();

                            let viz_href = visualization_href(&stem());
                            view! {
                                // Player controls (top)
                                <Player track=track_for_player active_page="analysis" />

                                // Timeline (directly below player)
                                <Timeline track=track_for_timeline />

                                // Main content + sidebar
                                <div class="relative flex flex-1 min-h-0 overflow-hidden">
                                    <div class="flex-1 overflow-y-auto p-6">
                                        <AnalysisContent track=track viz_href=viz_href />
                                    </div>
                                    <div class="w-64 flex-shrink-0 bg-gray-900 border-l border-gray-800 p-3 overflow-y-auto">
                                        <StemMixer />
                                    </div>
                                    // SectionCard はスクロール対象外の外側コンテナに配置
                                    <SectionCard track=track_for_card />
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
                    }
                })}
            </Suspense>
        </div>
    }
}

/// Main content area showing metadata and section list
#[component]
fn AnalysisContent(track: TrackDataset, viz_href: String) -> impl IntoView {
    let bpm_str = track
        .bpm
        .map(|b| format!("{:.0}", b))
        .unwrap_or_else(|| "-".into());
    let bpm_cands = if track.bpm_candidates.is_empty() {
        "-".to_string()
    } else {
        track
            .bpm_candidates
            .iter()
            .map(|b| format!("{:.0}", b))
            .collect::<Vec<_>>()
            .join(" / ")
    };
    let analyzed = track.analysis_timestamp.get(..10).unwrap_or("").to_string();
    let seg_count = track.segments.len().to_string();

    // Filter out Start/End segments for display
    let filtered_segments: Vec<(usize, SegmentResult)> = track
        .segments
        .iter()
        .filter(|seg| {
            let label_lower = seg.label.to_lowercase();
            label_lower != "start" && label_lower != "end"
        })
        .cloned()
        .enumerate()
        .collect();

    let ctx = use_context::<PlaybackContext>().expect("PlaybackContext missing");
    let viz_page_state = use_context::<VisualizationPageState>().expect("VisualizationPageState missing");
    let highlight_enabled = viz_page_state.highlight_enabled.read_only();
    let set_highlight_enabled = viz_page_state.highlight_enabled.write_only();
    let segments_sv = store_value(track.segments.clone());
    let active_seg_index: Memo<Option<usize>> = create_memo(move |_| {
        ctx.current_segment_idx
            .get()
            .and_then(|i| segments_sv.get_value().get(i).map(|s| s.index as usize))
    });

    view! {
        <div class="max-w-4xl mx-auto">

            // Metadata panel
            <div class="bg-gray-800 rounded-xl p-5 mb-6 border border-gray-700">
                <h2 class="text-lg font-semibold text-gray-100 mb-3">{track.track_filename.clone()}</h2>
                <div class="grid grid-cols-2 sm:grid-cols-4 gap-4">
                    <MetaItem label="BPM" value=bpm_str />
                    <MetaItem label="Sections" value=seg_count />
                    <MetaItem label="BPM candidates" value=bpm_cands />
                    <MetaItem label="Analyzed" value=analyzed />
                </div>
            </div>

            // Section list (filter out Start/End segments)
            <div class="flex items-center justify-between mb-3">
                <h3 class="text-sm font-medium text-gray-400 uppercase tracking-wider">"Sections"</h3>
                // ハイライト有効化ボタン
                <button
                    on:click=move |_| set_highlight_enabled.update(|v| *v = !*v)
                    class=move || if highlight_enabled.get() {
                        "text-xs px-2 py-0.5 rounded bg-blue-500/20 text-blue-400 border border-blue-500/40 transition-colors"
                    } else {
                        "text-xs px-2 py-0.5 rounded bg-gray-700/40 text-gray-500 border border-gray-600/30 transition-colors"
                    }
                >"Highlight"</button>
            </div>
            <div class="space-y-2">
                <For
                    each=move || filtered_segments.clone()
                    key=|(_, seg)| seg.index
                    children=move |(i, seg)| {
                        use crate::types::{segment_color, format_time};
                        let color = segment_color(&seg.label).to_string();
                        let time_str = format!(
                            "{} - {} ({:.1}s, {} beats)",
                            format_time(seg.start),
                            format_time(seg.end),
                            seg.duration,
                            seg.beat_count
                        );
                        let stagger_ms = (i * 60).min(400);
                        let seg_idx = seg.index;
                        let inner_class = move || {
                            let base = "bg-gray-800/60 rounded-lg px-4 py-3 border border-gray-700/50 flex items-start gap-3 transition-opacity duration-300";
                            if highlight_enabled.get() && ctx.is_playing.get() && active_seg_index.get().is_some() {
                                if active_seg_index.get() == Some(seg_idx as usize) {
                                    format!("{} opacity-100", base)
                                } else {
                                    format!("{} opacity-25", base)
                                }
                            } else {
                                base.to_string()
                            }
                        };
                        view! {
                            // 外側: animate-float-up (enter アニメーション担当)
                            <div id=format!("seg-item-{}", seg_idx) class="animate-float-up" style=format!("animation-delay: {}ms", stagger_ms)>
                                // 内側: ハイライト担当（アニメーションと干渉しない）
                                <div class=inner_class>
                                    <span class=format!("{color} px-2 py-0.5 rounded text-xs font-bold text-white flex-shrink-0 mt-0.5")>
                                        {seg.label.clone()}
                                    </span>
                                    <div class="flex-1 min-w-0">
                                        <p class="text-xs text-gray-400 font-mono mb-1">{time_str}</p>
                                        {seg.caption.clone().map(|cap| view! {
                                            <p class="text-sm text-gray-300 leading-relaxed">{cap}</p>
                                        })}
                                    </div>
                                </div>
                            </div>
                        }
                    }
                />
            </div>
        </div>
    }
}

#[component]
fn MetaItem(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div>
            <p class="text-xs text-gray-500 uppercase tracking-wider mb-1">{label}</p>
            <p class="text-sm text-gray-200 font-mono">{value}</p>
        </div>
    }
}
