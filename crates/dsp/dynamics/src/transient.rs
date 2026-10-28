//! A transient shaper: a tenth-root level detector, six envelope followers
//! that split the level into an attack part and a sustain part, a gain in
//! dB from each, a 5 Hz DC blocker and an optional clip. The followers'
//! rates and thresholds are the shape: no standard design fixes them, so
//! they are kept as measured (Kontakt's Transient Master core, setter
//! 0x141ac87b0, process 0x141aaac90, reset 0x141aa0b80).

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// dB per unit of the attack envelope; negative softens onsets.
    pub attack: f32,
    /// dB per unit of the sustain envelope; negative shortens tails.
    pub sustain: f32,
    pub gain_db: f32,
    /// Clip the output just inside full scale.
    pub clip: bool,
}

/// Follower rates in units per second: attack up/down, slow up/down, peak
/// release, level release, mid up, sustain up/down.
const RATES: [f32; 9] = [
    58_741.273,
    10.502_227,
    102.385_086,
    10.502_227,
    86.308_92,
    13.862_939,
    1_386.294,
    5_776.225_6,
    0.407_733_5,
];
/// The followers' state after a reset.
const START: [f32; 6] = [0.262, 0.425, 0.0, 0.403, 0.403, 0.63];
/// Detector and output DC blocker corner, and the attack part's, rad/s.
const DC_RAD: f32 = std::f32::consts::PI * 10.0;
const ATTACK_RAD: f32 = std::f32::consts::PI * 4.0;

/// A first-order high-pass `[b0, b1, a1]` at `w` rad/s.
fn high_pass(w: f32, rate: f32) -> [f32; 3] {
    let t = (w / rate).tan();
    let b = 1.0 / (t + 1.0);
    [b, -b, (t - 1.0) * b]
}

pub struct Transient {
    s: Settings,
    k: [f32; 9],
    dc: [f32; 3],
    sub: [f32; 3],
    e: [f32; 6],
    /// The attack part's high-pass input and output.
    attack_hp: [f32; 2],
    /// Detector and output high-pass input and output, per channel.
    detect: [[f32; 2]; 2],
    out: [[f32; 2]; 2],
}

impl Transient {
    #[must_use]
    pub fn new(s: Settings, rate: u32) -> Self {
        let rate = rate as f32;
        Self {
            s,
            k: RATES.map(|r| r / rate),
            dc: high_pass(DC_RAD, rate),
            sub: high_pass(ATTACK_RAD, rate),
            e: START,
            attack_hp: [0.0; 2],
            detect: [[0.0; 2]; 2],
            out: [[0.0; 2]; 2],
        }
    }

    /// Changes the settings while it plays.
    pub fn set(&mut self, s: Settings) {
        self.s = s;
    }

    /// Shapes `io` in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let [k1u, k1d, k2u, k2d, k3d, k4d, k5u, k6u, k6d] = self.k;
        let step = |env: f32, target: f32, up: f32, down: f32| {
            let d = target - env;
            (if d > 0.0 { up } else { down }) * d + env
        };
        let (c, s) = (self.dc, self.s);
        for frame in io {
            let mut det = 0.0f32;
            for (x, f) in frame.iter().zip(&mut self.detect) {
                let y = (x * c[0] + c[1] * f[0]) - c[2] * f[1];
                *f = [*x, y];
                det = det.max(y.abs());
            }
            let l = det.powf(0.1) * 0.7;
            let e = &mut self.e;
            e[0] = step(e[0], l, k1u, k1d);
            e[1] = step(e[1], l, k2u, k2d);
            let mut v = e[0] - e[1];
            if v > 0.0744 {
                v *= 1.1417;
            }
            let w = (v - 0.0504).clamp(0.0, 0.12);
            let h = self.sub;
            let y = (h[1] * self.attack_hp[0] + w * h[0]) - h[2] * self.attack_hp[1];
            self.attack_hp = [w, y];
            let d = y.abs() - e[2];
            e[2] = if d <= 0.0 { d * k3d + e[2] } else { y.abs() };
            let d = l - e[3];
            e[3] = if d <= 0.0 { d * k4d + e[3] } else { l };
            e[4] = step(e[4], l, k5u, k4d);
            let held = (e[3] - e[4] - 0.04).clamp(0.0, 10_000.0);
            e[5] = step(e[5], l, k6u, k6d);
            let sustain = (e[5] - e[4] - held).clamp(0.0, 10_000.0);
            let attack = e[2];
            let db =
                sustain * s.sustain - s.sustain * 0.33 * attack + attack * s.attack + s.gain_db;
            let g = 10f32.powf(db / 20.0);
            for (x, f) in frame.iter_mut().zip(&mut self.out) {
                let u = g * *x;
                let y = (u * c[0] + c[1] * f[0]) - f[1] * c[2];
                *f = [u, y];
                *x = if s.clip { y.clamp(-0.9999, 0.9999) } else { y };
            }
        }
        for v in self
            .attack_hp
            .iter_mut()
            .chain(self.detect.iter_mut().flatten())
        {
            if v.abs() < 1e-20 {
                *v = 0.0;
            }
        }
        for v in self.out.iter_mut().flatten() {
            if v.abs() < 1e-20 {
                *v = 0.0;
            }
        }
    }
}
