//! A diode ring modulator and frequency shifter: a one-pole high-pass, an
//! analytic pair from two polyphase allpass chains, each branch multiplied
//! by a quadrature carrier through a diode pair `√((k(c + x))² + 1) −
//! √((k(c − x))² + 1)` (first-order antiderivative antialiasing), the
//! quadrature branch weighted in, a silence gate and the dry mixed in.
//! The structure is the oscillator path of v2 sampler-core
//! dsp/color/freak.rs (a port of a native core); the Hilbert pair here is
//! Olli Niemitalo's fixed design, a phase difference within 0.7° over
//! 0.002..0.998 of the band, not the native per-rate elliptic one (which
//! differs in phase only).

/// The settings in signal terms (see `ir::RingMod`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingMod {
    pub hz: f32,
    pub spread: f32,
    pub bias: f32,
    pub quadrature: f32,
    pub drive: f32,
    pub highpass_hz: f32,
    pub gate: bool,
    pub wet: f32,
    pub dry: f32,
}

/// The two branches' allpass coefficients `a`; each stage is
/// `(a² − z⁻²)/(1 − a²·z⁻²)`.
const BRANCHES: [[f32; 4]; 2] = [
    [0.692_387_8, 0.936_065_4, 0.988_229_5, 0.998_748_8],
    [0.402_192_1, 0.856_171_1, 0.972_291, 0.995_288_5],
];

/// The ADAA fallback's relative threshold.
const CLOSE: f32 = 3.452_7e-4;

/// The carrier's sine of a triangle phase −1..1 (`sin(π/2·t)`).
fn sine(t: f32) -> f32 {
    let t2 = t * t;
    (1.570_796_4 * t - 0.641_592_7 * t * t2) + 0.070_796_33 * t * (t2 * t2)
}

/// `−cos(2π·u)` for `u` in 0..1.
fn carrier(u: f32) -> f32 {
    let u = u - u.floor();
    sine(1.0 - (2.0 - 4.0 * u).abs())
}

/// The antiderivative of `√(q² + 1) − 1`.
fn antiderivative(q: f32) -> f32 {
    let r = (q * q + 1.0).sqrt();
    ((q + r).ln() + q * r) * 0.5 - q
}

/// `√(q² + 1) − 1` antialiased against the previous input.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Diode {
    x: f32,
    f: f32,
}

impl Diode {
    fn tick(&mut self, x: f32) -> f32 {
        let f = antiderivative(x);
        let dx = x - self.x;
        let y = if dx.abs() < (x.abs() + self.x.abs() + 10.0) * CLOSE {
            let m = (x + self.x) * 0.5;
            (m * m + 1.0).sqrt() - 1.0
        } else {
            (f - self.f) / dx
        };
        (self.x, self.f) = (x, f);
        y
    }
}

/// One channel's high-pass, allpass branches and diodes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Channel {
    hp: f32,
    /// Per branch and stage `[x₁, x₂, y₁, y₂]`.
    allpass: [[[f32; 4]; 4]; 2],
    /// The first branch's last output (its one-frame delay).
    delayed: f32,
    /// Sum and difference diodes per branch.
    diodes: [[Diode; 2]; 2],
}

impl Channel {
    /// The analytic pair of `x`.
    fn analytic(&mut self, x: f32) -> [f32; 2] {
        let mut out = [x; 2];
        for ((o, stages), coefs) in out.iter_mut().zip(&mut self.allpass).zip(BRANCHES) {
            for (s, a) in stages.iter_mut().zip(coefs) {
                let [x1, x2, y1, y2] = *s;
                let y = a * a * (*o + y2) - x2;
                *s = [*o, x1, y, y1];
                *o = y;
            }
        }
        let [re, im] = out;
        [std::mem::replace(&mut self.delayed, re), im]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ring {
    s: RingMod,
    inc: f32,
    hg: f32,
    k: f32,
    norm: f32,
    attack: f32,
    release: f32,
    phase: f32,
    envelope: f32,
    channels: [Channel; 2],
}

impl Ring {
    #[must_use]
    pub fn new(s: RingMod, rate: u32) -> Self {
        let mut r = Self {
            s,
            inc: 0.0,
            hg: 0.0,
            k: 0.5,
            norm: 1.0,
            attack: 1.0,
            release: 1.0,
            phase: 0.0,
            envelope: 0.0,
            channels: [Channel::default(); 2],
        };
        r.set(s, rate);
        r
    }

    /// New settings; the carrier and filter state carry on.
    pub fn set(&mut self, s: RingMod, rate: u32) {
        let rate = rate.max(1) as f32;
        let x = f64::from(std::f32::consts::PI * s.highpass_hz.clamp(0.0, 0.45 * rate) / rate);
        // The native one-pole prewarp: a tan series as `P/(P + 1)`.
        let p = x * x * x * (1.0 / 3.0) + x + x * x * x * x * x * (2.0 / 15.0);
        let decay = |seconds: f32| 1.0 - (-std::f32::consts::LN_10 * 2.0 / (seconds * rate)).exp();
        let k = s.drive.max(1e-3);
        (self.s, self.inc, self.hg, self.k) = (s, s.hz / rate, (p / (p + 1.0)) as f32, k);
        self.norm = (k * k * 0.5 + 1.0).sqrt() * (0.5 / (k * k));
        (self.attack, self.release) = (decay(0.005), decay(0.1));
    }

    pub fn reset(&mut self) {
        (self.phase, self.envelope) = (0.0, 0.0);
        self.channels = [Channel::default(); 2];
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let s = self.s;
        let scale = 1.0 - s.bias * 0.5;
        for frame in io {
            let level = frame[0].abs().max(frame[1].abs());
            let e = self.envelope;
            self.envelope = e
                + (level - e)
                    * if level <= e {
                        self.release
                    } else {
                        self.attack
                    };
            let open = if s.gate && self.envelope < 1e-5 {
                self.envelope * 1e5
            } else {
                1.0
            };
            let lead = s.spread * 0.5;
            for ((x, ch), offset) in frame.iter_mut().zip(&mut self.channels).zip([lead, -lead]) {
                let input = *x;
                // The input's high-pass (TPT one-pole).
                let v = (input - ch.hp) * self.hg;
                let low = v + ch.hp;
                ch.hp = low + v;
                let pair = ch.analytic(input - low);
                let u = self.phase + offset;
                let carriers = [carrier(u + 0.5), carrier(u + 0.25)];
                let mut wet = 0.0;
                for (((a, c), diodes), weight) in pair
                    .into_iter()
                    .zip(carriers)
                    .zip(&mut ch.diodes)
                    .zip([1.0, s.quadrature])
                {
                    let m = (s.bias + c) * scale;
                    let [sum, difference] = diodes;
                    let y = sum.tick(self.k * (m + a)) - difference.tick(self.k * (m - a));
                    wet += y * self.norm * weight;
                }
                *x = s.dry * input + s.wet * open * wet;
            }
            self.phase += self.inc;
            self.phase -= self.phase.floor();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(bias: f32, quadrature: f32) -> RingMod {
        RingMod {
            hz: 500.0,
            spread: 0.0,
            bias,
            quadrature,
            drive: 0.5,
            highpass_hz: 8.0,
            gate: false,
            wet: 1.0,
            dry: 0.0,
        }
    }

    /// Level of `tone` Hz in the settled output of a 2 kHz sine.
    fn level(s: RingMod, tone: f32) -> f32 {
        let mut ring = Ring::new(s, 48_000);
        let mut io: Vec<[f32; 2]> = (0..48_000)
            .map(|n| [0.1 * (std::f32::consts::TAU * 2_000.0 * n as f32 / 48_000.0).sin(); 2])
            .collect();
        ring.process(&mut io);
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, f) in io.iter().enumerate().skip(24_000) {
            let w = f64::from(std::f32::consts::TAU * tone) * n as f64 / 48_000.0;
            re += f64::from(f[0]) * w.cos();
            im += f64::from(f[0]) * w.sin();
        }
        (2.0 * (re * re + im * im).sqrt() / 24_000.0) as f32
    }

    #[test]
    fn a_ring_makes_both_sidebands_and_a_shift_one() {
        let ring = settings(0.0, 0.0);
        let (up, down) = (level(ring, 2_500.0), level(ring, 1_500.0));
        assert!(
            (up / 0.05 - 1.0).abs() < 0.1 && (down / 0.05 - 1.0).abs() < 0.1,
            "{up} {down}"
        );
        let shift = settings(0.0, 1.0);
        let (up, down) = (level(shift, 2_500.0), level(shift, 1_500.0));
        assert!((up / 0.1 - 1.0).abs() < 0.1 && down < 0.003, "{up} {down}");
        let am = settings(1.0, 0.0);
        let carrier_less = level(am, 2_000.0);
        assert!((carrier_less / 0.05 - 1.0).abs() < 0.1, "{carrier_less}");
    }
}
