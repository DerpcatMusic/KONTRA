//! Cat (module `0x55`, saved v`0x10`, `$EFFECT_TYPE_CAT` = 35) onto
//! [`ir::AmpStack`] (`AmpVoicing::HardClipPedal`).

use kontra_ir as ir;

use crate::amps::base_amp;
use crate::module::{MixMode, eq_db, exp_range, ids, unit};

crate::define_effect! {
    struct Cat,
    init: INIT,
    id: ids::CAT,
    module_id: 0x55,
    versions: (0x10, 0x10),
    name: "Cat",
    script_type: 35,
    mix: MixMode::InPlace,
    saved: 0x10,
    build: |c| ir::EffectKind::AmpStack(amp(c)),
    fields: [
        (volume:     f32  = 0.5,   "$ENGINE_PAR_CT_VOLUME"),
        (distortion: f32  = 0.5,   "$ENGINE_PAR_CT_DISTORTION"),
        (filter:     f32  = 0.35,  "$ENGINE_PAR_CT_FILTER"),
        (bass:       f32  = 0.5,   "$ENGINE_PAR_CT_BASS"),
        (balls:      f32  = 0.5,   "$ENGINE_PAR_CT_BALLS"),
        (treble:     f32  = 0.5,   "$ENGINE_PAR_CT_TREBLE"),
        (tone:       f32  = 0.5,   "$ENGINE_PAR_CT_TONE"),
        (mono:       bool = false, "$ENGINE_PAR_CT_MONO"),
    ]
}

/// Physical IR settings for `c`.
#[must_use]
pub fn amp(c: Cat) -> ir::AmpStack {
    let dist = unit(c.distortion);
    let filt = unit(c.filter);
    let vol = unit(c.volume);
    ir::AmpStack {
        preamp_gain: 2.15 * 1.56f32.powf(dist - 0.5),
        drive: dist,
        master: 2.0 * vol,
        high_gain: dist > 0.65,
        bass_db: eq_db(c.bass, 6.0),
        mid_db: eq_db(c.tone, 6.0),
        mid_hz: 950.0,
        treble_db: eq_db(c.treble, 6.0),
        resonance_db: eq_db(c.balls, 6.0),
        tone_cut_hz: exp_range(1.0 - filt, 1_120.0, 14_000.0),
        mono: c.mono,
        ..base_amp(ir::AmpVoicing::HardClipPedal)
    }
}
