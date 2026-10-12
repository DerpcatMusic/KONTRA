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

mod async_data;
mod modules;
#[cfg(feature = "scan")]
pub mod diagnostics;
#[path = "parameters.rs"]
pub(crate) mod parameters;
mod ui;
pub use ui::{SavedValue, UiState, control_id};

const PRELUDE: &str = include_str!("script_prelude.lua");
const INIT_BUDGET: &str = "uvi_lua_init unsupported: initialization work budget exceeded";
/// Where `require` finds a module: a bank's script members.
pub trait Files {
    fn script(&self, module: &str) -> Option<String>;
    fn script_path(&self, _module: &str) -> Option<String> {
        None
    }
}

impl<T: Files> Files for std::rc::Rc<T> {
    fn script(&self, module: &str) -> Option<String> {
        (**self).script(module)
    }
    fn script_path(&self, module: &str) -> Option<String> {
        (**self).script_path(module)
    }
}

/// A bank's Lua members, by path.
#[derive(Clone, Default)]
pub struct Scripts {
    files: Vec<(String, String)>,
}

impl Scripts {
    /// The paths of the scripts held, for surveys.
    #[doc(hidden)]
    pub fn names(&self) -> Vec<&str> {
        self.files.iter().map(|(p, _)| p.as_str()).collect()
    }

    pub fn insert(&mut self, path: &str, source: String) {
        self.files
            .push((path.to_lowercase().replace('\\', "/"), source));
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
    /// Exact bank members win; relative slash/dot aliases require identical source.
    fn script(&self, module: &str) -> Option<String> {
        self.member(module).map(|(_, source)| source.clone())
    }
    fn script_path(&self, module: &str) -> Option<String> {
        self.member(module).map(|(path, _)| path.clone())
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

#[derive(Clone, Debug, PartialEq)]
pub struct Play {
    /// The voice id `playNote` returned to the script.
    pub id: u64,
    pub at_ms: f64,
    pub key: u8,
    pub velocity: u8,
    /// Milliseconds until its own release; `Some(0.0)` sends only the note-on
    /// (the script releases it); `None` (-1, or unset) follows the originating
    /// note.
    pub duration_ms: Option<f64>,
    /// 1-based layers it may sound in; empty is all of them.
    pub layers: Layers,
    /// Oscillator within each keygroup as the script wrote it: 0-based (a script
    /// maps velocity ranges to `0, rr, 2*rr...` plus a round-robin offset).
    pub osc: Option<u32>,
    pub vol: f64,
    pub pan: f64,
    pub tune: f64,
    /// The script event that caused it, if any.
    pub parent: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// A typed insert write, normalized by the shared native law.
    EngineParameter {
        address: sampler_core::EngineParameterAddress,
        value: i32,
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
    note: Option<u64>,
    release: bool,
}

struct Shared {
    #[cfg(feature = "scan")]
    progress: Option<std::sync::Arc<diagnostics::ScanProgress>>,
    #[cfg(feature = "scan")]
    init_api: RefCell<BTreeMap<&'static str, (u64, Duration)>>,
    #[cfg(feature = "scan")]
    audit_init: bool,
    #[cfg(feature = "scan")]
    scan: RefCell<ScanFaults>,
    initializing: Cell<bool>,
    faults: RefCell<FaultCounts>,
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
    current: Cell<Option<u64>>,
    commands: RefCell<Vec<Command>>,
    findings: RefCell<BTreeMap<(String, Option<SetterTypes>), Finding>>,
    waiting: RefCell<Vec<Waiting>>,
    deferred: RefCell<Vec<(Thread, MultiValue, Option<u64>)>>,
    data_loads: RefCell<async_data::DataLoads>,
    params: RefCell<Vec<Vec<(String, String)>>>,
    scopes: RefCell<Vec<Option<Scope>>>,
    nodes: RefCell<Vec<(usize, bool)>>,
    kinds: RefCell<Vec<String>>,
    /// Globals the scripts assign somewhere: reading one that is still unset
    /// answers nil, as in Lua; only unknown API names answer a stub.
    assigned: RefCell<std::collections::HashSet<String>>,
    /// Names the scripts call or index as objects: unmodeled API when unset.
    called: RefCell<std::collections::HashSet<String>>,
    /// The preset's saved widget values and table data (ScriptProcessor
    /// attributes and ScriptData), by widget name.
    saved: RefCell<BTreeMap<String, String>>,
    files: Box<dyn Files>,
    config: Config,
    tempo: Cell<f64>,
    /// When each key went down (ms), if it is down.
    down: RefCell<[Option<f64>; 128]>,
    held: RefCell<std::collections::BTreeSet<u64>>,
    voices: RefCell<std::collections::BTreeSet<u64>>,
    playing: Cell<bool>,
    beat_anchor: Cell<(f64, f64)>,
    cc: RefCell<[u8; 128]>,
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
            (self.audit_init && self.initializing.get()).then(Instant::now),
        )
    }
}

impl Shared {
    fn find(&self, feature: &str, value: &str) {
        self.finding_revision
            .set(self.finding_revision.get().saturating_add(1));
        if matches!(feature, "lua error" | "lua UI callback") {
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

    fn command(&self, command: Command) -> bool {
        let mut commands = self.commands.borrow_mut();
        if commands.len() < 1 << 16 {
            commands.push(command);
            true
        } else {
            drop(commands);
            self.find("command queue full, commands dropped", "");
            false
        }
    }
}

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
    /// Per element: its XML node and whether it is an insert.
    nodes: Vec<(usize, bool)>,
    kinds: Vec<String>,
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
    tree.nodes.push((node.id().get_usize(), insert));
    tree.kinds.push(node.tag_name().name().to_owned());
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
        let mut names = self.assigned.borrow_mut();
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
        let mut names = self.called.borrow_mut();
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
        let options = roxmltree::ParsingOptions {
            nodes_limit: 4_000_000,
            ..Default::default()
        };
        let doc = {
            let _span = Span::new("uvi_lua_xml");
            roxmltree::Document::parse_with_options(xml, options).map_err(|e| e.to_string())?
        };
        let lua = {
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
            #[cfg(feature = "scan")]
            progress: (config.audit_seed.is_some() && config.audit_progress)
                .then(|| std::sync::Arc::new(diagnostics::ScanProgress::new(config.load_work))),
            #[cfg(feature = "scan")]
            init_api: RefCell::new(BTreeMap::new()),
            #[cfg(feature = "scan")]
            audit_init: std::env::var_os("KONTRA_AUDIT_LOAD").is_some(),
            #[cfg(feature = "scan")]
            scan: RefCell::new(ScanFaults::default()),
            initializing: Cell::new(true),
            faults: RefCell::new(FaultCounts::default()),
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
            current: Cell::new(None),
            commands: RefCell::new(Vec::new()),
            findings: RefCell::new(BTreeMap::new()),
            waiting: RefCell::new(Vec::new()),
            deferred: RefCell::new(Vec::new()),
            data_loads: RefCell::new(async_data::DataLoads::default()),
            params: RefCell::new(Vec::new()),
            scopes: RefCell::new(Vec::new()),
            nodes: RefCell::new(Vec::new()),
            kinds: RefCell::new(Vec::new()),
            assigned: RefCell::new(Default::default()),
            called: RefCell::new(Default::default()),
            saved: RefCell::new(BTreeMap::new()),
            files: Box::new(files),
            config,
            tempo: Cell::new(120.0),
            down: RefCell::new([None; 128]),
            held: RefCell::new(Default::default()),
            voices: RefCell::new(Default::default()),
            playing: Cell::new(false),
            beat_anchor: Cell::new((0., 0.)),
            cc: RefCell::new([0; 128]),
        });
        let host = Self { lua, shared };
        let initialized = host
            .install()
            .and_then(|_| host.build_program(&doc))
            .map_err(lua_error)
            .and_then(|_| host.load_scripts(&doc, state));
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
        host.shared.initializing.set(false);
        Ok(host)
    }

    fn build_program(&self, doc: &roxmltree::Document) -> mlua::Result<()> {
        let _span = Span::new("uvi_lua_graph");
        let program = doc
            .descendants()
            .find(|n| n.has_tag_name("Program"))
            .ok_or_else(|| mlua::Error::runtime("no Program"))?;
        for processor in program
            .descendants()
            .filter(|n| n.has_tag_name("ScriptProcessor"))
        {
            let mut saved = self.shared.saved.borrow_mut();
            for node in std::iter::once(processor).chain(
                processor
                    .children()
                    .filter(|c| c.has_tag_name("ScriptData")),
            ) {
                for a in node.attributes() {
                    saved.insert(a.name().to_owned(), a.value().to_owned());
                }
            }
        }
        let mut tree = Tree {
            params: Vec::new(),
            scopes: Vec::new(),
            nodes: Vec::new(),
            kinds: Vec::new(),
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
        // The part the program sits in (MidiChannel, MidiInput...): inert.
        let part = self.lua.create_table()?;
        part.raw_set("__id", tree.params.len())?;
        tree.params.push(Vec::new());
        tree.scopes.push(None);
        tree.nodes.push((usize::MAX, false));
        tree.kinds.push("Part".into());
        part.raw_set("type", "Part")?;
        part.raw_set("name", "")?;
        part.set_metatable(Some(class))?;
        root.raw_set("parent", part.clone())?;
        root.raw_set("part", part)?;
        *self.shared.params.borrow_mut() = tree.params;
        *self.shared.scopes.borrow_mut() = tree.scopes;
        *self.shared.nodes.borrow_mut() = tree.nodes;
        *self.shared.kinds.borrow_mut() = tree.kinds;
        self.lua.globals().raw_set("Program", root)
    }

    fn install(&self) -> mlua::Result<()> {
        let _span = Span::new("uvi_lua_host_install");
        let lua = &self.lua;
        // Only table/string/math/coroutine are loaded (no io, os, debug,
        // package); `lua.sandbox` would give every coroutine its own proxy
        // environment, which breaks the globals the prelude and scripts share.
        let globals = lua.globals();
        let shared = &self.shared;

        // Count Luau call/loop-backedge checkpoints, not elapsed scheduler time.
        let budget = shared.clone();
        lua.set_interrupt(move |_| {
            budget
                .vm_checkpoints
                .set(budget.vm_checkpoints.get().saturating_add(1));
            budget.consume_work()?;
            Ok(VmState::Continue)
        });

        // Natives the prelude wraps (`__native`) and the engine API (globals).
        let native = lua.create_table()?;
        async_data::install(lua, shared)?;
        for name in ["loadSample", "loadImpulse"] {
            let s = shared.clone();
            globals.raw_set(
                name,
                lua.create_function(move |lua, args: Variadic<Value>| {
                    // Failed resource tasks still complete; do not invent a successful load.
                    s.find(&format!("lua {name} resource task unavailable"), "");
                    if let Some(Value::Function(callback)) =
                        args.iter().find(|v| matches!(v, Value::Function(_)))
                    {
                        let thread = lua.create_thread(callback.clone())?;
                        s.deferred.borrow_mut().push((
                            thread,
                            MultiValue::from_vec(vec![Value::Nil]),
                            None,
                        ));
                    }
                    Ok(())
                })?,
            )?;
        }
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
            lua.create_function(|lua, path: String| {
                // v1 4bffbb18:src/uvi/host.rs retains empty artwork/font assignments.
                // They clear a resource; resolving them to a module directory creates a phantom asset.
                if path.is_empty() || path.starts_with(['/', '$']) {
                    return Ok(path);
                }
                for level in 0..32 {
                    let Some(source) =
                        lua.inspect_stack(level, |d| d.source().source.map(|s| s.into_owned()))
                    else {
                        break;
                    };
                    if let Some(source) = source {
                        let source = source.trim_start_matches('@');
                        if source.ends_with(".lua")
                            && let Some((base, _)) = source.rsplit_once('/')
                        {
                            return Ok(format!("/{base}/{path}"));
                        }
                    }
                }
                Ok(path)
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "saved",
            lua.create_function(move |_, name: String| Ok(s.saved.borrow().get(&name).cloned()))?,
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
                    .map(|(_, value)| {
                        if value.parse::<f64>().is_ok() {
                            "float"
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
        let retained_fields = lua.create_table()?;
        let clone_table: Function = globals.raw_get::<Table>("table")?.raw_get("clone")?;
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
                            schema.raw_push(d)?;
                        }
                        schemas.raw_set(kind, schema.clone())?;
                        schema
                    }
                };
                let defs = lua.create_table()?;
                for (p, template) in parameters::definitions(kind)
                    .iter()
                    .zip(schema.sequence_values::<Table>())
                {
                    let d: Table = clone_table.call(template?)?;
                    defs.raw_set(p.name, d.clone())?;
                    defs.raw_push(d)?;
                }
                // The public numeric catalog omits string fields and some graph
                // nodes. Retain their typed XML identity without fabricating native
                // bounds or omitted defaults. Provenance distinguishes these facts.
                if let Some(params) = s.params.borrow().get(id) {
                    for (name, value) in params {
                        if !defs.raw_get::<Value>(name.as_str())?.is_nil() {
                            continue;
                        }
                        let kind = if value.parse::<f64>().is_ok() {
                            "float"
                        } else {
                            "string"
                        };
                        let key = format!("{kind}:{name}");
                        let template = match retained_fields.raw_get::<Value>(key.as_str())? {
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
                                retained_fields.raw_set(key.as_str(), template.clone())?;
                                template
                            }
                        };
                        let d: Table = clone_table.call(template)?;
                        d.set("id", defs.raw_len() + 1)?;
                        defs.raw_set(name.as_str(), d.clone())?;
                        defs.raw_push(d)?;
                    }
                }
                Ok(defs)
            })?,
        )?;
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
                // Retained XML fields have local model identity, not DSP acceptance.
                let catalogued = parameters::definitions(
                    s.kinds.borrow().get(id).map_or("", String::as_str),
                )
                .iter()
                .any(|p| p.name == name);
                if !catalogued {
                    return Ok(s.params.borrow().get(id).and_then(|params| {
                        params.iter().any(|(key, _)| *key == name).then_some(false)
                    }));
                }
                let value = match value {
                    Value::Integer(n) => n as f64,
                    Value::Number(n) => n,
                    _ => return Ok(None),
                };
                let processor = s
                    .nodes
                    .borrow()
                    .get(id)
                    .copied()
                    .filter(|(_, insert)| *insert)
                    .and_then(|(node, _)| {
                        crate::engine_parameters::binding(
                            node,
                            s.kinds.borrow().get(id)?.as_str(),
                            &name,
                        )
                    });
                if let Some(binding) = processor {
                    let Ok(value) = binding.law.normalized_value(value) else {
                        return Ok(None);
                    };
                    return Ok(s
                        .command(Command::EngineParameter {
                            address: binding.address,
                            value,
                        })
                        .then_some(true));
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
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "param",
            lua.create_function(move |lua, (id, name): (usize, String)| {
                #[cfg(feature = "scan")]
                let _timer = s.api_timer("uvi_lua_api_get_param");
                let kinds = s.kinds.borrow();
                let definition = parameters::definitions(kinds.get(id).map_or("", String::as_str))
                    .iter()
                    .find(|p| p.name == name);
                let found = s
                    .params
                    .borrow()
                    .get(id)
                    .and_then(|p| p.iter().find(|(k, _)| *k == name))
                    .map(|(_, v)| v.clone());
                Ok(match found {
                    None => match definition {
                        Some(p) if p.kind == "boolean" => Value::Boolean(p.default != 0.),
                        Some(p) => Value::Number(p.default),
                        None => Value::Nil,
                    },
                    Some(v) if definition.is_some_and(|p| p.kind == "boolean") => {
                        Value::Boolean(v == "1" || v == "true")
                    }
                    Some(v) => match v.parse::<f64>() {
                        Ok(n) => Value::Number(n),
                        Err(_) => Value::String(lua.create_string(&v)?),
                    },
                })
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "defglobal",
            // The main environment: a coroutine's own is a throwaway proxy.
            lua.create_function({
                let env = globals.clone();
                move |_, (name, value): (String, Value)| env.raw_set(name, value)
            })?,
        )?;
        native.set("nextId", lua.create_function(move |_, ()| Ok(s.next_id()))?)?;
        let s = shared.clone();
        native.set(
            "source",
            lua.create_function(move |_, name: String| {
                Ok(s.files.script(&name).or_else(|| {
                    (name == "uvi.ChordRec").then(|| modules::CHORD_REC.to_owned())
                }))
            })?,
        )?;
        native.set("asyncUpdater", lua.create_function(|lua, ()| {
            let factory = lua.create_userdata(modules::AsyncUpdaterFactory)?;
            factory.set_user_value(lua.globals())?;
            Ok(factory)
        })?)?;
        let s = shared.clone();
        native.set(
            "assigned",
            lua.create_function(move |_, name: String| {
                // Capitalized names are API classes (`WaveView = WaveView{...}`
                // reads the class first). Unset lowercase names the scripts
                // only read as values are variables (nil, as in Lua); the
                // ones they call are unmodeled API and answer a stub.
                Ok(name.starts_with(|c: char| c.is_ascii_lowercase())
                    && (s.assigned.borrow().contains(&name) || !s.called.borrow().contains(&name)))
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "compile",
            lua.create_function(move |lua, (source, name): (mlua::LuaString, String)| {
                let _span = Span::new("uvi_lua_module_compile");
                {
                    let _span = Span::new("uvi_lua_module_names");
                    s.note_assigned(&source.to_string_lossy());
                }
                let name = s.files.script_path(&name).unwrap_or(name);
                match lua
                    .load(source.as_bytes().as_ref())
                    .set_name(name)
                    .into_function()
                {
                    Ok(f) => Ok((Some(f), None)),
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
                let play = parse_play(&s, &args);
                let id = play.id;
                s.voices.borrow_mut().insert(id);
                s.command(Command::Play(play));
                Ok(id)
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "postNote",
            lua.create_function(move |_, event: Table| {
                let mut play = parse_play(&s, &[Value::Table(event.clone())]);
                play.id = field(&event, "id")
                    .or_else(|| field(&event, "voiceId"))
                    .map_or(play.id, |id| id as u64);
                s.voices.borrow_mut().insert(play.id);
                s.command(Command::Play(play.clone()));
                Ok(play.id)
            })?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "releaseVoice",
            lua.create_function(move |_, id: f64| {
                if !s.voices.borrow_mut().remove(&(id as u64)) {
                    return Ok(false);
                }
                s.command(Command::Release {
                    id: id as u64,
                    at_ms: s.now.get(),
                });
                Ok(true)
            })?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "sendScriptModulation",
            lua.create_function(
                move |_, (id, value, glide, voice): (f64, f64, Option<f64>, Option<f64>)| {
                    s.command(Command::Modulation {
                        id: id.clamp(0.0, f64::from(u16::MAX)) as u16,
                        value,
                        glide_ms: glide.unwrap_or(20.0).max(0.0),
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
                s.deferred.borrow_mut().push((thread, args, None));
                Ok(())
            })?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "run",
            lua.create_function(move |lua, (f, args): (Function, MultiValue)| {
                let thread = lua.create_thread(f)?;
                resume(&s, thread, args, s.current.get());
                Ok(())
            })?,
        )?;
        self.install_api()?;
        modules::install_class(lua, &globals)?;
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
            let down = s.down.borrow();
            Ok(down
                .get(note as usize)
                .copied()
                .flatten()
                .map_or(0.0, |at| (s.now.get() - at).max(0.0)))
        });
        define!("isKeyDown", s, move |_, note: f64| {
            Ok(s.down
                .borrow()
                .get(note as usize)
                .is_some_and(Option::is_some))
        });
        define!("isOctaveKeyDown", s, move |_, note: f64| {
            let class = (note as usize) % 12;
            Ok(s.down
                .borrow()
                .iter()
                .enumerate()
                .any(|(k, d)| k % 12 == class && d.is_some()))
        });
        define!("isNoteHeld", s, move |_, ()| Ok(s
            .current
            .get()
            .is_some_and(|id| s.held.borrow().contains(&id))));
        define!("getCC", s, move |_, cc: f64| {
            Ok(s.cc.borrow().get(cc as usize).copied().unwrap_or(0))
        });
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
            s.command(Command::Modulation {
                id,
                value: from,
                glide_ms: 0.0,
                voice,
                at_ms,
            });
            s.command(Command::Modulation {
                id,
                value: to,
                glide_ms: ramp.unwrap_or(20.0).max(0.0),
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

    fn load_scripts(
        &self,
        doc: &roxmltree::Document,
        state: Option<&UiState>,
    ) -> Result<(), String> {
        let _span = Span::new("uvi_lua_scripts");
        for script in doc.descendants().filter(|n| n.has_tag_name("script")) {
            if script.ancestors().any(|n| {
                n.has_tag_name("ScriptProcessor")
                    && n.attribute("Bypass")
                        .is_some_and(|v| v == "1" || v == "true")
            }) {
                continue;
            }
            let text: String = script.text().unwrap_or_default().to_owned();
            if text.trim().is_empty() {
                continue;
            }
            self.shared.note_assigned(&text);
            self.shared
                .arm(self.shared.config.load, self.shared.config.load_work);
            let function = {
                let _span = Span::new("uvi_lua_root_compile");
                self.lua
                    .load(&text)
                    .set_name("script")
                    .into_function()
                    .map_err(lua_error)?
            };
            let thread = self.lua.create_thread(function).map_err(lua_error)?;
            {
                let _span = Span::new("uvi_lua_script_body");
                resume(&self.shared, thread, MultiValue::new(), None);
                self.cycle();
            }
            let restored = Span::new("uvi_lua_restore");
            // Initial load sees constructor values; explicit restoration below
            // uses the opposite order and never reruns onInit.
            if let Some(state) = state {
                self.restore_ui_custom(state)?;
            } else if let Some(saved) = script
                .parent()
                .and_then(|p| p.children().find(|n| n.has_tag_name("state")))
            {
                self.restore_ui_custom(&UiState {
                    custom: Some(ui::json_state(saved.text().unwrap_or_default())?),
                    ..Default::default()
                })?;
            }
            // Saved widget values and their `changed` callbacks come after the script body
            // and before onInit, which is why scripts test for a restored zero there.
            if let Ok(Value::Function(f)) = self.lua.globals().raw_get::<Value>("__restore") {
                self.shared
                    .arm(self.shared.config.load, self.shared.config.load_work);
                if let Ok(thread) = self.lua.create_thread(f) {
                    resume(&self.shared, thread, MultiValue::new(), None);
                    self.cycle();
                }
            }
            if let Some(state) = state {
                self.restore_ui_values(state)?;
            }
            drop(restored);
            let _span = Span::new("uvi_lua_on_init");
            self.call("onInit", None);
        }
        Ok(())
    }

    /// Start spawned threads until none remain.
    fn cycle(&self) {
        loop {
            let batch = std::mem::take(&mut *self.shared.deferred.borrow_mut());
            if batch.is_empty() {
                break;
            }
            for (thread, args, note) in batch {
                if self.shared.consume_work().is_err() {
                    self.shared.deferred.borrow_mut().clear();
                    self.shared.find("lua error", "work budget exceeded");
                    return;
                }
                resume(&self.shared, thread, args, note);
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
        let note = event
            .as_ref()
            .and_then(|e| field(e, "id"))
            .map(|id| id as u64);
        let args: MultiValue = event.map(Value::Table).into_iter().collect();
        resume(&self.shared, thread, args, note);
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
        self.shared.held.borrow_mut().insert(id);
        self.shared.voices.borrow_mut().insert(id);
        self.shared.down.borrow_mut()[usize::from(key & 127)] = Some(self.shared.now.get());
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
        self.shared.held.borrow_mut().remove(&id);
        self.shared.down.borrow_mut()[usize::from(key & 127)] = None;
        let woken: Vec<Waiting> = {
            let mut waiting = self.shared.waiting.borrow_mut();
            let (woken, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut *waiting)
                .into_iter()
                .partition(|w| w.release && w.note == Some(id));
            *waiting = rest;
            woken
        };
        for w in woken {
            self.shared.arm(
                self.shared.config.callback,
                self.shared.config.callback_work,
            );
            resume(&self.shared, w.thread, MultiValue::new(), w.note);
        }
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
        self.shared.cc.borrow_mut()[usize::from(controller & 127)] = value;
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
            resume(&self.shared, thread, args, None);
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
            self.shared.now.set(w.due.max(self.shared.now.get()));
            if self.shared.consume_work().is_err() {
                self.shared.find("lua error", "work budget exceeded");
                break;
            }
            resume(&self.shared, w.thread, MultiValue::new(), w.note);
            self.cycle();
        }
        self.shared.now.set(now_ms.max(self.shared.now.get()));
        async_data::poll(self);
        self.cycle();
        #[cfg(feature = "scan")]
        if let Some(progress) = &self.shared.progress {
            progress.clock(self.shared.now.get());
        }
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
        let mut out = Vec::new();
        let Ok(touched) = self.lua.globals().raw_get::<Table>("__touched") else {
            return out;
        };
        let nodes = self.shared.nodes.borrow();
        for element in touched.sequence_values::<Table>().flatten() {
            let Ok(id) = element.raw_get::<usize>("__id") else {
                continue;
            };
            let Some(&(node, true)) = nodes.get(id) else {
                continue;
            };
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
        out
    }

    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut *self.shared.commands.borrow_mut())
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

    pub fn memory(&self) -> usize {
        self.lua.used_memory()
    }
}

fn resume(shared: &Rc<Shared>, thread: Thread, args: MultiValue, note: Option<u64>) {
    #[cfg(feature = "scan")]
    if let Some(progress) = &shared.progress {
        progress.resume(shared.now.get());
    }
    let before = shared.current.replace(note);
    let result = thread.resume::<MultiValue>(args);
    shared.current.set(before);
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
                note,
                release,
            });
        }
    }
}

fn parse_play(shared: &Shared, args: &[Value]) -> Play {
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
    Play {
        id: shared.next_id(),
        at_ms: shared.now.get(),
        key: num(0).unwrap_or(60.0).clamp(0.0, 127.0) as u8,
        velocity: num(1).unwrap_or(100.0).clamp(1.0, 127.0) as u8,
        duration_ms: duration,
        layers,
        osc: num(10).map(|n| n as u32),
        vol: num(6).unwrap_or(1.0),
        pan: num(7).unwrap_or(0.0),
        tune: num(8).unwrap_or(0.0),
        parent: shared.current.get(),
    }
}

#[cfg(test)]
mod tests {
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
            "function onNote(e) run(function() waitForRelease(); releaseVoice(e.id) end) end\n\
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
        assert!(
            matches!(&c[6], Command::Modulation { id: 3, value, glide_ms: 0.0, .. } if *value == 0.2)
        );
        assert!(matches!(
            &c[7],
            Command::Modulation {
                id: 3,
                glide_ms: 100.0,
                ..
            }
        ));
        assert_eq!(
            c[8],
            Command::Midi(MidiOut {
                status: 0xb1,
                a: 1,
                b: 64
            })
        );
        assert_eq!(
            c[9],
            Command::Midi(MidiOut {
                status: 0xe0,
                a: 0,
                b: 64
            })
        );
        assert_eq!(
            c[10],
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
