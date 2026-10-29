//! Bounded opt-in events for offline diagnostics; compiled out of normal builds.

pub use kontra_trace::{RouteUnit, RouteValue, TraceEvent, VoiceEvent};

pub(crate) struct Trace {
    pub events: Vec<TraceEvent>,
    pub dropped: u64,
    pub next_action: u64,
    pub routes: kontra_trace::RouteBuffer,
}

impl Trace {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            events: Vec::with_capacity(capacity),
            dropped: 0,
            next_action: 0,
            routes: kontra_trace::RouteBuffer::new(capacity),
        }
    }
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
    pub(crate) fn text(&mut self, call_id: u64, index: usize, mut text: &str) {
        let mut chunk = 0;
        loop {
            let mut len = text.len().min(96);
            while !text.is_char_boundary(len) {
                len -= 1;
            }
            let mut value = [0; 96];
            if let (Some(dst), Some(src)) = (value.get_mut(..len), text.as_bytes().get(..len)) {
                dst.copy_from_slice(src);
            }
            self.push(TraceEvent::ActionText {
                call_id,
                index,
                chunk,
                len: len as u8,
                value,
            });
            text = text.get(len..).unwrap_or_default();
            if text.is_empty() {
                break;
            }
            chunk += 1;
        }
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
    fn text_chunks_preserve_unicode_and_empty_arguments() {
        let mut trace = Trace::new(16);
        let original = "界".repeat(80);
        trace.text(2, 0, &original);
        trace.text(2, 1, "");
        let mut joined = String::new();
        for event in &trace.events {
            if let TraceEvent::ActionText {
                index: 0,
                len,
                value,
                ..
            } = event
            {
                joined.push_str(
                    std::str::from_utf8(value.get(..usize::from(*len)).unwrap()).unwrap(),
                );
            }
        }
        assert_eq!(joined, original);
        assert!(trace.events.iter().any(|event| matches!(
            event,
            TraceEvent::ActionText {
                index: 1,
                len: 0,
                ..
            }
        )));
        assert_eq!(trace.dropped, 0);
    }

    #[test]
    fn bounded_trace_counts_overflow_without_growing() {
        let mut trace = Trace::new(1);
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

impl crate::Engine {
    pub(crate) fn trace_route(
        &self,
        zone: &crate::Zone,
        route: &kontra_ir::Mod,
        source: Option<f32>,
        value: f32,
        unit: RouteUnit,
    ) {
        let Some(trace) = &self.trace else { return };
        let Some(cc) = trace.routes.controller() else {
            return;
        };
        let direct = self
            .groups
            .get(zone.group as usize)
            .is_some_and(|g| kontra_plan::route_source(route.kind).is_some_and(|s| g.reads(s, cc)));
        trace
            .routes
            .push(self.now.0, route, source, value, unit, direct);
    }
}
