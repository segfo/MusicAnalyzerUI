use std::collections::HashMap;
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

// ── Segment overrides (編集履歴・ユーザー上書き) ─────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct HistoryEntry {
    timestamp: String,
    segment_index: i64,
    old_label: String,
    new_label: String,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct SegmentOverrides {
    stem: String,
    updated_at: String,
    #[serde(default)]
    history: Vec<HistoryEntry>,
    #[serde(default)]
    current_overrides: HashMap<String, String>,
}

fn resolve_overrides_path(base_dir: &PathBuf, stem: &str) -> PathBuf {
    base_dir
        .join("output")
        .join("overrides")
        .join(format!("{}_overrides.json", stem))
}

fn load_overrides(base_dir: &PathBuf, stem: &str) -> SegmentOverrides {
    let path = resolve_overrides_path(base_dir, stem);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_else(|| SegmentOverrides {
            stem: stem.to_string(),
            updated_at: String::new(),
            history: Vec::new(),
            current_overrides: HashMap::new(),
        })
}

fn save_overrides(base_dir: &PathBuf, stem: &str, overrides: &SegmentOverrides) -> Result<(), String> {
    let path = resolve_overrides_path(base_dir, stem);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(overrides).map_err(|e| e.to_string())?;
    std::fs::write(path, data).map_err(|e| e.to_string())
}

fn now_timestamp() -> String {
    // JS-interop なしの簡易タイムスタンプ（UNIX秒）
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
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

    // music/ ディレクトリの音声ファイル名（拡張子なし）をステムとして列挙する。
    // output/*.json を列挙すると "ユカリ戦 copy.json" のような不要ファイルが混入するため。
    let music_dir = base_dir.join("music");
    if !music_dir.exists() {
        return Ok(vec![]);
    }

    let mut stems: Vec<String> = std::fs::read_dir(&music_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            for ext in AUDIO_EXTS {
                if name.to_lowercase().ends_with(ext) {
                    let stem = name[..name.len() - ext.len()].to_string();
                    return Some(stem);
                }
            }
            None
        })
        .filter(|stem| resolve_json(base_dir, stem).is_some())
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

/// 指定 stem の TrackDataset JSON を返す（ユーザー上書きを適用済み）
#[tauri::command]
fn get_track(stem: String, state: State<AppState>) -> Result<TrackDataset, String> {
    let guard = state.base_dir.lock().map_err(|e| e.to_string())?;
    let base_dir = guard
        .as_ref()
        .ok_or("Base directory not configured.")?;
    let json_file = resolve_json(base_dir, &stem)
        .ok_or_else(|| format!("Track not found: {}", stem))?;
    let data = std::fs::read_to_string(&json_file).map_err(|e| e.to_string())?;
    let mut track: TrackDataset = serde_json::from_str(&data).map_err(|e| e.to_string())?;

    // ユーザー編集のオーバーライドを適用
    let overrides = load_overrides(base_dir, &stem);
    if !overrides.current_overrides.is_empty() {
        for seg in &mut track.segments {
            if let Some(new_label) = overrides.current_overrides.get(&seg.index.to_string()) {
                seg.label = new_label.clone();
            }
        }
    }

    Ok(track)
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

/// セグメントのラベルを更新してオーバーライドJSONに保存する
#[tauri::command]
fn update_segment_label(
    stem: String,
    segment_index: i64,
    new_label: String,
    state: State<AppState>,
) -> Result<(), String> {
    let guard = state.base_dir.lock().map_err(|e| e.to_string())?;
    let base_dir = guard.as_ref().ok_or("Base directory not configured.")?;

    // 現在のラベル（既存overrideまたはベースJSON）を取得
    let mut overrides = load_overrides(base_dir, &stem);
    let old_label = if let Some(lbl) = overrides.current_overrides.get(&segment_index.to_string()) {
        lbl.clone()
    } else {
        // ベースJSONから取得
        let json_file = resolve_json(base_dir, &stem)
            .ok_or_else(|| format!("Track not found: {}", stem))?;
        let data = std::fs::read_to_string(&json_file).map_err(|e| e.to_string())?;
        let track: TrackDataset = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        track.segments
            .iter()
            .find(|s| s.index == segment_index)
            .map(|s| s.label.clone())
            .unwrap_or_default()
    };

    overrides.stem = stem.clone();
    overrides.updated_at = now_timestamp();
    overrides.history.push(HistoryEntry {
        timestamp: now_timestamp(),
        segment_index,
        old_label,
        new_label: new_label.clone(),
    });
    overrides.current_overrides.insert(segment_index.to_string(), new_label);

    save_overrides(base_dir, &stem, &overrides)
}

/// 最後のセグメントラベル変更を元に戻す。変更があれば true、履歴が空なら false を返す
#[tauri::command]
fn undo_segment_label(stem: String, state: State<AppState>) -> Result<bool, String> {
    let guard = state.base_dir.lock().map_err(|e| e.to_string())?;
    let base_dir = guard.as_ref().ok_or("Base directory not configured.")?;

    let mut overrides = load_overrides(base_dir, &stem);
    if overrides.history.is_empty() {
        return Ok(false);
    }

    overrides.history.pop();
    overrides.updated_at = now_timestamp();

    // current_overrides を履歴から再構築
    let mut rebuilt: HashMap<String, String> = HashMap::new();
    for entry in &overrides.history {
        rebuilt.insert(entry.segment_index.to_string(), entry.new_label.clone());
    }
    overrides.current_overrides = rebuilt;

    save_overrides(base_dir, &stem, &overrides)?;
    Ok(true)
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
            update_segment_label,
            undo_segment_label,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
