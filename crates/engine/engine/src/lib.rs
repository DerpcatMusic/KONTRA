//! The audio-thread engine: one clock, one queue, one note ledger, one voice
//! pool, one render loop (02-ENGINE.md, 13-FRESH-CORE.md).
//!
//! Every event carries a sample time and takes effect at it; the loop renders
//! up to the next due item, so the host block size never changes the output
//! (rule 3). A note owns the voices it starts, and they all begin on the
//! note's sample (rule 4). Sampler behaviour (articulations, round robin,
//! crossfades, release triggers, voice groups, mic buses) is plan data the
//! engine executes, not script code. Nothing allocates after [`Engine::new`].

mod admit;
mod behaviour;
mod buses;
mod callbacks;
mod controls;

mod engine_pars;
mod filters;
mod fx_types;
mod group_masks;
mod group_racks;
mod impulse_knobs;
mod inspect;
mod knobs;
mod ledger;
mod midi;
mod modulators;
mod par_display;
mod params;
mod pgs;
mod plan;
mod queue;
mod racks;

mod script;
mod transport;
mod ui;
mod voice_gain;
mod voice_groups;
mod voice_rack;

use std::sync::Arc;

use kontra_stream::StreamClient;
use kontra_time::Frames;
use kontra_voice::{Voice, VoiceStart};

pub use engine_pars::{EnginePar, EnginePars};
pub use fx_types::{EffectRequest, Prepared};
pub use kontra_effects::response;
pub use kontra_effects::stem;
pub use kontra_events::{Event, HostNote, HostPattern, NoteExpression};
pub use kontra_ir as ir;
pub use kontra_script_ui::{Key, KeyRange, MenuItem, Page};
pub use kontra_time::SampleTime;
pub use kontra_vm::{Action, Faults, Foreign, Op, Program, Text};
pub use kontra_voice::{Ahdsr, Loop, Region};
pub use ledger::NoteRef;
pub use plan::{
    Atom, Bus, Control, Crossfade, Group, Level, Plan, Response, RoundRobin, RrCounter, Trigger,
    Zone,
};
pub use queue::Timed;
pub use script::{Callbacks, Controls, LookPars, ScriptPlan};
pub use transport::Play;
pub use ui::{ControlLook, FIRST_FONT};

use behaviour::Behaviour;
use buses::BusState;
use group_masks::GroupMasks;
use ledger::{Ledger, Note, Origin};
use queue::{EventQueue, Work};
use script::{Kind, Running};

use kontra_effects::MAX_SPAN;
use kontra_effects::matrix as effects;
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
    /// Key released.
    released: bool,
    /// When the envelope began its release; a one-shot's never does.
    released_at: Option<SampleTime>,
    /// Fading out after a steal, choke or group limit; no longer counts as playing.
    fading: bool,
    /// Extra gain fixed at start (release-trigger decay).
    factor: f32,
    /// Its group's inserts that run on this voice alone.
    rack: Vec<voice_rack::Unit>,
    /// Its lagging pitch routes' semitones: where they are, where they go.
    bend: [f32; 2],
    /// Its group's pan, gliding the same way.
    pan: [f32; 2],
    /// Cycles its LFOs' live rate routes added ([`modulators::Age::warp`]).
    warp: [f32; modulators::WARPS],
    /// Its chaotic LFOs' states ([`modulators::Chaos`]).
    chaos: [modulators::Chaos; modulators::WARPS],
}

/// Per control, the parameters a script set other than its value.
type ControlPars = Box<[Vec<(i32, i32)>]>;

pub struct Engine {
    now: SampleTime,
    rate: u32,
    /// Host tempo, quarter notes per minute (tempo-synced LFOs).
    tempo: f32,
    transport: transport::Transport,
    /// Script modulation values sent to every event, read by notes not sent
    /// their own; while one ramps (until `ramping_until`) voices re-read routes.
    script_mods: Box<[Option<(u16, ledger::Ramp)>]>,
    ramping_until: u64,
    plan: Arc<Plan>,
    /// Responses scripts loaded while loading (`load_ir_sample`): name,
    /// how prepared, the response. Convolution indices past the plan's.
    loaded: Vec<(String, ir::Impulse, Response)>,
    init_impulses: Vec<impulse_knobs::InitImpulse>,
    /// Effects scripts asked to load since the host last drained them.
    requests: Vec<fx_types::EffectRequest>,
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
    /// Each script slot's state; taken out while its callback runs.
    vms: Box<[Option<kontra_vm::Vm>]>,
    running: Running,
    /// Notes through every script slot, starting once the callback returns.
    pending: Vec<NoteRef>,
    /// Notes a callback played, still to pass the slots after it.
    forward: Vec<NoteRef>,
    pgs: pgs::Pgs,
    /// Each script slot's listener timer.
    /// Per script slot: the millisecond and beat timers.
    timers: Box<[callbacks::Timers]>,
    /// `stop_wait` calls not yet applied: (slot, callback id, skip later waits).
    stop_waits: Vec<(u8, u32, bool)>,
    masks: GroupMasks,
    /// Scratch for script event targets (an action may run inside a ledger query).
    targets_scratch: Vec<NoteRef>,
    /// Latest controller values on any channel, as scripts read them.
    cc_values: [u8; 130],
    last_message: Text,
    /// Calls per action the engine does not implement yet, by action id.
    unhandled: Box<[u32]>,
    called: Box<[u32]>,
    /// The instrument's level, pan and tune (a script's group -1); level and
    /// pan move [`Self::output`], after every effect.
    instrument: Level,
    /// The expression controller's share of the output gain.
    expression: f32,
    /// How the output's pan turns the far channel down.
    balance: ir::Balance,
    output: kontra_dsp::core::Fader,
    /// The instrument insert rack, on the final mix.
    rack: racks::Rack,
    /// One single-slot rack per send slot, its input, then the main rack.
    sends: Box<[racks::Rack]>,
    taps: Box<[Box<[[f32; 2]]>]>,
    main: racks::Rack,
    ids: callbacks::Ids,
    /// Per script slot, its controls' parameters other than the value.
    control_pars: Box<[ControlPars]>,
    ui: kontra_script_ui::Store,
    /// The picture names scripts asked a font id for, by script slot, in
    /// the order asked.
    fonts: Vec<(u8, Text)>,
    /// The plan's groups as scripts have changed them.
    groups: Box<[Group]>,
    /// Shared EQ chains, and each zone's chain.
    chains: Vec<filters::Chain>,
    zone_chain: Box<[Option<u16>]>,
    /// Per group, the inserts that run on each voice.
    voice_units: Box<[Arc<[voice_rack::Unit]>]>,
    /// Per group, the first slot its summed voices run (group_racks.rs).
    group_split: Box<[Option<usize>]>,
    /// One voice's audio on its way through its filter.
    scratch: Box<[[f32; 2]]>,
}

impl Engine {
    /// Allocates every pool the plan asks for, plus a quarter more voices so
    /// stolen voices can fade. Not real-time.
    #[must_use]
    pub fn new(plan: Arc<Plan>, stream: StreamClient, rate: u32) -> Self {
        let out = plan.output;
        let shared: Box<[Group]> = plan.groups.clone().into();
        let rack = racks::Rack::new(&plan.inserts, rate, &[]);
        let sends: Box<[racks::Rack]> = plan
            .sends
            .iter()
            .map(|slot| racks::Rack::new(std::slice::from_ref(slot), rate, &[]))
            .collect();
        let taps = (0..sends.len())
            .map(|_| vec![[0.0; 2]; MAX_SPAN as usize].into())
            .collect();
        let main = racks::Rack::new(&plan.main, rate, &[]);
        let pool = plan.voices + plan.voices / 4 + 1;
        let voices = (0..pool)
            .map(|_| Slot {
                voice: Voice::allocate(),
                note: None,
                zone: 0,
                started: SampleTime(0),
                released_at: None,
                released: false,
                fading: false,
                factor: 1.0,
                rack: Vec::new(),
                bend: [0.0; 2],
                pan: [0.0; 2],
                warp: [0.0; modulators::WARPS],
                chaos: [modulators::CHAOS_START; modulators::WARPS],
            })
            .collect();
        let buses = plan.buses.iter().map(|b| BusState::new(b, rate)).collect();
        let notes = Ledger::new(plan.notes);
        let capacity = notes.capacity();
        // `$NUM_GROUPS` counts every group, those without zones too (ANALOG
        // STRINGS: 483 in Kontakt; its zones reach group 139).
        let groups = plan
            .zones
            .iter()
            .map(|z| z.group as usize + 1)
            .max()
            .unwrap_or(0)
            .max(plan.groups.len());
        let vms = plan
            .scripts
            .iter()
            .map(|s| {
                Some(kontra_vm::Vm::new(
                    &s.program,
                    s.coroutines,
                    s.foreign.as_ref(),
                ))
            })
            .collect();
        let timers = vec![[None; 2]; plan.scripts.len()].into();
        let control_pars = plan
            .scripts
            .iter()
            .map(|s| vec![Vec::new(); s.controls.cells.len()].into())
            .collect();
        let ui = kontra_script_ui::Store::new(plan.scripts.iter().map(|s| s.controls.cells.len()));
        let mut engine = Self {
            now: SampleTime(0),
            rate: rate.max(1),
            tempo: 120.0,
            transport: transport::Transport::new(plan.scripts.len()),
            script_mods: ledger::script_ramps(&plan),
            ramping_until: 0,
            queue: EventQueue::new(plan.queue),
            hits: Vec::with_capacity(notes.capacity()),
            ended: Vec::with_capacity(notes.capacity()),
            behaviour: Behaviour::new(&plan),
            notes,
            plan,
            loaded: Vec::with_capacity(fx_types::REQUESTS),
            init_impulses: Vec::new(),
            requests: Vec::with_capacity(fx_types::REQUESTS),
            stream,
            voices,
            buses,
            pedal: [false; 16],
            stats: Stats::default(),
            vms,
            groups: shared,
            running: Running::IDLE,
            pending: Vec::with_capacity(capacity),
            forward: Vec::with_capacity(capacity),
            pgs: pgs::Pgs::new(),
            timers,
            stop_waits: Vec::with_capacity(16),
            masks: GroupMasks::new(capacity, groups),
            targets_scratch: Vec::with_capacity(capacity),
            cc_values: [0; 130],
            last_message: Text::EMPTY,
            unhandled: vec![0; kontra_vm::Action::all().len()].into(),
            called: vec![0; kontra_vm::Action::all().len()].into(),
            control_pars,
            ui,
            fonts: Vec::with_capacity(64),
            ids: callbacks::Ids::START,
            instrument: Level {
                gain: 10f32.powf(out.volume_db / 20.0),
                pan: out.pan,
                tune: 0.0,
            },
            expression: 1.0,
            balance: out.balance,
            output: kontra_dsp::core::Fader::new(rate, out.volume_tau_ms, out.pan_slew_ms),
            rack,
            sends,
            taps,
            main,
            chains: Vec::new(),
            zone_chain: Box::new([]),
            voice_units: Box::new([]),
            group_split: Box::new([]),
            scratch: vec![[0.0; 2]; MAX_SPAN as usize].into(),
        };
        engine.cc_values[7] = 100;
        engine.cc_values[11] = 127;
        let plan = Arc::clone(&engine.plan);
        for (slot, script) in (0u8..).zip(&plan.scripts) {
            if let Some(init) = script.callbacks.init {
                engine.run_callback(slot, init, Running::new(Kind::Init, None));
            }
            let restore: Vec<_> = (script.restore.iter())
                .map(|&(cell, value)| (cell, engine.saved_value(slot, cell, value)))
                .collect();
            if let Some(vm) = engine
                .vms
                .get_mut(usize::from(slot))
                .and_then(Option::as_mut)
            {
                for (cell, value) in restore {
                    vm.set_cell(cell as usize, value);
                }
                for &(cell, text) in &script.restore_texts {
                    vm.set_text(cell as usize, text);
                }
            }
            if let Some(changed) = script.callbacks.persistence_changed {
                engine.run_callback(slot, changed, Running::new(Kind::Init, None));
            }
        }
        engine.rebuild_racks();
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
                // Spans end on a fixed grid, so timed modulators move at the
                // same samples whatever the host's block size.
                .min(SampleTime((self.now.0 / MAX_SPAN + 1) * MAX_SPAN));
            let len = (until.0 - self.now.0) as usize;
            self.render_span(len);
            self.advance_transport(len);
            let from = (self.now.0 - start.0) as usize;
            if let Some(span) = out.get_mut(from..from + len) {
                self.mix_out(span);
            }
            self.now = until;
        }
        self.end_silent_notes();
    }

    fn dispatch(&mut self, work: Work) {
        let event = match work {
            Work::Input(event) => event,
            Work::Resume(slot, co, wait) => return self.resume(slot, co, wait),
            Work::Release(note) => return self.end_note(note),
            Work::PgsChanged => return self.run_pgs_changed(),
            Work::Listen {
                slot,
                signal,
                generation,
            } => return self.listen(slot, signal, generation),
            Work::Async { slot, id } => return self.async_complete(slot, id),
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
        self.behaviour.play(&self.plan, host.key, host.channel);
        let draw = self.behaviour.draw(host.key);
        let Some(note) = self.admit(Note::new(host, velocity, tune, self.now, draw)) else {
            self.stats.notes_refused += 1;
            return;
        };
        self.masks.reset(note.slot());
        self.pass_note(note);
        self.start_pending();
    }

    /// Starts every zone `accept`s whose rules match the note, all at now.
    /// `attack` steps round-robin counters; release triggers re-read the attack's draw.
    fn start_zones(&mut self, note: NoteRef, attack: bool, accept: impl Fn(Trigger) -> bool) {
        let Some(n) = self.notes.get(note).copied() else {
            return;
        };
        let plan = self.plan.clone();
        let key = usize::from(n.key);
        if attack && let Some(r) = self.behaviour.rls_reset.get_mut(key) {
            *r = self.now.0;
        }
        let since = self.behaviour.rls_reset.get(key).map_or(n.at.0, |&r| r);
        let held_seconds = self.now.0.saturating_sub(since) as f32 / self.rate as f32;
        for (index, zone) in plan.zones.iter().enumerate() {
            let selected = accept(zone.trigger)
                && self.masks.allows(note.slot(), zone.group)
                && self
                    .groups
                    .get(zone.group as usize)
                    .is_none_or(|g| !g.purged)
                && zone.keys.contains(&n.key)
                && zone.velocities.contains(&n.velocity_byte)
                && (n.zone_slot < 0 || n.zone_slot == i32::from(zone.slot))
                && self
                    .buses
                    .get(plan::bus(&self.groups, zone))
                    .is_some_and(|b| b.enabled);
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
            if let [start, length] = self.loop_shift(zone, &n)
                && (start != 0.0 || length != 0.0)
                && !region.reverse
            {
                region.shift_loops(start, length);
            }
            if let Some(mix) = self.voice_mix(zone, &n, self.age(self.now, None)) {
                region.mix = Some(mix);
            }
            let pan = self.group_pan(zone);
            let (gain, lag) = self.voice_gain(zone, &n, factor, self.age(self.now, None), pan);
            let (step, bend) = self.step(zone, &n, self.age(self.now, None));
            let start = VoiceStart {
                region,
                step: voice_gain::bent(self.plan.route_pitch, step, bend.instant + bend.semitones),
                gain,
                lag,
                envelope: self.envelope(zone, &n, held_seconds),
            };
            let now = self.now;
            let Some(i) = self.free_slot() else {
                continue;
            };
            let Some(slot) = self.voices.get_mut(i) else {
                continue;
            };
            slot.voice.start(start);
            slot.note = Some(note);
            slot.zone = index;
            slot.started = now;
            slot.released = false;
            slot.released_at = None;
            slot.fading = false;
            slot.factor = factor;
            slot.bend = [bend.semitones; 2];
            slot.pan = [pan; 2];
            slot.warp = [0.0; modulators::WARPS];
            slot.chaos = [modulators::CHAOS_START; modulators::WARPS];
            self.start_rack(i, zone, &n, self.age(now, None));
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

    fn note_off(&mut self, pattern: HostPattern) {
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.matching(pattern, &mut hits);
        for &note in &hits {
            let Some(n) = self.notes.get_mut(note) else {
                continue;
            };
            if n.is_up() {
                continue;
            }
            let channel = n.host.channel;
            if self.release_callbacks(note) {
                if let Some(n) = self.notes.get_mut(note) {
                    n.key_up = true;
                }
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
        if n.origin != Origin::Host {
            self.release_callbacks(note);
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
            // A one-shot plays to its end marker whatever the key does.
            if !self.plan.zones.get(slot.zone).is_some_and(|z| z.one_shot) {
                slot.voice.release();
                slot.released_at = Some(self.now);
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
            let age = modulators::Age {
                warp: slot.warp,
                chaos: slot.chaos.map(|c| c[3]),
                ..self.age(slot.started, slot.released_at)
            };
            let pan = [slot.pan[0], self.group_pan(zone)];
            let (gain, lag) = self.voice_gain(zone, &n, slot.factor, age, pan[0]);
            let mix = self.voice_mix(zone, &n, age);
            let (step, bend) = self.step(zone, &n, age);
            let law = self.plan.route_pitch;
            let pitch = cc.is_none_or(|cc| self.pitch_follows(zone, cc));
            self.retune_rack(i, zone, &n, age);
            if let Some(slot) = self.voices.get_mut(i) {
                slot.pan = pan;
                slot.voice.set_gain(gain);
                slot.voice.set_lag(lag);
                if let Some(mix) = mix {
                    slot.voice.set_mix(mix);
                }
                if pitch {
                    slot.bend[1] = bend.semitones;
                    slot.voice
                        .set_step(voice_gain::bent(law, step, bend.instant + slot.bend[0]));
                }
            }
        }
    }
}
