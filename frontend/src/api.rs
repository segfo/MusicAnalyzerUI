/// API layer — ALL backend calls go through this module.
/// config::effective_mode() に従い Tauri IPC または HTTP REST に自動ディスパッチする。
use crate::config::{self, BackendMode};
use crate::types::{StemAvailability, TrackDataset, TrackSummary};
use js_sys::JsString;
use wasm_bindgen::prelude::*;

// ── Tauri 2.x 内部 API バインド ──────────────────────────────────────────────
// withGlobalTauri 不要。window.__TAURI_INTERNALS__ は Tauri が常に注入する。
// @tauri-apps/api/core もこれを内部で使用している。

#[wasm_bindgen]
extern "C" {
    /// Tauri IPC invoke (window.__TAURI_INTERNALS__.invoke)
    #[wasm_bindgen(js_namespace = ["window", "__TAURI_INTERNALS__"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    /// ローカルパスを asset:// URL に変換する (window.__TAURI_INTERNALS__.convertFileSrc)
    #[wasm_bindgen(js_namespace = ["window", "__TAURI_INTERNALS__"], js_name = "convertFileSrc")]
    fn convert_file_src(file_path: &str) -> JsString;
}

// ── Tauri ヘルパー ────────────────────────────────────────────────────────────

async fn tauri_invoke<T: for<'de> serde::Deserialize<'de>>(
    cmd: &str,
    args: JsValue,
) -> Result<T, String> {
    let result = invoke(cmd, args)
        .await
        .map_err(|e| format!("Tauri invoke [{}]: {:?}", cmd, e))?;
    serde_wasm_bindgen::from_value(result).map_err(|e| e.to_string())
}

// ── HTTP ヘルパー ─────────────────────────────────────────────────────────────

fn http_url(cfg: &config::BackendConfig, path: &str) -> String {
    if cfg.http_base_url.is_empty() {
        // 相対 URL → Trunk プロキシ経由 (同一オリジン、CORS 不要)
        format!("/api{}", path)
    } else {
        // 絶対 URL → 直接クロスオリジンリクエスト
        format!("{}/api{}", cfg.http_base_url.trim_end_matches('/'), path)
    }
}

fn encode(s: &str) -> String {
    js_sys::encode_uri_component(s)
        .as_string()
        .unwrap_or_default()
}

async fn http_get_json<T: for<'de> serde::Deserialize<'de>>(url: &str) -> Result<T, String> {
    let resp = gloo_net::http::Request::get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<T>().await.map_err(|e| e.to_string())
}

async fn http_post_json<T: for<'de> serde::Deserialize<'de>>(
    url: &str,
    body: &impl serde::Serialize,
) -> Result<T, String> {
    let resp = gloo_net::http::Request::post(url)
        .json(body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<T>().await.map_err(|e| e.to_string())
}

async fn fetch_binary_url(url: &str) -> Result<js_sys::ArrayBuffer, String> {
    let bytes = gloo_net::http::Request::get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .binary()
        .await
        .map_err(|e| e.to_string())?;
    let arr = js_sys::Uint8Array::from(bytes.as_slice());
    Ok(arr.buffer())
}

// ── Public API ───────────────────────────────────────────────────────────────

pub async fn fetch_tracks() -> Result<Vec<TrackSummary>, String> {
    let cfg = config::get_config();
    match config::effective_mode(&cfg) {
        BackendMode::Tauri => tauri_invoke("list_tracks", JsValue::NULL).await,
        BackendMode::Http => http_get_json(&http_url(&cfg, "/tracks")).await,
        BackendMode::Auto => unreachable!(),
    }
}

pub async fn fetch_track(stem: &str) -> Result<TrackDataset, String> {
    let cfg = config::get_config();
    match config::effective_mode(&cfg) {
        BackendMode::Tauri => {
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "stem": stem }))
                .map_err(|e| e.to_string())?;
            tauri_invoke("get_track", args).await
        }
        BackendMode::Http => {
            http_get_json(&http_url(&cfg, &format!("/tracks/{}", encode(stem)))).await
        }
        BackendMode::Auto => unreachable!(),
    }
}

/// 音声ファイルの URL を返す。
/// Tauri モード: get_audio_path → convertFileSrc → asset:// URL
/// HTTP  モード: /api/audio/{stem} の HTTP URL
pub async fn audio_asset_url(stem: &str) -> Result<Option<String>, String> {
    let cfg = config::get_config();
    match config::effective_mode(&cfg) {
        BackendMode::Tauri => {
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "stem": stem }))
                .map_err(|e| e.to_string())?;
            let path: Option<String> = tauri_invoke("get_audio_path", args).await?;
            Ok(path.map(|p| String::from(convert_file_src(&p))))
        }
        BackendMode::Http => Ok(Some(http_url(&cfg, &format!("/audio/{}", encode(stem))))),
        BackendMode::Auto => unreachable!(),
    }
}

pub async fn fetch_stem_availability(stem: &str) -> Result<StemAvailability, String> {
    let cfg = config::get_config();
    match config::effective_mode(&cfg) {
        BackendMode::Tauri => {
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "stem": stem }))
                .map_err(|e| e.to_string())?;
            tauri_invoke("get_stem_availability", args).await
        }
        BackendMode::Http => {
            http_get_json(&http_url(&cfg, &format!("/stems/{}", encode(stem)))).await
        }
        BackendMode::Auto => unreachable!(),
    }
}

/// ステム音声の ArrayBuffer を返す (Web Audio API decode 用)
pub async fn fetch_stem_array_buffer(
    stem: &str,
    track: &str,
) -> Result<js_sys::ArrayBuffer, String> {
    let cfg = config::get_config();
    match config::effective_mode(&cfg) {
        BackendMode::Tauri => {
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                "stem": stem,
                "trackName": track,
            }))
            .map_err(|e| e.to_string())?;
            let path: Option<String> = tauri_invoke("get_stem_path", args).await?;
            let path = path.ok_or_else(|| format!("Stem not found: {}/{}", stem, track))?;
            let url = String::from(convert_file_src(&path));
            fetch_binary_url(&url).await
        }
        BackendMode::Http => {
            let url = http_url(&cfg, &format!("/stems/{}/{}", encode(stem), track));
            fetch_binary_url(&url).await
        }
        BackendMode::Auto => unreachable!(),
    }
}

/// 音声ファイル全体を ArrayBuffer として返す (Web Audio API decode 用)
pub async fn fetch_audio_array_buffer(stem: &str) -> Result<js_sys::ArrayBuffer, String> {
    let url = audio_asset_url(stem)
        .await?
        .ok_or_else(|| format!("Audio file not found for: {}", stem))?;
    fetch_binary_url(&url).await
}

/// ベースディレクトリを取得する (Tauri モードのみ)
pub async fn get_base_dir() -> Result<Option<String>, String> {
    tauri_invoke("get_base_dir", JsValue::NULL).await
}

/// ベースディレクトリを設定して永続化する (Tauri モードのみ)
pub async fn set_base_dir(path: &str) -> Result<(), String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "path": path }))
        .map_err(|e| e.to_string())?;
    tauri_invoke("set_base_dir", args).await
}

/// セグメントのラベルを更新してオーバーライドJSONに保存する
pub async fn update_segment_label(stem: &str, segment_index: u32, new_label: &str) -> Result<(), String> {
    let cfg = config::get_config();
    match config::effective_mode(&cfg) {
        BackendMode::Tauri => {
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({
                "stem": stem,
                "segmentIndex": segment_index,
                "newLabel": new_label,
            }))
            .map_err(|e| e.to_string())?;
            tauri_invoke("update_segment_label", args).await
        }
        BackendMode::Http => {
            #[derive(serde::Serialize)]
            struct Body<'a> { stem: &'a str, segment_index: u32, new_label: &'a str }
            #[derive(serde::Deserialize)]
            struct Resp { ok: bool }
            let resp: Resp = http_post_json(
                &http_url(&cfg, "/segments/update"),
                &Body { stem, segment_index, new_label },
            ).await?;
            if resp.ok { Ok(()) } else { Err("update_segment_label failed".into()) }
        }
        BackendMode::Auto => unreachable!(),
    }
}

/// 最後のセグメントラベル変更を元に戻す。変更があれば true、履歴が空なら false を返す
pub async fn undo_segment_label(stem: &str) -> Result<bool, String> {
    let cfg = config::get_config();
    match config::effective_mode(&cfg) {
        BackendMode::Tauri => {
            let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "stem": stem }))
                .map_err(|e| e.to_string())?;
            tauri_invoke("undo_segment_label", args).await
        }
        BackendMode::Http => {
            #[derive(serde::Serialize)]
            struct Body<'a> { stem: &'a str }
            #[derive(serde::Deserialize)]
            struct Resp { undone: bool }
            let resp: Resp = http_post_json(
                &http_url(&cfg, "/segments/undo"),
                &Body { stem },
            ).await?;
            Ok(resp.undone)
        }
        BackendMode::Auto => unreachable!(),
    }
}
