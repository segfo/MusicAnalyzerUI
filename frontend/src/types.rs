use serde::{Deserialize, Serialize};

/// ソート方向を表現する列挙型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// ソート対象のフィールドを表現する列挙型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortField {
    Bpm,
    Filename,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubCaption {
    pub chunk_index: u32,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentResult {
    pub index: u32,
    pub label: String,
    pub start: f64,
    pub end: f64,
    pub duration: f64,
    pub beat_count: u32,
    pub caption: Option<String>,
    pub caption_note: Option<String>,
    #[serde(default)]
    pub sub_captions: Vec<SubCaption>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverallDescription {
    pub prompt_file: String,
    pub prompt_text: String,
    pub response: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChordResult {
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub label: Option<String>,
    pub label_raw: Option<String>,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackDataset {
    pub schema_version: String,
    pub track_path: String,
    pub track_filename: String,
    pub analysis_timestamp: String,
    pub bpm: Option<f64>,
    #[serde(default)]
    pub bpm_candidates: Vec<f64>,
    #[serde(default)]
    pub beats: Vec<f64>,
    #[serde(default)]
    pub downbeats: Vec<f64>,
    #[serde(default)]
    pub overall_descriptions: Vec<OverallDescription>,
    #[serde(default)]
    pub segments: Vec<SegmentResult>,
    #[serde(default)]
    pub chords: Vec<ChordResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StemAvailability {
    pub vocals: bool,
    pub drums: bool,
    pub bass: bool,
    pub other: bool,
}

impl StemAvailability {
    pub fn any_available(&self) -> bool {
        self.vocals || self.drums || self.bass || self.other
    }
    pub fn all_available(&self) -> bool {
        self.vocals && self.drums && self.bass && self.other
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackSummary {
    pub stem: String,
    pub filename: String,
    pub bpm: Option<f64>,
    pub segment_count: usize,
    pub has_audio: bool,
}

/// Returns a Tailwind bg color class for the given section label.
/// Must return full class strings (not dynamic) for Tailwind JIT to work.
pub fn segment_color(label: &str) -> &'static str {
    match label.to_lowercase().as_str() {
        "intro" => "bg-gray-500",
        "outro" => "bg-gray-400",
        "verse" => "bg-blue-600",
        "chorus" | "refrain" => "bg-orange-500",
        "bridge" => "bg-purple-600",
        "break" => "bg-teal-500",
        "solo" => "bg-yellow-500",
        "pre-chorus" | "prechorus" => "bg-amber-500",
        _ => "bg-indigo-500",
    }
}

/// Maps a chord label to a hue value (0–360) for color visualization.
pub fn chord_hue(label: &str) -> f64 {
    // Strip common prefixes like "N" (no chord) or "("
    let s = label.trim();
    if s == "N" || s == "N/A" || s.is_empty() {
        return 220.0; // default blue-grey
    }
    match s.chars().next().unwrap_or('N') {
        'C' => 0.0,
        'D' => 40.0,
        'E' => 70.0,
        'F' => 130.0,
        'G' => 180.0,
        'A' => 240.0,
        'B' => 300.0,
        _ => 220.0,
    }
}

/// Harte 記法のコードラベルを表示用文字列に変換する。
/// - マイナーコード → "Cm", "F#m" のように小文字 m を付与
/// - メジャーコード → "C", "F#" のようにルート音のみ
/// - "N" / 空文字 → 空文字列
pub fn format_chord_display(label: &str) -> String {
    let s = label.trim();
    if s == "N" || s == "N/A" || s.is_empty() {
        return String::new();
    }

    let bytes = s.as_bytes();
    let mut i = 0;

    // ルート音（A〜G）
    if i >= bytes.len() || !matches!(bytes[i], b'A'..=b'G') {
        return String::new();
    }
    i += 1;

    // 臨時記号（# または b）
    if i < bytes.len() && (bytes[i] == b'#' || bytes[i] == b'b') {
        i += 1;
    }

    let root = &s[..i];
    let quality = &s[i..];

    // マイナー判定: "m"/"min" 接頭辞（"maj" は除外）、":min"、":m"
    let is_minor = (quality.starts_with('m') && !quality.starts_with("maj"))
        || quality.starts_with(":min")
        || quality == ":m";

    if is_minor {
        format!("{}m", root)
    } else {
        root.to_string()
    }
}

/// Format seconds as m:ss
pub fn format_time(secs: f64) -> String {
    if secs.is_nan() || secs.is_infinite() {
        return "0:00".to_string();
    }
    let total = secs as u64;
    let m = total / 60;
    let s = total % 60;
    format!("{m}:{s:02}")
}
