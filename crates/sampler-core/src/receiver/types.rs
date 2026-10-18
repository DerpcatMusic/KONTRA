//! Transport-independent typed receiver values. Framing/word decoding stays
//! in sampler-midi; Core receives an already validated channel-voice value.
use crate::NoteId;

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
    /// MIDI 2.0 (M2-104) min-center-max upscaling to 32 bits: zero, center
    /// and maximum land exactly on 0, 0x8000_0000 and u32::MAX, and the high
    /// bits still read back the original value.
    pub fn full_scale(self) -> u32 {
        let (value, bits) = match self {
            Self::Bits7(v) => (u32::from(v), 7),
            Self::Bits14(v) => (u32::from(v), 14),
            Self::Bits16(v) => (u32::from(v), 16),
            Self::Bits32(v) => return v,
        };
        let scale = 32 - bits;
        let shifted = value << scale;
        if value <= 1 << (bits - 1) {
            return shifted;
        }
        // Above center, repeat the low source bits through the new low bits.
        let repeat_bits = bits - 1;
        let low = value & ((1 << repeat_bits) - 1);
        let mut repeat = if scale > repeat_bits {
            low << (scale - repeat_bits)
        } else {
            low >> (repeat_bits - scale)
        };
        let mut out = shifted;
        while repeat != 0 {
            out |= repeat;
            repeat >>= repeat_bits;
        }
        out
    }

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
pub enum ApplyError {
    DisabledGroup,
    ProtocolMismatch,
    Core(crate::Error),
}
impl From<crate::Error> for ApplyError {
    fn from(value: crate::Error) -> Self {
        Self::Core(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    Started(NoteId),
    Expression {
        owners: usize,
    },
    Released {
        note: NoteId,
        velocity: Option<Value>,
        attribute: Attribute,
    },
    Pedal,
    /// Accepted effective selection CC; no implicit note or modulation is generated.
    Controller,
    /// A controller selector/state change, not a claim of full device configuration.
    Configuration,
    /// Accepted selection state/action, not a promise that a preset was loaded.
    ProgramChange { targets: usize },
    AllNotesOff {
        released: usize,
    },
    AllSoundOff {
        stopped: usize,
    },
    /// Reset All Controllers (CC121): the channel's controllers are back at
    /// their RP-015 reset values.
    ResetControllers,
    /// A recognized message intentionally has no effect in this receiver mode.
    Ignored,
    Unsupported,
}

impl Value {
    pub(super) fn valid(self) -> bool {
        match self {
            Self::Bits7(value) => value < 128,
            Self::Bits14(value) => value < 16384,
            Self::Bits16(_) | Self::Bits32(_) => true,
        }
    }
}
impl ChannelVoice {
    /// Public typed calls cannot rely on the four-bit packet decoder's bounds.
    pub(super) fn valid(self) -> bool {
        if self.group >= 16 || self.channel >= 16 {
            return false;
        }
        match self.message {
            Message::NoteOn { key, velocity, .. } => key < 128 && velocity.valid(),
            Message::NoteOff { key, velocity, .. } => {
                key < 128 && velocity.is_none_or(Value::valid)
            }
            Message::PolyPressure { key, value } => key < 128 && value.valid(),
            Message::Control { index, value } => index < 128 && value.valid(),
            Message::Program { program, bank } => {
                program < 128 && (self.version==Version::Midi2||bank.is_none()) && bank.is_none_or(|bank| bank.into_iter().all(|value| value < 128))
            }
            Message::ChannelPressure(value) | Message::PitchBend(value) => value.valid(),
            Message::PerNotePitch { key, .. }
            | Message::PerNoteControl { key, .. }
            | Message::PerNoteManagement { key, .. } => key < 128,
            Message::ChannelControl { bank, index, .. }
            | Message::RelativeControl { bank, index, .. } => bank < 128 && index < 128,
        }
    }
}


/// Real registered receiver attachment epoch; no Part/UI/controller epoch.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ReceiverWireEpoch { pub(crate) receiver:super::ReceiverId,pub(crate) serial:u64 }
/// The actual physical-decision arena generation, never a vector position.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct RawAdmissionId { pub(crate) receiver:super::ReceiverId,pub(crate) decision:crate::Handle }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct RawTerminalId { pub(crate) admission:RawAdmissionId }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum RawDecisionPhase {Pending,Accepted,Rejected}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum RawRejection {Capacity,Profile,Canceled,Core(crate::Error)}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum RawAdmission {Pending(RawAdmissionId),Rejected{decision:RawAdmissionId,reason:RawRejection}}
/// Original validated packet and immutable actual source at true ingress.
#[derive(Clone,Copy,Debug)]
pub struct RawAttackIntent {pub voice:ChannelVoice,pub source:crate::Input,pub frame:u64}
/// Producer's actual accepted logical route, distinct from physical source.
#[derive(Clone,Copy,Debug)]
pub struct CapturedLogicalRawAttack {pub input:crate::Input,pub pitch:crate::NotePitch,pub velocity:f64,pub tuning:f64,pub silent:bool}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct RawTerminalClaim {pub(crate) terminal:RawTerminalId,pub(crate) phase:RawDecisionPhase}
impl RawTerminalClaim {
    pub fn terminal(self)->RawTerminalId{self.terminal}
    pub fn admission(self)->RawAdmissionId{self.terminal.admission}
    pub fn phase(self)->RawDecisionPhase{self.phase}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum RawTerminalOutcome {Released(crate::NoteId),AbsorbedRejected,AbsorbedAlreadySynthetic}
/// Q is TOTAL queue+active+future obligations, not queued cells alone.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct WireAdmissionStatus {pub epoch:ReceiverWireEpoch,pub fenced_unknown_debt:bool,pub decisions:usize,pub pending:usize,pub accepted:usize,pub rejected:usize,pub claimed:usize,pub decision_capacity:usize,pub receipt_limit:usize,pub queued_receipts:usize,pub active_receipts:usize,pub future_receipts:usize,pub replacement_blocked:bool}

impl RawAdmissionId {pub fn receiver(self)->super::ReceiverId{self.receiver}}
impl RawTerminalId {pub fn admission(self)->RawAdmissionId{self.admission}}
