//! The engine calls a KSP script makes on its events (`play_note`, `note_off`, ...).
//! One trait, two implementations: [`LiveSink`] forwards to a real
//! `sampler_core::Runtime`; [`Recorder`] logs the calls for trace comparison.
//!
//! Events are opaque [`EventId`]s chosen by the caller (the Luau host's note
//! number). Velocities are normalised to 0..=1 by the caller: the trait speaks
//! engine units, not KSP's 0..127. Durations are frames, already converted
//! from KSP microseconds with the interpreter's rule (`MicrosToFrames`: ceil).

use sampler_core::{Inheritance, NoteId, NotePitch, Runtime};
use std::collections::BTreeMap;

/// A script-visible event, as the caller numbers it.
pub type EventId = u32;

/// How long a `play_note` child lasts (KSP's duration argument).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Duration {
    /// Duration 0: the child sounds until the engine silences it.
    UntilSilent,
    /// Negative duration in a note callback: the child ends with its parent's gate.
    Gate,
    /// Positive duration: the child is released after this many frames.
    Frames(u64),
}

/// One `play_note` request. Inheritance is `Expression`, as the interpreter lowers it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Play {
    pub parent: EventId,
    pub key: u8,
    pub velocity: f64,
    pub duration: Duration,
}

pub trait EngineSink {
    /// KSP `play_note`: a child of `parent`. None when the engine refuses it.
    fn play(&mut self, request: Play) -> Option<EventId>;

    /// KSP `note_off`: release `event` now (`delay` None) or after `delay` frames.
    /// False when the engine does not know the event.
    fn note_off(&mut self, event: EventId, delay: Option<u64>) -> bool;
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
    fn play(&mut self, request: Play) -> Option<EventId> {
        let parent = *self.events.get(&request.parent)?;
        let linked = request.duration == Duration::Gate;
        let child = self
            .runtime
            .child_pitched(
                parent,
                NotePitch::Key(request.key),
                request.velocity,
                linked,
                Inheritance::Expression,
            )
            .ok()?;
        if let Duration::Frames(frames) = request.duration {
            let at = self.runtime.now() + frames;
            if self.runtime.release_at(child, at).is_err() {
                let _ = self.runtime.release(child);
                return None;
            }
        }
        let id = self.next;
        // ponytail: ids grow without reuse; add recycling when a long session nears u32::MAX.
        self.next += 1;
        self.events.insert(id, child);
        Some(id)
    }

    fn note_off(&mut self, event: EventId, delay: Option<u64>) -> bool {
        let Some(&note) = self.events.get(&event) else {
            return false;
        };
        match delay {
            None => self.runtime.release(note).is_ok(),
            Some(frames) => {
                let at = self.runtime.now() + frames;
                self.runtime.release_at(note, at).is_ok()
            }
        }
    }
}

/// One engine call, as [`Recorder`] logs it.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    Play(Play),
    NoteOff { event: EventId, delay: Option<u64> },
}

/// Logs every call and accepts all of them; child ids count up from 1.
#[derive(Default)]
pub struct Recorder {
    pub calls: Vec<Call>,
    next: EventId,
}

impl EngineSink for Recorder {
    fn play(&mut self, request: Play) -> Option<EventId> {
        self.calls.push(Call::Play(request));
        self.next += 1;
        Some(self.next)
    }

    fn note_off(&mut self, event: EventId, delay: Option<u64>) -> bool {
        self.calls.push(Call::NoteOff { event, delay });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_logs_calls_and_numbers_children() {
        let mut rec = Recorder::default();
        let request = Play {
            parent: 7,
            key: 60,
            velocity: 1.0 / 127.0,
            duration: Duration::Frames(480),
        };
        assert_eq!(rec.play(request), Some(1));
        assert!(rec.note_off(1, Some(12)));
        assert_eq!(
            rec.calls,
            vec![
                Call::Play(request),
                Call::NoteOff {
                    event: 1,
                    delay: Some(12)
                }
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
        let unbound = Play {
            parent: 2,
            key: 61,
            velocity: 0.5,
            duration: Duration::UntilSilent,
        };
        assert!(sink.play(unbound).is_none(), "unbound parent");
        let child = sink
            .play(Play {
                parent: 1,
                ..unbound
            })
            .expect("bound parent plays a child");
        assert!(sink.note_off(child, None));
        assert!(
            sink.play(Play {
                parent: 1,
                duration: Duration::Frames(480),
                ..unbound
            })
            .is_some()
        );
        assert!(!sink.note_off(999, None));
    }
}
