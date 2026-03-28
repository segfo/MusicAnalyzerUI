use crate::{
    api,
    audio_setup::{setup_stem_handoff_effect, setup_switch_song_effect, use_playback_polling},
    components::{
        error_display::ErrorPanel,
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
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

// Helper: build the /visualization/:stem URL from current route params
fn visualization_href(stem: &str) -> String {
    format!("/visualization/{}", js_sys::encode_uri_component(stem).as_string().unwrap_or_default())
}

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

    // UI 表示用
    let stems_loading = global.stems_loading;
    let stems_error   = global.stems_error;

    // ローディング状態（楽曲データ or ステム）
    let any_loading = move || {
        track_data.loading().get() || audio_buffer_res.loading().get() || stems_loading.get()
    };
    let loading_message = move || {
        if stems_loading.get() { "ステム読み込み中..." } else { "楽曲データ読み込み中..." }
    };

    // 楽曲切り替えを stem() の変化から即座に検出して音量を UI に反映
    setup_switch_song_effect(global.clone(), viz_page_state.clone(), stem);

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
    setup_stem_handoff_effect(global.clone(), viz_page_state.clone());

    // current_segment_idx: 再生位置からセグメントインデックスを導出
    let ct = global.current_time;
    let current_segment_idx = create_memo(move |_| {
        let t = ct.get();
        track_data.get().and_then(|r| r.ok()).and_then(|track| {
            track.segments.iter().position(|seg| t >= seg.start && t < seg.end)
        })
    });

    // PlaybackContext・VizContext を provide し、ポーリングフックを登録
    provide_context(PlaybackContext::new(&global, current_segment_idx));
    provide_context(VizContext::new_dummy());
    use_playback_polling(global.clone(), viz_page_state.clone(), |_| {});

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
            // ステムロードエラーバナー（非ブロッキング）
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
                            <ErrorPanel title="Failed to load track" message=e />
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
    let section_card_enabled = viz_page_state.section_card_enabled.read_only();
    let set_section_card_enabled = viz_page_state.section_card_enabled.write_only();
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
                <div class="flex items-center gap-3">
                    // SectionCard 表示トグル
                    <button
                        on:click=move |_| set_section_card_enabled.update(|v| *v = !*v)
                        class=move || if section_card_enabled.get() {
                            "text-xs px-2 py-0.5 rounded bg-blue-500/20 text-blue-400 border border-blue-500/40 transition-colors"
                        } else {
                            "text-xs px-2 py-0.5 rounded bg-gray-700/40 text-gray-500 border border-gray-600/30 transition-colors"
                        }
                    >"SectionCard"</button>
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
