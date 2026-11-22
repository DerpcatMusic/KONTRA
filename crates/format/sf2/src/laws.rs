//! SoundFont units into the IR's (SoundFont 2.01 §8.1.3): timecents,
//! absolute cents, centibels and 0.1 % steps. Pure functions; `translate`
//! calls them.

use std::f32::consts::SQRT_2;

/// dB per centibel of `initialAttenuation`. The spec reads it as 0.1 dB;
/// the E-mu hardware the format was made for, and FluidSynth after it
/// (`fluid_voice.c`, `EMU_ATTENUATION_FACTOR` 0.4 on the generator only,
/// not on modulators), apply 0.4 of that, and banks are voiced by ear on
/// those players. The SF2.01 text stands for every other centibel
/// (sustain, filter Q, LFO volume, the velocity curve).
pub const ATTENUATION_DB_PER_CB: f32 = 0.04;

/// The amplitude envelope's decay and release shape in the IR's
/// `expm1(k·t)/expm1(k)` law: k = ln(10^(-100/20)), a fall linear in dB that
/// covers the format's 100 dB over the stage's time (§8.1.2 gens 36 and 38:
/// "the time for a 100% change", constant dB per time unit).
pub const FALL_SHAPE: f32 = -11.512_925;

/// Absolute cents 0 (MIDI key 0): 440·2^(−69/12) Hz; the spec rounds it to 8.176.
const CENTS_ZERO_HZ: f32 = 8.175_799;

/// Seconds of a timecent value, `2^(tc/1200)`, held in the spec's widest
/// range (−12000 is about a millisecond, 8000 is 101.6 s).
pub fn timecents(tc: i32) -> f32 {
    (tc.clamp(-12_000, 8_000) as f32 / 1200.0).exp2()
}

/// A delay or hold: its default −12000 (and below) is none at all, not a
/// millisecond.
pub fn delay(tc: i32) -> f32 {
    if tc <= -12_000 { 0.0 } else { timecents(tc) }
}

/// Hz of an absolute-cents value: `8.176·2^(c/1200)`.
pub fn hz(cents: i32) -> f32 {
    CENTS_ZERO_HZ * (cents as f32 / 1200.0).exp2()
}

/// `initialFilterFc`, 1500..=13500 cents (20 Hz..=20 kHz).
pub fn cutoff_hz(cents: i32) -> f32 {
    hz(cents.clamp(1_500, 13_500))
}

/// `freqModLFO`/`freqVibLFO`, −16000..=4500 cents (1 mHz..=100 Hz).
pub fn lfo_hz(cents: i32) -> f32 {
    hz(cents.clamp(-16_000, 4_500))
}

/// `initialAttenuation` centibels (0..=1440) as a dB gain.
pub fn attenuation_db(cb: i32) -> f32 {
    -(cb.clamp(0, 1_440) as f32) * ATTENUATION_DB_PER_CB
}

/// `sustainVolEnv`: centibels below full, `10^(−cB/200)`; 1000 and more is silence.
pub fn sustain(cb: i32) -> f32 {
    if cb >= 1_000 {
        0.0
    } else {
        10f32.powf(-(cb.max(0) as f32) / 200.0)
    }
}

/// `sustainModEnv`: 0.1 % steps below full (1000 is zero), linear.
pub fn mod_sustain(permille: i32) -> f32 {
    1.0 - permille.clamp(0, 1_000) as f32 / 1000.0
}

/// `pan`, −500..=500 (0.1 % steps, −500 full left) as −1..=1.
pub fn pan(value: i32) -> f32 {
    value.clamp(-500, 500) as f32 / 500.0
}

/// Damping `k = 1/Q` of `initialFilterQ`, the resonance peak's height in
/// centibels (0..=960). FluidSynth's law (`fluid_voice.c`: `cB/10 − 3.01`
/// dB): `Q = 10^(cB/200)/√2`, so 0 cB is Butterworth, with no peak.
pub fn damping(cb: i32) -> f32 {
    SQRT_2 / 10f32.powf(cb.clamp(0, 960) as f32 / 200.0)
}

pub fn semitones(cents: i32) -> f32 {
    cents as f32 / 100.0
}

/// `modLfoToVolume`: dB a full positive LFO swing adds (centibels / 10).
pub fn swing_db(cb: i32) -> f32 {
    cb as f32 / 10.0
}
