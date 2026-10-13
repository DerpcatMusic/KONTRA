//! Kontakt `dynamics` effects onto sampler_ir::fx::dynamics.
//!
//! Compressor 0x19 (Classic / Enhanced / Pro): each mode is a native law of the
//! one `Config` superset, saved knobs passed through (dispatcher
//! `0x1409047a0`; cores `0x140ab2250`, `0x140ab2c80`, `0x140ab35d0`).

use sampler_ir::fx::dynamics::{Config, Law, Link, Native};
use sampler_ir::{Gain, Processor, Reason, Time};

/// One note for the diagnostics: feature, detail, reason.
pub(super) type Note = (String, String, Reason);

/// `BParFXCompressor`: integer mode, threshold dB, saved ratio (0..=1), attack
/// and release ms, stereo-link byte.
pub(super) fn compressor(
    mode: f32,
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    link: bool,
) -> Result<Processor, Note> {
    let native = Native { threshold_db: f64::from(threshold_db), ratio: f64::from(ratio) };
    let law = match mode {
        0.0 => Law::KontaktClassic(native),
        1.0 => Law::KontaktEnhanced(native),
        2.0 => Law::KontaktPro(native),
        _ => return Err(("compressor mode".into(), format!("{mode}"), Reason::NotModeled)),
    };
    // Enhanced takes ln(attack): zero has no value. Every core takes ratio 0..=1.
    let knobs = [threshold_db, ratio, attack_ms, release_ms];
    if !knobs.iter().all(|v| v.is_finite())
        || !(0.0..=1.0).contains(&ratio)
        || attack_ms < 0.0
        || release_ms < 0.0
        || (mode == 1.0 && attack_ms <= 0.0)
    {
        return Err((
            "compressor parameters".into(),
            format!("mode {mode} {knobs:?}"),
            Reason::InvalidValue,
        ));
    }
    Ok(Processor::Dynamics(Config {
        law,
        attack: Time::Milliseconds(f64::from(attack_ms)),
        release: Time::Milliseconds(f64::from(release_ms)),
        // Classic and Enhanced detect on the signed channel mean when linked;
        // Pro always detects on the channel maximum and ignores the byte.
        link: match (mode == 2.0, link) {
            (true, _) => Link::Max,
            (false, true) => Link::Mean,
            (false, false) => Link::Off,
        },
        makeup: Gain::UNITY,
        mix: 1.0,
    }))
}
