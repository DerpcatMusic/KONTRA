//! Event data for opt-in engine inspection.
use kontra_vm::Action;
use kontra_voice::Gain;

/// One event captured by the engine at its sample clock.
#[derive(Clone, Copy, Debug)]
pub enum TraceEvent {
    /// Script callback starts or resumes.
    Callback {
        frame: u64,
        slot: u8,
        kind: u8,
        id: u32,
        note_id: Option<i32>,
        controller: u8,
        control: i32,
        resume: bool,
    },
    /// A script called an engine action; extra integer and all text arguments follow.
    Action {
        frame: u64,
        slot: u8,
        callback: u32,
        action: Action,
        call_id: u64,
        args: [i64; 8],
        len: u8,
        integer_count: usize,
        text_count: usize,
    },
    /// An integer argument beyond the fixed inline prefix.
    ActionInteger {
        call_id: u64,
        index: usize,
        value: i64,
    },
    /// A UTF-8 chunk of a text argument, including empty text.
    ActionText {
        call_id: u64,
        index: usize,
        chunk: usize,
        len: u8,
        value: [u8; 96],
    },
    /// Source sampled at voice start for a note-on-only route.
    RouteOnset {
        frame: u64,
        voice_slot: usize,
        zone: usize,
        mod_slot: u16,
        target: u16,
        kind: kontra_ir::ModKind,
        source: Option<f32>,
    },
    /// A voice was started, stolen, cut or ended.
    Voice {
        frame: u64,
        slot: usize,
        zone: usize,
        event: VoiceEvent,
        position: f64,
        step: f64,
        gain: Gain,
    },
}

/// Voice lifecycle transition. End timestamps are the end of the render span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceEvent {
    Start,
    Steal,
    Cut,
    End,
}
