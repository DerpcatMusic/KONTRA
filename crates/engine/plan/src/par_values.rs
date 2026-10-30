//! What an engine parameter sets in a level or an effect slot, in KSP's
//! 0..=1_000_000 encoding: Kontakt's laws (sampler-core `EngineParameterLaw`).

use kontra_ir as ir;

use crate::par_laws::{CUBE, cube_root};
use crate::{EnginePar, Level};

/// Effect gains are cubic, unity at this value, so 1_000_000 is +24 dB (v2 sampler-core,
/// fitted to slots whose saved gains follow their scripts' init writes).
pub const UNITY_EFFECT_GAIN: f64 = 396_851.0;

/// Volume is cubic in the setting, 0 dB at this value, +12 dB at 1_000_000.
pub const UNITY_VOLUME: f64 = 629_960.524_947_436_1;

/// Bus n is `BUS_OFFSET + n` (measured, probes/output_channel.lua).
pub const BUS_OFFSET: i32 = 1024;

/// Which of a [`Level`]'s settings a parameter is.
#[derive(Clone, Copy)]
pub enum LevelPar {
    Volume,
    Pan,
    Tune,
}

impl LevelPar {
    pub fn set(self, level: &mut Level, raw: f64) {
        match self {
            Self::Volume => level.gain = CUBE.eval_unclamped_f64(raw / UNITY_VOLUME) as f32,
            Self::Pan => level.pan = ((raw - 500_000.0) / 500_000.0) as f32,
            // ±36 semitones.
            Self::Tune => level.tune = (raw - 500_000.0) * 7.2e-5,
        }
    }

    pub fn get(self, level: &Level) -> f64 {
        match self {
            Self::Volume => UNITY_VOLUME * cube_root(level.gain),
            Self::Pan => f64::from(level.pan) * 500_000.0 + 500_000.0,
            Self::Tune => level.tune / 7.2e-5 + 500_000.0,
        }
    }
}

/// What an effect parameter sets in a slot.
#[derive(Clone, Copy)]
pub enum EffectPar {
    Bypass,
    Output,
    Dry,
    /// A Send Levels slot's level to send slot n.
    Send(usize),
    Gain,
    Width,
    Pan,
}

impl EffectPar {
    /// Sets it from the raw value; false when the slot has no such control.
    pub fn set(self, e: &mut ir::Effect, raw: f64) -> bool {
        let gain = CUBE.eval_unclamped_f64(raw / UNITY_EFFECT_GAIN) as f32;
        match (self, &mut e.kind) {
            (Self::Bypass, _) => e.bypass = raw != 0.0,
            (Self::Output, _) => e.output = gain,
            (Self::Dry, _) => e.dry = gain,
            // The volume law: Chorus saves 0.24994 for the init write 396821.
            (Self::Send(n), ir::EffectKind::Sends(levels)) => match levels.get_mut(n) {
                Some(level) => *level = CUBE.eval_unclamped_f64(raw / UNITY_VOLUME) as f32,
                None => return false,
            },
            (Self::Gain, ir::EffectKind::Gain(g)) => *g = gain,
            // ponytail: width and pan linear in the setting; no measurement yet.
            (Self::Width, ir::EffectKind::Stereo { width, .. }) => *width = (raw / 1e6) as f32,
            (Self::Pan, ir::EffectKind::Stereo { pan, .. }) => *pan = (raw / 5e5 - 1.0) as f32,
            _ => return false,
        }
        true
    }

    pub fn get(self, e: &ir::Effect) -> Option<f64> {
        let gain = |g: f32| UNITY_EFFECT_GAIN * cube_root(g);
        Some(match (self, e.kind) {
            (Self::Bypass, _) => f64::from(u8::from(e.bypass)),
            (Self::Output, _) => gain(e.output),
            (Self::Dry, _) => gain(e.dry),
            (Self::Send(n), ir::EffectKind::Sends(levels)) => {
                UNITY_VOLUME * cube_root(*levels.get(n)?)
            }
            (Self::Gain, ir::EffectKind::Gain(g)) => gain(g),
            (Self::Width, ir::EffectKind::Stereo { width, .. }) => f64::from(width) * 1e6,
            (Self::Pan, ir::EffectKind::Stereo { pan, .. }) => (f64::from(pan) + 1.0) * 5e5,
            _ => return None,
        })
    }
}

pub fn effect_par(par: EnginePar) -> Option<EffectPar> {
    Some(match par {
        EnginePar::EffectBypass => EffectPar::Bypass,
        EnginePar::EffectOutput => EffectPar::Output,
        EnginePar::EffectDry => EffectPar::Dry,
        EnginePar::SendLevel(n) => EffectPar::Send(usize::from(n)),
        EnginePar::GainerGain => EffectPar::Gain,
        EnginePar::StereoWidth => EffectPar::Width,
        EnginePar::StereoPan => EffectPar::Pan,
        _ => return None,
    })
}

pub fn level_par(par: EnginePar) -> Option<LevelPar> {
    Some(match par {
        EnginePar::Volume => LevelPar::Volume,
        EnginePar::Pan => LevelPar::Pan,
        EnginePar::Tune => LevelPar::Tune,
        _ => return None,
    })
}
