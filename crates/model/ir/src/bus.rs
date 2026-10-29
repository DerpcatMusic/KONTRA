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
    /// Routes moving its inserts ([`ModKind::EffectParam`], `insert` a slot
    /// of [`Self::inserts`]) and its sends' levels ([`ModKind::Send`]), from sources no note owns (a controller, pitch
    /// bend, a script's value for every note, or [`Source::Blend`] of
    /// [`Self::blends`]).
    ///
    /// [`ModKind::EffectParam`]: crate::ModKind::EffectParam
    /// [`ModKind::Send`]: crate::ModKind::Send
    /// [`Source::Blend`]: crate::Source::Blend
    pub mods: Vec<crate::ModKind>,
    pub blends: Vec<crate::Blend>,
}

impl Bus {
    /// A bus at unity on the main output, with no effects or sends.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            volume_db: 0.0,
            pan: 0.0,
            inserts: Vec::new(),
            sends: Vec::new(),
            to_main: true,
            mods: Vec::new(),
            blends: Vec::new(),
        }
    }
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
