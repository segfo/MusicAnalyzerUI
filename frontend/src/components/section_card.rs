use crate::components::player::PlaybackContext;
use crate::types::{format_time, segment_color, SegmentResult, TrackDataset};
use leptos::*;

#[component]
pub fn SectionCard(track: TrackDataset) -> impl IntoView {
    let ctx = use_context::<PlaybackContext>().expect("PlaybackContext missing");
    let (card_open, set_card_open) = create_signal(false);
    let (prev_idx, set_prev_idx) = create_signal::<Option<usize>>(None);
    let segments = store_value(track.segments.clone());

    // Auto open/close based on segment changes
    create_effect(move |_| {
        let new_idx = ctx.current_segment_idx.get();
        let old_idx = prev_idx.get_untracked();
        if new_idx != old_idx {
            set_prev_idx.set(new_idx);
            set_card_open.set(new_idx.is_some());
        }
    });

    // Close when playback ends
    create_effect(move |_| {
        let playing = ctx.is_playing.get();
        let t = ctx.current_time.get();
        let d = ctx.duration.get();
        if !playing && d > 0.0 && t >= d - 0.5 {
            set_card_open.set(false);
        }
    });

    // Derive each field separately to avoid FnOnce in view!
    let current_seg: Memo<Option<SegmentResult>> = create_memo(move |_| {
        ctx.current_segment_idx
            .get()
            .and_then(|i| segments.get_value().get(i).cloned())
    });

    let label = create_memo(move |_| {
        current_seg.get().map(|s| s.label.clone()).unwrap_or_default()
    });
    let seg_start = create_memo(move |_| current_seg.get().map(|s| s.start).unwrap_or(0.0));
    let seg_end = create_memo(move |_| current_seg.get().map(|s| s.end).unwrap_or(0.0));
    let beat_count = create_memo(move |_| current_seg.get().map(|s| s.beat_count).unwrap_or(0));
    let caption = create_memo(move |_| current_seg.get().and_then(|s| s.caption.clone()));
    let sub_caps = create_memo(move |_| {
        current_seg
            .get()
            .map(|s| s.sub_captions.clone())
            .unwrap_or_default()
    });

    view! {
        // 楽曲のセクションラベルがStart/End以外のときだけ描画する
        <Show when=move || {
            let seg = current_seg.get();
            card_open.get() 
                && seg.is_some() 
                && seg.map_or(false, |s| s.label.to_lowercase() != "end"&&s.label.to_lowercase() != "start")
        }>
            <div class="fixed bottom-4 right-4 w-80 max-h-96 overflow-y-auto bg-gray-800 rounded-xl p-4 shadow-2xl border border-gray-600 z-20">
                // Header
                <div class="flex items-center justify-between mb-3">
                    <div class="flex items-center gap-2">
                        <span
                            class=move || {
                                format!("{} px-2 py-0.5 rounded text-xs font-bold text-white",
                                    segment_color(&label.get()))
                            }
                        >
                            {move || label.get()}
                        </span>
                        <span class="text-gray-400 text-xs font-mono">
                            {move || format!("{} - {}",
                                format_time(seg_start.get()),
                                format_time(seg_end.get())
                            )}
                        </span>
                    </div>
                    <button
                        class="text-gray-500 hover:text-white text-sm leading-none ml-1 flex-shrink-0"
                        on:click=move |_| set_card_open.set(false)
                    >"[X]"</button>
                </div>

                // Caption
                <Show when=move || caption.get().is_some()>
                    <p class="text-sm text-gray-200 leading-relaxed mb-3">
                        {move || caption.get().unwrap_or_default()}
                    </p>
                </Show>

                // Beat count
                <p class="text-xs text-gray-500 mt-3 border-t border-gray-700 pt-2">
                    {move || beat_count.get()}" beats"
                </p>
            </div>
        </Show>
    }
}
