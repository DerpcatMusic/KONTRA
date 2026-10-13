//! Kontakt Feedback Compressor (SerType 0x4c, `BEffectFBComp`, core
//! `NI::DSP::FBComp` at effect + 0x1c0): process `0x141aa80d0`, setter
//! `0x141ac56d0`, ratio law `0x141a91d10`, prepare `0x141aa1280`, time
//! constants `0x141a92370` / `0x141ac3250`. f32 audio path, f64 detector.
//!
//! Per frame: the input is driven, its magnitude (the channel maximum when
//! linked) passes a 10 Hz high-pass, then the feedback detector runs 2 (4 in
//! high quality) sub-steps: the detector reads the input times the last gain,
//! a peak follower rises with the attack and relaxes with the release toward
//! a ratio-dependent floor while two coupled one-poles (178.8 rad/s per
//! sub-step) average it; above the threshold `T` the gain is
//! `exp(c4 * ln(m/T)^4)` up to the knee top `T * k`, then the power law
//! `g0 * (m / (T k))^(1 - s)`. The gain times the driven audio passes a
//! first-order low-pass and a 10 Hz DC blocker, then the output gain and the
//! dry/wet mix (dry is the undriven input). Exponentials use the native
//! `2^(i/60 - 20)` table (`ar_kernel::exponential`).

use super::super::filter::ar_kernel::exponential;
use super::{Planar, ProcessorState, Shared};
use sampler_ir::fx::dynamics::Link;

const EPS: f32 = f32::from_bits(0x2193_92ef); // core + 0x48, 1e-18
/// `60 / ln 2`: table units per neper (0x1479ab5b0).
const K: f32 = f32::from_bits(0x42ad_1f97);

/// Detector state cells: `[A, D, E, G]` per detector channel (f64).
pub(super) const CELLS: usize = 4;

struct Coefficients {
    drive: f32,
    attack: f32,
    release: f32,
    couple: f32,
    steps: usize,
    threshold: f32,
    knee_top: f32,
    slope: f32,
    g0: f32,
    quartic: f32,
    floor: f32,
    bypass: bool,
    hp: [f32; 3],
    lp: [f32; 3],
}

fn table_exp(nepers_times_k: f32) -> f32 {
    exponential(nepers_times_k + 1200.)
}

impl Coefficients {
    /// `knobs` = [drive (linear), ratio (1..=20), attack s, release s].
    fn new([drive, ratio, attack, release]: [f64; 4], high_quality: bool, rate: u32) -> Self {
        let fs = rate as f32;
        let steps = if high_quality { 4 } else { 2 };
        let sub = 1. / steps as f32;
        let one_pole = |t: f32| 1. - table_exp(-((sub / (t * fs)) * K));
        // Ratio law, 0x141a91d10.
        let r = ratio as f32;
        let r1 = r - 1.;
        let r1d = f64::from(r1);
        let slope = ((r1d * r1d) * (r1d * r1d) * f64::from_bits(0x3f10_ecd0_0000_0000)
            + f64::from(r1 * f32::from_bits(0x4019_702b))
            + 1.) as f32;
        let threshold = r.max(2.).powf(f32::from_bits(0x3e4c_cccd)) * f32::from_bits(0x3f34_359a)
            - f32::from_bits(0x3f2c_2971);
        let knee = ((f32::from_bits(0x4066_83fb) / r + f32::from_bits(0x3f57_1cdc))
            - f32::from_bits(0x3fb8_6405) / (r.sqrt() * r))
            + f32::from_bits(0x3d4c_cccd);
        let l = knee.ln();
        let om = 1. - slope;
        let g0 = table_exp(om * l * f32::from_bits(0x41ad_1f97));
        let ld = f64::from(l);
        let quartic = ((f64::from(om) / (ld * ld * ld)) * 0.25) as f32;
        let root = (f64::from_bits(0xbf87_9415_0000_0000) / f64::from(quartic).min(-0.1)).powf(0.25);
        let floor = table_exp(root as f32 * K) * threshold;
        // Detector high-pass and output DC blocker (10 Hz), gain low-pass.
        let t = ((1. / fs) * f32::from_bits(0x41fb_53d1)).tan();
        let b = 1. / (t + 1.);
        let w = (10_000. / fs).atan().tan() * f32::from_bits(0x4875_6fda);
        let inv = 1. / (w + 20_000.);
        Self {
            drive: drive as f32,
            attack: one_pole(attack as f32),
            release: one_pole(release as f32),
            couple: 1. - table_exp(-((f32::from_bits(0x4332_d8b3) / fs) * sub * K)),
            steps,
            threshold,
            knee_top: threshold * knee,
            slope,
            g0,
            quartic,
            floor,
            bypass: slope == 1.,
            hp: [b, -b, (t - 1.) * b],
            lp: [w * inv, w * inv, (w - 20_000.) * inv],
        }
    }

    /// One detector sub-step on channel state `[a, d, e, g]`.
    fn step(&self, det: f64, s: &mut [f64; 4]) {
        let [mut a, mut d, mut e, _] = *s;
        let v = det * s[3];
        if v >= e {
            e = (v - e) * f64::from(self.attack) + e;
        }
        let eps = f64::from(EPS);
        let couple = f64::from(self.couple);
        let e2 = ((f64::from(self.floor) - e) * f64::from(self.release) + e) + (a - e) * couple + eps;
        d = (a - d) * couple + d + eps;
        a = (e2 + d) * 0.5 + eps;
        e = e2;
        let g = if a > f64::from(self.threshold) {
            if a > f64::from(self.knee_top) {
                let y = (a / f64::from(self.knee_top)).ln() as f32;
                f64::from(table_exp(y * (1. - self.slope) * K)) * f64::from(self.g0)
            } else {
                let y = (a / f64::from(self.threshold)).ln();
                let y2 = y * y;
                f64::from(table_exp((f64::from(self.quartic) * y2 * y2) as f32 * K))
            }
        } else {
            1.
        };
        *s = [a, d, e, eps + g];
    }
}

/// `knobs` as [`Coefficients::new`]; `aux[0..2]` detector high-pass (x1, y1)
/// per channel pair, `aux[4..8]` gain low-pass, `aux[8..12]` DC blocker
/// (x1 then y1, two channels each), `aux[12]` set once the detector started.
pub(super) fn process(
    shared: &Shared,
    knobs: [f64; 4],
    high_quality: bool,
    rate: u32,
    state: &mut ProcessorState,
    cells: &mut [[f64; 2]],
    block: &mut Planar,
    len: usize,
) {
    let c = Coefficients::new(knobs, high_quality, rate);
    let linked = shared.link != Link::Off;
    let mut det_state: [[f64; 4]; 2] =
        std::array::from_fn(|ch| std::array::from_fn(|k| cells[k][ch]));
    if state.aux[12] == 0. || c.bypass {
        let floor = f64::from(c.floor);
        det_state = [[floor, floor, floor, 1.]; 2];
        state.aux[12] = 1.;
    }
    let mut f: [f32; 12] = std::array::from_fn(|i| state.aux[i] as f32);
    let mix = shared.mix as f32;
    let makeup = shared.makeup as f32;
    for i in 0..len {
        let x = [block[0][i] as f32, block[1][i] as f32];
        let s = x.map(|v| v * c.drive + EPS);
        let dets = if linked { 1 } else { 2 };
        let raw = if linked { [s[0].abs().max(s[1].abs()); 2] } else { s.map(f32::abs) };
        for (k, st) in det_state.iter_mut().enumerate().take(dets) {
            let (x1, y1) = (f[2 * k], f[2 * k + 1]);
            let y = (raw[k] * c.hp[0] + x1 * c.hp[1]) - y1 * c.hp[2];
            (f[2 * k], f[2 * k + 1]) = (raw[k] + EPS, y + EPS);
            for _ in 0..c.steps {
                c.step(f64::from(y), st);
            }
        }
        if c.bypass {
            let floor = f64::from(c.floor);
            det_state = [[floor, floor, floor, 1.]; 2];
        }
        for ch in 0..2 {
            let g = det_state[if linked { 0 } else { ch }][3] as f32;
            let u = g * s[ch];
            let (lx, ly) = (4 + 2 * ch, 5 + 2 * ch);
            let o = (u * c.lp[0] + f[lx] * c.lp[1]) - c.lp[2] * f[ly];
            (f[lx], f[ly]) = (u, o);
            let (hx, hy) = (8 + 2 * ch, 9 + 2 * ch);
            let y = (o * c.hp[0] + f[hx] * c.hp[1]) - f[hy] * c.hp[2];
            (f[hx], f[hy]) = (o + EPS, y + EPS);
            let wet = y * makeup;
            block[ch][i] = f64::from((1. - mix) * x[ch] + mix * wet);
        }
    }
    for (i, v) in f.iter().enumerate() {
        state.aux[i] = f64::from(*v);
    }
    for (k, cell) in cells.iter_mut().enumerate().take(CELLS) {
        *cell = [det_state[0][k], det_state[1][k]];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: usize = super::super::super::BLOCK;

    fn run(knobs: [f64; 4], link: Link, input: &[[f64; 2]]) -> Vec<[f64; 2]> {
        let shared = Shared { link, makeup: 1., mix: 1. };
        let mut state = ProcessorState::default();
        let mut cells = [[0.; 2]; CELLS];
        let mut out = vec![];
        for chunk in input.chunks(BLOCK) {
            let mut block = [[0.; BLOCK]; 2];
            for (i, f) in chunk.iter().enumerate() {
                (block[0][i], block[1][i]) = (f[0], f[1]);
            }
            process(&shared, knobs, true, 48_000, &mut state, &mut cells, &mut block, chunk.len());
            out.extend((0..chunk.len()).map(|i| [block[0][i], block[1][i]]));
        }
        out
    }

    fn sine(amp: f64, frames: usize) -> Vec<[f64; 2]> {
        (0..frames)
            .map(|i| {
                let v = amp * (i as f64 * 1000. * std::f64::consts::TAU / 48_000.).sin();
                [v, v]
            })
            .collect()
    }

    fn peak(out: &[[f64; 2]]) -> f64 {
        out.iter().map(|f| f[0].abs()).fold(0., f64::max)
    }

    #[test]
    fn ratio_law_is_continuous_at_the_knee_top() {
        for x in [0.1f32, 0.5, 1.] {
            let c = Coefficients::new([1., f64::from(x * 19. + 1.), 0.01, 0.1], true, 48_000);
            let l = c.knee_top / c.threshold;
            let knee = (c.quartic * l.ln().powi(4)).exp();
            assert!((knee - c.g0).abs() < 1e-3 * c.g0, "{x}: {knee} {}", c.g0);
            assert!(c.threshold > 0. && l > 1. && c.slope > 1.);
        }
    }

    #[test]
    fn ratio_one_is_transparent_apart_from_the_filters() {
        let out = run([1., 1., 0.001, 0.1], Link::Max, &sine(0.5, 9600));
        assert!((peak(&out[4800..]) - 0.5).abs() < 0.01, "{}", peak(&out[4800..]));
    }

    #[test]
    fn a_loud_signal_is_compressed_more_than_a_quiet_one() {
        // The detector high-passes the rectified drive, so a steady sine
        // reads about 0.36 of its driven peak: drive 10 puts 1.0 above T.
        let knobs = [10., 20., 0.001, 0.1];
        let loud = peak(&run(knobs, Link::Max, &sine(1., 24_000))[19_200..]);
        let quiet = peak(&run(knobs, Link::Max, &sine(0.1, 24_000))[19_200..]);
        // 20 dB in gives clearly less than 20 dB out.
        assert!(loud / quiet < 5., "{loud} {quiet}");
    }
}
