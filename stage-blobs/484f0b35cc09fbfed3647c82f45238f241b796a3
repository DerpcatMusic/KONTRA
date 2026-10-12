//! Forward decoding of the strict v1 0cb7a8a0 wavetable admission subset.
use ni_file::kontakt::objects::WavetableSource;
use sampler_ir::Wavetable;
pub(crate) fn admitted(source: &WavetableSource, tracking: bool) -> Option<Wavetable> {
    // port from v1 src/import.rs::wavetable_params and engine/wavetable.rs::supported.
    if !tracking || source.inharmonic_enabled || source.mod_type != 0 || source.phase_random != 0. {
        return None;
    }
    let form = |serialized| match serialized {
        1 => Some(0),
        17 => Some(16),
        _ => None,
    };
    let result = Wavetable {
        position: source.position,
        phase: source.phase,
        form1: source.form1,
        form2: source.form2,
        form1_type: form(source.form_type)?,
        form2_type: form(source.form2_type)?,
    };
    result.valid().then_some(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> WavetableSource {
        WavetableSource {
            common: [0; 30],
            position: 0.25,
            phase: 0.5,
            form1: 0.5,
            form2: 0.,
            phase_random: 0.,
            form_type: 17,
            form2_type: 1,
            quality: 1,
            inharmonic_enabled: false,
            inharmonic: 0.,
            mod_wave: 0,
            mod_type: 0,
            mod_amount: 0.,
            mod_tune: 0.,
            unknown_tail: [0; 16],
        }
    }
    #[test]
    fn saved_scalars_forward_decode_and_unproved_modes_stay_out() {
        let original = source();
        let wave = admitted(&original, true).unwrap();
        assert_eq!((wave.form1_type, wave.form2_type), (16, 0));
        assert_eq!(wave.position.to_bits(), original.position.to_bits());
        assert!(admitted(&original, false).is_none());
        for field in 0..7 {
            let mut bad = original.clone();
            match field {
                0 => bad.form_type = 2,
                1 => bad.mod_type = 1,
                2 => bad.inharmonic_enabled = true,
                3 => bad.phase_random = 0.01,
                4 => bad.position = f32::NAN,
                5 => bad.phase = 1.01,
                _ => bad.form2 = -0.01,
            }
            assert!(admitted(&bad, true).is_none());
        }
    }
}
