//! KSP engine-parameter writes that land on a Time Machine source record.
//!
//! Measured on Kontakt 8 with `tools/kontakt-oracle/tm_sweep.py`: a script's
//! `set_engine_par(par, raw, group, -1, 0)` at init changes the stored source
//! record that Kontakt saves, so the value the engine plays is the decoded write.
//! Each law below is the one measured for that stored field; the decode is the
//! shared engine law, so the format only names the field.
//!
//! Measured fields (`mode`: stored source mode):
//! - `SPEED` -> speed, all modes (TM1 mode 2, TM2 mode 5, Pro mode 8).
//! - `SMOOTH` -> `machine_float_1`, TM1 only.
//! - `GRAIN_LENGTH` -> `machine_float_2` (ms), TM1 only.
//! - `FORMANT_SHIFT` -> `pro_float_1`, Pro only.
//! - `TMPRO_KEEP_FORMANTS` -> `pro_flag_2`, Pro only.
//! - `TRANSIENT_SIZE` -> `dfd_integer_1` (source positions), TM2 only.
//! - `TM_LEGATO` -> `common_flag_11`, stored by all three kinds; no IR field consumes it yet.
//!
//! TM2 `SMOOTH` / `GRAIN_LENGTH` and Pro `SMOOTH` / `GRAIN_LENGTH` wrote no stored
//! field in the sweep, so they are reported as unapplied rather than guessed.

use sampler_core::EngineParameterLaw as Law;
use sampler_ir as ir;

/// Stored speed for raw 0, 0.05, ..., 1.0 (1e6 scale); 1.0 at 0.5 is the default.
const SPEED: [f64; 21] = [
    0., 0.11573704, 0.26411438, 0.43185267, 0.5996453, 0.7480983, 0.86338854, 0.9402863, 0.9819788,
    0.99773204, 1., 1.0022726, 1.018304, 1.0629349, 1.1547296, 1.3212686, 1.6095178, 2.1074643,
    2.992968, 4.6530685, 8.,
];

/// Stored TM1 grain in milliseconds, same grid as [`SPEED`].
const GRAIN_MS: [f64; 21] = [
    0.3, 0.7801707, 1.3605866, 2.0621753, 2.910236, 3.9353442, 5.174465, 6.6722775, 8.482789,
    10.671275, 13.316657, 16.514317, 20.379545, 25.051718, 30.699295, 37.52593, 45.77776,
    55.752308, 67.80926, 82.3833, 100.,
];

/// Stored TM2 transient size (source positions) for raw 0, 0.1, ..., 1.0; the stored field
/// is the full table, measured at the 0.1 grid (the default 2048 is not a point of this curve).
const TRANSIENT_SIZE: [f64; 11] = [
    100., 149., 220., 326., 482., 712., 1052., 1554., 2294., 3387., 5000.,
];

/// Stored `timing_value` (cycle length in beats) for raw `SPEED_UNIT` 0..=21, on
/// every kind (Kontakt 8, `tools/kontakt-oracle/tm_sweep.py --pars SPEED_UNIT`,
/// raw 0..=25). `None` where the sweep left the timing fields at baseline (free).
/// The values are 4 / k for k = 1, 1.5, 2, 3, 4, 6, 8, 12, 16, 24, 32, 64, 256 (raw
/// 5, 6, 7, 8, 9, 10, 12, 13, 15, 16, 17, 19, 21). Raw 22 sets `common_flag_16`, which
/// has no consumer, so it is reported.
const SPEED_UNIT_BEATS: [Option<f64>; 22] = [
    None,
    None,
    None,
    None,
    None,
    Some(4.),
    Some(2.6666667),
    Some(2.),
    Some(1.3333334),
    Some(1.),
    Some(0.6666667),
    None,
    Some(0.5),
    Some(0.33333334),
    None,
    Some(0.25),
    Some(0.16666667),
    Some(0.125),
    Some(0.083333336),
    Some(0.0625),
    Some(0.041666668),
    Some(0.015625),
];

/// Apply every measured write for `stretch`'s kind. `write` reads the raw value
/// of a named engine parameter from the script. Returns the parameters that were
/// written but have no stored field for this kind, so the caller can report them.
pub(crate) fn apply(
    stretch: &mut ir::TimeStretch,
    write: impl Fn(&str) -> Option<i32>,
) -> Vec<&'static str> {
    let tm1 = stretch.kind == ir::StretchKind::TimeMachine1;
    let pro = stretch.kind == ir::StretchKind::TimeMachinePro;
    let tm2 = stretch.kind == ir::StretchKind::TimeMachine2;
    let mut unapplied = Vec::new();
    if let Some(raw) = write("ENGINE_PAR_SPEED") {
        stretch.speed = Law::Table(&SPEED).decode(raw) as f32;
        // Same base-rate floor as the record path (TM2 and Pro).
        if !tm1 {
            stretch.speed = stretch.speed.max(0.01);
        }
    }
    if let Some(raw) = write("ENGINE_PAR_SPEED_UNIT") {
        stretch.sync_beats = usize::try_from(raw)
            .ok()
            .and_then(|i| SPEED_UNIT_BEATS.get(i).copied().flatten())
            .map(|beats| beats as f32);
        if raw == 22 {
            unapplied.push("ENGINE_PAR_SPEED_UNIT");
        }
    }
    if let Some(raw) = write("ENGINE_PAR_SMOOTH") {
        if tm1 {
            stretch.smooth = Law::Linear { low: 0., high: 1. }.decode(raw) as f32;
        } else {
            unapplied.push("ENGINE_PAR_SMOOTH");
        }
    }
    if let Some(raw) = write("ENGINE_PAR_GRAIN_LENGTH") {
        if tm1 {
            stretch.grain_ms = Law::Table(&GRAIN_MS).decode(raw) as f32;
        } else {
            unapplied.push("ENGINE_PAR_GRAIN_LENGTH");
        }
    }
    if let Some(raw) = write("ENGINE_PAR_FORMANT_SHIFT") {
        if pro {
            stretch.formant_shift = Law::Linear { low: -1., high: 1. }.decode(raw) as f32;
        } else {
            unapplied.push("ENGINE_PAR_FORMANT_SHIFT");
        }
    }
    if let Some(raw) = write("ENGINE_PAR_TMPRO_KEEP_FORMANTS") {
        if pro {
            stretch.keep_formants = Law::Switch.decode(raw) != 0.;
        } else {
            unapplied.push("ENGINE_PAR_TMPRO_KEEP_FORMANTS");
        }
    }
    if let Some(raw) = write("ENGINE_PAR_TRANSIENT_SIZE") {
        if tm2 {
            stretch.transient_size = Law::Table(&TRANSIENT_SIZE).decode(raw).round() as u32;
        } else {
            unapplied.push("ENGINE_PAR_TRANSIENT_SIZE");
        }
    }
    // `TM_LEGATO` writes a stored flag on every kind, but the IR has no field for it.
    if write("ENGINE_PAR_TM_LEGATO").is_some() {
        unapplied.push("ENGINE_PAR_TM_LEGATO");
    }
    unapplied
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tm(kind: ir::StretchKind) -> ir::TimeStretch {
        ir::TimeStretch::new(kind, 1.0)
    }

    /// Native vectors: raw write -> stored value, as read back from Kontakt 8's saved records.
    #[test]
    fn measured_points_decode_exactly() {
        let mut speed = tm(ir::StretchKind::TimeMachine1);
        for (raw, want) in [
            (0, 0.),
            (250_000, 0.7480983),
            (750_000, 1.3212686),
            (1_000_000, 8.),
        ] {
            apply(&mut speed, |n| (n == "ENGINE_PAR_SPEED").then_some(raw));
            assert!(
                (speed.speed - want).abs() <= 1e-6 * want.max(1.),
                "speed at {raw}: {}",
                speed.speed
            );
        }
        let mut grain = tm(ir::StretchKind::TimeMachine1);
        for (raw, want) in [
            (0, 0.3),
            (250_000, 3.9353442),
            (500_000, 13.316657),
            (1_000_000, 100.),
        ] {
            apply(&mut grain, |n| {
                (n == "ENGINE_PAR_GRAIN_LENGTH").then_some(raw)
            });
            assert!(
                (grain.grain_ms - want).abs() <= 1e-4 * want,
                "grain at {raw}: {}",
                grain.grain_ms
            );
        }
    }

    /// One script with every write: each applies to the fields measured for its kind,
    /// the rest are reported, and TM2/Pro keep the base-rate floor.
    #[test]
    fn writes_apply_per_kind() {
        let writes = |n: &str| match n {
            "ENGINE_PAR_SPEED" => Some(0),
            "ENGINE_PAR_SMOOTH" => Some(250_000),
            "ENGINE_PAR_GRAIN_LENGTH" => Some(500_000),
            "ENGINE_PAR_FORMANT_SHIFT" => Some(250_000),
            "ENGINE_PAR_TMPRO_KEEP_FORMANTS" => Some(0),
            _ => None,
        };
        let mut tm1 = tm(ir::StretchKind::TimeMachine1);
        assert_eq!(
            apply(&mut tm1, writes),
            vec!["ENGINE_PAR_FORMANT_SHIFT", "ENGINE_PAR_TMPRO_KEEP_FORMANTS"]
        );
        assert_eq!((tm1.speed, tm1.smooth), (0., 0.25));
        assert!((tm1.grain_ms - 13.316657).abs() < 1e-4);

        let mut pro = tm(ir::StretchKind::TimeMachinePro);
        assert_eq!(
            apply(&mut pro, writes),
            vec!["ENGINE_PAR_SMOOTH", "ENGINE_PAR_GRAIN_LENGTH"]
        );
        assert_eq!(pro.speed, 0.01);
        assert!((pro.formant_shift + 0.5).abs() < 1e-6);
        assert!(!pro.keep_formants);
    }

    /// TM2 transient size follows the measured stored curve; TM1 and Pro have no such field.
    /// Measured `SPEED_UNIT` table: raw 0..=4 and 11, 14 are free, raw 22 is reported.
    #[test]
    fn speed_unit_selects_the_measured_beat_cycle() {
        let mut s = tm(ir::StretchKind::TimeMachine1);
        for (raw, want) in [(5, Some(4.)), (6, Some(2.6666667)), (12, Some(0.5)), (21, Some(0.015625)), (0, None), (11, None)] {
            let unapplied = apply(&mut s, |n| (n == "ENGINE_PAR_SPEED_UNIT").then_some(raw));
            assert!(unapplied.is_empty(), "raw {raw} reported");
            match want {
                Some(w) => assert!(s.sync_beats.is_some_and(|b| (f64::from(b) - w).abs() < 1e-6), "raw {raw}"),
                None => assert_eq!(s.sync_beats, None, "raw {raw}"),
            }
        }
        let unapplied = apply(&mut s, |n| (n == "ENGINE_PAR_SPEED_UNIT").then_some(22));
        assert_eq!(unapplied, vec!["ENGINE_PAR_SPEED_UNIT"]);
    }

    #[test]
    fn transient_size_is_measured_on_tm2_only() {
        let mut tm2 = tm(ir::StretchKind::TimeMachine2);
        for (raw, want) in [(0, 100), (800_000, 2294), (1_000_000, 5000)] {
            apply(&mut tm2, |n| {
                (n == "ENGINE_PAR_TRANSIENT_SIZE").then_some(raw)
            });
            assert_eq!(tm2.transient_size, want, "transient at {raw}");
        }
        let mut pro = tm(ir::StretchKind::TimeMachinePro);
        let unapplied = apply(&mut pro, |n| {
            (n == "ENGINE_PAR_TRANSIENT_SIZE").then_some(500_000)
        });
        assert_eq!(unapplied, vec!["ENGINE_PAR_TRANSIENT_SIZE"]);
    }
}
