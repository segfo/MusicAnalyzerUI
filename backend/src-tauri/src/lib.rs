use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

// ── Schema types (Python schema.py の移植) ───────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SubCaption {
    pub chunk_index: i64,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SegmentResult {
    pub index: i64,
    pub label: String,
    pub start: f64,
    pub end: f64,
    pub duration: f64,
    pub beat_count: i64,
    pub bpm: Option<i64>,
    pub caption: Option<String>,
    pub caption_note: Option<String>,
    #[serde(default)]
    pub sub_captions: Vec<SubCaption>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct OverallDescription {
    pub prompt_file: String,
    pub prompt_text: String,
    pub response: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProcessingLog {
    pub allin1_duration_sec: Option<f64>,
    pub lpmc_duration_sec: Option<f64>,
    pub mullama_duration_sec: Option<f64>,
    pub total_duration_sec: Option<f64>,
    #[serde(default)]
    pub lpmc_chunks_processed: i64,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ChordResult {
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub label: Option<String>,
    pub label_raw: Option<String>,
    pub confidence: Option<f64>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct TrackDataset {
    pub schema_version: String,
    pub track_path: String,
    pub track_filename: String,
    pub analysis_timestamp: String,
    pub bpm: Option<f64>,
    #[serde(default)]
    pub bpm_candidates: Vec<f64>,
    pub bpm_selection_reason: Option<String>,
    #[serde(default)]
    pub beats: Vec<f64>,
    #[serde(default)]
    pub original_beats: Vec<f64>,
    #[serde(default)]
    pub downbeats: Vec<f64>,
    #[serde(default)]
    pub original_downbeats: Vec<f64>,
    #[serde(default)]
    pub beat_positions: Vec<i64>,
    #[serde(default)]
    pub original_beat_positions: Vec<i64>,
    #[serde(default)]
    pub overall_descriptions: Vec<OverallDescription>,
    #[serde(default)]
    pub segments: Vec<SegmentResult>,
    #[serde(default)]
    pub chords: Vec<ChordResult>,
    pub processing_log: ProcessingLog,
}

#[derive(serde::Serialize)]
pub struct TrackSummary {
    pub stem: String,
    pub filename: String,
    pub bpm: Option<f64>,
    pub segment_count: usize,
    pub has_audio: bool,
}

#[derive(serde::Serialize)]
pub struct StemAvailability {
    pub vocals: bool,
    pub drums: bool,
    pub bass: bool,
    pub other: bool,
}

// ── App state ────────────────────────────────────────────────────────────────

pub struct AppState {
    pub base_dir: Mutex<Option<PathBuf>>,
}

// ── Config persistence ───────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct AppConfig {
    base_dir: Option<String>,
}

fn config_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("config.json")
}

fn load_config(app: &AppHandle) -> AppConfig {
    let path = config_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

fn save_config(app: &AppHandle, cfg: &AppConfig) {
    let path = config_path(app);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(data) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(path, data);
    }
}

// ── Path helpers (Python main.py のロジックを移植) ──────────────────────────

const AUDIO_EXTS: &[&str] = &[".mp3", ".flac", ".wav", ".ogg", ".m4a"];
const STEM_TRACKS: &[&str] = &["vocals", "drums", "bass", "other"];

/// output_ja/{stem}_ja.json を優先し、なければ output/{stem}.json を返す
fn resolve_json(base_dir: &PathBuf, stem: &str) -> Option<PathBuf> {
    let ja = base_dir
        .join("output_ja")
        .join(format!("{}_ja.json", stem));
    if ja.exists() {
        return Some(ja);
    }
    let default = base_dir.join("output").join(format!("{}.json", stem));
    if default.exists() {
        return Some(default);
    }
    None
}

fn find_audio_in_music_dir(base_dir: &PathBuf, stem: &str) -> Option<PathBuf> {
    let music_dir = base_dir.join("music");
    for ext in AUDIO_EXTS {
        let candidate = music_dir.join(format!("{}{}", stem, ext));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// music/stems/{stem}/{track_name}.{ext} を検索する
fn find_stem_file(base_dir: &PathBuf, stem: &str, track_name: &str) -> Option<PathBuf> {
    let stem_dir = base_dir.join("music").join("stems").join(stem);
    for ext in AUDIO_EXTS {
        let candidate = stem_dir.join(format!("{}{}", track_name, ext));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

// ── Tauri commands ───────────────────────────────────────────────────────────

/// 現在設定されているベースディレクトリを返す
#[tauri::command]
fn get_base_dir(state: State<AppState>) -> Result<Option<String>, String> {
    let dir = state.base_dir.lock().map_err(|e| e.to_string())?;
    Ok(dir.as_ref().map(|p| p.to_string_lossy().into_owned()))
}

/// ベースディレクトリを設定して永続化する
#[tauri::command]
fn set_base_dir(path: String, app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let pb = PathBuf::from(&path);
    if !pb.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    {
        let mut dir = state.base_dir.lock().map_err(|e| e.to_string())?;
        *dir = Some(pb);
    }
    save_config(&app, &AppConfig { base_dir: Some(path) });
    Ok(())
}

/// output/ ディレクトリのトラック一覧を返す
#[tauri::command]
fn list_tracks(state: State<AppState>) -> Result<Vec<TrackSummary>, String> {
    let guard = state.base_dir.lock().map_err(|e| e.to_string())?;
    let base_dir = guard
        .as_ref()
        .ok_or("Base directory not configured. Call set_base_dir first.")?;

    let output_dir = base_dir.join("output");
    if !output_dir.exists() {
        return Ok(vec![]);
    }

    let mut stems: Vec<String> = std::fs::read_dir(&output_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".json").map(|s| s.to_string())
        })
        .collect();
    stems.sort();

    let mut results = Vec::new();
    for stem in stems {
        let json_file = match resolve_json(base_dir, &stem) {
            Some(f) => f,
            None => continue,
        };
        let data = match std::fs::read_to_string(&json_file) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let track: TrackDataset = match serde_json::from_str(&data) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let has_audio = find_audio_in_music_dir(base_dir, &stem).is_some()
            || PathBuf::from(&track.track_path).exists();
        results.push(TrackSummary {
            stem,
            filename: track.track_filename,
            bpm: track.bpm,
            segment_count: track.segments.len(),
            has_audio,
        });
    }
    Ok(results)
}

/// 指定 stem の TrackDataset JSON を返す
#[tauri::command]
fn get_track(stem: String, state: State<AppState>) -> Result<TrackDataset, String> {
    let guard = state.base_dir.lock().map_err(|e| e.to_string())?;
    let base_dir = guard
        .as_ref()
        .ok_or("Base directory not configured.")?;
    let json_file = resolve_json(base_dir, &stem)
        .ok_or_else(|| format!("Track not found: {}", stem))?;
    let data = std::fs::read_to_string(&json_file).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

/// ステムファイル (vocals/drums/bass/other) の存在チェック
#[tauri::command]
fn get_stem_availability(stem: String, state: State<AppState>) -> Result<StemAvailability, String> {
    let guard = state.base_dir.lock().map_err(|e| e.to_string())?;
    let base_dir = guard
        .as_ref()
        .ok_or("Base directory not configured.")?;
    Ok(StemAvailability {
        vocals: find_stem_file(base_dir, &stem, "vocals").is_some(),
        drums: find_stem_file(base_dir, &stem, "drums").is_some(),
        bass: find_stem_file(base_dir, &stem, "bass").is_some(),
        other: find_stem_file(base_dir, &stem, "other").is_some(),
    })
}

/// 音声ファイルの絶対パスを返す (asset:// URL変換用)
#[tauri::command]
fn get_audio_path(stem: String, state: State<AppState>) -> Result<Option<String>, String> {
    let guard = state.base_dir.lock().map_err(|e| e.to_string())?;
    let base_dir = guard
        .as_ref()
        .ok_or("Base directory not configured.")?;

    if let Some(path) = find_audio_in_music_dir(base_dir, &stem) {
        return Ok(Some(path.to_string_lossy().into_owned()));
    }

    // フォールバック: JSON の track_path を確認
    if let Some(json_file) = resolve_json(base_dir, &stem) {
        if let Ok(data) = std::fs::read_to_string(&json_file) {
            if let Ok(track) = serde_json::from_str::<TrackDataset>(&data) {
                let fallback = PathBuf::from(&track.track_path);
                if fallback.exists() {
                    return Ok(Some(track.track_path));
                }
            }
        }
    }

    Ok(None)
}

/// ステム音声ファイルの絶対パスを返す (asset:// URL変換用)
#[tauri::command]
fn get_stem_path(
    stem: String,
    track_name: String,
    state: State<AppState>,
) -> Result<Option<String>, String> {
    if !STEM_TRACKS.contains(&track_name.as_str()) {
        return Err(format!(
            "Invalid track: {}. Must be one of {:?}",
            track_name, STEM_TRACKS
        ));
    }
    let guard = state.base_dir.lock().map_err(|e| e.to_string())?;
    let base_dir = guard
        .as_ref()
        .ok_or("Base directory not configured.")?;
    Ok(find_stem_file(base_dir, &stem, &track_name)
        .map(|p| p.to_string_lossy().into_owned()))
}

// ── App entry point ──────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // 保存済み設定を読み込み、base_dir を初期化
            let cfg = load_config(app.handle());
            let base_dir = cfg
                .base_dir
                .map(PathBuf::from)
                .filter(|p| p.exists());
            app.manage(AppState {
                base_dir: Mutex::new(base_dir),
            });

            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_base_dir,
            set_base_dir,
            list_tracks,
            get_track,
            get_stem_availability,
            get_audio_path,
            get_stem_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
