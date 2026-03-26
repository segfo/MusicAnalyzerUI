use crate::{api, components::track_list::TrackCard, state::GlobalPlayback, types::{SortDirection, SortField}};
use leptos::*;

#[component]
pub fn Home() -> impl IntoView {
    // リストへ戻ったら再生を停止する
    if let Some(g) = use_context::<GlobalPlayback>() {
        g.stop_playback();
    }
    let tracks = create_resource(|| (), |_| api::fetch_tracks());
    
    let (sort_direction, set_sort_direction) = create_signal(SortDirection::Ascending);
    let (sort_field, set_sort_field) = create_signal(Some(SortField::Filename));

    fn sort_tracks(tracks: Vec<crate::types::TrackSummary>, direction: SortDirection, field: Option<SortField>) -> Vec<crate::types::TrackSummary> {
        let mut sorted = tracks;
        
        if let Some(field) = field {
            match field {
                SortField::Bpm => {
                    sorted.sort_by(|a, b| {
                        let cmp = a.bpm.partial_cmp(&b.bpm).unwrap_or(std::cmp::Ordering::Equal);
                        match direction {
                            SortDirection::Ascending => cmp,
                            SortDirection::Descending => cmp.reverse(),
                        }
                    });
                }
                SortField::Filename => {
                    sorted.sort_by(|a, b| {
                        let cmp = a.filename.cmp(&b.filename);
                        match direction {
                            SortDirection::Ascending => cmp,
                            SortDirection::Descending => cmp.reverse(),
                        }
                    });
                }
            }
        }
        
        sorted
    }

    view! {
        <div class="min-h-screen bg-gray-950 p-8">
            <div class="max-w-6xl mx-auto">
                <h1 class="text-3xl font-bold text-gray-100 mb-2">"MusicAnalyzer"</h1>
                <p class="text-gray-400 text-sm mb-8">"解析済みトラック一覧"</p>
                
                // ソートコントロール
                <div class="mb-4 flex items-center gap-2">
                    <select
                        class="px-3 py-1.5 bg-gray-800 border border-gray-700 rounded text-xs text-gray-300 focus:outline-none focus:border-gray-500"
                        on:change=move |ev| {
                            use wasm_bindgen::JsCast;
                            let val = ev.target()
                                .and_then(|t| t.dyn_into::<web_sys::HtmlSelectElement>().ok())
                                .map(|el| el.value())
                                .unwrap_or_default();
                            set_sort_field.set(match val.as_str() {
                                "bpm" => Some(SortField::Bpm),
                                _     => Some(SortField::Filename),
                            });
                        }
                    >
                        <option value="filename">"楽曲名"</option>
                        <option value="bpm">"BPM"</option>
                    </select>
                    <button
                        on:click=move |_| {
                            set_sort_direction.set(match sort_direction.get() {
                                SortDirection::Ascending  => SortDirection::Descending,
                                SortDirection::Descending => SortDirection::Ascending,
                            });
                        }
                        class="px-3 py-1.5 bg-gray-800 hover:bg-gray-700 border border-gray-700 rounded text-xs text-gray-300 transition-colors"
                    >
                        {move || if sort_direction.get() == SortDirection::Ascending { "昇順" } else { "降順" }}
                    </button>
                </div>
                
                <Suspense fallback=|| view! {
                    <div class="flex items-center justify-center py-20">
                        <p class="text-gray-400">"Loading tracks..."</p>
                    </div>
                }>
                    {move || tracks.get().map(|result| match result {
                        Ok(list) => {
                            let sorted_list = sort_tracks(
                                list.clone(), 
                                sort_direction.get(), 
                                sort_field.get()
                            );
                            
                            if sorted_list.is_empty() {
                                view! {
                                    <p class="text-gray-500 text-center py-20">
                                        "output/ に JSON ファイルが見つかりません"
                                    </p>
                                }.into_view()
                            } else {
                                view! {
                                    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
                                        <For
                                            each=move || sorted_list.clone()
                                            key=|t| t.stem.clone()
                                            children=|track| view! {
                                                <TrackCard track=track />
                                            }
                                        />
                                    </div>
                                }.into_view()
                            }
                        },
                        Err(e) => view! {
                            <div class="bg-red-900/30 border border-red-700 rounded-xl p-4">
                                <p class="text-red-400">"Error: "{e}</p>
                            </div>
                        }.into_view(),
                    })}
                </Suspense>
            </div>
        </div>
    }
}
