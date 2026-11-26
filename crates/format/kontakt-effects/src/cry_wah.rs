//! Cry Wah (module `0x58`, saved v`0x10`, `$EFFECT_TYPE_CRYWAH` = 37) onto
//! [`ir::Wah`].
//!
//! Saved record (5 bytes, `FB`): `pedal` (f32, `0..=1`), `mono` (u8, bool).
//!
//! Laws (pitch index `P(x) = 2^(x / 60 - 20)` Hz):
//! - `peak_hz = P(135.84999 * p + 1731.0316)` (~461.6 Hz heel to ~2217.5 Hz toe);
//! - `peak_q = 1 / (0.21074 * p + 0.10227)` (~9.78 heel to ~3.19 toe);
//! - `peak_gain = 9.2921 * p^2 - 0.2921`;
//! - `low_hz = 0.7545 * P(-153.6096 * p^2 + 1137.4288)`;
//! - `low_gain = P(-153.6096 * p^2 + 1031.3928) - 1`;
//! - `mid_hz = 2000` Hz, `mid_gain = 0.23182878 - 2 * 0.048911855 * (p - 1)^4`;
//! - `high_hz = 21536.875 / (2*pi)` (~3427.7 Hz), `high_gain = 2 * 2.9415 * p^4 + 5.9703722`.

use std::f32::consts::TAU;

use kontra_ir as ir;

use crate::module::{RAW_LAW, UNIT_LAW, f32_le, knob_row};

/// Saved fields in record order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CryWah {
    pub pedal: f32,
    pub mono: bool,
}

/// Fresh INIT defaults for a script-loaded Cry Wah.
pub const INIT: CryWah = CryWah {
    pedal: 0.5,
    mono: false,
};

#[inline]
fn pitch_index(idx: f32) -> f32 {
    (idx / 60.0 - 20.0).exp2()
}

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<CryWah> {
    if version != 0x10 {
        return None;
    }
    Some(CryWah {
        pedal: f32_le(public, 0)?,
        mono: *public.get(4)? != 0,
    })
}

/// Physical IR settings for `w`.
#[must_use]
pub fn wah(w: CryWah) -> ir::Wah {
    let p = w.pedal.clamp(0.0, 1.0);
    let p2 = p * p;
    let p4 = p2 * p2;
    let pm1 = p - 1.0;
    let pm1_4 = (pm1 * pm1) * (pm1 * pm1);
    ir::Wah {
        peak_hz: pitch_index(135.849_99 * p + 1_731.031_6),
        peak_q: 1.0 / (0.210_74 * p + 0.102_27),
        peak_gain: 9.2921 * p2 - 0.2921,
        low_hz: 0.7545 * pitch_index(-153.6096 * p2 + 1_137.428_8),
        low_gain: pitch_index(-153.6096 * p2 + 1_031.392_8) - 1.0,
        mid_hz: 2000.0,
        mid_gain: 0.231_828_78 - 2.0 * 0.048_911_855 * pm1_4,
        high_hz: 21_536.875 / TAU,
        high_gain: 2.0 * 2.9415 * p4 + 5.970_372,
        mono: w.mono,
    }
}

/// Single-encoding module descriptor for Cry Wah (`0x58`).
pub static MODULE: crate::module::Module = crate::module::Module {
    id: crate::module::ids::CRY_WAH,
    module_id: 0x58,
    versions: (0x10, 0x10),
    name: "Cry Wah",
    script_type: 37,
    mix: crate::module::MixMode::InPlace,
    knobs: crate::module::KNOBS.split_at(2).0,
    build: |v| {
        let [pedal, mono] = *v else {
            return None;
        };
        Some(ir::EffectKind::Wah(wah(CryWah {
            pedal,
            mono: mono > 0.0,
        })))
    },
};

#[must_use]
pub fn model(w: CryWah) -> ir::KnobModel {
    MODULE.knob_model(&[w.pedal, if w.mono { 1.0 } else { 0.0 }])
}

/// KSP `$ENGINE_PAR_CW_*` knob rows.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 2] {
    [
        knob_row("$ENGINE_PAR_CW_PEDAL", 0, UNIT_LAW),
        knob_row("$ENGINE_PAR_CW_MONO", 1, RAW_LAW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_decodes_and_pedal_sweeps_peak_from_heel_to_toe() {
        let mut bytes = 0.25f32.to_le_bytes().to_vec();
        bytes.push(1);
        let w = saved(0x10, &bytes).unwrap();
        assert_eq!(
            w,
            CryWah {
                pedal: 0.25,
                mono: true
            }
        );
        assert!(saved(0x11, &bytes).is_none());

        let heel = wah(CryWah {
            pedal: 0.0,
            mono: false,
        });
        let toe = wah(CryWah {
            pedal: 1.0,
            mono: false,
        });
        assert!(
            (heel.peak_hz - 461.61).abs() < 0.5,
            "heel peak {}",
            heel.peak_hz
        );
        assert!(
            (toe.peak_hz - 2217.46).abs() < 1.0,
            "toe peak {}",
            toe.peak_hz
        );
        assert!(heel.peak_q > 9.0 && toe.peak_q > 3.0 && toe.peak_q < 3.5);
    }

    #[test]
    fn knob_model_roundtrips() {
        let m = model(INIT);
        assert_eq!(m.build(&m.saved), Some(ir::EffectKind::Wah(wah(INIT))));
    }
}
