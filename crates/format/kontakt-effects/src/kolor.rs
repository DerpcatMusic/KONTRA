//! Kolor (module `0x79`, saved v`0x10`, `$EFFECT_TYPE_KOLOR` = 70) onto
//! [`ir::ColorSat`].

use kontra_ir as ir;

use crate::module::{MixMode, eq_db, exp_range, ids, unit};

crate::define_effect! {
    struct Kolor,
    init: INIT,
    id: ids::KOLOR,
    module_id: 0x79,
    versions: (0x10, 0x10),
    name: "Kolor",
    script_type: 70,
    mix: MixMode::OwnMix,
    saved: 0x10,
    build: |k| ir::EffectKind::ColorSat(color_sat(k)),
    fields: [
        (mode:        i32  = 0,     "$ENGINE_PAR_KOLOR_MODE"),
        (drive:       f32  = 0.4,   "$ENGINE_PAR_KOLOR_DRIVE"),
        (boost_level: f32  = 0.5,   "$ENGINE_PAR_KOLOR_BOOST_LEVEL"),
        (bass_saver:  f32  = 0.0,   "$ENGINE_PAR_KOLOR_BASS_SAVER"),
        (lpf:         f32  = 1.0,   "$ENGINE_PAR_KOLOR_LPF"),
        (hpf:         f32  = 0.0,   "$ENGINE_PAR_KOLOR_HPF"),
        (bass:        f32  = 0.5,   "$ENGINE_PAR_KOLOR_BASS"),
        (mid:         f32  = 0.5,   "$ENGINE_PAR_KOLOR_MID"),
        (mid_freq:    f32  = 0.5,   "$ENGINE_PAR_KOLOR_MID_FREQ"),
        (mid_q:       f32  = 0.35,  "$ENGINE_PAR_KOLOR_MID_Q"),
        (treble:      f32  = 0.5,   "$ENGINE_PAR_KOLOR_TREBLE"),
        (mix:         f32  = 1.0,   "$ENGINE_PAR_KOLOR_MIX"),
        (boost:       bool = false, "$ENGINE_PAR_KOLOR_BOOST"),
    ]
}

/// Physical IR settings for `k`.
#[must_use]
pub fn color_sat(k: Kolor) -> ir::ColorSat {
    ir::ColorSat {
        character: k.mode.clamp(0, 9) as u8,
        drive: unit(k.drive),
        boost: k.boost,
        boost_db: unit(k.boost_level) * 24.0,
        bass_saver: unit(k.bass_saver),
        lpf_hz: exp_range(k.lpf, 10.0, 20_000.0),
        hpf_hz: exp_range(k.hpf, 10.0, 20_000.0),
        bass_db: eq_db(k.bass, 12.0),
        mid_db: eq_db(k.mid, 12.0),
        mid_hz: exp_range(k.mid_freq, 150.0, 6_000.0),
        mid_q: exp_range(k.mid_q, 0.4, 6.0),
        treble_db: eq_db(k.treble, 12.0),
        mix: unit(k.mix),
    }
}
