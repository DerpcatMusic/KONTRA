//! Voices: envelopes, fades, source windows and the per-block render kernel.

use super::{
    EventId, MAX_BLOCK,
    bank::{Bank, Span},
    filter::VoiceFilter,
    map::{FOREVER, PlayMap, Run},
    params::{Inputs, VOICE_MODS},
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
    /// Attack shape, -1..=1: 0 is linear, positive convex (fast rise),
    /// negative concave (slow start).
    pub curve: f32,
    pub hold: f32,
    pub decay: f32,
    /// Linear sustain level, 0–1.
    pub sustain: f32,
    pub release: f32,
}

impl Ahdsr {
    /// Holds 1 until the voice ends another way: the partner of a lone flex envelope.
    pub const UNITY: Self = Self {
        attack: 0.0,
        curve: 0.0,
        hold: 0.0,
        decay: 0.0,
        sustain: 1.0,
        release: f32::INFINITY,
    };
}

/// Breakpoint (flex) envelope: glides from silence through `points` and
/// holds at `points[sustain]` while the key is down.
#[derive(Clone, Debug, PartialEq)]
pub struct Flex {
    pub points: Box<[FlexPoint]>,
    pub sustain: usize,
}

/// A flex envelope point, reached from the previous level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexPoint {
    pub seconds: f32,
    /// Linear level, 0–1.
    pub level: f32,
    /// Segment shape like [`Ahdsr::curve`]: positive moves fast early.
    pub curve: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    /// Start the glide toward this flex point from the current level.
    Enter(u8),
    /// Gliding toward this flex point.
    Point(u8),
    Done,
}

/// Curved attack, then exponential decay and release (−60 dB over the stage
/// time, like Kontakt's AHDSR); or a flex envelope's curved segments.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Envelope {
    stage: Stage,
    level: f32,
    /// Glide step of the attack or flex segment, `level = level * step.0 +
    /// step.1`: a one-pole glide whose target lies past the end (convex),
    /// before the start (concave), or a line.
    step: (f32, f32),
    /// Frames left in the hold stage or flex segment.
    left: u32,
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

/// Exponent of the attack curve at |curve| = 1: `(1 - e^(-k t)) / (1 - e^(-k))`
/// with `k = CURVE_STEEPNESS * curve`. Kontakt's exact law is unverified.
const CURVE_STEEPNESS: f32 = 5.0;

/// Per-frame step from `from` to `to` over `frames` (at least 1), bent by
/// `curve`.
fn glide(from: f32, to: f32, frames: f32, curve: f32) -> (f32, f32) {
    let k = CURVE_STEEPNESS * curve.clamp(-1.0, 1.0);
    if k.abs() < 1e-3 {
        return (1.0, (to - from) / frames);
    }
    // The shape is a one-pole glide toward `target`, reached asymptotically.
    let target = from + (to - from) / -(-k).exp_m1();
    let mul = (-k / frames).exp();
    (mul, target * (1.0 - mul))
}

impl Envelope {
    pub fn new(p: &Ahdsr, rate: f32) -> Self {
        let attack = p.attack * rate;
        Self {
            stage: Stage::Attack,
            level: 0.0,
            step: if attack > 0.0 {
                glide(0.0, 1.0, attack, p.curve)
            } else {
                (1.0, 1.0)
            },
            left: (p.hold.max(0.0) * rate) as u32,
            decay: exp_coef(p.decay, rate),
            sustain: p.sustain.clamp(0.0, 1.0),
            release: exp_coef(p.release, rate),
        }
    }

    /// A flex envelope's state; its points are passed to `render`.
    pub fn flex() -> Self {
        Self {
            stage: Stage::Enter(0),
            level: 0.0,
            step: (1.0, 0.0),
            left: 0,
            decay: 0.0,
            sustain: 0.0,
            release: 0.0,
        }
    }

    /// Enter the release: the flex segment after the sustain point, or the
    /// AHDSR release.
    pub fn release(&mut self, flex: Option<&Flex>) {
        self.stage = match (self.stage, flex) {
            (Stage::Done, _) => Stage::Done,
            (_, Some(flex)) => Stage::Enter((flex.sustain + 1) as u8),
            _ => Stage::Release,
        };
    }

    pub fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    /// Write one gain per frame; `flex` is the envelope's points, if it is one.
    pub fn render(&mut self, out: &mut [f32], flex: Option<&Flex>, rate: f32) {
        let mut i = 0;
        while i < out.len() {
            let rest = &mut out[i..];
            let written = match self.stage {
                Stage::Attack => {
                    let mut n = 0;
                    for o in rest.iter_mut() {
                        self.level = (self.level * self.step.0 + self.step.1).min(1.0);
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
                    let n = rest.len().min(self.left as usize);
                    rest[..n].fill(self.level);
                    self.left -= n as u32;
                    if self.left == 0 {
                        self.stage = Stage::Decay;
                    }
                    n
                }
                Stage::Enter(i) => {
                    self.stage = match flex.and_then(|f| f.points.get(i as usize)) {
                        Some(p) => {
                            let frames = (p.seconds * rate).round().max(1.0);
                            self.step = glide(self.level, p.level, frames, p.curve);
                            self.left = frames as u32;
                            Stage::Point(i)
                        }
                        None => Stage::Done,
                    };
                    0
                }
                Stage::Point(i) => {
                    let n = rest.len().min(self.left as usize);
                    for o in &mut rest[..n] {
                        self.level = self.level * self.step.0 + self.step.1;
                        *o = self.level;
                    }
                    self.left -= n as u32;
                    if self.left == 0 {
                        let point = flex.and_then(|f| Some((f.sustain, f.points.get(i as usize)?)));
                        self.stage = match point {
                            Some((sustain, p)) => {
                                // Land exactly on the point.
                                self.level = p.level;
                                rest[n - 1] = p.level;
                                if i as usize == sustain {
                                    Stage::Sustain
                                } else {
                                    Stage::Enter(i + 1)
                                }
                            }
                            None => Stage::Done,
                        };
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
    pub group: u32,
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
    /// Source frames per output frame before group tune, modulation and
    /// scripted tuning.
    pub step: f64,
    /// Scripted tuning ratio.
    pub tune: f64,
    /// Cached `(semitones, ratio)` of group, instrument and modulated pitch.
    pub pitch: (f32, f64),
    /// Current value of each of the group's voiced modulation assignments.
    pub mods: [f32; VOICE_MODS],
    pub stream: Option<Stream>,
    /// AHDSR envelope (the engine defaults without one, unity with only a flex).
    pub env: Envelope,
    /// The group's flex envelope, multiplied with `env`.
    pub flex: Option<Envelope>,
    pub fade: Fade,
    /// Zone gain with velocity and key crossfades; group volume and
    /// modulation apply per block.
    pub base_level: f32,
    /// Scripted event volume.
    pub volume: f32,
    /// Zone pan.
    pub base_pan: f32,
    /// Scripted event pan, added to zone and group pan.
    pub pan: f32,
    /// Channel gains reached at the end of the last block.
    pub gains: [f32; 2],
    /// Group insert filter state (untouched when the group has none).
    pub filter: VoiceFilter,
}

/// Borrowed state shared by all voices of one render block.
pub(crate) struct Context<'a> {
    pub bank: &'a Bank,
    pub slots: &'a [Slot],
    pub cc: &'a [[u8; 128]; 16],
    pub bend: &'a [f32; 16],
    pub pressure: &'a [u8; 16],
    /// Instrument tune in semitones.
    pub tune: f32,
    pub rate: f32,
    pub blocking: bool,
}

impl Context<'_> {
    /// Modulation inputs for a note on `channel`.
    pub fn inputs(&self, channel: u8, note: u8, velocity: u8) -> Inputs<'_> {
        let c = channel as usize & 15;
        Inputs {
            cc: &self.cc[c],
            bend: self.bend[c],
            pressure: self.pressure[c],
            note,
            velocity,
        }
    }
}

/// Preallocated per-engine render buffers.
pub(crate) struct Scratch {
    pub window: Box<[Frame]>,
    pub amp: [f32; MAX_BLOCK],
    pub flex: [f32; MAX_BLOCK],
    /// A filtered voice's own output before it joins the mix.
    pub out: [[f32; MAX_BLOCK]; 2],
}

impl Default for Scratch {
    fn default() -> Self {
        Self {
            window: vec![[0.0; 2]; WINDOW].into_boxed_slice(),
            amp: [0.0; MAX_BLOCK],
            flex: [0.0; MAX_BLOCK],
            out: [[0.0; MAX_BLOCK]; 2],
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
        let group = &cx.bank.settings[self.group as usize];
        self.env.render(amp, None, cx.rate);
        if let Some(env) = &mut self.flex {
            let flex = &mut scratch.flex[..n];
            env.render(flex, group.flex.as_ref(), cx.rate);
            amp.iter_mut().zip(flex.iter()).for_each(|(a, f)| *a *= f);
        }
        self.fade.apply(amp);

        let inputs = cx.inputs(self.channel, self.note, self.velocity);
        let (modulation, semitones) = group.mods.modulate(&mut self.mods, &inputs, n, cx.rate);
        let semitones = semitones + group.tune + cx.tune;
        if semitones != self.pitch.0 {
            self.pitch = (semitones, 2f64.powf(f64::from(semitones) / 12.0));
        }
        let step = (self.step * self.tune * self.pitch.1).min(MAX_STEP);

        // The window starts one frame before the position for the cubic's left tap.
        let first = self.pos as i64 - 1;
        // 32.32 fixed point inside the window: exact, cheap to index.
        let base = ((self.pos - first as f64) * FIXED_ONE) as u64;
        let step = (step * FIXED_ONE) as u64;
        let count = ((base + step * (n as u64 - 1)) >> 32) as usize + 4;
        let mut underrun = false;
        let window = match self.resident_window(cx.bank, first, &mut scratch.window[..count]) {
            Some(window) => window,
            None => {
                let window = &mut scratch.window[..count];
                underrun = self.gather(cx, first, window);
                window
            }
        };

        let level = self.base_level * group.gain * modulation * self.volume;
        let target = balance(
            level,
            (self.base_pan + group.pan + self.pan).clamp(-1.0, 1.0),
        );
        let delta = [
            (target[0] - self.gains[0]) / n as f32,
            (target[1] - self.gains[1]) / n as f32,
        ];
        let [out_l, out_r] = &mut scratch.out;
        let (l, r) = match &group.filter {
            Some(_) => {
                out_l[..n].fill(0.0);
                out_r[..n].fill(0.0);
                (&mut out_l[..n], &mut out_r[..n])
            }
            None => (&mut left[..n], &mut right[..n]),
        };
        mix(window, base, step, amp, self.gains, delta, l, r);
        self.gains = target;
        if let Some(filter) = &group.filter {
            let (l, r) = (&mut out_l[..n], &mut out_r[..n]);
            self.filter
                .process(filter, &group.mods, &inputs, &mut scratch.flex, l, r, cx.rate);
            left[..n].iter_mut().zip(l.iter()).for_each(|(o, x)| *o += x);
            right[..n].iter_mut().zip(r.iter()).for_each(|(o, x)| *o += x);
        }

        self.pos += (step * n as u64) as f64 / FIXED_ONE;
        if let Some(stream) = &self.stream {
            cx.slots[stream.slot as usize].release_below((self.pos as u64).saturating_sub(1));
        }
        let alive = !self.env.done()
            && !self.flex.as_ref().is_some_and(Envelope::done)
            && !self.fade.finished()
            && self.pos < self.length as f64;
        (alive, underrun)
    }

    /// Fast path: the window is one contiguous, unblended resident run,
    /// borrowed when stored as f32 and otherwise decoded into `buf`.
    fn resident_window<'b>(
        &self,
        bank: &'b Bank,
        first: i64,
        buf: &'b mut [Frame],
    ) -> Option<&'b [Frame]> {
        let v = u64::try_from(first).ok()?;
        let count = buf.len() as u64;
        if v + count > self.limit {
            return None;
        }
        let run = self.map.run(v, self.wraps)?;
        if run.reverse || run.blend.is_some() || run.len < count {
            return None;
        }
        let span = self.span(bank);
        span.data.window((run.frame - span.start) as usize, buf)
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
        self.env.release(None);
        self.filter.release();
        if let Some(env) = &mut self.flex {
            env.release(bank.settings[self.group as usize].flex.as_ref());
        }
        let wraps = self.map.wraps(self.pos as u64 + 3);
        if wraps == self.wraps {
            return;
        }
        let diverge = self.map.divergence(wraps);
        self.wraps = wraps;
        self.length = self.map.len(wraps);
        let span = self.span(bank);
        let first = (self.pos as u64).saturating_sub(1);
        self.limit = self.map.resident_limit(first, wraps, span.start, span.end());
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
        let decoded = at(run.frame)
            .and_then(|top| (top + 1).checked_sub(n))
            .is_some_and(|low| span.data.decode(low, out));
        if decoded {
            out.reverse();
        } else {
            out.fill([0.0; 2]);
        }
        return;
    }
    if !at(run.frame).is_some_and(|s| span.data.decode(s, out)) {
        out.fill([0.0; 2]);
        return;
    }
    let (Some(blend), Some(partner)) = (run.blend, run.blend.and_then(|b| at(b.partner))) else {
        return;
    };
    // Partners decode in stack-sized chunks: no allocation on the audio thread.
    let mut partners = [[0.0; 2]; 64];
    for (c, chunk) in out.chunks_mut(partners.len()).enumerate() {
        let done = c * partners.len();
        let partners = &mut partners[..chunk.len()];
        if !span.data.decode(partner + done, partners) {
            return;
        }
        for (i, (o, p)) in chunk.iter_mut().zip(partners.iter()).enumerate() {
            *o = blend.apply((done + i) as u64, *o, *p);
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
            curve: 0.0,
            hold: 0.005,
            decay: 0.1,
            sustain: 0.5,
            release: 0.1,
        };
        let mut env = Envelope::new(&p, rate);
        let mut out = [0.0; 400];
        env.render(&mut out, None, rate);
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
        env.release(None);
        env.render(&mut out[..100], None, rate);
        assert!(
            (out[99] - 0.0005).abs() < 1e-4,
            "release reaches −60 dB at its time"
        );
        env.render(&mut out[..100], None, rate);
        assert!(env.done());
    }

    #[test]
    fn attack_curve_bends_and_keeps_its_time() {
        let rate = 1000.0;
        let attack = |curve| {
            let p = Ahdsr {
                attack: 0.1,
                curve,
                hold: 0.0,
                decay: 0.0,
                sustain: 1.0,
                release: 0.1,
            };
            let mut out = [0.0; 120];
            Envelope::new(&p, rate).render(&mut out, None, rate);
            out
        };
        // (1 - e^(-k t)) / (1 - e^(-k)), k = 5 · curve.
        let expected = |k: f32, t: f32| (-k * t).exp_m1() / (-k).exp_m1();
        for curve in [-1.0, -0.33, 0.0, 0.5, 1.0] {
            let out = attack(curve);
            for frame in [19, 49, 79] {
                let t = (frame + 1) as f32 / 100.0;
                let want = if curve == 0.0 {
                    t
                } else {
                    expected(5.0 * curve, t)
                };
                assert!(
                    (out[frame] - want).abs() < 1e-3,
                    "curve {curve} at {t}: {} vs {want}",
                    out[frame]
                );
            }
            let peak = out.iter().position(|&x| x == 1.0).unwrap();
            assert!((99..=100).contains(&peak), "curve {curve} peaks at {peak}");
        }
        // Positive is convex (fast rise), negative concave (slow start).
        assert!(attack(1.0)[19] > 0.6 && attack(-1.0)[49] < 0.08);
    }

    #[test]
    fn flex_envelope_glides_holds_and_releases() {
        let rate = 1000.0;
        let point = |seconds, level, curve| FlexPoint {
            seconds,
            level,
            curve,
        };
        let flex = Flex {
            points: [
                point(0.01, 1.0, 0.0),
                point(0.01, 0.5, 0.0),
                point(0.02, 0.0, 1.0),
            ]
            .into(),
            sustain: 1,
        };
        let mut env = Envelope::flex();
        let mut out = [0.0; 100];
        env.render(&mut out, Some(&flex), rate);
        assert!((out[4] - 0.5).abs() < 1e-6 && out[9] == 1.0, "attack");
        assert!((out[14] - 0.75).abs() < 1e-6 && out[19] == 0.5, "decay");
        assert!(out[20..].iter().all(|&x| x == 0.5), "sustain point holds");
        env.release(Some(&flex));
        env.render(&mut out[..30], Some(&flex), rate);
        // Convex release: more than halfway down after a quarter of its time.
        assert!(out[4] < 0.25, "{}", out[4]);
        assert_eq!(out[19], 0.0);
        assert!(env.done());

        // Released before the sustain point: straight to the release segment.
        let mut env = Envelope::flex();
        env.render(&mut out[..5], Some(&flex), rate);
        env.release(Some(&flex));
        env.render(&mut out[..30], Some(&flex), rate);
        assert!(out[0] < 0.5 && env.done());
    }
}
