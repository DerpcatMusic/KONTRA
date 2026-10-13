//! Kontakt Limiter (SerType 0x1c, `BEffectLimiter`): process `0x140904890`,
//! setter `0x140960140`, reset `0x14094bb10`. f32 throughout, as the core.
//!
//! Two saved knobs: the input gain `x1` (`2^(x1/6)`) and the release `x2` in
//! milliseconds, the half-life of the level follower. Per frame:
//!
//! 1. the input is scaled by the gain and enters a 32-frame audio delay;
//! 2. the channel maximum of `|x|` drives a level follower that rises
//!    instantly and falls by a one-pole step of `ln 2 / release_frames`;
//! 3. the sliding maximum of the level over itself and the previous 32 frames
//!    feeds a fixed low-pass (RBJ biquad at one cycle per 32 frames, shared
//!    with the Pro compressor), so the gain leads the audio by the window;
//! 4. the filtered level `y` becomes the gain `1 / (1 + y^2/4)` up to
//!    `y = 2` and `1 / y` beyond, and the delayed audio times the gain is
//!    clamped to full scale.
//!
//! The wrapper adds no wet/dry stage (its mix helper `0x140ac21f0` only
//! applies a smoothed output level), so `mix` below 1 blends the scaled,
//! delayed input with the limited signal; Kontakt itself never does.

use super::pro::biquad;
use super::{Planar, ProcessorState, Shared};

/// The audio delay and the sliding-maximum ring, 32 frames each.
pub(super) const LATENCY: usize = 32;
pub(super) const CELLS: usize = 2 * LATENCY;

const EPS: f32 = f32::from_bits(0x2193_92ef); // 0x144f385a4, anti-denormal
const LN2: f32 = f32::from_bits(0x3f31_7215); // 0x144f38800
const SIXTH: f32 = f32::from_bits(0x3e2a_aaab); // 0x144737728
const TAU: f32 = f32::from_bits(0x40c9_0fe4); // 0x144f38be0

pub(super) struct Coefficients {
    /// Input gain `2^(x1/6)`.
    gain: f32,
    /// Fraction of a falling step the follower takes per frame.
    release: f32,
    b0: f32,
    b1: f32,
    a1: f32,
    a2: f32,
}

impl Coefficients {
    /// `knobs` = [input gain x1, unused, unused, release s].
    pub fn new([x1, _, _, release]: [f64; 4], rate: u32) -> Self {
        let rate = rate as f32;
        let frames = (release as f32 * 1000. * 0.001 * rate).max(1.);
        // Cut-off at a fixed 1/32 cycle per frame: (rate/32) * (tau / rate).
        let (b0, b1, a1, a2) = biquad((rate * 0.031_25) * (TAU / rate));
        Self {
            gain: 2_f32.powf(x1 as f32 * SIXTH),
            release: LN2 / frames,
            b0,
            b1,
            a1,
            a2,
        }
    }
}

/// State: `aux[0]` audio cursor, `aux[1]` ring cursor, `aux[2]` follower,
/// `aux[3..5]` the filter memory (w1, w2). `cells[..32]` the delayed audio
/// (both channels), `cells[32..]` the level ring.
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
    let (audio, levels) = cells.split_at_mut(LATENCY);
    let [mut cursor, mut ring] = [state.aux[0] as usize, state.aux[1] as usize];
    let [mut follower, mut w1, mut w2] = [2, 3, 4].map(|i| state.aux[i] as f32);
    for i in 0..len {
        let scaled = [block[0][i] as f32 * c.gain, block[1][i] as f32 * c.gain];
        cursor = (cursor + 1) % LATENCY;
        let delayed = audio[cursor].map(|v| v as f32);
        audio[cursor] = scaled.map(f64::from);
        let peak = scaled[0].abs().max(scaled[1].abs());
        let step = peak - follower;
        let level = if step <= 0. { step * c.release + follower } else { peak };
        follower = level + EPS;
        // The window holds the 32 previous levels and this one.
        ring = (ring + 1) % LATENCY;
        let window = levels.iter().fold(level, |m, cell| m.max(cell[0] as f32));
        levels[ring][0] = f64::from(level);
        let w = window + w1 * c.a1 + w2 * c.a2;
        let y = w * c.b0 + w1 * c.b1 + w2 * c.b0;
        (w2, w1) = (w1 + EPS, w + EPS);
        let m = y.min(2.);
        let gain = 1. / ((y - (1. - m * 0.25) * m) + 1.);
        for ch in 0..2 {
            let limited = (gain * delayed[ch]).clamp(-1., 1.);
            block[ch][i] = if shared.mix == 1. {
                f64::from(limited) * shared.makeup
            } else {
                f64::from(delayed[ch]) + shared.mix * (f64::from(limited) * shared.makeup - f64::from(delayed[ch]))
            };
        }
    }
    state.aux[0] = cursor as f64;
    state.aux[1] = ring as f64;
    for (i, v) in [follower, w1, w2].into_iter().enumerate() {
        state.aux[2 + i] = f64::from(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ir::fx::dynamics::Link;

    const BLOCK: usize = super::super::super::BLOCK;

    fn run(knobs: [f64; 4], input: &[[f64; 2]]) -> Vec<[f64; 2]> {
        let shared = Shared { link: Link::Max, makeup: 1., mix: 1. };
        let mut state = ProcessorState::default();
        let mut cells = [[0.; 2]; CELLS];
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
    fn audio_is_delayed_by_the_window() {
        let mut input = vec![[0.; 2]; 100];
        input[0] = [0.01, -0.02];
        let out = run([0., 0., 0., 0.1], &input);
        assert!(out[..LATENCY].iter().all(|f| *f == [0., 0.]));
        // A tiny sample sees y ~ 0: unity gain, 32 frames late.
        assert!((out[LATENCY][0] - f64::from(0.01_f32)).abs() < 1e-5, "{:?}", out[LATENCY]);
        assert!((out[LATENCY][1] + f64::from(0.02_f32)).abs() < 1e-5);
    }

    #[test]
    fn steady_full_scale_settles_to_the_soft_curve() {
        // y -> 1 (unity filter DC gain), gain 1/(1 + 1/4) = 0.8.
        let out = run([0., 0., 0., 0.1], &vec![[1., 1.]; 4800]);
        assert!((out[4799][0] - 0.8).abs() < 1e-3, "{}", out[4799][0]);
    }

    #[test]
    fn a_hot_input_is_held_at_full_scale() {
        // 12 saved dB is x4: y -> 4 > 2, gain 1/y, output exactly full scale.
        let out = run([12., 0., 0., 0.1], &vec![[1., -1.]; 4800]);
        assert!((out[4799][0] - 1.).abs() < 1e-3, "{}", out[4799][0]);
        assert!((out[4799][1] + 1.).abs() < 1e-3);
        assert!(out.iter().flatten().all(|v| v.abs() <= 1.));
    }

    #[test]
    fn the_channel_maximum_drives_both_channels() {
        let out = run([0., 0., 0., 0.1], &vec![[1., 0.01]; 4800]);
        let ratio = out[4799][1] / out[4799][0];
        assert!((ratio - 0.01).abs() < 1e-5, "{ratio}");
    }

    #[test]
    fn release_is_the_half_life_of_the_level_follower() {
        let c = Coefficients::new([0., 0., 0., 0.1], 48_000);
        // 100 ms = 4800 frames: after that many, the gap halves (to ln2 accuracy).
        let remaining = (1. - c.release).powi(4800);
        assert!((remaining - 0.5).abs() < 0.02, "{remaining}");
        // Gain is 2^(x/6): six saved dB double the input.
        assert!((Coefficients::new([6., 0., 0., 0.1], 48_000).gain - 2.).abs() < 1e-6);
    }
}
