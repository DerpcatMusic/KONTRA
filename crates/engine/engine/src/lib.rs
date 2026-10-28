//! The audio-thread engine: one clock, one queue, one note ledger, one voice
//! pool, one render loop (02-ENGINE.md, 13-FRESH-CORE.md).
//!
//! Every event carries a sample time and takes effect at it; the loop renders
//! up to the next due item, so the host block size never changes the output
//! (rule 3). A note owns the voices it starts, and they all begin on the
//! note's sample (rule 4). Sampler behaviour (articulations, round robin,
//! crossfades, release triggers, voice groups, mic buses) is plan data the
//! engine executes, not script code. Nothing allocates after [`Engine::new`].

mod behaviour;
mod controls;
mod effects;
mod filters;
mod inspect;
mod ledger;
mod midi;
mod modulators;
mod params;
mod plan;
mod prepare;
mod queue;
mod racks;
mod script;
mod voice_gain;
mod voice_groups;

use std::sync::Arc;

use kontra_dsp::filter::Svf;
use kontra_stream::StreamClient;
use kontra_time::Frames;
use kontra_vm::Instance;
use kontra_voice::{Voice, VoiceStart};

pub use kontra_events::{Event, HostNote, HostPattern, NoteExpression};
pub use kontra_ir as ir;
pub use kontra_time::SampleTime;
pub use kontra_vm::{Action, Faults, Op, Program, Text};
pub use kontra_voice::{Ahdsr, Loop, Region};
pub use ledger::NoteRef;
pub use plan::{
    Atom, Bus, Control, Crossfade, Group, Level, Plan, Response, RoundRobin, RrCounter, Trigger,
    Zone,
};
pub use prepare::{Open, prepare};
pub use queue::Timed;
pub use script::{Callbacks, Controls, EnginePars, ScriptPlan};

use behaviour::Behaviour;
use ledger::{Ledger, Note, Origin};
use queue::{EventQueue, Work};
use script::{GroupMasks, Kind, Running};

/// Frames rendered per inner span; sizes the bus buffers.
const MAX_SPAN: u64 = 256;
/// A stolen, choked or limited voice fades over this long instead of clicking.
const FADE_SECONDS: f64 = 0.005;

/// Counters since the engine was created. Each names a refusal or a loss.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Voices faded out because the plan's polyphony was reached.
    pub voices_stolen: u64,
    /// Voices cut without a fade because even the fade reserve was full.
    pub voices_cut: u64,
    /// Note-ons refused because the note table was full.
    pub notes_refused: u64,
    /// Events refused because the queue was full.
    pub events_refused: u64,
    /// Source frames voices played as silence because data was late.
    pub missing_frames: u64,
    /// Script actions this engine does not implement yet (they returned 0).
    pub actions_unhandled: u64,
    /// Script `message` calls.
    pub messages: u64,
}

struct Slot {
    voice: Voice,
    /// The owner. `None` only while the voice is idle.
    note: Option<NoteRef>,
    zone: usize,
    started: SampleTime,
    /// Key released (the envelope is in release).
    released: bool,
    released_at: SampleTime,
    /// Fading out after a steal, choke or group limit; no longer counts as playing.
    fading: bool,
    /// Extra gain fixed at start (release-trigger decay).
    factor: f32,
    /// Its group's filter whose cutoff moves, run on this voice alone.
    filter: Option<Svf>,
}

struct BusState {
    gain: f32,
    enabled: bool,
    /// Its rack, on what it carries.
    rack: racks::Rack,
    /// -1..=1 balance after the rack.
    pan: f32,
    mix: Box<[[f32; 2]]>,
}

pub struct Engine {
    now: SampleTime,
    rate: u32,
    plan: Arc<Plan>,
    stream: StreamClient,
    queue: EventQueue,
    notes: Ledger,
    behaviour: Behaviour,
    voices: Vec<Slot>,
    buses: Vec<BusState>,
    /// Sustain pedal down, per MIDI channel.
    pedal: [bool; 16],
    /// Scratch for ledger queries, capacity = note table size.
    hits: Vec<NoteRef>,
    ended: Vec<HostNote>,
    stats: Stats,
    /// The script's state; taken out while a callback runs.
    vm: Option<Instance>,
    running: Running,
    /// Notes waiting for their callback to return before they start.
    pending: Vec<NoteRef>,
    masks: GroupMasks,
    /// Scratch for script event targets (an action may run inside a ledger query).
    targets_scratch: Vec<NoteRef>,
    /// Latest controller values on any channel, as scripts read them.
    cc_values: [u8; 130],
    last_message: Text,
    /// Calls per action the engine does not implement yet, by action id.
    unhandled: Box<[u32]>,
    called: Box<[u32]>,
    /// The instrument's level, pan and tune (a script's group -1).
    instrument: Level,
    /// The instrument insert rack, on the final mix.
    rack: racks::Rack,
    /// One single-slot rack per send slot, its input, then the main rack.
    sends: Box<[racks::Rack]>,
    taps: Box<[Box<[[f32; 2]]>]>,
    main: racks::Rack,
    /// Callbacks run so far (24 bits): the source of `$NI_CALLBACK_ID`.
    callbacks_started: u32,
    /// Per control, parameters the script set other than its value.
    control_pars: Box<[Vec<(i32, i32)>]>,
    /// The plan's groups as scripts have changed them.
    groups: Box<[Group]>,
    /// Shared EQ chains, and each zone's chain.
    chains: Vec<filters::Chain>,
    zone_chain: Box<[Option<u16>]>,
    /// Per group, the insert slot of its filter that runs per voice.
    voice_filters: Box<[Option<u8>]>,
    /// One voice's audio on its way through its filter.
    scratch: Box<[[f32; 2]]>,
}

impl Engine {
    /// Allocates every pool the plan asks for, plus a quarter more voices so
    /// stolen voices can fade. Not real-time.
    #[must_use]
    pub fn new(plan: Arc<Plan>, stream: StreamClient, rate: u32) -> Self {
        let shared: Box<[Group]> = plan.groups.clone().into();
        let rack = racks::Rack::new(&plan.inserts, rate);
        let sends: Box<[racks::Rack]> = plan
            .sends
            .iter()
            .map(|slot| racks::Rack::new(std::slice::from_ref(slot), rate))
            .collect();
        let taps = (0..sends.len())
            .map(|_| vec![[0.0; 2]; MAX_SPAN as usize].into())
            .collect();
        let main = racks::Rack::new(&plan.main, rate);
        let pool = plan.voices + plan.voices / 4 + 1;
        let voices = (0..pool)
            .map(|_| Slot {
                voice: Voice::allocate(),
                note: None,
                zone: 0,
                started: SampleTime(0),
                released_at: SampleTime(0),
                released: false,
                fading: false,
                factor: 1.0,
                filter: None,
            })
            .collect();
        let buses = plan
            .buses
            .iter()
            .map(|b| BusState {
                gain: b.gain,
                enabled: b.enabled,
                rack: racks::Rack::new(&b.inserts, rate),
                pan: b.pan,
                mix: vec![[0.0; 2]; MAX_SPAN as usize].into(),
            })
            .collect();
        let notes = Ledger::new(plan.notes);
        let capacity = notes.capacity();
        let groups = plan
            .zones
            .iter()
            .map(|z| z.group as usize + 1)
            .max()
            .unwrap_or(0);
        let vm = plan
            .script
            .as_ref()
            .map(|s| Instance::new(&s.program, s.coroutines.max(1)));
        let control_pars = plan
            .script
            .as_ref()
            .map(|s| vec![Vec::new(); s.controls.cells.len()].into())
            .unwrap_or_default();
        let mut engine = Self {
            now: SampleTime(0),
            rate: rate.max(1),
            queue: EventQueue::new(plan.queue),
            hits: Vec::with_capacity(notes.capacity()),
            ended: Vec::with_capacity(notes.capacity()),
            behaviour: Behaviour::new(&plan),
            notes,
            plan,
            stream,
            voices,
            buses,
            pedal: [false; 16],
            stats: Stats::default(),
            vm,
            groups: shared,
            running: Running::IDLE,
            pending: Vec::with_capacity(capacity),
            masks: GroupMasks::new(capacity, groups),
            targets_scratch: Vec::with_capacity(capacity),
            cc_values: [0; 130],
            last_message: Text::EMPTY,
            unhandled: vec![0; kontra_vm::Action::all().len()].into(),
            called: vec![0; kontra_vm::Action::all().len()].into(),
            control_pars,
            callbacks_started: 0,
            instrument: Level::UNITY,
            rack,
            sends,
            taps,
            main,
            chains: Vec::new(),
            zone_chain: Box::new([]),
            voice_filters: Box::new([]),
            scratch: vec![[0.0; 2]; MAX_SPAN as usize].into(),
        };
        engine.cc_values[7] = 100;
        engine.cc_values[11] = 127;
        let plan = Arc::clone(&engine.plan);
        if let Some(script) = &plan.script {
            if let Some(init) = script.callbacks.init {
                engine.run_callback(init, Running::new(Kind::Init, None));
            }
            if let Some(vm) = engine.vm.as_mut() {
                for &(cell, value) in &script.restore {
                    vm.set_cell(cell as usize, value);
                }
                for &(cell, text) in &script.restore_texts {
                    vm.set_text(cell as usize, text);
                }
            }
            if let Some(changed) = script.callbacks.persistence_changed {
                engine.run_callback(changed, Running::new(Kind::Init, None));
            }
        }
        engine.build_chains();
        engine
    }

    #[must_use]
    pub fn now(&self) -> SampleTime {
        self.now
    }

    #[must_use]
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Host notes that ended during the last [`Engine::process`] (CLAP note end).
    #[must_use]
    pub fn ended(&self) -> &[HostNote] {
        &self.ended
    }

    /// Voices sounding, fading ones included.
    #[must_use]
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|s| s.voice.is_active()).count()
    }

    /// Voices sounding and not fading out: what polyphony counts.
    #[must_use]
    pub fn playing_voices(&self) -> usize {
        self.voices
            .iter()
            .filter(|s| s.voice.is_active() && !s.fading)
            .count()
    }

    /// The controller's current value (128 is aftertouch, 129 pitch bend).
    #[must_use]
    pub fn controller(&self, cc: usize) -> Option<u8> {
        self.cc_values.get(cc).copied()
    }

    /// The current articulation keyswitch.
    #[must_use]
    pub fn keyswitch(&self) -> Option<u8> {
        self.behaviour.keyswitch
    }

    pub fn set_bus_gain(&mut self, bus: usize, gain: f32) {
        if let Some(b) = self.buses.get_mut(bus) {
            b.gain = gain;
        }
    }

    /// A disabled bus (purged mic) fades its voices and starts no new ones.
    pub fn set_bus_enabled(&mut self, bus: usize, enabled: bool) {
        if let Some(b) = self.buses.get_mut(bus) {
            b.enabled = enabled;
        }
        if !enabled {
            let fade = self.fade_frames();
            let plan = self.plan.clone();
            for slot in self.voices.iter_mut().filter(|s| s.voice.is_active()) {
                if plan.zones.get(slot.zone).is_some_and(|z| z.bus == bus) {
                    slot.voice.fade(fade);
                    slot.fading = true;
                }
            }
        }
    }

    /// Renders `out.len()` frames from [`Engine::now`], applying each input at
    /// its sample time. Input stamped before now applies at once.
    pub fn process(&mut self, out: &mut [[f32; 2]], input: &[Timed<Event>]) {
        out.fill([0.0; 2]);
        self.ended.clear();
        for e in input {
            if !self.queue.push(e.at, Work::Input(e.body)) {
                self.stats.events_refused += 1;
            }
        }
        let start = self.now;
        let end = start.advance(Frames(u32::try_from(out.len()).unwrap_or(u32::MAX)));
        while self.now < end {
            while let Some(work) = self.queue.pop_due(self.now) {
                self.dispatch(work);
            }
            let until = self
                .queue
                .next_at()
                .map_or(end, |at| at.min(end))
                .min(SampleTime(self.now.0 + MAX_SPAN));
            let len = (until.0 - self.now.0) as usize;
            self.render_span(len);
            let from = (self.now.0 - start.0) as usize;
            if let Some(span) = out.get_mut(from..from + len) {
                self.mix_out(span);
            }
            self.now = until;
        }
        self.end_silent_notes();
    }

    fn render_span(&mut self, len: usize) {
        self.refresh_timed();
        for bus in &mut self.buses {
            if let Some(mix) = bus.mix.get_mut(..len) {
                mix.fill([0.0; 2]);
            }
        }
        for slot in self.voices.iter_mut().filter(|s| s.voice.is_active()) {
            let bus = self.plan.zones.get(slot.zone).map_or(0, |z| z.bus);
            let chain = self.zone_chain.get(slot.zone).copied().flatten();
            let mix = match chain.and_then(|c| self.chains.get_mut(usize::from(c))) {
                Some(chain) => {
                    chain.live = true;
                    chain.mix.get_mut(..len)
                }
                None => self.buses.get_mut(bus).and_then(|b| b.mix.get_mut(..len)),
            };
            let Some(mix) = mix else {
                continue;
            };
            let before = slot.voice.missing_frames();
            match (slot.filter.as_mut(), self.scratch.get_mut(..len)) {
                (Some(filter), Some(scratch)) => {
                    scratch.fill([0.0; 2]);
                    slot.voice.render(&self.stream, self.now.0, scratch);
                    filter.process(scratch);
                    for (m, x) in mix.iter_mut().zip(scratch.iter()) {
                        m[0] += x[0];
                        m[1] += x[1];
                    }
                }
                _ => slot.voice.render(&self.stream, self.now.0, mix),
            }
            self.stats.missing_frames += slot.voice.missing_frames() - before;
        }
        self.mix_chains(len);
    }

    fn dispatch(&mut self, work: Work) {
        let event = match work {
            Work::Input(event) => event,
            Work::Resume(co) => return self.resume(co),
            Work::Release(note) => return self.end_note(note),
        };
        match event {
            Event::NoteOn {
                note,
                velocity,
                tune,
            } => self.note_on(note, velocity, tune),
            Event::NoteOff(pattern) | Event::NoteOffVelocity(pattern, _) => self.note_off(pattern),
            Event::Choke(pattern) => self.choke(pattern),
            Event::Expression(pattern, expression) => self.expression(pattern, expression),
            Event::Ump(words) => self.ump(words),
        }
    }

    fn fade_frames(&self) -> u32 {
        (FADE_SECONDS * f64::from(self.rate)) as u32
    }

    fn note_on(&mut self, host: HostNote, velocity: f64, tune: f64) {
        self.behaviour.press(&self.plan, host.key);
        let draw = self.behaviour.draw(host.key);
        let Some(note) = self
            .notes
            .insert(Note::new(host, velocity, tune, self.now, draw))
        else {
            self.stats.notes_refused += 1;
            return;
        };
        self.masks.reset(note.slot());
        self.pending.push(note);
        match self.plan.script.as_ref().and_then(|s| s.callbacks.note) {
            Some(entry) => {
                self.run_callback(entry, Running::new(Kind::Note, Some(note)));
            }
            None => self.start_pending(),
        }
    }

    /// Starts every zone `accept`s whose rules match the note, all at now.
    /// `attack` steps round-robin counters; release triggers re-read the attack's draw.
    fn start_zones(&mut self, note: NoteRef, attack: bool, accept: impl Fn(Trigger) -> bool) {
        let Some(n) = self.notes.get(note).copied() else {
            return;
        };
        let plan = self.plan.clone();
        let held_seconds = (self.now.0 - n.at.0) as f32 / self.rate as f32;
        for (index, zone) in plan.zones.iter().enumerate() {
            let selected = accept(zone.trigger)
                && self.masks.allows(note.slot(), zone.group)
                && self
                    .groups
                    .get(zone.group as usize)
                    .is_none_or(|g| !g.purged)
                && zone.keys.contains(&n.key)
                && zone.velocities.contains(&n.velocity_byte)
                && self.buses.get(zone.bus).is_some_and(|b| b.enabled);
            if !selected
                || !self
                    .behaviour
                    .starts(zone, n.host.channel, n.key, n.draw, attack)
            {
                continue;
            }
            self.choke_by(zone.group, note);
            self.limit_group(zone.group);
            if let Some(vg) = zone.voice_group {
                self.limit_voice_group(vg);
            }
            let factor = if zone.trigger == Trigger::Release {
                10f32.powf(-zone.release_decay_db * held_seconds / 20.0)
                    * self.release_counter(zone, held_seconds)
            } else {
                1.0
            };
            let mut region = zone.region.clone();
            if !region.reverse {
                let script = u64::from(n.offset) * u64::from(zone.sample_rate) / 1_000_000;
                let frames = self.start_offset(zone, &n, held_seconds);
                region.start = region
                    .start
                    .saturating_add(script)
                    .saturating_add_signed(frames)
                    .min(region.end);
            }
            let start = VoiceStart {
                region,
                step: self.step(zone, &n, self.age(self.now, None)),
                gain: self.voice_gain(zone, &n, factor, self.age(self.now, None)),
                envelope: self
                    .groups
                    .get(zone.group as usize)
                    .map_or(Ahdsr::GATE, |g| g.envelope),
            };
            let now = self.now;
            let rate = self.rate;
            let filter = self
                .cutoff(zone, &n, self.age(self.now, None))
                .map(|(f, hz)| {
                    let mut svf = Svf::new(filters::shape(&f));
                    svf.set_cutoff(hz, rate);
                    svf
                });
            let Some(slot) = self.free_slot() else {
                continue;
            };
            slot.voice.start(start);
            slot.filter = filter;
            slot.note = Some(note);
            slot.zone = index;
            slot.started = now;
            slot.released = false;
            slot.fading = false;
            slot.factor = factor;
        }
    }

    /// Fades voices that a new voice of `group` chokes (`off_by`), except the note's own.
    fn choke_by(&mut self, group: u32, note: NoteRef) {
        let fade = self.fade_frames();
        let plan = self.plan.clone();
        for slot in self
            .voices
            .iter_mut()
            .filter(|s| s.voice.is_active() && !s.fading && s.note != Some(note))
        {
            if plan
                .zones
                .get(slot.zone)
                .is_some_and(|z| z.off_by == Some(group))
            {
                slot.voice.fade(fade);
                slot.fading = true;
            }
        }
    }

    /// Makes room for one more voice in `group` under its polyphony limit.
    fn limit_group(&mut self, group: u32) {
        let Some(&(_, limit)) = self.plan.polyphony.iter().find(|(g, _)| *g == group) else {
            return;
        };
        let plan = self.plan.clone();
        let in_group = |s: &Slot| {
            s.voice.is_active()
                && !s.fading
                && plan.zones.get(s.zone).is_some_and(|z| z.group == group)
        };
        if self.voices.iter().filter(|s| in_group(s)).count() < usize::from(limit) {
            return;
        }
        let fade = self.fade_frames();
        if let Some(oldest) = self
            .voices
            .iter_mut()
            .filter(|s| in_group(s))
            .min_by_key(|s| (!s.released, s.started))
        {
            oldest.voice.fade(fade);
            oldest.fading = true;
        }
    }

    /// An idle voice. At the plan's polyphony the oldest playing voice fades
    /// (released ones first) and an idle reserve voice takes the new start.
    fn free_slot(&mut self) -> Option<&mut Slot> {
        if self.playing_voices() >= self.plan.voices {
            let fade = self.fade_frames();
            if let Some(victim) = self
                .voices
                .iter_mut()
                .filter(|s| s.voice.is_active() && !s.fading)
                .min_by_key(|s| (!s.released, s.started))
            {
                victim.voice.fade(fade);
                victim.fading = true;
                self.stats.voices_stolen += 1;
            }
        }
        let index = match self.voices.iter().position(|s| !s.voice.is_active()) {
            Some(i) => i,
            None => {
                // Even the fade reserve is full: cut the oldest fading voice.
                let (i, _) = self
                    .voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, s)| (!s.fading, s.started))?;
                self.stats.voices_cut += 1;
                i
            }
        };
        let slot = self.voices.get_mut(index)?;
        slot.voice.kill();
        Some(slot)
    }

    fn note_off(&mut self, pattern: HostPattern) {
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.matching(pattern, &mut hits);
        for &note in &hits {
            let Some(n) = self.notes.get_mut(note) else {
                continue;
            };
            if n.released {
                continue;
            }
            let channel = n.host.channel;
            if let Some(entry) = self.plan.script.as_ref().and_then(|s| s.callbacks.release)
                && self
                    .run_callback(entry, Running::new(Kind::Release, Some(note)))
                    .ignore_release
            {
                continue;
            }
            if self
                .pedal
                .get(usize::from(channel))
                .copied()
                .unwrap_or(false)
            {
                if let Some(n) = self.notes.get_mut(note) {
                    n.pedal_held = true;
                }
            } else {
                self.release(note);
            }
        }
        self.hits = hits;
    }

    /// A script note's note-off (its duration, `note_off`, or its parent's
    /// release): like a key-up it runs the release callback, which cannot
    /// ignore it, then releases the note (KSP manual, note_off and play_note).
    fn end_note(&mut self, note: NoteRef) {
        let Some(n) = self.notes.get(note) else {
            return;
        };
        if n.released {
            return;
        }
        if n.origin != Origin::Host
            && let Some(entry) = self.plan.script.as_ref().and_then(|s| s.callbacks.release)
        {
            self.run_callback(entry, Running::new(Kind::Release, Some(note)));
        }
        self.release(note);
    }

    /// Ends `note` at the current time, after the running callback returns.
    pub(crate) fn queue_end(&mut self, note: NoteRef) {
        if !self.queue.push(self.now, Work::Release(note)) {
            self.stats.events_refused += 1;
            self.release(note);
        }
    }

    /// Releases a note's voices and starts its release triggers.
    fn release(&mut self, note: NoteRef) {
        let Some(n) = self.notes.get_mut(note) else {
            return;
        };
        if n.released {
            return;
        }
        n.released = true;
        n.pedal_held = false;
        for slot in self
            .voices
            .iter_mut()
            .filter(|s| s.note == Some(note) && !s.released)
        {
            slot.released_at = self.now;
            // A one-shot plays to its end marker whatever the key does.
            if !self.plan.zones.get(slot.zone).is_some_and(|z| z.one_shot) {
                slot.voice.release();
            }
            slot.released = true;
        }
        self.start_zones(note, false, |trigger| trigger == Trigger::Release);
        // Script notes played for the length of this one end with it.
        let mut children = std::mem::take(&mut self.targets_scratch);
        self.notes
            .select(&mut children, |n| n.origin == Origin::Child(note));
        for &child in &children {
            self.queue_end(child);
        }
        self.targets_scratch = children;
    }

    fn choke(&mut self, pattern: HostPattern) {
        let fade = self.fade_frames();
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.matching(pattern, &mut hits);
        for &note in &hits {
            if let Some(n) = self.notes.get_mut(note) {
                n.released = true;
            }
            for slot in self.voices.iter_mut().filter(|s| s.note == Some(note)) {
                slot.voice.fade(fade);
                slot.fading = true;
            }
        }
        self.hits = hits;
    }

    fn expression(&mut self, pattern: HostPattern, expression: NoteExpression) {
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.matching(pattern, &mut hits);
        for &note in &hits {
            let Some(n) = self.notes.get_mut(note) else {
                continue;
            };
            match expression {
                NoteExpression::Tune(semitones) => n.expression_tune = semitones,
                NoteExpression::Gain(g) => n.expression_gain = g as f32,
                NoteExpression::Pan(p) => n.expression_pan = p as f32,
                // ponytail: pressure and brightness reach the modulation matrix once it exists.
                NoteExpression::Pressure(_) | NoteExpression::Brightness(_) => continue,
            }
            self.refresh_note(note, None);
        }
        self.hits = hits;
    }

    /// Recomputes pitch and gain of a note's voices; with `cc`, only the gain
    /// of voices whose zone crossfades on that controller.
    fn refresh_note(&mut self, note: NoteRef, cc: Option<u8>) {
        let Some(n) = self.notes.get(note).copied() else {
            return;
        };
        let plan = self.plan.clone();
        for i in 0..self.voices.len() {
            let Some(slot) = self.voices.get(i) else {
                continue;
            };
            if slot.note != Some(note) || !slot.voice.is_active() {
                continue;
            }
            let Some(zone) = plan.zones.get(slot.zone) else {
                continue;
            };
            if cc.is_some_and(|cc| !self.follows(zone, cc)) {
                continue;
            }
            let age = self.age(slot.started, slot.released.then_some(slot.released_at));
            let gain = self.voice_gain(zone, &n, slot.factor, age);
            let step = self.step(zone, &n, age);
            let pitch = cc.is_none_or(|cc| self.pitch_follows(zone, cc));
            let cutoff = self.cutoff(zone, &n, age).map(|(_, hz)| hz);
            let rate = self.rate;
            if let Some(slot) = self.voices.get_mut(i) {
                if let (Some(f), Some(hz)) = (slot.filter.as_mut(), cutoff) {
                    f.set_cutoff(hz, rate);
                }
                slot.voice.set_gain(gain);
                if pitch {
                    slot.voice.set_step(step);
                }
            }
        }
    }

    /// Ends notes with no sounding voice and reports them to the host. A key
    /// still down stays alive while the plan has release triggers to play.
    fn end_silent_notes(&mut self) {
        // A held key stays a live event while something may still act on its release.
        let release_triggers = self
            .plan
            .zones
            .iter()
            .any(|z| z.trigger == Trigger::Release)
            || self
                .plan
                .script
                .as_ref()
                .is_some_and(|s| s.callbacks.release.is_some());
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.select(&mut hits, |_| true);
        for &note in &hits {
            let sounding = self
                .voices
                .iter()
                .any(|s| s.note == Some(note) && s.voice.is_active());
            let waiting = release_triggers
                && self
                    .notes
                    .get(note)
                    .is_some_and(|n| n.origin == Origin::Host && !n.released);
            if !sounding
                && !waiting
                && let Some(n) = self.notes.remove(note)
            {
                self.ended.push(n.host);
            }
        }
        self.hits = hits;
        for slot in self.voices.iter_mut().filter(|s| !s.voice.is_active()) {
            slot.note = None;
        }
    }
}
