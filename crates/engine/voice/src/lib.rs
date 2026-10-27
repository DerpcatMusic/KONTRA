//! One sampler voice: cursor, interpolation, envelope, gain (02-ENGINE.md §5).
//!
//! A voice never waits for storage. It reads through `kontra-stream`; frames
//! not in memory are silence and counted, and the cursor keeps moving, so a
//! note is never late and never drifts (rule 4).

mod envelope;
mod zone;

use kontra_stream::StreamClient;

pub use envelope::Ahdsr;
pub use zone::{Loop, Region};

use envelope::Envelope;

/// Source frames decoded per inner chunk. Bounds per-voice memory, not pitch.
const SCRATCH: usize = 512;
/// Highest pitch step (source frames per output frame): +4 octaves at 4× rate.
pub const MAX_STEP: f64 = 64.0;
/// Output frames of source asked for ahead of the cursor each render.
pub const LOOKAHEAD: f64 = 8192.0;
/// One-pole gain smoothing per frame (~2 ms at 48 kHz).
const GAIN_SMOOTH: f32 = 0.01;

/// Everything a voice needs to start, resolved by the engine from the plan.
#[derive(Clone)]
pub struct VoiceStart {
    pub region: Region,
    /// Source frames per output frame: pitch and sample-rate ratio together.
    pub step: f64,
    pub gain: [f32; 2],
    pub envelope: Ahdsr,
}

pub struct Voice {
    region: Option<Region>,
    /// Position at `elapsed == 0`; moved only when the step changes, so the
    /// cursor is the same whatever block sizes rendered it (rule 3).
    anchor: f64,
    elapsed: u64,
    step: f64,
    gain: [f32; 2],
    target: [f32; 2],
    env: Envelope,
    scratch: Box<[[f32; 2]]>,
    /// The frames a loop crossfade blends in.
    partner: Box<[[f32; 2]]>,
    missing: u64,
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
            gain: [0.0; 2],
            target: [0.0; 2],
            env: Envelope::new(Ahdsr::GATE),
            scratch: vec![[0.0; 2]; SCRATCH].into_boxed_slice(),
            partner: vec![[0.0; 2]; SCRATCH].into_boxed_slice(),
            missing: 0,
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
        self.env = Envelope::new(start.envelope);
        self.missing = 0;
    }

    pub fn release(&mut self) {
        self.env.release();
    }

    /// Fades out over `frames` (a stolen or choked voice): no click, no tail.
    pub fn fade(&mut self, frames: u32) {
        self.env.fade(frames);
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
    pub fn set_gain(&mut self, gain: [f32; 2]) {
        self.target = gain;
    }

    /// The gain the voice is moving to, left and right.
    #[must_use]
    pub fn target_gain(&self) -> [f32; 2] {
        self.target
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.region.is_some()
    }

    /// Source frames this voice read while their data was not in memory.
    #[must_use]
    pub fn missing_frames(&self) -> u64 {
        self.missing
    }

    /// Frames played so far, in source frames from the region start.
    #[must_use]
    pub fn position(&self) -> f64 {
        self.at(0)
    }

    /// Cursor position `ahead` output frames from now.
    fn at(&self, ahead: u64) -> f64 {
        self.anchor + (self.elapsed + ahead) as f64 * self.step
    }

    /// Mixes the next `out.len()` frames into `out`. `now` is the sample time of
    /// `out[0]`, the deadline for the pages this render asks the store for.
    pub fn render(&mut self, client: &StreamClient, now: u64, out: &mut [[f32; 2]]) {
        let Some(region) = self.region.take() else {
            return;
        };
        let sample = &region.sample;
        let ahead = (LOOKAHEAD * self.step) as usize;
        region.runs(self.at(0) as i64, ahead, |_, first, count| {
            let low = if region.reverse {
                first + 1 - count as u64
            } else {
                first
            };
            sample.prefetch(client, low, count as u64, now);
            if let Some((_, partner, n, _, _)) = region.crossfade(first, count) {
                sample.prefetch(client, partner, n as u64, now);
            }
        });

        let mut done = 0;
        let mut alive = true;
        while done < out.len() && alive {
            let fit = (((SCRATCH - 4) as f64 / self.step).floor() as usize).max(1);
            let n = fit.min(out.len() - done);
            let lo = self.at(0).floor() as i64 - 1;
            let span = (self.at(n as u64 - 1).floor() as i64 + 3 - lo) as usize;
            let (anchor, elapsed, step) = (self.anchor, self.elapsed, self.step);
            let partner = &mut self.partner;
            let Some(window) = self.scratch.get_mut(..span.min(SCRATCH)) else {
                break;
            };
            window.fill([0.0; 2]);
            let mut missing = 0;
            region.runs(lo, window.len(), |offset, first, count| {
                if let Some(dst) = window.get_mut(offset..offset + count) {
                    if region.reverse {
                        missing += sample.read(client, first + 1 - count as u64, dst).missing;
                        dst.reverse();
                    } else {
                        missing += sample.read(client, first, dst).missing;
                        if let Some((at, from, n, fade_start, fade)) =
                            region.crossfade(first, count)
                            && let (Some(blend), Some(head)) =
                                (partner.get_mut(..n), dst.get_mut(at..at + n))
                        {
                            missing += sample.read(client, from, blend).missing;
                            let p0 = first + at as u64 - fade_start;
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

            for (i, frame) in out.iter_mut().skip(done).take(n).enumerate() {
                let x = anchor + (elapsed + i as u64) as f64 * step - lo as f64;
                let base = x.floor() as usize;
                let t = (x - base as f64) as f32;
                let Some(&[y0, y1, y2, y3]) = window.get(base - 1..base + 3) else {
                    break;
                };
                let level = self.env.next();
                self.gain[0] += (self.target[0] - self.gain[0]) * GAIN_SMOOTH;
                self.gain[1] += (self.target[1] - self.gain[1]) * GAIN_SMOOTH;
                frame[0] += hermite(y0[0], y1[0], y2[0], y3[0], t) * level * self.gain[0];
                frame[1] += hermite(y0[1], y1[1], y2[1], y3[1], t) * level * self.gain[1];
                if self.env.is_off() {
                    alive = false;
                    break;
                }
            }
            self.elapsed += n as u64;
            if region.finished(self.at(0)) {
                alive = false;
            }
            done += n;
        }
        if alive {
            self.region = Some(region);
        }
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
