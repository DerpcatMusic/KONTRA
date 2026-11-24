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
    /// The effect type identifier a slot holds (switched asynchronously when not in place).
    SlotKind,
    Volume,
    Pan,
    Tune,
    /// Read only: the group plays on release.
    ReleaseTrigger,
    /// The bus a group plays into: bus n is `bus_offset + n` (plan bus n + 1),
    /// −1 the main bus. On a bus, its own output.
    OutputChannel,
    /// Entry n of the script plan's knobs: a 0..=1 knob through its law.
    Knob(u16),
    /// Read only, by zone id: what a zone was saved with.
    Zone(ZonePar),
    /// Whether an LFO restarts with each voice (1) or runs free (0).
    LfoRetrigger,
    /// An LFO's rate unit: the value indexes the script's unit table (free
    /// Hz, or a note value whose count the rate knob then moves).
    LfoUnit,
    /// A wavetable value, 0..=1 linear in the raw 0..=1e6.
    Wavetable(crate::TableValue),
    /// The kind of wavetable form n (0 or 1): the value indexes the
    /// script's form table.
    WarpMode(u8),
    /// Whether a wavetable's partials stretch ([`crate::Inharmonic::on`]):
    /// 0 off, any other value on.
    InharmonicOn,
    /// A wavetable's mod oscillator kind: the value indexes the script's
    /// kind table.
    ModOscKind,
    /// A wavetable's mod oscillator waveform index (`0..=8`).
    TableModWave,
    /// What its tune reads in ([`crate::TuneUnit`]).
    ModOscUnit,
    /// A wavetable's oversampling/resampling quality index ([`crate::Quality`]).
    TableQuality,
    /// An opaque slot-local parameter not backed by a DSP processor, keyed by
    /// [`held_key`]: the slot stores and reads back the raw value without
    /// altering audio.
    Opaque(u32),
}

impl EnginePar {
    /// Compatibility constructor for [`Self::Opaque`].
    #[doc(hidden)]
    #[allow(non_snake_case)]
    #[must_use]
    pub const fn Held(key: u32) -> Self {
        Self::Opaque(key)
    }
}

/// What a script reads of a zone (`get_zone_par`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZonePar {
    LowKey,
    HighKey,
    LowVelocity,
    HighVelocity,
    RootKey,
    /// Its group's index.
    Group,
    /// Frames of the sample it plays, and how far a start offset may move.
    SampleStart,
    SampleEnd,
    StartRange,
    SampleRate,
    /// Whether it is selected in an editor (KONTRA has none: never).
    Selected,
    Volume,
    Pan,
    Tune,
    /// Fade lengths in keys / velocity steps at each edge of the range.
    FadeLowKey,
    FadeHighKey,
    FadeLowVelocity,
    FadeHighVelocity,
    /// The sample's tempo times 1000.
    Bpm,
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

/// The key a held control is known by in every script of an instrument:
/// FNV-1a of its name without case.
#[must_use]
pub const fn held_key(name: &str) -> u32 {
    let (mut h, mut rest) = (0x811c_9dc5_u32, name.as_bytes());
    while let [b, tail @ ..] = rest {
        h = (h ^ b.to_ascii_uppercase() as u32).wrapping_mul(0x0100_0193);
        rest = tail;
    }
    h
}
