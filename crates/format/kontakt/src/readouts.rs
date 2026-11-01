//! The text `get_engine_par_disp` answers for the parameters scripts show,
//! as Kontakt 8.13.1 printed it to an oracle probe (2026-10-29): ANALOG
//! STRINGS' own slots set to 0, .1, .25, .5, .75 and 1, each answer read
//! back from a script title (scratch/laneB/disp, tools/kontakt-oracle dump).

use kontra_ir as ir;

use crate::script_knobs::{
    COMP_ATTACK_MS, COMP_RELEASE_MS, LIMITER_RELEASE_MS, REVERB_PREDELAY_MS,
};

/// The readouts of `$ENGINE_PAR_*` names, by name.
// The chorus depth and JMP amp knobs read 0.0..100.0 too, but KONTRA
// carries neither module, so they have no row (scratch/laneB/disp).
// ponytail: the cutoff law is the one filter measured (ANALOG's); measure
// each filter kind when a library shows another.
pub(crate) fn readouts() -> Vec<(&'static str, ir::Readout)> {
    use ir::Form::{Decimal, Padded, Pan, Ratio, Whole, Wide};
    let lin = |lo, hi| ir::Law::Linear { lo, hi };
    let exp = |lo, hi| ir::Law::Exp { lo, hi };
    let curve = |points: [(f32, f32, f32); 2]| ir::Law::Breakpoints(points.into());
    let percent = [
        "$ENGINE_PAR_RESONANCE",
        "$ENGINE_PAR_DAMPING",
        "$ENGINE_PAR_DRIVE",
        "$ENGINE_PAR_DL_FEEDBACK",
        "$ENGINE_PAR_DL_PAN",
        "$ENGINE_PAR_RV_SIZE",
        "$ENGINE_PAR_PH_DEPTH",
        "$ENGINE_PAR_PH_FEEDBACK",
    ];
    let none = [None; 2];
    let mut rows: Vec<_> = percent
        .into_iter()
        .map(|n| (n, lin(0.0, 100.0), Decimal, none))
        .collect();
    rows.extend([
        // MIDI note 0 (8.176 Hz) to note 145, a semitone per 1/145.
        ("$ENGINE_PAR_CUTOFF", exp(8.176, 35_485.0), Padded, none),
        ("$ENGINE_PAR_SHAPE", lin(-100.0, 100.0), Decimal, none),
        (
            "$ENGINE_PAR_STEREO",
            lin(-100.0, 100.0),
            Decimal,
            [Some("mono"), None],
        ),
        ("$ENGINE_PAR_STEREO_PAN", lin(0.0, 1.0), Pan, none),
        ("$ENGINE_PAR_ATK_CURVE", lin(-100.0, 100.0), Whole, none),
        (
            "$ENGINE_PAR_BITS",
            lin(0.0, 16.0),
            Padded,
            [Some("  1.0"), Some("Off")],
        ),
        (
            "$ENGINE_PAR_FREQUENCY",
            lin(0.0, 48_000.0),
            Padded,
            [Some(" 50.0"), Some("Off")],
        ),
        ("$ENGINE_PAR_CH_SPEED", exp(0.05, 8.0), Padded, none),
        ("$ENGINE_PAR_PH_SPEED", exp(0.05, 8.0), Padded, none),
        ("$ENGINE_PAR_CH_PHASE", lin(0.0, 90.0), Wide, none),
        (
            "$ENGINE_PAR_COMP_ATTACK",
            curve(COMP_ATTACK_MS),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_COMP_DECAY",
            curve(COMP_RELEASE_MS),
            Padded,
            none,
        ),
        ("$ENGINE_PAR_THRESHOLD", lin(-68.0, -3.0), Padded, none),
        ("$ENGINE_PAR_RATIO", exp(1.0, 25.0), Ratio, none),
        ("$ENGINE_PAR_LIM_IN_GAIN", lin(-24.0, 24.0), Padded, none),
        (
            "$ENGINE_PAR_LIM_RELEASE",
            curve(LIMITER_RELEASE_MS),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_RV_PREDELAY",
            curve(REVERB_PREDELAY_MS),
            Padded,
            none,
        ),
        // An LFO's delay: the envelope stage law, 0..10 s.
        (
            "$ENGINE_PAR_LFO_DELAY",
            curve([(0.0, 0.0, 5001f32.ln()), (1.0, 10_000.0, 0.0)]),
            Padded,
            none,
        ),
        ("$ENGINE_PAR_FREQ1", exp(20.0, 20_000.0), Padded, none),
        ("$ENGINE_PAR_FREQ2", exp(20.0, 20_000.0), Padded, none),
        ("$ENGINE_PAR_FREQ3", exp(20.0, 20_000.0), Padded, none),
        ("$ENGINE_PAR_GAIN1", lin(-18.0, 18.0), Padded, none),
        ("$ENGINE_PAR_GAIN2", lin(-18.0, 18.0), Padded, none),
        ("$ENGINE_PAR_GAIN3", lin(-18.0, 18.0), Padded, none),
    ]);
    rows.into_iter()
        .map(|(name, law, form, ends)| (name, ir::Readout { law, form, ends }))
        .collect()
}
