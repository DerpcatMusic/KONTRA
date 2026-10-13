//! Kontakt Transparent Limiter (SerType 0x5f, `BEffectHilbertLimiter`, core
//! `NI::DSP::HilbertLimiter` at effect + 0x1c0): process `0x141ae3400`,
//! setter `0x141aea680`, prepare `0x141ae23a0`, Hilbert pair `0x141aa4400`
//! (designed by `0x141a81a90`), gain smoother `0x141aa42a0` (`0x141acb630`).
//! f32 throughout except the smoother, which is f64 like the core.
//!
//! Controls (normalized, clamped to 0..=1 by the wrapper `0x1409aa9e0`):
//! 0 drive `(1 - x) * 40` dB, 1 release `max(x, 0.001)` s, 2 ceiling
//! `-0.35 - (1 - x) * 39.7` dB, 3 external side-chain (not modelled here).
//!
//! Per frame, with `L = 128 * max(1, int(rate / 44100))`:
//!
//! 1. both channels are driven, delayed `L - 1` frames, and their driven
//!    magnitudes join an `L`-frame sliding maximum;
//! 2. each channel's Hilbert magnitude (two 7-stage first-order allpass
//!    chains, warped by -0.89) is followed by a one-pole that rises with
//!    `1 - 71.43/rate` and falls with `1 - 333.3/rate`;
//! 3. the largest of the two envelopes and the two raw magnitudes, in dB,
//!    rises instantly and falls with the release; a symmetric one-pole
//!    (`1 - 1666.7/rate`) smooths it into a gain `-max(level, 0)` dB, further
//!    capped by the sliding maximum so no driven peak in the window passes
//!    0 dBFS;
//! 4. the linear gain passes an `L`-tap Hann FIR (a comb plus resonator,
//!    re-synchronised once per second as the core does), then scales the
//!    delayed audio together with the ceiling.

use super::{Planar, ProcessorState, Shared};

const EPS: f32 = f32::from_bits(0x2b8c_bccc); // 0x144f385a8, 1e-12
const STAGES: usize = 7;
/// Prototype allpass coefficients, lanes [A, B, A, B], 0x1479ab7f0..
const PROTOTYPE: [[u32; 2]; 6] = [
    [0x3ea8_8690, 0x3f18_24e4],
    [0x3f46_065d, 0x3f61_b493],
    [0x3f71_c293, 0x3f7b_d4e2],
    [0xbea8_8690, 0xbf18_24e4],
    [0xbf46_065d, 0xbf61_b493],
    [0xbf71_c293, 0xbf7b_d4e2],
];
/// Warp of the pair, `-0.89` (reset `0x141ae15b0` stores 0.89, negated).
const WARP: f32 = -f32::from_bits(0x3f63_d70a);

/// `max(1, int(rate / 44100))`, the core's `rate * 2.2675737e-5`.
fn multiple(rate: u32) -> usize {
    ((rate as f32 * f32::from_bits(0x37be_37c6)) as i32).max(1) as usize
}

/// Look-ahead window `L`.
pub(super) fn window(rate: u32) -> usize {
    multiple(rate) * 128
}

pub(super) fn latency(rate: u32) -> usize {
    window(rate) - 1
}

/// Ring length shared by the audio, peak and smoother histories.
fn ring(rate: u32) -> usize {
    (window(rate) + 1).next_power_of_two()
}

/// Cells: audio ring `[l, r]`, history ring `[peak, gain]`, Hilbert state
/// (7 stages x 8 slots x 4 lanes, two lanes per cell), the smoother's fresh
/// accumulators.
pub(super) fn cells(rate: u32) -> usize {
    2 * ring(rate) + STAGES * 8 * 2 + 2
}

struct Coefficients {
    drive: f32,
    ceiling: f32,
    release: f32,
    rise: f32,
    fall: f32,
    smooth: f32,
    /// Hilbert stage coefficients per lane.
    allpass: [[f32; 4]; STAGES],
    delay: usize,
    taps: usize,
    cos: f64,
    /// `1 / L`, as the core keeps it.
    scale: f64,
    period: usize,
}

impl Coefficients {
    /// `knobs` = [drive x0, ceiling x2, unused, release x1 s], all 0..=1.
    fn new([drive, ceiling, _, release]: [f64; 4], rate: u32) -> Self {
        let fs = rate as f32;
        let unit = |x: f64| (x as f32).clamp(0., 1.);
        let db = |d: f32| 10f32.powf(d * f32::from_bits(0x3d4c_cccd));
        let drive_db = (1. - unit(drive)) * 40.;
        let ceiling_db = f32::from_bits(0xbeb3_3333) - (1. - unit(ceiling)) * f32::from_bits(0x421e_cccd);
        let release = unit(release).max(f32::from_bits(0x3a83_126f));
        let mut allpass = [[WARP; 4]; STAGES];
        for (stage, [a, b]) in allpass.iter_mut().zip(PROTOTYPE) {
            let warp = |c: u32| {
                let c = f32::from_bits(c);
                (c + WARP) / (c * WARP + 1.)
            };
            *stage = [warp(a), warp(b), warp(a), warp(b)];
        }
        let taps = window(rate);
        Self {
            drive: db(drive_db),
            ceiling: db(ceiling_db),
            release: 1. - 1. / (release * fs),
            rise: 1. - f32::from_bits(0x428e_db6d) / fs,
            fall: 1. - f32::from_bits(0x43a6_aaab) / fs,
            smooth: 1. - f32::from_bits(0x44d0_5555) / fs,
            allpass,
            delay: multiple(rate).min(8),
            taps,
            cos: (1. / taps as f64 * std::f64::consts::TAU).cos(),
            scale: 1. / taps as f64,
            period: (rate as usize).max(taps),
        }
    }
}

/// State: `aux[0]` frame counter, `aux[1..3]` the Hilbert envelopes,
/// `aux[3]` released level, `aux[4]` smoothed level (dB), `aux[5]` held
/// window maximum, `aux[6]` its remaining frames, `aux[7..10]` the smoother
/// (sum, y1, y2), `aux[10]` frames since the last re-sync.
pub(super) fn process(
    shared: &Shared,
    knobs: [f64; 4],
    rate: u32,
    state: &mut ProcessorState,
    cells: &mut [[f64; 2]],
    block: &mut Planar,
    len: usize,
) {
    let c = Coefficients::new(knobs, rate);
    let size = ring(rate);
    let mask = size - 1;
    let (audio, rest) = cells.split_at_mut(size);
    let (history, rest) = rest.split_at_mut(size);
    let (allpass, fresh) = rest.split_at_mut(STAGES * 16);
    let mut n = state.aux[0] as usize;
    let [mut env_l, mut env_r, mut released, mut smoothed, mut held] =
        [1, 2, 3, 4, 5].map(|i| state.aux[i] as f32);
    let mut left = state.aux[6] as usize;
    let [mut sum, mut y1, mut y2] = [state.aux[7], state.aux[8], state.aux[9]];
    let mut since = state.aux[10] as usize;
    let taps = c.taps;
    for i in 0..len {
        let at = n & mask;
        let x = [block[0][i] as f32, block[1][i] as f32];
        let driven = x.map(|v| c.drive * v + EPS);
        audio[at] = driven.map(f64::from);
        // Sliding maximum of the driven magnitudes over the last `L` frames
        // (the core keeps one per channel in dB; the larger one wins).
        let peak = driven[0].abs().max(driven[1].abs());
        history[at][0] = f64::from(peak);
        if peak >= held {
            held = peak;
            left = taps - 1;
        } else if left == 0 {
            held = 0.;
            let mut age = 0;
            for back in 0..taps {
                let v = history[(n.wrapping_sub(back)) & mask][0] as f32;
                if v > held {
                    held = v;
                    age = back;
                }
            }
            left = taps - age - 1;
        } else {
            left -= 1;
        }
        // Hilbert pair per channel: lanes [L a, L b, R a, R b].
        let mut lanes = [driven[0], driven[0], driven[1], driven[1]];
        let mut last_input = lanes;
        let (write, read) = ((n + 1) & 7, (n + 1).wrapping_sub(c.delay) & 7);
        for (s, coef) in c.allpass.iter().enumerate() {
            last_input = lanes;
            let slot = |k: usize, j: usize| s * 16 + k * 2 + j / 2;
            for (k, lane) in lanes.iter_mut().enumerate() {
                let r = allpass[slot(read, k)][k & 1] as f32;
                let v = (*lane - r) * coef[k];
                allpass[slot(write, k)][k & 1] = f64::from(v + *lane);
                *lane = v + r;
            }
        }
        // Lanes a skip the last (warp) stage.
        let magnitude = |re: f32, im: f32| (re * re + im * im).sqrt();
        let follow = |env: f32, m: f32| {
            let k = if m <= env { c.fall } else { c.rise };
            (env - m) * k + m
        };
        env_l = follow(env_l, magnitude(last_input[0], lanes[1]));
        env_r = follow(env_r, magnitude(last_input[2], lanes[3]));
        let level = env_l.max(env_r).max(driven[0].abs()).max(driven[1].abs());
        let mut db = level.log10() * 20.;
        if db <= released {
            db = (released - db) * c.release + db;
        }
        released = db;
        smoothed = (smoothed - db) * c.smooth + db;
        let mut gain_db = (-smoothed).min(0.);
        let window_db = held.log10() * 20.;
        if window_db + gain_db > 0. {
            gain_db = -window_db;
        }
        let gain = f64::from(10f32.powf(gain_db * f32::from_bits(0x3d4c_cccd)));
        // L-tap Hann FIR: sum minus the first DFT bin's real part, over L.
        history[at][1] = gain;
        let d = gain - history[n.wrapping_sub(taps) & mask][1];
        sum += d;
        let y0 = y1 * 2. * c.cos + d - y2;
        let smoothed_gain = (sum - (y0 - y1 * c.cos)) * c.scale;
        (y2, y1) = (y1, y0);
        since += 1;
        if since > c.period - taps {
            let [[s, a], [b, _]] = [fresh[0], fresh[1]];
            let f0 = a * 2. * c.cos + gain - b;
            fresh[0] = [s + gain, f0];
            fresh[1][0] = a;
            if since >= c.period {
                (sum, y1, y2) = (fresh[0][0], fresh[0][1], fresh[1][0]);
                fresh[0] = [0.; 2];
                fresh[1] = [0.; 2];
                since = 0;
            }
        }
        let scale = smoothed_gain as f32 * c.ceiling;
        let delayed = audio[n.wrapping_sub(taps - 1) & mask];
        for ch in 0..2 {
            let wet = f64::from(scale * delayed[ch] as f32) * shared.makeup;
            block[ch][i] = if shared.mix == 1. { wet } else { delayed[ch] + shared.mix * (wet - delayed[ch]) };
        }
        n = n.wrapping_add(1);
    }
    state.aux[0] = (n % (1 << 52)) as f64;
    for (i, v) in [env_l, env_r, released, smoothed, held].into_iter().enumerate() {
        state.aux[1 + i] = f64::from(v);
    }
    state.aux[6] = left as f64;
    [state.aux[7], state.aux[8], state.aux[9]] = [sum, y1, y2];
    state.aux[10] = since as f64;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ir::fx::dynamics::Link;

    const BLOCK: usize = super::super::super::BLOCK;

    fn run(knobs: [f64; 4], input: &[[f64; 2]]) -> Vec<[f64; 2]> {
        let shared = Shared { link: Link::Max, makeup: 1., mix: 1. };
        let mut state = ProcessorState::default();
        let mut cells = vec![[0.; 2]; cells(48_000)];
        let mut out = vec![];
        for chunk in input.chunks(BLOCK) {
            let mut block = [[0.; BLOCK]; 2];
            for (i, f) in chunk.iter().enumerate() {
                (block[0][i], block[1][i]) = (f[0], f[1]);
            }
            process(&shared, knobs, 48_000, &mut state, &mut cells, &mut block, chunk.len());
            out.extend((0..chunk.len()).map(|i| [block[0][i], block[1][i]]));
        }
        out
    }

    #[test]
    fn window_and_latency_follow_the_rate() {
        assert_eq!((window(44_100), latency(48_000)), (128, 127));
        assert_eq!(window(96_000), 256);
    }

    #[test]
    fn quiet_audio_passes_at_the_ceiling_after_the_latency() {
        // Unity drive (x0 = 1), ceiling -0.35 dB (x2 = 1).
        let mut input = vec![[0.; 2]; 400];
        input[10] = [0.01, -0.02];
        let out = run([1., 1., 0., 0.01], &input);
        assert!(out[..10 + 127].iter().all(|f| f[0].abs() < 1e-9));
        let ceiling = 10f64.powf(-0.35 / 20.);
        assert!((out[137][0] - 0.01 * ceiling).abs() < 1e-5, "{:?}", out[137]);
        assert!((out[137][1] + 0.02 * ceiling).abs() < 1e-5);
    }

    #[test]
    fn a_hot_sine_never_passes_the_ceiling() {
        // 20 dB of drive into a full-scale sine: every output peak stays at
        // or under the ceiling, and the limiter is doing real work.
        let input: Vec<[f64; 2]> = (0..48_000)
            .map(|i| {
                let v = (i as f64 * 440. * std::f64::consts::TAU / 48_000.).sin();
                [v, v]
            })
            .collect();
        let out = run([0.5, 1., 0., 0.05], &input);
        let ceiling = 10f64.powf(-0.35 / 20.);
        let peak = out.iter().map(|f| f[0].abs()).fold(0., f64::max);
        assert!(peak <= ceiling * 1.000_01, "{peak}");
        assert!(peak > ceiling * 0.9, "{peak}");
    }

    #[test]
    fn hilbert_pair_is_ninety_degrees_apart_mid_band() {
        // A 1 kHz sine: the envelope magnitude settles flat (no ripple at 2f).
        let c = Coefficients::new([1., 1., 0., 0.01], 48_000);
        let mut rings = [[0f32; 8]; STAGES * 4];
        let mut mags = vec![];
        for n in 0..4_000usize {
            let x = (n as f32 * 1000. * std::f32::consts::TAU / 48_000.).sin();
            let mut lanes = [x; 4];
            let mut last = lanes;
            for (s, coef) in c.allpass.iter().enumerate() {
                last = lanes;
                for (k, lane) in lanes.iter_mut().enumerate() {
                    let ring = &mut rings[s * 4 + k];
                    let r = ring[(n + 1 - c.delay) & 7];
                    let v = (*lane - r) * coef[k];
                    ring[(n + 1) & 7] = v + *lane;
                    *lane = v + r;
                }
            }
            mags.push((last[0] * last[0] + lanes[1] * lanes[1]).sqrt());
        }
        let tail = &mags[3_000..];
        let (lo, hi) = tail.iter().fold((f32::MAX, 0f32), |(a, b), m| (a.min(*m), b.max(*m)));
        assert!(hi - lo < 0.01 && (hi - 1.).abs() < 0.01, "{lo} {hi}");
    }
}
