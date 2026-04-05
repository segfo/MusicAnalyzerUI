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
// Internal animation state (lives inside the rAF closure)
// ---------------------------------------------------------------------------

struct Particle {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    life: f64,     // 0.0 → 1.0 (1.0 = just spawned, 0.0 = dead)
    decay: f64,    // per-frame life reduction (varies per particle)
    friction: f64, // velocity multiplier per frame (0.92 = decelerates)
    hue: f64,
    size: f64,
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

        // N 経過時間を更新（再生中かつ N 区間のみ加算、それ以外はリセット）
        {
            let mut s = state.borrow_mut();
            if s.chord_unknown && is_playing.get_untracked() {
                s.n_elapsed += 1.0 / 60.0;
            } else {
                s.n_elapsed = 0.0;
            }
        }

        // ピッチ偏差 / コードアンティシペーション / ドリフト計算（N 区間のみ適用）
        {
            let (chord_unknown, vocals_available, next_hue, target, completion, n_elapsed) = {
                let s = state.borrow();
                (s.chord_unknown, s.vocals_available, s.next_key_hue, s.target_hue,
                 s.chord_completion, s.n_elapsed)
            };
            let dev = if !chord_unknown {
                // コード検出済み → 偏差なし
                0.0
            } else {
                // N 区間: 0.5s 後から立ち上がるアニメドリフト
                let drift = n_section_drift(n_elapsed);

                if vocals_available {
                    // ボーカルあり: リアルタイムピッチ + drift
                    let pitch = stem_engine_sv.with_value(|se| {
                        let se = se.as_ref()?;
                        let data = se.get_vocal_time_domain_data()?;
                        let sr = se.sample_rate();
                        Some(autocorrelation_pitch(&data, sr).map(hz_to_hue_deviation).unwrap_or(0.0))
                    }).unwrap_or(0.0);
                    pitch + drift
                } else {
                    // ボーカルなし: アンティシペーション（0.5s 未満）→ drift（0.5s 以上）
                    let ramp = ((n_elapsed - 0.5) * 2.0).clamp(0.0, 1.0);
                    let anticipation = {
                        let diff = hue_diff_wrapped(next_hue, target);
                        diff * completion.min(0.8) * 0.30 * (1.0 - ramp)
                    };
                    anticipation + drift
                }
            };
            state.borrow_mut().vocal_pitch_dev = dev;
        }

        if let Ok(ctx) = canvas
            .get_context("2d")
            .ok()
            .flatten()
            .and_then(|o| o.dyn_into::<CanvasRenderingContext2d>().ok())
            .ok_or(())
        {
            let sv = stem_volumes.get_untracked();
            render_frame(&ctx, &mut state.borrow_mut(), w as f64, h as f64, sv);
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
    state: &mut AnimState,
    w: f64,
    h: f64,
    sv: StemVolumes,
) {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let base_r = h.min(w) * 0.18;

    // --- Lerp state ---
    // ピッチ偏差を加味した色相ブレンド（0/360° 境界を短経路で補間）
    let blended_hue = (state.target_hue + state.vocal_pitch_dev * 0.35).rem_euclid(360.0);
    let hue_delta = hue_diff_wrapped(blended_hue, state.hue);
    state.hue = (state.hue + hue_delta * 0.04).rem_euclid(360.0);
    lerp_to(&mut state.energy, state.target_energy, 0.03);
    lerp_to(&mut state.density, state.target_density, 0.03);

    // --- 1. Background ---
    ctx.clear_rect(0.0, 0.0, w, h);

    // Deep dark fill
    ctx.set_fill_style_str("#030712");
    ctx.fill_rect(0.0, 0.0, w, h);

    // Chord-colored radial glow
    let grad = ctx
        .create_radial_gradient(cx, cy, 0.0, cx, cy, base_r * 2.5)
        .unwrap();
    let _ = grad.add_color_stop(0.0, &format!("hsla({:.0},60%,30%,0.18)", state.hue));
    let _ = grad.add_color_stop(1.0, "rgba(0,0,0,0)");
    ctx.set_fill_style_canvas_gradient(&grad);
    ctx.fill_rect(0.0, 0.0, w, h);

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

    // Fill with chord color
    let lightness = 45.0 + state.beat_pulse * 15.0 * sv.drums;
    ctx.set_fill_style_str(&format!(
        "hsl({:.0},65%,{:.0}%)", state.hue, lightness
    ));
    ctx.fill();

    // Glow stroke
    ctx.set_stroke_style_str(&format!(
        "hsla({:.0},80%,75%,{:.2})",
        state.hue,
        0.4 + state.beat_pulse * 0.4 * sv.drums
    ));
    ctx.set_line_width(2.0);
    ctx.stroke();

    // --- 4. Particles ---
    spawn_particles(state, cx, cy, base_r, sv);
    draw_particles(ctx, state, w, h);

    // --- 5. Chord name at center ---
    if !state.chord_label.is_empty() {
        let font_size = (base_r * 0.55).round() as u32;
        ctx.set_font(&format!("bold {}px sans-serif", font_size));
        ctx.set_text_align("center");
        ctx.set_text_baseline("middle");
        ctx.set_fill_style_str(&format!("hsla({:.0},60%,90%,0.85)", state.hue));
        let _ = ctx.fill_text(&state.chord_label, cx, cy);
    }

    // --- 6. Decay ---
    state.beat_pulse *= 0.85;
    state.downbeat_pulse *= 0.88;
    state.distortion_seed *= 0.80;
    if state.beat_pulse < 0.001 { state.beat_pulse = 0.0; }
    if state.downbeat_pulse < 0.001 { state.downbeat_pulse = 0.0; }
    if state.distortion_seed < 0.001 { state.distortion_seed = 0.0; }
}

fn spawn_particles(state: &mut AnimState, cx: f64, cy: f64, base_r: f64, sv: StemVolumes) {
    let max_particles = 300usize;

    // Beat burst: on every beat_pulse the count spikes, tapers between beats.
    // Downbeat emits extra radial spray scaled by bass volume.
    let beat_burst   = (state.beat_pulse * 20.0 * sv.drums).round() as usize;
    let db_burst     = (state.downbeat_pulse * 12.0 * sv.bass).round() as usize;
    // Ambient trickle proportional to density (few particles between beats)
    let ambient      = (state.density * sv.others * state.energy * 1.5).round() as usize;

    let spawn_count = beat_burst + db_burst + ambient;
    if spawn_count == 0 { return; }

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
            vx: angle.cos() * speed,
            vy: angle.sin() * speed,
            life: 1.0,
            decay,
            friction,
            hue: (cur_hue + hue_offset).rem_euclid(360.0),
            size,
        });
    }
}

fn draw_particles(ctx: &CanvasRenderingContext2d, state: &mut AnimState, w: f64, h: f64) {
    state.particles.retain_mut(|p| {
        p.vx *= p.friction;
        p.vy *= p.friction;
        p.x += p.vx;
        p.y += p.vy;
        p.life -= p.decay;

        if p.life <= 0.0 || p.x < -20.0 || p.x > w + 20.0 || p.y < -20.0 || p.y > h + 20.0 {
            return false;
        }

        let alpha = (p.life * p.life * 0.9).min(0.9); // quadratic fade-out
        ctx.begin_path();
        let _ = ctx.arc(p.x, p.y, (p.size * p.life).max(0.5), 0.0, 2.0 * PI);
        ctx.set_fill_style_str(&format!(
            "hsla({:.0},85%,72%,{:.2})", p.hue, alpha
        ));
        ctx.fill();
        true
    });
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
    // 0.5s〜1.0s で 0 → 25.0 にランプアップ
    let ramp = ((n_elapsed - 0.5) * 2.0).min(25.0);
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
