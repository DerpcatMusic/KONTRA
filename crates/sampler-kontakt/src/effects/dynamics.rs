//! Kontakt `dynamics` effects onto sampler_ir::fx::dynamics.
//!
//! Compressor 0x19 (Classic / Enhanced / Pro): each mode is a native law of the
//! one `Config` superset, saved knobs passed through (dispatcher
//! `0x1409047a0`; cores `0x140ab2250`, `0x140ab2c80`, `0x140ab35d0`).

use super::{Lowering, Notes, Site};
use sampler_ir::fx::dynamics::{Config, Key, Law, Link, Native};
use sampler_ir::{Gain, Processor, Reason, Time};

/// One note for the diagnostics: feature, detail, reason.
pub(super) type Note = (String, String, Reason);

/// Why a dynamics module has no processor yet, for the diagnostic note. Only
/// modules whose saved fields are read but whose law is unproven belong here.
pub(super) fn unproved(module: u16) -> Option<&'static str> {
    Some(match module {
        0x60 => {
            "Supercharger GT: saved fields read, but no verified law (variant identity, \
             transfer curve, detector and timing not extracted; see docs/re/engine/dyn-compressors.md)"
        }
        _ => return None,
    })
}

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
        address: None,
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
        address: None,
        law: Law::KontaktLimiter(Native { threshold_db: f64::from(input_gain), ratio: 0.0 }),
        attack: Time::Milliseconds(0.0),
        release: Time::Milliseconds(f64::from(release_ms)),
        link: Link::Max,
        makeup: Gain::UNITY,
        mix: 1.0,
    }))
}

/// `BParFXHilbertLimiter` (0x5f, "Transparent Limiter", version 0x51): floats
/// at saved +8, +0xc, +0x10 (normalized drive, release s, ceiling), then the
/// side-chain byte at +0x14. Decompile: reader `0x140d01ae0`; controls 0..2
/// are those floats in order (setter `0x141aea680`), control 3 is the
/// side-chain flag (`0x1409aa9e0` clamps 0..=1 and stores it). The side-chain
/// selects `BEffectHilbertLimiterSC`, whose key input the core now supports
/// (`Key::External`); no slot routes one yet, so the flag stays diagnosed.
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
        address: None,
        law: Law::KontaktHilbert(Native { threshold_db: unit(drive), ratio: unit(ceiling) }, Key::Internal),
        attack: Time::Milliseconds(0.0),
        release: Time::Seconds(unit(release)),
        link: Link::Max,
        makeup: Gain::UNITY,
        mix: 1.0,
    }))
}

/// `BParFXFBComp` v0x10 (0x4c): seven floats then three bytes, feeding
/// `BEffectFBComp` (`0x1409a9980`, no clamp) and the core setter `0x141ac56d0`.
/// Measured bindings (kontakt-bindings/fx_dynamics_dist_tape.tsv, KE150):
/// fields 0..5 input, makeup, attack, release, ratio, mix (identity), field 7
/// (first byte) high quality. Field 6 (float) and fields 8, 9 (bytes) have no
/// measured binding: link and side-chain are inferred from the layout.
pub(super) fn feedback(v: [f32; 7], [quality, link, sidechain]: [bool; 3]) -> Result<Processor, Note> {
    if !v.iter().all(|x| x.is_finite() && (-1.0..=2.0).contains(x)) {
        return Err(("feedback compressor parameters".into(), format!("{v:?}"), Reason::InvalidValue));
    }
    if sidechain {
        return Err(("feedback compressor external side-chain".into(), String::new(), Reason::NotModeled));
    }
    let [input, output, attack, release, ratio, mix, _] = v;
    let attack = attack * f32::from_bits(0x3d4c_6061) + f32::from_bits(0x38d8_d874);
    let release = release * f32::from_bits(0x3f93_3334) + 0.05;
    Ok(Processor::Dynamics(Config {
        address: None,
        law: Law::KontaktFeedback {
            native: Native {
                threshold_db: f64::from(10f32.powf(input * 2.5 - 0.5)),
                ratio: f64::from(ratio * 19.0 + 1.0),
            },
            high_quality: quality,
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
        address: None,
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
/// not traced. The v0x12 trailing float has no traced binding: the processor
/// still runs, and the note reports the field as unmodelled.
pub(super) fn bus_comp(
    v: [f32; 6],
    [link, sidechain]: [bool; 2],
    tail: Option<f32>,
) -> Result<(Processor, Option<Note>), Note> {
    if !v.iter().all(|x| (0.0..=1.0).contains(x)) {
        return Err(("solid bus comp parameters".into(), format!("{v:?}"), Reason::InvalidValue));
    }
    if sidechain {
        return Err(("solid bus comp external side-chain".into(), String::new(), Reason::NotModeled));
    }
    let [threshold, makeup, attack, release, ratio, mix] = v.map(f64::from);
    let processor = Processor::Dynamics(Config {
        address: None,
        law: Law::KontaktBusComp { threshold, makeup, attack, release, ratio, link },
        attack: Time::Milliseconds(0.0),
        release: Time::Milliseconds(0.0),
        link: if link { Link::Max } else { Link::Off },
        makeup: Gain::UNITY,
        mix,
    });
    let note = tail.map(|t| ("solid bus comp trailing field".into(), format!("{t}"), Reason::NotModeled));
    Ok((processor, note))
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
        assert!(matches!(h.law, Law::KontaktHilbert(Native { threshold_db: 1.0, .. }, Key::Internal)));
        assert!(hilbert(0.5, 0.5, 0.5, true).is_err());
        let Ok(Processor::Dynamics(f)) = feedback([0.2, 0.8, 0.0, 0.0, 1.0, 1.0, 0.0], [true, true, false]) else {
            panic!()
        };
        assert!(matches!(f.law, Law::KontaktFeedback { native: Native { ratio: 20.0, .. }, high_quality: true }));
        assert!(feedback([0.5; 7], [false, false, true]).is_err());
        let Ok(Processor::Dynamics(t)) = transient(1.0, 0.5, 0.5, 1) else { panic!() };
        assert!(matches!(t.law, Law::KontaktTransient { attack: 1.0, clip: true, .. }));
        assert!(transient(0.5, 0.5, 0.5, 2).is_err());
        let Ok((Processor::Dynamics(b), Some(_))) = bus_comp([0.5, 0.2, 0.5, 1.0, 1.0, 0.7], [true, false], Some(0.6))
        else {
            panic!()
        };
        assert!(matches!(b.law, Law::KontaktBusComp { release: 1.0, link: true, .. }) && b.mix == f64::from(0.7f32));
        assert!(bus_comp([0.5; 6], [true, true], None).is_err());
    }
}

/// The native dynamics cores (Limiter 0x1c and the other parse-time laws).
pub(super) fn slot_dynamics(b: &mut Lowering, s: &Site, notes: &mut Notes, result: &Result<Processor, Note>) -> bool {
    b.flush();
    match result.clone() {
        Ok(processor) => {
            b.out.processors.push(processor);
            b.combined = s.gain;
            true
        }
        Err(note) => {
            notes.push(note);
            false
        }
    }
}

/// `BParFXCompressor` (0x19): the mode selects the native law.
#[allow(clippy::too_many_arguments)]
pub(super) fn slot_compressor(
    b: &mut Lowering,
    s: &Site,
    notes: &mut Notes,
    mode: f32,
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    link: bool,
) -> bool {
    b.flush();
    match compressor(mode, threshold_db, ratio, attack_ms, release_ms, link) {
        Ok(processor) => {
            b.out.processors.push(processor);
            b.combined = s.gain;
            true
        }
        Err(note) => {
            notes.push(note);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::effects::*;
    use crate::effects::test_support::*;

    #[test]
    fn compressor_modes_without_a_core_are_not_modeled() {
        for mode in [3i32, -1] {
            let mut bytes = mode.to_le_bytes().to_vec();
            for v in [-12f32, 0.5, 10., 100.] {
                bytes.extend(v.to_le_bytes());
            }
            bytes.push(0);
            let mut fx = slot(0x19, bytes, 1.);
            fx.version = 0x70;
            let built = crate::effects::chain(&[fx], crate::effects::Scope::Voice);
            assert!(
                built.processors.is_empty(),
                "mode {mode} has no core"
            );
            assert!(built.notes.iter().any(|(_, feature, _, reason)| {
                feature.ends_with("compressor mode") && *reason == sampler_ir::Reason::NotModeled
            }));
        }
    }

    #[test]
    fn enhanced_compressor_uses_native_detector_and_integer_selector() {
        let mut payload = 1i32.to_le_bytes().to_vec();
        for value in [-6.0f32, 0.5, 1.0, 100.0] { payload.extend(value.to_le_bytes()); }
        payload.push(0);
        let mut fx = slot(0x19, payload.clone(), 1.0); fx.version = 0x70;
        let built = chain(&[fx], Scope::Voice);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
        let output = fx_probe(built.processors, 120., &[[1., -0.5]; 4]);
        for (actual, expected) in output.iter().zip([0.999157310f32, 0.997557640, 0.995283782, 0.992414534]) { assert!((actual[0] - expected).abs() < 2e-7, "{actual:?} vs {expected}"); }
        assert_eq!(output[0][1], -0.5);
        *payload.last_mut().unwrap() = 1;
        let mut fx = slot(0x19, payload, 1.0); fx.version = 0x70;
        let linked = chain(&[fx], Scope::Voice);
        assert!(linked.notes.is_empty(), "{:?}", linked.notes);
        assert_eq!(fx_probe(linked.processors, 120., &[[1., -1.]])[0], [1., -1.]);
    }

    #[test]
    fn compressor_output_and_bypass_init_writes_use_the_shared_slot_law() {
        // Pinned v1 0cb7a8a0, engine/params.rs: effect_gain = 16*x^3.
        // The shared service's rounded unity differs by less than 0.00005 dB.
        for value in [0, 125_919, 396_851, 560_434, 1_000_000] {
            let native = 16.0 * (f64::from(value) / 1_000_000.0).powi(3);
            let gain = f64::from(engine_gain(value));
            if native > 0.0 {
                assert!((20.0 * (gain / native).log10()).abs() < 0.00005);
            } else {
                assert_eq!(gain, 0.0);
            }
        }
        assert_eq!(engine_gain(-1), 0.0);
        assert_eq!(engine_gain(i32::MAX), engine_gain(1_000_000));
        let mut fx = slot(0x19, Vec::new(), 1.0);
        fx.slot = 1;
        let mut writes = vec![
            sampler_ksp::EnginePar {
                parameter: "$ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN".into(),
                value: 560_434,
                group: -1,
                slot: 1,
                generic: 1,
            },
            sampler_ksp::EnginePar {
                parameter: "$ENGINE_PAR_EFFECT_BYPASS".into(),
                value: 1,
                group: -1,
                slot: 1,
                generic: 1,
            },
        ];
        apply_writes(std::slice::from_mut(&mut fx), &writes, -1, 1);
        assert!(fx.bypass && fx.output_set);
        assert!((20.0 * f64::from(fx.output_gain).log10() - 8.99385).abs() < 0.0001);
        writes[1].value = 0;
        apply_writes(std::slice::from_mut(&mut fx), &writes, -1, 1);
        assert!(!fx.bypass);
    }

    #[test]
    #[ignore = "installed library metadata probe; run through kontakto-heavy"]
    fn analog_saved_compressor_state() {
        let path = std::path::Path::new(
            "/mnt/MAIN_STORAGE/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki",
        );
        if !path.exists() {
            return;
        }
        let chunks = crate::read_chunks(path).unwrap();
        let program = Program::try_from(chunks.find_first(0x28).unwrap()).unwrap();
        let racks = program_racks(&program, &[], |_, _| panic!("rack decode failed"));
        let (_, insert) = racks
            .iter()
            .find(|(name, _)| name == "instrument insert")
            .unwrap();
        let compressors: Vec<_> = insert.iter().filter(|slot| slot.module == 0x19).collect();
        assert_eq!(compressors.len(), 1);
        let fx = compressors[0];
        eprintln!(
            "saved compressor slot={} bypass={} output_gain={} output_db={}",
            fx.slot,
            fx.bypass,
            fx.output_gain,
            20.0 * f64::from(fx.output_gain).log10()
        );
        assert_eq!(fx.slot, 1);
        assert!(!fx.bypass);
        let v1_gain = 16.0 * (560_434.0f64 / 1_000_000.0).powi(3);
        assert!((f64::from(fx.output_gain) - v1_gain).abs() < 1e-6);
        assert!(
            (20.0 * (f64::from(fx.output_gain) / f64::from(engine_gain(560_434))).log10()).abs()
                < 0.00005
        );
    }

    #[test]
    fn classic_compressor_slot_is_the_native_classic_law_with_its_link_flag() {
        let mut bytes = Vec::new();
        for x in [0.0f32, -18.0, 0.25, 10.0, 120.0] {
            bytes.extend(x.to_le_bytes());
        }
        bytes.push(1);
        let built = chain(&[slot(0x19, bytes, 1.0)], Scope::Bus);
        let [sampler_ir::Processor::Dynamics(c), ..] = built.processors[..] else {
            panic!("{:?}", built.processors)
        };
        use sampler_ir::fx::dynamics::{Law, Link, Native};
        assert_eq!(c.law, Law::KontaktClassic(Native { threshold_db: -18.0, ratio: 0.25 }));
        assert_eq!(c.link, Link::Mean);
        assert_eq!(c.attack.seconds(), 0.01);
        assert_eq!(c.release.seconds(), 0.12);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
    }

    #[test]
    fn pro_compressor_is_the_native_pro_law_and_always_max_linked() {
        let mut bytes = 2i32.to_le_bytes().to_vec();
        for x in [-18.0f32, 0.25, 10.0, 120.0] {
            bytes.extend(x.to_le_bytes());
        }
        bytes.push(1);
        let built = chain(&[slot(0x19, bytes, 1.0)], Scope::Bus);
        let [sampler_ir::Processor::Dynamics(c), ..] = built.processors[..] else {
            panic!("{:?}", built.processors)
        };
        use sampler_ir::fx::dynamics::{Law, Link};
        assert!(matches!(c.law, Law::KontaktPro(_)));
        assert_eq!(c.link, Link::Max);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
    }
}
