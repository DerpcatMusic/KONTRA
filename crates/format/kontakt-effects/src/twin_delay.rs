//! Twin Delay (module `0x6b`, saved v`0x10`, `$EFFECT_TYPE_TWINDELAY` = 57)
//! onto [`ir::DualDelay`].
//!
//! Saved record (92 bytes):
//! - Four 13-byte tempo-sync blocks at offsets `0`, `13`, `26`, `39` (each
//!   3 × `f32` + `u8`; first `f32` is quarter-note beats, active when `> 1e-9`)
//!   for `predelay_l`, `predelay_r`, `time_l`, `time_r`;
//! - Ten `f32` controls at offsets `52..92`: `predelay_l`, `time_l`,
//!   `feedback_l`, `level_l`, `predelay_r`, `time_r`, `feedback_r`, `level_r`,
//!   `cross_feedback`, `width` (each `0..=1`).
//!
//! Laws:
//! - Free `predelay`: `2000 * x` ms (`0..2000` ms);
//! - Free `time`: `2002 * x^2 - 3 * x + 1` ms (`1..2000` ms);
//! - Synced `predelay`/`time`: quarter-note beats from `$NI_SYNC_UNIT_*` (or
//!   saved sync block);
//! - `feedback_l/r`, `level_l/r`, `cross_feedback`, `width` pass in `0..=1`
//!   (`width = 0` swapped stereo, `0.5` mono, `1` normal stereo).

use kontra_ir as ir;

use crate::module::{RAW_LAW, UNIT_LAW, f32_le, knob_row, sync_beats, sync_unit};

/// Saved fields in record order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwinDelay {
    pub predelay_l: f32,
    pub time_l: f32,
    pub feedback_l: f32,
    pub level_l: f32,
    pub predelay_r: f32,
    pub time_r: f32,
    pub feedback_r: f32,
    pub level_r: f32,
    pub cross_feedback: f32,
    pub width: f32,
    /// `$NI_SYNC_UNIT_*` indices (0 = free ms).
    pub predelay_l_unit: i32,
    pub predelay_r_unit: i32,
    pub time_l_unit: i32,
    pub time_r_unit: i32,
}

/// Fresh INIT defaults for a script-loaded Twin Delay.
pub const INIT: TwinDelay = TwinDelay {
    predelay_l: 0.0,
    time_l: 0.35,
    feedback_l: 0.3,
    level_l: 0.5,
    predelay_r: 0.0,
    time_r: 0.5,
    feedback_r: 0.3,
    level_r: 0.5,
    cross_feedback: 0.0,
    width: 1.0,
    predelay_l_unit: 0,
    predelay_r_unit: 0,
    time_l_unit: 0,
    time_r_unit: 0,
};

/// Free main delay time law: `2002 * x^2 - 3 * x + 1` ms (`1..2000` ms).
#[must_use]
pub fn time_ms(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    (2002.0 * x * x - 3.0 * x + 1.0).clamp(1.0, 2000.0)
}

/// Free pre-delay time law: `2000 * x` ms (`0..2000` ms).
#[must_use]
pub fn predelay_ms(x: f32) -> f32 {
    2000.0 * x.clamp(0.0, 1.0)
}

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<TwinDelay> {
    if version != 0x10 {
        return None;
    }
    Some(TwinDelay {
        predelay_l: f32_le(public, 52)?,
        time_l: f32_le(public, 56)?,
        feedback_l: f32_le(public, 60)?,
        level_l: f32_le(public, 64)?,
        predelay_r: f32_le(public, 68)?,
        time_r: f32_le(public, 72)?,
        feedback_r: f32_le(public, 76)?,
        level_r: f32_le(public, 80)?,
        cross_feedback: f32_le(public, 84)?,
        width: f32_le(public, 88)?,
        predelay_l_unit: sync_unit(f32_le(public, 0)?),
        predelay_r_unit: sync_unit(f32_le(public, 13)?),
        time_l_unit: sync_unit(f32_le(public, 26)?),
        time_r_unit: sync_unit(f32_le(public, 39)?),
    })
}

/// Physical IR settings for `t`.
#[must_use]
pub fn delay(t: TwinDelay) -> ir::DualDelay {
    let pre = |x: f32, unit: i32| match sync_beats(unit) {
        Some(beats) => ir::DelayTime::Beats(beats),
        None => ir::DelayTime::Ms(predelay_ms(x)),
    };
    let main = |x: f32, unit: i32| match sync_beats(unit) {
        Some(beats) => ir::DelayTime::Beats(beats),
        None => ir::DelayTime::Ms(time_ms(x)),
    };
    ir::DualDelay {
        predelay_l: pre(t.predelay_l, t.predelay_l_unit),
        predelay_r: pre(t.predelay_r, t.predelay_r_unit),
        time_l: main(t.time_l, t.time_l_unit),
        time_r: main(t.time_r, t.time_r_unit),
        feedback_l: t.feedback_l.clamp(0.0, 1.0),
        feedback_r: t.feedback_r.clamp(0.0, 1.0),
        cross_feedback: t.cross_feedback.clamp(0.0, 1.0),
        level_l: t.level_l.clamp(0.0, 1.0),
        level_r: t.level_r.clamp(0.0, 1.0),
        width: t.width.clamp(0.0, 1.0),
    }
}

/// Single-encoding module descriptor for Twin Delay (`0x6b`).
pub static MODULE: crate::module::Module = crate::module::Module {
    id: crate::module::ids::TWIN_DELAY,
    module_id: 0x6b,
    versions: (0x10, 0x10),
    name: "Twin Delay",
    script_type: 57,
    mix: crate::module::MixMode::WetDry,
    knobs: crate::module::KNOBS.split_at(14).0,
    build: |v| {
        let [
            predelay_l,
            time_l,
            feedback_l,
            level_l,
            predelay_r,
            time_r,
            feedback_r,
            level_r,
            cross_feedback,
            width,
            predelay_l_unit,
            predelay_r_unit,
            time_l_unit,
            time_r_unit,
        ] = *v
        else {
            return None;
        };
        Some(ir::EffectKind::DualDelay(delay(TwinDelay {
            predelay_l,
            time_l,
            feedback_l,
            level_l,
            predelay_r,
            time_r,
            feedback_r,
            level_r,
            cross_feedback,
            width,
            predelay_l_unit: predelay_l_unit.round() as i32,
            predelay_r_unit: predelay_r_unit.round() as i32,
            time_l_unit: time_l_unit.round() as i32,
            time_r_unit: time_r_unit.round() as i32,
        })))
    },
};

#[must_use]
pub fn model(t: TwinDelay) -> ir::KnobModel {
    MODULE.knob_model(&[
        t.predelay_l,
        t.time_l,
        t.feedback_l,
        t.level_l,
        t.predelay_r,
        t.time_r,
        t.feedback_r,
        t.level_r,
        t.cross_feedback,
        t.width,
        t.predelay_l_unit as f32,
        t.predelay_r_unit as f32,
        t.time_l_unit as f32,
        t.time_r_unit as f32,
    ])
}

/// KSP `$ENGINE_PAR_TDL_*` knob rows.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 14] {
    [
        knob_row("$ENGINE_PAR_TDL_PREDELAY_L", 0, UNIT_LAW),
        knob_row("$ENGINE_PAR_TDL_TIME_L", 1, UNIT_LAW),
        knob_row("$ENGINE_PAR_TDL_FEEDBACK_L", 2, UNIT_LAW),
        knob_row("$ENGINE_PAR_TDL_LEVEL_L", 3, UNIT_LAW),
        knob_row("$ENGINE_PAR_TDL_PREDELAY_R", 4, UNIT_LAW),
        knob_row("$ENGINE_PAR_TDL_TIME_R", 5, UNIT_LAW),
        knob_row("$ENGINE_PAR_TDL_FEEDBACK_R", 6, UNIT_LAW),
        knob_row("$ENGINE_PAR_TDL_LEVEL_R", 7, UNIT_LAW),
        knob_row("$ENGINE_PAR_TDL_CROSS_FEEDBACK", 8, UNIT_LAW),
        knob_row("$ENGINE_PAR_TDL_WIDTH", 9, UNIT_LAW),
        knob_row("$ENGINE_PAR_TDL_PREDELAY_L_UNIT", 10, RAW_LAW),
        knob_row("$ENGINE_PAR_TDL_PREDELAY_R_UNIT", 11, RAW_LAW),
        knob_row("$ENGINE_PAR_TDL_TIME_L_UNIT", 12, RAW_LAW),
        knob_row("$ENGINE_PAR_TDL_TIME_R_UNIT", 13, RAW_LAW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_decodes_and_time_laws_match_endpoints() {
        let mut bytes = Vec::new();
        // 4 sync blocks (13 bytes each): pre_l free, pre_r free, time_l synced 1 beat, time_r free
        for beats in [-1.0f32, -1.0, 1.0, -1.0] {
            bytes.extend(beats.to_le_bytes());
            bytes.extend(0.0f32.to_le_bytes());
            bytes.extend(0.0f32.to_le_bytes());
            bytes.push(0u8);
        }
        for v in [0.1f32, 0.5, 0.4, 0.8, 0.25, 1.0, 0.6, 0.7, 0.2, 0.5] {
            bytes.extend(v.to_le_bytes());
        }
        assert_eq!(bytes.len(), 92);
        let t = saved(0x10, &bytes).unwrap();
        assert_eq!(t.time_l_unit, 9);
        assert_eq!(t.time_r_unit, 0);
        let d = delay(t);
        assert_eq!(d.predelay_l, ir::DelayTime::Ms(200.0));
        assert_eq!(d.predelay_r, ir::DelayTime::Ms(500.0));
        assert_eq!(d.time_l, ir::DelayTime::Beats(1.0));
        assert_eq!(d.time_r, ir::DelayTime::Ms(2000.0));
        assert!((time_ms(0.0) - 1.0).abs() < 1e-5);
        assert!((time_ms(0.5) - 500.0).abs() < 1e-4);
        assert!((time_ms(1.0) - 2000.0).abs() < 1e-4);
    }

    #[test]
    fn knob_model_roundtrips() {
        let m = model(INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::DualDelay(delay(INIT)))
        );
    }
}
