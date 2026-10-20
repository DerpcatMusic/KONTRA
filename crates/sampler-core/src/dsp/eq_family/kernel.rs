//! The one EQ kernel: a second-order design from [`crate::dsp::biquad`] (or a native law's own
//! coefficients) realised as a TPT state-variable section: [`Svf::from_biquad`] and [`Svf::run`].
//!
//! Realisation: the Cytomic/Zavalishin TPT SVF (state `s1`, `s2`, output `m0 x + m1 v1 + m2 v2`).
//! Any biquad maps onto it exactly, its states are well behaved when coefficients change at
//! control cadence, and it costs one 3-state update per sample and channel (about 14 flops).
pub(super) use crate::dsp::biquad::Coefficients as Biquad;
#[cfg(test)]
use std::f64::consts::TAU;

/// TPT state-variable realisation of a biquad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Svf {
    a: [f64; 3],
    m: [f64; 3],
}

impl Svf {
    pub const PASS: Self = Self { a: [1., 0., 0.], m: [1., 0., 0.] };

    /// Same poles and numerator as `f` (denominator sums must be positive: a stable section).
    pub fn from_biquad(f: &Biquad) -> Option<Self> {
        let ([b0, b1, b2], [a1, a2]) = (f.b, f.a);
        let (plus, minus) = (1. + a1 + a2, 1. - a1 + a2);
        if !(plus > 0. && minus > 0.) {
            return None;
        }
        let g = (plus / minus).sqrt();
        let k = 2. * (1. - a2) / (g * minus);
        let d = 4. / minus; // 1 + g k + g^2
        let m0 = d * (b0 + b2 - b1) / 4.;
        let m2 = (d * b1 - 2. * m0 * (g * g - 1.)) / (2. * g * g);
        let m1 = (d * (b0 - b2) - 2. * g * k * m0) / (2. * g);
        let a_1 = 1. / (1. + g * (g + k));
        let c = Self { a: [a_1, g * a_1, g * g * a_1], m: [m0, m1, m2] };
        c.a.iter().chain(&c.m).all(|v| v.is_finite()).then_some(c)
    }

    pub fn to_array(self) -> [f64; 6] {
        [self.a[0], self.a[1], self.a[2], self.m[0], self.m[1], self.m[2]]
    }

    pub fn from_array([a0, a1, a2, m0, m1, m2]: [f64; 6]) -> Self {
        Self { a: [a0, a1, a2], m: [m0, m1, m2] }
    }

    /// Filter `x` in place; `s` is the section's `[s1, s2]` state.
    #[cfg(test)]
    #[inline]
    pub fn run(&self, s: &mut [f64; 2], x: &mut [f64]) {
        let ([a1, a2, a3], [m0, m1, m2]) = (self.a, self.m);
        let [mut s1, mut s2] = *s;
        for sample in x {
            let v3 = *sample - s2;
            let v1 = a1 * s1 + a2 * v3;
            let v2 = s2 + a2 * s1 + a3 * v3;
            s1 = 2. * v1 - s1;
            s2 = 2. * v2 - s2;
            *sample = m0 * *sample + m1 * v1 + m2 * v2;
        }
        *s = [s1, s2];
    }

    /// The same recurrence for two independent channels in one sample loop.
    /// Keeping the channel states in adjacent lanes lets the compiler use
    /// baseline stereo SIMD without changing any per-channel arithmetic.
    #[inline]
    pub fn run_stereo(&self, states: &mut [[f64; 2]; 2], [left, right]: [&mut [f64]; 2]) {
        debug_assert_eq!(left.len(), right.len());
        let ([a1, a2, a3], [m0, m1, m2]) = (self.a, self.m);
        let mut s1 = [states[0][0], states[1][0]];
        let mut s2 = [states[0][1], states[1][1]];
        for (left, right) in left.iter_mut().zip(right) {
            let sample = [*left, *right];
            let v3: [f64; 2] = std::array::from_fn(|c| sample[c] - s2[c]);
            let v1: [f64; 2] = std::array::from_fn(|c| a1 * s1[c] + a2 * v3[c]);
            let v2: [f64; 2] = std::array::from_fn(|c| s2[c] + a2 * s1[c] + a3 * v3[c]);
            s1 = std::array::from_fn(|c| 2. * v1[c] - s1[c]);
            s2 = std::array::from_fn(|c| 2. * v2[c] - s2[c]);
            [*left, *right] = std::array::from_fn(|c| m0 * sample[c] + m1 * v1[c] + m2 * v2[c]);
        }
        *states = std::array::from_fn(|c| [s1[c], s2[c]]);
    }

    /// Independent channel/voice lanes. The recurrence and its expression
    /// association are identical to run_stereo; masking rests completed voices.
    #[cfg(test)]
    #[inline(always)]
    pub fn run_lanes<const N: usize, const MASK: bool>(
        coefficients: &[Self; N],
        states: &mut [[f64; N]; 2],
        block: &mut [[f64; N]],
        ends: &[usize; N],
    ) {
        let a: [[f64; N]; 3] = std::array::from_fn(|i| std::array::from_fn(|lane| coefficients[lane].a[i]));
        let m: [[f64; N]; 3] = std::array::from_fn(|i| std::array::from_fn(|lane| coefficients[lane].m[i]));
        let [mut s1, mut s2] = *states;
        for (i, samples) in block.iter_mut().enumerate() {
            let v3: [f64; N] = std::array::from_fn(|lane| samples[lane] - s2[lane]);
            let v1: [f64; N] = std::array::from_fn(|lane| a[0][lane] * s1[lane] + a[1][lane] * v3[lane]);
            let v2: [f64; N] = std::array::from_fn(|lane| s2[lane] + a[1][lane] * s1[lane] + a[2][lane] * v3[lane]);
            for lane in 0..N {
                if !MASK || i < ends[lane] {
                    s1[lane] = 2. * v1[lane] - s1[lane];
                    s2[lane] = 2. * v2[lane] - s2[lane];
                    samples[lane] = m[0][lane] * samples[lane] + m[1][lane] * v1[lane] + m[2][lane] * v2[lane];
                }
            }
        }
        *states = [s1, s2];
    }

    /// Dispatch the nonrecursive numeric kernel locally. The outer stage
    /// dispatcher recurses for Mix and therefore remains an out-of-line
    /// baseline function even when called inside an AVX2 closure.
    #[inline]
    pub fn run_lanes_dispatched<const N: usize, const MASK: bool>(
        coefficients: &[Self; N],
        states: &mut [[f64; N]; 2],
        block: &mut [[f64; N]],
        ends: &[usize; N],
    ) {
        sampler_simd::dispatch(
            #[inline(always)]
            || Self::run_lanes_blocked::<N, MASK>(coefficients, states, block, ends),
        );
    }

    /// Four independent doubles fit one AVX2 vector. Complete their temporal
    /// recurrences before moving to the next group, limiting live coefficient
    /// and history registers. Per-lane sample order and association are intact.
    #[inline(always)]
    fn run_lanes_blocked<const N: usize, const MASK: bool>(
        coefficients: &[Self; N],
        states: &mut [[f64; N]; 2],
        block: &mut [[f64; N]],
        ends: &[usize; N],
    ) {
        assert!(N.is_multiple_of(4));
        for group in 0..N / 4 {
            let first = group * 4;
            let a: [[f64; 4]; 3] = std::array::from_fn(|i| std::array::from_fn(|lane| coefficients[first + lane].a[i]));
            let m: [[f64; 4]; 3] = std::array::from_fn(|i| std::array::from_fn(|lane| coefficients[first + lane].m[i]));
            let mut s1: [f64; 4] = std::array::from_fn(|lane| states[0][first + lane]);
            let mut s2: [f64; 4] = std::array::from_fn(|lane| states[1][first + lane]);
            for (i, samples) in block.iter_mut().enumerate() {
                for lane in 0..4 {
                    if !MASK || i < ends[first + lane] {
                        let sample = samples[first + lane];
                        let v3 = sample - s2[lane];
                        let v1 = a[0][lane] * s1[lane] + a[1][lane] * v3;
                        let v2 = s2[lane] + a[1][lane] * s1[lane] + a[2][lane] * v3;
                        s1[lane] = 2. * v1 - s1[lane];
                        s2[lane] = 2. * v2 - s2[lane];
                        samples[first + lane] = m[0][lane] * sample + m[1][lane] * v1 + m[2][lane] * v2;
                    }
                }
            }
            for lane in 0..4 {
                states[0][first + lane] = s1[lane];
                states[1][first + lane] = s2[lane];
            }
        }
    }
}

#[cfg(test)]
mod stereo_cpu_tests {
    use super::*;
    use crate::dsp::biquad::cookbook;
    use sampler_ir::fx::eq::Kind;

    #[test]
    fn blocked_dispatch_preserves_scalar_lane_bits_and_inactive_histories() {
        const N: usize = 16;
        let coefficients = std::array::from_fn(|lane| Svf::from_biquad(&cookbook(Kind::Peak, 48000, 500. + (lane/2) as f64 * 113., 0.7, 6.).unwrap()).unwrap());
        for len in [0, 1, 3, 31, 32, 33, 63, 64] {
            let initial: [[f64; N]; 2] = std::array::from_fn(|row| std::array::from_fn(|lane| if lane%3 == 0 { -0.0 } else { ((row + lane) as f64 * 0.173).sin() }));
            let input: [[f64; N]; 64] = std::array::from_fn(|i| std::array::from_fn(|lane| match (i+lane)%17 {
                0 => -0.0, 1 => f64::from_bits(1), _ => ((i + lane*7) as f64 * 0.137).sin(),
            }));
            for masked in [false, true] {
                let ends = std::array::from_fn(|lane| if masked { [0,1,3,7,31,32,33,63][lane/2].min(len) } else { len });
                let (mut old, mut new) = (input, input);
                let (mut old_states, mut new_states) = (initial, initial);
                if masked {
                    Svf::run_lanes::<N, true>(&coefficients, &mut old_states, &mut old[..len], &ends);
                    Svf::run_lanes_dispatched::<N, true>(&coefficients, &mut new_states, &mut new[..len], &ends);
                } else {
                    Svf::run_lanes::<N, false>(&coefficients, &mut old_states, &mut old[..len], &ends);
                    Svf::run_lanes_dispatched::<N, false>(&coefficients, &mut new_states, &mut new[..len], &ends);
                }
                assert_eq!(new.map(|frame| frame.map(f64::to_bits)), old.map(|frame| frame.map(f64::to_bits)));
                assert_eq!(new_states.map(|row| row.map(f64::to_bits)), old_states.map(|row| row.map(f64::to_bits)));
            }
        }
    }

    #[test]
    #[ignore = "isolated optimized baseline-versus-dispatched EQ recurrence CPU receipt"]
    fn lane_dispatch_cpu_receipt() {
        use std::{hint::black_box, time::Instant};
        const N: usize = 16;
        let coefficients = std::array::from_fn(|lane| Svf::from_biquad(&cookbook(Kind::Peak, 48000, 500. + lane as f64 * 113., 0.7, 6.).unwrap()).unwrap());
        let input: [[f64; N]; 64] = std::array::from_fn(|i| std::array::from_fn(|lane| ((i + lane*7) as f64 * 0.137).sin()));
        let ends = [64; N];
        for round in 0..4 {
            let mut elapsed = [0.; 2];
            let mut fingerprints = [0.; 2];
            let mut final_states = [[[0.; N]; 2]; 2];
            for candidate in [round % 2, 1 - round % 2] {
                let mut states = [[0.; N]; 2];
                let mut block = input;
                let started = Instant::now();
                for _ in 0..100000 {
                    block = input;
                    if candidate == 0 {
                        Svf::run_lanes::<N, false>(black_box(&coefficients), &mut states, black_box(&mut block), &ends);
                    } else {
                        Svf::run_lanes_dispatched::<N, false>(black_box(&coefficients), &mut states, black_box(&mut block), &ends);
                    }
                    fingerprints[candidate] += black_box(block[63][3]);
                }
                elapsed[candidate] = started.elapsed().as_secs_f64();
                final_states[candidate] = states;
                black_box(block);
            }
            assert_eq!(fingerprints[0].to_bits(), fingerprints[1].to_bits());
            assert_eq!(final_states[0].map(|s| s.map(f64::to_bits)), final_states[1].map(|s| s.map(f64::to_bits)));
            println!("eq_dispatch_stage round={round} baseline_ms={:.3} dispatched_ms={:.3} ratio={:.4}", elapsed[0]*1000., elapsed[1]*1000., elapsed[1]/elapsed[0]);
        }
    }

    #[test]
    fn stereo_recurrence_preserves_scalar_pcm_and_history_bits_when_coefficients_move() {
        let mut random = 0x1234_5678_u64;
        for len in [0, 1, 3, 4, 7, 31, 32, 63, 64] {
            for start in [[[0., -0.], [-0., 0.]], [[1e-300, -1e-300], [-0.01, 0.02]]] {
                let (mut actual_state, mut expected_state) = (start, start);
                for cell in 0..200 {
                    let rate = [32000, 44100, 48000, 96000][cell % 4];
                    let kind = [Kind::Peak, Kind::LowShelf, Kind::HighShelf, Kind::LowPass, Kind::HighPass, Kind::Notch][cell % 6];
                    let hz = 20. + (cell % 97) as f64 / 100. * 0.49 * f64::from(rate);
                    let f = cookbook(kind, rate, hz, 0.5 + (cell % 5) as f64, (cell % 19) as f64 - 9.).unwrap();
                    let svf = Svf::from_biquad(&f).unwrap();
                    let mut actual: [[f64; 64]; 2] = std::array::from_fn(|_| std::array::from_fn(|i| {
                        random ^= random << 13;
                        random ^= random >> 7;
                        random ^= random << 17;
                        match i % 11 {
                            0 => 0., 1 => -0., 2 => 1e-300, 3 => -1e-300,
                            _ => (random as f64 / u64::MAX as f64 - 0.5) * 0.25,
                        }
                    }));
                    let mut expected = actual;
                    for (channel, state) in expected.iter_mut().zip(&mut expected_state) {
                        svf.run(state, &mut channel[..len]);
                    }
                    let [left, right] = &mut actual;
                    svf.run_stereo(&mut actual_state, [&mut left[..len], &mut right[..len]]);
                    assert_eq!(actual.map(|c| c.map(f64::to_bits)), expected.map(|c| c.map(f64::to_bits)), "PCM len={len} cell={cell}");
                    assert_eq!(actual_state.map(|c| c.map(f64::to_bits)), expected_state.map(|c| c.map(f64::to_bits)), "history len={len} cell={cell}");
                }
            }
        }
    }
}

/// Complex response of a biquad at `hz` (tests, error tables).
#[cfg(test)]
pub(super) fn response(f: &Biquad, rate: u32, hz: f64) -> (f64, f64) {
    let w = TAU * hz / f64::from(rate);
    let poly = |c: [f64; 3]| {
        (c[0] + c[1] * w.cos() + c[2] * (2. * w).cos(), -c[1] * w.sin() - c[2] * (2. * w).sin())
    };
    let ((nr, ni), (dr, di)) = (poly(f.b), poly([1., f.a[0], f.a[1]]));
    let d = dr * dr + di * di;
    ((nr * dr + ni * di) / d, (ni * dr - nr * di) / d)
}

/// Magnitude of a biquad at `hz`.
#[cfg(test)]
pub(super) fn magnitude(f: &Biquad, rate: u32, hz: f64) -> f64 {
    let (re, im) = response(f, rate, hz);
    re.hypot(im)
}
