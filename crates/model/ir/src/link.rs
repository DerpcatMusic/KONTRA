//! MIDI controllers the instrument file ties to script controls.

/// A MIDI controller that moves one of a script's controls, as a user's
/// edit would: the control's value is stored and its callbacks run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CcLink {
    pub cc: u8,
    pub control: Linked,
    /// The part of the control's range the controller sweeps (0..1 each):
    /// controller 0 sets `from`, 127 sets `to`.
    pub from: f32,
    pub to: f32,
}

/// A route a format makes to a script knob by name: the knob (range
/// `lo..hi`) moves to `saved + m` of its range, held within it, where `m`
/// is `source` (a [`crate::Source::Blend`] of [`crate::Instrument::knob_blends`]
/// read at control rate, its oscillators [`crate::Instrument::knob_lfos`]),
/// and its callbacks run when it changes.
#[derive(Clone, Debug, PartialEq)]
pub struct KnobMod {
    /// The script, by its place in [`crate::Instrument::scripts`].
    pub script: usize,
    pub name: String,
    /// The knob's own value, which the route moves from.
    pub saved: f32,
    pub source: crate::Source,
}

/// The `n`th control of one kind its script declares (from 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Linked {
    Knob(u16),
    Slider(u16),
    Switch(u16),
}
