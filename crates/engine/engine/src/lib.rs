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
mod buses;
mod callbacks;
mod controls;
mod detect;

mod filters;
mod fx_types;
mod impulse_knobs;
mod inspect;
mod knobs;
mod midi;
mod midi_in;
mod midi_object;
mod modulators;
mod par_display;
mod params;
mod pgs;
mod racks;
mod recall;
mod refresh;
mod reshaped;
mod runtime;
mod script_files;
mod zone_commands;

mod script;
mod start;
#[cfg(feature = "trace")]
mod trace;
mod transport;
mod ui;
mod voice_gain;
mod voice_groups;
mod voice_rack;
mod zone_edits;
mod zone_index;

use std::sync::Arc;

use kontra_ledger::{group_masks, ledger, queue};
use kontra_racks::group_racks;

use kontra_plan as plan;

use kontra_stream::StreamClient;
use kontra_time::Frames;
use kontra_voice::{KeyUp, Scratch, Voice, VoiceStart};

pub use fx_types::{EffectRequest, Prepared};
pub use inspect::{
    BusRouteObservation, EqRouteObservation, RouteObservation, SharedEffectObservation,
    VoiceEffectObservation, VoiceObservation,
};
pub use kontra_effects::response;
pub use kontra_effects::stem;
pub use kontra_events::{Event, HostNote, HostPattern, NoteExpression};
pub use kontra_ir as ir;
pub use kontra_plan::{
    EnginePar, EnginePars, Recall, Saved, ZoneFacts, ZoneRules, files, midi_file, zone_name,
};
pub use kontra_script_ui::{ControlLook, Key, KeyRange, MenuItem, Page};
pub use kontra_script_ui::{ENTRY, FIRST_FONT};
pub use kontra_time::SampleTime;
pub use kontra_vm::{Action, Faults, Foreign, Op, Program, Text, Vm, WidgetLook};
pub use kontra_voice::{Ahdsr, Loop, Region, RenderMode};
pub use ledger::NoteRef;
pub use pgs::PgsBudget;
pub use plan::{
    Atom, Bus, Control, Crossfade, Dormant, Group, Level, Plan, ReleaseZones, Response, RoundRobin,
    RrCounter, SustainPedal, Trigger, Zone,
};
pub use queue::Timed;
pub use script::{Callbacks, CcLink, Controls, KnobModPlan, LookPars, ScriptPlan};
#[cfg(feature = "trace")]
pub use trace::{RouteUnit, RouteValue, TraceEvent, VoiceEvent};
pub use transport::Play;
pub use zone_commands::detect_sample;
pub use zone_edits::{Unpurge, ZoneEdit};

use buses::BusState;
use group_masks::GroupMasks;
use kontra_behaviour::Behaviour;
use ledger::{Ledger, Note, Origin, Releases};
use queue::{EventQueue, Work};
use script::{Kind, Running};

use kontra_effects::MAX_SPAN;
use kontra_effects::matrix as effects;
/// Loop turns the resumed script callbacks may run in one block, a
/// millisecond or less of a heavy script. A run that reaches it yields at its next
/// loop turn and goes on at the next block, as the rest of a chain of waits
/// does: a block never holds all of a script's chunked work.
const BLOCK_SCRIPT_TURNS: u64 = 12;

/// A voice ended early outside a voice group fades this long, a cut that never clicks.
const FADE_SECONDS: f64 = 0.010;

/// Counters since the engine was created. Each names a refusal or a loss.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Voices faded out because the plan's polyphony was reached.
    pub voices_stolen: u64,
    /// Voices cut without a fade: more than the voice limit were fading at once.
    pub voices_cut: u64,
    /// Note-ons refused because the note table was full.
    pub notes_refused: u64,
    /// Events refused because the queue was full.
    pub events_refused: u64,
    /// Script resumes put off to the next frame: one frame already ran as
    /// many as the queue holds.
    pub resumes_deferred: u64,
    /// Source frames voices played as silence because data was late.
    pub missing_frames: u64,
    /// Script actions this engine does not implement yet (they returned 0).
    pub actions_unhandled: u64,
    /// Script `message` calls.
    pub messages: u64,
    /// Script callbacks started, and waits resumed.
    pub script_starts: u64,
    pub script_resumes: u64,
    /// Script loop turns run, a measure of the work a script did.
    pub script_turns: u64,
    /// Controllers (and aftertouch) a script kept from the instrument, and
    /// those it kept and set its own value for instead.
    pub controllers_ignored: u64,
    pub controllers_replaced: u64,
}

struct Slot {
    voice: Voice,
    /// The owner. `None` only while the voice is idle.
    note: Option<NoteRef>,
    zone: usize,
    started: SampleTime,
    /// Extra gain fixed at start (release-trigger decay).
    factor: f32,
    /// Its group's inserts that run on this voice alone.
    rack: Vec<voice_rack::Unit>,
    /// The delay lines its combs run on, sized at build.
    comb_lines: Vec<[f32; 2]>,
    /// Its lagging pitch routes' semitones: where they are, where they go.
    bend: [f32; 2],
    /// Its group's pan, gliding the same way.
    pan: [f32; 2],
    /// Cycles its LFOs' live rate routes added ([`modulators::Age::warp`]).
    warp: [f32; modulators::WARPS],
    /// Its chaotic LFOs' states ([`modulators::Chaos`]).
    chaos: [modulators::Chaos; modulators::WARPS],
    /// Its envelopes' attack stretch, fixed at note-on ([`modulators::Age::stretch`]).
    stretch: [f32; modulators::WARPS],
    #[cfg(feature = "trace")]
    pending_trace_cc: std::cell::Cell<u128>,
}

pub struct Engine {
    #[cfg(feature = "trace")]
    trace: Option<trace::Trace>,
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
    /// `set_zone_par` calls waiting for the host (zone_edits.rs).
    zone_edits: Vec<ZoneEdit>,
    /// `purge_group(g, 1)` of dormant groups waiting for the host (zone_edits.rs).
    unpurges: Vec<Unpurge>,
    /// Per script slot, the edits refused because that queue was full.
    zone_refused: Box<[u32]>,
    /// The block being rendered ends here; script turns run before it began.
    block_end: SampleTime,
    /// Loop turns resumed callbacks have run this block.
    block_turns: u64,
    /// Loop turns the next resumed script run may take before it yields.
    fuel: u32,
    /// Whether the plan has zones that play on release (a scan of every zone,
    /// so kept, not asked each block).
    release_zones: bool,
    /// The host keys held down (bit = key) as of this ledger version.
    keys_down: (u64, u128),
    /// Each zone's key range, packed: a note's start scans these, not the zones.
    zone_index: Arc<zone_index::ZoneIndex>,
    stream: StreamClient,
    queue: EventQueue,
    notes: Ledger,
    behaviour: Behaviour,
    voices: Vec<Slot>,
    /// The buffers every voice renders through, in turn.
    voice_scratch: Scratch,
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
    /// Notes a running callback changed; their voices refresh when it ends.
    refresh_later: Vec<NoteRef>,
    /// Latest controller values on any channel, as scripts read them.
    cc_values: [u8; 130],
    /// Each key's last polyphonic aftertouch (`%POLY_AT`).
    poly_at: [u8; 128],
    /// Time Machine Pro voice limits: standard, high quality.
    voice_limits: [i32; 2],
    /// What `set_midi` made in the running `on midi_in`: count and messages.
    midi_made: (usize, [[u8; 4]; midi_in::MADE]),
    /// Where `$KSP_TIMER` counts from (`reset_ksp_timer`).
    timer_at: SampleTime,
    last_message: Text,
    /// What a recall of saved values does to the scripts.
    recall: Recall,
    /// The values recalled last, by slot (`read_persistent_var` reads them).
    recalled: Box<[Saved]>,
    /// True while `new` runs the scripts' `init`, off the audio thread.
    building: bool,
    /// While `on init` runs at load, the cells `read_persistent_var`
    /// reloaded, by slot (texts flagged): the end-of-init restore skips them.
    reloaded: Vec<(u8, std::ops::Range<u32>, bool)>,
    /// The MIDI object (`mf_*`).
    midi: midi_object::Object,
    /// Array files scripts asked to load or save, for the control side.
    files: Vec<kontra_plan::files::FileRequest>,
    /// The editor's answers to picks (`PickFile`): slot, async id, and the
    /// file name, none when cancelled; until the script reads them.
    picked: Vec<(u8, i32, Option<kontra_vm::Text>)>,
    /// Calls per action the engine does not implement yet, by action id.
    unhandled: Box<[u32]>,
    /// The engine-parameter addresses behind those calls (par, group, slot,
    /// generic) and their counts; fixed size, the first ones seen.
    unanswered_pars: Box<[([i32; 4], u32)]>,
    called: Box<[u32]>,
    memo: modulators::Memo,
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
    ui: kontra_script_ui::Store,
    /// The picture names scripts asked a font id for, by script slot, in
    /// the order asked.
    /// The plan's groups as scripts have changed them.
    groups: Box<[Group]>,
    /// What each group's routes reshape, by group.
    reshaped: Box<[reshaped::Reshaped]>,
    /// The routes each script `#key` names.
    route_names: ir::RouteNames,
    /// The group whose route amounts the running callback moved, and whether
    /// others too: its voices refresh once, when it returns or waits.
    routes_moved: Option<(usize, bool)>,
    /// Shared EQ chains, and each group's chain (a zone plays through its
    /// group's, so `set_zone_par` moving it needs no rebuild).
    chains: Vec<filters::Chain>,
    group_chain: Box<[Option<u16>]>,
    /// Per group on a shared chain, the private chain it takes once a write
    /// moves its inserts (filters.rs).
    group_private: Box<[Option<u16>]>,
    /// Per group, whether its inserts were written since the last span.
    group_moved: Box<[bool]>,
    groups_moved: bool,
    /// Per group, the inserts that run on each voice.
    voice_units: Box<[Arc<[voice_rack::Unit]>]>,
    /// Per group, the first slot its summed voices run (group_racks.rs).
    group_split: Box<[Option<usize>]>,
    /// One voice's audio on its way through its filter.
    scratch: Box<[[f32; 2]]>,
}

impl Engine {
    /// Allocates every pool the plan asks for, plus as many voices again so
    /// every stolen voice can fade. Not real-time.
    #[must_use]
    pub fn new(plan: Arc<Plan>, stream: StreamClient, rate: u32) -> Self {
        Self::new_inner(plan, stream, rate, None)
    }

    /// Create an engine with bounded tracing enabled before its init callbacks.
    #[cfg(feature = "trace")]
    #[must_use]
    pub fn new_traced(plan: Arc<Plan>, stream: StreamClient, rate: u32, capacity: usize) -> Self {
        Self::new_inner(plan, stream, rate, Some(capacity))
    }

    fn new_inner(
        plan: Arc<Plan>,
        stream: StreamClient,
        rate: u32,
        _trace_capacity: Option<usize>,
    ) -> Self {
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
        // A voice that ends early (stolen, choked or faded by a script)
        // fades for FADE_SECONDS in a spare slot. A chord on a full pool
        // steals one voice per note at once, up to the whole limit, so with
        // a spare per voice no fade is cut short (a click) unless more than
        // the limit are fading at once (tests/playability.rs).
        let pool = 2 * plan.voices + 1;
        let voices = (0..pool)
            .map(|_| Slot {
                voice: Voice::allocate(),
                note: None,
                zone: 0,
                started: SampleTime(0),
                factor: 1.0,
                rack: Vec::new(),
                comb_lines: Vec::new(),
                bend: [0.0; 2],
                pan: [0.0; 2],
                warp: [0.0; modulators::WARPS],
                chaos: [modulators::CHAOS_START; modulators::WARPS],
                stretch: [1.0; modulators::WARPS],
                #[cfg(feature = "trace")]
                pending_trace_cc: std::cell::Cell::new(0),
            })
            .collect();
        let buses = plan.buses.iter().map(|b| BusState::new(b, rate)).collect();
        let notes = Ledger::new(plan.notes);
        let capacity = notes.capacity();
        // `$NUM_GROUPS` counts every group, those without zones too (measured).
        let groups = plan
            .zones
            .iter()
            .map(|z| z.group as usize + 1)
            .max()
            .unwrap_or(0)
            .max(plan.groups.len());
        let script_count = plan.scripts.len();
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
        let ui = kontra_script_ui::Store::new(plan.scripts.iter().map(|s| s.controls.cells.len()));
        let zone_index = Arc::new(zone_index::ZoneIndex::new(&plan.zones));
        let release_zones = plan.zones.iter().any(|z| z.trigger == Trigger::Release);
        let mut engine = Self {
            #[cfg(feature = "trace")]
            trace: _trace_capacity.map(trace::Trace::new),
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
            zone_edits: Vec::with_capacity(zone_edits::ZONE_EDITS),
            unpurges: Vec::with_capacity(16),
            zone_refused: vec![0; script_count].into(),
            block_end: SampleTime(0),
            block_turns: 0,
            fuel: 0,
            release_zones,
            keys_down: (u64::MAX, 0),
            zone_index,
            stream,
            voices,
            voice_scratch: Scratch::allocate(),
            buses,
            pedal: [false; 16],
            stats: Stats::default(),
            vms,
            routes_moved: None,
            route_names: ir::RouteNames::of_names(
                shared
                    .iter()
                    .map(|g| g.mods.iter().map(|m| m.name.as_str())),
            ),
            reshaped: shared.iter().map(reshaped::Reshaped::of).collect(),
            groups: shared,
            running: Running::IDLE,
            pending: Vec::with_capacity(capacity),
            forward: Vec::with_capacity(capacity),
            pgs: pgs::Pgs::new(),
            timers,
            stop_waits: Vec::with_capacity(16),
            masks: GroupMasks::new(capacity, groups),
            targets_scratch: Vec::with_capacity(capacity),
            refresh_later: Vec::with_capacity(64),
            cc_values: [0; 130],
            poly_at: [0; 128],
            voice_limits: kontra_vm::midi::VL_DEFAULT,
            midi_made: (0, [[0; 4]; midi_in::MADE]),
            timer_at: SampleTime(0),
            last_message: Text::EMPTY,
            recall: Recall::Restart,
            recalled: Box::new([]),
            building: true,
            reloaded: Vec::new(),
            midi: midi_object::Object::new(),
            files: Vec::with_capacity(script_files::FILES),
            picked: Vec::with_capacity(script_files::FILES),
            unhandled: vec![0; kontra_vm::Action::all().len()].into(),
            unanswered_pars: vec![([0; 4], 0); 64].into(),
            called: vec![0; kontra_vm::Action::all().len()].into(),
            memo: Default::default(),
            ui,
            ids: callbacks::Ids::START,
            instrument: Level::panned(ir::common::DB_GAIN.eval_unclamped(out.volume_db), out.pan),
            expression: 1.0,
            balance: out.balance,
            output: kontra_dsp::core::Fader::new(rate, out.volume_tau_ms, out.pan_slew_ms),
            rack,
            sends,
            taps,
            main,
            chains: Vec::new(),
            group_chain: Box::new([]),
            group_private: Box::new([]),
            group_moved: Box::new([]),
            groups_moved: false,
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
            engine.restore_after_init(slot);
            engine.persistence_changed(slot);
        }
        engine.pgs.prepare_runtime();
        engine.building = false;
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
            .filter(|s| s.voice.is_active() && !s.voice.is_ending())
            .count()
    }

    /// The controller's current value (129 is channel pressure).
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
        self.block_end = end;
        self.block_turns = 0;
        while self.now < end {
            // One frame runs at most a queue's worth of resumes: a script
            // chain of sub-frame waits goes on next frame, never spins here.
            let mut resumes = 0;
            while let Some((due, work)) = self.queue.pop_due(self.now) {
                if let Work::Resume(..) = work {
                    resumes += 1;
                    if resumes > self.queue.capacity() {
                        let next = queue::Due::at(SampleTime(self.now.0 + 1));
                        self.stats.resumes_deferred += 1;
                        if !self.queue.push_due(next, work) {
                            self.stats.events_refused += 1;
                        }
                        continue;
                    }
                }
                self.dispatch(due, work);
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

    fn dispatch(&mut self, due: queue::Due, work: Work) {
        let event = match work {
            Work::Input(event) => event,
            Work::Resume(slot, co, wait) => return self.resume(slot, co, wait, due),
            Work::Release(note) => return self.end_note(note),
            Work::ParentUp(note) => return self.parent_up(note),
            Work::PgsChanged => return self.run_pgs_changed(),
            Work::Controller { controller, value } => {
                return self.set_controller(i32::from(controller), i32::from(value));
            }
            Work::PolyAt {
                channel,
                key,
                value,
            } => return self.set_poly_at(channel, key, value),
            Work::Listen {
                slot,
                signal,
                generation,
            } => return self.listen(slot, signal, generation),
            Work::Async { slot, id, exit } => return self.async_complete(slot, id, exit),
        };
        if let Some(event) = self.through_midi_in(event, &mut Self::apply) {
            self.apply(event);
        }
    }

    /// An event the scripts' `on midi_in` let through.
    fn apply(&mut self, event: Event) {
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
        // A script that plays its own release notes: the key going down
        // starts its release-trigger counter, not the notes it plays.
        if self.plan.release_zones == ReleaseZones::WithNote
            && let Some(r) = self.behaviour.rls_reset.get_mut(usize::from(host.key))
        {
            *r = self.now.0;
        }
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
    /// Makes room for one more voice in `group` under its polyphony limit.
    fn limit_group(&mut self, group: u32) {
        let Some(&(_, limit)) = self.plan.polyphony.iter().find(|(g, _)| *g == group) else {
            return;
        };
        let plan = self.plan.clone();
        let in_group = |s: &Slot| {
            s.voice.is_active()
                && !s.voice.is_ending()
                && plan.zones.get(s.zone).is_some_and(|z| z.group == group)
        };
        if self.voices.iter().filter(|s| in_group(s)).count() < usize::from(limit) {
            return;
        }
        let oldest = (self.voices.iter().enumerate())
            .filter(|(_, s)| in_group(s))
            .min_by_key(|(_, s)| (!s.voice.is_released(), s.started));
        if let Some((index, _)) = oldest {
            self.steal(index, self.fade_frames());
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
            // `%KEY_DOWN` reads 0 in the key's own `on release`.
            n.key_up = true;
            if let Some(slot) = self.release_callbacks(note) {
                // The key is up whatever the script does with its event:
                // a `play_note(.., -1)` child still ends (KSP manual,
                // play_note), through its own release callbacks.
                let script = self.plan.scripts.get(usize::from(slot));
                if script.is_some_and(|s| s.held_release_ends_children) {
                    self.end_children(note);
                }
                continue;
            }
            if self.pedal.get(usize::from(channel)) == Some(&true) {
                // The key is up (KSP manual): only its sound is held.
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

    /// A `play_note(.., -1)` child's parent is released: its release
    /// callbacks run, and one that ignores it keeps it playing until a
    /// `note_off` (Una Corda's pedal holds its dry tones this way; native
    /// rings on, and stops at the pedal-up `note_off`).
    fn parent_up(&mut self, note: NoteRef) {
        match self.notes.get(note) {
            Some(n) if !n.released && !n.key_up => {}
            _ => return,
        }
        if let Some(slot) = self.release_callbacks(note) {
            if let Some(n) = self.notes.get_mut(note) {
                n.key_up = true;
            }
            // As for a held key-up: its own children still end (note_off).
            let script = self.plan.scripts.get(usize::from(slot));
            if script.is_some_and(|s| s.held_release_ends_children) {
                self.end_children(note);
            }
        } else {
            self.release(note);
        }
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
            .filter(|s| s.note == Some(note) && !s.voice.is_released())
        {
            let one_shot = self.plan.zones.get(slot.zone).is_some_and(|z| z.one_shot);
            let key_up = if one_shot {
                KeyUp::PlayOn
            } else {
                KeyUp::Release
            };
            slot.voice.key_up(self.now.0, key_up);
        }
        if self.plan.release_zones == ReleaseZones::AtRelease
            && self.notes.get(note).map(|n| n.releases) == Some(Releases::OnRelease)
        {
            self.start_zones(note, false, |trigger| trigger == Trigger::Release);
        }
        self.end_children(note);
    }

    /// Ends the script notes played for the length of `note`.
    fn end_children(&mut self, note: NoteRef) {
        // No scratch list: a full queue releases a child at once, which ends
        // its own children through here.
        let mut from = 0;
        while let Some(child) = self.notes.next_child(note, from) {
            from = child.slot() + 1;
            if !self.queue.push(self.now, Work::ParentUp(child)) {
                self.stats.events_refused += 1;
                self.release(child);
            }
        }
    }

    fn choke(&mut self, pattern: HostPattern) {
        let fade = self.fade_frames();
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.matching(pattern, &mut hits);
        self.fade_notes(
            &hits,
            fade,
            kontra_voice::FadeTarget::End,
            kontra_voice::FadeCurve::Linear,
        );
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
                NoteExpression::Pressure(p) => n.expression_pressure = Some(p as f32),
                NoteExpression::Brightness(b) => n.expression_brightness = Some(b as f32),
            }
            self.refresh_note(note, None);
        }
        // A note's pressure is its key's polyphonic aftertouch to the
        // scripts: `on poly_at` hears it, as it hears an MPE controller's.
        let mut keys = [false; 128];
        if let NoteExpression::Pressure(p) = expression {
            for n in hits.iter().filter_map(|&note| self.notes.get(note)) {
                if let Some(k) = keys.get_mut(usize::from(n.key)) {
                    *k = true;
                }
            }
            let value = (p.clamp(0.0, 1.0) * 127.0).round() as u8;
            for (v, _) in self.poly_at.iter_mut().zip(keys).filter(|(_, k)| *k) {
                *v = value;
            }
        }
        // The scripts run with the hit list back in place: they use it.
        self.hits = hits;
        for key in (0u8..).zip(keys).filter_map(|(key, k)| k.then_some(key)) {
            // A note's own expression is the host's: it stands either way.
            let _ = self.run_poly_at(key);
        }
    }
}
