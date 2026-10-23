//! Kontakt `color` effects onto `sampler_ir::fx::color::Config`.
//!
//! Each admitted native module is a preset of the one color superset. A module
//! whose law is not proved is not mapped; [`unproved`] says exactly what is
//! missing, and the translator reports it next to the generic "not modeled".
use super::{Lowering, Notes, Params, Site, Slot, product};
use sampler_ir::fx::color::Config;
use sampler_ir::{Processor, Reason, SlotAddress};

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

/// 0x21 Skreamer: TONE, DRIVE, BASS, BRIGHT, MIX (identity-bound, setter 0x140968150).
pub(super) fn skreamer(knobs: &[f32]) -> Option<Config> {
    let knobs: [f32; 5] = knobs.try_into().ok()?;
    knobs.iter().all(|v| (0.0..=1.0).contains(v)).then(|| Config::skreamer(knobs))
}

/// 0x78 Skreamer Deluxe (BParFXSkreamerDlx, reader 0x140ac6040, apply 0x140abb9f0): the mode
/// enum (0..=2), drive, tone, bass, mid, treble, mono flag. Each knob is clamped to 0..1 by the
/// apply and lands on the Tubescreamer core index: drive 0, bass 1, mid 2, treble 3, tone 4, level
/// 5 fixed at 0.5, mode 6 = enum/2. The stereo wrapper (flag 0) runs two cores; the mono branch
/// (flag != 0) is not modeled, so it returns None.
pub(super) fn skreamer_deluxe(fields: &[ni_file::kontakt::objects::EffectField]) -> Option<Config> {
    let v = raw(fields)?;
    let &[mode, drive, tone, bass, mid, treble, mono] = &v[..] else { return None };
    // The reader accepts only enum 0..=2; the mono flag is the unmodeled branch.
    if mono != 0.0 || !matches!(mode, 0.0 | 1.0 | 2.0) {
        return None;
    }
    let knobs = [drive, bass, mid, treble, tone, 0.5, mode * 0.5].map(|k| k.clamp(0.0, 1.0));
    Some(Config::stereorizer_tubescreamer(sampler_ir::fx::color::Tubescreamer { knobs }))
}

/// The live knobs of a stored record: (module, field count, normalized float fields,
/// field index). Skreamer: TONE, DRIVE, BASS, BRIGHT, MIX (fx_dynamics_dist_tape.tsv).
/// Fuzz: amount, bass, treble are floats; the mono flag is a byte and is not live.
pub(super) fn live_field(parameter: &str) -> Option<(u16, usize, usize, usize)> {
    Some(match parameter {
        "ENGINE_PAR_SK_TONE" => (0x21, 5, 5, 0),
        "ENGINE_PAR_SK_DRIVE" => (0x21, 5, 5, 1),
        "ENGINE_PAR_SK_BASS" => (0x21, 5, 5, 2),
        "ENGINE_PAR_SK_BRIGHT" => (0x21, 5, 5, 3),
        "ENGINE_PAR_SK_MIX" => (0x21, 5, 5, 4),
        "ENGINE_PAR_FUZZ_AMOUNT" => (0x6e, 4, 3, 0),
        "ENGINE_PAR_FUZZ_BASS" => (0x6e, 4, 3, 1),
        "ENGINE_PAR_FUZZ_TREBLE" => (0x6e, 4, 3, 2),
        _ => return None,
    })
}

/// The modules whose saved fields `typed_core` maps.
pub(super) const TYPED_CORES: [u16; 13] = [0x23, 0x24, 0x42, 0x4d, 0x53, 0x55, 0x56, 0x58, 0x66, 0x67, 0x68, 0x6e, 0x6f];

/// The amp, cabinet and pedal modules `amp` maps.
const AMPS: [u16; 9] = [0x23, 0x24, 0x4d, 0x53, 0x55, 0x56, 0x58, 0x6e, 0x6f];

/// The record's fields as f32 (an i32 enum as its value), or None for any other kind.
fn raw(fields: &[ni_file::kontakt::objects::EffectField]) -> Option<Vec<f32>> {
    use ni_file::kontakt::objects::EffectValue::{Byte, Float, Integer};
    fields
        .iter()
        .map(|f| match f.value {
            Float(v) if v.is_finite() => Some(v),
            Integer(i) => Some(i as f32),
            Byte(b) => Some(f32::from(b)),
            _ => None,
        })
        .collect()
}

/// Amp, cabinet and pedal records (bindings: kontakt-bindings/fx_other.tsv) in core order.
fn amp(module: u16, fields: &[ni_file::kontakt::objects::EffectField]) -> Option<Config> {
    use sampler_ir::fx::color::{Amp, AmpModel};
    let v = raw(fields)?;
    let (model, values) = match (module, &v[..]) {
        // Cabinet (reader 0x140d005b0, Fx4I): size, air, treble, bass (stored -1..1), type
        // (the type law 0x140ab8c70 clamps it to 0..=10).
        (0x24, &[size, air, treble, bass, kind]) => (
            AmpModel::Cabinet,
            vec![size.clamp(0.0, 1.0), air.clamp(0.0, 1.0), treble.clamp(-1.0, 1.0), bass.clamp(-1.0, 1.0), kind.clamp(0.0, 10.0)],
        ),
        // Cry Wah (reader 0x140d009f0, FB): PEDAL, MONO.
        (0x58, &[pedal, mono]) => (AmpModel::CryWah, vec![pedal.clamp(0.0, 1.0), f32::from(mono != 0.0)]),
        // DStortion (reader 0x140d00a50, Fx6B): volume, tone, drive, bass, mid, treble, mono.
        (0x56, &[vol, tone, drive, bass, mid, treble, mono]) => (AmpModel::DStortion, vec![vol, tone, drive, bass, mid, treble].into_iter().map(|v| v.clamp(0.0, 1.0)).chain([f32::from(mono != 0.0)]).collect()),
        // Cat (Fx7B): volume, distortion, filter, bass, balls, treble, tone, mono.
        (0x55, &[vol, dist, filt, bass, balls, treble, tone, mono]) => (AmpModel::Cat, vec![vol, dist, filt, bass, balls, treble, tone].into_iter().map(|v| v.clamp(0.0, 1.0)).chain([f32::from(mono != 0.0)]).collect()),
        // AC Box (Fx7B, apply 0x140ab8970): normal volume, brilliant volume, bass, treble, tone cut, tremolo speed, tremolo depth, mono.
        (0x53, &[normal, brilliant, bass, treble, cut, speed, depth, mono]) => (AmpModel::AcBox, vec![normal, brilliant, bass, treble, cut, speed, depth].into_iter().map(|v| v.clamp(0.0, 1.0)).chain([f32::from(mono != 0.0)]).collect()),
        // Twang (Fx4BB, apply 0x140adf660): volume, treble, mid, bass, bright, mono.
        (0x23, &[vol, treble, mid, bass, bright, mono]) => (AmpModel::Twang, vec![vol, treble, mid, bass].into_iter().map(|v| v.clamp(0.0, 1.0)).chain([f32::from(bright != 0.0), f32::from(mono != 0.0)]).collect()),
        // Jump (reader 0x140d01e10, Fx6BB): preamp, master, presence, treble, mid, bass, higain, mono.
        // The engine applies its fixed preset; these knobs are stored, not yet used.
        (0x4d, &[preamp, master, presence, treble, mid, bass, higain, mono]) => (AmpModel::Jump, vec![preamp, master, presence, treble, mid, bass].into_iter().map(|v| v.clamp(0.0, 1.0)).chain([f32::from(higain != 0.0), f32::from(mono != 0.0)]).collect()),
        // Fuzz (FFFB, apply 0x140abaa80): amount, bass, treble, mono.
        (0x6e, &[amount, bass, treble, mono]) => (AmpModel::Fuzz, vec![amount, bass, treble].into_iter().map(|v| v.clamp(0.0, 1.0)).chain([f32::from(mono != 0.0)]).collect()),
        // Big Fuzz (Fx4B, apply 0x140ab9f40): sustain, tone, bass, treble, mono.
        (0x6f, &[sustain, tone, bass, treble, mono]) => (AmpModel::BigFuzz, vec![sustain, tone, bass, treble].into_iter().map(|v| v.clamp(0.0, 1.0)).chain([f32::from(mono != 0.0)]).collect()),
        _ => return None,
    };
    let mut knobs = [0.0; 24];
    knobs[..values.len()].copy_from_slice(&values);
    Some(Config::amp(Amp { model, knobs }))
}

/// A saved field as its core value (the param setters 0x1409aa540 & co.): f32 clamped to
/// 0..1, u8 as 0/1, an i32 enum 0..=2 as i/2 (anything else throws in the reader).
fn core_value(v: &ni_file::kontakt::objects::EffectValue) -> Option<f32> {
    use ni_file::kontakt::objects::EffectValue::{Byte, Float, Integer};
    match *v {
        Float(f) if f.is_finite() => Some(f.clamp(0.0, 1.0)),
        Byte(b) => Some(if b != 0 { 1.0 } else { 0.0 }),
        Integer(i @ 0..=2) => Some(i as f32 * 0.5),
        _ => None,
    }
}

/// Places saved fields at their core indices (`to[field] = index`) over `defaults`.
fn scatter<const N: usize>(values: &[f32], to: &[usize], defaults: [f32; N]) -> Option<[f32; N]> {
    (values.len() == to.len()).then(|| {
        let mut knobs = defaults;
        for (&v, &i) in values.iter().zip(to) {
            knobs[i] = v;
        }
        knobs
    })
}

/// Effektrig/KONTAKTFX cores from their typed records (per-type apply, kcolor/records.md).
pub(super) fn typed_core(module: u16, fields: &[ni_file::kontakt::objects::EffectField]) -> Option<Config> {
    if AMPS.contains(&module) {
        return amp(module, fields);
    }
    let values: Vec<f32> = fields.iter().map(|f| core_value(&f.value)).collect::<Option<_>>()?;
    match module {
        // Tape Saturator: gain, warmth, roll-off, quality, identity (setter 0x140ae1e10).
        0x42 => {
            let [gain, warmth, rolloff, quality] = scatter(&values, &[0, 1, 2, 3], [0.0; 4])?;
            Some(Config::tape(sampler_ir::fx::color::Tape { gain, warmth, rolloff, quality: quality >= 0.5 }))
        }
        // Bite (apply 0x140aba100): hpf, frequency, bits, jitter, crunch, dither, expand,
        // saturate, prefilter, postfilter, mix, dcquant.
        0x66 => {
            let knobs = scatter(&values, &[10, 2, 5, 3, 4, 6, 8, 0, 1, 9, 11, 7], [0.0; 12])?;
            Some(Config::bite(sampler_ir::fx::color::Bite { knobs }))
        }
        // Dirt (apply 0x140aba420): f32 x10, comp A/B, type A/B, routing; the level (p15)
        // stays at its default 0.75 (unity).
        0x67 => {
            const DEFAULTS: [f32; 16] =
                [0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 0.0, 1.0, 0.5, 0.75];
            let knobs = scatter(&values, &[0, 2, 6, 4, 1, 3, 7, 5, 13, 14, 10, 11, 8, 9, 12], DEFAULTS)?;
            Some(Config::dirt(sampler_ir::fx::color::Dirt { knobs }))
        }
        // Freak (apply 0x140aba860): f32 x13, u8 x5, the mode enum; unsaved knobs keep the
        // init-slot defaults.
        0x68 => {
            const DEFAULTS: [f32; 19] =
                [0.0, 0.5, 1.0, 0.5, 0.0, 0.5, 0.5, 0.5, 0.0, 0.0, 0.5, 0.0, 1.0, 0.5, 0.5, 0.0, 0.5, 1.0, 1.0];
            let to = [3, 4, 11, 18, 13, 12, 14, 1, 0, 10, 5, 7, 6, 15, 17, 2, 8, 9, 16];
            let knobs = scatter(&values, &to, DEFAULTS)?;
            Some(Config::freak(sampler_ir::fx::color::Freak { knobs }))
        }
        _ => None,
    }
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
        0x1d => "Surround Panner (version 0x70): speaker gain law is ported (sampler_core::dsp::surround_panner); the block driver (0x140ab0070, 0x140ab4eb0), LFE routing and channel layouts are not mapped",
        0x70 => "Stereo Tune outside a bus chain: its delay state is bus-owned like a reverb, so a voice or group insert passes audio through",
        0x22 => "Rotator: summed-input core 0x140aae920 with four-state recurrences and ring buffers; rotation, acceleration and distance laws not extracted",
        0x78 => "Skreamer Deluxe mono branch (stored flag != 0, driver 140902480 calls only core 1480 in place): not decoded, so the slot passes audio through unprocessed",
        // Reader 0x140ac51f0 (Fx4B) is shared with Big Fuzz, but the Chainsaw ctor, apply and
        // kernel are not extracted and KSP has no ENGINE_PAR_CHAINSAW_* names: its knobs are
        // not guessed from the Big Fuzz layout.
        0x77 => "Chainsaw: record reader 0x140ac51f0 (Fx4B) is shared with Big Fuzz, but its ctor, apply, kernel and parameter names are not extracted",
        // The record is lowered (`Params::Kolor`); the shaper cases and control tick are not run.
        0x79 => "Kolor: record lowered to sampler_ir::fx::kolor::Record; shaper cases 0..9 and the control tick (14197ab90) are not wired, so the slot passes audio through",
        0x52 | 0x54 | 0x61 | 0x6c | 0x76 => {
            "amp, cabinet or pedal model: cores and tables are not extracted, so no topology is proved"
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ni_file::kontakt::objects::{EffectField, EffectValue};

    fn fields(values: Vec<EffectValue>) -> Vec<EffectField> {
        values.into_iter().map(|value| EffectField { name: "param_n", value }).collect()
    }

    #[test]
    fn bite_record_fields_land_on_their_core_indices() {
        use EffectValue::{Byte, Float, Integer};
        let saved = fields(
            [Integer(2)]
                .into_iter()
                .chain((1..=10).map(|i| Float(i as f32 / 10.0)))
                .chain([Byte(0x5a)])
                .collect(),
        );
        let Some(Config { curve: sampler_ir::fx::color::Curve::Bite(b), .. }) = typed_core(0x66, &saved) else {
            panic!("Bite not mapped");
        };
        // saturate .7, prefilter .8, frequency .1, jitter .3, crunch .4, bits .2, dither .5,
        // dcquant 1, expand .6, postfilter .9, hpf 1 (i32 2), mix 1.
        assert_eq!(b.knobs, [0.7, 0.8, 0.1, 0.3, 0.4, 0.2, 0.5, 1.0, 0.6, 0.9, 1.0, 1.0]);
        // An out-of-range enum is a reader error, not a guess.
        let mut bad = saved.clone();
        bad[0].value = Integer(3);
        assert!(typed_core(0x66, &bad).is_none());
    }

    #[test]
    fn skreamer_deluxe_record_lands_on_its_tubescreamer_indices() {
        use EffectValue::{Byte, Float, Integer};
        let saved = fields(vec![Integer(2), Float(0.1), Float(0.4), Float(0.2), Float(0.3), Float(0.6), Byte(0)]);
        let Some(Config { curve: sampler_ir::fx::color::Curve::Stereorizer(t), .. }) = skreamer_deluxe(&saved) else {
            panic!("Skreamer Deluxe not mapped");
        };
        // drive 0, bass 1, mid 2, treble 3, tone 4, level .5, mode enum/2.
        assert_eq!(t.knobs, [0.1, 0.2, 0.3, 0.6, 0.4, 0.5, 1.0]);
        // The mono flag and an out-of-range enum are not guesses.
        assert!(skreamer_deluxe(&fields(vec![Integer(0), Float(0.5), Float(0.5), Float(0.5), Float(0.5), Float(0.5), Byte(1)])).is_none());
        assert!(skreamer_deluxe(&fields(vec![Integer(3), Float(0.5), Float(0.5), Float(0.5), Float(0.5), Float(0.5), Byte(0)])).is_none());
    }

    #[test]
    fn chainsaw_is_diagnosed_and_never_mapped_onto_big_fuzz_knobs() {
        use EffectValue::{Byte, Float};
        let saved = fields(vec![Float(0.5), Float(0.5), Float(0.5), Float(0.5), Byte(1)]);
        assert!(typed_core(0x77, &saved).is_none());
        assert!(unproved(0x77).is_some_and(|why| why.starts_with("Chainsaw")));
    }

    #[test]
    fn cabinet_record_fields_land_on_their_core_indices() {
        use EffectValue::{Float, Integer};
        let saved = fields(vec![Float(0.3), Float(0.6), Float(-0.5), Float(2.0), Integer(12)]);
        let Some(Config { curve: sampler_ir::fx::color::Curve::Amp(a), .. }) = typed_core(0x24, &saved) else {
            panic!("Cabinet not mapped");
        };
        assert_eq!(a.knobs[..5], [0.3, 0.6, -0.5, 1.0, 10.0]);
    }

    #[test]
    fn dirt_record_fields_land_on_their_core_indices() {
        use EffectValue::{Byte, Float, Integer};
        let saved = fields(
            (1..=10)
                .map(|i| Float(i as f32 / 10.0))
                .chain([Byte(1), Byte(0), Integer(1), Integer(2), Integer(0)])
                .collect(),
        );
        let Some(Config { curve: sampler_ir::fx::color::Curve::Dirt(d), .. }) = typed_core(0x67, &saved) else {
            panic!("Dirt not mapped");
        };
        assert_eq!(
            d.knobs,
            [0.1, 0.5, 0.2, 0.6, 0.4, 0.8, 0.3, 0.7, 0.5, 1.0, 1.0, 0.0, 0.0, 0.9, 1.0, 0.75]
        );
    }

    use crate::effects::*;
    use crate::effects::test_support::*;

    /// Conflux's Volume knob writes GN_GAIN to its insert Gainer; unity is
    /// 396851 and full scale +24 dB (16x), the effect output-gain law.
    #[test]
    fn live_gainer_gain_reaches_its_slot_with_the_output_gain_law() {
        let fx = slot(0x13, 1f32.to_le_bytes().to_vec(), 1.);
        let built = crate::effects::chain_with(&[fx], crate::effects::Scope::Voice, None, Some((0, -1)), (0, -1), &[]);
        let mut rt = fx_runtime(built.processors, 120., &[[0., 0.], [0.25, -0.25]]);
        let address = sampler_core::EngineParameterAddress {
            parameter: sampler_core::engine_parameter_id("ENGINE_PAR_GN_GAIN").unwrap(),
            group: 0, slot: 0, generic: -1,
        };
        assert_eq!(rt.engine_parameter(address), Ok(396851), "saved unity gain reads back as unity");
        assert!(rt.set_engine_parameter(sampler_core::EngineParameterAddress { slot: 1, ..address }, 0).is_err());
        rt.set_engine_parameter(address, 1_000_000).unwrap();
        assert_eq!(rt.engine_parameter(address), Ok(1_000_000));
    }

    #[test]
    fn live_shape_drive_and_damping_reach_only_the_physical_slot() {
        for (module, payload, parameter, value, expected) in [
            (0x1d, vec![0.0f32, 0.], "ENGINE_PAR_SHAPE", 750_000, 0.75f32),
            (0x1e, vec![0.0f32, 0., 0.], "ENGINE_PAR_DRIVE", 1_000_000, 0.74903756),
            (0x1e, vec![0.0f32, 0., 0.], "ENGINE_PAR_DAMPING", 1_000_000, 0.031603266),
        ] {
            let bytes = payload.iter().flat_map(|v| v.to_le_bytes()).collect();
            let fx = slot(module, bytes, 1.);
            let built = crate::effects::chain_with(&[fx], crate::effects::Scope::Voice, None,
                Some((0, -1)), (0, -1), &[]);
            assert!(built.notes.is_empty(), "{:?}", built.notes);
            let mut rt = fx_runtime(built.processors, 120., &[[0., 0.], [0.25, -0.25]]);
            let address = sampler_core::EngineParameterAddress {
                parameter: sampler_core::engine_parameter_id(parameter).unwrap(),
                group: 0, slot: 0, generic: -1,
            };
            let mut before = [[0.; 2]; 1]; rt.render(&mut before).unwrap();
            assert!(rt.set_engine_parameter(sampler_core::EngineParameterAddress {
                slot: 1, ..address }, value).is_err());
            rt.set_engine_parameter(address, value).unwrap();
            let mut after = [[0.; 2]; 1]; rt.render(&mut after).unwrap();
            assert!((after[0][0] - expected).abs() < 2e-6,
                "{parameter}: {:?} expected {expected}", after[0]);
        }
    }

    #[test]
    fn native_typed_fx_modes_and_lofi_fields_reach_executable_consumers() {
        // Primitive layouts recovered from gpt-format-fxmod@b7f6af0b dirty
        // native-reader evidence; generated bytes, no installed bank required.
        let mut distortion = 1i32.to_le_bytes().to_vec();
        distortion.extend([0.8f32, 0.2].into_iter().flat_map(f32::to_le_bytes));
        let effect = slot(0x1e, distortion, 1.);
        assert_eq!(effect.params(), Some(Params::Distortion { mode: 1., drive: 0.8, damping: 0.2 }));
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(std::slice::from_ref(&effect), scope);
            assert!(out.notes.is_empty(), "{:?}", out.notes);
            assert_eq!(out.processors.first(), Some(&sampler_ir::Processor::Color(color::distortion(1., 0.8, 0.2).unwrap())));
        }
        let mut saturation = 0.5f32.to_le_bytes().to_vec();
        saturation.extend(1i32.to_le_bytes());
        assert_eq!(slot(0x1d, saturation, 1.).params(), Some(Params::Saturation { shape: 0.5, mode: 1. }));
        let mut lofi: Vec<_> = [0.4f32, 0.2].into_iter().flat_map(f32::to_le_bytes).collect();
        lofi.push(0);
        lofi.extend([0.3f32, 0.5].into_iter().flat_map(f32::to_le_bytes));
        let effect = slot(0x20, lofi, 1.);
        assert_eq!(effect.params(), Some(Params::LoFi { values: [0.4, 0.2, 0.3, 0.5], flag: false }));
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(std::slice::from_ref(&effect), scope);
            assert!(out.notes.is_empty(), "{:?}", out.notes);
            assert_eq!(out.processors.first(), Some(&sampler_ir::Processor::Color(color::lofi(&[0.4, 0.2, 0.3, 0.5]).unwrap())));
        }
    }

    #[test]
    fn authored_lofi_slot_is_an_executable_processor_in_both_scopes() {
        let mut payload: Vec<u8> = [0.4f32, 0.2]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        payload.push(0); // Native FFBFF: byte before NoiseLevel and NoiseColor.
        payload.extend(0.0f32.to_le_bytes());
        payload.extend(0.5f32.to_le_bytes());
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(&[slot(0x20, payload.clone(), 1.)], scope);
            assert!(out.notes.is_empty(), "{:?}", out.notes);
            assert!(!out.processors.is_empty(), "Lo-Fi cannot disappear");
        }
    }

    #[test]
    fn skreamer_init_writes_validate_module_record_and_complete_physical_address() {
        let source = "on init\nset_engine_par($ENGINE_PAR_SK_TONE,100000,-1,3,$NI_INSERT_BUS)\nset_engine_par($ENGINE_PAR_SK_DRIVE,900000,-1,3,$NI_INSERT_BUS)\nset_engine_par($ENGINE_PAR_SK_BASS,200000,-1,3,$NI_INSERT_BUS)\nset_engine_par($ENGINE_PAR_SK_BRIGHT,800000,-1,3,$NI_INSERT_BUS)\nset_engine_par($ENGINE_PAR_SK_MIX,300000,-1,3,$NI_INSERT_BUS)\nend on";
        let initialized = sampler_ksp::initialize(source, sampler_ksp::Limits::LIBRARY, &Default::default()).unwrap();
        let writes = initialized.engine_pars();
        let payload: Vec<_> = [0.5f32; 5].into_iter().flat_map(f32::to_le_bytes).collect();
        let mut fx = slot(0x21, payload.clone(), 0.25);
        fx.slot = 3;
        apply_writes(std::slice::from_mut(&mut fx), &writes, 0, -1);
        assert_eq!(fx.public, payload);
        apply_writes(std::slice::from_mut(&mut fx), &writes, -1, 2);
        assert_eq!(fx.public, payload);
        apply_writes(std::slice::from_mut(&mut fx), &writes, -1, 1);
        assert_eq!(fx.public, [0.1f32, 0.9, 0.2, 0.8, 0.3].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>());
        assert_eq!(fx.output_gain, 0.25);
        fx.module = 0x42;
        fx.version = 0x10;
        fx.public = [0.5f32; 3].into_iter().flat_map(f32::to_le_bytes).chain([0]).collect();
        let tape = fx.public.clone();
        apply_writes(std::slice::from_mut(&mut fx), &writes, -1, 1);
        assert_eq!(fx.public, tape, "SK controls cannot edit Tape");
        fx.module = 0x21;
        fx.version = 0x50;
        fx.public = payload[..16].to_vec();
        let invalid = fx.public.clone();
        apply_writes(std::slice::from_mut(&mut fx), &writes, -1, 1);
        assert_eq!(fx.public, invalid, "a truncated record cannot be repaired by a guessed write");
    }

    #[test]
    fn skreamer_live_lanes_keep_bypass_output_mix_and_tape_limit_distinct() {
        let mut fx = slot(0x21, [0.5f32; 5].into_iter().flat_map(f32::to_le_bytes).collect(), 0.25);
        fx.slot = 3;
        fx.bypass = true;
        let built = chain_with(&[fx], Scope::Bus, None, Some((-1, 1)), (-1, 1), &[]);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
        assert!(matches!(built.processors[0], sampler_ir::Processor::Mix { count: 1, address: sampler_ir::SlotAddress { group: -1, slot: 3, generic: 1 }, dry: 0., wet: 0.25, bypass: true }));
        let sampler_ir::Processor::Color(config) = built.processors[1] else { panic!("missing Skreamer") };
        assert_eq!(config.lanes().len(), 5);
        assert_eq!(built.lane_slots, [(3, 1)]);
        let mut tape = slot(0x42, [0.5f32; 3].into_iter().flat_map(f32::to_le_bytes).chain([0]).collect(), 1.);
        tape.version = 0x10;
        let tape = chain_with(&[tape], Scope::Bus, None, Some((-1, 1)), (-1, 1), &[]);
        assert!(tape.notes.iter().any(|(_, feature, value, _)| feature.contains("live controls") && value == "knobs are constants"));
    }

    #[test]
    fn init_shape_drive_and_damping_writes_update_only_addressed_valid_slots() {
        let initialized = sampler_ksp::initialize(
            "on init\nset_engine_par($ENGINE_PAR_SHAPE, 750000, -1, 3, $NI_INSERT_BUS)\nset_engine_par($ENGINE_PAR_DRIVE, 250000, -1, 5, $NI_INSERT_BUS)\nset_engine_par($ENGINE_PAR_DAMPING, 1000000, -1, 5, $NI_INSERT_BUS)\nend on",
            sampler_ksp::Limits::LIBRARY, &Default::default(),
        ).unwrap();
        let writes = initialized.engine_pars();
        let mut saturation = slot(0x1d, [0f32, 0.].into_iter().flat_map(f32::to_le_bytes).collect(), 1.);
        saturation.slot = 3;
        let mut distortion_payload = 1i32.to_le_bytes().to_vec();
        distortion_payload.extend([0.8f32, 0.2].into_iter().flat_map(f32::to_le_bytes));
        let mut distortion = slot(0x1e, distortion_payload, 1.);
        distortion.slot = 5;
        let mut slots = [saturation, distortion];
        let saved: Vec<_> = slots.iter().map(|fx| fx.public.clone()).collect();
        apply_writes(&mut slots, &writes, 0, -1);
        assert_eq!(slots.iter().map(|fx| fx.public.clone()).collect::<Vec<_>>(), saved);
        apply_writes(&mut slots, &writes, -1, 1);
        assert_eq!(slots[0].params(), Some(Params::Saturation { shape: 0.5, mode: 0. }));
        assert_eq!(slots[1].params(), Some(Params::Distortion { mode: 1., drive: 0.25, damping: 1. }));
        slots[1].public.truncate(4);
        apply_writes(&mut slots, &writes, -1, 1);
        assert_eq!(slots[1].public, 1i32.to_le_bytes());
        slots[1].module = 0x1d;
        slots[1].public = saved[0].clone();
        apply_writes(&mut slots, &writes, -1, 1);
        assert_eq!(slots[1].public, saved[0], "Drive/Damping cannot overwrite another module");
    }

    #[test]
    fn classic_saturation_saved_slot_is_admitted_in_both_scopes() {
        assert_eq!(module_name(0x1d), "Saturation");
        let payload: Vec<_> = [0.30065f32, 0.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(&[slot(0x1d, payload.clone(), 0.5)], scope);
            assert!(out.notes.is_empty(), "{:?}", out.notes);
            assert_eq!(out.processors.len(), 2, "saturation and slot output");
        }
    }

    #[test]
    fn analog_distortion_saved_slot_is_admitted_in_both_scopes() {
        let payload: Vec<_> = [0.0f32, 0.523648, 0.0]
            .into_iter().flat_map(f32::to_le_bytes).collect();
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(&[slot(0x1e, payload.clone(), 0.5)], scope);
            assert!(out.notes.is_empty(), "{:?}", out.notes);
            assert_eq!(out.processors.len(), 2, "distortion and slot output");
        }
    }

    #[test]
    fn gainer_mixes_its_stored_dry_level_with_the_gained_signal() {
        // KONTAKT_REFERENCE s.25: a fresh Gainer at -6 dB reads 0.5 + 0.5 * 0.501.
        let mut g = slot(0x13, 0.501f32.to_le_bytes().to_vec(), 0.5);
        g.dry_level = 0.5;
        let c = chain(&[g], Scope::Bus);
        let [sampler_ir::Processor::Color(config)] = c.processors[..] else {
            panic!("{:?}", c.processors)
        };
        let sampler_ir::fx::color::Gainer { gain, dry } = config.gainer.unwrap();
        assert!((dry + gain.linear() - 0.7505).abs() < 1e-6);
    }

    #[test]
    fn modeller_follows_the_measured_spread_and_pan_laws_and_the_inverter_output_applies() {
        let modeller = |spread: f32, pan: f32| {
            let mut bytes = spread.to_le_bytes().to_vec();
            bytes.extend(pan.to_le_bytes());
            bytes.push(0);
            match chain(&[slot(0x1f, bytes, 1.0)], Scope::Voice)
                .processors
                .as_slice()
            {
                [
                    sampler_ir::Processor::Color(config),
                ] => {
                    let stereo = config.stereo.unwrap();
                    assert!(!stereo.pseudo);
                    assert_eq!(stereo.width, (f64::from(spread).clamp(-1.0, 1.0) + 1.0) * 0.5);
                    assert_eq!(stereo.pan, f64::from(pan));
                }
                other => panic!("{other:?}"),
            }
        };
        // The width/pan fields reach the kernel; its measured laws (KONTAKT_REFERENCE.md
        // s.20) are tested in sampler-core's color::stereo.
        modeller(1.0, 0.0);
        modeller(1.5, 0.0);
        modeller(-0.5, 0.0);
        modeller(0.0, -0.5);
        // An Inverter that changes nothing still applies its Output (Una's tone
        // groups store +6 dB against -6 dB on their instrument bus).
        let inverter = chain(&[slot(0x1a, vec![0, 0], 2.0)], Scope::Voice).processors;
        assert_eq!(
            inverter,
            vec![sampler_ir::Processor::StereoMatrix([
                [2.0, 0.0],
                [0.0, 2.0]
            ])]
        );
    }
}

/// A color unit, addressed by its slot when a script may write it.
fn push_color(b: &mut Lowering, s: &Site, config: Config) {
    let config = match s.dynamic {
        Some((group, generic)) => {
            b.out.lane_slots.push((s.fx.slot, b.out.processors.len()));
            config.at(SlotAddress { group, slot: s.fx.slot as i32, generic })
        }
        None => config,
    };
    b.out.processors.push(Processor::Color(config));
}

/// 0x1e Distortion: one color unit, or not modelled.
pub(super) fn slot_distortion(b: &mut Lowering, s: &Site, notes: &mut Notes, mode: f32, drive: f32, damping: f32) -> bool {
    match distortion(mode, drive, damping) {
        Some(config) => {
            b.flush();
            push_color(b, s, config);
            b.combined = s.gain;
            true
        }
        None => {
            notes.push(("distortion parameters".into(), format!("{mode}/{drive}/{damping}"), Reason::InvalidValue));
            false
        }
    }
}

/// 0x1d Saturation (the Surround Panner, version 0x70, is handled by the caller).
pub(super) fn slot_saturation(b: &mut Lowering, s: &Site, notes: &mut Notes, shape: f32, mode: f32) -> bool {
    match saturation(shape, mode) {
        Some(config) => {
            b.flush();
            push_color(b, s, config);
            b.combined = s.gain;
            true
        }
        None => {
            notes.push(("saturation algorithm/shape".into(), format!("{mode}/{shape}"), Reason::UnknownLaw));
            false
        }
    }
}

/// Skreamer (0x21): fields TONE, DRIVE, BASS, BRIGHT, MIX (identity, bindings TSV).
pub(super) fn slot_skreamer(b: &mut Lowering, s: &Site, fields: &[ni_file::kontakt::objects::EffectField]) -> bool {
    b.flush();
    let knobs: Vec<f32> = fields
        .iter()
        .filter_map(|f| match f.value {
            ni_file::kontakt::objects::EffectValue::Float(v) => Some(v),
            _ => None,
        })
        .collect();
    match skreamer(&knobs) {
        Some(config) => {
            push_color(b, s, config);
            b.combined = s.gain;
            true
        }
        None => false,
    }
}

/// 0x78 Skreamer Deluxe: `Stereorizer<Tubescreamer>`, stereo branch only.
pub(super) fn slot_skreamer_deluxe(b: &mut Lowering, s: &Site, notes: &mut Notes, fields: &[ni_file::kontakt::objects::EffectField]) -> bool {
    b.flush();
    match skreamer_deluxe(fields) {
        Some(config) => {
            push_color(b, s, config);
            b.combined = s.gain;
            if s.dynamic.is_some() {
                notes.push(("Skreamer Deluxe live controls".into(), "knobs are constants".into(), Reason::NotModeled));
            }
            true
        }
        None => false,
    }
}

/// The typed cores (`TYPED_CORES`): constant knobs, so no live lanes.
pub(super) fn slot_typed(
    b: &mut Lowering,
    s: &Site,
    notes: &mut Notes,
    fields: &[ni_file::kontakt::objects::EffectField],
) -> bool {
    b.flush();
    match typed_core(s.fx.module, fields) {
        Some(config) if s.fx.module == 0x6e => {
            // Fuzz amount, bass and treble are script-addressed lanes; its mono flag is not.
            push_color(b, s, config);
            b.combined = s.gain;
            if s.dynamic.is_some() {
                notes.push(("Fuzz mono flag".into(), "live mono switch is not modeled; stored value only".into(), Reason::NotModeled));
            }
            true
        }
        Some(config) if s.fx.module == 0x68 => {
            // Freak: the continuous knobs are script lanes (binding rows `F`); the bool
            // switches and the MODE enum stay stored values.
            push_color(b, s, config);
            b.combined = s.gain;
            if s.dynamic.is_some() {
                notes.push(("Freak switches and mode".into(), "stored values only; not live".into(), Reason::NotModeled));
            }
            true
        }
        Some(config) => {
            b.out.processors.push(Processor::Color(config));
            b.combined = s.gain;
            if s.fx.module == 0x4d {
                notes.push(("Jump knobs".into(), "fixed fitted preset; knob laws not measured".into(), Reason::NotModeled));
            }
            if s.dynamic.is_some() {
                notes.push(("Creative FX live controls".into(), "knobs are constants".into(), Reason::NotModeled));
            }
            true
        }
        None => false,
    }
}

/// Lo-Fi (0x20): bits, frequency, and two more knobs plus a flag.
pub(super) fn slot_lofi(b: &mut Lowering, s: &Site, notes: &mut Notes, values: &[f32; 4], flag: bool) -> bool {
    b.flush();
    match lofi(values) {
        Some(config) => {
            push_color(b, s, config);
            b.combined = product(s.gain, b.combined);
            if flag {
                notes.push(("Lo-Fi third field flag".into(), "true".into(), Reason::UnknownLaw));
            }
            true
        }
        None => {
            notes.push((
                "Lo-Fi parameters".into(),
                "non-finite or outside normalized range".into(),
                Reason::InvalidValue,
            ));
            false
        }
    }
}

/// A Gainer mixes its slot's dry level with the gained signal, `dry + out * g`
/// (KONTAKT_REFERENCE s.25 measured a fresh module at 0.5 + 0.5 g). Every stored
/// Gainer in the local corpus has dry 0 and output 1, so they stay plain `g`; a
/// nonzero stored dry is honoured. Addressed, so a script's GN_GAIN reaches it
/// (Conflux's Volume knob). The gain is inside the unit, so the matrix is not set.
pub(super) fn slot_gainer(b: &mut Lowering, s: &Site, parameter: f32) {
    b.flush();
    push_color(
        b,
        s,
        gainer(
            f64::from(parameter) * if s.mix { 1.0 } else { s.wet },
            if s.mix { 0.0 } else { f64::from(s.fx.dry_level) },
        ),
    );
}

/// Stereo Modeller (0x1f): spread, pan and pseudo-stereo.
pub(super) fn slot_stereo(b: &mut Lowering, s: &Site, spread: f32, pan: f32, pseudo: bool) {
    b.flush();
    push_color(b, s, stereo_modeller(spread, pan, pseudo));
    b.combined = s.gain;
}

impl Slot {
    /// Stores a live knob's init-time write in the saved record. The module's first
    /// `float_fields` fields are normalized floats; `index` is the field. The layout is
    /// Skreamer's (4-byte fields); the Fuzz record's offsets are not measured.
    pub(super) fn apply_live_field(&mut self, module: u16, count: usize, float_fields: usize, index: usize, value: i32) {
        if self.module != module {
            return;
        }
        let Some(Params::Fields(fields)) = self.params() else { return };
        if fields.len() != count || !fields[..float_fields].iter().all(|f| {
            matches!(f.value, ni_file::kontakt::objects::EffectValue::Float(v) if (0.0..=1.0).contains(&v))
        }) {
            return;
        }
        let stored = sampler_core::EngineParameterLaw::Linear { low: 0., high: 1. }.decode(value) as f32;
        let at = index * 4;
        self.public[at..at + 4].copy_from_slice(&stored.to_le_bytes());
    }

    pub(super) fn apply_drive_parameter(&mut self, parameter: &str, value: i32) {
        let (at, low, high) = match (parameter, self.params()) {
            ("ENGINE_PAR_SHAPE", Some(Params::Saturation { .. })) => (0, -1., 1.),
            ("ENGINE_PAR_DRIVE", Some(Params::Distortion { .. })) => (4, 0., 1.),
            ("ENGINE_PAR_DAMPING", Some(Params::Distortion { .. })) => (8, 0., 1.),
            _ => return,
        };
        let stored = sampler_core::EngineParameterLaw::Linear { low, high }.decode(value) as f32;
        self.public[at..at + 4].copy_from_slice(&stored.to_le_bytes());
    }
}
