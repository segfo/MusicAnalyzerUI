use crate::types::{format_time, TrackDataset};
use leptos::*;
use leptos::html::Div;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioBuffer, AudioBufferSourceNode, AudioContext, AudioScheduledSourceNode, GainNode};

// ---------------------------------------------------------------------------
// AudioEngine — wraps Web Audio API for sample-accurate seeking
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AudioEngine {
    ctx: AudioContext,
    buffer: Rc<AudioBuffer>,
    gain: GainNode,
    state: Rc<RefCell<EngineState>>,
}

struct EngineState {
    source: Option<AudioBufferSourceNode>,
    offset_at_start: f64,   // buffer seconds when play() was called
    ctx_time_at_start: f64, // AudioContext.currentTime at that moment
    playing: bool,
}

impl AudioEngine {
    pub fn new(ctx: AudioContext, buffer: AudioBuffer) -> Result<Self, String> {
        let gain = ctx.create_gain().map_err(|e| format!("{e:?}"))?;
        gain.connect_with_audio_node(&ctx.destination())
            .map_err(|e| format!("{e:?}"))?;
        Ok(Self {
            ctx,
            buffer: Rc::new(buffer),
            gain,
            state: Rc::new(RefCell::new(EngineState {
                source: None,
                offset_at_start: 0.0,
                ctx_time_at_start: 0.0,
                playing: false,
            })),
        })
    }

    pub fn duration(&self) -> f64 {
        self.buffer.duration()
    }

    pub fn current_time(&self) -> f64 {
        let state = self.state.borrow();
        if state.playing {
            let elapsed = self.ctx.current_time() - state.ctx_time_at_start;
            (state.offset_at_start + elapsed).min(self.buffer.duration())
        } else {
            state.offset_at_start
        }
    }

    pub fn is_playing(&self) -> bool {
        self.state.borrow().playing
    }

    /// Resume the AudioContext (needed after browser autoplay policy suspends it).
    pub async fn resume_ctx(&self) {
        if self.ctx.state() != web_sys::AudioContextState::Running {
            if let Ok(promise) = self.ctx.resume() {
                let _ = JsFuture::from(promise).await;
            }
        }
    }

    pub fn play(&self) {
        if !self.state.borrow().playing {
            let current = self.current_time();
            let duration = self.duration();
            // 再生残り秒数が 1 秒未満（曲が終了している）場合は先頭から再生
            let offset = if duration - current < 1.0 { 0.0 } else { current };
            self.play_from(offset);
        }
    }

    pub fn pause(&self) {
        let mut state = self.state.borrow_mut();
        if state.playing {
            if let Some(src) = state.source.take() {
                let _ = AudioScheduledSourceNode::stop_with_when(&src, 0.0);
            }
            let elapsed = self.ctx.current_time() - state.ctx_time_at_start;
            state.offset_at_start =
                (state.offset_at_start + elapsed).min(self.buffer.duration());
            state.playing = false;
        }
    }

    /// Seek to an exact time in seconds. Restarts playback if currently playing.
    pub fn seek(&self, time: f64) {
        let time = time.clamp(0.0, self.buffer.duration());
        let playing = self.state.borrow().playing;
        if playing {
            self.play_from(time);
        } else {
            self.state.borrow_mut().offset_at_start = time;
        }
    }

    pub fn set_volume(&self, vol: f64) {
        self.gain.gain().set_value(vol as f32);
    }

    fn play_from(&self, offset: f64) {
        let offset = offset.clamp(0.0, self.buffer.duration());

        // Stop any existing source
        {
            let mut state = self.state.borrow_mut();
            if let Some(src) = state.source.take() {
                let _ = AudioScheduledSourceNode::stop_with_when(&src, 0.0);
            }
        }

        let Ok(src) = self.ctx.create_buffer_source() else { return };
        src.set_buffer(Some(&self.buffer));
        let _ = src.connect_with_audio_node(&self.gain);
        // start(when=0 → now, grain_offset=offset → start position in seconds)
        let _ = src.start_with_when_and_grain_offset(0.0, offset);

        let mut state = self.state.borrow_mut();
        state.ctx_time_at_start = self.ctx.current_time();
        state.offset_at_start = offset;
        state.playing = true;
        state.source = Some(src);
    }
}

// ---------------------------------------------------------------------------
// StemVolumes — per-stem visual/audio intensity (0.0–1.0)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct StemVolumes {
    pub vocals: f64,
    pub drums: f64,
    pub bass: f64,
    pub others: f64,
}

impl Default for StemVolumes {
    fn default() -> Self {
        Self { vocals: 1.0, drums: 1.0, bass: 1.0, others: 1.0 }
    }
}

// ---------------------------------------------------------------------------
// StemGains — GainNodes for individual stem volume control
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct StemGains {
    pub vocals: GainNode,
    pub drums: GainNode,
    pub bass: GainNode,
    pub others: GainNode,
}

// ---------------------------------------------------------------------------
// StemAudioEngine — plays 4 stems in sync on a shared AudioContext
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct StemAudioEngine {
    ctx: AudioContext,
    buffers: Rc<[Option<AudioBuffer>; 4]>,
    gains: [GainNode; 4],
    state: Rc<RefCell<StemEngineState>>,
}

struct StemEngineState {
    sources: [Option<AudioBufferSourceNode>; 4],
    offset_at_start: f64,
    ctx_time_at_start: f64,
    playing: bool,
    duration: f64,
}

impl StemAudioEngine {
    pub fn new(ctx: AudioContext, buffers: [Option<AudioBuffer>; 4], gains: [GainNode; 4]) -> Self {
        let duration = buffers.iter().flatten().map(|b| b.duration()).fold(0.0_f64, f64::max);
        Self {
            ctx,
            buffers: Rc::new(buffers),
            gains,
            state: Rc::new(RefCell::new(StemEngineState {
                sources: [None, None, None, None],
                offset_at_start: 0.0,
                ctx_time_at_start: 0.0,
                playing: false,
                duration,
            })),
        }
    }

    pub fn duration(&self) -> f64 { self.state.borrow().duration }

    pub fn current_time(&self) -> f64 {
        let s = self.state.borrow();
        if s.playing {
            (s.offset_at_start + self.ctx.current_time() - s.ctx_time_at_start).min(s.duration)
        } else {
            s.offset_at_start
        }
    }

    pub fn is_playing(&self) -> bool { self.state.borrow().playing }

    pub async fn resume_ctx(&self) {
        if self.ctx.state() != web_sys::AudioContextState::Running {
            if let Ok(p) = self.ctx.resume() { let _ = JsFuture::from(p).await; }
        }
    }

    pub fn play(&self) {
        if !self.state.borrow().playing {
            let current = self.current_time();
            let duration = self.duration();
            // 再生残り秒数が 1 秒未満（曲が終了している）場合は先頭から再生
            let offset = if duration - current < 1.0 { 0.0 } else { current };
            self.play_from(offset);
        }
    }

    pub fn pause(&self) {
        let mut s = self.state.borrow_mut();
        if !s.playing { return; }
        for src in s.sources.iter_mut().flatten() {
            let _ = AudioScheduledSourceNode::stop_with_when(src, 0.0);
        }
        let elapsed = self.ctx.current_time() - s.ctx_time_at_start;
        s.offset_at_start = (s.offset_at_start + elapsed).min(s.duration);
        s.sources = [None, None, None, None];
        s.playing = false;
    }

    pub fn seek(&self, time: f64) {
        let time = time.clamp(0.0, self.state.borrow().duration);
        if self.state.borrow().playing { self.play_from(time); }
        else { self.state.borrow_mut().offset_at_start = time; }
    }

    fn play_from(&self, offset: f64) {
        let offset = offset.clamp(0.0, self.state.borrow().duration);
        {
            let mut s = self.state.borrow_mut();
            for src in s.sources.iter_mut().flatten() {
                let _ = AudioScheduledSourceNode::stop_with_when(src, 0.0);
            }
            s.sources = [None, None, None, None];
        }
        let mut new_sources: [Option<AudioBufferSourceNode>; 4] = [None, None, None, None];
        for (i, buf) in self.buffers.iter().enumerate() {
            let Some(buf) = buf else { continue };
            let Ok(src) = self.ctx.create_buffer_source() else { continue };
            src.set_buffer(Some(buf));
            let _ = src.connect_with_audio_node(&self.gains[i]);
            let _ = src.start_with_when_and_grain_offset(0.0, offset);
            new_sources[i] = Some(src);
        }
        let mut s = self.state.borrow_mut();
        s.ctx_time_at_start = self.ctx.current_time();
        s.offset_at_start = offset;
        s.playing = true;
        s.sources = new_sources;
    }
}

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

// ---------------------------------------------------------------------------
// Player — 両画面共通プレーヤーバー
//
// active_page: "analysis" または "visualization"
//   現在表示中の画面を指定。ナビゲーションボタンの表示に使用する。
// ---------------------------------------------------------------------------

#[component]
pub fn Player(track: TrackDataset, active_page: &'static str) -> impl IntoView {
    use crate::pages::visualization::VizContext;
    let ctx = use_context::<PlaybackContext>().expect("PlaybackContext missing");
    let viz = use_context::<VizContext>().expect("VizContext missing");
    let params = leptos_router::use_params_map();
    let stem = params.with(|p| p.get("stem").cloned().unwrap_or_default());
    let analysis_href    = format!("/analysis/{}", js_sys::encode_uri_component(&stem).as_string().unwrap_or_default());
    let visualize_href   = format!("/visualization/{}", js_sys::encode_uri_component(&stem).as_string().unwrap_or_default());
    let seekbar_ref = create_node_ref::<Div>();

    let toggle_play = {
        let ctx = ctx.clone();
        let stem_eng = viz.stem_engine;
        move |_| {
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

    let seek = {
        let ctx = ctx.clone();
        let stem_eng = viz.stem_engine;
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
        let stem_gains = viz.stem_gains;
        let stem_volumes = viz.stem_volumes;
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
