//! Stereo Tune (module `0x70`, saved v`0x10`, `$EFFECT_TYPE_STEREOTUNE` = 61)
//! onto [`ir::StereoDetune`].

use kontra_ir as ir;

use crate::module::{MixMode, ids, unit};

crate::define_effect! {
    struct StereoTune,
    init: INIT,
    id: ids::STEREO_TUNE,
    module_id: 0x70,
    versions: (0x10, 0x10),
    name: "Stereo Tune",
    script_type: 61,
    mix: MixMode::OwnMix,
    saved: 0x10,
    build: |s| ir::EffectKind::StereoDetune(detune(s)),
    fields: [
        (split:  f32 = 0.25, "$ENGINE_PAR_STEREOTUNE_SPLIT"),
        (drift:  f32 = 0.3,  "$ENGINE_PAR_STEREOTUNE_DRIFT"),
        (spread: f32 = 0.5,  "$ENGINE_PAR_STEREOTUNE_SPREAD"),
        (mix:    f32 = 0.5,  "$ENGINE_PAR_STEREOTUNE_MIX"),
    ]
}

/// Physical IR settings for `s`.
#[must_use]
pub fn detune(s: StereoTune) -> ir::StereoDetune {
    let f = 0.85 * 0.4 * unit(s.drift) + 0.06;
    let semis = 4.0 * f * f * f;
    ir::StereoDetune {
        split_hz: 900.0 * unit(s.split) + 100.0,
        ratio: (semis / 12.0).exp2(),
        spread: unit(s.spread),
        mix: unit(s.mix),
    }
}
