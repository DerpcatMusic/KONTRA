//! Voices: envelopes, fades, source windows and the per-block render kernel.

use super::{
    EventId, MAX_BLOCK,
    bank::{Bank, Span},
    map::{FOREVER, PlayMap, Run},
    stream::Slot,
};
use crate::audio::Frame;
use std::time::{Duration, Instant};

/// Highest playback increment (source frames per output frame): a 192 kHz
/// sample three octaves up at 48 kHz.
pub(crate) const MAX_STEP: f64 = 32.0;
/// Source frames one block can read: the pitched span plus interpolation taps.
pub(crate) const WINDOW: usize = MAX_BLOCK * MAX_STEP as usize + 8;
/// 1.0 in the kernel's 32.32 fixed-point positions.
const FIXED_ONE: f64 = (1u64 << 32) as f64;
/// Envelope level treated as silence (−80 dB); decays below it end the voice.
const SILENT: f32 = 1e-4;
/// Offline renders wait at most this long for one streamed window.
const OFFLINE_WAIT: Duration = Duration::from_secs(5);

/// Attack-hold-decay-sustain-release amplitude envelope, times in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ahdsr {
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    /// Linear sustain level, 0–1.
    pub sustain: f32,
    pub release: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Done,
}

/// Linear attack, then exponential decay and release (−60 dB over the stage
/// time, like Kontakt's AHDSR).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Envelope {
    stage: Stage,
    level: f32,
    attack: f32,
    hold: u32,
    decay: f32,
    sustain: f32,
    release: f32,
}

/// Per-frame multiplier that reaches −60 dB after `seconds`.
fn exp_coef(seconds: f32, rate: f32) -> f32 {
    if seconds > 0.0 {
        0.001f32.powf(1.0 / (seconds * rate))
    } else {
        0.0
    }
}

impl Envelope {
    pub fn new(p: &Ahdsr, rate: f32) -> Self {
        Self {
            stage: Stage::Attack,
            level: 0.0,
            attack: if p.attack > 0.0 {
                1.0 / (p.attack * rate)
            } else {
                1.0
            },
            hold: (p.hold.max(0.0) * rate) as u32,
            decay: exp_coef(p.decay, rate),
            sustain: p.sustain.clamp(0.0, 1.0),
            release: exp_coef(p.release, rate),
        }
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Done {
            self.stage = Stage::Release;
        }
    }

    pub fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    /// Write one gain per frame.
    pub fn render(&mut self, out: &mut [f32]) {
        let mut i = 0;
        while i < out.len() {
            let rest = &mut out[i..];
            let written = match self.stage {
                Stage::Attack => {
                    let mut n = 0;
                    for o in rest.iter_mut() {
                        self.level = (self.level + self.attack).min(1.0);
                        *o = self.level;
                        n += 1;
                        if self.level >= 1.0 {
                            self.stage = Stage::Hold;
                            break;
                        }
                    }
                    n
                }
                Stage::Hold => {
                    let n = rest.len().min(self.hold as usize);
                    rest[..n].fill(self.level);
                    self.hold -= n as u32;
                    if self.hold == 0 {
                        self.stage = Stage::Decay;
                    }
                    n
                }
                Stage::Decay => {
                    let mut n = 0;
                    for o in rest.iter_mut() {
                        self.level = self.sustain + (self.level - self.sustain) * self.decay;
                        *o = self.level;
                        n += 1;
                        if self.level - self.sustain <= SILENT {
                            self.level = self.sustain;
                            self.stage = Stage::Sustain;
                            break;
                        }
                    }
                    n
                }
                Stage::Sustain if self.level > SILENT => {
                    rest.fill(self.level);
                    rest.len()
                }
                Stage::Release => {
                    let mut n = 0;
                    for o in rest.iter_mut() {
                        self.level *= self.release;
                        *o = self.level;
                        n += 1;
                        if self.level < SILENT {
                            self.stage = Stage::Done;
                            break;
                        }
                    }
                    n
                }
                Stage::Sustain | Stage::Done => {
                    self.stage = Stage::Done;
                    self.level = 0.0;
                    rest.fill(0.0);
                    rest.len()
                }
            };
            i += written;
        }
    }
}

/// Linear gain ramp for steals, chokes and scripted fades.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fade {
    value: f32,
    target: f32,
    step: f32,
    left: u32,
    /// End the voice once the ramp reaches silence.
    stop: bool,
}

impl Fade {
    pub const FULL: Self = Self {
        value: 1.0,
        target: 1.0,
        step: 0.0,
        left: 0,
        stop: false,
    };

    pub fn start(&mut self, target: f32, frames: u32, stop: bool) {
        self.target = target;
        self.stop = stop;
        self.left = frames;
        if frames == 0 {
            self.value = target;
        } else {
            self.step = (target - self.value) / frames as f32;
        }
    }

    /// Restart from silence and rise to unity.
    pub fn fade_in(&mut self, frames: u32) {
        self.value = 0.0;
        self.start(1.0, frames, false);
    }

    pub fn dying(&self) -> bool {
        self.stop && self.target <= 0.0
    }

    pub fn finished(&self) -> bool {
        self.dying() && self.left == 0
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    fn apply(&mut self, amp: &mut [f32]) {
        if self.left == 0 {
            if self.value != 1.0 {
                amp.iter_mut().for_each(|a| *a *= self.value);
            }
            return;
        }
        for a in amp {
            if self.left > 0 {
                self.left -= 1;
                self.value = if self.left == 0 {
                    self.target
                } else {
                    self.value + self.step
                };
            }
            *a *= self.value;
        }
    }
}

/// A voice's streaming slot and how far its data is known to be published.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stream {
    pub slot: u16,
    pub tag: u16,
    pub trusted: u64,
}

/// One playing zone. Plain data; the engine owns the storage.
pub(crate) struct Voice {
    pub event: EventId,
    pub voice_group: Option<u16>,
    pub channel: u8,
    pub note: u8,
    pub velocity: u8,
    /// The key or event is still down.
    pub held: bool,
    /// The envelope has been released.
    pub released: bool,
    /// Started by a note release; ignores later note-offs and pedal changes.
    pub release_trigger: bool,
    pub age: u64,
    pub sample: u32,
    pub span: u32,
    pub map: PlayMap,
    pub wraps: u64,
    /// Virtual path length for the current wraps.
    pub length: u64,
    /// Virtual frames below this are resident; the rest stream.
    pub limit: u64,
    /// Virtual playback position.
    pub pos: f64,
    /// Source frames per output frame before bend and scripted tuning.
    pub step: f64,
    /// Scripted tuning ratio.
    pub tune: f64,
    pub bend_range: f32,
    /// Cached `(bend input, ratio)`.
    pub bend: (f32, f64),
    pub stream: Option<Stream>,
    pub env: Envelope,
    pub fade: Fade,
    /// Static gain without scripted volume.
    pub base_level: f32,
    pub volume: f32,
    pub base_pan: f32,
    pub pan: f32,
    /// Channel gains reached at the end of the last block.
    pub gains: [f32; 2],
}

/// Borrowed state shared by all voices of one render block.
pub(crate) struct Context<'a> {
    pub bank: &'a Bank,
    pub slots: &'a [Slot],
    pub bend: &'a [f32; 16],
    pub blocking: bool,
}

/// Preallocated per-engine render buffers.
pub(crate) struct Scratch {
    pub window: Box<[Frame]>,
    pub amp: [f32; MAX_BLOCK],
}

impl Default for Scratch {
    fn default() -> Self {
        Self {
            window: vec![[0.0; 2]; WINDOW].into_boxed_slice(),
            amp: [0.0; MAX_BLOCK],
        }
    }
}

/// Balance law shared with the rack: the far side attenuates linearly.
#[inline]
pub(crate) fn balance(gain: f32, pan: f32) -> [f32; 2] {
    [gain * (1.0 - pan.max(0.0)), gain * (1.0 + pan.min(0.0))]
}

impl Voice {
    /// Mix one block (at most [`MAX_BLOCK`] frames) into `left`/`right`.
    /// Returns `(alive, underrun)`.
    pub fn render(
        &mut self,
        cx: &Context,
        scratch: &mut Scratch,
        left: &mut [f32],
        right: &mut [f32],
    ) -> (bool, bool) {
        let n = left.len().min(right.len()).min(MAX_BLOCK);
        let amp = &mut scratch.amp[..n];
        self.env.render(amp);
        self.fade.apply(amp);

        let bend = cx.bend[self.channel as usize];
        if bend != self.bend.0 {
            self.bend = (
                bend,
                2f64.powf(f64::from(bend) * f64::from(self.bend_range) / 12.0),
            );
        }
        let step = (self.step * self.tune * self.bend.1).min(MAX_STEP);

        // The window starts one frame before the position for the cubic's left tap.
        let first = self.pos as i64 - 1;
        // 32.32 fixed point inside the window: exact, cheap to index.
        let base = ((self.pos - first as f64) * FIXED_ONE) as u64;
        let step = (step * FIXED_ONE) as u64;
        let count = ((base + step * (n as u64 - 1)) >> 32) as usize + 4;
        let mut underrun = false;
        let window = match self.resident_window(cx.bank, first, count) {
            Some(window) => window,
            None => {
                let window = &mut scratch.window[..count];
                underrun = self.gather(cx, first, window);
                window
            }
        };

        let target = balance(self.base_level * self.volume, self.pan);
        let delta = [
            (target[0] - self.gains[0]) / n as f32,
            (target[1] - self.gains[1]) / n as f32,
        ];
        mix(
            window,
            base,
            step,
            amp,
            self.gains,
            delta,
            &mut left[..n],
            &mut right[..n],
        );
        self.gains = target;

        self.pos += (step * n as u64) as f64 / FIXED_ONE;
        if let Some(stream) = &self.stream {
            cx.slots[stream.slot as usize].release_below((self.pos as u64).saturating_sub(1));
        }
        let alive = !self.env.done() && !self.fade.finished() && self.pos < self.length as f64;
        (alive, underrun)
    }

    /// Fast path: the window is one contiguous, unblended resident run.
    fn resident_window<'b>(&self, bank: &'b Bank, first: i64, count: usize) -> Option<&'b [Frame]> {
        let v = u64::try_from(first).ok()?;
        if v + count as u64 > self.limit {
            return None;
        }
        let run = self.map.run(v, self.wraps)?;
        if run.reverse || run.blend.is_some() || run.len < count as u64 {
            return None;
        }
        let span = self.span(bank);
        let start = (run.frame - span.start) as usize;
        span.data.get(start..start + count)
    }

    fn span<'b>(&self, bank: &'b Bank) -> &'b Span {
        &bank.samples[self.sample as usize].spans[self.span as usize]
    }

    /// Assemble the window from resident runs and the stream ring; returns
    /// true if streamed frames were missing (they play as silence).
    fn gather(&mut self, cx: &Context, first: i64, out: &mut [Frame]) -> bool {
        let lead = usize::try_from(-first).unwrap_or(0).min(out.len());
        out[..lead].fill([0.0; 2]);
        let mut i = lead;
        let mut underrun = false;
        while i < out.len() {
            let v = (first + i as i64) as u64;
            let rest = &mut out[i..];
            let n = if v < self.limit {
                let Some(run) = self.map.run(v, self.wraps) else {
                    rest.fill([0.0; 2]);
                    break;
                };
                let n = run.len.min(self.limit - v).min(rest.len() as u64) as usize;
                copy_run(self.span(cx.bank), &run, &mut rest[..n]);
                n
            } else {
                let n = self.length.saturating_sub(v).min(rest.len() as u64) as usize;
                if n == 0 {
                    rest.fill([0.0; 2]);
                    break;
                }
                underrun |= self.copy_streamed(cx, v, &mut rest[..n]);
                n
            };
            i += n;
        }
        underrun
    }

    fn copy_streamed(&mut self, cx: &Context, v: u64, out: &mut [Frame]) -> bool {
        let Some(stream) = &mut self.stream else {
            out.fill([0.0; 2]);
            return true;
        };
        let slot = &cx.slots[stream.slot as usize];
        let need = v + out.len() as u64;
        let refresh = |stream: &mut Stream| {
            if let Some(end) = slot.published(stream.tag) {
                stream.trusted = stream.trusted.max(end);
            }
        };
        refresh(stream);
        if cx.blocking && stream.trusted < need {
            let deadline = Instant::now() + OFFLINE_WAIT;
            while stream.trusted < need && Instant::now() < deadline {
                std::thread::yield_now();
                refresh(stream);
            }
        }
        let ready = stream.trusted.saturating_sub(v).min(out.len() as u64) as usize;
        slot.copy(v, &mut out[..ready]);
        out[ready..].fill([0.0; 2]);
        ready < out.len()
    }

    /// Release the envelope and, for release-ended loops, re-plan the path.
    /// Slots come from and return to `free`.
    pub fn release(&mut self, bank: &Bank, free: &mut Vec<u16>) {
        if self.released {
            return;
        }
        self.released = true;
        self.held = false;
        self.env.release();
        let wraps = self.map.wraps(self.pos as u64 + 3);
        if wraps == self.wraps {
            return;
        }
        let diverge = self.map.divergence(wraps);
        self.wraps = wraps;
        self.length = self.map.len(wraps);
        let span = self.span(bank);
        self.limit = self.map.resident_limit(wraps, span.start, span.end());
        let slots = bank.slots();
        if self.limit == FOREVER {
            if let Some(stream) = self.stream.take() {
                slots[stream.slot as usize].stop();
                free.push(stream.slot);
            }
            return;
        }
        let (slot, trusted) = match self.stream {
            Some(stream) => (stream.slot, stream.trusted),
            None => match free.pop() {
                Some(slot) => (slot, self.limit),
                None => return,
            },
        };
        // Published frames before the divergence stay valid; restart after them.
        let from = self.limit.max(trusted.min(diverge));
        let tag =
            slots[slot as usize].configure(self.sample, &self.map, wraps, from, self.pos as u64);
        self.stream = Some(Stream {
            slot,
            tag,
            trusted: from,
        });
    }
}

/// Copy a resident run into `out`, applying reverse order and loop crossfades.
fn copy_run(span: &Span, run: &Run, out: &mut [Frame]) {
    let n = out.len();
    let at = |frame: u64| frame.checked_sub(span.start).map(|f| f as usize);
    if run.reverse {
        let Some(top) = at(run.frame).filter(|&t| t + 1 >= n && t < span.data.len()) else {
            out.fill([0.0; 2]);
            return;
        };
        out.iter_mut()
            .zip(span.data[top + 1 - n..=top].iter().rev())
            .for_each(|(o, s)| *o = *s);
        return;
    }
    let Some(src) = at(run.frame).and_then(|s| span.data.get(s..s + n)) else {
        out.fill([0.0; 2]);
        return;
    };
    out.copy_from_slice(src);
    if let Some(blend) = run.blend {
        let Some(partners) = at(blend.partner).and_then(|p| span.data.get(p..p + n)) else {
            return;
        };
        for (i, (o, p)) in out.iter_mut().zip(partners).enumerate() {
            *o = blend.apply(i as u64, *o, *p);
        }
    }
}

/// 4-point, 3rd-order Hermite (Catmull-Rom) interpolation between `q[1]` and `q[2]`.
#[inline(always)]
fn hermite(q: &[Frame; 4], t: f32) -> Frame {
    std::array::from_fn(|c| {
        let (xm1, x0, x1, x2) = (q[0][c], q[1][c], q[2][c], q[3][c]);
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * t + c2) * t + c1) * t + x0
    })
}

/// The inner loop: resample `window` from `base` by `step` (32.32 fixed
/// point), apply per-frame amplitude and ramped channel gains, and accumulate.
/// Dispatches to an AVX2/FMA build of the same code when the CPU has it
/// (about 15% faster than SSE2; an AVX-512 build measured no better).
#[allow(clippy::too_many_arguments)]
fn mix(
    window: &[Frame],
    base: u64,
    step: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
) {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
        // SAFETY: the running CPU supports every feature `mix_avx2` is compiled for.
        return unsafe { mix_avx2(window, base, step, amp, gains, delta, left, right) };
    }
    mix_body(window, base, step, amp, gains, delta, left, right);
}

#[cfg(target_arch = "x86_64")]
#[allow(clippy::too_many_arguments)]
#[target_feature(enable = "avx2,fma")]
fn mix_avx2(
    window: &[Frame],
    base: u64,
    step: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
) {
    mix_body(window, base, step, amp, gains, delta, left, right);
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn mix_body(
    window: &[Frame],
    base: u64,
    step: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
) {
    for (i, ((l, r), a)) in left.iter_mut().zip(right.iter_mut()).zip(amp).enumerate() {
        let p = base + step * i as u64;
        let j = (p >> 32) as usize;
        let t = ((p as u32) >> 8) as f32 * (1.0 / (1 << 24) as f32);
        let Some(q) = window.get(j - 1..).and_then(<[Frame]>::first_chunk::<4>) else {
            break;
        };
        let [yl, yr] = hermite(q, t);
        let fi = i as f32;
        *l += yl * a * (gains[0] + delta[0] * fi);
        *r += yr * a * (gains[1] + delta[1] * fi);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_stages() {
        let rate = 1000.0;
        let p = Ahdsr {
            attack: 0.01,
            hold: 0.005,
            decay: 0.1,
            sustain: 0.5,
            release: 0.1,
        };
        let mut env = Envelope::new(&p, rate);
        let mut out = [0.0; 400];
        env.render(&mut out);
        assert!((out[4] - 0.5).abs() < 1e-5, "linear attack");
        let peak = out.iter().position(|&x| x == 1.0).unwrap();
        assert!((9..=10).contains(&peak));
        assert!(out[peak..=peak + 5].iter().all(|&x| x == 1.0), "hold");
        // −60 dB of the distance to sustain after the decay time.
        assert!(
            (out[peak + 105] - 0.5005).abs() < 1e-5,
            "{}",
            out[peak + 105]
        );
        assert_eq!(out[399], 0.5);
        env.release();
        env.render(&mut out[..100]);
        assert!(
            (out[99] - 0.0005).abs() < 1e-4,
            "release reaches −60 dB at its time"
        );
        env.render(&mut out[..100]);
        assert!(env.done());
    }
}
