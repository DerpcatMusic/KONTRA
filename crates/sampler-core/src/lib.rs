#![forbid(unsafe_code)]
//! Experimental native ownership kernel. No plugin, file, or language dependencies.
//!
//! Construction/destruction are control-thread operations. After preparation, methods
//! do not allocate or free. Prepared PCM is owned by the runtime. This slice
//! supports resident stereo PCM with bounded rate conversion, native linear envelopes
//! and sample-time commands; vendor fidelity requires separate conformance evidence.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Internal, non-owning links are unlinked before a slot can be reused. Public
/// identities remain generational handles. The niche keeps each optional link
/// one word without adding pointers or allocations to the ownership graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Index(std::num::NonZeroUsize);

impl Index {
    fn new(index: usize) -> Self {
        Self(std::num::NonZeroUsize::new(index + 1).unwrap())
    }
    fn get(self) -> usize {
        self.0.get() - 1
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Siblings {
    previous: Option<Index>,
    next: Option<Index>,
}

mod control;
pub use control::{
    ControlCallback, ControlClient, ControlContext, ControlDefinition, ControlDomain, ControlId,
    ControlOperation, ControlQueueError, ControlReply, ControlRequest, ControlValue, ControlWrite,
    RejectedControls,
};
mod controller_event;
mod performance;
pub use performance::{Keyswitch, PerformanceId, SelectionPolicy, SelectionSnapshot};
mod switching;
pub use switching::{Driver, Selector, Switch, SwitchKeys, Switching};
mod behavior;
use behavior::Continuation;
pub use behavior::{
    BehaviorId, BehaviorOwner, Comparison, Duration, DurationValue, Instruction, Outcome, Program,
    Velocity, WaitLifetime,
};
mod stages;
pub use stages::Stage;
mod stream;
pub use stream::{
    DecodeFailure, DecodeJob, PAGE_FRAMES, PageKey, PageStatus, PageUpdate, RejectedDecode,
    StreamCache, StreamError, StreamWorker,
};
mod source;
pub use source::{Direction, Loop, LoopMode, LoopShape, Playback, SampleDemand};
mod bus;
pub use bus::{Bus, BusMix, BusSend};
pub use resample::{ResampleQuality, read_radius};
mod dsp;
pub use dsp::{
    Biquad, ControlRange, Delay, FilterKind, Impulse, MAX_IMPULSE_FRAMES, Parameter, Processor,
    ReverbSettings, StateVariableFilter, SvfMode, VoiceChain,
};
mod envelope;
use envelope::EnvelopeState;
pub use envelope::{Envelope, EnvelopeCurve};
mod gate;
mod modulation;
mod plan_programs;
mod script_params;
mod steal;
mod voice_mod;
pub use plan_programs::{PlanProgram, SignalProgram};
pub use script_params::{EnvelopeStage, GroupParams, ParamScope};
pub use steal::{Kill, Stealing, VoiceLimit};
pub use voice_mod::{
    Breakpoint, Breakpoints, Lfo, LfoRate, LfoShape, ModProgram, ModRoute, ModScale, ModSource,
    ModTarget,
};
mod ownership;
use modulation::RenderedExpression;
pub use modulation::{Destination, ExpressionSource, Modulation, Route};
mod pitch;
pub use pitch::NotePitch;
mod groups;
mod note_event;
pub use note_event::NoteProperties;
mod packed;
mod plans;
mod prepare;
pub use packed::Packed;
mod release;
mod grow;
mod parallel;
pub use parallel::Threads;
mod render;
pub use release::{
    GateRelease, KeyRelease, ReleaseCause, ReleaseContext, ReleaseOptions, ReleaseReserve,
    ReleaseStatus, ReleaseVelocity, Trigger,
};
pub use render::RuntimeStats;
mod resample;
use plans::{Generation, PlanQueues};
pub use plans::{PlanControl, PlanError, PlanId, PlanTransfer, RejectedPlan};
pub use prepare::{
    AssetId, ControllerCondition, Pcm, Prepared, Ranges, Region, Tuning, VelocityCurve,
    service_mipmaps,
};
mod integer;
pub mod lower;
pub use integer::{IntegerBinary, IntegerUnary};
mod ops;
mod script;
pub use ops::{
    CALL_DEPTH, EFFECT_ARGS, EFFECT_CAPACITY, Effect, HOST_VALUES, IntegerExtra, Op, RealBinary,
    RealUnary, STORE_KEY, ScriptResources, TEXT_CAPACITY, Text, TextPart, TextRef, real, real_bits,
};
pub use script::{ScriptArray, ScriptInstanceId};
mod schedule;
mod variation;
use gate::Channel;
pub use gate::{ChannelAddress, ChannelId, ChannelScope};
pub use ownership::{Expression, ExpressionId, FamilyId, Inheritance, MAX_EXPRESSION_GAIN};
use ownership::{ExpressionOwner, Family};
pub use schedule::Event;
use schedule::{Action, Scheduled};
pub use variation::{Sequence, SequenceScope, Take, TakePolicy};

pub type Frame = [f32; 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Handle {
    runtime: u64,
    index: usize,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteId(Handle);

/// Process-local ownership domain. Remains stable across moves and plan changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceId(Handle);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Native,
    Midi1,
    Midi2,
    Clap,
    Vst3,
}

/// Original host address, never the transposed playback address. Adapters own
/// wildcard matching and protocol-specific interpretation of absent/signed IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub protocol: Protocol,
    pub port: u16,
    /// UMP group; zero for transports without group addressing.
    pub group: u8,
    pub channel: u8,
    pub key: u8,
    pub external_id: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Capacity,
    InvalidInput,
    /// Required decoded source data is not resident; no source was admitted.
    NotReady,
    StaleHandle,
    DuplicateInput,
    ClosedNote,
    ClosedFamily,
    PastEvent,
    ClockOverflow,
    ArithmeticOverflow,
    RevisionConflict,
    /// Unbiased selection exceeded its fixed draw budget; no decision was committed.
    RandomBudget,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Capacity => "a prepared capacity is exhausted",
            Self::InvalidInput => "invalid input",
            Self::NotReady => "required source data is not resident",
            Self::StaleHandle => "the handle no longer refers to a live object",
            Self::DuplicateInput => "the input is already held",
            Self::ClosedNote => "the note's gate is closed",
            Self::ClosedFamily => "the family is sealed",
            Self::PastEvent => "the event time is in the past",
            Self::ClockOverflow => "the sample clock would overflow",
            Self::ArithmeticOverflow => "arithmetic overflow",
            Self::RevisionConflict => "the revision changed",
            Self::RandomBudget => "random selection exceeded its draw budget",
        })
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub notes: usize,
    pub channels: usize,
    /// Fixed musical routing domains, independent of expressive channels; at least one.
    pub performances: usize,
    pub families: usize,
    pub expressions: usize,
    pub voices: usize,
    /// Retained take decisions, independently of source/family and note capacities.
    pub decisions: usize,
    pub commands: usize,
    pub behaviors: usize,
    /// Instructions a callback runs per block before it is preempted and
    /// continued at the next block (see [`Limits::DEFAULT_BEHAVIOR_FUEL`]).
    pub behavior_fuel: usize,
    pub behavior_cells: usize,
    /// Total note-owned integer cells, divided evenly across logical note slots.
    pub note_cells: usize,
}

impl Limits {
    /// About 115 us of script work per callback per block on a desktop CPU,
    /// under a tenth of the 64-frame deadline at 48 kHz. Measured over 52
    /// library scripts: all but two finish every callback within it; the
    /// other two need up to five blocks.
    pub const DEFAULT_BEHAVIOR_FUEL: usize = 10_000;

    /// Keys a part's scripts are sized to run at once (chords, pedalled runs).
    pub const SCRIPT_KEYS: usize = 32;
    /// Cap on script callback state per part: 4M cells, 32 MB.
    pub const SCRIPT_CELLS: usize = 1 << 22;

    /// Script callbacks a plan can have running at once: each note of
    /// [`Self::SCRIPT_KEYS`] keys runs a release callback in every stage and
    /// notes the scripts play pass the later stages too (about four per
    /// stage), plus one listener per stage; within [`Self::SCRIPT_CELLS`].
    /// 16 for a plan without stages.
    pub fn script_capacity(plan: &Prepared) -> usize {
        let stages = plan.stage_count();
        if stages == 0 {
            return 16;
        }
        let wanted = (4 * stages * Self::SCRIPT_KEYS + stages).clamp(16, 4096);
        wanted
            .min(Self::SCRIPT_CELLS / plan.behavior_local_count().max(1))
            .max(16)
    }

    /// Capacities for playing `plan`: `notes` held at once and `voices`,
    /// with script state sized by [`Self::script_capacity`]. Hosts and test
    /// harnesses share this so a plan that plays in one plays in the other.
    /// `voices` is the initial polyphony; each script note also gets the
    /// release voices of every stage on top, which its release phase reserves.
    pub fn for_plan(plan: &Prepared, notes: usize, voices: usize) -> Self {
        let behaviors = Self::script_capacity(plan);
        let voices = voices
            + plan.stage_count() * plan.release_voices() * Self::SCRIPT_KEYS.min(notes);
        Self {
            notes,
            channels: 16,
            performances: 1,
            families: 256,
            decisions: 256,
            expressions: notes,
            voices,
            commands: 256,
            behaviors,
            behavior_fuel: 1 << 20,
            behavior_cells: plan.behavior_local_count().saturating_mul(behaviors),
            note_cells: plan.note_cell_count().saturating_mul(notes),
        }
    }
}

#[derive(Clone, Copy)]
enum NoteOrigin {
    Input(Input, Expression, usize),
    Child(NoteId, bool, Inheritance),
    Generated(PlanId, ChannelAddress, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReleaseLink {
    None,
    Gate,
    Stage(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AttackStatus {
    Pending,
    Forwarded,
    Suppressed,
}

#[derive(Clone, Copy, Debug)]
struct Note {
    input: Option<Input>,
    input_down: bool,
    address: ChannelAddress,
    plan: PlanId,
    parent: Option<NoteId>,
    release_link: ReleaseLink,
    retire_when_silent: bool,
    attack: AttackStatus,
    siblings: Siblings,
    first_child: Option<Index>,
    first_family: Option<Index>,
    first_decision: Option<Index>,
    pitch: NotePitch,
    velocity: f64,
    key_release: Option<ReleaseCause>,
    gate_release: Option<ReleaseCause>,
    sostenuto: bool,
    work: usize,
    pins: usize,
    order: u64,
    expression: ExpressionId,
    families: usize,
    children: usize,
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    family: FamilyId,
    siblings: Siblings,
    sample: usize,
    cursor: source::Cursor,
    base_step: f64,
    chain: Option<usize>,
    bus: Option<usize>,
    tail_remaining: Option<u32>,
    dsp_fade: Option<(u32, f32)>,
    envelope: EnvelopeState,
    gain: f32,
    started: bool,
    /// Admission order, for oldest-first stealing.
    born: u64,
    /// Fading out after being stolen; no longer counts against polyphony.
    stolen: bool,
    /// The region's group, for script group layers.
    group: Option<u32>,
    /// Frames a releasing voice's gain bound has stayed under `render::INAUDIBLE`.
    quiet: u32,
}

#[derive(Clone, Copy, Debug)]
struct Slot<T> {
    generation: u64,
    value: Option<T>,
}

struct Arena<T> {
    runtime: u64,
    slots: Box<[Slot<T>]>,
    occupied: usize,
    available: usize,
    reserved: usize,
    free: Box<[u64]>,
}

impl<T> Arena<T> {
    fn at_mut(&mut self, index: Index) -> &mut T {
        self.slots[index.get()].value.as_mut().unwrap()
    }

    fn new(runtime: u64, capacity: usize) -> Self {
        let mut free = vec![u64::MAX; capacity.div_ceil(64)].into_boxed_slice();
        if !capacity.is_multiple_of(64) {
            *free.last_mut().unwrap() = (1u64 << (capacity % 64)) - 1;
        }
        Self {
            runtime,
            occupied: 0,
            available: capacity,
            reserved: 0,
            free,
            slots: std::iter::repeat_with(|| Slot {
                generation: 0,
                value: None,
            })
            .take(capacity)
            .collect(),
        }
    }

    /// Empty slots and an all-free bitmap for `capacity` entries.
    fn blank(capacity: usize) -> (Box<[Slot<T>]>, Box<[u64]>) {
        let mut free = vec![u64::MAX; capacity.div_ceil(64)].into_boxed_slice();
        if !capacity.is_multiple_of(64) {
            *free.last_mut().unwrap() = (1u64 << (capacity % 64)) - 1;
        }
        let slots = std::iter::repeat_with(|| Slot { generation: 0, value: None })
            .take(capacity)
            .collect();
        (slots, free)
    }

    /// Swap in larger storage from `blank`, moving every slot over at its
    /// index so handles stay valid. The old storage ends up in the
    /// arguments, to be freed off the audio thread. No allocation.
    fn grow(&mut self, slots: &mut Box<[Slot<T>]>, free: &mut Box<[u64]>) {
        let old = self.slots.len();
        assert!(slots.len() > old);
        slots[..old].swap_with_slice(&mut self.slots);
        let words = old.div_ceil(64);
        let kept = if old % 64 == 0 { u64::MAX } else { (1u64 << (old % 64)) - 1 };
        for (w, &bits) in self.free.iter().enumerate() {
            let mask = if w + 1 == words { kept } else { u64::MAX };
            free[w] = (bits & mask) | (free[w] & !mask);
        }
        self.available += slots.len() - old;
        std::mem::swap(&mut self.slots, slots);
        std::mem::swap(&mut self.free, free);
    }

    fn insert(&mut self, value: T) -> Result<Handle, Error> {
        if self.available() == 0 {
            return Err(Error::Capacity);
        }
        // Scan words rather than every slot, retaining lowest-slot allocation and
        // therefore deterministic voice summation after arbitrary holes/reuse.
        let (word, bits) = self
            .free
            .iter_mut()
            .enumerate()
            .find(|(_, bits)| **bits != 0)
            .ok_or(Error::Capacity)?;
        let index = word * 64 + bits.trailing_zeros() as usize;
        *bits &= *bits - 1;
        let slot = &mut self.slots[index];
        debug_assert!(slot.value.is_none() && slot.generation < u64::MAX);
        slot.generation += 1; // Exhausted generations are quarantined, never wrapped.
        slot.value = Some(value);
        self.occupied += 1;
        self.available -= 1;
        Ok(Handle {
            runtime: self.runtime,
            index,
            generation: slot.generation,
        })
    }

    fn get(&self, id: Handle) -> Option<&T> {
        self.slots
            .get(id.index)
            .filter(|s| id.runtime == self.runtime && s.generation == id.generation)?
            .value
            .as_ref()
    }

    fn get_mut(&mut self, id: Handle) -> Option<&mut T> {
        self.slots
            .get_mut(id.index)
            .filter(|s| id.runtime == self.runtime && s.generation == id.generation)?
            .value
            .as_mut()
    }

    fn remove(&mut self, id: Handle) {
        drop(self.take(id));
    }

    fn take(&mut self, id: Handle) -> Option<T> {
        self.get(id)?;
        let slot = &mut self.slots[id.index];
        let value = slot.value.take();
        self.occupied -= 1;
        if slot.generation < u64::MAX {
            self.available += 1;
            self.free[id.index / 64] |= 1 << (id.index % 64);
        }
        value
    }

    /// Roll back a failed ownership transfer into the exact slot, without
    /// allocating another generation or making the old handle stale.
    fn restore(&mut self, id: Handle, value: T) {
        let slot = &mut self.slots[id.index];
        assert!(
            id.runtime == self.runtime && id.generation == slot.generation && slot.value.is_none()
        );
        slot.value = Some(value);
        self.occupied += 1;
        self.available -= usize::from(slot.generation < u64::MAX);
        self.free[id.index / 64] &= !(1 << (id.index % 64));
    }

    fn id(&self, index: usize) -> Handle {
        Handle {
            runtime: self.runtime,
            index,
            generation: self.slots[index].generation,
        }
    }

    fn available(&self) -> usize {
        self.available - self.reserved
    }

    fn reserve(&mut self, count: usize) {
        assert!(count <= self.available());
        self.reserved += count;
    }

    fn unreserve(&mut self, count: usize) {
        assert!(count <= self.reserved);
        self.reserved -= count;
    }

    fn count(&self) -> usize {
        self.occupied
    }
}

/// All capacities are supplied at preparation. Terminal delivery uses the note's
/// existing slot, so a full command queue cannot discard its cleanup or notification.
pub struct Runtime {
    rate: u32,
    /// Quarter notes per minute for tempo-synced modulation.
    tempo: f64,
    plans: Arena<Generation>,
    active_plan: PlanId,
    plan_queues: Option<PlanQueues>,
    control_queues: Option<control::ControlQueues>,
    notes: Arena<Note>,
    release_times: Box<[release::ReleaseTimes]>,
    note_events: Box<[note_event::NoteEvent]>,
    source_ids: Vec<(i32, NoteId)>,
    last_source_id: i32,
    closed_notes: Vec<NoteId>,
    channels: Arena<Channel>,
    voices: Arena<Voice>,
    voice_activity: Box<[u64]>,
    stealing: Option<steal::Stealing>,
    stolen: usize,
    /// Voices stolen since the runtime started.
    steals: u64,
    voice_order: u64,
    /// Worker pool and scratch for multicore rendering; None renders on the audio thread.
    parallel: Option<parallel::Parallel>,
    /// Render lanes new plans size their filter caches for.
    lanes: Arc<std::sync::atomic::AtomicUsize>,
    kernel: resample::Kernel,
    stream_cache: Option<StreamCache>,
    stream_underruns: u64,
    voice_drops: u64,
    /// Voice-pool growths adopted, and refused (see `grow`).
    voice_growths: u64,
    growth_failures: u64,
    growth: Option<grow::GrowthQueues>,
    /// Set on the audio side when the pool runs three quarters full.
    voice_pressure: grow::Pressure,
    steal_releases: bool,
    cold_starts: bool,
    cold_started: u64,
    /// Last and peak `render` nanoseconds, and the last call's frames.
    render_time: [u64; 3],
    families: Arena<Family>,
    decisions: Arena<variation::Decision>,
    expressions: Arena<ExpressionOwner>,
    expression_changes: Box<[Option<RenderedExpression>]>,
    commands: Vec<Scheduled>,
    behaviors: Arena<Continuation>,
    behavior_ready: Vec<behavior::Ready>,
    /// Callbacks preempted on fuel, and callbacks queued behind them to keep
    /// their script's event order; resumed at the next block, oldest first.
    yielded: std::collections::VecDeque<BehaviorId>,
    preemptions: u64,
    longest_preempted: u64,
    dispatching_behavior: bool,
    /// The plan whose plan programs have started.
    started_plan: Option<PlanId>,
    behavior_fuel: usize,
    behavior_stride: usize,
    behavior_locals: Box<[i64]>,
    note_stride: usize,
    note_values: Box<[i64]>,
    note_params: Box<[script_params::NoteParams]>,
    /// Set once a script writes a voice parameter; voices then render in chunks.
    // ponytail: sticky for the runtime's life; count live layers if chunking costs show up.
    script_params: bool,
    /// Keys whose latest physical event was a note-on: one note-off clears the
    /// key however many presses stacked, as `%KEY_DOWN` does in Kontakt.
    input_keys: u128,
    executing_due: bool,
    command_limit: usize,
    reserved_commands: usize,
    performance_state: performance::PerformanceState,
    selections: Box<[performance::NoteSelection]>,
    now: u64,
    order: u64,
    nonfinite_frames: u64,
    ops: ops::OpState,
}

impl Runtime {
    /// Select rate-conversion quality: Realtime (the default) for live playback,
    /// High for offline renders and reference comparisons. Control side; takes
    /// effect at the next rendered frame and keeps every voice's source phase.
    pub fn set_resample_quality(&mut self, quality: ResampleQuality) {
        self.kernel = resample::Kernel::new(quality);
    }
    pub fn with_resample_quality(mut self, quality: ResampleQuality) -> Self {
        self.set_resample_quality(quality);
        self
    }

    pub fn id(&self) -> RuntimeId {
        RuntimeId(self.notes.runtime)
    }

    pub fn new(plan: Prepared, limits: Limits) -> Result<Self, Error> {
        if limits.notes == 0 || limits.performances == 0 {
            return Err(Error::InvalidInput);
        }
        let note_stride = limits.note_cells / limits.notes;
        if plan.note_cells > note_stride {
            return Err(Error::Capacity);
        }
        let note_cells = note_stride * limits.notes;
        std::alloc::Layout::array::<i64>(note_cells).map_err(|_| Error::Capacity)?;
        let behavior_stride = limits
            .behavior_cells
            .checked_div(limits.behaviors)
            .unwrap_or(0);
        if plan.programs.iter().any(|p| p.locals > behavior_stride) {
            return Err(Error::Capacity);
        }
        let cells = behavior_stride
            .checked_mul(limits.behaviors)
            .ok_or(Error::Capacity)?;
        if cells > limits.behavior_cells {
            return Err(Error::Capacity);
        }
        std::alloc::Layout::array::<i64>(cells).map_err(|_| Error::Capacity)?;
        // One frame per callback plus one queued native release per logical note.
        let ready_capacity = limits
            .behaviors
            .checked_add(limits.notes)
            .ok_or(Error::Capacity)?;
        std::alloc::Layout::array::<behavior::Ready>(ready_capacity)
            .map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<release::ReleaseTimes>(limits.notes)
            .map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<(i32, NoteId)>(limits.notes).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<note_event::NoteEvent>(limits.notes)
            .map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<performance::NoteSelection>(limits.notes)
            .map_err(|_| Error::Capacity)?;
        let state_capacity =
            performance::PerformanceState::validate(limits.notes, limits.performances)?;
        static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(1);
        #[allow(deprecated, reason = "fetch_update supports the Rust 1.92 minimum")]
        let id = NEXT_RUNTIME
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::Capacity)?;
        let rate = plan.rate;
        let mut plans = Arena::new(id, 1);
        let active_plan = PlanId(plans.insert(Generation {
            request: 0,
            sequences: variation::SequenceState::new(&plan),
            controls: control::ControlState::new(&plan),
            scripts: plan.script_initial.clone(),
            dsp: dsp::DspState::new(&plan, limits.voices, limits.expressions, 1)?,
            groups: groups::GroupState::new(plan.group_count, limits.notes, plan.stages.len())?,
            controllers: controller_event::ControllerState::new(&plan, limits.performances)?,
            projections: note_event::NoteProjections::new(plan.stages.len(), limits.notes)?,
            modulation: voice_mod::VoiceModState::new(&plan.voice_modulation, limits.voices)?,
            script: script_params::EngineLayers::new(&plan),
            prepared: Box::new(plan),
            notes: 0,
            callbacks: 0,
        })?);
        Ok(Self {
            rate,
            tempo: 120.0,
            plans,
            active_plan,
            plan_queues: None,
            control_queues: None,
            notes: Arena::new(id, limits.notes),
            closed_notes: Vec::with_capacity(limits.notes),
            source_ids: Vec::with_capacity(limits.notes),
            last_source_id: 0,
            channels: Arena::new(id, limits.channels),
            voices: Arena::new(id, limits.voices),
            voice_activity: vec![0; limits.voices.div_ceil(64)].into_boxed_slice(),
            stealing: None,
            stolen: 0,
            steals: 0,
            voice_order: 0,
            parallel: None,
            lanes: Arc::new(std::sync::atomic::AtomicUsize::new(1)),
            kernel: resample::Kernel::new(ResampleQuality::default()),
            stream_cache: None,
            stream_underruns: 0,
            voice_drops: 0,
            voice_growths: 0,
            growth_failures: 0,
            growth: None,
            voice_pressure: Arc::default(),
            steal_releases: false,
            cold_starts: false,
            cold_started: 0,
            render_time: [0; 3],
            families: Arena::new(id, limits.families),
            decisions: Arena::new(id, limits.decisions),
            expressions: Arena::new(id, limits.expressions),
            expression_changes: vec![None; limits.expressions].into_boxed_slice(),
            commands: Vec::with_capacity(limits.commands),
            behaviors: Arena::new(id, limits.behaviors),
            behavior_ready: Vec::with_capacity(ready_capacity),
            yielded: std::collections::VecDeque::with_capacity(limits.behaviors),
            preemptions: 0,
            longest_preempted: 0,
            dispatching_behavior: false,
            started_plan: None,
            behavior_fuel: limits.behavior_fuel,
            behavior_stride,
            behavior_locals: vec![0; cells].into_boxed_slice(),
            executing_due: false,
            command_limit: limits.commands,
            reserved_commands: 0,
            now: 0,
            order: 0,
            nonfinite_frames: 0,
            ops: ops::OpState::default(),
            // Keep cold payload allocation after the frequently traversed pools.
            note_stride,
            note_values: vec![0; note_cells].into_boxed_slice(),
            note_params: vec![script_params::NoteParams::default(); limits.notes]
                .into_boxed_slice(),
            script_params: false,
            input_keys: 0,
            release_times: vec![release::ReleaseTimes::default(); limits.notes].into_boxed_slice(),
            note_events: vec![note_event::NoteEvent::new(NotePitch::Key(0), 0.); limits.notes]
                .into_boxed_slice(),
            selections: vec![performance::NoteSelection::default(); limits.notes]
                .into_boxed_slice(),
            performance_state: performance::PerformanceState::new(
                state_capacity,
                limits.performances,
            ),
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }
    /// Times a callback ran out of per-block fuel and continued next block.
    pub fn preemptions(&self) -> u64 {
        self.preemptions
    }
    /// Longest time, in frames, a preempted callback has spanned so far.
    pub fn longest_preempted_frames(&self) -> u64 {
        self.longest_preempted
    }
    pub fn now(&self) -> u64 {
        self.now
    }
    pub fn note_count(&self) -> usize {
        self.notes.count()
    }
    pub fn voice_count(&self) -> usize {
        self.voices.count()
    }
    pub fn pending_commands(&self) -> usize {
        self.commands.len()
    }

    /// Frames silenced because finite source values overflowed during summation.
    /// Buses of the active plan.
    pub fn bus_count(&self) -> usize {
        self.plans
            .get(self.active_plan.0)
            .map_or(0, |g| g.prepared.buses.len())
    }

    /// Each bus of the active plan's peak level since the last call, after
    /// its mix gain; taking them resets them.
    pub fn take_bus_peaks(&mut self, mut each: impl FnMut(usize, [f32; 2])) {
        if let Some(g) = self.plans.get_mut(self.active_plan.0) {
            for (bus, peak) in g.dsp.buses.peaks.iter_mut().enumerate() {
                each(bus, std::mem::take(peak));
            }
        }
    }

    /// Mix `bus` of the active plan from the next rendered frame on. Older
    /// generations still sounding keep their own mix.
    pub fn set_bus_mix(&mut self, bus: usize, mix: BusMix) -> Result<(), Error> {
        if !mix.gain.iter().all(|g| g.is_finite()) {
            return Err(Error::InvalidInput);
        }
        let generation = self
            .plans
            .get_mut(self.active_plan.0)
            .ok_or(Error::StaleHandle)?;
        *generation
            .dsp
            .buses
            .mix
            .get_mut(bus)
            .ok_or(Error::InvalidInput)? = mix;
        Ok(())
    }

    pub fn nonfinite_frames(&self) -> u64 {
        self.nonfinite_frames
    }

    /// Admit ownership before preparing any source or scheduled work. A caller
    /// receiving Err has not admitted an input; protocol rejection remains its job.
    pub fn note_on(&mut self, input: Input, key: u8, velocity: f64) -> Result<NoteId, Error> {
        self.note_on_with_expression(input, key, velocity, Expression::default())
    }

    /// Initial expression is part of ownership admission, before any callback or
    /// source can snapshot it. Unsupported PCM rates are checked at source admission.
    pub fn note_on_with_expression(
        &mut self,
        input: Input,
        key: u8,
        velocity: f64,
        expression: Expression,
    ) -> Result<NoteId, Error> {
        self.note_on_pitched(input, NotePitch::Key(key), velocity, expression)
    }

    /// Admit inherent pitch independently of the physical address and expression.
    pub fn note_on_pitched(
        &mut self,
        input: Input,
        pitch: NotePitch,
        velocity: f64,
        expression: Expression,
    ) -> Result<NoteId, Error> {
        self.note_on_pitched_in(
            self.performance(0).unwrap(),
            input,
            pitch,
            velocity,
            expression,
        )
    }

    /// Admit a logical input in an explicit musical routing domain, without selecting sources.
    pub fn note_on_pitched_in(
        &mut self,
        performance: PerformanceId,
        input: Input,
        pitch: NotePitch,
        velocity: f64,
        expression: Expression,
    ) -> Result<NoteId, Error> {
        let performance = self.performance_index(performance)?;
        self.apply_due();
        if input.channel >= 16 || input.group >= 16 || input.key >= 128 || !expression.valid() {
            return Err(Error::InvalidInput);
        }
        if input.external_id.is_some()
            && self.notes.slots.iter().enumerate().any(|(i, s)| {
                s.value.is_some_and(|n| n.input == Some(input))
                    && self.selections[i].performance == performance
            })
        {
            return Err(Error::DuplicateInput);
        }
        self.admit(
            NoteOrigin::Input(input, expression, performance),
            pitch,
            velocity,
        )
    }

    /// Detached children do not release with their parent, but still retain its
    /// provenance until they finish. Linked children cannot attach to a closed gate.
    pub fn child(
        &mut self,
        parent: NoteId,
        key: u8,
        velocity: f64,
        linked_release: bool,
        inheritance: Inheritance,
    ) -> Result<NoteId, Error> {
        self.child_pitched(
            parent,
            NotePitch::Key(key),
            velocity,
            linked_release,
            inheritance,
        )
    }

    pub fn child_pitched(
        &mut self,
        parent: NoteId,
        pitch: NotePitch,
        velocity: f64,
        linked_release: bool,
        inheritance: Inheritance,
    ) -> Result<NoteId, Error> {
        self.apply_due();
        let p = self.notes.get(parent.0).ok_or(Error::StaleHandle)?;
        if linked_release && !p.gate() {
            return Err(Error::ClosedNote);
        }
        self.admit(
            NoteOrigin::Child(parent, linked_release, inheritance),
            pitch,
            velocity,
        )
    }

    fn admit(
        &mut self,
        origin: NoteOrigin,
        pitch: NotePitch,
        velocity: f64,
    ) -> Result<NoteId, Error> {
        let performance = match origin {
            NoteOrigin::Input(_, _, performance) | NoteOrigin::Generated(_, _, performance) => {
                performance
            }
            NoteOrigin::Child(parent, ..) => {
                self.notes.get(parent.0).ok_or(Error::StaleHandle)?;
                self.selections[parent.0.index].performance
            }
        };
        let (input, parent, linked_release, inheritance, initial) = match origin {
            NoteOrigin::Input(input, expression, _) => (
                Some(input),
                None,
                false,
                Inheritance::Independent,
                expression,
            ),
            NoteOrigin::Generated(..) => (
                None,
                None,
                false,
                Inheritance::Independent,
                Expression::default(),
            ),
            NoteOrigin::Child(parent, linked, inheritance) => (
                None,
                Some(parent),
                linked,
                inheritance,
                Expression::default(),
            ),
        };
        if !pitch.valid() || !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) {
            return Err(Error::InvalidInput);
        }
        let order = self.order.checked_add(1).ok_or(Error::ClockOverflow)?;
        let parent_note = parent
            .map(|p| self.notes.get(p.0).ok_or(Error::StaleHandle))
            .transpose()?;
        let address = match origin {
            NoteOrigin::Input(input, ..) => input.channel_address(),
            NoteOrigin::Child(..) => parent_note.unwrap().address,
            NoteOrigin::Generated(_, address, _) => address,
        };
        let parent_expression = parent_note.map(|n| n.expression);
        let next_sibling = parent_note.and_then(|n| n.first_child);
        let plan = match origin {
            NoteOrigin::Generated(plan, ..) => plan,
            _ => parent_note.map_or(self.active_plan, |n| n.plan),
        };
        let expression = match (inheritance, parent_expression) {
            (Inheritance::Linked, Some(id)) => {
                let owner = self.expressions.get_mut(id.0).unwrap();
                owner.notes = owner.notes.checked_add(1).ok_or(Error::Capacity)?;
                id
            }
            (policy, parent) => {
                let program = self.modulation_plan(plan);
                let (value, rendered) =
                    if let (Inheritance::Snapshot, Some(parent)) = (policy, parent) {
                        let owner = self.expressions.get(parent.0).unwrap();
                        (owner.value, owner.rendered)
                    } else {
                        (initial, self.project_expression(program, initial, None)?)
                    };
                ExpressionId(self.expressions.insert(ExpressionOwner {
                    value,
                    rendered,
                    program,
                    notes: 1,
                })?)
            }
        };
        if let Some(input) = input {
            self.input_keys |= 1 << (input.key & 127);
        }
        let id = match self.notes.insert(Note {
            input,
            input_down: input.is_some(),
            address,
            plan,
            parent,
            release_link: if linked_release {
                ReleaseLink::Gate
            } else {
                ReleaseLink::None
            },
            retire_when_silent: false,
            attack: AttackStatus::Pending,
            siblings: Siblings {
                previous: None,
                next: next_sibling,
            },
            first_child: None,
            first_family: None,
            first_decision: None,
            pitch,
            velocity,
            key_release: None,
            gate_release: None,
            sostenuto: false,
            work: 0,
            pins: 0,
            order,
            expression,
            families: 0,
            children: 0,
        }) {
            Ok(id) => id,
            Err(error) => {
                self.drop_expression(expression);
                return Err(error);
            }
        };
        if self.note_stride != 0 {
            let begin = id.index * self.note_stride;
            let cells = self.plans.get(plan.0).unwrap().prepared.note_cells;
            self.note_values[begin..begin + cells].fill(0);
        }
        self.selections[id.index] = performance::NoteSelection {
            performance,
            snapshot: self.performance_state.capture(performance),
            consumed_switch: false,
        };
        self.release_times[id.index] = release::ReleaseTimes {
            admitted_at: self.now,
            ..release::ReleaseTimes::default()
        };
        self.note_events[id.index] = note_event::NoteEvent::new(pitch, velocity);
        self.note_params[id.index] = script_params::NoteParams::default();
        self.plans.get_mut(plan.0).unwrap().projections.admit(
            id.index,
            0,
            NoteProperties { pitch, velocity },
        );
        self.plans
            .get_mut(plan.0)
            .unwrap()
            .groups
            .admit(id.index, parent.map(|p| p.0.index));
        if let Some(parent) = parent {
            let index = Index::new(id.index);
            if let Some(next) = next_sibling {
                self.notes.at_mut(next).siblings.previous = Some(index);
            }
            // Successful admission bounds this count by the allocated note slots.
            let parent = self.notes.get_mut(parent.0).unwrap();
            parent.children += 1;
            parent.first_child = Some(index);
        }
        self.plans.get_mut(plan.0).unwrap().notes += 1;
        self.order = order;
        Ok(NoteId(id))
    }

    pub fn note(&self, id: NoteId) -> Result<(u8, f64, bool), Error> {
        let n = self.notes.get(id.0).ok_or(Error::StaleHandle)?;
        Ok((n.pitch.key(), n.velocity, n.gate()))
    }

    pub fn note_pitch(&self, id: NoteId) -> Result<NotePitch, Error> {
        Ok(self.notes.get(id.0).ok_or(Error::StaleHandle)?.pitch)
    }

    /// A continuation pins logical ownership even after source completion/release.
    pub fn pin(&mut self, id: NoteId) -> Result<(), Error> {
        let n = self.notes.get_mut(id.0).ok_or(Error::StaleHandle)?;
        n.pins = n.pins.checked_add(1).ok_or(Error::Capacity)?;
        Ok(())
    }

    pub fn unpin(&mut self, id: NoteId) -> Result<(), Error> {
        let n = self.notes.get_mut(id.0).ok_or(Error::StaleHandle)?;
        n.pins = n.pins.checked_sub(1).ok_or(Error::InvalidInput)?;
        Ok(())
    }

    /// Native anonymous-input fallback: FIFO within the exact original input address.
    /// Release velocity is normalized; None means it was not supplied.
    pub fn note_off(&mut self, input: Input, velocity: Option<f64>) -> Result<NoteId, Error> {
        self.note_off_in(self.performance(0).unwrap(), input, velocity)
    }

    /// FIFO pairing within the exact physical input identity and musical domain.
    pub fn note_off_in(
        &mut self,
        performance: PerformanceId,
        input: Input,
        velocity: Option<f64>,
    ) -> Result<NoteId, Error> {
        let performance = self.performance_index(performance)?;
        release::validate_velocity(velocity)?;
        self.apply_due();
        let id = self
            .notes
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.value
                    .as_ref()
                    .filter(|n| {
                        n.input_down
                            && n.input == Some(input)
                            && self.selections[i].performance == performance
                    })
                    .map(|n| (i, n.order))
            })
            .min_by_key(|(_, order)| *order)
            .map(|(i, _)| NoteId(self.notes.id(i)))
            .ok_or(Error::StaleHandle)?;
        self.input_keys &= !(1 << (input.key & 127));
        self.key_up_now(id, velocity)?;
        Ok(id)
    }

    /// Convenience for a single-source selection: creates and seals one family.
    /// Multi-source selections explicitly create a family and use start_family.
    pub fn start(
        &mut self,
        note: NoteId,
        sample: usize,
        at: u64,
        gain: f32,
    ) -> Result<VoiceId, Error> {
        let family = self.create_family(note)?;
        let result = self.start_family(
            family,
            sample,
            at,
            gain,
            Envelope::default(),
            Playback::default(),
        );
        self.finish_family(family)?;
        result
    }

    /// Reserve a voice and its delayed start atomically. A failed admission leaves
    /// the family unchanged. Sealed families cannot admit additional sources.
    pub fn start_family(
        &mut self,
        family: FamilyId,
        sample: usize,
        at: u64,
        gain: f32,
        envelope: Envelope,
        playback: Playback,
    ) -> Result<VoiceId, Error> {
        self.check_time(at)?;
        if at == self.now {
            self.apply_due();
        }
        let f = self.families.get(family.0).ok_or(Error::StaleHandle)?;
        if !f.open {
            return Err(Error::ClosedFamily);
        }
        let plan = &self
            .plans
            .get(self.notes.get(f.note.0).unwrap().plan.0)
            .unwrap()
            .prepared;
        if sample >= plan.pcm.len() || !gain.is_finite() || !(0.0..=1.0).contains(&gain) {
            return Err(Error::InvalidInput);
        }
        let cursor = playback.cursor(
            plan.pcm[sample].frame_count(),
            plan.pcm[sample].sample_rate(),
            self.rate,
        )?;
        self.admit_voice(family, sample, at, gain, envelope, cursor)
    }

    // Inputs and cursor are validated either by Prepared or start_family. Both
    // paths drain due work before reaching this sole voice admission boundary.
    fn admit_voice(
        &mut self,
        family: FamilyId,
        sample: usize,
        at: u64,
        gain: f32,
        envelope: Envelope,
        cursor: source::Cursor,
    ) -> Result<VoiceId, Error> {
        let f = self.families.get(family.0).ok_or(Error::StaleHandle)?;
        if !f.open {
            return Err(Error::ClosedFamily);
        }
        let owner = self.notes.get(f.note.0).unwrap().expression;
        let base_step = cursor.step();
        let step = self.pitch_range(owner, true)?.apply(base_step)?;
        let cursor = cursor.with_step(step);
        let note = self.notes.get(f.note.0).unwrap();
        let asset = &self.plans.get(note.plan.0).unwrap().prepared.pcm[sample];
        let cold = self.check_source_ready(asset, cursor, envelope)?;
        f.voices.checked_add(1).ok_or(Error::Capacity)?;
        self.steal_voices(1);
        if at > self.now && self.available_commands() == 0 {
            return Err(Error::Capacity);
        }
        self.note_voice_pressure();
        if self.voices.available() == 0 {
            // ponytail: only silent voices waiting on the stream are taken; no
            // audible-voice stealing policy yet, so a full pool drops the start.
            let waiting = (0..self.voices.slots.len()).find(|&i| {
                self.voices.slots[i]
                    .value
                    .as_ref()
                    .is_some_and(|v| v.started && v.cursor.waiting())
            });
            match waiting {
                Some(i) => self.end_voice(VoiceId(self.voices.id(i))),
                None => {
                    self.voice_drops = self.voice_drops.saturating_add(1);
                    return Err(Error::Capacity);
                }
            }
        }
        let f = self.families.get(family.0).unwrap();
        let count = f.voices + 1;
        let next_sibling = f.first_voice;
        let id = VoiceId(self.voices.insert(Voice {
            family,
            siblings: Siblings {
                previous: None,
                next: next_sibling,
            },
            sample,
            cursor: if cold { cursor.cold() } else { cursor },
            base_step,
            chain: None,
            bus: None,
            tail_remaining: None,
            dsp_fade: None,
            envelope: EnvelopeState::new(envelope),
            gain,
            started: at == self.now,
            born: self.voice_order,
            stolen: false,
            group: None,
            quiet: 0,
        })?);
        self.cold_started += u64::from(cold);
        self.voice_order += 1;
        self.voice_activity[id.0.index / 64] |= 1 << (id.0.index % 64);
        let index = Index::new(id.0.index);
        if let Some(next) = next_sibling {
            self.voices.at_mut(next).siblings.previous = Some(index);
        }
        let family_state = self.families.get_mut(family.0).unwrap();
        family_state.voices = count;
        family_state.first_voice = Some(index);
        if at > self.now {
            self.queue(at, Action::Start(id));
        }
        Ok(id)
    }

    pub fn stop_voice(&mut self, id: VoiceId) -> Result<(), Error> {
        self.apply_due();
        self.voices.get(id.0).ok_or(Error::StaleHandle)?;
        self.commands
            .retain(|c| !matches!(c.action, Action::Start(v) if v == id));
        self.end_voice(id);
        Ok(())
    }

    pub fn voice_active(&self, id: VoiceId) -> bool {
        self.voices.get(id.0).is_some()
    }

    pub fn release(&mut self, id: NoteId) -> Result<(), Error> {
        self.apply_due();
        self.release_now(id, ReleaseCause::Explicit)
    }

    fn release_now(&mut self, id: NoteId, cause: ReleaseCause) -> Result<(), Error> {
        let n = self.notes.get_mut(id.0).ok_or(Error::StaleHandle)?;
        n.sostenuto = false;
        self.release_key(id, cause, None);
        self.close_gate(id, cause);
        self.cleanup_closed_notes();
        Ok(())
    }

    fn close_gate(&mut self, id: NoteId, cause: ReleaseCause) {
        let note = self.notes.get_mut(id.0).unwrap();
        let first = note.gate();
        if first {
            self.release_times[id.0.index].gate_at = self.now;
            self.release_times[id.0.index].held = false;
            note.gate_release = Some(cause);
        }
        let times = &mut self.release_times[id.0.index];
        let hard = !cause.musical() && !times.cleanup_hard;
        times.cleanup_hard |= !cause.musical();
        if first || hard {
            // First-transition records are immutable. A later fault still has to
            // visit linked descendants that a downstream module kept alive.
            let queued = &mut times.cleanup;
            if queued.is_none() {
                assert!(self.closed_notes.len() < self.closed_notes.capacity());
                self.closed_notes.push(id);
            }
            if queued.is_none() || !cause.musical() {
                *queued = Some(cause);
            }
        }
        if first {
            self.run_release_behavior(id, false);
            self.release_note_callbacks(id);
            // Forced closure must return any suppressed physical-release quota.
            self.run_release(id, Trigger::KeyRelease, false);
            self.run_release(id, Trigger::GateRelease, cause.musical());
        }
    }

    /// Resolve voices and future commands. Behavior outcomes remain retained until
    /// accepted; external owners must release manual pins. IDs stay valid for NOTE_END.
    pub fn panic(&mut self) {
        for i in 0..self.notes.slots.len() {
            if let Some(n) = &mut self.notes.slots[i].value {
                n.sostenuto = false;
                self.release_key(NoteId(self.notes.id(i)), ReleaseCause::Panic, None);
                self.close_gate(NoteId(self.notes.id(i)), ReleaseCause::Panic);
            }
        }
        for slot in &mut self.behaviors.slots {
            if let Some(c) = &mut slot.value
                && c.outcome.is_none()
            {
                c.outcome = Some(Outcome::Cancelled);
            }
        }
        // Panic hard-stops tails as well as held voices.
        for i in 0..self.voices.slots.len() {
            if self.voices.slots[i].value.is_some() {
                self.end_voice(VoiceId(self.voices.id(i)));
            }
        }
        for generation in self.plans.slots.iter_mut().filter_map(|s| s.value.as_mut()) {
            generation.dsp.buses.reset();
        }
        self.cleanup_closed_notes();
        for command in &self.commands {
            if let Action::Event(Event::Control(plan, _)) = command.action {
                self.plans.get_mut(plan.0).unwrap().controls.pending -= 1;
            }
        }
        self.commands.clear();
        for s in &mut self.channels.slots {
            if let Some(c) = &mut s.value {
                c.sustain = false;
                c.sostenuto = false;
            }
        }
    }

    fn cleanup_closed_notes(&mut self) {
        while let Some(note) = self.closed_notes.pop() {
            let n = self.notes.get(note.0).unwrap();
            let cause = self.release_times[note.0.index].cleanup.take().unwrap();
            let child_cause = if cause.musical() {
                ReleaseCause::Parent
            } else {
                cause
            };
            let mut child = n.first_child;
            while let Some(index) = child {
                let state = self.notes.at_mut(index);
                child = state.siblings.next;
                if state.release_link == ReleaseLink::Gate
                    || (!cause.musical() && state.release_link != ReleaseLink::None)
                {
                    state.sostenuto = false;
                    self.release_key(NoteId(self.notes.id(index.get())), child_cause, None);
                    self.close_gate(NoteId(self.notes.id(index.get())), child_cause);
                }
            }
            let mut family = self.notes.get(note.0).unwrap().first_family;
            while let Some(index) = family {
                let state = self.families.at_mut(index);
                family = state.siblings.next;
                if state.trigger == Trigger::Attack || !cause.musical() {
                    self.release_family_now(FamilyId(self.families.id(index.get())));
                }
            }
        }
        self.cancel_closed_work();
    }
}

#[cfg(test)]
mod tests;
