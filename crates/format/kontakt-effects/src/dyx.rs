//! DYX / AET Filter (module `0x1b`, saved v`0x60`, `$EFFECT_TYPE_AET_FILTER` = 25)
//! onto [`ir::SpectralMorph`].

use kontra_ir as ir;

use crate::module::{MixMode, ids, unit};

crate::define_effect! {
    struct Dyx,
    init: INIT,
    id: ids::DYX,
    module_id: 0x1b,
    versions: (0x60, 0x60),
    name: "DYX",
    script_type: 25,
    mix: MixMode::InPlace,
    saved: 0x60,
    build: |d| ir::EffectKind::SpectralMorph(spectral_morph(d)),
    fields: [
        (morph:    f32  = 0.5,   "$ENGINE_PAR_EXP_FILTER_MORPH"),
        (amount:   f32  = 0.75,  "$ENGINE_PAR_EXP_FILTER_AMOUNT"),
        (bank:     i32  = 0),
        (tracking: bool = false),
    ]
}

/// Physical IR settings for `d`.
#[must_use]
pub fn spectral_morph(d: Dyx) -> ir::SpectralMorph {
    ir::SpectralMorph {
        morph: unit(d.morph),
        amount: unit(d.amount),
        bank: d.bank.clamp(0, 3) as u8,
        tracking: d.tracking,
    }
}
