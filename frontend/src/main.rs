mod api;
mod components;
mod pages;
mod state;
mod types;

use leptos::*;
use leptos_router::*;
use state::{GlobalPlayback, VisualizationPageState};

#[component]
fn App() -> impl IntoView {
    provide_context(GlobalPlayback::new());
    provide_context(VisualizationPageState::new());
    view! {
        <Router>
            <Routes>
                <Route path="/" view=pages::home::Home />
                <Route path="/analysis/:stem" view=pages::analysis::Analysis />
                <Route path="/visualization/:stem" view=pages::visualization::Visualization />
            </Routes>
        </Router>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount_to_body(App);
}
