use crate::types::ChordResult;

// ---------------------------------------------------------------------------
// chord_root_index — コードラベルからルート音（0=C〜11=B）を取得
// ---------------------------------------------------------------------------

/// "C", "F#", "Bb:maj7", "Am" 等の Harte 記法ラベルからルート音インデックスを返す。
/// "N" / "N/A" / 空文字は None を返す。
pub fn chord_root_index(label: &str) -> Option<u8> {
    let s = label.trim();
    if s == "N" || s == "N/A" || s.is_empty() {
        return None;
    }
    let mut chars = s.chars();
    let base: i8 = match chars.next()? {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let accidental: i8 = match chars.next() {
        Some('#') => 1,
        Some('b') => -1,
        _ => 0,
    };
    Some(((base + accidental).rem_euclid(12)) as u8)
}

// ---------------------------------------------------------------------------
// score_keys — コードウィンドウから最も確からしいキーインデックスを返す
// ---------------------------------------------------------------------------
// キーインデックス: 0〜11 = 長調 C〜B、12〜23 = 短調 C〜B

fn score_keys(chord_labels: &[&str]) -> u8 {
    // ダイアトニック音程（半音、ルートからの距離）
    const MAJOR: &[i32] = &[0, 2, 4, 5, 7, 9, 11];
    const MINOR: &[i32] = &[0, 2, 3, 5, 7, 8, 10];

    let roots: Vec<i32> = chord_labels
        .iter()
        .filter_map(|&l| chord_root_index(l).map(|r| r as i32))
        .collect();

    if roots.is_empty() {
        return 0; // データなし → C major デフォルト
    }

    let mut best_key: u8 = 0;
    let mut best_score: usize = 0;

    for key_root in 0i32..12 {
        // 長調スコア
        let score = roots
            .iter()
            .filter(|&&r| MAJOR.contains(&((r - key_root).rem_euclid(12))))
            .count();
        if score > best_score {
            best_score = score;
            best_key = key_root as u8;
        }

        // 短調スコア（key_idx = key_root + 12）
        let score = roots
            .iter()
            .filter(|&&r| MINOR.contains(&((r - key_root).rem_euclid(12))))
            .count();
        if score > best_score {
            best_score = score;
            best_key = 12 + key_root as u8;
        }
    }

    best_key
}

// ---------------------------------------------------------------------------
// key_index_to_hue — 五度圏ベースのキー色（0°〜360°）
// ---------------------------------------------------------------------------

/// キーインデックス（0-23）を色相（HSL の H 値、0〜360°）に変換する。
///
/// - 長調（0-11）: 五度圏順で 0°〜132° の暖色域（赤→橙→黄→黄緑）
/// - 短調（12-23）: 五度圏順で 200°〜332° の寒色域（青緑→青→藍→紫）
///
/// 隣接するキー（五度関係）は色相が近くなるため、転調時の遷移もなめらか。
pub fn key_index_to_hue(key_idx: u8) -> f64 {
    // ルート音クラス（0=C〜11=B）→ 五度圏上の位置（0〜11）
    // 五度圏順: C G D A E B F# Db Ab Eb Bb F
    //  root:    0 7 2 9 4 11 6  1  8  3  10 5
    const FIFTHS_POS: [usize; 12] = [
        0,  // C
        7,  // C#/Db
        2,  // D
        9,  // D#/Eb
        4,  // E
        11, // F
        6,  // F#/Gb
        1,  // G
        8,  // G#/Ab
        3,  // A
        10, // A#/Bb
        5,  // B
    ];
    let root = (key_idx % 12) as usize;
    let pos = FIFTHS_POS[root] as f64; // 0.0〜11.0
    
    if key_idx < 12 {
        // 長調: 暖色系 0°〜132°（12 ステップ × 12°）
        pos * 12.0
    } else {
        // 短調: 寒色系 200°〜332°（同じ五度圏順序で +200°）
        200.0 + pos * 12.0
    }
}

// ---------------------------------------------------------------------------
// estimate_key_timeline — コード列からキー推定タイムラインを生成
// ---------------------------------------------------------------------------

/// コード列全体を解析し、(時刻, 色相) ペアのタイムラインを返す。
/// 各コードを中心に前後 8 コードのウィンドウでキーをスコアリングし、
/// キーが変わった時点だけエントリを追加（重複排除）。
pub fn estimate_key_timeline(chords: &[ChordResult]) -> Vec<(f64, f64)> {
    if chords.is_empty() {
        return Vec::new();
    }

    let mut result: Vec<(f64, f64)> = Vec::new();
    let mut prev_key: Option<u8> = None;

    for (i, chord) in chords.iter().enumerate() {
        let Some(t) = chord.start else { continue };

        let window_start = i.saturating_sub(8);
        let window_end = (i + 9).min(chords.len());
        let window: Vec<&str> = chords[window_start..window_end]
            .iter()
            .filter_map(|c| c.label.as_deref())
            .collect();

        let key = score_keys(&window);
        if prev_key != Some(key) {
            prev_key = Some(key);
            result.push((t, key_index_to_hue(key)));
        }
    }

    result
}

// ---------------------------------------------------------------------------
// lookup_key_hue — タイムラインから指定時刻の色相を返す（二分探索）
// ---------------------------------------------------------------------------

/// タイムラインを二分探索し、時刻 t 以前の最後のキー色相を返す。
/// タイムラインが空の場合はデフォルト値 220.0 を返す。
pub fn lookup_key_hue(timeline: &[(f64, f64)], t: f64) -> f64 {
    if timeline.is_empty() {
        return 220.0;
    }
    let idx = timeline.partition_point(|(time, _)| *time <= t);
    if idx == 0 {
        timeline[0].1
    } else {
        timeline[idx - 1].1
    }
}
