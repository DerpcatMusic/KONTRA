//! The script host. Callbacks run at their sample and act on notes through
//! [`Action`]s. The event of a note callback, and the notes it plays, start
//! when the callback returns or first waits; `wait` resumes through the queue.

use kontra_events::HostNote;
use kontra_time::SampleTime;
use kontra_vm::{Action, HostApi, HostCall, HostValue, Text, event};

use crate::ledger::{Note, NoteRef, Origin, Releases};
use crate::queue::{Exit, Work};
use crate::{Engine, EnginePar};

pub use kontra_plan::{Callbacks, CcLink, Controls, LookPars, ScriptPlan};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Init,
    Note,
    Release,
    Controller,
    Listener,
    Pgs,
    UiControl,
    UiControls,
    AsyncComplete,
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
    /// `$NI_UI_ID` of a UI callback; 0 otherwise.
    pub(crate) ui: i32,
}

/// Marks a UI id in the low bits of a coroutine context; event ids never set it.
const UI_BIT: u32 = 1 << 31;

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
        ui: 0,
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
            Kind::UiControl => 6,
            Kind::UiControls => 15,
            Kind::AsyncComplete => 13,
        };
        Self {
            kind,
            event,
            callback_type,
            ..Self::IDLE
        }
    }

    /// The coroutine context, so a resumed callback still knows its event
    /// or UI id (low 32 bits, a UI id with the top bit set), callback type
    /// (8), callback id (20) and listener signal (4).
    pub(crate) fn context(&self) -> u64 {
        let low = match self.event {
            Some(n) => n.event_id() as u32,
            None => self.ui as u32 | UI_BIT,
        };
        u64::from(low)
            | u64::from(self.callback_type) << 32
            | u64::from(self.callback_id & 0xF_FFFF) << 40
            | u64::from(self.signal & 0xF) << 60
    }

    pub(crate) fn resumed(context: u64) -> Self {
        let low = context as u32;
        Self {
            ui: if low & UI_BIT != 0 {
                (low & !UI_BIT) as i32
            } else {
                0
            },
            kind: Kind::Resumed,
            event: NoteRef::from_event_id(context as u32 as i32),
            callback_type: (context >> 32) as u8,
            callback_id: (context >> 40) as u32 & 0xF_FFFF,
            signal: (context >> 60) as u8,
            ..Self::IDLE
        }
    }
}

/// KSP's `$NI_NOT_FOUND`, what the `get_*_idx` lookups answer for a miss
/// (measured on the oracle: `probes/ni_not_found.lua`).
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
        // A release callback stays one across its waits.
        let ending = self.running.callback_type == Running::new(Kind::Release, None).callback_type
            || parent
                .and_then(|p| self.notes.get(p))
                .is_some_and(|n| n.released);
        note.releases = match (ending, duration > 0) {
            (false, _) => Releases::OnRelease,
            (true, false) => Releases::AtStart,
            (true, true) => Releases::Never,
        };
        note.origin = match (duration, parent) {
            (-1, Some(p)) if !ending => Origin::Child(p),
            _ => Origin::Script,
        };
        let Some(r) = self.admit(note) else {
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
        let player = self.running.slot + 1;
        for &note in &hits {
            if let Some(n) = self.notes.get_mut(note) {
                n.noted_off |= n.first == player;
            }
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
            self.fade_where(frames, |s| s.note == Some(note));
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
        #[cfg(feature = "trace")]
        if let Some(trace) = &mut self.trace {
            let mut args = [0i64; 8];
            let count = call.ints.len().min(args.len());
            if let (Some(dst), Some(src)) = (args.get_mut(..count), call.ints.get(..count)) {
                dst.copy_from_slice(src);
            }
            let call_id = trace.next_action;
            trace.next_action = trace.next_action.wrapping_add(1);
            trace.push(crate::TraceEvent::Action {
                frame: self.now.0,
                slot: self.running.slot,
                callback: self.running.callback_id,
                action: call.action,
                call_id,
                args,
                len: count as u8,
                integer_count: call.ints.len(),
                text_count: call.texts.len(),
            });
            for (index, &value) in call.ints.iter().enumerate().skip(count) {
                trace.push(crate::TraceEvent::ActionInteger {
                    call_id,
                    index,
                    value,
                });
            }
            for (index, value) in call.texts.iter().enumerate() {
                trace.text(call_id, index, value.as_str());
            }
        }
        if let Some(n) = self.called.get_mut(usize::from(call.action.id())) {
            *n += 1;
        }
        if let Some(value) = self.pgs_call(&mut call) {
            return value;
        }
        if let Some(value) = self.ui_call(&mut call) {
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
            // 1 for the controller that changed (KSP manual): the one this
            // controller callback runs for (Pacific's pedal-up release).
            Action::CcTouched => flag(
                self.running.kind == Kind::Controller
                    && int(0) == i32::from(self.running.controller),
            ),
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
            Action::RouteAmount => {
                self.set_route_amount(int(0), int(1));
                HostValue::Nothing
            }
            Action::BusVolume => {
                let gain = match int(1) {
                    ..=-200_000 => 0.0,
                    mdb => crate::ir::common::MILLIBEL_GAIN.eval_unclamped(mdb as f32),
                };
                if let Ok(bus) = usize::try_from(int(0)) {
                    self.set_bus_gain(bus, gain);
                }
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
            Action::GetControlParArr => {
                HostValue::Int(self.control_par_at(call.cells, int(0), int(1), int(2)))
            }
            Action::SetControlParArr => {
                self.set_control_par_at(call.cells, int(0), int(1), int(2), int(3));
                HostValue::Nothing
            }
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
            Action::UiIdVar => HostValue::Int(self.running.ui),
            Action::ControlParIdx => HostValue::Int(self.ui.last_cell(self.running.slot) as i32),
            Action::ResetRlsTrigCounter => self.reset_rls_trig_counter(int(0)),
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
            Action::GetFolder => match self.script().and_then(|s| s.folder(int(0))) {
                Some(folder) => {
                    *call.text_out = folder;
                    HostValue::Text
                }
                None => {
                    self.count_unhandled(Action::GetFolder);
                    HostValue::Nothing
                }
            },
            Action::GroupName => {
                let name = usize::try_from(int(0))
                    .ok()
                    .and_then(|g| self.plan.groups.get(g))
                    .map_or("", |g| g.name.as_str());
                *call.text_out = Text::from_str_cut(name).0;
                HostValue::Text
            }
            // ponytail: answered at once, then on async_complete; the samples
            // stay in memory until the host reloads without the group.
            Action::PurgeGroup => {
                if let Some(g) = self.groups.get_mut(int(0) as usize) {
                    g.purged = int(1) == 0;
                }
                HostValue::Int(self.start_async(Exit::Done))
            }
            Action::AsyncId => HostValue::Int(self.ids.async_done),
            Action::AsyncExitStatus => HostValue::Int(i32::from(self.ids.async_exit == Exit::Done)),
            // A command still running (an effect type loading off the audio
            // thread) holds the callback; it looks again each millisecond.
            Action::WaitAsync if int(0) > 0 && self.ids.running.contains(&int(0)) => {
                HostValue::Wait(1000)
            }
            Action::WaitAsync | Action::DisableLogging => HostValue::Nothing,
            Action::NumOutputChannels => HostValue::Int(2), // one stereo pair; Kontakt: its output section (28)
            Action::StopWait => self.stop_wait(int(0), int(1) != 0),
            Action::GetPurgeState => HostValue::Int(
                self.groups
                    .get(int(0) as usize)
                    .map_or(0, |g| i32::from(!g.purged)),
            ),
            // Any modulator, carried or not. A miss: find_mod answers 0 (as
            // measured: probes/find_mod_index.lua), get_mod_idx $NI_NOT_FOUND.
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
            action @ (Action::GetEngineParDisp | Action::GetEngineParDispExt)
                if self.par_text(action == Action::GetEngineParDispExt, int, call.text_out) =>
            {
                HostValue::Text
            }
            // Kontakt answers −1; an effect type or subtype set outside `on
            // init` runs asynchronously: its id, then `on async_complete`
            // (measured, probes/async_engine_par.ksp).
            // A type the slot cannot switch to in place loads (fx_types).
            Action::SetEnginePar => {
                let par = self.engine_par(int(0));
                let swap = matches!(par, Some(EnginePar::EffectType | EnginePar::EffectSubtype));
                let set = self.set_engine_par(int(0), int(1), int(2), int(3), int(4));
                let subtype = par == Some(EnginePar::EffectSubtype);
                let loaded = (swap && !set)
                    .then(|| self.load_type(int(1), subtype, int(2), int(3), int(4)))
                    .flatten();
                if !set && loaded.is_none() {
                    self.count_unhandled(Action::SetEnginePar);
                }
                HostValue::Int(match loaded {
                    Some(answer) => answer,
                    // Set in place succeeded; a type that could not load failed.
                    None if swap && self.running.kind != Kind::Init => {
                        self.start_async(Exit::from(set))
                    }
                    None => -1,
                })
            }
            Action::LoadIrSample => {
                let name = call.texts.first().map_or("", Text::as_str);
                match self.load_ir_sample(name, int(0), int(1)) {
                    Some(answer) => HostValue::Int(answer),
                    None => {
                        self.count_unhandled(Action::LoadIrSample);
                        HostValue::Nothing
                    }
                }
            }
            Action::SetController => {
                self.set_controller(int(0), int(1));
                HostValue::Nothing
            }
            // Mode 2 is the library's data folder, read at once (the next line
            // sees it). User-file modes and async text loads remain unsupported.
            Action::LoadArray if int(2) == 2 && int(3) == 2 && self.running.kind == Kind::Init => {
                if self.script().is_some_and(|s| {
                    s.load_text_data(int(0) as u32, int(1) as usize, call.text_cells)
                }) {
                    HostValue::Int(0)
                } else {
                    self.count_unhandled(Action::LoadArray);
                    HostValue::Nothing
                }
            }
            // Mode 1 reads the user's Data folder (what `save_array` wrote),
            // which KONTRA never holds: the load misses, the array stays as it
            // is and `on async_complete` reports failure (KSP manual).
            Action::LoadArray if int(2) == 1 => HostValue::Int(self.start_async(Exit::Failed)),
            Action::LoadArray if int(2) == 2 && int(3) != 2 => {
                if let Some(s) = self.script() {
                    s.load_number_data(int(0) as u32, int(1) as usize, call.cells);
                }
                HostValue::Int(0)
            }
            Action::ReadPersistentVar => {
                let whole = |i| u32::try_from(int(i)).unwrap_or(0);
                let range = whole(0)..whole(0).saturating_add(whole(1));
                self.read_persistent(range, int(2) == 1, call.cells, call.text_cells);
                HostValue::Nothing
            }
            // An array file named by path: loaded at once while the engine is
            // built (off the audio thread, as `on init` reads it); otherwise,
            // and every save, the control side carries it out (files.rs).
            action @ (Action::LoadArrayStr | Action::SaveArrayStr) => {
                let path = call.texts.first().copied().unwrap_or(Text::EMPTY);
                let (at, kind) = ((int(0) as u32, int(1) as u32), i64::from(int(2)));
                let save = action == Action::SaveArrayStr;
                let slot = self.running.slot;
                let script = self.script();
                let request = script.map(|s| s.file_request(slot, path, at, kind, save));
                let Some(request) = request.filter(|_| save || !self.building) else {
                    let cells = (call.cells, call.text_cells);
                    let loaded = script.zip(request).is_some_and(|(s, r)| s.load(&r, cells));
                    return HostValue::Int(self.start_async(Exit::from(loaded)));
                };
                HostValue::Int(self.request_file(request))
            }
            Action::SetSnapshotType => {
                self.recall = kontra_plan::Recall::of_type(i64::from(int(0)));
                HostValue::Nothing
            }
            Action::EventStatus => {
                let live = NoteRef::from_event_id(int(0)).and_then(|r| self.notes.get(r));
                HostValue::Int(i32::from(live.is_some()))
            }
            other => {
                self.count_unhandled(other);
                HostValue::Nothing
            }
        }
    }
}

impl Engine {
    /// Counts a call the engine does not carry.
    fn count_unhandled(&mut self, action: Action) {
        self.stats.actions_unhandled += 1;
        if let Some(n) = self.unhandled.get_mut(usize::from(action.id())) {
            *n += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_ids_round_trip_and_reject_garbage() {
        assert_eq!(NoteRef::from_event_id(0), None);
        assert_eq!(NoteRef::from_event_id(-5), None);
        assert_eq!(NoteRef::from_event_id(17), None);
    }
}
