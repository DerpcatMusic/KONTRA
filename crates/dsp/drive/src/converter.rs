//! A low-quality sampler as a converter pair, the reducer's modelled mode:
//! four-pole low-passes around a jittered clock, cubic Hermite reads, a
//! companding quantizer with Gaussian dither and a soft saturator, steps
//! made band-limited by a 4-tap polynomial correction, a one-pole
//! high-pass, the dry delayed to meet it (three core frames sooner than
//! the native core: see `Channel::history`). Up to 48 kHz it runs inside a
//! 2x halfband. Ported from v2 sampler-core dsp/color/bite.rs, checked
//! there against vectors of the emulated native core; the format
//! translators carry the provenance and the knob laws.

use kontra_dsp_core::Halfband;

const PI: f32 = std::f32::consts::PI;

/// The converter's settings in signal terms (see `ir::Converter`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Converter {
    pub clock_hz: f32,
    pub jitter: f32,
    pub pre_hz: f32,
    pub post_hz: f32,
    pub highpass_hz: f32,
    pub step: f32,
    pub mu: f32,
    pub midrise: bool,
    pub dither: f32,
    pub drive_in: f32,
    pub drive_out: f32,
    pub soft: f32,
    pub trim: f32,
    pub wet: f32,
    pub dry: f32,
}

/// The tan series the native filters use for their prewarp.
fn tan3(x: f32) -> f32 {
    let c = x * x * x;
    c * (1.0 / 3.0) + x + c * x * x * 0.133_333_34
}

/// The MINSTD generator.
fn minstd(x: u32) -> u32 {
    ((u64::from(x) * 48_271) % 0x7fff_ffff) as u32
}

/// −1..1 from a MINSTD state.
fn signed(x: u32) -> f32 {
    2.0 * ((x as f32 - 1.0) * 4.656_613e-10) - 1.0
}

/// A stereo TPT state-variable low-pass, damping `k`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Svf([f32; 3]);

impl Svf {
    fn new(k: f32, hz: f32, inverse_rate: f32) -> Self {
        let g = tan3(hz * PI * inverse_rate);
        let a1 = 1.0 / ((g + k) * g + 1.0);
        Self([a1, a1 * g, a1 * g * g])
    }

    fn tick(self, s: &mut [[f32; 2]; 2], x: [f32; 2]) -> [f32; 2] {
        let [a1, a2, a3] = self.0;
        let [ic1, ic2] = s;
        std::array::from_fn(|c| {
            let (Some(i1), Some(i2), Some(&x)) = (ic1.get_mut(c), ic2.get_mut(c), x.get(c)) else {
                return 0.0;
            };
            let v3 = x - *i2;
            let v1 = *i1 * a1 + v3 * a2;
            let v2 = *i2 + *i1 * a2 + v3 * a3;
            (*i1, *i2) = (2.0 * v1 - *i1, 2.0 * v2 - *i2);
            v2
        })
    }
}

/// Four-pole Butterworth: two sections.
fn butterworth(hz: f32, inverse_rate: f32) -> [Svf; 2] {
    [
        Svf::new(0.765_4, hz, inverse_rate),
        Svf::new(1.847_8, hz, inverse_rate),
    ]
}

/// The settings the frames read (the corners live on as coefficients).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Levels {
    jitter: f32,
    step: f32,
    mu: f32,
    midrise: bool,
    dither: f32,
    drive_in: f32,
    drive_out: f32,
    soft: f32,
    trim: f32,
    wet: f32,
    dry: f32,
}

/// The settings at the core's rate.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Coefficients {
    inc: f32,
    s: Levels,
    istep: f32,
    ilog: f32,
    pre: [Svf; 2],
    post: [Svf; 2],
    hg: f32,
    hgain: f32,
    attack: f32,
    release: f32,
}

impl Coefficients {
    fn new(s: Converter, rate: u32, core: f32) -> Self {
        let inverse = 1.0 / core;
        let mu = s.mu.max(1e-6);
        let hg = tan3(s.highpass_hz.max(0.0) * PI * inverse);
        // The level follower runs in the core but is timed at the host rate.
        let host = rate.max(1) as f32;
        let decay = |seconds: f32| 1.0 - (-std::f32::consts::LN_10 / (seconds * host)).exp();
        Self {
            inc: (s.clock_hz * inverse).clamp(0.001, 1.0),
            s: Levels {
                jitter: s.jitter,
                step: s.step,
                mu,
                midrise: s.midrise,
                dither: s.dither,
                drive_in: s.drive_in,
                drive_out: s.drive_out,
                soft: s.soft,
                trim: s.trim,
                wet: s.wet,
                dry: s.dry,
            },
            istep: 1.0 / s.step.max(1e-9),
            ilog: 1.0 / mu.ln_1p(),
            pre: butterworth(s.pre_hz, inverse),
            post: butterworth(s.post_hz, inverse),
            hg,
            hgain: 1.0 / (hg + 1.0),
            attack: decay(0.005),
            release: decay(0.1),
        }
    }
}

/// One channel's clock, reads and output ring.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Channel {
    phase: f32,
    inc: f32,
    held: f32,
    /// The last four pre-filtered inputs, newest first. The native core
    /// reads three to six frames back and delays the dry eleven; reading
    /// the newest four with the dry eight back is the same output three
    /// frames sooner (the silence follower then leads the reads by three
    /// core frames).
    history: [f32; 4],
    /// Output frames three back to one ahead, with their pending step
    /// corrections; the oldest leaves each frame.
    ring: [f32; 5],
    jitter_seed: u32,
}

impl Channel {
    fn new(inc: f32) -> Self {
        Self {
            phase: 0.0,
            inc,
            held: 0.0,
            history: [0.0; 4],
            ring: [0.0; 5],
            jitter_seed: 12_345,
        }
    }

    /// Takes `y`, captures when the clock comes round (not while `awake` is
    /// false: silence holds the last sample), and gives the frame three back.
    fn tick(&mut self, c: &Coefficients, dither: &mut Dither, y: f32, awake: bool) -> f32 {
        self.capture(c, dither, y, awake);
        let out = self.ring[0];
        self.ring.rotate_left(1);
        self.ring[4] = 0.0;
        out
    }

    fn capture(&mut self, c: &Coefficients, dither: &mut Dither, y: f32, awake: bool) {
        self.history.rotate_right(1);
        if let Some(newest) = self.history.first_mut() {
            *newest = y;
        }
        let held = self.held;
        let ph = self.phase + self.inc;
        if !awake || ph < 1.0 {
            if awake {
                self.phase = ph;
            }
            self.ring[3] = held;
            return;
        }
        let ph = ph - 1.0;
        self.phase = ph;
        let t = ph / self.inc;
        // Cubic Hermite between the second and third newest (tangents from
        // the newest and the oldest).
        let [x3, x4, x5, x6] = self.history;
        let sq = (1.0 - t) * (1.0 - t);
        let t2 = t * t;
        let slope = (x5 - x3) * (sq * t) + (x6 - x4) * ((t - 1.0) * t2);
        let v = slope * 0.5 + (2.0 * t + 1.0) * sq * x4 + (3.0 - 2.0 * t) * t2 * x5;
        let out = quantize(c, dither, v);
        self.ring[3] = out;
        // The step made band-limited: a polynomial kernel either side.
        let d = out - held;
        for (k, r) in self.ring.iter_mut().skip(1).enumerate() {
            let u = if k < 2 {
                (t - (2 - k) as f32).abs()
            } else {
                (t + (k - 2) as f32).abs()
            } * 0.5;
            let mut p = -2.180_11_f32;
            for a in [
                6.751_9, -5.756_4, -1.625_47, 3.917_64, -0.119_65, -1.487_83, 0.499_93,
            ] {
                p = p * u + a;
            }
            *r += p * if k < 2 { d } else { -d };
        }
        self.held = out;
        let jitter = if c.s.jitter > 0.0 {
            self.jitter_seed = minstd(self.jitter_seed);
            signed(self.jitter_seed) * c.inc * c.s.jitter
        } else {
            0.0
        };
        self.inc = (c.inc + jitter).clamp(0.001, 1.0);
    }
}

/// Gaussian dither from one MINSTD stream (Marsaglia's polar method).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Dither {
    seed: u32,
    cached: Option<f32>,
}

impl Dither {
    const FRESH: Self = Self {
        seed: 12_345,
        cached: None,
    };

    /// One value, a quarter wide.
    fn next(&mut self) -> f32 {
        if let Some(v) = self.cached.take() {
            return v * 0.25;
        }
        loop {
            let u1 = minstd(self.seed);
            let u2 = minstd(u1);
            self.seed = u2;
            let (a, b) = (signed(u1), signed(u2));
            let q = a * a + b * b;
            if q < 1.0 && a != 0.0 && b != 0.0 {
                let g = (-2.0 * q.ln() / q).sqrt();
                self.cached = Some(g * b);
                return g * a * 0.25;
            }
        }
    }
}

/// A captured sample `v` through dither, compander, quantizer and saturator.
fn quantize(c: &Coefficients, dither: &mut Dither, v: f32) -> f32 {
    let s = c.s;
    let v = if s.dither > 0.0 {
        v + s.step * s.dither * dither.next()
    } else {
        v
    };
    let sign = if v < 0.0 { -1.0 } else { 1.0 };
    let m = c.istep * c.ilog * (v.abs() * s.mu).ln_1p() * sign;
    let q = if s.midrise {
        m.floor() + 0.5
    } else {
        (m + 0.5).floor()
    };
    let e = ((s.mu + 1.0).powf((q * s.step).abs()) - 1.0) / s.mu;
    let y = (e * s.drive_out * sign).clamp(-2.0, 2.0);
    (1.0 - y.abs() * s.soft) * y * s.trim
}

/// The converter's running state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Unit {
    c: Coefficients,
    halfband: Option<Halfband>,
    pre: [[[f32; 2]; 2]; 2],
    post: [[[f32; 2]; 2]; 2],
    hp: [f32; 2],
    envelope: f32,
    channels: [Channel; 2],
    /// The dry input, eight frames back to meet the wet.
    dry: [[f32; 2]; 9],
    dry_at: u8,
    dither: Dither,
}

impl Unit {
    pub(crate) fn new(s: Converter, rate: u32) -> Self {
        let halfband = (rate <= 48_000).then(|| Halfband::new(rate));
        let core = rate.max(1) as f32 * if halfband.is_some() { 2.0 } else { 1.0 };
        let c = Coefficients::new(s, rate, core);
        Self {
            c,
            halfband,
            pre: Default::default(),
            post: Default::default(),
            hp: [0.0; 2],
            envelope: 0.0,
            channels: [Channel::new(c.inc); 2],
            dry: [[0.0; 2]; 9],
            dry_at: 0,
            dither: Dither::FRESH,
        }
    }

    /// New settings at the same rate; the state carries on.
    pub(crate) fn set(&mut self, s: Converter, rate: u32) {
        let core = rate.max(1) as f32 * if self.halfband.is_some() { 2.0 } else { 1.0 };
        self.c = Coefficients::new(s, rate, core);
    }

    pub(crate) fn reset(&mut self) {
        if let Some(h) = &mut self.halfband {
            h.reset();
        }
        (self.pre, self.post, self.hp, self.envelope) = Default::default();
        self.channels = [Channel::new(self.c.inc); 2];
        (self.dry, self.dry_at, self.dither) = ([[0.0; 2]; 9], 0, Dither::FRESH);
    }

    pub(crate) fn process(&mut self, io: &mut [[f32; 2]]) {
        if let Some(mut h) = self.halfband.take() {
            h.process(io, |x| self.tick(x));
            self.halfband = Some(h);
        } else {
            for frame in io {
                *frame = self.tick(*frame);
            }
        }
    }

    fn tick(&mut self, x: [f32; 2]) -> [f32; 2] {
        let c = self.c;
        // Write this frame, read the one eight back (the next slot).
        let at = usize::from(self.dry_at);
        if let Some(d) = self.dry.get_mut(at) {
            *d = x;
        }
        let next = (at + 1) % 9;
        let dry = self.dry.get(next).copied().unwrap_or_default();
        self.dry_at = next as u8;
        let [p0, p1] = &mut self.pre;
        let [s0, s1] = c.pre;
        let y = s1.tick(p1, s0.tick(p0, x.map(|v| v * c.s.drive_in)));
        let a = ((x[0] + x[1]) * 0.5).abs();
        let e = self.envelope;
        self.envelope = e + (a - e) * if a > e { c.attack } else { c.release };
        let awake = self.envelope.abs() > 1e-4;
        let mut w = [0.0; 2];
        for ((w, channel), y) in w.iter_mut().zip(&mut self.channels).zip(y) {
            *w = channel.tick(&c, &mut self.dither, y, awake);
        }
        let [q0, q1] = &mut self.post;
        let [s0, s1] = c.post;
        let w = s1.tick(q1, s0.tick(q0, w));
        let mut out = [0.0; 2];
        for (((o, st), w), dl) in out.iter_mut().zip(&mut self.hp).zip(w).zip(dry) {
            let hp = w - (c.hg * w + 2.0 * *st) * c.hgain;
            *st += hp * c.hg;
            *o = hp * c.s.wet + dl * c.s.dry;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Converter {
        Converter {
            clock_hz: 44_100.0,
            jitter: 0.0,
            pre_hz: 22_050.0,
            post_hz: 22_050.0,
            highpass_hz: 5.0,
            step: 2f32.powi(-15),
            mu: 2f32.powf(0.1) - 1.0,
            midrise: false,
            dither: 0.0,
            drive_in: 1.0,
            drive_out: 1.0,
            soft: 0.05,
            trim: 1.0,
            wet: 1.0,
            dry: 0.0,
        }
    }

    /// Peak of the settled output of a 1 kHz sine at `amp`.
    fn peak(s: Converter, amp: f32) -> f32 {
        let mut unit = Unit::new(s, 48_000);
        let mut io: Vec<[f32; 2]> = (0..9_600)
            .map(|n| [amp * (std::f32::consts::TAU * 1_000.0 * n as f32 / 48_000.0).sin(); 2])
            .collect();
        unit.process(&mut io);
        io.iter()
            .skip(4_800)
            .map(|f| f[0].abs())
            .fold(0.0, f32::max)
    }

    #[test]
    fn fresh_it_passes_with_the_saturators_compression_and_few_bits_step() {
        // y·(1 − 0.05·|y|): 0.94 → 0.896 (Kontakt 0.856 with its droop).
        let p = peak(fresh(), 0.94);
        assert!((p - 0.896).abs() < 0.02, "{p}");
        // Two bits, linear: steps of 0.5, 0.06 holds at zero.
        let two = Converter {
            step: 0.5,
            drive_out: 0.6,
            ..fresh()
        };
        assert!(peak(two, 0.06) < 1e-3);
        assert!(peak(two, 0.9) > 0.2);
    }
}
