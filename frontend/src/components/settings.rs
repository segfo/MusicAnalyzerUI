use crate::{api, config::{self, BackendConfig, BackendMode}};
use leptos::*;
use wasm_bindgen_futures::spawn_local;

/// 設定パネル — 右上のギアアイコンから開く
#[component]
pub fn SettingsPanel(show: RwSignal<bool>) -> impl IntoView {
    let cfg_ctx = use_context::<RwSignal<BackendConfig>>()
        .expect("BackendConfig context not provided");

    // ローカルの編集用シグナル (Save 押下で確定)
    let mode = create_rw_signal(cfg_ctx.get_untracked().mode.clone());
    let http_url = create_rw_signal(cfg_ctx.get_untracked().http_base_url.clone());
    let base_dir_input = create_rw_signal(String::new());
    let base_dir_status = create_rw_signal::<Option<Result<(), String>>>(None);

    // Tauri モード時は現在の base_dir を取得して表示
    create_effect(move |_| {
        if config::is_tauri_env() {
            spawn_local(async move {
                if let Ok(Some(dir)) = api::get_base_dir().await {
                    base_dir_input.set(dir);
                }
            });
        }
    });

    // 設定を保存して適用
    let save = move |_| {
        let new_cfg = BackendConfig {
            mode: mode.get(),
            http_base_url: http_url.get(),
        };
        config::save_to_storage(&new_cfg);
        config::set_config(new_cfg.clone());
        cfg_ctx.set(new_cfg);
        show.set(false);
    };

    // デフォルトにリセット
    let reset = move |_| {
        let default_cfg = BackendConfig::default();
        mode.set(default_cfg.mode.clone());
        http_url.set(default_cfg.http_base_url.clone());
        config::save_to_storage(&default_cfg);
        config::set_config(default_cfg.clone());
        cfg_ctx.set(default_cfg);
    };

    // Tauri: base_dir を適用
    let apply_base_dir = move |_| {
        let path = base_dir_input.get();
        base_dir_status.set(None);
        spawn_local(async move {
            match api::set_base_dir(&path).await {
                Ok(()) => base_dir_status.set(Some(Ok(()))),
                Err(e) => base_dir_status.set(Some(Err(e))),
            }
        });
    };

    let detected = if config::is_tauri_env() { "Tauri" } else { "Web (HTTP)" };

    view! {
        // オーバーレイ
        <div
            class="fixed inset-0 bg-black/60 z-40"
            on:click=move |_| show.set(false)
        />

        // パネル本体
        <div class="fixed right-0 top-0 h-full w-80 bg-gray-900 border-l border-gray-700 z-50 flex flex-col shadow-2xl">

            // ヘッダー
            <div class="flex items-center justify-between px-5 py-4 border-b border-gray-700">
                <h2 class="text-gray-100 font-semibold text-base">"接続設定"</h2>
                <button
                    on:click=move |_| show.set(false)
                    class="text-gray-400 hover:text-gray-200 transition-colors text-xl leading-none"
                >"✕"</button>
            </div>

            // コンテンツ (スクロール可)
            <div class="flex-1 overflow-y-auto px-5 py-4 space-y-6">

                // 自動検出の表示
                <div class="text-xs text-gray-500">
                    "自動検出: " <span class="text-gray-300">{detected}</span>
                </div>

                // モード選択
                <div class="space-y-2">
                    <label class="text-xs font-medium text-gray-400 uppercase tracking-wide">
                        "バックエンドモード"
                    </label>
                    <div class="space-y-1.5">
                        <ModeOption
                            label="自動検出 (推奨)"
                            value=BackendMode::Auto
                            selected=mode
                        />
                        <ModeOption
                            label="Tauri (デスクトップ / Android)"
                            value=BackendMode::Tauri
                            selected=mode
                        />
                        <ModeOption
                            label="Web (HTTP API)"
                            value=BackendMode::Http
                            selected=mode
                        />
                    </div>
                </div>

                // HTTP モード: API URL 設定
                <div
                    class="space-y-2"
                    class:opacity-40={move || mode.get() == BackendMode::Tauri}
                >
                    <label class="text-xs font-medium text-gray-400 uppercase tracking-wide">
                        "API サーバー URL"
                    </label>
                    <input
                        type="text"
                        prop:value=move || http_url.get()
                        on:input=move |ev| {
                            use wasm_bindgen::JsCast;
                            if let Some(el) = ev.target()
                                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                            {
                                http_url.set(el.value());
                            }
                        }
                        placeholder="空欄 = プロキシ経由 (localhost:7777)"
                        class="w-full px-3 py-2 bg-gray-800 border border-gray-600 rounded text-sm text-gray-200 focus:outline-none focus:border-blue-500 placeholder-gray-600"
                    />
                    <p class="text-xs text-gray-500">
                        "空欄: Trunk プロキシ経由 (trunk serve 使用時)"<br />
                        "URL 指定: 直接接続 (例: http://192.168.1.10:7777)"
                    </p>
                </div>

                // Tauri モード: ベースディレクトリ設定
                <div
                    class="space-y-2"
                    class:opacity-40={move || mode.get() == BackendMode::Http}
                >
                    <label class="text-xs font-medium text-gray-400 uppercase tracking-wide">
                        "MusicAnalyzer ルートディレクトリ"
                    </label>
                    <div class="flex gap-2">
                        <input
                            type="text"
                            prop:value=move || base_dir_input.get()
                            on:input=move |ev| {
                                use wasm_bindgen::JsCast;
                                if let Some(el) = ev.target()
                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                {
                                    base_dir_input.set(el.value());
                                }
                            }
                            placeholder="/path/to/MusicAnalyzer"
                            class="flex-1 min-w-0 px-3 py-2 bg-gray-800 border border-gray-600 rounded text-sm text-gray-200 focus:outline-none focus:border-blue-500 placeholder-gray-600"
                        />
                        <button
                            on:click=apply_base_dir
                            class="px-3 py-2 bg-gray-700 hover:bg-gray-600 border border-gray-600 rounded text-xs text-gray-300 transition-colors whitespace-nowrap"
                        >"適用"</button>
                    </div>
                    {move || match base_dir_status.get() {
                        Some(Ok(())) => view! {
                            <p class="text-xs text-green-400">"✓ ディレクトリを設定しました"</p>
                        }.into_view(),
                        Some(Err(e)) => view! {
                            <p class="text-xs text-red-400">"✗ "{e}</p>
                        }.into_view(),
                        None => view! { <span /> }.into_view(),
                    }}
                    <p class="text-xs text-gray-500">
                        "output/, music/ を含む MusicAnalyzer のルートパス"
                    </p>
                </div>

            </div>

            // フッター: 保存 / リセット
            <div class="px-5 py-4 border-t border-gray-700 space-y-2">
                <button
                    on:click=save
                    class="w-full py-2 bg-blue-600 hover:bg-blue-500 rounded text-sm text-white font-medium transition-colors"
                >"保存して閉じる"</button>
                <button
                    on:click=reset
                    class="w-full py-1.5 bg-transparent hover:bg-gray-800 rounded text-xs text-gray-500 hover:text-gray-300 transition-colors"
                >"デフォルトにリセット"</button>
            </div>
        </div>
    }
}

// ── ラジオボタン的な選択肢コンポーネント ───────────────────────────────────────

#[component]
fn ModeOption(
    label: &'static str,
    value: BackendMode,
    selected: RwSignal<BackendMode>,
) -> impl IntoView {
    // StoredValue は Copy なので複数の move クロージャで安全にキャプチャできる
    let val = store_value(value);
    view! {
        <button
            on:click=move |_| selected.set(val.get_value())
            class="w-full flex items-center gap-3 px-3 py-2 rounded text-sm transition-colors text-left"
            class:bg-blue-600={move || selected.get() == val.get_value()}
            class:text-white={move || selected.get() == val.get_value()}
            class:bg-gray-800={move || selected.get() != val.get_value()}
            class:text-gray-300={move || selected.get() != val.get_value()}
            class:hover:bg-gray-700={move || selected.get() != val.get_value()}
        >
            <span
                class="w-3.5 h-3.5 rounded-full border-2 flex-shrink-0 flex items-center justify-center"
                class:border-white={move || selected.get() == val.get_value()}
                class:border-gray-500={move || selected.get() != val.get_value()}
            >
                {move || if selected.get() == val.get_value() {
                    view! { <span class="w-1.5 h-1.5 rounded-full bg-white" /> }.into_view()
                } else {
                    view! { <span /> }.into_view()
                }}
            </span>
            {label}
        </button>
    }
}
