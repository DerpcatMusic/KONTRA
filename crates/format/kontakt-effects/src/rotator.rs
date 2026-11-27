//! Rotator (module `0x22`, saved v`0x50`) onto the engine's two-rotor rotary
//! speaker ([`ir::Rotary`]).

use kontra_ir as ir;

use crate::module::{MixMode, ids, unit};

const HORN_SLOW_HZ: f32 = 0.806;
const HORN_FAST_HZ: f32 = 6.09;
const DRUM_SLOW_HZ: f32 = 0.68;
const DRUM_FAST_HZ: f32 = 5.40;
const CROSSOVER_HZ: f32 = 800.0;

crate::define_effect! {
    struct Rotator,
    init: INIT,
    id: ids::ROTATOR,
    module_id: 0x22,
    versions: (0x50, 0x50),
    name: "Rotator",
    script_type: 22,
    mix: MixMode::OwnMix,
    saved: 0x50,
    build: |r| ir::EffectKind::Rotary(rotary(r)),
    fields: [
        (speed:    f32 = 0.0, "$ENGINE_PAR_RT_SPEED"),
        (balance:  f32 = 0.5, "$ENGINE_PAR_RT_BALANCE"),
        (accel_hi: f32 = 0.5, "$ENGINE_PAR_RT_ACCEL_HI"),
        (accel_lo: f32 = 0.5, "$ENGINE_PAR_RT_ACCEL_LO"),
        (distance: f32 = 0.3, "$ENGINE_PAR_RT_DISTANCE"),
        (mix:      f32 = 1.0, "$ENGINE_PAR_RT_MIX"),
    ]
}

/// Translates Kontakt Rotator fields into format-neutral [`ir::Rotary`].
#[must_use]
pub fn rotary(r: Rotator) -> ir::Rotary {
    let mix_u = unit(r.mix);
    ir::Rotary {
        fast: r.speed > 0.5,
        horn_slow_hz: HORN_SLOW_HZ,
        horn_fast_hz: HORN_FAST_HZ,
        drum_slow_hz: DRUM_SLOW_HZ,
        drum_fast_hz: DRUM_FAST_HZ,
        accel_hi: unit(r.accel_hi),
        accel_lo: unit(r.accel_lo),
        crossover_hz: CROSSOVER_HZ,
        balance: unit(r.balance),
        distance: unit(r.distance),
        mix: (mix_u * mix_u * mix_u).clamp(0.0, 1.0),
    }
}
