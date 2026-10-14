//! Kontakt Bite (`ni::effektrig::dsp::bite::Bite`, module 0x66) as the color chain's shaper:
//! process 0x141937fb0 inside the effektrig x2 pair (x2 at 44.1/48 kHz, x1 above). Two
//! cascaded TPT SVF low-passes (0x141938bf0) before and after; an envelope-gated
//! resampling clock with cubic Hermite reads, jitter and Gaussian dither (MINSTD); a log
//! (expand) quantizer with soft saturation; band-limited step corrections into an 8-tap ring;
//! a one-pole high-pass (5/100/200 Hz) and an 11-sample dry delay for the mix. All binary32
//! in the native order (model kcolor/bite.py, max error 1e-6 against the emulated core).
// ponytail: knobs are constants (the native 20 ms control ramps are not modelled).
// The level follower uses host-rate coefficients while running at the core rate.
use super::super::filter::ladder_kernel::{exponential_effektrig as l, prepare};
use super::super::kaiser::{Effektrig, EffektrigState};
use super::super::Planar;
pub use sampler_ir::fx::color::Bite;

const PI: f32 = 3.141_592_74;

fn tan3(x: f32) -> f32 {
    let c = (x * x) * x;
    ((c * 0.333_333_343) + x) + (((c * x) * x) * 0.133_333_34)
}

fn minstd(x: u32) -> u32 {
    ((u64::from(x) * 48_271) % 0x7fff_ffff) as u32
}

/// Two-channel TPT state-variable low-pass, damping `k`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Svf {
    a: [f32; 3],
}

impl Svf {
    fn new(k: f32, fc: f32, inv: f32) -> Self {
        let g = tan3((fc * PI) * inv);
        let a1 = 1.0 / (((g + k) * g) + 1.0);
        let a2 = a1 * g;
        Self { a: [a1, a2, a2 * g] }
    }

    #[inline(always)]
    fn tick(&self, s: &mut [[f32; 2]; 2], x: [f32; 2]) -> [f32; 2] {
        let [a1, a2, a3] = self.a;
        std::array::from_fn(|c| {
            let (ic1, ic2) = (s[0][c], s[1][c]);
            let v3 = x[c] - ic2;
            let v2 = ((ic1 * a2) + (v3 * a3)) + ic2;
            s[0][c] = ((((ic1 + ic1) * a1) + (v3 * a2)) + (v3 * a2)) - ic1;
            s[1][c] = (((((ic1 + ic1) * a2) + ic2) + ic2) + (v3 * a3) + (v3 * a3)) - ic2;
            v2
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Engine {
    inc0: f32,
    step: f32,
    istep: f32,
    bitgain: f32,
    drive_in: f32,
    drive_out: f32,
    soft: f32,
    trim: f32,
    dither: f32,
    jitter: f32,
    dc: bool,
    c: f32,
    ic: f32,
    ilog: f32,
    wet: f32,
    dry: f32,
    pre: [Svf; 2],
    post: [Svf; 2],
    hg: f32,
    hgain: f32,
    attack: f32,
    release: f32,
    oversampler: Option<Effektrig>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct State {
    pre: [[[f32; 2]; 2]; 2],
    post: [[[f32; 2]; 2]; 2],
    hp: [f32; 2],
    envelope: f32,
    phase: [f32; 2],
    inc: Option<[f32; 2]>,
    iinc: [f32; 2],
    held: [f32; 2],
    history: [[f32; 8]; 2],
    ring: [[f32; 8]; 2],
    at: usize,
    dry: [[f32; 2]; 16],
    dry_at: usize,
    jitter_seeds: [u32; 2],
    dither_seed: u32,
    cached: Option<f32>,
    up: [EffektrigState; 2],
    down: [EffektrigState; 2],
}

impl Default for State {
    fn default() -> Self {
        Self {
            pre: Default::default(),
            post: Default::default(),
            hp: [0.0; 2],
            envelope: 0.0,
            phase: [0.0; 2],
            inc: None,
            iinc: [0.0; 2],
            held: [0.0; 2],
            history: [[0.0; 8]; 2],
            ring: [[0.0; 8]; 2],
            at: 0,
            dry: [[0.0; 2]; 16],
            dry_at: 0,
            // The emulated core's seeds.
            jitter_seeds: [12_345; 2],
            dither_seed: 12_345,
            cached: None,
            up: Default::default(),
            down: Default::default(),
        }
    }
}

impl State {
    pub(super) fn finite(&self) -> bool {
        self.pre.iter().chain(&self.post).flatten().flatten().chain(&self.hp).chain(self.ring.iter().flatten()).all(|v| v.is_finite())
            && self.envelope.is_finite()
            && self.up.iter().chain(&self.down).all(EffektrigState::finite)
    }
}

impl Engine {
    #[allow(clippy::excessive_precision)]
    pub(super) fn new(b: Bite, rate: u32) -> Self {
        prepare();
        let [saturate, prefilter, frequency, jitter, crunch, bits, dither, dcquant, expand, postfilter, hpf, mix] =
            b.knobs;
        let factor = if super::dirt::oversampled(rate) { 2.0 } else { 1.0 };
        let sr = rate as f32 * factor;
        let inv = 1.0 / sr;
        let pitch = frequency * 105.415_619 + 43.349_956_5;
        let inc0 = l(frequency * 527.078_125 + 1_598.631_35) * inv;
        let nb = (2 - (bits * -15.0) as i32).clamp(2, 16);
        let step = 2f32.powi(1 - nb);
        let bitgain = [0.6, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95, 1.0].get(nb as usize - 2).copied().unwrap_or(1.0);
        let lg = f64::from(step).log10() as f32;
        let cr = (crunch * 18.0) * lg;
        let c = 2f64.powf(f64::from(8.0 - expand * 7.900_000_1)) as f32 - 1.0;
        let w = (mix + mix) * mix;
        let wet = if mix > 0.5 { ((mix * 4.0) - w) - 1.0 } else { w };
        let cut = |k: f32| {
            let (a, b) = if k < 0.5 { (1.0 - (k + k), 43.349_956_5 - pitch) } else { ((k + k) - 1.0, 148.765_579 - pitch) };
            l(((a * b + pitch) * 5.0) + 1_381.881_59) * 0.5
        };
        let (fpre, fpost) = (cut(prefilter), cut(postfilter));
        let svfs = |fc: f32| [Svf::new(0.765_399_992, fc, inv), Svf::new(1.847_800_02, fc, inv)];
        let hz = [5.0, 100.0, 200.0][((hpf * 3.0) as usize).min(2)];
        let hg = tan3((hz * PI) * inv);
        // The level follower runs in the core but is timed at the host rate.
        let host = rate as f32;
        let decay = |seconds: f32| 1.0 - (f64::from(-2.302_585_12 / (seconds * host)).exp() as f32);
        Self {
            inc0,
            step,
            istep: 1.0 / step,
            bitgain,
            drive_in: l(cr * 9.965_784_07 + 1200.0),
            drive_out: l(((saturate * 24.0) - cr) * 9.965_784_07 + 1200.0),
            soft: saturate * 0.2 + 0.05,
            trim: l(1200.0 - saturate * 79.726_272_6),
            dither: dither * dither,
            jitter: jitter * jitter,
            dc: dcquant >= 0.5,
            c,
            ic: 1.0 / c,
            ilog: 1.0 / (f64::from(c).ln_1p() as f32),
            wet,
            dry: 1.0 - wet,
            pre: svfs(fpre),
            post: svfs(fpost),
            hg,
            hgain: 1.0 / (hg + 1.0),
            attack: decay(0.005),
            release: decay(0.1),
            oversampler: (factor > 1.0).then(Effektrig::new),
        }
    }

    #[allow(clippy::excessive_precision)]
    fn gauss(s: &mut State) -> f32 {
        if let Some(v) = s.cached.take() {
            return v * 0.25;
        }
        loop {
            let u1 = minstd(s.dither_seed);
            let u2 = minstd(u1);
            s.dither_seed = u2;
            let a = ((u1 as f32) - 1.0) * 4.656_612_87e-10;
            let a = (a + a) - 1.0;
            let b = ((u2 as f32) - 1.0) * 4.656_612_87e-10;
            let b = (b + b) - 1.0;
            let q = (b * b) + (a * a);
            if q < 1.0 && a != 0.0 && b != 0.0 {
                let g = f64::from(((f64::from(q).ln() as f32) * -2.0) / q).sqrt() as f32;
                s.cached = Some(g * b);
                return (g * a) * 0.25;
            }
        }
    }

    #[allow(clippy::excessive_precision)]
    #[inline(always)]
    fn tick(&self, s: &mut State, x: [f32; 2]) -> [f32; 2] {
        s.dry[s.dry_at] = x;
        s.dry_at = (s.dry_at + 1) & 15;
        let y = self.pre[0].tick(&mut s.pre[0], [self.drive_in * x[0], self.drive_in * x[1]]);
        let y = self.pre[1].tick(&mut s.pre[1], y);
        let n = s.at;
        s.history[0][n] = y[0];
        s.history[1][n] = y[1];
        let a = ((x[1] + x[0]) * 0.5).abs();
        let e = s.envelope;
        s.envelope = (a - e) * (if a > e { self.attack } else { self.release }) + e;
        let mut inc = *s.inc.get_or_insert([self.inc0; 2]);
        if s.iinc[0] == 0.0 {
            s.iinc = [1.0 / self.inc0; 2];
        }
        if s.envelope.abs() <= 9.999_999_75e-5 {
            for c in 0..2 {
                s.ring[c][n] = s.held[c];
            }
        } else {
            for c in 0..2 {
                let ph = inc[c] + s.phase[c];
                s.phase[c] = ph;
                if !(ph >= 1.0) {
                    s.ring[c][n] = s.held[c];
                    continue;
                }
                let ph = ph - 1.0;
                s.phase[c] = ph;
                let t = ph * s.iinc[c];
                let h = &s.history[c];
                let (x4, x5, x3, x6) = (h[(n + 4) & 7], h[(n + 3) & 7], h[(n + 5) & 7], h[(n + 2) & 7]);
                let sq = (1.0 - t) * (1.0 - t);
                let t2 = t * t;
                let v = (x5 - x3) * (sq * t) + (x6 - x4) * ((t - 1.0) * t2);
                let mut v = ((v * 0.5) + ((((t + t) + 1.0) * sq) * x4)) + (((3.0 - (t + t)) * t2) * x5);
                if self.dither > 0.0 {
                    let g = Self::gauss(s);
                    v += (self.step * self.dither) * g;
                }
                let sg = if v < 0.0 { -1.0f32 } else { 1.0 };
                let m = (self.istep * self.ilog) * ((f64::from(v.abs() * self.c).ln_1p() as f32) * sg);
                let q = if self.dc { m.floor() + 0.5 } else { (m + 0.5).floor() };
                let e2 = f64::from(self.c + 1.0).powf(f64::from((q * self.step).abs())) as f32;
                let yy = (((e2 - 1.0) * self.ic) * ((self.drive_out * self.bitgain) * sg)).clamp(-2.0, 2.0);
                let out = ((1.0 - (yy.abs() * self.soft)) * yy) * self.trim;
                let r = &mut s.ring[c];
                r[n] = out;
                let d = out - s.held[c];
                for (k, lag) in [6usize, 7, 0, 1].into_iter().enumerate() {
                    let u = if k < 2 { (t - (2 - k) as f32).abs() * 0.5 } else { (t + (k - 2) as f32).abs() * 0.5 };
                    let mut pv = -2.180_109_98f32;
                    for cc in [6.751_900_2, -5.756_400_1, -1.625_470_04, 3.917_639_97, -0.119_65, -1.487_830_04, 0.499_929_994] {
                        pv = pv * u + cc;
                    }
                    let j = (n + lag) & 7;
                    r[j] = pv * (if k < 2 { d } else { -d }) + r[j];
                }
                s.held[c] = out;
                let j = if self.jitter > 0.0 {
                    s.jitter_seeds[c] = minstd(s.jitter_seeds[c]);
                    let rr = 2.0 * (((s.jitter_seeds[c] as f32) - 1.0) * 4.656_612_87e-10) + -1.0;
                    (rr * self.inc0) * (self.jitter * 0.899_999_976)
                } else {
                    0.0
                };
                let ni = (j + self.inc0).clamp(0.001_000_000_05, 1.0);
                inc[c] = ni;
                s.iinc[c] = 1.0 / ni;
            }
        }
        s.inc = Some(inc);
        s.at = (n + 1) & 7;
        let k = (s.at + 4) & 7;
        let w = [s.ring[0][k], s.ring[1][k]];
        s.ring[0][k] = 0.0;
        s.ring[1][k] = 0.0;
        let w = self.post[0].tick(&mut s.post[0], w);
        let w = self.post[1].tick(&mut s.post[1], w);
        let dl = s.dry[(s.dry_at + 4) & 15];
        std::array::from_fn(|c| {
            let st = s.hp[c];
            let hp = w[c] - (((self.hg * w[c]) + st) + st) * self.hgain;
            s.hp[c] = hp * self.hg + st;
            hp * self.wet + dl[c] * self.dry
        })
    }

    pub(super) fn process(&self, s: &mut State, block: &mut Planar, len: usize) {
        for i in 0..len {
            let x = [block[0][i] as f32, block[1][i] as f32];
            let y = if let Some(os) = &self.oversampler {
                let (l0, l1) = os.up(&mut s.up[0], x[0]);
                let (r0, r1) = os.up(&mut s.up[1], x[1]);
                let a = self.tick(s, [l0, r0]);
                let b = self.tick(s, [l1, r1]);
                [os.down(&mut s.down[0], a[0], b[0]), os.down(&mut s.down[1], a[1], b[1])]
            } else {
                self.tick(s, x)
            };
            block[0][i] = f64::from(y[0]);
            block[1][i] = f64::from(y[1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The emulated native chain (kcolor/vec_bite.json, 48 kHz, x2 inside): an impulse of
    /// 0.5 (128 frames) and a 0.5 sine at 200 Hz, frames 1920..2400, left channel.
    #[test]
    fn matches_the_native_core() {
        let cases: serde_json::Value = serde_json::from_str(include_str!("tables/kontakt_bite_vectors.json")).unwrap();
        const BLOCK: usize = super::super::super::BLOCK;
        // Core defaults (bite.py ctor) by core index.
        let defaults = [0.0, 0.5, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.5, 0.0, 1.0];
        for case in cases.as_array().unwrap() {
            let mut knobs: [f32; 12] = defaults;
            for (k, v) in case["params"].as_object().unwrap() {
                knobs[k.parse::<usize>().unwrap()] = v.as_f64().unwrap() as f32;
            }
            let unit = Engine::new(Bite { knobs }, 48_000);
            let run = |input: &dyn Fn(usize) -> f32, frames: usize| {
                let mut s = State::default();
                let mut out = Vec::new();
                for start in (0..frames).step_by(BLOCK) {
                    let mut block: Planar = [[0.0; BLOCK]; 2];
                    for i in 0..BLOCK {
                        let x = f64::from(input(start + i));
                        block[0][i] = x;
                        block[1][i] = x;
                    }
                    unit.process(&mut s, &mut block, BLOCK);
                    out.extend(block[0].iter().map(|&v| v as f32));
                }
                out
            };
            let check = |name: &str, out: &[f32], native: &serde_json::Value, at: usize| {
                for (i, n) in native.as_array().unwrap().iter().enumerate() {
                    let n = n.as_f64().unwrap() as f32;
                    // One 16-bit step (3e-5) where the sine lands on a quantizer edge.
                    assert!((out[at + i] - n).abs() < 3e-5, "{knobs:?} {name} {i}: {} vs {n}", out[at + i]);
                }
            };
            check("impulse", &run(&|i| if i == 0 { 0.5 } else { 0.0 }, 128), &case["impulse_L"], 0);
            let sine = |i: usize| (0.5 * (std::f64::consts::TAU * 200.0 * i as f64 / 48_000.0).sin()) as f32;
            check("sine", &run(&sine, 2400), &case["sine_L_settled_1920_2400"], 1920);
        }
    }
}
