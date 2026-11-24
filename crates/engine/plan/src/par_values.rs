//! What a script parameter sets in a level or an effect slot, in the normalized
//! `0..=1_000_000` encoding.

use kontra_ir as ir;

use crate::par_laws::{CUBE, cube_root};
use crate::{EnginePar, Level};

/// Effect gains are cubic, unity at this value, so 1_000_000 is +24 dB.
pub const UNITY_EFFECT_GAIN: f64 = 396_851.0;

/// Volume is cubic in the setting, 0 dB at this value, +12 dB at 1_000_000.
pub const UNITY_VOLUME: f64 = 629_960.524_947_436_1;

/// Bus n is `BUS_OFFSET + n` in the default rack addressing scheme.
pub const BUS_OFFSET: i32 = 1024;

/// Format-neutral rack addressing scheme mapping `(group, generic)` integers
/// from a script host onto [`ir::RackAt`] and bus indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RackScheme {
    pub send_rack: i32,
    pub insert_rack: i32,
    pub main_rack: i32,
    pub bus_offset: i32,
}

impl RackScheme {
    /// Standard rack numbering: send 0, insert 1 (or -1), main 2, buses `1024 + n`.
    pub const DEFAULT: Self = Self {
        send_rack: 0,
        insert_rack: 1,
        main_rack: 2,
        bus_offset: BUS_OFFSET,
    };

    /// Resolves `(group, generic)` into its [`ir::RackAt`].
    #[must_use]
    pub fn rack_at(self, group: i32, generic: i32) -> Option<ir::RackAt> {
        if let Ok(g) = u32::try_from(group) {
            return Some(ir::RackAt::Group(g));
        }
        if generic == self.send_rack {
            Some(ir::RackAt::Sends)
        } else if generic == self.insert_rack || generic == -1 {
            Some(ir::RackAt::Inserts)
        } else if generic == self.main_rack {
            Some(ir::RackAt::Main)
        } else {
            let bus = u8::try_from(generic.checked_sub(self.bus_offset)?).ok()?;
            Some(ir::RackAt::Bus(bus))
        }
    }

    /// Converts a bus `generic` selector to its plan bus index (`bus_n + 1`,
    /// where bus 0 is the main output bus).
    #[must_use]
    pub fn plan_bus(self, generic: i32) -> Option<usize> {
        let offset = usize::try_from(generic.checked_sub(self.bus_offset)?).ok()?;
        offset.checked_add(1)
    }
}

/// Which of a [`Level`]'s settings a parameter is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LevelPar {
    Volume,
    Pan,
    Tune,
}

impl LevelPar {
    /// Sets it in `level` from a raw 0..=1_000_000 value.
    pub fn set(self, level: &mut Level, raw: f64) {
        match self {
            Self::Volume => level.gain = CUBE.eval_unclamped_f64(raw / UNITY_VOLUME) as f32,
            Self::Pan => level.pan = ((raw - 500_000.0) / 500_000.0) as f32,
            // ±36 semitones.
            Self::Tune => level.tune = (raw - 500_000.0) * 7.2e-5,
        }
    }

    /// Its raw 0..=1_000_000 value in `level`.
    pub fn get(self, level: &Level) -> f64 {
        match self {
            Self::Volume => UNITY_VOLUME * cube_root(level.gain),
            Self::Pan => f64::from(level.pan) * 500_000.0 + 500_000.0,
            Self::Tune => level.tune / 7.2e-5 + 500_000.0,
        }
    }
}

/// What an effect parameter sets in a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

    /// Its raw 0..=1_000_000 value in `e`; `None` when the slot has no such
    /// control.
    pub fn get(self, e: &ir::Effect) -> Option<f64> {
        let gain = |g: f32| UNITY_EFFECT_GAIN * cube_root(g);
        Some(match (self, &e.kind) {
            (Self::Bypass, _) => f64::from(u8::from(e.bypass)),
            (Self::Output, _) => gain(e.output),
            (Self::Dry, _) => gain(e.dry),
            (Self::Send(n), ir::EffectKind::Sends(levels)) => {
                UNITY_VOLUME * cube_root(*levels.get(n)?)
            }
            (Self::Gain, ir::EffectKind::Gain(g)) => gain(*g),
            (Self::Width, ir::EffectKind::Stereo { width, .. }) => f64::from(*width) * 1e6,
            (Self::Pan, ir::EffectKind::Stereo { pan, .. }) => (f64::from(*pan) + 1.0) * 5e5,
            _ => return None,
        })
    }
}

/// The effect setting `par` addresses; `None` for any other parameter.
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

/// The level setting `par` addresses; `None` for any other parameter.
pub fn level_par(par: EnginePar) -> Option<LevelPar> {
    Some(match par {
        EnginePar::Volume => LevelPar::Volume,
        EnginePar::Pan => LevelPar::Pan,
        EnginePar::Tune => LevelPar::Tune,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::RackScheme;

    #[test]
    fn plan_bus_rejects_selectors_below_bus_offset() {
        let s = RackScheme::DEFAULT;
        assert_eq!(s.plan_bus(s.bus_offset - 1), None);
        assert_eq!(s.plan_bus(s.bus_offset), Some(1));
        assert_eq!(s.plan_bus(i32::MIN), None);
    }
}
