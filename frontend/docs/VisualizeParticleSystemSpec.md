# Visualize Particle System Spec

対象ファイル: `src/components/viz_canvas.rs`

---

## 概要

楽曲のセクション構造・RMS・ビート情報をリアルタイムに解析し、パーティクルと図形エフェクトで可視化するシステム。

エフェクトは以下の系統で構成される:

| 系統 | 概要 |
|------|------|
| **Pattern A** | RMS 上昇トレンド（Accumulation） |
| **Pattern B** | Chorus 遷移時の収束→フラッシュ→インパクト |
| **Pattern C** | Bridge 低 RMS（Drop / Spiral） |
| **Chorus→Chorus** | 八角形回転 + スパイラルアーム |
| **DIP_RECOVER** | セクション内 RMS ディップ→回復に応じた演出 |
| **Soft Rings** | Chorus 内 RMS 下降時の拡散リング |

---

## RMS パイプライン

### 変数一覧

| 変数 | 説明 |
|------|------|
| `rms_current` | 生 RMS を α=0.15 で lerp 平滑化（即応性重視） |
| `rms_delta` | `rms_current - rms_prev`（フレーム間差分） |
| `energy_trend` | `rms_delta` に α=0.02 で lerp（長期傾向） |
| `section_rms_avg` | セクション開始時にリセットされる区間内 RMS 平均 |
| `rms_density` | `rms_current / section_rms_avg`（区間内相対音量、0〜2） |
| `baseline_rms` | 曲全体の非対称 EMA（上昇時 α=0.002、下降時 α=0.0005） |
| `drop_ratio` | `rms_current / baseline_rms`（曲全体基準の相対音量） |
| `drop_intensity` | `((0.70 - drop_ratio) / 0.70).clamp(0,1)`（落差の深さ 0〜1） |

### 非対称 EMA（baseline_rms）

```
rms_current > baseline_rms の時: alpha = 0.002（音量上昇時はゆっくり追従）
rms_current < baseline_rms の時: alpha = 0.0005（音量下降時はさらにゆっくり）
最低値ガード: baseline_rms.max(0.01)
```

これにより「一瞬の静寂」で基準値が下がりすぎるのを防ぎ、溜め判定の誤発を抑制する。

### drop_intensity スケール感

| drop_ratio | drop_intensity | 意味 |
|-----------|---------------|------|
| 1.0〜0.70 | 0.0（発動なし）| 通常音量帯 |
| 0.70（-30%）| 0.0〜開始 | 溜め開始ライン |
| 0.50（-50%）| 約 0.5 | 中程度の落差 |
| 0.30以下（-70%+）| 1.0 | 最大落差（Super Burst ゾーン）|

---

## Charge システム

Anticipation 中に `charge`（0〜1）を蓄積し、Impact の強度に反映する。

| 変数 | 説明 |
|------|------|
| `charge` | 現在の蓄積量（Anticipation 中に 1.0 へ漸近） |
| `charge_peak` | charge の最大到達値（Impact 解放後もリセットまで保持） |
| `charge_count` | 累積 Anticipation 発動回数（ブルーム長の対数スケールに使用） |

```
Anticipation 中: charge += (1.0 - charge) * 0.3 * dt
Inactive 時:    charge *= 0.95（キャンセル時は減衰）
Impact 後:      charge = 0.0, charge_peak = 0.0
```

---

## Pattern A — 上昇トレンド（Accumulation）

**発動条件**: `energy_trend > 0.0005` が 3 秒以上継続

**エフェクト**:
- パーティクル生成数を `RMS²` でスケール（最大 3 倍）
- 色相を暖色方向へ最大 +30° シフト（上昇継続時間に比例）

**解除**: `energy_trend` が低下し `rms_trend_timer < 1.0` になったとき

---

## Pattern B — Chorus 遷移（収束→フラッシュ→インパクト）

### 溜め判定（`rms_is_dropping`）

```
drop_ratio < 0.70  OR  energy_trend < -0.001
```

曲全体 `baseline_rms` から 30% 以上の下降、または持続的な下降傾向で真になる。  
**Pre-Chorus（盛り上がりながらコーラスへ入るケース）では発動しない。**

### `approaching_chorus` 条件

```
!cur_is_chorus AND nxt_is_chorus AND secs_until_next < 8.0 AND rms_is_dropping
```

### 状態遷移

```
Inactive
  ├─ [approaching_chorus]
  │    drop_intensity 確定 → Anticipation（収束エフェクト開始）
  │
  └─ [!cur_is_chorus AND nxt_is_chorus AND secs < 0.5s]
       RMS 関係なく直前フラッシュ → SilenceFlash
       ※ Chorus→Chorus の場合はこのパスに入らない

Anticipation（パーティクルが中心に引き寄せられる）
  ├─ [cur_is_chorus]             → Impact（直接）
  ├─ [secs < 0.5s, !cur_is_chorus] → SilenceFlash
  │    ※ Chorus→Chorus 時は直接 Impact へ
  └─ [!approaching_chorus]       → Inactive（キャンセル）

SilenceFlash（白フラッシュオーバーレイ）
  持続時間: 通常 0.25s / ラスサビ 0.5〜0.75s（last_chorus_intensity で増幅）
  └─ [cur_is_chorus OR timer >= flash_dur] → Impact

Impact（ラジアルブラー + 爆発バースト、0.5s）
  └─ [timer >= 0.5s] → Inactive
```

### Impact エフェクト詳細

**ブルーム（ラジアルブラー）フレーム数:**
```
bloom_scale = drop_intensity.max(charge_peak * 0.8)
count_multiplier = log2(charge_count + 1).max(1.0)
radial_blur_frames = (18 + bloom_scale * 100 * count_multiplier).min(180)
```
ラスサビ時はさらに `* (1.0 + last_chorus_intensity * 0.75)` 倍（最大 1.75 倍）。

**爆発パーティクル数 / 速度:**
```
scale = drop_intensity.max(charge_peak * 0.8)
burst_count = 120 + scale * 180   // 120〜300 粒子
speed_max   = 40 + scale * 20     // 40〜60
```

**スパイラルアーム + 八角形回転（落差大時のみ）:**
```
drop_intensity >= 0.5 の場合:
  octagon_spin_timer = 1.0 + (drop_intensity - 0.5) * 2.0  // 1.0〜2.0s
```

### Anticipation 中の視覚フィードバック

- **パーティクル引力**: `charge` に応じて中心へ引き寄せる力を強化
  ```
  pull = 0.03 + charge * 0.12  // charge=1 で 0.15（通常の5倍）
  ```
- **赤みグロウ**: `charge > 0.1` のとき中央からのラジアルグラデーション
  ```
  alpha = charge * 0.15 + drop_intensity * 0.15  // 最大 0.30
  ```

### セクション別の動作まとめ

| 遷移パターン | RMS 状態 | 収束 | フラッシュ | 爆発 | スパイラル |
|------------|---------|:----:|:--------:|:---:|:--------:|
| Verse → Chorus | 通常 | ✗ | ✓ | ✓ | 落差次第 |
| Verse → Chorus | 下降中（溜め）| ✓ | ✓ | ✓ | 落差次第 |
| Bridge → Chorus | 下降中（溜め）| ✓ | ✓ | ✓ | 落差次第 |
| Chorus → Chorus | 通常 | ✗ | ✗ | ✗ | ✓（常時） |
| Chorus → Chorus | 下降中 | ✓ | ✗ | ✓ | ✓ |
| ラスサビ直前 | 下降中 | ✓ | ✓（長め）| ✓ | 落差次第 |

---

## Pattern C — Bridge 低 RMS（Drop / Spiral）

**発動条件**: `cur_is_bridge AND rms_current < 0.05`

**エフェクト**:
- パーティクルに接線方向加速度を加えて螺旋運動
- `visual_pattern = Drop`、ワイヤーフレーム化（wireframe_alpha → 1.0）
- パーティクル数を半減（particle_count_scale → 0.5）
- `rms_current > 0.1` になった瞬間にズームアウト（base_r_scale → 0.85）してから lerp 復帰

---

## Chorus 出口演出

**発動条件**: `cur_is_chorus AND !nxt_is_chorus AND secs_until_next < 3.0`

**エフェクト**:
- wireframe_alpha → 0.5
- particle_count_scale → 0.7

---

## Chorus→Chorus 専用エフェクト（八角形回転 + スパイラルアーム）

### 境界検知

`current_section_label` シグナルは同一ラベル（"chorus"→"chorus"）では変化しないため、  
**`secs_until_next` のジャンプ**（前フレーム値 + 5.0 秒以上の増加）で境界を検知する。

```
secs_jumped = secs_until_next > last_secs_until_next + 5.0
発動: secs_jumped AND cur_is_chorus AND prev_cur_was_chorus
→ octagon_spin_timer = 1.0（固定1秒）
```

### 八角形1回転エフェクト

- `octagon_spin_timer`: 1.0 → 0.0（1 秒）
- 回転角: `(1.0 - timer) * 2π`
- アルファ: `sin(progress * π)`（フェードイン/アウト）
- 外側八角形（`base_r * 1.6`、時計回り）+ 内側八角形（`base_r * 1.1`、逆回転 × 0.7）
- ストロークカラー: `hue + 60°`、ビートでライン幅パルス

### スパイラルアームパーティクル

境界から `octagon_spin_timer` 秒間、八角形の各頂点（8箇所）から毎フレームスポーン。

**スポーン位置**: `base_r * 1.7〜1.9` の各頂点角度（外側から出現）

**初速:**
```
接線方向（反時計回り）: (sin θ, -cos θ) * 20.0 * presence
外向き成分:             (cos θ,  sin θ) *  5.0
```

**継続接線加速度（パーティクル更新ループ内）:**
```
strength = c2c_vortex * 0.05
vx += (dy / dist) * strength
vy += (-dx / dist) * strength
```

| パラメータ | 値 |
|-----------|-----|
| decay | 0.014 |
| friction | 0.97 |
| size | 3.0〜5.5 px |
| hue | `state.hue + 60° ± 20°` |

---

## DIP_RECOVER — セクション内 RMS ディップ→回復

### 検知ロジック

```
DIP_ENTER = 0.50  // baseline の 50% 未満でディップ開始
DIP_EXIT  = 0.75  // baseline の 75% 以上に回復でトリガー

ディップ中: dip_min_ratio を更新（最深落差を記録）
回復時:    dip_intensity = ((0.50 - dip_min_ratio) / 0.50).clamp(0,1)
```

### セクション別の発火内容

| セクション | 発火内容 |
|-----------|---------|
| Verse / Pre-Chorus | **雨パーティクル**（画面全体からランダムにスポーン、下向き） |
| Bridge | **スパイラルアーム**のみ（`dip_intensity >= 0.3` で `octagon_spin_timer` 設定） |
| Chorus | **スキップ**（Soft Rings が別途動作） |
| Intro / Outro / その他 | **スキップ** |

> 爆発バーストは **Chorus 遷移時（Pattern B）のみ** 発火し、DIP_RECOVER からは発火しない。

### 雨パーティクル（Verse/Pre-Chorus）

```
count = 80 + rain_intensity * 160   // 80〜240 粒子
x     = rand(0, w)                  // 画面全体のランダム位置
y     = rand(0, h)
vy    = 2.5〜4.5（下向き）
vx    = ±0.3（微小横揺れ）
friction = 0.99
decay    = 0.005
fade_in  = 1.0  // 徐々にアルファが上がるフェードイン（約33フレーム）
```

---

## Soft Rings — Chorus 内 RMS 下降

**発動条件**: `cur_is_chorus AND rms_is_dropping AND pattern_b_state == Inactive`

毎フレーム最大 6 個のリングを維持し、中心から外側へ拡散する。

| パラメータ | 値 |
|-----------|-----|
| radius 増加 | `ring.speed * dt`（フレームごと） |
| speed | 30〜80 px/s |
| life | 1.0 → 0.0（decay 0.4/s） |
| hue | `state.hue + 40°` |
| 描画 | stroke（線幅 2px）、`lighter` 合成 |

---

## ラスサビ判定（last_chorus_active）

```
cur_is_chorus AND !nxt_is_chorus AND nxt ∈ {"", "outro", "end", "coda", ...}
```

強度: `last_chorus_intensity = (0.5 + drop_intensity * 0.5).min(1.0)`

| 効果 | 通常 | ラスサビ（強度最大）|
|------|------|--------------|
| フラッシュ持続 | 0.25s | 0.75s |
| ブルーム倍率 | × 1.0 | × 1.75 |

---

## パーティクル共通

| 種別 | 説明 |
|------|------|
| 通常スポーン | `base_r ± 10%` のリング上からランダム角度で外向き放出 |
| decay | 0.005〜0.025（フレームごとの life 減少量） |
| friction | 0.91〜0.99（速度減衰） |
| 高 RMS 時 | 加算合成（`lighter`）でブルーム効果 |
| 低 RMS 時 | 輝度 +25% でハロー効果 |
| `fade_in` | 1.0→0.0 で徐々に不透明化（雨パーティクルのみ使用） |

### ラジアルブラー（Impact 時）

```
alpha = 0.10 + progress * 0.55   // フレーム開始時は明るく、徐々に減衰
scale = radial_blur_scale         // 1.04〜1.08（大落差時に拡大）
```
