//! A transient shaper: a tenth-root level detector, six envelope followers
//! that split the level into an attack part and a sustain part, a gain in
//! dB from each, a 5 Hz DC blocker and an optional clip. The followers'
//! rates and thresholds are the shape: no standard design fixes them, so
//! they are kept as measured (their source is in the translator's law).

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
        if io.is_empty() {
            return;
        }
        if self.detect == [[0.0; 2]; 2]
            && self.out == [[0.0; 2]; 2]
            && self.attack_hp == [0.0; 2]
            && self.e == [0.0; 6]
            && io.iter().all(|f| f[0] == 0.0 && f[1] == 0.0)
        {
            return;
        }
        let [k1u, k1d, k2u, k2d, k3d, k4d, k5u, k6u, k6d] = self.k;
        #[inline(always)]
        fn step(env: f32, target: f32, up: f32, down: f32) -> f32 {
            let d = target - env;
            (if d > 0.0 { up } else { down }) * d + env
        }
        let ([c0, c1, c2], s) = (self.dc, self.s);
        let [h0, h1, h2] = self.sub;
        let [mut e0, mut e1, mut e2, mut e3, mut e4, mut e5] = self.e;
        let [mut ah0, mut ah1] = self.attack_hp;
        let [[mut dl0, mut dl1], [mut dr0, mut dr1]] = self.detect;
        let [[mut ol0, mut ol1], [mut or0, mut or1]] = self.out;
        let attack_coef = s.attack - s.sustain * 0.33;
        for frame in io {
            let xl = frame[0];
            let xr = frame[1];
            let yl = (xl * c0 + c1 * dl0) - c2 * dl1;
            let yr = (xr * c0 + c1 * dr0) - c2 * dr1;
            dl0 = xl;
            dl1 = yl;
            dr0 = xr;
            dr1 = yr;
            let det = yl.abs().max(yr.abs());
            let l = kontra_dsp_core::math::fast_powf_pos_f32(det, 0.1) * 0.7;
            e0 = step(e0, l, k1u, k1d);
            e1 = step(e1, l, k2u, k2d);
            let mut v = e0 - e1;
            if v > 0.0744 {
                v *= 1.1417;
            }
            let w = (v - 0.0504).clamp(0.0, 0.12);
            let y = (h1 * ah0 + w * h0) - h2 * ah1;
            ah0 = w;
            ah1 = y;
            let ya = y.abs();
            let d = ya - e2;
            e2 = if d <= 0.0 { d * k3d + e2 } else { ya };
            let d = l - e3;
            e3 = if d <= 0.0 { d * k4d + e3 } else { l };
            e4 = step(e4, l, k5u, k4d);
            let held = (e3 - e4 - 0.04).clamp(0.0, 10_000.0);
            e5 = step(e5, l, k6u, k6d);
            let sustain = (e5 - e4 - held).clamp(0.0, 10_000.0);
            let db = sustain * s.sustain + e2 * attack_coef + s.gain_db;
            let g = kontra_dsp_core::math::db_to_gain_f32(db);
            let ul = g * xl;
            let ur = g * xr;
            let out_l = (ul * c0 + c1 * ol0) - ol1 * c2;
            let out_r = (ur * c0 + c1 * or0) - or1 * c2;
            ol0 = ul;
            ol1 = out_l;
            or0 = ur;
            or1 = out_r;
            if s.clip {
                frame[0] = out_l.clamp(-0.9999, 0.9999);
                frame[1] = out_r.clamp(-0.9999, 0.9999);
            } else {
                frame[0] = out_l;
                frame[1] = out_r;
            }
        }
        self.e = [e0, e1, e2, e3, e4, e5];
        self.attack_hp = [ah0, ah1];
        self.detect = [[dl0, dl1], [dr0, dr1]];
        self.out = [[ol0, ol1], [or0, or1]];
        // The followers decay toward zero in silence; flush them too.
        for v in self
            .attack_hp
            .iter_mut()
            .chain(self.detect.iter_mut().flatten())
            .chain(self.e.iter_mut())
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
