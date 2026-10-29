//! Bounded opt-in events for offline diagnostics; compiled out of normal builds.
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
        resume: bool,
    },
    /// A script called an engine action. Arguments beyond the first eight are omitted.
    Action {
        frame: u64,
        slot: u8,
        callback: u32,
        action: Action,
        args: [i64; 8],
        len: u8,
        truncated: bool,
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
        };
        let event = TraceEvent::Callback {
            frame: 0,
            slot: 0,
            kind: 1,
            id: 1,
            resume: false,
        };
        trace.push(event);
        trace.push(event);
        assert_eq!(trace.events.len(), 1);
        assert_eq!(trace.dropped, 1);
    }
}
