//! Gain maps (amount to linear gain) and output stages (gain on the heard
//! audio, post-filters, make-up, wet/dry, clip).

use super::detector::EPS_18;
use super::pipeline::{each, Frame, Shared, Stage};
use super::ProcessorState;

/// Kontakt Classic: `1 - (1 - g) * b`, f32.
pub(super) struct Reach(pub f64);

impl Stage for Reach {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        f.gain = each(f.level, |b| f64::from((1. - self.0 * b) as f32));
    }
}

/// Kontakt Enhanced: `1 / (hold + 1)`, f32.
pub(super) struct Reciprocal;

impl Stage for Reciprocal {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        f.gain = each(f.level, |h| f64::from(1. / (h as f32 + 1.)));
    }
}

/// Kontakt Pro: `2^((filtered * -(1 - ratio) + offset) / 6.02)` by a
/// quadratic `2^x`, f32.
pub(super) struct Exp2Quadratic {
    pub reduction: f32,
    pub offset: f32,
}

const EXP_SCALE: f32 = f32::from_bits(0x3e2a_1512); // 0x144f3871c
const EXP_A: f32 = f32::from_bits(0x3e7d_f3b6); // 0x144f38744
const EXP_B: f32 = f32::from_bits(0x3f35_3f7d); // 0x144f38808

impl Stage for Exp2Quadratic {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let v = (f.level[0] as f32 * self.reduction + self.offset) * EXP_SCALE;
        let n = v.round_ties_even() as i32;
        let fraction = v - n as f32;
        let gain =
            ((fraction * EXP_A + EXP_B) * fraction + 1.) * f32::from_bits(((n.max(-126) + 127) as u32) << 23);
        f.gain = [f64::from(gain); 2];
    }
}

/// UVI Maximizer: `10^((ceiling - (held + ceiling + threshold)) / 20)`
/// through the native neper constants, f32.
pub(super) struct NeperGain {
    pub offset: f32,
    pub ceiling: f32,
}

/// `ln 10 / 20` (`0x141ea34c0`).
const NEPER_PER_DB: f32 = f32::from_bits(0x3deb_c8e3);

impl Stage for NeperGain {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let held = f.level[0] as f32;
        f.gain = [f64::from(((held + self.offset) * -NEPER_PER_DB + self.ceiling * NEPER_PER_DB).exp()); 2];
    }
}

/// Generic: the heard audio times make-up and wet/dry of its gain.
pub(super) struct Mix(pub Shared);

impl Stage for Mix {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        f.out = [f.heard[0] * self.0.factor(f.gain[0]), f.heard[1] * self.0.factor(f.gain[1])];
    }
}

/// Kontakt Classic / Enhanced: f32 gain times f32 audio, then make-up and
/// wet/dry of unity (the cores have no look-ahead).
pub(super) struct F32Apply(pub Shared);

impl Stage for F32Apply {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let out = |ch: usize| f64::from(f.gain[ch] as f32 * f.audio[ch] as f32) * self.0.factor(1.);
        f.out = [out(0), out(1)];
    }
}

/// Kontakt Pro: f32 gain on the delayed audio; wet/dry against it.
pub(super) struct ProMix(pub Shared);

impl Stage for ProMix {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let (s, gain) = (self.0, f.gain[0] as f32);
        f.out = each(f.heard, |d| {
            if s.mix == 1. { f64::from(gain * d as f32) * s.makeup } else { d * s.factor(f64::from(gain)) }
        });
    }
}

/// Kontakt Limiter: the gained delayed audio clamped to full scale; the
/// wrapper has no wet/dry, so `mix` blends the scaled delayed input.
pub(super) struct LimiterOut(pub Shared);

impl Stage for LimiterOut {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let (s, gain) = (self.0, f.gain[0] as f32);
        f.out = each(f.heard, |d| {
            let limited = f64::from((gain * d as f32).clamp(-1., 1.));
            if s.mix == 1. { limited * s.makeup } else { d + s.mix * (limited * s.makeup - d) }
        });
    }
}

/// Kontakt Transparent Limiter: the smoothed gain times the ceiling on the
/// delayed driven audio.
pub(super) struct CeilingOut {
    pub ceiling: f32,
    pub shared: Shared,
}

impl Stage for CeilingOut {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let (s, scale) = (self.shared, f.gain[0] as f32 * self.ceiling);
        f.out = each(f.heard, |d| {
            let wet = f64::from(scale * d as f32) * s.makeup;
            if s.mix == 1. { wet } else { d + s.mix * (wet - d) }
        });
    }
}

/// UVI Maximizer: the gain on the delayed audio; hard mode clips at the
/// linear ceiling. No make-up or wet/dry (the module has none).
pub(super) struct ClipOut {
    pub ceiling: f32,
    pub hard: bool,
}

impl Stage for ClipOut {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let g = f.gain[0] as f32;
        f.out = each(f.heard, |d| {
            let y = d as f32 * g;
            f64::from(if self.hard { y.clamp(-self.ceiling, self.ceiling) } else { y })
        });
    }
}

/// Kontakt Feedback Compressor: the gain on the driven audio, a first-order
/// low-pass and the 10 Hz DC blocker, then the output gain and the mix with
/// the undriven input. State `aux[4..12]` (low-pass then blocker, x1 and y1
/// per channel), f32.
pub(super) struct FeedbackOut {
    pub lp: [f32; 3],
    pub hp: [f32; 3],
    pub makeup: f32,
    pub mix: f32,
    f: [f32; 8],
}

impl FeedbackOut {
    pub fn new(lp: [f32; 3], hp: [f32; 3], shared: &Shared, state: &ProcessorState) -> Self {
        Self {
            lp,
            hp,
            makeup: shared.makeup as f32,
            mix: shared.mix as f32,
            f: std::array::from_fn(|i| state.aux[4 + i] as f32),
        }
    }
}

impl Stage for FeedbackOut {
    #[inline(always)]
    fn tick(&mut self, fr: &mut Frame) {
        let (lp, hp, f) = (self.lp, self.hp, &mut self.f);
        for ch in 0..2 {
            let u = fr.gain[ch] as f32 * fr.audio[ch] as f32;
            let (lx, ly) = (2 * ch, 1 + 2 * ch);
            let o = (u * lp[0] + f[lx] * lp[1]) - lp[2] * f[ly];
            (f[lx], f[ly]) = (u, o);
            let (hx, hy) = (4 + 2 * ch, 5 + 2 * ch);
            let y = (o * hp[0] + f[hx] * hp[1]) - f[hy] * hp[2];
            (f[hx], f[hy]) = (o + EPS_18, y + EPS_18);
            let wet = y * self.makeup;
            fr.out[ch] = f64::from((1. - self.mix) * fr.x[ch] as f32 + self.mix * wet);
        }
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        for (i, v) in self.f.iter().enumerate() {
            state.aux[4 + i] = f64::from(*v);
        }
    }
}

/// Kontakt Transient Master: the gain on the audio, the 10 Hz DC blocker
/// and the optional ±0.9999 clip. State `aux[12..16]`, f32.
pub(super) struct DcBlock {
    pub hp: [f32; 3],
    pub clip: bool,
    f: [f32; 4],
}

impl DcBlock {
    pub fn new(hp: [f32; 3], clip: bool, state: &ProcessorState) -> Self {
        Self { hp, clip, f: std::array::from_fn(|i| state.aux[12 + i] as f32) }
    }
}

impl Stage for DcBlock {
    #[inline(always)]
    fn tick(&mut self, fr: &mut Frame) {
        let (c, f) = (self.hp, &mut self.f);
        for ch in 0..2 {
            let u = fr.gain[ch] as f32 * fr.audio[ch] as f32;
            let y = (u * c[0] + c[1] * f[2 * ch]) - f[1 + 2 * ch] * c[2];
            (f[2 * ch], f[1 + 2 * ch]) = (u, y);
            fr.out[ch] = f64::from(if self.clip { y.clamp(-0.9999, 0.9999) } else { y });
        }
    }

    #[inline(always)]
    fn store(&mut self, state: &mut ProcessorState) {
        for (i, v) in self.f.iter().enumerate() {
            state.aux[12 + i] = f64::from(*v);
        }
    }
}

/// Solid Bus Comp: linked, every channel takes the smallest gain; make-up,
/// then the dry/wet mix rounded to f32.
pub(super) struct BusOut {
    pub link: bool,
    pub makeup: f64,
    pub wet: f64,
}

impl Stage for BusOut {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let low = f.gain[0].min(f.gain[1]);
        let (wet, dry) = (self.wet, 1. - self.wet);
        let out = |ch: usize| {
            let x = f.audio[ch];
            let w = x * if self.link { low } else { f.gain[ch] } * self.makeup;
            f64::from((x * dry + w * wet) as f32)
        };
        f.out = [out(0), out(1)];
    }
}
