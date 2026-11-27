//! Cry Wah (module `0x58`, saved v`0x10`, `$EFFECT_TYPE_CRYWAH` = 37) onto
//! [`ir::Wah`].

use std::f32::consts::TAU;

use kontra_ir as ir;

use crate::module::{MixMode, ids, unit};

crate::define_effect! {
    struct CryWah,
    init: INIT,
    id: ids::CRY_WAH,
    module_id: 0x58,
    versions: (0x10, 0x10),
    name: "Cry Wah",
    script_type: 37,
    mix: MixMode::InPlace,
    saved: 0x10,
    build: |w| ir::EffectKind::Wah(wah(w)),
    fields: [
        (pedal: f32  = 0.5,   "$ENGINE_PAR_CW_PEDAL"),
        (mono:  bool = false, "$ENGINE_PAR_CW_MONO"),
    ]
}

#[inline]
fn pitch_index(idx: f32) -> f32 {
    (idx / 60.0 - 20.0).exp2()
}

/// Physical IR settings for `w`.
#[must_use]
pub fn wah(w: CryWah) -> ir::Wah {
    let p = unit(w.pedal);
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
