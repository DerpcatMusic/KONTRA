#![forbid(unsafe_code)]
//! UMP v1.1.2 word framing and channel-voice decoding. Word byte order is the
//! transport's responsibility. No allocation, protocol negotiation or clock inference.
mod ingress;
pub use ingress::{Applied, ApplyError, Ingress};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    Midi1,
    Midi2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    Bits7(u8),
    Bits14(u16),
    Bits16(u16),
    Bits32(u32),
}
impl Value {
    /// Exact integer precision is retained until a consumer asks for normalization.
    pub fn normalized(self) -> f64 {
        match self {
            Self::Bits7(v) => f64::from(v) / 127.0,
            Self::Bits14(v) => f64::from(v) / 16383.0,
            Self::Bits16(v) => f64::from(v) / 65535.0,
            Self::Bits32(v) => f64::from(v) / f64::from(u32::MAX),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attribute {
    pub kind: u8,
    pub data: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controllers {
    Registered,
    Assignable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    NoteOn {
        key: u8,
        velocity: Value,
        attribute: Attribute,
    },
    NoteOff {
        key: u8,
        velocity: Option<Value>,
        attribute: Attribute,
    },
    PolyPressure {
        key: u8,
        value: Value,
    },
    Control {
        index: u8,
        value: Value,
    },
    Program {
        program: u8,
        bank: Option<[u8; 2]>,
    },
    ChannelPressure(Value),
    PitchBend(Value),
    PerNotePitch {
        key: u8,
        value: u32,
    },
    PerNoteControl {
        space: Controllers,
        key: u8,
        index: u8,
        value: u32,
    },
    ChannelControl {
        space: Controllers,
        bank: u8,
        index: u8,
        value: u32,
    },
    RelativeControl {
        space: Controllers,
        bank: u8,
        index: u8,
        delta: i32,
    },
    PerNoteManagement {
        key: u8,
        detach: bool,
        reset: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelVoice {
    pub version: Version,
    pub group: u8,
    pub channel: u8,
    pub message: Message,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Truncated {
    pub expected_words: usize,
    pub available_words: usize,
}

/// Complete borrowed packet, including unsupported/reserved message types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    words: &'a [u32],
}
impl<'a> Packet<'a> {
    pub fn words(self) -> &'a [u32] {
        self.words
    }
    pub fn message_type(self) -> u8 {
        (self.words[0] >> 28) as u8
    }

    /// Reserved fields are ignored, not required to be zero (spec section 2.1.3).
    /// Unknown opcodes and non-channel packets remain observable as raw packets.
    pub fn channel_voice(self) -> Option<ChannelVoice> {
        let first = self.words[0];
        let status = ((first >> 20) & 15) as u8;
        let a = ((first >> 8) & 127) as u8;
        let b = (first & 127) as u8;
        let (version, message) = match self.message_type() {
            2 => (Version::Midi1, midi1(status, a, b)?),
            4 => (Version::Midi2, midi2(status, first, self.words[1])?),
            _ => return None,
        };
        Some(ChannelVoice {
            version,
            group: ((first >> 24) & 15) as u8,
            channel: ((first >> 16) & 15) as u8,
            message,
        })
    }
}

/// Framing consumes one whole packet at a time, including reserved MT lengths.
/// An incomplete final packet reports once and terminates; payload is not resynced.
pub struct Packets<'a> {
    remaining: &'a [u32],
}
impl<'a> Packets<'a> {
    pub fn new(words: &'a [u32]) -> Self {
        Self { remaining: words }
    }
}
impl<'a> Iterator for Packets<'a> {
    type Item = Result<Packet<'a>, Truncated>;
    fn next(&mut self) -> Option<Self::Item> {
        let first = *self.remaining.first()?;
        let length = match first >> 28 {
            0..=2 | 6..=7 => 1,
            3..=4 | 8..=10 => 2,
            11..=12 => 3,
            _ => 4,
        };
        if self.remaining.len() < length {
            let error = Truncated {
                expected_words: length,
                available_words: self.remaining.len(),
            };
            self.remaining = &[];
            return Some(Err(error));
        }
        let (words, rest) = self.remaining.split_at(length);
        self.remaining = rest;
        Some(Ok(Packet { words }))
    }
}

fn midi1(status: u8, a: u8, b: u8) -> Option<Message> {
    let attribute = Attribute::default();
    Some(match status {
        8 => Message::NoteOff {
            key: a,
            velocity: Some(Value::Bits7(b)),
            attribute,
        },
        9 if b == 0 => Message::NoteOff {
            key: a,
            velocity: None,
            attribute,
        },
        9 => Message::NoteOn {
            key: a,
            velocity: Value::Bits7(b),
            attribute,
        },
        10 => Message::PolyPressure {
            key: a,
            value: Value::Bits7(b),
        },
        11 => Message::Control {
            index: a,
            value: Value::Bits7(b),
        },
        12 => Message::Program {
            program: a,
            bank: None,
        },
        13 => Message::ChannelPressure(Value::Bits7(a)),
        14 => Message::PitchBend(Value::Bits14(u16::from(a) | (u16::from(b) << 7))),
        _ => return None,
    })
}

fn midi2(status: u8, first: u32, data: u32) -> Option<Message> {
    let key = ((first >> 8) & 127) as u8;
    let index = first as u8;
    let space = if status & 1 == 0 {
        Controllers::Registered
    } else {
        Controllers::Assignable
    };
    let attribute = Attribute {
        kind: index,
        data: data as u16,
    };
    Some(match status {
        0 | 1 => Message::PerNoteControl {
            space,
            key,
            index,
            value: data,
        },
        2 | 3 => Message::ChannelControl {
            space,
            bank: key,
            index: index & 127,
            value: data,
        },
        4 | 5 => Message::RelativeControl {
            space,
            bank: key,
            index: index & 127,
            delta: data as i32,
        },
        6 => Message::PerNotePitch { key, value: data },
        8 => Message::NoteOff {
            key,
            velocity: Some(Value::Bits16((data >> 16) as u16)),
            attribute,
        },
        9 => Message::NoteOn {
            key,
            velocity: Value::Bits16((data >> 16) as u16),
            attribute,
        },
        10 => Message::PolyPressure {
            key,
            value: Value::Bits32(data),
        },
        11 => Message::Control {
            index: key,
            value: Value::Bits32(data),
        },
        12 => Message::Program {
            program: ((data >> 24) & 127) as u8,
            bank: (first & 1 != 0).then_some([((data >> 8) & 127) as u8, (data & 127) as u8]),
        },
        13 => Message::ChannelPressure(Value::Bits32(data)),
        14 => Message::PitchBend(Value::Bits32(data)),
        15 => Message::PerNoteManagement {
            key,
            detach: first & 2 != 0,
            reset: first & 1 != 0,
        },
        _ => return None,
    })
}
