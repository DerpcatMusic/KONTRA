//! One sampler voice: cursor, interpolation, envelope, gain (02-ENGINE.md §5).
//!
//! A voice never waits for storage. It reads through `kontra-stream`; frames
//! not in memory are silence and counted, and the cursor keeps moving, so a
//! note is never late and never drifts (rule 4).

mod envelope;
mod fade;
mod wavetable;
mod zone;

use kontra_stream::{Mix, StreamClient};

pub use envelope::Ahdsr;
pub use wavetable::{
    CYCLE, LEVELS, PERIOD, Read, SIZE, Shape, Shaper, Side, TOP, Table, Warp, shape, warp,
};
pub use zone::{Loop, Loops, Region};

use envelope::Envelope;
use fade::Fade;
pub use fade::{FadeCurve, FadeTarget};

/// Source frames decoded per inner chunk. Bounds per-voice memory, not pitch.
const SCRATCH: usize = 512;
/// Highest pitch step (source frames per output frame): +4 octaves at 4× rate.
pub const MAX_STEP: f64 = 64.0;
/// Output frames of source asked for ahead of the cursor each render.
pub const LOOKAHEAD: f64 = 8192.0;
/// One-pole gain smoothing per frame (~2 ms at 48 kHz).
const GAIN_SMOOTH: f32 = 0.01;
/// The fastest a stretching voice plays (the stretch law's top).
const MAX_SPEED: f64 = 8.0;
/// A stretching voice's grain, output frames (43 ms at 48 kHz).
const GRAIN: u64 = 2048;
const HALF_GRAIN: u64 = GRAIN / 2;

/// How each output channel (row) takes each source channel (column).
pub type Gain = [[f32; 2]; 2];

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
    /// Source frames per output frame: pitch and sample-rate ratio together.
    pub step: f64,
    pub gain: Gain,
    /// A slower gain on top of `gain`: the routes that lag.
    pub lag: Lag,
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
    /// Position at `elapsed == 0`; moved only when the step changes, so the
    /// cursor is the same whatever block sizes rendered it (rule 3).
    anchor: f64,
    elapsed: u64,
    step: f64,
    gain: Gain,
    target: Gain,
    lagged: f32,
    lag: Lag,
    env: Envelope,
    fade: Fade,
    life: Life,
    scratch: Box<[[f32; 2]]>,
    /// The frames a loop crossfade blends in.
    partner: Box<[[f32; 2]]>,
    /// Interpolated frames before the envelope and gain.
    raw: Box<[[f32; 2]]>,
    /// Cursor position of the note's release, once it came.
    released: Option<u64>,
    missing: u64,
    /// See [`VoiceStart::speed`]; the cursor moves at `step · speed`.
    speed: f64,
    /// A stretching voice's two read heads, each a pitch-rate cursor that
    /// restarts at the cursor once a grain, half a grain apart.
    heads: [f64; 2],
    /// Frames since the stretch began, which places its grains.
    grain: u64,
    /// Head 0's frames while head 1 reads.
    head: Box<[[f32; 2]]>,
    #[cfg(feature = "trace")]
    last_peak: f32,
    #[cfg(feature = "trace")]
    offline_wait: bool,
    #[cfg(feature = "trace")]
    offline_wait_timeouts: u64,
}

impl Voice {
    /// Allocates the voice's scratch; call at prepare, never on the audio thread.
    #[must_use]
    pub fn allocate() -> Self {
        Self {
            region: None,
            anchor: 0.0,
            elapsed: 0,
            step: 1.0,
            gain: [[0.0; 2]; 2],
            target: [[0.0; 2]; 2],
            lagged: 1.0,
            lag: Lag::NONE,
            env: Envelope::new(Ahdsr::GATE),
            fade: Fade::NONE,
            life: Life::Held,
            scratch: vec![[0.0; 2]; SCRATCH].into_boxed_slice(),
            partner: vec![[0.0; 2]; SCRATCH].into_boxed_slice(),
            raw: vec![[0.0; 2]; SCRATCH].into_boxed_slice(),
            released: None,
            missing: 0,
            speed: 1.0,
            heads: [0.0; 2],
            grain: 0,
            head: vec![[0.0; 2]; SCRATCH].into_boxed_slice(),
            #[cfg(feature = "trace")]
            last_peak: 0.0,
            #[cfg(feature = "trace")]
            offline_wait: false,
            #[cfg(feature = "trace")]
            offline_wait_timeouts: 0,
        }
    }

    /// Starts playing. No allocation. The previous region's sample reference is
    /// dropped here, so the plan must keep its samples alive until it retires.
    pub fn start(&mut self, start: VoiceStart) {
        self.region = Some(start.region);
        self.anchor = 0.0;
        self.elapsed = 0;
        self.step = clamp_step(start.step);
        self.gain = start.gain;
        self.target = start.gain;
        self.lagged = start.lag.gain;
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
        self.heads = [0.0; 2];
        self.grain = 0;
        #[cfg(feature = "trace")]
        {
            self.last_peak = 0.0;
        }
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
    }

    /// Changes pitch from the next frame.
    pub fn set_step(&mut self, step: f64) {
        self.anchor = self.at(0);
        self.elapsed = 0;
        self.step = clamp_step(step);
    }

    /// Changes gain, smoothed over a few milliseconds.
    pub fn set_gain(&mut self, gain: Gain) {
        self.target = gain;
    }

    /// Changes the lagging gain, gliding at its own pace.
    pub fn set_lag(&mut self, lag: Lag) {
        self.lag = lag;
    }

    /// Changes the channel matrix (live cells) from the next chunk read.
    ///
    /// ponytail: a step at each refresh, not smoothed like the gain; ramp it
    /// per chunk if a fast sweep zippers.
    pub fn set_mix(&mut self, mix: Mix) {
        if let Some(r) = self.region.as_mut() {
            r.mix = Some(mix);
        }
    }

    /// Where a wavetable voice reads its table from now on; others ignore it.
    // ponytail: steps at each control span; Kontakt ramps over 32 frames
    // (0x102ac): ramp inside `Table::render` if a fast route steps audibly.
    pub fn set_read(&mut self, read: Read) {
        if let Some(table) = self.region.as_mut().and_then(|r| r.table.as_mut()) {
            table.read = read;
        }
    }

    /// Current gain matrix including its envelope and lag, before sample and effects.
    #[must_use]
    pub fn current_gain(&self) -> Gain {
        let level = self.lagged * self.env.level() * self.fade.level();
        self.gain.map(|row| row.map(|gain| gain * level))
    }

    /// The gain the voice is moving to, its lagging gain included.
    #[must_use]
    pub fn target_gain(&self) -> Gain {
        self.target.map(|row| row.map(|g| g * self.lag.gain))
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

    /// Peak of this voice's last render call before group effects, when tracing is enabled.
    #[cfg(feature = "trace")]
    #[must_use]
    pub fn last_peak(&self) -> f32 {
        self.last_peak
    }

    /// Enable blocking page waits for an offline renderer; never call from a live host.
    #[cfg(feature = "trace")]
    pub fn set_offline_wait(&mut self, enabled: bool) {
        self.offline_wait = enabled;
    }

    /// How many offline page waits timed out, including previous notes in this slot.
    #[cfg(feature = "trace")]
    #[must_use]
    pub fn offline_wait_timeouts(&self) -> u64 {
        self.offline_wait_timeouts
    }

    /// Source frames this voice read while their data was not in memory.
    #[must_use]
    pub fn missing_frames(&self) -> u64 {
        self.missing
    }

    /// Source frames per output frame at the current pitch.
    #[must_use]
    pub fn step(&self) -> f64 {
        self.step
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
        region.frame_at(self.at(0).max(0.) as u64, self.released)
    }

    /// Cursor position `ahead` output frames from now.
    fn at(&self, ahead: u64) -> f64 {
        self.anchor + (self.elapsed + ahead) as f64 * self.step * self.speed
    }

    /// Mixes the next `out.len()` frames into `out`. `now` is the sample time of
    /// `out[0]`, the deadline for the pages this render asks the store for.
    pub fn render(&mut self, client: &StreamClient, now: u64, out: &mut [[f32; 2]]) {
        self.render_through(client, now, out, |_| {});
    }

    /// As [`Self::render`], with `insert` run on the interpolated signal
    /// before the envelope and gain (a voice's own effects), in chunks of at
    /// most 512 frames.
    pub fn render_through(
        &mut self,
        client: &StreamClient,
        now: u64,
        out: &mut [[f32; 2]],
        mut insert: impl FnMut(&mut [[f32; 2]]),
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
        let step = self.step * self.speed;
        // A stretching voice's heads trail or lead the cursor by up to a grain.
        let from = if self.speed == 1.0 {
            self.at(0)
        } else {
            self.heads.iter().fold(self.at(0), |a, &h| a.min(h))
        };
        let ahead = (LOOKAHEAD * step) as usize + (self.at(0) - from) as usize;
        let table = region.table.as_ref();
        let ahead = if table.is_some() { 0 } else { ahead };
        region.runs(from as i64, ahead, released, |offset, run| {
            let low = if run.descending {
                run.first + 1 - run.count as u64
            } else {
                run.first
            };
            // The voice reaches this run `offset` source frames from now.
            let due = now + (offset as f64 / step) as u64;
            let count = run.count as u64;
            sample.prefetch(client, low, count, due, step, run.descending);
            if let Some((at, partner, n, _, _)) = run.crossfade() {
                let due = due + (at as f64 / step) as u64;
                sample.prefetch(client, partner, n as u64, due, step, false);
            }
        });

        let mut done = 0;
        let mut alive = true;
        while done < out.len() && alive {
            let fit = if table.is_some() {
                SCRATCH
            } else {
                (((SCRATCH - 4) as f64 / self.step).floor() as usize).max(1)
            };
            let mut n = fit.min(out.len() - done);
            let stretched = self.speed != 1.0;
            if stretched {
                // Heads restart only at a chunk's start.
                n = n.min((HALF_GRAIN - self.grain % HALF_GRAIN) as usize);
            }
            let filled = if let Some(table) = table {
                let cursor = (self.at(0), self.step);
                let raw = self.raw.get_mut(..n).unwrap_or_default();
                table.render(cursor, raw);
                raw.len()
            } else if stretched {
                self.read_stretched(client, &region, n)
            } else {
                self.read_window(client, &region, n, (self.anchor, self.elapsed))
            };
            let raw = self.raw.get_mut(..filled).unwrap_or_default();
            insert(raw);
            for (frame, &[l, r]) in out.iter_mut().skip(done).zip(raw.iter()) {
                self.lagged += (self.lag.gain - self.lagged) * self.lag.coef;
                let level = self.env.next() * self.fade.next() * self.lagged;
                for (g, target) in self
                    .gain
                    .as_flattened_mut()
                    .iter_mut()
                    .zip(self.target.as_flattened())
                {
                    *g += (target - *g) * GAIN_SMOOTH;
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
            }
            self.elapsed += n as u64;
            if table.is_none() && region.finished(self.at(0), released) {
                alive = false;
            }
            done += n;
        }
        if alive {
            self.region = Some(region);
        }
    }

    /// `n` frames of a stretching voice into `raw`: its two heads read at the
    /// pitch's rate, each faded in and out over its grain (triangles that sum
    /// to one), so the cursor moves at `speed` while the pitch holds. Head 0
    /// plays the note's start in full.
    // ponytail: a plain two-head overlap at a fixed 2048-frame grain, no
    // waveform alignment; Kontakt's stretch is smoother on sustained tones.
    fn read_stretched(&mut self, client: &StreamClient, region: &Region, n: usize) -> usize {
        let at = self.at(0);
        if self.grain % GRAIN == HALF_GRAIN {
            self.heads[0] = at;
        }
        if self.grain.is_multiple_of(GRAIN) {
            self.heads[1] = at;
        }
        let a = self.read_window(client, region, n, (self.heads[0], 0));
        if let (Some(head), Some(raw)) = (self.head.get_mut(..a), self.raw.get(..a)) {
            head.copy_from_slice(raw);
        }
        let filled = a.min(self.read_window(client, region, n, (self.heads[1], 0)));
        let grain = self.grain;
        let pairs = self.raw.iter_mut().zip(self.head.iter()).take(filled);
        for (i, (b, a)) in pairs.enumerate() {
            let phase = ((grain + HALF_GRAIN + i as u64) % GRAIN) as f32 / GRAIN as f32;
            let w = 1.0 - (2.0 * phase - 1.0).abs();
            *b = [w * a[0] + (1.0 - w) * b[0], w * a[1] + (1.0 - w) * b[1]];
        }
        for h in &mut self.heads {
            *h += n as f64 * self.step;
        }
        self.grain += n as u64;
        filled
    }

    /// Reads the region about a cursor at the pitch's step, frame `i` at
    /// `base + (frames + i) · step` (one product, so every host block size
    /// lands on the same positions), and interpolates `n` frames into `raw`;
    /// returns how many it filled.
    fn read_window(
        &mut self,
        client: &StreamClient,
        region: &Region,
        n: usize,
        (base, frames): (f64, u64),
    ) -> usize {
        let sample = &region.sample;
        let mix = region.mix.as_ref();
        let released = self.released;
        let step = self.step;
        let at = |i: u64| base + (frames + i) as f64 * step;
        let lo = at(0).floor() as i64 - 1;
        let span = (at(n as u64 - 1).floor() as i64 + 3 - lo) as usize;
        let partner = &mut self.partner;
        let Some(window) = self.scratch.get_mut(..span.min(SCRATCH)) else {
            return 0;
        };
        window.fill([0.0; 2]);
        #[cfg(feature = "trace")]
        if self.offline_wait {
            region.runs(lo, window.len(), released, |_, run| {
                let low = if run.descending {
                    run.first + 1 - run.count as u64
                } else {
                    run.first
                };
                sample.prefetch(client, low, run.count as u64, 0, step, run.descending);
                if let Some((_, partner, n, _, _)) = run.crossfade() {
                    sample.prefetch(client, partner, n as u64, 0, step, false);
                }
            });
            if !client.settle(std::time::Duration::from_secs(5)) {
                self.offline_wait_timeouts += 1;
            }
        }
        let mut missing = 0;
        region.runs(lo, window.len(), released, |offset, run| {
            let count = run.count;
            if let Some(dst) = window.get_mut(offset..offset + count) {
                if run.descending {
                    missing += sample
                        .read_mixed(client, run.first + 1 - count as u64, dst, mix)
                        .missing;
                    dst.reverse();
                } else {
                    missing += sample.read_mixed(client, run.first, dst, mix).missing;
                    if let Some((at, from, n, fade_start, fade)) = run.crossfade()
                        && let (Some(blend), Some(head)) =
                            (partner.get_mut(..n), dst.get_mut(at..at + n))
                    {
                        missing += sample.read_mixed(client, from, blend, mix).missing;
                        let p0 = run.first + at as u64 - fade_start;
                        for (k, (d, b)) in head.iter_mut().zip(blend.iter()).enumerate() {
                            let w = (p0 + k as u64) as f32 / fade as f32;
                            d[0] += (b[0] - d[0]) * w;
                            d[1] += (b[1] - d[1]) * w;
                        }
                    }
                }
            }
        });
        self.missing += u64::from(missing);

        let mut filled = 0;
        for (i, frame) in self.raw.iter_mut().take(n).enumerate() {
            let x = at(i as u64) - lo as f64;
            let base = x.floor() as usize;
            let t = (x - base as f64) as f32;
            let Some(&[y0, y1, y2, y3]) = window.get(base - 1..base + 3) else {
                break;
            };
            *frame = [
                hermite(y0[0], y1[0], y2[0], y3[0], t),
                hermite(y0[1], y1[1], y2[1], y3[1], t),
            ];
            filled += 1;
        }
        filled
    }
}

fn clamp_step(step: f64) -> f64 {
    if step.is_finite() {
        step.clamp(1.0 / 1024.0, MAX_STEP)
    } else {
        1.0
    }
}

/// 4-point, 3rd-order Hermite between `y1` and `y2`.
// ponytail: cubic Hermite for every pitch; a band-limited (sinc) kernel for
// steps above 1 when an aliasing measurement on real pitched-up material needs it.
fn hermite(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}
