//! Bounded host identity, independent of a finite attack voice or MIDI key row.
use super::{EventId, Expression};
const _: () = assert!(crate::ksp::EVENT_CAPACITY <= u16::MAX as usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostNote {
    pub port: u8,
    pub channel: u8,
    pub key: u8,
    pub id: i32,
    /// CLAP supports a plugin-to-host NOTE_END; VST3 does not.
    pub clap: bool,
}

/// Each -1 axis is a host wildcard. Invalid axes never match an owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostPattern {
    pub port: i32,
    pub channel: i32,
    pub key: i32,
    pub id: i32,
    pub clap: bool,
}

impl HostPattern {
    pub fn matches(self, note: HostNote) -> bool {
        self.clap == note.clap && (self.port == -1 || self.port == i32::from(note.port))
            && (self.channel == -1 || self.channel == i32::from(note.channel))
            && (self.key == -1 || self.key == i32::from(note.key))
            && (self.id == -1 || self.id == note.id)
    }
}

/// An internal generation, never a host-provided note ID or a key index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostRef { index: u16, generation: u32 }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HostExpression {
    Gain(f32), Tune(f32), Pan(f32),
    Brightness(crate::midi::ControlValue),
    Pressure(crate::midi::ControlValue),
}

/// A prospective accepted start is not a live Player event identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AcceptedStart(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RootBinding { Live(EventId), Starting(AcceptedStart) }

impl RootBinding {
    pub(super) fn live(self) -> Option<EventId> {
        match self { Self::Live(event) => Some(event), Self::Starting(_) => None }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Owner {
    pub note: HostNote,
    pub accepted: Option<super::note_mode::AcceptedNote>,
    pub per_note: Option<super::PerNoteInput>,
    pub channel: u8,
    pub key: u8,
    pub velocity: crate::midi::Velocity,
    /// Captured at physical key-up, independent of the onset and pedal-up.
    pub release_velocity: Option<crate::midi::Velocity>,
    pub event: RootBinding,
    pub held: bool,
    pub silenced: bool,
    pub sostenuto: bool,
    pub start: u64,
    pub stop: Option<u64>,
    /// Latest terminal sample of an attached voice or completed owner work.
    /// Retained after voice removal and until NOTE_END is accepted.
    pub pending_end: Option<u64>,
    overlay: Expression,
    generation: u32,
    live: bool,
    pub pinned: bool,
}

/// Canonical slot/generation allocation. A prepared program bank lends this
/// storage to one retained audio owner while that owner's active list stays local.
#[derive(Default)]
pub(super) struct Storage {
    slots: Box<[Owner]>,
    free: Vec<u16>,
    shared: bool,
}

impl Storage {
    pub(super) fn get(&self, id: HostRef) -> Option<&Owner> {
        self.slots.get(id.index as usize).filter(|o|o.live && o.generation==id.generation)
    }
    pub(super) fn terminal(&self, id: HostRef) -> Option<(HostNote,u64)> {
        let o=self.get(id).filter(|o|!o.held && !o.pinned)?;
        Some((o.note,o.pending_end?.max(o.stop.unwrap_or(o.start))))
    }
    pub(super) fn retire_terminal(&mut self,id:HostRef)->bool {
        if self.terminal(id).is_none() { return false; }
        self.slots[id.index as usize].live=false;
        self.free.push(id.index);
        true
    }
    pub(super) fn live_count(&self)->usize { self.slots.len()-self.free.len() }
    pub(super) fn is_empty(&self)->bool { self.live_count()==0 }
    pub(super) fn set_shared(&mut self) { self.shared=true; }
}

pub(super) struct Owners {
    storage: Storage,
    pub active: Vec<HostRef>,
    /// Duplicate or full ownership cannot fall back to a different note.
    pub dropped: u64,
}

#[cfg(test)]
#[path = "sostenuto_root_tests.rs"]
mod sostenuto_root_tests;

impl Owners {
    pub fn new() -> Self {
        let count = crate::ksp::EVENT_CAPACITY;
        let empty = Owner { note: HostNote { port: 0, channel: 0, key: 0, id: -1, clap: true }, accepted: None,
            per_note: None, channel: 0, key: 0, velocity: crate::midi::Velocity::from_midi1(0), release_velocity: None, event: RootBinding::Live(EventId(0)), held: false,
            silenced: false, sostenuto: false, start: 0, stop: None, pending_end: None, overlay: Expression::default(),
            generation: 0, live: false, pinned: false };
        Self { storage: Storage { slots: vec![empty; count].into_boxed_slice(),
            free: (0..count as u16).rev().collect(), shared: false }, active: Vec::with_capacity(count), dropped: 0 }
    }

    /// Prepared-bank setup only: all retained programs use the first pool.
    /// Inner active lists remain the original program provenance.
    pub(super) fn take_storage(&mut self) -> Storage {
        let mut storage = std::mem::take(&mut self.storage);
        storage.shared = true;
        storage
    }

    pub(super) fn swap_storage(&mut self, storage: &mut Storage) {
        std::mem::swap(&mut self.storage, storage);
    }
    pub(super) fn roots(&self)->&Storage { &self.storage }
    pub(super) fn roots_mut(&mut self)->&mut Storage { &mut self.storage }
    pub(super) fn detach_terminal_refs(&mut self, ended:&mut Vec<HostRef>)->bool {
        if ended.capacity()-ended.len()<self.active.len()
            || self.active.iter().any(|id|self.storage.terminal(*id).is_none()) { return false; }
        ended.append(&mut self.active);
        true
    }

    fn duplicate(&self, note: HostNote) -> bool {
        note.id != -1 && if self.storage.shared {
            self.storage.slots.iter().any(|o| o.live && o.note == note)
        } else {
            self.active.iter().any(|r| self.get(*r).is_some_and(|o| o.note == note))
        }
    }

    pub fn get(&self, id: HostRef) -> Option<&Owner> {
        self.storage.slots.get(id.index as usize).filter(|o| o.live && o.generation == id.generation)
    }
    pub fn get_mut(&mut self, id: HostRef) -> Option<&mut Owner> {
        self.storage.slots.get_mut(id.index as usize).filter(|o| o.live && o.generation == id.generation)
    }
    pub fn admits(&self, note:HostNote) -> bool {
        !self.storage.free.is_empty() && !self.duplicate(note)
    }
    pub fn allocate(&mut self, note: HostNote, channel: u8, key: u8, velocity: crate::midi::Velocity, event: EventId, start: u64) -> Option<HostRef> {
        self.allocate_binding(note, channel, key, velocity, RootBinding::Live(event), start)
    }

    /// Worker-prepared projection storage uses the live allocator and free order.
    pub(super) fn copy_from(&mut self, other: &Self) {
        self.storage.slots.copy_from_slice(&other.storage.slots);
        self.storage.free.clear(); self.storage.free.extend_from_slice(&other.storage.free);
        self.active.clear(); self.active.extend_from_slice(&other.active);
        self.storage.shared = other.storage.shared;
        self.dropped = other.dropped;
    }

    pub(super) fn reserve_start(&mut self, note: HostNote, channel: u8, key: u8,
        velocity: crate::midi::Velocity, start: AcceptedStart, at: u64) -> Option<HostRef> {
        self.allocate_binding(note, channel, key, velocity, RootBinding::Starting(start), at)
    }

    fn allocate_binding(&mut self, note: HostNote, channel: u8, key: u8,
        velocity: crate::midi::Velocity, event: RootBinding, start: u64) -> Option<HostRef> {
        // Anonymous host notes can overlap. A concrete ID must remain unique
        // for its tuple’s entire sounding lifetime, including released voices.
        if self.duplicate(note) {
            self.dropped = self.dropped.saturating_add(1); return None;
        }
        let Some(index) = self.storage.free.pop() else { self.dropped = self.dropped.saturating_add(1); return None };
        let slot = &mut self.storage.slots[index as usize];
        let generation = slot.generation.wrapping_add(1).max(1);
        *slot = Owner { note, accepted: None, per_note: None, channel, key, velocity, release_velocity: None, event, held: true, silenced: false, sostenuto: false,
            start, stop: None, pending_end: None, overlay: Expression::default(), generation, live: true, pinned: true };
        let id = HostRef { index, generation };
        self.active.push(id);
        Some(id)
    }
    pub fn overlay(&self, id: Option<HostRef>, mut expression: Expression) -> Expression {
        if let Some(o) = id.and_then(|id| self.get(id)) {
            expression.gain *= o.overlay.gain;
            expression.tune += o.overlay.tune;
            expression.pan += o.overlay.pan;
            expression.note_cc74 = o.overlay.note_cc74.or(expression.note_cc74);
            expression.note_pressure = o.overlay.note_pressure.or(expression.note_pressure);
        }
        expression
    }
    pub fn change(&mut self, id: HostRef, expression: HostExpression) -> bool {
        let Some(o) = self.get_mut(id) else { return false };
        let before = o.overlay;
        match expression {
            HostExpression::Gain(v) if v.is_finite() && (0.0..=4.0).contains(&v) => o.overlay.gain = v,
            HostExpression::Tune(v) if v.is_finite() => o.overlay.tune = v,
            HostExpression::Pan(v) if v.is_finite() && (-1.0..=1.0).contains(&v) => o.overlay.pan = v,
            HostExpression::Brightness(v) => o.overlay.note_cc74 = Some(v),
            HostExpression::Pressure(v) => o.overlay.note_pressure = Some(v),
            _ => {}
        }
        before != o.overlay
    }
    pub fn pin(&mut self, id: Option<HostRef>) {
        if let Some(o) = id.and_then(|id| self.get_mut(id)) { o.pinned = true; }
    }
    pub fn record_end(&mut self, id: Option<HostRef>, frame: u64) {
        if let Some(o) = id.and_then(|id| self.get_mut(id)) {
            o.pending_end = Some(o.pending_end.map_or(frame, |previous| previous.max(frame)));
        }
    }
    pub fn retire(&mut self, id: HostRef) {
        if self.storage.shared && !self.active.contains(&id) { return; }
        let Some(o) = self.get_mut(id) else { return };
        o.live = false;
        self.active.retain(|r| *r != id);
        self.storage.free.push(id.index);
    }
    pub fn silence(&mut self, logical: Option<u8>, physical: Option<u16>, now: u64) {
        for &r in &self.active {
            let o = &mut self.storage.slots[r.index as usize];
            // A completed row can remain live solely because NOTE_END was
            // rejected. A later stop cannot move its already recorded end.
            if (o.held || o.pinned) && logical.is_none_or(|c| o.channel == c) && physical.is_none_or(|m| m & (1 << o.note.channel) != 0) {
                o.silenced = true; o.stop.get_or_insert(now);
                o.pending_end = Some(o.pending_end.map_or(now, |previous| previous.max(now)));
            }
        }
    }
    pub fn reset(&mut self) {
        if self.active.is_empty() { return; }
        if self.storage.shared {
            // A program reset retires its own rows, never another program's pool.
            while let Some(r) = self.active.pop() {
                if let Some(o) = self.get_mut(r) {
                    o.live = false;
                    self.storage.free.push(r.index);
                }
            }
        } else {
            for &r in &self.active { self.storage.slots[r.index as usize].live=false; }
            self.active.clear(); self.storage.free.clear();
            self.storage.free.extend((0..self.storage.slots.len() as u16).rev());
        }
    }
    pub fn stop(&mut self, logical:Option<u8>, physical:Option<u16>, now:u64) {
        self.silence(logical,physical,now);
        for &r in &self.active {
            let o=&mut self.storage.slots[r.index as usize];
            if logical.is_none_or(|c| o.channel==c) && physical.is_none_or(|m| m & (1 << o.note.channel)!=0) { o.held=false; }
        }
    }

    pub(super) fn sostenuto(&mut self, capture: u16, up: u16) {
        for &root in &self.active {
            let owner = &mut self.storage.slots[root.index as usize];
            let input = 1 << owner.note.channel;
            if up & input != 0 { owner.sostenuto = false; }
            else if owner.held && capture & input != 0 { owner.sostenuto = true; }
        }
    }
}

#[cfg(all(test,feature="plugin"))]
mod tests {
    use super::*;
    use crate::{audio::Sample, engine::{Bank, Engine}, import::{Group, Loop, Zone}};
    use std::path::PathBuf;

    fn note(id:i32) -> HostNote { HostNote { port:0, channel:4, key:60, id, clap:true } }
    fn pattern(id:i32) -> HostPattern { HostPattern { port:0, channel:4, key:60, id, clap:true } }
    fn bank(finite:bool) -> Bank {
        let attack = PathBuf::from("attack"); let tail = PathBuf::from("tail");
        Bank::from_samples(vec![Group::default(),Group { release_trigger:true,..Default::default() }],
            vec![Zone { sample:attack.clone(),root:48,loop_range:(!finite).then_some(Loop { start:0,end:128,alternating:false,until_release:false,crossfade:0 }),..Default::default() },
                Zone { group:1,sample:tail.clone(),root:48,..Default::default() }],
            vec![(attack,Sample { rate:48000,frames:vec![[0.2;2];128] }),
                (tail,Sample { rate:48000,frames:vec![[0.1;2];4800] })]).unwrap()
    }
    fn engine(finite:bool) -> Engine {
        let mut e = Engine::default(); e.reset(48000.); e.attack=0.0001; e.release=0.03;
        e.set_bank(Some(Box::new(bank(finite)))); e
    }
    fn render(e:&mut Engine, blocks:usize) {
        let (mut l,mut r)=([0.;128],[0.;128]);
        for _ in 0..blocks { e.render(&mut l,&mut r); }
    }

    #[test]
    fn waited_zone_and_play_cursor_getters_follow_actual_layer_geometry_in_complete_pcm() {
        use crate::{engine::EventChange, ksp::Value};
        // Source84k/output48k with an octave transpose travels3.5 physical
        // frames per output frame, exposing reverse fractional-cursor polarity.
        let checkpoints = [(25, 520, -6000), (120, 1980, 0), (240, 2500, -12000),
            (960, 15000, -3000), (1440, 10000, 0)];
        for (reverse, looped) in [(false, false), (true, false), (false, true)] {
            let mut source = String::from("on init\ndeclare $bad\nmake_persistent($bad)\nend on\non note\nset_event_par($EVENT_ID,$EVENT_PAR_NOTE,72)\nset_event_par($EVENT_ID,$EVENT_PAR_MIDI_CHANNEL,7)\nif (get_event_par($EVENT_ID,$EVENT_PAR_ZONE_ID) # -1 or get_event_par($EVENT_ID,$EVENT_PAR_PLAY_POS) # 0)\ninc($bad)\nend if\n");
            for (frame, wait, volume) in checkpoints {
                let travel = frame as f64 * 3.5;
                // Independent source geometry: high zone starts at96, ends at
                //4096, and its forward loop repeats256..1024 after the first pass.
                let position = if looped && travel >= 928. {
                    Some(256. + (travel - 928.) % 768.)
                } else if travel < 4000. {
                    Some(if reverse { 4095. - travel } else { 96. + travel })
                } else { None };
                let zone = if position.is_some() { 1 } else { -1 };
                let micros = position.map_or(0, |p| (p * 1e6 / 84000.) as i32);
                source.push_str(&format!("wait({wait})\nif (get_event_par($EVENT_ID,$EVENT_PAR_ZONE_ID) # {zone} or get_event_par($EVENT_ID,$EVENT_PAR_PLAY_POS) # {micros})\ninc($bad)\nchange_vol($EVENT_ID,-120000,0)\nelse\nchange_vol($EVENT_ID,{volume},0)\nend if\n"));
            }
            source.push_str("end on\non release\nend on");
            let make = |scripted| {
                let sample = PathBuf::from("fractional cursor source");
                let groups = vec![Group { channel: 7, ..Group::default() },
                    Group { channel: 7, reverse, ..Group::default() }];
                let zones = vec![Zone { sample: sample.clone(), end: -2048, pan: -1., ..Zone::default() },
                    Zone { sample: sample.clone(), group: 1, start: 96, pan: 1.,
                        loop_range: looped.then_some(Loop { start: 256, end: 1024,
                            alternating: false, until_release: true, crossfade: 0 }), ..Zone::default() }];
                let frames = (0..4096).map(|i| {
                    let x = 0.15 * (std::f32::consts::TAU * i as f32 / 128.).sin(); [x; 2]
                }).collect();
                let mut e = Engine::default(); e.reset(48000.); e.attack = 0.; e.release = 0.002;
                e.set_bank(Some(Box::new(Bank::from_samples(groups, zones,
                    vec![(sample, Sample { rate: 84000, frames })]).unwrap())));
                if scripted {
                    let (rt, errors) = crate::ksp::Runtime::with_scripts(&[&source],
                        &mut crate::ksp::LogEngine::default(), 0, Vec::new());
                    assert!(errors.iter().all(Option::is_none)); e.set_script(Some(Box::new(rt)));
                }
                e
            };
            for fragmented in [false, true] {
                let (mut actual, mut oracle) = (make(true), make(false));
                let owner = note(420);
                let (mut a, mut ar, mut b, mut br) = ([0.; 4096], [0.; 4096], [0.; 4096], [0.; 4096]);
                let (mut frame, mut chunk) = (0, 0);
                let mut exercise = || {
                    assert!(actual.host_note_on(owner, 3, 60, 100, 0.));
                    assert!(oracle.host_note_on(owner, 7, 72, 100, 0.));
                    let reference_event = oracle.player.host_notes.get(oracle.player.host_notes.active[0]).unwrap().event.live().expect("oracle root is actually admitted");
                    for stop in [25, 120, 240, 960, 1440, 1600, 4096] {
                        let mut reference_frame = frame;
                        while frame < stop {
                            let n = if fragmented { [1, 7, 31, 128][chunk % 4] } else { 128 }.min(stop - frame);
                            actual.render(&mut a[frame..frame+n], &mut ar[frame..frame+n]);
                            frame += n; chunk += 1;
                        }
                        while reference_frame < stop {
                            let n = 128.min(stop - reference_frame);
                            oracle.render(&mut b[reference_frame..reference_frame+n], &mut br[reference_frame..reference_frame+n]);
                            reference_frame += n;
                        }
                        if let Some((_, _, volume)) = checkpoints.iter().find(|(frame, _, _)| *frame == stop) {
                            oracle.change_event(reference_event, EventChange::Volume(10f32.powf(*volume as f32 / 20000.)));
                        }
                        if stop == 1600 {
                            actual.host_note_off(pattern(420)); oracle.host_note_off(pattern(420));
                        }
                        actual.mark_host_notes(); oracle.mark_host_notes();
                        if stop < 1600 { assert!(actual.host_note_pending(owner) && oracle.host_note_pending(owner)); }
                    }
                    assert_eq!(actual.active_voices(), 0); assert_eq!(oracle.active_voices(), 0);
                    assert!(!actual.host_note_pending(owner) && !oracle.host_note_pending(owner));
                    assert_eq!(actual.host_note_end_at(owner), oracle.host_note_end_at(owner));
                };
                #[cfg(feature = "plugin")]
                assert_eq!(crate::plugin::tests::allocations(&mut exercise), 0);
                #[cfg(not(feature = "plugin"))]
                exercise();
                let error = a.iter().zip(&b).chain(ar.iter().zip(&br)).fold(0_f32,
                    |max, (x, y)| max.max((x-y).abs()));
                assert!(a[..240].iter().any(|v| v.abs() > 0.01) && ar[..240].iter().any(|v| v.abs() > 0.01), "both actual layers must sound");
                assert!(a[3072..].iter().chain(&ar[3072..]).all(|v| v.abs() < 1e-6));
                assert!(error < 1e-6, "reverse={reverse} loop={looped} fragmented={fragmented}: getter-driven full PCM differs from exterior volume oracle by{error}");
                assert_eq!(actual.script().unwrap().persistence()[0]["$bad"], Value::Int(0), "waited getters disagree with independent source cursor geometry");
                assert!(actual.script().unwrap().diagnostics().is_empty());
            }
        }
    }

    #[test]
    fn missing_attack_voices_keep_physical_mpe_release_pcm_through_waits_and_pedals() {
        use crate::{engine::{Expression, InputAddress}, midi::Velocity};
        let make = |mpe: bool, finite_attack: bool, source: Option<&str>| {
            let attack = PathBuf::from("short attack");
            let tail = PathBuf::from("release sine");
            let groups = vec![Group { muted: !finite_attack, ..Group::default() },
                Group { release_trigger: true, ..Group::default() }];
            // The oracle has no MPE. Its immutable octave transposition is
            // independent of the actual member's quarter bend over48 semitones.
            let tune = if mpe { 1. } else { 2. };
            let zones = vec![Zone { sample: attack.clone(), root: 48, tune, ..Zone::default() },
                Zone { group: 1, sample: tail.clone(), root: 48, tune, ..Zone::default() }];
            let frames = (0..4096).map(|i| {
                let v = 0.2 * (std::f32::consts::TAU * i as f32 / 64.).sin();
                [v; 2]
            }).collect();
            let bank = Bank::from_samples(groups, zones, vec![
                (attack, Sample { rate: 48000, frames: vec![[0.2; 2]; 64] }),
                (tail, Sample { rate: 48000, frames }),
            ]).unwrap();
            let mut e = Engine::default();
            e.reset(48000.); e.attack = 0.; e.release = 0.002;
            e.set_bank(Some(Box::new(bank)));
            if mpe { e.configure_mpe(Some((0, 4, 48))); }
            for key in [48, 60] {
                e.set_expression_on(7, key, Expression { gain: 0.8, tune: 3., ..Default::default() });
            }
            if let Some(source) = source {
                let (rt, errors) = crate::ksp::Runtime::with_scripts(&[source],
                    &mut crate::ksp::LogEngine::default(), 0, Vec::new());
                assert!(errors.iter().all(Option::is_none));
                e.set_script(Some(Box::new(rt)));
            }
            if mpe { e.pitch_bend_from(7, 4, 10240); }
            e
        };
        let sources = [None, Some("on note\nend on\non release\nwait(5000)\nend on"),
            Some("on note\nignore_event($EVENT_ID)\nwait(30000)\nplay_note($EVENT_NOTE+12,$EVENT_VELOCITY,0,-1)\nend on\non release\nwait(5000)\nend on")];
        for source in sources {
            for finite_attack in [false, true] {
                for exact in [false, true] {
                    for pedal in [64, 66] {
                        for fragmented in [false, true] {
                            let (mut actual, mut oracle) = (make(true, finite_attack, source), make(false, finite_attack, source));
                            let onset = Velocity::new(0.87654321).unwrap();
                            let release = Velocity::new(0.23456789).unwrap();
                            let owner = note(410);
                            let address = InputAddress { port: 0, channel: 4, key: 60 };
                            let (mut a, mut ar, mut b, mut br) = ([0.; 6144], [0.; 6144], [0.; 6144], [0.; 6144]);
                            let (mut frame, mut chunk) = (0, 0);
                            let mut exercise = || {
                                for stop in [0, 256, 1024, 1536, 2048, 2176, 4096, 6144] {
                                    let mut reference_frame = frame;
                                    while frame < stop {
                                        let n = if fragmented { [1, 7, 31, 128][chunk % 4] } else { 128 }.min(stop - frame);
                                        actual.render(&mut a[frame..frame+n], &mut ar[frame..frame+n]);
                                        frame += n; chunk += 1;
                                    }
                                    while reference_frame < stop {
                                        let n = 128.min(stop - reference_frame);
                                        oracle.render(&mut b[reference_frame..reference_frame+n], &mut br[reference_frame..reference_frame+n]);
                                        reference_frame += n;
                                    }
                                    for e in [&mut actual, &mut oracle] {
                                        match stop {
                                            0 => {
                                                if exact { assert!(e.host_note_on_velocity(owner, 7, 48, onset, 0.)); }
                                                else { assert!(e.note_on_input_velocity_pitch(address, 7, 48, onset, None)); }
                                            }
                                            256 => e.cc_from(7, 4, pedal, 127),
                                            1024 => {
                                                assert_eq!(e.active_voices(), 0, "fixture must reach physical key-up without an attack voice");
                                                if exact { e.host_note_off_velocity(pattern(410), release); }
                                                else { e.note_off_from_velocity(7, 4, 48, release); }
                                            }
                                            1536 if exact => {
                                                e.host_expression(pattern(410), HostExpression::Gain(0.5));
                                                e.host_expression(pattern(410), HostExpression::Tune(1.));
                                            }
                                            2048 => e.cc_from(7, 4, pedal, 0),
                                            // The unlatched transposed child's finite tail
                                            // ends near2301; inspect while it still sounds.
                                            2176 => {
                                                let mut tails = 0;
                                                for voice in e.player.voices.iter().filter(|v| v.release_trigger) {
                                                    tails += 1;
                                                    assert_eq!(voice.onset_velocity, onset);
                                                    assert_eq!(voice.release_velocity, Some(release));
                                                    if exact { assert_eq!(e.player.host_notes.get(voice.host_note.unwrap()).unwrap().note, owner); }
                                                }
                                                assert!(tails > 0, "release family must retain precise data while sounding");
                                            }
                                            _ => {}
                                        }
                                        e.mark_host_notes();
                                        if exact && stop <= 2048 { assert!(e.host_note_pending(owner)); }
                                    }
                                    if stop == 1024 {
                                        // Reuse member controls immediately while release callbacks
                                        // and generated children can still be waiting or pedal-held.
                                        actual.pitch_bend_from(7, 4, 4096);
                                        for key in [48, 60] {
                                            actual.set_expression_on(7, key, Expression { gain: 0.2, tune: 9., ..Default::default() });
                                        }
                                    }
                                }
                                assert_eq!(actual.active_voices(), 0);
                                assert_eq!(oracle.active_voices(), 0);
                                assert!(actual.player.pending_releases.is_empty());
                                assert!(oracle.player.pending_releases.is_empty());
                                if exact {
                                    assert!(!actual.host_note_pending(owner) && !oracle.host_note_pending(owner));
                                    assert_eq!(actual.host_note_end_at(owner), oracle.host_note_end_at(owner), "physical MPE changed the exact root's terminal sample");
                                    assert!(actual.host_note_end_at(owner).is_some_and(|end| end > 2048 && end < 6144));
                                }
                            };
                            #[cfg(feature = "plugin")]
                            assert_eq!(crate::plugin::tests::allocations(&mut exercise), 0);
                            #[cfg(not(feature = "plugin"))]
                            exercise();
                            let error = a.iter().zip(&b).chain(ar.iter().zip(&br))
                                .fold(0_f32, |max, (x, y)| max.max((x-y).abs()));
                            assert!(b[2048..4096].iter().any(|v| v.abs() > 0.01), "independent release oracle must sound");
                            assert!(a[4096..].iter().chain(&ar[4096..]).all(|v| v.abs() < 1e-6), "complete release must reach silence");
                            assert!(error < 1e-6, "finite={finite_attack} exact={exact} pedal={pedal} fragmented={fragmented} source={source:?}: full PCM error={error}");
                            if source == sources[2] && !exact && pedal == 66 {
                                // Sostenuto captured the physical root before this child
                                // existed. Its release sample must run before pedal-up.
                                assert!(a[1440..2048].iter().any(|v| v.abs() > 0.01));
                                assert!(a[2304..].iter().chain(&ar[2304..]).all(|v| v.abs() < 1e-6),
                                    "generated child incorrectly inherited physical sostenuto capture");
                            }
                            if let Some(rt) = actual.script() { assert!(rt.diagnostics().is_empty(), "{:?}", rt.diagnostics()); }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn exact_roots_keep_finite_attacks_pedals_children_and_late_expressions_without_heap() {
        let mut e=engine(false);
        assert_eq!(crate::plugin::tests::allocations(|| {
            assert!(e.host_note_on(note(10),7,48,100,0.)); render(&mut e,1);
            e.host_note_off(pattern(10));
            assert!(e.host_note_on(note(11),7,48,100,0.));
            e.host_expression(pattern(10),HostExpression::Gain(0.)); render(&mut e,2);
            let old=e.player.voices.iter().filter(|v| e.player.host_notes.get(v.host_note.unwrap()).unwrap().note.id==10);
            for v in old { assert!(v.gains.iter().all(|x| x.abs()<1e-7)); }
            assert!(e.player.voices.iter().any(|v| e.player.host_notes.get(v.host_note.unwrap()).unwrap().note.id==11 && v.gains[0]>0.01));
            assert!(e.key_down(7,48));
            e.mark_host_notes(); assert!(e.host_note_pending(note(10))); assert!(e.host_note_pending(note(11)));
            // Old-id choke does not cancel the new same-key owner.
            e.host_note_choke(pattern(10)); render(&mut e,4); e.mark_host_notes();
            assert!(!e.host_note_pending(note(10))); assert!(e.host_note_pending(note(11)));
            let stale=e.player.host_notes.active[0]; e.retire_host_note(note(10));
            assert!(e.host_note_on(note(12),7,48,100,0.));
            assert!(!e.player.host_notes.change(stale,HostExpression::Gain(0.)));
            e.host_note_off(HostPattern { id:-1,..pattern(10) }); render(&mut e,64); e.mark_host_notes();
            assert!(!e.host_note_pending(note(11))); assert!(!e.host_note_pending(note(12)));
        }),0);

        let mut e=engine(true);
        assert_eq!(crate::plugin::tests::allocations(|| {
            assert!(e.host_note_on(note(20),7,48,100,0.)); render(&mut e,8);
            assert_eq!(e.active_voices(),0); e.mark_host_notes(); assert!(e.host_note_pending(note(20)));
            // The exact root arrives on physical channel4 and is routed to7.
            // Its pedals keep that physical provenance.
            e.cc_from(7,4,66,127); e.host_note_off(pattern(20));
            assert_eq!(e.player.pending_releases.len(),1); e.mark_host_notes(); assert!(e.host_note_pending(note(20)));
            e.host_expression(pattern(20),HostExpression::Tune(12.));
            e.cc_from(7,4,66,0); render(&mut e,1);
            let voice=e.player.voices.iter().find(|v| v.release_trigger).unwrap();
            assert_eq!(e.player.host_notes.get(voice.host_note.unwrap()).unwrap().note.id,20);
            assert!((voice.pitch.1-2.).abs()<1e-6);
            e.cc(7,120,0); render(&mut e,8); e.mark_host_notes(); assert!(!e.host_note_pending(note(20)));
        }),0);

        let mut e=engine(false);
        e.set_mpe_zone(Some((0,1<<1)));
        let first=HostNote { channel:1,..note(25) };
        let old=HostPattern { channel:1,..pattern(25) };
        assert_eq!(crate::plugin::tests::allocations(|| {
            e.set_expression_on(1,48,Expression { tune:2.,..Default::default() });
            assert!(e.host_note_on(first,1,48,100,0.)); render(&mut e,1);
            e.host_note_off(old);
            e.set_expression_on(1,48,Expression { tune:8.,..Default::default() });
            assert!(e.host_note_on(HostNote { id:26,..first },1,48,100,0.));
            e.host_expression(old,HostExpression::Tune(12.)); render(&mut e,1);
            for voice in &e.player.voices {
                let id=e.player.host_notes.get(voice.host_note.unwrap()).unwrap().note.id;
                let semitones=if id==25 { 14. } else { 8. };
                assert!((voice.pitch.1-2f64.powf(semitones/12.)).abs()<1e-6,"MPE member reuse replaced the old owner default");
            }
        }),0);

        let mut e=engine(false);
        let script="on note\nignore_event($EVENT_ID)\nplay_note($EVENT_NOTE+12,100,0,-1)\nwait(10000)\nplay_note($EVENT_NOTE+13,100,0,20000)\nend on\non release\nwait(5000)\nplay_note(72,100,0,0)\nend on";
        let (rt,errors)=crate::ksp::Runtime::with_scripts(&[script],&mut crate::ksp::LogEngine::default(),0,Vec::new());
        assert!(errors.iter().all(Option::is_none)); e.set_script(Some(Box::new(rt)));
        assert_eq!(crate::plugin::tests::allocations(|| {
            assert!(e.host_note_on(note(30),7,48,100,0.)); render(&mut e,1);
            e.host_note_off(pattern(30));
            assert!(e.host_note_on(note(31),7,48,100,0.));
            e.host_expression(pattern(30),HostExpression::Gain(0.)); render(&mut e,8); e.mark_host_notes();
            assert!(e.host_note_pending(note(30)),"waiting root and independent child retain NOTE_END ownership");
            assert!(e.script().unwrap().key_down_from(4,7,48));
            assert!(e.player.voices.iter().any(|v| e.player.host_notes.get(v.host_note.unwrap()).unwrap().note.id==31));
            e.host_note_choke(pattern(30)); render(&mut e,16); e.mark_host_notes();
            assert!(!e.host_note_pending(note(30))); assert!(e.host_note_pending(note(31)));
            e.panic(); render(&mut e,32); e.mark_host_notes(); assert!(!e.host_note_pending(note(31)));
            assert!(e.host_note_on(note(32),7,48,100,0.)); render(&mut e,1);
            e.all_sound_off_from(7,1<<4); render(&mut e,8); e.mark_host_notes();
            assert!(e.host_note_pending(note(32)),"CC120 retains the physical root for its eventual cleanup");
            e.host_note_off(pattern(32)); render(&mut e,32); e.mark_host_notes();
            assert!(!e.host_note_pending(note(32)),"exact key-up failed to clean a silenced root");
            assert_eq!(e.active_voices(),0,"late key-up resurrected a stopped child/release");
            e.reset(48000.);
            assert!(e.host_note_on(note(31),7,48,100,0.),"host reset must allow the same ID immediately");
        }),0);
    }

    #[test]
    fn budget_paused_scoped_cleanup_keeps_live_key_visibility_without_heap() {
        let script="on init\ndeclare $i\ndeclare $b := -1\ndeclare $c := -1\nmake_persistent($i)\nmake_persistent($b)\nmake_persistent($c)\nend on\non release\nif ($EVENT_NOTE=48)\nwhile ($i<2000)\ninc($i)\nend while\n$b := %KEY_DOWN[50]\n$c := %KEY_DOWN[52]\nend if\nend on";
        for full_sound_off in [false,true] {
            let (rt,errors)=crate::ksp::Runtime::with_scripts(&[script],&mut crate::ksp::LogEngine::default(),0,Vec::new());
            assert!(errors.iter().all(Option::is_none));
            let mut e=engine(false); e.set_script(Some(Box::new(rt)));
            let b=HostNote { key:62,..note(11) }; let bp=HostPattern { key:62,..pattern(11) };
            let c=HostNote { key:64,..note(12) };
            assert!(e.host_note_on(note(10),7,48,100,0.)); assert!(e.host_note_on(b,7,50,100,0.)); render(&mut e,1);
            let spent=e.script().unwrap().env.spent;
            assert_eq!(crate::plugin::tests::allocations(|| {
                // Offline mode removes wall-clock variability; the fixed fuel
                // forces A's release to yield inside its loop, before reads.
                e.begin_audio_block(128,1,true);
                e.script.as_deref_mut().unwrap().env.block_fuel=64;
                if full_sound_off {
                    e.host_note_off(pattern(10)); e.cc(7,120,0);
                } else { e.host_note_choke(pattern(10)); }
                assert_eq!(e.script().unwrap().env.block_fuel,0);
                assert!(e.script().unwrap().env.spent>spent);
                e.host_note_off(bp);
                assert!(e.host_note_on(c,7,52,100,0.));
                e.mark_host_notes(); assert!(e.host_note_pending(note(10)));
            }),0);
            let paused=e.script().unwrap().persistence();
            assert!(matches!(paused[0]["$i"],crate::ksp::Value::Int(i) if i>0 && i<2000));
            assert_eq!(paused[0]["$b"],crate::ksp::Value::Int(-1));
            assert_eq!(paused[0]["$c"],crate::ksp::Value::Int(-1));
            assert_eq!(crate::plugin::tests::allocations(|| {
                e.begin_audio_block(128,1,true); render(&mut e,4);
                assert!(e.key_down(7,52));
            }),0);
            let saved=e.script().unwrap().persistence();
            assert_eq!(saved[0]["$b"],crate::ksp::Value::Int(0));
            assert_eq!(saved[0]["$c"],crate::ksp::Value::Int(i32::from(!full_sound_off)),"scoped input visibility leaked across budget pause, or full sound-off lost suppression");
        }
    }

    #[test]
    fn scoped_choke_exposes_unselected_keys_and_bank_only_replacement_closes_waits() {
        let script="on release\nif ($EVENT_NOTE=48)\nmessage(%KEY_DOWN[50])\nend if\nend on";
        let (rt,errors)=crate::ksp::Runtime::with_scripts(&[script],&mut crate::ksp::LogEngine::default(),0,Vec::new());
        assert!(errors.iter().all(Option::is_none));
        let mut e=engine(false); e.set_script(Some(Box::new(rt)));
        assert_eq!(crate::plugin::tests::allocations(|| {
            assert!(e.host_note_on(note(10),7,48,100,0.));
            assert!(e.host_note_on(HostNote { key:62,..note(11) },7,50,100,0.)); render(&mut e,1);
            e.host_note_choke(pattern(10));
            assert_eq!(e.script().unwrap().last_message(),"1","scoped cleanup hid another held key from on release");
            assert!(e.script().unwrap().key_down_from(4,7,50));
            assert!(e.key_down(7,50));
        }),0);
        let script="on note\nignore_event($EVENT_ID)\nwait(100000)\nplay_note($EVENT_NOTE,100,0,-1)\nend on\non release\nwait(10000)\nplay_note(72,100,0,0)\nend on";
        let (rt,errors)=crate::ksp::Runtime::with_scripts(&[script],&mut crate::ksp::LogEngine::default(),0,Vec::new());
        assert!(errors.iter().all(Option::is_none));
        let mut e=engine(false); e.set_script(Some(Box::new(rt)));
        let mut old=None;
        assert_eq!(crate::plugin::tests::allocations(|| {
            assert!(e.host_note_on(note(20),7,48,100,0.)); render(&mut e,1);
            e.mark_host_notes(); assert!(e.host_note_pending(note(20)));
            old=e.set_bank(None);
            render(&mut e,64); e.mark_host_notes();
            assert!(!e.script().unwrap().key_down_from(4,7,48));
            assert!(!e.host_note_pending(note(20)),"bank-only replacement retained an old held/waiting root");
            assert_eq!(e.active_voices(),0);
            e.host_note_off(pattern(20)); e.mark_host_notes();
            assert!(!e.host_note_pending(note(20)));
        }),0);
        assert!(old.is_some());
    }

    #[test]
    fn anonymous_key_up_preserves_exact_roots_but_exact_wildcard_releases_them() {
        for scripted in [false,true] {
            let mut e=engine(false);
            if scripted {
                let (rt,errors)=crate::ksp::Runtime::with_scripts(&["on note\nend on"],&mut crate::ksp::LogEngine::default(),0,Vec::new());
                assert!(errors.iter().all(Option::is_none)); e.set_script(Some(Box::new(rt)));
            }
            assert_eq!(crate::plugin::tests::allocations(|| {
                assert!(e.host_note_on(note(10),7,48,100,0.));
                assert!(e.host_note_on(note(11),7,48,100,0.));
                e.note_on_from(7,4,48,100); render(&mut e,1);
                e.note_off_from(7,4,48); render(&mut e,1);
                assert!(e.key_down(7,48),"anonymous MIDI key-up cleared an exact root");
                assert!(e.host_key_held(4,60));
                for id in [10,11] {
                    assert!(e.player.voices.iter().any(|v| v.held && !v.released
                        && v.host_note.and_then(|r| e.player.host_notes.get(r)).is_some_and(|o| o.note.id==id)),"anonymous key-up released exact id {id}");
                }
                assert!(e.player.voices.iter().filter(|v| v.host_note.is_none()).all(|v| !v.held));
                if let Some(rt)=e.script() { assert!(rt.key_down_from(4,7,48)); }
                // CLAP id=-1 explicitly matches every exact owner on the PCK.
                e.host_note_off(HostPattern { id:-1,..pattern(10) }); render(&mut e,1);
                assert!(!e.host_key_held(4,60)); assert!(!e.key_down(7,48));
                assert!(e.player.voices.iter().all(|v| !v.held || v.release_trigger));
            }),0);
        }
    }

    #[test]
    fn ownership_exhaustion_and_duplicate_tuples_do_not_steal_generations() {
        let mut owners=Owners::new();
        assert!(HostPattern { port:-1,channel:-1,key:-1,id:-1,clap:true }.matches(note(10)));
        assert!(!HostPattern { port:1,..pattern(10) }.matches(note(10)));
        assert!(!HostPattern { clap:false,..pattern(10) }.matches(note(10)));
        let velocity=crate::midi::Velocity::from_midi1(100);
        let first=owners.allocate(note(0),7,48,velocity,EventId(1),0).unwrap();
        assert!(owners.allocate(HostNote { channel:5,..note(0) },7,48,velocity,EventId(2),0).is_some(),"ID alone is not a complete host tuple");
        assert!(owners.allocate(note(0),7,48,velocity,EventId(3),0).is_none());
        assert_eq!(crate::plugin::tests::allocations(|| {
            for id in 1..crate::ksp::EVENT_CAPACITY-1 { assert!(owners.allocate(note(id as i32),7,48,velocity,EventId(id as u32),0).is_some()); }
            assert!(owners.allocate(note(99999),7,48,velocity,EventId(4),0).is_none());
            assert_eq!(owners.dropped,2); assert_eq!(owners.get(first).unwrap().note,note(0));
            owners.retire(first);
            let next=owners.allocate(note(99999),7,48,velocity,EventId(4),0).unwrap();
            assert_ne!(next,first); assert!(owners.get(first).is_none());
        }),0);
    }
}
