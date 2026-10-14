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

/// 0x21 Skreamer: TONE, DRIVE, BASS, BRIGHT, MIX (identity-bound, setter 0x140968150).
pub(super) fn skreamer(knobs: &[f32]) -> Option<Config> {
    let knobs: [f32; 5] = knobs.try_into().ok()?;
    knobs.iter().all(|v| (0.0..=1.0).contains(v)).then(|| Config::skreamer(knobs))
}

/// The modules whose saved fields `typed_core` maps.
pub(super) const TYPED_CORES: [u16; 10] = [0x24, 0x42, 0x53, 0x55, 0x56, 0x58, 0x66, 0x67, 0x68, 0x78];

/// The amp, cabinet and pedal modules `amp` maps.
const AMPS: [u16; 5] = [0x24, 0x53, 0x55, 0x56, 0x58];

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
        // Tubescreamer (BParFXSkreamerDlx, apply 0x140abb9f0): mode, drive, tone, pot a, pot b,
        // filter, then a u8 that only resets the cores (no param). Level is fixed at 0.5.
        0x78 => {
            let (_reset, values) = values.split_last()?;
            let knobs = scatter(values, &[6, 0, 4, 1, 2, 3], [0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0])?;
            Some(Config::tubescreamer(sampler_ir::fx::color::Tubescreamer { knobs }))
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
        0x1d => "Surround Panner (version 0x70): variable speaker lists and an embedded resampling-style panner (0x140ab0070, 0x140ab4eb0); speaker matrix and pan law not extracted",
        0x22 => "Rotator: summed-input core 0x140aae920 with four-state recurrences and ring buffers; rotation, acceleration and distance laws not extracted",
        0x23 | 0x4d | 0x52 | 0x54 | 0x61 | 0x6c | 0x6e | 0x6f | 0x76 | 0x77 | 0x79 => {
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
    fn cabinet_record_fields_land_on_their_core_indices() {
        use EffectValue::{Float, Integer};
        let saved = fields(vec![Float(0.3), Float(0.6), Float(-0.5), Float(2.0), Integer(12)]);
        let Some(Config { curve: sampler_ir::fx::color::Curve::Amp(a), .. }) = typed_core(0x24, &saved) else {
            panic!("Cabinet not mapped");
        };
        assert_eq!(a.knobs[..5], [0.3, 0.6, -0.5, 1.0, 10.0]);
    }

    #[test]
    fn tubescreamer_record_fields_land_on_their_core_indices() {
        use EffectValue::{Byte, Float, Integer};
        let saved = fields(vec![Integer(2), Float(0.1), Float(0.2), Float(0.3), Float(0.4), Float(0.6), Byte(1)]);
        let Some(Config { curve: sampler_ir::fx::color::Curve::Tubescreamer(t), .. }) = typed_core(0x78, &saved) else {
            panic!("Tubescreamer not mapped");
        };
        // drive .1, pot a .3, pot b .4, filter .6, tone .2, level fixed .5, circuit 2 (1.0).
        assert_eq!(t.knobs, [0.1, 0.3, 0.4, 0.6, 0.2, 0.5, 1.0]);
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
}
