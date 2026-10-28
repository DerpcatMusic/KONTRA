//! The script host. Callbacks run at their sample and act on notes through
//! [`Action`]s. The event of a note callback, and the notes it plays, start
//! when the callback returns or first waits; `wait` resumes through the queue.

use kontra_events::HostNote;
use kontra_time::SampleTime;
use kontra_vm::{Action, HostApi, HostCall, HostValue, Program, Text, event};

use crate::Engine;
use crate::ledger::{Note, NoteRef, Origin};
use crate::queue::Work;

/// A compiled script and where its callbacks start.
#[derive(Clone, Debug)]
pub struct ScriptPlan {
    pub program: Program,
    pub callbacks: Callbacks,
    /// Callbacks that may run or wait at once.
    pub coroutines: usize,
    /// Library data arrays `load_array(%x, 2)` reads: (first cell, values).
    pub data: Vec<(u32, Vec<i64>)>,
    /// Saved values written after `on init`, before `on persistence_changed`.
    pub restore: Vec<(u32, i64)>,
    pub restore_texts: Vec<(u32, Text)>,
    pub controls: Controls,
    pub engine_pars: EnginePars,
    /// The translator's law for each reverb control, in `ir::ReverbPar::ALL` order.
    pub reverb_laws: [crate::ir::Law; 8],
    /// Code in another language, run by its own machine instead of `program`.
    pub foreign: Option<std::sync::Arc<dyn kontra_vm::Foreign>>,
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
    /// A modulator's bypass.
    pub mod_bypass: i32,
    pub attack_curve: i32,
    pub effect_bypass: i32,
    pub effect_output: i32,
    /// A send effect's bypass, wet (or return) level and dry level.
    pub send_bypass: i32,
    pub send_output: i32,
    pub send_dry: i32,
    /// A Send Levels slot's level to each send slot.
    pub send_levels: [i32; 8],
    /// A reverb control's id, in `ir::ReverbPar::ALL` order.
    pub reverb: [i32; 8],
    pub gainer_gain: i32,
    pub stereo_width: i32,
    pub stereo_pan: i32,
    pub volume: i32,
    pub pan: i32,
    pub tune: i32,
}

/// The script's UI controls as the engine sees them: ids, and where values live.
#[derive(Clone, Debug, Default)]
pub struct Controls {
    /// The UI id of variable 0.
    pub first: i32,
    /// The parameter id that means a control's value.
    pub value_par: i32,
    /// By UI id from `first` (every variable has one), the value cell of
    /// each control whose value is a number.
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
    pub listener: Option<u32>,
    pub pgs_changed: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Init,
    Note,
    Release,
    Controller,
    Listener,
    Pgs,
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
    /// `$NI_CALLBACK_TYPE` of the callback, kept across waits.
    pub(crate) callback_type: u8,
    /// `$NI_CALLBACK_ID`: unique per callback run, kept across waits.
    pub(crate) callback_id: u32,
    /// The script slot running it.
    pub(crate) slot: u8,
    /// `$NI_SIGNAL_TYPE` of a listener callback.
    pub(crate) signal: u8,
}

impl Running {
    pub(crate) const IDLE: Self = Self {
        kind: Kind::Resumed,
        event: None,
        controller: 0,
        ignore_release: false,
        ignore_controller: false,
        callback_type: 0,
        callback_id: 0,
        slot: 0,
        signal: 0,
    };

    pub(crate) fn new(kind: Kind, event: Option<NoteRef>) -> Self {
        // KSP's $NI_CB_TYPE_* values (measured, probes/callback_constants.lua).
        let callback_type = match kind {
            Kind::Init | Kind::Resumed => 5,
            Kind::Note => 1,
            Kind::Release => 2,
            Kind::Controller => 3,
            Kind::Listener => 12,
            Kind::Pgs => 8,
        };
        Self {
            kind,
            event,
            callback_type,
            ..Self::IDLE
        }
    }

    /// The coroutine context, so a resumed callback still knows its event
    /// (low 32 bits), callback type (8), callback id (20: a UI id, 32768 plus
    /// a variable index) and listener signal (4).
    pub(crate) fn context(&self) -> u64 {
        u64::from(self.event.map_or(0, |n| n.event_id() as u32))
            | u64::from(self.callback_type) << 32
            | u64::from(self.callback_id & 0xF_FFFF) << 40
            | u64::from(self.signal & 0xF) << 60
    }

    pub(crate) fn resumed(context: u64) -> Self {
        Self {
            kind: Kind::Resumed,
            event: NoteRef::from_event_id(context as u32 as i32),
            callback_type: (context >> 32) as u8,
            callback_id: (context >> 40) as u32 & 0xF_FFFF,
            signal: (context >> 60) as u8,
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

/// KSP's `$NI_NOT_FOUND`, what the `get_*_idx` lookups answer for a miss
/// (real Kontakt 8, `probes/ni_not_found.lua`).
const NOT_FOUND: i32 = -100;

pub(crate) fn micros_to_frames(micros: i64, rate: u32) -> u64 {
    (micros.max(0) as u128 * u128::from(rate) / 1_000_000) as u64
}

impl Engine {
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
        // -1 ends with the note that started the callback; 0 plays the whole
        // sample; a positive duration is a note-off after that long.
        // Once the starting note is released (its release callback), -1 has
        // nothing left to wait for and the sample plays out.
        let ending = self.running.kind == Kind::Release
            || parent
                .and_then(|p| self.notes.get(p))
                .is_some_and(|n| n.released);
        note.origin = match (duration, parent) {
            (-1, Some(p)) if !ending => Origin::Child(p),
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
        // Only the slots after this one see the note.
        let first = self.running.slot.saturating_add(1);
        if let Some(n) = self.notes.get_mut(r) {
            n.first = first;
            n.pass = first;
        }
        let list = if usize::from(first) < self.plan.scripts.len() {
            &mut self.forward
        } else {
            &mut self.pending
        };
        if list.len() < list.capacity() {
            list.push(r);
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
            self.queue_end(note);
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
            event::ZONE_SLOT => n.zone_slot,
            event::SOURCE => match n.origin {
                Origin::Host => -1,
                Origin::Child(_) | Origin::Script => i32::from(n.first) - 1,
            },
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
            event::ZONE_SLOT if !n.started => n.zone_slot = value,
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
            .count(|n| n.origin == Origin::Host && i32::from(n.host.key) == key && !n.is_up())
            > 0
    }

    /// The value sent every event for "from script" modulator `index`; 0 until sent.
    pub(crate) fn script_mod(&self, index: u16) -> i32 {
        crate::ledger::ramp_at(&self.script_mods, index, self.now).unwrap_or(0)
    }

    fn random(&mut self, low: i32, high: i32) -> i32 {
        let (low, high) = (low.min(high), low.max(high));
        let span = (i64::from(high) - i64::from(low) + 1) as u64;
        (i64::from(low) + (u64::from(self.behaviour.random()) % span) as i64) as i32
    }
}

impl HostApi for Engine {
    fn call(&mut self, mut call: HostCall<'_>) -> HostValue {
        if let Some(n) = self.called.get_mut(usize::from(call.action.id())) {
            *n += 1;
        }
        if let Some(value) = self.pgs_call(&mut call) {
            return value;
        }
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        if let Some(value) = self.transport_value(call.action, int(0)) {
            return value;
        }
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
            Action::GroupVolume | Action::GroupPan | Action::GroupTune => {
                self.set_group_level(call.action, int(0), int(1));
                HostValue::Nothing
            }
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
            Action::SetEventParArr if int(1) == event::MOD_VALUE => {
                if let Ok(index) = u16::try_from(int(3)) {
                    // An optional fifth argument ramps there over microseconds.
                    let (value, now) = (int(2), self.now);
                    let frames =
                        u64::try_from(int(4)).unwrap_or(0) * u64::from(self.rate) / 1_000_000;
                    let from = self.script_mod(index);
                    if frames > 0 {
                        // A step past the end lands the final value.
                        let end = now.0 + frames + u64::from(self.rate / 100);
                        self.ramping_until = self.ramping_until.max(end);
                    }
                    if int(0) == event::ALL {
                        // Sent to every event: notes that start later read it too.
                        crate::ledger::set_ramp(
                            &mut self.script_mods,
                            index,
                            value,
                            now,
                            frames,
                            from,
                        );
                    }
                    self.each(int(0), |n| n.set_mod_value(index, value, now, frames, from));
                }
                HostValue::Nothing
            }
            Action::GetEventParArr if int(1) == event::MOD_VALUE => {
                let note = NoteRef::from_event_id(int(0)).and_then(|r| self.notes.get(r));
                let index = u16::try_from(int(2)).ok();
                HostValue::Int(note.zip(index).map_or(0, |(n, i)| {
                    n.mod_value(i, self.now)
                        .unwrap_or_else(|| self.script_mod(i))
                }))
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
            Action::SignalType => HostValue::Int(i32::from(self.running.signal)),
            Action::SetListener | Action::ChangeListenerPar => {
                self.set_listener(int(0), int(1));
                HostValue::Nothing
            }
            Action::CurrentScriptSlot => HostValue::Int(i32::from(self.running.slot)),
            Action::MidiChannel => HostValue::Int(current.map_or(0, |n| i32::from(n.host.channel))),
            Action::NoteHeld => flag(current.is_some_and(|n| !n.is_up())),
            Action::PlayedVoicesInst | Action::PlayedVoicesTotal => {
                HostValue::Int(self.active_voices() as i32)
            }
            Action::CallbackType => HostValue::Int(i32::from(self.running.callback_type)),
            Action::CallbackId => HostValue::Int(self.running.callback_id as i32),
            Action::EventParAt => HostValue::Int(
                running.map_or(0, |r| self.event_par(r.event_id(), int(0).clamp(0, 3))),
            ),
            Action::NumZones | Action::GetNumZones => HostValue::Int(self.plan.zones.len() as i32),
            // find_group answers 0 for an unknown name, get_group_idx $NI_NOT_FOUND.
            Action::FindGroup | Action::GetGroupIdx => {
                let name = call.texts.first().map_or("", Text::as_str);
                let found = self.plan.groups.iter().position(|g| g.name == name);
                let missing = if call.action == Action::FindGroup {
                    0
                } else {
                    NOT_FOUND
                };
                HostValue::Int(found.map_or(missing, |g| g as i32))
            }
            Action::GroupName => {
                let name = usize::try_from(int(0))
                    .ok()
                    .and_then(|g| self.plan.groups.get(g))
                    .map_or("", |g| g.name.as_str());
                *call.text_out = Text::from_str_cut(name).0;
                HostValue::Text
            }
            // ponytail: answered at once, no on async_complete; the samples
            // stay in memory until the host reloads without the group.
            Action::PurgeGroup => {
                if let Some(g) = self.groups.get_mut(int(0) as usize) {
                    g.purged = int(1) == 0;
                }
                HostValue::Int(0)
            }
            Action::GetPurgeState => HostValue::Int(
                self.groups
                    .get(int(0) as usize)
                    .map_or(0, |g| i32::from(!g.purged)),
            ),
            // Any modulator, carried or not. A miss: find_mod answers 0 (real
            // Kontakt 8, probes/find_mod_index.lua), get_mod_idx $NI_NOT_FOUND.
            Action::FindMod | Action::GetModIdx => {
                let name = call.texts.first().map_or("", Text::as_str);
                let found = self.groups.get(int(0) as usize).and_then(|g| {
                    let route = g.routes.iter().find(|r| r.name == name);
                    route
                        .map(|r| r.slot)
                        .or_else(|| g.mods.iter().find(|m| m.name == name).map(|m| m.slot))
                });
                let missing = if call.action == Action::FindMod {
                    0
                } else {
                    NOT_FOUND
                };
                HostValue::Int(found.map_or(missing, i32::from))
            }
            Action::FindTarget | Action::GetTargetIdx => {
                let name = call.texts.first().map_or("", Text::as_str);
                let found = self.groups.get(int(0) as usize).and_then(|g| {
                    let routes = g.routes.iter().find(|r| i32::from(r.slot) == int(1))?;
                    routes.names.iter().position(|n| n == name)
                });
                // get_target_idx misses with $NI_NOT_FOUND (measured); find_target
                // answers 0 like find_mod and find_group (not measured).
                let missing = if call.action == Action::FindTarget {
                    0
                } else {
                    NOT_FOUND
                };
                HostValue::Int(found.map_or(missing, |n| n as i32))
            }
            Action::GetEnginePar
                if let Some(value) = self.get_engine_par(int(0), int(1), int(2), int(3)) =>
            {
                HostValue::Int(value)
            }
            Action::SetEnginePar if self.set_engine_par(int(0), int(1), int(2), int(3), int(4)) => {
                HostValue::Nothing
            }
            Action::SetController => {
                self.set_controller(int(0), int(1));
                HostValue::Nothing
            }
            // Mode 2 is the library's data folder, read at once (the next line
            // sees it). ponytail: text arrays and the user-file modes wait for a
            // text-cell host API and file dialogs.
            Action::LoadArray if int(2) == 2 && int(3) != 2 => {
                let (base, len) = (int(0) as u32, int(1) as usize);
                let data = self.script().map(|s| s.data.as_slice());
                if let Some((_, values)) = data.unwrap_or_default().iter().find(|(b, _)| *b == base)
                {
                    let cells = call.cells.get_mut(base as usize..).unwrap_or_default();
                    for (cell, value) in cells.iter_mut().zip(values).take(len) {
                        *cell = *value;
                    }
                }
                HostValue::Int(0)
            }
            Action::ReadPersistentVar => {
                let whole = |i| u32::try_from(int(i)).unwrap_or(0);
                let (base, len) = (whole(0), whole(1));
                let saved = self.script().map(|s| s.restore.as_slice());
                for &(cell, value) in saved.unwrap_or_default() {
                    if (base..base.saturating_add(len)).contains(&cell)
                        && let Some(c) = call.cells.get_mut(cell as usize)
                    {
                        *c = value;
                    }
                }
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
