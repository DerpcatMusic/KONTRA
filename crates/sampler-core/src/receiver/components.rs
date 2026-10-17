//! Source components belong to the real expression owner. Physical NoteId
//! retirement cannot erase retained note tuning/cohort/member snapshots.
use super::{
    ReceiverId,
    per_note::Snapshot,
    state::{ChannelDomain, Controls},
};
use crate::Input;

#[derive(Clone, Copy)]
pub(crate) struct ReceiverComponents {
    pub receiver: ReceiverId,
    /// Generational component-origin identity; copies never key-match siblings.
    pub origin: Option<crate::ExpressionId>,
    pub domain: ChannelDomain,
    pub channel: u8,
    pub key: u8,
    pub source: Input,
    /// Latest accepted member state; frozen at the actual paired key-up.
    pub member: Controls,
    pub manager: Controls,
    pub tuning: f64,
    pub per_note: Option<Snapshot>,
    pub held: bool,
}

/// Wire precision is a typed input fact. Generated writes are explicitly
/// Script, even when the replacement happens to equal the original raw value.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ReceiverProvenance {
    Wire(super::Value),
    Script,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CapturedReceiver {
    pub receiver: ReceiverId,
    pub kind: ReceiverActionKind,
    pub provenance: ReceiverProvenance,
    pub source: crate::ChannelAddress,
    /// Global role/config actions validate their actual target epochs as a unit.
    pub host_epochs: Option<[u64; 16]>,
    pub policy_epochs: [u64; 16],
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ReceiverActionKind {
    /// Interpret the actual forwarded controller number in its current profile.
    Controller,
    /// An explicitly configured selected-MPE timbre-to-controller bridge.
    TimbreBridge,
}

#[derive(Clone, Copy)]
pub(crate) enum Gesture {
    Pitch,
    Pressure,
    Timbre,
    Reset,
    All,
}
#[derive(Clone, Copy)]
pub(crate) struct GestureProposal {
    pub controls: [[Controls; 16]; 3],
    pub managers: [[super::state::ManagerState; 16]; 3],
    pub domains: u8,
    pub channels: [Option<u8>; 3],
    pub gesture: Gesture,
    pub all_notes: bool,
    pub bends: [Option<(u8, super::Value)>; 3],
}
