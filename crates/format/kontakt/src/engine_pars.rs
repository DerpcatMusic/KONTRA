//! KSP's `$ENGINE_PAR_*` names as one table: what each drives in the
//! engine, through which law, and the text `get_engine_par_disp` answers.
//!
//! A known name on a target the instrument lacks reads 0, shows "" and
//! drops the set, and the script runs on (Kontakt 8.13.1's getter miss
//! path, [ref]; `get_engine_par_disp` [ref] calls the getter
//! [ref] in text mode). The engine applies that as `Engine::absent`.

use kontra_ir as ir;

use crate::readouts::readouts;
use crate::script_knobs::script_knobs;

/// Every `$ENGINE_PAR_*` name KONTRA carries.
pub fn engine_pars() -> Vec<ir::ScriptPar> {
    let mut texts = readouts();
    let mut readout = |name: &str| {
        let at = texts.iter().position(|(n, _)| *n == name)?;
        Some(texts.swap_remove(at).1)
    };
    let fixed = FIXED
        .iter()
        .map(|&(name, par)| (name, ir::Drives::Engine(par)));
    let knobs = script_knobs()
        .into_iter()
        .map(|(name, target, law)| (name, ir::Drives::Knob(target, law)));
    let group = CRITERIA
        .iter()
        .map(|&name| (name, ir::HeldSet::Drop))
        .chain(GROUP_HELD.iter().copied())
        .chain(crate::source_pars::SOURCE.iter().copied())
        .map(|(name, set)| {
            let key = ir::held_key(name);
            (
                name,
                ir::Drives::Engine(ir::EnginePar::GroupHeld { key, set }),
            )
        });
    let mut pars: Vec<_> = fixed
        .chain(knobs)
        .chain(group)
        .map(|(name, drives)| ir::ScriptPar {
            name,
            drives,
            readout: readout(name),
        })
        .collect();
    // A module control KONTRA has no processor for still shows Kontakt's
    // text: its slot holds the value.
    pars.extend(
        texts
            .into_iter()
            .filter(|(n, _)| held(n))
            .map(|(name, r)| ir::ScriptPar {
                name,
                drives: ir::Drives::Engine(ir::EnginePar::Opaque(ir::held_key(name))),
                readout: Some(r),
            }),
    );
    pars
}

/// The effect type (its `$EFFECT_TYPE_*` name) parameter `name` belongs
/// to, for the families only one module has. On a slot holding another
/// type it names nothing: Kontakt 8.13.1 drops the set and reads 0, and the
/// script runs on (`STEREO_PAN` addressed to a Lo-Fi slot, the group index
/// winning over a bus `<generic>`; probe bus_group_override.lua,
/// 2026-11-12). A name not listed is left to the slot's own check.
/// The rate kind each `$NI_SYNC_UNIT_*` value picks (KSP reference: ABS 0,
/// WHOLE 5 .. 256TH 21; 11 and 14 are unused): free Hz, or one count of the
/// note value in quarter-note beats (−1: no unit).
#[must_use]
pub fn lfo_units() -> Vec<Option<ir::Rate>> {
    const T: f32 = 2.0 / 3.0;
    #[rustfmt::skip]
    let beats = [0.0, -1.0, -1.0, -1.0, -1.0, 4.0, 4.0 * T, 2.0, 2.0 * T, 1.0, T, -1.0, 0.5, 0.5 * T,
        -1.0, 0.25, 0.25 * T, 0.125, 0.125 * T, 0.0625, 0.0625 * T, 1.0 / 64.0];
    #[rustfmt::skip]
    let unit = |b: f32| if b > 0.0 { Some(ir::Rate::Steps { beats: b, count: 1.0 }) }
        else { (b == 0.0).then_some(ir::Rate::Hz(1.0)) };
    beats.into_iter().map(unit).collect()
}

#[must_use]
pub fn owner(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_uppercase();
    let family = |pattern: &str| match pattern.strip_suffix('_') {
        Some(_) => name.starts_with(pattern),
        None => name == pattern,
    };
    OWNERS
        .iter()
        .find(|(pattern, _)| family(pattern))
        .map(|&(_, ty)| ty)
}

/// Whether `name`, a parameter not in [`engine_pars`], is a control of an
/// effect module: its slot holds the value ([`ir::EnginePar::Held`]).
#[must_use]
pub fn held(name: &str) -> bool {
    owner(name).is_some()
        || UNCARRIED_FILTER
            .iter()
            .any(|n| name.eq_ignore_ascii_case(n))
}

/// The values `set_engine_par` takes for `name`, when it takes only some:
/// any other is refused, and the control keeps what it held.
#[must_use]
pub fn accepts(name: &str) -> Option<(i32, i32)> {
    ACCEPTS
        .iter()
        .find(|(n, ..)| name.eq_ignore_ascii_case(n))
        .map(|&(_, lo, hi)| (lo, hi))
}

/// The reference 8.13's setter takes these in their own
/// units (a count, a note, a menu index), each checked unsigned against its
/// range; outside it the script gets an error ("Phasis notch count must be
/// between 2 and 12!", "Invalid Replika delay type!", ...) and nothing is
/// stored. Notches store `(n − 2)·0.1` and Flair's pitch `n/96`; the rest as
/// given. Each range is the one its message's check gives, and the
/// measured `$NI_*` value sets agree. Checks whose message names no one
/// control (the wavetable forms, a filter feedback type, the reverb mode,
/// S1200's filter) are not listed.
const ACCEPTS: &[(&str, i32, i32)] = &[
    ("$ENGINE_PAR_PHASIS_NOTCHES", 2, 12),
    ("$ENGINE_PAR_FLAIR_PITCH", 0, 96),
    ("$ENGINE_PAR_FLAIR_MODE", 0, 2),
    ("$ENGINE_PAR_FLAIR_SCANMODE", 0, 2),
    ("$ENGINE_PAR_FLAIR_CHORD", 0, 23),
    ("$ENGINE_PAR_RDL_TYPE", 0, 4),
    ("$ENGINE_PAR_CHORAL_MODE", 0, 3),
    ("$ENGINE_PAR_SHAPE_TYPE", 0, 2),
    ("$ENGINE_PAR_DISTORTION_TYPE", 0, 1),
    ("$ENGINE_PAR_SUPERGT_HPF_MODE", 0, 2),
    ("$ENGINE_PAR_SUPERGT_SAT_MODE", 0, 2),
    ("$ENGINE_PAR_SUPERGT_CHAR_MODE", 0, 2),
    ("$ENGINE_PAR_SUPERGT_CHANNEL_LINK_MODE", 0, 2),
    ("$ENGINE_PAR_RINGMOD_LFO_WAVE", 0, 1),
    ("$ENGINE_PAR_WT_MOD_TYPE", 0, 9),
    ("$ENGINE_PAR_WT_MOD_WAVE", 0, 12),
    ("$ENGINE_PAR_WT_MOD_TUNE_UNIT", 0, 2),
    ("$ENGINE_PAR_HQI_MODE", 0, 2),
    ("$ENGINE_PAR_STEPSEQ_NUM_STEPS", 2, 32),
    ("$ENGINE_PAR_FLEXENV_LOOP_START", 1, 30),
    ("$ENGINE_PAR_FLEXENV_LOOP_END", 2, 31),
    ("$ENGINE_PAR_FLEXENV_NUM_STAGES", 3, 31),
    ("$ENGINE_PAR_BEATMASHER_LENGTH", 0, 8),
    ("$ENGINE_PAR_BEATSLICER_PATTERN", 0, 4),
    ("$ENGINE_PAR_BEATSLICER_STYLE", 0, 4),
];

/// Filter-slot controls (KSP manual, "Filters") with no processor yet.
const UNCARRIED_FILTER: &[&str] = &[
    "$ENGINE_PAR_GAIN",
    "$ENGINE_PAR_BANDWIDTH",
    "$ENGINE_PAR_FILTER_LADDER_HQ",
    "$ENGINE_PAR_LP_CUTOFF",
    "$ENGINE_PAR_HP_CUTOFF",
    "$ENGINE_PAR_S1200_FILTER_MODE",
];

/// Parameter families (a trailing `_` is a prefix, else the exact name)
/// and their effect type, from the KSP manual's engine parameter sections.
const OWNERS: &[(&str, &str)] = &[
    ("$ENGINE_PAR_STEREO", "$EFFECT_TYPE_STEREO"),
    ("$ENGINE_PAR_STEREO_PAN", "$EFFECT_TYPE_STEREO"),
    ("$ENGINE_PAR_COMP_", "$EFFECT_TYPE_COMPRESSOR"),
    ("$ENGINE_PAR_LIM_", "$EFFECT_TYPE_LIMITER"),
    ("$ENGINE_PAR_SCOMP_", "$EFFECT_TYPE_BUS_COMP"),
    ("$ENGINE_PAR_FCOMP_", "$EFFECT_TYPE_FB_COMP"),
    ("$ENGINE_PAR_SEQ_", "$EFFECT_TYPE_SOLID_GEQ"),
    ("$ENGINE_PAR_SK_", "$EFFECT_TYPE_SKREAMER"),
    ("$ENGINE_PAR_SKDLX_", "$EFFECT_TYPE_SKREAMERDLX"),
    ("$ENGINE_PAR_TP_", "$EFFECT_TYPE_TAPE_SAT"),
    ("$ENGINE_PAR_IRC_", "$EFFECT_TYPE_IRC"),
    ("$ENGINE_PAR_RV_", "$EFFECT_TYPE_REVERB"),
    ("$ENGINE_PAR_DL_", "$EFFECT_TYPE_DELAY"),
    ("$ENGINE_PAR_CH_", "$EFFECT_TYPE_CHORUS"),
    ("$ENGINE_PAR_FL_", "$EFFECT_TYPE_FLANGER"),
    ("$ENGINE_PAR_PH_", "$EFFECT_TYPE_PHASER"),
    ("$ENGINE_PAR_RDL_", "$EFFECT_TYPE_REPLIKA"),
    ("$ENGINE_PAR_TDL_", "$EFFECT_TYPE_TWINDELAY"),
    ("$ENGINE_PAR_PSYDL_", "$EFFECT_TYPE_PSYCHEDELAY"),
    ("$ENGINE_PAR_RAUM_", "$EFFECT_TYPE_RAUM"),
    ("$ENGINE_PAR_JMP_", "$EFFECT_TYPE_JUMP"),
    ("$ENGINE_PAR_EPP_", "$EFFECT_TYPE_EP_PREAMPS"),
    ("$ENGINE_PAR_FREAK_", "$EFFECT_TYPE_FREAK"),
    ("$ENGINE_PAR_FLAIR_", "$EFFECT_TYPE_FLAIR"),
    ("$ENGINE_PAR_PHASIS_", "$EFFECT_TYPE_PHASIS"),
    ("$ENGINE_PAR_CHORAL_", "$EFFECT_TYPE_CHORAL"),
    ("$ENGINE_PAR_KOLOR_", "$EFFECT_TYPE_KOLOR"),
    ("$ENGINE_PAR_DIRT_", "$EFFECT_TYPE_DIRT"),
    ("$ENGINE_PAR_BITE_", "$EFFECT_TYPE_BITE"),
    ("$ENGINE_PAR_BASSPRO_", "$EFFECT_TYPE_BASSPRO"),
    ("$ENGINE_PAR_BASSINVADER_", "$EFFECT_TYPE_BASSINVADER"),
    ("$ENGINE_PAR_V5_", "$EFFECT_TYPE_VAN51"),
    ("$ENGINE_PAR_SF100_", "$EFFECT_TYPE_SUPERFAST100"),
    ("$ENGINE_PAR_SUPERGT_", "$EFFECT_TYPE_SUPERGT"),
    ("$ENGINE_PAR_REVERBDELIGHT_", "$EFFECT_TYPE_REVERBDELIGHT"),
    ("$ENGINE_PAR_WOWFLUTTER_", "$EFFECT_TYPE_WOWFLUTTER"),
    ("$ENGINE_PAR_RINGMOD_", "$EFFECT_TYPE_RINGMOD"),
    ("$ENGINE_PAR_GATER_", "$EFFECT_TYPE_GATER"),
    ("$ENGINE_PAR_BEATSLICER_", "$EFFECT_TYPE_BEATSLICER"),
    ("$ENGINE_PAR_BEATMASHER_", "$EFFECT_TYPE_BEATMASHER"),
    ("$ENGINE_PAR_BIGFUZZ_", "$EFFECT_TYPE_BIGFUZZ"),
    ("$ENGINE_PAR_FUZZ_", "$EFFECT_TYPE_FUZZ"),
    ("$ENGINE_PAR_STEREOTUNE_", "$EFFECT_TYPE_STEREO_TUNE"),
    ("$ENGINE_PAR_VC_", "$EFFECT_TYPE_VIBRATO_CHORUS"),
    ("$ENGINE_PAR_RT_", "$EFFECT_TYPE_ROTATOR"),
    ("$ENGINE_PAR_TR_", "$EFFECT_TYPE_TRANS_MASTER"),
    ("$ENGINE_PAR_RV2_", "$EFFECT_TYPE_REVERB2"),
    ("$ENGINE_PAR_DS_", "$EFFECT_TYPE_DSTORTION"),
    ("$ENGINE_PAR_GN_", "$EFFECT_TYPE_GAINER"),
    ("$ENGINE_PAR_AC_", "$EFFECT_TYPE_ACBOX"),
    ("$ENGINE_PAR_HS_", "$EFFECT_TYPE_HOTSOLO"),
    ("$ENGINE_PAR_CAT_", "$EFFECT_TYPE_CAT"),
    ("$ENGINE_PAR_CW_", "$EFFECT_TYPE_CRYWAH"),
    ("$ENGINE_PAR_TW_", "$EFFECT_TYPE_TWANG"),
    ("$ENGINE_PAR_CB_", "$EFFECT_TYPE_CABINET"),
    ("$ENGINE_PAR_RG_", "$EFFECT_TYPE_REVGRAIN"),
    ("$ENGINE_PAR_TS_", "$EFFECT_TYPE_TRANS_STRETCH"),
    ("$ENGINE_PAR_CT_", "$EFFECT_TYPE_CAT"),
    ("$ENGINE_PAR_CS_", "$EFFECT_TYPE_CHAINSAW"),
    ("$ENGINE_PAR_CABINET_TYPE", "$EFFECT_TYPE_CABINET"),
    ("$ENGINE_PAR_NOISELEVEL", "$EFFECT_TYPE_LOFI"),
    ("$ENGINE_PAR_NOISECOLOR", "$EFFECT_TYPE_LOFI"),
    ("$ENGINE_PAR_DISTORTION_TYPE", "$EFFECT_TYPE_DISTORTION"),
    ("$ENGINE_PAR_SHAPE_TYPE", "$EFFECT_TYPE_SHAPER"),
    ("$ENGINE_PAR_TRANSLIM_", "$EFFECT_TYPE_TRANSLIM"),
    ("$ENGINE_PAR_SP_", "$EFFECT_TYPE_SURROUND_PANNER"),
    ("$ENGINE_PAR_PR_", "$EFFECT_TYPE_PLATEREVERB"),
    ("$ENGINE_PAR_EXP_FILTER_", "$EFFECT_TYPE_AET_FILTER"),
    ("$ENGINE_PAR_PHASE_INVERT", "$EFFECT_TYPE_INVERTER"),
    ("$ENGINE_PAR_LR_SWAP", "$EFFECT_TYPE_INVERTER"),
    ("$ENGINE_PAR_STEREO_PSEUDO", "$EFFECT_TYPE_STEREO"),
];

/// A group's start criteria, read only (the KSP manual: "only work with
/// get_engine_par()"), at index 0..=3, in the order of a saved criterion's
/// fields ([`crate::translate`] seeds them). The reference 8.13 reads a
/// criterion never set as mode 0, keys 24..24, controller 1, 0..64, class 0,
/// zone and slice −1, and drops every set (measured 2026-11-25,
/// sweeps/startc*.tsv).
pub(crate) const CRITERIA: [&str; 11] = [
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
];

/// Other held group controls, and what a set keeps.
/// `VOICE_GROUP` reads the saved voice group (−1 none) and keeps any set,
/// but moves no voice: KONTRA's voice groups are only those the groups name
/// at load. `POST_FX_SLOT` reads 0 and drops every set in the reference
/// 8.13, with group effects loaded or not (measured 2026-11-25).
const GROUP_HELD: [(&str, ir::HeldSet); 2] = [
    ("$ENGINE_PAR_VOICE_GROUP", ir::HeldSet::Keep),
    ("$ENGINE_PAR_POST_FX_SLOT", ir::HeldSet::Drop),
];

/// The names with an engine parameter of their own.
const FIXED: &[(&str, ir::EnginePar)] = {
    use ir::EnginePar as P;
    use ir::ReverbPar as R;
    use ir::ZonePar as Z;
    &[
        ("$ENGINE_PAR_ATTACK", P::Attack),
        ("$ENGINE_PAR_HOLD", P::Hold),
        ("$ENGINE_PAR_DECAY", P::Decay),
        ("$ENGINE_PAR_SUSTAIN", P::Sustain),
        ("$ENGINE_PAR_RELEASE", P::Release),
        ("$ENGINE_PAR_ATK_CURVE", P::AttackCurve),
        ("$ENGINE_PAR_MOD_TARGET_INTENSITY", P::RouteIntensity),
        ("$ENGINE_PAR_MOD_TARGET_MP_INTENSITY", P::RouteDepth),
        ("$ENGINE_PAR_INTMOD_INTENSITY", P::RouteDepth),
        ("$ENGINE_PAR_INTMOD_BYPASS", P::ModBypass),
        ("$ENGINE_PAR_LFO_DELAY", P::LfoFade),
        ("$ENGINE_PAR_INTMOD_RETRIGGER", P::LfoRetrigger),
        ("$ENGINE_PAR_INTMOD_FREQUENCY_UNIT", P::LfoUnit),
        // Position, the form amounts and Inharm are 0..=1 linear (readouts.rs
        // shows 0..100); the voice clamps each with its routes to 0..=1
        //. The modulator is not carried yet: its text answers,
        // a set is reported. The form modes are the
        // `$NI_WT_FORM_*` kinds (wavetable::warp_modes); no mode has text.
        (
            "$ENGINE_PAR_WT_POSITION",
            P::Wavetable(ir::TableValue::Position),
        ),
        ("$ENGINE_PAR_WT_FORM", P::Wavetable(ir::TableValue::Warp(0))),
        ("$ENGINE_PAR_WT_FORM_MODE", P::WarpMode(0)),
        ("$ENGINE_PAR_WT_FORM2_MODE", P::WarpMode(1)),
        (
            "$ENGINE_PAR_WT_FORM2",
            P::Wavetable(ir::TableValue::Warp(1)),
        ),
        (
            "$ENGINE_PAR_WT_MOD_AMOUNT",
            P::Wavetable(ir::TableValue::ModAmount),
        ),
        ("$ENGINE_PAR_WT_MOD_TYPE", P::ModOscKind),
        ("$ENGINE_PAR_WT_MOD_WAVE", P::TableModWave),
        ("$ENGINE_PAR_WT_MOD_TUNE_UNIT", P::ModOscUnit),
        ("$ENGINE_PAR_WT_QUALITY", P::TableQuality),
        ("$ENGINE_PAR_WT_PHASE", P::Wavetable(ir::TableValue::Phase)),
        (
            "$ENGINE_PAR_WT_PHASE_RAND",
            P::Wavetable(ir::TableValue::PhaseRandom),
        ),
        (
            "$ENGINE_PAR_WT_INHARMONIC",
            P::Wavetable(ir::TableValue::Inharmonic),
        ),
        ("$ENGINE_PAR_WT_INHARMONIC_MODE", P::InharmonicOn),
        (
            "$ENGINE_PAR_WT_MOD_TUNE",
            P::Wavetable(ir::TableValue::ModTune),
        ),
        ("$ENGINE_PAR_RELEASE_TRIGGER", P::ReleaseTrigger),
        ("$ENGINE_PAR_OUTPUT_CHANNEL", P::OutputChannel),
        ("$ENGINE_PAR_EFFECT_BYPASS", P::EffectBypass),
        ("$ENGINE_PAR_EFFECT_SUBTYPE", P::EffectSubtype),
        ("$ENGINE_PAR_EFFECT_TYPE", P::SlotKind),
        ("$ENGINE_PAR_SEND_EFFECT_TYPE", P::SlotKind),
        ("$ENGINE_PAR_SEND_EFFECT_BYPASS", P::EffectBypass),
        ("$ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN", P::EffectOutput),
        ("$ENGINE_PAR_SEND_EFFECT_OUTPUT_GAIN", P::EffectOutput),
        ("$ENGINE_PAR_SEND_EFFECT_DRY_LEVEL", P::EffectDry),
        ("$ENGINE_PAR_SENDLEVEL_0", P::SendLevel(0)),
        ("$ENGINE_PAR_SENDLEVEL_1", P::SendLevel(1)),
        ("$ENGINE_PAR_SENDLEVEL_2", P::SendLevel(2)),
        ("$ENGINE_PAR_SENDLEVEL_3", P::SendLevel(3)),
        ("$ENGINE_PAR_SENDLEVEL_4", P::SendLevel(4)),
        ("$ENGINE_PAR_SENDLEVEL_5", P::SendLevel(5)),
        ("$ENGINE_PAR_SENDLEVEL_6", P::SendLevel(6)),
        ("$ENGINE_PAR_SENDLEVEL_7", P::SendLevel(7)),
        ("$ENGINE_PAR_RV2_MOD", P::Reverb(R::Modulation)),
        ("$ENGINE_PAR_RV2_TYPE", P::ReverbNetwork),
        ("$ENGINE_PAR_RV2_TIME", P::Reverb(R::Seconds)),
        ("$ENGINE_PAR_RV2_SIZE", P::Reverb(R::Size)),
        ("$ENGINE_PAR_RV2_DAMPING", P::Reverb(R::Damping)),
        ("$ENGINE_PAR_RV2_DIFF", P::Reverb(R::Diffusion)),
        ("$ENGINE_PAR_RV2_PREDELAY", P::Reverb(R::Predelay)),
        ("$ENGINE_PAR_RV2_HIGHCUT", P::Reverb(R::HighCut)),
        ("$ENGINE_PAR_RV2_LOWSHELF", P::Reverb(R::LowShelf)),
        ("$ENGINE_PAR_RV2_STEREO", P::Reverb(R::Stereo)),
        ("$ENGINE_PAR_GN_GAIN", P::GainerGain),
        ("$ENGINE_PAR_STEREO", P::StereoWidth),
        ("$ENGINE_PAR_STEREO_PAN", P::StereoPan),
        ("$ENGINE_PAR_VOLUME", P::Volume),
        ("$ENGINE_PAR_PAN", P::Pan),
        ("$ENGINE_PAR_TUNE", P::Tune),
        // get_zone_par's and set_zone_par's.
        ("$ZONE_PAR_LOW_KEY", P::Zone(Z::LowKey)),
        ("$ZONE_PAR_HIGH_KEY", P::Zone(Z::HighKey)),
        ("$ZONE_PAR_LOW_VELO", P::Zone(Z::LowVelocity)),
        ("$ZONE_PAR_HIGH_VELO", P::Zone(Z::HighVelocity)),
        ("$ZONE_PAR_ROOT_KEY", P::Zone(Z::RootKey)),
        ("$ZONE_PAR_GROUP", P::Zone(Z::Group)),
        ("$ZONE_PAR_SAMPLE_START", P::Zone(Z::SampleStart)),
        ("$ZONE_PAR_SAMPLE_END", P::Zone(Z::SampleEnd)),
        ("$ZONE_PAR_SAMPLE_MOD_RANGE", P::Zone(Z::StartRange)),
        ("$ZONE_PAR_SAMPLE_RATE", P::Zone(Z::SampleRate)),
        ("$ZONE_PAR_SELECTED", P::Zone(Z::Selected)),
        ("$ZONE_PAR_VOLUME", P::Zone(Z::Volume)),
        ("$ZONE_PAR_PAN", P::Zone(Z::Pan)),
        ("$ZONE_PAR_TUNE", P::Zone(Z::Tune)),
        ("$ZONE_PAR_FADE_LOW_KEY", P::Zone(Z::FadeLowKey)),
        ("$ZONE_PAR_FADE_HIGH_KEY", P::Zone(Z::FadeHighKey)),
        ("$ZONE_PAR_FADE_LOW_VELO", P::Zone(Z::FadeLowVelocity)),
        ("$ZONE_PAR_FADE_HIGH_VELO", P::Zone(Z::FadeHighVelocity)),
        ("$ZONE_PAR_BPM", P::Zone(Z::Bpm)),
    ]
};

#[cfg(test)]
mod tests {
    /// A text law joins its parameter by name; a renamed or dropped row
    /// must not silently lose its text.
    #[test]
    fn every_readout_names_a_parameter() {
        let rows = super::engine_pars();
        let orphans: Vec<_> = super::readouts()
            .into_iter()
            .filter(|(name, _)| !rows.iter().any(|r| r.name == *name && r.readout.is_some()))
            .map(|(name, _)| name)
            .collect();
        assert!(orphans.is_empty(), "{orphans:?}");
    }
}
