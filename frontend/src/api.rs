/// API layer — ALL network calls go through this module.
/// Tauri migration: replace gloo_net calls with tauri_sys::invoke here only.
use crate::types::{StemAvailability, TrackDataset, TrackSummary};
use gloo_net::http::Request;

const API_BASE: &str = "/api";

fn encode(s: &str) -> String {
    js_sys::encode_uri_component(s).as_string().unwrap_or_default()
}

pub async fn fetch_tracks() -> Result<Vec<TrackSummary>, String> {
    let resp = Request::get(&format!("{API_BASE}/tracks"))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    resp.json::<Vec<TrackSummary>>().await.map_err(|e| e.to_string())
}

pub async fn fetch_track(stem: &str) -> Result<TrackDataset, String> {
    let resp = Request::get(&format!("{API_BASE}/tracks/{}", encode(stem)))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("HTTP {} for: {stem}", resp.status()));
    }

    resp.json::<TrackDataset>().await.map_err(|e| e.to_string())
}

/// Returns the raw API URL for the audio file.
pub fn audio_url(stem: &str) -> String {
    format!("{API_BASE}/audio/{}", encode(stem))
}

/// Checks which stem tracks are available for the given stem.
pub async fn fetch_stem_availability(stem: &str) -> Result<StemAvailability, String> {
    let resp = Request::get(&format!("{API_BASE}/stems/{}", encode(stem)))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<StemAvailability>().await.map_err(|e| e.to_string())
}

/// Downloads a stem audio file (vocals/drums/bass/other) and returns an ArrayBuffer.
pub async fn fetch_stem_array_buffer(stem: &str, track: &str) -> Result<js_sys::ArrayBuffer, String> {
    let bytes = Request::get(&format!("{API_BASE}/stems/{}/{}", encode(stem), track))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .binary()
        .await
        .map_err(|e| e.to_string())?;

    let uint8_arr = js_sys::Uint8Array::from(bytes.as_slice());
    Ok(uint8_arr.buffer())
}

/// Downloads the full audio file and returns an ArrayBuffer for Web Audio API decoding.
/// Web Audio API's decodeAudioData + AudioBufferSourceNode gives sample-accurate seeking,
/// bypassing the browser's VBR byte-offset estimation that causes multi-second drift.
/// Tauri migration: use tauri_sys to read the local file directly.
pub async fn fetch_audio_array_buffer(stem: &str) -> Result<js_sys::ArrayBuffer, String> {
    let bytes = Request::get(&audio_url(stem))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .binary()
        .await
        .map_err(|e| e.to_string())?;

    let uint8_arr = js_sys::Uint8Array::from(bytes.as_slice());
    Ok(uint8_arr.buffer())
}
