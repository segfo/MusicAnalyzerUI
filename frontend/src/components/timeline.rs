use crate::components::player::PlaybackContext;
use crate::state::VisualizationPageState;
use crate::types::{segment_color, TrackDataset};
use leptos::*;
use leptos::html::Div;

// ---------------------------------------------------------------------------
// Timeline — 両画面共通タイムラインコンポーネント
//
// 操作:
//   通常クリック  : セクション先頭へジャンプ（stem_engine 優先）
//   Shift+クリック: クリック位置にシーク
//   Ctrl+クリック : セクション選択 ON/OFF → ループゾーン自動更新
//
// ループオーバーレイ: VisualizationPageState の loop_start / loop_end / loop_active を参照
// ---------------------------------------------------------------------------

#[component]
pub fn Timeline(track: TrackDataset) -> impl IntoView {
    let ctx       = use_context::<PlaybackContext>().expect("PlaybackContext missing");
    let viz_state = use_context::<VisualizationPageState>().expect("VisualizationPageState missing");
    let stem_engine_sv = ctx.stem_engine;
    let segments = track.segments.clone();
    let timeline_ref = create_node_ref::<Div>();

    let loop_start              = viz_state.loop_start.read_only();
    let loop_end                = viz_state.loop_end.read_only();
    let loop_active             = viz_state.loop_active.read_only();
    let selected_segment_indices     = viz_state.selected_segment_indices.read_only();
    let set_selected_segment_indices = viz_state.selected_segment_indices.write_only();
    let set_loop_start          = viz_state.loop_start.write_only();
    let set_loop_end            = viz_state.loop_end.write_only();
    let set_loop_active         = viz_state.loop_active.write_only();

    // セクション選択が変わったらループゾーンを自動更新
    {
        let segs = track.segments.clone();
        create_effect(move |_| {
            let indices = selected_segment_indices.get();
            if indices.is_empty() { return; }
            let selected: Vec<_> = segs.iter()
                .filter(|s| indices.contains(&s.index))
                .collect();
            if selected.is_empty() { return; }
            let min_start = selected.iter().map(|s| s.start).fold(f64::INFINITY, f64::min);
            let max_end   = selected.iter().map(|s| s.end).fold(f64::NEG_INFINITY, f64::max);
            set_loop_start.set(Some(min_start));
            set_loop_end.set(Some(max_end));
            set_loop_active.set(true);
        });
    }

    view! {
        <div
            node_ref=timeline_ref
            class="relative w-full h-12 bg-gray-800 border-b border-gray-700 overflow-hidden select-none flex-shrink-0"
            on:click={
                let ctx = ctx.clone();
                move |ev: web_sys::MouseEvent| {
                    if !ev.shift_key() { return; }
                    let Some(el) = timeline_ref.get() else { return };
                    let rect = el.get_bounding_client_rect();
                    let w = rect.width();
                    let d = ctx.duration.get();
                    if w > 0.0 && d > 0.0 {
                        let frac = (ev.client_x() as f64 - rect.left()) / w;
                        let t = frac.clamp(0.0, 1.0) * d;
                        if let Some(s) = stem_engine_sv.get_value() { s.seek(t); }
                        else if let Some(e) = ctx.engine.get_value() { e.seek(t); }
                        ctx.set_current_time.set(t);
                    }
                }
            }
        >
            // セグメントバー
            <For
                each=move || segments.clone()
                key=|seg| seg.index
                children={
                    let ctx = ctx.clone();
                    move |seg: crate::types::SegmentResult| {
                        let dur_s    = ctx.duration;
                        let seg_start = seg.start;
                        let seg_dur   = seg.duration;
                        let seg_idx   = seg.index;
                        let color     = segment_color(&seg.label);
                        let label     = seg.label.clone();
                        let title     = format!("{} ({:.1}s–{:.1}s)", seg.label, seg.start, seg.end);
                        let eng       = ctx.engine;
                        let stem_eng  = ctx.stem_engine;
                        let set_t     = ctx.set_current_time;
                        let current_time = ctx.current_time;
                        let seg_end   = seg.end;

                        view! {
                            <div
                                class=move || {
                                    let sel = selected_segment_indices.get().contains(&seg_idx);
                                    let t   = current_time.get();
                                    let playing = t >= seg_start && t < seg_end;
                                    format!(
                                        "{color} absolute top-0 h-full cursor-pointer flex items-center \
                                         justify-center overflow-hidden transition-opacity {}",
                                        if sel     { "opacity-100 outline outline-2 outline-orange-400" }
                                        else if playing { "opacity-95" }
                                        else       { "opacity-50 hover:opacity-95" }
                                    )
                                }
                                style=move || {
                                    let d = dur_s.get();
                                    if d > 0.0 {
                                        format!("left:{:.3}%;width:{:.3}%", seg_start/d*100.0, seg_dur/d*100.0)
                                    } else { "left:0%;width:0%".into() }
                                }
                                on:click=move |ev: web_sys::MouseEvent| {
                                    if ev.ctrl_key() || ev.meta_key() {
                                        // Ctrl+click: セクション選択トグル
                                        set_selected_segment_indices.update(|ids| {
                                            if let Some(pos) = ids.iter().position(|&i| i == seg_idx) {
                                                ids.remove(pos);
                                            } else {
                                                ids.push(seg_idx);
                                            }
                                        });
                                    } else if ev.shift_key() {
                                        // Shift+click: コンテナのハンドラに委ねる（バブルアップ）
                                    } else {
                                        // 通常クリック: セクション先頭にジャンプ（stem_engine 優先）
                                        if let Some(s) = stem_eng.get_value() { s.seek(seg_start); }
                                        else if let Some(e) = eng.get_value() { e.seek(seg_start); }
                                        set_t.set(seg_start);
                                    }
                                }
                                title=title
                            >
                                <span class="text-white text-xs font-medium px-1 truncate pointer-events-none">
                                    {label}
                                </span>
                            </div>
                        }
                    }
                }
            />

            // ループゾーンオーバーレイ
            {move || {
                let d = ctx.duration.get();
                if let (Some(ls), Some(le), true, true) = (
                    loop_start.get(), loop_end.get(), loop_active.get(), d > 0.0
                ) {
                    if le > ls {
                        let left_pct  = ls / d * 100.0;
                        let width_pct = (le - ls) / d * 100.0;
                        Some(view! {
                            <div
                                class="absolute top-0 h-full bg-orange-400/20 border-x-2 border-orange-400 pointer-events-none z-10"
                                style=move || format!("left:{left_pct:.3}%;width:{width_pct:.3}%")
                            />
                        })
                    } else { None }
                } else { None }
            }}

            // 再生カーソル
            <div
                class="absolute top-0 h-full w-0.5 bg-white pointer-events-none z-20 shadow-sm"
                style=move || {
                    let d = ctx.duration.get();
                    let pct = if d > 0.0 { ctx.current_time.get() / d * 100.0 } else { 0.0 };
                    format!("left:{pct:.3}%")
                }
            />
        </div>
    }
}
