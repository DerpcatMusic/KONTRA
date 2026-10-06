//! Host input as every core implementation receives it.
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
        self.clap == note.clap && (self.port == -1 || self.port == i32::from(note.port))
            && (self.channel == -1 || self.channel == i32::from(note.channel))
            && (self.key == -1 || self.key == i32::from(note.key))
            && (self.id == -1 || self.id == note.id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HostExpression { Gain(f32), Tune(f32), Pan(f32) }

/// What reaches a part.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum In {
    /// Exact owner, velocity and initial tuning in semitones.
    HostOn(HostNote, u8, f32),
    HostOff(HostPattern),
    HostChoke(HostPattern),
    HostExpression(HostPattern, HostExpression),
    NoteOn(u8, u8, u8),
    NoteOff(u8, u8),
    Cc(u8, u8, u8),
    /// 0..=16383, centre 8192.
    Bend(u8, u16),
    Pressure(u8, u8),
    PolyAt(u8, u8, u8),
    /// Host note expressions, by channel and key: semitones, pressure
    /// (0..=127), linear gain, pan (−1..=1), brightness (0..=127).
    NoteTune(u8, u8, f32),
    NotePressure(u8, u8, u8),
    NoteGain(u8, u8, f32),
    NotePan(u8, u8, f32),
    NoteBrightness(u8, u8, u8),
}

