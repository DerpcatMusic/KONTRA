//! Choral (Kontakt module `0x5d`): a seven-voice modulated delay bank.
//!
//! The bank is Kontakt's, recovered from the 8.13.1 initialiser and read back
//! from the emulator (`docs/re/engine/k-choral-modes.md`): seven base delays,
//! seven gains and seven pans per mode, with mode 4 the same as mode 3.
//!
//! Measured laws (emulator, 2026-10-30):
//! - a voice's delay is `base × (delay · 5 + 0.1)` seconds. Checked against the
//!   first tap at delay 0.5 (374 frames predicted 374) and 1.0 (734 vs 734).
//! - the LFO is a triangle, and its rate is `0.25 + 7.4 · rate³` Hz
//!   ([`lfo_hz`], seven rates checked exactly).
//! - its per-voice phase offset is the mode table's `pan` field
//!   ([`Voice::pan`]), read as the offset in `voice+(-0x158)`.
//! - `amount` sets the modulation depth in samples as
//!   `(fs / hz) · ½ · (2^(0.025 · m) − 1)`, `m = 0.99 · amount² + 0.01`
//!   ([`modulation_depth`]); measured 3.54 / 25.46 / 91.35 / 201.63 / 357.03
//!   samples at amount 0 / .25 / .5 / .75 / 1, rate 0.5, 48 kHz.
//!
//! Not yet read, so [`Chorus::process`] stays unimplemented: the per-voice
//! smoothing and filter networks behind `voice+(-0x1f8)`, `+(-0x200)` and
//! `+(-0x1e0)` (whose source field is not identified), the pan rotation
//! matrices at `core+0x8a0`, the per-frame use of the seven-tap mix at
//! `core+0x7c0`, and the global tone filter at `core+0x1c20`. Those are needed
//! before this can claim to match the native vectors `tools/re/kemu_timemod.py
//! choral` renders.

/// One voice of a mode's bank.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voice {
    /// Base delay in seconds, before the `delay` parameter scales it.
    pub base_s: f32,
    pub gain: f32,
    /// 0 left, 1 right; 0.0 is a voice the mode leaves centred.
    pub pan: f32,
}

/// A mode's seven voices and its loop-filter constants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
    pub voices: [Voice; 7],
    pub filter: [f32; 6],
}

const fn mode(delays: [f32; 7], gains: [f32; 7], pans: [f32; 7], filter: [f32; 6]) -> Mode {
    let mut voices = [Voice {
        base_s: 0.0,
        gain: 1.0,
        pan: 0.0,
    }; 7];
    let mut i = 0;
    while i < 7 {
        voices[i] = Voice {
            base_s: delays[i],
            gain: gains[i],
            pan: pans[i],
        };
        i += 1;
    }
    Mode { voices, filter }
}

/// Kontakt 8.13.1's four banks, read from `[core+0x6c8]` after the mode setter.
pub const MODES: [Mode; 4] = [
    mode(
        [0.003, 0.003, 0.007, 0.007, 0.011, 0.011, 0.013],
        [1.0, 1.0, 1.42857, 1.43, 1.7242, 1.72765, 0.7423],
        [0.0, 0.5, 0.0, 0.5, 0.0, 0.5, 0.0],
        [6.1, 6.1, 6.1, 6.1, 0.1, 0.1],
    ),
    mode(
        [0.007, 0.007, 0.007, 0.007, 0.013, 0.013, 0.029],
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.7423],
        [0.0, 0.5, 0.33333, 0.83333, 0.66666, 0.16666, 0.0],
        [4.1, 4.1, 4.0, 4.0, 4.12, 4.1241],
    ),
    mode(
        [0.011, 0.011, 0.011, 0.011, 0.011, 0.011, 0.053],
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.7423],
        [0.0, 0.5, 0.5, 0.0, 0.25, 0.75, 0.0],
        [6.1, 6.1, 6.1, 6.1, 6.1, 6.1],
    ),
    mode(
        [0.007, 0.007, 0.011, 0.011, 0.013, 0.013, 0.029],
        [1.0, 1.0, 1.42857, 1.42857, 1.7242, 1.7242, 0.7423],
        [0.0, 0.0, 0.0, 0.25, 0.0, 0.25, 0.0],
        [6.1, 6.1, 6.1, 6.1, 6.1, 6.1],
    ),
];

/// The mode a `mode` parameter picks: `(int)(p × 4)` clamped to 0..=3, and
/// mode 4 is mode 3 (the initialiser's switch falls through to it).
#[must_use]
pub fn mode_at(p: f32) -> usize {
    let k = (p * 4.0) as i32;
    match k.clamp(0, 4) {
        4 => 3,
        n => n as usize,
    }
}

/// A voice's delay in seconds at `delay` 0..=1: `base × (delay · 5 + 0.1)`.
#[must_use]
pub fn voice_delay_s(base_s: f32, delay: f32) -> f32 {
    base_s * (delay * 5.0 + 0.1)
}

/// The LFO rate in Hz: `0.25 + 7.4 · rate³`.
///
/// Measured from `voice+(-0x220)` after prepare, 48 kHz: rate 0, 0.05, 0.1,
/// 0.25, 0.5, 0.75 and 1 give 0.25, 0.250925, 0.2574, 0.365625, 1.175,
/// 3.371875 and 7.65 Hz, which is the cubic exactly (7.4 · 0.05³ = 0.000925).
#[must_use]
pub fn lfo_hz(rate: f32) -> f32 {
    0.25 + 7.4 * rate * rate * rate
}

/// The LFO's waveform at `phase` in 0..1: a triangle, `4φ − 1` on the rising
/// half and `3 − 4φ` on the falling half.
///
/// Measured at 48 kHz, rate 0.5, amount 0.6: the value at frame `n` is
/// `4 · n · (hz / fs) − 1` to the last bit up to the peak at frame 20429
/// (`hz / fs` = 2.4479e-5, half a period), then the mirror.
#[must_use]
pub fn lfo_triangle(phase: f32) -> f32 {
    let p = phase.rem_euclid(1.0);
    if p < 0.5 {
        4.0 * p - 1.0
    } else {
        3.0 - 4.0 * p
    }
}

/// The peak-to-peak modulation in samples at `amount`, for an LFO at `hz` and
/// `fs`: `(fs / hz) · ½ · (2^(0.025 · m) − 1)`, `m = 0.99 · amount² + 0.01`.
///
/// Read out of the kernel: `0x14193a2f0` (amount setter) stores
/// `m = 0.99 · amount² + 0.01` at `voice+(-0x218)` and
/// `1 / (2^(0.025 m) − 0.9999)` at `voice+0x58`, and `0x14193dc00` stores
/// `(fs / hz) · ½ · (2^(0.025 m) − 1)` at `voice+(-0x210)`, which `0x14193be50`
/// adds to the base delay. Measured: 3.5398, 25.4558, 91.3451, 201.6327 and
/// 357.0320 samples at amount 0, 0.25, 0.5, 0.75, 1 (rate 0.5, 48 kHz), and
/// 611.4650 at amount 0.6, rate 0 (0.25 Hz).
#[must_use]
pub fn modulation_depth(amount: f32, hz: f32, fs: f32) -> f32 {
    // The setter squares in f32 and does the rest in f64.
    let m = 0.99 * f64::from(amount * amount) + 0.01;
    let p = 2.0f64.powf(0.025 * m);
    ((f64::from(fs) / f64::from(hz)) * 0.5 * (p - 1.0)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measured taps: mode 0's first voice at delay 0.5 landed on frame 374
    /// and at delay 1.0 on frame 734 (48 kHz, emulator 2026-10-30).
    #[test]
    fn a_voice_delay_is_the_base_scaled_by_the_delay_knob() {
        let base = MODES[0].voices[0].base_s;
        assert_eq!(voice_delay_s(base, 0.5) * 48_000.0, 374.4);
        assert_eq!(voice_delay_s(base, 1.0) * 48_000.0, 734.4);
    }

    #[test]
    fn the_mode_parameter_maps_four_ways_and_four_is_three() {
        assert_eq!(mode_at(0.0), 0);
        assert_eq!(mode_at(0.3), 1);
        assert_eq!(mode_at(0.55), 2);
        assert_eq!(mode_at(0.8), 3);
        assert_eq!(mode_at(1.0), 3);
    }

    #[test]
    fn every_bank_has_seven_voices_and_a_centred_voice_is_pan_zero() {
        for (n, m) in MODES.iter().enumerate() {
            assert_eq!(m.voices.len(), 7, "mode {n}");
            assert!(m.voices.iter().any(|v| v.pan == 0.0), "mode {n}");
        }
    }

    /// The seven rates read back from `voice+(-0x220)` after prepare.
    #[test]
    fn the_lfo_rate_is_cubic_in_the_rate_knob() {
        let measured = [
            (0.0, 0.25),
            (0.05, 0.250925),
            (0.1, 0.2574),
            (0.25, 0.365625),
            (0.5, 1.175),
            (0.75, 3.371875),
            (1.0, 7.65),
        ];
        for (rate, hz) in measured {
            assert!(
                (lfo_hz(rate) - hz).abs() < 1e-6,
                "rate {rate}: {} vs {hz}",
                lfo_hz(rate)
            );
        }
    }

    /// The triangle: -1 at phase 0, +1 at the half period, and the value the
    /// kernel produced on the rising half (frame 2000 at rate 0.5 is -0.804167).
    #[test]
    fn the_lfo_is_a_triangle() {
        assert_eq!(lfo_triangle(0.0), -1.0);
        assert!((lfo_triangle(0.5) - 1.0).abs() < 1e-6);
        assert!(lfo_triangle(0.25).abs() < 1e-6);
        assert!((lfo_triangle(0.75) + 0.0).abs() < 1e-6);
        let phase = 2000.0 * lfo_hz(0.5) / 48_000.0;
        assert!((lfo_triangle(phase) - -0.8041667).abs() < 1e-6);
        assert_eq!(lfo_triangle(1.0), -1.0);
    }

    /// The five depths read out of `voice+(-0x210)` at rate 0.5, 48 kHz.
    #[test]
    fn the_modulation_depth_is_the_kernels_law() {
        let hz = lfo_hz(0.5);
        let measured = [3.5398, 25.4558, 91.3451, 201.6327, 357.0320];
        for (n, amount) in [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().enumerate() {
            let depth = modulation_depth(amount, hz, 48_000.0);
            assert!(
                (depth - measured[n]).abs() < 1e-3,
                "amount {amount}: {depth} vs {}",
                measured[n]
            );
        }
        // rate 0 is 0.25 Hz, amount 0.6.
        assert!((modulation_depth(0.6, lfo_hz(0.0), 48_000.0) - 611.4650).abs() < 1e-3);
    }
}
