//! The gain body `0x8e04c0..0x8e53b1`, translated region by region.
//!
//! One call runs one hop of one analyzer. Everything the body computes itself is here, in
//! binary order. The plug-in callees it makes go through [`Callees`], so the body can be
//! checked against a replay of the emulator's callee records (the test below) before every
//! callee is translated. The translated callees live in the sibling modules.
//!
//! Pipeline (addresses are the body's own blocks):
//!
//! 1. `0x8e0629..0x8e06ee` zero the output view and the frame scratch; the frame is built
//!    and transformed by [`Callees::spectrum`] (`0x894a70`, `0x894210`, `0x8e67a0`, `0x8e6da0`,
//!    the sidechain path `0x8e0ab8`, and the IPP real FFT `0x1010`).
//! 2. `0x8e1390` per-bin magnitude `0x6181b0` into `D` (ch x bins).
//! 3. `0x8e13b5` more than two channels: `0x8ec2a0`, `0x8ec320` on `D`.
//! 4. `0x8e13dc..0x8e15af` spectral-flatness gate ([`flatness_gate`]).
//! 5. Quality 1 (`arg8`, `0x8e15c9`): floor `D` at `10^((focus*20-60)/20) * 0x89ad70(D)`.
//!    Otherwise (`0x8e17c4..0x8e1b2e`): weights `0x8ec820`, low-band floor subtraction, `max 0`.
//! 6. `0x8e1ecb` spectral smoother `0x8ec8e0` on state `a18`, `0x8e2025` copy back `0x8ed100`.
//! 7. `0x8e202a` `cfg+0x53` set: `D *= 0` (MulC with `xmm13` = 0).
//! 8. Quality 1 (`0x8e20eb..0x8e2c58`): [`quality1_gain`] law, then the common tail.
//!    Otherwise (`0x8e2c5d..0x8e4b7f`): band analysis `0x8ef8b0`, [`band_tilt`], for the long
//!    analyzer the band-state ratio (`0x8e2eac..0x8e3bb9`), `max 0.01`, `ln`, synthesis
//!    `0x8f03f0`, `exp(-x)`, depth scale, curve, `P = D * G`, link, `A = -ln(max(P, 1))`.
//! 9. `0x8e4b9f..0x8e5098` common tail ([`tail_channel`]), `0x8f08c0`, final multiplier `478`.

use super::ipp;
use super::tables::{T800, T900, T940, T980, T9C0};

/// A multichannel vector ("view"). A view with one channel broadcasts against N channels.
pub type View = Vec<Vec<f32>>;

/// The channel of `v` that pairs with channel `c` of an `n`-channel operand, by the binary's
/// broadcast rule: same channel count -> per channel; one channel -> broadcast; else no-op.
fn pick(v: &View, c: usize, n: usize) -> Option<&[f32]> {
    if v.len() == n {
        Some(&v[c])
    } else if v.len() == 1 {
        Some(&v[0])
    } else {
        None
    }
}

/// The analyzer fields the body reads (`rcx`): DSP object + `0x3248` or + `0x600`.
#[derive(Clone, Debug, Default)]
pub struct Analyzer {
    /// `[+0x48]` channels.
    pub channels: usize,
    /// `[+0x40]` bins (257 or 2049).
    pub bins: usize,
    /// `[+0x60]` hop.
    pub hop: u64,
    /// `[+0x20]` sample rate.
    pub rate: f32,
    /// `[+0x58]` long analyzer flag (band-state ratio path).
    pub long: bool,
    /// `+0x2c8` per-bin weights.
    pub weights: View,
    /// `+0x358` quality-1 mask.
    pub mask: View,
    /// `+0x3e8` gain curve.
    pub curve: View,
    /// `+0x478` final multiplier.
    pub final_mul: View,
    /// `+0x8f8` link multiplier.
    pub link_mul: View,
    /// `+0xa18` spectral smoother state.
    pub spec_state: View,
    /// `+0xbc8` / `+0xc58` spectral smoother rise / fall coefficients.
    pub spec_rise: View,
    pub spec_fall: View,
    /// `+0xaa8` output smoother state (log domain).
    pub out_state: View,
    /// `+0xd78` / `+0xce8` output smoother rise / fall coefficients (used only with one channel).
    pub out_rise: View,
    pub out_fall: View,
    /// `+0xb38` band state (long analyzer), read after [`Callees::band_state`].
    pub band_state: View,
}

/// The parameter block (`[+0x2c38]` = DSP object + `0x54`) and the stack flags.
#[derive(Clone, Copy, Debug, Default)]
pub struct Params {
    /// `cfg+0` depth (normalised).
    pub depth: f32,
    /// `cfg+4` focus.
    pub focus: f32,
    /// `cfg+0x28` link amount (`0x8ed1c0` `xmm1`).
    pub link: f32,
    /// `cfg+0x3c` (`0x8f08c0` `xmm1`).
    pub out_smooth: f32,
    /// `cfg+0x4c` max cut, dB.
    pub max_cut_db: f32,
    /// `cfg+0x52` (`0x8f08c0` `r9b`, frame path `0x8e6da0`).
    pub flag52: bool,
    /// `cfg+0x53` zeroes the smoothed spectrum.
    pub flag53: bool,
    /// `arg8`: quality index == 1 (`[cfg+0xb0] == 1`).
    pub quality1: bool,
}

/// The plug-in callees the body makes. Bank 0 of [`Callees::bin_smooth`] is the record array
/// at `+0x15b8`, bank 1 the one at `+0x20b8` (record `c` at `+c*0xb0`).
pub trait Callees {
    /// Frame building and the forward FFT: per channel, CCS spectrum of `2 * bins` floats.
    fn spectrum(&mut self, spec: &mut View);
    /// `0x8ec2a0` then `0x8ec320` (more than two channels).
    fn multi_pre(&mut self, d: &mut View);
    /// `0x89ad70`: a level from `D` (quality 1).
    fn flat_level(&mut self, d: &View) -> f32;
    /// `0x88dbc0(rec, dst, src)`, `dst` may alias `src` (then `src` is `None`).
    fn bin_smooth(&mut self, bank: usize, c: usize, dst: &mut [f32], src: Option<&[f32]>);
    /// `0x8ed1c0(view, groups, link)`.
    fn link(&mut self, p: &mut View, link: f32);
    /// `0x8eda30(analyzer, view)`.
    fn link_multi(&mut self, p: &mut View);
    /// `0x8ef8b0(filterbank +0xf28, E, D)`: band energies, ch x 16.
    fn band_analysis(&mut self, d: &View, e: &mut View);
    /// `0x8ef9c0`: band-state update of `+0xb38` from `E` with coefficient `coef`.
    fn band_state(&mut self, e: &View, coef: f32, state: &mut View);
    /// `0x8f0210` on the clamped ratio view.
    fn band_spread(&mut self, r: &mut View);
    /// `0x8f03f0(filterbank, G, L)`: bands to bins.
    fn band_synthesis(&mut self, l: &View, g: &mut View);
    /// `0x8ec2a0`, `0x8ec320`, `0xf0500` on the output (more than two channels).
    fn multi_post(&mut self, a: &mut View);
    /// `0x8f08c0(view, groups, cfg+0x3c, cfg+0x52)`.
    fn out_smooth(&mut self, a: &mut View, amount: f32, flag52: bool);
}

/// Body scratch (the arena allocations), sized once.
#[derive(Clone, Debug, Default)]
pub struct Scratch {
    pub spec: View,
    pub d: View,
    pub e: View,
    pub g: View,
    pub p: View,
    pub tv: View,
    pub dnv: View,
    pub t: Vec<f32>,
    pub dn: Vec<f32>,
    pub r: View,
    pub r2: View,
}

impl Scratch {
    pub fn new(channels: usize, bins: usize) -> Self {
        let v = |n: usize| vec![vec![0.0; n]; channels];
        Self {
            spec: v(2 * bins),
            d: v(bins),
            e: v(BANDS),
            g: v(bins),
            p: v(bins),
            tv: v(bins),
            dnv: v(bins),
            t: vec![0.0; bins],
            dn: vec![0.0; bins],
            r: v(BANDS),
            r2: v(BANDS),
        }
    }
}

/// Bands of the filterbank (`0x8e2c94`: 16).
pub const BANDS: usize = 16;

fn clamp01(x: f32) -> f32 {
    // maxss x, 0 ; minss x, 1 (x in the destination: a NaN x gives 0... then 0)
    let x = if x > 0.0 { x } else { 0.0 };
    if x < 1.0 { x } else { 1.0 }
}

/// `0x8e13dc..0x8e15af`: per channel, mean relative step over the first `bins >> 2` bins;
/// when it is below 0.001 the channel is scaled by it (`0x1220` MulC).
pub fn flatness_gate(d: &mut [f32], n4: usize) {
    let mut acc = 0.0f32;
    let mut prev = d[0];
    for k in 1..n4 {
        let m = d[k];
        let step = (m - prev).abs(); // subss, andps 0x7fffffff
        acc = (f64::from(step) / (f64::from(prev) + 1e-7) + f64::from(acc)) as f32;
        prev = m;
    }
    acc /= n4 as i32 as f32;
    if !(f64::from(acc) >= 0.001) {
        ipp::mul_c_i(acc, d);
    }
}

/// `0x8e1960..0x8e1b2e`: per channel in order, `v = dot(D[c][..32], 0x209a800) * 10^-0.45`
/// is subtracted from every channel; then `D = max(D, 0)`.
pub fn floor_subtract(d: &mut View, table: &[f32; 32]) {
    let p = 10f32.powf(-0.45);
    for c in 0..d.len() {
        let v = ipp::dot_prod(&d[c][..32], table) * p;
        for ch in d.iter_mut() {
            ipp::sub_c_i(v, ch);
        }
    }
    for ch in d.iter_mut() {
        ipp::threshold_lt_i(ch, 0.0);
    }
}

/// `0x8ec8e0`: `t = D - S; dn = min(t, 0); t = max(t, 0); S += t * rise; S += dn * fall`
/// (each step over all channels before the next).
pub fn spec_smooth(s: &mut View, d: &View, rise: &View, fall: &View, t: &mut View, dn: &mut View) {
    let n = d.len();
    for c in 0..n {
        ipp::sub(&s[c], &d[c], &mut t[c]);
    }
    for c in 0..n {
        ipp::threshold_gt(&t[c], &mut dn[c], 0.0);
    }
    for c in 0..n {
        ipp::threshold_lt_i(&mut t[c], 0.0);
    }
    for c in 0..n {
        if let Some(k) = pick(rise, c, n) {
            ipp::add_product(&t[c], k, &mut s[c]);
        }
    }
    for c in 0..n {
        if let Some(k) = pick(fall, c, n) {
            ipp::add_product(&dn[c], k, &mut s[c]);
        }
    }
}

/// `0x8e20eb..0x8e21f2`: the quality-1 scale applied to `D` (`0x1220` MulC, `xmm8 * xmm7`).
pub fn quality1_scale(depth: f32, focus: f32) -> f32 {
    let x2 = (depth - 0.8) * 5.000_000_5; // 0x209d250, 0x209d5f0
    let m = if x2 >= 0.0 { if x2 <= 1.0 { x2 } else { 1.0 } } else { 0.0 };
    let a = clamp01((1.0 - focus * 0.5) * m) * 1.386_294_4; // 0x209d324 = ln 4
    let e8 = a.exp();
    let l = (10f32.powf(2.4) + 0.1).ln(); // 0x209d500, 0x209d0c8
    let x = (l - -2.302_585_1) * clamp01(depth) - 2.302_585_1;
    let mut x7 = x.exp() - 0.1;
    let f = clamp01(focus * focus) * -24.0 * 0.05; // 0x209da4c, xmm10
    x7 *= 10f32.powf(f);
    e8 * x7
}

/// `0x8e22c0..0x8e23c0`, one channel: `D -= smooth0(D); D = smooth1(D); D = max(D, 0)`.
fn quality1_channel(cb: &mut impl Callees, c: usize, d: &mut [f32], tmp: &mut [f32]) {
    cb.bin_smooth(0, c, tmp, Some(d));
    ipp::sub_i(tmp, d);
    cb.bin_smooth(1, c, tmp, Some(d));
    ipp::copy(tmp, d);
    ipp::threshold_lt_i(d, 0.0);
}

/// `0x8e2de0..0x8e2e88`, one channel of band energies `e` (16 bands).
pub fn band_tilt(e: &mut [f32], focus: f32) {
    let a = maxss(ipp::dot_prod(&T980, e), 1e-7); // 0x209d004
    let b = maxss(ipp::dot_prod(&T940, e), 1e-7);
    ipp::add_product_c(&T900, b / a * a, e);
    ipp::add_product_c(&T9C0, a / b * b * (1.0 - focus), e);
}

/// `maxss a, b`: `b` unless `a > b`.
fn maxss(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

/// `0x8e40ec..0x8e413d`: the depth scale of the synthesised gain.
pub fn depth_scale(depth: f32, focus: f32) -> f32 {
    let x = (2.0 - focus * focus + 0.333).ln(); // 0x209d4a0, 0x209d16c
    let x = (x - -1.099_612_8) * clamp01(depth) - 1.099_612_8;
    x.exp() - 0.333
}

/// `0x8e2eac..0x8e2f50`: band-state coefficient `1 - 0.1^(1 / (rate/hop * 1e-3 * 500))` (f64 pow).
pub fn band_coef(rate: f32, hop: u64) -> f32 {
    let h = hop as f32; // cvtsi2ss (unsigned fix-up for the top bit)
    let x = rate / h * 0.001 * 500.0;
    // 0x209d348 is 0.1f32 widened (0.10000000149011612)
    (1.0 - f64::from(0.1f32).powf(1.0 / f64::from(x))) as f32
}

/// `0x8e4ce0..0x8e5074`, one channel of the common tail. `lo` = `ln(10^-3)`,
/// `cut` = `ln(10^(-0.05 * max_cut_db))`.
#[allow(clippy::too_many_arguments)]
fn tail_channel(
    cb: &mut impl Callees,
    c: usize,
    a: &mut [f32],
    s: &mut [f32],
    rise: &View,
    fall: &View,
    t: &mut [f32],
    dn: &mut [f32],
    lo: f32,
    cut: f32,
) {
    ipp::threshold_lt_i(a, lo);
    cb.bin_smooth(1, c, a, None);
    ipp::sub(s, a, t);
    ipp::threshold_gt(t, dn, 0.0);
    ipp::threshold_lt_i(t, 0.0);
    if rise.len() == 1 {
        ipp::add_product(t, &rise[0], s);
    }
    if fall.len() == 1 {
        ipp::add_product(dn, &fall[0], s);
    }
    ipp::copy(s, a);
    ipp::threshold_lt_i(a, cut);
    cb.bin_smooth(1, c, a, None);
}

/// The body. `out` is the caller's view (`rdx`): `-ln(gain)` per bin on return.
pub fn body(an: &mut Analyzer, p: &Params, out: &mut View, cb: &mut impl Callees, sc: &mut Scratch) {
    let n = an.channels;
    for ch in out.iter_mut() {
        ipp::zero(ch);
    }
    cb.spectrum(&mut sc.spec);
    let d = &mut sc.d;
    for c in 0..n {
        ipp::zero(&mut d[c]);
    }
    for c in 0..n {
        super::super::magnitude::magnitude_bins(&mut d[c], &sc.spec[c]);
    }
    if n > 2 {
        cb.multi_pre(d);
    }
    for ch in d.iter_mut() {
        flatness_gate(ch, an.bins >> 2);
    }
    if p.quality1 {
        let s = cb.flat_level(d);
        let lvl = 10f32.powf((clamp01(p.focus) * 20.0 - 60.0) * 0.05) * s; // 0x209d6c8, 0x209d78c
        for ch in d.iter_mut() {
            ipp::threshold_lt_i(ch, lvl);
        }
    } else {
        for c in 0..n {
            if let Some(w) = pick(&an.weights, c, n) {
                ipp::mul_i(w, &mut d[c]);
            }
        }
        floor_subtract(d, &T800);
    }
    spec_smooth(&mut an.spec_state, d, &an.spec_rise, &an.spec_fall, &mut sc.tv, &mut sc.dnv);
    for c in 0..n {
        ipp::copy(&an.spec_state[c], &mut d[c]);
    }
    if p.flag53 {
        for ch in d.iter_mut() {
            ipp::mul_c_i(0.0, ch);
        }
    }
    if p.quality1 {
        let k = quality1_scale(p.depth, p.focus);
        for ch in d.iter_mut() {
            ipp::mul_c_i(k, ch);
        }
        for c in 0..n {
            quality1_channel(cb, c, &mut d[c], &mut sc.t);
        }
        mul_view(d, &an.mask);
        cb.link(d, p.link);
        cb.link_multi(d);
        mul_view(d, &an.link_mul);
        for ch in d.iter_mut() {
            for x in ch.iter_mut() {
                *x *= *x; // 0x1340 Mul_I(src = dst)
            }
        }
        for ch in d.iter_mut() {
            ipp::add_c_i(1.0, ch);
        }
        for c in 0..n {
            ipp::ln(&d[c], &mut out[c]);
        }
        let k = -0.25 - p.focus.powf(4.0) * 0.25; // 0x209d5d0, 0x209d130, 0x209d994
        for ch in out.iter_mut() {
            ipp::mul_c_i(k, ch);
        }
    } else {
        let e = &mut sc.e;
        cb.band_analysis(d, e);
        for ch in e.iter_mut() {
            band_tilt(ch, p.focus);
        }
        if an.long {
            let coef = band_coef(an.rate, an.hop);
            cb.band_state(e, coef, &mut an.band_state);
            band_ratio(e, &an.band_state, &mut sc.r, &mut sc.r2, cb);
        }
        for ch in e.iter_mut() {
            ipp::threshold_lt_i(ch, 0.01);
        }
        for c in 0..n {
            ipp::ln(&e[c], &mut sc.r[c]); // L (arena, ch x 16)
        }
        cb.band_synthesis(&sc.r, &mut sc.g);
        let g = &mut sc.g;
        for ch in g.iter_mut() {
            ipp::mul_c_i(-1.0, ch);
        }
        for ch in g.iter_mut() {
            ipp::exp_i(ch);
        }
        let k = depth_scale(p.depth, p.focus);
        for ch in g.iter_mut() {
            ipp::mul_c_i(k, ch);
        }
        mul_view(g, &an.curve);
        let pv = &mut sc.p;
        for c in 0..n {
            ipp::mul(&d[c], &g[c], &mut pv[c]);
        }
        cb.link(pv, p.link);
        cb.link_multi(pv);
        mul_view(pv, &an.link_mul);
        for ch in pv.iter_mut() {
            ipp::threshold_lt_i(ch, 1.0);
        }
        for c in 0..n {
            ipp::ln(&pv[c], &mut out[c]);
        }
        for ch in out.iter_mut() {
            ipp::mul_c_i(-1.0, ch);
        }
    }
    // common tail 0x8e4b9f
    if n > 2 {
        cb.multi_post(out);
    }
    let lo = 10f32.powf(-3.0).ln(); // 0x209d9fc
    let cut = 10f32.powf(p.max_cut_db * -0.05).ln(); // 0x209d984
    for c in 0..n {
        let (t, dn) = (&mut sc.t, &mut sc.dn);
        tail_channel(cb, c, &mut out[c], &mut an.out_state[c], &an.out_rise, &an.out_fall, t, dn, lo, cut);
    }
    cb.out_smooth(out, p.out_smooth, p.flag52);
    mul_view(out, &an.final_mul);
}

/// `0x8e2fxx..0x8e3b9e` (long analyzer): `R = max(E / max(B, 1e-5), 1)`, `C = spread(R)`,
/// `R /= max(C, 1)`, `E *= R`. `B` is the band state after `0x8ef9c0`.
fn band_ratio(e: &mut View, b: &View, r: &mut View, c2: &mut View, cb: &mut impl Callees) {
    let n = e.len();
    // T = Threshold(B, 1e-5) per B channel; R = E / T with the broadcast rule.
    let mut tb = [[0.0f32; BANDS]; 16];
    for (i, ch) in b.iter().enumerate() {
        ipp::threshold_lt(ch, &mut tb[i], 1e-5); // 0x209d010
    }
    for c in 0..n {
        let ti = if b.len() == n { c } else if b.len() == 1 { 0 } else { continue };
        ipp::div(&tb[ti], &e[c], &mut r[c]);
    }
    for ch in r.iter_mut() {
        ipp::threshold_lt_i(ch, 1.0);
    }
    for c in 0..n {
        ipp::copy(&r[c], &mut c2[c]);
    }
    cb.band_spread(c2);
    for ch in c2.iter_mut() {
        ipp::threshold_lt_i(ch, 1.0);
    }
    for c in 0..n {
        ipp::div_i(&c2[c], &mut r[c]);
    }
    for c in 0..n {
        ipp::mul_i(&r[c], &mut e[c]);
    }
}

/// `p *= m` with the broadcast rule (the view-multiply blocks, e.g. `0x8e2559..0x8e25ce`).
fn mul_view(p: &mut View, m: &View) {
    let n = p.len();
    for c in 0..n {
        if let Some(k) = pick(m, c, n) {
            ipp::mul_i(k, &mut p[c]);
        }
    }
}
