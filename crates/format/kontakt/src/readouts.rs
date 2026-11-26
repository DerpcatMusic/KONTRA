//! The text `get_engine_par_disp` answers for the parameters scripts show,
//! as Kontakt 8.13.1 printed it to an oracle probe (2026-10-29): ANALOG
//! STRINGS' own slots set to 0, .1, .25, .5, .75 and 1, each answer read
//! back from a script title (scratch/laneB/disp, tools/kontakt-oracle dump).

use kontra_ir as ir;

use crate::script_knobs::{
    COMP_ATTACK_MS, COMP_RELEASE_MS, LIMITER_RELEASE_MS, REVERB_PREDELAY_MS,
};

/// The readouts of `$ENGINE_PAR_*` names, by name.
// A control of a module KONTRA has no processor for (the JMP amp knobs) is held by its slot and shows this text (engine_pars).
// Default `$ENGINE_PAR_CUTOFF` is AR LP2; per-subtype filter readouts live in [`subtype_readouts`].
pub(crate) fn readouts() -> Vec<(&'static str, ir::Readout)> {
    use ir::Form::{
        Blank, Bpm, Decimal, Hundredths, Menu, NarrowWhole, Note, Padded, Pan, Rate, Ratio, Times,
        Whole, Wide, WideWhole,
    };
    use kontra_law::FormulaId::{
        AttackCurvePercent, BassProGainDb, BassProGeqDb, BassProGeqVolumeDb, BassProMasterDb,
        BassProMidDb, BiteBits, BiteFrequencyHz, BusCompMakeupDb, BusCompThresholdDb, ChoralRateHz,
        EnvelopeOrder, EppTremoloRateHz, FlairPitchNote, FlairRateHz, FormantShiftPercent,
        FreakBandPassHz, FreakCarrierHz, FreakFrequencyHz, LofiNoiseDb, PhasisCenterHz,
        PhasisNotches, PhasisRateHz, PsycheDetuneCents, PsychePitchSemitones, RaumDecayMs,
        RaumRateHz, RotatorFast, RotatorMixDb, SimpleFilterCutoffHz, SliceAttackMs, SliceReleaseMs,
        SolidEqHfHz, SolidEqHmfHz, SolidEqHpHz, SolidEqLfHz, SolidEqLmfHz, SolidEqLpHz, SolidEqQ,
        StereoTuneDriftPercent, TmGrainMs, TransLimReleaseMs, TransStretchKey, TwinDelayTimeMs,
    };
    let lin = |lo, hi| ir::Law::Linear { lo, hi };
    let exp = |lo, hi| ir::Law::Exp { lo, hi };
    let curve = |points: [(f32, f32, f32); 2]| ir::Law::Breakpoints(points.into());
    let formula = ir::Law::Formula;
    let percent = [
        // Time Machine's smooth (descriptor 2).
        "$ENGINE_PAR_SMOOTH",
        "$ENGINE_PAR_RESONANCE",
        "$ENGINE_PAR_DAMPING",
        "$ENGINE_PAR_DRIVE",
        "$ENGINE_PAR_DL_FEEDBACK",
        "$ENGINE_PAR_DL_PAN",
        "$ENGINE_PAR_RV_SIZE",
        "$ENGINE_PAR_PH_DEPTH",
        "$ENGINE_PAR_PH_FEEDBACK",
        "$ENGINE_PAR_CH_DEPTH",
        "$ENGINE_PAR_FL_DEPTH",
        "$ENGINE_PAR_FL_FEEDBACK",
        "$ENGINE_PAR_GATER_MIX",
        "$ENGINE_PAR_GATER_NOISE",
        "$ENGINE_PAR_GATER_SHAPE",
        "$ENGINE_PAR_WOWFLUTTER_WOW",
        "$ENGINE_PAR_WOWFLUTTER_FLUTTER",
        "$ENGINE_PAR_WOWFLUTTER_SCRAPE",
        "$ENGINE_PAR_WOWFLUTTER_SPEED",
        "$ENGINE_PAR_WOWFLUTTER_STEREO",
        "$ENGINE_PAR_WOWFLUTTER_AGE",
        "$ENGINE_PAR_WOWFLUTTER_SATURATION",
        "$ENGINE_PAR_WOWFLUTTER_MIX",
        "$ENGINE_PAR_SCOMP_MIX",
        "$ENGINE_PAR_DL_DAMPING",
        "$ENGINE_PAR_RV2_SIZE",
        "$ENGINE_PAR_RV2_DAMPING",
        "$ENGINE_PAR_RV2_MOD",
        "$ENGINE_PAR_RV2_DIFF",
        "$ENGINE_PAR_RV2_STEREO",
        "$ENGINE_PAR_RV_COLOUR",
        "$ENGINE_PAR_RV_STEREO",
        "$ENGINE_PAR_RV_DAMPING",
        "$ENGINE_PAR_JMP_PREAMP",
        "$ENGINE_PAR_JMP_BASS",
        "$ENGINE_PAR_JMP_MID",
        "$ENGINE_PAR_JMP_TREBLE",
        "$ENGINE_PAR_FILTER_BYPA",
        "$ENGINE_PAR_FILTER_BYPB",
        "$ENGINE_PAR_FILTER_BYPC",
        "$ENGINE_PAR_FILTER_RESB",
        "$ENGINE_PAR_FILTER_RESC",
        "$ENGINE_PAR_FORMANT_TALK",
        "$ENGINE_PAR_FORMANT_SHARP",
        "$ENGINE_PAR_FORMANT_SIZE",
        "$ENGINE_PAR_BEATMASHER_MIX",
        "$ENGINE_PAR_BEATMASHER_ROTATE",
        "$ENGINE_PAR_BEATSLICER_BUZZ",
        "$ENGINE_PAR_BEATSLICER_MIX",
        "$ENGINE_PAR_RG_GRAIN",
        "$ENGINE_PAR_RG_MIX",
        "$ENGINE_PAR_RG_PITCH",
        "$ENGINE_PAR_RG_SPEED",
        "$ENGINE_PAR_TS_AMOUNT",
        "$ENGINE_PAR_TS_MIX",
        "$ENGINE_PAR_TS_SIZE",
        "$ENGINE_PAR_STEREOTUNE_SPREAD",
        "$ENGINE_PAR_STEREOTUNE_MIX",
        "$ENGINE_PAR_CB_SIZE",
        "$ENGINE_PAR_CB_AIR",
        "$ENGINE_PAR_EPP_DRIVE",
        "$ENGINE_PAR_EPP_EQ_BASS",
        "$ENGINE_PAR_EPP_EQ_MID",
        "$ENGINE_PAR_EPP_EQ_TREBLE",
        "$ENGINE_PAR_EPP_PASSIVE_BASS",
        "$ENGINE_PAR_EPP_TREMOLO_AMOUNT",
        "$ENGINE_PAR_EPP_TREMOLO_WIDTH",
        "$ENGINE_PAR_CT_VOLUME",
        "$ENGINE_PAR_CT_DISTORTION",
        "$ENGINE_PAR_CT_FILTER",
        "$ENGINE_PAR_CT_BASS",
        "$ENGINE_PAR_CT_BALLS",
        "$ENGINE_PAR_CT_TREBLE",
        "$ENGINE_PAR_CT_TONE",
        "$ENGINE_PAR_CS_DISTORTION",
        "$ENGINE_PAR_CS_LEVEL",
        "$ENGINE_PAR_NOISECOLOR",
        "$ENGINE_PAR_SP_OFFSET_DISTANCE",
        "$ENGINE_PAR_SP_DIVERGENCE",
        "$ENGINE_PAR_PR_STEREO",
        "$ENGINE_PAR_EXP_FILTER_MORPH",
        "$ENGINE_PAR_EXP_FILTER_AMOUNT",
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
        // Beat Masher's gate `(x − 0.5)·200`; Stereo Tune's split
        // `(9x + 1)·100` and drift; Transpose Stretch's key, `%2.2f`.
        (
            "$ENGINE_PAR_BEATMASHER_GATE",
            lin(-100.0, 100.0),
            Decimal,
            none,
        ),
        ("$ENGINE_PAR_CB_TREBLE", lin(-100.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_CB_BASS", lin(-100.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_CS_LOW", lin(-100.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_CS_HIGH", lin(-100.0, 100.0), Decimal, none),
        (
            "$ENGINE_PAR_EPP_TREMOLO_RATE",
            formula(EppTremoloRateHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_NOISELEVEL",
            formula(LofiNoiseDb),
            Decimal,
            none,
        ),
        ("$ENGINE_PAR_SP_OFFSET_X", lin(-100.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_SP_OFFSET_Y", lin(-100.0, 100.0), Decimal, none),
        // Transparent Limiter: threshold `40x − 40`, ceiling `39.7x − 40` ([ref],
        // [ref]), release `500^x`.
        (
            "$ENGINE_PAR_TRANSLIM_THRESHOLD",
            lin(-40.0, 0.0),
            Decimal,
            none,
        ),
        (
            "$ENGINE_PAR_TRANSLIM_CEILING",
            lin(-40.0, -0.3),
            Decimal,
            none,
        ),
        // The source module (descriptors 2..18): Time Machine's formant `24x − 12`
        // semitones (bipolar), its grain, Beat Machine's slice times and
        // Time Machine 2's transient size (`e^(kx + c) − 2`), Pro's envelope order and
        // formant shift (2026-11-25, sweeps/a-ext.tsv).
        ("$ENGINE_PAR_FORMANT", lin(-12.0, 12.0), Decimal, none),
        ("$ENGINE_PAR_GRAIN_LENGTH", formula(TmGrainMs), Padded, none),
        (
            "$ENGINE_PAR_SLICE_ATTACK",
            formula(SliceAttackMs),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_SLICE_RELEASE",
            formula(SliceReleaseMs),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_ENVELOPE_ORDER",
            formula(EnvelopeOrder),
            NarrowWhole,
            none,
        ),
        (
            "$ENGINE_PAR_FORMANT_SHIFT",
            formula(FormantShiftPercent),
            NarrowWhole,
            none,
        ),
        (
            "$ENGINE_PAR_TRANSLIM_RELEASE",
            formula(TransLimReleaseMs),
            Decimal,
            none,
        ),
        // Surround Panner: LFE `72x − 36` dB, size `0.9x + 0.1`.
        (
            "$ENGINE_PAR_SP_OFFSET_AZIMUTH",
            lin(-180.0, 180.0),
            Wide,
            none,
        ),
        ("$ENGINE_PAR_SP_LFE_VOLUME", lin(-36.0, 36.0), Padded, none),
        ("$ENGINE_PAR_SP_SIZE", lin(10.0, 100.0), Decimal, none),
        // Plate Reverb ([ref] cases 0, 1, 9, 10): predelay `250x`, decay
        // `6400x + 556`, low shelf `5.625 − 34.375x`, damping `10851 − 4851x`.
        ("$ENGINE_PAR_PR_PREDELAY", lin(0.0, 250.0), Padded, none),
        ("$ENGINE_PAR_PR_DECAY", lin(556.0, 6_956.0), Padded, none),
        ("$ENGINE_PAR_PR_LOWSHELF", lin(5.625, -28.75), Padded, none),
        (
            "$ENGINE_PAR_PR_HIDAMP",
            lin(10_851.0, 6_000.0),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_SEQ_HP_FREQ",
            formula(SolidEqHpHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_SEQ_LP_FREQ",
            formula(SolidEqLpHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_LP_CUTOFF",
            formula(SimpleFilterCutoffHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_HP_CUTOFF",
            formula(SimpleFilterCutoffHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_STEREOTUNE_SPLIT",
            lin(100.0, 1_000.0),
            Decimal,
            none,
        ),
        (
            "$ENGINE_PAR_STEREOTUNE_DRIFT",
            formula(StereoTuneDriftPercent),
            Decimal,
            none,
        ),
        (
            "$ENGINE_PAR_TS_KEY",
            formula(TransStretchKey),
            Hundredths,
            none,
        ),
        // Filter: Gain `60·x − 30`, Shift `80·x` and Type
        // `x`, all `%3.1f`; EQ bandwidth `0.33333334 + 2.6666667·x`
        // through the `k` formatter [ref].
        ("$ENGINE_PAR_FILTER_GAIN", lin(-30.0, 30.0), Decimal, none),
        ("$ENGINE_PAR_FILTER_SHIFTB", lin(0.0, 80.0), Decimal, none),
        ("$ENGINE_PAR_FILTER_SHIFTC", lin(0.0, 80.0), Decimal, none),
        ("$ENGINE_PAR_FILTER_TYPEA", lin(0.0, 1.0), Decimal, none),
        ("$ENGINE_PAR_FILTER_TYPEB", lin(0.0, 1.0), Decimal, none),
        ("$ENGINE_PAR_FILTER_TYPEC", lin(0.0, 1.0), Decimal, none),
        ("$ENGINE_PAR_BW1", lin(0.333_333_34, 3.0), Padded, none),
        ("$ENGINE_PAR_BW2", lin(0.333_333_34, 3.0), Padded, none),
        ("$ENGINE_PAR_BW3", lin(0.333_333_34, 3.0), Padded, none),
        // Solid G-EQ: frequencies and Qs through its parameter objects ([ref] →
        // [ref]) and the `k` formatter.
        (
            "$ENGINE_PAR_SEQ_HF_FREQ",
            formula(SolidEqHfHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_SEQ_HMF_FREQ",
            formula(SolidEqHmfHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_SEQ_LMF_FREQ",
            formula(SolidEqLmfHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_SEQ_LF_FREQ",
            formula(SolidEqLfHz),
            Padded,
            none,
        ),
        ("$ENGINE_PAR_SEQ_HMF_Q", formula(SolidEqQ), Padded, none),
        ("$ENGINE_PAR_SEQ_LMF_Q", formula(SolidEqQ), Padded, none),
        (
            "$ENGINE_PAR_STEREO",
            lin(-100.0, 100.0),
            Decimal,
            [Some("mono"), None],
        ),
        ("$ENGINE_PAR_STEREO_PAN", lin(0.0, 1.0), Pan, none),
        (
            "$ENGINE_PAR_ATK_CURVE",
            formula(AttackCurvePercent),
            Whole,
            none,
        ),
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
        ("$ENGINE_PAR_FL_SPEED", exp(0.05, 8.0), Padded, none),
        ("$ENGINE_PAR_FL_PHASE", lin(0.0, 90.0), Wide, none),
        ("$ENGINE_PAR_PH_PHASE", lin(0.0, 90.0), Wide, none),
        ("$ENGINE_PAR_FL_COLOR", lin(0.0, 100.0), Wide, none),
        // [ref]: `20 + 380·x` as `%3.0f BPM`.
        ("$ENGINE_PAR_GATER_RATE", lin(20.0, 400.0), Bpm, none),
        // Feedback Compressor ([ref], cases 2..5), printed by the shared `%5.1f`.
        (
            "$ENGINE_PAR_FCOMP_ATTACK",
            lin(0.1034, 0.1034 + 49.896_603),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_FCOMP_RELEASE",
            lin(50.0, 1_200.000_1),
            Padded,
            none,
        ),
        ("$ENGINE_PAR_FCOMP_RATIO", lin(1.0, 20.0), Padded, none),
        ("$ENGINE_PAR_FCOMP_MIX", lin(0.0, 100.0), Padded, none),
        // Bus Compressor: its levels through a parameter object ([ref] →
        // [ref]), its menus six entries picked by `floor(6x)` (a 101-point sweep).
        (
            "$ENGINE_PAR_SCOMP_THRESHOLD",
            formula(BusCompThresholdDb),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_SCOMP_MAKEUP",
            formula(BusCompMakeupDb),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_SCOMP_RATIO",
            lin(0.0, 1.0),
            Menu(&["1.5", "2", "3", "4", "5", "10"]),
            none,
        ),
        (
            "$ENGINE_PAR_SCOMP_ATTACK",
            lin(0.0, 1.0),
            Menu(&["0.1", "0.3", "1", "3", "10", "30"]),
            none,
        ),
        (
            "$ENGINE_PAR_SCOMP_RELEASE",
            lin(0.0, 1.0),
            Menu(&["100", "200", "400", "800", "1600", "Auto"]),
            none,
        ),
        // Reverb2 ([ref], cases 5..7): 250·x, (1 − x)·19000 + 2000, x·−12 (−0 at 0,
        // which prints "-0.00").
        ("$ENGINE_PAR_RV2_PREDELAY", lin(0.0, 250.0), Padded, none),
        (
            "$ENGINE_PAR_RV2_HIGHCUT",
            lin(21_000.0, 2_000.0),
            Padded,
            none,
        ),
        ("$ENGINE_PAR_RV2_LOWSHELF", lin(-0.0, -12.0), Padded, none),
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
        // An LFO's delay (Fade In, [ref]): `e^(ln 5002·x + ln 2) − 2`, 0..10.002 s.
        (
            "$ENGINE_PAR_LFO_DELAY",
            curve([(0.0, 0.0, 5002f32.ln()), (1.0, 10_002.0, 0.0)]),
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
        // An LFO's waveform mixes, bipolar percents ([ref] on −1..1).
        ("$ENGINE_PAR_LFO_SINE", lin(-100.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_LFO_RECT", lin(-100.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_LFO_TRI", lin(-100.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_LFO_SAW", lin(-100.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_LFO_RAND", lin(-100.0, 100.0), Decimal, none),
        // Convolution predelay (2·(151^x − 1) ms, 0..300 ms; impulse.rs).
        (
            "$ENGINE_PAR_IRC_PREDELAY",
            kontra_law::kontakt::CONVOLUTION_PREDELAY,
            Padded,
            none,
        ),
        ("$ENGINE_PAR_WT_PHASE_RAND", lin(0.0, 100.0), Decimal, none),
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
        // Group controls the reference 8.13 shows no text for (KSP:
        // get_engine_par only), and effect bypass switches.
        "$ENGINE_PAR_EFFECT_BYPASS",
        "$ENGINE_PAR_SEND_EFFECT_BYPASS",
        "$ENGINE_PAR_START_CRITERIA_MODE",
        "$ENGINE_PAR_START_CRITERIA_NEXT_CRIT",
        "$ENGINE_PAR_START_CRITERIA_KEY_MIN",
        "$ENGINE_PAR_START_CRITERIA_KEY_MAX",
        "$ENGINE_PAR_START_CRITERIA_CONTROLLER",
        "$ENGINE_PAR_START_CRITERIA_CC_MIN",
        "$ENGINE_PAR_START_CRITERIA_CC_MAX",
        "$ENGINE_PAR_START_CRITERIA_CYCLE_CLASS",
        "$ENGINE_PAR_START_CRITERIA_ZONE_IDX",
        "$ENGINE_PAR_START_CRITERIA_SLICE_IDX",
        "$ENGINE_PAR_START_CRITERIA_SEQ_ONLY",
        "$ENGINE_PAR_VOICE_GROUP",
        "$ENGINE_PAR_POST_FX_SLOT",
        // The source module's mode and switches.
        "$ENGINE_PAR_SOURCE_MODE",
        "$ENGINE_PAR_TRACKING",
        "$ENGINE_PAR_HQI_MODE",
        "$ENGINE_PAR_TM_LEGATO",
        "$ENGINE_PAR_TMPRO_KEEP_FORMANTS",
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
        "$ENGINE_PAR_BITE_DC_QUANT",
        "$ENGINE_PAR_BITE_HPF",
        "$ENGINE_PAR_FREAK_BP_FILTER",
        "$ENGINE_PAR_FREAK_DEMOD",
        "$ENGINE_PAR_FREAK_GATE",
        "$ENGINE_PAR_FREAK_MODE",
        "$ENGINE_PAR_FREAK_WIDE_RANGE",
        "$ENGINE_PAR_FLAIR_CHORD",
        "$ENGINE_PAR_FLAIR_INVERT_PHASE",
        "$ENGINE_PAR_FLAIR_MODE",
        "$ENGINE_PAR_FLAIR_RATE_UNIT",
        "$ENGINE_PAR_FLAIR_SCANMODE",
        "$ENGINE_PAR_PHASIS_INVERT_MOD_MIX",
        "$ENGINE_PAR_PHASIS_INVERT_PHASE",
        "$ENGINE_PAR_PHASIS_RATE_UNIT",
        "$ENGINE_PAR_PHASIS_ULTRA",
        "$ENGINE_PAR_BASSINVADER_HI_BOOST",
        "$ENGINE_PAR_BASSINVADER_LO_CUT",
        "$ENGINE_PAR_BASSINVADER_MID_CONTOUR",
        "$ENGINE_PAR_BASSINVADER_MONO",
        "$ENGINE_PAR_SUPERGT_CHANNEL_LINK_MODE",
        "$ENGINE_PAR_SUPERGT_CHAR_MODE",
        "$ENGINE_PAR_SUPERGT_HPF_MODE",
        "$ENGINE_PAR_SUPERGT_SAT_MODE",
        "$ENGINE_PAR_SKDLX_MODE",
        "$ENGINE_PAR_SKDLX_MONO",
        "$ENGINE_PAR_TW_BRIGHT",
        "$ENGINE_PAR_TW_MONO",
        "$ENGINE_PAR_CW_MONO",
        "$ENGINE_PAR_AC_MONO",
        "$ENGINE_PAR_HS_MONO",
        "$ENGINE_PAR_HS_OVERDRIVE",
        "$ENGINE_PAR_DS_MONO",
        "$ENGINE_PAR_BIGFUZZ_MONO",
        "$ENGINE_PAR_FUZZ_MONO",
        "$ENGINE_PAR_TR_SMOOTH",
        "$ENGINE_PAR_VC_COLOR",
        "$ENGINE_PAR_VC_DEPTH",
        "$ENGINE_PAR_WOWFLUTTER_GATE",
        "$ENGINE_PAR_BEATMASHER_LENGTH",
        "$ENGINE_PAR_BEATMASHER_MASH",
        "$ENGINE_PAR_BEATMASHER_REVERSE",
        "$ENGINE_PAR_BEATMASHER_WRAP",
        "$ENGINE_PAR_BEATSLICER_GATE",
        "$ENGINE_PAR_BEATSLICER_PATTERN",
        "$ENGINE_PAR_BEATSLICER_SLICE",
        "$ENGINE_PAR_BEATSLICER_STYLE",
        "$ENGINE_PAR_BEATSLICER_TWOBARS",
        "$ENGINE_PAR_GATER_MUTE_INPUT",
        "$ENGINE_PAR_GATER_STUTTER",
        "$ENGINE_PAR_RG_FORWARD",
        "$ENGINE_PAR_RG_INVERT_GRAINS",
        "$ENGINE_PAR_RG_REVERSE",
        "$ENGINE_PAR_TS_GRAIN",
        "$ENGINE_PAR_TS_STRETCH",
        "$ENGINE_PAR_TS_TWOBARS",
        "$ENGINE_PAR_CABINET_TYPE",
        "$ENGINE_PAR_EPP_DRIVE_MODE",
        "$ENGINE_PAR_EPP_EQ_MODE",
        "$ENGINE_PAR_EPP_TREMOLO_MODE",
        "$ENGINE_PAR_EPP_TREMOLO_WAVE",
        "$ENGINE_PAR_EPP_TREMOLO_RATE_UNIT",
        "$ENGINE_PAR_EPP_MONO",
        "$ENGINE_PAR_CT_MONO",
        "$ENGINE_PAR_CS_MONO",
        "$ENGINE_PAR_DISTORTION_TYPE",
        "$ENGINE_PAR_SHAPE_TYPE",
        "$ENGINE_PAR_SEQ_HP",
        "$ENGINE_PAR_SEQ_LP",
        "$ENGINE_PAR_COMP_LINK",
        "$ENGINE_PAR_COMP_TYPE",
        "$ENGINE_PAR_IRC_ER_LR_BOUNDARY",
        "$ENGINE_PAR_IRC_AUTO_GAIN",
        "$ENGINE_PAR_IRC_REVERSE",
        "$ENGINE_PAR_STEREO_PSEUDO",
        "$ENGINE_PAR_S1200_FILTER_MODE",
        "$ENGINE_PAR_FILTER_LADDER_HQ",
        "$ENGINE_PAR_PHASE_INVERT",
        "$ENGINE_PAR_LR_SWAP",
    ];
    rows.extend(blank.map(|n| (n, lin(0.0, 1.0), Blank, none)));
    let bands = [
        "$ENGINE_PAR_BASSPRO_GEQ_40",
        "$ENGINE_PAR_BASSPRO_GEQ_90",
        "$ENGINE_PAR_BASSPRO_GEQ_180",
        "$ENGINE_PAR_BASSPRO_GEQ_300",
        "$ENGINE_PAR_BASSPRO_GEQ_500",
        "$ENGINE_PAR_BASSPRO_GEQ_1K",
        "$ENGINE_PAR_BASSPRO_GEQ_2K",
        "$ENGINE_PAR_BASSPRO_GEQ_4K",
        "$ENGINE_PAR_BASSPRO_GEQ_10K",
    ];
    rows.extend(bands.map(|n| (n, formula(BassProGeqDb), Wide, none)));
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
        "$ENGINE_PAR_BITE_CRUNCH",
        "$ENGINE_PAR_BITE_DITHER",
        "$ENGINE_PAR_BITE_EXPAND",
        "$ENGINE_PAR_BITE_JITTER",
        "$ENGINE_PAR_BITE_MIX",
        "$ENGINE_PAR_BITE_POSTFILTER",
        "$ENGINE_PAR_BITE_PREFILTER",
        "$ENGINE_PAR_FREAK_ANTIFOLD",
        "$ENGINE_PAR_FREAK_CONTOUR",
        "$ENGINE_PAR_FREAK_FEEDBACK",
        "$ENGINE_PAR_FREAK_HARMONICS",
        "$ENGINE_PAR_FREAK_MIX",
        "$ENGINE_PAR_FREAK_RELEASE",
        "$ENGINE_PAR_FREAK_TYPE",
        "$ENGINE_PAR_FLAIR_AMOUNT",
        "$ENGINE_PAR_FLAIR_DAMPING",
        "$ENGINE_PAR_FLAIR_DETUNE",
        "$ENGINE_PAR_FLAIR_FEEDBACK",
        "$ENGINE_PAR_FLAIR_MIX",
        "$ENGINE_PAR_FLAIR_OFFSET",
        "$ENGINE_PAR_FLAIR_WIDTH",
        "$ENGINE_PAR_PHASIS_AMOUNT",
        "$ENGINE_PAR_PHASIS_MIX",
        "$ENGINE_PAR_PHASIS_MOD_MIX",
        "$ENGINE_PAR_PHASIS_SPREAD",
        "$ENGINE_PAR_BASSINVADER_BASS",
        "$ENGINE_PAR_BASSINVADER_BOOST",
        "$ENGINE_PAR_BASSINVADER_HI_MID",
        "$ENGINE_PAR_BASSINVADER_LO_MID",
        "$ENGINE_PAR_BASSINVADER_MASTER",
        "$ENGINE_PAR_BASSINVADER_TREBLE",
        "$ENGINE_PAR_BASSINVADER_VOLUME",
        "$ENGINE_PAR_SUPERGT_MIX",
        "$ENGINE_PAR_SK_BASS",
        "$ENGINE_PAR_SK_BRIGHT",
        "$ENGINE_PAR_SK_DRIVE",
        "$ENGINE_PAR_SK_MIX",
        "$ENGINE_PAR_SK_TONE",
        "$ENGINE_PAR_SKDLX_DRIVE",
        "$ENGINE_PAR_SKDLX_TONE",
        "$ENGINE_PAR_TW_BASS",
        "$ENGINE_PAR_TW_MID",
        "$ENGINE_PAR_TW_TREBLE",
        "$ENGINE_PAR_TW_VOLUME",
        "$ENGINE_PAR_CW_PEDAL",
        "$ENGINE_PAR_AC_BASS",
        "$ENGINE_PAR_AC_BRILLIANTVOLUME",
        "$ENGINE_PAR_AC_NORMALVOLUME",
        "$ENGINE_PAR_AC_TONECUT",
        "$ENGINE_PAR_AC_TREBLE",
        "$ENGINE_PAR_AC_TREMOLODEPTH",
        "$ENGINE_PAR_AC_TREMOLOSPEED",
        "$ENGINE_PAR_HS_BASS",
        "$ENGINE_PAR_HS_DEPTH",
        "$ENGINE_PAR_HS_MASTER",
        "$ENGINE_PAR_HS_MID",
        "$ENGINE_PAR_HS_PRENORMAL",
        "$ENGINE_PAR_HS_PREOVERDRIVE",
        "$ENGINE_PAR_HS_PRESENCE",
        "$ENGINE_PAR_HS_TREBLE",
        "$ENGINE_PAR_DS_BASS",
        "$ENGINE_PAR_DS_DRIVE",
        "$ENGINE_PAR_DS_MID",
        "$ENGINE_PAR_DS_TONE",
        "$ENGINE_PAR_DS_TREBLE",
        "$ENGINE_PAR_DS_VOLUME",
        "$ENGINE_PAR_BIGFUZZ_BASS",
        "$ENGINE_PAR_BIGFUZZ_SUSTAIN",
        "$ENGINE_PAR_BIGFUZZ_TREBLE",
        "$ENGINE_PAR_FUZZ_AMOUNT",
        "$ENGINE_PAR_FUZZ_BASS",
        "$ENGINE_PAR_FUZZ_TREBLE",
        "$ENGINE_PAR_RT_ACCEL_HI",
        "$ENGINE_PAR_RT_ACCEL_LO",
        "$ENGINE_PAR_RT_BALANCE",
        "$ENGINE_PAR_RT_DISTANCE",
        "$ENGINE_PAR_VC_BLEND",
        "$ENGINE_PAR_VC_MIX",
        "$ENGINE_PAR_VC_WIDTH",
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
        (
            "$ENGINE_PAR_FREAK_STEREO",
            lin(-100.0, 100.0),
            Decimal,
            none,
        ),
        (
            "$ENGINE_PAR_FREAK_TUNING",
            lin(-100.0, 100.0),
            Decimal,
            none,
        ),
        (
            "$ENGINE_PAR_PHASIS_STEREO",
            lin(-100.0, 100.0),
            Decimal,
            none,
        ),
        ("$ENGINE_PAR_SKDLX_BASS", lin(-100.0, 100.0), Decimal, none),
        ("$ENGINE_PAR_SKDLX_MID", lin(-100.0, 100.0), Decimal, none),
        (
            "$ENGINE_PAR_SKDLX_TREBLE",
            lin(-100.0, 100.0),
            Decimal,
            none,
        ),
        (
            "$ENGINE_PAR_BIGFUZZ_TONE",
            lin(-100.0, 100.0),
            Decimal,
            none,
        ),
        ("$ENGINE_PAR_BITE_SATURATE", lin(0.0, 24.0), Decimal, none),
        ("$ENGINE_PAR_FREAK_WIDTH", lin(0.1, 2.0), Padded, none),
        ("$ENGINE_PAR_FLAIR_VOICES", lin(1.0, 4.0), Padded, none),
        ("$ENGINE_PAR_PHASIS_FEEDBACK", lin(0.0, 95.0), Decimal, none),
        ("$ENGINE_PAR_SUPERGT_ATTACK", lin(1.0, 10.0), Decimal, none),
        (
            "$ENGINE_PAR_SUPERGT_CHARACTER",
            lin(1.0, 10.0),
            Decimal,
            none,
        ),
        (
            "$ENGINE_PAR_SUPERGT_COMPRESS",
            lin(1.0, 10.0),
            Decimal,
            none,
        ),
        ("$ENGINE_PAR_SUPERGT_RELEASE", lin(1.0, 10.0), Decimal, none),
        (
            "$ENGINE_PAR_SUPERGT_SATURATION",
            lin(1.0, 10.0),
            Decimal,
            none,
        ),
        ("$ENGINE_PAR_SUPERGT_TRIM", lin(-30.0, 30.0), Decimal, none),
        ("$ENGINE_PAR_TR_ATTACK", lin(-100.0, 100.0), Padded, none),
        ("$ENGINE_PAR_TR_SUSTAIN", lin(-100.0, 100.0), Padded, none),
        // -24 dB to 0 over the first half, then to +6.
        (
            "$ENGINE_PAR_TR_INPUT",
            ir::Law::Breakpoints([(0.0, -24.0, 0.0), (0.5, 0.0, 0.0), (1.0, 6.0, 0.0)].into()),
            Padded,
            none,
        ),
        ("$ENGINE_PAR_VC_RATE", exp(0.75, 24.0), Padded, none),
        // Read from each knob's display code in the reference (kontra_law::FormulaId).
        (
            "$ENGINE_PAR_BASSPRO_GAIN",
            formula(BassProGainDb),
            Wide,
            none,
        ),
        (
            "$ENGINE_PAR_BASSPRO_MASTER",
            formula(BassProMasterDb),
            Wide,
            none,
        ),
        ("$ENGINE_PAR_BASSPRO_MID", formula(BassProMidDb), Wide, none),
        (
            "$ENGINE_PAR_BASSPRO_GEQ_VOLUME",
            formula(BassProGeqVolumeDb),
            Wide,
            none,
        ),
        (
            "$ENGINE_PAR_PSYDL_DETUNE",
            formula(PsycheDetuneCents),
            Whole,
            none,
        ),
        (
            "$ENGINE_PAR_PSYDL_PITCH",
            formula(PsychePitchSemitones),
            Whole,
            none,
        ),
        ("$ENGINE_PAR_RAUM_DECAY", formula(RaumDecayMs), Padded, none),
        (
            "$ENGINE_PAR_FREAK_CARRIER",
            formula(FreakCarrierHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_FREAK_BP_FREQ",
            formula(FreakBandPassHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_BITE_FREQUENCY",
            formula(BiteFrequencyHz),
            Padded,
            none,
        ),
        ("$ENGINE_PAR_BITE_BITS", formula(BiteBits), Whole, none),
        ("$ENGINE_PAR_FLAIR_RATE", formula(FlairRateHz), Padded, none),
        (
            "$ENGINE_PAR_FLAIR_PITCH",
            formula(FlairPitchNote),
            Note,
            none,
        ),
        (
            "$ENGINE_PAR_PHASIS_NOTCHES",
            formula(PhasisNotches),
            Whole,
            none,
        ),
        (
            "$ENGINE_PAR_PHASIS_RATE",
            formula(PhasisRateHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_PHASIS_CENTER",
            formula(PhasisCenterHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_FREAK_FREQUENCY",
            formula(FreakFrequencyHz),
            Padded,
            none,
        ),
        ("$ENGINE_PAR_RT_MIX", formula(RotatorMixDb), Decimal, none),
        (
            "$ENGINE_PAR_RT_SPEED",
            formula(RotatorFast),
            Menu(&["slow", "fast"]),
            none,
        ),
        ("$ENGINE_PAR_RAUM_RATE", formula(RaumRateHz), Padded, none),
        (
            "$ENGINE_PAR_CHORAL_RATE",
            formula(ChoralRateHz),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_TDL_TIME_L",
            formula(TwinDelayTimeMs),
            Padded,
            none,
        ),
        (
            "$ENGINE_PAR_TDL_TIME_R",
            formula(TwinDelayTimeMs),
            Padded,
            none,
        ),
    ]);
    // The Convolution's knobs read what they set: its cut-offs through the measured knob laws
    // (impulse.rs), its sizes as 50..150 % in one decimal (`get_engine_par_disp_ext`,
    // 2026-11-25). Its predelay and switches have their own rows above.
    rows.extend(
        crate::impulse::script_knobs()
            .into_iter()
            .filter(|(name, ..)| name.contains("_FREQ_") || name.contains("LENGTH"))
            .map(|(name, _, law)| {
                if name.contains("LENGTH") {
                    (name, lin(50.0, 150.0), Decimal, none)
                } else {
                    (name, law, Padded, none)
                }
            }),
    );
    let cutoff_subtypes = cutoff_subtype_readouts();
    let empty: std::sync::Arc<[(i32, ir::Readout)]> = std::sync::Arc::from([]);
    rows.into_iter()
        .map(|(name, law, form, ends)| {
            let by_subtype = if name == "$ENGINE_PAR_CUTOFF" {
                std::sync::Arc::clone(&cutoff_subtypes)
            } else {
                std::sync::Arc::clone(&empty)
            };
            (
                name,
                ir::Readout {
                    law,
                    form,
                    ends,
                    by_subtype,
                },
            )
        })
        .collect()
}

/// Filter-subtype `$ENGINE_PAR_CUTOFF` readouts, measured across every filter
/// family on Kontakt 8.13.1 (`disp_sweep.py`, 2026-11-26):
/// - AR (100..=108): note `0..145` (8.176..35485 Hz)
/// - SV (50..=62) & Ladder (30..=41): note `20..135` (25.96..19912 Hz)
/// - SV Par/Ser (63..=65): note `-20..160` (2.58..84390 Hz)
/// - Daft (70..=71): note `20..145` (25.96..35485 Hz)
/// - Pro-53 (16) & Legacy Ladder (17): note `20..120` (25.96..8372 Hz)
/// - 3x2 Versatile (19): note `20..131` (25.96..15804 Hz)
/// - Legacy LP/HP/BP/BR (2, 4, 6..=12): 8-term Taylor `exp(6.15x)-1` (43.6..21830 Hz)
/// - Legacy HP1 (3): `0.82825` of Legacy LP (36.1..18080 Hz)
/// - Phaser (13), Vowel A (14), Vowel B (15): linear `0..100`
fn cutoff_subtype_readouts() -> std::sync::Arc<[(i32, ir::Readout)]> {
    use ir::Form::Padded;
    use kontra_law::FormulaId::{LegacyCutoffHz, LegacyHp1CutoffHz};
    let empty: std::sync::Arc<[(i32, ir::Readout)]> = std::sync::Arc::from([]);
    let note_hz = |n: f32| 440.0 * ((n - 69.0) / 12.0).exp2();
    let exp_notes = |lo, hi| ir::Law::Exp {
        lo: note_hz(lo),
        hi: note_hz(hi),
    };
    let families: &[(&[i32], ir::Law)] = &[
        (
            &[100, 101, 102, 103, 104, 105, 106, 107, 108],
            ir::Law::Exp {
                lo: 8.176,
                hi: 35_485.0,
            },
        ),
        (
            &[
                30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 50, 51, 52, 53, 54, 55, 56, 57, 58,
                59, 60, 61, 62,
            ],
            exp_notes(20.0, 135.0),
        ),
        (&[63, 64, 65], exp_notes(-20.0, 160.0)),
        (
            &[70, 71],
            ir::Law::Exp {
                lo: 25.958,
                hi: 35_485.0,
            },
        ),
        (&[16, 17], exp_notes(20.0, 120.0)),
        (&[19], exp_notes(20.0, 131.0)),
        (
            &[2, 4, 6, 7, 8, 9, 10, 11, 12],
            ir::Law::Formula(LegacyCutoffHz),
        ),
        (&[3], ir::Law::Formula(LegacyHp1CutoffHz)),
        (&[13, 14, 15], ir::Law::Linear { lo: 0.0, hi: 100.0 }),
    ];
    families
        .iter()
        .flat_map(|(ids, law)| {
            let r = ir::Readout {
                law: law.clone(),
                form: Padded,
                ends: [None; 2],
                by_subtype: std::sync::Arc::clone(&empty),
            };
            ids.iter().map(move |&id| (id, r.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::readouts;

    /// Every filter family's `$ENGINE_PAR_CUTOFF` readout resolves to its
    /// family's law (KT-10); end-to-end string checks run in `kontra-host`.
    #[test]
    fn filter_subtype_cutoff_readouts_cover_all_families() {
        let (_, cutoff) = readouts()
            .into_iter()
            .find(|(name, _)| *name == "$ENGINE_PAR_CUTOFF")
            .expect("cutoff readout");
        let mid = |sub| cutoff.for_subtype(sub).law.eval(0.5);
        assert!((mid(None) - 538.6).abs() < 0.2);
        assert!((mid(Some(100)) - 538.6).abs() < 0.2); // AR
        assert!((mid(Some(52)) - 718.9).abs() < 0.2); // SV
        assert!((mid(Some(31)) - 718.9).abs() < 0.2); // Ladder
        assert!((mid(Some(70)) - 959.7).abs() < 0.2); // Daft
        assert!((mid(Some(16)) - 466.2).abs() < 0.2); // Pro-53
        assert!((mid(Some(17)) - 466.2).abs() < 0.2); // Legacy Ladder
        assert!((mid(Some(19)) - 640.5).abs() < 0.2); // 3x2 Versatile
        assert!((mid(Some(8)) - 1195.6).abs() < 1.0); // Legacy LP4
        assert!((mid(Some(3)) - 990.3).abs() < 0.5); // Legacy HP1
        assert!((mid(Some(13)) - 50.0).abs() < 1e-4); // Phaser
        assert!((mid(Some(63)) - 466.2).abs() < 0.2); // SV Par LP/HP
    }
}
