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

/// `BParFXLimiter` (0x1c): saved input gain (`2^(x/6)`) and release ms, the
/// half-life of the level follower. Process `0x140904890`, setter `0x140960140`.
pub(super) fn limiter(input_gain: f32, release_ms: f32) -> Result<Processor, Note> {
    if !input_gain.is_finite() || !release_ms.is_finite() || release_ms < 0.0 || input_gain.abs() > 600.0 {
        return Err((
            "limiter parameters".into(),
            format!("{input_gain} {release_ms}"),
            Reason::InvalidValue,
        ));
    }
    Ok(Processor::Dynamics(Config {
        law: Law::KontaktLimiter(Native { threshold_db: f64::from(input_gain), ratio: 0.0 }),
        attack: Time::Milliseconds(0.0),
        release: Time::Milliseconds(f64::from(release_ms)),
        link: Link::Max,
        makeup: Gain::UNITY,
        mix: 1.0,
    }))
}

/// `BParFXHilbertLimiter` (0x5f, "Transparent Limiter"): normalized drive,
/// release (s) and ceiling, then the side-chain byte; they feed controls 0..3
/// of `BEffectHilbertLimiter` (`0x1409aa9e0` clamps each to 0..=1 before the
/// core setter `0x141aea680`). The saved-order to control-id map follows the
/// four-control layout (three floats, then the side-chain bool as id 3); it
/// is inferred, not traced.
pub(super) fn hilbert(drive: f32, release: f32, ceiling: f32, sidechain: bool) -> Result<Processor, Note> {
    if ![drive, release, ceiling].iter().all(|v| v.is_finite()) {
        return Err((
            "transparent limiter parameters".into(),
            format!("{drive} {release} {ceiling}"),
            Reason::InvalidValue,
        ));
    }
    if sidechain {
        return Err(("transparent limiter external side-chain".into(), String::new(), Reason::NotModeled));
    }
    let unit = |v: f32| f64::from(v.clamp(0.0, 1.0));
    Ok(Processor::Dynamics(Config {
        law: Law::KontaktHilbert(Native { threshold_db: unit(drive), ratio: unit(ceiling) }),
        attack: Time::Milliseconds(0.0),
        release: Time::Seconds(unit(release)),
        link: Link::Max,
        makeup: Gain::UNITY,
        mix: 1.0,
    }))
}

/// `BParFXFBComp` v0x10 (0x4c): seven normalized floats (input, output,
/// attack, release, ratio, mix, high quality) and three bytes (side-chain,
/// link, side-chain routing), feeding controls 0..9 of `BEffectFBComp`
/// (`0x1409a9980`, no clamp) and the core setter `0x141ac56d0`. Saved order
/// to control id is inferred from the layout, not traced.
pub(super) fn feedback(v: [f32; 7], [sidechain, link, routed]: [bool; 3]) -> Result<Processor, Note> {
    if !v.iter().all(|x| x.is_finite() && (-1.0..=2.0).contains(x)) {
        return Err(("feedback compressor parameters".into(), format!("{v:?}"), Reason::InvalidValue));
    }
    if sidechain || routed {
        return Err(("feedback compressor external side-chain".into(), String::new(), Reason::NotModeled));
    }
    let [input, output, attack, release, ratio, mix, quality] = v;
    let attack = attack * f32::from_bits(0x3d4c_6061) + f32::from_bits(0x38d8_d874);
    let release = release * f32::from_bits(0x3f93_3334) + 0.05;
    Ok(Processor::Dynamics(Config {
        law: Law::KontaktFeedback {
            native: Native {
                threshold_db: f64::from(10f32.powf(input * 2.5 - 0.5)),
                ratio: f64::from(ratio * 19.0 + 1.0),
            },
            high_quality: quality > 0.5,
        },
        attack: Time::Seconds(f64::from(attack)),
        release: Time::Seconds(f64::from(release)),
        link: if link { Link::Max } else { Link::Off },
        makeup: Gain::Linear(f64::from(10f32.powf(output * 2.5 - 2.0))),
        mix: f64::from(mix.clamp(0.0, 1.0)),
    }))
}

/// `BParFXTrans` v0x10 (0x43): normalized attack, sustain and output gain
/// (0.5 neutral) and the clip switch, feeding controls 0..3 of `BEffectTrans`
/// (setter `0x141ac87b0`). Saved order to control id is inferred from the
/// layout, not traced; controls 4..7 have no saved field and stay off.
pub(super) fn transient(attack: f32, sustain: f32, gain: f32, clip: i32) -> Result<Processor, Note> {
    if ![attack, sustain, gain].iter().all(|v| (0.0..=1.0).contains(v)) || !(0..=1).contains(&clip) {
        return Err((
            "transient master parameters".into(),
            format!("{attack} {sustain} {gain} {clip}"),
            Reason::InvalidValue,
        ));
    }
    Ok(Processor::Dynamics(Config {
        law: Law::KontaktTransient {
            attack: f64::from(attack),
            sustain: f64::from(sustain),
            gain: f64::from(gain),
            clip: clip == 1,
        },
        attack: Time::Milliseconds(0.0),
        release: Time::Milliseconds(0.0),
        link: Link::Max,
        makeup: Gain::UNITY,
        mix: 1.0,
    }))
}

/// `BParFXSSLGBusComp` (0x46, reader `0x140d02850`): six floats, the link
/// byte, (v0x11+) the side-chain byte (`0x140d19c50`), (v0x12) a trailing
/// float. Floats feed core controls 0..=4 (threshold, makeup, attack, release,
/// ratio) and 7 (mix), bytes 5 (link) and 6 (side-chain) of `BEffectSSLGBusComp`
/// (setter `0x1409a9d40`, no clamp). That order is inferred from the core ids,
/// not traced; the trailing float has no traced binding and is ignored.
pub(super) fn bus_comp(v: [f32; 6], [link, sidechain]: [bool; 2], _tail: Option<f32>) -> Result<Processor, Note> {
    if !v.iter().all(|x| (0.0..=1.0).contains(x)) {
        return Err(("solid bus comp parameters".into(), format!("{v:?}"), Reason::InvalidValue));
    }
    if sidechain {
        return Err(("solid bus comp external side-chain".into(), String::new(), Reason::NotModeled));
    }
    let [threshold, makeup, attack, release, ratio, mix] = v.map(f64::from);
    Ok(Processor::Dynamics(Config {
        law: Law::KontaktBusComp { threshold, makeup, attack, release, ratio, link },
        attack: Time::Milliseconds(0.0),
        release: Time::Milliseconds(0.0),
        link: if link { Link::Max } else { Link::Off },
        makeup: Gain::UNITY,
        mix,
    }))
}

#[cfg(test)]
mod limiter_tests {
    use super::*;

    #[test]
    fn limiter_maps_to_the_native_law_and_rejects_bad_values() {
        let Ok(Processor::Dynamics(c)) = limiter(6.0, 100.0) else { panic!() };
        assert!(matches!(c.law, Law::KontaktLimiter(_)));
        assert!(limiter(f32::NAN, 100.0).is_err());
        assert!(limiter(0.0, -1.0).is_err());
        let Ok(Processor::Dynamics(h)) = hilbert(2.0, 0.5, 0.9, false) else { panic!() };
        assert!(matches!(h.law, Law::KontaktHilbert(Native { threshold_db: 1.0, .. })));
        assert!(hilbert(0.5, 0.5, 0.5, true).is_err());
        let Ok(Processor::Dynamics(f)) = feedback([0.2, 0.8, 0.0, 0.0, 1.0, 1.0, 1.0], [false, true, false]) else {
            panic!()
        };
        assert!(matches!(f.law, Law::KontaktFeedback { native: Native { ratio: 20.0, .. }, high_quality: true }));
        assert!(feedback([0.5; 7], [true, false, false]).is_err());
        let Ok(Processor::Dynamics(t)) = transient(1.0, 0.5, 0.5, 1) else { panic!() };
        assert!(matches!(t.law, Law::KontaktTransient { attack: 1.0, clip: true, .. }));
        assert!(transient(0.5, 0.5, 0.5, 2).is_err());
        let Ok(Processor::Dynamics(b)) = bus_comp([0.5, 0.2, 0.5, 1.0, 1.0, 0.7], [true, false], Some(0.6)) else {
            panic!()
        };
        assert!(matches!(b.law, Law::KontaktBusComp { release: 1.0, link: true, .. }) && b.mix == f64::from(0.7f32));
        assert!(bus_comp([0.5; 6], [true, true], None).is_err());
    }
}
