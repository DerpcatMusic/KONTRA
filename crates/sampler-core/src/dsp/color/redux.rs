//! UVI Workstation 4.0.9 Redux2's rate and bit reduction as the color chain's
//! quantize/decimate stage (dispatch 0x1411e4de0 to the 8 variants keyed by Type,
//! QuantizeLaw and Antialiasing; Butterworth design 0x1411e54c0 / 0x1411e56c0): a jittered
//! sample clock; an anti-alias 8th-order analog Butterworth evaluated exactly between
//! samples (or a linear interpolation); a linear or mu-law quantizer with triangular dither;
//! the held (Type 1) or impulse (Type 0) output reconstructed through a second Butterworth at
//! the band limit. Analog filter states are complex f64 per pole, as the native (model
//! uvicolor/redux_model.py, checked against all 8 emulated variants).
use super::super::Planar;
pub use sampler_ir::fx::color::Redux;

type C = (f64, f64);

fn cexp(re: f64, im: f64) -> C {
    let m = re.exp();
    (m * im.cos(), m * im.sin())
}

/// Smith's complex division (0x1411ecb90).
fn cdiv((ar, ai): C, (br, bi): C) -> C {
    if bi.abs() < br.abs() {
        let r = bi / br;
        let den = r * bi + br;
        ((ai * r + ar) * (1.0 / den), (ai - ar * r) * (1.0 / den))
    } else {
        let r = br / bi;
        let den = r * br + bi;
        ((ar * r + ai) * (1.0 / den), (ai * r - ar) * (1.0 / den))
    }
}

/// An 8th-order analog Butterworth low-pass at `fc` cycles/sample as poles `p`, residues
/// `r`, `z = e^p` and `d = r / p`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Poles {
    p: [C; 8],
    r: [C; 8],
    z: [C; 8],
    d: [C; 8],
}

impl Poles {
    fn new(fc: f64) -> Self {
        let w = fc * 6.283185307179586;
        let p: [C; 8] = std::array::from_fn(|k| {
            let a = (k + 1) as f64 * 0.39269908169872414 + 1.3744467859455345;
            (a.cos() * w, a.sin() * w)
        });
        let f4 = fc * fc * fc * fc;
        let g = f4 * f4 * 2429063.9401140665;
        let r: [C; 8] = std::array::from_fn(|k| {
            let mut acc = (1.0, 0.0);
            for j in (0..8).filter(|&j| j != k) {
                let d = (p[k].0 - p[j].0, p[k].1 - p[j].1);
                acc = (d.0 * acc.0 - d.1 * acc.1, acc.1 * d.0 + d.1 * acc.0);
            }
            cdiv((g, 0.0), acc)
        });
        Self { p, r, z: p.map(|p| cexp(p.0, p.1)), d: std::array::from_fn(|k| cdiv(r[k], p[k])) }
    }

    fn e(&self, k: usize, t: Option<f64>) -> C {
        t.map_or(self.z[k], |t| cexp(self.p[k].0 * t, self.p[k].1 * t))
    }

    fn out(s: &[C; 8]) -> f64 {
        s.iter().fold(0.0, |y, s| y + s.0)
    }

    /// `s <- s e^{p t}`.
    fn decay(&self, s: &mut [C; 8], t: Option<f64>) {
        for (k, s) in s.iter_mut().enumerate() {
            let (er, ei) = self.e(k, t);
            *s = (s.0 * er - s.1 * ei, er * s.1 + s.0 * ei);
        }
    }

    /// `s <- (s + r v) e^{p t}`: an impulse of area `v`.
    fn kick(&self, s: &mut [C; 8], v: f64, t: f64) {
        for (k, s) in s.iter_mut().enumerate() {
            let (a, b) = (s.0 + v * self.r[k].0, s.1 + v * self.r[k].1);
            let (er, ei) = cexp(self.p[k].0 * t, self.p[k].1 * t);
            *s = (er * a - ei * b, ei * a + er * b);
        }
    }

    /// `s <- (s + d v) e^{p t} - d v`: the input held at `v` for `t`.
    fn hold(&self, s: &mut [C; 8], v: f64, t: Option<f64>) {
        for (k, s) in s.iter_mut().enumerate() {
            let (dr, di) = self.d[k];
            let (a, b) = (dr * v + s.0, di * v + s.1);
            let (er, ei) = self.e(k, t);
            *s = ((a * er - b * ei) - dr * v, (a * ei + b * er) - di * v);
        }
    }

    /// The output at `t` with the input held at `v`, state unchanged.
    fn peek_hold(&self, s: &[C; 8], v: f64, t: f64) -> f64 {
        let mut y = 0.0;
        for (k, s) in s.iter().enumerate() {
            let (dr, di) = self.d[k];
            let (er, ei) = cexp(self.p[k].0 * t, self.p[k].1 * t);
            y += ((s.0 + dr * v) * er - (di * v + s.1) * ei) - dr * v;
        }
        y
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Engine {
    pre: Poles,
    post: Poles,
    fs: f32,
    frequency: f32,
    jitter: f32,
    dither: f32,
    levels: f32,
    top: f32,
    inverse: f32,
    mu_law: bool,
    impulses: bool,
    antialias: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct State {
    pre: [[C; 8]; 2],
    post: [[C; 8]; 2],
    previous: [f32; 2],
    held: [f32; 2],
    phase: f32,
    /// Jitter and dither LCG seeds (two CRT `rand()` draws at construction: 41, 18467).
    seeds: [u32; 2],
}

impl Default for State {
    fn default() -> Self {
        Self {
            pre: [[(0.0, 0.0); 8]; 2],
            post: [[(0.0, 0.0); 8]; 2],
            previous: [0.0; 2],
            held: [0.0; 2],
            phase: 0.0,
            seeds: [41, 18467],
        }
    }
}

impl State {
    pub(super) fn finite(&self) -> bool {
        self.pre.iter().chain(&self.post).flatten().all(|c| c.0.is_finite() && c.1.is_finite()) && self.phase.is_finite()
    }
}

fn round_away(x: f64) -> f64 {
    (x.abs() + 0.5).floor().copysign(x)
}

fn lcg(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(0x0019_660d).wrapping_add(0x3c6e_f35f);
    *seed
}

impl Engine {
    pub(super) fn new(r: Redux, rate: u32) -> Self {
        let fs = rate as f32;
        let inv = 1.0 / fs;
        let band = (inv * r.band_limit).min(inv * 16_000.0).min(0.5);
        let levels = 2f32.powf(r.bits - 1.0);
        Self {
            pre: Poles::new(f64::from((0.5 / fs) * r.frequency) * 0.8),
            post: Poles::new(f64::from(band)),
            fs,
            frequency: r.frequency,
            jitter: r.jitter,
            dither: r.dither,
            levels,
            top: levels - 1.0,
            inverse: 1.0 / levels,
            mu_law: r.mu_law,
            impulses: r.impulses,
            antialias: r.antialias,
        }
    }

    fn quantize(&self, xi: f32, tri: f32) -> f32 {
        let sg = if 0.0 <= xi { 1.0f32 } else { -1.0 };
        let level = if self.mu_law {
            let c = ((((xi / sg).abs() * 255.0) + 1.0) as f64).ln() as f32;
            round_away(f64::from(((c * f32::from_bits(0x3e38_aa3b)) * self.levels) + (tri * self.dither)))
        } else {
            round_away(f64::from(((xi / sg) * self.levels) + (tri * self.dither)))
        };
        let level = if level > 0.0 { level as f32 } else { 0.0 };
        let level = if level <= self.top { level } else { self.top };
        if self.mu_law {
            (((256f64.powf(f64::from((self.inverse * level).abs())) as f32) - 1.0) * f32::from_bits(0x3b80_8081)) * sg
        } else {
            (self.inverse * level) * sg
        }
    }

    pub(super) fn process(&self, state: &mut State, block: &mut Planar, len: usize) {
        for i in 0..len {
            let mut ph = state.phase;
            if ph >= 1.0 {
                for c in 0..2 {
                    let x = block[c][i] as f32;
                    if self.antialias {
                        self.pre.hold(&mut state.pre[c], f64::from(x), None);
                    } else {
                        state.previous[c] = x;
                    }
                    block[c][i] = f64::from(Poles::out(&state.post[c]) as f32);
                    if self.impulses {
                        self.post.decay(&mut state.post[c], None);
                    } else {
                        self.post.hold(&mut state.post[c], f64::from(state.held[c]), None);
                    }
                }
            } else {
                let u = (lcg(&mut state.seeds[1]) as f32) * f32::from_bits(0x2f80_0000);
                let tri = if u >= 0.5 { 1.0 - ((1.0 - u) + (1.0 - u)).sqrt() } else { (u + u).sqrt() - 1.0 };
                let t = f64::from(ph);
                for c in 0..2 {
                    let x = block[c][i] as f32;
                    let xi = if self.antialias {
                        let y = self.pre.peek_hold(&state.pre[c], f64::from(x), t) as f32;
                        self.pre.hold(&mut state.pre[c], f64::from(x), None);
                        y
                    } else {
                        let y = ((1.0 - ph) * x) + (ph * state.previous[c]);
                        state.previous[c] = x;
                        y
                    };
                    block[c][i] = f64::from(Poles::out(&state.post[c]) as f32);
                    if self.impulses {
                        self.post.decay(&mut state.post[c], Some(t));
                    } else {
                        self.post.hold(&mut state.post[c], f64::from(state.held[c]), Some(t));
                    }
                    let v = self.quantize(xi, tri);
                    state.held[c] = v;
                    let rest = 1.0 - ph;
                    if self.impulses {
                        self.post.kick(&mut state.post[c], f64::from(v), f64::from(rest));
                    } else {
                        self.post.hold(&mut state.post[c], f64::from(v), (rest != 1.0).then_some(f64::from(rest)));
                    }
                }
                let j = self.jitter;
                let seed = lcg(&mut state.seeds[0]) as f32;
                let den = ((((seed * j) * f32::from_bits(0x2dcc_cccd)) - (j * 0.05)) + 1.0) * self.frequency;
                let p = self.fs / den;
                ph += if p > 1.0 { p } else { 1.0 };
            }
            state.phase = ph - 1.0;
        }
    }
}
