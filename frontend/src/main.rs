mod api;
mod audio;
mod audio_setup;
mod components;
mod config;
mod pages;
mod state;
mod types;

use crate::components::settings::SettingsPanel;
use leptos::*;
use leptos_router::*;
use state::{GlobalPlayback, VisualizationPageState};

#[component]
fn App() -> impl IntoView {
    // localStorage からバックエンド設定を読み込み、グローバルに適用する
    let initial_cfg = config::load_from_storage();
    config::set_config(initial_cfg.clone());

    let backend_config = create_rw_signal(initial_cfg);
    provide_context(backend_config);

    provide_context(GlobalPlayback::new());
    provide_context(VisualizationPageState::new());

    let show_settings = create_rw_signal(false);

    view! {
        <Router>
            <div class="relative">
                <Routes>
                    <Route path="/" view=pages::home::Home />
                    <Route path="/analysis/:stem" view=pages::analysis::Analysis />
                    <Route path="/visualization/:stem" view=pages::visualization::Visualization />
                </Routes>

                // 設定ボタン・パネル (Home画面 "/" のみ表示)
                <Show when=move || use_location().pathname.get() == "/" fallback=|| ()>
                    <button
                        on:click=move |_| show_settings.set(true)
                        title="接続設定"
                        class="fixed top-4 right-4 z-30 w-9 h-9 flex items-center justify-center rounded-full bg-gray-800 hover:bg-gray-700 border border-gray-700 text-gray-400 hover:text-gray-200 transition-colors shadow-lg"
                    >
                        // ギアアイコン (SVG)
                        <svg
                            xmlns="http://www.w3.org/2000/svg"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            class="w-4 h-4"
                        >
                            <circle cx="12" cy="12" r="3" />
                            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
                        </svg>
                    </button>

                    <Show when=move || show_settings.get() fallback=|| ()>
                        <SettingsPanel show=show_settings />
                    </Show>
                </Show>
            </div>
        </Router>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount_to_body(App);
}
