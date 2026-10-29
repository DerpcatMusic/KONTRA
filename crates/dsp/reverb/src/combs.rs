//! A [`CombNetwork`]: per channel, eight combs with a one-pole in their
//! loop summed into four allpasses in series. The channels are first
//! mixed toward their mean as the width narrows, scaled, low-passed by a
//! two-pole Butterworth at `high_cut_hz` and delayed. Settings read physically: `seconds` the combs' 60 dB time over
//! their mean length, `damping` the share of each new comb sample its
//! one-pole drops, `diffusion` the allpass gain; `size` is not used.

use crate::Settings;
use crate::fade::flush;
use crate::fdn::MAX_PREDELAY_MS;
use kontra_dsp_core::Biquad;
use kontra_law::CombNetwork;

/// A power-of-two ring with one read head `delay` frames behind its write.
pub(crate) struct Line {
    buf: Box<[f32]>,
    pub(crate) delay: usize,
}

impl Line {
    /// Not real-time: allocates.
    pub(crate) fn new(delay: usize) -> Self {
        Self {
            buf: vec![0.0; (delay + 1).next_power_of_two()].into(),
            delay,
        }
    }

    pub(crate) fn read(&self, w: usize) -> f32 {
        let mask = self.buf.len() - 1;
        self.buf
            .get(w.wrapping_sub(self.delay) & mask)
            .copied()
            .unwrap_or(0.0)
    }

    /// The value `d` frames behind `w`, linearly between frames; `d` ≥ 1.
    pub(crate) fn at(&self, w: usize, d: f32) -> f32 {
        let mask = self.buf.len() - 1;
        let whole = d as usize;
        let get = |back: usize| {
            let i = w.wrapping_sub(back) & mask;
            self.buf.get(i).copied().unwrap_or(0.0)
        };
        let t = d - whole as f32;
        get(whole) + t * (get(whole + 1) - get(whole))
    }

    /// Frames the ring can reach back.
    pub(crate) fn reach(&self) -> usize {
        self.buf.len() - 1
    }

    pub(crate) fn write(&mut self, w: usize, v: f32) {
        let mask = self.buf.len() - 1;
        if let Some(s) = self.buf.get_mut(w & mask) {
            *s = v;
        }
    }
}

struct Channel {
    pre: Line,
    combs: [(Line, f32); 8],
    allpasses: [Line; 4],
}

pub struct Combs {
    law: &'static CombNetwork,
    channels: [Channel; 2],
    rate: f32,
    feedback: f32,
    keep: f32,
    gain: f32,
    narrow: f32,
    input: Biquad,
    w: usize,
}

impl Combs {
    /// Not real-time: allocates the lines.
    #[must_use]
    pub fn new(law: &'static CombNetwork, s: Settings, rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        let frames = |ms: f32| (ms * 0.001 * rate) as usize;
        let channel = |c: f32| {
            let spread = c * law.spread_ms;
            let mut pre = Line::new(frames(MAX_PREDELAY_MS));
            pre.delay = 0;
            Channel {
                pre,
                combs: law.combs_ms.map(|ms| (Line::new(frames(ms + spread)), 0.0)),
                allpasses: law.allpasses_ms.map(|ms| Line::new(frames(ms + spread))),
            }
        };
        let mut combs = Self {
            law,
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
            c.pre.delay = pre.min(c.pre.buf.len() - 1);
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
                c.pre.write(w, *x);
                let input = c.pre.read(w);
                let mut sum = 0.0;
                for (line, f) in &mut c.combs {
                    let out = line.read(w);
                    *f = flush(out * keep + (1.0 - keep) * *f);
                    line.write(w, *f * feedback + input);
                    sum += out;
                }
                for line in &mut c.allpasses {
                    let v = line.read(w) - g * sum;
                    line.write(w, flush(g * v + sum));
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
