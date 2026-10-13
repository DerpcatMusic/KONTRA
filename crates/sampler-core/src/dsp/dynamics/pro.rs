//! Kontakt Compressor, Pro (mode 2): core `0x140ab35d0`, coefficients from the
//! setter `0x140ab9220` (mode 2 branch) and the state initialiser around
//! `0x140ac3cdd`. f32 throughout, as the core.
//!
//! Signal path, per frame: the channel maximum of `|x|` joins a 32-frame
//! sliding-maximum ring (33 values: the 32 before it and itself); its
//! logarithm (a quadratic mantissa approximation, in dB) minus the threshold
//! passes a 12 dB quadratic soft knee; falling levels follow a one-pole
//! release, rising ones pass instantly into a low-pass biquad whose cut-off
//! comes from the attack; the filtered excess becomes a gain through a
//! quadratic `2^x`. The audio reaches the gain through a 32-frame delay, so
//! the gain leads the signal by the whole window. The core never reads the
//! stereo-link byte: it always detects on the channel maximum.

use super::{Planar, ProcessorState, Shared};

/// The delay ring and the sliding-maximum ring, 32 frames each.
pub(super) const LATENCY: usize = 32;
pub(super) const CELLS: usize = 2 * LATENCY;

const EPS: f32 = f32::from_bits(0x2193_92ef); // 0x144f385a4, anti-denormal
const FLOOR: f32 = f32::from_bits(0x2b8c_bccc); // 0x144f385a8
const DB_PER_OCTAVE: f32 = f32::from_bits(0x40c0_a82f); // 0x144f38bd4
const LOG_A: f32 = f32::from_bits(0x4101_aea6); // 0x144fc4484
const LOG_B: f32 = f32::from_bits(0x4005_69ee); // 0x144fc406c
const KNEE_DB: f32 = 6.; // 0x144f38bd0: half the 12 dB knee, and the dB per knee unit
const KNEE_UNIT: f32 = f32::from_bits(0x3e2a_aac1); // 0x144f38720
const EXP_SCALE: f32 = f32::from_bits(0x3e2a_1512); // 0x144f3871c
const EXP_A: f32 = f32::from_bits(0x3e7d_f3b6); // 0x144f38744
const EXP_B: f32 = f32::from_bits(0x3f35_3f7d); // 0x144f38808

/// The detector low-pass shared by the Kontakt limiter cores: the RBJ biquad
/// (Q = 1/(2 * 0.91)) at angle `w`, sine and cosine from the setter's
/// polynomials rather than libm. Returns `(b0, b1, a1, a2)`; `b2 = b0`.
pub(super) fn biquad(w: f32) -> (f32, f32, f32, f32) {
    let w2 = w * w;
    let sin = (((((w2 * f32::from_bits(0xb2cd_4ca8) + f32::from_bits(0x3638_b94f)) * w2
        - f32::from_bits(0x3950_0c03))
        * w2
        + f32::from_bits(0x3c08_8885))
        * w2
        - f32::from_bits(0x3e2a_aac1))
        * w2
        + 1.)
        * w
        * f32::from_bits(0x3f68_f5c3);
    let inverse = 1. / (sin + 1.);
    let cos = ((((w2 * f32::from_bits(0xb48b_dad9) + f32::from_bits(0x37cf_b5a1)) * w2
        - f32::from_bits(0x3ab6_09bd))
        * w2
        + f32::from_bits(0x3d2a_aa99))
        * w2
        - f32::from_bits(0x3eff_f5e8))
        * w2
        + 1.;
    let one_minus_cos = 1. - cos;
    (
        one_minus_cos * 0.5 * inverse,
        one_minus_cos * inverse,
        -(cos * -2. * inverse),
        -((1. - sin) * inverse),
    )
}

pub(super) struct Coefficients {
    threshold_db: f32,
    /// `-(1 - ratio)`.
    reduction: f32,
    /// `(1 - ratio) * threshold_db * -0.666`: the threshold-tracking make-up.
    offset: f32,
    /// Release: fraction of a falling step taken per frame.
    release: f32,
    /// Biquad: b0 (= b2), b1, -a1, -a2.
    b0: f32,
    b1: f32,
    a1: f32,
    a2: f32,
}

impl Coefficients {
    /// `knobs` = [threshold dB, saved ratio, attack s, release s].
    pub fn new([db, ratio, attack, release]: [f64; 4], rate: u32) -> Self {
        let rate = rate as f32;
        // Threshold: powf(2, dB/6) then back through ln in the setter.
        let threshold = 2_f32.powf(db as f32 * f32::from_bits(0x3e2a_aaab));
        let threshold_db = (f64::from(threshold.ln()) * 8.656_170_245_333_781) as f32;
        let remaining = 1. - ratio as f32;
        let offset = remaining * threshold_db * f32::from_bits(0xbf2a_7efa);
        // Release 10..1000 ms, at least one frame.
        let release_ms = (release as f32 * 1000.).clamp(10., 1000.);
        let release = f32::from_bits(0x3f31_7215) / (release_ms * 0.001 * rate).max(1.);
        // Attack 0.1..10 ms sets the detector low-pass: 1000/ms Hz, clamped.
        let attack_ms = (attack as f32 * 1000.).clamp(0.1, 10.);
        let wanted = f32::from_bits(0x4479_ffff) / attack_ms;
        let (low, high) = (
            rate * f32::from_bits(0x382a_aaab),
            rate * f32::from_bits(0x3ef0_f0f1),
        );
        let hertz = if wanted <= high { low.max(wanted) } else { high };
        let (b0, b1, a1, a2) = biquad(hertz * (f32::from_bits(0x40c9_0fe4) / rate));
        Self {
            threshold_db,
            reduction: -remaining,
            offset,
            release,
            b0,
            b1,
            a1,
            a2,
        }
    }
}

/// State: `aux[0]` delay cursor, `aux[1]` ring cursor, `aux[2..4]` the filter
/// inputs (x1, x2; x1 doubles as the release follower), `aux[4..6]` its
/// outputs (y1, y2). `cells[..32]` the delayed audio (both channels), `cells[32..]`
/// the sliding-maximum ring.
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
    let (audio, maxima) = cells.split_at_mut(LATENCY);
    let [mut cursor, mut ring] = [state.aux[0] as usize, state.aux[1] as usize];
    let [mut x1, mut x2, mut y1, mut y2] = [2, 3, 4, 5].map(|i| state.aux[i] as f32);
    for i in 0..len {
        let input = [block[0][i] as f32, block[1][i] as f32];
        // Anti-denormal rotation of the filter memory.
        let (x1_old, x2_old, y1_old, y2_old) = (x1, x2, y1, y2);
        x2 = x1_old + EPS;
        y2 = y1_old + EPS;
        cursor = (cursor + 1) % LATENCY;
        let delayed = audio[cursor].map(|v| v as f32);
        audio[cursor] = input.map(f64::from);
        let now = input[0].abs().max(input[1].abs());
        ring = (ring + 1) % LATENCY;
        let window = maxima.iter().fold(now, |m, cell| m.max(cell[0] as f32));
        maxima[ring][0] = f64::from(now);
        let window = window.max(FLOOR);
        // dB of the window maximum: quadratic mantissa, exponent in 6.02 dB.
        let bits = window.to_bits();
        let mantissa = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000) - 1.;
        let exponent = ((bits & 0x7f80_0000) >> 23) as i32 - 127;
        let level = (LOG_A - mantissa * LOG_B) * mantissa + exponent as f32 * DB_PER_OCTAVE
            - c.threshold_db;
        // 12 dB quadratic soft knee in units of 6 dB.
        let t = (level + KNEE_DB).max(0.) * KNEE_UNIT;
        let k = t.min(2.);
        let mut excess = (t - (k - k * k * 0.25)) * KNEE_DB;
        // The release follower is the filter's previous input.
        let step = excess - x1_old;
        if step <= 0. {
            excess = step * c.release + x1_old;
        }
        let filtered =
            excess * c.b0 + x1_old * c.b1 + y1_old * c.a1 + y2_old * c.a2 + x2_old * c.b0;
        y1 = filtered + EPS;
        x1 = excess + EPS;
        let v = (filtered * c.reduction + c.offset) * EXP_SCALE;
        let n = v.round_ties_even() as i32;
        let fraction = v - n as f32;
        let gain = ((fraction * EXP_A + EXP_B) * fraction + 1.)
            * f32::from_bits(((n.max(-126) + 127) as u32) << 23);
        for ch in 0..2 {
            let wet = if shared.mix == 1. {
                f64::from(gain * delayed[ch]) * shared.makeup
            } else {
                f64::from(delayed[ch]) * shared.factor(f64::from(gain))
            };
            block[ch][i] = wet;
        }
    }
    state.aux[0] = cursor as f64;
    state.aux[1] = ring as f64;
    for (i, v) in [x1, x2, y1, y2].into_iter().enumerate() {
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
        let out = run([0., 0.5, 0.001, 0.1], &input);
        assert!(out[..LATENCY].iter().all(|f| *f == [0., 0.]));
        // Far below the threshold: unity, 32 frames late.
        assert!((out[LATENCY][0] - f64::from(0.01_f32)).abs() < 1e-7, "{:?}", out[LATENCY]);
        assert!((out[LATENCY][1] + f64::from(0.02_f32)).abs() < 1e-7);
    }

    #[test]
    fn steady_loud_input_settles_to_the_static_gain() {
        // 0 dBFS against -20 dB, saved ratio .5: 20 dB over, half removed,
        // plus the threshold-tracking make-up (0.666 * .5 * 20 dB).
        let out = run([-20., 0.5, 0.001, 0.1], &vec![[1., 1.]; 4800]);
        let gain = out[4799][0];
        let want = 2_f64.powf((-10. + 0.5 * 20. * 0.666) * 0.1661);
        assert!((gain - want).abs() < 0.01, "{gain} vs {want}");
        assert!(gain < 1.);
    }

    #[test]
    fn the_gain_leads_a_burst_by_the_window() {
        // A burst after silence: its gain starts falling before the burst
        // leaves the delay, because the window saw it 32 frames earlier.
        let mut input = vec![[0.; 2]; 400];
        input[100..300].fill([1., 1.]);
        let out = run([-20., 0.5, 0.0001, 0.1], &input);
        let (first, settled) = (out[132][0], out[299][0]);
        assert!(first < 1. && first > settled * 0.9, "{first} {settled}");
    }

    #[test]
    fn the_channel_maximum_drives_both_channels() {
        let out = run([-20., 0.5, 0.001, 0.1], &vec![[1., 0.01]; 4800]);
        let ratio = out[4799][1] / out[4799][0];
        assert!((ratio - 0.01).abs() < 1e-6, "one gain for both channels: {ratio}");
    }

    #[test]
    fn detector_low_pass_has_unity_dc_gain_and_clamped_times() {
        let c = Coefficients::new([-20., 0.5, 0.0001, 0.001], 48_000);
        // Release clamps at 10 ms, attack at 0.1 ms.
        let same = Coefficients::new([-20., 0.5, 0.0001, 0.01], 48_000);
        assert_eq!(c.release.to_bits(), same.release.to_bits());
        let dc = (2. * c.b0 + c.b1) / (1. - c.a1 - c.a2);
        assert!((dc - 1.).abs() < 1e-3, "{dc}");
    }
}
