use leptos::*;

/// 汎用エラー表示パネル。
/// API フェッチ失敗やオーディオデコードエラーを無限スピナーの代わりに表示する。
#[component]
pub fn ErrorPanel(
    title: &'static str,
    message: String,
    #[prop(optional)] on_retry: Option<Box<dyn Fn()>>,
) -> impl IntoView {
    view! {
        <div class="flex items-center justify-center h-full">
            <div class="bg-red-900/30 border border-red-700 rounded-xl p-6 max-w-md">
                <p class="text-red-400 font-medium mb-1">{title}</p>
                <p class="text-red-300 text-sm">{message}</p>
                {on_retry.map(|retry| view! {
                    <button
                        class="mt-4 px-4 py-1.5 bg-red-800 hover:bg-red-700 text-red-100 text-sm rounded-lg"
                        on:click=move |_| retry()
                    >"Retry"</button>
                })}
            </div>
        </div>
    }
}
