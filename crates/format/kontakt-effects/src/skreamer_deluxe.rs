//! Skreamer Deluxe (module `0x78`, saved v`0x10`, `$EFFECT_TYPE_SKREAMERDLX` = 69)
//! onto [`ir::AmpStack`] (`AmpVoicing::MidOverdrive`).

use kontra_ir as ir;

use crate::amps::base_amp;
use crate::module::{MixMode, eq_db, exp_range, ids, unit};

crate::define_effect! {
    struct SkreamerDeluxe,
    init: INIT,
    id: ids::SKREAMER_DELUXE,
    module_id: 0x78,
    versions: (0x10, 0x10),
    name: "Skreamer Deluxe",
    script_type: 69,
    mix: MixMode::InPlace,
    saved: 0x10,
    build: |s| ir::EffectKind::AmpStack(amp(s)),
    fields: [
        (mode:   i32  = 0,     "$ENGINE_PAR_SKDLX_MODE"),
        (drive:  f32  = 0.5,   "$ENGINE_PAR_SKDLX_DRIVE"),
        (tone:   f32  = 0.5,   "$ENGINE_PAR_SKDLX_TONE"),
        (bass:   f32  = 0.5,   "$ENGINE_PAR_SKDLX_BASS"),
        (mid:    f32  = 0.5,   "$ENGINE_PAR_SKDLX_MID"),
        (treble: f32  = 0.5,   "$ENGINE_PAR_SKDLX_TREBLE"),
        (mono:   bool = false, "$ENGINE_PAR_SKDLX_MONO"),
    ]
}

/// Physical IR settings for `s`.
#[must_use]
pub fn amp(s: SkreamerDeluxe) -> ir::AmpStack {
    let mode = s.mode.clamp(0, 2) as u8;
    let drv = unit(s.drive);
    let tone = unit(s.tone);
    let mode_gain = match mode {
        1 => 1.15,
        2 => 1.55,
        _ => 2.75,
    };
    ir::AmpStack {
        preamp_gain: mode_gain * 8.5f32.powf(drv - 0.5),
        drive: drv,
        master: 1.0,
        high_gain: mode == 2,
        bass_db: eq_db(s.bass, 8.0),
        mid_db: eq_db(s.mid, 8.0),
        mid_hz: 720.0,
        treble_db: eq_db(s.treble, 8.0) + eq_db(tone, 14.5),
        tone_cut_hz: exp_range(tone, 1_500.0, 18_000.0),
        mono: s.mono,
        ..base_amp(ir::AmpVoicing::MidOverdrive(mode))
    }
}
