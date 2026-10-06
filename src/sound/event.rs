//! Host input as the core receives it: exact host notes with per-note
//! expression, and every channel message as a Universal MIDI Packet.
//!
//! Plain `Copy` data: converting a host event allocates nothing, so the audio
//! thread can pass these straight to [`Core::event`](super::Core::event).

/// One exact host note identity (CLAP note ID or VST3 note ID).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostNote {
    pub port: u8,
    pub channel: u8,
    pub key: u8,
    pub id: i32,
    /// CLAP supports a plugin-to-host NOTE_END; VST3 does not.
    pub clap: bool,
}

/// Each -1 axis is a host wildcard. Invalid axes never match an owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostPattern {
    pub port: i32,
    pub channel: i32,
    pub key: i32,
    pub id: i32,
    pub clap: bool,
}

impl HostPattern {
    pub fn matches(self, note: HostNote) -> bool {
        self.clap == note.clap
            && (self.port == -1 || self.port == i32::from(note.port))
            && (self.channel == -1 || self.channel == i32::from(note.channel))
            && (self.key == -1 || self.key == i32::from(note.key))
            && (self.id == -1 || self.id == note.id)
    }
}

/// A per-note expression (CLAP note expression, MPE dimension).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoteExpression {
    /// Semitones from the note's key.
    Tune(f64),
    /// Linear, 0..=1.
    Gain(f64),
    /// −1..=1.
    Pan(f64),
    /// 0..=1.
    Pressure(f64),
    /// 0..=1.
    Brightness(f64),
}

/// What reaches the core.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// An exact host note: velocity 0..=1, initial tuning in semitones.
    NoteOn { note: HostNote, velocity: f64, tune: f64 },
    NoteOff(HostPattern),
    Choke(HostPattern),
    Expression(HostPattern, NoteExpression),
    /// A MIDI 1.0 (message type 2, one word used) or MIDI 2.0 (type 4)
    /// channel voice packet, its group the host port's.
    Ump([u32; 2]),
}

impl Event {
    /// A MIDI 1.0 channel voice message as a packet.
    pub fn midi1(status: u8, data1: u8, data2: u8) -> Self {
        Self::Ump([0x2000_0000 | u32::from(status) << 16 | u32::from(data1 & 127) << 8 | u32::from(data2 & 127), 0])
    }

    /// The MIDI channel this event is addressed to; None for host-wide patterns.
    pub fn channel(&self) -> Option<u8> {
        match self {
            Self::NoteOn { note, .. } => Some(note.channel),
            Self::NoteOff(p) | Self::Choke(p) | Self::Expression(p, _) => u8::try_from(p.channel).ok(),
            Self::Ump([word, _]) => Some((word >> 16) as u8 & 15),
        }
    }
}
