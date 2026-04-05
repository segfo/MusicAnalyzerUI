// Re-export engine types so existing callers (state.rs, viz_canvas.rs, etc.)
// can continue using `crate::components::player::*` without changes.
pub use crate::audio::{AudioEngine, StemAudioEngine, StemGains};
use crate::state::GlobalPlayback;
use crate::types::{format_time, TrackDataset};
use leptos::*;
use leptos::html::Div;
use wasm_bindgen::JsCast;

// ---------------------------------------------------------------------------
// PlaybackContext — shared via provide_context
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PlaybackContext {
    pub current_time: ReadSignal<f64>,
    pub set_current_time: WriteSignal<f64>,
    pub is_playing: ReadSignal<bool>,
    pub set_is_playing: WriteSignal<bool>,
    pub duration: ReadSignal<f64>,
    pub set_duration: WriteSignal<f64>,
    pub volume: ReadSignal<f64>,
    pub set_volume: WriteSignal<f64>,
    pub current_segment_idx: Memo<Option<usize>>,
    /// Web Audio engine. None until the audio file has been decoded.
    pub engine: StoredValue<Option<AudioEngine>>,
    /// Stem engine — Some when stems are loaded and usable.
    pub stem_engine: StoredValue<Option<StemAudioEngine>>,
    /// Stem GainNodes for per-stem volume control.
    pub stem_gains: StoredValue<Option<StemGains>>,
}

impl PlaybackContext {
    /// GlobalPlayback からシグナルを分割して PlaybackContext を生成する。
    /// current_segment_idx はページごとに計算方法が異なるため引数で受け取る。
    pub fn new(global: &GlobalPlayback, current_segment_idx: Memo<Option<usize>>) -> Self {
        Self {
            current_time:     global.current_time.read_only(),
            set_current_time: global.current_time.write_only(),
            is_playing:       global.is_playing.read_only(),
            set_is_playing:   global.is_playing.write_only(),
            duration:         global.duration.read_only(),
            set_duration:     global.duration.write_only(),
            volume:           global.volume.read_only(),
            set_volume:       global.volume.write_only(),
            current_segment_idx,
            engine:      global.engine,
            stem_engine: global.stem_engine,
            stem_gains:  global.stem_gains,
        }
    }
}

// ---------------------------------------------------------------------------
// Player — 両画面共通プレーヤーバー
//
// active_page: "analysis" または "visualization"
//   現在表示中の画面を指定。ナビゲーションボタンの表示に使用する。
// ---------------------------------------------------------------------------

#[component]
pub fn Player(track: TrackDataset, active_page: &'static str) -> impl IntoView {
    use crate::state::VisualizationPageState;
    let ctx = use_context::<PlaybackContext>().expect("PlaybackContext missing");
    let viz_state = use_context::<VisualizationPageState>().expect("VisualizationPageState missing");
    let params = leptos_router::use_params_map();
    let stem = params.with_untracked(|p| p.get("stem").cloned().unwrap_or_default());
    let analysis_href    = format!("/analysis/{}", js_sys::encode_uri_component(&stem).as_string().unwrap_or_default());
    let visualize_href   = format!("/visualization/{}", js_sys::encode_uri_component(&stem).as_string().unwrap_or_default());
    let seekbar_ref = create_node_ref::<Div>();

    // 再生/一時停止の共通ロジック（ボタン・キーボード両方から呼ぶ）
    let do_toggle = {
        let ctx = ctx.clone();
        let stem_eng = ctx.stem_engine;
        move || {
            let ctx = ctx.clone();
            spawn_local(async move {
                if ctx.is_playing.get() {
                    if let Some(s) = stem_eng.get_value() { s.pause(); }
                    else if let Some(e) = ctx.engine.get_value() { e.pause(); }
                    ctx.set_is_playing.set(false);
                } else {
                    if let Some(s) = stem_eng.get_value() {
                        s.resume_ctx().await;
                        s.play();
                    } else if let Some(e) = ctx.engine.get_value() {
                        e.resume_ctx().await;
                        e.play();
                    }
                    ctx.set_is_playing.set(true);
                }
            });
        }
    };

    let toggle_play = { let f = do_toggle.clone(); move |_: web_sys::MouseEvent| f() };

    // スペースキーで再生/一時停止トグル
    {
        let do_toggle_sv = store_value(do_toggle.clone());
        use wasm_bindgen::closure::Closure;
        let cb = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
            // 入力フィールドにフォーカスがある場合はスキップ
            if let Some(target) = ev.target() {
                if let Ok(el) = target.dyn_into::<web_sys::HtmlElement>() {
                    let tag = el.tag_name().to_lowercase();
                    if tag == "input" || tag == "textarea" { return; }
                }
            }
            if ev.code() == "Space" {
                ev.prevent_default();
                do_toggle_sv.with_value(|f| f());
            }
        });
        let win = web_sys::window().unwrap();
        let cb_ref = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
        let _ = win.add_event_listener_with_callback("keydown", &cb_ref);
        // Player アンマウント時にリスナーを除去する。
        // cb.forget() を使うと window にリスナーが残り続け、Player が再マウントされて
        // do_toggle_sv のスコープが破棄された後に発火すると WASM パニックになる。
        on_cleanup(move || {
            if let Some(w) = web_sys::window() {
                let _ = w.remove_event_listener_with_callback("keydown", &cb_ref);
            }
            drop(cb);
        });
    }

    let seek = {
        let ctx = ctx.clone();
        let stem_eng = ctx.stem_engine;
        move |ev: web_sys::MouseEvent| {
            if let Some(bar) = seekbar_ref.get() {
                let rect = bar.get_bounding_client_rect();
                let w = rect.width();
                if w <= 0.0 { return; }
                let frac = (ev.client_x() as f64 - rect.left()) / w;
                let t = frac.clamp(0.0, 1.0) * ctx.duration.get();
                if let Some(s) = stem_eng.get_value() { s.seek(t); }
                else if let Some(e) = ctx.engine.get_value() { e.seek(t); }
                ctx.set_current_time.set(t);
            }
        }
    };

    let set_vol = {
        let ctx = ctx.clone();
        let stem_gains   = ctx.stem_gains;
        let stem_volumes = viz_state.stem_volumes.read_only();
        move |ev: web_sys::Event| {
            let val: f64 = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                .map(|el| el.value())
                .unwrap_or_default()
                .parse()
                .unwrap_or(1.0);
            ctx.set_volume.set(val);
            // ステムが有効なら個別音量を掛け合わせて適用、なければメインエンジン
            if let Some(gains) = stem_gains.get_value() {
                let sv = stem_volumes.get();
                gains.vocals.gain().set_value((val * sv.vocals) as f32);
                gains.drums.gain().set_value((val * sv.drums) as f32);
                gains.bass.gain().set_value((val * sv.bass) as f32);
                gains.others.gain().set_value((val * sv.others) as f32);
            } else if let Some(eng) = ctx.engine.get_value() {
                eng.set_volume(val);
            }
        }
    };

    let current_pct = create_memo(move |_| {
        let d = ctx.duration.get();
        if d > 0.0 { ctx.current_time.get() / d * 100.0 } else { 0.0 }
    });

    let bpm_display = track.bpm.map(|b| format!("{:.0} BPM", b)).unwrap_or_default();

    view! {
        <div class="bg-gray-900 border-b border-gray-700 px-4 py-3 flex items-center gap-3 flex-shrink-0">
            // ホームへ戻る
            <a href="/" class="text-gray-500 hover:text-gray-300 text-sm transition-colors flex-shrink-0">"◀"</a>

            // 再生/一時停止ボタン
            <button
                on:click=toggle_play
                class="w-10 h-10 rounded-full bg-orange-600 hover:bg-orange-500 flex items-center justify-center text-white transition-colors flex-shrink-0"
            >
                {move || if ctx.is_playing.get() { "||" } else { "▶" }}
            </button>

            // トラック名 + BPM
            <div class="flex-shrink-0 min-w-0 max-w-40">
                <p class="text-xs text-gray-100 truncate font-medium">{track.track_filename.clone()}</p>
                <p class="text-xs text-orange-400 font-mono">{bpm_display}</p>
            </div>

            // シークバー
            <div
                node_ref=seekbar_ref
                class="flex-1 h-4 bg-gray-700 rounded-full cursor-pointer relative group"
                on:click=seek
            >
                <div
                    class="h-full bg-orange-500 rounded-full group-hover:bg-orange-400 transition-colors pointer-events-none"
                    style=move || format!("width:{:.3}%", current_pct.get())
                />
            </div>

            // 再生時刻
            <span class="font-mono text-xs text-gray-300 flex-shrink-0">
                {move || format_time(ctx.current_time.get())}
                " / "
                {move || format_time(ctx.duration.get())}
            </span>

            // マスター音量
            <input
                type="range" min="0" max="1" step="0.02"
                class="w-16 accent-orange-500 flex-shrink-0"
                prop:value=move || ctx.volume.get().to_string()
                on:input=set_vol
            />

            // 画面切り替えナビゲーション
            <div class="flex gap-1 flex-shrink-0">
                {if active_page == "analysis" {
                    view! {
                        <span class="text-xs px-2 py-1 rounded bg-orange-600 text-white font-semibold">"Analysis"</span>
                        <a href=visualize_href class="text-xs px-2 py-1 rounded bg-gray-700 hover:bg-gray-600 text-gray-300 transition-colors">"Visualize"</a>
                    }.into_view()
                } else {
                    view! {
                        <a href=analysis_href class="text-xs px-2 py-1 rounded bg-gray-700 hover:bg-gray-600 text-gray-300 transition-colors">"Analysis"</a>
                        <span class="text-xs px-2 py-1 rounded bg-orange-600 text-white font-semibold">"Visualize"</span>
                    }.into_view()
                }}
            </div>
        </div>
    }
}
