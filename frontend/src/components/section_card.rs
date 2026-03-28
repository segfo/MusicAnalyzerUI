use crate::components::player::PlaybackContext;
use crate::state::VisualizationPageState;
use crate::types::{format_time, segment_color, SegmentResult, TrackDataset};
use leptos::*;
use leptos::leptos_dom::helpers::{set_timeout_with_handle, TimeoutHandle};
use std::time::Duration;

/// viewport 上の list_id 要素 top と card_id 要素 top の差を返す
fn measure_delta_y(list_id: &str, card_id: &str) -> f64 {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return 0.0;
    };
    let Some(list_el) = doc.get_element_by_id(list_id) else {
        return 0.0;
    };
    let Some(card_el) = doc.get_element_by_id(card_id) else {
        return 0.0;
    };
    list_el.get_bounding_client_rect().top() - card_el.get_bounding_client_rect().top()
}

#[component]
pub fn SectionCard(track: TrackDataset) -> impl IntoView {
    let ctx = use_context::<PlaybackContext>().expect("PlaybackContext missing");
    let viz_page_state = use_context::<VisualizationPageState>().expect("VisualizationPageState missing");
    let section_card_enabled = viz_page_state.section_card_enabled.read_only();
    let (card_open, set_card_open) = create_signal(false);
    // カードが視覚的に表示されているか（card_open とは独立）
    // card_open: 有効セグメント内にいるか
    // card_showing: 実際にアニメーションで表示されているか
    let (card_showing, set_card_showing) = create_signal(false);
    let (prev_idx, set_prev_idx) = create_signal::<Option<usize>>(None);
    // カードに表示するセグメントのインデックス（アニメーションと切り離してEnter時のみ更新）
    let (displayed_idx, set_displayed_idx) = create_signal::<Option<usize>>(None);
    let segments = store_value(track.segments.clone());

    // アニメーション用インラインスタイル
    let (anim_style, set_anim_style) = create_signal("opacity:0;pointer-events:none".to_string());
    // 高速切替時の stale closure 防止カウンタ
    let (anim_gen, set_anim_gen) = create_signal(0u32);
    // 保留中のタイムアウトハンドル（コンポーネントアンマウント時にキャンセル）
    let timeout_handle: StoredValue<Option<TimeoutHandle>> = store_value(None);
    // セクション切替 0.5s 前からフェードアウト中かどうか
    let (pre_leaving, set_pre_leaving) = create_signal(false);
    // pre_leaving 開始時の to_y（アニメーション補正用）
    let pre_leave_to_y: StoredValue<f64> = store_value(0.0);
    // トグル OFF による Leave が進行中かどうか（高速 ON/OFF 対策）
    let toggle_leave_pending: StoredValue<bool> = store_value(false);

    on_cleanup(move || {
        if let Some(h) = timeout_handle.get_value() {
            h.clear();
        }
    });

    // 現在時刻を監視してセクション終了 0.5s 前にフェードアウトを開始
    create_effect(move |_| {
        let t = ctx.current_time.get();

        // カードが表示中かつフェードアウト未開始の場合のみ処理
        if !card_open.get_untracked() || !card_showing.get_untracked() || pre_leaving.get_untracked() {
            return;
        }

        let idx = ctx.current_segment_idx.get_untracked();
        let segs = segments.get_value();
        let Some(i) = idx else { return; };
        let Some(seg) = segs.get(i) else { return; };

        let l = seg.label.to_lowercase();
        if l == "start" || l == "end" {
            return;
        }

        // セグメント終了 0.5s 前に達したらフェードアウト開始
        if t >= seg.end - 0.5 {
            set_pre_leaving.set(true);

            let gen = anim_gen.get_untracked() + 1;
            set_anim_gen.set(gen);

            // 進行中の enter タイムアウトをキャンセル
            if let Some(h) = timeout_handle.get_value() {
                h.clear();
            }

            let to_y = measure_delta_y(&format!("seg-item-{}", seg.index), "section-card-root");
            pre_leave_to_y.set_value(to_y);
            set_anim_style.set(format!(
                "--to-y:{to_y}px;animation:cardLeave 0.5s ease-in both"
            ));
        }
    });

    // セグメント変更時のアニメーション制御
    create_effect(move |_| {
        let new_idx = ctx.current_segment_idx.get();
        let old_idx = prev_idx.get_untracked();
        if new_idx == old_idx {
            return;
        }

        let segs = segments.get_value();

        let is_valid = |idx: Option<usize>| -> bool {
            idx.and_then(|i| segs.get(i)).map_or(false, |s| {
                let l = s.label.to_lowercase();
                l != "start" && l != "end"
            })
        };
        let seg_struct_index = |idx: Option<usize>| -> Option<usize> {
            idx.and_then(|i| segs.get(i)).map(|s| s.index as usize)
        };

        set_prev_idx.set(new_idx);

        let new_valid = is_valid(new_idx);
        // old_valid は card_showing に基づく（非表示カードは Leave しない）
        let old_valid = is_valid(old_idx) && card_showing.get_untracked();

        // pre_leaving・toggle_leave 状態をリセット（次セグメントの監視に備える）
        let was_pre_leaving = pre_leaving.get_untracked();
        set_pre_leaving.set(false);
        toggle_leave_pending.set_value(false);

        // 世代カウンタをインクリメント
        let gen = anim_gen.get_untracked() + 1;
        set_anim_gen.set(gen);

        // 進行中のタイムアウトをキャンセル
        if let Some(h) = timeout_handle.get_value() {
            h.clear();
        }

        let should_show = section_card_enabled.get_untracked() || ctx.is_playing.get_untracked();

        if old_valid && new_valid {
            if was_pre_leaving {
                // 0.5s 前からフェードアウト済み → セクション切替タイミングで即フェードイン
                // cardLeave 終了時、カード要素は translateY(to_y) の位置にある。
                // getBoundingClientRect() はこの transform を含むため、
                // 正しい from_y を得るには to_y 分を補正する必要がある。
                let stored_to_y = pre_leave_to_y.get_value();
                let raw_from_y = seg_struct_index(new_idx).map_or(0.0, |id| {
                    measure_delta_y(&format!("seg-item-{}", id), "section-card-root")
                });
                let from_y = raw_from_y + stored_to_y;
                set_displayed_idx.set(new_idx);
                set_card_showing.set(true);
                set_anim_style.set(format!(
                    "--from-y:{from_y}px;animation:cardEnter 0.35s ease-out both;pointer-events:auto"
                ));
            } else {
                // 通常遷移: Leave → Enter（2段階を維持）
                let to_y = seg_struct_index(old_idx).map_or(0.0, |id| {
                    measure_delta_y(&format!("seg-item-{}", id), "section-card-root")
                });
                let from_y = seg_struct_index(new_idx).map_or(0.0, |id| {
                    measure_delta_y(&format!("seg-item-{}", id), "section-card-root")
                });
                set_anim_style.set(format!(
                    "--to-y:{to_y}px;animation:cardLeave 0.3s ease-in both"
                ));
                timeout_handle.set_value(set_timeout_with_handle(move || {
                    if anim_gen.get_untracked() == gen {
                        set_displayed_idx.set(new_idx); // Leave 完了後に新セグメント情報に切替
                        set_card_showing.set(true);
                        set_anim_style.set(format!(
                            "--from-y:{from_y}px;animation:cardEnter 0.35s ease-out both;pointer-events:auto"
                        ));
                    }
                }, Duration::from_millis(310)).ok());
            }
        } else if old_valid && !new_valid {
            if was_pre_leaving {
                // フェードアウト済み → そのまま非表示
                set_card_open.set(false);
                set_card_showing.set(false);
                set_anim_style.set("opacity:0;pointer-events:none".to_string());
            } else {
                // Leave してから非表示
                let to_y = seg_struct_index(old_idx).map_or(0.0, |id| {
                    measure_delta_y(&format!("seg-item-{}", id), "section-card-root")
                });
                set_anim_style.set(format!(
                    "--to-y:{to_y}px;animation:cardLeave 0.3s ease-in both"
                ));
                timeout_handle.set_value(set_timeout_with_handle(move || {
                    if anim_gen.get_untracked() == gen {
                        set_card_open.set(false);
                        set_card_showing.set(false);
                        set_anim_style.set("opacity:0;pointer-events:none".to_string());
                    }
                }, Duration::from_millis(310)).ok());
            }
        } else if !old_valid && new_valid {
            // 初回表示
            set_displayed_idx.set(new_idx);
            set_card_open.set(true);
            if should_show {
                let from_y = seg_struct_index(new_idx).map_or(0.0, |id| {
                    measure_delta_y(&format!("seg-item-{}", id), "section-card-root")
                });
                set_card_showing.set(true);
                set_anim_style.set(format!(
                    "--from-y:{from_y}px;animation:cardEnter 0.35s ease-out both;pointer-events:auto"
                ));
            } else {
                // should_show=false なので非表示のまま（トグルON or 再生開始で後から表示）
                set_card_showing.set(false);
                set_anim_style.set("opacity:0;pointer-events:none".to_string());
            }
        } else {
            set_card_open.set(false);
            set_card_showing.set(false);
            set_anim_style.set("opacity:0;pointer-events:none".to_string());
        }
    });

    // トグル・再生状態変化によるカード表示/非表示
    // セグメント変更 effect の後に登録されるため、同バッチ内では後から実行される
    create_effect(move |_| {
        let enabled = section_card_enabled.get();
        let playing = ctx.is_playing.get();
        let should_show = enabled || playing;

        let currently_showing = card_showing.get_untracked();
        let currently_open = card_open.get_untracked();

        if should_show && !currently_showing && currently_open {
            // 有効セグメント内だが非表示 → タイムラインクリックと同じ直接 Enter
            let idx = prev_idx.get_untracked();
            let segs = segments.get_value();
            if let Some(i) = idx {
                if let Some(seg) = segs.get(i) {
                    if let Some(h) = timeout_handle.get_value() { h.clear(); }
                    toggle_leave_pending.set_value(false);
                    set_pre_leaving.set(false);
                    let gen = anim_gen.get_untracked() + 1;
                    set_anim_gen.set(gen);
                    let from_y = measure_delta_y(
                        &format!("seg-item-{}", seg.index),
                        "section-card-root",
                    );
                    set_displayed_idx.set(idx);
                    set_card_showing.set(true);
                    set_anim_style.set(format!(
                        "--from-y:{from_y}px;animation:cardEnter 0.35s ease-out both;pointer-events:auto"
                    ));
                }
            }
        } else if should_show && currently_showing && toggle_leave_pending.get_value() {
            // トグル OFF による Leave が進行中に再度 ON → Leave をキャンセルして Enter に戻す
            toggle_leave_pending.set_value(false);
            if let Some(h) = timeout_handle.get_value() { h.clear(); }
            let gen = anim_gen.get_untracked() + 1;
            set_anim_gen.set(gen);
            let idx = prev_idx.get_untracked();
            let segs = segments.get_value();
            if let Some(i) = idx {
                if let Some(seg) = segs.get(i) {
                    let from_y = measure_delta_y(
                        &format!("seg-item-{}", seg.index),
                        "section-card-root",
                    );
                    set_displayed_idx.set(idx);
                    set_anim_style.set(format!(
                        "--from-y:{from_y}px;animation:cardEnter 0.35s ease-out both;pointer-events:auto"
                    ));
                }
            }
        } else if !should_show && currently_showing {
            // 表示中だが非表示にすべき → cardLeave で退場
            if let Some(h) = timeout_handle.get_value() { h.clear(); }
            toggle_leave_pending.set_value(true);
            set_pre_leaving.set(false);
            let gen = anim_gen.get_untracked() + 1;
            set_anim_gen.set(gen);
            let idx = prev_idx.get_untracked();
            let segs = segments.get_value();
            let to_y = idx.and_then(|i| segs.get(i)).map_or(0.0, |seg| {
                measure_delta_y(&format!("seg-item-{}", seg.index), "section-card-root")
            });
            set_anim_style.set(format!(
                "--to-y:{to_y}px;animation:cardLeave 0.3s ease-in both"
            ));
            timeout_handle.set_value(set_timeout_with_handle(move || {
                if anim_gen.get_untracked() == gen {
                    toggle_leave_pending.set_value(false);
                    set_card_showing.set(false);
                    // card_open は変更しない（セグメント内にいることは保持）
                    set_anim_style.set("opacity:0;pointer-events:none".to_string());
                }
            }, Duration::from_millis(310)).ok());
        }
        // should_show && currently_showing && !toggle_leave_pending: 正常表示中、何もしない
        // !should_show && !currently_showing: 既に非表示、何もしない
    });

    // 再生終了時に非表示
    create_effect(move |_| {
        let playing = ctx.is_playing.get();
        let t = ctx.current_time.get();
        let d = ctx.duration.get();
        if !playing && d > 0.0 && t >= d - 0.5 {
            set_card_open.set(false);
            set_card_showing.set(false);
            set_anim_style.set("opacity:0;pointer-events:none".to_string());
        }
    });

    // Derive each field separately to avoid FnOnce in view!
    // displayed_idx を使うことで、Leave 中は旧セグメント情報を保持し、Enter 時に新セグメントへ切替
    let current_seg: Memo<Option<SegmentResult>> = create_memo(move |_| {
        displayed_idx
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
        <div
            id="section-card-root"
            class="absolute bottom-[20%] left-6 right-[280px] max-w-4xl mx-auto max-h-96 overflow-y-auto bg-gray-800 rounded-xl p-4 shadow-2xl border border-gray-600 z-20"
            style=move || anim_style.get()
        >
            <Show when=move || {
                let seg = current_seg.get();
                card_open.get()
                    && seg.is_some()
                    && seg.map_or(false, |s| s.label.to_lowercase() != "end" && s.label.to_lowercase() != "start")
            }>
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
                        on:click=move |_| {
                            set_card_open.set(false);
                            set_card_showing.set(false);
                            set_anim_style.set("opacity:0;pointer-events:none".to_string());
                        }
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
            </Show>
        </div>
    }
}
