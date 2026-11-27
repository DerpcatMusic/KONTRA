//! A [`CombNetwork`]: per channel, eight combs with a one-pole in their
//! loop summed into four allpasses in series. The channels are first
//! mixed toward their mean as the width narrows, scaled, low-passed by a
//! two-pole Butterworth at `high_cut_hz` and delayed. Settings read physically: `seconds` the combs' 60 dB time over
//! their mean length, `damping` the share of each new comb sample its
//! one-pole drops, `diffusion` the allpass gain; `size` is not used.

use crate::Settings;
use crate::fade::flush;
use crate::fdn::MAX_PREDELAY_MS;
use kontra_dsp_core::{Biquad, DelayLine};
use kontra_law::CombNetwork;

struct Channel {
    pre: DelayLine,
    combs: [(DelayLine, f32); 8],
    allpasses: [DelayLine; 4],
}

pub struct Combs {
    law: CombNetwork,
    channels: [Channel; 2],
    rate: f32,
    feedback: f32,
    keep: f32,
    gain: f32,
    narrow: f32,
    input: Biquad,
    w: usize,
}

/// Native comb-network predelay ring capacity mask (8192-frame ring, max 8191
/// frames / ~170.65 ms at 48 kHz). Predelays past this bound wrap to 0 frames,
/// matching the reference 8.13 measurement where 180 ms echoes at frame 2914.
const MAX_PRE_FRAMES: usize = 0x1fff;

impl Combs {
    /// Not real-time: allocates the lines.
    #[must_use]
    pub fn new(law: &CombNetwork, s: Settings, rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        let frames = |ms: f32| (ms * 0.001 * rate) as usize;
        let channel = |c: f32| {
            let spread = c * law.spread_ms;
            let mut pre = DelayLine::new(MAX_PRE_FRAMES);
            pre.set_len(0);
            Channel {
                pre,
                combs: law
                    .combs_ms
                    .map(|ms| (DelayLine::new(frames(ms + spread)), 0.0)),
                allpasses: law
                    .allpasses_ms
                    .map(|ms| DelayLine::new(frames(ms + spread))),
            }
        };
        let mut combs = Self {
            law: *law,
            channels: [channel(0.0), channel(1.0)],
            rate,
            feedback: 0.0,
            keep: 1.0,
            gain: 0.0,
            narrow: 0.0,
            input: Biquad::new([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            w: 0,
        };
        combs.set(s);
        combs
    }

    /// Never allocates.
    pub fn set(&mut self, s: Settings) {
        self.feedback = 10f32.powf(-3.0 * self.law.mean_s() / s.seconds.max(1e-3));
        self.keep = 1.0 - s.damping.clamp(0.0, 1.0);
        self.gain = s.diffusion.clamp(0.0, 0.99);
        self.narrow = 1.0 - s.stereo.clamp(0.0, 1.0);
        let hz = f64::from(s.high_cut_hz.clamp(1.0, 0.495 * self.rate));
        self.input.retune(&butterworth(hz / f64::from(self.rate)));
        let pre = (s.predelay_ms.clamp(0.0, MAX_PREDELAY_MS) * 0.001 * self.rate) as usize;
        for c in &mut self.channels {
            c.pre.set_len(if pre > MAX_PRE_FRAMES { 0 } else { pre });
        }
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let (feedback, keep, g) = (self.feedback, self.keep, self.gain);
        let (narrow, input_gain) = (self.narrow, self.law.input_gain);
        for frame in io.iter_mut() {
            let mean = 0.5 * (frame[0] + frame[1]);
            *frame = frame.map(|x| input_gain * (narrow * mean + (1.0 - narrow) * x));
        }
        self.input.process(io);
        for frame in io {
            let w = self.w;
            for (x, c) in frame.iter_mut().zip(&mut self.channels) {
                c.pre.write_at(w, *x);
                let input = c.pre.read_at(w);
                let mut sum = 0.0;
                for (line, f) in &mut c.combs {
                    let out = line.read_at(w);
                    *f = flush(out * keep + (1.0 - keep) * *f);
                    line.write_at(w, *f * feedback + input);
                    sum += out;
                }
                for line in &mut c.allpasses {
                    let v = line.read_at(w) - g * sum;
                    line.write_at(w, flush(g * v + sum));
                    sum = v;
                }
                *x = sum;
            }
            self.w = w.wrapping_add(1);
        }
    }
}

/// The two-pole Butterworth low-pass at `fc` of the rate (−3 dB there).
fn butterworth(fc: f64) -> Biquad {
    let (sin, cos) = (std::f64::consts::TAU * fc).sin_cos();
    let alpha = sin * std::f64::consts::FRAC_1_SQRT_2;
    let b = 0.5 * (1.0 - cos);
    Biquad::new([b, 2.0 * b, b, 1.0 + alpha, -2.0 * cos, 1.0 - alpha])
}
