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
use kontra_time::{Frames, SampleTime};
use kontra_vm::midi;

use crate::midi::Heard;
use crate::queue::{Due, Work};
use crate::{CallbackContext, Engine, MAX_SPAN, Play, Running};

/// `set_midi` messages held in the inline ring before overflow is counted.
pub(crate) const MADE: usize = 64;

impl Engine {
    /// `event` as MIDI bytes `[status nibble, channel, data 1, data 2]`, when
    /// `on midi_in` sees it: notes with an exact key and channel, and channel
    /// messages. The channel counts across ports, 0..=63 (KSP
    /// `$MIDI_CHANNEL`).
    fn midi_bytes(event: &Event) -> Option<[u8; 4]> {
        let mut m = Self::port_bytes(event)?;
        m[1] += event.port().unwrap_or(0).min(3) * 16;
        Some(m)
    }

    fn port_bytes(event: &Event) -> Option<[u8; 4]> {
        match *event {
            Event::NoteOn { note, velocity, .. } => Some([
                midi::NOTE_ON,
                note.channel,
                note.key,
                ((velocity * 127.0).round() as u8).clamp(1, 127),
            ]),
            Event::NoteOff(p) => {
                let (channel, key) = (u8::try_from(p.channel).ok()?, u8::try_from(p.key).ok()?);
                Some([midi::NOTE_OFF, channel, key, 0])
            }
            Event::NoteOffVelocity(p, v) => {
                let (channel, key) = (u8::try_from(p.channel).ok()?, u8::try_from(p.key).ok()?);
                Some([
                    midi::NOTE_OFF,
                    channel,
                    key,
                    ((v * 127.0).round() as u8).min(127),
                ])
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
        let [cmd, across, d1, d2] = m;
        // Kontakt's channel across ports back to a port and its channel.
        let (port, channel) = (across / 16 % 4, across % 16);
        let note_like = match like {
            Event::NoteOn { note, .. } => Some(*note),
            _ => None,
        };
        match cmd {
            midi::NOTE_ON if d2 > 0 => Event::NoteOn {
                note: HostNote {
                    port,
                    channel,
                    key: d1,
                    ..note_like.unwrap_or(HostNote {
                        port,
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
            midi::NOTE_ON | midi::NOTE_OFF => {
                let pattern = HostPattern {
                    port: i32::from(port),
                    channel: i32::from(channel),
                    key: i32::from(d1),
                    id: match like {
                        Event::NoteOff(p) | Event::NoteOffVelocity(p, _) => p.id,
                        _ => -1,
                    },
                    clap: false,
                };
                if cmd == midi::NOTE_OFF && (d2 > 0 || matches!(like, Event::NoteOffVelocity(..))) {
                    Event::NoteOffVelocity(pattern, f64::from(d2) / 127.0)
                } else {
                    Event::NoteOff(pattern)
                }
            }
            _ => match Event::midi1(cmd | channel, d1, d2) {
                Event::Ump([word, extra]) => Event::Ump([word | u32::from(port) << 24, extra]),
                other => other,
            },
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
        let heard = m;
        let id = i32::from(self.next_midi_id);
        self.next_midi_id = self.next_midi_id.wrapping_add(1).max(1);
        let plan = std::sync::Arc::clone(&self.plan);
        let mut ignored = false;
        for (slot, script) in (0u8..).zip(&plan.scripts) {
            let Some(entry) = script.callbacks.midi_in else {
                continue;
            };
            let base = self.midi_made.0;
            let running = Running::new(CallbackContext::MidiIn { bytes: m, id });
            let ran = self.run_callback(slot, entry, running);
            m = ran.midi();
            ignored |= Heard::of(&ran) != Heard::Passed;
            let end = self.midi_made.0.min(MADE);
            let mut batch = [[0u8; 4]; MADE];
            let count = end.saturating_sub(base);
            if let (Some(dst), Some(src)) =
                (batch.get_mut(..count), self.midi_made.1.get(base..end))
            {
                dst.copy_from_slice(src);
            }
            self.midi_made.0 = base;
            for &bytes in batch.iter().take(count) {
                apply(self, Self::rebuilt(bytes, heard, &event));
            }
            if ignored {
                return None;
            }
        }
        // Untouched, the host's event goes on as sent: a note-off on any
        // port (wildcard) still ends notes held on ports B–D.
        if m == heard {
            return Some(event);
        }
        Some(Self::rebuilt(m, heard, &event))
    }

    /// The event `m` makes from `like` (heard as `heard`). A note-off on any
    /// port (wildcard) that a script moved but kept on its channel stays on
    /// any port, so the moved note still ends on ports B–D.
    fn rebuilt(m: [u8; 4], heard: [u8; 4], like: &Event) -> Event {
        let mut out = Self::event_from(m, like);
        if let (
            Event::NoteOff(was) | Event::NoteOffVelocity(was, _),
            Event::NoteOff(now) | Event::NoteOffVelocity(now, _),
        ) = (like, &mut out)
            && was.port < 0
            && m[1] == heard[1]
        {
            now.port = was.port;
        }
        out
    }

    /// Converts standalone `set_midi` bytes (from a delayed `wait`, `on
    /// listener`, or `on ui_control`) into an [`Event`].
    fn midi_event(bytes: [u8; 4]) -> Event {
        Self::event_from(bytes, &Event::midi1(0xB0, 0, 0))
    }

    /// Drains any pending `set_midi` messages produced by resumed waits,
    /// `on listener`, or `on ui_control` into `out` at `offset`, without
    /// growing `out` past its pre-allocated capacity.
    fn drain_midi_made(&mut self, offset: u32, out: &mut Vec<(u32, Event)>) {
        let count = self.midi_made.0.min(MADE);
        self.midi_made.0 = 0;
        for &bytes in self.midi_made.1.iter().take(count) {
            if out.len() < out.capacity() {
                out.push((offset, Self::midi_event(bytes)));
            } else {
                self.stats.events_refused += 1;
            }
        }
    }

    /// Drives MIDI-learned UI controls on a multi-script engine when an
    /// incoming CC arrives (instrument engines handle links in `controller`).
    fn apply_multi_cc_links(&mut self, event: &Event) {
        if let Some([0xB0, _, controller, value]) = Self::midi_bytes(event) {
            let plan = std::sync::Arc::clone(&self.plan);
            for (slot, script) in (0u8..).zip(&plan.scripts) {
                for link in script.links.iter().filter(|l| l.cc == controller) {
                    self.set_control(slot, link.id, link.value(value));
                }
            }
        }
    }

    /// A multi script's turn at `event`: its `on midi_in` runs, and what it
    /// lets through or sends (`set_midi`) goes to `out`, for the rack's
    /// parts. The engine itself plays none of it.
    pub fn midi_in(&mut self, event: Event, out: &mut dyn FnMut(Event)) {
        self.apply_multi_cc_links(&event);
        let mut made = |_: &mut Self, e: Event| out(e);
        if let Some(e) = self.through_midi_in(event, &mut made) {
            out(e);
        }
    }

    /// Sample-accurate multi-script block processor: forwards host `tempo` and
    /// `transport`, schedules `input` at each event's exact sample offset,
    /// advances the script clock / `wait(...)` resumes / `on listener` timers
    /// across `frames`, and emits every passed-through or `set_midi` event
    /// (including delayed, listener, and pre-block `ui_control` emissions) into
    /// `out` with its exact block-relative sample offset. Zero heap allocations
    /// on the audio thread (never grows `out` past `out.capacity()`).
    pub fn process_midi(
        &mut self,
        frames: usize,
        input: &[(u32, Event)],
        tempo: Option<f32>,
        transport: Option<(Play, f64, (u8, u8))>,
        out: &mut Vec<(u32, Event)>,
    ) {
        out.clear();
        if let Some(bpm) = tempo {
            self.set_tempo(bpm);
        }
        if let Some((play, quarters, signature)) = transport {
            self.set_transport(play, quarters, signature);
        }
        // Emit any `set_midi` queued between blocks (e.g. from `on ui_control`).
        self.drain_midi_made(0, out);

        let start = self.now;
        let frames_u32 = u32::try_from(frames).unwrap_or(u32::MAX);
        let end = start.advance(Frames(frames_u32));
        for &(at, event) in input {
            let clamped = at.min(frames_u32.saturating_sub(1));
            if !self
                .queue
                .push(SampleTime(start.0 + u64::from(clamped)), Work::Input(event))
            {
                self.stats.events_refused += 1;
            }
        }
        self.block_end = end;
        self.block_turns = 0;
        for vm in self.vms.iter_mut().flatten() {
            vm.begin_block();
        }
        while self.now < end {
            let mut resumes = 0;
            while let Some((due, work)) = self.queue.pop_due(self.now) {
                if let Work::Resume(..) = work {
                    resumes += 1;
                    if resumes > self.queue.capacity() {
                        let next = Due::at(SampleTime(self.now.0 + 1));
                        self.stats.resumes_deferred += 1;
                        if !self.queue.push_due(next, work) {
                            self.stats.events_refused += 1;
                        }
                        continue;
                    }
                }
                let offset = u32::try_from(self.now.0.saturating_sub(start.0)).unwrap_or(u32::MAX);
                match work {
                    Work::Input(event) => {
                        self.apply_multi_cc_links(&event);
                        let mut emit = |engine: &mut Self, e: Event| {
                            if out.len() < out.capacity() {
                                out.push((offset, e));
                            } else {
                                engine.stats.events_refused += 1;
                            }
                        };
                        if let Some(passed) = self.through_midi_in(event, &mut emit) {
                            emit(self, passed);
                        }
                    }
                    other => {
                        self.dispatch(due, other);
                        self.drain_midi_made(offset, out);
                    }
                }
            }
            let until = self
                .queue
                .next_at()
                .map_or(end, |at| at.min(end))
                .min(SampleTime((self.now.0 / MAX_SPAN + 1) * MAX_SPAN));
            let len = (until.0 - self.now.0) as usize;
            self.advance_transport(len);
            self.follow_knobs(false);
            let offset = u32::try_from(self.now.0.saturating_sub(start.0)).unwrap_or(u32::MAX);
            self.drain_midi_made(offset, out);
            self.now = until;
        }
    }

    /// `set_midi` from `on midi_in`, a resumed wait, `on listener`, or `on ui_control`.
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
        let context = CallbackContext::NoteController {
            cc: controller.clamp(0, 255) as u8,
            note: note.clamp(0, 127) as u8,
            value,
        };
        for (slot, script) in (0u8..).zip(&plan.scripts) {
            let Some(entry) = script.callbacks.note_controller.filter(|_| slot != from) else {
                continue;
            };
            self.run_callback(slot, entry, Running::new(context));
        }
    }
}
