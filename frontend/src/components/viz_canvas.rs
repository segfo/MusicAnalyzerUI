use leptos::*;
use leptos::html::Canvas;
use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::audio::{StemAudioEngine, StemVolumes};
use crate::pages::visualization::VizContext;
use crate::state::{GlobalPlayback, VisualizationPageState};

// ---------------------------------------------------------------------------
// ビジュアルパターン / Pattern B 状態機械
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum VisualPattern { Normal, Accumulation, Contrast, Drop }

#[derive(Clone, Copy, PartialEq)]
enum PatternBState { Inactive, Anticipation, SilenceFlash, Impact }

/// アニメーション全体への指示。rAF ループが毎フレーム is_playing から設定する。
#[derive(Clone, Copy, PartialEq)]
enum AnimDirective {
    /// 再生中: 音楽に完全同期したリアクティブアニメーション
    Playing,
    /// 停止/一時停止: 音楽非依存の環境アニメーション
    Idle,
}

// ---------------------------------------------------------------------------
// Internal animation state (lives inside the rAF closure)
// ---------------------------------------------------------------------------

struct Particle {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    life: f64,      // 0.0 → 1.0 (1.0 = just spawned, 0.0 = dead)
    decay: f64,     // per-frame life reduction (varies per particle)
    friction: f64,  // velocity multiplier per frame (0.92 = decelerates)
    hue: f64,
    size: f64,
    fade_in: f64,   // 0.0 = フェードイン完了、>0 = まだフェードイン中（1.0→0.0）
}

struct SoftRing {
    radius: f64,    // 現在の半径
    speed: f64,     // 拡大速度 (px/frame)
    life: f64,      // 1.0→0.0
    hue: f64,
}

struct AnimState {
    beat_pulse: f64,
    downbeat_pulse: f64,
    distortion_seed: f64,
    hue: f64,
    target_hue: f64,
    energy: f64,
    target_energy: f64,
    density: f64,
    target_density: f64,
    particles: Vec<Particle>,
    chord_label: String,
    // pseudo-random state (simple LCG)
    rng: u64,
    // --- ピッチ偏差 / キーアンティシペーション ---
    vocal_pitch_dev: f64,   // ±20° 以内の色相偏差
    next_key_hue: f64,      // 次コードのキー色相（アンティシペーション用）
    chord_completion: f64,  // 現コード内進行度 0.0〜1.0
    vocals_available: bool, // ボーカルステム有無
    chord_unknown: bool,    // 現コードが N（未検出）かどうか
    n_elapsed: f64,         // N 区間の連続経過秒数（chord_unknown = false でリセット）
    // --- RMS パイプライン ---
    rms_current: f64,          // lerp 平滑化 (α=0.15)
    rms_prev: f64,
    rms_delta: f64,            // rms_current - rms_prev
    energy_trend: f64,         // long-term slope、lerp toward rms_delta (α=0.02)
    section_rms_acc: f64,      // 区間内 RMS 累積
    section_rms_count: u32,    // 区間内フレーム数
    section_rms_avg: f64,      // 区間平均 RMS（0 除算ガード: 0.0001）
    rms_density: f64,          // rms_current / section_rms_avg, clamp 0..2
    // --- セクション遷移情報（VizContext から同期）---
    current_section: String,
    next_section: String,      // 次セクションのラベル（末尾なら ""）
    secs_until_next: f64,      // 現セクション終了までの秒数
    // --- パターン状態 ---
    visual_pattern: VisualPattern,
    rms_trend_timer: f64,      // Pattern A: energy_trend > 閾値の継続秒数
    // Pattern B
    pattern_b_state: PatternBState,
    pattern_b_timer: f64,
    radial_blur_frames: u32,   // >0 のフレームは radial blur を描画
    impact_burst: bool,        // true = 次フレームで爆発バースト生成
    // Pattern C / 出口演出
    wireframe_alpha: f64,      // 0→1 でワイヤーフレーム化 (lerp)
    spiral_particles: bool,    // 接線方向加速度を粒子に加える
    particle_count_scale: f64, // 1.0→0.5 で粒子数半減 (lerp)
    base_r_scale: f64,         // Drop 回復時に一時 0.85→lerp back
    pitch_frame_counter: u32,  // autocorrelation を間引くフレームカウンタ
    // --- Chorus→Chorus 八角形1回転エフェクト ---
    // secs_until_next のジャンプ（0付近→大値）で Chorus→Chorus 境界を検知する
    octagon_spin_timer: f64,        // 1.0→0.0（1秒で1回転）、0なら非アクティブ
    last_secs_until_next: f64,      // 前フレームの secs_until_next（ジャンプ検知用）
    prev_cur_was_chorus: bool,      // 前フレームが chorus だったか
    // --- 動的 RMS 基準 ---
    baseline_rms: f64,              // 曲全体の非対称 EMA（上昇は遅く、下降はさらに遅く）
    // --- Charge システム ---
    charge: f64,                    // 0.0〜1.0、溜まり具合
    charge_peak: f64,               // charge の最大到達値（Super Burst 強度に使用）
    charge_count: u32,              // Anticipation 発動回数（曲中の累積、リセットなし）
    drop_intensity: f64,            // 0.0〜1.0、落差の大きさ（全エフェクトのスケール係数）
    radial_blur_scale: f64,         // Impact 時の拡大スケール（1.04〜1.08）
    // --- Soft Rings（Chorus内 RMS 下降時）---
    soft_rings: Vec<SoftRing>,      // 拡散リングのリスト
    // --- RMS drop→recover 検知 ---
    in_rms_dip: bool,               // 現在 baseline の 50% 未満のディップ中か
    dip_min_ratio: f64,             // ディップ中の最小 drop_ratio（落差の深さを記録）
    rain_burst: bool,               // true = 次フレームで降り注ぎパーティクル生成
    rain_intensity: f64,            // 雨パーティクルの強度（0〜1）
    // --- ラスサビ判定 ---
    last_chorus_active: bool,       // 次がOutro/End/空 → ラスサビ中
    last_chorus_intensity: f64,     // ラスサビ強度（0.0〜1.0）落差で増幅
    // --- デバッグログ用タイマー ---
    debug_log_timer: f64,           // 毎秒1回の RMS ログ用カウンタ
    // --- アニメーション指示 ---
    directive: AnimDirective,       // Playing / Idle（毎フレーム is_playing から設定）
}

impl AnimState {
    fn new() -> Self {
        Self {
            beat_pulse: 0.0,
            downbeat_pulse: 0.0,
            distortion_seed: 0.0,
            hue: 220.0,
            target_hue: 220.0,
            energy: 0.5,
            target_energy: 0.5,
            density: 0.5,
            target_density: 0.5,
            particles: Vec::with_capacity(200),
            chord_label: String::new(),
            rng: 12345,
            vocal_pitch_dev: 0.0,
            next_key_hue: 220.0,
            chord_completion: 0.0,
            vocals_available: false,
            chord_unknown: false,
            n_elapsed: 0.0,
            rms_current: 0.0,
            rms_prev: 0.0,
            rms_delta: 0.0,
            energy_trend: 0.0,
            section_rms_acc: 0.0,
            section_rms_count: 0,
            section_rms_avg: 0.0001,
            rms_density: 1.0,
            current_section: String::new(),
            next_section: String::new(),
            secs_until_next: 999.0,
            visual_pattern: VisualPattern::Normal,
            rms_trend_timer: 0.0,
            pattern_b_state: PatternBState::Inactive,
            pattern_b_timer: 0.0,
            radial_blur_frames: 0,
            impact_burst: false,
            wireframe_alpha: 0.0,
            spiral_particles: false,
            particle_count_scale: 1.0,
            base_r_scale: 1.0,
            pitch_frame_counter: 0,
            octagon_spin_timer: 0.0,
            last_secs_until_next: 999.0,
            prev_cur_was_chorus: false,
            baseline_rms: 0.05,
            charge: 0.0,
            charge_peak: 0.0,
            charge_count: 0,
            drop_intensity: 0.0,
            radial_blur_scale: 1.04,
            soft_rings: Vec::new(),
            in_rms_dip: false,
            dip_min_ratio: 1.0,
            rain_burst: false,
            rain_intensity: 0.0,
            last_chorus_active: false,
            last_chorus_intensity: 0.0,
            debug_log_timer: 0.0,
            directive: AnimDirective::Idle,
        }
    }

    fn rand(&mut self) -> f64 {
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.rng >> 33) as f64 / ((u64::MAX >> 33) as f64)
    }

    fn rand_range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.rand() * (hi - lo)
    }
}

// ---------------------------------------------------------------------------
// VizCanvas component
// ---------------------------------------------------------------------------

#[component]
pub fn VizCanvas() -> impl IntoView {
    let canvas_ref = create_node_ref::<Canvas>();
    let viz = use_context::<VizContext>().expect("VizContext missing");
    let viz_state = use_context::<VisualizationPageState>().expect("VisualizationPageState missing");
    let global = use_context::<GlobalPlayback>().expect("GlobalPlayback missing");

    // Start the rAF loop once the canvas is mounted; cancel it on unmount
    create_effect({
        let viz = viz.clone();
        let stem_volumes = viz_state.stem_volumes.read_only();
        let stem_engine_sv = global.stem_engine;
        let is_playing = global.is_playing;
        move |_| {
            let Some(canvas_el) = canvas_ref.get() else { return };
            let canvas = canvas_el.unchecked_ref::<HtmlCanvasElement>().clone();
            let (alive, raf_id) = start_animation_loop(canvas, viz.clone(), stem_volumes, stem_engine_sv, is_playing, global.current_time, global.duration);
            on_cleanup(move || {
                alive.set(false);
                if let Some(win) = web_sys::window() {
                    let _ = win.cancel_animation_frame(raf_id.get());
                }
            });
        }
    });

    // Resize canvas to its CSS size
    create_effect(move |_| {
        let Some(canvas_el) = canvas_ref.get() else { return };
        let canvas = canvas_el.unchecked_ref::<HtmlCanvasElement>().clone();
        let w = canvas.offset_width() as u32;
        let h = canvas.offset_height() as u32;
        if w > 0 && h > 0 {
            canvas.set_width(w);
            canvas.set_height(h);
        }
    });

    view! {
        <canvas
            node_ref=canvas_ref
            class="w-full h-full block"
            style="background:#030712"
        />
    }
}

// ---------------------------------------------------------------------------
// rAF loop setup
// ---------------------------------------------------------------------------

fn start_animation_loop(
    canvas: HtmlCanvasElement,
    viz: VizContext,
    stem_volumes: ReadSignal<StemVolumes>,
    stem_engine_sv: StoredValue<Option<StemAudioEngine>>,
    is_playing: RwSignal<bool>,
    current_time: RwSignal<f64>,
    duration: RwSignal<f64>,
) -> (Rc<Cell<bool>>, Rc<Cell<i32>>) {
    let state = Rc::new(RefCell::new(AnimState::new()));

    // alive flag: set to false by on_cleanup to stop the loop
    let alive = Rc::new(Cell::new(true));
    // latest rAF ID: used by on_cleanup to cancel a scheduled-but-not-yet-run frame
    let raf_id: Rc<Cell<i32>> = Rc::new(Cell::new(0));

    // Signals: beat/downbeat triggers (u32 counter — increments fire pulses)
    let prev_beat = Rc::new(RefCell::new(0u32));
    let prev_downbeat = Rc::new(RefCell::new(0u32));

    // Set up reactive effects to capture triggers and push into AnimState
    {
        let state_b = state.clone();
        let prev = prev_beat.clone();
        create_effect(move |_| {
            let cur = viz.beat_trigger.get();
            let p = *prev.borrow();
            if cur != p {
                *prev.borrow_mut() = cur;
                let sv = stem_volumes.get();
                state_b.borrow_mut().beat_pulse = sv.drums;
                state_b.borrow_mut().distortion_seed = sv.vocals;
            }
        });
    }
    {
        let state_db = state.clone();
        let prev = prev_downbeat.clone();
        create_effect(move |_| {
            let cur = viz.downbeat_trigger.get();
            let p = *prev.borrow();
            if cur != p {
                *prev.borrow_mut() = cur;
                let sv = stem_volumes.get();
                state_db.borrow_mut().downbeat_pulse = sv.bass;
            }
        });
    }
    {
        // estimated_key_hue → target_hue（chord_hue フォールバック含む）
        let state_c = state.clone();
        create_effect(move |_| {
            let hue = viz.estimated_key_hue.get();
            state_c.borrow_mut().target_hue = hue;
        });
    }
    {
        let state_e = state.clone();
        create_effect(move |_| {
            let e = viz.energy.get();
            let d = viz.density.get();
            let mut s = state_e.borrow_mut();
            s.target_energy = e;
            s.target_density = d;
        });
    }
    {
        let state_ch = state.clone();
        create_effect(move |_| {
            let label = viz.current_chord.get();
            state_ch.borrow_mut().chord_label = label;
        });
    }
    {
        let state_n = state.clone();
        create_effect(move |_| {
            let h = viz.next_key_hue.get();
            state_n.borrow_mut().next_key_hue = h;
        });
    }
    {
        let state_cc = state.clone();
        create_effect(move |_| {
            let c = viz.chord_completion.get();
            state_cc.borrow_mut().chord_completion = c;
        });
    }
    {
        let state_va = state.clone();
        create_effect(move |_| {
            let v = viz.vocals_available.get();
            state_va.borrow_mut().vocals_available = v;
        });
    }
    {
        let state_cu = state.clone();
        create_effect(move |_| {
            let v = viz.chord_unknown.get();
            state_cu.borrow_mut().chord_unknown = v;
        });
    }
    {
        let state_ip = state.clone();
        create_effect(move |_| {
            if !is_playing.get() {
                let t = current_time.get_untracked();
                let dur = duration.get_untracked();
                // 先頭（0:00）または末尾（曲終了）のときのみ色を初期状態にリセット
                let at_start = t < 0.5;
                let at_end = dur > 0.0 && dur - t < 1.0;
                if at_start || at_end {
                    let mut s = state_ip.borrow_mut();
                    s.target_hue = 220.0;
                    s.vocal_pitch_dev = 0.0;
                    s.n_elapsed = 0.0;
                }
            }
        });
    }
    // current_section_label → AnimState 同期（セクション変化時 RMS 累積リセット）
    {
        let state_sec = state.clone();
        create_effect(move |_| {
            let label = viz.current_section_label.get();
            let mut s = state_sec.borrow_mut();
            s.section_rms_acc = 0.0;
            s.section_rms_count = 0;
            s.current_section = label;
        });
    }
    // next_section_label / secs_until_next_section → AnimState 同期（毎 tick）
    {
        let state_ns = state.clone();
        create_effect(move |_| {
            let next = viz.next_section_label.get();
            let secs = viz.secs_until_next_section.get();
            let mut s = state_ns.borrow_mut();
            s.next_section = next;
            s.secs_until_next = secs;
        });
    }

    // Build rAF closure
    let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();

    let alive_inner = alive.clone();
    let raf_id_inner = raf_id.clone();
    *g.borrow_mut() = Some(Closure::new(move || {
        // Stop the loop if the component has been unmounted
        if !alive_inner.get() {
            return;
        }

        // Sync canvas size to CSS layout
        let w = canvas.offset_width() as u32;
        let h = canvas.offset_height() as u32;
        if w > 0 && h > 0 && (canvas.width() != w || canvas.height() != h) {
            canvas.set_width(w);
            canvas.set_height(h);
        }

        // 毎フレーム先頭で directive を is_playing から設定
        {
            let mut s = state.borrow_mut();
            s.directive = if is_playing.get_untracked() {
                AnimDirective::Playing
            } else {
                AnimDirective::Idle
            };
        }

        // N 経過時間を更新（再生中かつ N 区間のみ加算、それ以外はリセット）
        {
            let mut s = state.borrow_mut();
            if s.chord_unknown && s.directive == AnimDirective::Playing {
                s.n_elapsed += 1.0 / 60.0;
            } else {
                s.n_elapsed = 0.0;
            }
        }

        // --- RMS 計算 / Idle 時のニュートラル減衰 ---
        if state.borrow().directive == AnimDirective::Playing {
            let raw_rms = stem_engine_sv.with_value(|se| {
                let data = se.as_ref()?.get_mix_time_domain_data()?;
                let n = data.len() as f32;
                Some(((data.iter().map(|&x| x * x).sum::<f32>() / n).sqrt()) as f64)
            }).unwrap_or(0.0);

            let mut s = state.borrow_mut();
            lerp_to(&mut s.rms_current, raw_rms, 0.15);
            s.rms_delta = s.rms_current - s.rms_prev;
            s.rms_prev = s.rms_current;
            let rms_delta_copy = s.rms_delta;
            lerp_to(&mut s.energy_trend, rms_delta_copy, 0.02);
            s.section_rms_acc += s.rms_current;
            s.section_rms_count += 1;
            s.section_rms_avg = (s.section_rms_acc / s.section_rms_count.max(1) as f64).max(0.0001);
            s.rms_density = (s.rms_current / s.section_rms_avg).clamp(0.0, 2.0);

            let rms_cur = s.rms_current;
            let alpha = if rms_cur > s.baseline_rms { 0.002 } else { 0.0005 };
            lerp_to(&mut s.baseline_rms, rms_cur, alpha);
            s.baseline_rms = s.baseline_rms.max(0.01);

            // デバッグログ（毎秒1回）
            s.debug_log_timer += 1.0 / 60.0;
            if s.debug_log_timer >= 1.0 {
                s.debug_log_timer = 0.0;
                let drop_ratio = s.rms_current / s.baseline_rms;
                let drop_intensity = ((0.70 - drop_ratio) / 0.70).clamp(0.0, 1.0);
                web_sys::console::log_1(&format!(
                    "[RMS] baseline={:.3} current={:.3} drop_ratio={:.0}% drop_intensity={:.0}%",
                    s.baseline_rms, s.rms_current, drop_ratio * 100.0, drop_intensity * 100.0
                ).into());
            }
        } else {
            // Idle: RMS をゆっくり 0 へ減衰、target_energy/density をニュートラルへ
            let mut s = state.borrow_mut();
            lerp_to(&mut s.rms_current, 0.0, 0.03);
            lerp_to(&mut s.target_energy,  0.35, 0.005);
            lerp_to(&mut s.target_density, 0.25, 0.005);
        }

        // --- パターン状態機械（Playing のみ）---
        if state.borrow().directive == AnimDirective::Playing {
            update_pattern_state(&mut state.borrow_mut(), 1.0 / 60.0);
        }

        // ピッチ偏差 / コードアンティシペーション / ドリフト計算（Playing かつ N 区間のみ）
        if state.borrow().directive == AnimDirective::Playing {
            let (chord_unknown, vocals_available, next_hue, target, completion, n_elapsed,
                 prev_pitch_dev, pitch_frame_counter) = {
                let s = state.borrow();
                (s.chord_unknown, s.vocals_available, s.next_key_hue, s.target_hue,
                 s.chord_completion, s.n_elapsed, s.vocal_pitch_dev, s.pitch_frame_counter)
            };
            let dev = if !chord_unknown {
                0.0
            } else {
                let drift = n_section_drift(n_elapsed);
                if vocals_available {
                    let pitch = if pitch_frame_counter % 6 == 0 {
                        stem_engine_sv.with_value(|se| {
                            let se = se.as_ref()?;
                            let data = se.get_vocal_time_domain_data()?;
                            let sr = se.sample_rate();
                            Some(autocorrelation_pitch(&data, sr).map(hz_to_hue_deviation).unwrap_or(0.0))
                        }).unwrap_or(0.0)
                    } else {
                        prev_pitch_dev - n_section_drift(n_elapsed - 1.0 / 60.0)
                    };
                    pitch + drift
                } else {
                    let ramp = ((n_elapsed - 0.5) * 2.0).clamp(0.0, 1.0);
                    let anticipation = {
                        let diff = hue_diff_wrapped(next_hue, target);
                        diff * completion.min(0.8) * 0.30 * (1.0 - ramp)
                    };
                    anticipation + drift
                }
            };
            {
                let mut s = state.borrow_mut();
                s.vocal_pitch_dev = dev;
                s.pitch_frame_counter = pitch_frame_counter.wrapping_add(1);
            }
        } else {
            // Idle: ピッチ偏差をゆっくり 0 へ減衰
            let mut s = state.borrow_mut();
            lerp_to(&mut s.vocal_pitch_dev, 0.0, 0.05);
        }

        if let Ok(ctx) = canvas
            .get_context("2d")
            .ok()
            .flatten()
            .and_then(|o| o.dyn_into::<CanvasRenderingContext2d>().ok())
            .ok_or(())
        {
            let sv = stem_volumes.get_untracked();
            render_frame(&ctx, &canvas, &mut state.borrow_mut(), w as f64, h as f64, sv);
        }

        // Schedule next frame; store the ID so it can be cancelled
        let id = request_animation_frame(f.borrow().as_ref().unwrap());
        raf_id_inner.set(id);
    }));

    let id = request_animation_frame(g.borrow().as_ref().unwrap());
    raf_id.set(id);

    (alive, raf_id)
}

fn request_animation_frame(f: &Closure<dyn FnMut()>) -> i32 {
    web_sys::window()
        .expect("no window")
        .request_animation_frame(f.as_ref().unchecked_ref())
        .expect("requestAnimationFrame failed")
}

// ---------------------------------------------------------------------------
// Per-frame rendering
// ---------------------------------------------------------------------------

fn render_frame(
    ctx: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    state: &mut AnimState,
    w: f64,
    h: f64,
    sv: StemVolumes,
) {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let base_r = h.min(w) * 0.18 * state.base_r_scale;

    // --- Radial Blur（Impact フェーズ）: Playing のみ ---
    if state.radial_blur_frames > 0 && state.directive == AnimDirective::Playing {
        let scale = state.radial_blur_scale; // 落差に応じて 1.04〜1.08
        // 残フレーム比率: 1.0（直後）→ 0.0（末尾）でフェードアウト
        let total = (18.0 + state.drop_intensity.max(state.charge_peak * 0.8)
            * 100.0
            * ((state.charge_count as f64 + 1.0).log2()).max(1.0))
            .min(180.0);
        let progress = state.radial_blur_frames as f64 / total;
        // 序盤は強く（0.65）、末尾に向けて薄く（0.10）
        let alpha = 0.10 + progress * 0.55;
        ctx.save();
        ctx.set_global_alpha(alpha);
        let _ = ctx.translate(cx, cy);
        let _ = ctx.scale(scale, scale);
        let _ = ctx.translate(-cx, -cy);
        let _ = ctx.draw_image_with_html_canvas_element(canvas, 0.0, 0.0);
        ctx.restore();
    }

    // --- Lerp state ---
    // Pattern A 温色補正（上昇トレンド中は色相を暖色方向へシフト）
    let pattern_a_warm = if matches!(state.visual_pattern, VisualPattern::Accumulation) {
        state.rms_trend_timer.min(10.0) / 10.0 * 30.0
    } else {
        0.0
    };
    // ピッチ偏差を加味した色相ブレンド（0/360° 境界を短経路で補間）
    let blended_hue = (state.target_hue - pattern_a_warm + state.vocal_pitch_dev * 0.35).rem_euclid(360.0);
    let hue_delta = hue_diff_wrapped(blended_hue, state.hue);
    state.hue = (state.hue + hue_delta * 0.04).rem_euclid(360.0);
    lerp_to(&mut state.energy, state.target_energy, 0.03);
    lerp_to(&mut state.density, state.target_density, 0.03);

    // --- 1. Background ---
    ctx.clear_rect(0.0, 0.0, w, h);

    // Deep dark fill
    ctx.set_fill_style_str("#030712");
    ctx.fill_rect(0.0, 0.0, w, h);

    // --- SilenceFlash（Chorus 直前の白フラッシュ）: Playing のみ ---
    if matches!(state.pattern_b_state, PatternBState::SilenceFlash)
        && state.directive == AnimDirective::Playing {
        let flash_dur = if state.last_chorus_active {
            0.5 + state.last_chorus_intensity * 0.25
        } else {
            0.25
        };
        let flash_alpha = (1.0 - state.pattern_b_timer / flash_dur).max(0.0);
        ctx.set_fill_style_str(&format!("rgba(255,255,255,{:.2})", flash_alpha));
        ctx.fill_rect(0.0, 0.0, w, h);
    }

    // Chord-colored radial glow
    let grad = ctx
        .create_radial_gradient(cx, cy, 0.0, cx, cy, base_r * 2.5)
        .unwrap();
    let _ = grad.add_color_stop(0.0, &format!("hsla({:.0},60%,30%,0.18)", state.hue));
    let _ = grad.add_color_stop(1.0, "rgba(0,0,0,0)");
    ctx.set_fill_style_canvas_gradient(&grad);
    ctx.fill_rect(0.0, 0.0, w, h);

    // Charge グロウ（Anticipation 中）: Playing のみ
    if state.charge > 0.1 && matches!(state.pattern_b_state, PatternBState::Anticipation)
        && state.directive == AnimDirective::Playing {
        let alpha = (state.charge * 0.15 + state.drop_intensity * 0.15).min(0.30);
        let charge_grad = ctx
            .create_radial_gradient(cx, cy, 0.0, cx, cy, base_r * 1.5)
            .unwrap();
        let _ = charge_grad.add_color_stop(0.0, &format!("rgba(255,30,30,{:.2})", alpha));
        let _ = charge_grad.add_color_stop(1.0, "rgba(0,0,0,0)");
        ctx.set_fill_style_canvas_gradient(&charge_grad);
        ctx.fill_rect(0.0, 0.0, w, h);
    }

    // --- 2a. Soft Rings（Chorus内 RMS 下降時）---
    for ring in &state.soft_rings {
        let alpha = ring.life * ring.life * 0.5; // quadratic fade-out, max 0.5
        ctx.begin_path();
        let _ = ctx.arc(cx, cy, base_r + ring.radius, 0.0, 2.0 * PI);
        ctx.set_stroke_style_str(&format!(
            "hsla({:.0},70%,75%,{:.2})", ring.hue, alpha
        ));
        ctx.set_line_width(2.5 * ring.life);
        ctx.stroke();
    }

    // --- 2. Outer concentric rings (downbeat) ---
    let ring_scale = 1.0 + state.downbeat_pulse * 0.30 * sv.bass;
    for i in 0..2usize {
        let r = base_r * (1.4 + i as f64 * 0.35) * ring_scale;
        let alpha = 0.25 - i as f64 * 0.08 + state.downbeat_pulse * 0.15 * sv.bass;
        ctx.begin_path();
        let _ = ctx.arc(cx, cy, r, 0.0, 2.0 * PI);
        ctx.set_stroke_style_str(&format!(
            "hsla({:.0},60%,55%,{:.2})", state.hue, alpha.max(0.0)
        ));
        ctx.set_line_width(1.5 - i as f64 * 0.4);
        ctx.stroke();
    }

    // --- 3. Center polygon (beat + distortion) ---
    let beat_r = base_r * (1.0 + state.beat_pulse * 0.12 * sv.drums);
    let n_sides = 8usize;
    ctx.begin_path();
    for i in 0..=n_sides {
        let angle = 2.0 * PI * i as f64 / n_sides as f64 - PI / 2.0;
        // Add vertex-level distortion driven by vocals
        let distort = state.distortion_seed * sv.vocals;
        let wobble = if distort > 0.01 {
            let seed = (i as f64 * 137.508 + state.beat_pulse * 10.0).sin();
            seed * distort * base_r * 0.12
        } else {
            0.0
        };
        let r = beat_r + wobble;
        let x = cx + r * angle.cos();
        let y = cy + r * angle.sin();
        if i == 0 {
            ctx.move_to(x, y);
        } else {
            ctx.line_to(x, y);
        }
    }
    ctx.close_path();

    // Fill with chord color（wireframe モード時は透過）
    let fill_alpha = 1.0 - state.wireframe_alpha;
    if fill_alpha > 0.01 {
        let lightness = 45.0 + state.beat_pulse * 15.0 * sv.drums;
        ctx.set_global_alpha(fill_alpha);
        ctx.set_fill_style_str(&format!("hsl({:.0},65%,{:.0}%)", state.hue, lightness));
        ctx.fill();
        ctx.set_global_alpha(1.0);
    }

    // Glow stroke（wireframe モード時は輝度を上げる）
    ctx.set_stroke_style_str(&format!(
        "hsla({:.0},80%,75%,{:.2})",
        state.hue,
        (0.4 + state.beat_pulse * 0.4 * sv.drums) * (0.3 + state.wireframe_alpha * 0.7)
    ));
    ctx.set_line_width(2.0 + state.wireframe_alpha * 1.5 + state.charge * 3.0);
    ctx.stroke();

    // --- 4. Chorus→Chorus 八角形1回転エフェクト ---
    if state.octagon_spin_timer > 0.01 {
        // progress: 0→1（回転の進行度）、sin でフェードイン/アウト
        let progress_ratio = 1.0 - state.octagon_spin_timer; // 0→1
        let progress = (progress_ratio * std::f64::consts::PI).sin(); // 0→1→0
        let rotation = progress_ratio * 2.0 * std::f64::consts::PI; // 0→2π（1回転）
        let n = 8usize;
        let oct_hue = (state.hue + 60.0).rem_euclid(360.0); // 補色方向にシフト

        // 外側八角形（時計回り）
        let oct_r_outer = base_r * 1.6 * (1.0 + state.beat_pulse * 0.08 * sv.drums);
        ctx.begin_path();
        for i in 0..=n {
            let angle = 2.0 * PI * i as f64 / n as f64 + rotation;
            let x = cx + oct_r_outer * angle.cos();
            let y = cy + oct_r_outer * angle.sin();
            if i == 0 { ctx.move_to(x, y); } else { ctx.line_to(x, y); }
        }
        ctx.close_path();
        ctx.set_stroke_style_str(&format!(
            "hsla({:.0},80%,65%,{:.2})", oct_hue, progress * 0.75
        ));
        ctx.set_line_width(2.5 + state.beat_pulse * 2.0 * sv.drums);
        ctx.stroke();

        // 内側八角形（逆回転・遅め）
        let oct_r_inner = base_r * 1.1 * (1.0 + state.downbeat_pulse * 0.06 * sv.bass);
        ctx.begin_path();
        for i in 0..=n {
            let angle = 2.0 * PI * i as f64 / n as f64 - rotation * 0.7;
            let x = cx + oct_r_inner * angle.cos();
            let y = cy + oct_r_inner * angle.sin();
            if i == 0 { ctx.move_to(x, y); } else { ctx.line_to(x, y); }
        }
        ctx.close_path();
        ctx.set_stroke_style_str(&format!(
            "hsla({:.0},70%,75%,{:.2})", (oct_hue + 30.0).rem_euclid(360.0), progress * 0.5
        ));
        ctx.set_line_width(1.5);
        ctx.stroke();
    }

    // --- 5. Particles ---
    spawn_particles(state, cx, cy, base_r, w, h, sv);
    draw_particles(ctx, state, w, h, cx, cy);

    // --- 6. Chord name at center ---
    if !state.chord_label.is_empty() {
        let font_size = (base_r * 0.55).round() as u32;
        ctx.set_font(&format!("bold {}px sans-serif", font_size));
        ctx.set_text_align("center");
        ctx.set_text_baseline("middle");
        ctx.set_fill_style_str(&format!("hsla({:.0},60%,90%,0.85)", state.hue));
        let _ = ctx.fill_text(&state.chord_label, cx, cy);
    }

    // --- 7. Decay ---
    state.beat_pulse *= 0.85;
    state.downbeat_pulse *= 0.88;
    state.distortion_seed *= 0.80;
    if state.beat_pulse < 0.001 { state.beat_pulse = 0.0; }
    if state.downbeat_pulse < 0.001 { state.downbeat_pulse = 0.0; }
    if state.distortion_seed < 0.001 { state.distortion_seed = 0.0; }
}

fn spawn_particles(state: &mut AnimState, cx: f64, cy: f64, base_r: f64, w: f64, h: f64, sv: StemVolumes) {
    let max_particles = 300usize;

    // Idle モード: バースト系をすべてスキップし、最小限のアンビエント粒子のみ生成
    if state.directive == AnimDirective::Idle {
        let idle_count = ((state.density * 0.5).min(0.5) * 2.0).round() as usize;
        for _ in 0..idle_count {
            if state.particles.len() >= max_particles { break; }
            let angle = state.rand_range(0.0, 2.0 * std::f64::consts::PI);
            let spawn_r = base_r * state.rand_range(0.9, 1.1);
            let speed = state.rand_range(0.3, 1.0);
            let size = state.rand_range(1.5, 3.5);
            let hue_off = state.rand_range(-30.0, 30.0);
            let cur_hue = state.hue;
            let decay = state.rand_range(0.008, 0.015);
            let friction = state.rand_range(0.93, 0.98);
            state.particles.push(Particle {
                x: cx + angle.cos() * spawn_r,
                y: cy + angle.sin() * spawn_r,
                vx: angle.cos() * speed,
                vy: angle.sin() * speed,
                life: 1.0,
                decay: decay,
                friction: friction,
                hue: (cur_hue + hue_off).rem_euclid(360.0),
                size,
                fade_in: 0.0,
            });
        }
        return;
    }

    // Beat burst: on every beat_pulse the count spikes, tapers between beats.
    // Downbeat emits extra radial spray scaled by bass volume.
    let beat_burst   = (state.beat_pulse * 20.0 * sv.drums).round() as usize;
    let db_burst     = (state.downbeat_pulse * 12.0 * sv.bass).round() as usize;
    // Ambient trickle proportional to density (few particles between beats)
    let ambient      = (state.density * sv.others * state.energy * 1.5).round() as usize;

    // Pattern A: 粒子数を RMS² でスケール
    let rms_sq_scale = if matches!(state.visual_pattern, VisualPattern::Accumulation) {
        (state.rms_current * state.rms_current * 4.0).min(3.0)
    } else {
        1.0
    };
    let spawn_count = ((beat_burst + db_burst + ambient) as f64
        * rms_sq_scale * state.particle_count_scale) as usize;

    // Pattern B Impact: 爆発バースト（1 フレームのみ、落差スケール適用）
    if state.impact_burst {
        state.impact_burst = false;
        // drop_intensity（落差の深さ）と charge_peak（溜め時間）の大きい方でスケール
        let scale = state.drop_intensity.max(state.charge_peak * 0.8);
        let burst_count = (120.0 + scale * 180.0) as usize; // 120〜300粒子
        let speed_max = 40.0 + scale * 20.0;                // 40〜60
        let size_base = 5.0 + scale * 5.0;                  // 5〜10px
        let splash_hue = (state.hue + 180.0).rem_euclid(360.0);
        web_sys::console::log_1(&format!(
            "[BURST] count={} speed_max={:.1} drop={:.0}% charge_peak={:.0}% scale={:.0}%",
            burst_count, speed_max,
            state.drop_intensity * 100.0,
            state.charge_peak * 100.0,
            scale * 100.0
        ).into());
        // life > 0.5 のもの以外を削除してスペースを確保（burst が確実にスポーンできるように）
        state.particles.retain(|p| p.life > 0.5);
        for _ in 0..burst_count {
            let a = state.rand_range(0.0, 2.0 * PI);
            let spd = state.rand_range(20.0, speed_max);
            let sz_r = state.rand_range(0.6, 1.4);
            let hue_r = state.rand_range(-30.0, 30.0);
            state.particles.push(Particle {
                x: cx, y: cy,
                vx: a.cos() * spd, vy: a.sin() * spd,
                life: 1.0, decay: 0.005, friction: 0.96,
                hue: (splash_hue + hue_r).rem_euclid(360.0),
                size: size_base * sz_r, fade_in: 0.0,
            });
        }
        state.charge = 0.0;
        state.charge_peak = 0.0;
    }

    // Verse DIP_RECOVER: 画面上から下へ降り注ぐパーティクル
    if state.rain_burst {
        state.rain_burst = false;
        let count = (80.0 + state.rain_intensity * 160.0) as usize; // 80〜240粒子
        let rain_hue = state.hue;
        for _ in 0..count {
            let x    = state.rand_range(0.0, w);
            let y    = state.rand_range(0.0, h);
            let vy   = state.rand_range(4.0, 10.0 + state.rain_intensity * 8.0);
            let vx   = state.rand_range(-1.0, 1.0);
            let sz   = state.rand_range(2.0, 5.0 + state.rain_intensity * 3.0);
            let hue_r = state.rand_range(-30.0, 30.0);
            let hue  = (rain_hue + hue_r).rem_euclid(360.0);
            state.particles.push(Particle {
                x, y, vx, vy,
                life: 1.0,
                decay: 0.006,
                friction: 0.99,
                hue,
                size: sz,
                fade_in: 1.0, // 雨パーティクルはフェードイン
            });
        }
    }

    // Chorus→Chorus 八角形スパイラルアーム
    // 各頂点から「接線方向（反時計回り）+ 外向き」の初速でパーティクルを放出し
    // 弧を描く軌跡を作る。
    if state.octagon_spin_timer > 0.01 {
        let progress_ratio = 1.0 - state.octagon_spin_timer; // 0→1
        let rotation = progress_ratio * 2.0 * PI;
        let presence = (progress_ratio * PI).sin(); // 0→1→0（フェードイン/アウト）
        let arm_hue = (state.hue + 60.0).rem_euclid(360.0);

        for i in 0..8usize {
            if state.particles.len() >= max_particles { break; }
            let angle = 2.0 * PI * i as f64 / 8.0 + rotation;
            let r = base_r * state.rand_range(1.7, 1.9);
            let x = cx + r * angle.cos();
            let y = cy + r * angle.sin();

            // 反時計回り接線（canvas 座標、y 下向き）: (sin θ, -cos θ)
            // 外向き成分: (cos θ, sin θ)
            let tan_spd = 20.0 * presence;
            let rad_spd = 5.0;
            let vx = angle.cos() * rad_spd + angle.sin() * tan_spd;
            let vy = angle.sin() * rad_spd - angle.cos() * tan_spd;
            let speed_var = state.rand_range(0.8, 1.2);
            let hue_off = state.rand_range(-20.0, 20.0);
            let size = state.rand_range(3.0, 5.5);

            state.particles.push(Particle {
                x, y,
                vx: vx * speed_var,
                vy: vy * speed_var,
                life: 1.0,
                decay: 0.014,
                friction: 0.97,
                hue: (arm_hue + hue_off).rem_euclid(360.0),
                size, fade_in: 0.0,
            });
        }
    }

    if spawn_count == 0 { return; }

    // Pattern B Anticipation: 収束方向（内向き）
    let inward = matches!(state.pattern_b_state, PatternBState::Anticipation);
    let dir = if inward { -1.0 } else { 1.0 };

    for _ in 0..spawn_count {
        if state.particles.len() >= max_particles { break; }

        let angle = state.rand_range(0.0, 2.0 * PI);

        // Emit from polygon edge (base_r ± 10%) so particles clearly fly outward
        let spawn_r = base_r * state.rand_range(0.9, 1.1);
        // Speed: faster on beat/downbeat burst, slower for ambient
        let speed_base = if beat_burst > 0 || db_burst > 0 { 4.0 } else { 1.5 };
        let speed = state.rand_range(speed_base, speed_base * 2.0)
            * (0.6 + state.energy * 0.8)
            * (0.5 + sv.others * 0.5);

        let size = state.rand_range(2.0, 5.5) * (0.6 + state.energy * 0.5);
        let hue_offset = state.rand_range(-40.0, 40.0);
        // Per-particle decay so lifetimes vary (0.012..0.025 → ~40-80 frames ≈ 0.7-1.3s)
        let decay = state.rand_range(0.012, 0.025);
        let friction = state.rand_range(0.91, 0.97);
        let cur_hue = state.hue;

        state.particles.push(Particle {
            x: cx + angle.cos() * spawn_r,
            y: cy + angle.sin() * spawn_r,
            vx: angle.cos() * speed * dir,
            vy: angle.sin() * speed * dir,
            life: 1.0,
            decay,
            friction,
            hue: (cur_hue + hue_offset).rem_euclid(360.0),
            size, fade_in: 0.0,
        });
    }
}

fn draw_particles(ctx: &CanvasRenderingContext2d, state: &mut AnimState, w: f64, h: f64, cx: f64, cy: f64) {
    let high_rms = state.rms_current > 0.15;
    // 高 RMS 時は加算合成でブルーム感を出す。パーティクルが 0 個でも後でリセット必須。
    if high_rms { let _ = ctx.set_global_composite_operation("lighter"); }

    let spiral = state.spiral_particles;
    let anticipation = matches!(state.pattern_b_state, PatternBState::Anticipation);
    // octagon_spin_timer: 1.0→0.0, sin でフェードイン/アウト
    let c2c_vortex = (std::f64::consts::PI * (1.0 - state.octagon_spin_timer)).sin()
        * state.octagon_spin_timer.min(1.0);
    let rms_for_lightness = state.rms_current;

    state.particles.retain_mut(|p| {
        // Pattern C: 接線方向加速度（螺旋運動）
        if spiral {
            let dx = p.x - cx;
            let dy = p.y - cy;
            let dist = (dx * dx + dy * dy).sqrt().max(1.0);
            p.vx += (-dy / dist) * 0.08;
            p.vy += (dx / dist) * 0.08;
        }
        // Chorus→Chorus: 反時計回り渦（八角形と逆方向）
        if c2c_vortex > 0.01 {
            let dx = p.x - cx;
            let dy = p.y - cy;
            let dist = (dx * dx + dy * dy).sqrt().max(1.0);
            // 反時計回り接線: (dy, -dx)/dist
            let strength = c2c_vortex * 0.05;
            p.vx += (dy / dist) * strength;
            p.vy += (-dx / dist) * strength;
        }
        // Pattern B Anticipation: 中心引力（charge に応じて強化）
        if anticipation {
            let pull = 0.03 + state.charge * 0.12; // charge=1.0 で 0.15（3倍）
            p.vx += (cx - p.x).signum() * pull;
            p.vy += (cy - p.y).signum() * pull;
        }

        p.vx *= p.friction;
        p.vy *= p.friction;
        p.x += p.vx;
        p.y += p.vy;
        p.fade_in = (p.fade_in - 0.03).max(0.0); // ~33フレームでフェードイン完了
        p.life -= p.decay;

        if p.life <= 0.0 || p.x < -20.0 || p.x > w + 20.0 || p.y < -20.0 || p.y > h + 20.0 {
            return false;
        }

        let base_alpha = (p.life * p.life * 0.9).min(0.9) * (1.0 - p.fade_in); // quadratic fade-out + fade-in
        // 低/高 RMS で輝度・アルファを調整
        let (lightness, alpha) = if rms_for_lightness < 0.03 {
            let boost = (1.0 - rms_for_lightness / 0.03) * 25.0;
            (72.0 + boost, base_alpha)
        } else if high_rms {
            let reduce = ((rms_for_lightness - 0.15) / 0.15).min(1.0) * 0.5;
            (72.0, base_alpha * (1.0 - reduce * 0.6))
        } else {
            (72.0, base_alpha)
        };

        ctx.begin_path();
        let _ = ctx.arc(p.x, p.y, (p.size * p.life).max(0.5), 0.0, 2.0 * PI);
        ctx.set_fill_style_str(&format!("hsla({:.0},85%,{:.0}%,{:.2})", p.hue, lightness, alpha));
        ctx.fill();
        true
    });

    // 加算合成を元に戻す（パーティクル 0 個でも必ず実行）
    let _ = ctx.set_global_composite_operation("source-over");
}

// ---------------------------------------------------------------------------
// パターン状態機械
// ---------------------------------------------------------------------------

fn is_chorus_label(l: &str) -> bool {
    l == "chorus" || l == "refrain"
}

fn update_pattern_state(s: &mut AnimState, dt: f64) {
    let cur = s.current_section.to_lowercase();
    let nxt = s.next_section.to_lowercase();

    let cur_is_chorus = is_chorus_label(&cur);
    let nxt_is_chorus = is_chorus_label(&nxt);
    let cur_is_bridge = cur == "bridge" || cur.contains("inst");

    // ── Pattern C: Bridge + 低 RMS ───────────────────────────────────────────
    let drop_active = cur_is_bridge && s.rms_current < 0.05;
    s.spiral_particles = drop_active;
    if drop_active {
        s.visual_pattern = VisualPattern::Drop;
    } else if s.visual_pattern == VisualPattern::Drop {
        s.visual_pattern = VisualPattern::Normal;
    }
    // Drop 回復時ズームアウト（RMS が上がった瞬間だけ一時縮小）
    if cur_is_bridge && s.rms_current > 0.1 {
        s.base_r_scale = 0.85;
    }
    lerp_to(&mut s.base_r_scale, 1.0, 0.02);

    // ── Chorus 出口演出（Chorus → 非 Chorus、残り ≤3s）───────────────────────
    let chorus_exit = cur_is_chorus && !nxt_is_chorus && !nxt.is_empty() && s.secs_until_next < 3.0;
    let wire_target = if drop_active { 1.0 } else if chorus_exit { 0.5 } else { 0.0 };
    lerp_to(&mut s.wireframe_alpha, wire_target, 0.05);
    let count_target = if drop_active { 0.5 } else if chorus_exit { 0.7 } else { 1.0 };
    lerp_to(&mut s.particle_count_scale, count_target, 0.04);

    // ── Pattern A: 上昇トレンド ───────────────────────────────────────────────
    if s.energy_trend > 0.0005 {
        s.rms_trend_timer += dt;
    } else {
        s.rms_trend_timer = (s.rms_trend_timer - dt * 2.0).max(0.0);
    }
    if s.rms_trend_timer > 3.0 && s.visual_pattern == VisualPattern::Normal {
        s.visual_pattern = VisualPattern::Accumulation;
    }
    if s.rms_trend_timer < 1.0 && s.visual_pattern == VisualPattern::Accumulation {
        s.visual_pattern = VisualPattern::Normal;
    }

    // ── Pattern B: 「次セクション」ベースの収束→爆発 ─────────────────────────
    // 動的閾値: 曲全体の baseline_rms から 30% 以上の下降で「溜め」判定
    // Verse→Chorus のように盛り上がりながら入るケース（Pre-Chorus）では発動させない。
    // cur_is_chorus の場合は Pattern B を発火させない（Chorus内は Soft Rings で対応）
    let drop_ratio = s.rms_current / s.baseline_rms.max(0.001);
    let rms_is_dropping = drop_ratio < 0.70 || s.energy_trend < -0.001;
    let approaching_chorus = !cur_is_chorus && nxt_is_chorus && s.secs_until_next < 8.0 && rms_is_dropping;

    // Soft Rings: Chorus内で RMS が下降したときにリングをスポーン
    if cur_is_chorus && rms_is_dropping && s.pattern_b_state == PatternBState::Inactive {
        if s.soft_rings.len() < 6 {
            let ring_hue = (s.hue + 40.0).rem_euclid(360.0);
            s.soft_rings.push(SoftRing {
                radius: 0.0,
                speed: 3.0 + (1.0 - drop_ratio).clamp(0.0, 1.0) * 3.0,
                life: 1.0,
                hue: ring_hue,
            });
        }
    }
    // Soft Rings 更新
    s.soft_rings.retain_mut(|r| {
        r.radius += r.speed;
        r.life -= 0.012;
        r.life > 0.0
    });

    // ── RMS drop→recover バースト（セクション問わず）──────────────────────────
    // drop_ratio < 0.50（baseline の 50% 未満）でディップ中と判定。
    // ディップから脱出（drop_ratio > 0.75）したとき、最小落差に応じてバースト。
    const DIP_ENTER: f64 = 0.50; // ディップ開始閾値（baseline の 50% 未満）
    const DIP_EXIT:  f64 = 0.75; // ディップ終了閾値（baseline の 75% 以上に回復）
    let cur_is_verse = cur == "verse" || cur == "pre-chorus" || cur == "prechorus";
    if !s.in_rms_dip && drop_ratio < DIP_ENTER {
        s.in_rms_dip = true;
        s.dip_min_ratio = drop_ratio;
    } else if s.in_rms_dip {
        s.dip_min_ratio = s.dip_min_ratio.min(drop_ratio);
        if drop_ratio > DIP_EXIT {
            let dip_intensity = ((DIP_ENTER - s.dip_min_ratio) / DIP_ENTER).clamp(0.0, 1.0);
            if cur_is_verse {
                // Verse: 降り注ぎパーティクル（中央バーストなし）
                s.rain_burst = true;
                s.rain_intensity = dip_intensity;
                s.radial_blur_frames = (8.0 + dip_intensity * 20.0) as u32; // 短めブルーム
                s.radial_blur_scale = 1.02 + dip_intensity * 0.02;
                web_sys::console::log_1(&format!(
                    "[DIP_RECOVER/VERSE] dip={:.0}% rain intensity={:.0}%",
                    dip_intensity * 100.0, dip_intensity * 100.0
                ).into());
            } else if cur_is_bridge {
                // Bridge: スパイラルのみ（中央バーストなし）
                if dip_intensity >= 0.3 {
                    s.octagon_spin_timer = 1.0 + (dip_intensity - 0.3) * 1.5;
                }
                s.radial_blur_frames = (10.0 + dip_intensity * 30.0) as u32;
                s.radial_blur_scale = 1.03 + dip_intensity * 0.03;
                web_sys::console::log_1(&format!(
                    "[DIP_RECOVER/BRIDGE] dip={:.0}% spin={:.1}s",
                    dip_intensity * 100.0,
                    if dip_intensity >= 0.3 { 1.0 + (dip_intensity - 0.3) * 1.5 } else { 0.0 }
                ).into());
            } else {
                // Chorus内・Intro/Outro等: 爆発なし（爆発はChorus遷移時の Pattern B のみ）
                web_sys::console::log_1(&format!(
                    "[DIP_RECOVER/SKIP] section={} dip={:.0}%",
                    cur, dip_intensity * 100.0
                ).into());
            }
            s.in_rms_dip = false;
            s.dip_min_ratio = 1.0;
        }
    }

    // charge 蓄積・減衰（Anticipation 中のみ増加）
    if matches!(s.pattern_b_state, PatternBState::Anticipation) {
        let charge_rate = (1.0 - s.charge) * 0.3 * dt;
        s.charge = (s.charge + charge_rate).min(1.0);
        s.charge_peak = s.charge_peak.max(s.charge);
        // drop_intensity は Anticipation 中も最大値でトラッキング
        let cur_drop_intensity = ((0.70 - drop_ratio) / 0.70).clamp(0.0, 1.0);
        s.drop_intensity = s.drop_intensity.max(cur_drop_intensity);
        // デバッグ: Anticipation 中は毎秒ログ済みの代わりにこちらで出力
        web_sys::console::log_1(&format!(
            "[CHARGE] state=Anticipation charge={:.0}% peak={:.0}% drop={:.0}% count={}回 bloom={}f",
            s.charge * 100.0, s.charge_peak * 100.0, s.drop_intensity * 100.0,
            s.charge_count,
            {
                let sc = s.drop_intensity.max(s.charge_peak * 0.8);
                let cm = ((s.charge_count as f64 + 1.0).log2()).max(1.0);
                (18.0 + sc * 100.0 * cm).min(180.0) as u32
            }
        ).into());
    } else if matches!(s.pattern_b_state, PatternBState::Inactive) {
        s.charge *= 0.95; // キャンセル時は徐々に減衰
    }

    // Impact 遷移時のヘルパークロージャ（inline で radial_blur_scale を設定）
    let fire_impact = |s: &mut AnimState| {
        s.pattern_b_state = PatternBState::Impact;
        s.pattern_b_timer = 0.0;
        // ブルーム長: 落差1%につき1フレーム × チャージ回数の対数倍（基底18f）
        // charge_count: 1回→×1.0、2回→×1.58、3回→×2.0、4回→×2.32（指数的知覚に対応）
        let bloom_scale = s.drop_intensity.max(s.charge_peak * 0.8);
        let count_multiplier = ((s.charge_count as f64 + 1.0).log2()).max(1.0);
        let bloom_frames = 18.0 + bloom_scale * 100.0 * count_multiplier;
        s.radial_blur_frames = bloom_frames.min(180.0) as u32; // 上限180f（3s）
        s.radial_blur_scale = 1.04 + bloom_scale * 0.04;       // 1.04〜1.08
        // ラスサビ: ブルームをさらに延長（強度0.5→1.25倍、強度1.0→1.75倍）
        if s.last_chorus_active {
            let mult = 1.0 + s.last_chorus_intensity * 0.75;
            s.radial_blur_frames = (s.radial_blur_frames as f64 * mult).min(180.0) as u32;
        }
        s.impact_burst = true;
        // 落差50%以上で回転スパイラルアームも発動（落差に応じて持続時間を延長）
        // 50%→1.0s、75%→1.5s、100%→2.0s
        if s.drop_intensity >= 0.5 {
            s.octagon_spin_timer = 1.0 + (s.drop_intensity - 0.5) * 2.0;
        }
    };

    match s.pattern_b_state {
        PatternBState::Inactive => {
            if approaching_chorus {
                // RMS が下降中: drop_intensity を確定して Anticipation へ
                s.drop_intensity = ((0.70 - drop_ratio) / 0.70).clamp(0.0, 1.0);
                s.charge_count += 1;
                s.pattern_b_state = PatternBState::Anticipation;
                s.pattern_b_timer = 0.0;
                s.visual_pattern = VisualPattern::Contrast;
            } else if nxt_is_chorus && !cur_is_chorus && s.secs_until_next < 0.5 {
                // RMS が下がっていなくてもコーラス直前には必ずフラッシュ
                // ただし Chorus→Chorus の場合はフラッシュしない
                // drop_intensity は実際の落差のみ（スパイラルは発動させない）
                s.drop_intensity = ((0.70 - drop_ratio) / 0.70).clamp(0.0, 0.49);
                s.pattern_b_state = PatternBState::SilenceFlash;
                s.pattern_b_timer = 0.0;
            }
        }
        PatternBState::Anticipation => {
            s.pattern_b_timer += dt;
            if cur_is_chorus {
                fire_impact(s);
            } else if s.secs_until_next < 0.5 {
                if !cur_is_chorus {
                    // 非コーラス→コーラス: フラッシュ経由
                    s.pattern_b_state = PatternBState::SilenceFlash;
                    s.pattern_b_timer = 0.0;
                } else {
                    // Chorus→Chorus: フラッシュなし、直接インパクト
                    fire_impact(s);
                }
            } else if !approaching_chorus {
                s.pattern_b_state = PatternBState::Inactive;
                s.visual_pattern = VisualPattern::Normal;
            }
        }
        PatternBState::SilenceFlash => {
            s.pattern_b_timer += dt;
            // ラスサビはフラッシュを長く（0.5〜0.75s）、通常は 0.25s
            let flash_dur = if s.last_chorus_active {
                0.5 + s.last_chorus_intensity * 0.25
            } else {
                0.25
            };
            if cur_is_chorus || s.pattern_b_timer >= flash_dur {
                fire_impact(s);
            }
        }
        PatternBState::Impact => {
            s.pattern_b_timer += dt;
            if s.pattern_b_timer >= 0.5 {
                s.pattern_b_state = PatternBState::Inactive;
                s.visual_pattern = VisualPattern::Normal;
                s.drop_intensity = 0.0;
            }
        }
    }

    if s.radial_blur_frames > 0 {
        s.radial_blur_frames -= 1;
    }

    // ── Chorus→Chorus 境界検知（八角形1回転エフェクト）────────────────────────────
    // 同ラベル（"chorus"→"chorus"）ではシグナルが変化しないため、
    // secs_until_next のジャンプ（0付近→大値）で境界を検知する。
    let secs_jumped = s.secs_until_next > s.last_secs_until_next + 5.0;
    if secs_jumped && cur_is_chorus && s.prev_cur_was_chorus {
        // Chorus→Chorus 境界: 1回転エフェクト開始
        s.octagon_spin_timer = 1.0;
    }
    s.last_secs_until_next = s.secs_until_next;
    s.prev_cur_was_chorus = cur_is_chorus;

    // 1回転アニメーション更新（timer 1.0→0.0 で rotation 0→2π）
    if s.octagon_spin_timer > 0.0 {
        s.octagon_spin_timer = (s.octagon_spin_timer - dt).max(0.0);
    }

    // ── ラスサビ判定 ────────────────────────────────────────────────────────────
    // cur が Chorus かつ次が Outro/End 系 or 空 → ラスサビ
    let is_finale = |sec: &str| -> bool {
        sec.is_empty() || sec.contains("outro") || sec.contains("end") || sec.contains("coda")
    };
    s.last_chorus_active = cur_is_chorus && !nxt_is_chorus && is_finale(&nxt);
    if s.last_chorus_active {
        // 強度: 基本 0.5 + 直近落差スケール（大落差ほど強く）
        s.last_chorus_intensity = (0.5 + s.drop_intensity * 0.5).min(1.0);
    } else {
        s.last_chorus_intensity = 0.0;
    }
}

#[inline]
fn lerp_to(current: &mut f64, target: f64, t: f64) {
    *current += (target - *current) * t;
}

/// N 区間で 0.5s 後から立ち上がるアニメーション色相ドリフト。
/// 2つの周波数を重ねることで機械的でない揺らぎを作る。
fn n_section_drift(n_elapsed: f64) -> f64 {
    if n_elapsed < 0.5 {
        return 0.0;
    }
    // 0.5s〜1.0s で 0 → 1.0 にランプアップ（出力は osc * 25.0 で ±25° スケール）
    let ramp = ((n_elapsed - 0.5) * 2.0).clamp(0.0, 1.0);
    let osc = f64::sin(n_elapsed * 0.9) * 0.55 + f64::sin(n_elapsed * 2.1) * 0.45;
    osc * 25.0 * ramp
}

/// 色相の符号付き最短経路差分（±180° 以内に正規化）
#[inline]
fn hue_diff_wrapped(target: f64, current: f64) -> f64 {
    let d = (target - current).rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

/// 時間領域サンプルから自己相関法で基本周波数（Hz）を推定する。
/// 無音（RMS < 0.01）や推定失敗時は None を返す。
fn autocorrelation_pitch(samples: &[f32], sample_rate: f32) -> Option<f32> {
    let n = samples.len();
    let rms: f32 = (samples.iter().map(|&s| s * s).sum::<f32>() / n as f32).sqrt();
    if rms < 0.01 {
        return None;
    }

    // ボーカル音域: 80 Hz〜1200 Hz
    let tau_min = (sample_rate / 1200.0) as usize;
    let tau_max = ((sample_rate / 80.0) as usize).min(n / 2);
    if tau_min >= tau_max {
        return None;
    }

    let (best_tau, best_val) = (tau_min..tau_max)
        .map(|tau| {
            let corr: f32 = samples[..n - tau]
                .iter()
                .zip(&samples[tau..])
                .map(|(&a, &b)| a * b)
                .sum();
            (tau, corr)
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?;

    if best_val <= 0.0 {
        return None;
    }
    Some(sample_rate / best_tau as f32)
}

/// Hz → 音高に基づく色相偏差（±20°）
fn hz_to_hue_deviation(hz: f32) -> f64 {
    let semitone = 12.0 * (hz / 440.0_f32).log2();
    let t = (semitone.rem_euclid(12.0) / 12.0) as f64; // 0.0〜1.0
    (t * 2.0 * std::f64::consts::PI).sin() * 20.0
}
