//! One sampler voice: cursor, interpolation, envelope, gain (02-ENGINE.md §5).
//!
//! A voice never waits for storage. It reads through `kontra-stream`; frames
//! not in memory are silence and counted, and the cursor keeps moving, so a
//! note is never late and never drifts (rule 4).

mod envelope;
mod fade;
mod granular;
mod mod_waves;
mod oscillator;
mod wavetable;
mod zone;

use kontra_stream::{Mix, StreamClient};

pub use envelope::{Ahdsr, Envelope};
pub use granular::{Granular, GranularSpec, MAX_GRAINS};
pub use oscillator::{OSC_PERIOD, OscWave, Oscillator};
pub use wavetable::{
    CYCLE, Inharmonic, LEVELS, ModOsc, ModOscKind, Osc, PERIOD, Quality, Read, SIZE, Shape, Shaper,
    Side, TOP, Table, Unit, Warp, shape, warp,
};
pub use zone::{Loop, Loops, Region};

use fade::Fade;
pub use fade::{FadeCurve, FadeTarget};

/// How the host renders: live, where a read never waits, or offline (a
/// bounce), where a read waits for the sample pages it needs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderMode {
    #[default]
    Live,
    Offline,
}

/// Which side of a voice's amplitude envelope and gain stage an insert callback
/// runs on ([`Voice::render_through_stages`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceStage {
    /// Before the amplitude envelope, fade, lag, and voice gain/pan matrix.
    PreAmp,
    /// After the amplitude envelope, fade, lag, and voice gain/pan matrix.
    PostAmp,
}

/// Source frames decoded per inner chunk. Bounds per-voice memory, not pitch.
const SCRATCH: usize = 512;
/// Highest pitch step (source frames per output frame): +4 octaves at 4× rate.
pub const MAX_STEP: f64 = 64.0;
/// Output frames of source asked for ahead of the cursor each render.
pub const LOOKAHEAD: f64 = 8192.0;
/// One-pole gain smoothing time constant (ms), rate-invariant across host rates.
const GAIN_SMOOTH_MS: f32 = 1.0;
/// Default sample rate assumed by [`Voice::allocate`] until [`Voice::set_sample_rate`] is called.
const DEFAULT_RATE: u32 = 48_000;
/// The fastest a stretching voice plays (the stretch law's top).
const MAX_SPEED: f64 = 8.0;
/// A stretching voice's grain, output frames (43 ms at 48 kHz).
const GRAIN: u64 = 2048;
const HALF_GRAIN: u64 = GRAIN / 2;
/// Silence tail threshold (-120 dBFS) and consecutive frames before culling a releasing voice.
const TAIL_SILENCE: f32 = 1e-6;
const TAIL_FRAMES: u32 = 512;

/// Windowed-sinc anti-aliasing resampler parameters for upward pitch shifts (`step > 1.0`).
const SINC_LOBES: usize = 4;
/// Cutoff fraction of output Nyquist (`0.90 * Fs_out / 2`) for >60 dB alias suppression.
const SINC_CUTOFF: f64 = 0.90;
/// Kaiser window beta parameter (~67 dB stopband attenuation).
const KAISER_BETA: f64 = 6.5;
/// Maximum source tap radius per side (limits work at high ratios).
const MAX_SINC_HALF: usize = 32;
/// Lookup table resolution over `[0, SINC_LOBES]`.
const SINC_TABLE_LEN: usize = 2048;

static SINC_TABLE: std::sync::LazyLock<[f32; SINC_TABLE_LEN + 1]> =
    std::sync::LazyLock::new(build_sinc_table);

/// Per-frame one-pole gain smoothing coefficient for [`GAIN_SMOOTH_MS`] at `rate` Hz.
#[must_use]
fn gain_smooth_coef(rate: u32) -> f32 {
    let frames = GAIN_SMOOTH_MS * rate.max(1) as f32 / 1000.0;
    if frames > 1.0 {
        1.0 - (-1.0 / frames).exp()
    } else {
        1.0
    }
}

/// Zeroth-order modified Bessel function of the first kind `I_0(x)` for the Kaiser window.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half_x = 0.5 * x;
    for k in 1..=25 {
        let ratio = half_x / f64::from(k);
        term *= ratio * ratio;
        sum += term;
        if term < 1e-15 * sum {
            break;
        }
    }
    sum
}

/// Precomputes the half-sided Kaiser-windowed sinc table over `u in [0, SINC_LOBES]`.
fn build_sinc_table() -> [f32; SINC_TABLE_LEN + 1] {
    let inv_i0 = 1.0 / bessel_i0(KAISER_BETA);
    std::array::from_fn(|i| {
        if i == 0 {
            return 1.0;
        }
        if i == SINC_TABLE_LEN {
            return 0.0;
        }
        let u = (i as f64 / SINC_TABLE_LEN as f64) * SINC_LOBES as f64;
        let x = std::f64::consts::PI * u;
        let sinc = x.sin() / x;
        let r = u / SINC_LOBES as f64;
        let win = bessel_i0(KAISER_BETA * (1.0 - r * r).max(0.0).sqrt()) * inv_i0;
        (sinc * win) as f32
    })
}

/// Evaluates the precomputed Kaiser-windowed sinc at `u >= 0` using linear interpolation.
#[inline]
fn sinc_lookup(table: &[f32; SINC_TABLE_LEN + 1], u: f64) -> f32 {
    let pos = u * (SINC_TABLE_LEN as f64 / SINC_LOBES as f64);
    if !pos.is_finite() || pos < 0.0 || pos >= SINC_TABLE_LEN as f64 {
        return 0.0;
    }
    let idx = pos as usize;
    let frac = (pos - idx as f64) as f32;
    let a = table.get(idx).copied().unwrap_or(0.0);
    let b = table.get(idx + 1).copied().unwrap_or(0.0);
    a + (b - a) * frac
}

/// Tap radius (source frames per side) and frequency scale for upward shift `step > 1.0`.
#[inline]
fn sinc_params(step: f64) -> (usize, f64) {
    let ideal_scale = SINC_CUTOFF / step;
    let ideal_half = (SINC_LOBES as f64 / ideal_scale).ceil() as usize;
    // At extreme ratios we truncate the kernel; raising its cutoff would
    // let frequencies above the output Nyquist through.
    (ideal_half.min(MAX_SINC_HALF), ideal_scale)
}

/// How each output channel (row) takes each source channel (column).
pub type Gain = [[f32; 2]; 2];

/// How many lagging gains a voice glides, each a one-pole of its own.
pub const LAGS: usize = 4;

/// A voice's lagging gains ([`Lag`]), multiplied.
pub type Lags = [Lag; LAGS];

/// A gain gliding as a one-pole toward `gain`, `coef` of the way each frame
/// (1: at once).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lag {
    pub gain: f32,
    pub coef: f32,
}

impl Lag {
    pub const NONE: Self = Self {
        gain: 1.0,
        coef: 1.0,
    };

    /// Toward `gain` with time constant `tau_ms` at `rate`.
    #[must_use]
    pub fn new(gain: f32, tau_ms: f32, rate: u32) -> Self {
        let frames = tau_ms * rate as f32 / 1000.0;
        let coef = if frames > 1.0 {
            1.0 - (-1.0 / frames).exp()
        } else {
            1.0
        };
        Self { gain, coef }
    }
}

/// Everything a voice needs to start, resolved by the engine from the plan.
#[derive(Clone)]
pub struct VoiceStart {
    pub region: Region,
    /// Onset loop shift `[start, length]` as fractions of sample frames (zero
    /// leaves loops untouched; the caller negates it for a reversed region).
    /// Applied into [`Voice`]'s inline loop array
    /// with zero allocation.
    pub loop_shift: [f32; 2],
    /// Optional modulated channel matrix overriding [`Region::mix`] at onset.
    pub mix: Option<Mix>,
    /// Optional modulated wavetable read state overriding [`Table::read`] at onset.
    pub read: Option<Read>,
    /// Optional real-time synth oscillator overriding sample playback (zero allocation).
    pub oscillator: Option<Oscillator>,
    /// Optional real-time granular cloud generator reading from `region` (zero allocation).
    pub granular: Option<Granular>,
    /// Source frames per output frame: pitch and sample-rate ratio together.
    pub step: f64,
    pub gain: Gain,
    /// Slower gains on top of `gain`: the routes that lag, each its own.
    pub lag: Lags,
    pub envelope: Ahdsr,
    /// A fade its event asked for before it started; one up to a gain starts
    /// it from silence.
    pub fade: Option<(FadeTarget, u32, FadeCurve)>,
    /// Source time per output frame over the pitch's: 1 plays as recorded;
    /// otherwise the voice stretches time at its own pitch.
    pub speed: f64,
}

/// What a voice does when its key goes up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyUp {
    /// Its envelope releases.
    Release,
    /// It plays on to its end (a one-shot).
    PlayOn,
}

/// Where a voice is between its start and its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Life {
    /// Its key is down.
    Held,
    /// Its key is up; the engine frame its envelope began releasing (a
    /// one-shot's never does).
    Released(Option<u64>),
    /// Fading out to its end: no longer counted as playing.
    Ending(Option<u64>),
}

pub struct Voice {
    region: Option<Region>,
    /// Per-voice inline copies of `region`'s mutable loop/mix/wavetable state
    /// so [`Region`] stays compact (64 B) while [`Voice::start`] and live
    /// modulation never allocate on the audio thread.
    loops: Loops,
    mix: Option<Mix>,
    table: Option<Table>,
    osc: Option<Oscillator>,
    granular: Option<Granular>,
    /// Position at `elapsed == 0`; moved only when the step changes, so the
    /// cursor is the same whatever block sizes rendered it (rule 3).
    anchor: f64,
    elapsed: u64,
    step: f64,
    /// How the step moves from the anchor on (a glide), frame by frame.
    ramp: Ramp,
    gain: Gain,
    target: Gain,
    /// Per-frame one-pole gain smoothing coefficient derived from the host sample rate.
    gain_smooth: f32,
    lagged: [f32; LAGS],
    lag: Lags,
    env: Envelope,
    fade: Fade,
    life: Life,
    /// The codec block last decoded for the frames read and for the blend.
    decoded: [Box<kontra_stream::Decoded>; 2],
    /// Cursor position of the note's release, once it came.
    released: Option<u64>,
    missing: u64,
    /// See [`VoiceStart::speed`]; the cursor moves at `step · speed`.
    speed: f64,
    /// Active loop tuning multiplier (1.0 outside tuned loop repeats).
    loop_tune: f64,
    /// Consecutive frames below `-120 dB` while releasing/ending.
    silent_tail: u32,
    /// A stretching voice's two read heads, each a pitch-rate cursor that
    /// restarts at the cursor once a grain, half a grain apart.
    heads: [f64; 2],
    /// Frames since the stretch began, which places its grains.
    grain: u64,
    #[cfg(feature = "trace")]
    last_peak: f32,
    /// A host's offline render: a read waits for its pages.
    mode: RenderMode,
    offline_wait_timeouts: u64,
}

/// The buffers a voice renders through, one set for every voice an engine
/// renders in turn: none holds anything between one chunk and the next.
pub struct Scratch {
    /// Source frames read before interpolation.
    window: Box<[[f32; 2]]>,
    /// The frames a loop crossfade blends in.
    partner: Box<[[f32; 2]]>,
    /// Interpolated frames before the envelope and gain.
    raw: Box<[[f32; 2]]>,
    /// Head 0's frames while head 1 reads.
    head: Box<[[f32; 2]]>,
}

impl Scratch {
    /// Allocates the buffers; call at prepare, never on the audio thread.
    #[must_use]
    pub fn allocate() -> Self {
        let buffer = || vec![[0.0; 2]; SCRATCH].into_boxed_slice();
        Self {
            window: buffer(),
            partner: buffer(),
            raw: buffer(),
            head: buffer(),
        }
    }
}

impl Voice {
    /// Allocates the voice's state; call at prepare, never on the audio thread.
    #[must_use]
    pub fn allocate() -> Self {
        // Eagerly initialize the static windowed-sinc table off the audio thread.
        let _ = std::sync::LazyLock::force(&SINC_TABLE);
        Self {
            region: None,
            loops: Loops::new(),
            mix: None,
            table: None,
            osc: None,
            granular: None,
            anchor: 0.0,
            elapsed: 0,
            step: 1.0,
            ramp: Ramp::NONE,
            gain: [[0.0; 2]; 2],
            target: [[0.0; 2]; 2],
            gain_smooth: gain_smooth_coef(DEFAULT_RATE),
            lagged: [1.0; LAGS],
            lag: [Lag::NONE; LAGS],
            env: Envelope::new(Ahdsr::GATE),
            fade: Fade::NONE,
            life: Life::Held,
            decoded: [kontra_stream::Decoded::new(), kontra_stream::Decoded::new()],
            released: None,
            missing: 0,
            speed: 1.0,
            loop_tune: 1.0,
            silent_tail: 0,
            heads: [0.0; 2],
            grain: 0,
            #[cfg(feature = "trace")]
            last_peak: 0.0,
            mode: RenderMode::Live,
            offline_wait_timeouts: 0,
        }
    }

    /// Configures host sample rate for rate-invariant gain smoothing.
    pub fn set_sample_rate(&mut self, rate: u32) {
        self.gain_smooth = gain_smooth_coef(rate);
    }

    /// Starts playing. No allocation. The previous region's sample reference is
    /// dropped here, so the plan must keep its samples alive until it retires.
    pub fn start(&mut self, start: VoiceStart) {
        self.loops.clear();
        if let Some(loops) = &start.region.loops {
            // Capped at inline capacity (8) so the audio thread never spills to the heap.
            self.loops.extend(loops.iter().take(8).copied());
        }
        if start.loop_shift[0] != 0.0 || start.loop_shift[1] != 0.0 {
            zone::shift_loops(
                &mut self.loops,
                start.region.sample.frames(),
                start.region.start,
                start.region.end,
                start.loop_shift[0],
                start.loop_shift[1],
            );
        }
        self.mix = start.mix.or_else(|| start.region.mix.as_deref().copied());
        self.table = start.region.table.as_deref().cloned();
        self.osc = start.oscillator;
        self.granular = start.granular;
        if let (Some(read), Some(table)) = (start.read, self.table.as_mut()) {
            table.read = read;
        }
        self.region = Some(start.region);
        self.anchor = 0.0;
        self.elapsed = 0;
        self.step = clamp_step(start.step);
        self.ramp = Ramp::NONE;
        self.gain = start.gain;
        self.target = start.gain;
        self.lagged = start.lag.map(|l| l.gain);
        self.lag = start.lag;
        self.env = Envelope::new(start.envelope);
        self.fade = match start.fade {
            Some((FadeTarget::Hold(level), ..)) if level > 0.0 => Fade::SILENT,
            _ => Fade::NONE,
        };
        self.life = Life::Held;
        if let Some((target, frames, curve)) = start.fade {
            self.fade_along(target, frames, curve);
        }
        self.released = None;
        self.missing = 0;
        self.speed = if start.speed.is_finite() {
            start.speed.clamp(0.0, MAX_SPEED)
        } else {
            1.0
        };
        self.loop_tune = 1.0;
        self.silent_tail = 0;
        self.heads = [0.0; 2];
        self.grain = 0;
        #[cfg(feature = "trace")]
        {
            self.last_peak = 0.0;
        }
    }

    /// Retriggers an active voice on a rapid repeated note without hard-resetting
    /// the amplitude envelope or gain smoother to silence (avoiding step clicks
    /// while restarting the sample/synth cursor cleanly). No allocation.
    pub fn retrigger(&mut self, start: VoiceStart) {
        let prev_env = self.env;
        let prev_gain = self.gain;
        let prev_lagged = self.lagged;
        let prev_fade = self.fade;
        let active = self.is_active();
        let envelope = start.envelope;
        let has_fade = start.fade.is_some();
        self.start(start);
        if active {
            self.env = prev_env;
            self.env.retrigger(envelope);
            self.gain = prev_gain;
            self.lagged = prev_lagged;
            if !has_fade {
                self.fade = prev_fade;
            }
        }
    }

    /// Sets the amplitude envelope's release duration before [`Self::key_up`].
    pub fn set_release_frames(&mut self, frames: u32) {
        self.env.set_release(frames);
    }

    pub fn release(&mut self) {
        let at = self.at(0) as u64;
        self.released = self.released.or(Some(at));
        self.env.release();
    }

    /// Its key went up at engine frame `now`.
    pub fn key_up(&mut self, now: u64, key_up: KeyUp) {
        let at = (key_up == KeyUp::Release).then(|| {
            self.release();
            now
        });
        self.life = match self.life {
            Life::Ending(_) => Life::Ending(at),
            _ => Life::Released(at),
        };
    }

    /// Ends it through its envelope's release at engine frame `now` (a voice
    /// stopped by a rule that releases): it no longer counts as playing.
    /// `true` when this first sets it ending.
    pub fn end_by_release(&mut self, now: u64) -> bool {
        let first = !self.is_ending();
        let at = self.released_at().unwrap_or(now);
        let position = self.at(0) as u64;
        self.released = self.released.or(Some(position));
        self.env.force_release();
        self.life = Life::Ending(Some(at));
        first
    }

    /// Fades over `frames` to `target`; ending (a stolen or choked voice)
    /// is click-free, and a fade already ending it ends at the sooner.
    /// `true` when this fade first sets it ending.
    pub fn fade(&mut self, target: FadeTarget, frames: u32) -> bool {
        self.fade_along(target, frames, FadeCurve::Linear)
    }

    /// As [`Self::fade`], along `curve`.
    pub fn fade_along(&mut self, target: FadeTarget, frames: u32, curve: FadeCurve) -> bool {
        self.fade.start(target, frames, curve);
        let first = target == FadeTarget::End && !self.is_ending();
        if target == FadeTarget::End {
            self.life = Life::Ending(self.released_at());
        }
        first
    }

    /// Its key is up (an ending voice's may still be down).
    #[must_use]
    pub fn is_released(&self) -> bool {
        matches!(self.life, Life::Released(_))
    }

    /// Fading to its end: it no longer counts as playing.
    #[must_use]
    pub fn is_ending(&self) -> bool {
        matches!(self.life, Life::Ending(_))
    }

    /// The engine frame its envelope began releasing.
    #[must_use]
    pub fn released_at(&self) -> Option<u64> {
        match self.life {
            Life::Held => None,
            Life::Released(at) | Life::Ending(at) => at,
        }
    }

    /// Stops at once, without a release.
    pub fn kill(&mut self) {
        self.region = None;
        self.loops.clear();
        self.mix = None;
        self.table = None;
        self.osc = None;
        self.granular = None;
    }

    /// Changes pitch from the next frame.
    pub fn set_step(&mut self, step: f64) {
        self.anchor = self.at(0);
        self.elapsed = 0;
        self.step = clamp_step(step);
        self.ramp = Ramp::NONE;
    }

    /// Changes pitch from the next frame to `from`, moving it linearly frame
    /// by frame to `to` over `frames` frames and holding there: a glide is
    /// smooth within a control step, not a stair of them.
    pub fn set_step_ramp(&mut self, from: f64, to: f64, frames: u32) {
        self.set_step(from);
        if frames > 0 {
            self.ramp = Ramp {
                per_frame: (clamp_step(to) - self.step) / f64::from(frames),
                frames: u64::from(frames),
            };
        }
    }

    /// Changes gain, smoothed over a few milliseconds.
    pub fn set_gain(&mut self, gain: Gain) {
        self.target = gain;
    }

    /// Changes the lagging gain, gliding at its own pace.
    pub fn set_lag(&mut self, lag: Lags) {
        self.lag = lag;
    }

    /// Steps each route's lag one frame and returns their product.
    fn tick_lags(&mut self) -> f32 {
        let mut lagged = 1.0;
        for (at, l) in self.lagged.iter_mut().zip(&self.lag) {
            *at += (l.gain - *at) * l.coef;
            lagged *= *at;
        }
        lagged
    }

    /// Changes the channel matrix (live cells) from the next chunk read.
    ///
    /// ponytail: a step at each refresh, not smoothed like the gain; ramp it
    /// per chunk if a fast sweep zippers.
    pub fn set_mix(&mut self, mix: Mix) {
        if self.region.is_some() {
            self.mix = Some(mix);
        }
    }

    /// Where a wavetable voice reads its table from now on; others ignore it.
    // ponytail: steps at each control span; Kontakt ramps over 32 frames
    // (0x102ac): ramp inside `Table::render` if a fast route steps audibly.
    pub fn set_read(&mut self, read: Read) {
        if let Some(table) = self.table.as_mut() {
            table.read = read;
        }
    }

    /// Updates the running amplitude envelope smoothly without clicks.
    pub fn update_envelope(&mut self, settings: Ahdsr) {
        self.env.update_params(settings);
    }

    /// Current gain matrix including its envelope and lag, before sample and effects.
    #[must_use]
    pub fn current_gain(&self) -> Gain {
        let lagged: f32 = self.lagged.iter().product();
        let level = lagged * self.env.output_gain() * self.fade.level();
        self.gain.map(|row| row.map(|gain| gain * level))
    }

    /// The gain the voice is moving to, its lagging gain included.
    #[must_use]
    pub fn target_gain(&self) -> Gain {
        let lagged: f32 = self.lag.iter().map(|l| l.gain).product();
        self.target.map(|row| row.map(|g| g * lagged))
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.region.is_some()
    }

    /// Current amplitude envelope stage, for offline inspection.
    #[must_use]
    pub fn envelope_stage(&self) -> &'static str {
        self.env.stage_name()
    }

    /// Current amplitude envelope level, 0..=1.
    pub fn envelope_level(&self) -> f32 {
        self.env.level()
    }

    /// Peak of this voice's last render call before group effects, when tracing is enabled.
    #[cfg(feature = "trace")]
    #[must_use]
    pub fn last_peak(&self) -> f32 {
        self.last_peak
    }

    /// How the host renders: offline, a read waits for its pages; never
    /// offline in a live host.
    pub fn set_render_mode(&mut self, mode: RenderMode) {
        self.mode = mode;
    }

    /// How many offline page waits timed out, including previous notes in this slot.
    #[must_use]
    pub fn offline_wait_timeouts(&self) -> u64 {
        self.offline_wait_timeouts
    }

    /// Source frames this voice read while their data was not in memory.
    #[must_use]
    pub fn missing_frames(&self) -> u64 {
        self.missing
    }

    /// Source frames per output frame at the current pitch (including active loop tuning).
    #[must_use]
    pub fn step(&self) -> f64 {
        self.ramp.step_at(self.step, self.elapsed) * self.loop_tune
    }

    /// Frames played so far, in source frames from the region start.
    #[must_use]
    pub fn position(&self) -> f64 {
        self.at(0)
    }

    /// The sample frame playing now in `region` (this voice's), through its
    /// loops and reverse.
    #[must_use]
    pub fn sample_frame(&self, region: &Region) -> Option<u64> {
        let loops = if self.region.is_some() {
            self.loops.as_slice()
        } else {
            region.loops_slice()
        };
        region
            .locate_with(loops, self.at(0).max(0.) as u64, self.released)
            .map(|run| run.first)
    }

    /// Cursor position `ahead` output frames from now.
    fn at(&self, ahead: u64) -> f64 {
        self.anchor
            + self.ramp.travel(self.step, self.elapsed + ahead) * self.speed * self.loop_tune
    }

    /// Mixes the next `out.len()` frames into `out`. `now` is the sample time of
    /// `out[0]`, the deadline for the pages this render asks the store for.
    pub fn render(
        &mut self,
        scratch: &mut Scratch,
        client: &StreamClient,
        now: u64,
        out: &mut [[f32; 2]],
    ) {
        self.render_inner::<false, true>(scratch, client, now, out, |_, _| {});
    }

    /// As [`Self::render`], with `insert` run on the interpolated signal
    /// before the envelope and gain (a voice's own effects), in chunks of at
    /// most 512 frames.
    pub fn render_through(
        &mut self,
        scratch: &mut Scratch,
        client: &StreamClient,
        now: u64,
        out: &mut [[f32; 2]],
        mut insert: impl FnMut(&mut [[f32; 2]]),
    ) {
        self.render_inner::<false, false>(scratch, client, now, out, |stage, io| {
            if stage == VoiceStage::PreAmp {
                insert(io);
            }
        });
    }

    /// As [`Self::render`], with `insert` invoked on both sides of the voice's
    /// amplitude envelope, fade, lag and gain/pan stage: [`VoiceStage::PreAmp`]
    /// on the interpolated chunk before amp, and [`VoiceStage::PostAmp`] on the
    /// amp-shaped chunk before mixing into `out`.
    pub fn render_through_stages(
        &mut self,
        scratch: &mut Scratch,
        client: &StreamClient,
        now: u64,
        out: &mut [[f32; 2]],
        insert: impl FnMut(VoiceStage, &mut [[f32; 2]]),
    ) {
        self.render_inner::<true, false>(scratch, client, now, out, insert);
    }

    /// `PLAIN`: no inserts see the voice, so a chunk it cannot be heard in
    /// (gain and its target exactly zero, not releasing) skips reading and
    /// interpolating its source; everything else steps as when it renders.
    fn render_inner<const POST: bool, const PLAIN: bool>(
        &mut self,
        scratch: &mut Scratch,
        client: &StreamClient,
        now: u64,
        out: &mut [[f32; 2]],
        mut insert: impl FnMut(VoiceStage, &mut [[f32; 2]]),
    ) {
        #[cfg(feature = "trace")]
        {
            self.last_peak = 0.0;
        }
        let Some(region) = self.region.take() else {
            return;
        };
        let sample = &region.sample;
        let released = self.released;
        let step = self.step() * self.speed;
        // A stretching voice's heads trail or lead the cursor by up to a grain.
        let from = if self.speed == 1.0 {
            self.at(0)
        } else {
            self.heads.iter().fold(self.at(0), |a, &h| a.min(h))
        };
        let ahead = (LOOKAHEAD * step) as usize + (self.at(0) - from) as usize;
        let synth = self.table.is_some() || self.osc.is_some() || self.granular.is_some();
        let ahead = if synth { 0 } else { ahead };
        region.runs_with(&self.loops, from as i64, ahead, released, |offset, run| {
            let low = if run.descending {
                run.first + 1 - run.count as u64
            } else {
                run.first
            };
            // The voice reaches this run `offset` source frames from now.
            let due = now + (offset as f64 / step) as u64;
            let count = run.count as u64;
            sample.prefetch(client, low, count, due, step, run.descending);
            if let Some(xf) = run.crossfade() {
                let due = due + (xf.offset as f64 / step) as u64;
                sample.prefetch(
                    client,
                    xf.partner_low,
                    xf.count as u64,
                    due,
                    step,
                    xf.descending,
                );
            }
        });

        // A live voice never stalls waiting on storage (02-ENGINE.md §5): any
        // unresident frames render as silence while the cursor keeps exact time,
        // preserving phase alignment across multi-mic layers. Offline bounces
        // settle inside `read_window`.
        let has_tuned = !synth && zone::Region::has_tuned_loop(&self.loops);
        let mut done = 0;
        let mut alive = true;
        while done < out.len() && alive {
            let mut tune_boundary = None;
            if has_tuned {
                let pos = self.at(0);
                if let Some((target_tune, run_left)) =
                    region.tune_span(&self.loops, pos.max(0.0) as u64, released)
                {
                    if (target_tune - self.loop_tune).abs() > 1e-9 {
                        self.anchor = pos;
                        self.step = self.ramp.step_at(self.step, self.elapsed);
                        self.ramp.frames = self.ramp.frames.saturating_sub(self.elapsed);
                        self.elapsed = 0;
                        self.loop_tune = target_tune;
                    }
                    tune_boundary = Some(pos.max(0.0).floor() + run_left as f64);
                }
            }
            let fit = if synth {
                SCRATCH
            } else {
                let peak = self.ramp.peak(self.step) * self.loop_tune;
                let margin = if peak > 1.0 {
                    2 * (sinc_params(peak).0 + 1) + 2
                } else {
                    4
                };
                (((SCRATCH.saturating_sub(margin)) as f64 / peak).floor() as usize).max(1)
            };
            let mut n = fit.min(out.len() - done);
            let stretched = self.speed != 1.0;
            if stretched {
                // Heads restart only at a chunk's start.
                n = n.min((HALF_GRAIN - self.grain % HALF_GRAIN) as usize);
            }
            if let Some(boundary) = tune_boundary
                && self.at(n as u64) >= boundary
            {
                // The first cursor at or beyond the boundary belongs to the
                // next chunk. Search the same ramp positions that rendering uses.
                let (mut lo, mut hi) = (1, n);
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    if self.at(mid as u64) >= boundary {
                        hi = mid;
                    } else {
                        lo = mid + 1;
                    }
                }
                n = lo;
            }
            let missing_before = self.missing;
            let releasing = self.released.is_some() || self.is_ending();
            let silent = PLAIN
                && !synth
                && !stretched
                && !releasing
                && self.gain.as_flattened().iter().all(|&g| g == 0.0)
                && self.target.as_flattened().iter().all(|&g| g == 0.0);
            let filled = if silent {
                let cursor = (self.anchor, self.elapsed, self.step * self.loop_tune);
                let ramp = Ramp {
                    per_frame: self.ramp.per_frame * self.loop_tune,
                    frames: self.ramp.frames,
                };
                window_frames(scratch, n, cursor, ramp)
            } else if let Some(mut osc) = self.osc.take() {
                let raw = scratch.raw.get_mut(..n).unwrap_or_default();
                let (base_step, elapsed, ramp, loop_tune) =
                    (self.step, self.elapsed, self.ramp, self.loop_tune);
                osc.render(|i| ramp.step_at(base_step, elapsed + i) * loop_tune, raw);
                self.osc = Some(osc);
                raw.len()
            } else if let Some(mut gran) = self.granular.take() {
                let raw = scratch.raw.get_mut(..n).unwrap_or_default();
                let cur_step = self.step();
                let mix = self.mix.as_ref();
                let [main_decoded, _] = &mut self.decoded;
                let miss = gran.render(
                    &region,
                    client,
                    mix,
                    main_decoded,
                    &mut scratch.window,
                    cur_step,
                    raw,
                );
                self.missing += u64::from(miss);
                self.granular = Some(gran);
                raw.len()
            } else if let Some(mut tbl) = self.table.take() {
                let raw = scratch.raw.get_mut(..n).unwrap_or_default();
                // Each frame's own cursor: a gliding table moves pitch every
                // frame, the same at any block size.
                tbl.render(|i| self.at(i), self.step(), raw);
                self.table = Some(tbl);
                raw.len()
            } else if stretched {
                self.read_stretched(scratch, client, &region, n)
            } else {
                let cursor = (self.anchor, self.elapsed, self.step * self.loop_tune);
                let ramp = Ramp {
                    per_frame: self.ramp.per_frame * self.loop_tune,
                    frames: self.ramp.frames,
                };
                self.read_window(scratch, client, &region, n, cursor, ramp)
            };
            let chunk_missing = self.missing != missing_before;
            let limit = filled.min(out.len().saturating_sub(done));
            let raw = scratch.raw.get_mut(..limit).unwrap_or_default();
            if !raw.is_empty() {
                insert(VoiceStage::PreAmp, raw);
            }
            if silent {
                // The amp stage of a voice that adds exactly zero: its
                // envelope, fade, lags and gain step frame by frame as below.
                for _ in 0..limit {
                    self.env.tick();
                    self.fade.next();
                    self.tick_lags();
                    for (g, target) in self
                        .gain
                        .as_flattened_mut()
                        .iter_mut()
                        .zip(self.target.as_flattened())
                    {
                        *g += (target - *g) * self.gain_smooth;
                    }
                    if self.env.is_off() || self.fade.ended() {
                        alive = false;
                        break;
                    }
                    self.silent_tail = 0;
                }
            } else if POST {
                let mut amp_frames = 0usize;
                for sample in raw.iter_mut() {
                    let level = self.env.tick() * self.fade.next() * self.tick_lags();
                    for (g, target) in self
                        .gain
                        .as_flattened_mut()
                        .iter_mut()
                        .zip(self.target.as_flattened())
                    {
                        *g += (target - *g) * self.gain_smooth;
                    }
                    let (l, r) = (sample[0] * level, sample[1] * level);
                    let [[ll, lr], [rl, rr]] = self.gain;
                    sample[0] = ll * l + lr * r;
                    sample[1] = rl * l + rr * r;
                    amp_frames += 1;
                    if self.env.is_off() || self.fade.ended() {
                        alive = false;
                        break;
                    }
                }
                let post = raw.get_mut(..amp_frames).unwrap_or_default();
                if !post.is_empty() {
                    insert(VoiceStage::PostAmp, post);
                }
                for (frame, &[left, right]) in out.iter_mut().skip(done).zip(post.iter()) {
                    #[cfg(feature = "trace")]
                    {
                        self.last_peak = self.last_peak.max(left.abs()).max(right.abs());
                    }
                    frame[0] += left;
                    frame[1] += right;
                    if releasing && !chunk_missing {
                        if left.abs().max(right.abs()) < TAIL_SILENCE {
                            self.silent_tail += 1;
                            if self.silent_tail >= TAIL_FRAMES {
                                alive = false;
                                break;
                            }
                        } else {
                            self.silent_tail = 0;
                        }
                    } else {
                        self.silent_tail = 0;
                    }
                }
            } else {
                for (frame, &[l, r]) in out.iter_mut().skip(done).zip(raw.iter()) {
                    let level = self.env.tick() * self.fade.next() * self.tick_lags();
                    for (g, target) in self
                        .gain
                        .as_flattened_mut()
                        .iter_mut()
                        .zip(self.target.as_flattened())
                    {
                        *g += (target - *g) * self.gain_smooth;
                    }
                    let (l, r) = (l * level, r * level);
                    let [[ll, lr], [rl, rr]] = self.gain;
                    let left = ll * l + lr * r;
                    let right = rl * l + rr * r;
                    #[cfg(feature = "trace")]
                    {
                        self.last_peak = self.last_peak.max(left.abs()).max(right.abs());
                    }
                    frame[0] += left;
                    frame[1] += right;
                    if self.env.is_off() || self.fade.ended() {
                        alive = false;
                        break;
                    }
                    if releasing && !chunk_missing {
                        if left.abs().max(right.abs()) < TAIL_SILENCE {
                            self.silent_tail += 1;
                            if self.silent_tail >= TAIL_FRAMES {
                                alive = false;
                                break;
                            }
                        } else {
                            self.silent_tail = 0;
                        }
                    } else {
                        self.silent_tail = 0;
                    }
                }
            }
            self.elapsed += n as u64;
            if !synth && region.finished_with(&self.loops, self.at(0), released) {
                alive = false;
            }
            done += n;
        }
        if alive {
            self.region = Some(region);
        } else {
            self.loops.clear();
            self.mix = None;
            self.table = None;
            self.osc = None;
            self.granular = None;
        }
    }

    /// `n` frames of a stretching voice into `raw`: its two heads read at the
    /// pitch's rate, each faded in and out over its grain (triangles that sum
    /// to one), so the cursor moves at `speed` while the pitch holds. Head 0
    /// plays the note's start in full.
    // ponytail: a plain two-head overlap at a fixed 2048-frame grain, no
    // waveform alignment; Kontakt's stretch is smoother on sustained tones.
    fn read_stretched(
        &mut self,
        scratch: &mut Scratch,
        client: &StreamClient,
        region: &Region,
        n: usize,
    ) -> usize {
        let at = self.at(0);
        if self.grain % GRAIN == HALF_GRAIN {
            self.heads[0] = at;
        }
        if self.grain.is_multiple_of(GRAIN) {
            self.heads[1] = at;
        }
        let step = self.step();
        let a = self.read_window(
            scratch,
            client,
            region,
            n,
            (self.heads[0], 0, step),
            Ramp::NONE,
        );
        if let (Some(head), Some(raw)) = (scratch.head.get_mut(..a), scratch.raw.get(..a)) {
            head.copy_from_slice(raw);
        }
        let filled = a.min(self.read_window(
            scratch,
            client,
            region,
            n,
            (self.heads[1], 0, step),
            Ramp::NONE,
        ));
        let grain = self.grain;
        let pairs = scratch.raw.iter_mut().zip(scratch.head.iter()).take(filled);
        for (i, (b, a)) in pairs.enumerate() {
            let phase = ((grain + HALF_GRAIN + i as u64) % GRAIN) as f32 / GRAIN as f32;
            let w = 1.0 - (2.0 * phase - 1.0).abs();
            *b = [w * a[0] + (1.0 - w) * b[0], w * a[1] + (1.0 - w) * b[1]];
        }
        for h in &mut self.heads {
            *h += n as f64 * step;
        }
        self.grain += n as u64;
        filled
    }

    /// Reads the region about a cursor at the pitch's step, frame `i` at
    /// `base + travel(frames + i)` (a closed form from the anchor, so every
    /// host block size lands on the same positions), and interpolates `n`
    /// frames into `raw`; returns how many it filled.
    fn read_window(
        &mut self,
        scratch: &mut Scratch,
        client: &StreamClient,
        region: &Region,
        n: usize,
        (base, frames, first_step): (f64, u64, f64),
        ramp: Ramp,
    ) -> usize {
        let sample = &region.sample;
        let mix = self.mix.as_ref();
        let loops = &self.loops;
        let released = self.released;
        let step = ramp.step_at(first_step, frames);
        let at = |i: u64| base + ramp.travel(first_step, frames + i);
        let (lo, span) = window_span(n, (base, frames, first_step), ramp);
        let partner = &mut scratch.partner;
        let [main, blend_cache] = &mut self.decoded;
        let Some(window) = scratch.window.get_mut(..span.min(SCRATCH)) else {
            return 0;
        };
        if self.mode == RenderMode::Offline {
            region.runs_with(loops, lo, window.len(), released, |_, run| {
                let low = if run.descending {
                    run.first + 1 - run.count as u64
                } else {
                    run.first
                };
                sample.prefetch(client, low, run.count as u64, 0, step, run.descending);
                if let Some(xf) = run.crossfade() {
                    sample.prefetch(
                        client,
                        xf.partner_low,
                        xf.count as u64,
                        0,
                        step,
                        xf.descending,
                    );
                }
            });
            if !client.settle(std::time::Duration::from_secs(5)) {
                self.offline_wait_timeouts += 1;
            }
        }
        let mut missing = 0;
        // Runs tile the window from the first one's offset; each read writes
        // all its frames, so only what no run covers needs zeroing.
        let (mut first, mut end) = (window.len(), 0);
        region.runs_with(loops, lo, window.len(), released, |offset, run| {
            let count = run.count;
            first = first.min(offset);
            end = end.max(offset + count);
            if let Some(dst) = window.get_mut(offset..offset + count) {
                if run.descending {
                    missing += sample
                        .read_cached(client, run.first + 1 - count as u64, dst, mix, Some(main))
                        .missing;
                    dst.reverse();
                } else {
                    missing += sample
                        .read_cached(client, run.first, dst, mix, Some(main))
                        .missing;
                }
                if let Some(xf) = run.crossfade()
                    && let (Some(blend), Some(head)) = (
                        partner.get_mut(..xf.count),
                        dst.get_mut(xf.offset..xf.offset + xf.count),
                    )
                {
                    missing += sample
                        .read_cached(client, xf.partner_low, blend, mix, Some(blend_cache))
                        .missing;
                    if xf.descending {
                        blend.reverse();
                    }
                    for (k, (d, b)) in head.iter_mut().zip(blend.iter()).enumerate() {
                        let w = (xf.progress + k as u64) as f32 / xf.len as f32;
                        d[0] += (b[0] - d[0]) * w;
                        d[1] += (b[1] - d[1]) * w;
                    }
                }
            }
        });
        self.missing += u64::from(missing);
        if first >= end {
            window.fill([0.0; 2]);
        } else {
            window.get_mut(..first).unwrap_or_default().fill([0.0; 2]);
            window.get_mut(end..).unwrap_or_default().fill([0.0; 2]);
        }

        let sinc_table = &*SINC_TABLE;
        let mut filled = 0;
        for (i, frame) in scratch.raw.iter_mut().take(n).enumerate() {
            let cur_step = ramp.step_at(first_step, frames + i as u64);
            let x = at(i as u64) - lo as f64;
            if !reads(window.len(), x, cur_step) {
                break;
            }
            if cur_step > 1.0 {
                let (half, scale) = sinc_params(cur_step);
                let center_i = x.round() as isize;
                let frac = x - center_i as f64;
                let Ok(center) = usize::try_from(center_i) else {
                    break;
                };
                if center < half || center + half >= window.len() {
                    break;
                }
                let w0 = f64::from(sinc_lookup(sinc_table, frac.abs() * scale));
                let Some(&c_frame) = window.get(center) else {
                    break;
                };
                let mut acc_l = f64::from(c_frame[0]) * w0;
                let mut acc_r = f64::from(c_frame[1]) * w0;
                let mut w_sum = w0;
                for d in 1..=half {
                    let (Some(&lo_f), Some(&hi_f)) =
                        (window.get(center - d), window.get(center + d))
                    else {
                        break;
                    };
                    if frac == 0.0 {
                        let w = f64::from(sinc_lookup(sinc_table, d as f64 * scale));
                        acc_l += (f64::from(lo_f[0]) + f64::from(hi_f[0])) * w;
                        acc_r += (f64::from(lo_f[1]) + f64::from(hi_f[1])) * w;
                        w_sum += 2.0 * w;
                    } else {
                        let w_lo = f64::from(sinc_lookup(sinc_table, (d as f64 + frac) * scale));
                        let w_hi = f64::from(sinc_lookup(sinc_table, (d as f64 - frac) * scale));
                        acc_l += f64::from(lo_f[0]) * w_lo + f64::from(hi_f[0]) * w_hi;
                        acc_r += f64::from(lo_f[1]) * w_lo + f64::from(hi_f[1]) * w_hi;
                        w_sum += w_lo + w_hi;
                    }
                }
                *frame = if w_sum.abs() > 1e-12 {
                    [(acc_l / w_sum) as f32, (acc_r / w_sum) as f32]
                } else {
                    [0.0, 0.0]
                };
            } else {
                let base = x.floor() as usize;
                let t = (x - base as f64) as f32;
                let Some(start) = base.checked_sub(1) else {
                    break;
                };
                let Some(&[y0, y1, y2, y3]) = window.get(start..base + 3) else {
                    break;
                };
                *frame = [
                    hermite(y0[0], y1[0], y2[0], y3[0], t),
                    hermite(y0[1], y1[1], y2[1], y3[1], t),
                ];
            }
            filled += 1;
        }
        filled
    }
}

/// The window [`Voice::read_window`] reads for `n` frames: its first source
/// frame and its length, with the interpolator's margin on both sides.
fn window_span(n: usize, (base, frames, first_step): (f64, u64, f64), ramp: Ramp) -> (i64, usize) {
    let at = |i: u64| base + ramp.travel(first_step, frames + i);
    let (left_pad, right_pad) = if ramp.peak(first_step) > 1.0 {
        let h = sinc_params(ramp.peak(first_step)).0 as i64 + 1;
        (h, h + 1)
    } else {
        (1, 3)
    };
    let lo = at(0).floor() as i64 - left_pad;
    let span = (at(n as u64 - 1).floor() as i64 + right_pad - lo) as usize;
    (lo, span)
}

/// Whether the interpolator has every frame it needs at window position `x`.
fn reads(window: usize, x: f64, step: f64) -> bool {
    if !x.is_finite() || x < 1.0 {
        return false;
    }
    if step > 1.0 {
        let half = sinc_params(step).0;
        usize::try_from(x.round() as isize).is_ok_and(|c| c >= half && c + half < window)
    } else {
        x.floor() as usize + 3 <= window
    }
}

/// How many of `n` frames [`Voice::read_window`] fills at this cursor,
/// without reading the source.
fn window_frames(scratch: &Scratch, n: usize, cursor: (f64, u64, f64), ramp: Ramp) -> usize {
    let (base, frames, first_step) = cursor;
    let (lo, span) = window_span(n, cursor, ramp);
    let Some(window) = scratch.window.get(..span.min(SCRATCH)).map(<[_]>::len) else {
        return 0;
    };
    (0..n.min(scratch.raw.len()) as u64)
        .take_while(|&i| {
            let x = base + ramp.travel(first_step, frames + i) - lo as f64;
            reads(window, x, ramp.step_at(first_step, frames + i))
        })
        .count()
}

/// A step that rises by `per_frame` each output frame for `frames` frames
/// from the anchor, then holds: a glide's piece between two control steps.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ramp {
    per_frame: f64,
    frames: u64,
}

impl Ramp {
    const NONE: Self = Self {
        per_frame: 0.0,
        frames: 0,
    };

    /// The step of output frame `k` after the anchor.
    fn step_at(self, step: f64, k: u64) -> f64 {
        step + self.per_frame * k.min(self.frames) as f64
    }

    /// Source frames the first `k` output frames after the anchor cover:
    /// the sum of their steps, in closed form.
    fn travel(self, step: f64, k: u64) -> f64 {
        let r = k.min(self.frames) as f64;
        let held = k.saturating_sub(self.frames) as f64;
        step * k as f64 + self.per_frame * (r * (r - 1.0) / 2.0 + held * self.frames as f64)
    }

    /// The largest step it reaches.
    fn peak(self, step: f64) -> f64 {
        step.max(self.step_at(step, self.frames))
    }
}

fn clamp_step(step: f64) -> f64 {
    if step.is_finite() {
        step.clamp(1.0 / 1024.0, MAX_STEP)
    } else {
        1.0
    }
}

/// 4-point, 3rd-order Hermite between `y1` and `y2` (used for `step <= 1.0`;
/// upward shifts `step > 1.0` use the band-limited Kaiser-windowed sinc kernel).
fn hermite(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}
