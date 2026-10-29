//! Bounded opt-in events for offline diagnostics; compiled out of normal builds.
use kontra_vm::{Action, Text};
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
    /// A text argument of a script action, including empty text.
    ActionText {
        call_id: u64,
        index: usize,
        value: Text,
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

pub(crate) struct Trace {
    pub events: Vec<TraceEvent>,
    pub dropped: u64,
    pub next_action: u64,
}

impl Trace {
    pub(crate) fn voice(&mut self, frame: u64, id: usize, slot: &crate::Slot, event: VoiceEvent) {
        self.push(TraceEvent::Voice {
            frame,
            slot: id,
            zone: slot.zone,
            event,
            position: slot.voice.position(),
            step: slot.voice.step(),
            gain: slot.voice.current_gain(),
        });
    }
    pub fn push(&mut self, event: TraceEvent) {
        if self.events.len() < self.events.capacity() {
            self.events.push(event);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_trace_counts_overflow_without_growing() {
        let mut trace = Trace {
            events: Vec::with_capacity(1),
            dropped: 0,
            next_action: 0,
        };
        let event = TraceEvent::Callback {
            frame: 0,
            slot: 0,
            kind: 1,
            id: 1,
            note_id: None,
            controller: 0,
            control: 0,
            resume: false,
        };
        trace.push(event);
        trace.push(event);
        assert_eq!(trace.events.len(), 1);
        assert_eq!(trace.dropped, 1);
    }
}
