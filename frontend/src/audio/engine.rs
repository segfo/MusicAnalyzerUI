use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AnalyserNode, AudioBuffer, AudioBufferSourceNode, AudioContext, AudioScheduledSourceNode, GainNode};

// ---------------------------------------------------------------------------
// StemVolumes — per-stem visual/audio intensity (0.0–1.0)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct StemVolumes {
    pub vocals: f64,
    pub drums: f64,
    pub bass: f64,
    pub others: f64,
}

impl Default for StemVolumes {
    fn default() -> Self {
        Self { vocals: 1.0, drums: 1.0, bass: 1.0, others: 1.0 }
    }
}

// ---------------------------------------------------------------------------
// StemGains — GainNodes for individual stem volume control
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct StemGains {
    pub vocals: GainNode,
    pub drums: GainNode,
    pub bass: GainNode,
    pub others: GainNode,
}

// ---------------------------------------------------------------------------
// AudioEngine — wraps Web Audio API for sample-accurate seeking
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AudioEngine {
    ctx: AudioContext,
    buffer: Rc<AudioBuffer>,
    gain: GainNode,
    state: Rc<RefCell<EngineState>>,
}

struct EngineState {
    source: Option<AudioBufferSourceNode>,
    offset_at_start: f64,   // buffer seconds when play() was called
    ctx_time_at_start: f64, // AudioContext.currentTime at that moment
    playing: bool,
}

impl AudioEngine {
    pub fn new(ctx: AudioContext, buffer: AudioBuffer) -> Result<Self, String> {
        let gain = ctx.create_gain().map_err(|e| format!("{e:?}"))?;
        gain.connect_with_audio_node(&ctx.destination())
            .map_err(|e| format!("{e:?}"))?;
        Ok(Self {
            ctx,
            buffer: Rc::new(buffer),
            gain,
            state: Rc::new(RefCell::new(EngineState {
                source: None,
                offset_at_start: 0.0,
                ctx_time_at_start: 0.0,
                playing: false,
            })),
        })
    }

    pub fn duration(&self) -> f64 {
        self.buffer.duration()
    }

    pub fn current_time(&self) -> f64 {
        let state = self.state.borrow();
        if state.playing {
            let elapsed = self.ctx.current_time() - state.ctx_time_at_start;
            (state.offset_at_start + elapsed).min(self.buffer.duration())
        } else {
            state.offset_at_start
        }
    }

    pub fn is_playing(&self) -> bool {
        self.state.borrow().playing
    }

    /// Resume the AudioContext (needed after browser autoplay policy suspends it).
    pub async fn resume_ctx(&self) {
        if self.ctx.state() != web_sys::AudioContextState::Running {
            if let Ok(promise) = self.ctx.resume() {
                let _ = JsFuture::from(promise).await;
            }
        }
    }

    pub fn play(&self) {
        if !self.state.borrow().playing {
            let current = self.current_time();
            let duration = self.duration();
            // 再生残り秒数が 1 秒未満（曲が終了している）場合は先頭から再生
            let offset = if duration - current < 1.0 { 0.0 } else { current };
            self.play_from(offset);
        }
    }

    pub fn pause(&self) {
        let mut state = self.state.borrow_mut();
        if state.playing {
            if let Some(src) = state.source.take() {
                let _ = AudioScheduledSourceNode::stop_with_when(&src, 0.0);
            }
            let elapsed = self.ctx.current_time() - state.ctx_time_at_start;
            state.offset_at_start =
                (state.offset_at_start + elapsed).min(self.buffer.duration());
            state.playing = false;
        }
    }

    /// Seek to an exact time in seconds. Restarts playback if currently playing.
    pub fn seek(&self, time: f64) {
        let time = time.clamp(0.0, self.buffer.duration());
        let playing = self.state.borrow().playing;
        if playing {
            self.play_from(time);
        } else {
            self.state.borrow_mut().offset_at_start = time;
        }
    }

    pub fn set_volume(&self, vol: f64) {
        self.gain.gain().set_value(vol as f32);
    }

    fn play_from(&self, offset: f64) {
        let offset = offset.clamp(0.0, self.buffer.duration());

        // Stop any existing source
        {
            let mut state = self.state.borrow_mut();
            if let Some(src) = state.source.take() {
                let _ = AudioScheduledSourceNode::stop_with_when(&src, 0.0);
            }
        }

        let Ok(src) = self.ctx.create_buffer_source() else { return };
        src.set_buffer(Some(&self.buffer));
        let _ = src.connect_with_audio_node(&self.gain);
        // start(when=0 → now, grain_offset=offset → start position in seconds)
        let _ = src.start_with_when_and_grain_offset(0.0, offset);

        let mut state = self.state.borrow_mut();
        state.ctx_time_at_start = self.ctx.current_time();
        state.offset_at_start = offset;
        state.playing = true;
        state.source = Some(src);
    }
}

// ---------------------------------------------------------------------------
// StemAudioEngine — plays 4 stems in sync on a shared AudioContext
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct StemAudioEngine {
    ctx: AudioContext,
    buffers: Rc<[Option<AudioBuffer>; 4]>,
    gains: [GainNode; 4],
    state: Rc<RefCell<StemEngineState>>,
    analyser: Option<AnalyserNode>,
}

struct StemEngineState {
    sources: [Option<AudioBufferSourceNode>; 4],
    offset_at_start: f64,
    ctx_time_at_start: f64,
    playing: bool,
    duration: f64,
}

impl StemAudioEngine {
    pub fn new(ctx: AudioContext, buffers: [Option<AudioBuffer>; 4], gains: [GainNode; 4]) -> Self {
        let duration = buffers.iter().flatten().map(|b| b.duration()).fold(0.0_f64, f64::max);
        // AnalyserNode を vocals GainNode (index 0) の出力側にタップとして接続
        let analyser = ctx.create_analyser().ok().map(|a| {
            a.set_fft_size(2048);
            let _ = gains[0].connect_with_audio_node(&a);
            a
        });
        Self {
            ctx,
            buffers: Rc::new(buffers),
            gains,
            state: Rc::new(RefCell::new(StemEngineState {
                sources: [None, None, None, None],
                offset_at_start: 0.0,
                ctx_time_at_start: 0.0,
                playing: false,
                duration,
            })),
            analyser,
        }
    }

    /// ボーカルステムの時間領域データを取得する（ピッチ検出用）。
    /// AnalyserNode が存在しない場合は None を返す。
    pub fn get_vocal_time_domain_data(&self) -> Option<Vec<f32>> {
        let analyser = self.analyser.as_ref()?;
        let len = analyser.fft_size() as usize;
        let mut buf = vec![0.0f32; len];
        analyser.get_float_time_domain_data(&mut buf);
        Some(buf)
    }

    pub fn sample_rate(&self) -> f32 {
        self.ctx.sample_rate()
    }

    pub fn duration(&self) -> f64 { self.state.borrow().duration }

    pub fn current_time(&self) -> f64 {
        let s = self.state.borrow();
        if s.playing {
            (s.offset_at_start + self.ctx.current_time() - s.ctx_time_at_start).min(s.duration)
        } else {
            s.offset_at_start
        }
    }

    pub fn is_playing(&self) -> bool { self.state.borrow().playing }

    pub async fn resume_ctx(&self) {
        if self.ctx.state() != web_sys::AudioContextState::Running {
            if let Ok(p) = self.ctx.resume() { let _ = JsFuture::from(p).await; }
        }
    }

    pub fn play(&self) {
        if !self.state.borrow().playing {
            let current = self.current_time();
            let duration = self.duration();
            // 再生残り秒数が 1 秒未満（曲が終了している）場合は先頭から再生
            let offset = if duration - current < 1.0 { 0.0 } else { current };
            self.play_from(offset);
        }
    }

    pub fn pause(&self) {
        let mut s = self.state.borrow_mut();
        if !s.playing { return; }
        for src in s.sources.iter_mut().flatten() {
            let _ = AudioScheduledSourceNode::stop_with_when(src, 0.0);
        }
        let elapsed = self.ctx.current_time() - s.ctx_time_at_start;
        s.offset_at_start = (s.offset_at_start + elapsed).min(s.duration);
        s.sources = [None, None, None, None];
        s.playing = false;
    }

    pub fn seek(&self, time: f64) {
        let time = time.clamp(0.0, self.state.borrow().duration);
        if self.state.borrow().playing { self.play_from(time); }
        else { self.state.borrow_mut().offset_at_start = time; }
    }

    fn play_from(&self, offset: f64) {
        let offset = offset.clamp(0.0, self.state.borrow().duration);
        {
            let mut s = self.state.borrow_mut();
            for src in s.sources.iter_mut().flatten() {
                let _ = AudioScheduledSourceNode::stop_with_when(src, 0.0);
            }
            s.sources = [None, None, None, None];
        }
        let mut new_sources: [Option<AudioBufferSourceNode>; 4] = [None, None, None, None];
        for (i, buf) in self.buffers.iter().enumerate() {
            let Some(buf) = buf else { continue };
            let Ok(src) = self.ctx.create_buffer_source() else { continue };
            src.set_buffer(Some(buf));
            let _ = src.connect_with_audio_node(&self.gains[i]);
            let _ = src.start_with_when_and_grain_offset(0.0, offset);
            new_sources[i] = Some(src);
        }
        let mut s = self.state.borrow_mut();
        s.ctx_time_at_start = self.ctx.current_time();
        s.offset_at_start = offset;
        s.playing = true;
        s.sources = new_sources;
    }
}
