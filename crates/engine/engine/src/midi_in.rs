//! `on midi_in` and `on note_controller`: scripts that see MIDI before the
//! engine does (the KSP manual's "Multi Script" chapter), and the
//! per-note controllers one script slot sends another.
//!
//! Each script slot with `on midi_in` runs in slot order on every incoming
//! note, note-off and channel message. It may change the message
//! (`set_event_par($EVENT_PAR_MIDI_COMMAND / _BYTE_1 / _BYTE_2 / _CHANNEL)`),
//! drop it (`ignore_midi`, the same flag `ignore_controller` sets, read as a
//! [`Heard`]) and send more (`set_midi`, which skip the slots
//! that already ran). What is left reaches the engine's own handlers.

use kontra_events::{Event, HostNote, HostPattern};
use kontra_vm::midi;

use crate::midi::Heard;
use crate::{Engine, Kind, Running};

/// `set_midi` messages one callback may add before the rest are dropped.
pub(crate) const MADE: usize = 16;

impl Engine {
    /// `event` as MIDI bytes `[status nibble, channel, data 1, data 2]`, when
    /// `on midi_in` sees it: notes with an exact key and channel, and channel
    /// messages.
    fn midi_bytes(event: &Event) -> Option<[u8; 4]> {
        match *event {
            Event::NoteOn { note, velocity, .. } => Some([
                midi::NOTE_ON,
                note.channel,
                note.key,
                ((velocity * 127.0).round() as u8).clamp(1, 127),
            ]),
            Event::NoteOff(p) | Event::NoteOffVelocity(p, _) => {
                let (channel, key) = (u8::try_from(p.channel).ok()?, u8::try_from(p.key).ok()?);
                Some([midi::NOTE_OFF, channel, key, 0])
            }
            Event::Ump([word, _]) if word >> 28 == 0x2 => {
                let status = (word >> 16) as u8;
                Some([
                    status & 0xF0,
                    status & 0x0F,
                    (word >> 8) as u8 & 0x7F,
                    word as u8 & 0x7F,
                ])
            }
            _ => None,
        }
    }

    /// The event for `m`, keeping the host's note identity when `like` was a note.
    fn event_from(m: [u8; 4], like: &Event) -> Event {
        let [cmd, channel, d1, d2] = m;
        let note_like = match like {
            Event::NoteOn { note, .. } => Some(*note),
            _ => None,
        };
        match cmd {
            midi::NOTE_ON if d2 > 0 => Event::NoteOn {
                note: HostNote {
                    channel,
                    key: d1,
                    ..note_like.unwrap_or(HostNote {
                        port: 0,
                        channel,
                        key: d1,
                        id: -1,
                        clap: false,
                    })
                },
                velocity: f64::from(d2) / 127.0,
                tune: match like {
                    Event::NoteOn { tune, .. } => *tune,
                    _ => 0.0,
                },
            },
            midi::NOTE_ON | midi::NOTE_OFF => Event::NoteOff(HostPattern {
                port: -1,
                channel: i32::from(channel),
                key: i32::from(d1),
                id: match like {
                    Event::NoteOff(p) | Event::NoteOffVelocity(p, _) => p.id,
                    _ => -1,
                },
                clap: false,
            }),
            _ => Event::midi1(cmd | channel, d1, d2),
        }
    }

    /// Runs the scripts' `on midi_in` on `event`; `None` when one ignored it.
    /// What `set_midi` made is handed to `apply` after the scripts ran.
    pub(crate) fn through_midi_in(
        &mut self,
        event: Event,
        apply: &mut dyn FnMut(&mut Self, Event),
    ) -> Option<Event> {
        let Some(mut m) = Self::midi_bytes(&event) else {
            return Some(event);
        };
        let plan = std::sync::Arc::clone(&self.plan);
        let mut ignored = false;
        for (slot, script) in (0u8..).zip(&plan.scripts) {
            let Some(entry) = script.callbacks.midi_in else {
                continue;
            };
            self.midi_made.0 = 0;
            let running = Running {
                midi: m,
                ..Running::new(Kind::MidiIn, None)
            };
            let ran = self.run_callback(slot, entry, running);
            m = ran.midi;
            ignored |= Heard::of(&ran) != Heard::Passed;
            let made = std::mem::take(&mut self.midi_made);
            for &bytes in made.1.iter().take(made.0) {
                apply(self, Self::event_from(bytes, &event));
            }
            if ignored {
                return None;
            }
        }
        Some(Self::event_from(m, &event))
    }

    /// `set_midi` from the running `on midi_in`.
    pub(crate) fn set_midi(&mut self, bytes: [u8; 4]) {
        let (n, made) = &mut self.midi_made;
        if let Some(slot) = made.get_mut(*n) {
            *slot = bytes;
            *n += 1;
        } else {
            self.stats.events_refused += 1;
        }
    }

    /// `set_note_controller(controller, note, value)` from script slot `from`:
    /// every other slot's `on note_controller` runs with `$NC_NUM`,
    /// `$NC_NOTE` and `$NC_VALUE`. (Kontakt receives no MIDI 2.0 from outside;
    /// the manual's only source is another script slot.)
    pub(crate) fn note_controller(&mut self, from: u8, controller: i32, note: i32, value: i32) {
        let plan = std::sync::Arc::clone(&self.plan);
        for (slot, script) in (0u8..).zip(&plan.scripts) {
            let Some(entry) = script.callbacks.note_controller.filter(|_| slot != from) else {
                continue;
            };
            let running = Running {
                controller: controller.clamp(0, 255) as u8,
                midi: [0, 0, note.clamp(0, 127) as u8, 0],
                value,
                ..Running::new(Kind::NoteController, None)
            };
            self.run_callback(slot, entry, running);
        }
    }
}
