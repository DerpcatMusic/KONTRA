//! Kontakt `color` effects onto `sampler_ir::fx::color::Config`.
//!
//! Each admitted native module is a preset of the one color superset. A module
//! whose law is not proved is not mapped; [`unproved`] says exactly what is
//! missing, and the translator reports it next to the generic "not modeled".
use sampler_ir::fx::color::Config;

/// 0x1e Distortion (reader 0x140d00c60): mode 0 Tube, 1 Transistor; Drive and
/// Damping are direct normalized fields (probe-measured identity binding).
pub(super) fn distortion(mode: f32, drive: f32, damping: f32) -> Option<Config> {
    (matches!(mode, 0.0 | 1.0) && [drive, damping].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
        .then(|| Config::distortion(mode == 1.0, drive, damping))
}

/// 0x1d v0x80 Saturator / Shaper (reader 0x140d02c30): signed Shape -1..1
/// (probe: `linear -1 + 2v`) and algorithm 0 Classic, 1 Enhanced, 2 Drums
/// (audio 0x140aa4d20 dispatching to 0x140ab27f0 / 0x140ab3310).
pub(super) fn saturation(shape: f32, algorithm: f32) -> Option<Config> {
    (shape.is_finite() && (-1.0..=1.0).contains(&shape)).then_some(())?;
    match algorithm {
        0.0 => Some(Config::classic(shape)),
        1.0 => Some(Config::enhanced(shape)),
        2.0 => Some(Config::drums(shape)),
        _ => None,
    }
}

/// 0x20 Lo-Fi (reader 0x140d01f20, `FFBFF`): Bits, Frequency, flag, Noise
/// level, Noise color; probe-measured field order (BITS 0, FREQUENCY 1,
/// NOISELEVEL 3, NOISECOLOR 4).
pub(super) fn lofi(values: &[f32; 4]) -> Option<Config> {
    values
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        .then(|| Config::lofi(values[0], values[1], values[2], values[3]))
}

/// 0x13 Gainer (reader 0x140d019b0): `dry + out * g`, g slewing.
pub(super) fn gainer(gain: f64, dry: f64) -> Config {
    Config::gainer(sampler_ir::Gain::Linear(gain), dry)
}

/// 0x1f Stereo Modeller (reader 0x140d02d10): signed spread, signed pan, pseudo.
pub(super) fn stereo_modeller(spread: f32, pan: f32, pseudo: bool) -> Config {
    Config::stereo(
        (f64::from(spread).clamp(-1.0, 1.0) + 1.0) * 0.5,
        f64::from(pan).clamp(-1.0, 1.0),
        pseudo,
    )
}

/// Why a drive, amp or character module is not mapped: what is proved and what
/// is missing. `None` for modules this family does not own or that are mapped.
pub(super) fn unproved(module: u16) -> Option<&'static str> {
    Some(match module {
        0x1d => "Surround Panner (version 0x70): variable speaker lists and an embedded resampling-style panner (0x140ab0070, 0x140ab4eb0); speaker matrix and pan law not extracted",
        0x21 => "Skreamer: stateful four-lane core 0x140aaf220 with table lookups; saved-field to coefficient binding and the table contents are not extracted",
        0x22 => "Rotator: summed-input core 0x140aae920 with four-state recurrences and ring buffers; rotation, acceleration and distance laws not extracted",
        0x42 => "Tape Saturator: NI::KONTAKTFX::Tape core 0x140ae1190 with table interpolation; only the indexed setters 0x140ae1e10 are extracted, the saved-field binding and tables are not",
        0x23 | 0x24 | 0x4d | 0x52 | 0x53 | 0x54 | 0x55 | 0x56 | 0x58 | 0x61 | 0x6c | 0x6e | 0x6f | 0x76 | 0x77 | 0x78 | 0x79 => {
            "amp, cabinet or pedal model: cores and tables are not extracted, so no topology is proved"
        }
        0x66 | 0x67 | 0x68 => "creative drive module: core not extracted",
        _ => return None,
    })
}
