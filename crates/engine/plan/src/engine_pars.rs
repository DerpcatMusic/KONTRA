//! The engine parameters scripts read and set, by what they change. A script
//! names them by its own ids; the host maps those to [`EnginePar`] once, so the
//! engine never sees a format's parameter names.

use kontra_ir as ir;

/// What an engine parameter changes. Values arrive in 0..=1_000_000.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnginePar {
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    AttackCurve,
    /// A route's intensity knob: depth = x², the route keeps its sign.
    RouteIntensity,
    /// A route's signed depth, bipolar around 500_000: depth = (2x − 1)³.
    RouteDepth,
    /// A modulator's bypass.
    ModBypass,
    /// An LFO's fade-in time.
    LfoFade,
    /// An insert's or send effect's bypass, output level and dry level.
    EffectBypass,
    EffectOutput,
    EffectDry,
    /// A send-levels slot's level to send slot n.
    SendLevel(u8),
    Reverb(ir::ReverbPar),
    /// The reverb's network: n is entry n of the script plan's networks.
    ReverbNetwork,
    GainerGain,
    StereoWidth,
    StereoPan,
    /// The shape a filter slot switches to ([`ir::SlotShape::id`]).
    EffectSubtype,
    /// The effect a slot holds; not carried yet, answered asynchronously.
    EffectType,
    Volume,
    Pan,
    Tune,
    /// Read only: the group plays on release.
    ReleaseTrigger,
    /// The bus a group plays into: bus n is `1024 + n` (plan bus n + 1),
    /// −1 the main bus. On a bus (generic `1024 + n`), its own output.
    OutputChannel,
    /// Entry n of the script plan's knobs: a 0..=1 knob through its law.
    Knob(u16),
}

/// A script's parameter ids: id `first + n` is `by_id[n]`.
#[derive(Clone, Debug, Default)]
pub struct EnginePars {
    pub first: i32,
    pub by_id: Box<[Option<EnginePar>]>,
}

impl EnginePars {
    /// What parameter id `id` changes; `None` when the engine does not answer it.
    #[must_use]
    pub fn get(&self, id: i32) -> Option<EnginePar> {
        let n = usize::try_from(id.checked_sub(self.first)?).ok()?;
        self.by_id.get(n).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::{EnginePar, EnginePars};

    #[test]
    fn ids_resolve_from_first() {
        let pars = EnginePars {
            first: 100,
            by_id: vec![None, Some(EnginePar::Attack)].into(),
        };
        assert_eq!(pars.get(101), Some(EnginePar::Attack));
        assert_eq!(pars.get(100), None);
        assert_eq!(pars.get(99), None);
        assert_eq!(pars.get(i32::MIN), None);
    }
}
