use crate::api;
use crate::components::player::PlaybackContext;
use crate::state::VisualizationPageState;
use crate::types::{segment_color, SegmentResult, TrackDataset};
use leptos::*;
use leptos::html::Div;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

// ---------------------------------------------------------------------------
// Timeline — 両画面共通タイムラインコンポーネント
//
// 操作:
//   通常クリック  : セクション先頭へジャンプ（stem_engine 優先）
//   Shift+クリック: クリック位置にシーク
//   Ctrl+クリック : セクション選択 ON/OFF → ループゾーン自動更新
//   右クリック    : コンテキストメニュー → ラベル変更 / Undo
//
// ループオーバーレイ: VisualizationPageState の loop_start / loop_end / loop_active を参照
// ---------------------------------------------------------------------------

const SECTION_LABELS: &[(&str, &str)] = &[
    ("verse",      "Verse"),
    ("chorus",     "Chorus"),
    ("pre-chorus", "Pre-Chorus"),
    ("bridge",     "Bridge"),
    ("intro",      "Intro"),
    ("outro",      "Outro"),
    ("break",      "Break"),
    ("solo",       "Solo"),
];

// コンテキストメニューの状態: (segment_index, mouse_x, mouse_y)
#[derive(Clone, Copy)]
struct MenuState {
    segment_index: u32,
    x: i32,
    y: i32,
}

#[component]
pub fn Timeline(
    track: TrackDataset,
    stem: String,
    /// ラベル変更・Undo 後に呼び出されるコールバック（親が track_data をリフレッシュするために使用）
    #[prop(optional)] on_updated: Option<Callback<()>>,
) -> impl IntoView {
    let ctx       = use_context::<PlaybackContext>().expect("PlaybackContext missing");
    let viz_state = use_context::<VisualizationPageState>().expect("VisualizationPageState missing");
    let stem_engine_sv = ctx.stem_engine;
    let timeline_ref = create_node_ref::<Div>();

    let loop_start              = viz_state.loop_start.read_only();
    let loop_end                = viz_state.loop_end.read_only();
    let loop_active             = viz_state.loop_active.read_only();
    let selected_segment_indices     = viz_state.selected_segment_indices.read_only();
    let set_selected_segment_indices = viz_state.selected_segment_indices.write_only();
    let set_loop_start          = viz_state.loop_start.write_only();
    let set_loop_end            = viz_state.loop_end.write_only();
    let set_loop_active         = viz_state.loop_active.write_only();

    // ローカルのセグメントシグナル（変更即反映のため）
    let local_segments: RwSignal<Vec<SegmentResult>> = create_rw_signal(track.segments.clone());

    // on_updated を spawn_local 内から呼べるよう StoredValue で保持
    let on_updated_sv = store_value(on_updated);

    // コンテキストメニュー状態
    let menu_state: RwSignal<Option<MenuState>> = create_rw_signal(None);

    // セクション選択が変わったらループゾーンを自動更新
    {
        let segs_signal = local_segments.read_only();
        create_effect(move |_| {
            let indices = selected_segment_indices.get();
            if indices.is_empty() { return; }
            let segs = segs_signal.get();
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

    // ドキュメントクリックでメニューを閉じる
    {
        let menu_state_w = menu_state.write_only();
        create_effect(move |_| {
            let closure = Closure::<dyn Fn(web_sys::MouseEvent)>::new(move |_ev: web_sys::MouseEvent| {
                menu_state_w.set(None);
            });
            if let Some(win) = web_sys::window() {
                let _ = win.add_event_listener_with_callback(
                    "click",
                    closure.as_ref().unchecked_ref(),
                );
            }
            closure.forget(); // メモリリークを許容（コンポーネントライフタイム全体で有効）
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
                each=move || local_segments.get()
                key=|seg| seg.index
                children={
                    let ctx = ctx.clone();
                    let stem = stem.clone();
                    move |seg: SegmentResult| {
                        let dur_s     = ctx.duration;
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
                        let stem_for_menu = stem.clone();

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
                                on:contextmenu=move |ev: web_sys::MouseEvent| {
                                    ev.prevent_default();
                                    ev.stop_propagation();
                                    menu_state.set(Some(MenuState {
                                        segment_index: seg_idx,
                                        x: ev.client_x(),
                                        y: ev.client_y(),
                                    }));
                                    let _ = stem_for_menu.clone(); // キャプチャ確認用
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

        // コンテキストメニュー（fixed 表示）
        {move || {
            let Some(ms) = menu_state.get() else { return None };
            let stem_cm = stem.clone();

            // 画面端はみ出し対策: メニュー幅160px, 高さ約280px
            let x = ms.x;
            let y = ms.y;
            let win_w = web_sys::window().and_then(|w| w.inner_width().ok()).and_then(|v| v.as_f64()).unwrap_or(1920.0) as i32;
            let win_h = web_sys::window().and_then(|w| w.inner_height().ok()).and_then(|v| v.as_f64()).unwrap_or(1080.0) as i32;
            let adj_x = if x + 168 > win_w { win_w - 172 } else { x };
            let adj_y = if y + 290 > win_h { win_h - 294 } else { y };

            Some(view! {
                <div
                    class="fixed bg-gray-800 border border-gray-600 rounded shadow-xl text-sm text-white z-50 py-1 min-w-40"
                    style=format!("left:{}px;top:{}px", adj_x, adj_y)
                    on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()
                    on:contextmenu=|ev: web_sys::MouseEvent| ev.prevent_default()
                >
                    {SECTION_LABELS.iter().map(|(value, display)| {
                        let v = value.to_string();
                        let stem_item = stem_cm.clone();
                        let seg_idx = ms.segment_index;
                        view! {
                            <button
                                class="w-full text-left px-4 py-1.5 hover:bg-gray-700 transition-colors"
                                on:click=move |_| {
                                    let label = v.clone();
                                    let stem_async = stem_item.clone();
                                    menu_state.set(None);
                                    spawn_local(async move {
                                        match api::update_segment_label(&stem_async, seg_idx, &label).await {
                                            Ok(()) => {
                                                local_segments.update(|segs| {
                                                    if let Some(s) = segs.iter_mut().find(|s| s.index == seg_idx) {
                                                        s.label = label.clone();
                                                    }
                                                });
                                                // 親コンポーネントに track_data のリフレッシュを要求
                                                on_updated_sv.with_value(|cb| {
                                                    if let Some(cb) = cb { cb.call(()); }
                                                });
                                            }
                                            Err(e) => {
                                                web_sys::console::error_1(&format!("update_segment_label error: {}", e).into());
                                            }
                                        }
                                    });
                                }
                            >
                                {*display}
                            </button>
                        }
                    }).collect_view()}
                    <div class="border-t border-gray-600 my-1" />
                    <button
                        class="w-full text-left px-4 py-1.5 hover:bg-gray-700 transition-colors text-gray-300"
                        on:click={
                            let stem_undo = stem_cm.clone();
                            move |_| {
                                let stem_async = stem_undo.clone();
                                menu_state.set(None);
                                spawn_local(async move {
                                    match api::undo_segment_label(&stem_async).await {
                                        Ok(true) => {
                                            // バックエンドの最新状態を取得して反映
                                            match api::fetch_track(&stem_async).await {
                                                Ok(updated_track) => {
                                                    local_segments.set(updated_track.segments);
                                                    // 親コンポーネントに track_data のリフレッシュを要求
                                                    on_updated_sv.with_value(|cb| {
                                                        if let Some(cb) = cb { cb.call(()); }
                                                    });
                                                }
                                                Err(e) => {
                                                    web_sys::console::error_1(&format!("fetch_track after undo error: {}", e).into());
                                                }
                                            }
                                        }
                                        Ok(false) => {
                                            // 履歴なし、何もしない
                                        }
                                        Err(e) => {
                                            web_sys::console::error_1(&format!("undo_segment_label error: {}", e).into());
                                        }
                                    }
                                });
                            }
                        }
                    >
                        "↩ 元に戻す (Undo)"
                    </button>
                </div>
            })
        }}
    }
}
