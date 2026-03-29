/// バックエンド設定 — Tauri IPC モードと HTTP REST モードを切り替える。
/// localStorage に永続化し、アプリ起動時に読み込む。
use std::cell::RefCell;

// ── 型定義 ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum BackendMode {
    /// 起動時に自動検出 (window.__TAURI__ が存在すれば Tauri、なければ Http)
    Auto,
    /// Tauri IPC コマンド経由
    Tauri,
    /// HTTP REST API (Python FastAPI 等) 経由
    Http,
}

impl Default for BackendMode {
    fn default() -> Self {
        BackendMode::Auto
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BackendConfig {
    #[serde(default)]
    pub mode: BackendMode,
    /// Http モード時の API サーバーベース URL (例: "http://localhost:7777")
    #[serde(default = "default_base_url")]
    pub http_base_url: String,
}

fn default_base_url() -> String {
    // 空文字 = 相対パス (/api/...) → Trunk プロキシ経由で localhost:7777 へ転送
    // 外部サーバーに接続する場合は "http://hostname:7777" のように設定する
    String::new()
}

impl Default for BackendConfig {
    fn default() -> Self {
        BackendConfig {
            mode: BackendMode::Auto,
            http_base_url: default_base_url(),
        }
    }
}

// ── グローバル設定 (WASM はシングルスレッド) ─────────────────────────────────

thread_local! {
    static CURRENT_CONFIG: RefCell<BackendConfig> = RefCell::new(BackendConfig::default());
}

pub fn get_config() -> BackendConfig {
    CURRENT_CONFIG.with(|c| c.borrow().clone())
}

pub fn set_config(config: BackendConfig) {
    CURRENT_CONFIG.with(|c| *c.borrow_mut() = config);
}

// ── Auto モードの解決 ─────────────────────────────────────────────────────────

/// Auto モードを実際の Tauri/Http に解決して返す
pub fn effective_mode(cfg: &BackendConfig) -> BackendMode {
    match &cfg.mode {
        BackendMode::Auto => {
            if is_tauri_env() {
                BackendMode::Tauri
            } else {
                BackendMode::Http
            }
        }
        m => m.clone(),
    }
}

/// Tauri 環境かどうかを確認する。
/// withGlobalTauri 不要: __TAURI_INTERNALS__ は Tauri が常に注入するため、
/// これの存在を確認するのが確実。
pub fn is_tauri_env() -> bool {
    web_sys::window()
        .and_then(|w| {
            js_sys::Reflect::has(
                &w.into(),
                &wasm_bindgen::JsValue::from_str("__TAURI_INTERNALS__"),
            )
            .ok()
        })
        .unwrap_or(false)
}

// ── localStorage 永続化 ────────────────────────────────────────────────────────

const STORAGE_KEY: &str = "music_analyzer_backend_config";

pub fn load_from_storage() -> BackendConfig {
    let stored = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(STORAGE_KEY).ok().flatten());

    if let Some(json) = stored {
        serde_json::from_str(&json).unwrap_or_default()
    } else {
        BackendConfig::default()
    }
}

pub fn save_to_storage(config: &BackendConfig) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        if let Ok(json) = serde_json::to_string(config) {
            let _ = storage.set_item(STORAGE_KEY, &json);
        }
    }
}
