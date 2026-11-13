//! KSP's `$ENGINE_PAR_*` names as one table: what each drives in the
//! engine, through which law, and the text `get_engine_par_disp` answers.
//!
//! A known name on a target the instrument lacks reads 0, shows "" and
//! drops the set, and the script runs on (Kontakt 8.13.1's getter miss
//! path, 0x1408a4207; `get_engine_par_disp` 0x140785d40 calls the getter
//! 0x140898570 in text mode). The engine applies that as `Engine::absent`.

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
    fixed
        .chain(knobs)
        .map(|(name, drives)| ir::ScriptPar {
            name,
            drives,
            readout: readout(name),
        })
        .collect()
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
        // (0x140b52133). The modulator is not carried yet: its text answers,
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
        ("$ENGINE_PAR_WT_MOD_AMOUNT", P::Shown),
        (
            "$ENGINE_PAR_WT_INHARMONIC",
            P::Wavetable(ir::TableValue::Inharmonic),
        ),
        ("$ENGINE_PAR_WT_INHARMONIC_MODE", P::InharmonicOn),
        ("$ENGINE_PAR_WT_MOD_TUNE", P::Shown),
        ("$ENGINE_PAR_RELEASE_TRIGGER", P::ReleaseTrigger),
        ("$ENGINE_PAR_OUTPUT_CHANNEL", P::OutputChannel),
        ("$ENGINE_PAR_EFFECT_BYPASS", P::EffectBypass),
        ("$ENGINE_PAR_EFFECT_SUBTYPE", P::EffectSubtype),
        ("$ENGINE_PAR_EFFECT_TYPE", P::EffectType),
        ("$ENGINE_PAR_SEND_EFFECT_TYPE", P::EffectType),
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
        // RV2_MOD is not carried.
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
        // get_zone_par's; the fades, volume, pan, tune and BPM are not read.
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
