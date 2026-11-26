//! Filters whose 0..=1 cutoff and resonance knobs follow a control clock
//! (Kontakt 8's [ref]): in 32-frame segments, carried across calls,
//! the cutoff steps at most `clamp(1/(31·r + 4), 1/35, 1/4)` toward its
//! knob (`r` the resonance at the call's start) and the resonance moves
//! 0.015 of the way while either is more than 1e-4 off. A still call runs in
//! one piece; the first snaps to the knobs.
//!
//! The kernels: four first-order allpasses around up to 0.85 of feedback,
//! mixed half and half with the input (setter [ref], kernel
//! [ref]); or two formant sections in parallel, the low one warped
//! down to unit gain at DC and the high one mirrored to unit gain at Nyquist
//! (setters [ref], kernel [ref]).

/// Frames per clock segment.
const TICK: usize = 32;
/// Either knob this far off its goal keeps the clock running.
const MOVING: f32 = 1e-4;
/// Below these the series terms are dropped: the allpass corner's, the poles'.
const SERIES_MIN: f32 = 1e-5;
const TINY: f32 = 1e-4;
/// `1/n!` for n = 3..=8, as the setters' truncated series use them.
const C3: f32 = 1.0 / 6.0;
const C4: f32 = 1.0 / 24.0;
const C5: f32 = 1.0 / 120.0;
const C6: f32 = 1.0 / 720.0;
const C7: f32 = 1.0 / 5040.0;
/// The formant sections' prototypes (`[b1, b2, c1, c2, b0]`): sections 0 and
/// 1 of a fourth-order Butterworth, bilinear at `tan(w/2) = 0.5463`.
const LOW: [u32; 5] = [
    0x3e84_6b75,
    0x3e04_6b75,
    0x3f1b_a37a,
    0xbe00_3bba,
    0x3e04_6b75,
];
const HIGH: [u32; 5] = [
    0x3eb2_08f2,
    0x3e32_08f2,
    0x3f51_4087,
    0xbf03_4979,
    0x3e32_08f2,
];

/// What a [`Clocked`] filter runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockedShape {
    Allpass,
    VowelLow,
    VowelHigh,
}

/// `y = b0·x + b1·x1 + b2·x2 + c1·y1 + c2·y2`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Section {
    b0: f32,
    b1: f32,
    b2: f32,
    c1: f32,
    c2: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Law {
    Allpass { a: f32, feedback: f32 },
    Vowel([Section; 2]),
}

/// A stereo clocked filter.
#[derive(Clone, Debug, PartialEq)]
pub struct Clocked {
    shape: ClockedShape,
    /// The vowels' `44011.976/rate`; the allpasses' `48000/rate`.
    rate_factor: f32,
    allpass_scale: f32,
    goal: [f32; 2],
    now: [f32; 2],
    started: bool,
    countdown: usize,
    law: Law,
    designed: Option<[f32; 2]>,
    /// Per channel: the allpasses' last inputs and outputs, or each
    /// section's `[x1, x2, y1, y2]`.
    state: [[[f32; 4]; 2]; 2],
}

impl Clocked {
    /// At `[cutoff, resonance]`; the first call snaps there.
    #[must_use]
    pub fn new(shape: ClockedShape, knobs: [f32; 2], rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        let mut c = Self {
            shape,
            rate_factor: (44_011.976_047_904_19 / f64::from(rate)) as f32,
            allpass_scale: 48_000.0 / rate,
            goal: [0.0; 2],
            now: [0.0; 2],
            started: false,
            countdown: 0,
            law: Law::Allpass {
                a: 0.0,
                feedback: 0.0,
            },
            designed: None,
            state: [[[0.0; 4]; 2]; 2],
        };
        c.set(knobs);
        c
    }

    /// New goals for the clock. No allocation.
    pub fn set(&mut self, [cutoff, resonance]: [f32; 2]) {
        // The wrapper slews the raw cutoff (its setters clamp it).
        let cutoff = if cutoff.is_finite() { cutoff } else { 0.0 };
        self.goal = [cutoff, clamp01(resonance)];
    }

    /// Filters `io` in place.
    pub fn process(&mut self, mut io: &mut [[f32; 2]]) {
        if !self.started {
            self.now = self.goal;
            self.started = true;
            self.countdown = 0;
        }
        let step = (1.0 / (31.0 * self.now[1] + 4.0)).clamp(1.0 / 35.0, 0.25);
        while !io.is_empty() {
            self.design();
            let [cut, res] = self.now;
            let moving = (self.goal[0] - cut).abs() > MOVING || (self.goal[1] - res).abs() > MOVING;
            let n = if moving {
                self.countdown.min(io.len())
            } else {
                io.len()
            };
            let (chunk, rest) = io.split_at_mut(n);
            self.run(chunk);
            io = rest;
            if moving {
                self.countdown -= n;
                // The countdown reloads only while frames remain.
                if self.countdown == 0 && !io.is_empty() {
                    self.countdown = TICK;
                }
                let delta = self.goal[0] - cut;
                self.now[0] = cut + delta.clamp(-step, step);
                if (self.goal[1] - res).abs() > MOVING {
                    self.now[1] = res + (self.goal[1] - res) * 0.015;
                }
            }
        }
    }

    fn design(&mut self) {
        if self.designed == Some(self.now) {
            return;
        }
        self.designed = Some(self.now);
        let [cutoff, resonance] = self.now;
        self.law = match self.shape {
            ClockedShape::Allpass => allpass(cutoff, resonance, self.allpass_scale),
            ClockedShape::VowelLow => Law::Vowel(vowel(false, cutoff, resonance, self.rate_factor)),
            ClockedShape::VowelHigh => Law::Vowel(vowel(true, cutoff, resonance, self.rate_factor)),
        };
    }

    fn run(&mut self, io: &mut [[f32; 2]]) {
        if io.is_empty() {
            return;
        }
        if self.state.iter().flatten().flatten().all(|&v| v == 0.0)
            && io.iter().all(|f| f[0] == 0.0 && f[1] == 0.0)
        {
            return;
        }
        match self.law {
            Law::Allpass { a, feedback } => {
                let [[mut lx, mut ly], [mut rx, mut ry]] = self.state;
                for frame in io.iter_mut() {
                    let vl = frame[0];
                    let vr = frame[1];
                    let mut sl = feedback * ly[3] + vl;
                    let mut sr = feedback * ry[3] + vr;

                    let ol0 = (sl + ly[0]) * a - lx[0];
                    let or0 = (sr + ry[0]) * a - rx[0];
                    lx[0] = sl;
                    rx[0] = sr;
                    ly[0] = ol0;
                    ry[0] = or0;
                    sl = ol0;
                    sr = or0;

                    let ol1 = (sl + ly[1]) * a - lx[1];
                    let or1 = (sr + ry[1]) * a - rx[1];
                    lx[1] = sl;
                    rx[1] = sr;
                    ly[1] = ol1;
                    ry[1] = or1;
                    sl = ol1;
                    sr = or1;

                    let ol2 = (sl + ly[2]) * a - lx[2];
                    let or2 = (sr + ry[2]) * a - rx[2];
                    lx[2] = sl;
                    rx[2] = sr;
                    ly[2] = ol2;
                    ry[2] = or2;
                    sl = ol2;
                    sr = or2;

                    let ol3 = (sl + ly[3]) * a - lx[3];
                    let or3 = (sr + ry[3]) * a - rx[3];
                    lx[3] = sl;
                    rx[3] = sr;
                    ly[3] = ol3;
                    ry[3] = or3;

                    frame[0] = (ol3 + vl) * 0.5;
                    frame[1] = (or3 + vr) * 0.5;
                }
                self.state = [[lx, ly], [rx, ry]];
            }
            Law::Vowel([c0, c1]) => {
                let [[mut l0, mut l1], [mut r0, mut r1]] = self.state;
                for frame in io.iter_mut() {
                    let vl = frame[0];
                    let vr = frame[1];
                    let yl0 =
                        c0.c1 * l0[2] + c0.c2 * l0[3] + c0.b2 * l0[1] + c0.b1 * l0[0] + c0.b0 * vl;
                    l0 = [vl, l0[0], yl0, l0[2]];
                    let yr0 =
                        c0.c1 * r0[2] + c0.c2 * r0[3] + c0.b2 * r0[1] + c0.b1 * r0[0] + c0.b0 * vr;
                    r0 = [vr, r0[0], yr0, r0[2]];

                    let yl1 =
                        c1.c1 * l1[2] + c1.c2 * l1[3] + c1.b2 * l1[1] + c1.b1 * l1[0] + c1.b0 * vl;
                    l1 = [vl, l1[0], yl1, l1[2]];
                    let yr1 =
                        c1.c1 * r1[2] + c1.c2 * r1[3] + c1.b2 * r1[1] + c1.b1 * r1[0] + c1.b0 * vr;
                    r1 = [vr, r1[0], yr1, r1[2]];

                    frame[0] = yl0 + yl1;
                    frame[1] = yr0 + yr1;
                }
                self.state = [[l0, l1], [r0, r1]];
            }
        }
        for v in self.state.as_flattened_mut().as_flattened_mut() {
            if v.abs() < 1e-20 {
                *v = 0.0;
            }
        }
    }
}

fn clamp01(v: f32) -> f32 {
    if v >= 0.0 { v.min(1.0) } else { 0.0 }
}

/// The allpass coefficient `(1 − f)/(1 + f)` with
/// `f = 0.0543·(e − 1)·0.33 + 0.02`, `e` the series of `e^(3c)` through its sixth power; feedback
/// `0.85·r`. Kontakt's `f` ignores the rate (its corner follows the host's);
/// here it is held at its 48 kHz corner.
fn allpass(cutoff: f32, resonance: f32, scale: f32) -> Law {
    let p = clamp01(cutoff);
    let v = p * 3.0;
    let square = p * p * 9.0;
    let cube = square * v;
    let fourth = cube * p * 3.0;
    let fifth = fourth * v;
    let e = if v.abs() >= SERIES_MIN {
        fifth * p * 3.0 * C6 + fifth * C5 + cube * C3 + fourth * C4 + square * 0.5 + v + 1.0
    } else {
        1.0
    };
    let f =
        (f64::from(e - 1.0) * 0.054_310_930_074_677_53 * 0.329_999_999_999_999_96 + 0.02) as f32;
    let f = if scale == 1.0 {
        f
    } else {
        (f.atan() * scale).tan()
    };
    Law::Allpass {
        a: (1.0 - f) / (f + 1.0),
        feedback: (f64::from(clamp01(resonance)) * 0.85) as f32,
    }
}

/// The two formant sections for these knobs.
fn vowel(high: bool, cutoff: f32, resonance: f32, rate_factor: f32) -> [Section; 2] {
    let r = clamp01(1.0 - (1.0 - resonance) * (1.0 - resonance));
    let (k, slope, base) = if high {
        (0.22, 0.083, 0.052)
    } else {
        (0.18, 0.068, 0.071_24)
    };
    let r1 = (f64::from(r) * k + 0.85) as f32;
    let r2 = if high {
        (f64::from(r) * 0.4 + 0.2) as f32
    } else {
        (f64::from(r * 0.5) + 0.2) as f32
    };
    let level = f64::from((1.0 - r2) * 0.68 + 0.32);
    let u = clamp01((clamp01(cutoff) + 0.002) * rate_factor);
    let t = f64::from((f64::from(u) * std::f64::consts::FRAC_PI_2) as f32);
    let wa = if high {
        (t * 0.0101 + 0.018_37) as f32
    } else {
        (0.035_62 - t * 0.011_33) as f32
    };
    let wb = (t * slope + base) as f32;
    let x = warp(LOW, 0.647, 6.294, r1, lp_pole(wa));
    let mut y = warp(HIGH, 0.3, 0.796, r2, hp_pole(wb));
    y.b1 = -y.b1;
    y.c1 = -y.c1;
    let dx = ((1.0 - f64::from(x.c2 + x.c1)) / f64::from((x.b1 + x.b0) + x.b2)) * level;
    let dy = ((1.0 - f64::from(y.c2 - y.c1)) / f64::from((y.b0 - y.b1) + y.b2)) * level;
    [scale(x, dx), scale(y, dy)]
}

fn scale(s: Section, d: f64) -> Section {
    let m = |v: f32| (f64::from(v) * d) as f32;
    Section {
        b0: m(s.b0),
        b1: m(s.b1),
        b2: m(s.b2),
        ..s
    }
}

/// Low-pass allpass pole `sin(.5 − t)/sin(.5 + t)`, `t = 0.99878·w`.
fn lp_pole(w: f32) -> f32 {
    let t = (f64::from(w) * 0.998_780_904_966_9) as f32;
    let x = 0.5 - t;
    let num = if x.abs() >= TINY {
        let x3 = x * x * x;
        let x5 = x3 * x * x;
        (x5 * C5 - x * x5 * x * C7) + (x - x3 * C3)
    } else {
        0.0
    };
    let y = t + 0.5;
    let y3 = y * y * y;
    let y5 = y3 * y * y;
    let den = (y5 * C5 - y * y5 * y * C7) + 0.5 + (t - y3 * C3);
    num / den
}

/// High-pass allpass pole `−cos(t + .5)/cos(t − .5)`, `t = 0.99713·w`.
fn hp_pole(w: f32) -> f32 {
    let t = (f64::from(w) * 0.997_133_173_841_07) as f32;
    let y = t + 0.5;
    let num = if y.abs() >= TINY {
        let y4 = y * y * y * y;
        (y * y4 * y * C6 - y4 * C4) + (y * y * 0.5 - 1.0)
    } else {
        -1.0
    };
    let z = t - 0.5;
    let den = if z.abs() >= TINY {
        let z4 = z * z * z * z;
        (z4 * C4 - z * z4 * z * C6) + (1.0 - z * z * 0.5)
    } else {
        1.0
    };
    num / den
}

/// A prototype section, its feedback taps scaled by `1 + f·r`, moved by the
/// allpass substitution `z⁻¹ → (z⁻¹ − a)/(1 − a·z⁻¹)` and normalised to a
/// unit feedback term at z⁰.
fn warp(p: [u32; 5], f1: f32, f2: f32, r: f32, a: f32) -> Section {
    let [b1, b2, c1, c2, b0] = p.map(f32::from_bits);
    let [b0, b1, b2] = [b0, b1, b2].map(f64::from);
    let c1 = f64::from((r * f1 + 1.0) * c1);
    let c2 = f64::from((r * f2 + 1.0) * c2);
    let a = f64::from(a);
    let a2 = a * a;
    let d = 1.0 / ((c1 * a + 1.0) - a2 * c2);
    let s = |v: f64| (f64::from(v as f32) * d) as f32;
    Section {
        b1: s(((b1 - (b0 + b0) * a) + a2 * b1) - (b2 + b2) * a),
        b2: s((a2 * b0 - b1 * a) + b2),
        c1: s(((a2 + 1.0) * c1 + a + a) - (c2 + c2) * a),
        c2: s(c2 - (c1 * a + a2)),
        b0: s((b0 - b1 * a) + a2 * b2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The largest second difference after a cutoff jump on a held tone.
    fn kink(res: f32, instant: bool) -> f32 {
        let tone = |n: usize| (n as f32 * 0.05).sin() * 0.5;
        let mut f = Clocked::new(ClockedShape::Allpass, [0.2, res], 48_000);
        let mut io: Vec<[f32; 2]> = (0..48_000).map(|n| [tone(n); 2]).collect();
        let (head, tail) = io.split_at_mut(24_000);
        f.process(head);
        f.set([0.9, res]);
        if instant {
            f.now = f.goal;
        }
        for c in tail.chunks_mut(256) {
            f.process(c);
        }
        let curve = |w: &[[f32; 2]]| match w {
            [a, b, c] => (c[0] - 2.0 * b[0] + a[0]).abs(),
            _ => 0.0,
        };
        tail.windows(3).map(curve).fold(0.0, f32::max)
    }

    /// The clock spreads an allpass cutoff jump over its steps: the kink is
    /// a fifth of an instant jump's or less (measured 0.09 at resonance 0,
    /// 0.04 at 1). Vowel sweeps are checked by the native vectors.
    #[test]
    fn a_cutoff_jump_glides_without_a_step() {
        for res in [0.0, 1.0] {
            let (c, i) = (kink(res, false), kink(res, true));
            assert!(c < i / 5.0, "resonance {res}: {c} vs {i}");
        }
    }
}
