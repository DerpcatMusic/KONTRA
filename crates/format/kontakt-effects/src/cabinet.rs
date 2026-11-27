//! Cabinet (module `0x24`, saved v`0x50`, `$EFFECT_TYPE_CABINET` = 24) onto
//! [`ir::Cabinet`].

use kontra_ir as ir;

use crate::module::{MixMode, ids};

const BIPOLAR_TONE: ir::Law = ir::Law::Linear { lo: -1.0, hi: 1.0 };

crate::define_effect! {
    struct Cabinet,
    init: INIT,
    id: ids::CABINET,
    module_id: 0x24,
    versions: (0x50, 0x50),
    name: "Cabinet",
    script_type: 24,
    mix: MixMode::InPlace,
    saved: 0x50,
    build: |c| ir::EffectKind::Cabinet(cabinet(c)),
    fields: [
        (size:         f32 = 0.5,  "$ENGINE_PAR_CB_SIZE"),
        (air:          f32 = 0.25, "$ENGINE_PAR_CB_AIR"),
        (treble:       f32 = 0.0,  "$ENGINE_PAR_CB_TREBLE", BIPOLAR_TONE),
        (bass:         f32 = 0.0,  "$ENGINE_PAR_CB_BASS",   BIPOLAR_TONE),
        (cabinet_type: i32 = 0,    "$ENGINE_PAR_CABINET_TYPE"),
    ]
}

/// Physical IR settings for `c`.
#[must_use]
pub fn cabinet(c: Cabinet) -> ir::Cabinet {
    ir::Cabinet {
        model: c.cabinet_type.clamp(0, 11) as u8,
        size: c.size.clamp(0.0, 1.0),
        air: c.air.clamp(0.0, 1.0),
        bass_db: c.bass.clamp(-1.0, 1.0) * 16.0,
        treble_db: c.treble.clamp(-1.0, 1.0) * 16.0,
    }
}
