//! A [`CombNetwork`]: per channel, eight combs with a one-pole in their
//! loop summed into four allpasses in series. The channels are first
//! mixed toward their mean as the width narrows, scaled, low-passed and
//! delayed. Settings read physically: `seconds` the combs' 60 dB time over
//! their mean length, `damping` the share of each new comb sample its
//! one-pole drops, `diffusion` the allpass gain; `size` is not used.

use crate::Settings;
use crate::fade::flush;
use crate::fdn::MAX_PREDELAY_MS;
use kontra_law::CombNetwork;

/// A power-of-two ring with one read head `delay` frames behind its write.
struct Line {
    buf: Box<[f32]>,
    delay: usize,
}

impl Line {
    /// Not real-time: allocates.
    fn new(delay: usize) -> Self {
        Self {
            buf: vec![0.0; (delay + 1).next_power_of_two()].into(),
            delay,
        }
    }

    fn read(&self, w: usize) -> f32 {
        let mask = self.buf.len() - 1;
        self.buf
            .get(w.wrapping_sub(self.delay) & mask)
            .copied()
            .unwrap_or(0.0)
    }

    fn write(&mut self, w: usize, v: f32) {
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
    low: f32,
}

pub struct Combs {
    law: &'static CombNetwork,
    channels: [Channel; 2],
    rate: f32,
    feedback: f32,
    keep: f32,
    gain: f32,
    narrow: f32,
    pole: f32,
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
                low: 0.0,
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
            pole: 0.0,
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
        let hz = s.high_cut_hz.clamp(1.0, 0.49 * self.rate);
        self.pole = (-std::f32::consts::TAU * hz / self.rate).exp();
        let pre = (s.predelay_ms.clamp(0.0, MAX_PREDELAY_MS) * 0.001 * self.rate) as usize;
        for c in &mut self.channels {
            c.pre.delay = pre.min(c.pre.buf.len() - 1);
        }
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let (feedback, keep, g) = (self.feedback, self.keep, self.gain);
        for frame in io {
            let [l, r] = *frame;
            let mean = 0.5 * (l + r);
            let w = self.w;
            for (x, c) in frame.iter_mut().zip(&mut self.channels) {
                let mixed = self.narrow * mean + (1.0 - self.narrow) * *x;
                c.low = flush(c.low + (1.0 - self.pole) * (self.law.input_gain * mixed - c.low));
                c.pre.write(w, c.low);
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
