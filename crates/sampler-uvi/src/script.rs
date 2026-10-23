//! The Falcon/UVI Lua script runtime (design: `docs/architecture-v2/UVI_LUA.md`).
//!
//! A [`ScriptHost`] runs a program's scripts in a sandboxed Luau state on a
//! control thread, never on the audio thread. It is fed note events with a
//! time and answers with timed [`Command`]s; `wait`/`spawn` are coroutines
//! resumed by [`ScriptHost::advance`], so timing follows the host's clock.
//! Whatever the host does not model is inert and reported, not silent.

use mlua::{
    Function, Lua, LuaOptions, MultiValue, StdLib, Table, Thread, Value, Variadic, VmState,
};
use sampler_kontakt::audit::Span;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};

mod input_authority;
pub use input_authority::{ProcessorAddress,ProcessorInputDisposition,CoreAuthorityPort,AuthorityOperation,AuthorityRequest,AuthorityReply,AuthorityValue,PreparedRootTask};
pub(crate) use input_authority::{OwnerAuthorityPort,authority_pair,PreparedRootBinding,source_property_pair};
pub use input_authority::{CoreSourcePropertyPort,OwnerSourcePropertyPort,SourcePropertyOperation};
mod async_data;
mod sample_loading;
mod live_graph;
pub use live_graph::{LiveGraphQuery, LiveGraphReceipt};
pub use sample_loading::{SampleLoadStatus, SampleResource, SampleSource, SamplePath, SampleLoadTimings, SampleSourceTimings};
mod sources;
pub use sources::{ScriptOccurrence, ScriptReadError};
mod catalog_copy;
#[cfg(test)]
mod ui_invalidation_tests;
#[cfg(feature = "scan")]
pub mod diagnostics;
mod modules;
#[path = "parameters.rs"]
pub(crate) mod parameters;
mod profile;
mod ui;
pub use ui::{SavedValue, UiState, ProcessorUiState, control_id};

const PRELUDE: &str = include_str!("script_prelude.lua");
const INIT_BUDGET: &str = "uvi_lua_init unsupported: initialization work budget exceeded";
/// Where `require` finds a module: a bank's script members.
pub trait Files {
    fn sample_source(&self) -> Option<SampleSource> { None }
    fn script(&self, module: &str) -> Option<String>;
    /// A requested member that exists but cannot be admitted is distinct from
    /// absence. Existing providers retain their optional lookup contract.
    fn script_result(&self, module: &str) -> Result<Option<String>, ScriptReadError> {
        Ok(self.script(module))
    }
    fn image_size(&self, _path: &str) -> Option<sampler_ui_ir::Size> {
        None
    }
    fn script_path(&self, _module: &str) -> Option<String> {
        None
    }
    /// The impulse response at `path` (bank-rooted when it starts with `/`,
    /// else relative to the program), if the bank can supply one.
    fn impulse(&self, _path: &str) -> Option<sampler_ir::Impulse> {
        None
    }
    /// The convolution slot of the bus insert element `node`, if it has one.
    fn convolver_slot(&self, _node: usize) -> Option<usize> {
        None
    }
}

impl<T: Files> Files for std::rc::Rc<T> {
    fn sample_source(&self) -> Option<SampleSource> { (**self).sample_source() }
    fn image_size(&self, path: &str) -> Option<sampler_ui_ir::Size> {
        (**self).image_size(path)
    }
    fn script(&self, module: &str) -> Option<String> {
        (**self).script(module)
    }
    fn script_result(&self, module: &str) -> Result<Option<String>, ScriptReadError> {
        (**self).script_result(module)
    }
    fn script_path(&self, module: &str) -> Option<String> {
        (**self).script_path(module)
    }
    fn impulse(&self, path: &str) -> Option<sampler_ir::Impulse> {
        (**self).impulse(path)
    }
    fn convolver_slot(&self, node: usize) -> Option<usize> {
        (**self).convolver_slot(node)
    }
}

/// Decodes a bank resource as an impulse response.
pub type ImpulseSource = std::sync::Arc<dyn Fn(&str) -> Option<sampler_ir::Impulse> + Send + Sync>;

/// A convolution built for the audio thread to swap in. The swap leaves the
/// replaced processor inside, so the control side drops it, not the audio thread.
#[derive(Clone)]
pub struct ImpulseUpload(std::sync::Arc<std::sync::Mutex<Option<sampler_core::ConvolutionUpload>>>);

impl ImpulseUpload {
    /// Exchange convolution `slot` of `rt` with this one. Never blocks: a
    /// contended upload is skipped.
    pub fn swap(&self, rt: &mut sampler_core::Runtime, slot: usize) -> Result<(), sampler_core::Error> {
        match self.0.try_lock().as_deref_mut() {
            Ok(Some(upload)) => rt.swap_convolution(slot, upload),
            _ => Err(sampler_core::Error::InvalidInput),
        }
    }
}

impl std::fmt::Debug for ImpulseUpload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ImpulseUpload")
    }
}

impl PartialEq for ImpulseUpload {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

/// A bank's Lua members, by path.
#[derive(Clone, Default)]
pub struct Scripts {
    files: std::sync::Arc<Vec<(String, String)>>,
    occurrences: std::sync::Arc<Vec<ScriptOccurrence>>,
    impulses: Option<ImpulseSource>,
    samples: Option<SampleSource>,
    images: Option<std::sync::Arc<dyn Fn(&str) -> Option<sampler_ui_ir::Size> + Send + Sync>>,
    /// Bus insert element (node id) to its convolution slot.
    convolvers: std::collections::HashMap<usize, usize>,
}

impl Scripts {
    pub(crate) fn with_samples(mut self, source: SampleSource) -> Self { self.samples=Some(source); self }
    pub(crate) fn with_images(
        mut self,
        source: impl Fn(&str) -> Option<Vec<u8>> + Send + Sync + 'static,
    ) -> Self {
        let cache = std::sync::Mutex::new(std::collections::HashMap::new());
        self.images = Some(std::sync::Arc::new(move |path| {
            let mut cache = cache.lock().ok()?;
            *cache
                .entry(path.to_owned())
                .or_insert_with(|| source(path).as_deref().and_then(crate::resources::png_size))
        }));
        self
    }
    pub fn sources(&self) -> std::sync::Arc<Vec<(String, String)>> {
        self.files.clone()
    }

    /// Bank inventory before successful-source filtering. Failed unused
    /// members are optional; `require` reports their error only if requested.
    pub fn occurrences(&self) -> &[ScriptOccurrence] {
        &self.occurrences
    }

    pub(crate) fn record(&mut self, occurrence: ScriptOccurrence, source: Option<String>) {
        if let (Some(path), Some(source)) = (&occurrence.path, source) {
            debug_assert!(occurrence.outcome.is_ok());
            self.insert(path, source);
        }
        std::sync::Arc::make_mut(&mut self.occurrences).push(occurrence);
    }

    pub fn from_sources(files: std::sync::Arc<Vec<(String, String)>>) -> Self {
        Self {
            files,
            ..Default::default()
        }
    }

    /// The paths of the scripts held, for surveys.
    #[doc(hidden)]
    pub fn names(&self) -> Vec<&str> {
        self.files.iter().map(|(p, _)| p.as_str()).collect()
    }

    /// Let scripts load impulse responses from `source` into the convolvers `slots` names.
    pub fn with_convolution(
        mut self,
        source: ImpulseSource,
        slots: std::collections::HashMap<usize, usize>,
    ) -> Self {
        self.impulses = Some(source);
        self.convolvers = slots;
        self
    }

    pub fn insert(&mut self, path: &str, source: String) {
        std::sync::Arc::make_mut(&mut self.files)
            .push((path.to_lowercase().replace('\\', "/"), source));
    }
    /// `roots` and every member whose name `roots` (transitively) mentions: the
    /// text a program's scripts can run, without executing it.
    pub fn reachable(&self, roots: &str) -> String {
        let mut text = roots.to_lowercase();
        let mut taken = vec![false; self.files.len()];
        loop {
            let mut grew = false;
            for (n, (path, source)) in self.files.iter().enumerate() {
                let stem = path.rsplit('/').next().unwrap_or(path);
                let stem = stem.strip_suffix(".lua").unwrap_or(stem);
                if !taken[n] && !stem.is_empty() && text.contains(stem) {
                    taken[n] = true;
                    text.push('\n');
                    text.push_str(&source.to_lowercase());
                    grew = true;
                }
            }
            if !grew {
                return text;
            }
        }
    }
    fn member(&self, module: &str) -> Option<&(String, String)> {
        let module = module.to_lowercase().replace('\\', "/");
        let wanted = format!("{module}.lua");
        if let Some(exact) = self.files.iter().find(|(path, _)| *path == wanted) {
            return Some(exact);
        }
        let relative = module.replace('/', ".");
        let suffix = format!(".{relative}");
        let mut matches = self.files.iter().filter(|(path, _)| {
            let name = path.strip_suffix(".lua").unwrap_or(path).replace('/', ".");
            name == relative || name.ends_with(&suffix)
        });
        let first = matches.next()?;
        if matches.any(|(_, source)| source != &first.1) { return None; }
        Some(first)
    }
}

impl Files for Scripts {
    fn sample_source(&self) -> Option<SampleSource> { self.samples.clone() }
    fn image_size(&self, path: &str) -> Option<sampler_ui_ir::Size> {
        self.images.as_ref()?(path)
    }
    /// Exact bank members win; relative slash/dot aliases require identical source.
    fn script(&self, module: &str) -> Option<String> {
        self.script_result(module).ok().flatten()
    }
    fn script_result(&self, module: &str) -> Result<Option<String>, ScriptReadError> {
        let module = module.to_lowercase().replace('\\', "/");
        let wanted = format!("{module}.lua");
        // Exact names retain priority, including a failed exact occurrence.
        if let Some(occurrence) = self.occurrences.iter().find(|o|
            o.path.as_ref().is_some_and(|p| p.to_lowercase().replace('\\', "/") == wanted))
        {
            occurrence.outcome?;
        } else if !self.files.iter().any(|(path, _)| *path == wanted) {
            let relative = module.replace('/', ".");
            let suffix = format!(".{relative}");
            // An unreadable alias cannot be proved identical to a readable
            // one. Never silently resolve past it to a different occurrence.
            for occurrence in self.occurrences.iter() {
                if let Some(path) = &occurrence.path {
                    let path = path.to_lowercase().replace('\\', "/");
                    let name = path.strip_suffix(".lua").unwrap_or(&path).replace('/', ".");
                    if name == relative || name.ends_with(&suffix) {
                        occurrence.outcome?;
                    }
                }
            }
        }
        Ok(self.member(&module).map(|(_, source)| source.clone()))
    }
    fn script_path(&self, module: &str) -> Option<String> {
        self.member(module).map(|(path, _)| path.clone())
    }
    fn impulse(&self, path: &str) -> Option<sampler_ir::Impulse> {
        self.impulses.as_ref()?(path)
    }
    fn convolver_slot(&self, node: usize) -> Option<usize> {
        self.convolvers.get(&node).copied()
    }
}

impl Files for () {
    fn script(&self, _: &str) -> Option<String> {
        None
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Elapsed callback observation threshold; scheduler delay never rejects work.
    pub callback: Duration,
    /// Elapsed initialization observation threshold; scheduler delay never rejects work.
    pub load: Duration,
    /// Luau call/backedge checkpoints and deferred resumes per initialization.
    pub load_work: u64,
    /// Luau call/backedge checkpoints and deferred resumes per live callback.
    pub callback_work: u64,
    /// Bytes the Lua state may allocate.
    pub memory: usize,
    /// The host's sample rate, for `getSamplingRate` and the sample conversions.
    pub rate: f64,
    /// Explicit audit seed; absent from normal builds and user preferences.
    #[cfg(feature = "scan")]
    pub audit_seed: Option<u32>,
    /// Optional numeric progress observations, only for an explicitly seeded audit.
    #[cfg(feature = "scan")]
    pub audit_progress: bool,
}

/// Explicit scanner/test option, never read by ordinary plugin builds.
#[cfg(feature = "scan")]
pub fn audit_seed() -> Result<Option<u32>, String> {
    std::env::var("KONTRA_UVI_AUDIT_SEED").map_or_else(
        |e| {
            if matches!(e, std::env::VarError::NotPresent) {
                Ok(None)
            } else {
                Err("invalid UVI audit seed".into())
            }
        },
        |value| {
            value
                .parse()
                .map(Some)
                .map_err(|_| "invalid UVI audit seed".into())
        },
    )
}

impl Config {
    /// Plugin observation threshold; deterministic work bounds remain the same.
    pub fn realtime() -> Self {
        Self {
            callback: Duration::from_millis(8),
            ..Self::default()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            callback: Duration::from_millis(200),
            load: Duration::from_secs(20),
            load_work: 1 << 25,
            callback_work: 1 << 20,
            memory: 1536 << 20,
            rate: 48000.0,
            #[cfg(feature = "scan")]
            audit_seed: None,
            #[cfg(feature = "scan")]
            audit_progress: false,
        }
    }
}

/// One note a script asked for. Times are milliseconds on the host's clock.
/// 1-based layer numbers as a bit set (1..=64), so a command owns no heap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layers(pub u64);

impl Layers {
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn insert(&mut self, layer: u32) {
        if (1..=64).contains(&layer) {
            self.0 |= 1 << (layer - 1);
        }
    }

    pub fn contains(self, layer: u32) -> bool {
        (1..=64).contains(&layer) && self.0 >> (layer - 1) & 1 != 0
    }
}

impl<const N: usize> From<[u32; N]> for Layers {
    fn from(layers: [u32; N]) -> Self {
        let mut set = Self::default();
        layers.into_iter().for_each(|l| set.insert(l));
        set
    }
}

/// Actual coroutine creator, independent of the played voice's optional parent.
/// A spawned/run coroutine has a manual event creator when launched by any
/// callback; top-level/init/transport/async completion have no creator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayCreator {
    #[default]
    None,
    NoteOn(u64),
    Event,
    Widget,
}

impl PlayCreator {
    fn note(self) -> Option<u64> {
        match self { Self::NoteOn(id) => Some(id), _ => None }
    }
    fn independent(self) -> Self {
        match self { Self::None => Self::None, _ => Self::Event }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayOrigin { Helper, PostedEvent }

#[derive(Clone, Debug, PartialEq)]
pub struct Play {
    /// The voice id `playNote` returned to the script.
    pub id: u64,
    pub at_ms: f64,
    pub key: u8,
    pub velocity: u8,
    /// Milliseconds until its own release; `Some(0.0)` is manual. `None`
    /// (-1 or unset) uses the actual creator branch, never parent absence.
    pub duration_ms: Option<f64>,
    /// 1-based layers it may sound in; empty is all of them.
    pub layers: Layers,
    /// Oscillator within each keygroup as the script wrote it: 0-based (a script
    /// maps velocity ranges to `0, rr, 2*rr...` plus a round-robin offset).
    pub osc: Option<u32>,
    pub vol: f64,
    pub pan: f64,
    pub tune: f64,
    /// Actual callback/thread provenance; parent absence is not a lifetime law.
    pub creator: PlayCreator,
    pub origin: PlayOrigin,
    /// The physical onNote event that caused it, if any.
    pub parent: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    ConstantProperty {node:u32,field:sampler_core::SourceConstantField,value:sampler_core::ControlValue},
    /// An actual compiled source-owner operand. The authoritative consumer
    /// supplies the validated retained PlanId; this command is not a lease.
    OwnerControl {
        owner:sampler_ir::SourceOwnerId,
        field:sampler_core::SourceOwnerControl,
        value:sampler_core::ControlValue,
    },
    /// A typed insert write, normalized by the shared native law.
    EngineParameter {
        address: sampler_core::EngineParameterAddress,
        value: i32,
    },
    /// A rebuilt bus convolver (`Time`, `loadImpulse`, ...) for convolution `slot`.
    Impulse {
        slot: usize,
        upload: ImpulseUpload,
    },
    Play(Play),
    Release {
        id: u64,
        at_ms: f64,
    },
    /// `sendScriptModulation(id, value, glide_ms, voice)`: set (gliding to)
    /// "Script Event Modulation `id`" for one voice, or all when `voice` is None.
    Modulation {
        id: u16,
        /// Explicit start from sendScriptModulation2, committed with its ramp.
        /// None reads each existing recipient's accepted Core value.
        start: Option<f64>,
        value: f64,
        glide_ms: f64,
        voice: Option<u64>,
        at_ms: f64,
    },
    /// `changeVolume`/`changePan`/`changeTune` on one voice.
    Change {
        id: u64,
        what: Change,
        value: f64,
        relative: bool,
        immediate: bool,
        at_ms: f64,
    },
    /// `fadein`/`fadeout`/`fade`/`fade2` on one voice: its gain from `from`
    /// (its current level when `None`) to `to` over `ms`, ending the voice at
    /// silence when `kill`.
    Fade {
        id: u64,
        from: Option<f64>,
        to: f64,
        ms: f64,
        kill: bool,
        layer: u32,
        at_ms: f64,
    },
    /// A MIDI message the script generated, for the host to play into the part.
    Midi(MidiOut),
    /// `setParameter` on a program or layer: `value` is the new authored-unit
    /// value, `authored` the one the preset carries (the runtime edits offsets).
    Parameter {
        scope: Scope,
        param: Param,
        value: f64,
        authored: f64,
    },
}

/// The element a `setParameter` reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Program,
    /// The 1-based ordinal of the layer in document order.
    Layer(u32),
    Keygroup(usize),
    Oscillator(usize),
}

/// The parameters the runtime follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    /// Linear gain.
    Gain,
    Pan,
    Pitch,
    /// Program voice limit.
    Polyphony,
}

/// What `changeVolume`/`changeVolumedB`, `changePan` and `changeTune` set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Decibels,
    Pan,
    /// Semitones.
    Tune,
}

/// A MIDI 1.0 channel voice message (`status` carries the channel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidiOut {
    pub status: u8,
    pub a: u8,
    pub b: u8,
}

/// What the host left inert or could not run: feature, one example, count.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub feature: String,
    pub value: String,
    pub count: usize,
    pub setter_type_mismatch: Option<SetterTypes>,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum ParameterType {
    Int,
    Float,
    Bool,
    String,
    Nil,
    Table,
    Function,
    Thread,
    Userdata,
    Other,
}
impl ParameterType {
    fn of(name: &str) -> Self {
        match name {
            "int" => Self::Int,
            "float" => Self::Float,
            "bool" => Self::Bool,
            "string" => Self::String,
            "nil" => Self::Nil,
            "table" => Self::Table,
            "function" => Self::Function,
            "thread" => Self::Thread,
            "userdata" => Self::Userdata,
            _ => Self::Other,
        }
    }
}
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct SetterTypes {
    pub expected: ParameterType,
    pub actual: ParameterType,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SetterTypeFinding {
    pub types: SetterTypes,
    pub count: u64,
}

/// Public fault categories; messages remain in the in-memory findings only.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum FaultCategory {
    Lua,
    UiCallback,
    Budget,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FaultCounts {
    pub first: Option<FaultCategory>,
    /// First error text per phase (bounded), so a report can say why, not only which category.
    #[serde(default)]
    pub init_message: Option<String>,
    #[serde(default)]
    pub runtime_message: Option<String>,
    #[serde(default)]
    pub setter_type_mismatches: Vec<SetterTypeFinding>,
    pub init: BTreeMap<FaultCategory, u64>,
    pub runtime: BTreeMap<FaultCategory, u64>,
}

/// Scanner-only diagnostics. Raw messages stay in memory; the scanner sanitizes them.
#[cfg(feature = "scan")]
#[derive(Clone, Debug, Default)]
pub struct ScanFaults {
    pub init_count: usize,
    pub init_first: Option<String>,
    pub runtime_count: usize,
    pub runtime_first: Option<String>,
    pub budget_hits: usize,
    pub native_valid_keys: Vec<u8>,
    pub native_invalid_keys: Vec<u8>,
    pub native_key_conflicts: usize,
}

#[cfg(feature = "scan")]
static SCAN_LOAD_ERROR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
#[cfg(feature = "scan")]
pub fn scan_load_error() -> Option<String> {
    SCAN_LOAD_ERROR.lock().unwrap().take()
}
#[cfg(feature = "scan")]
pub(crate) fn scan_failed_load(error: &str) {
    *SCAN_LOAD_ERROR.lock().unwrap() = Some(error.to_owned());
}

struct Waiting {
    thread: Thread,
    due: f64,
    seq: u64,
    creator: PlayCreator,
    processor: Option<usize>,
    release: bool,
    remaining_work:u64,
    exhausted:bool,
    prepared_origin:input_authority::PreparedLuaOrigin,
}

struct Shared {
    profile: profile::Profile,
    #[cfg(feature = "scan")]
    progress: Option<std::sync::Arc<diagnostics::ScanProgress>>,
    #[cfg(feature = "scan")]
    owner_audit: std::cell::OnceCell<std::sync::Arc<diagnostics::OwnerAudit>>,
    #[cfg(feature = "scan")]
    init_api: RefCell<BTreeMap<&'static str, (u64, Duration)>>,
    #[cfg(feature = "scan")]
    audit_init: bool,
    #[cfg(feature = "scan")]
    scan: RefCell<ScanFaults>,
    initializing: Cell<bool>,
    /// Only the offaudio staged constructor suppresses legacy timer cycling.
    staged_preparation: Cell<bool>,
    /// Staged hosts are clocked only by genuine Core segments, never polling.
    canonical_tasks: bool,
    faults: RefCell<FaultCounts>,
    inspection: RefCell<Option<crate::parity::Inspection>>,
    finding_revision: Cell<u64>,
    #[cfg(feature = "scan")]
    key_declarations: RefCell<BTreeMap<u8, u8>>,
    now: Cell<f64>,
    ids: Cell<u64>,
    seq: Cell<u64>,
    /// Elapsed observation only; admission is bounded by deterministic work.
    deadline: Cell<Option<Instant>>,
    remaining_work: Cell<u64>,
    work_exhausted: Cell<bool>,
    vm_checkpoints: Cell<u64>,
    graph_nodes: Cell<usize>,
    graph_depth: Cell<usize>,
    script_bytes: Cell<usize>,
    creator: Cell<PlayCreator>,
    in_thread: Cell<bool>,
    commands: RefCell<Vec<Command>>,
    // A batch sent before UI export stays charged until the ordinary collector
    // has rebuilt stale impulses, preserving the authored queue admission limit.
    ready_command_charge: Cell<usize>,
    findings: RefCell<BTreeMap<(String, Option<SetterTypes>), Finding>>,
    waiting: RefCell<Vec<Waiting>>,
    deferred: RefCell<Vec<(Thread, MultiValue, PlayCreator, Option<usize>,input_authority::PreparedLuaOrigin)>>,
    prepared_origin:RefCell<input_authority::PreparedLuaOrigin>,
    restoring_widget_values:Cell<bool>,
    data_loads: RefCell<async_data::DataLoads>,
    sample_loads: RefCell<sample_loading::SampleLoads>,
    params: RefCell<Vec<Vec<(String, String)>>>,
    scopes: RefCell<Vec<Option<Scope>>>,
    nodes: RefCell<Vec<(Option<usize>, bool)>>,
    kinds: RefCell<Vec<String>>,
    /// Initialization writes with no live path, by (XML node, field): the
    /// element kind and the written value. The translation that follows
    /// initialization applies them (`ScriptHost::insert_overrides`).
    folded: RefCell<BTreeMap<(usize, String), (String, String)>>,
    /// Per element: the catalog fields with a DSP lane.
    live: RefCell<Vec<Vec<&'static str>>>,
    /// Globals the scripts assign somewhere: reading one that is still unset
    /// answers nil, as in Lua; only unknown API names answer a stub.
    processor_view: RefCell<Rc<input_authority::ProcessorView>>,
    processor: Cell<Option<usize>>,
    context_failed:Cell<bool>,
    processors: RefCell<Vec<input_authority::ProcessorContext>>,
    authority:RefCell<Option<OwnerAuthorityPort>>,
    source_properties:RefCell<Option<OwnerSourcePropertyPort>>,
    constant_owners:RefCell<Vec<(u32,sampler_ir::SourceOwnerId)>>,
    external_projection:RefCell<Option<Rc<input_authority::ControllerProjection>>>,
    external_waiting:RefCell<Vec<input_authority::ExternalWaiting>>,
    prepared_roots:RefCell<Option<Vec<input_authority::PreparedLuaRoot>>>,
    owner_shapes:RefCell<Vec<input_authority::OwnerShape>>,
    pristine_globals: RefCell<Option<mlua::RegistryKey>>,
    /// Names the scripts call or index as objects: unmodeled API when unset.
    /// The preset's saved widget values and table data (ScriptProcessor
    /// attributes and ScriptData), by widget name.
    /// Each scripted bus convolver's source impulse, by element node.
    impulses: RefCell<std::collections::HashMap<usize, sampler_ir::Impulse>>,
    /// Uploads sent to the audio thread; dropped here once it has let go.
    retired: RefCell<Vec<ImpulseUpload>>,
    /// Convolver elements whose impulse fields changed since commands were
    /// last taken. A script sets fields one by one (Time, Damping, Width...)
    /// and each rebuild costs a whole impulse, so they rebuild once per batch.
    stale_impulses: RefCell<Vec<usize>>,
    files: Box<dyn Files>,
    config: Config,
    tempo: Cell<f64>,
    /// Last physical note-on time per key, retained through release callbacks.
    last_note_on: RefCell<[Option<f64>; 128]>,
    held: RefCell<BTreeMap<u64, u8>>,
    voices: RefCell<std::collections::BTreeSet<u64>>,
    playing: Cell<bool>,
    beat_anchor: Cell<(f64, f64)>,
}

struct InspectionBudget<'a> {
    shared: &'a Shared,
    saved: Option<(u64, bool, Option<Instant>)>,
}

impl Drop for InspectionBudget<'_> {
    fn drop(&mut self) {
        if let Some((work, exhausted, deadline)) = self.saved {
            if self.shared.work_exhausted.get() {
                self.shared
                    .find("lua error", "work budget exceeded during UI snapshot");
            }
            self.shared.remaining_work.set(work);
            self.shared.work_exhausted.set(exhausted);
            self.shared.deadline.set(deadline);
            #[cfg(feature = "scan")]
            self.shared.observe_work();
        }
    }
}

#[cfg(feature = "scan")]
struct ApiTimer<'a>(&'a Shared, &'static str, Option<Instant>);
#[cfg(feature = "scan")]
impl Drop for ApiTimer<'_> {
    fn drop(&mut self) {
        if let Some(start) = self.2 {
            let mut timings = self.0.init_api.borrow_mut();
            let entry = timings.entry(self.1).or_default();
            entry.0 += 1;
            entry.1 += start.elapsed();
        }
    }
}
impl Shared {
    #[cfg(feature = "scan")]
    fn observe_work(&self) {
        if let Some(progress) = &self.progress {
            progress.work(
                self.remaining_work.get(),
                self.vm_checkpoints.get(),
                self.work_exhausted.get(),
            );
        }
    }

    fn inspection_budget(&self) -> InspectionBudget<'_> {
        // Readback gets load-sized work without spending or refilling live work.
        let saved = (!self.initializing.get()).then(|| {
            (
                self.remaining_work.replace(self.config.load_work),
                self.work_exhausted.replace(false),
                self.deadline
                    .replace(Some(Instant::now() + self.config.load)),
            )
        });
        InspectionBudget {
            shared: self,
            saved,
        }
    }

    #[cfg(feature = "scan")]
    fn api_timer(&self, name: &'static str) -> ApiTimer<'_> {
        ApiTimer(
            self,
            name,
            ((self.audit_init || self.profile.enabled()) && self.initializing.get())
                .then(Instant::now),
        )
    }
}

impl Shared {
    fn find(&self, feature: &str, value: &str) {
        self.finding_revision
            .set(self.finding_revision.get().saturating_add(1));
        if matches!(feature, "lua error" | "lua UI callback") {
            if let Some(inspection) = self.inspection.borrow_mut().as_mut() {
                inspection.fault(value);
            }
            let category = if value.contains("budget exceeded") {
                FaultCategory::Budget
            } else if feature == "lua UI callback" {
                FaultCategory::UiCallback
            } else {
                FaultCategory::Lua
            };
            let mut faults = self.faults.borrow_mut();
            faults.first.get_or_insert(category);
            let counts = if self.initializing.get() {
                &mut faults.init
            } else {
                &mut faults.runtime
            };
            let count = counts.entry(category).or_default();
            *count = count.saturating_add(1);
            let message = if self.initializing.get() {
                &mut faults.init_message
            } else {
                &mut faults.runtime_message
            };
            if message.is_none() {
                *message = Some(value.chars().take(200).collect());
            }
        }
        #[cfg(feature = "scan")]
        if matches!(feature, "lua error" | "lua UI callback") {
            let mut scan = self.scan.borrow_mut();
            if self.initializing.get() {
                scan.init_count += 1;
                scan.init_first.get_or_insert_with(|| value.to_owned());
            } else {
                scan.runtime_count += 1;
                scan.runtime_first.get_or_insert_with(|| value.to_owned());
            }
            scan.budget_hits += usize::from(value.contains("budget exceeded"));
        }
        let mut findings = self.findings.borrow_mut();
        let key = (feature.to_owned(), None);
        match findings.get_mut(&key) {
            Some(f) => f.count = f.count.saturating_add(1),
            None => {
                if findings.len() < 2000 {
                    findings.insert(
                        key,
                        Finding {
                            feature: feature.to_owned(),
                            value: value.to_owned(),
                            count: 1,
                            setter_type_mismatch: None,
                        },
                    );
                }
            }
        }
    }

    /// Fold an initialization write that has no live path into the program:
    /// the translation after initialization reads it from the patched XML.
    /// False when the element has no field `name` to carry it.
    fn fold(&self, id: usize, name: &str, value: f64) -> bool {
        let kind = self.kinds.borrow().get(id).cloned().unwrap_or_default();
        let mut params = self.params.borrow_mut();
        let Some(params) = params.get_mut(id) else {
            return false;
        };
        let retained = params.iter().any(|(k, _)| k == name);
        if !retained && !parameters::definitions(&kind).iter().any(|p| p.name == name) {
            return false;
        }
        let Some(&(Some(node), _)) = self.nodes.borrow().get(id) else {
            return false;
        };
        let text = if value.fract() == 0. && value.abs() < 1e15 {
            (value as i64).to_string()
        } else {
            value.to_string()
        };
        match params.iter_mut().find(|(k, _)| k == name) {
            Some((_, v)) => *v = text.clone(),
            None => params.push((name.to_owned(), text.clone())),
        }
        self.folded.borrow_mut().insert((node, name.to_owned()), (kind, text));
        true
    }

    fn setter_mismatch(&self, types: SetterTypes) {
        self.finding_revision
            .set(self.finding_revision.get().saturating_add(1));
        let mut findings = self.findings.borrow_mut();
        let finding = findings
            .entry(("setter_type_mismatch".into(), Some(types)))
            .or_insert_with(|| Finding {
                feature: "setter_type_mismatch".into(),
                value: String::new(),
                count: 0,
                setter_type_mismatch: Some(types),
            });
        finding.count = finding.count.saturating_add(1);
        let mut faults = self.faults.borrow_mut();
        if let Some(f) = faults
            .setter_type_mismatches
            .iter_mut()
            .find(|f| f.types == types)
        {
            f.count = f.count.saturating_add(1);
        } else {
            faults
                .setter_type_mismatches
                .push(SetterTypeFinding { types, count: 1 });
        }
    }

    /// All initialization phases share one allowance, including spawned work.
    fn arm(&self, elapsed: Duration, work: u64) {
        if !self.initializing.get() {
            self.deadline.set(Some(Instant::now() + elapsed));
            self.remaining_work.set(work);
            self.work_exhausted.set(false);
            #[cfg(feature = "scan")]
            self.observe_work();
        }
    }

    fn consume_work(&self) -> mlua::Result<()> {
        if let Some(left) = self.remaining_work.get().checked_sub(1) {
            self.remaining_work.set(left);
            #[cfg(feature = "scan")]
            self.observe_work();
            Ok(())
        } else {
            self.work_exhausted.set(true);
            #[cfg(feature = "scan")]
            self.observe_work();
            Err(mlua::Error::runtime("work budget exceeded"))
        }
    }

    fn next_id(&self) -> u64 {
        self.ids.set(self.ids.get() + 1);
        self.ids.get()
    }

    /// Rebuild the impulses whose fields changed, once each.
    fn rebuild_stale_impulses(&self) {
        let stale = std::mem::take(&mut *self.stale_impulses.borrow_mut());
        for id in stale {
            self.rebuild_impulse(id, None);
        }
    }

    /// Rebuild element `id`'s bus convolver from its parameters and source
    /// impulse (`path` replaces the source) and queue the swap. False when the
    /// element has no convolution slot or the impulse cannot be had.
    fn rebuild_impulse(&self, id: usize, path: Option<&str>) -> bool {
        let Some(node) = self.nodes.borrow().get(id).and_then(|n| n.0) else {
            return false;
        };
        let Some(slot) = self.files.convolver_slot(node) else {
            self.find("lua loadImpulse on an insert without a bus convolution", "");
            return false;
        };
        let kind = self.kinds.borrow().get(id).cloned().unwrap_or_default();
        let read = |name: &str| -> f64 {
            let params = self.params.borrow();
            let retained = params.get(id).and_then(|p| p.iter().find(|(k, _)| k == name));
            retained
                .and_then(|(_, v)| match v.as_str() {
                    "true" => Some(1.0),
                    "false" => Some(0.0),
                    v => v.parse().ok(),
                })
                .or_else(|| {
                    parameters::definitions(&kind)
                        .iter()
                        .find(|p| p.name == name)
                        .map(|p| p.default)
                })
                .unwrap_or(0.0)
        };
        let source = match path {
            Some(path) => self.files.impulse(path),
            None if self.impulses.borrow().contains_key(&node) => None,
            None => {
                let named = self.params.borrow().get(id).and_then(|p| {
                    p.iter().find(|(k, _)| k == "SamplePath").map(|(_, v)| v.clone())
                });
                match named.filter(|p| !p.is_empty()) {
                    Some(path) => self.files.impulse(&path),
                    None => Some(sampler_ir::Impulse {
                        rate: 48000,
                        left: vec![1.0],
                        right: vec![1.0],
                        asset: None,
                    }),
                }
            }
        };
        let mut impulses = self.impulses.borrow_mut();
        match source {
            Some(source) => {
                impulses.insert(node, source);
            }
            None if path.is_some() || !impulses.contains_key(&node) => return false,
            None => {}
        }
        let base = &impulses[&node];
        let mut shape = sampler_core::neutral_impulse_shape();
        shape.normalize = read("NormalizePower") != 0.0;
        if kind == "SampledReverb" {
            shape.sampled_reverb = true;
            shape.time = read("Time");
            shape.damping_low = read("DampingLow");
            shape.damping_high = read("DampingHigh");
            shape.pre_delay_ms = read("PreDelay");
            shape.width = read("Width");
        }
        let (left, right) = sampler_core::shape_impulse(base.rate, &base.left, &base.right, &shape);
        let shaped = sampler_ir::Impulse { rate: base.rate, left, right, asset: None };
        drop(impulses);
        let Ok(upload) = sampler_core::ConvolutionUpload::from_ir(
            &shaped,
            self.config.rate as u32,
            read("Dry"),
            read("Wet"),
        ) else {
            return false;
        };
        let upload = ImpulseUpload(std::sync::Arc::new(std::sync::Mutex::new(Some(upload))));
        let mut retired = self.retired.borrow_mut();
        // Uploads the audio thread has let go of hold the replaced processors.
        retired.retain(|u| std::sync::Arc::strong_count(&u.0) > 1);
        retired.push(upload.clone());
        drop(retired);
        self.command(Command::Impulse { slot, upload })
    }

    fn command(&self, command: Command) -> bool {
        if self.context_failed.get(){return false;}
        if self.authority.borrow().as_ref().and_then(|owner|owner.current()).is_some(){
            let accepted=matches!(self.authority.borrow_mut().as_mut().map(|owner|owner.call(AuthorityOperation::Command(command))),Some(Ok(AuthorityValue::Applied)));
            if !accepted{self.find("external command refused","");}
            return accepted;
        }
        if self.staged_preparation.get(){
            // Only values which the initialized XML exporter folds are preparation
            // effects. No executable command is queued before actual Core authority.
            if matches!(&command,Command::EngineParameter{..}|Command::Parameter{..}) {return true;}
            self.find("staged execution command refused","no actual Core task");return false;
        }
        if self.canonical_tasks||self.authority.borrow().is_some(){
            self.find("external command refused","no actual Running Core turn");return false;
        }
        let mut commands = self.commands.borrow_mut();
        if self.command_count(commands.len()) < 1 << 16 {
            commands.push(command);
            true
        } else {
            drop(commands);
            self.find("command queue full, commands dropped", "");
            false
        }
    }

    fn constant_property_field(&self,id:usize,name:&str)->bool{
        self.kinds.borrow().get(id).is_some_and(|kind|kind=="ConstantModulation")&&matches!(name,"Value"|"Style")
            &&self.params.borrow().get(id).is_some_and(|params|params.iter().any(|(field,_)|field=="ScriptLive"))
    }
    fn command_count(&self, queued: usize) -> usize {
        queued.saturating_add(self.ready_command_charge.get())
    }
}

/// A resource path as the calling script means it: relative to that script's
/// directory in its bank, written bank-rooted. Volume-rooted and empty paths
/// stay as they are.
fn script_relative(lua: &Lua, path: String) -> String {
    // v1 4bffbb18:src/uvi/host.rs retains empty artwork/font assignments.
    // They clear a resource; resolving them to a module directory creates a phantom asset.
    if path.is_empty() || path.starts_with(['/', '$']) {
        return path;
    }
    script_directory(lua).map_or_else(|| path.clone(), |base| format!("/{base}/{path}"))
}

fn script_directory(lua: &Lua) -> Option<String> {
    for level in 0..32 {
        let Some(source) = lua.inspect_stack(level, |d| d.source().source.map(|s| s.into_owned()))
        else {
            break;
        };
        if let Some(source) = source {
            let source = source.trim_start_matches('@');
            if source.ends_with(".lua")
                && let Some((base, _)) = source.rsplit_once('/')
            {
                return Some(base.to_owned());
            }
        }
    }
    None
}

/// The catalog fields that shape a bus convolver's impulse response.
pub(crate) const IMPULSE_FIELDS: [&str; 9] = [
    "Dry",
    "Wet",
    "Time",
    "DampingLow",
    "DampingHigh",
    "PreDelay",
    "Width",
    "NormalizePower",
    "UseWindowAtOriginalSize",
];

pub struct ScriptHost {
    lua: Lua,
    shared: Rc<Shared>,
}

impl Drop for ScriptHost {
    fn drop(&mut self) {
        self.shared.data_loads.borrow_mut().stop();
    }
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => Some(*n),
        Value::Integer(n) => Some(*n as f64),
        _ => None,
    }
}

fn field(table: &Table, name: &str) -> Option<f64> {
    table.get::<Value>(name).ok().as_ref().and_then(number)
}

fn lua_error(e: mlua::Error) -> String {
    // The message plus the innermost frames (file:line), not the whole trace.
    let text = e.to_string();
    let mut lines = text.lines();
    let mut out = lines.next().unwrap_or("").to_owned();
    for frame in lines
        .filter(|l| l.contains(".lua") || l.contains("[string"))
        .take(4)
    {
        out.push_str(" < ");
        out.push_str(
            frame
                .trim()
                .split(':')
                .take(2)
                .collect::<Vec<_>>()
                .join(":")
                .as_str(),
        );
    }
    out
}

/// The elements of a program the scripts can reach (`Program.layers[i]`...).
struct Tree {
    params: Vec<Vec<(String, String)>>,
    scopes: Vec<Option<Scope>>,
    /// Per element: admitted XML identity (None for host placeholders), and insert ownership.
    nodes: Vec<(Option<usize>, bool)>,
    kinds: Vec<String>,
    live: Vec<Vec<&'static str>>,
    layers: u32,
}


fn element(
    lua: &Lua,
    tree: &mut Tree,
    node: roxmltree::Node,
    parent: Option<&Table>,
    insert: bool,
    shared: &Shared,
    depth: usize,
    class: &Table,
    list_class: &Table,
) -> mlua::Result<Table> {
    if shared.graph_nodes.get() >= 1 << 18 || depth > 192 {
        return Err(mlua::Error::runtime(
            "uvi_lua_init unsupported: graph node/depth budget exceeded",
        ));
    }
    let table = lua.create_table()?;
    shared.graph_nodes.set(shared.graph_nodes.get() + 1);
    shared.graph_depth.set(shared.graph_depth.get().max(depth));
    let id = tree.params.len();
    tree.params.push(
        node.attributes()
            .map(|a| (a.name().to_owned(), a.value().to_owned()))
            .collect(),
    );
    tree.nodes.push((Some(node.id().get_usize()), insert));
    tree.kinds.push(node.tag_name().name().to_owned());
    tree.live.push(crate::engine_parameters::live_names(node));
    if node.has_tag_name("ConstantModulation")&&node.attribute("ScriptLive").is_some(){
        let definition=u32::try_from(node.id().get_usize()).map_err(|_|mlua::Error::runtime("Constant definition overflow"))?;
        let parent=node.ancestors().skip(1).find(|n|n.is_element()&&(matches!(n.tag_name().name(),"Program"|"Layer"|"Keygroup"|"SamplePlayer"|"AuxEffect")||n.parent().is_some_and(|p|p.has_tag_name("Oscillators")))).ok_or_else(||mlua::Error::runtime("Constant structural owner missing"))?;
        let parent=u32::try_from(parent.id().get_usize()).map_err(|_|mlua::Error::runtime("Constant owner overflow"))?;
        shared.constant_owners.borrow_mut().push((definition,sampler_ir::SourceOwnerId::UviNode(parent)));
    }
    tree.scopes.push(match node.tag_name().name() {
        "Program" => Some(Scope::Program),
        "Layer" => {
            tree.layers += 1;
            Some(Scope::Layer(tree.layers))
        }
        "Keygroup" => Some(Scope::Keygroup(node.id().get_usize())),
        "SamplePlayer" => Some(Scope::Oscillator(node.id().get_usize())),
        _ => None,
    });
    table.raw_set("__id", id)?;
    table.raw_set("id", id)?;
    table.raw_set("type", node.tag_name().name())?;
    table.raw_set("name", node.attribute("Name").unwrap_or_default())?;
    table.raw_set(
        "displayName",
        node.attribute("DisplayName")
            .or_else(|| node.attribute("Name"))
            .unwrap_or_default(),
    )?;
    table.raw_set("bypass", node.attribute("Bypass") == Some("1"))?;
    if let Some(parent) = parent {
        table.raw_set("parent", parent.clone())?;
    }
    table.set_metatable(Some(class.clone()))?;
    let fields = [
        "layers",
        "keygroups",
        "oscillators",
        "inserts",
        "auxs",
        "sends",
        "modulations",
        "eventProcessors",
        "connections",
    ];
    let mut lists: [Option<Table>; 9] = std::array::from_fn(|_| None);
    for container in node.children().filter(|n| n.is_element()) {
        let field = match container.tag_name().name() {
            "Layers" => "layers",
            "Keygroups" => "keygroups",
            "Oscillators" => "oscillators",
            "Inserts" => "inserts",
            "Auxs" | "Chains" => "auxs",
            "BusRouters" => "sends",
            "ControlSignalSources" => "modulations",
            "EventProcessors" => "eventProcessors",
            "Connections" => "connections",
            _ => continue,
        };
        let list = lua.create_table()?;
        list.set_metatable(Some(list_class.clone()))?;
        for child in container.children().filter(|n| n.is_element()) {
            list.raw_push(element(
                lua,
                tree,
                child,
                Some(&table),
                field == "inserts",
                shared,
                depth + 1,
                class,
                list_class,
            )?)?;
        }
        table.raw_set(field, list.clone())?;
        lists[fields.iter().position(|f| *f == field).unwrap()] = Some(list);
    }
    let children = lua.create_table()?;
    let synthesis = (matches!(node.tag_name().name(), "Program" | "Layer" | "Keygroup")
        || lists[0].is_some()
        || lists[1].is_some())
    .then(|| lua.create_table())
    .transpose()?;
    for (field, list) in fields.into_iter().zip(lists) {
        // v1 host.rs builds only collections present on leaf processors.
        let Some(list) = list else { continue };
        for child in list.sequence_values::<Table>().flatten() {
            let name: String = child.raw_get("name")?;
            if !name.is_empty() {
                children.raw_set(name, child.clone())?;
            }
            if matches!(field, "layers" | "keygroups")
                && let Some(synthesis) = &synthesis
            {
                synthesis.raw_push(child.clone())?;
            }
            children.raw_push(child)?;
        }
    }
    table.raw_set("children", children)?;
    if let Some(synthesis) = synthesis {
        table.raw_set("synthChildren", synthesis)?;
    }
    if let Some(list) = table.raw_get::<Option<Table>>("modulations")? {
        table.raw_set("mods", list)?;
    }
    Ok(table)
}

impl Shared {
    /// Remember the names a script assigns at the start of a line (`name = ...`,
    /// `function name`), a cheap stand-in for a parse.
    fn note_assigned(&self, source: &str) {
        self.script_bytes
            .set(self.script_bytes.get().saturating_add(source.len()));
        self.note_called(source);
        let view=self.processor_view.borrow().clone();
        let mut names = view.assigned.borrow_mut();
        for line in source.lines() {
            let line = line.trim_start();
            let (line, function) = match line.strip_prefix("function ") {
                Some(rest) => (rest.trim_start(), true),
                None => (line, false),
            };
            let end = line
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(line.len());
            let (name, rest) = line.split_at(end);
            let rest = rest.trim_start();
            let defined = if function {
                rest.starts_with('(')
            } else {
                rest.starts_with('=') && !rest.starts_with("==")
            };
            if defined && !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) {
                names.insert(name.to_owned());
            }
        }
    }
}

impl Shared {
    /// Remember the plain names followed by a call or a field access.
    fn note_called(&self, source: &str) {
        let view=self.processor_view.borrow().clone();
        let mut names = view.called.borrow_mut();
        let b = source.as_bytes();
        let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
        let mut i = 0;
        while i < b.len() {
            if !(b[i].is_ascii_alphabetic() || b[i] == b'_')
                || (i > 0 && (word(b[i - 1]) || b[i - 1] == b'.' || b[i - 1] == b':'))
            {
                i += 1;
                continue;
            }
            let end = (i..b.len()).find(|&j| !word(b[j])).unwrap_or(b.len());
            let next = b[end..]
                .iter()
                .position(|c| !c.is_ascii_whitespace())
                .map(|k| end + k);
            let used = next.is_some_and(|n| match b[n] {
                b'(' | b'{' | b':' => true,
                b'.' => b.get(n + 1) != Some(&b'.'),
                _ => false,
            });
            if used {
                names.insert(source[i..end].to_owned());
            }
            i = end;
        }
    }
}

impl ScriptHost {
    /// Run the scripts of the program `xml` (its `ScriptProcessor`s). Fails when
    /// the sandbox cannot be built or a script does not load.
    pub fn new(xml: &str, files: impl Files + 'static, config: Config) -> Result<Self, String> {
        Self::new_with_ui_state(xml, files, config, None)
    }

    pub fn new_with_ui_state(
        xml: &str,
        files: impl Files + 'static,
        config: Config,
        state: Option<&UiState>,
    ) -> Result<Self, String> {
        Self::create(xml, files, config, state, false)
    }

    /// Offaudio processor preparation: main chunk, saved widgets and synchronous
    /// onLoad only. No onInit callback or legacy deferred timer is executed.
    /// First-F Init must be admitted through the actual prepared Core task entry.
    pub fn new_staged_with_ui_state(
        xml:&str, files:impl Files+'static, config:Config, state:Option<&UiState>,
    )->Result<Self,String>{
        Self::create_profiled_mode(xml,files,config,state,false,false,true)
    }

    /// Inspector-only host: real Luau statement-line coverage, including init.
    /// Ordinary constructors compile without coverage instructions. Inspection
    /// lasts for this host's lifetime; recreate normally to turn it off.
    pub fn new_inspected(
        xml: &str,
        files: impl Files + 'static,
        config: Config,
    ) -> Result<Self, String> {
        Self::create(xml, files, config, None, true)
    }

    /// Explicit source-free load clocks and bounded VM safepoint attribution.
    /// Normal plugin construction never enables this profiler.
    pub fn new_profiled(
        xml: &str,
        files: impl Files + 'static,
        config: Config,
        coverage: bool,
    ) -> Result<Self, String> {
        Self::create_profiled(xml, files, config, None, coverage, true)
    }

    fn create(
        xml: &str,
        files: impl Files + 'static,
        config: Config,
        state: Option<&UiState>,
        inspect: bool,
    ) -> Result<Self, String> {
        Self::create_profiled(xml, files, config, state, inspect, false)
    }

    fn create_profiled(
        xml: &str,
        files: impl Files + 'static,
        config: Config,
        state: Option<&UiState>,
        inspect: bool,
        profiled: bool,
    ) -> Result<Self, String> {
        Self::create_profiled_mode(xml,files,config,state,inspect,profiled,false)
    }

    fn create_profiled_mode(
        xml:&str, files:impl Files+'static, config:Config, state:Option<&UiState>,
        inspect:bool, profiled:bool, staged:bool,
    )->Result<Self,String>{
        let profile = profile::Profile::new(profiled);
        let _total = profile.span("host_create");
        let options = roxmltree::ParsingOptions {
            nodes_limit: 4_000_000,
            ..Default::default()
        };
        let doc = {
            let _profile = profile.span("xml_parse");
            let _span = Span::new("uvi_lua_xml");
            roxmltree::Document::parse_with_options(xml, options).map_err(|e| e.to_string())?
        };
        let lua = {
            let _profile = profile.span("vm_create");
            let _span = Span::new("uvi_lua_vm");
            Lua::new_with(
                StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::COROUTINE | StdLib::BIT,
                LuaOptions::new(),
            )
            .map_err(lua_error)?
        };
        lua.set_memory_limit(config.memory).map_err(lua_error)?;
        #[cfg(feature = "scan")]
        if let Some(seed) = config.audit_seed {
            let math: Table = lua.globals().get("math").map_err(lua_error)?;
            math.get::<Function>("randomseed")
                .map_err(lua_error)?
                .call::<()>(seed)
                .map_err(lua_error)?;
        }

        let shared = Rc::new(Shared {
            profile,
            #[cfg(feature = "scan")]
            progress: (config.audit_seed.is_some() && config.audit_progress)
                .then(|| std::sync::Arc::new(diagnostics::ScanProgress::new(config.load_work))),
            #[cfg(feature = "scan")]
            owner_audit: std::cell::OnceCell::new(),
            #[cfg(feature = "scan")]
            init_api: RefCell::new(BTreeMap::new()),
            #[cfg(feature = "scan")]
            audit_init: std::env::var_os("KONTRA_AUDIT_LOAD").is_some(),
            #[cfg(feature = "scan")]
            scan: RefCell::new(ScanFaults::default()),
            initializing: Cell::new(true),
            staged_preparation:Cell::new(staged),
            canonical_tasks:staged,
            faults: RefCell::new(FaultCounts::default()),
            inspection: RefCell::new(inspect.then(crate::parity::Inspection::default)),
            finding_revision: Cell::new(0),
            #[cfg(feature = "scan")]
            key_declarations: RefCell::new(BTreeMap::new()),
            now: Cell::new(0.0),
            ids: Cell::new(1 << 32),
            seq: Cell::new(0),
            deadline: Cell::new(Some(Instant::now() + config.load)),
            remaining_work: Cell::new(config.load_work),
            work_exhausted: Cell::new(false),
            vm_checkpoints: Cell::new(0),
            graph_nodes: Cell::new(0),
            graph_depth: Cell::new(0),
            script_bytes: Cell::new(PRELUDE.len()),
            creator: Cell::new(PlayCreator::None),
            in_thread: Cell::new(false),
            commands: RefCell::new(Vec::new()),
            ready_command_charge: Cell::new(0),
            findings: RefCell::new(BTreeMap::new()),
            waiting: RefCell::new(Vec::new()),
            deferred: RefCell::new(Vec::new()),
            prepared_origin:RefCell::new(input_authority::PreparedLuaOrigin::None),
            restoring_widget_values:Cell::new(false),
            data_loads: RefCell::new(async_data::DataLoads::default()),
            sample_loads: RefCell::new(sample_loading::SampleLoads::new(&doc, files.sample_source(), config.rate as u32)),
            params: RefCell::new(Vec::new()),
            scopes: RefCell::new(Vec::new()),
            nodes: RefCell::new(Vec::new()),
            kinds: RefCell::new(Vec::new()),
            folded: RefCell::new(BTreeMap::new()),
            live: RefCell::new(Vec::new()),
            processor_view: RefCell::new(Rc::new(input_authority::ProcessorView::default())),
            processor: Cell::new(None),context_failed:Cell::new(false),
            processors: RefCell::new(Vec::new()),
            authority:RefCell::new(None),
            source_properties:RefCell::new(None),
            constant_owners:RefCell::new(Vec::new()),
            external_projection:RefCell::new(None),
            external_waiting:RefCell::new(Vec::new()),prepared_roots:RefCell::new(None),
            owner_shapes:RefCell::new(Vec::new()),
            pristine_globals: RefCell::new(None),
            impulses: RefCell::new(Default::default()),
            retired: RefCell::new(Vec::new()),
            stale_impulses: RefCell::new(Vec::new()),
            files: Box::new(files),
            config,
            tempo: Cell::new(120.0),
            last_note_on: RefCell::new([None; 128]),
            held: RefCell::new(Default::default()),
            voices: RefCell::new(Default::default()),
            playing: Cell::new(false),
            beat_anchor: Cell::new((0., 0.)),
        });
        let pristine = input_authority::fresh_environment(&lua, &lua.globals()).map_err(lua_error)?;
        *shared.pristine_globals.borrow_mut()=Some(lua.create_registry_value(pristine).map_err(lua_error)?);
        let host = Self { lua, shared };
        let mut trace = crate::LoadTrace::start();
        trace.phase("host_setup");
        let installed = host.install().map_err(lua_error);
        trace.phase("install_api");
        let built = installed.and_then(|_| host.build_program(&doc).map_err(lua_error));
        trace.phase("build_program");
        let initialized = built.and_then(|_| host.load_scripts(&doc, state));
        trace.phase("load_scripts");
        #[cfg(feature = "scan")]
        for (name, (calls, elapsed)) in host.shared.init_api.borrow().iter() {
            eprintln!(
                "AUDIT {{\"stage\":\"{name}\",\"ms\":{},\"calls\":{calls}}}",
                elapsed.as_secs_f64() * 1000.
            );
        }
        if std::env::var_os("KONTRA_AUDIT_LOAD").is_some() {
            eprintln!(
                "AUDIT {}",
                serde_json::json!({"stage":"uvi_lua_work",
                "xml_bytes":xml.len(),"script_bytes":host.shared.script_bytes.get(),
                "graph_nodes":host.shared.graph_nodes.get(),"graph_depth":host.shared.graph_depth.get(),
                "vm_checkpoints":host.shared.vm_checkpoints.get(),
                "work_limit":config.load_work,"work_remaining":host.shared.remaining_work.get(),
                "work_exhausted":host.shared.work_exhausted.get(),"memory_bytes":host.lua.used_memory(),
                "wall_limit_ms":config.load.as_millis(),
                "wall_expired":host.shared.deadline.get().is_some_and(|d| Instant::now() >= d),
                "initialized":initialized.is_ok()})
            );
        }
        if host.shared.work_exhausted.get() {
            return Err(INIT_BUDGET.into());
        }
        initialized?;
        // Refresh only policy/recipe writes accepted during initialization.
        // Pending task identities/order/cancellation remain unchanged; no
        // worker/control service exists before the owner is returned.
        let refresh = host.shared.folded.borrow().iter().any(|((_,field),(kind,_))|
            kind=="Program" && field=="Streaming" || kind=="SamplePlayer" && matches!(field.as_str(),"AllowStreaming"|"Reverse"));
        if refresh {
            let final_xml = crate::apply_overrides(xml, &host.insert_overrides());
            let final_doc = crate::parse_program_xml(&final_xml).map_err(|e|e.to_string())?;
            host.shared.sample_loads.borrow_mut().refresh_initial(&final_doc)?;
        }
        trace.phase("refresh_check");
        host.shared.initializing.set(false);
        host.shared.staged_preparation.set(false);
        Ok(host)
    }

    pub fn initialization_profile(&self) -> serde_json::Value {
        let mut report = self
            .shared
            .profile
            .report(self.shared.inspection.borrow().is_some());
        if report.is_null() {
            return report;
        }
        #[cfg(feature = "scan")]
        {
            report["native_apis"]=serde_json::json!(self.shared.init_api.borrow().iter().map(|(phase,(calls,elapsed))|serde_json::json!({"phase":phase,"calls":calls,"elapsed_ms":elapsed.as_secs_f64()*1000.})).collect::<Vec<_>>());
        }
        report["work"] = serde_json::json!({"vm_checkpoints":self.shared.vm_checkpoints.get(),"graph_nodes":self.shared.graph_nodes.get(),"graph_depth":self.shared.graph_depth.get(),"script_bytes":self.shared.script_bytes.get(),"lua_memory_bytes":self.lua.used_memory()});
        report
    }

    pub(crate) fn profile_span(&self, name: &'static str) -> profile::Span {
        self.shared.profile.span(name)
    }
    pub(crate) fn profile_record(&self, name: &'static str, elapsed_ms: f64) {
        self.shared.profile.record(name, elapsed_ms);
    }

    fn build_program(&self, doc: &roxmltree::Document) -> mlua::Result<()> {
        let _profile = self.shared.profile.span("program_object_graph");
        let _span = Span::new("uvi_lua_graph");
        let program = doc
            .descendants()
            .find(|n| n.has_tag_name("Program"))
            .ok_or_else(|| mlua::Error::runtime("no Program"))?;
        let mut tree = Tree {
            params: Vec::new(),
            scopes: Vec::new(),
            nodes: Vec::new(),
            kinds: Vec::new(),
            live: Vec::new(),
            layers: 0,
        };
        // Port v1 host.rs's shared metatable handles outside the object loop.
        let class: Table = self.lua.globals().raw_get("__element_mt")?;
        let list_class: Table = self.lua.globals().raw_get("__list_mt")?;
        let root = element(
            &self.lua,
            &mut tree,
            program,
            None,
            false,
            &self.shared,
            1,
            &class,
            &list_class,
        )?;
        // The part the program sits in (MidiChannel, MidiInput...). Its level and
        // expression controllers are real connections on Gain, which scripts such as
        // hornScript3 query (and bypass) unguarded; the part itself stays inert.
        let part_xml = roxmltree::Document::parse(
            r#"<Part Name="" Gain="1" Pan="0" Bypass="0"><Connections>
              <SignalConnection Name="SignalConnection 0" Ratio="1" Source="@MIDI CC 7" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
              <SignalConnection Name="SignalConnection 1" Ratio="1" Source="@MIDI CC 11" Destination="Gain" Mapper="" ConnectionMode="0" Bypass="0" Inverted="0"/>
            </Connections></Part>"#,
        )
        .map_err(mlua::Error::external)?;
        let source_elements = tree.nodes.len();
        let part = element(
            &self.lua,
            &mut tree,
            part_xml.root_element(),
            None,
            false,
            &self.shared,
            1,
            &class,
            &list_class,
        )?;
        // These host placeholders belong to a separate synthetic document.
        // Its numeric NodeIds must never alias the admitted program XML.
        for (node, _) in &mut tree.nodes[source_elements..] { *node = None; }
        root.raw_set("parent", part.clone())?;
        root.raw_set("part", part)?;
        *self.shared.params.borrow_mut() = tree.params;
        *self.shared.scopes.borrow_mut() = tree.scopes;
        *self.shared.nodes.borrow_mut() = tree.nodes;
        *self.shared.kinds.borrow_mut() = tree.kinds;
        *self.shared.live.borrow_mut() = tree.live;
        self.lua.globals().raw_set("Program", root)
    }

    fn install(&self) -> mlua::Result<()> {
        let _profile = self.shared.profile.span("host_globals_and_prelude");
        let _span = Span::new("uvi_lua_host_install");
        let lua = &self.lua;
        // Only table/string/math/coroutine are loaded (no io, os, debug,
        // package); `lua.sandbox` would give every coroutine its own proxy
        // environment, which breaks the globals the prelude and scripts share.
        let globals = lua.globals();
        let shared = &self.shared;

        // Count Luau call/loop-backedge checkpoints, not elapsed scheduler time.
        let budget = shared.clone();
        lua.set_interrupt(move |lua| {
            budget
                .vm_checkpoints
                .set(budget.vm_checkpoints.get().saturating_add(1));
            budget.consume_work()?;
            if budget.initializing.get() {
                budget.profile.sample(lua, budget.vm_checkpoints.get());
            }
            Ok(VmState::Continue)
        });

        // Natives the prelude wraps (`__native`) and the engine API (globals).
        let native = lua.create_table()?;
        native.set(
            "float",
            lua.create_function(|_, value: f64| {
                let value = value as f32;
                if !value.is_finite() {
                    return Err(mlua::Error::runtime("UVI widget value exceeds float32 range"));
                }
                Ok(f64::from(value))
            })?,
        )?;
        let s = shared.clone();
        native.set("widgetChanged", lua.create_function(move |lua, (f, args): (Function, MultiValue)| {
            let previous = s.creator.get();
            let previous_origin=s.prepared_origin.borrow().clone();
            if !s.in_thread.get() {
                let origin=if s.staged_preparation.get()&&s.restoring_widget_values.get(){
                    let widget=match args.front(){Some(Value::Table(widget))=>Some(widget.clone()),_=>None};
                    let canonical=if let Some(widget)=widget {
                        let list=lua.globals().raw_get::<Table>("__ui").and_then(|ui|ui.raw_get::<Table>("widgets"));
                        let mut found=None;
                        if let Ok(list)=list {for candidate in list.sequence_values::<Table>() {
                            s.consume_work()?;let candidate=candidate?;
                            if candidate.to_pointer()==widget.to_pointer(){found=Some(widget.clone());break;}
                        }}
                        found
                    }else{None};
                    canonical.map_or(input_authority::PreparedLuaOrigin::UnclassifiedEvent,input_authority::PreparedLuaOrigin::RestoredWidget)
                }else{input_authority::PreparedLuaOrigin::UnclassifiedEvent};
                s.creator.set(PlayCreator::Widget);*s.prepared_origin.borrow_mut()=origin;
            }
            let result = f.call::<MultiValue>(args);
            s.creator.set(previous);*s.prepared_origin.borrow_mut()=previous_origin;
            result
        })?)?;
        async_data::install(lua, shared)?;
        let s = shared.clone();
        globals.raw_set(
            "loadImpulse",
            lua.create_function(move |lua, args: Variadic<Value>| {
                let id = match args.first() {
                    Some(Value::Table(element)) => element.raw_get::<Option<usize>>("__id")?,
                    _ => None,
                };
                let path = match args.get(1) {
                    Some(Value::String(path)) => {
                        Some(script_relative(lua, path.to_str()?.to_owned()))
                    }
                    _ => None,
                };
                let loaded = id
                    .zip(path)
                    .is_some_and(|(id, path)| s.rebuild_impulse(id, Some(&path)));
                let task = async_data::completed_task(lua, s.next_id(), loaded)?;
                // A response the bank cannot supply fails the task, as in UVI.
                if let Some(Value::Function(callback)) =
                    args.iter().find(|v| matches!(v, Value::Function(_)))
                {
                    let thread = lua.create_thread(callback.clone())?;
                    s.deferred
                        .borrow_mut()
                        .push((thread, MultiValue::from_vec(vec![Value::UserData(task.clone())]), PlayCreator::None, s.processor.get(), input_authority::PreparedLuaOrigin::None));
                }
                Ok(task)
            })?,
        )?;
        sample_loading::install(lua, shared)?;
        let s=shared.clone();
        native.set("sampleInfo", lua.create_function(move |lua, id: usize| {
            let target=s.nodes.borrow().get(id).and_then(|n|n.0).and_then(|n|u32::try_from(n).ok());
            match target { Some(target)=>s.sample_loads.borrow().info(lua,target), None=>Ok(None) }
        })?)?;
        let s = shared.clone();
        native.set(
            "setterMismatch",
            lua.create_function(move |_, (expected, actual): (String, String)| {
                s.setter_mismatch(SetterTypes {
                    expected: ParameterType::of(&expected),
                    actual: ParameterType::of(&actual),
                });
                Ok(())
            })?,
        )?;
        native.set(
            "resourcePath",
            lua.create_function(|lua, path: String| Ok(script_relative(lua, path)))?,
        )?;
        native.set(
            "sampleDirectory",
            lua.create_function(|lua, ()| Ok(script_directory(lua)))?,
        )?;
        let s = shared.clone();
        native.set(
            "saved",
            lua.create_function(move |_, name: String| Ok(s.processor_view.borrow().saved.borrow().get(&name).cloned()))?,
        )?;
        let s = shared.clone();
        // Port v1 host.rs's direct named lookup; preserve v2's typed catalog laws.
        native.set(
            "paramCount",
            lua.create_function(move |_, id: usize| {
                let kinds = s.kinds.borrow();
                let defs = parameters::definitions(kinds.get(id).map_or("", String::as_str));
                Ok(defs.len()
                    + s.params.borrow().get(id).map_or(0, |params| {
                        params
                            .iter()
                            .filter(|(name, _)| !defs.iter().any(|p| p.name == *name))
                            .count()
                    }))
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "paramName",
            lua.create_function(move |_, (id, parameter): (usize, f64)| {
                if !parameter.is_finite() || parameter < 1. || parameter.fract() != 0. {
                    return Ok(None);
                }
                let kinds = s.kinds.borrow();
                let defs = parameters::definitions(kinds.get(id).map_or("", String::as_str));
                let index = parameter as usize - 1;
                if let Some(p) = defs.get(index) {
                    return Ok(Some(p.name.to_owned()));
                }
                Ok(s.params
                    .borrow()
                    .get(id)
                    .and_then(|params| {
                        params
                            .iter()
                            .filter(|(name, _)| !defs.iter().any(|p| p.name == *name))
                            .nth(index - defs.len())
                    })
                    .map(|(name, _)| name.clone()))
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "hasParameter",
            lua.create_function(move |_, (id, name): (usize, String)| {
                #[cfg(feature = "scan")]
                let _timer = s.api_timer("uvi_lua_api_has_param");
                Ok(
                    parameters::definitions(s.kinds.borrow().get(id).map_or("", String::as_str))
                        .iter()
                        .any(|p| p.name == name)
                        || s.params
                            .borrow()
                            .get(id)
                            .is_some_and(|params| params.iter().any(|(key, _)| *key == name)),
                )
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "definition",
            lua.create_function(move |_, (id, name): (usize, String)| {
                #[cfg(feature = "scan")]
                let _timer = s.api_timer("uvi_lua_api_definition");
                if s.constant_property_field(id,&name){
                    return Ok((Some(if name=="Style"{"int"}else{"float"}),Some(if name=="Style"{0.}else{f64::MIN}),Some(if name=="Style"{1.}else{f64::MAX})));
                }
                if let Some(p) =
                    parameters::definitions(s.kinds.borrow().get(id).map_or("", String::as_str))
                        .iter()
                        .find(|p| p.name == name)
                {
                    return Ok((
                        Some(match p.kind {
                            "integer" => "int",
                            "boolean" => "bool",
                            kind => kind,
                        }),
                        Some(p.min),
                        Some(p.max),
                    ));
                }
                let params = s.params.borrow();
                let kind = params
                    .get(id)
                    .and_then(|params| params.iter().find(|(key, _)| *key == name))
                    // A retained field has no declared type (see `element.setParameter`).
                    .map(|(_, value)| {
                        if value.parse::<f64>().is_ok() {
                            "any"
                        } else {
                            "string"
                        }
                    });
                Ok((kind, None, None))
            })?,
        )?;
        let s = shared.clone();
        // v1 host.rs keeps inventories in Lua; retain one private scalar schema per kind.
        let schemas = lua.create_table()?;
        // Float and string templates, keyed by field name: one table lookup each,
        // with no per-field formatted key.
        let retained_float = lua.create_table()?;
        let retained_string = lua.create_table()?;
        // Clone the scalar schema and current retained-field templates in one
        // VM entry, rather than crossing Rust/Lua once for every parameter.
        // Retained templates are gathered anew for this element on every call.
        // Public definitions remain fresh mutable tables, with numeric and named
        // entries sharing one identity.
        shared.profile.register_chunk("@kontra/catalog-definitions");
        let clone_schema = catalog_copy::cloner(lua)?;
        native.set(
            "definitions",
            lua.create_function(move |lua, id: usize| {
                #[cfg(feature = "scan")]
                let _timer = s.api_timer("uvi_lua_api_definitions");
                let kinds = s.kinds.borrow();
                let kind = kinds.get(id).map_or("", String::as_str);
                let schema = match schemas.raw_get::<Value>(kind)? {
                    Value::Table(schema) => schema,
                    _ => {
                        let schema = lua.create_table()?;
                        for (i, p) in parameters::definitions(kind).iter().enumerate() {
                            let d = lua.create_table()?;
                            d.raw_set("id", i + 1)?;
                            d.raw_set("name", p.name)?;
                            d.raw_set(
                                "type",
                                match p.kind {
                                    "integer" => "int",
                                    "boolean" => "bool",
                                    kind => kind,
                                },
                            )?;
                            d.raw_set("displayName", p.name)?;
                            d.raw_set("description", "")?;
                            d.raw_set("readOnly", false)?;
                            d.raw_set("serialize", true)?;
                            let value = |n| {
                                if p.kind == "boolean" {
                                    Value::Boolean(n != 0.)
                                } else {
                                    Value::Number(n)
                                }
                            };
                            d.raw_set("min", value(p.min))?;
                            d.raw_set("max", value(p.max))?;
                            d.raw_set("default", value(p.default))?;
                            d.raw_set("unit", p.unit)?;
                            d.raw_set(
                                "mapper",
                                if p.unit == "Hz" && p.min > 0. {
                                    "Exponential"
                                } else {
                                    "Linear"
                                },
                            )?;
                            // Private, immutable name aliases avoid searching
                            // the catalog for every current XML attribute.
                            schema.raw_set(p.name, d.clone())?;
                            schema.raw_push(d)?;
                        }
                        schemas.raw_set(kind, schema.clone())?;
                        schema
                    }
                };
                // The public numeric catalog omits string fields and some graph
                // nodes. Retain their typed XML identity without fabricating native
                // bounds or omitted defaults. Provenance distinguishes these facts.
                let mut templates: Option<Table> = None;
                if let Some(params) = s.params.borrow().get(id) {
                    for (name, value) in params {
                        if !schema.raw_get::<Value>(name.as_str())?.is_nil() {
                            continue;
                        }
                        let kind = if value.parse::<f64>().is_ok() {
                            "float"
                        } else {
                            "string"
                        };
                        let retained = if kind == "float" { &retained_float } else { &retained_string };
                        let template = match retained.raw_get::<Value>(name.as_str())? {
                            Value::Table(template) => template,
                            _ => {
                                let template = lua.create_table()?;
                                template.raw_set("name", name.as_str())?;
                                template.raw_set("displayName", name.as_str())?;
                                template.raw_set("description", "")?;
                                template.raw_set("readOnly", false)?;
                                template.raw_set("serialize", true)?;
                                template.raw_set("type", kind)?;
                                template.raw_set("provenance", "retained-xml")?;
                                retained.raw_set(name.as_str(), template.clone())?;
                                template
                            }
                        };
                        let list = match templates {
                            Some(ref list) => list,
                            None => templates
                                .insert(lua.create_table_with_capacity(params.len(), 0)?),
                        };
                        list.raw_push(template)?;
                    }
                }
                let definitions=clone_schema.call::<Table>((schema, templates))?;
                if s.constant_property_field(id,"Value"){
                    // Modify only the fresh per-element clone. Never poison the
                    // cached native/static schema used by unpromoted elements.
                    for (field,min,max,kind)in [("Value",f64::MIN,f64::MAX,"float"),("Style",0.,1.,"int")]{
                        let definition:Table=definitions.raw_get(field)?;
                        definition.raw_set("min",min)?;definition.raw_set("max",max)?;definition.raw_set("type",kind)?;
                        definition.raw_set("default",0.)?;definition.raw_set("provenance","declared-core-constant-target")?;
                    }
                }
                Ok(definitions)
            })?,
        )?;
        let s=shared.clone();
        native.set("constantPropertyField",lua.create_function(move |_,(id,name):(usize,mlua::String)|{
            // Borrow the Lua string: this runs on every getParameter/setParameter.
            let property=s.constant_property_field(id,&name.to_str()?);
            Ok((property,property&&!s.initializing.get()))
        })?)?;
        let s = shared.clone();
        native.set(
            "paramNames",
            lua.create_function(move |lua, id: usize| {
                #[cfg(feature = "scan")]
                let _timer = s.api_timer("uvi_lua_api_param_names");
                let names = lua.create_table()?;
                if let Some(p) = s.params.borrow().get(id) {
                    for (k, _) in p {
                        names.raw_push(k.as_str())?;
                    }
                }
                Ok(names)
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "setParam",
            lua.create_function(move |_, (id, name, value): (usize, String, Value)| {
                #[cfg(feature = "scan")]
                let _timer = s.api_timer("uvi_lua_api_set_param");
                if s.context_failed.get(){return Ok(None);}
                let bound=s.authority.borrow().is_some();
                let current=s.authority.borrow().as_ref().and_then(|owner|owner.current()).is_some();
                if current{
                    // Validate actual Core continuation BEFORE owner-only bool
                    // writes and current-value no-op decisions, not merely
                    // before a later numerical control command.
                    let valid=matches!(s.authority.borrow_mut().as_mut().map(|owner|owner.call(AuthorityOperation::Validate)),Some(Ok(AuthorityValue::Validated)));
                    if !valid{return Ok(None);}
                }else if !s.staged_preparation.get()&&(s.canonical_tasks||bound){return Ok(None);}
                let written = match &value {
                    Value::Integer(n) => Some(*n as f64),
                    Value::Number(n) => Some(*n),
                    Value::Boolean(b) => Some(f64::from(*b)),
                    _ => None,
                };
                let field = name.clone();
                // Synthetic host Part settings have no admitted physical/XML
                // owner yet, even when a catalog default equals the request.
                if name=="Streaming" && s.kinds.borrow().get(id).is_some_and(|k|k=="Part") {
                    return Ok(None);
                }
                let applied = (|| -> mlua::Result<Option<bool>> {
                if name=="Bypass"&&s.kinds.borrow().get(id).is_some_and(|kind|kind=="ScriptProcessor") {
                    let Value::Boolean(bypassed)=&value else{return Ok(None);};
                    let mut retained=s.params.borrow_mut();
                    let Some(params)=retained.get_mut(id)else{return Ok(None);};
                    let text=if *bypassed {"true"}else{"false"}.to_owned();
                    match params.iter_mut().find(|(name,_)|name=="Bypass"){Some((_,value))=>*value=text,None=>params.push(("Bypass".into(),text))}
                    return Ok(Some(true));
                }
                // Retained XML fields have local model identity, not DSP acceptance.
                let catalogued = parameters::definitions(
                    s.kinds.borrow().get(id).map_or("", String::as_str),
                )
                .iter()
                .any(|p| p.name == name);
                let kind=s.kinds.borrow().get(id).cloned().unwrap_or_default();
                if matches!(kind.as_str(),"Program"|"Layer") && !s.initializing.get() {
                    let owner_field=match name.as_str(){
                        "Gain" if current=>Some(sampler_core::SourceOwnerControl::Gain),
                        "Pan" if current=>Some(sampler_core::SourceOwnerControl::Pan),
                        "Mute"=>Some(sampler_core::SourceOwnerControl::OutputMute),
                        "MidiMute"=>Some(sampler_core::SourceOwnerControl::MidiMute),
                        // No native-law/capability proof follows from catalog kind.
                        "BypassInsertFX"|"FXPostGain"=>return Ok(None),
                        _=>None,
                    };
                    if let Some(owner_field)=owner_field {
                        // This branch precedes same-default acceptance: a missing
                        // compiled home never becomes writable through a no-op.
                        if !catalogued || !current{return Ok(None);}
                        let control_value=match (owner_field,&value){
                            (sampler_core::SourceOwnerControl::OutputMute|sampler_core::SourceOwnerControl::MidiMute,Value::Boolean(value))=>sampler_core::ControlValue::Toggle(*value),
                            (sampler_core::SourceOwnerControl::Gain|sampler_core::SourceOwnerControl::Pan,Value::Integer(value))=>sampler_core::ControlValue::Real(*value as f64),
                            (sampler_core::SourceOwnerControl::Gain|sampler_core::SourceOwnerControl::Pan,Value::Number(value))if value.is_finite()=>sampler_core::ControlValue::Real(*value),
                            _=>return Ok(None),
                        };
                        let Some(node)=s.nodes.borrow().get(id).and_then(|(node,_)|*node)else{return Ok(None);};
                        let Ok(node)=u32::try_from(node)else{return Ok(None);};
                        if s.params.borrow().get(id).is_none(){return Ok(None);}
                        if !s.command(Command::OwnerControl{owner:sampler_ir::SourceOwnerId::UviNode(node),field:owner_field,value:control_value}){return Ok(None);}
                        let text=match control_value{sampler_core::ControlValue::Toggle(value)=>value.to_string(),sampler_core::ControlValue::Real(value)=>value.to_string(),_=>return Ok(None)};
                        let mut retained=s.params.borrow_mut();let Some(params)=retained.get_mut(id)else{return Ok(None);};
                        match params.iter_mut().find(|(field,_)|field==&name){Some((_,value))=>*value=text,None=>params.push((name.clone(),text))}
                        return Ok(Some(true));
                    }
                }
                if kind=="ConstantModulation" && matches!(name.as_str(),"Value"|"Style")
                    && s.params.borrow().get(id).is_some_and(|p|p.iter().any(|(field,_)|field=="ScriptLive")) {
                    if !matches!(&value,Value::Integer(_)|Value::Number(_)){return Ok(None);}
                    let Some(raw)=written.filter(|value|value.is_finite())else{return Ok(None);};
                    let (field,control)=match name.as_str(){
                        "Value"=>(sampler_core::SourceConstantField::Value,sampler_core::ControlValue::Real(raw)),
                        "Style" if raw==0.||raw==1.=>(sampler_core::SourceConstantField::Style,sampler_core::ControlValue::Toggle(raw==1.)),
                        _=>return Ok(None),
                    };
                    if s.initializing.get(){
                        // Pure initialized XML folding; no runtime/listener command.
                        return Ok(s.fold(id,&name,raw).then_some(true));
                    }
                    let Some(node)=s.nodes.borrow().get(id).and_then(|(node,_)|*node).and_then(|node|u32::try_from(node).ok())else{return Ok(None);};
                    let accepted=if current{s.command(Command::ConstantProperty{node,field,value:control})}
                        else if let Some(owner)=s.source_properties.borrow_mut().as_mut(){owner.call(SourcePropertyOperation::Set{node,field,value:control}).is_ok()}
                        else{false};
                    if !accepted{return Ok(None);}
                    let mut retained=s.params.borrow_mut();let Some(params)=retained.get_mut(id)else{return Ok(None);};
                    match params.iter_mut().find(|(key,_)|key==&name){Some((_,value))=>*value=raw.to_string(),None=>params.push((name.clone(),raw.to_string()))}
                    return Ok(Some(true));
                }
                // A write of the value the engine already holds changes nothing:
                // rejected writes are never retained, so the retained field is
                // what plays (UVI fields are f32, so compare at f32).
                let number = match &value {
                    Value::Integer(n) => Some(*n as f64),
                    Value::Number(n) => Some(*n),
                    Value::Boolean(b) => Some(f64::from(*b)),
                    _ => None,
                };
                if let Some(v) = number {
                    let kind = s.kinds.borrow().get(id).cloned().unwrap_or_default();
                    let current = s
                        .params
                        .borrow()
                        .get(id)
                        .and_then(|p| p.iter().find(|(k, _)| *k == name))
                        .and_then(|(_, v)| match v.as_str() {
                            "true" => Some(1.),
                            "false" => Some(0.),
                            v => v.parse::<f64>().ok(),
                        })
                        .or_else(|| parameters::definitions(&kind).iter().find(|p| p.name == name).map(|p| p.default));
                    if current.is_some_and(|c| c as f32 == v as f32) {
                        return Ok(Some(true));
                    }
                }
                if !catalogued {
                    return Ok(s.params.borrow().get(id).and_then(|params| {
                        params.iter().any(|(key, _)| *key == name).then_some(false)
                    }));
                }
                let value = match value {
                    Value::Integer(n) => n as f64,
                    Value::Number(n) => n,
                    Value::Boolean(b) => f64::from(b),
                    _ => return Ok(None),
                };
                // What the translator gave a DSP lane (`engine_parameters::live_names`).
                let live = s.live.borrow().get(id).cloned().unwrap_or_default();
                let node = s.nodes.borrow().get(id).and_then(|(node, _)| *node);
                let kind = s.kinds.borrow().get(id).cloned().unwrap_or_default();
                if matches!(kind.as_str(), "Convolver" | "SampledReverb")
                    && IMPULSE_FIELDS.contains(&name.as_str())
                    && node.is_some_and(|n| s.files.convolver_slot(n).is_some())
                {
                    // Deferred rebuilding is not a synchronous Core ACK.
                    if current{return Ok(None);}
                    // The impulse is built from the retained fields.
                    if let Some(params) = s.params.borrow_mut().get_mut(id) {
                        match params.iter_mut().find(|(k, _)| *k == name) {
                            Some((_, v)) => *v = value.to_string(),
                            None => params.push((name.clone(), value.to_string())),
                        }
                    }
                    // Rebuilt when commands are next taken, once for all fields.
                    let mut stale = s.stale_impulses.borrow_mut();
                    if !stale.contains(&id) {
                        stale.push(id);
                    }
                    return Ok(Some(true));
                }
                let engine = |name: &str, value: f64| {
                    let (name, value) = {
                        let params = s.params.borrow();
                        crate::engine_parameters::effective(&kind, name, value, |n| {
                            params
                                .get(id)?
                                .iter()
                                .find(|(k, _)| k == n)
                                .and_then(|(_, v)| v.parse().ok())
                        })
                    };
                    let name = name.as_str();
                    let binding = crate::engine_parameters::binding(node?, &kind, name)?;
                    Some(Command::EngineParameter {
                        address: binding.address,
                        value: binding.law.normalized_value(value).ok()?,
                    })
                };
                if name != "Bypass"
                    && live.contains(&name.as_str())
                    && crate::engine_parameters::lane_kind(&kind)
                    && let Some(node) = node
                {
                    // A delay, modulation or reverb unit: its lanes, or nothing. One
                    // left bypassed without a Mix of its own was never built: silent.
                    let silent = !live.contains(&"Bypass")
                        && s.params.borrow().get(id).is_some_and(|p| {
                            p.iter().any(|(k, v)| k == "Bypass" && v.parse::<f64>().is_ok_and(|b| b != 0.))
                        });
                    let writes = if silent {
                        Some(Vec::new())
                    } else {
                        s.params.borrow().get(id).and_then(|p| crate::engine_parameters::retune(node, &kind, p, &name, value))
                    };
                    let Some(writes) = writes else { return Ok(None) };
                    if current&&writes.len()>1{return Ok(None);}
                    for (address, value) in writes {
                        if !s.command(Command::EngineParameter { address, value }) {
                            return Ok(None);
                        }
                    }
                    if let Some(params) = s.params.borrow_mut().get_mut(id) {
                        match params.iter_mut().find(|(k, _)| *k == name) {
                            Some((_, v)) => *v = value.to_string(),
                            None => params.push((name.clone(), value.to_string())),
                        }
                    }
                    return Ok(Some(true));
                }
                let command = if name == "Bypass" && live.contains(&"Bypass") {
                    // The insert's Mix: a core slot control.
                    sampler_core::engine_parameter_id("ENGINE_PAR_EFFECT_BYPASS").zip(node).map(
                        |(parameter, node)| Command::EngineParameter {
                            address: sampler_core::EngineParameterAddress {
                                parameter,
                                group: -1,
                                slot: -1,
                                generic: i32::try_from(node).unwrap_or(-1),
                            },
                            value: i32::from(value != 0.0),
                        },
                    )
                } else if kind == "AuxEffect"
                    && live.contains(&"Gain")
                    && (name == "Gain" || name == "Bypass" && live.contains(&"BypassGain"))
                {
                    // A rack chain's bypass is its gain at zero: send the
                    // effective gain whichever of the two was written.
                    let params = s.params.borrow();
                    let retained = params.get(id).map(|p| {
                        let read = |n: &str, d: f64| {
                            if n==name{return value;}
                            p.iter().find(|(k, _)| k == n)
                                .and_then(|(_, v)| v.parse().ok()).unwrap_or(d)
                        };
                        let bypassed = live.contains(&"BypassGain") && read("Bypass", 0.) != 0.;
                        if bypassed { 0. } else { read("Gain", 1.) }
                    });
                    drop(params);
                    live.contains(&"Gain")
                        .then_some(retained)
                        .flatten()
                        .and_then(|effective| engine("Gain", effective))
                } else if kind == "ConstantModulation" && live.contains(&name.as_str()) {
                    // A listener broadcast is not a Constant property acceptance.
                    // Replaced by the typed definition-property path below42f.
                    None
                } else if live.contains(&name.as_str()) {
                    engine(&name, value)
                } else {
                    None
                };
                if live.contains(&name.as_str()) || command.is_some() {
                    let accepted = command.and_then(|c| s.command(c).then_some(true));
                    if accepted.is_some()
                        && let Some(params) = s.params.borrow_mut().get_mut(id)
                    {
                        match params.iter_mut().find(|(k, _)| *k == name) {
                            Some((_, v)) => *v = value.to_string(),
                            None => params.push((name.clone(), value.to_string())),
                        }
                    }
                    return Ok(accepted);
                }
                let Some(scope) = s.scopes.borrow().get(id).copied().flatten() else {
                    return Ok(None);
                };
                let (param, default) = match (scope, name.as_str()) {
                    (_, "Gain") => (Param::Gain, 1.0),
                    (_, "Pan") => (Param::Pan, 0.0),
                    (Scope::Oscillator(_), "Pitch") => (Param::Pitch, 1.0),
                    (Scope::Program, "Polyphony") => (Param::Polyphony, 16.0),
                    _ => return Ok(None),
                };
                if !value.is_finite() {
                    return Ok(None);
                }
                let authored = s
                    .params
                    .borrow()
                    .get(id)
                    .and_then(|p| p.iter().find(|(k, _)| *k == name))
                    .and_then(|(_, v)| v.parse().ok())
                    .unwrap_or(default);
                if !s.command(Command::Parameter {
                    scope,
                    param,
                    value,
                    authored,
                }) {
                    return Ok(None);
                }
                let mut params = s.params.borrow_mut();
                if let Some(params) = params.get_mut(id) {
                    if let Some((_, v)) = params.iter_mut().find(|(k, _)| *k == name) {
                        *v = value.to_string();
                    } else {
                        params.push((name, value.to_string()));
                    }
                }
                Ok(Some(true))
                })()?;
                // While initializing, a write with no live path is applied by the
                // translation that follows (it reads the patched XML). A write a
                // full queue refused stays refused.
                if applied != Some(true)
                    && s.initializing.get()
                    && s.command_count(s.commands.borrow().len()) < 1 << 16
                    && let Some(v) = written
                    && s.fold(id, &field, v)
                {
                    return Ok(Some(true));
                }
                Ok(applied)
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "param",
            lua.create_function(move |lua, (id, name): (usize, String)| {
                #[cfg(feature = "scan")]
                let _timer = s.api_timer("uvi_lua_api_get_param");
                let kinds = s.kinds.borrow();
                if kinds.get(id).is_some_and(|kind|kind=="ConstantModulation") && matches!(name.as_str(),"Value"|"Style")
                    && !s.initializing.get() && s.params.borrow().get(id).is_some_and(|p|p.iter().any(|(field,_)|field=="ScriptLive")) {
                    let node=s.nodes.borrow().get(id).and_then(|(node,_)|*node).and_then(|node|u32::try_from(node).ok()).ok_or_else(||mlua::Error::runtime("missing actual Constant definition"))?;
                    let snapshot=if s.authority.borrow().as_ref().and_then(|owner|owner.current()).is_some(){
                        match s.authority.borrow_mut().as_mut().ok_or_else(||mlua::Error::runtime("missing captured property owner"))?.call(AuthorityOperation::QueryConstant{node}){
                            Ok(AuthorityValue::Constant(value))=>Ok(value),Ok(_)=>Err(sampler_core::Error::InvalidInput),Err(error)=>Err(error),
                        }
                    }else if let Some(owner)=s.source_properties.borrow_mut().as_mut(){owner.call(SourcePropertyOperation::Query{node})}
                    else{Err(sampler_core::Error::NotReady)}.map_err(|error|mlua::Error::runtime(format!("Constant property query refused: {error:?}")))?;
                    return Ok(Value::Number(if name=="Value"{snapshot.value}else{f64::from(u8::from(snapshot.style))}));
                }
                if kinds.get(id).is_some_and(|kind| kind == "StepEnvelope")
                    && let Some(index) = name.strip_prefix("Level").and_then(|s| s.parse::<usize>().ok())
                    && index < 128
                    && name == format!("Level{index}")
                {
                    // UVI Element.getParameter / Step Envelope; native 0x141150e40
                    // registers 128 float levels (getter 0x141153110), independently of NumSteps.
                    let params = s.params.borrow();
                    let saved = params.get(id).and_then(|p| p.iter().find(|(k, _)| k == "Levels"));
                    let value = saved
                        .and_then(|(_, levels)| levels.split_whitespace().nth(index))
                        .and_then(|v| v.replace(',', ".").parse::<f32>().ok())
                        .filter(|v| v.is_finite())
                        .unwrap_or(if matches!(index, 0 | 4 | 8 | 10 | 12) { 1. } else { 0. });
                    s.profile.step_envelope_level();
                    return Ok(Value::Number(f64::from(value)));
                }
                if s.profile.enabled() {
                    return s.profile.parameter(lua, kinds.get(id).map_or("", String::as_str), &name, &s.params, id);
                }
                let definition = parameters::definition(kinds.get(id).map_or("", String::as_str), &name);
                // Read under the field borrow; only a string result leaves it as
                // an owned copy, since a Lua string allocation must not run inside it.
                enum Found {
                    Bool(bool),
                    Number(f64),
                    Text(String),
                }
                let found = {
                    let params = s.params.borrow();
                    params
                        .get(id)
                        .and_then(|p| p.iter().find(|(k, _)| *k == name))
                        .map(|(_, v)| {
                            if definition.is_some_and(|p| p.kind == "boolean") {
                                Found::Bool(v == "1" || v == "true")
                            } else {
                                match v.parse::<f64>() {
                                    Ok(n) => Found::Number(n),
                                    Err(_) => Found::Text(v.clone()),
                                }
                            }
                        })
                };
                Ok(match found {
                    None => match definition {
                        Some(p) if p.kind == "boolean" => Value::Boolean(p.default != 0.),
                        Some(p) => Value::Number(p.default),
                        None => Value::Nil,
                    },
                    Some(Found::Bool(b)) => Value::Boolean(b),
                    Some(Found::Number(n)) => Value::Number(n),
                    Some(Found::Text(v)) => Value::String(lua.create_string(&v)?),
                })
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "defglobal",
            lua.create_function(move |lua, (name, value): (String, Value)| lua.globals().raw_set(name, value))?,
        )?;
        native.set("nextId", lua.create_function(move |_, ()| Ok(s.next_id()))?)?;
        let s = shared.clone();
        native.set(
            "source",
            lua.create_function(move |_, name: String| {
                Ok(s.files
                    .script_result(&name).map_err(mlua::Error::external)?
                    .or_else(|| (name == "uvi.ChordRec").then(|| modules::CHORD_REC.to_owned())))
            })?,
        )?;
        native.set(
            "asyncUpdater",
            lua.create_function(|lua, ()| {
                let factory = lua.create_userdata(modules::AsyncUpdaterFactory)?;
                factory.set_user_value(lua.globals())?;
                Ok(factory)
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "assigned",
            lua.create_function(move |_, name: String| {
                // Capitalized names are API classes (`WaveView = WaveView{...}`
                // reads the class first). Unset lowercase names the scripts
                // only read as values are variables (nil, as in Lua); the
                // ones they call are unmodeled API and answer a stub.
                Ok(name.starts_with(|c: char| c.is_ascii_lowercase())
                    && (s.processor_view.borrow().assigned.borrow().contains(&name) || !s.processor_view.borrow().called.borrow().contains(&name)))
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "compile",
            lua.create_function(move |lua, (source, name): (mlua::LuaString, String)| {
                let _profile = s.profile.span("module_compile");
                let _span = Span::new("uvi_lua_module_compile");
                {
                    let _profile = s.profile.span("module_name_inventory");
                    let _span = Span::new("uvi_lua_module_names");
                    s.note_assigned(&source.to_string_lossy());
                }
                let id = format!("uvi:module:{name}");
                let resolved_name = s.files.script_path(&name);
                if let Some(name) = &resolved_name {
                    s.profile.register_chunk(name);
                } else if name == "uvi.ChordRec" {
                    s.profile.register_chunk(&name);
                }
                let chunk_name = resolved_name.unwrap_or(name);
                let bytes = source.as_bytes();
                let mut chunk = lua.load(bytes.as_ref()).set_name(&chunk_name).set_environment(lua.globals());
                if s.inspection.borrow().is_some() {
                    chunk =
                        chunk.set_compiler(mlua::chunk::Compiler::default().set_coverage_level(1));
                }
                match chunk.into_function() {
                    Ok(f) => {
                        if let Some(inspection) = s.inspection.borrow_mut().as_mut() {
                            inspection.register(
                                id,
                                chunk_name,
                                &source.to_string_lossy(),
                                f.clone(),
                            );
                        }
                        Ok((Some(f), None))
                    }
                    Err(e) => Ok((None, Some(lua_error(e)))),
                }
            })?,
        )?;
        #[cfg(feature = "scan")]
        {
            let s = shared.clone();
            native.set(
                "scanKey",
                lua.create_function(move |_, (name, args): (String, Table)| {
                    let Some(note) = args
                        .raw_get::<Value>(1)
                        .ok()
                        .as_ref()
                        .and_then(number)
                        .filter(|n| n.is_finite() && *n >= 0. && *n <= 127. && n.fract() == 0.)
                    else {
                        return Ok(());
                    };
                    let mut keys = s.key_declarations.borrow_mut();
                    if name == "resetKeyColour" {
                        keys.remove(&(note as u8));
                    } else if let Ok(Value::String(c)) = args.raw_get::<Value>(2) {
                        let c = c.to_string_lossy();
                        let state = if c.eq_ignore_ascii_case("#00FFFFFF") {
                            1
                        } else if c.eq_ignore_ascii_case("#00000000") {
                            2
                        } else {
                            0
                        };
                        keys.entry(note as u8)
                            .and_modify(|old| {
                                if *old != state {
                                    *old = 3;
                                }
                            })
                            .or_insert(state);
                    }
                    let mut scan = s.scan.borrow_mut();
                    scan.native_valid_keys = keys
                        .iter()
                        .filter(|(_, c)| **c == 1)
                        .map(|(k, _)| *k)
                        .collect();
                    scan.native_invalid_keys = keys
                        .iter()
                        .filter(|(_, c)| **c == 2)
                        .map(|(k, _)| *k)
                        .collect();
                    scan.native_key_conflicts = keys.values().filter(|c| **c == 3).count();
                    Ok(())
                })?,
            )?;
        }
        globals.raw_set("__native", native.clone())?;
        let s = shared.clone();
        globals.raw_set(
            "__report",
            lua.create_function(move |_, (feature, value): (String, String)| {
                if feature == "lua error" {
                    s.find(&feature, &value);
                } else {
                    s.find(&format!("lua {feature}"), &value);
                }
                Ok(())
            })?,
        )?;

        let s = shared.clone();
        globals.raw_set(
            "getTime",
            lua.create_function(move |_, ()| Ok(s.now.get()))?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "playNote",
            lua.create_function(move |_, args: Variadic<Value>| {
                if s.staged_preparation.get(){return Err(mlua::Error::runtime("staged playNote requires an admitted Core task"));}
                let play = parse_play(&s, &args, PlayOrigin::Helper)?;
                let id = play.id;
                let requires_ack=s.canonical_tasks||s.authority.borrow().is_some();
                if !s.command(Command::Play(play))&&(requires_ack||s.staged_preparation.get()){
                    return Err(mlua::Error::runtime("Core generated-note admission refused"));
                }
                s.voices.borrow_mut().insert(id);
                Ok(id)
            })?,
        )?;
        let s=shared.clone();
        native.set("postExternal",lua.create_function(move |_,table:Table|{
            let Some(projection)=s.external_projection.borrow().clone() else{
                if s.authority.borrow().as_ref().and_then(|owner|owner.current()).is_some(){
                    return Err(mlua::Error::runtime("this actual captured task has no typed authored event route"));
                }
                if !s.staged_preparation.get()&&(s.canonical_tasks||s.authority.borrow().is_some()){
                    return Err(mlua::Error::runtime("authored event requires an actual Running Core turn"));
                }
                return Ok(false);
            };
            let output=projection.output(&table)?;
            let result=s.authority.borrow_mut().as_mut().ok_or_else(||mlua::Error::runtime("external owner disconnected"))?
                .call(AuthorityOperation::Post(output)).map_err(|error|mlua::Error::runtime(format!("Core post refused: {error:?}")))?;
            if !matches!(result,AuthorityValue::Posted(_)){return Err(mlua::Error::runtime("invalid Core post receipt"));}
            Ok(true)
        })?)?;
        let s = shared.clone();
        native.set(
            "postNote",
            lua.create_function(move |_, event: Table| {
                if s.staged_preparation.get(){return Err(mlua::Error::runtime("staged posted note requires an admitted Core task"));}
                let mut play = parse_play(&s, &[Value::Table(event.clone())], PlayOrigin::PostedEvent)?;
                play.id = field(&event, "id")
                    .or_else(|| field(&event, "voiceId"))
                    .map_or(play.id, |id| id as u64);
                let id=play.id;
                let requires_ack=s.canonical_tasks||s.authority.borrow().is_some();
                if !s.command(Command::Play(play))&&(requires_ack||s.staged_preparation.get()){
                    return Err(mlua::Error::runtime("Core posted-note admission refused"));
                }
                s.voices.borrow_mut().insert(id);
                Ok(id)
            })?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "releaseVoice",
            lua.create_function(move |_, id: f64| {
                if !id.is_finite()||!s.voices.borrow().contains(&(id as u64)){return Ok(false);}
                if !s.command(Command::Release{id:id as u64,at_ms:s.now.get()}){return Ok(false);}
                s.voices.borrow_mut().remove(&(id as u64));Ok(true)
            })?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "sendScriptModulation",
            lua.create_function(
                move |_, (id, value, glide, voice): (f64, f64, Option<f64>, Option<f64>)| {
                    let glide = glide.unwrap_or(20.0);
                    // Normalize finite negative durations only. Nonfinite inputs
                    // must reach the Driver's pure request preflight unchanged.
                    let glide = if glide.is_finite() { glide.max(0.0) } else { glide };
                    s.command(Command::Modulation {
                        id: id.clamp(0.0, f64::from(u16::MAX)) as u16,
                        start: None,
                        value,
                        glide_ms: glide,
                        voice: voice.map(|v| v as u64),
                        at_ms: s.now.get(),
                    });
                    Ok(())
                },
            )?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "spawn",
            lua.create_function(move |lua, (f, args): (Function, MultiValue)| {
                let thread = lua.create_thread(f)?;
                s.deferred.borrow_mut().push((thread, args, s.creator.get().independent(), s.processor.get(),s.prepared_origin.borrow().clone()));
                Ok(())
            })?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "run",
            lua.create_function(move |lua, (f, args): (Function, MultiValue)| {
                let thread = lua.create_thread(f)?;
                // Immediate execution does not transfer the calling note's
                // ownership to this independent coroutine (as with spawn).
                resume(lua, &s, thread, args, s.creator.get().independent(), s.processor.get());
                Ok(())
            })?,
        )?;
        self.install_api()?;
        modules::install_class(lua, &globals)?;
        shared.profile.register_chunk("prelude");
        lua.load(PRELUDE).set_name("prelude").exec()
    }

    /// The musical context, conversions, voice manipulation and MIDI
    /// generation functions of the engine API.
    fn install_api(&self) -> mlua::Result<()> {
        let (lua, shared) = (&self.lua, &self.shared);
        let globals = lua.globals();
        macro_rules! define {
            ($name:literal, $shared:ident, $f:expr) => {{
                let $shared = shared.clone();
                globals.raw_set($name, lua.create_function($f)?)?;
            }};
        }
        let beat = |s: &Shared| 60_000.0 / s.tempo.get();
        // Musical context.
        define!("getTempo", s, move |_, ()| Ok(s.tempo.get()));
        define!("getBeatDuration", s, move |_, ()| Ok(beat(&s)));
        define!("getBarDuration", s, move |_, ()| Ok(beat(&s) * 4.0));
        globals.raw_set("getTimeSignature", lua.create_function(|_, ()| Ok((4, 4)))?)?;
        define!("getSamplingRate", s, move |_, ()| Ok(s.config.rate));
        define!("getBeatTime", s, move |_, ()| {
            let (at, value) = s.beat_anchor.get();
            Ok(value
                + if s.playing.get() {
                    (s.now.get() - at) / beat(&s)
                } else {
                    0.
                })
        });
        define!("getRunningBeatTime", s, move |_, ()| Ok(
            s.now.get() / beat(&s)
        ));
        define!("getNoteDuration", s, move |_, note: f64| {
            let down = s.last_note_on.borrow();
            Ok(down
                .get(note as usize)
                .copied()
                .flatten()
                .map_or(0.0, |at| (s.now.get() - at).max(0.0)))
        });
        define!("isKeyDown", s, move |_, note: f64| {
            Ok(s.held.borrow().values().any(|key| usize::from(*key) == note as usize))
        });
        define!("isOctaveKeyDown", s, move |_, note: f64| {
            let class = (note as usize) % 12;
            Ok(s.held.borrow().values().any(|key| usize::from(*key) % 12 == class))
        });
        define!("isNoteHeld", s, move |_, ()| Ok(s
            .creator
            .get()
            .note()
            .is_some_and(|id| s.held.borrow().contains_key(&id))));
        define!("getCC", s, move |_, cc: f64| {
            Ok(s.processor_view.borrow().cc.borrow().get(cc as usize).copied().unwrap_or(0))
        });
        for (name,family) in [
            ("enable_rpn_as_cc",input_authority::ControllerFamily::Rpn),
            ("enable_nrpn_as_cc",input_authority::ControllerFamily::Nrpn),
            ("enable_data_entry_as_cc",input_authority::ControllerFamily::DataEntry),
        ] {
            let s=shared.clone();
            globals.raw_set(name,lua.create_function(move |_,enabled:bool| {
                s.processor_view.borrow().visibility.set(family,enabled);Ok(())
            })?)?;
        }
        // Conversions.
        define!("beat2ms", s, move |_, beats: f64| Ok(beats * beat(&s)));
        define!("ms2beat", s, move |_, ms: f64| Ok(ms / beat(&s)));
        define!("ms2samples", s, move |_, ms: f64| Ok(
            ms * s.config.rate / 1000.0
        ));
        define!("samples2ms", s, move |_, n: f64| Ok(
            n * 1000.0 / s.config.rate
        ));
        // Voice manipulation.
        let change = |what: Change, to_value: fn(f64) -> f64| {
            move |s: Rc<Shared>| {
                move |_: &Lua, (id, value, relative, immediate): (f64, f64, Option<bool>, Option<bool>)| {
                    s.command(Command::Change {
                        id: id as u64,
                        what,
                        value: to_value(value),
                        relative: relative.unwrap_or(false),
                        immediate: immediate.unwrap_or(false),
                        at_ms: s.now.get(),
                    });
                    Ok(())
                }
            }
        };
        let db = |gain: f64| 20.0 * gain.max(1e-6).log10();
        globals.raw_set(
            "changeVolume",
            lua.create_function(change(Change::Decibels, db)(shared.clone()))?,
        )?;
        globals.raw_set(
            "changeVolumedB",
            lua.create_function(change(Change::Decibels, |v| v)(shared.clone()))?,
        )?;
        globals.raw_set(
            "changePan",
            lua.create_function(change(Change::Pan, |v| v)(shared.clone()))?,
        )?;
        globals.raw_set(
            "changeTune",
            lua.create_function(change(Change::Tune, |v| v)(shared.clone()))?,
        )?;
        define!("fadein", s, move |_,
                                   (id, ms, reset, layer): (
            f64,
            f64,
            Option<bool>,
            Option<u32>
        )| {
            s.command(Command::Fade {
                id: id as u64,
                from: reset.unwrap_or(false).then_some(0.0),
                to: 1.0,
                ms: ms.max(0.0),
                kill: false,
                layer: layer.unwrap_or(0),
                at_ms: s.now.get(),
            });
            Ok(())
        });
        define!("fadeout", s, move |_,
                                    (id, ms, kill, reset, layer): (
            f64,
            f64,
            Option<bool>,
            Option<bool>,
            Option<u32>
        )| {
            s.command(Command::Fade {
                id: id as u64,
                from: reset.unwrap_or(false).then_some(1.0),
                to: 0.0,
                ms: ms.max(0.0),
                kill: kill.unwrap_or(false),
                layer: layer.unwrap_or(0),
                at_ms: s.now.get(),
            });
            Ok(())
        });
        define!("fade", s, move |_,
                                 (id, to, ms, layer): (
            f64,
            f64,
            f64,
            Option<u32>
        )| {
            s.command(Command::Fade {
                id: id as u64,
                from: None,
                to,
                ms: ms.max(0.0),
                kill: false,
                layer: layer.unwrap_or(0),
                at_ms: s.now.get(),
            });
            Ok(())
        });
        define!("fade2", s, move |_,
                                  (id, from, to, ms, layer): (
            f64,
            f64,
            f64,
            f64,
            Option<u32>
        )| {
            s.command(Command::Fade {
                id: id as u64,
                from: Some(from),
                to,
                ms: ms.max(0.0),
                kill: false,
                layer: layer.unwrap_or(0),
                at_ms: s.now.get(),
            });
            Ok(())
        });
        define!("sendScriptModulation2", s, move |_,
                                                  (
            id,
            from,
            to,
            ramp,
            voice,
        ): (
            f64,
            f64,
            f64,
            Option<f64>,
            Option<f64>
        )| {
            let id = id.clamp(0.0, f64::from(u16::MAX)) as u16;
            let voice = voice.map(|v| v as u64);
            let at_ms = s.now.get();
            let ramp = ramp.unwrap_or(20.0);
            // Preserve NaN and infinities for the same Driver validation as
            // explicit start/target values; finite negatives retain zero-time policy.
            let ramp = if ramp.is_finite() { ramp.max(0.0) } else { ramp };
            // One logical request: queue and Core admission cannot accept the
            // start while refusing its accompanying ramp.
            s.command(Command::Modulation {
                id,
                start: Some(from),
                value: to,
                glide_ms: ramp,
                voice,
                at_ms,
            });
            Ok(())
        });
        // MIDI generation: the channel is 1..=16, 0 or none the first.
        let midi = |s: &Shared, status: u8, channel: Option<f64>, a: f64, b: f64| {
            let channel = channel.map_or(0, |c| (c as i64 - 1).clamp(0, 15) as u8);
            s.command(Command::Midi(MidiOut {
                status: status | channel,
                a: a.clamp(0.0, 127.0) as u8,
                b: b.clamp(0.0, 127.0) as u8,
            }));
        };
        define!("controlChange", s, move |_,
                                          (cc, v, ch, _): (
            f64,
            f64,
            Option<f64>,
            Option<f64>
        )| {
            midi(&s, 0xb0, ch, cc, v);
            Ok(())
        });
        define!("programChange", s, move |_,
                                          (v, ch, _): (
            f64,
            Option<f64>,
            Option<f64>
        )| {
            midi(&s, 0xc0, ch, v, 0.0);
            Ok(())
        });
        define!("pitchBend", s, move |_,
                                      (bend, ch, _): (
            f64,
            Option<f64>,
            Option<f64>
        )| {
            let raw = (8192.0 + bend.clamp(-1.0, 1.0) * 8192.0)
                .round()
                .min(16383.0) as u16;
            midi(&s, 0xe0, ch, f64::from(raw & 127), f64::from(raw >> 7));
            Ok(())
        });
        define!("afterTouch", s, move |_,
                                       (v, ch, _): (
            f64,
            Option<f64>,
            Option<f64>
        )| {
            midi(&s, 0xd0, ch, v, 0.0);
            Ok(())
        });
        define!("polyAfterTouch", s, move |_,
                                           (v, note, ch, _): (
            f64,
            f64,
            Option<f64>,
            Option<f64>
        )| {
            midi(&s, 0xa0, ch, note, v);
            Ok(())
        });
        Ok(())
    }

    fn load_scripts(&self, doc:&roxmltree::Document, state:Option<&UiState>)->Result<(),String> {
        let _profile=self.shared.profile.span("scripts_total");
        let _span=Span::new("uvi_lua_scripts");
        if let Some(state)=state {state.validate()?;}
        if let Some(state)=state
            && let Some(first)=doc.descendants().find(|n|n.has_tag_name("ScriptProcessor"))
            && state.processors.iter().any(|row|row.source_node as usize==first.id().get_usize())
            && (!state.widgets.is_empty()||state.custom.is_some()) {
                return Err("duplicate legacy/typed first-processor state".into());
            }
        let bootstrap=self.lua.globals();
        let program:Table=bootstrap.raw_get("Program").map_err(lua_error)?;
        let element_class:Table=bootstrap.raw_get("__element_mt").map_err(lua_error)?;
        let list_class:Table=bootstrap.raw_get("__list_mt").map_err(lua_error)?;
        // The canonical graph is built exactly once. Resolve `this` before any
        // authored callback can edit a collection; the object itself is shared.
        let mut objects=BTreeMap::new();
        let mut pending=vec![program.clone()];
        while let Some(object)=pending.pop() {
            let id:usize=object.raw_get("__id").map_err(lua_error)?;
            if let Some(node)=self.shared.nodes.borrow().get(id).and_then(|n|n.0) {
                if objects.insert(node,object.clone()).is_some(){return Err("duplicate canonical processor object".into());}
            }
            let children:Table=object.raw_get("children").map_err(lua_error)?;
            for child in children.sequence_values::<Table>() {pending.push(child.map_err(lua_error)?);}
            if objects.len()>1<<18 {return Err("ScriptProcessor graph budget exceeded".into());}
        }
        for processor in doc.descendants().filter(|n|n.has_tag_name("ScriptProcessor")) {
            if self.shared.processors.borrow().len()>=input_authority::MAX_PROCESSORS {
                return Err("ScriptProcessor count exceeds 1024".into());
            }
            let list=processor.parent().filter(|n|n.has_tag_name("EventProcessors"))
                .ok_or("ScriptProcessor is outside a canonical EventProcessors list")?;
            let owner_node=list.parent().filter(|n|matches!(n.tag_name().name(),"Program"|"Layer"|"Keygroup"|"SamplePlayer"))
                .ok_or("ScriptProcessor has no direct admitted source owner")?;
            let owner_kind=match owner_node.tag_name().name() {
                "Program"=>sampler_ir::SourceOwnerKind::Instrument,
                "Layer"=>sampler_ir::SourceOwnerKind::Layer,
                "Keygroup"=>sampler_ir::SourceOwnerKind::Keygroup,
                "SamplePlayer"=>sampler_ir::SourceOwnerKind::Player,
                _=>return Err("unadmitted ScriptProcessor owner kind".into()),
            };
            let owner_parent=owner_node.parent().into_iter().flat_map(|n|n.ancestors())
                .find(|n|matches!(n.tag_name().name(),"Program"|"Layer"|"Keygroup"|"SamplePlayer"))
                .map(|n|u32::try_from(n.id().get_usize()).map(sampler_ir::SourceOwnerId::UviNode))
                .transpose().map_err(|_|"ScriptProcessor owner parent identity overflow")?;
            let node=u32::try_from(processor.id().get_usize()).map_err(|_|"ScriptProcessor XML identity overflow")?;
            let program_level=owner_node.has_tag_name("Program");
            let owner=u32::try_from(owner_node.id().get_usize()).map_err(|_|"ScriptProcessor owner identity overflow")?;
            let ordinal=u32::try_from(list.children().filter(|n|n.is_element()).position(|n|n.id()==processor.id()).ok_or("processor list membership")?).map_err(|_|"processor ordinal overflow")?;
            let address=ProcessorAddress {processor:sampler_ir::SourceEventProcessorId::UviNode(node), owner:sampler_ir::SourceOwnerId::UviNode(owner),ordinal};
            let owner_id=sampler_ir::SourceOwnerId::UviNode(owner);
            if !self.shared.owner_shapes.borrow().iter().any(|shape|shape.id==owner_id) {
                let mut events=Vec::new();
                for (ordinal,row) in list.children().filter(|n|n.is_element()).enumerate() {
                    let id=u32::try_from(row.id().get_usize()).map_err(|_|"event-list member identity overflow")?;
                    let ordinal=u32::try_from(ordinal).map_err(|_|"event-list ordinal overflow")?;
                    let bypass=match row.attribute("Bypass") {None|Some("0"|"false")=>false,Some("1"|"true")=>true,_=>return Err("invalid event processor saved bypass".into())};
                    events.push(input_authority::EventShape {id:sampler_ir::SourceEventProcessorId::UviNode(id),ordinal,kind:row.tag_name().name().to_owned(),bypass});
                }
                self.shared.owner_shapes.borrow_mut().push(input_authority::OwnerShape {id:owner_id,parent:owner_parent,kind:owner_kind,events});
            }
            let view=Rc::new(input_authority::ProcessorView::default());
            for source in std::iter::once(processor).chain(processor.children().filter(|n|n.has_tag_name("ScriptData"))) {
                for attr in source.attributes(){view.saved.borrow_mut().insert(attr.name().to_owned(),attr.value().to_owned());}
            }
            let environment=if self.shared.processors.borrow().is_empty(){bootstrap.clone()}else{
                let pristine:Table=self.lua.registry_value(self.shared.pristine_globals.borrow().as_ref().ok_or("missing pristine globals")?).map_err(lua_error)?;
                input_authority::fresh_environment(&self.lua,&pristine).map_err(lua_error)?
            };
            let index=self.shared.processors.borrow().len();
            let saved_bypass=processor.attribute("Bypass").is_some_and(|v|v=="1"||v=="true");
            self.shared.processors.borrow_mut().push(input_authority::ProcessorContext {
                address,source_node:node,environment:self.lua.create_registry_value(environment.clone()).map_err(lua_error)?,view,program_level,
            });
            let _context=input_authority::enter(&self.lua,&self.shared,Some(index)).map_err(lua_error)?;
            if index>0 {
                self.shared.script_bytes.set(self.shared.script_bytes.get().checked_add(PRELUDE.len()).ok_or("aggregate script byte overflow")?);
                self.install().map_err(lua_error)?;
            }
            environment.raw_set("Program",program.clone()).map_err(lua_error)?;
            // Objects retain the one authoritative graph's class identity.
            environment.raw_set("__element_mt",element_class.clone()).map_err(lua_error)?;
            environment.raw_set("__list_mt",list_class.clone()).map_err(lua_error)?;
            environment.raw_set("this",objects.get(&(node as usize)).ok_or("canonical ScriptProcessor element unavailable")?.clone()).map_err(lua_error)?;
            // The processor's module remains loaded while bypassed. Saved
            // bypass still suppresses the historical initialization path below;
            // exact native onInit-while-bypassed behavior is not claimed here.
            let mut scripts=processor.children().filter(|n|n.has_tag_name("script"));
            let script=scripts.next();
            if scripts.next().is_some(){return Err("multiple script bodies in one ScriptProcessor".into());}
            if let Some(script)=script {
                let text=script.text().unwrap_or_default();
                self.shared.note_assigned(text);
                self.shared.arm(self.shared.config.load,self.shared.config.load_work);
                if !text.trim().is_empty() {
                    let compile_profile=self.shared.profile.span("root_script_compile");
                    let chunk_name=if self.shared.inspection.borrow().is_some(){format!("uvi-script-{}",script.id().get_usize())}else{"script".into()};
                    self.shared.profile.register_chunk(&chunk_name);
                    let mut chunk=self.lua.load(text).set_name(&chunk_name).set_environment(environment.clone());
                    if self.shared.inspection.borrow().is_some(){chunk=chunk.set_compiler(mlua::chunk::Compiler::default().set_coverage_level(1));}
                    let function=chunk.into_function().map_err(lua_error)?;
                    if let Some(inspection)=self.shared.inspection.borrow_mut().as_mut(){inspection.register(crate::parity::script_id(script),chunk_name,text,function.clone());}
                    drop(compile_profile);
                    let _body_profile=self.shared.profile.span("script_body_and_require");
                    let thread=self.lua.create_thread(function).map_err(lua_error)?;
                    if self.shared.staged_preparation.get(){
                        self.resume_prepared_chunk(thread,MultiValue::new())?;
                    }else{
                        resume(&self.lua,&self.shared,thread,MultiValue::new(),PlayCreator::None,Some(index));
                        self.cycle();
                    }
                }
            }
            if saved_bypass {continue;}
            let restore_profile=self.shared.profile.span("saved_ui_restore");
            if let Ok(Value::Function(f))=environment.raw_get::<Value>("__restore") {
                let thread=self.lua.create_thread(f).map_err(lua_error)?;
                if self.shared.staged_preparation.get(){self.resume_prepared_chunk(thread,MultiValue::new())?;}
                else{resume(&self.lua,&self.shared,thread,MultiValue::new(),PlayCreator::None,Some(index));self.cycle();}
            }
            let saved=state.and_then(|state|state.for_processor(node,index==0));
            let before_restore=self.shared.restoring_widget_values.replace(self.shared.staged_preparation.get());
            let restored=(||->Result<(),String>{
                if let Some(state)=saved {self.restore_ui_values(&state)?;self.restore_ui_custom(&state)?;}
                else if let Some(custom)=processor.children().find(|n|n.has_tag_name("state")) {
                    self.restore_ui_custom(&UiState {custom:Some(ui::json_state(custom.text().unwrap_or_default())?),..Default::default()})?;
                }Ok(())
            })();
            self.shared.restoring_widget_values.set(before_restore);restored?;
            drop(restore_profile);
            let _init_profile=self.shared.profile.span("on_init_and_deferred_callbacks");
            if !self.shared.staged_preparation.get(){self.call("onInit",None);}
        }
        Ok(())
    }

    /// A synchronous preparation chunk has no rendered clock or Core turn.
    /// Preserve actual independent spawn/run rows for subsequent genuine task
    /// adoption; never execute their deferred suffix from a load-time cycle.
    fn resume_prepared_chunk(&self,thread:Thread,args:MultiValue)->Result<(),String>{
        let before=self.shared.creator.replace(PlayCreator::None);
        let was_thread=self.shared.in_thread.replace(true);
        let result=thread.resume::<MultiValue>(args);
        self.shared.in_thread.set(was_thread);self.shared.creator.set(before);
        result.map_err(lua_error)?;
        if thread.is_resumable(){return Err("main/restore chunk yielded without a rendered Core task context".into());}
        if self.shared.waiting.borrow().len().checked_add(self.shared.deferred.borrow().len()).is_none_or(|n|n>4096){return Err("prepared independent task capacity".into());}
        Ok(())
    }

    fn checked_prepared_origin(&self,index:Option<usize>,origin:&input_authority::PreparedLuaOrigin)->Result<sampler_core::PreparedGlobalTaskOrigin,String>{
        if let input_authority::PreparedLuaOrigin::RestoredWidget(widget)=origin {
            let contexts=self.shared.processors.borrow();
            let context=contexts.get(index.ok_or("restored widget processor missing")?).ok_or("restored widget processor unavailable")?;
            let environment=self.lua.registry_value::<Table>(&context.environment).map_err(lua_error)?;
            drop(contexts);
            let registry=environment.raw_get::<Table>("__ui").and_then(|ui|ui.raw_get::<Table>("widgets")).map_err(lua_error)?;
            let mut found=false;
            for actual in registry.sequence_values::<Table>() {
                self.shared.consume_work().map_err(lua_error)?;
                if actual.map_err(lua_error)?.to_pointer()==widget.to_pointer(){found=true;break;}
            }
            if !found{return Err("restored widget root no longer belongs to its canonical processor registry".into());}
        }
        origin.core_origin()
    }

    /// Freeze actual staged rows on their sole Lua owner. No authored function
    /// is run here and no Core authority is manufactured from these positions.
    pub(crate) fn prepared_root_tasks(&mut self)->Result<Vec<PreparedRootTask>,String>{
        use sampler_core::SourceTaskFamily as Family;
        use input_authority::{PreparedLuaRoot,PreparedRootTask};
        if !self.shared.canonical_tasks||self.shared.staged_preparation.get(){return Err("not a completed staged preparation".into());}
        if let Some(roots)=self.shared.prepared_roots.borrow().as_ref(){if roots.iter().any(|root|root.behavior.is_some()){return Err("prepared task roots already bound".into());}return Ok(roots.iter().map(|root|root.descriptor).collect());}
        let waiting=self.shared.waiting.borrow();let deferred=self.shared.deferred.borrow();
        let count=waiting.len().checked_add(deferred.len()).filter(|n|*n<=4096).ok_or("prepared task capacity")?;
        let contexts=self.shared.processors.borrow();
        let processor=|index:Option<usize>|contexts.get(index.ok_or("prepared task has no actual processor")?).map(|row|row.address.processor).ok_or("prepared task processor missing");
        for row in waiting.iter(){
            processor(row.processor)?;
            let origin=self.checked_prepared_origin(row.processor,&row.prepared_origin)?;
            if row.release||!matches!((row.creator,origin),(PlayCreator::None,sampler_core::PreparedGlobalTaskOrigin::None)|(PlayCreator::Event,sampler_core::PreparedGlobalTaskOrigin::RestoredWidgetEvent)){return Err("prepared Event/release-wait creator needs its genuine captured or nonmusical task entry".into());}
            let frames=(row.due*self.shared.config.rate/1000.).ceil();
            if !row.due.is_finite()||row.due<=0.||!frames.is_finite()||frames<1.||frames>=u64::MAX as f64{return Err("invalid prepared residual wait".into());}
        }
        for (_,_,creator,index,origin) in deferred.iter(){processor(*index)?;let origin=self.checked_prepared_origin(*index,origin)?;
            if !matches!((*creator,origin),(PlayCreator::None,sampler_core::PreparedGlobalTaskOrigin::None)|(PlayCreator::Event,sampler_core::PreparedGlobalTaskOrigin::RestoredWidgetEvent)){return Err("prepared Event task needs its genuine captured or nonmusical creator".into());}}
        // Logical layout admission; existing Lua allocator limit still owns
        // thread/argument values. This is not a hard RSS/allocator ceiling.
        std::alloc::Layout::array::<PreparedLuaRoot>(count).map_err(|_|"prepared task layout")?;
        std::alloc::Layout::array::<PreparedRootTask>(count).map_err(|_|"prepared task descriptor layout")?;
        let mut roots=Vec::new();roots.try_reserve_exact(count).map_err(|_|"prepared task allocation")?;
        drop(waiting);drop(deferred);
        // All admission checks preceded moving any retained LuaThread.
        for row in self.shared.waiting.borrow_mut().drain(..){
            let descriptor=PreparedRootTask{row:roots.len(),processor:processor(row.processor)?,family:Family::Run,origin:row.prepared_origin.core_origin()?};
            roots.push(PreparedLuaRoot{origin:row.prepared_origin,descriptor,behavior:None,thread:row.thread,args:MultiValue::new(),delay:Some((row.due*self.shared.config.rate/1000.).ceil() as u64),remaining_work:row.remaining_work,exhausted:row.exhausted});
        }
        for (thread,args,_,index,origin) in self.shared.deferred.borrow_mut().drain(..){
            let descriptor=PreparedRootTask{row:roots.len(),processor:processor(index)?,family:Family::Spawned,origin:origin.core_origin()?};
            roots.push(PreparedLuaRoot{origin,descriptor,behavior:None,thread,args,delay:None,remaining_work:self.shared.config.load_work,exhausted:false});
        }
        let descriptors=roots.iter().map(|root|root.descriptor).collect();
        *self.shared.prepared_roots.borrow_mut()=Some(roots);Ok(descriptors)
    }
    pub(crate) fn bind_prepared_roots(&mut self,runtime:sampler_core::RuntimeId,plan:sampler_core::PlanId,bindings:Vec<input_authority::PreparedRootBinding>)->Result<(),String>{
        let owner=self.shared.authority.borrow();let owner=owner.as_ref().ok_or("prepared task authority missing")?;
        if owner.runtime!=runtime||owner.plan!=plan||owner.current().is_some()||owner.cancelled(){return Err("prepared task binding identity/current turn mismatch".into());}
        let mut roots=self.shared.prepared_roots.borrow_mut();let roots=roots.as_mut().ok_or("prepared tasks not frozen")?;
        if roots.len()!=bindings.len(){return Err("prepared task binding count".into());}
        for (at,binding) in bindings.iter().enumerate(){
            if binding.row!=at||roots[at].descriptor.row!=at||roots[at].behavior.is_some()||bindings[..at].iter().any(|old|old.behavior==binding.behavior){return Err("prepared task duplicate/stale binding".into());}
        }
        for (root,binding) in roots.iter_mut().zip(bindings){root.behavior=Some(binding.behavior);}
        Ok(())
    }

    /// Called only on the existing Lua owner, before engine publication. The
    /// transient proof came from ONE actual retained Core plan table, not from
    /// imported source numbers or a permanent second dispatch hierarchy.
    pub(crate) fn bind_owner_authority(&mut self,mut owner:OwnerAuthorityPort)->Result<(),String>{
        if !self.shared.canonical_tasks{return Err("external authority requires the genuine staged constructor; eager Init cannot be adopted".into());}
        if self.shared.authority.borrow().is_some(){return Err("external owner authority already bound".into());}
        let proof=owner.proof.take().ok_or("missing actual Core owner proof")?;
        let shapes=self.shared.owner_shapes.borrow();
        if proof.len()!=shapes.len(){return Err("actual Core/VM owner shape count differs".into());}
        for expected in shapes.iter(){
            let actual=proof.iter().find(|row|row.id==expected.id).ok_or("actual Core owner missing")?;
            if actual.parent!=expected.parent||actual.kind!=expected.kind||actual.events.len()!=expected.events.len(){return Err("actual Core/VM owner parent/kind/list differs".into());}
            for (actual,expected) in actual.events.iter().zip(&expected.events){
                if actual.id!=expected.id||actual.ordinal!=expected.ordinal||actual.kind!=expected.kind||actual.bypass!=expected.bypass{return Err("actual Core/VM processor ID/ordinal/kind/initial field differs".into());}
            }
        }
        drop(shapes);drop(proof);owner.bind_current_thread();*self.shared.authority.borrow_mut()=Some(owner);Ok(())
    }
    pub(crate) fn external_fault(&self,error:&str){self.shared.find("lua error",error);}
    pub(crate) fn requires_external_authority(&self)->bool{self.shared.canonical_tasks}
    pub fn external_runtime(&self)->Option<sampler_core::RuntimeId>{self.shared.authority.borrow().as_ref().map(|owner|owner.runtime)}

    /// Ordered owner reset after Core has canceled old continuations at H.
    /// Drop actual coroutine values here, on the Lua owner. Physical held keys
    /// remain owed to their retained source terminal; no synthetic key-up occurs.
    pub(crate) fn reset_execution_barrier(&mut self,runtime:sampler_core::RuntimeId,plan:sampler_core::PlanId)->Result<(),sampler_core::Error>{
        let mut authority=self.shared.authority.borrow_mut();
        authority.as_mut().ok_or(sampler_core::Error::InvalidInput)?.reset_barrier(runtime,plan)?;
        drop(authority);
        self.shared.external_waiting.borrow_mut().clear();
        self.shared.waiting.borrow_mut().clear();
        self.shared.deferred.borrow_mut().clear();
        self.shared.prepared_roots.borrow_mut().take();
        self.shared.commands.borrow_mut().clear();
        self.shared.creator.set(PlayCreator::None);
        *self.shared.prepared_origin.borrow_mut()=input_authority::PreparedLuaOrigin::None;
        self.shared.restoring_widget_values.set(false);
        Ok(())
    }

    pub(crate) fn bind_source_property_owner(&mut self,owner:OwnerSourcePropertyPort)->Result<(),String>{
        if self.shared.canonical_tasks||self.shared.authority.borrow().is_some()||self.shared.source_properties.borrow().is_some(){return Err("property-only fallback forbidden for captured/product owner".into());}
        if owner.definitions.len()!=self.shared.constant_owners.borrow().len(){return Err("incomplete actual Constant property roster".into());}
        for definition in &owner.definitions {
            let index=self.shared.nodes.borrow().iter().position(|(node,_)|*node==Some(definition.node as usize)).ok_or("actual Constant definition missing")?;
            if self.shared.kinds.borrow().get(index).is_none_or(|kind|kind!="ConstantModulation")
                ||!self.shared.params.borrow()[index].iter().any(|(name,_)|name=="ScriptLive") {return Err("actual property class/promotion mismatch".into());}
            let key=crate::engine_parameters::key(definition.node as usize,"ConstantModulation","Value");
            let style=crate::engine_parameters::key(definition.node as usize,"ConstantModulation","Style");
            if sampler_core::lower::ir_control_id(&key)!=definition.value||sampler_core::lower::ir_control_id(&style)!=definition.style{return Err("actual property control identity mismatch".into());}
            // Actual owner shape is retained from the same parsed Host generation.
            if !self.shared.constant_owners.borrow().contains(&(definition.node,definition.owner)){return Err("actual property structural owner mismatch".into());}
            let polarity=self.shared.params.borrow()[index].iter().find(|(field,_)|field=="Bipolar").map(|(_,text)|text.parse::<f64>().ok().filter(|value|value.is_finite())).unwrap_or(Some(0.)).ok_or("actual Constant Bipolar is malformed")?!=0.;
            if polarity!=definition.bipolar{return Err("actual Constant polarity differs from compiled definition".into());}
        }
        *self.shared.source_properties.borrow_mut()=Some(owner);Ok(())
    }

    /// Execute exactly the real canonical segment selected for this processor.
    /// This runs only on the existing non-RT Lua owner. Core remains parked at
    /// turn.resume_frame until the explicit operation receipt has been ACKed.
    pub fn deliver_external_turn(&mut self,turn:sampler_core::ExternalTurn)->Result<(),String>{
        let result=self.deliver_controller_turn(turn);
        if result.is_err(){
            if let Some(owner)=self.shared.authority.borrow_mut().as_mut(){let _=owner.call(AuthorityOperation::Cancel);}
        }
        result
    }
    fn deliver_controller_turn(&mut self,turn:sampler_core::ExternalTurn)->Result<(),String>{
        use sampler_core::{ExternalEvent,ExternalOutput};
        let index=self.shared.processors.borrow().iter().position(|p|p.address.processor==turn.processor).ok_or("unknown actual processor")?;
        self.shared.authority.borrow_mut().as_mut().ok_or("missing actual Core authority")?.begin(turn).map_err(|e|format!("Core turn admission: {e:?}"))?;
        // Routing metadata is not liveness. Nothing below may touch the owner
        // clock, retained waiting rows, processor cache or authored Lua before ACK.
        if !matches!(self.external_operation(AuthorityOperation::Validate)?,AuthorityValue::Validated){return Err("invalid initial Running receipt".into());}
        let now=turn.resume_frame as f64*1000./self.shared.config.rate;
        if !now.is_finite(){return Err("invalid external frame/rate".into());}
        self.shared.now.set(now);
        let waiting={let mut rows=self.shared.external_waiting.borrow_mut();rows.iter().position(|row|row.behavior==turn.input.behavior()).map(|at|rows.remove(at))};
        let lua=self.lua.clone();let shared=self.shared.clone();
        let _context=input_authority::enter(&lua,&shared,Some(index)).map_err(lua_error)?;
        let prepared_at={let rows=self.shared.prepared_roots.borrow();rows.as_ref().and_then(|rows|rows.iter().position(|row|row.behavior==Some(turn.input.behavior())))};
        if let Some(at)=prepared_at {
            let rows=self.shared.prepared_roots.borrow();let root=&rows.as_ref().ok_or("prepared root store missing")?[at];
            let task=match turn.event{ExternalEvent::Task(task)=>task,_=>return Err("prepared root received non-task turn".into())};
            let family=match task.reason{sampler_core::SourceTaskReason::Spawned=>sampler_core::SourceTaskFamily::Spawned,sampler_core::SourceTaskReason::Run=>sampler_core::SourceTaskFamily::Run,_=>return Err("prepared root task reason changed".into())};
            let expected=match root.descriptor.origin {
                sampler_core::PreparedGlobalTaskOrigin::None=>sampler_core::SourceTaskCreator::None,
                sampler_core::PreparedGlobalTaskOrigin::RestoredWidgetEvent=>sampler_core::SourceTaskCreator::NonMusicalEvent(sampler_core::NonMusicalTaskOrigin::RestoredWidget),
            };
            if root.descriptor.processor!=turn.processor||root.descriptor.family!=family||task.creator!=expected{return Err("prepared root Core creator/processor mismatch".into());}
        }
        let prepared=if let Some(at)=prepared_at {
            let mut rows=self.shared.prepared_roots.borrow_mut();let rows=rows.as_mut().ok_or("prepared root store changed")?;
            if at>=rows.len(){return Err("prepared root row changed".into());}Some(rows.remove(at))
        }else{None};
        let (thread,projection,args,creator,origin,wall)=if let Some(root)=prepared{
            let creator=if root.descriptor.origin==sampler_core::PreparedGlobalTaskOrigin::None{PlayCreator::None}else{PlayCreator::Event};
            self.shared.remaining_work.set(root.remaining_work);self.shared.work_exhausted.set(root.exhausted);
            let wall=self.shared.config.load;self.shared.deadline.set(Instant::now().checked_add(wall));
            if let Some(delay)=root.delay{
                let next=turn.resume_frame.checked_add(delay).ok_or("prepared residual wait overflow")?;
                let mut rows=self.shared.external_waiting.borrow_mut();if rows.len()>=4096{return Err("external coroutine capacity".into());}rows.try_reserve(1).map_err(|_|"external coroutine allocation")?;drop(rows);
                _context.restore().map_err(lua_error)?;self.external_operation(AuthorityOperation::Complete{next_frame:Some(next)})?;
                self.shared.external_waiting.borrow_mut().push(input_authority::ExternalWaiting{behavior:turn.input.behavior(),processor:turn.processor,thread:root.thread,projection:None,creator,origin:root.origin,wall,remaining_work:root.remaining_work,exhausted:root.exhausted});
                return Ok(());
            }
            (root.thread,None,root.args,creator,root.origin,wall)
        }else if let Some(waiting)=waiting{
            if waiting.processor!=turn.processor{return Err("external resume processor changed".into());}
            self.shared.remaining_work.set(waiting.remaining_work);self.shared.work_exhausted.set(waiting.exhausted);
            // Sleeping time is Core-rendered time, not Lua execution time. A
            // new segment gets a wall deadline without refilling callback fuel.
            self.shared.deadline.set(Instant::now().checked_add(waiting.wall));
            (waiting.thread,waiting.projection,MultiValue::new(),waiting.creator,waiting.origin,waiting.wall)
        }else{
            match turn.event{
                ExternalEvent::Task(task)=>{
                    use sampler_core::{SourceTaskCreator,SourceTaskReason};
                    let (name,args,init)=match task.reason{
                        SourceTaskReason::Init(host)=>{
                            self.set_task_host_context(host,turn.resume_frame)?;
                            ("onInit",MultiValue::new(),true)
                        },
                        SourceTaskReason::Transport(host)=>{
                            self.set_task_host_context(host,turn.resume_frame)?;
                            ("onTransport",std::iter::once(Value::Boolean(host.playing)).collect(),false)
                        },
                        // The actual staged/parent-spawned LuaThread must be
                        // joined by real BehaviorId; there is no handler name
                        // from which a missing coroutine can be reconstructed.
                        SourceTaskReason::Spawned|SourceTaskReason::Run=>return Err("actual independent Lua coroutine adoption pending".into()),
                    };
                    let creator=match task.creator{SourceTaskCreator::None=>PlayCreator::None,SourceTaskCreator::Event|SourceTaskCreator::NonMusicalEvent(_)=>PlayCreator::Event};
                    let handler=match lua.globals().raw_get::<Value>(name).map_err(lua_error)?{Value::Function(handler)=>Some(handler),_=>None};
                    let Some(handler)=handler else{_context.restore().map_err(lua_error)?;self.external_operation(AuthorityOperation::Complete{next_frame:None})?;return Ok(());};
                    self.shared.arm(if init{self.shared.config.load}else{self.shared.config.callback},if init{self.shared.config.load_work}else{self.shared.config.callback_work});
                    (lua.create_thread(handler).map_err(lua_error)?,None,args,creator,input_authority::PreparedLuaOrigin::None,if init{self.shared.config.load}else{self.shared.config.callback})
                },
                ExternalEvent::Controller{source,number,value,poly_key}=>{
                    if self.processor_bypassed(turn.processor)?||number<128&&!self.shared.processor_view.borrow().visibility.receives(number){
                        self.external_operation(AuthorityOperation::Post(ExternalOutput::Original))?;
                        _context.restore().map_err(lua_error)?;self.external_operation(AuthorityOperation::Complete{next_frame:None})?;return Ok(());
                    }
                    if number<128{self.shared.processor_view.borrow().cc.borrow_mut()[usize::from(number)]=(value>>25) as u8;}
                    let projection=Rc::new(input_authority::ControllerProjection::new_with_channel(&lua,source,turn.logical_channel.ok_or("captured logical channel missing")?,number,value,poly_key).map_err(lua_error)?);
                    let globals=lua.globals();
                    let handler=match globals.raw_get::<Value>("onEvent").map_err(lua_error)?{
                        Value::Function(handler)=>Some(handler),_=>match globals.raw_get::<Value>(projection.handler).map_err(lua_error)?{Value::Function(handler)=>Some(handler),_=>None},
                    };
                    let Some(handler)=handler else{
                        self.external_operation(AuthorityOperation::Post(ExternalOutput::Original))?;
                        _context.restore().map_err(lua_error)?;self.external_operation(AuthorityOperation::Complete{next_frame:None})?;return Ok(());
                    };
                    self.shared.arm(self.shared.config.callback,self.shared.config.callback_work);
                    let args=std::iter::once(Value::Table(projection.table.clone())).collect();
                    (lua.create_thread(handler).map_err(lua_error)?,Some(projection),args,PlayCreator::Event,input_authority::PreparedLuaOrigin::UnclassifiedEvent,self.shared.config.callback)
                },
                ExternalEvent::ProgramChange{source,program,bank,..}=>{
                    if self.processor_bypassed(turn.processor)?{
                        self.external_operation(AuthorityOperation::Post(ExternalOutput::Original))?;
                        _context.restore().map_err(lua_error)?;self.external_operation(AuthorityOperation::Complete{next_frame:None})?;return Ok(());
                    }
                    let projection=Rc::new(input_authority::ControllerProjection::program(&lua,source,turn.logical_channel.ok_or("captured logical channel missing")?,program,bank).map_err(lua_error)?);
                    let globals=lua.globals();
                    let handler=match globals.raw_get::<Value>("onEvent").map_err(lua_error)?{
                        Value::Function(handler)=>Some(handler),_=>match globals.raw_get::<Value>(projection.handler).map_err(lua_error)?{Value::Function(handler)=>Some(handler),_=>None},
                    };
                    let Some(handler)=handler else{
                        self.external_operation(AuthorityOperation::Post(ExternalOutput::Original))?;
                        _context.restore().map_err(lua_error)?;self.external_operation(AuthorityOperation::Complete{next_frame:None})?;return Ok(());
                    };
                    self.shared.arm(self.shared.config.callback,self.shared.config.callback_work);
                    let args=std::iter::once(Value::Table(projection.table.clone())).collect();
                    (lua.create_thread(handler).map_err(lua_error)?,Some(projection),args,PlayCreator::Event,input_authority::PreparedLuaOrigin::UnclassifiedEvent,self.shared.config.callback)
                },
                _=>return Err("note canonical context bridge not yet installed".into()),
            }
        };
        *self.shared.external_projection.borrow_mut()=projection.clone();
        let before=self.shared.creator.replace(creator);let before_origin=self.shared.prepared_origin.replace(origin.clone());let was_thread=self.shared.in_thread.replace(true);
        let result=thread.resume::<MultiValue>(args);
        self.shared.in_thread.set(was_thread);self.shared.creator.set(before);*self.shared.prepared_origin.borrow_mut()=before_origin;
        self.shared.external_projection.borrow_mut().take();
        let values=result.map_err(lua_error)?;
        // Independent tasks require their own real Core Continuation. Never let
        // an external spawn escape onto the legacy wall-clock polling path.
        if !self.shared.deferred.borrow().is_empty(){return Err("external independent task continuation bridge pending".into());}
        if thread.is_resumable(){
            let ms=values.front().and_then(number).ok_or("external wait needs a positive numeric duration")?;
            if !ms.is_finite()||ms<=0.{return Err("external wait duration must be greater than zero".into());}
            let frames=(ms*self.shared.config.rate/1000.).ceil();
            if !frames.is_finite()||frames<1.||frames>=u64::MAX as f64{return Err("external wait frame overflow".into());}
            let next=turn.resume_frame.checked_add(frames as u64).ok_or("external wait frame overflow")?;
            if self.shared.external_waiting.borrow().len()>=4096{return Err("external coroutine capacity".into());}
            _context.restore().map_err(lua_error)?;self.external_operation(AuthorityOperation::Complete{next_frame:Some(next)})?;
            self.shared.external_waiting.borrow_mut().push(input_authority::ExternalWaiting{behavior:turn.input.behavior(),processor:turn.processor,thread,projection,creator,origin,wall,remaining_work:self.shared.remaining_work.get(),exhausted:self.shared.work_exhausted.get()});
        }else{_context.restore().map_err(lua_error)?;self.external_operation(AuthorityOperation::Complete{next_frame:None})?;}
        Ok(())
    }
    fn set_task_host_context(&self,host:sampler_core::SourceTaskHostContext,frame:u64)->Result<(),String>{
        if host.frame!=frame||!host.tempo.is_finite()||host.tempo<=0.{return Err("invalid actual task host context".into());}
        // A real Core task supplies this actual local F. Do not dispatch another
        // callback or infer an absolute rack clock while updating the API view.
        let at=frame as f64*1000./self.shared.config.rate;
        if !at.is_finite(){return Err("invalid actual task frame/rate".into());}
        let (anchor,beat)=self.shared.beat_anchor.get();
        let current=beat+(at-anchor)*self.shared.tempo.get()/60_000.;
        self.shared.beat_anchor.set((at,current));
        self.shared.tempo.set(host.tempo);self.shared.playing.set(host.playing);Ok(())
    }
    fn external_operation(&self,operation:AuthorityOperation)->Result<AuthorityValue,String>{
        self.shared.authority.borrow_mut().as_mut().ok_or("missing actual Core authority")?.call(operation).map_err(|error|format!("Core authority operation: {error:?}"))
    }


    /// Actual source addresses, never filtered script ordinals or VM IDs.
    pub fn processors(&self)->Vec<ProcessorAddress> {
        self.shared.processors.borrow().iter().map(|p|p.address).collect()
    }

    /// Offaudio shape proof against the actual initialized translation. The
    /// caller must still bind this proof to its real PlanId/Arc epoch and Core
    /// selection transaction; document-local numbers alone are not a lease.
    pub fn validate_owner_contract(&self,owners:&[sampler_ir::SourceOwner])->Result<(),String> {
        let contexts=self.shared.processors.borrow();
        let script_rows=owners.iter().flat_map(|owner|owner.event_processors.iter()).filter(|row|row.kind=="ScriptProcessor").count();
        if script_rows!=contexts.len(){return Err("initialized ScriptProcessor count changed".into());}
        for shape in self.shared.owner_shapes.borrow().iter() {
            let owner=owners.iter().find(|owner|owner.id==shape.id).ok_or("initialized script owner missing")?;
            if owner.parent!=shape.parent||owner.kind!=shape.kind||owner.event_processors.len()!=shape.events.len(){return Err("initialized script owner/list shape changed".into());}
            for (actual,expected) in owner.event_processors.iter().zip(&shape.events) {
                if actual.id!=expected.id||actual.owner!=shape.id||actual.ordinal!=expected.ordinal||actual.kind!=expected.kind||actual.bypass.saved!=expected.bypass {
                    return Err("initialized event-processor identity/order/kind/bypass changed".into());
                }
            }
        }
        Ok(())
    }

    /// Deliver to the exact processor selected by the canonical topology stage.
    /// This method does not broadcast to sibling scopes or decide Core routing.
    pub fn processor_input(&mut self,processor:sampler_ir::SourceEventProcessorId,input:crate::scripted::HostInput)->Result<ProcessorInputDisposition,String> {
        let index=self.shared.processors.borrow().iter().position(|p|p.address.processor==processor).ok_or("unknown ScriptProcessor identity")?;
        if self.processor_bypassed(processor)?{return Ok(input_authority::ProcessorInputDisposition::Bypassed);}
        let lua=self.lua.clone();let shared=self.shared.clone();
        let _context=input_authority::enter(&lua,&shared,Some(index)).map_err(lua_error)?;
        crate::scripted::Script::input(self,input);
        Ok(input_authority::ProcessorInputDisposition::CallbackDispatched)
    }

    /// The actual accepted unmodulated owner parameter, read at delivery. No
    /// source snapshot, Lua raw-property override or parallel cached bool gates
    /// routing. An actual bound modulation/control must be queried at Core F by
    /// the future binding join instead of treating this target as its signal.
    pub fn processor_bypassed(&self,processor:sampler_ir::SourceEventProcessorId)->Result<bool,String> {
        let contexts=self.shared.processors.borrow();
        let context=contexts.iter().find(|p|p.address.processor==processor).ok_or("unknown ScriptProcessor identity")?;
        let nodes=self.shared.nodes.borrow();
        let source_node=usize::try_from(context.source_node).map_err(|_|"ScriptProcessor source identity is out of range")?;
        let mut matches=nodes.iter().enumerate().filter(|(_,row)|row.0==Some(source_node));
        let (id,_)=matches.next().ok_or("canonical ScriptProcessor owner unavailable")?;
        if matches.next().is_some()||self.shared.kinds.borrow().get(id).is_none_or(|kind|kind!="ScriptProcessor"){return Err("ambiguous ScriptProcessor owner".into());}
        let params=self.shared.params.borrow();
        match params.get(id).and_then(|params|params.iter().find(|(name,_)|name=="Bypass")).map(|(_,value)|value.as_str()) {
            None|Some("0"|"false")=>Ok(false),Some("1"|"true")=>Ok(true),_=>Err("invalid accepted ScriptProcessor.Bypass value".into()),
        }
    }

    pub fn processor_global_text(&self,processor:sampler_ir::SourceEventProcessorId,name:&str)->Result<String,String> {
        let index=self.shared.processors.borrow().iter().position(|p|p.address.processor==processor).ok_or("unknown ScriptProcessor identity")?;
        let _context=input_authority::enter(&self.lua,&self.shared,Some(index)).map_err(lua_error)?;
        Ok(self.global_text(name))
    }

    /// The single non-bypassed owner for the document-level legacy input
    /// entry points. A bypassed ScriptProcessor receives no callback, so it
    /// may neither swallow the input nor make an active sibling ambiguous.
    /// `None` means every processor is bypassed.
    fn default_processor(&self)->mlua::Result<Option<input_authority::ContextGuard<'_>>> {
        if let Some(current)=self.shared.processor.get(){
            return input_authority::enter(&self.lua,&self.shared,Some(current)).map(Some);
        }
        let contexts=self.shared.processors.borrow();
        let mut active=None;
        for (index,context) in contexts.iter().enumerate() {
            if self.processor_bypassed(context.address.processor).map_err(|error|mlua::Error::runtime(error))?{continue;}
            if active.replace(index).is_some(){return Err(mlua::Error::runtime("multiple ScriptProcessors require exact topology-stage delivery"));}
        }
        match active {
            Some(index)=>input_authority::enter(&self.lua,&self.shared,Some(index)).map(Some),
            None=>Ok(None),
        }
    }

    /// Enter the single legacy input owner, reporting a routing refusal once
    /// and yielding `None` when every processor is bypassed.
    fn begin_input(&self)->Option<input_authority::ContextGuard<'_>> {
        match self.default_processor() {
            Ok(guard)=>guard,
            Err(error)=>{self.shared.find("lua input routing",&lua_error(error));None}
        }
    }

    /// Start spawned threads until none remain.
    fn cycle(&self) {
        if self.shared.canonical_tasks{return;}
        #[cfg(feature = "scan")]
        let _owner_span = self.owner_span(diagnostics::OwnerStage::DeferredCycle);
        loop {
            let batch = std::mem::take(&mut *self.shared.deferred.borrow_mut());
            if batch.is_empty() {
                break;
            }
            for (thread, args, creator, processor,origin) in batch {
                #[cfg(feature = "scan")]
                self.owner_count(diagnostics::OwnerCounter::DeferredResumes, 1);
                if self.shared.consume_work().is_err() {
                    self.shared.deferred.borrow_mut().clear();
                    self.shared.find("lua error", "work budget exceeded");
                    return;
                }
                resume_with_prepared_origin(&self.lua, &self.shared, thread, args, creator, processor,origin);
            }
        }
    }

    /// Whether the scripts handle note-ons themselves (the original attack is
    /// then theirs to replay).
    pub fn handles_notes(&self) -> bool {
        let globals = self.lua.globals();
        ["onNote", "onEvent"]
            .iter()
            .any(|name| matches!(globals.raw_get::<Value>(*name), Ok(Value::Function(_))))
    }

    fn call(&self, name: &str, event: Option<Table>) {
        let globals = self.lua.globals();
        // `onEvent` takes precedence over the specialized handlers.
        let handler = match (event.is_some(), globals.raw_get::<Value>("onEvent")) {
            (true, Ok(Value::Function(f))) => Some(f),
            _ => match globals.raw_get::<Value>(name) {
                Ok(Value::Function(f)) => Some(f),
                _ => None,
            },
        };
        let Some(f) = handler else {
            return;
        };
        // onInit is load work, including its spawned callbacks. The realtime
        // note/UI callback budget must not truncate the authored editor.
        self.shared.arm(
            if self.shared.initializing.get() {
                self.shared.config.load
            } else {
                self.shared.config.callback
            },
            if self.shared.initializing.get() {
                self.shared.config.load_work
            } else {
                self.shared.config.callback_work
            },
        );
        let Ok(thread) = self.lua.create_thread(f) else {
            return;
        };
        let creator = if name == "onNote" {
            event.as_ref().and_then(|e| field(e, "id"))
                .map_or(PlayCreator::None, |id| PlayCreator::NoteOn(id as u64))
        } else if event.is_some() { PlayCreator::Event } else { PlayCreator::None };
        let args: MultiValue = event.map(Value::Table).into_iter().collect();
        resume(&self.lua, &self.shared, thread, args, creator, self.shared.processor.get());
        self.cycle();
    }

    fn event(&self, kind: i32, fields: &[(&str, f64)]) -> mlua::Result<Table> {
        let table = self.lua.create_table()?;
        table.set("type", kind)?;
        for (name, value) in fields {
            table.set(*name, *value)?;
            if *name == "id" {
                table.set("voiceId", *value)?;
            }
        }
        Ok(table)
    }

    /// A note-on the host received, as the script's `onNote(e)`.
    pub fn note_on(&mut self, id: u64, key: u8, velocity: u8, channel: u8) {
        let Some(_processor)=self.begin_input() else{return;};
        self.shared.held.borrow_mut().insert(id, key & 127);
        self.shared.voices.borrow_mut().insert(id);
        self.shared.last_note_on.borrow_mut()[usize::from(key & 127)] = Some(self.shared.now.get());
        let e = self.event(
            1,
            &[
                ("id", id as f64),
                ("note", f64::from(key)),
                ("velocity", f64::from(velocity)),
                ("channel", f64::from(channel) + 1.0),
            ],
        );
        match e {
            Ok(e) => self.call("onNote", Some(e)),
            Err(e) => self.shared.find("onNote event", &lua_error(e)),
        }
    }

    /// A note-off for the note-on `id`: `onRelease(e)`, and wakes `waitForRelease`.
    pub fn note_off(&mut self, id: u64, key: u8, velocity: u8, channel: u8) {
        let processor=self.begin_input();
        self.shared.held.borrow_mut().remove(&id);
        let woken: Vec<Waiting> = {
            let mut waiting = self.shared.waiting.borrow_mut();
            let (woken, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut *waiting)
                .into_iter()
                .partition(|w| w.release && w.creator.note() == Some(id));
            *waiting = rest;
            woken
        };
        for w in woken {
            self.shared.arm(
                self.shared.config.callback,
                self.shared.config.callback_work,
            );
            resume_with_prepared_origin(&self.lua, &self.shared, w.thread, MultiValue::new(), w.creator, w.processor,w.prepared_origin);
        }
        // A bypassed processor still owns release bookkeeping, but receives no
        // callback; the waking continuations above keep their own identity.
        if processor.is_none() { self.cycle(); return; }
        let e = self.event(
            2,
            &[
                ("id", id as f64),
                ("note", f64::from(key)),
                ("velocity", f64::from(velocity)),
                ("channel", f64::from(channel) + 1.0),
            ],
        );
        match e {
            Ok(e) => self.call("onRelease", Some(e)),
            Err(e) => self.shared.find("onRelease event", &lua_error(e)),
        }
        self.cycle();
    }

    pub fn controller(&mut self, controller: u8, value: u8, channel: u8) {
        let Some(_processor)=self.begin_input() else{return;};
        if !self.shared.processor_view.borrow().visibility.receives(controller) {return;}
        self.shared.processor_view.borrow().cc.borrow_mut()[usize::from(controller & 127)] = value;
        let e = self.event(
            3,
            &[
                ("controller", f64::from(controller)),
                ("value", f64::from(value)),
                ("channel", f64::from(channel) + 1.0),
            ],
        );
        match e {
            Ok(e) => self.call("onController", Some(e)),
            Err(e) => self.shared.find("onController event", &lua_error(e)),
        }
    }

    fn deliver(&self, name: &str, kind: i32, fields: &[(&str, f64)]) {
        match self.event(kind, fields) {
            Ok(e) => self.call(name, Some(e)),
            Err(e) => self.shared.find(&format!("{name} event"), &lua_error(e)),
        }
    }

    /// Pitch bend in -1..=1.
    pub fn pitch_bend(&mut self, value: f64, channel: u8) {
        let Some(_processor)=self.begin_input() else{return;};
        self.deliver(
            "onPitchBend",
            4,
            &[
                ("value", value),
                ("bend", value),
                ("channel", f64::from(channel) + 1.0),
            ],
        );
    }

    pub fn after_touch(&mut self, value: u8, channel: u8) {
        let Some(_processor)=self.begin_input() else{return;};
        self.deliver(
            "onAfterTouch",
            5,
            &[
                ("value", f64::from(value)),
                ("channel", f64::from(channel) + 1.0),
            ],
        );
    }

    pub fn poly_after_touch(&mut self, key: u8, value: u8, channel: u8) {
        let Some(_processor)=self.begin_input() else{return;};
        self.deliver(
            "onPolyAfterTouch",
            6,
            &[
                ("note", f64::from(key)),
                ("value", f64::from(value)),
                ("channel", f64::from(channel) + 1.0),
            ],
        );
    }

    pub fn program_change(&mut self, value: u8, channel: u8) {
        let Some(_processor)=self.begin_input() else{return;};
        self.deliver(
            "onProgramChange",
            7,
            &[
                ("value", f64::from(value)),
                ("program", f64::from(value)),
                ("channel", f64::from(channel) + 1.0),
            ],
        );
    }

    /// The host's transport: `onTransport(playing)`.
    pub fn transport(&mut self, playing: bool) {
        let processor=self.begin_input();
        let (at, value) = self.shared.beat_anchor.get();
        let elapsed = if self.shared.playing.get() {
            (self.shared.now.get() - at) * self.shared.tempo.get() / 60_000.
        } else {
            0.
        };
        self.shared
            .beat_anchor
            .set((self.shared.now.get(), value + elapsed));
        self.shared.playing.set(playing);
        // Transport time is host state; only the callback belongs to a
        // non-bypassed processor owner.
        if processor.is_none() { return; }
        let Ok(Value::Function(f)) = self.lua.globals().raw_get::<Value>("onTransport") else {
            return;
        };
        self.shared.arm(
            self.shared.config.callback,
            self.shared.config.callback_work,
        );
        if let Ok(thread) = self.lua.create_thread(f) {
            let mut args = MultiValue::new();
            args.push_front(Value::Boolean(playing));
            resume(&self.lua, &self.shared, thread, args, PlayCreator::None, self.shared.processor.get());
            self.cycle();
        }
    }

    /// The host's tempo in beats per minute, for the beat conversions.
    pub fn set_tempo(&mut self, bpm: f64) {
        if bpm.is_finite() && bpm > 0.0 {
            self.shared.tempo.set(bpm);
        }
    }

    /// Move the clock to `now_ms`, resuming every thread whose wait ends by
    /// then, in time order. Commands carry the time they were issued.
    pub fn advance(&mut self, now_ms: f64) {
        // Staged initialization/deferred continuations are owned by actual
        // Core tasks. A thread poll cannot advance or resume them independently.
        if self.shared.canonical_tasks{return;}
        #[cfg(feature = "scan")]
        let scheduled_span = self.owner_span(diagnostics::OwnerStage::AdvanceScheduled);
        self.shared.arm(
            self.shared.config.callback,
            self.shared.config.callback_work,
        );
        loop {
            let next = {
                let mut waiting = self.shared.waiting.borrow_mut();
                let index = waiting
                    .iter()
                    .enumerate()
                    .filter(|(_, w)| !w.release && w.due <= now_ms)
                    .min_by(|(_, a), (_, b)| a.due.total_cmp(&b.due).then(a.seq.cmp(&b.seq)))
                    .map(|(i, _)| i);
                index.map(|i| waiting.swap_remove(i))
            };
            let Some(w) = next else { break };
            #[cfg(feature = "scan")]
            self.owner_count(diagnostics::OwnerCounter::ScheduledResumes, 1);
            self.shared.now.set(w.due.max(self.shared.now.get()));
            if self.shared.consume_work().is_err() {
                self.shared.find("lua error", "work budget exceeded");
                break;
            }
            resume_with_prepared_origin(&self.lua, &self.shared, w.thread, MultiValue::new(), w.creator, w.processor,w.prepared_origin);
            self.cycle();
        }
        self.shared.now.set(now_ms.max(self.shared.now.get()));
        #[cfg(feature = "scan")]
        drop(scheduled_span);
        {
            #[cfg(feature = "scan")]
            let _owner_span = self.owner_span(diagnostics::OwnerStage::AdvanceData);
            async_data::poll(self);
        }
        {
            #[cfg(feature = "scan")]
            let _owner_span = self.owner_span(diagnostics::OwnerStage::AdvanceSamples);
            sample_loading::poll(self);
        }
        self.cycle();
        #[cfg(feature = "scan")]
        if let Some(progress) = &self.shared.progress {
            progress.clock(self.shared.now.get());
        }
    }

    /// Install off audio on this Lua owner's dedicated source producer.
    pub fn bind_sample_control(&mut self, control: sampler_core::SampleControl) -> Result<(), String> {
        self.shared.sample_loads.borrow_mut().bind(control)
    }
    pub fn bind_sample_stream(&mut self, sources: sampler_kontakt::DynamicSources) -> Result<(), String> {
        self.shared.sample_loads.borrow_mut().stream(sources)
    }
    pub(crate) fn sample_collection_timer(&self) -> Option<std::time::Duration> {
        self.shared.sample_loads.borrow().collection_timer()
    }
    pub(crate) fn poll_sample_loads(&self) {
        sample_loading::poll(self);
        self.cycle();
    }

    /// Set the clock without resuming anything (a starting point).
    pub fn set_time(&mut self, now_ms: f64) {
        self.shared.now.set(now_ms);
        #[cfg(feature = "scan")]
        if let Some(progress) = &self.shared.progress {
            progress.clock(now_ms);
        }
    }

    /// The commands issued since the last call.
    /// A global of the scripts as text, for debugging surveys.
    #[doc(hidden)]
    pub fn global_text(&self, name: &str) -> String {
        match self.lua.globals().raw_get::<Value>(name) {
            Ok(Value::Nil) | Err(_) => "nil".into(),
            Ok(Value::Boolean(b)) => b.to_string(),
            Ok(Value::Integer(i)) => i.to_string(),
            Ok(Value::Number(n)) => n.to_string(),
            Ok(Value::String(s)) => s.to_string_lossy(),
            Ok(other) => other.type_name().into(),
        }
    }

    /// What the scripts wrote to insert elements while loading, as (XML node,
    /// attribute, value): the program's starting state, which the translator
    /// applies so the baked chains match what the scripts left them as.
    pub fn insert_overrides(&self) -> Vec<(usize, String, String)> {
        let mut out: Vec<(usize, String, String)> = self
            .shared
            .folded
            .borrow()
            .iter()
            .map(|((node, name), (_, value))| (*node, name.clone(), value.clone()))
            .collect();
        let Ok(touched) = self.lua.globals().raw_get::<Table>("__touched") else {
            return out;
        };
        let nodes = self.shared.nodes.borrow();
        for element in touched.sequence_values::<Table>().flatten() {
            let Ok(id) = element.raw_get::<usize>("__id") else {
                continue;
            };
            let Some(&(Some(node), insert)) = nodes.get(id) else {
                continue;
            };
            if !insert && !self.shared.live.borrow().get(id).is_some_and(|l| !l.is_empty()) {
                continue;
            }
            let Ok(set) = element.raw_get::<Table>("__set") else {
                continue;
            };
            for (name, value) in set.pairs::<String, Value>().flatten() {
                let value = match value {
                    Value::Boolean(b) => u8::from(b).to_string(),
                    Value::Integer(i) => i.to_string(),
                    Value::Number(n) if n.is_finite() => n.to_string(),
                    _ => continue,
                };
                out.push((node, name, value));
            }
        }
        // One edit per field: the last write wins.
        let mut seen = std::collections::HashSet::new();
        let mut last: Vec<_> = out.into_iter().rev().filter(|(node, name, _)| seen.insert((*node, name.clone()))).collect();
        last.reverse();
        last
    }

    /// The initialization writes folded into the program (no live path):
    /// (XML node, `Kind.Field`). A translation that never reads one reports it.
    pub fn folded_writes(&self) -> Vec<(usize, String, String)> {
        self.shared
            .folded
            .borrow()
            .iter()
            .map(|((node, name), (kind, _))| (*node, kind.clone(), name.clone()))
            .collect()
    }

    pub fn take_commands(&mut self) -> Vec<Command> {
        self.shared.rebuild_stale_impulses();
        let commands = std::mem::take(&mut *self.shared.commands.borrow_mut());
        self.shared.ready_command_charge.set(0);
        commands
    }

    /// Transfer a whole already-generated safe batch before an expensive UI
    /// snapshot. This never executes Lua, rebuilds impulses or splits a batch.
    /// The ordinary collector still owns every unsafe or non-fitting batch.
    pub(crate) fn take_ready_command_batch(&self, free: usize) -> Vec<Command> {
        if self.shared.ready_command_charge.get() != 0
            || !self.shared.stale_impulses.borrow().is_empty()
        {
            return Vec::new();
        }
        let mut commands = self.shared.commands.borrow_mut();
        if commands.is_empty() || commands.len() > free.min(1024) {
            return Vec::new();
        }
        if !commands.iter().all(|command| matches!(command,
            Command::Play(_) | Command::Release { .. } | Command::Modulation { .. }
            | Command::Change { .. } | Command::Fade { .. } | Command::Midi(_)
            | Command::Parameter { .. }
        )) {
            return Vec::new();
        }
        let batch = std::mem::take(&mut *commands);
        self.shared.ready_command_charge.set(batch.len());
        batch
    }

    /// The earliest time a waiting thread resumes, if any.
    pub fn next_due(&self) -> Option<f64> {
        self.shared
            .waiting
            .borrow()
            .iter()
            .filter(|w| !w.release)
            .map(|w| w.due)
            .min_by(f64::total_cmp)
    }

    pub fn fault_counts(&self) -> FaultCounts {
        self.shared.faults.borrow().clone()
    }

    /// Cumulative real per-line counts and bounded runtime/init faults. Empty
    /// for ordinary hosts. Offsets point to the first byte of each source line.
    pub fn coverage(&self) -> Vec<sampler_parity::Coverage> {
        self.shared
            .inspection
            .borrow()
            .as_ref()
            .map_or_else(Vec::new, |i| i.snapshots())
    }
    pub fn finding_revision(&self) -> u64 {
        self.shared.finding_revision.get()
    }

    /// Everything inert or failed so far.
    pub fn findings(&self) -> Vec<Finding> {
        self.shared.findings.borrow().values().cloned().collect()
    }
    #[cfg(feature = "scan")]
    pub fn scan_faults(&self) -> ScanFaults {
        self.shared.scan.borrow().clone()
    }
    #[cfg(feature = "scan")]
    pub fn scan_progress(&self) -> Option<std::sync::Arc<diagnostics::ScanProgress>> {
        self.shared.progress.clone()
    }
    #[cfg(feature = "scan")]
    pub(crate) fn owner_phase(&self, phase: diagnostics::OwnerPhase) {
        if let Some(progress) = &self.shared.progress {
            progress.phase(phase);
        }
    }

    #[cfg(feature = "scan")]
    pub(crate) fn install_owner_audit(&self, audit: Option<std::sync::Arc<diagnostics::OwnerAudit>>) {
        if let Some(audit) = audit { let _ = self.shared.owner_audit.set(audit); }
    }
    #[cfg(feature = "scan")]
    pub(crate) fn owner_audit(&self) -> Option<std::sync::Arc<diagnostics::OwnerAudit>> {
        self.shared.owner_audit.get().cloned()
    }
    #[cfg(feature = "scan")]
    pub(crate) fn owner_span(&self, stage: diagnostics::OwnerStage) -> Option<diagnostics::OwnerSpan<'_>> {
        self.shared.owner_audit.get().map(|audit| audit.span(stage))
    }
    #[cfg(feature = "scan")]
    pub(crate) fn owner_count(&self, counter: diagnostics::OwnerCounter, n: usize) {
        if let Some(audit) = self.shared.owner_audit.get() { audit.add(counter, n); }
    }

    pub fn memory(&self) -> usize {
        self.lua.used_memory()
    }
}

fn resume(lua:&Lua, shared: &Rc<Shared>, thread: Thread, args: MultiValue, creator: PlayCreator, processor:Option<usize>) {
    let origin=shared.prepared_origin.borrow().clone();
    resume_with_prepared_origin(lua,shared,thread,args,creator,processor,origin)
}
fn resume_with_prepared_origin(lua:&Lua,shared:&Rc<Shared>,thread:Thread,args:MultiValue,creator:PlayCreator,processor:Option<usize>,origin:input_authority::PreparedLuaOrigin){
    let _context=match input_authority::enter(lua,shared,processor) {
        Ok(context)=>context,
        Err(error)=>{shared.find("lua error",&lua_error(error));return;}
    };
    let previous_origin=shared.prepared_origin.replace(origin.clone());
    #[cfg(feature = "scan")]
    if let Some(audit) = shared.owner_audit.get() { audit.add(diagnostics::OwnerCounter::CoroutineResumes, 1); }
    #[cfg(feature = "scan")]
    if let Some(progress) = &shared.progress {
        progress.resume(shared.now.get());
    }
    let before = shared.creator.replace(creator);
    let was_thread = shared.in_thread.replace(true);
    let result = thread.resume::<MultiValue>(args);
    shared.in_thread.set(was_thread);
    shared.creator.set(before);*shared.prepared_origin.borrow_mut()=previous_origin;
    match result {
        Err(e) => shared.find("lua error", &lua_error(e)),
        Ok(values) => {
            if !thread.is_resumable() {
                return;
            }
            let (due, release) = match values.front() {
                Some(Value::String(s)) if s.as_bytes().as_ref() == b"release" => (0.0, true),
                Some(v) => (shared.now.get() + number(v).unwrap_or(0.0).max(0.0), false),
                None => (shared.now.get(), false),
            };
            shared.seq.set(shared.seq.get() + 1);
            shared.waiting.borrow_mut().push(Waiting {
                thread,
                due,
                seq: shared.seq.get(),
                creator,
                processor,
                release,
                remaining_work:shared.remaining_work.get(),
                exhausted:shared.work_exhausted.get(),
                prepared_origin:origin,
            });
        }
    }
}

fn parse_play(shared: &Shared, args: &[Value], origin: PlayOrigin) -> mlua::Result<Play> {
    let mut values: [Option<Value>; 11] = Default::default();
    match args {
        [Value::Table(t)] => {
            for (i, name) in [
                "note", "velocity", "duration", "layer", "channel", "input", "vol", "pan", "tune",
                "slice", "oscIndex",
            ]
            .iter()
            .enumerate()
            {
                values[i] = t
                    .get::<Value>(*name)
                    .ok()
                    .filter(|v| !v.is_nil())
                    .or_else(|| t.get::<Value>(i as i64 + 1).ok().filter(|v| !v.is_nil()));
            }
        }
        _ => {
            for (i, v) in args.iter().take(11).enumerate() {
                if !v.is_nil() {
                    values[i] = Some(v.clone());
                }
            }
        }
    }
    let num = |i: usize| values[i].as_ref().and_then(number);
    let mut layers = Layers::default();
    match &values[3] {
        Some(Value::Table(t)) => t
            .sequence_values::<f64>()
            .filter_map(Result::ok)
            .for_each(|n| layers.insert(n as u32)),
        Some(v) => number(v).into_iter().for_each(|n| layers.insert(n as u32)),
        None => {}
    }
    if values[4].is_some() || values[5].is_some() || values[9].is_some() {
        shared.find("lua playNote channel/input/slice", "");
    }
    // lua.uvi.net: > 0 releases after that long, -1 with the originating note,
    // 0 sends only the note-on (the script ends it with releaseVoice).
    let duration = num(2).filter(|d| *d >= 0.0);
    let creator = shared.creator.get();
    if origin == PlayOrigin::Helper && duration.is_none() && creator == PlayCreator::Widget {
        return Err(mlua::Error::runtime("playNote duration -1 requires a script thread in a widget changed callback"));
    }
    Ok(Play {
        id: shared.next_id(),
        at_ms: shared.now.get(),
        key: num(0).unwrap_or(0.0).clamp(0.0, 127.0) as u8,
        velocity: num(1).unwrap_or(100.0).clamp(1.0, 127.0) as u8,
        duration_ms: duration,
        layers,
        osc: num(10).map(|n| n as u32),
        vol: num(6).unwrap_or(1.0),
        pan: num(7).unwrap_or(0.0),
        tune: num(8).unwrap_or(0.0),
        creator,
        origin,
        parent: creator.note(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn explicit_init_profile_preserves_ordinary_state_without_retaining_script_text() {
        let xml = r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
local private_source_marker='PROFILE_PRIVATE_AUTHORED_LITERAL'
function onInit()
  local k=Knob('Fixture',0,0,10)
  k:setValue(7)
  initialized=private_source_marker
end
]]></script></ScriptProcessor></EventProcessors></Program>"#;
        let ordinary = super::ScriptHost::new(xml, (), super::Config::default()).unwrap();
        let profiled = super::ScriptHost::new_profiled(xml, (), super::Config::default(), false).unwrap();
        let covered = super::ScriptHost::new_profiled(xml, (), super::Config::default(), true).unwrap();
        assert_eq!(ordinary.interface(), profiled.interface());
        assert_eq!(ordinary.interface(), covered.interface());
        assert_eq!(ordinary.global_text("initialized"), profiled.global_text("initialized"));
        assert!(ordinary.initialization_profile().is_null());
        assert!(profiled.coverage().is_empty());
        assert!(!covered.coverage().is_empty());
        let report=profiled.initialization_profile();
        assert_eq!(report["coverage"], false);
        let stages=report["stages"].as_array().unwrap();
        for phase in ["host_create","xml_parse","root_script_compile","on_init_and_deferred_callbacks","ui_ir_export"] {
            assert!(stages.iter().any(|s| s["phase"] == phase && s["calls"].as_u64().unwrap() > 0), "{phase}");
        }
        assert!(!report.to_string().contains("PROFILE_PRIVATE_AUTHORED_LITERAL"));
        assert!(report["lua_samples"].as_array().unwrap().len()<=512);
    }

    #[test]
    fn image_resource_headers_are_cached_across_snapshots() {
        use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
        let reads = Arc::new(AtomicUsize::new(0));
        let count = reads.clone();
        let files = Scripts::default().with_images(move |path| {
            count.fetch_add(1, Ordering::Relaxed);
            if path != "overlay.png" { return None; }
            let mut bytes = vec![0; 33];
            bytes[..16].copy_from_slice(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
            bytes[16..20].copy_from_slice(&720u32.to_be_bytes());
            bytes[20..24].copy_from_slice(&18u32.to_be_bytes());
            Some(bytes)
        });
        let host = ScriptHost::new("<UVI4><Program><EventProcessors><ScriptProcessor><script>Image('overlay.png'); Image('overlay.png'); Image('missing.png')</script></ScriptProcessor></EventProcessors></Program></UVI4>", files, Config::default()).unwrap();
        for _ in 0..2 {
            assert_eq!(host.interface().widgets[0].rect.width, 720.);
        }
        assert_eq!(reads.load(Ordering::Relaxed), 2, "cache successes and absence by resource path");
    }

    #[test]
    fn finite_initialization_survives_expired_elapsed_budget() {
        let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>function onInit() for n=1,100 do assert(n&gt;0) end; initialized=true end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
        let host = super::ScriptHost::new(
            xml,
            (),
            super::Config {
                load: std::time::Duration::ZERO,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(host.global_text("initialized"), "true");
        assert!(host.fault_counts().init.is_empty());
    }

    #[test]
    fn finite_initialization_records_deterministic_work() {
        let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>function onInit() for n=1,100 do assert(n&gt;0) end end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
        let first = super::ScriptHost::new(xml, (), super::Config::default()).unwrap();
        let second = super::ScriptHost::new(xml, (), super::Config::default()).unwrap();
        assert!(first.shared.vm_checkpoints.get() > 100);
        assert_eq!(
            first.shared.vm_checkpoints.get(),
            second.shared.vm_checkpoints.get()
        );
        assert_eq!(
            first.shared.graph_nodes.get(),
            second.shared.graph_nodes.get()
        );
        assert_eq!(
            first.shared.script_bytes.get(),
            second.shared.script_bytes.get()
        );
    }

    #[test]
    fn graph_node_and_depth_work_are_bounded() {
        // roxmltree parses nested elements recursively, and its debug frames
        // overflow the test thread's 2 MiB at this depth (gdb: the overflow is
        // inside `roxmltree::tokenizer::parse_element`, before any host code).
        std::thread::Builder::new()
            .stack_size(32 << 20)
            .spawn(graph_node_and_depth_bounds)
            .unwrap()
            .join()
            .unwrap();
    }

    fn graph_node_and_depth_bounds() {
        let h = super::ScriptHost::new("<UVI4><Program/></UVI4>", (), super::Config::default())
            .unwrap();
        h.shared.graph_nodes.set(1 << 18);
        let doc = roxmltree::Document::parse("<UVI4><Program/></UVI4>").unwrap();
        assert!(
            h.build_program(&doc)
                .unwrap_err()
                .to_string()
                .contains("node/depth budget")
        );
        let xml = format!(
            "<UVI4><Program>{}<Layer/>{}</Program></UVI4>",
            "<Layers><Layer>".repeat(192),
            "</Layer></Layers>".repeat(192)
        );
        let error = super::ScriptHost::new(&xml, (), super::Config::default())
            .err()
            .unwrap();
        assert!(error.contains("node/depth budget"), "{error}");
    }

    #[test]
    fn initialization_phases_cannot_refill_exhausted_work() {
        let h = super::ScriptHost::new("<UVI4><Program/></UVI4>", (), super::Config::default())
            .unwrap();
        h.shared.initializing.set(true);
        h.shared.remaining_work.set(0);
        assert!(h.shared.consume_work().is_err());
        h.shared.arm(std::time::Duration::from_secs(20), 100);
        assert_eq!(h.shared.remaining_work.get(), 0);
        assert!(h.shared.work_exhausted.get());
        h.shared.initializing.set(false);
        h.shared.arm(std::time::Duration::from_secs(20), 100);
        assert_eq!(h.shared.remaining_work.get(), 100);
        assert!(!h.shared.work_exhausted.get());
    }

    #[test]
    fn ui_inspection_preserves_live_work_and_exhaustion() {
        let h = super::ScriptHost::new("<UVI4><Program><EventProcessors><ScriptProcessor><script>Knob('K',1,0,127)</script></ScriptProcessor></EventProcessors></Program></UVI4>", (), super::Config::default()).unwrap();
        let deadline = h.shared.deadline.get();
        for (work, exhausted) in [(1, false), (0, true)] {
            h.shared.remaining_work.set(work);
            h.shared.work_exhausted.set(exhausted);
            let face = h.interface();
            assert_eq!(face.widgets[0].name, "K");
            assert_eq!(face.widgets[0].initial_value, 1.);
            assert_eq!(h.shared.remaining_work.get(), work);
            assert_eq!(h.shared.work_exhausted.get(), exhausted);
            assert_eq!(h.shared.deadline.get(), deadline);
        }
        h.shared.remaining_work.set(7);
        h.shared.work_exhausted.set(false);
        {
            let _inspection = h.shared.inspection_budget();
            h.shared.remaining_work.set(0);
            assert!(h.shared.consume_work().is_err());
        }
        assert_eq!(h.shared.remaining_work.get(), 7);
        assert!(!h.shared.work_exhausted.get());
        assert_eq!(
            h.fault_counts().runtime.get(&super::FaultCategory::Budget),
            Some(&1)
        );
        h.shared.initializing.set(true);
        h.shared.remaining_work.set(0);
        {
            let _inspection = h.shared.inspection_budget();
            assert_eq!(h.shared.remaining_work.get(), 0);
        }
        assert_eq!(h.shared.remaining_work.get(), 0);
    }

    use super::*;

    fn host(script: &str) -> ScriptHost {
        let xml = format!(
            "<UVI4><Program Name='P'><Layers><Layer Name='A'><Keygroups><Keygroup Name='K'>\
             <Oscillators><SamplePlayer Name='o1'/><SamplePlayer Name='o2'/></Oscillators>\
             </Keygroup></Keygroups></Layer></Layers>\
             <EventProcessors><ScriptProcessor Name='S'><script><![CDATA[{script}]]></script>\
             </ScriptProcessor></EventProcessors></Program></UVI4>"
        );
        ScriptHost::new(&xml, (), Config::default()).unwrap()
    }

    /// Every insert holds a convolver; impulses are a one-tap dirac.
    struct Convolvers;
    impl Files for Convolvers {
        fn script(&self, _: &str) -> Option<String> {
            None
        }
        fn convolver_slot(&self, _: usize) -> Option<usize> {
            Some(0)
        }
    }

    #[test]
    fn impulse_fields_set_together_rebuild_the_impulse_once() {
        let xml = "<UVI4><Program Name='P'><Auxs><AuxEffect Name='Aux0'><Inserts>\
             <Convolver Dry='0' Wet='1'/></Inserts></AuxEffect></Auxs>\
             <EventProcessors><ScriptProcessor Name='S'><script><![CDATA[\
             local c = Program.auxs[1].inserts[1]\n\
             function onInit() c:setParameter('Wet', 0.5); c:setParameter('Dry', 0.25);\
             c:setParameter('NormalizePower', true) end]]></script></ScriptProcessor></EventProcessors>\
             <Layers><Layer Name='A'><Keygroups><Keygroup Name='K'><Oscillators>\
             <SamplePlayer Name='o1'/></Oscillators></Keygroup></Keygroups></Layer></Layers>\
             </Program></UVI4>";
        let mut h = ScriptHost::new(xml, Convolvers, Config::default()).unwrap();
        let impulses = |c: Vec<Command>| {
            c.iter()
                .filter(|c| matches!(c, Command::Impulse { .. }))
                .count()
        };
        assert_eq!(impulses(h.take_commands()), 1, "{:?}", h.findings());
        assert_eq!(impulses(h.take_commands()), 0);
    }

    #[test]
    fn note_on_plays_the_oscillator_the_script_picks() {
        let mut h = host(
            "local n = 0\n\
             function onNote(e) n = n + 1; playNote(e.note, e.velocity, -1, 1, nil, nil, 1, 0, 0, nil, n) end",
        );
        assert!(h.handles_notes());
        h.note_on(1, 60, 100, 0);
        h.note_on(2, 62, 90, 0);
        let plays: Vec<_> = h
            .take_commands()
            .into_iter()
            .map(|c| match c {
                Command::Play(p) => (p.key, p.velocity, p.osc, p.layers, p.duration_ms, p.parent),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            plays,
            [
                (60, 100, Some(1), Layers::from([1]), None, Some(1)),
                (62, 90, Some(2), Layers::from([1]), None, Some(2))
            ]
        );
        assert!(h.findings().is_empty(), "{:?}", h.findings());
    }

    #[test]
    fn wait_and_spawn_follow_the_hosts_clock() {
        let mut h = host(
            "function onNote(e)\n\
               spawn(function() wait(100); playNote(e.note + 12, 80, 50) end)\n\
               playNote{ note = e.note, velocity = 70, oscIndex = 2 }\n\
             end",
        );
        h.note_on(7, 60, 100, 0);
        let first = h.take_commands();
        assert_eq!(first.len(), 1);
        assert!(matches!(&first[0], Command::Play(p) if p.osc == Some(2) && p.velocity == 70));
        assert_eq!(h.next_due(), Some(100.0));
        h.advance(99.0);
        assert!(h.take_commands().is_empty());
        h.advance(250.0);
        let later = h.take_commands();
        assert!(
            matches!(&later[..], [Command::Play(p)] if p.key == 72 && p.at_ms == 100.0 && p.duration_ms == Some(50.0))
        );
    }

    #[test]
    fn wait_for_release_resumes_on_note_off_and_release_is_called() {
        let mut h = host(
            "function onNote(e) waitForRelease(); releaseVoice(e.id) end\n\
             function onRelease(e) playNote(e.note, 1) end",
        );
        h.note_on(3, 60, 100, 0);
        assert!(h.take_commands().is_empty());
        h.note_off(3, 60, 0, 0);
        let c = h.take_commands();
        assert!(matches!(&c[0], Command::Release { id: 3, .. }));
        assert!(matches!(&c[1], Command::Play(p) if p.velocity == 1));
    }

    #[test]
    fn runaway_scripts_are_aborted_and_reported() {
        let mut h = host("function onNote(e) while true do end end");
        h.note_on(1, 60, 100, 0);
        let findings = h.findings();
        assert!(
            findings
                .iter()
                .any(|f| f.feature == "lua error" && f.value.contains("work budget")),
            "{findings:?}"
        );
        // The host is still usable.
        h.note_on(2, 60, 100, 0);
    }

    #[test]
    fn classes_tables_and_playnote_tables_work() {
        let mut h = host(
            "class 'A'\nfunction A:__init(x) self.x = x end\n\
             class 'B'(A)\nfunction B:__init(x) A.__init(self, x) end\nlocal b = B(7)\nassert(b.x == 7)\n\
             local t = Table{'t', 4, 1, 0, 9, true}\n\
             t.changed = function(self, i) lastIndex = i end\n\
             t:setValue(2, 5)\nassert(lastIndex == 2 and t:getValue(2) == 5)\n\
             assert(type(Program.layers[1]) == 'userdata')\n\
             function onNote(e) playNote{e.note, 90, 0, layer=1} end",
        );
        h.note_on(1, 62, 100, 0);
        let c = h.take_commands();
        assert!(
            matches!(&c[..], [Command::Play(p)] if p.key == 62 && p.velocity == 90 && p.duration_ms == Some(0.0)),
            "{c:?} {:?}",
            h.findings()
        );
    }

    #[test]
    fn send_script_modulation_becomes_a_command() {
        let mut h = host("function onNote(e) sendScriptModulation(9, 0.4, 1000, nil) end");
        h.note_on(1, 60, 100, 0);
        let c = h.take_commands();
        assert!(
            matches!(&c[..], [Command::Modulation { id: 9, glide_ms, voice: None, .. }] if *glide_ms == 1000.0),
            "{c:?}"
        );
    }

    #[test]
    fn context_conversions_and_key_state() {
        let mut h = host(
            "function onNote(e)\n\
               playNote(60 + beat2ms(1) / 100, 100 + (isKeyDown(e.note) and 1 or 0), 0)\n\
               playNote(getTempo(), ms2samples(1000) / 1000, 0)\n\
             end",
        );
        h.set_tempo(120.0);
        h.note_on(1, 60, 100, 0);
        let keys: Vec<_> = h
            .take_commands()
            .into_iter()
            .map(|c| match c {
                Command::Play(p) => (p.key, p.velocity),
                other => panic!("{other:?}"),
            })
            .collect();
        // 500 ms per beat; the key is down; 120 bpm; 48 samples per ms.
        assert_eq!(keys, [(65, 101), (120, 48)]);
        h.note_off(1, 60, 64, 0);
        assert!(h.findings().is_empty(), "{:?}", h.findings());
    }

    #[test]
    fn voice_manipulation_and_generated_midi_become_commands() {
        let mut h = host(
            "function onNote(e)\n\
               local v = playNote(e.note, 100, 0)\n\
               changeVolume(v, 0.5)\n changeTune(v, 2, true)\n changePan(v, -1)\n\
               fadeout(v, 100, true)\n fade2(v, 0, 1, 50)\n\
               sendScriptModulation2(3, 0.2, 0.8, 100, v)\n\
               controlChange(1, 64, 2)\n pitchBend(0)\n\
               postEvent{type = Event.Controller, controller = 7, value = 90}\n\
             end",
        );
        h.note_on(1, 60, 100, 0);
        let c = h.take_commands();
        assert!(
            matches!(&c[1], Command::Change { what: Change::Decibels, value, relative: false, .. } if (*value + 6.0206).abs() < 1e-3)
        );
        assert!(matches!(
            &c[2],
            Command::Change {
                what: Change::Tune,
                value: 2.0,
                relative: true,
                ..
            }
        ));
        assert!(matches!(
            &c[3],
            Command::Change {
                what: Change::Pan,
                value: -1.0,
                ..
            }
        ));
        assert!(
            matches!(&c[4], Command::Fade { from: None, to, ms: 100.0, kill: true, .. } if *to == 0.0)
        );
        assert!(matches!(
            &c[5],
            Command::Fade {
                from: Some(0.0),
                to: 1.0,
                ms: 50.0,
                ..
            }
        ));
        assert_eq!(c.len(), 10);
        assert!(matches!(
            &c[6],
            Command::Modulation {
                id: 3,
                start: Some(from),
                value: to,
                glide_ms: 100.0,
                ..
            } if *from == 0.2 && *to == 0.8
        ));
        assert_eq!(
            c[7],
            Command::Midi(MidiOut {
                status: 0xb1,
                a: 1,
                b: 64
            })
        );
        assert_eq!(
            c[8],
            Command::Midi(MidiOut {
                status: 0xe0,
                a: 0,
                b: 64
            })
        );
        assert_eq!(
            c[9],
            Command::Midi(MidiOut {
                status: 0xb0,
                a: 7,
                b: 90
            })
        );
        assert!(h.findings().is_empty(), "{:?}", h.findings());
    }

    #[test]
    fn set_parameter_on_program_and_layers_becomes_commands() {
        let mut h = host(
            "function onNote(e)\n\
               Program:setParameter('Polyphony', 4)\n\
               Program.layers[1]:setParameter('Gain', 0.5)\n\
               Program.layers[1]:setParameter('Pan', 0.25)\n\
               Program.layers[1]:setParameter('Mute', true)\n\
               assert(Program.layers[1]:getParameter('Gain') == 0.5)\n\
             end",
        );
        h.note_on(1, 60, 100, 0);
        let c = h.take_commands();
        assert_eq!(
            c[0],
            Command::Parameter {
                scope: Scope::Program,
                param: Param::Polyphony,
                value: 4.0,
                authored: 16.0
            }
        );
        assert_eq!(
            c[1],
            Command::Parameter {
                scope: Scope::Layer(1),
                param: Param::Gain,
                value: 0.5,
                authored: 1.0
            }
        );
        assert_eq!(
            c[2],
            Command::Parameter {
                scope: Scope::Layer(1),
                param: Param::Pan,
                value: 0.25,
                authored: 0.0
            }
        );
        assert_eq!(c.len(), 3);
        let found = h.findings();
        let f: Vec<_> = found.iter().map(|f| f.feature.as_str()).collect();
        assert_eq!(f, ["lua setParameter Layer.Mute"]);
    }

    #[test]
    fn saved_widget_values_apply_after_init_and_run_changed() {
        let xml = "<UVI4><Program Name='P'><EventProcessors><ScriptProcessor Name='S' Link='1'>\
             <script><![CDATA[\
             local calls, link = 0, 0\n\
             local b = OnOffButton{'Link', false, changed = function(self) calls = calls + 1; link = self.value and 1 or 0 end}\n\
             local atInit = b.value and 100 or 0\n\
             function onNote(e) playNote(e.note, atInit + calls * 10 + link) end]]></script>\
             </ScriptProcessor></EventProcessors></Program></UVI4>";
        let mut h = ScriptHost::new(xml, (), Config::default()).unwrap();
        h.note_on(1, 60, 100, 0);
        let c = h.take_commands();
        // Not set while initialising; set afterwards, with `changed` run once.
        assert!(
            matches!(&c[0], Command::Play(p) if p.velocity == 11),
            "{c:?}"
        );
    }

    #[test]
    fn other_host_events_reach_their_handlers_and_on_event_takes_precedence() {
        let mut h = host(
            "function onPitchBend(e) playNote(60, 100 + e.value * 10, 0) end\n\
             function onAfterTouch(e) playNote(61, e.value, 0) end\n\
             function onProgramChange(e) playNote(62, e.value, 0) end\n\
             function onTransport(playing) playNote(63, playing and 5 or 6, 0) end",
        );
        h.pitch_bend(0.5, 0);
        h.after_touch(40, 0);
        h.program_change(9, 0);
        h.transport(true);
        let seen: Vec<_> = h
            .take_commands()
            .into_iter()
            .map(|c| match c {
                Command::Play(p) => (p.key, p.velocity),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(seen, [(60, 105), (61, 40), (62, 9), (63, 5)]);
        let mut h = host(
            "function onEvent(e) playNote(70, 1, 0) end function onController(e) playNote(71, 1, 0) end",
        );
        h.controller(1, 2, 0);
        assert!(matches!(&h.take_commands()[..], [Command::Play(p)] if p.key == 70));
    }

    #[test]
    fn widgets_export_to_the_ui_ir() {
        let h = host(
            "setSize(400, 200)\n\
             local p = Panel('main')\n\
             p:Knob('gain', 0.5, 0, 1)\n\
             p:Menu{name='mode', items={'a','b'}}\n\
             p:OnOffButton('on', true)",
        );
        let ui = h.interface();
        assert_eq!(ui.source, sampler_ui_ir::Source::FalconLua);
        assert!(ui.widgets.len() >= 4, "{}", ui.widgets.len());
        assert_eq!(ui.pages.len(), 1);
    }

    #[test]
    fn unknown_globals_are_nil_and_invalid_calls_are_reported() {
        let mut h = host(
            "local p = Panel('main')\n\
             local k = p:Knob('gain', 0.5, 0, 1)\n\
             k.changed = function(self) playNote(61, 100) end\n\
             k:setValue(0.75)\n\
             assert(Mystery == nil)\n\
             Program.layers[1].keygroups[1].oscillators[2]:setParameter('Gain', 0.5)\n\
             assert(Program.layers[1].keygroups[1].oscillators[1].name == 'o1')\n\
             assert(k.value == 0.75)\n\
             Mystery.thing:go(1, 2)",
        );
        let features: Vec<_> = h.findings().into_iter().map(|f| f.feature).collect();
        assert!(features.contains(&"lua error".to_owned()), "{features:?}");
        assert!(
            h.take_commands()
                .iter()
                .any(|c| matches!(c, Command::Play(p) if p.key == 61))
        );
    }

    #[test]
    fn the_sandbox_has_no_io_os_or_package_access() {
        let mut h = host("function onNote(e) playNote(io == nil and 1 or 2, 1) end");
        h.note_on(1, 60, 100, 0);
        // `io` is never the real library: unset (nil) as an unassigned lowercase name.
        let c = h.take_commands();
        assert!(matches!(&c[0], Command::Play(p) if p.key == 1));
    }
}

#[cfg(test)]
#[path = "script/ready_command_tests.rs"]
mod ready_command_tests;

#[cfg(test)]
mod physical_key_state_regressions {
    use super::*;
    fn host(script:&str)->ScriptHost {
        let xml=format!("<UVI4><Program Name='P'><EventProcessors><ScriptProcessor Name='S'><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>");
        ScriptHost::new(&xml,(),Config::default()).unwrap()
    }
    #[test]
    fn key_state_keeps_overlapping_physical_owners_and_last_note_on_duration() {
        let mut h=host(r#"
            releases={}
            function onRelease(e)
                table.insert(releases,{getNoteDuration(e.note),isKeyDown(e.note),isOctaveKeyDown(e.note)})
            end
        "#);
        h.set_time(10.);h.note_on(1,60,100,1);
        h.set_time(30.);h.note_on(2,60,90,12);
        h.set_time(40.);h.note_on(3,72,80,8);
        h.set_time(80.);h.note_off(1,60,11,1);
        h.lua.load("assert(releases[1][1]==50 and releases[1][2] and releases[1][3])").exec().unwrap();
        h.set_time(110.);h.note_off(2,60,22,12);
        h.lua.load("assert(releases[2][1]==80 and not releases[2][2] and releases[2][3])").exec().unwrap();
        h.set_time(115.);h.note_off(999,72,0,8);
        h.lua.load("assert(releases[3][1]==75 and releases[3][2] and releases[3][3])").exec().unwrap();
        h.set_time(120.);h.note_off(3,72,33,8);
        h.lua.load("assert(releases[4][1]==80 and not releases[4][2] and not releases[4][3])").exec().unwrap();
        assert!(h.findings().is_empty(),"{:?}",h.findings());
    }
    #[test]
    fn releasing_one_same_key_owner_wakes_only_its_own_release_waiter() {
        let mut h=host(r#"
            resumed={}
            function onNote(e)
                waitForRelease()
                table.insert(resumed,{e.id,isNoteHeld(),isKeyDown(e.note)})
            end
        "#);
        h.note_on(41,60,100,0);h.note_on(42,60,100,15);
        h.note_off(41,60,64,0);
        h.lua.load("assert(#resumed==1 and resumed[1][1]==41 and not resumed[1][2] and resumed[1][3])").exec().unwrap();
        h.note_off(42,60,64,15);
        h.lua.load("assert(#resumed==2 and resumed[2][1]==42 and not resumed[2][2] and not resumed[2][3])").exec().unwrap();
        assert!(h.findings().is_empty(),"{:?}",h.findings());
    }
}

#[cfg(test)]
mod play_creator_tests {
    use super::*;

    fn host(source: &str) -> ScriptHost {
        let xml = format!("<Program><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors></Program>");
        ScriptHost::new(&xml, (), Config::default()).unwrap()
    }
    fn plays(host: &mut ScriptHost) -> Vec<Play> {
        host.take_commands().into_iter().filter_map(|c| match c { Command::Play(p) => Some(p), _ => None }).collect()
    }

    #[test]
    fn actual_creator_survives_wait_and_spawn_run_get_manual_callback_creator() {
        let mut host = host(r#"
            playNote(48,100,-1)
            spawn(function() playNote(49,100,-1) end)
            function onInit() playNote(50,100,-1) end
            function onNote(e)
                run(function() playNote(61,100,-1) end)
                spawn(function() wait(2); playNote(62,100,-1) end)
                wait(10); playNote(63,100,-1)
            end
            function onController(e) playNote(64,100,-1) end
            function onRelease(e) playNote(65,100,-1) end
            function onTransport(playing) playNote(66,100,-1) end
        "#);
        let init = plays(&mut host);
        assert_eq!(init.iter().map(|p| p.creator).collect::<Vec<_>>(), [PlayCreator::None; 3]);
        host.note_on(17,60,100,4);
        let immediate = plays(&mut host);
        assert_eq!(immediate.len(),1);
        assert_eq!((immediate[0].key,immediate[0].creator,immediate[0].parent), (61,PlayCreator::Event,None));
        host.advance(2.);
        let spawned = plays(&mut host);
        assert_eq!((spawned[0].key,spawned[0].creator,spawned[0].parent), (62,PlayCreator::Event,None));
        host.note_off(17,60,64,4);
        let release = plays(&mut host);
        assert_eq!((release[0].key,release[0].creator), (65,PlayCreator::Event));
        host.advance(10.);
        let late = plays(&mut host);
        assert_eq!((late[0].key,late[0].creator,late[0].parent), (63,PlayCreator::NoteOn(17),Some(17)));
        host.controller(1,127,4);
        assert_eq!(plays(&mut host)[0].creator,PlayCreator::Event);
        host.transport(true);
        assert_eq!(plays(&mut host)[0].creator,PlayCreator::None);
        assert!(host.findings().is_empty(),"{:?}",host.findings());
    }

    #[test]
    fn widget_negative_duration_errors_outside_thread_but_explicit_duration_and_spawn_remain_valid() {
        let mut host = host(r#"
            k=Knob('CreatorWidget',0,0,10)
            k.changed=function(self) playNote(60,100,-1) end
        "#);
        assert!(host.set_control(control_id(1,0),1.).is_err());
        assert!(plays(&mut host).is_empty());
        // Reassignment is deliberately observed by the ordinary edit path.
        host.lua.load("k.changed=function(self) playNote(61,100,0); spawn(function() playNote(62,100,-1) end) end").exec().unwrap();
        host.set_control(control_id(1,0),2.).unwrap();
        let commands=plays(&mut host);
        assert_eq!(commands.len(),2);
        assert_eq!((commands[0].creator,commands[0].duration_ms),(PlayCreator::Widget,Some(0.)));
        assert_eq!((commands[1].creator,commands[1].duration_ms),(PlayCreator::Event,None));
    }

    #[test]
    fn widget_changed_inside_real_init_thread_has_no_creator_and_inside_note_has_note_creator() {
        let mut host = host(r#"
            k=Knob('ThreadWidget',0,0,10)
            k.changed=function(self) playNote(60,100,-1) end
            function onInit() k:setValue(1) end
            function onNote(e) k:setValue(2) end
        "#);
        assert_eq!(plays(&mut host)[0].creator,PlayCreator::None);
        host.note_on(23,60,100,0);
        assert_eq!(plays(&mut host)[0].creator,PlayCreator::NoteOn(23));
        assert!(host.findings().is_empty(),"{:?}",host.findings());
    }
}


#[cfg(test)]
mod staged_preparation_tests {
    use super::*;
    fn xml(source:&str)->String{format!("<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>")}
    #[test]
    fn staged_main_restore_and_custom_load_precede_unexecuted_init(){
        let source=r#"
            order='main'; function __restore() order=order..':restore' end
            function onLoad(value) order=order..':'..value end
            function onInit() order=order..':init' end
        "#;
        let state=UiState{custom:Some(ui::SavedValue::String("load".into())),..Default::default()};
        let mut host=ScriptHost::new_staged_with_ui_state(&xml(source),(),Config::default(),Some(&state)).unwrap();
        assert_eq!(host.global_text("order"),"main:restore:load");
        host.advance(10000.);
        assert_eq!(host.global_text("order"),"main:restore:load");
        assert!(host.shared.external_waiting.borrow().is_empty());
    }
    #[test]
    fn staged_spawn_is_retained_without_running_or_polling_its_body(){
        let mut host=ScriptHost::new_staged_with_ui_state(&xml("count=0; spawn(function() count=count+1 end); function onInit() count=count+10 end"),(),Config::default(),None).unwrap();
        assert_eq!(host.global_text("count"),"0");
        assert_eq!(host.shared.deferred.borrow().len(),1);
        host.advance(100000.);
        assert_eq!(host.global_text("count"),"0");
        assert_eq!(host.shared.deferred.borrow().len(),1);
    }
    #[test]
    fn synchronous_main_yield_is_visible_and_never_a_fake_prepared_init(){
        let result=ScriptHost::new_staged_with_ui_state(&xml("wait(10); initialized=true"),(),Config::default(),None);
        assert!(matches!(result,Err(error)if error.contains("without a rendered Core task context")));
        let eager=ScriptHost::new(&xml("function onInit() initialized=123 end"),(),Config::default()).unwrap();
        assert_eq!(eager.global_text("initialized"),"123");
    }
}

#[cfg(test)]
mod actual_task_segment_tests{
    use super::*;
    use std::{sync::mpsc,thread,time::{Duration,Instant}};
    fn runtime(xml:&str)->sampler_core::Runtime{
        use sampler_core::{Instruction,Program,WaitLifetime,PreparedSourceTaskBinding as Binding,SourceTaskFamily as Family};
        let translated=crate::translate(xml,std::path::Path::new(".")).unwrap();
        let processor=translated.instrument.source_indices.owners.iter().flat_map(|owner|&owner.event_processors).find(|row|row.kind=="ScriptProcessor").unwrap().id;
        assert!(translated.instrument.assets.is_empty(),"authored task graph has no sample assets");
        // The final lower argument binds behavior modules; it is not an asset
        // reader. These fixtures bind the real external task program below.
        let prepared=sampler_core::lower::lower(&translated.instrument,48000,vec![],|_,plan|Ok(plan)).unwrap()
            .with_programs(vec![Program::new(vec![Instruction::AwaitExternal{processor},Instruction::End]).unwrap().with_wait_lifetime(WaitLifetime::Callback)],None).unwrap()
            .with_controller_programs(vec![0]).unwrap()
            .with_source_tasks(vec![Binding{processor,family:Family::Init,program:0},Binding{processor,family:Family::Transport,program:0},Binding{processor,family:Family::Spawned,program:0},Binding{processor,family:Family::Run,program:0}]).unwrap();
        let limits=sampler_core::Limits::for_plan(&prepared,8,8);
        sampler_core::Runtime::new(prepared,limits).unwrap()
    }
    #[test]
    fn actual_failed_environment_restoration_fences_later_callbacks_instead_of_guessing_context(){
        let xml="<Program><EventProcessors><ScriptProcessor><script>value=17</script></ScriptProcessor><ScriptProcessor><script>value=99</script></ScriptProcessor></EventProcessors></Program>";
        let host=ScriptHost::new_staged_with_ui_state(xml,(),Config::default(),None).unwrap();
        let guard=input_authority::enter(&host.lua,&host.shared,Some(0)).unwrap();
        // mlua explicitly rejects set_globals while its real sandbox is set.
        // Exercise that actual refusal, not a forged successful restoration.
        host.lua.sandbox(true).unwrap();assert!(guard.restore().is_err());drop(guard);
        assert!(host.shared.context_failed.get());
        assert!(input_authority::enter(&host.lua,&host.shared,Some(1)).is_err());
        host.lua.sandbox(false).unwrap();
        assert!(input_authority::enter(&host.lua,&host.shared,Some(1)).is_err(),"a failed context never silently rearms on a later callback");
        assert!(!host.shared.command(Command::Midi(MidiOut{status:0xb0,a:7,b:127})));
        assert!(host.shared.commands.borrow().is_empty());
    }
    #[test]
    fn staged_out_of_turn_commands_cannot_escape_to_legacy_queue_or_publish_fake_voices(){
        let xml="<Program><EventProcessors><ScriptProcessor><script>function onController(e) end</script></ScriptProcessor></EventProcessors></Program>";
        let mut staged=ScriptHost::new_staged_with_ui_state(xml,(),Config::default(),None).unwrap();
        assert!(staged.lua.load("playNote(60,100,0)").exec().is_err());
        assert!(staged.shared.voices.borrow().is_empty());assert!(staged.take_commands().is_empty());
        staged.lua.load("sendScriptModulation(7,0.5,20,nil)").exec().unwrap();
        assert!(staged.take_commands().is_empty(),"no future-value/legacy queue cache may replace actual Core task authority");
        let mut legacy=ScriptHost::new(xml,(),Config::default()).unwrap();
        legacy.lua.load("playNote(60,100,0)").exec().unwrap();
        assert!(matches!(&legacy.take_commands()[..],[Command::Play(play)]if play.key==60));
    }
    #[test]
    fn same_frame_cancel_is_checked_before_actual_processor_bypass_noop_or_retention(){
        let xml=String::from("<Program><EventProcessors><ScriptProcessor Bypass='0'><script>function onController(e) this:setParameter('Bypass',false) end</script></ScriptProcessor></EventProcessors></Program>");
        let mut rt=runtime(&xml);let plan=rt.active_plan();let perf=rt.performance(0).unwrap();
        let (mut core,owner)=authority_pair(&rt,plan,Duration::from_secs(2)).unwrap();
        rt.dispatch_controller(perf,sampler_core::ChannelAddress{protocol:sampler_core::Protocol::Midi1,port:0,group:0,channel:0},1,7,0).unwrap();
        rt.service_external_boundary().unwrap();let pending=rt.pending_external_turn().unwrap().unwrap();let turn=rt.begin_external_turn(pending.input).unwrap();
        let (send,done)=mpsc::sync_channel(1);
        let handle=thread::spawn(move||{
            let mut host=ScriptHost::new_staged_with_ui_state(&xml,(),Config::default(),None).unwrap();
            host.bind_owner_authority(owner).unwrap();let before=host.shared.params.borrow().clone();
            let result=host.deliver_external_turn(turn);
            assert_eq!(*host.shared.params.borrow(),before,"canceled no-op cannot retain a model/overlay write");
            send.send(result).unwrap();
        });
        let deadline=Instant::now()+Duration::from_secs(2);let mut validated=false;
        loop{
            if let Some(request)=core.pop(){
                assert_eq!(request.turn.input,turn.input);
                let result=match request.operation{
                    AuthorityOperation::Validate=>{
                        assert!(!validated);validated=true;rt.cancel_external_input(turn.input).unwrap();
                        rt.running_external_turn(turn.input).map(|_|AuthorityValue::Validated)
                    },
                    AuthorityOperation::Complete{next_frame}=>{assert!(validated);rt.complete_external_segment(turn.input,next_frame).map(|_|AuthorityValue::Completed)},
                    AuthorityOperation::Cancel=>{assert!(validated);rt.cancel_external_input(turn.input).map(|_|AuthorityValue::Completed)},
                    other=>panic!("canceled no-op must not post or mutate: {other:?}"),
                };
                assert!(result.is_err());
                core.reply(AuthorityReply{input:turn.input,ordinal:request.ordinal,result,retired_command:None}).unwrap();
            }
            match done.try_recv(){Ok(result)=>{assert!(validated);assert!(result.is_err());break;},Err(mpsc::TryRecvError::Disconnected)=>panic!("real Lua owner stopped"),Err(mpsc::TryRecvError::Empty)=>{}}
            assert!(Instant::now()<deadline,"canceled callback must receive its real terminal refusal");thread::sleep(Duration::from_micros(50));
        }
        handle.join().unwrap();assert_eq!(rt.now(),0);assert!(rt.pending_external_turn().unwrap().is_none());
    }
    #[test]
    fn refused_actual_command_returns_its_last_upload_reference_to_the_lua_owner(){
        let xml="<Program><EventProcessors><ScriptProcessor Script='function onController(e) end'/></EventProcessors></Program>";
        let mut rt=runtime(xml);let plan=rt.active_plan();
        let (mut core,mut owner)=authority_pair(&rt,plan,Duration::from_secs(2)).unwrap();
        let performance=rt.performance(0).unwrap();
        rt.dispatch_controller(performance,sampler_core::ChannelAddress{protocol:sampler_core::Protocol::Midi1,port:0,group:0,channel:0},1,7,0).unwrap();
        rt.service_external_boundary().unwrap();
        let pending=rt.pending_external_turn().unwrap().unwrap();let turn=rt.begin_external_turn(pending.input).unwrap();
        let (send,weak)=mpsc::sync_channel(1);
        let handle=thread::spawn(move||{
            owner.bind_current_thread();owner.begin(turn).unwrap();
            let upload=ImpulseUpload(std::sync::Arc::new(std::sync::Mutex::new(None)));
            send.send(std::sync::Arc::downgrade(&upload.0)).unwrap();
            let result=owner.call(AuthorityOperation::Command(Command::Impulse{slot:0,upload}));
            assert!(matches!(result,Err(sampler_core::Error::InvalidInput)));
        });
        let weak=weak.recv_timeout(Duration::from_secs(2)).unwrap();
        let deadline=Instant::now()+Duration::from_secs(2);
        let request=loop{
            if let Some(request)=core.pop(){break request;}
            assert!(Instant::now()<deadline);thread::sleep(Duration::from_micros(50));
        };
        assert_eq!(request.turn.input,turn.input);assert_eq!(weak.strong_count(),1);
        let AuthorityOperation::Command(command)=request.operation else{panic!("actual queued command required");};
        // A pure refusal still returns exactly the original payload. There is
        // no clone maintaining a second lifetime or engine-side last drop.
        core.reply(AuthorityReply{input:turn.input,ordinal:request.ordinal,result:Err(sampler_core::Error::InvalidInput),retired_command:Some(command)}).unwrap();
        handle.join().unwrap();assert!(weak.upgrade().is_none());
        rt.cancel_external_input(turn.input).unwrap();
    }
    fn complete_turn(rt:&mut sampler_core::Runtime,port:&mut CoreAuthorityPort,tx:&mpsc::SyncSender<sampler_core::ExternalTurn>,done:&mpsc::Receiver<Result<String,String>>)->String{
        let turn=rt.pending_external_turn().unwrap().expect("real Core continuation must own this segment");
        let turn=rt.begin_external_turn(turn.input).unwrap();tx.send(turn).unwrap();
        let deadline=Instant::now()+Duration::from_secs(3);
        loop{
            if let Some(request)=port.pop(){
                assert_eq!(request.turn.input,turn.input);
                let mut retired_command=None;
                let result=match request.operation{
                    AuthorityOperation::Validate=>rt.running_external_turn(turn.input).map(|_|AuthorityValue::Validated),
                    AuthorityOperation::QueryConstant{node}=>rt.running_external_turn(turn.input).and_then(|_|rt.source_constant_property(turn.plan,node)).map(AuthorityValue::Constant),
                    AuthorityOperation::Post(output)=>rt.post_external_output(turn.input,output).map(AuthorityValue::Posted),
                    AuthorityOperation::Complete{next_frame}=>rt.complete_external_segment(turn.input,next_frame).map(|_|AuthorityValue::Completed),
                    AuthorityOperation::Cancel=>rt.cancel_external_input(turn.input).map(|_|AuthorityValue::Completed),
                    AuthorityOperation::Command(command)=>{retired_command=Some(command);Err(sampler_core::Error::InvalidInput)},
                };
                port.reply(AuthorityReply{input:turn.input,ordinal:request.ordinal,result,retired_command}).unwrap();
            }
            match done.try_recv(){Ok(result)=>return result.unwrap(),Err(mpsc::TryRecvError::Disconnected)=>panic!("actual Lua owner stopped"),Err(mpsc::TryRecvError::Empty)=>{}}
            assert!(Instant::now()<deadline,"actual owner/Core receipt deadline");thread::sleep(Duration::from_micros(50));
        }
    }
    #[test]
    fn actual_core_transport_then_sleeping_init_allows_independent_controller_before_resume(){
        use sampler_core::{SourceTaskReason as Reason,SourceTaskHostContext as Host,ChannelAddress,Protocol};
        let xml=String::from(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
            order='main';
            function onTransport(playing) assert(playing);order=order..':transport' end
            function onInit() assert(getTempo()==91);order=order..':init1';wait(1);order=order..':init2' end
            function onController(e) assert(e.controller==1);order=order..':cc' end
        ]]></script></ScriptProcessor></EventProcessors></Program>"#);
        let mut rt=runtime(&xml);let plan=rt.active_plan();let perf=rt.performance(0).unwrap();
        let (mut core,owner)=input_authority::authority_pair(&rt,plan,Duration::from_secs(2)).unwrap();
        let (tx,receive)=mpsc::sync_channel(1);let (reply,done)=mpsc::sync_channel(1);
        let actor=thread::spawn(move||{
            let mut host=ScriptHost::new_staged_with_ui_state(&xml,(),Config::default(),None).unwrap();
            assert_eq!(host.global_text("order"),"main");host.bind_owner_authority(owner).unwrap();
            while let Ok(turn)=receive.recv(){let result=host.deliver_external_turn(turn).map(|_|host.global_text("order"));reply.send(result).unwrap();}
        });
        let host=Host{frame:0,tempo:91.,playing:true};
        assert_eq!(rt.start_source_tasks(plan,Reason::Transport(host),perf),Ok(1));
        assert_eq!(complete_turn(&mut rt,&mut core,&tx,&done),"main:transport");assert_eq!(rt.now(),0);
        assert_eq!(rt.start_source_tasks(plan,Reason::Init(host),perf),Ok(1));
        assert_eq!(complete_turn(&mut rt,&mut core,&tx,&done),"main:transport:init1");assert_eq!(rt.now(),0);
        assert_eq!(rt.start_source_tasks(plan,Reason::Init(host),perf),Ok(0),"actual per-generation once Init");
        let source=ChannelAddress{protocol:Protocol::Midi2,port:0,group:0,channel:0};
        rt.dispatch_controller(perf,source,1,1,0x9123_4567).unwrap();rt.service_external_boundary().unwrap();
        assert_eq!(complete_turn(&mut rt,&mut core,&tx,&done),"main:transport:init1:cc");assert_eq!(rt.now(),0);
        rt.render(&mut [[0.;2];48]).unwrap();rt.service_external_boundary().unwrap();
        assert_eq!(complete_turn(&mut rt,&mut core,&tx,&done),"main:transport:init1:cc:init2");assert_eq!(rt.now(),48);
        drop(tx);actor.join().unwrap();
    }
    #[test]
    fn genuine_prepared_run_wait_and_spawn_join_real_core_tasks_without_prefix_replay(){
        let xml=String::from(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
            order='main';
            run(function() order=order..':run-prefix';wait(1);order=order..':run-suffix' end);
            spawn(function() order=order..':spawn' end)
        ]]></script></ScriptProcessor></EventProcessors></Program>"#);
        let mut rt=runtime(&xml);let plan=rt.active_plan();let perf=rt.performance(0).unwrap();
        let (mut core,owner)=input_authority::authority_pair(&rt,plan,Duration::from_secs(2)).unwrap();
        let runtime_id=rt.id();
        let (metadata,roots)=mpsc::sync_channel(1);
        let (bindings,adoption)=mpsc::sync_channel::<Vec<input_authority::PreparedRootBinding>>(1);
        let (ready,accepted)=mpsc::sync_channel(1);
        let (tx,receive)=mpsc::sync_channel(1);let (reply,done)=mpsc::sync_channel(1);
        let actor=thread::spawn(move||{
            let mut host=ScriptHost::new_staged_with_ui_state(&xml,(),Config::default(),None).unwrap();
            assert_eq!(host.global_text("order"),"main:run-prefix");
            let tasks=host.prepared_root_tasks().unwrap();assert_eq!(tasks.len(),2);
            host.advance(1000.);assert_eq!(host.global_text("order"),"main:run-prefix");
            host.bind_owner_authority(owner).unwrap();metadata.send(tasks).unwrap();
            host.bind_prepared_roots(runtime_id,plan,adoption.recv().unwrap()).unwrap();ready.send(()).unwrap();
            assert!(host.prepared_root_tasks().is_err(),"a bound residual cannot be adopted twice");
            while let Ok(turn)=receive.recv(){let result=host.deliver_external_turn(turn).map(|_|host.global_text("order"));reply.send(result).unwrap();}
        });
        let roots=roots.recv_timeout(Duration::from_secs(3)).unwrap();
        let actual=roots.iter().map(|root|input_authority::PreparedRootBinding{row:root.row,behavior:rt.start_prepared_source_task(plan,root.processor,root.family,perf).unwrap()}).collect();
        bindings.send(actual).unwrap();accepted.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(complete_turn(&mut rt,&mut core,&tx,&done),"main:run-prefix");
        assert_eq!(rt.now(),0,"parking the residual wait does not render or replay");
        assert_eq!(complete_turn(&mut rt,&mut core,&tx,&done),"main:run-prefix:spawn");
        rt.render(&mut [[0.;2];48]).unwrap();rt.service_external_boundary().unwrap();
        assert_eq!(complete_turn(&mut rt,&mut core,&tx,&done),"main:run-prefix:spawn:run-suffix");
        assert!(rt.pending_external_turn().unwrap().is_none());
        drop(tx);actor.join().unwrap();
    }
    #[test]
    fn prepared_residual_wait_preserves_the_actual_executed_prefix_work_budget(){
        let xml="<Program><EventProcessors><ScriptProcessor><script>run(function() for i=1,100 do assert(i&gt;0) end;wait(2); finished=true end)</script></ScriptProcessor></EventProcessors></Program>";
        let mut host=ScriptHost::new_staged_with_ui_state(xml,(),Config::default(),None).unwrap();
        let (remaining,exhausted)={let rows=host.shared.waiting.borrow();assert_eq!(rows.len(),1);(rows[0].remaining_work,rows[0].exhausted)};
        let tasks=host.prepared_root_tasks().unwrap();assert_eq!(tasks.len(),1);
        let roots=host.shared.prepared_roots.borrow();let root=&roots.as_ref().unwrap()[0];
        assert_eq!((root.remaining_work,root.exhausted),(remaining,exhausted));assert_eq!(root.delay,Some(96));
        assert_eq!(host.global_text("finished"),"nil");
    }
    #[test]
    fn prepared_task_capture_rejects_real_widget_event_creator_before_moving_any_row(){
        let xml="<Program><EventProcessors><ScriptProcessor><script><![CDATA[k=Knob('root',0,0,1);k.changed=function(self) spawn(function() end) end]]></script></ScriptProcessor></EventProcessors></Program>";
        let mut host=ScriptHost::new_staged_with_ui_state(xml,(),Config::default(),None).unwrap();
        // This is the ordinary widget edit entry and genuine changed callback,
        // not a forged Event creator or a fabricated Core parent.
        let _=host.set_control(control_id(1,0),1.);
        let before=host.shared.deferred.borrow().len();assert_eq!(before,1);
        assert!(host.prepared_root_tasks().is_err());
        assert_eq!(host.shared.deferred.borrow().len(),before);
        assert!(host.shared.prepared_roots.borrow().is_none());
    }

    #[test]
    fn canceled_queued_resume_does_not_advance_clock_remove_waiter_cache_cc_or_execute_lua(){
        let xml=String::from("<Program><EventProcessors><ScriptProcessor><script>count=0;function onController(e) count=count+1;wait(1);count=count+100 end</script></ScriptProcessor></EventProcessors></Program>");
        let mut rt=runtime(&xml);let plan=rt.active_plan();let perf=rt.performance(0).unwrap();
        let(mut core,owner)=authority_pair(&rt,plan,Duration::from_secs(2)).unwrap();
        let(tx,receive)=mpsc::sync_channel(1);let(reply,done)=mpsc::sync_channel(1);
        let actor=thread::spawn(move||{
            let mut host=ScriptHost::new_staged_with_ui_state(&xml,(),Config::default(),None).unwrap();host.bind_owner_authority(owner).unwrap();
            let first=receive.recv().unwrap();let result=host.deliver_external_turn(first).map(|_|host.global_text("count"));reply.send(result).unwrap();
            assert_eq!(host.shared.external_waiting.borrow().len(),1);
            let before=(host.shared.now.get(),host.shared.external_waiting.borrow().len(),host.shared.processor_view.borrow().cc.borrow()[7],host.global_text("count"));
            let resumed=receive.recv().unwrap();let result=host.deliver_external_turn(resumed);
            assert!(result.is_err());
            assert_eq!((host.shared.now.get(),host.shared.external_waiting.borrow().len(),host.shared.processor_view.borrow().cc.borrow()[7],host.global_text("count")),before);
            reply.send(result.map(|_|String::new())).unwrap();
        });
        rt.dispatch_controller(perf,sampler_core::ChannelAddress{protocol:sampler_core::Protocol::Midi1,port:0,group:0,channel:0},1,7,0x7200_0000).unwrap();rt.service_external_boundary().unwrap();
        assert_eq!(complete_turn(&mut rt,&mut core,&tx,&done),"1");let mut out=[[0.;2];48];rt.render(&mut out).unwrap();rt.service_external_boundary().unwrap();
        let pending=rt.pending_external_turn().unwrap().unwrap();let turn=rt.begin_external_turn(pending.input).unwrap();rt.cancel_external_input(turn.input).unwrap();tx.send(turn).unwrap();
        let deadline=Instant::now()+Duration::from_secs(2);
        loop{
            if let Some(request)=core.pop(){
                let result=match request.operation{
                    AuthorityOperation::Validate=>rt.running_external_turn(turn.input).map(|_|AuthorityValue::Validated),
                    AuthorityOperation::Cancel=>rt.cancel_external_input(turn.input).map(|_|AuthorityValue::Completed),
                    other=>panic!("canceled-before-entry must execute no other action: {other:?}"),
                };assert!(result.is_err());core.reply(AuthorityReply{input:turn.input,ordinal:request.ordinal,result,retired_command:None}).unwrap();
            }
            match done.try_recv(){Ok(result)=>{assert!(result.is_err());break;},Err(mpsc::TryRecvError::Empty)=>{},Err(error)=>panic!("{error}")}
            assert!(Instant::now()<deadline);thread::sleep(Duration::from_micros(50));
        }actor.join().unwrap();assert_eq!(rt.now(),48);
    }
    #[test]
    fn refused_aux_write_is_not_retained_and_identical_retry_still_reaches_core(){
        let xml=String::from("<Program><Auxs><AuxEffect Name='A' Gain='0.25' Bypass='0'/></Auxs><EventProcessors><ScriptProcessor><script>function onController(e) Program.auxs[1]:setParameter('Gain',0.75) end</script></ScriptProcessor></EventProcessors></Program>");
        let mut rt=runtime(&xml);let plan=rt.active_plan();let perf=rt.performance(0).unwrap();
        let(mut core,owner)=authority_pair(&rt,plan,Duration::from_secs(2)).unwrap();let(tx,receive)=mpsc::sync_channel(1);let(reply,done)=mpsc::sync_channel(1);
        let actor=thread::spawn(move||{
            let mut host=ScriptHost::new_staged_with_ui_state(&xml,(),Config::default(),None).unwrap();host.bind_owner_authority(owner).unwrap();let before=host.shared.params.borrow().clone();
            for _ in 0..2{let turn=receive.recv().unwrap();let result=host.deliver_external_turn(turn);assert_eq!(*host.shared.params.borrow(),before,"rejected Gain may not become the prospective Bypass/current-value input");reply.send(result).unwrap();}
        });
        let mut commands=0;
        for _ in 0..2{
            rt.dispatch_controller(perf,sampler_core::ChannelAddress{protocol:sampler_core::Protocol::Midi1,port:0,group:0,channel:0},1,1,0).unwrap();rt.service_external_boundary().unwrap();let pending=rt.pending_external_turn().unwrap().unwrap();let turn=rt.begin_external_turn(pending.input).unwrap();tx.send(turn).unwrap();
            let deadline=Instant::now()+Duration::from_secs(2);
            loop{
                if let Some(request)=core.pop(){let mut retired_command=None;let result=match request.operation{
                    AuthorityOperation::Validate=>rt.running_external_turn(turn.input).map(|_|AuthorityValue::Validated),
                    AuthorityOperation::Command(command)=>{
                        commands+=1;
                        // This low-level Core plan has no UVI adapter's native
                        // alias installation. Real Core address refusal is the
                        // causal boundary, not a fabricated successful ACK.
                        let Command::EngineParameter{address,value}=&command else{panic!("actual Aux engine command");};
                        let result=rt.set_engine_parameter(*address,*value).map(|_|AuthorityValue::Applied);assert!(result.is_err());retired_command=Some(command);result
                    },
                    AuthorityOperation::Complete{next_frame}=>rt.complete_external_segment(turn.input,next_frame).map(|_|AuthorityValue::Completed),
                    AuthorityOperation::Cancel=>rt.cancel_external_input(turn.input).map(|_|AuthorityValue::Completed),
                    other=>panic!("unexpected actual Aux operation: {other:?}"),
                };core.reply(AuthorityReply{input:turn.input,ordinal:request.ordinal,result,retired_command}).unwrap();}
                match done.try_recv(){Ok(result)=>{result.unwrap();break;},Err(mpsc::TryRecvError::Empty)=>{},Err(error)=>panic!("{error}")};assert!(Instant::now()<deadline);thread::sleep(Duration::from_micros(50));
            }
        }assert_eq!(commands,2,"refusal must not turn the retry into an owner-value no-op");actor.join().unwrap();
    }

}

#[cfg(test)]
mod documented_play_defaults_tests {
    use super::*;
    fn command(source:&str)->Play{
        let xml=format!("<Program><EventProcessors><ScriptProcessor><script><![CDATA[function onController(e) {source} end]]></script></ScriptProcessor></EventProcessors></Program>");
        let mut host=ScriptHost::new(&xml,(),Config::default()).unwrap();host.controller(1,64,0);
        let commands=host.take_commands();assert_eq!(commands.len(),1,"one helper call must emit exactly one play");
        assert!(host.findings().is_empty(),"{:?}",host.findings());
        let Command::Play(play)=commands.into_iter().next().unwrap()else{panic!("actual public playNote command required")};play
    }
    #[test]
    fn omitted_note_and_velocity_use_documented_zero_and_one_hundred_in_both_forms(){
        for source in ["playNote(nil,nil,0)","playNote{duration=0}"]{
            let play=command(source);assert_eq!((play.key,play.velocity),(0,100));
            assert_eq!(play.duration_ms,Some(0.));assert_eq!(play.creator,PlayCreator::Event);
        }
    }
    #[test]
    fn explicit_note_zero_middle_and_upper_keys_are_preserved_in_both_forms(){
        for key in [0,60,127]{
            for source in [format!("playNote({key},96,0)"),format!("playNote{{note={key},velocity=96,duration=0}}")]{
                let play=command(&source);assert_eq!((play.key,play.velocity),(key,96));assert_eq!(play.duration_ms,Some(0.));
            }
        }
    }
}

#[cfg(test)]
mod processor_completion_context42 {
    use super::*;
    use std::sync::atomic::{AtomicU64,Ordering};
    static NEXT:AtomicU64=AtomicU64::new(0);
    /// A writable scratch directory: the runner's temporary directory when it
    /// is set, otherwise the workspace scratch, otherwise the platform
    /// temporary directory. A hard-coded workspace path is not writable on a
    /// CI runner.
    fn scratch_root()->std::path::PathBuf{
        if let Some(dir)=std::env::var_os("TMPDIR"){return std::path::PathBuf::from(dir);}
        let local=std::path::PathBuf::from("/mnt/Windows11/DEV_WORKSPACE/BuildScratch");
        if std::fs::create_dir_all(&local).is_ok(){return local;}
        std::env::temp_dir()
    }
    struct DataFile(std::path::PathBuf);
    impl DataFile {
        fn new(value:&str)->Self{
            let directory=scratch_root();
            std::fs::create_dir_all(&directory).unwrap();
            let path=directory.join(format!("uvi-processor42-{}-{}.json",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
            std::fs::write(&path,value).unwrap();Self(path)
        }
    }
    impl Drop for DataFile{fn drop(&mut self){let _=std::fs::remove_file(&self.0);}}
    #[test]
    fn actual_load_data_completion_resumes_originating_processor_even_after_other_context_and_error(){
        let a=DataFile::new(r#"{"tag":"A"}"#);let b=DataFile::new(r#"{"tag":"B"}"#);
        let path=|file:&DataFile|serde_json::to_string(file.0.to_str().unwrap()).unwrap();
        let script=|file:&DataFile,fail:bool|format!(r#"
            marker='main'
            function onController(e)
                expected=e.value
                loadData({},function(data)
                    seen=data.tag..':'..getCC(1)
                    wait(10)
                    resumed=data.tag..':'..getCC(1)
                    {}
                end)
            end
        "#,path(file),if fail{"error('actual A completion failure')"}else{"marker='done'"});
        let xml=format!("<Program><EventProcessors><ScriptProcessor><script><![CDATA[{}]]></script></ScriptProcessor><ScriptProcessor><script><![CDATA[{}]]></script></ScriptProcessor></EventProcessors></Program>",script(&a,true),script(&b,false));
        let mut host=ScriptHost::new(&xml,(),Config::default()).unwrap();let processors=host.processors();assert_eq!(processors.len(),2);
        // Submit the second definition first, then leave the first active. The
        // actual file-worker completions must retain their originating context.
        for(index,value)in[(1,99),(0,17)]{
            host.processor_input(processors[index].processor,crate::scripted::HostInput::Controller{cc:1,value,channel:5}).unwrap();
        }
        let mut complete=false;
        for frame in 0..1000{
            host.advance(frame as f64);
            if host.processor_global_text(processors[0].processor,"resumed").unwrap()=="A:17"&&host.processor_global_text(processors[1].processor,"resumed").unwrap()=="B:99"{complete=true;break;}
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(complete,"actual file completions did not resume both owned coroutines");
        assert_eq!(host.processor_global_text(processors[0].processor,"seen").unwrap(),"A:17");
        assert_eq!(host.processor_global_text(processors[1].processor,"seen").unwrap(),"B:99");
        assert_eq!(host.processor_global_text(processors[1].processor,"marker").unwrap(),"done");
        assert!(host.findings().iter().any(|f|f.value.contains("actual A completion failure")));
        // A failed continuation restores the surrounding owner. A later B
        // callback must still update only B's last-received API view.
        host.processor_input(processors[1].processor,crate::scripted::HostInput::Controller{cc:1,value:71,channel:5}).unwrap();
        assert_eq!(host.processor_global_text(processors[0].processor,"expected").unwrap(),"17");
        assert_eq!(host.processor_global_text(processors[1].processor,"expected").unwrap(),"71");
    }
}


#[cfg(test)]
mod preparation_and_refusal42g_tests {
    use super::*;
    fn xml(script:&str)->String{format!("<Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program>")}
    #[test]
    fn staged_synchronous_main_and_onload_play_are_load_refused_before_voice_or_legacy_queue(){
        assert!(ScriptHost::new_staged_with_ui_state(&xml("playNote(60,100,0)"),(),Config::default(),None).is_err());
        let state=UiState{custom:Some(SavedValue::String("load".into())),..Default::default()};
        assert!(ScriptHost::new_staged_with_ui_state(&xml("function onLoad(state) playNote(60,100,0) end"),(),Config::default(),Some(&state)).is_err());
    }
    #[test]
    fn staged_restored_widget_play_cannot_escape_before_any_core_context(){
        let script="k=Knob('root',0,0,1);k.changed=function(self) playNote(60,100,0) end";
        let mut state=UiState::default();state.widgets.push((1,"root".into(),SavedValue::Number(1.)));
        let result=ScriptHost::new_staged_with_ui_state(&xml(script),(),Config::default(),Some(&state));
        match result{Err(_)=>{},Ok(mut host)=>{assert!(host.shared.voices.borrow().is_empty());assert!(host.take_commands().is_empty());assert!(host.findings().iter().any(|finding|finding.feature.contains("refused")||finding.value.contains("refused")||finding.value.contains("staged playNote")));}}
    }
    #[test]
    fn unbound_promoted_constant_refusal_does_not_retain_or_seed_a_listener(){
        let xml="<Program><ControlSignalSources><ConstantModulation Name='A' Value='0.25' Style='0' ScriptLive='1000'/></ControlSignalSources><EventProcessors><ScriptProcessor><script>function onController(e) Program.modulations['A']:setParameter('Value',1) end</script></ScriptProcessor></EventProcessors></Program>";
        let mut host=ScriptHost::new(xml,(),Config::default()).unwrap();assert!(host.take_commands().is_empty(),"construction is a standing schema default, never a nil listener send");let before=host.shared.params.borrow().clone();
        host.controller(1,127,0);assert_eq!(*host.shared.params.borrow(),before);assert!(host.take_commands().is_empty());host.controller(1,127,0);assert_eq!(*host.shared.params.borrow(),before);
    }
    #[test]
    fn rejected_release_retains_actual_played_voice_and_sounding_core_owner(){
        use crate::scripted::{Driver,HostInput};use sampler_core::{Prepared,Pcm,Region,Envelope,Playback,Limits,Runtime};
        let prepared=Prepared::new(48000,vec![Pcm::new(48000,vec![[0.25;2];256].into_boxed_slice()).unwrap()],vec![Region{sample:0,key_low:60,key_high:60,root_key:Some(60),velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::new(0,0,0,1.,64).unwrap(),playback:Playback::default()}],1).unwrap();let limits=Limits::for_plan(&prepared,8,8);let mut rt=Runtime::new(prepared,limits).unwrap();
        let host=ScriptHost::new(&xml("function onController(e) voice=playNote(60,100,0) end"),(),Config::default()).unwrap();let lua=host.lua.clone();let shared=host.shared.clone();let mut driver=Driver::new(host,vec![],48000);
        driver.input(&rt,HostInput::Controller{cc:1,value:127,channel:0});driver.wake(&mut rt).unwrap();let id=lua.globals().raw_get::<u64>("voice").unwrap();assert!(shared.voices.borrow().contains(&id));assert_eq!(rt.voice_count(),1);
        // Trigger the real context-restoration fence, not a made-up NoteId or
        // a mutated command acceptance result. No command can be accepted now.
        let context=input_authority::enter(&lua,&shared,Some(0)).unwrap();lua.sandbox(true).unwrap();assert!(context.restore().is_err());drop(context);lua.sandbox(false).unwrap();
        for _ in 0..2{let accepted:bool=lua.load(format!("return releaseVoice({id})")).eval().unwrap();assert!(!accepted);assert!(shared.voices.borrow().contains(&id));}
        driver.wake(&mut rt).unwrap();assert_eq!(rt.voice_count(),1);let mut output=[[0.;2];16];rt.render(&mut output).unwrap();assert!(output.iter().flatten().any(|v|*v>0.01));rt.panic();
    }
}
