//! The text `get_engine_par_disp` answers for the parameters scripts show,
//! as Kontakt 8.13.1 printed it to an oracle probe (2026-10-29): ANALOG
//! STRINGS' own slots set to 0, .1, .25, .5, .75 and 1, each answer read
//! back from a script title (scratch/laneB/disp, tools/kontakt-oracle dump).

use kontra_ir as ir;

use crate::script_knobs::{
    COMP_ATTACK_MS, COMP_RELEASE_MS, LIMITER_RELEASE_MS, REVERB_PREDELAY_MS,
};

/// The readouts of `$ENGINE_PAR_*` names, by name.
// A control of a module KONTRA has no processor for (the chorus depth, the
// JMP amp knobs) is held by its slot and shows this text (engine_pars).
// ponytail: the cutoff law is the one filter measured (ANALOG's); measure
// each filter kind when a library shows another.
pub(crate) fn readouts() -> Vec<(&'static str, ir::Readout)> {
    use ir::Form::{
        Blank, Decimal, Hundredths, Menu, Padded, Pan, Rate, Ratio, Times, Whole, Wide, WideWhole,
    };
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
        "$ENGINE_PAR_CH_DEPTH",
        "$ENGINE_PAR_JMP_PREAMP",
        "$ENGINE_PAR_JMP_BASS",
        "$ENGINE_PAR_JMP_MID",
        "$ENGINE_PAR_JMP_TREBLE",
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
        // Morphology's wavetable group 932 and ANALOG's LFO at 0, .1, .25,
        // .5, .75 and 1 (Kontakt 8.13.1, 2026-11-11, scratch/laneB/disp2).
        ("$ENGINE_PAR_WT_POSITION", lin(0.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_WT_FORM", lin(0.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_WT_FORM2", lin(0.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_WT_MOD_AMOUNT", lin(0.0, 100.0), Decimal, none),
        (
            "$ENGINE_PAR_WT_INHARMONIC",
            lin(-100.0, 100.0),
            Decimal,
            none,
        ),
        (
            "$ENGINE_PAR_WT_MOD_TUNE",
            lin(-36.0, 36.0),
            Hundredths,
            none,
        ),
        ("$ENGINE_PAR_LFO_PHASE", lin(0.0, 360.0), WideWhole, none),
        // Una Corda's Solid G-EQ, bus 0 slot 7 (Kontakt 8.13.1, 2026-11-12,
        // scratch/lane-scripts/probes/b2): every band's gain ±20 dB; the
        // bell switches show nothing.
        ("$ENGINE_PAR_SEQ_LF_GAIN", lin(-20.0, 20.0), Padded, none),
        ("$ENGINE_PAR_SEQ_LMF_GAIN", lin(-20.0, 20.0), Padded, none),
        ("$ENGINE_PAR_SEQ_HMF_GAIN", lin(-20.0, 20.0), Padded, none),
        ("$ENGINE_PAR_SEQ_HF_GAIN", lin(-20.0, 20.0), Padded, none),
        ("$ENGINE_PAR_SEQ_LF_BELL", lin(0.0, 1.0), Blank, none),
        ("$ENGINE_PAR_SEQ_HF_BELL", lin(0.0, 1.0), Blank, none),
    ]);
    // A fresh unit of each module in an empty instrument's insert slot
    // (Kontakt 8.13.1, 2026-11-25, docs/re/kontakt-bindings/engine-par-disp.tsv):
    // switches and modes show nothing; Raum's high cut reads "Off" at 1 and
    // Psyche Delay's time 5 ms at 0.
    let blank = [
        "$ENGINE_PAR_RAUM_TYPE",
        "$ENGINE_PAR_RAUM_FREEZE",
        "$ENGINE_PAR_RAUM_PREDELAY_UNIT",
        "$ENGINE_PAR_RAUM_SPARSE",
        "$ENGINE_PAR_RDL_TYPE",
        "$ENGINE_PAR_RDL_TIME_UNIT",
        "$ENGINE_PAR_RDL_NOISE",
        "$ENGINE_PAR_RDL_PINGPONG",
        "$ENGINE_PAR_RDL_PINGPONG_FLIP",
        "$ENGINE_PAR_RDL_STEREO",
        "$ENGINE_PAR_RDL_DENSE",
        "$ENGINE_PAR_PSYDL_TIME_UNIT",
        "$ENGINE_PAR_PSYDL_DETUNE_STEREO",
        "$ENGINE_PAR_PSYDL_REVERSE",
        "$ENGINE_PAR_PSYDL_REVERSE_STEREO",
        "$ENGINE_PAR_CHORAL_MODE",
        "$ENGINE_PAR_CHORAL_SCATTER",
        "$ENGINE_PAR_CHORAL_INVERT_PHASE",
        "$ENGINE_PAR_TP_QUALITY",
        "$ENGINE_PAR_JMP_HIGAIN",
        "$ENGINE_PAR_JMP_MONO",
        "$ENGINE_PAR_BASSPRO_BRIGHT",
        "$ENGINE_PAR_BASSPRO_GEQ",
        "$ENGINE_PAR_BASSPRO_MONO",
        "$ENGINE_PAR_BASSPRO_ULTRAHI",
        "$ENGINE_PAR_BASSPRO_ULTRALO",
        "$ENGINE_PAR_DIRT_MODEA",
        "$ENGINE_PAR_DIRT_MODEB",
        "$ENGINE_PAR_DIRT_ROUTING",
        "$ENGINE_PAR_DIRT_SAFETYA",
        "$ENGINE_PAR_DIRT_SAFETYB",
        "$ENGINE_PAR_KOLOR_BOOST",
        "$ENGINE_PAR_KOLOR_MODE",
        "$ENGINE_PAR_V5_BRIGHT",
        "$ENGINE_PAR_V5_CRUNCH",
        "$ENGINE_PAR_V5_HIGAIN",
        "$ENGINE_PAR_V5_LEADCHANNEL",
        "$ENGINE_PAR_V5_MONO",
        "$ENGINE_PAR_SF100_BRIGHT",
        "$ENGINE_PAR_SF100_CRUNCH",
        "$ENGINE_PAR_SF100_MONO",
        "$ENGINE_PAR_SF100_OVERDRIVE",
        "$ENGINE_PAR_TDL_PREDELAY_L_UNIT",
        "$ENGINE_PAR_TDL_PREDELAY_R_UNIT",
        "$ENGINE_PAR_TDL_TIME_L_UNIT",
        "$ENGINE_PAR_TDL_TIME_R_UNIT",
        "$ENGINE_PAR_RINGMOD_FAST_MODE",
        "$ENGINE_PAR_RINGMOD_LFO_RATE_UNIT",
        "$ENGINE_PAR_RINGMOD_LFO_WAVE",
    ];
    rows.extend(blank.map(|n| (n, lin(0.0, 1.0), Blank, none)));
    let unit_percent = [
        "$ENGINE_PAR_RAUM_SIZE",
        "$ENGINE_PAR_RAUM_DAMPING",
        "$ENGINE_PAR_RAUM_DIFFUSION",
        "$ENGINE_PAR_RAUM_FEEDBACK",
        "$ENGINE_PAR_RAUM_MOD",
        "$ENGINE_PAR_RAUM_REVERB",
        "$ENGINE_PAR_RDL_AMOUNT",
        "$ENGINE_PAR_RDL_SATURATION",
        "$ENGINE_PAR_RDL_DEPTH",
        "$ENGINE_PAR_RDL_DUCKING_RELEASE",
        "$ENGINE_PAR_RDL_DUCKING_SENSE",
        "$ENGINE_PAR_RDL_TAPEAGE",
        "$ENGINE_PAR_RDL_FLUTTER",
        "$ENGINE_PAR_RDL_SHUFFLE",
        "$ENGINE_PAR_PSYDL_FEEDBACK",
        "$ENGINE_PAR_PSYDL_CROSS_FEEDBACK",
        "$ENGINE_PAR_PSYDL_LR_OFFSET",
        "$ENGINE_PAR_CHORAL_FEEDBACK",
        "$ENGINE_PAR_CHORAL_WIDTH",
        "$ENGINE_PAR_CHORAL_MIX",
        "$ENGINE_PAR_JMP_MASTER",
        "$ENGINE_PAR_JMP_PRESENCE",
        "$ENGINE_PAR_DIRT_AMOUNTA",
        "$ENGINE_PAR_DIRT_AMOUNTB",
        "$ENGINE_PAR_DIRT_BIASA",
        "$ENGINE_PAR_DIRT_BIASB",
        "$ENGINE_PAR_DIRT_BLEND",
        "$ENGINE_PAR_DIRT_DRIVEA",
        "$ENGINE_PAR_DIRT_DRIVEB",
        "$ENGINE_PAR_DIRT_MIX",
        "$ENGINE_PAR_KOLOR_BASS_SAVER",
        "$ENGINE_PAR_KOLOR_DRIVE",
        "$ENGINE_PAR_KOLOR_MID_Q",
        "$ENGINE_PAR_KOLOR_MIX",
        "$ENGINE_PAR_V5_BASS",
        "$ENGINE_PAR_V5_MID",
        "$ENGINE_PAR_V5_POSTGAIN",
        "$ENGINE_PAR_V5_PREGAINLEAD",
        "$ENGINE_PAR_V5_PREGAINRHYTHM",
        "$ENGINE_PAR_V5_PRESENCE",
        "$ENGINE_PAR_V5_RESONANCE",
        "$ENGINE_PAR_V5_TREBLE",
        "$ENGINE_PAR_SF100_BASS",
        "$ENGINE_PAR_SF100_DEPTH",
        "$ENGINE_PAR_SF100_MASTER",
        "$ENGINE_PAR_SF100_MID",
        "$ENGINE_PAR_SF100_NORMAL_GAIN",
        "$ENGINE_PAR_SF100_OD_GAIN",
        "$ENGINE_PAR_SF100_PRESENCE",
        "$ENGINE_PAR_SF100_TREBLE",
        "$ENGINE_PAR_TDL_CROSS_FEEDBACK",
        "$ENGINE_PAR_TDL_FEEDBACK_L",
        "$ENGINE_PAR_TDL_FEEDBACK_R",
        "$ENGINE_PAR_TDL_LEVEL_L",
        "$ENGINE_PAR_TDL_LEVEL_R",
        "$ENGINE_PAR_RINGMOD_EDGE",
        "$ENGINE_PAR_RINGMOD_FM",
        "$ENGINE_PAR_RINGMOD_LFO_AMOUNT",
        "$ENGINE_PAR_RINGMOD_RING",
    ];
    rows.extend(unit_percent.map(|n| (n, lin(0.0, 100.0), Decimal, none)));
    rows.extend([
        ("$ENGINE_PAR_RAUM_LOWSHELF", lin(0.0, -24.0), Padded, none),
        (
            "$ENGINE_PAR_RAUM_PREDELAY",
            kontra_law::kontakt::RAUM_PREDELAY_MS,
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_RAUM_HIGHCUT",
            kontra_law::kontakt::RAUM_HIGH_CUT_HZ,
            Padded,
            [None, Some("Off")],
        ),
        ("$ENGINE_PAR_RDL_TIME", lin(0.0, 2000.0), Padded, none),
        ("$ENGINE_PAR_RDL_FEEDBACK", lin(0.0, 120.0), Padded, none),
        ("$ENGINE_PAR_RDL_LOWCUT", exp(10.0, 1000.0), Padded, none),
        (
            "$ENGINE_PAR_RDL_HIGHCUT",
            lin(200.0, 20_000.0),
            Padded,
            none,
        ),
        ("$ENGINE_PAR_RDL_RATE", exp(0.1, 10.0), Padded, none),
        ("$ENGINE_PAR_RDL_MODULATION", lin(0.0, 10.0), Padded, none),
        (
            "$ENGINE_PAR_RDL_WIDTH",
            lin(-100.0, 100.0),
            Decimal,
            [Some("mono"), None],
        ),
        ("$ENGINE_PAR_RDL_PAN", lin(0.0, 1.0), Pan, none),
        ("$ENGINE_PAR_RDL_LR_OFFSET", lin(0.0, 25.0), Decimal, none),
        (
            "$ENGINE_PAR_RDL_DUCKING_AMOUNT",
            lin(0.0, 60.0),
            Decimal,
            none,
        ),
        ("$ENGINE_PAR_RDL_SIZE", lin(1.0, 10.0), Padded, none),
        ("$ENGINE_PAR_RDL_FEEL", lin(-100.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_RDL_ACCENT", lin(-100.0, 100.0), Decimal, none),
        (
            "$ENGINE_PAR_PSYDL_TIME",
            lin(0.0, 2000.0),
            Padded,
            [Some("  5.0"), None],
        ),
        ("$ENGINE_PAR_CHORAL_VOICES", lin(1.0, 3.0), Padded, none),
        ("$ENGINE_PAR_CHORAL_AMOUNT", lin(1.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_TP_GAIN", lin(0.0, 48.0), Padded, none),
        ("$ENGINE_PAR_TP_WARMTH", lin(-3.0, 3.0), Padded, none),
        (
            "$ENGINE_PAR_TP_HF_ROLLOFF",
            lin(12_000.0, 20_000.0),
            Padded,
            none,
        ),
        ("$ENGINE_PAR_BASSPRO_BASS", lin(-15.0, 15.0), Wide, none),
        ("$ENGINE_PAR_BASSPRO_TREBLE", lin(-19.0, 19.0), Wide, none),
        ("$ENGINE_PAR_BASSPRO_DRIVE", lin(0.0, 18.0), Wide, none),
        (
            "$ENGINE_PAR_BASSPRO_MIDFREQ",
            exp(200.0, 3200.0),
            Padded,
            none,
        ),
        ("$ENGINE_PAR_DIRT_TILTA", lin(-100.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_DIRT_TILTB", lin(-100.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_KOLOR_BASS", lin(-20.0, 20.0), Padded, none),
        ("$ENGINE_PAR_KOLOR_MID", lin(-20.0, 20.0), Padded, none),
        ("$ENGINE_PAR_KOLOR_TREBLE", lin(-20.0, 20.0), Padded, none),
        (
            "$ENGINE_PAR_KOLOR_BOOST_LEVEL",
            lin(0.0, 12.0),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_KOLOR_MID_FREQ",
            lin(80.0, 2500.0),
            Padded,
            none,
        ),
        ("$ENGINE_PAR_KOLOR_HPF", exp(10.0, 20_480.0), Padded, none),
        ("$ENGINE_PAR_KOLOR_LPF", exp(10.0, 20_480.0), Padded, none),
        ("$ENGINE_PAR_TDL_PREDELAY_L", lin(0.0, 2000.0), Padded, none),
        ("$ENGINE_PAR_TDL_PREDELAY_R", lin(0.0, 2000.0), Padded, none),
        ("$ENGINE_PAR_TDL_WIDTH", lin(-100.0, 100.0), Decimal, none),
        (
            "$ENGINE_PAR_RINGMOD_LFO_RATE",
            exp(0.031_25, 32.0),
            Rate,
            none,
        ),
        ("$ENGINE_PAR_CHORAL_DELAY", lin(0.1, 5.1), Times, none),
        (
            "$ENGINE_PAR_RDL_QUALITY",
            lin(0.0, 1.0),
            Menu(&["Crunch", "Low", "Medium", "High"]),
            none,
        ),
        (
            "$ENGINE_PAR_RDL_BBDTYPE",
            lin(0.0, 1.0),
            Menu(&["Grunge", "Dark", "Warm", "Clean"]),
            none,
        ),
        (
            "$ENGINE_PAR_RINGMOD_FREQUENCY",
            exp(31.25, 4000.0),
            Padded,
            none,
        ),
    ]);
    // The Convolution's knobs read what they set: the measured knob laws
    // (impulse.rs). ponytail: the print forms are unmeasured.
    rows.extend(
        crate::impulse::script_knobs()
            .into_iter()
            .map(|(name, _, law)| {
                let form = if name.contains("LENGTH") {
                    Hundredths
                } else {
                    Padded
                };
                (name, law, form, none)
            }),
    );
    rows.into_iter()
        .map(|(name, law, form, ends)| (name, ir::Readout { law, form, ends }))
        .collect()
}
