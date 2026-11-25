//! A group's source-module `$ENGINE_PAR_*` controls: what a script reads of
//! each, seeded from the saved source record, and what a set keeps. KONTRA
//! plays none of them yet; they are held ([`ir::EnginePar::GroupHeld`]).
//!
//! Measured on the reference 8.13 (2026-11-25, `disp_sweep.py` `GET`/`AT`
//! on one group per source mode, `tm_sweep.py` for the saved fields):
//! a control the group's mode lacks reads 0 and drops a set; the mode, the
//! switches and the interpolation quality are on every group.

use kontra_ir as ir;
use ni_file::kontakt::objects::{BParSrcMode, GroupParams, SourceValue};

/// The source-module controls and what a set keeps (sweeps/a-sw.tsv,
/// a-rng.tsv, mid*.tsv): the mode is read only; the switches read 1 for any
/// value but 0; the interpolation quality keeps 0..=2 (the setter refuses
/// the rest, `ACCEPTS`); Smooth, Env Order and Formant Shift keep an
/// unclamped 0..=1 `f32`; the rest clamp to 0..=1e6.
pub(crate) const SOURCE: [(&str, ir::HeldSet); 12] = {
    use ir::HeldSet::{Clamp, Drop, Keep, Switch, Unit};
    const UNIT: ir::HeldSet = Clamp(0, 1_000_000);
    [
        ("$ENGINE_PAR_SOURCE_MODE", Drop),
        ("$ENGINE_PAR_TRACKING", Switch),
        ("$ENGINE_PAR_HQI_MODE", Keep),
        ("$ENGINE_PAR_TM_LEGATO", Switch),
        ("$ENGINE_PAR_TMPRO_KEEP_FORMANTS", Switch),
        ("$ENGINE_PAR_SMOOTH", Unit),
        ("$ENGINE_PAR_FORMANT", UNIT),
        ("$ENGINE_PAR_GRAIN_LENGTH", UNIT),
        ("$ENGINE_PAR_ENVELOPE_ORDER", Unit),
        ("$ENGINE_PAR_FORMANT_SHIFT", Unit),
        ("$ENGINE_PAR_SLICE_ATTACK", UNIT),
        ("$ENGINE_PAR_SLICE_RELEASE", UNIT),
    ]
};

/// The `$NI_SOURCE_MODE_*` value of each saved mode: Sampler, Tone Machine,
/// Time Machine 1, DFD, Beat Machine, Time Machine 2, S1200, MP60, Time
/// Machine Pro, Wavetable (saved and read back, sweeps/srcmodes.nki).
const KSP_MODE: [i32; 10] = [0, 1, 3, 4, 5, 6, 7, 8, 9, 2];

/// What a script reads of `group`'s source controls: `(−1, key, value)`.
pub(crate) fn held(record: &BParSrcMode, params: &GroupParams) -> Vec<(i8, u32, i32)> {
    let field = |name: &str| {
        record
            .fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.value)
    };
    let float = |name: &str| match field(name) {
        Some(SourceValue::Float(v)) => Some(v),
        _ => None,
    };
    let flag = |name: &str| match field(name) {
        Some(SourceValue::Flag(v)) => Some(i32::from(v)),
        _ => None,
    };
    // A bipolar control saved as −1..=1.
    let bipolar = |name: &str| float(name).map(|v| ir::unit_read((v + 1.0) * 0.5));
    // An exponential time saved in its unit: `(ln(y + 2) − c)·k`, the
    // inverse of the readout's `e^(x/k + c) − 2` (Grain, Slice Atk, Slice
    // Rel; [ref]).
    let time = |name: &str, c: f32, k: f32| {
        float(name).map(|y| ir::unit_read((((y + 2.0).ln() - c) * k).clamp(0.0, 1.0)))
    };
    let mut out = vec![
        (
            "$ENGINE_PAR_SOURCE_MODE",
            KSP_MODE.get(record.mode as usize).copied(),
        ),
        ("$ENGINE_PAR_TRACKING", Some(i32::from(params.key_tracking))),
        (
            "$ENGINE_PAR_HQI_MODE",
            Some(params.interp_quality.clamp(0, 2)),
        ),
        ("$ENGINE_PAR_TM_LEGATO", flag("common_flag_11")),
        // Saved by Time Machine Pro only; any other mode reads its fresh 1.
        (
            "$ENGINE_PAR_TMPRO_KEEP_FORMANTS",
            flag("pro_flag_2").or(Some(1)),
        ),
    ];
    // Tone Machine saves its formant, not its grain; Time Machine 1 the
    // reverse. The control not saved reads its fresh value.
    match record.mode {
        1 | 2 => out.extend([
            (
                "$ENGINE_PAR_SMOOTH",
                float("machine_float_1").map(ir::unit_read),
            ),
            (
                "$ENGINE_PAR_FORMANT",
                if record.mode == 1 {
                    bipolar("machine_float_2")
                } else {
                    Some(500_000)
                },
            ),
            (
                "$ENGINE_PAR_GRAIN_LENGTH",
                if record.mode == 2 {
                    time("machine_float_2", 0.832_909_1, 0.263_708_65)
                } else {
                    Some(328_721)
                },
            ),
        ]),
        4 => out.extend([
            (
                "$ENGINE_PAR_SLICE_ATTACK",
                time("slice_float_1", 0.717_839_8, 0.363_914_76),
            ),
            (
                "$ENGINE_PAR_SLICE_RELEASE",
                time("slice_float_2", 0.916_290_8, 0.297_585_8),
            ),
        ]),
        8 => out.extend([
            (
                "$ENGINE_PAR_ENVELOPE_ORDER",
                float("pro_float_2").map(ir::unit_read),
            ),
            ("$ENGINE_PAR_FORMANT_SHIFT", bipolar("pro_float_1")),
        ]),
        _ => {}
    }
    out.into_iter()
        .filter_map(|(name, v)| Some((-1, ir::held_key(name), v?)))
        .collect()
}
