use crate::types::TrackSummary;
use leptos::*;
use leptos_router::*;

#[component]
pub fn TrackCard(track: TrackSummary) -> impl IntoView {
    let href = format!("/analysis/{}", track.stem);
    let bpm_display = track
        .bpm
        .map(|b| format!("{:.0} BPM", b))
        .unwrap_or_else(|| "BPM unknown".to_string());
    let audio_indicator = if track.has_audio {
        view! { <span class="text-green-400 text-xs">"Audio OK"</span> }.into_view()
    } else {
        view! { <span class="text-gray-500 text-xs">"No audio"</span> }.into_view()
    };

    view! {
        <A href=href>
            <div class="bg-gray-800 hover:bg-gray-700 rounded-xl p-4 cursor-pointer transition-colors border border-gray-700 hover:border-gray-500">
                <p class="font-medium text-sm text-gray-100 truncate mb-2" title=track.filename.clone()>
                    {track.filename}
                </p>
                <div class="flex items-center justify-between">
                    <span class="text-blue-400 text-xs font-mono">{bpm_display}</span>
                    <span class="text-gray-400 text-xs">
                        {track.segment_count}" sections"
                    </span>
                </div>
                <div class="mt-2">{audio_indicator}</div>
            </div>
        </A>
    }
}
