use leptos::*;
use wasm_bindgen::JsCast;

use crate::pages::visualization::VizContext;

// ---------------------------------------------------------------------------
// StemMixer — Stem volume sliders + loop controls
// ---------------------------------------------------------------------------

#[component]
pub fn StemMixer() -> impl IntoView {
    let viz = use_context::<VizContext>().expect("VizContext missing");

    view! {
        <div class="flex flex-col gap-4 text-sm">
            // Stem sliders
            <div class="bg-gray-800 rounded-xl p-4 border border-gray-700 space-y-3">
                <div class="flex items-center justify-between mb-1">
                    <h3 class="text-xs font-semibold text-gray-400 uppercase tracking-wider">"Stem Mixer"</h3>
                    {move || {
                        if !viz.stems_available.get() {
                            Some(view! {
                                <span class="text-xs bg-gray-700 text-gray-400 px-2 py-0.5 rounded-full">"Visual only"</span>
                            })
                        } else {
                            None
                        }
                    }}
                </div>

                <StemSlider
                    label="Vocals"
                    icon="🎤"
                    get_vol=move || viz.stem_volumes.get().vocals
                    set_vol={
                        let viz = viz.clone();
                        move |v: f64| {
                            let mut vols = viz.stem_volumes.get();
                            vols.vocals = v;
                            viz.set_stem_volumes.set(vols);
                            if let Some(gains) = viz.stem_gains.get_value() {
                                gains.vocals.gain().set_value(v as f32);
                            }
                        }
                    }
                />
                <StemSlider
                    label="Drums"
                    icon="🥁"
                    get_vol=move || viz.stem_volumes.get().drums
                    set_vol={
                        let viz = viz.clone();
                        move |v: f64| {
                            let mut vols = viz.stem_volumes.get();
                            vols.drums = v;
                            viz.set_stem_volumes.set(vols);
                            if let Some(gains) = viz.stem_gains.get_value() {
                                gains.drums.gain().set_value(v as f32);
                            }
                        }
                    }
                />
                <StemSlider
                    label="Bass"
                    icon="🎸"
                    get_vol=move || viz.stem_volumes.get().bass
                    set_vol={
                        let viz = viz.clone();
                        move |v: f64| {
                            let mut vols = viz.stem_volumes.get();
                            vols.bass = v;
                            viz.set_stem_volumes.set(vols);
                            if let Some(gains) = viz.stem_gains.get_value() {
                                gains.bass.gain().set_value(v as f32);
                            }
                        }
                    }
                />
                <StemSlider
                    label="Others"
                    icon="🎹"
                    get_vol=move || viz.stem_volumes.get().others
                    set_vol={
                        let viz = viz.clone();
                        move |v: f64| {
                            let mut vols = viz.stem_volumes.get();
                            vols.others = v;
                            viz.set_stem_volumes.set(vols);
                            if let Some(gains) = viz.stem_gains.get_value() {
                                gains.others.gain().set_value(v as f32);
                            }
                        }
                    }
                />
            </div>

            // Beat offset
            <BeatOffsetControl />

            // Loop controls
            <LoopControls />
        </div>
    }
}

// ---------------------------------------------------------------------------
// Individual stem slider row
// ---------------------------------------------------------------------------

#[component]
fn StemSlider<GV, SV>(
    label: &'static str,
    icon: &'static str,
    get_vol: GV,
    set_vol: SV,
) -> impl IntoView
where
    GV: Fn() -> f64 + 'static + Clone,
    SV: Fn(f64) + 'static + Clone,
{
    let set_vol_clone = set_vol.clone();
    let get_vol_mute = get_vol.clone();
    let get_vol_muted = get_vol.clone();
    let get_vol_val = get_vol.clone();
    let on_input = move |ev: web_sys::Event| {
        let val: f64 = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
            .map(|el| el.value())
            .unwrap_or_default()
            .parse()
            .unwrap_or(1.0);
        set_vol(val);
    };

    let on_mute = move |_| {
        let current = get_vol_mute();
        if current > 0.0 {
            set_vol_clone(0.0);
        } else {
            set_vol_clone(1.0);
        }
    };

    let is_muted_class = move || get_vol_muted() == 0.0;
    let is_muted_title = {
        let gv = get_vol.clone();
        move || gv() == 0.0
    };
    let is_muted_text = {
        let gv = get_vol.clone();
        move || gv() == 0.0
    };

    view! {
        <div class="flex flex-col gap-1">
            // Row 1: icon · label · value · mute
            <div class="flex items-center gap-1.5">
                <span class="text-sm flex-shrink-0">{icon}</span>
                <span class="text-xs text-gray-300 flex-shrink-0 flex-1">{label}</span>
                <span class="text-xs text-gray-400 font-mono w-6 text-right flex-shrink-0">
                    {move || format!("{:.1}", get_vol())}
                </span>
                <button
                    class=move || {
                        if is_muted_class() {
                            "text-xs rounded px-1.5 py-0.5 bg-orange-600 text-white flex-shrink-0"
                        } else {
                            "text-xs rounded px-1.5 py-0.5 bg-gray-700 text-gray-300 hover:bg-gray-600 flex-shrink-0"
                        }
                    }
                    on:click=on_mute
                    title=move || if is_muted_title() { "Unmute" } else { "Mute" }
                >
                    {move || if is_muted_text() { "ON" } else { "MUTE" }}
                </button>
            </div>
            // Row 2: slider full width
            <input
                type="range"
                min="0"
                max="1"
                step="0.02"
                class="w-full accent-orange-500 h-1"
                prop:value=move || get_vol_val().to_string()
                on:input=on_input
            />
        </div>
    }
}

// ---------------------------------------------------------------------------
// Beat offset control
// ---------------------------------------------------------------------------

#[component]
fn BeatOffsetControl() -> impl IntoView {
    let viz = use_context::<VizContext>().expect("VizContext missing");

    let on_input = move |ev: web_sys::Event| {
        let val: f64 = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
            .map(|el| el.value())
            .unwrap_or_default()
            .parse()
            .unwrap_or(0.0);
        viz.set_beat_offset.set(val);
    };

    let on_reset = move |_| {
        viz.set_beat_offset.set(0.0);
    };

    view! {
        <div class="bg-gray-800 rounded-xl p-4 border border-gray-700">
            <div class="flex items-center justify-between mb-2">
                <h3 class="text-xs font-semibold text-gray-400 uppercase tracking-wider">"Beat Offset"</h3>
                <button
                    class="text-xs px-2 py-0.5 rounded bg-gray-700 hover:bg-gray-600 text-gray-300"
                    on:click=on_reset
                >"Reset"</button>
            </div>
            <input
                type="range"
                min="-0.5"
                max="0.5"
                step="0.01"
                class="w-full accent-orange-500 h-1"
                prop:value=move || viz.beat_offset.get().to_string()
                on:input=on_input
            />
            <div class="flex justify-between text-xs text-gray-600 mt-0.5">
                <span>"-0.5s"</span>
                <span>"0"</span>
                <span>"+0.5s"</span>
            </div>
            <p class="text-center font-mono text-sm text-orange-400 mt-1">
                {move || {
                    let v = viz.beat_offset.get();
                    if v >= 0.0 { format!("+{:.2}s", v) } else { format!("{:.2}s", v) }
                }}
            </p>
        </div>
    }
}

// ---------------------------------------------------------------------------
// Loop controls panel
// ---------------------------------------------------------------------------

#[component]
fn LoopControls() -> impl IntoView {
    use crate::components::player::PlaybackContext;
    use crate::types::format_time;

    let ctx = use_context::<PlaybackContext>().expect("PlaybackContext missing");
    let viz = use_context::<VizContext>().expect("VizContext missing");

    let set_loop_start = move |_| {
        let t = ctx.current_time.get();
        viz.set_loop_start.set(Some(t));
        viz.set_loop_active.set(true);
    };

    let set_loop_end = move |_| {
        let t = ctx.current_time.get();
        viz.set_loop_end.set(Some(t));
        viz.set_loop_active.set(true);
    };

    let toggle_loop = {
        let viz = viz.clone();
        move |_| {
            viz.set_loop_active.update(|v| *v = !*v);
        }
    };

    let clear_loop = {
        let viz = viz.clone();
        move |_| {
            viz.set_loop_start.set(None);
            viz.set_loop_end.set(None);
            viz.set_loop_active.set(false);
            viz.set_selected_segment_indices.set(vec![]);
        }
    };

    view! {
        <div class="bg-gray-800 rounded-xl p-4 border border-gray-700 space-y-3">
            <h3 class="text-xs font-semibold text-gray-400 uppercase tracking-wider">"Loop"</h3>

            // Loop start/end set buttons
            <div class="flex gap-2">
                <button
                    class="flex-1 text-xs bg-gray-700 hover:bg-gray-600 text-gray-200 rounded px-2 py-1.5 transition-colors"
                    on:click=set_loop_start
                    title="Set current position as loop start"
                >
                    "◀ Start"
                </button>
                <button
                    class="flex-1 text-xs bg-gray-700 hover:bg-gray-600 text-gray-200 rounded px-2 py-1.5 transition-colors"
                    on:click=set_loop_end
                    title="Set current position as loop end"
                >
                    "End ▶"
                </button>
            </div>

            // Loop range display
            <div class="flex items-center gap-1 text-xs font-mono text-gray-400">
                <span class=move || {
                    if viz.loop_start.get().is_some() { "text-orange-400" } else { "text-gray-600" }
                }>
                    {move || viz.loop_start.get().map(format_time).unwrap_or_else(|| "--:--".into())}
                </span>
                <span class="flex-1 text-center text-gray-600">"───────"</span>
                <span class=move || {
                    if viz.loop_end.get().is_some() { "text-orange-400" } else { "text-gray-600" }
                }>
                    {move || viz.loop_end.get().map(format_time).unwrap_or_else(|| "--:--".into())}
                </span>
            </div>

            // ON/OFF + Clear
            <div class="flex gap-2">
                <button
                    class=move || {
                        if viz.loop_active.get() {
                            "flex-1 text-xs rounded px-2 py-1.5 font-semibold bg-orange-600 hover:bg-orange-500 text-white transition-colors"
                        } else {
                            "flex-1 text-xs rounded px-2 py-1.5 font-semibold bg-gray-700 hover:bg-gray-600 text-gray-300 transition-colors"
                        }
                    }
                    on:click=toggle_loop
                >
                    {move || if viz.loop_active.get() { "Loop: ON" } else { "Loop: OFF" }}
                </button>
                <button
                    class="text-xs bg-gray-700 hover:bg-red-900/60 text-gray-400 hover:text-red-300 rounded px-3 py-1.5 transition-colors"
                    on:click=clear_loop
                    title="Clear loop zone and section selection"
                >
                    "✕"
                </button>
            </div>

            // Section selection hint
            <p class="text-xs text-gray-600 leading-relaxed">
                "Ctrl+click on timeline to select sections for loop"
            </p>
        </div>
    }
}
