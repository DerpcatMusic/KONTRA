//! Choral (Kontakt module `0x5d`): a seven-voice modulated delay bank.
//!
//! The bank is Kontakt's, recovered from the 8.13.1 initialiser and read back
//! from the emulator (`docs/re/engine/k-choral-modes.md`): seven base delays,
//! seven gains and seven pans per mode, with mode 4 the same as mode 3.
//!
//! Measured laws (emulator, 2026-10-30):
//! - a voice's delay is `base × (delay · 5 + 0.1)` seconds. Checked against the
//!   first tap at delay 0.5 (374 frames predicted 374) and 1.0 (734 vs 734).
//!
//! Not yet established, and deliberately not invented here: how `amount`
//! scales the LFO's swing, the LFO's shape and its per-voice phase, and what
//! the six loop-filter constants after the pans mean. Those are needed before
//! this can claim to match the native vectors `tools/re/kemu_timemod.py choral`
//! renders, so [`Chorus::process`] is not wired into the engine yet.

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
}
