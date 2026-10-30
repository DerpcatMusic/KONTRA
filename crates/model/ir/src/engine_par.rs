//! What a script's engine-parameter names drive: a neutral engine target,
//! the law from the script's 0..=1_000_000 value to it, and the text the
//! script reads back. A format translator supplies the table by name; the
//! engine sees only the targets.

use crate::{KnobTarget, Law, Readout};

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
    Reverb(crate::ReverbPar),
    /// The reverb's network: n is entry n of the script plan's networks.
    ReverbNetwork,
    GainerGain,
    StereoWidth,
    StereoPan,
    /// The shape a filter slot switches to ([`crate::SlotShape::id`]).
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

/// What one script parameter name drives.
#[derive(Clone, Debug, PartialEq)]
pub enum Drives {
    /// An engine parameter with its own law.
    Engine(EnginePar),
    /// A control the knob's `law` moves (value ÷ 1e6 is the knob x).
    Knob(KnobTarget, Law),
}

/// A script parameter: its name, what it drives and the text it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptPar {
    pub name: &'static str,
    pub drives: Drives,
    pub readout: Option<Readout>,
}
