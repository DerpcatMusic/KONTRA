//! Twin Delay (module `0x6b`, saved v`0x10`, `$EFFECT_TYPE_TWINDELAY` = 57)
//! onto [`ir::DualDelay`].

use kontra_ir as ir;

use crate::module::{Cursor, MixMode, f32_le, ids, sync_beats, sync_unit, unit};

crate::define_effect! {
    struct TwinDelay,
    init: INIT,
    id: ids::TWIN_DELAY,
    module_id: 0x6b,
    versions: (0x10, 0x10),
    name: "Twin Delay",
    script_type: 57,
    mix: MixMode::WetDry,
    build: |t| ir::EffectKind::DualDelay(delay(t)),
    fields: [
        (predelay_l:      f32 = 0.0,  "$ENGINE_PAR_TDL_PREDELAY_L"),
        (time_l:          f32 = 0.35, "$ENGINE_PAR_TDL_TIME_L"),
        (feedback_l:      f32 = 0.3,  "$ENGINE_PAR_TDL_FEEDBACK_L"),
        (level_l:         f32 = 0.5,  "$ENGINE_PAR_TDL_LEVEL_L"),
        (predelay_r:      f32 = 0.0,  "$ENGINE_PAR_TDL_PREDELAY_R"),
        (time_r:          f32 = 0.5,  "$ENGINE_PAR_TDL_TIME_R"),
        (feedback_r:      f32 = 0.3,  "$ENGINE_PAR_TDL_FEEDBACK_R"),
        (level_r:         f32 = 0.5,  "$ENGINE_PAR_TDL_LEVEL_R"),
        (cross_feedback:  f32 = 0.0,  "$ENGINE_PAR_TDL_CROSS_FEEDBACK"),
        (width:           f32 = 1.0,  "$ENGINE_PAR_TDL_WIDTH"),
        (predelay_l_unit: i32 = 0,    "$ENGINE_PAR_TDL_PREDELAY_L_UNIT"),
        (predelay_r_unit: i32 = 0,    "$ENGINE_PAR_TDL_PREDELAY_R_UNIT"),
        (time_l_unit:     i32 = 0,    "$ENGINE_PAR_TDL_TIME_L_UNIT"),
        (time_r_unit:     i32 = 0,    "$ENGINE_PAR_TDL_TIME_R_UNIT"),
    ]
}

/// Free main delay time law: `2002 * x^2 - 3 * x + 1` ms (`1..2000` ms).
#[must_use]
pub fn time_ms(x: f32) -> f32 {
    let x = unit(x);
    (2002.0 * x * x - 3.0 * x + 1.0).clamp(1.0, 2000.0)
}

/// Free pre-delay time law: `2000 * x` ms (`0..2000` ms).
#[must_use]
pub fn predelay_ms(x: f32) -> f32 {
    2000.0 * unit(x)
}

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<TwinDelay> {
    if version != 0x10 {
        return None;
    }
    let mut cur = Cursor::new(public.get(52..)?);
    Some(TwinDelay {
        predelay_l: cur.f32()?,
        time_l: cur.f32()?,
        feedback_l: cur.f32()?,
        level_l: cur.f32()?,
        predelay_r: cur.f32()?,
        time_r: cur.f32()?,
        feedback_r: cur.f32()?,
        level_r: cur.f32()?,
        cross_feedback: cur.f32()?,
        width: cur.f32()?,
        predelay_l_unit: sync_unit(f32_le(public, 0)?),
        predelay_r_unit: sync_unit(f32_le(public, 13)?),
        time_l_unit: sync_unit(f32_le(public, 26)?),
        time_r_unit: sync_unit(f32_le(public, 39)?),
    })
}

/// Physical IR settings for `t`.
#[must_use]
pub fn delay(t: TwinDelay) -> ir::DualDelay {
    let pre = |x: f32, u: i32| {
        sync_beats(u).map_or_else(|| ir::DelayTime::Ms(predelay_ms(x)), ir::DelayTime::Beats)
    };
    let main = |x: f32, u: i32| {
        sync_beats(u).map_or_else(|| ir::DelayTime::Ms(time_ms(x)), ir::DelayTime::Beats)
    };
    ir::DualDelay {
        predelay_l: pre(t.predelay_l, t.predelay_l_unit),
        predelay_r: pre(t.predelay_r, t.predelay_r_unit),
        time_l: main(t.time_l, t.time_l_unit),
        time_r: main(t.time_r, t.time_r_unit),
        feedback_l: unit(t.feedback_l),
        feedback_r: unit(t.feedback_r),
        cross_feedback: unit(t.cross_feedback),
        level_l: unit(t.level_l),
        level_r: unit(t.level_r),
        width: unit(t.width),
    }
}
