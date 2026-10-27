//! The script host. Callbacks run at their sample and act on notes through
//! [`Action`]s. The event of a note callback, and the notes it plays, start
//! when the callback returns or first waits; `wait` resumes through the queue.

use kontra_events::HostNote;
use kontra_time::SampleTime;
use kontra_vm::{Action, HostApi, HostCall, HostValue, Program, Step, Text, event};

use crate::ledger::{Note, NoteRef, Origin};
use crate::queue::Work;
use crate::{Engine, Trigger};

/// A compiled script and where its callbacks start.
#[derive(Clone, Debug)]
pub struct ScriptPlan {
    pub program: Program,
    pub callbacks: Callbacks,
    /// Callbacks that may run or wait at once.
    pub coroutines: usize,
    /// Saved values written after `on init`, before `on persistence_changed`.
    pub restore: Vec<(u32, i64)>,
    pub restore_texts: Vec<(u32, Text)>,
    pub controls: Controls,
    pub engine_pars: EnginePars,
}

/// The script's numbers for the engine parameters the engine answers.
#[derive(Clone, Copy, Debug, Default)]
pub struct EnginePars {
    pub attack: i32,
    pub hold: i32,
    pub decay: i32,
    pub sustain: i32,
    pub release: i32,
    /// A volume modulation's intensity.
    pub intensity: i32,
}

/// The script's UI controls as the engine sees them: ids, and where values live.
#[derive(Clone, Debug, Default)]
pub struct Controls {
    /// Id of control 0; control `n` has id `first + n`.
    pub first: i32,
    /// The parameter id that means a control's value.
    pub value_par: i32,
    /// Each control's value cell, if its value is a number.
    pub cells: Vec<Option<u32>>,
}

/// Entry points; a missing callback lets events through unchanged.
#[derive(Clone, Debug, Default)]
pub struct Callbacks {
    pub init: Option<u32>,
    pub note: Option<u32>,
    pub release: Option<u32>,
    pub controller: Option<u32>,
    pub persistence_changed: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Init,
    Note,
    Release,
    Controller,
    /// After a wait: what the callback was no longer matters.
    Resumed,
}

/// The callback being run and what it decided about its event.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Running {
    pub(crate) kind: Kind,
    pub(crate) event: Option<NoteRef>,
    pub(crate) controller: u8,
    pub(crate) ignore_release: bool,
    pub(crate) ignore_controller: bool,
}

impl Running {
    pub(crate) const IDLE: Self = Self {
        kind: Kind::Resumed,
        event: None,
        controller: 0,
        ignore_release: false,
        ignore_controller: false,
    };

    pub(crate) fn new(kind: Kind, event: Option<NoteRef>) -> Self {
        Self {
            kind,
            event,
            ..Self::IDLE
        }
    }
}

/// Which groups each note may play, one bit row per ledger slot.
pub(crate) struct GroupMasks {
    words: usize,
    groups: usize,
    bits: Box<[u64]>,
}

impl GroupMasks {
    pub(crate) fn new(notes: usize, groups: usize) -> Self {
        let words = groups.div_ceil(64).max(1);
        Self {
            words,
            groups,
            bits: vec![u64::MAX; notes * words].into(),
        }
    }

    fn row(&mut self, slot: usize) -> &mut [u64] {
        let start = slot * self.words;
        self.bits
            .get_mut(start..start + self.words)
            .unwrap_or_default()
    }

    pub(crate) fn reset(&mut self, slot: usize) {
        self.row(slot).fill(u64::MAX);
    }

    pub(crate) fn copy(&mut self, from: usize, to: usize) {
        let (from, to) = (from * self.words, to * self.words);
        if from + self.words <= self.bits.len() && to + self.words <= self.bits.len() {
            self.bits.copy_within(from..from + self.words, to);
        }
    }

    /// A group past the last addresses every group.
    pub(crate) fn set(&mut self, slot: usize, group: i32, allowed: bool) {
        let all = usize::try_from(group).map_or(true, |g| g >= self.groups);
        let row = self.row(slot);
        if all {
            row.fill(if allowed { u64::MAX } else { 0 });
            return;
        }
        let g = group as usize;
        if let Some(word) = row.get_mut(g / 64) {
            let bit = 1u64 << (g % 64);
            if allowed {
                *word |= bit;
            } else {
                *word &= !bit;
            }
        }
    }

    pub(crate) fn allows(&self, slot: usize, group: u32) -> bool {
        let g = group as usize;
        self.bits
            .get(slot * self.words + g / 64)
            .is_none_or(|word| word & (1 << (g % 64)) != 0)
    }
}

fn micros_to_frames(micros: i64, rate: u32) -> u64 {
    (micros.max(0) as u128 * u128::from(rate) / 1_000_000) as u64
}

impl Engine {
    /// Runs a callback now. Notes it let through or played start before this returns.
    pub(crate) fn run_callback(&mut self, entry: u32, running: Running) -> Running {
        self.running = running;
        let context = running.event.map_or(0, |n| n.event_id() as u64);
        if let Some(mut vm) = self.vm.take() {
            if let Some(co) = vm.start(entry, context) {
                self.step_script(&mut vm, co);
            }
            self.vm = Some(vm);
        }
        self.start_pending();
        std::mem::replace(&mut self.running, Running::IDLE)
    }

    /// A waiting callback's time has come.
    pub(crate) fn resume(&mut self, co: kontra_vm::CoId) {
        if let Some(mut vm) = self.vm.take() {
            let event = vm
                .context(co)
                .and_then(|c| NoteRef::from_event_id(c as i32));
            self.running = Running::new(Kind::Resumed, event);
            self.step_script(&mut vm, co);
            self.vm = Some(vm);
        }
        self.start_pending();
        self.running = Running::IDLE;
    }

    fn step_script(&mut self, vm: &mut kontra_vm::Instance, co: kontra_vm::CoId) {
        let plan = self.plan.clone();
        let Some(script) = plan.script.as_ref() else {
            return;
        };
        if let Step::Wait(micros) = vm.run(&script.program, co, self) {
            let at = SampleTime(self.now.0 + micros_to_frames(micros as i64, self.rate).max(1));
            if !self.queue.push(at, Work::Resume(co)) {
                self.stats.events_refused += 1;
                vm.cancel(co);
            }
        }
    }

    /// Starts every note waiting for its callback, in arrival order.
    pub(crate) fn start_pending(&mut self) {
        let mut pending = std::mem::take(&mut self.pending);
        for &note in &pending {
            let Some(n) = self.notes.get_mut(note) else {
                continue;
            };
            if n.started {
                continue;
            }
            n.started = true;
            if n.ignored || n.released {
                continue;
            }
            let held = self
                .notes
                .count(|n| n.started && !n.ignored && !n.released && !n.pedal_held)
                - 1;
            self.start_zones(note, true, |trigger| match trigger {
                Trigger::Attack => true,
                Trigger::First => held == 0,
                Trigger::Legato => held > 0,
                Trigger::Release => false,
            });
            self.behaviour.advance();
        }
        pending.clear();
        self.pending = pending;
    }

    /// Fills `out` with the notes an event argument addresses.
    fn targets(&self, id: i32, out: &mut Vec<NoteRef>) {
        out.clear();
        if id == event::ALL {
            self.notes.select(out, |_| true);
        } else if id & event::BY_MARKS != 0 && id > 0 {
            let marks = (id & !event::BY_MARKS) as u32;
            self.notes.select(out, |n| n.marks & marks != 0);
        } else if let Some(note) =
            NoteRef::from_event_id(id).filter(|&n| self.notes.get(n).is_some())
        {
            out.push(note);
        }
    }

    /// Applies `f` to every addressed note, then refreshes the sounding ones.
    fn each(&mut self, id: i32, mut f: impl FnMut(&mut Note)) {
        let mut hits = std::mem::take(&mut self.targets_scratch);
        self.targets(id, &mut hits);
        for &note in &hits {
            if let Some(n) = self.notes.get_mut(note) {
                f(n);
                if n.started {
                    self.refresh_note(note, None);
                }
            }
        }
        self.targets_scratch = hits;
    }

    fn play_note(&mut self, key: i32, velocity: i32, offset: i32, duration: i32) -> i32 {
        let parent = self.running.event;
        let key = key.clamp(0, 127) as u8;
        let host = parent.and_then(|p| self.notes.get(p)).map_or(
            HostNote {
                port: 0,
                channel: 0,
                key,
                id: -1,
                clap: false,
            },
            |n| n.host,
        );
        let draw = self.behaviour.draw(key);
        let mut note = Note::new(
            HostNote { key, ..host },
            f64::from(velocity.clamp(1, 127)) / 127.0,
            0.0,
            self.now,
            draw,
        );
        note.offset = offset.max(0) as u32;
        note.origin = match (duration, parent) {
            (0, Some(p)) => Origin::Child(p),
            _ => Origin::Script,
        };
        let Some(r) = self.notes.insert(note) else {
            self.stats.notes_refused += 1;
            return 0;
        };
        match parent {
            Some(p) => self.masks.copy(p.slot(), r.slot()),
            None => self.masks.reset(r.slot()),
        }
        if duration > 0 {
            let at = SampleTime(self.now.0 + micros_to_frames(i64::from(duration), self.rate));
            if !self.queue.push(at, Work::Release(r)) {
                self.stats.events_refused += 1;
            }
        }
        if self.pending.len() < self.pending.capacity() {
            self.pending.push(r);
        }
        r.event_id()
    }

    fn ignore_event(&mut self, id: i32) {
        if self.running.kind == Kind::Release
            && self.running.event.map(NoteRef::event_id) == Some(id)
        {
            self.running.ignore_release = true;
            return;
        }
        self.each(id, |n| {
            if !n.started {
                n.ignored = true;
            }
        });
    }

    fn script_note_off(&mut self, id: i32) {
        let mut hits = std::mem::take(&mut self.targets_scratch);
        self.targets(id, &mut hits);
        for &note in &hits {
            self.release(note);
        }
        self.targets_scratch = hits;
    }

    fn fade_out(&mut self, id: i32, micros: i32) {
        let frames = micros_to_frames(i64::from(micros), self.rate).max(1) as u32;
        let mut hits = std::mem::take(&mut self.targets_scratch);
        self.targets(id, &mut hits);
        for &note in &hits {
            if let Some(n) = self.notes.get_mut(note) {
                // ponytail: a fade always ends the voices (KSP stop_voice = 1);
                // fade_out without stopping, and fade_in, wait for the gain ramp API.
                n.released = true;
            }
            for slot in self.voices.iter_mut().filter(|s| s.note == Some(note)) {
                slot.voice.fade(frames);
                slot.fading = true;
            }
        }
        self.targets_scratch = hits;
    }

    fn event_par(&self, id: i32, par: i32) -> i32 {
        let Some(n) = NoteRef::from_event_id(id).and_then(|r| self.notes.get(r)) else {
            return 0;
        };
        match par {
            0..=event::CUSTOM_LAST => n.custom.get(par as usize).copied().unwrap_or(0),
            event::VOLUME => n.script_volume,
            event::TUNE => n.script_tune,
            event::PAN => n.script_pan,
            event::NOTE => i32::from(n.key),
            event::VELOCITY => i32::from(n.velocity_byte),
            _ => 0,
        }
    }

    fn set_event_par(&mut self, id: i32, par: i32, value: i32) {
        self.each(id, |n| match par {
            0..=event::CUSTOM_LAST => {
                if let Some(c) = n.custom.get_mut(par as usize) {
                    *c = value;
                }
            }
            event::VOLUME => n.script_volume = value,
            event::TUNE => n.script_tune = value,
            event::PAN => n.script_pan = value.clamp(-1000, 1000),
            event::NOTE if !n.started => n.key = value.clamp(0, 127) as u8,
            event::VELOCITY if !n.started => {
                n.velocity_byte = value.clamp(1, 127) as u8;
                n.velocity = f64::from(n.velocity_byte) / 127.0;
            }
            _ => {}
        });
    }

    fn set_group(&mut self, id: i32, group: i32, allowed: bool) {
        let mut hits = std::mem::take(&mut self.targets_scratch);
        self.targets(id, &mut hits);
        for &note in &hits {
            self.masks.set(note.slot(), group, allowed);
        }
        self.targets_scratch = hits;
    }

    fn key_down(&self, key: i32) -> bool {
        self.notes
            .count(|n| n.origin == Origin::Host && i32::from(n.host.key) == key && !n.released)
            > 0
    }

    fn random(&mut self, low: i32, high: i32) -> i32 {
        let (low, high) = (low.min(high), low.max(high));
        let span = (i64::from(high) - i64::from(low) + 1) as u64;
        (i64::from(low) + (u64::from(self.behaviour.random()) % span) as i64) as i32
    }
}

impl HostApi for Engine {
    fn call(&mut self, call: HostCall<'_>) -> HostValue {
        if let Some(n) = self.called.get_mut(usize::from(call.action.id())) {
            *n += 1;
        }
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        let running = self.running.event;
        let current = running.and_then(|r| self.notes.get(r));
        let flag = |b: bool| HostValue::Int(i32::from(b));
        match call.action {
            Action::EventId => HostValue::Int(running.map_or(0, NoteRef::event_id)),
            Action::EventNote => HostValue::Int(current.map_or(0, |n| i32::from(n.key))),
            Action::EventVelocity => {
                HostValue::Int(current.map_or(0, |n| i32::from(n.velocity_byte)))
            }
            Action::CcNum => HostValue::Int(i32::from(self.running.controller)),
            Action::Cc => HostValue::Int(
                usize::try_from(int(0))
                    .ok()
                    .and_then(|i| self.cc_values.get(i))
                    .map_or(0, |&v| i32::from(v)),
            ),
            Action::KeyDown => flag(self.key_down(int(0))),
            Action::EngineUptime => {
                HostValue::Int((self.now.0 * 1000 / u64::from(self.rate)) as i32)
            }
            Action::NumGroups => HostValue::Int(self.masks.groups as i32),
            Action::Random => HostValue::Int(self.random(int(0), int(1))),
            Action::Message => {
                self.last_message = call.texts.first().copied().unwrap_or(Text::EMPTY);
                self.stats.messages += 1;
                HostValue::Nothing
            }
            Action::IgnoreController => {
                self.running.ignore_controller = true;
                HostValue::Nothing
            }
            Action::IgnoreEvent => {
                self.ignore_event(int(0));
                HostValue::Nothing
            }
            Action::PlayNote => HostValue::Int(self.play_note(int(0), int(1), int(2), int(3))),
            Action::NoteOff => {
                self.script_note_off(int(0));
                HostValue::Nothing
            }
            Action::FadeOut => {
                self.fade_out(int(0), int(1));
                HostValue::Nothing
            }
            Action::ChangeTune | Action::ChangeVol | Action::ChangePan => {
                let (value, relative) = (int(1), int(2) != 0);
                let action = call.action;
                self.each(int(0), |n| {
                    let field = match action {
                        Action::ChangeTune => &mut n.script_tune,
                        Action::ChangeVol => &mut n.script_volume,
                        _ => &mut n.script_pan,
                    };
                    *field = if relative {
                        field.saturating_add(value)
                    } else {
                        value
                    };
                    n.script_pan = n.script_pan.clamp(-1000, 1000);
                });
                HostValue::Nothing
            }
            Action::ChangeNote => {
                self.set_event_par(int(0), event::NOTE, int(1));
                HostValue::Nothing
            }
            Action::ChangeVelo => {
                self.set_event_par(int(0), event::VELOCITY, int(1));
                HostValue::Nothing
            }
            Action::SetEventPar => {
                self.set_event_par(int(0), int(1), int(2));
                HostValue::Nothing
            }
            Action::GetEventPar => HostValue::Int(self.event_par(int(0), int(1))),
            Action::SetEventParArr if int(1) == event::ALLOW_GROUP => {
                self.set_group(int(0), int(3), int(2) != 0);
                HostValue::Nothing
            }
            Action::GetEventParArr if int(1) == event::ALLOW_GROUP => {
                let slot = NoteRef::from_event_id(int(0)).map(NoteRef::slot);
                flag(slot.is_some_and(|s| self.masks.allows(s, int(2).max(0) as u32)))
            }
            Action::AllowGroup | Action::DisallowGroup => {
                if let Some(event) = running {
                    let allowed = call.action == Action::AllowGroup;
                    self.masks.set(event.slot(), int(0), allowed);
                }
                HostValue::Nothing
            }
            Action::SetEventMark | Action::DeleteEventMark => {
                let (mark, set) = (int(1) as u32, call.action == Action::SetEventMark);
                self.each(int(0), |n| {
                    n.marks = if set { n.marks | mark } else { n.marks & !mark };
                });
                HostValue::Nothing
            }
            Action::GetEventMark => {
                let marks = NoteRef::from_event_id(int(0))
                    .and_then(|r| self.notes.get(r))
                    .map_or(0, |n| n.marks);
                flag(marks & int(1) as u32 != 0)
            }
            Action::ByMarks => HostValue::Int(event::BY_MARKS | (int(0) & !event::BY_MARKS)),
            Action::GetControlPar => HostValue::Int(self.control_par(call.cells, int(0), int(1))),
            Action::SetControlPar => {
                self.set_control_par(call.cells, int(0), int(1), int(2));
                HostValue::Nothing
            }
            Action::FindMod => {
                let name = call.texts.first().map_or("", Text::as_str);
                let found = self.groups.get(int(0) as usize).and_then(|g| {
                    g.mods
                        .iter()
                        .find(|m| m.name == name)
                        .map(|m| i32::from(m.slot))
                });
                HostValue::Int(found.unwrap_or(-1))
            }
            Action::SetEnginePar if self.set_engine_par(int(0), int(1), int(2), int(3)) => {
                HostValue::Nothing
            }
            Action::SetController => {
                self.set_controller(int(0), int(1));
                HostValue::Nothing
            }
            Action::EventStatus => {
                let live = NoteRef::from_event_id(int(0)).and_then(|r| self.notes.get(r));
                HostValue::Int(i32::from(live.is_some()))
            }
            other => {
                self.stats.actions_unhandled += 1;
                if let Some(n) = self.unhandled.get_mut(usize::from(other.id())) {
                    *n += 1;
                }
                HostValue::Nothing
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_masks() {
        let mut m = GroupMasks::new(2, 100);
        assert!(m.allows(0, 99));
        m.set(0, event::ALL_GROUPS, false);
        assert!(!m.allows(0, 3));
        m.set(0, 70, true);
        assert!(m.allows(0, 70) && !m.allows(0, 69));
        m.copy(0, 1);
        assert!(m.allows(1, 70) && !m.allows(1, 3));
        m.reset(1);
        assert!(m.allows(1, 3));
    }

    #[test]
    fn event_ids_round_trip_and_reject_garbage() {
        assert_eq!(NoteRef::from_event_id(0), None);
        assert_eq!(NoteRef::from_event_id(-5), None);
        assert_eq!(NoteRef::from_event_id(17), None);
    }
}
