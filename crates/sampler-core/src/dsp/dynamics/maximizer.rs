//! UVI Maximizer (UVI Workstation 4.0.9 snapshot): parameter registration
//! `0x141279360`, module `0x141278ac0`, core init `0x141665430`, prepare
//! `0x141666580`, block process `0x141665620`, envelope `0x141667480`,
//! sliding maximum `0x141667790`, moving average `0x141667180`, true-peak
//! detector `0x1416678d0` (four 12-tap phases at `0x14201f4e0`). f32 audio,
//! linked over the channels.
//!
//! Level is the channel maximum of |x| (or of the 4x true-peak phases) in
//! dB, floored at 1e-10. Below the threshold it is lifted to the threshold
//! (soft knee: `(v - t + k)^2 / 4k + t` within ±k). Less ceiling and
//! threshold, it is slew-limited downwards (`1e4 / (fs * slew ms)` dB per
//! sample), smoothed by an attack / two-speed release envelope (the slow
//! release is 100 times slower; `blend` mixes them; brickwall mode has an
//! instant attack), held by a sliding maximum over the look-ahead window `W`,
//! turned into linear gain `10^((ceiling - (z + ceiling + threshold) + ...)`
//! through the native constants, then averaged twice over `W / 2` frames.
//! The audio is delayed by `W` (plus 6 with true peak); hard mode clips at
//! the linear ceiling. The vector primitives are Intel IPP (`log10`, `exp`)
//! called through an encrypted dispatch table: f32 libm stands in for them.

use super::{Planar, ProcessorState};

/// `ln 10 / 20` (`0x141ea34c0`).
const NEPER_PER_DB: f32 = f32::from_bits(0x3deb_c8e3);
/// Level floor (`0x141e6e4f0`).
const FLOOR: f32 = f32::from_bits(0x2edb_e6ff);
/// True-peak phases, oldest tap first (`0x14201f4e0`).
pub(super) const PHASES: [[u32; 12]; 4] = [
    [
        0x3ae0_0000, 0x3c34_0000, 0xbca1_0000, 0x3d08_0000, 0xbd73_8000, 0x3e0c_a000, 0x3f78_e000,
        0xbdd1_8000, 0x3d43_0000, 0xbcda_0000, 0x3c74_0000, 0xbc08_0000,
    ],
    [
        0xbcef_0000, 0x3cf0_0000, 0xbd54_0000, 0x3db6_8000, 0xbe2a_8000, 0x3eee_2000, 0x3f47_a000,
        0xbe4d_2000, 0x3dd0_0000, 0xbd6e_8000, 0x3d07_8000, 0xbc9b_0000,
    ],
    [
        0xbc9b_0000, 0x3d07_8000, 0xbd6e_8000, 0x3dd0_0000, 0xbe4d_2000, 0x3f47_a000, 0x3eee_2000,
        0xbe2a_8000, 0x3db6_8000, 0xbd54_0000, 0x3cf0_0000, 0xbcef_0000,
    ],
    [
        0xbc08_0000, 0x3c74_0000, 0xbcda_0000, 0x3d43_0000, 0xbdd1_8000, 0x3f78_e000, 0x3e0c_a000,
        0xbd73_8000, 0x3d08_0000, 0xbca1_0000, 0x3c34_0000, 0x3ae0_0000,
    ],
];

/// One phase over the history `r` (`r(0)` oldest), in the native add tree
/// (three accumulators, `0x1416679c0..0x141667bc0`).
#[inline(always)]
pub(super) fn interpolate(p: &[u32; 12], r: impl Fn(usize) -> f32) -> f32 {
    let t = |j: usize| f32::from_bits(p[j]);
    let head = (((t(2) * r(2) + (r(0) * t(0) + r(1) * t(1))) + t(3) * r(3)) + t(4) * r(4)) + (t(5) * r(5) + t(6) * r(6));
    let tail = ((t(7) * r(7) + t(8) * r(8)) + t(9) * r(9)) + (r(11) * t(11) + t(10) * r(10));
    head + tail
}

#[derive(Clone, Copy)]
pub(super) struct Controls {
    pub threshold: f32,
    pub attack: f32,
    pub lookahead: f32,
    pub release: f32,
    pub ceiling: f32,
    pub knee: f32,
    pub blend: f32,
    pub slew: f32,
    /// 0 brickwall, 1 soft, 2 hard.
    pub mode: u8,
    pub true_peak: bool,
}

/// Look-ahead window `W` in frames: `int(ms * fs * .0005) * 2`.
pub(super) fn window(lookahead: f32, rate: u32) -> usize {
    ((lookahead * rate as f32 * 0.0005) as i32).max(0) as usize * 2
}

/// Delay in frames.
pub(super) fn latency(c: &Controls, rate: u32) -> usize {
    window(c.lookahead, rate) + if c.true_peak { 6 } else { 0 }
}

/// Cells: delay ring, deque `[frame, value]`, moving-average rings, true-peak rings.
pub(super) fn cells(c: &Controls, rate: u32) -> usize {
    let w = window(c.lookahead, rate);
    latency(c, rate) + w + w / 2 + 12
}

struct Coefficients {
    attack: f32,
    fast: f32,
    slow: f32,
    slew: f32,
}

impl Coefficients {
    fn new(c: &Controls, rate: u32) -> Self {
        let fs = rate as f32;
        let ln3 = f64::from_bits(0xbff1_93ea_7aad_030b); // -ln 3, 0x141e64248
        let attack = if c.mode == 0 { 0. } else { c.attack * f32::from_bits(0x3a83_126f) * fs };
        let release = (f64::from(c.release) * 0.001 * f64::from(fs)) as f32;
        let pole = |k: f64, t: f32| if t > 0. { (k / f64::from(t)).exp() as f32 } else { 0. };
        Self {
            attack: pole(ln3, attack),
            fast: pole(ln3, release),
            slow: pole(f64::from_bits(0xbf86_7fe4_7410_a7bc), release), // 0x14201f5b0
            slew: if c.slew > 0. { (1e4 / (f64::from(fs) * f64::from(c.slew))) as f32 } else { f32::MAX },
        }
    }
}

/// aux: 0 slew, 1..4 fast/slow/blended envelope, 4..6 moving sums, 6 moving
/// position, 7 delay position, 8 deque head, 9 deque length, 10 frame, 11
/// true-peak position.
pub(super) fn process(c: &Controls, rate: u32, state: &mut ProcessorState, cells: &mut [[f64; 2]], block: &mut Planar, len: usize) {
    let k = Coefficients::new(c, rate);
    let w = window(c.lookahead, rate);
    let n = w / 2;
    let d = latency(c, rate);
    let (delay, rest) = cells.split_at_mut(d);
    let (deque, rest) = rest.split_at_mut(w);
    let (average, tp) = rest.split_at_mut(n);
    let a = &mut state.aux;
    let ceiling = 10f32.powf(c.ceiling * 0.05);
    let offset = c.ceiling + c.threshold;
    for i in 0..len {
        let x = [block[0][i] as f32, block[1][i] as f32];
        let mut level = FLOOR;
        if c.true_peak {
            let at = a[11] as usize;
            tp[at] = [f64::from(x[0]), f64::from(x[1])];
            let at = (at + 1) % 12;
            a[11] = at as f64;
            for ch in 0..2 {
                let r = |j: usize| tp[(at + j) % 12][ch] as f32;
                for p in &PHASES {
                    level = level.max(interpolate(p, r).abs());
                }
            }
        } else {
            level = level.max(x[0].abs()).max(x[1].abs());
        }
        let v = 20. * level.log10();
        let v = if c.knee > 0. {
            if v < c.threshold - c.knee {
                c.threshold
            } else if v < c.knee + c.threshold {
                let u = (v - c.threshold) + c.knee;
                u * u * (0.25 / c.knee) + c.threshold
            } else {
                v
            }
        } else {
            v.max(c.threshold)
        };
        // Slew limit (downwards only), then the attack / release envelope.
        let slew = a[0] as f32;
        let step = (v + -offset) - slew;
        let z = slew + if step < 0. { step.max(-k.slew) } else { step };
        a[0] = f64::from(z);
        let (fast, slow, out) = (a[1] as f32, a[2] as f32, a[3] as f32);
        let (fast, slow) = if out <= z {
            let e = (1. - k.attack) * z;
            (fast * k.attack + e, slow * k.attack + e)
        } else {
            ((1. - k.fast) * z + fast * k.fast, (1. - k.slow) * z + slow * k.slow)
        };
        let out = (slow - fast) * c.blend + fast;
        (a[1], a[2], a[3]) = (f64::from(fast), f64::from(slow), f64::from(out));
        // Sliding maximum over the last `w` frames.
        let frame = a[10];
        let (mut head, mut count) = (a[8] as usize, a[9] as usize);
        while count > 0 && (out as f64) > deque[(head + count - 1) % w][1] {
            count -= 1;
        }
        while count > 0 && deque[head][0] + w as f64 <= frame {
            head = (head + 1) % w;
            count -= 1;
        }
        deque[(head + count) % w] = [frame, f64::from(out)];
        count += 1;
        let held = deque[head][1] as f32;
        (a[8], a[9], a[10]) = (head as f64, count as f64, frame + 1.);
        // Linear gain, then two moving averages.
        let mut g = ((held + offset) * -NEPER_PER_DB + c.ceiling * NEPER_PER_DB).exp();
        let at = a[6] as usize;
        for s in 0..2 {
            let old = average[at][s] as f32;
            average[at][s] = f64::from(g);
            a[4 + s] += f64::from(g) - f64::from(old);
            g = (a[4 + s] / n as f64) as f32;
        }
        a[6] = ((at + 1) % n) as f64;
        // Delay and apply.
        let at = a[7] as usize;
        let delayed = delay[at];
        delay[at] = [f64::from(x[0]), f64::from(x[1])];
        a[7] = ((at + 1) % d) as f64;
        for ch in 0..2 {
            let y = delayed[ch] as f32 * g;
            block[ch][i] = f64::from(if c.mode == 2 { y.clamp(-ceiling, ceiling) } else { y });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: usize = super::super::super::BLOCK;
    const DEFAULT: Controls = Controls {
        threshold: -6.,
        attack: 0.,
        lookahead: 2.,
        release: 10.,
        ceiling: -0.1,
        knee: 0.,
        blend: 0.1,
        slew: 20.,
        mode: 0,
        true_peak: false,
    };

    fn run(c: Controls, amp: f64) -> Vec<f64> {
        let mut state = ProcessorState::default();
        let mut cells = vec![[0.; 2]; cells(&c, 48_000)];
        let mut out = vec![];
        for b in 0..300 {
            let mut block = [[0.; BLOCK]; 2];
            for i in 0..BLOCK {
                let v = amp * ((b * BLOCK + i) as f64 * 997. * std::f64::consts::TAU / 48_000.).sin();
                (block[0][i], block[1][i]) = (v, v);
            }
            process(&c, 48_000, &mut state, &mut cells, &mut block, BLOCK);
            out.extend_from_slice(&block[0]);
        }
        out
    }

    fn peak(v: &[f64]) -> f64 {
        v.iter().fold(0., |p, x| p.max(x.abs()))
    }

    #[test]
    fn window_and_latency_follow_the_native_rounding() {
        assert_eq!(window(2., 48_000), 96);
        assert_eq!(latency(&Controls { true_peak: true, ..DEFAULT }, 44_100), 94);
    }

    #[test]
    fn quiet_audio_gets_the_threshold_to_ceiling_gain() {
        let out = run(DEFAULT, 0.1);
        let want = 0.1 * 10f64.powf(5.9 / 20.);
        assert!((peak(&out[9600..]) / want - 1.).abs() < 0.01, "{}", peak(&out[9600..]));
    }

    #[test]
    fn loud_audio_stays_at_the_ceiling() {
        for c in [DEFAULT, Controls { true_peak: true, knee: 3., ..DEFAULT }, Controls { mode: 2, attack: 1., ..DEFAULT }] {
            let p = peak(&run(c, 1.)[9600..]);
            assert!(p <= 10f64.powf(-0.1 / 20.) * 1.001 && p > 0.9, "{p}");
        }
    }

    #[test]
    fn true_peak_phases_pass_dc() {
        // BS.1770-4 Annex 2 phases sum to 1.0016 / 0.9730 (published taps).
        for p in &PHASES {
            let sum: f32 = p.iter().map(|b| f32::from_bits(*b)).sum();
            assert!((sum - 1.).abs() < 0.03, "{sum}");
        }
    }
}
