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

/// The names with an engine parameter of their own.
const FIXED: &[(&str, ir::EnginePar)] = {
    use ir::EnginePar as P;
    use ir::ReverbPar as R;
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
