//! The engine calls a KSP script makes on its events (`play_note`, `note_off`, ...).
//! One trait, two implementations: [`LiveSink`] forwards to a real
//! `sampler_core::Runtime`; [`Recorder`] logs the calls for trace comparison.
//!
//! Events are opaque [`EventId`]s chosen by the caller (the Luau host's note
//! number). Velocities are normalised to 0..=1 by the caller: the trait speaks
//! engine units, not KSP's 0..127.

use sampler_core::{Inheritance, NoteId, NotePitch, Runtime};
use std::collections::BTreeMap;

/// A script-visible event, as the caller numbers it.
pub type EventId = u32;

pub trait EngineSink {
    /// KSP `play_note`: a child of `parent` at `key`. None when the engine refuses it.
    fn play_child(
        &mut self,
        parent: EventId,
        key: u8,
        velocity: f64,
        linked: bool,
        inheritance: Inheritance,
    ) -> Option<EventId>;

    /// KSP `note_off`: key `event` up now. False when the engine does not know it.
    fn note_off(&mut self, event: EventId, velocity: Option<f64>) -> bool;
}

/// Forwards to a live runtime. The driver registers each trigger's [`NoteId`] with [`LiveSink::bind`].
pub struct LiveSink {
    runtime: Runtime,
    events: BTreeMap<EventId, NoteId>,
    next: EventId,
}

impl LiveSink {
    pub fn new(runtime: Runtime) -> Self {
        Self {
            runtime,
            events: BTreeMap::new(),
            next: 1 << 16,
        }
    }

    /// Make `event` refer to `note`. Ids the driver binds must stay below the child id range.
    pub fn bind(&mut self, event: EventId, note: NoteId) {
        self.events.insert(event, note);
    }

    pub fn runtime(&mut self) -> &mut Runtime {
        &mut self.runtime
    }
}

impl EngineSink for LiveSink {
    fn play_child(
        &mut self,
        parent: EventId,
        key: u8,
        velocity: f64,
        linked: bool,
        inheritance: Inheritance,
    ) -> Option<EventId> {
        let parent = *self.events.get(&parent)?;
        let child = self
            .runtime
            .child_pitched(parent, NotePitch::Key(key), velocity, linked, inheritance)
            .ok()?;
        let id = self.next;
        // Ponytail: ids grow without reuse; add recycling when a long session nears u32::MAX.
        self.next += 1;
        self.events.insert(id, child);
        Some(id)
    }

    fn note_off(&mut self, event: EventId, velocity: Option<f64>) -> bool {
        let Some(&note) = self.events.get(&event) else {
            return false;
        };
        // Input-held notes take the release velocity; play_note children have no input,
        // so they are released without one.
        // ponytail: child release velocity is dropped; add a runtime path when a script depends on it.
        self.runtime.note_off_exact(note, velocity).is_ok() || self.runtime.release(note).is_ok()
    }
}

/// One engine call, as [`Recorder`] logs it.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    PlayChild {
        parent: EventId,
        key: u8,
        velocity: f64,
        linked: bool,
        inheritance: Inheritance,
    },
    NoteOff {
        event: EventId,
        velocity: Option<f64>,
    },
}

/// Logs every call and accepts all of them; child ids count up from 1.
#[derive(Default)]
pub struct Recorder {
    pub calls: Vec<Call>,
    next: EventId,
}

impl EngineSink for Recorder {
    fn play_child(
        &mut self,
        parent: EventId,
        key: u8,
        velocity: f64,
        linked: bool,
        inheritance: Inheritance,
    ) -> Option<EventId> {
        self.calls.push(Call::PlayChild {
            parent,
            key,
            velocity,
            linked,
            inheritance,
        });
        self.next += 1;
        Some(self.next)
    }

    fn note_off(&mut self, event: EventId, velocity: Option<f64>) -> bool {
        self.calls.push(Call::NoteOff { event, velocity });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_logs_calls_and_numbers_children() {
        let mut rec = Recorder::default();
        let child = rec.play_child(1, 60, 1.0 / 127.0, false, Inheritance::Independent);
        assert_eq!(child, Some(1));
        assert!(rec.note_off(1, None));
        assert_eq!(
            rec.calls,
            vec![
                Call::PlayChild {
                    parent: 1,
                    key: 60,
                    velocity: 1.0 / 127.0,
                    linked: false,
                    inheritance: Inheritance::Independent
                },
                Call::NoteOff {
                    event: 1,
                    velocity: None
                },
            ]
        );
    }

    #[test]
    fn live_sink_plays_a_child_of_a_bound_note_and_refuses_unknown_events() {
        use sampler_core::{Input, Limits, Prepared, Protocol};
        let script = crate::compile("on note end on", 48000, crate::Limits::LIBRARY, &[]).unwrap();
        let plan = script
            .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
            .unwrap();
        let limits = Limits::for_plan(&plan, 4, 4);
        let mut sink = LiveSink::new(Runtime::new(plan, limits).unwrap());
        let input = Input {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: None,
        };
        let note = sink.runtime().trigger(input, 60, 0.8).unwrap();
        sink.bind(1, note);
        assert!(
            sink.play_child(2, 61, 0.5, false, Inheritance::Independent)
                .is_none(),
            "unbound parent"
        );
        let child = sink.play_child(1, 61, 0.5, false, Inheritance::Independent);
        assert!(child.is_some());
        assert!(sink.note_off(child.unwrap(), None));
        assert!(!sink.note_off(999, None));
    }
}
