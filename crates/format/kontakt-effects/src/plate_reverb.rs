//! Plate Reverb (module `0x57`, saved v`0x10`, `$EFFECT_TYPE_PLATEREVERB` = 38)
//! onto [`ir::Reverb`] with [`ir::Network::Plate`].

use kontra_ir as ir;

use crate::module::{MixMode, ids, unit};

crate::define_effect! {
    struct PlateReverb,
    init: INIT,
    id: ids::PLATE_REVERB,
    module_id: 0x57,
    versions: (0x10, 0x10),
    name: "Plate Reverb",
    script_type: 38,
    mix: MixMode::WetDry,
    saved: 0x10,
    build: |p| ir::EffectKind::Reverb(reverb(p)),
    fields: [
        (decay:     f32 = 0.5, "$ENGINE_PAR_PR_DECAY"),
        (low_shelf: f32 = 0.5, "$ENGINE_PAR_PR_LOWSHELF"),
        (high_damp: f32 = 0.5, "$ENGINE_PAR_PR_HIDAMP"),
        (predelay:  f32 = 0.0, "$ENGINE_PAR_PR_PREDELAY"),
        (stereo:    f32 = 1.0, "$ENGINE_PAR_PR_STEREO"),
    ]
}

/// Physical IR reverb settings for `p`.
#[must_use]
pub fn reverb(p: PlateReverb) -> ir::Reverb {
    let hd = unit(p.high_damp);
    ir::Reverb {
        seconds: 6.4 * unit(p.decay) + 0.556,
        size: 1.0,
        network: Some(ir::Network::Plate),
        damping: hd,
        diffusion: 0.9,
        predelay_ms: 250.0 * unit(p.predelay),
        high_cut_hz: (1.0 - hd) * 4851.0 + 6000.0,
        low_shelf_db: 20.0 * ((1.0 - unit(p.low_shelf)) * 1.71875 - 1.4375),
        stereo: unit(p.stereo),
        modulation_ms: 0.0,
        modulation_hz: 0.0,
    }
}
