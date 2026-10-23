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

pub use prepare::{SourceEventFamily,SourceEventRouting,ExecutableSourceBinding,PreparedSourceOwners, PreparedSourceOwner, PreparedSourceOwnerIndex, PreparedSourceEventProcessor, PreparedSourceSwitch, MAX_ZONE_BATCH_JOBS, source_zone_job_capacity, SourceZoneHome, SourceZoneIdentity,
    ZoneRouteVariant, ZoneRouting, ZoneRoutingLimits, ZoneRoutingBuildError,
    ZoneRoutingFeature, ZoneRoutingRefusal, ZoneRoutingReport};
mod allocation;
pub use allocation::{AllocationCounts, AllocationDomainId};

mod automation;
pub use automation::{AutomationBinding, AutomationSource};
mod widget;
pub use widget::{
    WIDGET_DROP_CAPACITY, WIDGET_EDIT_CAPACITY, WidgetDefinition, WidgetDropKind,
    WidgetDropStorage, WidgetEdit, WidgetEventType, WidgetInteraction, WidgetStorage, WidgetValue,
};
mod control;
pub use control::{SourceConstantProperty,SourceConstantField,SourceConstantSnapshot};
pub use control::{
    NativeSnapshotOverlay, SnapshotOverlayEpoch, SnapshotOverlayFence, SnapshotOverlayRefusal,
    SnapshotMenuPosition, WidgetMenuStorage,
    SNAPSHOT_OVERLAY_VALUE_CAPACITY,
    BehaviorAdmissionFailure, BehaviorAdmissionOwner,
    BUS_VOLUME_SLOT, ControlCallback, ControlClient, ControlContext, ControlDefinition,
    ControlDomain, ControlId, ControlOperation, ControlQueueError, ControlReply, ControlRequest,
    ControlValue, ControlWrite, RejectedControls, ScriptStateAddress, ScriptStateBuffer,
    ScriptStateCallback, ScriptStateEntry, ScriptStateValue, SlotKind, is_slot_control,
    slot_control,
};
mod controller_event;
mod performance;
pub use performance::{
    AXIS_SWITCH, Keyswitch, PerformanceId, SelectionPolicy, SelectionSnapshot, previous_key_value,
};
mod switching;
pub use switching::{Driver, Selector, Switch, SwitchKeys, Switching};
mod behavior;
use behavior::Continuation;
pub use behavior::{
    BehaviorId, BehaviorOwner, BehaviorProgress, Comparison, Duration, DurationValue, FadeCurve,
    Instruction, Outcome, Program, Velocity, WaitLifetime,
};
mod behavior_fault;
pub use behavior_fault::{BehaviorFault, SourceSite, FAULT_CALLERS, FAULT_REGISTERS};
mod stages;
pub use stages::Stage;
mod diagnose;
pub use diagnose::{ScriptFault, SilentNote, why_silent};
mod stream;
pub use stream::{
    DecodeFailure, DecodeJob, PAGE_FRAMES, PageKey, PageStatus, PageUpdate, RejectedDecode,
    StreamCache, StreamDemandSnapshot, StreamError, StreamWorker,
};
mod wavetable;
pub use sampler_ir::Wavetable;
mod source;
pub use source::{Direction, Loop, LoopMode, LoopShape, LoopSlot, Playback, SampleDemand};
pub use source::storage::{
    ChannelCount, SourceResidentView, SourceStorage, SourceStorageFamily, SourceStorageId,
    SourceStorageRef,
};
pub use source::bundle::SourceBundleRefusal;
mod bus;
pub mod bus_amp;
pub use bus::{Bus, BusMix, BusPeakTap, BusSend, ChannelBus, GroupFader, SourceOwnerControl, SourceOwnerOutputControl, SourceOwnerControlTarget, ScopeControlSite};
pub use resample::{ResampleQuality, read_radius};
mod dsp;
pub use dsp::surround_panner;
pub use dsp::{
    Biquad, DynamicsSettings, ControlRange, ConvolutionUpload, DaftSettings, Decimator, Delay,
    ArSettings, CascadeSettings, ColorSettings, FilterSettings, LegacyFilter, SvFilter, CurveSettings, FilterKind, GainerSettings, HoldSettings, Impulse, LadderSettings,
    MAX_IMPULSE_FRAMES, NoiseSettings, OutputLowPass, QuantizerSettings,
    EqBand, Parameter, KontaktLegacyEqSettings, Processor, Rectifier,
    StateVariableFilter, neutral_impulse_shape, shape_impulse,
    StereoSettings, SvfMode, VoiceChain, VoiceSendPosition, VoiceSendTap,
};
mod whole_voice;
pub mod blocks;
pub mod v1_voice_controls;
use blocks::envelope::EnvelopeState;
pub use blocks::envelope::{Envelope, EnvelopeCurve};
mod engine_parameter_names;
mod engine_parameters;
mod gate;
mod modulation;
mod plan_programs;
mod script_params;
pub use engine_parameter_names::{
    ENGINE_PARAMETER_INTERNAL_IDS, ENGINE_PARAMETER_NAMES, ENGINE_PARAMETER_NATIVE_CONSTANTS,
};
pub use engine_parameters::{
    BUS_VOLUME_LAW, EngineLookup, EngineMeterAddress, EngineParameterAddress, EngineParameterBinding,
    EngineParameterLaw, EngineParameterOffset, EngineParameterOutcome, engine_parameter_control,
    engine_parameter_display, engine_parameter_id, engine_parameter_name,
};
mod parameter_registry;
mod steal;
mod topology;
pub use topology::{ProcessingOwnerId, ProcessingOwnerSnapshot,ProcessingModulationGate};
pub use parameter_registry::{
    ParameterAddress, ParameterDescriptor, ParameterDisplay, ParameterLaw, ParameterRegistry,
    ParameterRole, ParameterScope, ParameterUnit, PreparedParameterRegistry,
};
mod digital_fade;
pub use digital_fade::KontaktLfoFade;
mod voice_mod;
pub use voice_mod::laws as native_mod_laws;
pub use plan_programs::{ParameterKind, ParameterProgram, PlanProgram, SignalProgram};
pub use script_params::{EnvelopeStage, GroupParams, ParamScope};
pub use steal::{Kill, Stealing, VoiceLimit};
pub use voice_mod::{
    Breakpoint, Breakpoints, Lfo, LfoRate, LfoShape, ModNative, ModProgram, ModRoute, ModScale, ModScaleLaw, ModSource,
    ModProvider,ProcessingSourceRef,
    ModTarget, MORE_SCALES,
};
mod ownership;
mod receiver;
pub const RECEIVER_PER_NOTE_CENTER:u32=0x8000_0000;
pub use receiver::{PerNoteState as ReceiverPerNoteState,KeyState as ReceiverKeyState,Snapshot as ReceiverSnapshot};
pub use receiver::{EventValueId,EventPostId,EventValueSnapshot,EventPostOutcome,EventValueMemory,SourceTaskFamily,SourceTaskHostContext,SourceTaskReason,SourceTaskCreator,NonMusicalTaskOrigin,PreparedGlobalTaskOrigin,PreparedGlobalSourceTask,PreparedSourceTaskBinding,SourceTaskEvent,ProgramBankIntent,ProgramSelection,ProgramPlaybackCapability,PhysicalReleaseEvent,
ExternalInputId, ExternalTurn, ExternalEvent, ExternalOutput, ExternalOutputResult, ExternalTarget,
ExternalAbortReport,
ReceiverWireEpoch,RawAdmissionId,RawTerminalId,RawDecisionPhase,RawRejection,RawAdmission,RawAttackIntent,CapturedLogicalRawAttack,RawTerminalClaim,RawTerminalOutcome,WireAdmissionStatus,
ReceiverId,ReceiverMemory,ReceiverTerminalDelivery,OwnedReceiver,Zone as ReceiverZone,TerminalRelease as ReceiverTerminalRelease,Applied as ReceiverApplied,ApplyError as ReceiverApplyError,Attribute as ReceiverAttribute,ChannelVoice as ReceiverChannelVoice,Controllers as ReceiverControllers,Message as ReceiverMessage,Value as ReceiverValue,Version as ReceiverVersion};
use modulation::RenderedExpression;
pub use modulation::{Destination, ExpressionSource, Modulation, Route};
mod pitch;
pub use pitch::NotePitch;
mod groups;
mod residency;
pub use residency::InitialGroupResidency;
pub use residency::{
    GroupResidencyProgress, ResidencyProgress, ResidencyRejection, ResidencyRejectionReason,
};
mod native_start;
mod note_event;
pub use note_event::NoteProperties;
mod packed;
mod plans;
mod prepare;
pub use packed::{Packed, Packing};
mod grow;
mod parallel;
mod release;
pub use parallel::Threads;
mod render;
mod onset;
pub use release::{
    GateRelease, KeyRelease, ReleaseCause, ReleaseContext, ReleaseOptions, ReleaseReserve,
    ReleaseStatus, ReleaseVelocity, Trigger,
};
pub use render::RuntimeStats;
mod resample;
use plans::{Generation, PlanQueues};
pub use plans::{PlanControl, PlanError, PlanId, PlanTransfer, RejectedPlan};
pub use prepare::{
    DORMANT_SAMPLE, MAX_SAMPLE_REVISIONS, PreparedSampleRevision, SampleControl,
    SampleRevisionLimits, SampleTarget, SampleTargetId, SampleTask, SampleTransaction,
    ResidencyRanges, ResidentFrames, ResidentBuffer, HeadBudget, HeadBudgetStatus, HeadPreparer, HeadRangeBuilder,
    AXIS_BASE, AssetId, ConstantStartPrefix, ControllerCondition, MAX_AXES, PREVIOUS_KEY, Pcm, Prepared, PreparedRanges, Ranges, Region,
    Tuning, VelocityCurve, ZoneFades, service_mipmaps,
};
mod integer;
pub mod lower;
pub use integer::{IntegerBinary, IntegerUnary};
mod array_file;
pub use array_file::{
    ARRAY_FILE_JOBS, ARRAY_FILE_SERVICE,
    ArrayFileArray, ArrayFileCompletion, ArrayFileKind,
};
mod midi_object;
mod ops;
pub mod waveform;
pub use midi_object::{
    MIDI_ALL_EVENTS, MIDI_ASYNC_SIGNAL, MIDI_CURRENT_EVENT, MIDI_MARKS_FLAG, MIDI_MAX_EVENTS,
    MIDI_SERVICE, MIDI_TRACK_FLAG, MidiAction, MidiCompletion, MidiExportArea, MidiObject,
    MidiObjectEvent, midi_par,
};
mod script;
pub use ops::{
    CALL_DEPTH, EFFECT_ARGS, EFFECT_CAPACITY, Effect, HOST_VALUES, IntegerExtra, Op, RANDOM_SEED, RealBinary,
    RealUnary, STORE_KEY, ScriptResources, TEXT_CAPACITY, Text, TextPart, TextRef, name_index,
    random, real, real_bits,
};
pub use script::{ScriptArray, ScriptInstanceId};
mod schedule;
mod variation;
use gate::Channel;
pub use gate::{ChannelAddress, ChannelId, ChannelScope};
pub use ownership::{Expression, ExpressionId, FamilyId, Inheritance, InputSelector, MAX_EXPRESSION_GAIN};
use ownership::{ExpressionOwner, Family};
pub use schedule::Event;
use schedule::{Action, Scheduled};
pub use variation::{Sequence, SequenceScope, Take, TakePolicy};

pub mod trace;
pub mod trace_report;

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

/// A built-in event parameter a script reads with `get_event_par`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventInfo {
    /// The event's key.
    Key,
    /// The event's velocity, 0..=127.
    Velocity,
    ReleaseVelocity,
    /// 1 for a live note event, 0 for an unknown or retired event.
    Status,
    /// Nonzero while the event has a sounding voice, 0 once it ended.
    ZoneId,
    /// The event's MIDI channel (0-based).
    MidiChannel,
    /// Physical creator script slot, or -1 for a host event.
    Source,
    /// Microseconds the play cursor of the event's first audible voice is
    /// past its zone start (`$EVENT_PAR_PLAY_POS`); 0 with no voice.
    PlayPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneParameter {
    Group,
    LowKey,
    HighKey,
}

/// First mod-value id of a note's four user event parameters (`$EVENT_PAR_0..3`).
pub const USER_EVENT_PAR: u16 = 1001;

/// Why a region mapped to a key did not sound for one selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    /// Belongs to another phase: a release-trigger region on an attack, or the reverse.
    Trigger,
    /// Needs a different articulation (keyswitch).
    Articulation,
    /// A controller condition (CC range) failed.
    Condition,
    /// Velocity outside the region's range.
    Velocity,
    /// The script disallowed the region's group.
    Group,
    /// Another round-robin take of the sequence was chosen.
    Take,
    /// An authored dormant player has no committed source yet.
    SourceUnavailable,
    /// Actual source/output rate and authored pitch exceed the supported step range.
    SourcePitch,
}

/// One mapped region's outcome in a selection; `rejected` is `None` if it sounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionVerdict {
    pub region: usize,
    pub group: Option<u32>,
    pub rejected: Option<Rejection>,
    /// Actual admitted source, after script offsets and start modulation.
    pub started: Option<SelectedSource>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectedSource {
    /// One-based original zone identity (the KSP zone ID).
    pub zone: u32,
    pub sample: usize,
    pub frame: u64,
    pub direction: Direction,
    pub loops: [Option<SelectedLoop>; 8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectedLoop {
    pub start: usize,
    pub end: usize,
    pub until_release: bool,
    pub alternating: bool,
    pub crossfade: usize,
    pub count: u32,
    pub tuning_bits: u64,
}

/// One selection's diagnostic: every region mapped to the key with its verdict.
/// A key with no candidates has an empty list (nothing is mapped there); a
/// script-suppressed attack has `suppressed` set and no candidates.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionRecord {
    pub event: u64,
    pub parent_event: Option<u64>,
    pub at: u64,
    pub key: u8,
    pub velocity: f64,
    pub trigger: Trigger,
    pub suppressed: bool,
    pub candidates: Vec<RegionVerdict>,
}

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

/// Input ownership address, never the transposed playback address. Adapters
/// own wildcard matching and protocol-specific interpretation of IDs. An
/// adapter mapping host tuples onto an internal wire retains the physical
/// tuple separately through [`Runtime::set_input_source`].
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
    /// Candidate preparation was canceled before publication.
    Canceled,
    /// Routed eligibility is prepared for MIDI-key pitch only.
    UnsupportedZoneRoutingPitch,
    UnsupportedZoneRoutingSampleUpdates,
    /// A source outlet or mutable source operation lacks the required channel layout.
    UnsupportedSourceLayout,
    /// A real receiver ownership reference has not drained.
    Busy,
}

impl Error {
    /// Every variant, so one can travel as its index.
    pub const ALL: [Error; 17] = [
        Self::Capacity,
        Self::InvalidInput,
        Self::NotReady,
        Self::StaleHandle,
        Self::DuplicateInput,
        Self::ClosedNote,
        Self::ClosedFamily,
        Self::PastEvent,
        Self::ClockOverflow,
        Self::ArithmeticOverflow,
        Self::RevisionConflict,
        Self::RandomBudget,
        Self::Canceled,
        Self::UnsupportedZoneRoutingPitch,
        Self::UnsupportedZoneRoutingSampleUpdates,
        Self::UnsupportedSourceLayout,
        Self::Busy,
    ];
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Busy => "the receiver still owns live references",
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
            Self::Canceled => "candidate preparation was canceled",
            Self::UnsupportedZoneRoutingPitch => "absolute pitch with zone routing is unsupported",
            Self::UnsupportedZoneRoutingSampleUpdates => "dynamic sample updates with zone routing are unsupported",
            Self::UnsupportedSourceLayout => "the source operation has no compatible channel-layout contract",
        })
    }
}

impl std::error::Error for Error {}

/// Additional simultaneously retained callbacks from the actual staged source
/// population. These are job counts, not a count inferred from owner metadata:
/// the caller includes every prepared root and Init/Transport context it can
/// retain alongside its existing ordinary callbacks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceCallbackPopulation {
    pub prepared_roots: usize,
    pub source_contexts: usize,
}

/// Explicit caller policy for callback array payload only. This does not change
/// any Runtime-wide limit or price PCM, DSP, queues, allocator metadata or the
/// Runtime header. Retained payload is callback storage still owned by old or
/// incoming runtimes during construction; it is not released at plan publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackStorageBudget {
    pub max_payload_bytes: usize,
    pub retained_payload_bytes: usize,
    pub max_construction_bytes: usize,
}

/// Checked callback-array payload, using Core's real private element layouts.
/// A Limits report uses requested capacities; a Runtime report uses the actual
/// reserved Vec/VecDeque capacities and boxed lengths. Neither includes allocator
/// bookkeeping. Read the Runtime report and recheck the same explicit budget
/// before publication; allocator-provided spare capacity is not assumed to be zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackStorage {
    pub callbacks: usize,
    pub local_cells: usize,
    pub ready_entries: usize,
    pub yielded_entries: usize,
    pub source_task_entries: usize,
    pub arena_bytes: usize,
    pub free_bitmap_bytes: usize,
    pub local_bytes: usize,
    pub ready_bytes: usize,
    pub yielded_bytes: usize,
    pub source_task_bytes: usize,
    payload_bytes: usize,
    boxing_overlap_bytes: usize,
}

impl CallbackStorage {
    fn for_capacities(callbacks: usize, free_words: usize, local_cells: usize,
        ready_entries: usize, yielded_entries: usize, source_task_entries: usize)
        -> Result<Self, Error>
    {
        fn array<T>(count: usize) -> Result<usize, Error> {
            std::alloc::Layout::array::<T>(count)
                .map(|layout| layout.size()).map_err(|_| Error::Capacity)
        }
        let arena_bytes = array::<Slot<Continuation>>(callbacks)?;
        let free_bitmap_bytes = array::<u64>(free_words)?;
        let local_bytes = array::<i64>(local_cells)?;
        let ready_bytes = array::<behavior::Ready>(ready_entries)?;
        let yielded_bytes = array::<BehaviorId>(yielded_entries)?;
        let source_task_bytes = array::<BehaviorId>(source_task_entries)?;
        let payload_bytes = [arena_bytes, free_bitmap_bytes, local_bytes, ready_bytes,
            yielded_bytes, source_task_bytes].into_iter().try_fold(0usize, |sum, bytes|
                sum.checked_add(bytes).ok_or(Error::Capacity))?;
        // Callback boxes are constructed sequentially. A shrinking Vec-to-Box
        // conversion may temporarily keep both old and new payloads; charge one
        // additional largest boxed callback array rather than assuming in-place
        // shrink at the reported capacities. Runtime's retained Vec/VecDeque
        // spare is separately visible in its report. A temporary Vec discarded
        // during Box construction is not observable through Runtime::new, so
        // this requested-capacity construction figure is not a strict bound on
        // its hidden temporary spare. Other Runtime allocations are outside it.
        let boxing_overlap_bytes = arena_bytes.max(free_bitmap_bytes).max(local_bytes);
        Ok(Self { callbacks, local_cells, ready_entries, yielded_entries,
            source_task_entries, arena_bytes, free_bitmap_bytes, local_bytes,
            ready_bytes, yielded_bytes, source_task_bytes, payload_bytes,
            boxing_overlap_bytes })
    }

    pub fn payload_bytes(self) -> usize { self.payload_bytes }

    /// Requested-capacity callback construction overlap: retained callback
    /// payload plus this storage and one additional boxed-array copy. Temporary
    /// Vec spare during the legacy constructor's Box conversion is not retained
    /// in Runtime and cannot be measured here; strict construction enforcement
    /// needs a constructor reservation/preflight join. This is neither an actual
    /// allocator-peak bound nor a process-memory/RSS claim.
    pub fn construction_bytes(self, retained_payload_bytes: usize) -> Result<usize, Error> {
        retained_payload_bytes.checked_add(self.payload_bytes)
            .and_then(|bytes| bytes.checked_add(self.boxing_overlap_bytes))
            .ok_or(Error::Capacity)
    }
}

impl CallbackStorageBudget {
    /// Pure payload admission check. Use before allocation on the Limits report
    /// and again before publication on the Runtime report. Construction checks
    /// have the explicit requested-capacity scope of construction_bytes().
    /// Runtime::new retains its policy and does not silently install this budget.
    pub fn check(self, storage: CallbackStorage) -> Result<(), Error> {
        let construction = storage.construction_bytes(self.retained_payload_bytes)?;
        if storage.payload_bytes() > self.max_payload_bytes
            || construction > self.max_construction_bytes
        { return Err(Error::Capacity); }
        Ok(())
    }
}

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
    /// Most held keys [`Self::for_plan`] sizes script callbacks for. Each callback
    /// slot costs about 2 KB per part, so the cap bounds script memory; past it
    /// the plan refuses notes with Capacity (about 128 live notes at any stage count).
    pub const SCRIPT_KEYS_MAX: usize = 64;
    /// Cap on script callback state per part: 4M cells, 32 MB.
    pub const SCRIPT_CELLS: usize = 1 << 22;

    /// Default callback arena/local/ready/yielded payload budget per part,
    /// including source-sized completion reservations. Note release queue
    /// entries are independently bounded by `notes`; the script cell ceiling
    /// also remains independent. These are KONTRA budgets, not native limits.
    pub const SCRIPT_CALLBACK_BYTES: usize = 64 << 20;

    /// Script callbacks a plan can have running at once: each note of
    /// [`Self::SCRIPT_KEYS`] keys runs a release callback in every stage and
    /// notes the scripts play pass the later stages too (about four per
    /// stage), plus one listener per stage; within [`Self::SCRIPT_CELLS`].
    /// Baseline16 for a plan without stages. A routing plan with an async
    /// callback adds its retained-job budget within the cell and byte ceilings.
    /// Smaller custom arenas or exhausted ceilings explicitly refuse new jobs.
    pub fn script_capacity(plan: &Prepared) -> usize {
        Self::script_capacity_for(plan, Self::SCRIPT_KEYS)
    }

    /// [`Self::script_capacity`] for `keys` notes held at once.
    pub fn script_capacity_for(plan: &Prepared, keys: usize) -> usize {
        let stages = plan.stage_count();
        let locals = plan.behavior_local_count().max(1);
        let ordinary = if stages == 0 { 16 } else {
            (4 * stages * keys + stages).clamp(16, 4096)
                .min(Self::SCRIPT_CELLS / locals).max(16)
        };
        let completions = plan.zone_completion_capacity();
        if completions == 0 {
            return ordinary;
        }
        // Every pending completion owns a real future arena slot, locals,
        // ready entry and numeric ID. Grow all three worker-side; never allocate
        // or force an authored boundary to make space on the audio thread.
        let per_callback = std::mem::size_of::<Slot<behavior::Continuation>>()
            .saturating_add(std::mem::size_of::<behavior::Ready>())
            .saturating_add(std::mem::size_of::<BehaviorId>())
            .saturating_add(std::mem::size_of::<u64>())
            .saturating_add(locals.saturating_mul(std::mem::size_of::<i64>()));
        let ceiling = (Self::SCRIPT_CELLS / locals)
            .min(Self::SCRIPT_CALLBACK_BYTES / per_callback);
        ordinary.saturating_add(completions.min(ceiling.saturating_sub(ordinary)))
    }

    /// Requested callback storage for these exact caller limits. Local cells
    /// use the same even per-callback stride as Runtime::new; any remainder in
    /// behavior_cells remains a caller ceiling, not a secretly allocated bank.
    pub fn callback_storage(self) -> Result<CallbackStorage, Error> {
        let stride = self.behavior_cells.checked_div(self.behaviors).unwrap_or(0);
        let cells = stride.checked_mul(self.behaviors).ok_or(Error::Capacity)?;
        let ready = self.behaviors.checked_add(self.notes).ok_or(Error::Capacity)?;
        let words = self.behaviors / 64 + usize::from(!self.behaviors.is_multiple_of(64));
        CallbackStorage::for_capacities(self.behaviors, words, cells, ready,
            self.behaviors, self.behaviors)
    }

    /// Offaudio aggregate admission for real staged jobs. Preserve the caller's
    /// ordinary callback count, local stride, unused cell remainder, fuel, and
    /// every unrelated limit. No baseline-16 replacement or implicit ceiling
    /// increase: every added row and its locals must fit the supplied policy.
    /// The caller proves these population counts from actual prepared/live jobs;
    /// this storage helper does not turn source metadata into executable work.
    pub fn with_source_callbacks(self, plan: &Prepared, population: SourceCallbackPopulation,
        budget: CallbackStorageBudget) -> Result<(Self, CallbackStorage), Error>
    {
        let additional = population.prepared_roots.checked_add(population.source_contexts)
            .ok_or(Error::Capacity)?;
        // With no ordinary arena there is no established local stride. Do not
        // reinterpret an unused legacy cell ceiling as a new per-job bank.
        if self.behaviors == 0 && self.behavior_cells != 0 && additional != 0 {
            return Err(Error::Capacity);
        }
        let stride = self.behavior_cells.checked_div(self.behaviors).unwrap_or(0);
        if plan.behavior_local_count() > stride { return Err(Error::Capacity); }
        let behaviors = self.behaviors.checked_add(additional).ok_or(Error::Capacity)?;
        let behavior_cells = additional.checked_mul(stride)
            .and_then(|cells| self.behavior_cells.checked_add(cells))
            .ok_or(Error::Capacity)?;
        let expanded = Self { behaviors, behavior_cells, ..self };
        let storage = expanded.callback_storage()?;
        budget.check(storage)?;
        Ok((expanded, storage))
    }

    /// Capacities for playing `plan`: `notes` held at once and `voices`,
    /// with script state sized by [`Self::script_capacity`]. Hosts and test
    /// harnesses share this so a plan that plays in one plays in the other.
    /// `voices` is the initial polyphony; each script note also gets the
    /// release voices of every stage on top, which its release phase reserves.
    pub fn for_plan(plan: &Prepared, notes: usize, voices: usize) -> Self {
        // Scripts hold a callback or two per held note, so size them by the note
        // capacity, clamped to [SCRIPT_KEYS, SCRIPT_KEYS_MAX] (the floor keeps small
        // harness plans unchanged).
        let behaviors = Self::script_capacity_for(
            plan,
            notes.clamp(Self::SCRIPT_KEYS, Self::SCRIPT_KEYS_MAX),
        );
        let voices =
            voices + plan.stage_count() * plan.release_voices() * Self::SCRIPT_KEYS.min(notes);
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
struct InputOrigin {
    input: Input,
    source: Input,
    expression: Expression,
    intrinsic:Option<f64>,
    performance: usize,
    receiver: Option<receiver::components::ReceiverComponents>,
}

#[derive(Clone, Copy)]
enum NoteOrigin {
    Input(InputOrigin),
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
    /// Physical provenance when an adapter maps host identities onto a wire.
    input_source: Option<Input>,
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
    processing_owners: usize,
    children: usize,
}

/// Fixed editor probe of actual occupied native voices (v1 playheads).
#[derive(Clone, Copy, Debug)]
pub struct VoiceTap {
    pub group: u32,
    pub key: u8,
    pub velocity: u8,
    pub phase: u8,
    pub level: f32,
}

/// A voice that plays through a time-stretch engine of its plan's pool. Its
/// cursor then runs on the time axis (`speed`); the engine plays the pitch.
#[derive(Clone, Copy, Debug)]
struct StretchLink {
    engine: u8,
    spec: source::StretchSpec,
    /// Source frames per output frame at unit speed (asset rate / output rate).
    rate_ratio: f64,
    /// Asset length, for the engine's reads.
    frames: usize,
    started: bool,
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    plan: PlanId,
    stretch: Option<StretchLink>,
    /// Pitch step of a stretch voice (its cursor holds the time step).
    stretch_step: f64,
    allocation: [allocation::Link; 4],
    family: FamilyId,
    processing: Option<(ProcessingOwnerId, usize)>,
    processing_gate: bool,
    siblings: Siblings,
    sample: usize,
    cursor: source::Cursor,
    base_step: f64,
    /// Immutable source rate/pitch at birth, including old-plan MTS and revision.
    inherent_step:f64,
    pitch_tracked:bool,
    /// v1 Voice::pitch: cache the exact modulation exponent and its ratio.
    mod_pitch: (f64, f64),
    stream_pages: usize,
    chain: Option<usize>,
    bus: Option<usize>,
    tail_remaining: Option<u32>,
    dsp_fade: Option<(u32, f32)>,
    script_fade: Option<script_params::Fade>,
    envelope: EnvelopeState,
    gain: f32,
    /// v1 audible_voices: applied channel gains at the last render chunk.
    last_gains: [f32; 2],
    started: bool,
    /// Authored absolute deadline: equal deadlines in one family form an onset cohort.
    start_at: u64,
    /// Admission order, for oldest-first stealing.
    born: u64,
    /// Fading out after being stolen; no longer counts against polyphony.
    stolen: bool,
    /// The region's group, for script group layers.
    group: Option<u32>,
    source_zone: u32,
    /// The region's native zone identity, which `EventInfo::ZoneId` reports
    /// when the instrument has a native catalog.
    native_zone: Option<u32>,
    /// Frames a releasing voice's gain bound has stayed under `render::INAUDIBLE`.
    quiet: u32,
    /// Mono of the voice's previous span (first `follow_len` frames): the audio
    /// an envelope follower samples at the next modulation step.
    follow_audio: [f32; dsp::BLOCK],
    follow_len: usize,
}

impl Voice {
    fn held_onset(&self) -> bool {
        self.cursor.holding_onset() || self.cursor.pending_start_runway() > 1
    }
    /// Set this block's pitch step. A stretch voice's cursor is the time axis:
    /// it advances at the speed-scaled rate while the engine plays `pitch`.
    fn set_step(&mut self, pitch: f64) {
        match self.stretch {
            None => self.cursor.set_step(pitch),
            Some(link) => {
                self.stretch_step = pitch;
                let time = f64::from(link.spec.speed) * link.rate_ratio;
                self.cursor.set_step(time.clamp(resample::MIN_STEP, resample::MAX_STEP));
            }
        };
    }
}

#[derive(Clone, Copy, Debug)]
struct Slot<T> {
    previous: Option<usize>,
    next: Option<usize>,
    generation: u64,
    value: Option<T>,
}

struct Arena<T> {
    first: Option<usize>,
    last: Option<usize>,
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
            first: None,
            last: None,
            occupied: 0,
            available: capacity,
            reserved: 0,
            free,
            slots: std::iter::repeat_with(|| Slot {
                previous: None,
                next: None,
                generation: 0,
                value: None,
            })
            .take(capacity)
            .collect(),
        }
    }

    /// Fallible offaudio reserve for opt-in owner arenas. Actual capacities are
    /// checked before initialization; published callbacks never call this.
    fn try_blank(capacity:usize)->Result<(Box<[Slot<T>]>,Box<[u64]>),Error>{
        std::alloc::Layout::array::<Slot<T>>(capacity).map_err(|_|Error::Capacity)?;
        std::alloc::Layout::array::<u64>(capacity.div_ceil(64)).map_err(|_|Error::Capacity)?;
        let mut slots=Vec::new();slots.try_reserve_exact(capacity).map_err(|_|Error::Capacity)?;
        std::alloc::Layout::array::<Slot<T>>(slots.capacity()).map_err(|_|Error::Capacity)?;
        let mut free=Vec::new();free.try_reserve_exact(capacity.div_ceil(64)).map_err(|_|Error::Capacity)?;
        std::alloc::Layout::array::<u64>(free.capacity()).map_err(|_|Error::Capacity)?;
        slots.resize_with(capacity,||Slot{previous:None,next:None,generation:0,value:None});
        free.resize(capacity.div_ceil(64),u64::MAX);
        if !capacity.is_multiple_of(64)&&let Some(last)=free.last_mut(){*last=(1u64<<(capacity%64))-1;}
        Ok((slots.into_boxed_slice(),free.into_boxed_slice()))
    }

    /// Empty slots and an all-free bitmap for `capacity` entries.
    fn blank(capacity: usize) -> (Box<[Slot<T>]>, Box<[u64]>) {
        let mut free = vec![u64::MAX; capacity.div_ceil(64)].into_boxed_slice();
        if !capacity.is_multiple_of(64) {
            *free.last_mut().unwrap() = (1u64 << (capacity % 64)) - 1;
        }
        let slots = std::iter::repeat_with(|| Slot {
            previous: None,
            next: None,
            generation: 0,
            value: None,
        })
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
        let kept = if old.is_multiple_of(64) {
            u64::MAX
        } else {
            (1u64 << (old % 64)) - 1
        };
        for (w, &bits) in self.free.iter().enumerate() {
            let mask = if w + 1 == words { kept } else { u64::MAX };
            free[w] = (bits & mask) | (free[w] & !mask);
        }
        self.available += slots.len() - old;
        std::mem::swap(&mut self.slots, slots);
        std::mem::swap(&mut self.free, free);
    }

    fn insert(&mut self, value: T) -> Result<Handle, Error> {
        self.insert_below(value, self.slots.len())
    }

    fn insert_below(&mut self, value: T, capacity: usize) -> Result<Handle, Error> {
        if self.available() == 0 {
            return Err(Error::Capacity);
        }
        // Scan words rather than every slot, retaining lowest-slot allocation and
        // therefore deterministic voice summation after arbitrary holes/reuse.
        let (word, bits) = self
            .free
            .iter_mut()
            .take(capacity.div_ceil(64))
            .enumerate()
            .find(|(word, bits)| {
                let mask = if (*word + 1) * 64 > capacity {
                    (1u64 << (capacity % 64)) - 1
                } else {
                    u64::MAX
                };
                **bits & mask != 0
            })
            .ok_or(Error::Capacity)?;
        let index = word * 64 + bits.trailing_zeros() as usize;
        *bits &= *bits - 1;
        let slot = &mut self.slots[index];
        debug_assert!(slot.value.is_none() && slot.generation < u64::MAX);
        slot.generation += 1; // Exhausted generations are quarantined, never wrapped.
        let generation = slot.generation;
        slot.value = Some(value);
        self.link(index);
        self.occupied += 1;
        self.available -= 1;
        Ok(Handle {
            runtime: self.runtime,
            index,
            generation,
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
        self.unlink(id.index);
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
        self.link(id.index);
    }

    fn link(&mut self, index: usize) {
        self.slots[index].previous = self.last;
        self.slots[index].next = None;
        if let Some(last) = self.last {
            self.slots[last].next = Some(index);
        } else {
            self.first = Some(index);
        }
        self.last = Some(index);
    }

    fn unlink(&mut self, index: usize) {
        let (previous, next) = (self.slots[index].previous, self.slots[index].next);
        if let Some(previous) = previous {
            self.slots[previous].next = next;
        } else {
            self.first = next;
        }
        if let Some(next) = next {
            self.slots[next].previous = previous;
        } else {
            self.last = previous;
        }
        // Keep the successor until slot reuse: retirement can also unlink a
        // parent that a live traversal has yet to visit.
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
    fallback_envelope: Option<[u32; 2]>,
    part_tone_cutoff: f64,
    /// Quarter notes per minute for tempo-synced modulation.
    tempo: f64,
    plans: Arena<Generation>,
    active_plan: PlanId,
    plan_queues: Option<PlanQueues>,
    control_queues: Option<control::ControlQueues>,
    sample_queues: Option<prepare::sample_revision::SampleQueues>,
    notes: Arena<Note>,
    release_times: Box<[release::ReleaseTimes]>,
    note_events: Box<[note_event::NoteEvent]>,
    source_ids: Vec<(i32, NoteId)>,
    last_source_id: i32,
    last_callback_id: i32,
    next_external_turn_order:u64,
    reserved_external_turn_orders:u64,
    external_execution_aborted:bool,
    source_task_scratch:Vec<BehaviorId>,
    event_values:Arena<receiver::event_value::EventValue>,
    event_posts:Arena<receiver::event_value::EventPost>,
    callback_id_reservations: usize,
    closed_notes: Vec<NoteId>,
    channels: Arena<Channel>,
    voices: Arena<Voice>,
    processing_owners: Arena<topology::ProcessingOwner>,
    processing_activation: u64,
    processing_admission: bool,
    voice_activity: Box<[u64]>,
    onsets: onset::Storage,
    pending_onsets: usize,
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
    offline: bool,
    stream_fault: Option<StreamError>,
    stream_horizon: u32,
    stream_start_runway: u32,
    stream_admission_errors: [u64; 2],
    stream_reserved: usize,
    voice_drops: u64,
    voice_pool_refusals: u64,
    waiting_voice_replacements: u64,
    refused_starts: u64,
    stretch_fallbacks: u64,
    /// Voice-pool growths adopted, and refused (see `grow`).
    voice_growths: u64,
    growth_failures: u64,
    growth: Option<grow::GrowthQueues>,
    /// Set on the audio side when the pool runs three quarters full.
    voice_pressure: grow::Pressure,
    note_pressure: grow::NotePressure,
    steal_releases: bool,
    cold_starts: bool,
    cold_started: u64,
    /// Last and peak `render` nanoseconds, and the last call's frames.
    render_time: [u64; 3],
    signal_trace: bool,
    families: Arena<Family>,
    decisions: Arena<variation::Decision>,
    expressions: Arena<ExpressionOwner>,
    receivers:Arena<receiver::ReceiverOwner>,
    publishing_receiver:bool,
    receiver_changes:Vec<(ExpressionId,Expression)>,
    receiver_policy_changes:Vec<(receiver::ReceiverId,receiver::components::GestureProposal)>,
    receiver_component_changes:Box<[Option<receiver::components::ReceiverComponents>]>,
    /// Some expression follows another, so changes must propagate.
    expression_followers: bool,
    expression_changes: Box<[Option<RenderedExpression>]>,
    /// Monotonic count of owners reached by forwarded poly-pressure inputs.
    poly_pressure_owners: usize,
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
    /// Instructions every callback together may run per block, and what is
    /// left of it. The excess waits in `yielded`, so a chord's script work
    /// spreads over blocks instead of landing in one.
    block_fuel: usize,
    block_fuel_left: usize,
    behavior_block_explicit: bool,
    behavior_time_left: Option<std::time::Duration>,
    behavior_stride: usize,
    behavior_locals: Box<[i64]>,
    note_stride: usize,
    note_values: Box<[i64]>,
    note_params: script_params::NoteParamsPool,
    /// Set once a script writes a voice parameter; voices then render in chunks.
    // ponytail: sticky for the runtime's life; count live layers if chunking costs show up.
    script_params: bool,
    /// Keys whose latest physical event was a note-on: one note-off clears the
    /// key however many presses stacked, as `%KEY_DOWN` does in Kontakt.
    input_keys: u128,
    /// Notes a running callback played whose attack waits for it to yield:
    /// (callback, note, selection stage). Never holds more than the note pool.
    deferred: Vec<(BehaviorId, NoteId, usize)>,
    /// Opt-in selection diagnostics, see [`Runtime::record_selections`].
    selection_log: Option<Vec<SelectionRecord>>,
    /// The last attack that selected no region, until taken; see [`SilentNote`].
    silent: Option<SilentNote>,
    /// The last callback fault until taken: (plan program, error).
    fault: Option<(usize, Error)>,
    /// Opt-in script parameter writes, see [`Runtime::record_script_writes`].
    write_log: Option<Vec<String>>,
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
        Self::new_cancelable(plan, limits, &||false)
    }

    pub fn new_cancelable(plan:Prepared,limits:Limits,canceled:&dyn Fn()->bool)->Result<Self,Error> {
        Self::new_with_note_params(plan, limits, limits.notes,canceled)
    }

    fn new_with_note_params(
        mut plan: Prepared,
        limits: Limits,
        initial_notes: usize,
        canceled: &dyn Fn()->bool,
    ) -> Result<Self, Error> {
        plan.validate_signal_execution()?;
        plan.validate_source_tasks()?;
        trace_report::configure(&mut plan)?;
        if initial_notes == 0 || initial_notes > limits.notes {
            return Err(Error::InvalidInput);
        }
        if limits.notes == 0 || limits.performances == 0 {
            return Err(Error::InvalidInput);
        }
        std::alloc::Layout::array::<Slot<Voice>>(limits.voices).map_err(|_|Error::Capacity)?;
        std::alloc::Layout::array::<Slot<topology::ProcessingOwner>>(limits.voices)
            .map_err(|_| Error::Capacity)?;
        receiver::allocation_bytes(limits.expressions,limits.performances,limits.expressions,limits.performances)?;
        // Price canonical component payload on EVERY expression slot, including
        // future detach/follower slots, before any arena allocation.
        std::alloc::Layout::array::<Slot<ExpressionOwner>>(limits.expressions).map_err(|_|Error::Capacity)?;
        std::alloc::Layout::array::<Slot<receiver::ReceiverOwner>>(limits.performances).map_err(|_|Error::Capacity)?;
        let mut receiver_policy_changes=Vec::new();
        receiver_policy_changes.try_reserve_exact(limits.performances).map_err(|_|Error::Capacity)?;
        std::alloc::Layout::array::<(receiver::ReceiverId,receiver::components::GestureProposal)>(receiver_policy_changes.capacity()).map_err(|_|Error::Capacity)?;
        let mut receiver_changes=Vec::new();
        receiver_changes.try_reserve_exact(limits.expressions).map_err(|_|Error::Capacity)?;
        std::alloc::Layout::array::<(ExpressionId,Expression)>(receiver_changes.capacity()).map_err(|_|Error::Capacity)?;
        std::alloc::Layout::array::<Option<receiver::components::ReceiverComponents>>(limits.expressions).map_err(|_|Error::Capacity)?;
        receiver::allocation_bytes(limits.expressions,limits.performances,receiver_changes.capacity(),receiver_policy_changes.capacity())?;
        let initial_controllers = plan.initial_controllers.clone();
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
        std::alloc::Layout::array::<BehaviorId>(limits.behaviors).map_err(|_|Error::Capacity)?;
        let mut source_task_scratch=Vec::new();
        source_task_scratch.try_reserve_exact(limits.behaviors).map_err(|_|Error::Capacity)?;
        std::alloc::Layout::array::<BehaviorId>(source_task_scratch.capacity()).map_err(|_|Error::Capacity)?;
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
            performance::PerformanceState::validate(limits.notes, limits.performances, limits.channels)?;
        static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(1);
        #[allow(deprecated, reason = "fetch_update supports the Rust 1.92 minimum")]
        let id = NEXT_RUNTIME
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::Capacity)?;
        let rate = plan.rate;
        let initial_residency=plan.prepare_initial_residency(canceled)?;
        let initial_callbacks=initial_residency.pending();
        let source_tasks=receiver::task::TaskState::new(&plan)?;
        let mut plans = Arena::new(id, 1);
        let active_plan = PlanId(
            plans.insert(Generation {
                script_revision: 0,
                native_cycle: 0,
                native_random: native_start::NativeRandom::new(0),
                native_draw: 0,
                request: 0,
                allocation: allocation::AllocationState::new(&plan),
                sequences: variation::SequenceState::new(&plan),
                controls: control::ControlState::new(&plan),
                scripts: plan
                    .script_initial
                    .iter()
                    .map(ops::ScriptInitial::bank)
                    .collect(),
                midi_object: plan.midi_object.clone(),
                zone_routing: plan.zone_routing.as_ref().map(prepare::zone_routing::PreparedRouting::state).transpose()?,
                dsp: dsp::DspState::new(&plan, limits.voices, limits.expressions, 1)?,
                groups: groups::GroupState::new(plan.group_count, limits.notes, plan.stages.len())?,
                controllers: controller_event::ControllerState::new(&plan, limits.performances, limits.channels)?,
                projections: note_event::NoteProjections::new(plan.stages.len(), limits.notes)?,
                modulation: voice_mod::VoiceModState::new(&plan.voice_modulation, limits.voices)?,
                script: script_params::EngineLayers::new(&plan),
                samples: prepare::sample_revision::SampleState::new(&plan),
                initial_residency,
                start_cursor: 0,
                prepared: Box::new(plan),
                notes: 0,
                callbacks: initial_callbacks,
                physical_events:0,
                source_tasks,event_values:0
            })?,
        );
        let signal_trace = plans
            .get(active_plan.0)
            .unwrap()
            .prepared
            .signal_trace
            .is_some();
        let mut runtime = Self {
            signal_trace,
            rate,
            fallback_envelope: None,
            part_tone_cutoff: 20_000.,
            tempo: 120.0,
            plans,
            active_plan,
            plan_queues: None,
            control_queues: None,
            sample_queues: None,
            notes: Arena::new(id, limits.notes),
            closed_notes: Vec::with_capacity(limits.notes),
            source_ids: Vec::with_capacity(limits.notes),
            last_source_id: 0,
            last_callback_id: 0,
            next_external_turn_order:0,
            reserved_external_turn_orders:0,
            external_execution_aborted:false,
            source_task_scratch,
            event_values:Arena::new(id,0),
            event_posts:Arena::new(id,0),
            callback_id_reservations: 0,
            channels: Arena::new(id, limits.channels),
            voices: Arena::new(id, limits.voices),
            processing_owners: Arena::new(id, limits.voices),
            processing_activation: 0,
            processing_admission: false,
            voice_activity: vec![0; limits.voices.div_ceil(64)].into_boxed_slice(),
            onsets: onset::Storage::new(limits.voices),
            pending_onsets: 0,
            stealing: None,
            stolen: 0,
            steals: 0,
            voice_order: 0,
            parallel: None,
            lanes: Arc::new(std::sync::atomic::AtomicUsize::new(1)),
            kernel: resample::Kernel::new(ResampleQuality::default()),
            stream_cache: None,
            stream_underruns: 0,
            offline: false,
            stream_fault: None,
            stream_horizon: 1,
            stream_start_runway: 1,
            stream_admission_errors: [0; 2],
            stream_reserved: 0,
            voice_drops: 0,
            voice_pool_refusals: 0,
            waiting_voice_replacements: 0,
            refused_starts: 0,
            stretch_fallbacks: 0,
            voice_growths: 0,
            growth_failures: 0,
            growth: None,
            voice_pressure: Arc::default(),
            note_pressure: Arc::default(),
            steal_releases: false,
            cold_starts: false,
            cold_started: 0,
            render_time: [0; 3],
            families: Arena::new(id, limits.families),
            decisions: Arena::new(id, limits.decisions),
            expressions: Arena::new(id, limits.expressions),
            receivers:Arena::new(id,limits.performances),
            publishing_receiver:false,
            receiver_changes,receiver_policy_changes,
            receiver_component_changes:vec![None;limits.expressions].into_boxed_slice(),
            expression_followers: false,
            expression_changes: vec![None; limits.expressions].into_boxed_slice(),
            poly_pressure_owners: 0,
            commands: Vec::with_capacity(limits.commands),
            behaviors: Arena::new(id, limits.behaviors),
            behavior_ready: Vec::with_capacity(ready_capacity),
            yielded: std::collections::VecDeque::with_capacity(limits.behaviors),
            preemptions: 0,
            longest_preempted: 0,
            dispatching_behavior: false,
            started_plan: None,
            behavior_fuel: limits.behavior_fuel,
            block_fuel: usize::MAX,
            block_fuel_left: usize::MAX,
            behavior_block_explicit: false,
            behavior_time_left: None,
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
            note_params: script_params::NoteParamsPool::new(limits.notes, initial_notes),
            script_params: false,
            input_keys: 0,
            deferred: Vec::with_capacity(limits.notes),
            selection_log: None,
            silent: None,
            fault: None,
            write_log: None,
            release_times: vec![release::ReleaseTimes::default(); limits.notes].into_boxed_slice(),
            note_events: vec![note_event::NoteEvent::new(NotePitch::Key(0), 0.); limits.notes]
                .into_boxed_slice(),
            selections: vec![performance::NoteSelection::default(); limits.notes]
                .into_boxed_slice(),
            performance_state: {
                let mut state =
                    performance::PerformanceState::new(state_capacity, limits.performances, limits.channels)?;
                for &(controller, value) in &initial_controllers {
                    state.states[0].controllers[usize::from(controller)] = value;
                    for input in state.input_controllers.iter_mut() {
                        input[usize::from(controller)] = value;
                    }
                }
                state
            },
        };
        // Authored init effects must reach the DSP before the first input or
        // getter, including hosts that send MIDI before their first render.
        runtime.start_plan_programs();
        Ok(runtime)
    }

    /// Actual reserved callback payload. This immutable, allocation-free query
    /// includes allocator spare Vec/VecDeque capacity. An offaudio candidate
    /// installer must recheck its explicit CallbackStorageBudget against this
    /// report before publishing; it does not alter legacy Runtime admission.
    pub fn callback_storage(&self) -> Result<CallbackStorage, Error> {
        CallbackStorage::for_capacities(self.behaviors.slots.len(), self.behaviors.free.len(),
            self.behavior_locals.len(), self.behavior_ready.capacity(), self.yielded.capacity(),
            self.source_task_scratch.capacity())
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
    /// Fixed ownership slots, including held notes, tails and generated notes.
    /// Transport adapters prepare matching binding/scratch storage off audio;
    /// MIDI key range does not bound overlapping host note IDs.
    pub fn note_capacity(&self) -> usize {
        self.notes.slots.len()
    }
    pub fn voice_count(&self) -> usize {
        self.voices.count()
    }
    /// Port from v1 0cb7a8a0:src/engine/mod.rs audible_voices predicate.
    /// Walk occupied voices, including script-muted layers; never scan capacity.
    pub fn audible_voice_count(&self) -> usize {
        let mut audible = 0;
        let mut next = self.voices.first;
        while let Some(i) = next {
            let slot = &self.voices.slots[i];
            let voice = slot.value.as_ref().unwrap();
            if voice.last_gains != [0.0; 2] {
                let family = self.families.get(voice.family.0).unwrap();
                let note = self.notes.get(family.note.0).unwrap();
                let plan = self.plans.get(note.plan.0).unwrap();
                audible += usize::from(!plan.script.fader_muted(voice.group));
            }
            next = slot.next;
        }
        audible
    }

    pub fn voice_taps(&self) -> [Option<VoiceTap>; 16] {
        let mut taps = [None; 16];
        let mut next = self.voices.first;
        for to in &mut taps {
            let Some(i) = next else {
                break;
            };
            let slot = &self.voices.slots[i];
            let v = slot.value.as_ref().unwrap();
            let family = self.families.get(v.family.0).unwrap();
            let note = self.notes.get(family.note.0).unwrap();
            *to = Some(VoiceTap {
                group: v.group.unwrap_or(u32::MAX),
                key: note.pitch.key(),
                velocity: (note.velocity * 127.).round() as u8,
                phase: v.envelope.editor_phase(),
                level: v.envelope.current(),
            });
            next = slot.next;
        }
        taps
    }

    pub fn pending_commands(&self) -> usize {
        self.commands.len()
    }

    /// Due frame of each scheduled command, earliest first (diagnostics).
    pub fn pending_command_frames(&self) -> impl Iterator<Item = u64> + '_ {
        self.commands.iter().map(|c| c.at)
    }

    /// Frames silenced because finite source values overflowed during summation.
    /// Buses of the active plan.
    pub fn bus_count(&self) -> usize {
        self.plans
            .get(self.active_plan.0)
            .map_or(0, |g| g.prepared.buses.len())
    }

    /// Replace convolution `slot` of the active plan (see
    /// [`Prepared::convolution_slots`]) with `upload`, at a block boundary and
    /// without allocating; `upload` then holds the old processor, to drop off
    /// the audio thread. The new one starts with empty history.
    pub fn swap_convolution(
        &mut self,
        slot: usize,
        upload: &mut ConvolutionUpload,
    ) -> Result<(), Error> {
        let g = self
            .plans
            .get_mut(self.active_plan.0)
            .ok_or(Error::StaleHandle)?;
        g.dsp
            .buses
            .swap_convolution(&g.prepared.buses, slot, upload)
    }

    /// Stereo-output buses of the active plan, after mix gain; taking resets
    /// their peaks. Wide outputs have no fabricated two-channel peak row;
    /// exact bus_peak queries report UnsupportedSourceLayout for those taps.
    pub fn take_bus_peaks(&mut self, mut each: impl FnMut(usize, [f32; 2])) {
        if let Some(g) = self.plans.get_mut(self.active_plan.0) {
            for (bus, peak) in g.dsp.buses.peaks.iter_mut().enumerate() {
                if g.prepared.buses.output_channels(bus)==Ok(2){each(bus, std::mem::take(peak));}
            }
        }
    }

    /// Enable raw input peaks for a bus of the active plan at a block boundary.
    /// No allocation or sample scan occurs for an unobserved input. Changing
    /// interest clears its previous observation; old generations are untouched.
    pub fn enable_bus_input_peak(&mut self, bus: usize, enabled: bool) -> Result<(), Error> {
        let generation=self.plans.get_mut(self.active_plan.0).ok_or(Error::StaleHandle)?;
        if enabled && generation.prepared.buses.input_channels(bus)?!=2{return Err(Error::UnsupportedSourceLayout);}
        generation.dsp.buses.enable_input_peak(bus, enabled)
    }

    /// Raw linear stereo peak of an exact runtime bus since last taken.
    /// Input must have been enabled before rendering. This does not map native
    /// element identities or approximate their meter display laws.
    pub fn bus_peak(&self, bus: usize, tap: BusPeakTap) -> Result<[f32; 2], Error> {
        let generation=self.plans.get(self.active_plan.0).ok_or(Error::StaleHandle)?;
        let channels=match tap {BusPeakTap::Input=>generation.prepared.buses.input_channels(bus)?,BusPeakTap::Output=>generation.prepared.buses.output_channels(bus)?};
        if channels!=2{return Err(Error::UnsupportedSourceLayout);}
        generation.dsp.buses.peak(bus, tap)
    }

    /// Take enabled input taps independently of the existing output taps.
    pub fn take_bus_input_peaks(&mut self, each: impl FnMut(usize, [f32; 2])) {
        if let Some(g) = self.plans.get_mut(self.active_plan.0) {
            g.dsp.buses.take_input_peaks(each);
        }
    }

    /// Resolve the actual immutable compiled operand on this retained plan.
    /// IDs alone do not pin its lifetime; real callbacks/tasks own that lease.
    pub fn source_owner_control(&self,plan:PlanId,owner:sampler_ir::SourceOwnerId,
        field:SourceOwnerControl)->Result<ControlId,Error>{
        let generation=self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let prepared=&generation.prepared;
        let owner_index=prepared.source_owner_index(owner).ok_or(Error::InvalidInput)?;
        let id=if field==SourceOwnerControl::MidiMute {
            prepared.source_owners().get(owner_index).and_then(|row|row.mute.as_ref())
                .and_then(|switch|switch.control).ok_or(Error::InvalidInput)?
        }else{prepared.buses.output_control(owner,field)?};
        let domain=prepared.controls[prepared.control_index(id)?].domain;
        let valid=match field {
            SourceOwnerControl::OutputMute|SourceOwnerControl::MidiMute=>domain==ControlDomain::Toggle,
            SourceOwnerControl::Gain=>matches!(domain,ControlDomain::Real{min,max}if min>=0. && max<=f64::from(f32::MAX)),
            SourceOwnerControl::Pan=>matches!(domain,ControlDomain::Real{min,max}if min>=-1. && max<=1.),
        };
        if !valid{return Err(Error::InvalidInput);}Ok(id)
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
        if generation.prepared.buses.output_channels(bus)?!=2 && mix.gain[0]!=mix.gain[1] {
            return Err(Error::UnsupportedSourceLayout);
        }
        if mix.output.is_some() && generation.prepared.buses.main_send_channels(bus)?!=2 {
            return Err(Error::UnsupportedSourceLayout);
        }
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
        self.note_on_owned_input(performance, input, input, pitch, velocity, expression, None)
    }

    /// Source and receiver ownership are present when the note is constructed,
    /// before any fresh callback captures its controller/receiver context.
    fn note_on_owned_input(
        &mut self, performance: PerformanceId, input: Input, source: Input,
        pitch: NotePitch, velocity: f64, expression: Expression,
        receiver: Option<receiver::components::ReceiverComponents>,
    ) -> Result<NoteId, Error> {
        self.note_on_owned_pitch_base(performance,input,source,pitch,None,velocity,expression,receiver)
    }
    fn note_on_owned_pitch_base(&mut self,performance:PerformanceId,input:Input,source:Input,pitch:NotePitch,intrinsic:Option<f64>,velocity:f64,expression:Expression,receiver:Option<receiver::components::ReceiverComponents>)->Result<NoteId,Error>{
        if intrinsic.is_some_and(|value|!NotePitch::Absolute(value).valid()){return Err(Error::InvalidInput);}
        if self.external_execution_aborted{return Err(Error::Canceled);}
        if
input.channel >= 16 || input.group >= 16 || input.key >= 128
            || source.channel >= 16 || source.group >= 16 || source.key >= 128
            || !expression.valid() { return Err(Error::InvalidInput); }
        if let Some(component)=receiver { self.receiver(component.receiver)?; }
        self.validate_zone_routing_pitch(self.active_plan, pitch)?;
        let performance = self.performance_index(performance)?;
        self.apply_due();
        if input.external_id.is_some() && self.owns_input(performance, input) {
            return Err(Error::DuplicateInput);
        }
        self.admit(NoteOrigin::Input(InputOrigin {
            input, source, expression, intrinsic, performance, receiver,
        }), pitch, velocity)
    }

    /// Admit a protocol-owned absolute intrinsic base independently of selection
    /// and physical source. None retains the actual inherent Key/MTS/Attribute base.
    pub fn note_on_in_with_pitch_base(&mut self,performance:PerformanceId,input:Input,source:Input,pitch:NotePitch,intrinsic:Option<f64>,velocity:f64,expression:Expression)->Result<NoteId,Error>{
        self.note_on_owned_pitch_base(performance,input,source,pitch,intrinsic,velocity,expression,None)
    }

    /// Retained terminal owners still reserve their identity. Traverse the
    /// existing occupancy bitmap while sparse, and contiguous slots while
    /// dense; pointer-chasing live notes is slower for a full arena.
    fn owns_input(&self, performance: usize, input: Input) -> bool {
        if self.notes.count() == 0 {
            return false;
        }
        if self.notes.count() > self.notes.slots.len() / 2 {
            return self.notes.slots.iter().enumerate().any(|(index, slot)| {
                slot.value.is_some_and(|n| n.input == Some(input))
                    && self.selections[index].performance == performance
            });
        }
        for (word, &free) in self.notes.free.iter().enumerate() {
            let valid = if (word + 1) * 64 > self.notes.slots.len() {
                (1u64 << (self.notes.slots.len() % 64)) - 1
            } else {
                u64::MAX
            };
            let mut occupied = !free & valid;
            while occupied != 0 {
                let index = word * 64 + occupied.trailing_zeros() as usize;
                let slot = &self.notes.slots[index];
                if slot.value.is_some_and(|n| n.input == Some(input))
                    && self.selections[index].performance == performance
                {
                    return true;
                }
                occupied &= occupied - 1;
            }
        }
        false
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

    /// Admit a script-generated root without owning a physical input key.
    pub fn generated_note(
        &mut self,
        address: ChannelAddress,
        key: u8,
        velocity: f64,
    ) -> Result<NoteId, Error> {
        self.apply_due();
        if address.channel >= 16 || address.group >= 16 {
            return Err(Error::InvalidInput);
        }
        self.performance(0)?;
        self.admit(
            NoteOrigin::Generated(self.active_plan, address, 0),
            NotePitch::Key(key),
            velocity,
        )
    }

    /// Retire an admitted source-owned note after its voices, tails and work finish.
    pub fn retire_when_silent(&mut self, note: NoteId) -> Result<(), Error> {
        let state = self.notes.get_mut(note.0).ok_or(Error::StaleHandle)?;
        if state.input.is_some() || state.attack == AttackStatus::Pending {
            return Err(Error::InvalidInput);
        }
        state.retire_when_silent = true;
        Ok(())
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
        let plan = match origin {
            NoteOrigin::Input(..) => self.active_plan,
            NoteOrigin::Generated(plan, ..) => plan,
            NoteOrigin::Child(parent, ..) => self.notes.get(parent.0).ok_or(Error::StaleHandle)?.plan,
        };
        if self.external_execution_aborted{return Err(Error::Canceled);}
        self.validate_zone_routing_pitch(plan, pitch)?;
        let performance = match origin {
            NoteOrigin::Input(input) => input.performance,
            NoteOrigin::Generated(_, _, performance) => performance,
            NoteOrigin::Child(parent, ..) => {
                self.notes.get(parent.0).ok_or(Error::StaleHandle)?;
                self.selections[parent.0.index].performance
            }
        };
        let (input, parent, linked_release, inheritance, initial) = match origin {
            NoteOrigin::Input(input) => (
                Some(input.input),
                None,
                false,
                Inheritance::Independent,
                input.expression,
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
            NoteOrigin::Input(input) => input.input.channel_address(),
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
                let mut follows = None;
                let (value, rendered) =
                    if let (Inheritance::Expression, Some(parent)) = (policy, parent) {
                        let owner = *self.expressions.get(parent.0).unwrap();
                        follows = Some(owner.follows.unwrap_or(parent));
                        let value = Expression {
                            pitch_semitones: owner.value.pitch_semitones,
                            pressure: owner.value.pressure,
                            timbre: owner.value.timbre,
                            bend: owner.value.bend,
                            ..initial
                        };
                        (value, self.project_expression(program, value, None)?)
                    } else if let (Inheritance::Snapshot, Some(parent)) = (policy, parent) {
                        let owner = self.expressions.get(parent.0).unwrap();
                        (owner.value, owner.rendered)
                    } else {
                        (initial, self.project_expression(program, initial, None)?)
                    };
                self.expression_followers |= follows.is_some();
                let retained=follows.map(|id| {
                    let source=self.expressions.get(id.0).ok_or(Error::StaleHandle)?;
                    Ok::<_,Error>((id,source.followers.checked_add(1).ok_or(Error::Capacity)?))
                }).transpose()?;
                let intrinsic=match (policy,parent){
                    (Inheritance::Expression|Inheritance::Snapshot,Some(parent))=>self.intrinsic_pitch(parent)?,
                    _=>crate::pitch::IntrinsicPitch{reference:self.plans.get(plan.0).ok_or(Error::StaleHandle)?.prepared.inherent_pitch(pitch)?,absolute:match origin{NoteOrigin::Input(input)=>input.intrinsic,_=>None}},
                };
                let expression=ExpressionId(self.expressions.insert(ExpressionOwner {
                    intrinsic,
                    value,
                    rendered,
                    program,
                    notes: 1,
                    followers:0,captures:0,
                    follows,
                    receiver:match origin { NoteOrigin::Input(input)=>input.receiver, _=>None },
                })?);
                if let Some(component)=self.expressions.get_mut(expression.0).and_then(|owner|owner.receiver.as_mut()) {
                    component.origin=Some(expression);
                }
                if let Some((from,count))=retained {
                    let Some(source)=self.expressions.get_mut(from.0) else {
                        self.expressions.remove(expression.0);return Err(Error::StaleHandle);
                    };
                    source.followers=count;
                }
                expression
            }
        };
        if let Some(input) = input {
            self.input_keys |= 1 << (input.key & 127);
        }
        self.note_note_pressure();
        let id = match self.notes.insert_below(
            Note {
                input,
                input_source: match origin { NoteOrigin::Input(input)=>Some(input.source), _=>None },
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
                processing_owners: 0,
                children: 0,
            },
            self.note_params.capacity(),
        ) {
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
        if !self
            .plans
            .get(plan.0)
            .unwrap()
            .prepared
            .native_start
            .is_empty()
        {
            let rows = u64::from(self.plans.get(plan.0).unwrap().prepared.native_random_rows);
            let generation = self.plans.get_mut(plan.0).unwrap();
            generation.native_draw = generation.native_random.below(rows) as u32;
            let state = self.performance_state.edit(performance);
            state.native_tick = generation.native_cycle;
            state.native_draw = generation.native_draw;
            generation.native_cycle = generation.native_cycle.wrapping_add(1);
            self.performance_state.sync_metadata(performance);
        }
        self.selections[id.index] = performance::NoteSelection {
            performance,
            snapshot: self.performance_state.capture_channel(performance, address),
            consumed_switch: false,
            controller_epoch: self.performance_state.epochs(performance, ChannelScope {
                protocol: address.protocol, port: address.port, group: address.group, channels: 1 << address.channel,
            })[usize::from(address.channel)],
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

    /// Merge live engine gates, including generated notes; release tails are unpressed.
    pub fn pressed_keys(&self, keys: &mut [u8; 128]) {
        let mut next = self.notes.first;
        while let Some(index) = next {
            let slot = &self.notes.slots[index];
            next = slot.next;
            let note = slot.value.as_ref().unwrap();
            if note.gate() {
                let key = &mut keys[usize::from(note.pitch.key())];
                *key = (*key).max((note.velocity * 127.).round().clamp(1., 127.) as u8);
            }
        }
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

    /// Release the physical input owner captured at onset. Hosts with an exact
    /// note identity need no FIFO search. Preserve the physical-key bookkeeping,
    /// pedals, release callbacks and first-release record of [`Self::note_off`].
    /// A generated owner, an already released input or a stale generation is
    /// rejected without releasing another note with the same key.
    pub fn note_off_exact(&mut self, id: NoteId, velocity: Option<f64>) -> Result<(), Error> {
        release::validate_velocity(velocity)?;
        self.apply_due();
        let note = self.notes.get(id.0).ok_or(Error::StaleHandle)?;
        let input = note.input.ok_or(Error::InvalidInput)?;
        if !note.input_down {
            return Err(Error::StaleHandle);
        }
        self.input_keys &= !(1 << (input.key & 127));
        self.key_up_now(id, velocity)
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
        if self.external_execution_aborted{return Err(Error::Canceled);}
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let generation = self.plans.get(n.plan.0).ok_or(Error::StaleHandle)?;
        if generation.source(sample).is_some_and(|s| s.stereo().is_none()) {
            return Err(Error::UnsupportedSourceLayout);
        }
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
        if self.external_execution_aborted{return Err(Error::Canceled);}
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
        if !gain.is_finite() || !(0.0..=1.0).contains(&gain) { return Err(Error::InvalidInput); }
        let source = plan.source_storage(sample).ok_or(Error::InvalidInput)?;
        if source.stereo().is_none() { return Err(Error::UnsupportedSourceLayout); }
        let cursor = playback.cursor(
            source.frame_count(),
            source.sample_rate(),
            self.rate,
        )?;
        self.admit_voice(family, sample, at, gain, envelope, cursor,false)
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
        pitch_tracked:bool,
    ) -> Result<VoiceId, Error> {
        if self.external_execution_aborted{return Err(Error::Canceled);}
        let f = self.families.get(family.0).ok_or(Error::StaleHandle)?;
        if !f.open {
            return Err(Error::ClosedFamily);
        }
        let owner = self.notes.get(f.note.0).unwrap().expression;
        let inherent_step=cursor.step();
        let base_step=inherent_step*if pitch_tracked{self.intrinsic_pitch(owner)?.ratio()}else{1.};
        let step = self
            .pitch_range(owner, true)?
            .apply_source(base_step, cursor.wavetable().is_some())?;
        let cursor = cursor.with_step(step);
        let note = self.notes.get(f.note.0).unwrap();
        let source = self.plans.get(note.plan.0).unwrap().source(sample).ok_or(Error::NotReady)?;
        // Full conservative geometry is shared by admission, service, and the
        // pinned cohort release gate. No requested latency is shortened.
        let cursor = self.stream_start_cursor(note.plan, cursor);
        let (cold,streamed)=self.check_logical_source_ready(source,cursor,envelope)?;
        f.voices.checked_add(1).ok_or(Error::Capacity)?;
        let stream_pages = if streamed && self.stream_cache.is_some() {
            match self.admit_source_streaming(source, cursor, cold) {
                Ok(pages) => pages,
                Err(error) => {
                    if matches!(error, StreamError::Capacity | StreamError::Disconnected) {
                        let index = usize::from(error == StreamError::Disconnected);
                        self.stream_admission_errors[index] =
                            self.stream_admission_errors[index].saturating_add(1);
                    }
                    self.voice_drops = self.voice_drops.saturating_add(1);
                    return Err(if error == StreamError::Capacity {
                        Error::Capacity
                    } else {
                        Error::NotReady
                    });
                }
            }
        } else {
            0
        };
        if !self.processing_admission { self.steal_voices(1); }
        if self.processing_admission && self.voices.available() == 0 {
            // A full voice pool is a voice drop, however the start was admitted.
            self.voice_drops = self.voice_drops.saturating_add(1);
            return Err(Error::Capacity);
        }
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
                Some(i) => {
                    self.waiting_voice_replacements = self.waiting_voice_replacements.saturating_add(1);
                    self.end_voice(VoiceId(self.voices.id(i)));
                }
                None => {
                    self.voice_pool_refusals = self.voice_pool_refusals.saturating_add(1);
                    self.voice_drops = self.voice_drops.saturating_add(1);
                    return Err(Error::Capacity);
                }
            }
        }
        let f = self.families.get(family.0).unwrap();
        let count = f.voices + 1;
        let next_sibling = f.first_voice;
        let plan = self.notes.get(f.note.0).unwrap().plan;
        let id = VoiceId(self.voices.insert(Voice {
            plan,
            stretch: None,
            stretch_step: 1.0,
            allocation: [allocation::Link::default(); 4],
            family,
            processing: None,
            processing_gate: false,
            siblings: Siblings {
                previous: None,
                next: next_sibling,
            },
            sample,
            cursor: if cold && !self.offline { cursor.cold() } else { cursor },
            base_step,
            inherent_step,
            pitch_tracked,
            mod_pitch: (f64::NAN, 1.0),
            stream_pages,
            chain: None,
            bus: None,
            tail_remaining: None,
            dsp_fade: None,
            script_fade: None,
            envelope: EnvelopeState::new(envelope),
            gain,
            last_gains: [0.0; 2],
            started: at == self.now,
            start_at: at,
            born: self.voice_order,
            stolen: false,
            follow_audio: [0.0; dsp::BLOCK],
            follow_len: 0,
            group: None,
            source_zone: 0,
            native_zone: None,
            quiet: 0,
        })?);
        let generation = self.plans.get_mut(plan.0).unwrap();
        generation.samples.pin(&generation.prepared, sample)
            .expect("an admitted voice pins its validated live revision");
        self.link_allocation(id, 0, 0);
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
        self.pending_onsets += usize::from(cursor.pending_start_runway() > 1);
        self.stream_reserved += stream_pages;
        self.cold_started += u64::from(cold);
        self.voice_order += 1;
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

    fn discard_note(&mut self, id: NoteId) -> Result<(), Error> {
        let note = self.notes.get_mut(id.0).ok_or(Error::StaleHandle)?;
        note.attack = AttackStatus::Suppressed;
        note.sostenuto = false;
        self.close_gate(id, ReleaseCause::Discarded);
        self.cleanup_closed_notes();
        Ok(())
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
        self.release_processing_note(id);
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
        self.abandon_zone_jobs();
        // Panic hard-stops tails as well as held voices.
        self.stop_processing_owners();
        for i in 0..self.voices.slots.len() {
            if self.voices.slots[i].value.is_some() {
                self.end_voice(VoiceId(self.voices.id(i)));
            }
        }
        for generation in self.plans.slots.iter_mut().filter_map(|s| s.value.as_mut()) {
            generation.callbacks=generation.callbacks.saturating_sub(generation.initial_residency.cancel());
            generation.start_cursor=generation.prepared.plan_programs.len();
            generation.callbacks = generation.callbacks.saturating_sub(
                generation
                    .midi_object
                    .jobs
                    .iter()
                    .filter(|j| !j.initial)
                    .count(),
            );
            generation.midi_object.jobs.clear();
            for bank in &mut generation.scripts {
                generation.callbacks = generation
                    .callbacks
                    .saturating_sub(bank.array_files.cancel());
            }
            generation.dsp.buses.reset();
        }
        self.cleanup_closed_notes();
        self.cancel_queued_event_value_posts_for_panic();
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
            if cause == ReleaseCause::Discarded {
                for slot in &mut self.behaviors.slots {
                    if let Some(callback) = &mut slot.value
                        && callback.owner == BehaviorOwner::Note(note)
                        && callback.outcome.is_none()
                    {
                        callback.outcome = Some(Outcome::Cancelled);
                    }
                }
            }
            let child_cause = if cause.musical() {
                ReleaseCause::Parent
            } else {
                cause
            };
            let mut child = n.first_child;
            while let Some(index) = child {
                let state = self.notes.at_mut(index);
                child = state.siblings.next;
                // Stage 0 is the gate stage. A linked child whose entry stage is
                // the gate follows the parent's gate release even when the note
                // has no release callback at that stage.
                let gate_stage = matches!(state.release_link, ReleaseLink::Stage(0))
                    && self.release_times[note.0.index].release_stage.is_none();
                if state.release_link == ReleaseLink::Gate
                    || gate_stage
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
                if cause == ReleaseCause::Discarded {
                    self.choke_family_now(FamilyId(self.families.id(index.get())), 0);
                } else if state.trigger == Trigger::Attack || !cause.musical() {
                    self.release_family_now(FamilyId(self.families.id(index.get())));
                }
            }
        }
        self.cancel_closed_work();
    }
}

#[cfg(test)]
mod tests;


#[cfg(test)]
mod callback_storage_budget_tests {
    use super::*;

    fn plan() -> Prepared {
        Prepared::new(48_000, vec![], vec![], 0).unwrap()
            .with_programs(vec![Program::new(vec![
                Instruction::SetLocal { local: 2, value: 7 }, Instruction::End,
            ]).unwrap()], None).unwrap()
    }

    fn limits(plan: &Prepared) -> Limits {
        Limits { behaviors: 51, behavior_cells: 51 * 3 + 2,
            behavior_fuel: 91, ..Limits::for_plan(plan, 3, 1) }
    }

    fn unlimited() -> CallbackStorageBudget {
        CallbackStorageBudget { max_payload_bytes: usize::MAX,
            retained_payload_bytes: 0, max_construction_bytes: usize::MAX }
    }

    #[test]
    fn population_preserves_ordinary_stride_remainder_and_all_other_limits() {
        let plan = plan();
        let original = limits(&plan);
        let (expanded, report) = original.with_source_callbacks(&plan,
            SourceCallbackPopulation { prepared_roots: 7, source_contexts: 7 },
            unlimited()).unwrap();
        assert_eq!(expanded.behaviors, 65);
        assert_eq!(expanded.behavior_cells, 65 * 3 + 2);
        assert_eq!(report.callbacks, 65);
        assert_eq!(report.local_cells, 195);
        assert_eq!(report.ready_entries, 68);
        assert_eq!(report.yielded_entries, 65);
        assert_eq!(report.source_task_entries, 65);
        assert_eq!(report.free_bitmap_bytes, 2 * std::mem::size_of::<u64>());
        assert_eq!(expanded.behavior_fuel, original.behavior_fuel);
        assert_eq!((expanded.notes, expanded.channels, expanded.performances,
            expanded.families, expanded.expressions, expanded.voices,
            expanded.decisions, expanded.commands, expanded.note_cells),
            (original.notes, original.channels, original.performances,
            original.families, original.expressions, original.voices,
            original.decisions, original.commands, original.note_cells));
        assert_eq!(original.behaviors, 51);
        assert_eq!(original.behavior_cells, 155);
    }

    #[test]
    fn payload_prices_each_actual_private_array_and_word_boundary() {
        let plan = plan();
        for count in [0, 1, 63, 64, 65] {
            let limits = Limits { behaviors: count, behavior_cells: count * 3,
                ..limits(&plan) };
            let report = limits.callback_storage().unwrap();
            let slots = std::alloc::Layout::array::<Slot<Continuation>>(count).unwrap().size();
            let words = if count == 0 { 0 } else if count <= 64 { 1 } else { 2 };
            let bitmap = words * std::mem::size_of::<u64>();
            let locals = count * 3 * std::mem::size_of::<i64>();
            let ready = (count + limits.notes) * std::mem::size_of::<behavior::Ready>();
            let yielded = count * std::mem::size_of::<BehaviorId>();
            let scratch = count * std::mem::size_of::<BehaviorId>();
            assert_eq!(report.arena_bytes, slots);
            assert_eq!(report.free_bitmap_bytes, bitmap);
            assert_eq!(report.local_bytes, locals);
            assert_eq!(report.ready_bytes, ready);
            assert_eq!(report.yielded_bytes, yielded);
            assert_eq!(report.source_task_bytes, scratch);
            assert_eq!(report.payload_bytes(), slots + bitmap + locals + ready + yielded + scratch);
            // Construction may need a second boxed callback array; old live
            // callback storage is charged separately and is not silently freed.
            assert_eq!(report.construction_bytes(123).unwrap(),
                123 + report.payload_bytes() + slots.max(bitmap).max(locals));
        }
    }

    #[test]
    fn exact_payload_and_construction_budget_admit_and_one_byte_less_refuses() {
        let plan = plan();
        let original = limits(&plan);
        let population = SourceCallbackPopulation { prepared_roots: 7, source_contexts: 7 };
        let (_, report) = original.with_source_callbacks(&plan, population, unlimited()).unwrap();
        let retained = original.callback_storage().unwrap().payload_bytes();
        let exact = CallbackStorageBudget { max_payload_bytes: report.payload_bytes(),
            retained_payload_bytes: retained,
            max_construction_bytes: report.construction_bytes(retained).unwrap() };
        assert!(original.with_source_callbacks(&plan, population, exact).is_ok());
        assert!(matches!(original.with_source_callbacks(&plan, population,
            CallbackStorageBudget { max_payload_bytes: exact.max_payload_bytes - 1, ..exact }),
            Err(Error::Capacity)));
        assert!(matches!(original.with_source_callbacks(&plan, population,
            CallbackStorageBudget { max_construction_bytes: exact.max_construction_bytes - 1, ..exact }),
            Err(Error::Capacity)));
        assert!(matches!(original.with_source_callbacks(&plan, population,
            CallbackStorageBudget { retained_payload_bytes: usize::MAX, ..exact }),
            Err(Error::Capacity)));
        assert_eq!(original.behaviors, 51);
        assert_eq!(original.behavior_cells, 155);
    }

    #[test]
    fn malformed_population_and_layout_overflow_refuse_without_allocation() {
        let plan = plan();
        let original = limits(&plan);
        for population in [
            SourceCallbackPopulation { prepared_roots: usize::MAX, source_contexts: 1 },
            SourceCallbackPopulation { prepared_roots: usize::MAX, source_contexts: 0 },
        ] {
            assert!(matches!(original.with_source_callbacks(&plan, population, unlimited()),
                Err(Error::Capacity)));
        }
        let overflow = Limits { behaviors: usize::MAX, behavior_cells: usize::MAX,
            ..original };
        assert_eq!(overflow.callback_storage(), Err(Error::Capacity));
        let ready_overflow = Limits { notes: usize::MAX, ..original };
        assert_eq!(ready_overflow.callback_storage(), Err(Error::Capacity));
        let too_few_locals = Limits { behavior_cells: 51 * 2, ..original };
        assert!(matches!(too_few_locals.with_source_callbacks(&plan,
            SourceCallbackPopulation { prepared_roots: 1, source_contexts: 0 }, unlimited()),
            Err(Error::Capacity)));
        assert_eq!(original.behaviors, 51);
        assert_eq!(original.behavior_cells, 155);
    }

    #[test]
    fn actual_runtime_report_includes_real_spare_capacity_before_publication() {
        let plan = plan();
        let original = limits(&plan);
        let (expanded, requested) = original.with_source_callbacks(&plan,
            SourceCallbackPopulation { prepared_roots: 7, source_contexts: 7 },
            unlimited()).unwrap();
        let runtime = Runtime::new(plan, expanded).unwrap();
        let actual = runtime.callback_storage().unwrap();
        assert_eq!(actual.callbacks, runtime.behaviors.slots.len());
        assert_eq!(actual.free_bitmap_bytes,
            runtime.behaviors.free.len() * std::mem::size_of::<u64>());
        assert_eq!(actual.local_cells, runtime.behavior_locals.len());
        assert_eq!(actual.ready_entries, runtime.behavior_ready.capacity());
        assert_eq!(actual.yielded_entries, runtime.yielded.capacity());
        assert_eq!(actual.source_task_entries, runtime.source_task_scratch.capacity());
        assert!(actual.payload_bytes() >= requested.payload_bytes());
        let exact = CallbackStorageBudget { max_payload_bytes: actual.payload_bytes(),
            retained_payload_bytes: requested.payload_bytes(),
            max_construction_bytes: actual.construction_bytes(requested.payload_bytes()).unwrap() };
        assert_eq!(exact.check(actual), Ok(()));
        assert_eq!(CallbackStorageBudget {
            max_payload_bytes: actual.payload_bytes() - 1, ..exact }.check(actual),
            Err(Error::Capacity));
    }

    #[test]
    fn zero_population_keeps_existing_caller_capacity_instead_of_baseline_sixteen() {
        let plan = plan();
        let original = Limits { behaviors: 7, behavior_cells: 21, ..limits(&plan) };
        let (expanded, requested) = original.with_source_callbacks(&plan,
            SourceCallbackPopulation::default(), unlimited()).unwrap();
        assert_eq!(expanded.behaviors, 7);
        assert_eq!(expanded.behavior_cells, 21);
        assert_eq!(requested, original.callback_storage().unwrap());
        assert_eq!(Runtime::new(plan, original).unwrap().behaviors.slots.len(), 7);
    }

    #[test]
    fn zero_arena_orphan_cell_ceiling_is_not_reinterpreted_as_new_callback_stride() {
        let empty = || Prepared::new(48_000, vec![], vec![], 0).unwrap();
        let plan = empty();
        let original = Limits { behaviors: 0, behavior_cells: 17,
            ..Limits::for_plan(&plan, 3, 1) };
        let runtime = Runtime::new(empty(), original).unwrap();
        assert_eq!(runtime.callback_storage().unwrap().local_cells, 0);
        let (same, report) = original.with_source_callbacks(&plan,
            SourceCallbackPopulation::default(), unlimited()).unwrap();
        assert_eq!(same.behaviors, 0);
        assert_eq!(same.behavior_cells, 17);
        assert_eq!(report.local_cells, 0);
        for population in [
            SourceCallbackPopulation { prepared_roots: 1, source_contexts: 0 },
            SourceCallbackPopulation { prepared_roots: 0, source_contexts: 1 },
        ] {
            assert!(matches!(original.with_source_callbacks(&plan, population, unlimited()),
                Err(Error::Capacity)));
        }
        assert_eq!(original.behaviors, 0);
        assert_eq!(original.behavior_cells, 17);
    }
}
