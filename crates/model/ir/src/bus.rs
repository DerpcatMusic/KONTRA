//! Output buses and the sends between them.

#[derive(Clone, Debug, PartialEq)]
pub struct Bus {
    pub name: String,
    pub volume_db: f32,
    /// -1..=1 balance.
    pub pan: f32,
    /// Effects on what the bus carries, by slot.
    pub inserts: Vec<Option<crate::Effect>>,
    /// Later buses it feeds, after its effects and before its pan.
    pub sends: Vec<BusSend>,
    /// Plays on the main output; otherwise it only feeds its sends.
    pub to_main: bool,
}

/// A bus's feed into a later bus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusSend {
    /// Index into [`crate::Instrument::buses`], above the sender's.
    pub bus: usize,
    /// Linear.
    pub level: f32,
    /// Scaled by the sender's volume as it changes.
    pub post_fader: bool,
}
