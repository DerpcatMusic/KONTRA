//! The plugin: persisted rack state, the loader task and the audio callback,
//! composed around the v2 sound core ([`crate::sound::v2`]).
//!
//! Threads: the host's audio thread runs [`Sampler::process`] and talks to
//! [`V2Core`] only; the serialized [`Load`] task prepares parts with
//! [`V2Loader`], routes outputs ([`routing`]) and hands parts, mixes and
//! larger storage to the audio thread through lock-free queues. Everything the
//! audio thread replaces goes back to the loader to be dropped.

mod automation;
mod host_probe;
mod held_keys;
mod snapshot_reuse;
#[cfg(test)]
pub(crate) mod snapshot_reuse_tests;
pub(crate) mod ui_activity;
use ui_activity::Count as UiCount;
pub(crate) use host_probe::export_multi_state;
pub(crate) mod automation_ids;

use crate::sound::{
    BUSES, BlockInfo, Core, CoreError, CoreLoader, LoadRequest, MAX_BLOCK, Progress, RACK_SLOTS,
    Rendered, Streaming, TUNE_RANGE, Transport,
    event::{Event as CoreEvent, HostNote, HostPattern, NoteExpression},
    mix::{BusControls, Mix, NO_AUX, PartControls, db_gain},
    report::{LoadReport, RuntimeProblems},
    tree::{MixTree, NodeMix},
    v2::{Part as CorePart, Retired as CoreRetired, V2Core, V2Loader},
};
use crate::support::{MutexExt, RwLockExt};
use crate::{library, routing};
use crossbeam_queue::ArrayQueue;
use moose::core::{
    ExactAddress, ExactEvent, ExactEventBody, ExactEventRef, ExactNoteAddress, ExactNoteKind,
    LosslessEventRef,
};
use moose::mui::mui::scene::Image;
use moose::prelude::*;
mod uvi_meters;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Mutex, RwLock,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    time::Instant,
};

/// One rack part, as the host saves it.
#[derive(State, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Part {
    /// Typed UVI widget/custom state, captured on the Lua worker.
    pub uvi_state: String,
    pub uvi_state_source: String,
    pub path: String,
    /// Program inside a bank file.
    pub program: u32,
    /// MIDI port 0..=3 (A..D).
    pub port: u8,
    /// The DAW stereo pair (output bus) the instrument plays to.
    pub output: u8,
    /// MIDI channel 0..=15, or -1 for omni.
    pub channel: i16,
    /// dB, -60..=6.
    pub gain: f32,
    pub pan: f32,
    /// Semitones, cents as the fraction.
    pub tune: f32,
    pub mute: bool,
    pub solo: bool,
    /// The name the player gave the part; empty shows the instrument's.
    pub name: String,
    /// The rack shows only the part's header.
    pub collapsed: bool,
    /// v1 codes: 0 global default, 1 Original, 2 KONTRA, 3 Vector.
    pub view: u8,
    /// The part's height in the rack when the player sized it; 0 is automatic.
    pub height: f32,
    /// Output bus (0..[`BUSES`]) the part also sends to, post-fader; -1 for none.
    pub aux: i16,
    /// Level of that send in dB (-60..=6).
    pub aux_gain: f32,
    /// The player picked [`Part::output`]: automatic routing leaves it.
    pub output_manual: bool,
    /// The settings of the instrument's output tree below its root
    /// ([`crate::sound::tree`]), one per node after node 0.
    pub nodes: Vec<NodeMix>,
    /// MPE: each note on its own member channel with its own bend,
    /// pressure and timbre (lower zone, manager channel 1).
    pub mpe: bool,
    /// Where the dynamics controllers (CC1/CC11...) start before the host
    /// moves them, 0..=127; -1 keeps Kontakt's own power-on state.
    pub dynamics: i16,
    /// Pitch-bend range in semitones each way; 0 keeps the instrument's own.
    pub bend_range: u8,
    /// How articulations are selected, as `sampler_ir::Switching::to_bits`
    /// with bit 7 set once the player remapped; 0 keeps the instrument's own.
    pub switching: u8,
    pub articulation_overlay: crate::sound::articulation::Overlay,
    /// Where samples play from; None follows the rack. Ported from v1.
    pub streaming: Option<Streaming>,
    /// Upper MPE zone: manager on channel 16.
    pub mpe_upper: bool,
    /// Snapshot applied to path, which remains the explicit base NKI.
    pub snapshot: String,
    /// Kontakt control identities and semantic values, independent of presentation.
    pub control_values: Vec<SavedControl>,
    pub group: u32,
    pub edits: crate::sound::edits::Edits,
    pub timing: crate::timing::Timing,
    pub script_state: String,
    pub script_state_source: String,
}

impl Default for Part {
    fn default() -> Self {
        Self {
            uvi_state: String::new(),
            uvi_state_source: String::new(),
            path: String::new(),
            program: 0,
            port: 0,
            output: 0,
            channel: -1,
            gain: 0.,
            pan: 0.,
            tune: 0.,
            mute: false,
            solo: false,
            name: String::new(),
            collapsed: false,
            view: 0,
            height: 0.,
            aux: -1,
            aux_gain: 0.,
            output_manual: false,
            nodes: Vec::new(),
            mpe: false,
            dynamics: -1,
            bend_range: 0,
            switching: 0,
            articulation_overlay: Default::default(),
            streaming: None,
            mpe_upper: false,
            snapshot: String::new(),
            control_values: Vec::new(),
            group: 0,
            edits: Default::default(),
            timing: Default::default(),
            script_state: String::new(),
            script_state_source: String::new(),
        }
    }
}

/// Split identity words stay exact in JSON and in the host's state codec.
#[derive(State, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SavedControl {
    pub high: u64,
    pub low: u64,
    pub value: f64,
}

impl SavedControl {
    fn new(id: sampler_ui_ir::ControlId, value: f64) -> Self {
        Self {
            high: (id.0 >> 64) as u64,
            low: id.0 as u64,
            value,
        }
    }
    fn id(&self) -> sampler_ui_ir::ControlId {
        sampler_ui_ir::ControlId(u128::from(self.high) << 64 | u128::from(self.low))
    }
}

impl Part {
    /// What the loader prepares for this part.
    pub(crate) fn source(&self) -> (String, u32, String) {
        (self.path.clone(), self.program, self.snapshot.clone())
    }

    pub fn streaming(&self, rack: Streaming) -> Streaming {
        self.streaming.unwrap_or(rack)
    }

    pub(crate) fn snapshot_base(&self) -> bool {
        self.program == 0
            && Path::new(&self.path)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
    }

    pub(crate) fn select_snapshot(&mut self, path: String) {
        self.snapshot = path;
        self.control_values.clear();
        // Project captures belong to host recall, not a new explicit preset.
        self.script_state.clear();
        self.script_state_source.clear();
        self.uvi_state.clear();
        self.uvi_state_source.clear();
    }
}

impl StateField for Streaming {
    fn write_field(&self, buf: &mut Vec<u8>) {
        u8::from(*self == Streaming::RamOnly).write_field(buf);
    }
    fn read_field(cursor: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        Some(match u8::read_field(cursor)? {
            1 => Streaming::RamOnly,
            _ => Streaming::Auto,
        })
    }
}
impl StateField for NodeMix {
    fn write_field(&self, buf: &mut Vec<u8>) {
        serde_json::to_string(self)
            .unwrap_or_default()
            .write_field(buf);
    }
    fn read_field(cursor: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        Some(serde_json::from_str(&String::read_field(cursor)?).unwrap_or_default())
    }
}

impl StateField for crate::sound::articulation::Overlay {
    fn write_field(&self, buf: &mut Vec<u8>) {
        serde_json::to_string(self)
            .unwrap_or_default()
            .write_field(buf);
    }
    fn read_field(cursor: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        serde_json::from_str(&String::read_field(cursor)?).ok()
    }
}

/// One of the rack's stereo output buses (Kontakt's st.1…st.16). Parts pick
/// one with [`Part::output`]; the bus's fader follows and it plays through
/// host output port `port`. Buses the player never touched are not stored:
/// [`Selection::bus`] fills in the defaults.
#[derive(State, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Bus {
    /// Empty shows "st.N".
    pub name: String,
    /// dB, -60..=6.
    pub gain: f32,
    /// -1..=1.
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Host stereo output port, 0..[`BUSES`]; -1 plays through the bus's own.
    pub port: i16,
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            name: String::new(),
            gain: 0.,
            pan: 0.,
            mute: false,
            solo: false,
            port: -1,
        }
    }
}

impl Bus {
    /// The name the mixer shows for bus `n`.
    pub fn label(&self, n: usize) -> String {
        if self.name.is_empty() {
            format!("st.{}", n + 1)
        } else {
            self.name.clone()
        }
    }
}

// Port from v1 4bffbb18:src/library.rs::UviSource; split UUID words fit the v2 state codec.
#[derive(State, Default, Clone, PartialEq)]
pub struct UviFavorite {
    pub bank: String,
    pub member: String,
    pub high: u64,
    pub low: u64,
}

impl UviFavorite {
    pub(crate) fn at(path: &Path, shelf: &library::Shelf) -> Option<Self> {
        let (bank, uuid) = path
            .ancestors()
            .find_map(|bank| shelf.bank_ids.get(bank).map(|uuid| (bank, uuid)))?;
        Some(Self {
            bank: bank.to_str()?.into(),
            member: path.strip_prefix(bank).ok()?.to_str()?.into(),
            high: u64::from_le_bytes(uuid[..8].try_into().unwrap()),
            low: u64::from_le_bytes(uuid[8..].try_into().unwrap()),
        })
    }

    pub(crate) fn same_program(&self, other: &Self) -> bool {
        self.high == other.high && self.low == other.low && self.member == other.member
    }

    /// `files` has the native path order emitted by library scans.
    pub(crate) fn resolve(&mut self, shelf: &library::Shelf, files: &[PathBuf]) -> Option<PathBuf> {
        let member = Path::new(&self.member);
        if member.is_absolute()
            || member
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return None;
        }
        let id_matches = |uuid: &[u8; 16]| {
            self.high == u64::from_le_bytes(uuid[..8].try_into().unwrap())
                && self.low == u64::from_le_bytes(uuid[8..].try_into().unwrap())
        };
        let old = Path::new(&self.bank);
        let path = old.join(member);
        if shelf.bank_ids.get(old).is_some_and(id_matches) && files.binary_search(&path).is_ok() {
            return Some(path);
        }
        let mut matches = shelf
            .bank_ids
            .iter()
            .filter(|(_, uuid)| id_matches(uuid))
            .filter_map(|(bank, _)| {
                let path = bank.join(member);
                files.binary_search(&path).is_ok().then_some((bank, path))
            });
        let (bank, path) = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        self.bank = bank.to_str()?.into();
        Some(path)
    }
}

#[derive(State, Default, Clone, PartialEq)]
pub struct Selection {
    pub parts: Vec<Part>,
    pub order: Vec<u32>,
    pub midi_thru: bool,
    pub multi: String,
    /// Presets the player starred, in the order starred.
    pub favorites: Vec<String>,
    /// Presets opened lately, newest first.
    pub recent: Vec<String>,
    /// The computer keyboard plays notes.
    pub qwerty: bool,
    /// The browser's width in points and its upper pane's share of the
    /// height; 0 is the default.
    pub browser_width: f32,
    pub browser_split: f32,
    /// What plays behind a part's controls: 0 plain, 1 its library's
    /// color, 2 its library's artwork.
    pub appearance: u8,
    /// Library artwork behind parts and in their headers shows sharp, not blurred.
    pub sharp_artwork: bool,
    /// Part headers scroll away with their parts instead of stacking at the
    /// rack's top and bottom edges.
    pub sticky_off: bool,
    /// Output buses by index; shorter than [`BUSES`] when the rest are default.
    pub buses: Vec<Bus>,
    /// How parts are routed to buses and host ports ([`routing::Outputs`]).
    pub outputs: u8,
    /// Megabytes of sample start data the rack keeps in memory; past it,
    /// samples idle for [`IDLE_SECONDS`] read from disk again when played.
    /// 0 keeps everything.
    pub memory_budget_mb: u32,
    /// Ported v1 rack sample mode; changing it reloads affected parts.
    pub streaming: Streaming,
    pub auto_align: bool,
    pub align_transport_only: bool,
    /// UVI stars retain authored bank UUID/member identity when a locator moves.
    pub uvi_favorites: Vec<UviFavorite>,
}

/// Seconds a streamed sample goes unplayed before the memory budget may drop it.
/// Largest saved UVI/script state blob a project may carry; save and load agree.
const MAX_SAVED_STATE: usize = 8 << 20;

pub const IDLE_SECONDS: f64 = 30.0;

impl Selection {
    /// Output bus `n`, default when never set.
    pub fn bus(&self, n: usize) -> Bus {
        self.buses.get(n).cloned().unwrap_or_default()
    }

    /// Output bus `n` to edit, created with defaults up to it.
    pub fn bus_mut(&mut self, n: usize) -> &mut Bus {
        let n = n.min(BUSES - 1);
        if self.buses.len() <= n {
            self.buses.resize(n + 1, Bus::default());
        }
        &mut self.buses[n]
    }

    /// Kontakt's auto-increment for a new part: the first MIDI port and
    /// channel (A1…A16, then B1…, up to D16) no loaded part listens on
    /// explicitly; omni on port A when all are taken.
    pub fn next_input(&self) -> (u8, i16) {
        let taken = |port: u8, channel: i16| {
            (self.parts.iter())
                .any(|p| !p.path.is_empty() && p.port == port && p.channel == channel)
        };
        (0..4u8)
            .flat_map(|port| (0..16i16).map(move |channel| (port, channel)))
            .find(|&(port, channel)| !taken(port, channel))
            .unwrap_or((0, -1))
    }
}

#[derive(Params)]
#[params(
    output_port_name = "port_name",
    output_port_names_revision = "port_names_revision",
    presentation = "host_presentation",
    presentation_revision = "host_presentation_revision",
    pre_save = "capture_ui_controls",
    post_load = "reload_ui_controls"
)]
pub struct SamplerParams {
    #[param(
        name = "Volume",
        range = "linear(-60, 6)",
        default = 0.0,
        unit = "dB",
        smooth = "exp(5)"
    )]
    pub volume: FloatParam,
    #[param(
        id = 0x95b8fd,
        name = "Attack",
        range = "log(0.0001, 5)",
        default = 0.002,
        unit = "s"
    )]
    pub attack: FloatParam,
    #[param(
        id = 0x36ae7e,
        name = "Release",
        range = "log(0.001, 10)",
        default = 0.15,
        unit = "s"
    )]
    pub release: FloatParam,
    #[param(
        id = 0x62f120,
        name = "Tone",
        range = "log(20, 20000)",
        default = 20000.0,
        unit = "Hz"
    )]
    pub cutoff: FloatParam,
    #[nested(base = 0)]
    pub host: automation::HostAutomation,
    // Raw MIDI stays port/channel-specific; VST3 supplies its own controller proxies.
    #[persist = "selection"]
    pub selection: RwLock<Selection>,
    #[skip]
    pub shared: Shared,
    #[meter]
    pub level: MeterSlot,
}
pub(crate) use SamplerParamsParamId as P;

impl SamplerParams {
    fn capture_ui_controls(&self) {
        self.shared.refresh_uvi(self);
        let mut selection = self.selection.read_unpoisoned().clone();
        self.shared.capture_ui_controls(&mut selection);
        let mut current = self.selection.write_unpoisoned();
        for (part, captured) in current.parts.iter_mut().zip(selection.parts) {
            if part.source() == captured.source() {
                part.control_values = captured.control_values;
                part.uvi_state = captured.uvi_state;
                part.uvi_state_source = captured.uvi_state_source;
                part.script_state = captured.script_state;
                part.script_state_source = captured.script_state_source;
            }
        }
    }

    fn reload_ui_controls(&self) {
        // Invalidate queued same-owner overlays before host recall can change
        // the desired state, including a recall of the identical source tuple.
        self.shared.with_parts(|parts| {
            for part in parts {
                let _ = part.snapshot_sequence.fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| v.checked_add(1));
            }
        });
        // Recall of the same source still needs fresh script initialization.
        let sources: Vec<_> = self
            .selection
            .read_unpoisoned()
            .parts
            .iter()
            .map(Part::source)
            .collect();
        for (slot, part) in self
            .shared
            .view
            .lock_unpoisoned()
            .parts
            .iter_mut()
            .enumerate()
        {
            part.attempted = None;
            part.active_target = None;
            // Fence recalled state immediately, before the background loader
            // begins; cancellation/failure cannot give the old owner its identity.
            part.recalled_source = sources.get(slot).cloned();
            part.recall_revision = part.recall_revision.wrapping_add(1);
        }
    }

    /// Host output port `index`'s name, as last published (`routing.rs`).
    fn port_name(&self, index: u32) -> Option<String> {
        let names = self.shared.port_names.lock_unpoisoned();
        names.published.get(index as usize).cloned()
    }

    fn port_names_revision(&self) -> u64 {
        self.shared.port_names_revision.load(Ordering::Acquire)
    }

    fn host_presentation(&self, id: u32) -> Option<moose::params::ParameterPresentation> {
        self.host.presentation(id)
    }

    fn host_presentation_revision(&self) -> u64 {
        self.host.revision()
    }

    /// Support context: call on an export worker, never paint or process.
    pub(crate) fn diagnostic_report(&self) -> serde_json::Value {
        let selection = self.selection.read_unpoisoned().clone();
        self.shared.ensure_parts(selection.parts.len());
        let view = self.shared.view.lock_unpoisoned();
        let parts: Vec<_> = selection
            .parts
            .iter()
            .enumerate()
            .map(|(slot, part)| {
                let v = &view.parts[slot];
                let atoms = self.shared.part(slot).unwrap();
                serde_json::json!({
                    "slot": slot, "path": part.path, "program": part.program, "name": part.name,
                    "port": part.port, "channel": part.channel, "output": part.output,
                    "aux": part.aux, "nodes": part.nodes,
                    "gain": part.gain, "pan": part.pan, "tune": part.tune, "mute": part.mute, "solo": part.solo,
                    "generation": atoms.generation.load(Ordering::Relaxed),
                    "status": v.status, "report": v.report.as_deref(), "load": v.trace.as_deref(),
                    "problems": atoms.problems(),
                })
            })
            .collect();
        drop(view);
        let mut context = serde_json::json!({
            "instance_id": self.shared.instance_id, "build": crate::build_info::BUILD,
            "signal_traces": sampler_core::trace_report::reports(),
            "host": {"sample_rate": self.shared.rate()},
            "rack": {"parts": parts, "buses": selection.buses, "outputs": selection.outputs,
                "midi_thru": selection.midi_thru},
            "unsupported_host_input": self.shared.unsupported_input.load(Ordering::Relaxed),
            "host_note_end_rejections": self.shared.end_rejections.load(Ordering::Relaxed),
            "keyboard": {"heard": self.shared.heard.iter().map(|v| v.load(Ordering::Relaxed)).collect::<Vec<_>>(),
                "played": self.shared.played.iter().map(|v| v.load(Ordering::Relaxed)).collect::<Vec<_>>()},
        });
        context["ui_cpu_phases"] = self.shared.ui_activity.phase_report();
        context["log_flush_error"] =
            serde_json::json!(crate::diagnostics::flush(std::time::Duration::from_secs(2)).err());
        context
    }
}

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

/// Stable per-part atoms shared with the editor/loader. Audio keeps its own
/// prepared Arc vector and never locks the growable registry.
#[derive(Default)]
pub(crate) struct PartShared {
    ui_activity: Option<Arc<ui_activity::Activity>>,
    effect_controls: Mutex<effect_controls::EffectControls>,
    pub(crate) generation: Arc<AtomicU64>,
    snapshot_sequence: Arc<AtomicU64>,
    snapshot_pending: Mutex<std::collections::VecDeque<snapshot_reuse::Pending>>,
    pub(crate) scalar_revision: AtomicU64,
    pub(crate) native_revision: AtomicU64,
    ingress: Mutex<Option<crate::sound::v2::ControlIngress>>,
    /// Out of [`Progress::DONE`], rising within each load.
    pub(crate) load_progress: AtomicU32,
    pub(crate) meter: [AtomicU32; 2],
    pub(crate) clip: AtomicBool,
    /// [`RuntimeProblems`] field by field, as the audio thread last saw them.
    problems: [AtomicU64; 25],
    /// The loaded part's controls; the audio thread refreshes their values.
    pub(crate) controls: Mutex<Arc<[ControlCell]>>,
    engine_meters: Mutex<Vec<EngineMeterCell>>,
    uvi_meters: Mutex<Vec<uvi_meters::Cell>>,
    uvi_meter_interest: AtomicUsize,
    pub(crate) editor_taps: [AtomicU64; 16],
    pub(crate) engine_bindings: Mutex<Arc<[sampler_core::EngineParameterBinding]>>,
    waveforms: Mutex<Option<crate::sound::waveform::Provider>>,
    /// Per node of the loaded part's tree, its level like [`Self::meter`]
    /// (node 0, the instrument, is the part's own meter).
    pub(crate) node_meters: Mutex<Arc<[[AtomicU32; 2]]>>,
    /// The loaded part's script interface models; [`Shared::apply_effects`]
    /// updates them and republishes [`PartView::interfaces`].
    scripts: Mutex<crate::sound::ScriptUi>,
    /// The loaded part's streamed samples, if they stream.
    stream: Mutex<Option<Arc<crate::sound::Stream>>>,
    /// The articulation playing, by index, as the audio thread last saw it;
    /// `u32::MAX` when unknown (none, or a script holds it).
    pub(crate) articulation: AtomicU32,
    /// The part's clock in frames, as the audio thread last saw it.
    clock: AtomicU64,
    /// Bytes of samples the part holds in memory, and would hold fully decoded.
    pub(crate) resident_bytes: AtomicU64,
    keep_resident: AtomicBool,
    pub(crate) full_bytes: AtomicU64,
}

/// One control's value as last seen (`f64` bits).
pub(crate) struct ControlCell {
    pub(crate) id: sampler_ui_ir::ControlId,
    value: AtomicU64,
}

impl ControlCell {
    pub(crate) fn new(id: sampler_ui_ir::ControlId, value: f64) -> Self {
        Self {
            id,
            value: AtomicU64::new(value.to_bits()),
        }
    }
    pub(crate) fn value(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::Relaxed))
    }

    fn set(&self, value: f64) {
        self.value.store(value.to_bits(), Ordering::Relaxed);
    }
}

struct EngineMeterCell {
    address: sampler_core::EngineMeterAddress,
    value: AtomicU32,
}

impl PartShared {
    /// Tree node `node`'s level, silent when the part has no such node.
    pub(crate) fn node_level(&self, node: usize) -> [f32; 2] {
        if node == 0 {
            return Meters::read(&self.meter);
        }
        self.node_meters
            .lock_unpoisoned()
            .get(node)
            .map_or([0.0; 2], Meters::read)
    }

    /// The part's controls and their values, in id order.
    pub(crate) fn control_values(&self) -> Vec<(sampler_ui_ir::ControlId, f64)> {
        let mut values:Vec<_>=self.controls.lock_unpoisoned().iter().map(|c|(c.id,c.value())).collect();
        // UI/save thread only. The audio-side original ControlCell allocation
        // stays fixed; the actual Lua bridge owns dynamic scalar/cell readback.
        if let Some(uvi)=&self.scripts.lock_unpoisoned().uvi {
            values.retain(|(id,_)|(id.0>>64)!=0x5556490000000000);
            values.extend(uvi.values());
            values.sort_by_key(|(id,_)|*id);
        }
        values
    }

    /// Audio thread: copy the core's values in, unless the loader holds the lock.
    pub(crate) fn refresh_controls(&self, value: impl Fn(sampler_ui_ir::ControlId) -> Option<f64>) {
        if let Some(audit) = &self.ui_activity {
            audit.add(UiCount::ReadbackCalls, 1);
        }
        if let Ok(cells) = self.controls.try_lock_unpoisoned() {
            let mut changed = 0;
            for cell in cells.iter() {
                if let Some(v) = value(cell.id) {
                    if cell.value().to_bits() != v.to_bits() {
                        cell.set(v);
                        changed += 1;
                    }
                }
            }
            if let Some(audit) = &self.ui_activity {
                audit.add(UiCount::ReadbackCells, cells.len() as u64);
                audit.add(UiCount::ReadbackChanges, changed);
            }
            if changed != 0 {
                self.scalar_revision.fetch_add(1, Ordering::Release);
            }
        } else if let Some(audit) = &self.ui_activity {
            audit.add(UiCount::ReadbackBusy, 1);
        }
    }

    pub(crate) fn display_values(&self) -> Vec<(sampler_ui_ir::ControlId, f64)> {
        let mut values = self.control_values();
        if let Some(ingress) = self.ingress.lock_unpoisoned().as_mut() {
            ingress.overlay(&mut values);
        }
        values
    }

    pub(crate) fn widget_meters(
        &self,
        face: &sampler_ui_ir::Interface,
        epoch: u64,
    ) -> std::collections::HashMap<sampler_ui_ir::WidgetRef, f64> {
        if let Some(audit) = &self.ui_activity {
            audit.add(UiCount::MeterCalls, 1);
        }
        let mut meters = self.engine_meters.lock_unpoisoned();
        if self.generation.load(Ordering::Acquire) != epoch {
            return Default::default();
        }
        // Only addresses in the current IR face stay registered; GUI prunes them.
        meters.retain(|m| {
            face.widgets.iter().filter_map(|w| w.meter).any(|a| {
                (a.group, a.slot, a.channel, a.bus)
                    == (
                        m.address.group,
                        m.address.slot,
                        m.address.channel,
                        m.address.bus,
                    )
            })
        });
        face.widgets
            .iter()
            .enumerate()
            .filter_map(|(n, widget)| {
                let address = widget.meter?;
                let address = sampler_core::EngineMeterAddress {
                    group: address.group,
                    slot: address.slot,
                    channel: address.channel,
                    bus: address.bus,
                };
                let index = meters
                    .iter()
                    .position(|m| m.address == address)
                    .unwrap_or_else(|| {
                        meters.push(EngineMeterCell {
                            address,
                            value: AtomicU32::new(0),
                        });
                        meters.len() - 1
                    });
                Some((
                    sampler_ui_ir::WidgetRef(n),
                    f64::from(f32::from_bits(meters[index].value.load(Ordering::Relaxed))),
                ))
            })
            .collect()
    }

    fn refresh_widget_meters(
        &self,
        epoch: u64,
        read: impl Fn(sampler_core::EngineMeterAddress) -> Option<f32>,
    ) {
        if let Ok(meters) = self.engine_meters.try_lock_unpoisoned() {
            if self.generation.load(Ordering::Acquire) != epoch {
                return;
            }
            let mut changed = false;
            for meter in meters.iter() {
                let value = read(meter.address).unwrap_or(0.);
                let value = if value.is_finite() { value.max(0.) } else { 0. };
                changed |= meter.value.swap(value.to_bits(), Ordering::Relaxed) != value.to_bits();
            }
            if changed {
                self.scalar_revision.fetch_add(1, Ordering::Release);
            }
        }
    }

    pub(crate) fn zone_waveform(
        &self,
        zone: u32,
        epoch: u64,
        bins: usize,
    ) -> Option<crate::sound::waveform::Envelope> {
        self.zone_waveform_window(zone, epoch, bins, None)
    }
    pub(crate) fn zone_waveform_window(
        &self,
        zone: u32,
        epoch: u64,
        bins: usize,
        range: Option<(u64, u64)>,
    ) -> Option<crate::sound::waveform::Envelope> {
        self.waveform_window(epoch, |provider| provider.get_window(zone, bins, range))
    }
    pub(crate) fn mapping_waveform_window(
        &self,
        region: usize,
        epoch: u64,
        bins: usize,
        range: Option<(u64, u64)>,
    ) -> Option<crate::sound::waveform::Envelope> {
        self.waveform_window(epoch, |provider| provider.get_region_window(region, bins, range))
    }
    fn waveform_window(
        &self,
        epoch: u64,
        read: impl FnOnce(&crate::sound::waveform::Provider) -> Option<crate::sound::waveform::Envelope>,
    ) -> Option<crate::sound::waveform::Envelope> {
        let plan = self
            .ingress
            .lock_unpoisoned()
            .as_ref()
            .map(|ingress| ingress.plan());
        let provider = self.waveforms.lock_unpoisoned();
        if self.generation.load(Ordering::Acquire) != epoch {
            return None;
        }
        read(provider
            .as_ref()
            .filter(|provider| Some(provider.plan) == plan)?)
    }

    pub(crate) fn widget_waveforms(
        &self,
        face: &sampler_ui_ir::Interface,
        epoch: u64,
        pixel_scale: f64,
    ) -> Vec<(sampler_ui_ir::WidgetRef, crate::sound::waveform::Envelope)> {
        let plan = self
            .ingress
            .lock_unpoisoned()
            .as_ref()
            .map(|ingress| ingress.plan());
        let provider = self.waveforms.lock_unpoisoned();
        if self.generation.load(Ordering::Acquire) != epoch {
            return Vec::new();
        }
        let Some(provider) = provider
            .as_ref()
            .filter(|provider| Some(provider.plan) == plan)
        else {
            return Vec::new();
        };
        face.widgets
            .iter()
            .enumerate()
            .filter_map(|(n, widget)| {
                if !face.visible(sampler_ui_ir::WidgetRef(n)) {
                    return None;
                }
                let zone = u32::try_from(widget.waveform.as_ref()?.zone)
                    .ok()?;
                let bins = (f64::from(face.page_rect(sampler_ui_ir::WidgetRef(n)).width)
                    * pixel_scale)
                    .ceil()
                    .clamp(1., 4096.) as usize;
                Some((sampler_ui_ir::WidgetRef(n), provider.get(zone, bins)?))
            })
            .collect()
    }

    pub(crate) fn widget_values(
        &self,
        face: &sampler_ui_ir::Interface,
    ) -> std::collections::HashMap<sampler_ui_ir::WidgetRef, sampler_ui_ir::Value> {
        if let Some(audit) = &self.ui_activity {
            audit.add(UiCount::TypedCalls, 1);
        }
        let mut ingress = self.ingress.lock_unpoisoned();
        let Some(ingress) = ingress.as_mut() else {
            return Default::default();
        };
        if ingress.settle() {
            self.scalar_revision.fetch_add(1, Ordering::Release);
        }
        ingress.values(face)
    }

    pub(crate) fn problems(&self) -> RuntimeProblems {
        let [
            a,
            b,
            c,
            d,
            e,
            f,
            g,
            h,
            silent_notes,
            s0,
            s1,
            s2,
            fault_program,
            fault_error,
            stream_capacity,
            stream_disconnected,
            stream_failed,
            stream_errors,
            offline_failures,
            lua_faults,
            cold_starts,
            voice_pool_refusals,
            waiting_voice_replacements,
            voice_drops,
            loop_limits,
        ] = self.problems.each_ref().map(|x| x.load(Ordering::Relaxed));
        RuntimeProblems {
            capacity_drops: a,
            underruns: b,
            nonfinite: c,
            script_overruns: d,
            narrowed_input: e,
            ignored_input: f,
            stolen_voices: g,
            refused_starts: h,
            silent_notes,
            silent: [s0, s1, s2],
            fault_program,
            fault_error,
            stream_capacity,
            stream_disconnected,
            stream_failed,
            stream_errors,
            offline_failures,
            lua_faults,
            cold_starts,
            voice_pool_refusals,
            waiting_voice_replacements,
            voice_drops,
            loop_limits,
        }
    }

    fn store_problems(&self, p: RuntimeProblems) {
        let values = [
            p.capacity_drops,
            p.underruns,
            p.nonfinite,
            p.script_overruns,
            p.narrowed_input,
            p.ignored_input,
            p.stolen_voices,
            p.refused_starts,
            p.silent_notes,
            p.silent[0],
            p.silent[1],
            p.silent[2],
            p.fault_program,
            p.fault_error,
            p.stream_capacity,
            p.stream_disconnected,
            p.stream_failed,
            p.stream_errors,
            p.offline_failures,
            p.lua_faults,
            p.cold_starts,
            p.voice_pool_refusals,
            p.waiting_voice_replacements,
            p.voice_drops,
            p.loop_limits,
        ];
        for (atom, value) in self.problems.iter().zip(values) {
            atom.store(value, Ordering::Relaxed);
        }
    }
}

/// What the audio thread hands back to the loader to drop.
#[derive(Default)]
#[allow(dead_code, reason = "held only to be dropped off the audio thread")]
struct Retired {
    part: Option<CoreRetired>,
    stale: Option<Box<CorePart>>,
    mix: Option<Mix>,
    growth: Option<Box<Growth>>,
}

/// A prepared part for a slot at a load generation; `None` empties it.
type Ready = (usize, u64, Option<Box<CorePart>>);

pub struct Shared {
    pub(crate) ui_activity: Arc<ui_activity::Activity>,
    instance_id: u64,
    ready: ArrayQueue<Ready>,
    pending_ready: Mutex<std::collections::VecDeque<Ready>>,
    discard: ArrayQueue<Retired>,
    /// Host sample rate (`f64` bits) parts are prepared for.
    pub(crate) editor_watch: AtomicUsize,
    pub(crate) reported: AtomicU32,
    pub(crate) rate: AtomicU64,
    pub(crate) key_owners: [AtomicU64; 128],
    /// The velocity each key sounds at, 0 when silent: `played` on screen
    /// or from the computer keyboard, `heard` from the host's MIDI.
    pub(crate) played: [AtomicU8; 128],
    pub(crate) heard: [AtomicU8; 128],
    pub(crate) learn_target: AtomicU32,
    pub(crate) learned_note: AtomicU64,
    /// What the on-screen keyboard and wheels play, by rack slot.
    pub(crate) keyboard: ArrayQueue<(usize, Play)>,
    /// The pitch wheel (0..=16383, centre 8192) and mod wheel (0..=127) as
    /// last moved, on screen or by incoming MIDI.
    pub(crate) bend: AtomicU32,
    pub(crate) modulation: AtomicU32,
    pub(crate) controls: ArrayQueue<Mix>,
    pub(crate) articulation_edits: ArrayQueue<(usize, usize)>,
    /// Script effects from the audio thread: rack slot, source epoch, script instance, effect.
    effects: ArrayQueue<(usize, u64, usize, sampler_core::Effect)>,
    /// One drainer at a time (loader tick or UI frame) keeps effect order.
    effects_drain: std::sync::Mutex<()>,
    /// Peak meters the audio thread keeps current; read them at paint time.
    pub meters: Meters,
    /// One strip's signal for a spectrum on screen.
    pub(crate) scope: Scope,
    /// Blocks processed: a stopped host stops counting.
    pub(crate) blocks: AtomicU64,
    parts: Mutex<Vec<std::sync::Arc<PartShared>>>,
    initial_parts: [std::sync::Arc<PartShared>; RACK_SLOTS],
    growth: ArrayQueue<Box<Growth>>,
    grown: AtomicU64,
    growth_prepared: AtomicU64,
    pub(crate) audition: AtomicBool,
    pub(crate) audition_note: AtomicU64,
    pub(crate) selected: AtomicU64,
    pub(crate) focus_request: AtomicU64,
    pub(crate) panic: AtomicBool,
    pub(crate) midi_thru: AtomicBool,
    pub(crate) multi_request: Mutex<Option<String>>,
    snapshot_request: Mutex<Option<SnapshotRequest>>,
    /// The app's library folders and the scan of them.
    pub(crate) libraries: library::Scanner,
    /// Dialog and file-operation workers retire at plugin teardown, not GUI close.
    #[cfg(all(feature = "plugin", target_os = "linux"))]
    pub(crate) dialog_runtime: Arc<crate::ui::picker::Runtime>,
    pub(crate) view: Mutex<View>,
    /// Voices sounding across the rack, reported by the audio thread.
    pub(crate) voices: AtomicU64,
    pub(crate) audible: AtomicU64,
    memory_freed: AtomicU64,
    /// Audio thread load (`f32` bits): render time over block time, peak-held.
    pub(crate) cpu: AtomicU64,
    /// Render time and the audio time it rendered, in nanoseconds, summed:
    /// the meter shows their ratio over its window, as Kontakt does.
    pub(crate) busy_ns: AtomicU64,
    pub(crate) span_ns: AtomicU64,
    /// Notes dropped for lack of room, summed over the rack.
    pub(crate) dropouts: AtomicU64,
    /// Host input the core does not model (counted, never silently dropped).
    pub(crate) unsupported_input: AtomicU64,
    /// NOTE_END events the host's output list refused.
    pub(crate) end_rejections: AtomicU64,
    /// Host port names; the loader publishes, the host's main thread reads.
    port_names: Mutex<routing::PortNames>,
    /// Bumped with each publication: format wrappers poll it and tell the host.
    port_names_revision: AtomicU64,
    measure: crate::timing::Measure,
    published: Mutex<(Option<Arc<crate::timing::Plan>>, Option<(f32, Instant)>)>,
    // Last: workers retire before the logging worker.
    _diagnostics: crate::diagnostics::DiagnosticLease,
    // The crash marker retires after every retained worker and diagnostics lease.
    _crash_session: Mutex<Option<crate::support::CrashSessionGuard>>,
}

/// Source, program, snapshot, rate bits, mpe, mpe upper, dynamics, streaming.
pub(crate) type LoadTarget = (String, u32, String, u64, bool, bool, i16, Streaming);

#[derive(Default, Clone)]
pub(crate) struct PartView {
    pub(crate) resource_path: Option<PathBuf>,
    pub(crate) resource_revision: Option<Arc<crate::sound::resource_revision::ResourceRevision>>,
    pub(crate) effects: Arc<[crate::sound::effect_controls::EffectSnapshot]>,
    pub(crate) program: u32,
    pub(crate) generation: u64,
    pub(crate) ui_revision: u64,
    /// The genuine UVI bridge/schema owning the interface drawn below.
    pub(crate) uvi_snapshot: Option<sampler_uvi::scripted::UiSnapshot>,
    pub(crate) updates: Arc<[sampler_ui_ir::InterfacePatch]>,
    /// The source and sample rate (bits) last prepared or being prepared.
    pub(crate) attempted: Option<LoadTarget>,
    /// What the committed instrument was built from. A request equal to it is a
    /// no-op even after a canceled or failed switch; host recall clears it to force a reload.
    pub(crate) active_target: Option<LoadTarget>,
    /// The committed source owning the visible model and live controls. A
    /// failed or pending replacement must never acquire its persistence state.
    pub(crate) active_source: Option<(String, u32, String)>,
    /// Host-recalled state retains its identity until a candidate commits or
    /// the slot is explicitly cleared, including failed/canceled preparation.
    pub(crate) recalled_source: Option<(String, u32, String)>,
    /// Distinguishes successive recalls of the same source during preparation.
    pub(crate) recall_revision: u64,
    /// Numbers each load attempt of the slot; only the latest ticket may commit.
    pub(crate) ticket: u64,
    pub(crate) status: String,
    pub(crate) timing_status: String,
    /// The loaded instrument's name.
    pub(crate) active: String,
    /// Samples are being read for this slot.
    pub(crate) loading: bool,
    /// The instrument's output tree, node 0 the instrument.
    pub(crate) tree: Option<Arc<MixTree>>,
    /// What the load decoded, translated and could not, and runtime problems.
    pub(crate) report: Option<Arc<LoadReport>>,
    /// The instrument's script interfaces, in script order.
    // TODO(v2 UI): drawn by the UI agent's sampler-ui-ir renderer.
    pub(crate) interfaces: Arc<[sampler_ui_ir::Interface]>,
    /// The translated instrument: articulations, mapping, sound.
    pub(crate) instrument: Option<Arc<sampler_ir::Instrument>>,
    /// Host automation slots the loaded plan routes into its controls.
    pub(crate) host_bindings: Arc<[crate::sound::HostBinding]>,
    /// Prepared with the immutable instrument on the load worker.
    pub(crate) mapping_index: Option<Arc<crate::ui::MappingIndex>>,
    /// The keys as the scripts colour and name them (128, or none).
    pub(crate) keys: Arc<[crate::sound::KeyLook]>,
    /// The load's log record ([`crate::diagnostics::LoadTrace`]).
    pub(crate) trace: Option<Arc<serde_json::Value>>,
    pub(crate) fault_inbox: Option<Arc<crate::sound::report::FaultInbox>>,
    pub(crate) runtime_log: Option<crate::diagnostics::grouped::RuntimeLog>,
}

impl PartView {
    pub(crate) fn publish_interface(&mut self, current: &sampler_ui_ir::Interface) -> bool {
        let Some(index) = self
            .interfaces
            .iter()
            .position(|f| f.source == current.source)
        else {
            return false;
        };
        let patch = sampler_ui_ir::InterfacePatch::between(&self.interfaces[index], current);
        if self
            .updates
            .get(index)
            .map_or(patch == Default::default(), |old| *old == patch)
        {
            return false;
        }
        let mut updates = self.updates.to_vec();
        updates.resize_with(self.interfaces.len(), Default::default);
        updates[index] = patch;
        self.updates = updates.into();
        self.ui_revision += 1;
        true
    }
}

#[derive(Clone)]
pub(crate) struct View {
    pub(crate) multi_status: String,
    pub(crate) artwork: HashMap<String, Arc<Image>>,
    /// The libraries found, and which scan found them ([`library::Scanner::wanted`]).
    pub(crate) shelf: Arc<library::Shelf>,
    pub(crate) scanned: u64,
    pub(crate) files: Arc<Vec<PathBuf>>,
    pub(crate) parts: Vec<PartView>,
    pub(crate) status: String,
}

#[cfg(all(feature = "plugin", target_os = "linux"))]
impl Drop for Shared {
    fn drop(&mut self) {
        self.dialog_runtime.shutdown();
    }
}

impl Default for Shared {
    fn default() -> Self {
        let crash_session = crate::support::start_plugin_session();
        let ui_activity = Arc::new(ui_activity::Activity::new(
            std::env::var("KONTRA_NATIVE_UI_TIMING").ok().as_deref() == Some("1"),
        ));
        let initial_parts: [Arc<PartShared>; RACK_SLOTS] = std::array::from_fn(|_| {
            Arc::new(PartShared {
                ui_activity: ui_activity.enabled.then(|| ui_activity.clone()),
                ..Default::default()
            })
        });
        let shared = Self {
            ui_activity,
            _crash_session: Mutex::new(crash_session),
            instance_id: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed),
            ready: ArrayQueue::new(64),
            pending_ready: Mutex::default(),
            discard: ArrayQueue::new(64),
            editor_watch: AtomicUsize::new(usize::MAX),
            reported: AtomicU32::new(0),
            measure: Default::default(),
            published: Mutex::default(),
            rate: AtomicU64::new(48000f64.to_bits()),
            key_owners: std::array::from_fn(|_| AtomicU64::new(0)),
            played: std::array::from_fn(|_| AtomicU8::new(0)),
            heard: std::array::from_fn(|_| AtomicU8::new(0)),
            keyboard: ArrayQueue::new(256),
            bend: AtomicU32::new(8192),
            modulation: AtomicU32::new(0),
            controls: ArrayQueue::new(1),
            articulation_edits: ArrayQueue::new(128),
            learn_target: AtomicU32::new(0),
            learned_note: AtomicU64::new(0),
            effects: ArrayQueue::new(1024),
            effects_drain: std::sync::Mutex::new(()),
            meters: Meters::default(),
            scope: Scope::default(),
            blocks: AtomicU64::new(0),
            parts: Mutex::new(initial_parts.to_vec()),
            initial_parts,
            growth: ArrayQueue::new(1),
            grown: AtomicU64::new(RACK_SLOTS as u64),
            growth_prepared: AtomicU64::new(RACK_SLOTS as u64),
            audition: AtomicBool::new(false),
            audition_note: AtomicU64::new(128),
            selected: AtomicU64::new(0),
            focus_request: AtomicU64::new(u64::MAX),
            panic: AtomicBool::new(false),
            midi_thru: AtomicBool::new(false),
            multi_request: Mutex::new(None),
            snapshot_request: Mutex::new(None),
            libraries: library::Scanner::default(),
            #[cfg(all(feature = "plugin", target_os = "linux"))]
            dialog_runtime: Arc::default(),
            voices: AtomicU64::new(0),
            audible: AtomicU64::new(0),
            memory_freed: AtomicU64::new(0),
            cpu: AtomicU64::new(0),
            busy_ns: AtomicU64::new(0),
            span_ns: AtomicU64::new(0),
            dropouts: AtomicU64::new(0),
            unsupported_input: AtomicU64::new(0),
            end_rejections: AtomicU64::new(0),
            port_names: Mutex::default(),
            port_names_revision: AtomicU64::new(0),
            _diagnostics: crate::diagnostics::acquire(),
            view: Mutex::new(View {
                multi_status: String::new(),
                artwork: Default::default(),
                shelf: Arc::default(),
                scanned: 0,
                files: Arc::default(),
                parts: (0..RACK_SLOTS).map(|_| PartView::default()).collect(),
                status: "Choose a library and select a preset".into(),
            }),
        };
        if let Some(guard) = shared._crash_session.lock_unpoisoned().as_mut() {
            guard.mark_initialized();
        }
        shared
    }
}

/// Samples [`Scope`] keeps: a spectrum's window and then some.
pub(crate) const SCOPE: usize = 8192;
/// [`Scope::source`] for everything sent to the host.
pub(crate) const SCOPE_MASTER: usize = usize::MAX;

/// One strip's post-fader signal, mono, for a spectrum drawn on the UI
/// thread. The audio thread copies into the ring only while
/// [`source`](Self::source) names a strip, which the editor sets while a
/// spectrum shows and clears otherwise. Lock-free: a reader may see a block
/// half-written, which a spectrum cannot tell from the signal.
pub(crate) struct Scope {
    /// 0 for none, a rack slot + 1, or [`SCOPE_MASTER`].
    pub(crate) source: AtomicUsize,
    samples: [AtomicU32; SCOPE],
    /// Samples written so far; the next goes at this, modulo [`SCOPE`].
    written: AtomicUsize,
}

impl Default for Scope {
    fn default() -> Self {
        Self {
            source: AtomicUsize::new(0),
            samples: std::array::from_fn(|_| AtomicU32::new(0)),
            written: AtomicUsize::new(0),
        }
    }
}

impl Scope {
    pub(crate) fn push(&self, x: &[f32]) {
        let mut at = self.written.load(Ordering::Relaxed);
        for &v in x {
            self.samples[at % SCOPE].store(v.to_bits(), Ordering::Relaxed);
            at = at.wrapping_add(1);
        }
        self.written.store(at, Ordering::Release);
    }

    /// The latest `out.len()` samples (at most [`SCOPE`]), oldest first,
    /// and the count written so far (which stops when the host does).
    pub(crate) fn latest(&self, out: &mut [f32]) -> usize {
        let end = self.written.load(Ordering::Acquire);
        let start = end.wrapping_sub(out.len());
        for (i, o) in out.iter_mut().enumerate() {
            *o =
                f32::from_bits(self.samples[start.wrapping_add(i) % SCOPE].load(Ordering::Relaxed));
        }
        end
    }
}

/// Peak meters, `[left, right]` as `f32` bits: absolute sample peaks falling
/// 20 dB a second, 0 once below -80 dB. The audio thread stores each once a
/// block; any number of readers may [`read`](Meters::read) them at paint time.
/// Parts' meters are on [`PartShared`].
#[derive(Default)]
pub struct Meters {
    /// Output buses, after their faders.
    pub buses: [[AtomicU32; 2]; BUSES],
    /// Everything sent to the host, after the Volume parameter.
    pub master: [AtomicU32; 2],
    /// Set when a meter's peak reached 0 dBFS; the editor clears them.
    pub clips: Clips,
}

/// A clip light per meter of [`Meters`].
#[derive(Default)]
pub struct Clips {
    pub buses: [AtomicBool; BUSES],
    pub master: AtomicBool,
}

impl Meters {
    pub fn read(meter: &[AtomicU32; 2]) -> [f32; 2] {
        meter
            .each_ref()
            .map(|m| f32::from_bits(m.load(Ordering::Relaxed)))
    }

    /// Hold `peak` or let the shown level fall by `fall`; light `clip` at 0 dBFS.
    fn publish(meter: &[AtomicU32; 2], peak: [f32; 2], fall: f32, clip: &AtomicBool) {
        if peak[0] >= 1.0 || peak[1] >= 1.0 {
            clip.store(true, Ordering::Relaxed);
        }
        for (m, peak) in meter.iter().zip(peak) {
            let shown = f32::from_bits(m.load(Ordering::Relaxed)) * fall;
            let level = if peak >= shown { peak } else { shown };
            let level = if level < 1e-4 || !level.is_finite() {
                0.
            } else {
                level
            };
            m.store(level.to_bits(), Ordering::Relaxed);
        }
    }
}

fn finite_or(x: f32, range: f32) -> f32 {
    if x.is_finite() {
        x.clamp(-range, range)
    } else {
        0.
    }
}

/// What the audio thread mixes by, from the persisted rack.
pub(crate) fn mix(selection: &Selection) -> Mix {
    let slots = selection.parts.len().max(RACK_SLOTS);
    Mix {
        parts: rack_controls(selection),
        articulation_routes: Vec::new(),
        editor_offsets: selection.parts.iter().map(|p| p.edits.native()).collect(),
        timing: std::sync::Arc::new(timing_plan(selection)),
        buses: std::array::from_fn(|n| {
            let b = selection.bus(n);
            BusControls {
                gain: db_gain(b.gain),
                pan: finite_or(b.pan, 1.),
                mute: b.mute,
                solo: b.solo,
                port: if (0..BUSES as i16).contains(&b.port) {
                    b.port as u8
                } else {
                    n as u8
                },
            }
        }),
        nodes: (0..slots)
            .map(|n| {
                selection
                    .parts
                    .get(n)
                    .map_or_else(Vec::new, |p| p.nodes.clone())
            })
            .collect(),
    }
}

pub(crate) fn rack_controls(selection: &Selection) -> Vec<PartControls> {
    (0..selection.parts.len().max(RACK_SLOTS))
        .map(|n| {
            selection
                .parts
                .get(n)
                .map(|p| PartControls {
                    port: p.port.min(3),
                    output: p.output.min(BUSES as u8 - 1),
                    channel: p.channel.clamp(-1, 15),
                    gain: db_gain(p.gain),
                    pan: finite_or(p.pan, 1.),
                    tune: finite_or(p.tune, TUNE_RANGE),
                    mute: p.mute,
                    solo: p.solo,
                    aux: if (0..BUSES as i16).contains(&p.aux) {
                        p.aux as u8
                    } else {
                        NO_AUX
                    },
                    aux_gain: db_gain(p.aux_gain),
                    mpe: p.mpe,
                    bend_range: p.bend_range.min(96),
                    switching: p.switching,
                })
                .unwrap_or_default()
        })
        .collect()
}

/// The slot the on-screen keyboard plays when no part is selected: every
/// part MIDI channel 1 on port A reaches, as if the host had sent it.
pub(crate) const EVERY_PART: usize = usize::MAX;

/// What the on-screen keyboard and wheels send a part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Play {
    /// A key at a velocity; 0 lets it go.
    Note(u8, u8),
    /// The pitch wheel, 0..=16383.
    Bend(u16),
    /// The mod wheel (CC1).
    Mod(u8),
}

impl Play {
    /// As MIDI 1.0 on channel 1.
    fn event(self) -> CoreEvent {
        match self {
            Self::Note(note, 0) => CoreEvent::midi1(0x80, note, 0),
            Self::Note(note, velocity) => CoreEvent::midi1(0x90, note, velocity),
            Self::Bend(value) => CoreEvent::midi1(0xe0, (value & 127) as u8, (value >> 7) as u8),
            Self::Mod(value) => CoreEvent::midi1(0xb0, 1, value),
        }
    }
}

/// Route parts as the mixer's "Outputs" choice says and publish the host
/// port names once they settle.
fn route(params: &SamplerParams) {
    let shared = &params.shared;
    let trees = shared.trees();
    // Never hold the selection's lock while taking `view` (in `trees`).
    let mut routed = params.selection.read_unpoisoned().clone();
    let before = routed.clone();
    routing::apply(&mut routed, &trees);
    let names = {
        let mut current = params.selection.write_unpoisoned();
        // Changed meanwhile: the next pass routes the new selection.
        if routed != before && *current == before {
            *current = routed;
        }
        routing::port_names(&current, &trees)
    };
    let now = Instant::now();
    let mut ports = shared.port_names.lock_unpoisoned();
    if ports.offer(names, now) {
        shared.port_names_revision.fetch_add(1, Ordering::Release);
    }
    drop(ports);
    let slots = automation::slots(&shared.view.lock_unpoisoned().parts);
    params.host.offer(slots, now);
}

impl Shared {
    /// Queue a snapshot against this exact source. Parsing and mutation stay
    /// on the existing worker; a later source request cancels its result.
    /// `reuse` admits the same-owner fast path, which keeps the previous
    /// preset's runtime, samples and ordinary values. A preset swap passes
    /// `false` so it always equals a fresh load of the base plus snapshot.
    pub(crate) fn queue_snapshot(&self, slot: usize, part: &Part, path: String, reuse: bool) -> bool {
        if !part.snapshot_base() {
            return false;
        }
        let mut request = self.snapshot_request.lock_unpoisoned();
        let Some(atoms) = self.part(slot) else {
            return false;
        };
        let generation = atoms.generation.load(Ordering::Acquire);
        // MAX is a permanent exhausted/cancelled sentinel. Host recall must
        // still be able to invalidate the last admitted sequence (MAX - 1).
        let Ok(previous) = atoms.snapshot_sequence.fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| v.checked_add(1).filter(|next| *next != u64::MAX)) else {
            return false;
        };
        let recall_revision = self.view.lock_unpoisoned().parts.get(slot).map_or(0, |v| v.recall_revision);
        *request = Some(SnapshotRequest {
            slot,
            source: part.source(),
            path,
            generation,
            sequence: previous + 1,
            recall_revision,
            allow_reuse: reuse,
        });
        true
    }

    /// Editor/loader only. Audio reads its prepared registry without this lock.
    pub(crate) fn with_parts<T>(&self, read: impl FnOnce(&[Arc<PartShared>]) -> T) -> T {
        read(&self.parts.lock_unpoisoned())
    }

    pub(crate) fn part(&self, slot: usize) -> Option<Arc<PartShared>> {
        self.parts.lock_unpoisoned().get(slot).cloned()
    }

    /// Prepare metadata for newly appended/restored rows off the audio thread.
    pub(crate) fn ensure_parts(&self, count: usize) {
        let mut parts = self.parts.lock_unpoisoned();
        while parts.len() < count {
            parts.push(Arc::new(PartShared {
                ui_activity: self.ui_activity.enabled.then(|| self.ui_activity.clone()),
                ..Default::default()
            }));
        }
        drop(parts);
        let mut view = self.view.lock_unpoisoned();
        if view.parts.len() < count {
            view.parts.resize_with(count, PartView::default);
        }
    }

    /// Each slot's output tree, as last loaded.
    pub(crate) fn trees(&self) -> Vec<Option<Arc<MixTree>>> {
        let view = self.view.lock_unpoisoned();
        view.parts.iter().map(|v| v.tree.clone()).collect()
    }

    /// [`routing::apply`] with the parts' trees as loaded.
    pub(crate) fn reroute(&self, selection: &mut Selection) {
        routing::apply(selection, &self.trees());
    }

    /// Loader only. Keep different parts when the bounded queue is full; a
    /// newer part for a slot replaces one still pending.
    fn publish_part(&self, item: Ready) {
        let mut pending = self.pending_ready.lock_unpoisoned();
        pending.retain(|(slot, ..)| *slot != item.0);
        pending.push_back(item);
        self.flush_ready_inner(&mut pending);
    }

    fn flush_ready(&self) {
        self.flush_ready_inner(&mut self.pending_ready.lock_unpoisoned());
    }

    fn flush_ready_inner(&self, pending: &mut std::collections::VecDeque<Ready>) {
        while let Some(item) = pending.pop_front() {
            if self
                .part(item.0)
                .is_none_or(|p| p.generation.load(Ordering::Acquire) != item.1)
            {
                continue;
            }
            if let Err(item) = self.ready.push(item) {
                pending.push_front(item);
                break;
            }
        }
    }

    /// Loader only: all larger audio storage is prepared and retired here.
    fn prepare_growth(&self) {
        let count = self.with_parts(|parts| parts.len());
        if count <= self.growth_prepared.load(Ordering::Acquire) as usize {
            return;
        }
        let parts = self.with_parts(|parts| parts.to_vec());
        let count = parts.len();
        let _ = self
            .growth
            .force_push(Box::new(Growth::new(parts, self.rate())));
        self.growth_prepared.store(count as u64, Ordering::Release);
    }

    pub(crate) fn rate(&self) -> f64 {
        f64::from_bits(self.rate.load(Ordering::Acquire))
    }

    /// Ask the loader to replace the rack with the multi at `path`.
    pub(crate) fn queue_multi(&self, path: String) {
        self.view.lock_unpoisoned().multi_status = "Loading multi…".into();
        *self.multi_request.lock_unpoisoned() = Some(path);
    }

    /// Retain even a note pressed and released between editor frames.
    pub(crate) fn record_learn(&self, port: u8, channel: u8, key: u8) {
        let target = self.learn_target.load(Ordering::Relaxed);
        let wanted_channel = (target >> 16) & 31;
        if key < 128
            && channel < 16
            && target >> 31 != 0
            && (target >> 8) as u8 == port
            && (wanted_channel == 0 || wanted_channel == u32::from(channel) + 1)
        {
            let _ = self
                .learned_note
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    Some((n.wrapping_add(256) & !255) | u64::from(key))
                });
        }
    }

    /// Select the source articulation independently of editable input mappings.
    pub(crate) fn select_articulation(&self, slot: usize, identity: usize) {
        let _ = self.articulation_edits.push((slot, identity));
    }

    /// Start a key on a slot; a repeated press releases the previous onset first.
    pub(crate) fn press_key(&self, slot: usize, note: u8, velocity: u8) {
        self.release_key(note);
        let velocity = velocity.clamp(1, 127);
        self.key_owners[note as usize & 127].store(slot as u64, Ordering::Release);
        self.played[note as usize & 127].store(velocity, Ordering::Relaxed);
        if self
            .keyboard
            .push((slot, Play::Note(note, velocity)))
            .is_err()
        {
            self.panic.store(true, Ordering::Release);
        }
    }

    /// Stop `note` on whichever slot the on-screen keyboard started it.
    pub(crate) fn release_key(&self, note: u8) {
        let was_down = self.played[note as usize & 127].swap(0, Ordering::AcqRel) != 0;
        let owner = self.key_owners[note as usize & 127].load(Ordering::Acquire);
        if was_down
            && self
                .keyboard
                .push((owner as usize, Play::Note(note, 0)))
                .is_err()
        {
            self.panic.store(true, Ordering::Release);
        }
    }

    /// Capture only the source actually loaded; a recalled or pending source
    /// must not be overwritten with values from the previous instrument.
    pub(crate) fn capture_ui_controls(&self, selection: &mut Selection) {
        let sources: Vec<_> = self
            .view
            .lock_unpoisoned()
            .parts
            .iter()
            .map(|p| {
                // A same-source host recall has a new state identity even
                // before it acquires an audio epoch. Preserve that candidate's
                // recalled values rather than saving the old live values into it.
                let same_source_recall = p.active_source.as_ref().is_some_and(|active| {
                    p.recalled_source.as_ref() == Some(active)
                });
                if same_source_recall {
                    None
                } else {
                    p.active_source.clone()
                }
            })
            .collect();
        for (slot, part) in selection.parts.iter_mut().enumerate() {
            if sources.get(slot).and_then(Option::as_ref) != Some(&part.source()) {
                continue;
            }
            if let Some(atoms) = self.part(slot) {
                part.control_values = atoms
                    .control_values()
                    .into_iter()
                    .filter(|(_, value)| value.is_finite())
                    .map(|(id, value)| SavedControl::new(id, value))
                    .collect();
                let scripts = atoms.scripts.lock_unpoisoned();
                if scripts.uvi_source.as_ref() == Some(&part.source())
                    && let Some(uvi) = &scripts.uvi
                    && let Ok(state) = uvi.state()
                    && let Ok(state) = serde_json::to_string(&state)
                    && state.len() <= MAX_SAVED_STATE
                {
                    part.uvi_state_source = serde_json::to_string(&part.source()).unwrap();
                    part.uvi_state = state;
                }
                if let Some(state) = atoms
                    .ingress
                    .lock_unpoisoned()
                    .as_ref()
                    .and_then(|client| client.save_script_state())
                    .filter(|state| state.len() <= MAX_SAVED_STATE)
                {
                    part.script_state_source = serde_json::to_string(&part.source()).unwrap();
                    part.script_state = state;
                }
            }
        }
    }

    /// Edit a control of the part in `slot` as its widget would; the
    /// script's `on ui_control` runs on the audio thread. False when the
    /// queue is full.
    #[cfg(test)]
    pub(crate) fn set_control(
        &self,
        slot: usize,
        control: sampler_ui_ir::ControlId,
        value: f64,
    ) -> bool {
        let Some(part) = self.part(slot) else {
            return false;
        };
        self.set_control_at(
            slot,
            part.generation.load(Ordering::Acquire),
            control,
            value,
        )
    }

    pub(crate) fn set_control_at(
        &self,
        slot: usize,
        epoch: u64,
        control: sampler_ui_ir::ControlId,
        value: f64,
    ) -> bool {
        let Some(part) = self.part(slot) else {
            return false;
        };
        if let Some(uvi) = &part.scripts.lock_unpoisoned().uvi {
            if part.generation.load(Ordering::Acquire) != epoch || !value.is_finite() {
                return false;
            }
            return uvi.edit(control, value);
        }
        let mut ingress = part.ingress.lock_unpoisoned();
        if part.generation.load(Ordering::Acquire) != epoch || !value.is_finite() {
            return false;
        }
        ingress
            .as_mut()
            .is_some_and(|client| client.submit(control, value))
    }

    /// A drawn UVI action carries the exact bridge/schema it was drawn from.
    /// Part/source epoch, owner identity and owner-thread current visibility are
    /// separate checks; queuing this action is not a Core Applied receipt.
    pub(crate) fn set_uvi_control_at(
        &self,slot:usize,epoch:u64,snapshot:&sampler_uvi::scripted::UiSnapshot,
        control:sampler_ui_ir::ControlId,value:f64,
    )->bool{
        let Some(part)=self.part(slot)else{return false;};
        let scripts=part.scripts.lock_unpoisoned();
        if part.generation.load(Ordering::Acquire)!=epoch||!value.is_finite(){return false;}
        scripts.uvi.as_ref().is_some_and(|owner|owner.edit_snapshot(snapshot,control,value))
    }

    pub(crate) fn activate_uvi_control_at(
        &self,slot:usize,epoch:u64,snapshot:&sampler_uvi::scripted::UiSnapshot,
        control:sampler_ui_ir::ControlId,activation:sampler_ui_ir::UiActivation,
    )->bool{
        let Some(part)=self.part(slot)else{return false;};
        let scripts=part.scripts.lock_unpoisoned();
        if part.generation.load(Ordering::Acquire)!=epoch||!activation.valid(){return false;}
        scripts.uvi.as_ref().is_some_and(|owner|owner.activate_snapshot(snapshot,control,activation))
    }

    /// Main-thread host automation uses the same epoch admission and reply queue.
    pub(crate) fn set_host_parameter_at(
        &self,
        slot: usize,
        epoch: u64,
        address: u16,
        value: f64,
    ) -> bool {
        let Some(part) = self.part(slot) else {
            return false;
        };
        let mut ingress = part.ingress.lock_unpoisoned();
        if part.generation.load(Ordering::Acquire) != epoch {
            return false;
        }
        ingress
            .as_mut()
            .is_some_and(|client| client.submit_host_parameter(address, value))
    }

    /// One authored gesture; XY axes and touched table cells stay one transaction.
    pub(crate) fn set_widget_batch_at(
        &self,
        slot: usize,
        epoch: u64,
        source_slot: u8,
        widget: &sampler_ui_ir::Widget,
        edits: Vec<(u32, sampler_ui_ir::Value)>,
        interaction: sampler_core::WidgetInteraction,
    ) -> bool {
        let Some(part) = self.part(slot) else {
            return false;
        };
        if part.scripts.lock_unpoisoned().uvi.is_some() {
            return match (&widget.binding, edits.as_slice()) {
                (
                    sampler_ui_ir::Binding::Control(id),
                    [(0, sampler_ui_ir::Value::Integer(value))],
                ) => self.set_control_at(slot, epoch, *id, f64::from(*value)),
                (sampler_ui_ir::Binding::Control(id), [(0, sampler_ui_ir::Value::Real(value))]) => {
                    self.set_control_at(slot, epoch, *id, *value)
                }
                _ => false,
            };
        }
        let mut ingress = part.ingress.lock_unpoisoned();
        if part.generation.load(Ordering::Acquire) != epoch {
            return false;
        }
        ingress.as_mut().is_some_and(|ingress| {
            ingress.submit_ui_widgets(source_slot, widget, edits, interaction)
        })
    }

    pub(crate) fn set_widget_at(
        &self,
        slot: usize,
        epoch: u64,
        source_slot: u8,
        widget: &sampler_ui_ir::Widget,
        index: Option<usize>,
        value: sampler_ui_ir::Value,
    ) -> bool {
        let Some(index) = u32::try_from(index.unwrap_or(0)).ok() else {
            return false;
        };
        let edits = match value {
            sampler_ui_ir::Value::Integers(values) => values
                .into_iter()
                .enumerate()
                .map(|(n, value)| {
                    u32::try_from(n)
                        .ok()
                        .and_then(|n| index.checked_add(n))
                        .map(|n| (n, sampler_ui_ir::Value::Integer(value)))
                })
                .collect::<Option<Vec<_>>>(),
            sampler_ui_ir::Value::Reals(values) => values
                .into_iter()
                .enumerate()
                .map(|(n, value)| {
                    u32::try_from(n)
                        .ok()
                        .and_then(|n| index.checked_add(n))
                        .map(|n| (n, sampler_ui_ir::Value::Real(value)))
                })
                .collect::<Option<Vec<_>>>(),
            value => Some(vec![(index, value)]),
        };
        edits.is_some_and(|edits| {
            self.set_widget_batch_at(slot, epoch, source_slot, widget, edits, Default::default())
        })
    }

    /// Apply the script effects the audio thread queued to their parts'
    /// interface models, and publish the interfaces that changed.
    pub(crate) fn apply_effects(&self) {
        let Ok(_drain) = self.effects_drain.try_lock() else {
            return;
        };
        let mut changed =
            std::collections::BTreeMap::<(usize, u64), std::collections::BTreeSet<usize>>::new();
        // Bound one service pass even when audio keeps producing. FIFO work
        // arriving during this batch remains queued for the next pass.
        for _ in 0..self.effects.len() {
            let Some((slot, epoch, instance, effect)) = self.effects.pop() else { break };
            let Some(part) = self.part(slot) else {
                continue;
            };
            if part.generation.load(Ordering::Acquire) != epoch {
                continue;
            }
            if effect.service == sampler_core::MIDI_SERVICE
                || effect.service == sampler_core::ARRAY_FILE_SERVICE
            {
                if let Some(ingress) = part.ingress.lock_unpoisoned().as_mut() {
                    ingress.service_midi(&effect);
                }
                continue;
            }
            let mut scripts = part.scripts.lock_unpoisoned();
            let key_only = scripts
                .views
                .get(instance)
                .and_then(|v| v.service(effect.service))
                .is_some_and(|s| s.starts_with("set_key_"));
            if scripts.apply(instance, &effect) {
                let instances = changed.entry((slot, epoch)).or_default();
                if !key_only {
                    instances.insert(instance);
                }
            }
        }
        self.with_parts(|parts| {
            for part in parts {
                if let Some(ingress) = part.ingress.lock_unpoisoned().as_mut() {
                    ingress.settle();
                }
            }
        });
        for ((slot, epoch), instances) in changed {
            let Some(part) = self.part(slot) else {
                continue;
            };
            if part.generation.load(Ordering::Acquire) != epoch {
                continue;
            }
            let (interfaces, keys) = {
                let mut scripts = part.scripts.lock_unpoisoned();
                (
                    instances
                        .into_iter()
                        .filter_map(|i| scripts.interface(i))
                        .collect::<Vec<_>>(),
                    scripts.keys(),
                )
            };
            if let Some(v) = self
                .view
                .lock_unpoisoned()
                .parts
                .get_mut(slot)
                .filter(|v| v.generation == epoch)
            {
                for interface in interfaces {
                    self.ui_activity.add(UiCount::PublicationCalls, 1);
                    let changed = v.publish_interface(&interface);
                    self.ui_activity.add(UiCount::PublicationChanges, u64::from(changed));
                }
                if v.keys != keys {
                    v.keys = keys;
                    v.ui_revision += 1;
                }
            }
        }
    }

    fn refresh_uvi(&self, params: &SamplerParams) {
        let parts = self.parts.lock_unpoisoned().clone();
        for (slot, part) in parts.iter().enumerate() {
            // Commit holds `view` and then takes `scripts`; take them in that
            // order only. `scripts` is released before `view` is requested, and the
            // generation read under it fences a commit that lands in between.
            let (uvi, generation) = {
                let mut scripts = part.scripts.lock_unpoisoned();
                let Some(uvi) = scripts.uvi.clone() else {
                    continue;
                };
                if scripts.uvi_source
                    != params
                        .selection
                        .read_unpoisoned()
                        .parts
                        .get(slot)
                        .map(Part::source)
                {
                    continue;
                }
                let revision = uvi.revision();
                if revision == scripts.uvi_revision {
                    continue;
                }
                scripts.uvi_revision = revision;
                (uvi, part.generation.load(Ordering::Acquire))
            };
            if let Some(view) = self
                .view
                .lock_unpoisoned()
                .parts
                .get_mut(slot)
                .filter(|view| view.generation == generation)
            {
                self.ui_activity.add(UiCount::PublicationCalls, 1);
                let snapshot=uvi.snapshot();
                let changed=view.publish_interface(snapshot.interface());
                view.uvi_snapshot=Some(snapshot);
                self.ui_activity.add(UiCount::PublicationChanges, u64::from(changed));
            }
        }
    }

    /// Fit streamed start data in `budget_mb`, shared evenly by the parts that
    /// stream, dropping what has idled longest; refresh what each holds.
    pub(crate) fn memory_snapshot(&self) -> (u64, u64) {
        let resident = self
            .parts
            .lock_unpoisoned()
            .iter()
            .map(|p| p.resident_bytes.load(Ordering::Relaxed))
            .sum();
        (resident, self.memory_freed.load(Ordering::Relaxed))
    }

    fn trim_streams(&self, budget_mb: u32) {
        let idle = (IDLE_SECONDS * self.rate()) as u64;
        let parts = self.parts.lock_unpoisoned().clone();
        let streams: Vec<_> = parts
            .iter()
            .filter_map(|p| Some((p, p.stream.lock_unpoisoned().clone()?)))
            .collect();
        let share = u64::from(budget_mb) * (1 << 20) / streams.len().max(1) as u64;
        for (part, stream) in streams {
            if budget_mb > 0 && !part.keep_resident.load(Ordering::Relaxed) {
                let freed = stream.trim(
                    share,
                    part.clock.load(Ordering::Relaxed).saturating_sub(idle),
                );
                self.memory_freed.fetch_add(freed as u64, Ordering::Relaxed);
            }
            part.resident_bytes
                .store(stream.resident_bytes(), Ordering::Relaxed);
        }
    }

    fn reset_midi(&self) {
        while self.keyboard.pop().is_some() {}
        for owner in &self.key_owners {
            owner.store(0, Ordering::Release);
        }
        for lit in self.played.iter().chain(&self.heard) {
            lit.store(0, Ordering::Relaxed);
        }
        self.bend.store(8192, Ordering::Relaxed);
        self.modulation.store(0, Ordering::Relaxed);
    }

    /// Move the pitch wheel (0..=16383) for `slot`, as the keys play it.
    pub(crate) fn bend(&self, slot: usize, value: u16) {
        self.bend.store(u32::from(value), Ordering::Relaxed);
        let _ = self.keyboard.push((slot, Play::Bend(value)));
    }

    /// Move the mod wheel (CC1, 0..=127) for `slot`, as the keys play it.
    pub(crate) fn modulate(&self, slot: usize, value: u8) {
        self.modulation.store(u32::from(value), Ordering::Relaxed);
        let _ = self.keyboard.push((slot, Play::Mod(value)));
    }

    /// Stop every note the on-screen keyboard holds.
    pub(crate) fn release_keyboard(&self) {
        for note in 0..128 {
            self.release_key(note);
        }
    }

    /// Play `note` (middle C when none) on the selected part for a moment.
    pub(crate) fn audition(&self, note: Option<u8>) {
        if let Some(note) = note {
            self.audition_note.store(u64::from(note), Ordering::Relaxed);
        }
        self.audition.store(true, Ordering::Release);
    }
}

/// A rack KONTRA saved: a `.kontra-multi` file, JSON, naming the
/// instruments with every part setting:
///
/// ```json
/// { "format": "kontra-multi", "version": 2, "name": "Evening",
///   "parts": [ { "path": "/…/Piano.nki", "program": 0, "channel": -1,
///                "port": 0, "output": 0, "gain": 0.0, "nodes": [] } ] }
/// ```
///
/// Parts are in rack order; a missing field takes its default. Version 1 is
/// KONTRA v1's format, which this engine does not read.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SavedMulti {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub parts: Vec<Part>,
}

impl SavedMulti {
    /// The rack in `selection`, in its order.
    pub fn of(name: &str, selection: &Selection) -> Self {
        Self {
            format: library::MULTI.into(),
            version: 2,
            name: name.into(),
            parts: (selection.order.iter())
                .filter_map(|n| selection.parts.get(*n as usize))
                .filter(|p| !p.path.is_empty())
                .cloned()
                .collect(),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn read(path: &Path) -> anyhow::Result<Self> {
        let multi: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        anyhow::ensure!(multi.format == library::MULTI, "Not a KONTRA multi");
        anyhow::ensure!(
            multi.version >= 2,
            "Saved by KONTRA v1, which this version cannot open"
        );
        anyhow::ensure!(multi.version <= 2, "Saved by a newer KONTRA");
        Ok(multi)
    }
}

// Port from v1 0cb7a8a0:src/plugin.rs; prepared v2 part stays off audio until validation.
#[derive(Clone)]
pub(crate) struct SnapshotRequest {
    slot: usize,
    source: (String, u32, String),
    path: String,
    generation: u64,
    sequence: u64,
    recall_revision: u64,
    allow_reuse: bool,
}

/// Validate on the loader before replacing any saved or playing state.
fn prepare_snapshot(
    params: &SamplerParams,
) -> Option<(
    usize,
    (String, u32, String),
    crate::sound::Loaded<Option<Box<CorePart>>>,
)> {
    let request = {
        let mut pending = params.shared.snapshot_request.lock_unpoisoned();
        if pending.as_ref()?.slot >= params.shared.grown.load(Ordering::Acquire) as usize {
            return None;
        }
        // At most two admitted payloads: the current receipt and one newer
        // request. Further gestures replace the one lightweight request above;
        // an inactive host cannot accumulate unbounded saved-state buffers.
        if params.shared.part(pending.as_ref()?.slot)
            .is_some_and(|part| part.snapshot_pending.lock_unpoisoned().len() >= 2) {
            return None;
        }
        pending.take()?
    };
    let mut trace = crate::diagnostics::LoadTrace::new(
        Path::new(&request.source.0),
        request.source.1,
        Some(request.slot),
    );
    trace.detail("instance_id", params.shared.instance_id);
    trace.detail("operation", "snapshot_validation");
    trace.detail("snapshot", request.path.clone());
    trace.detail(
        "scope",
        "container parsing, explicit base matching and supported state import; audio not installed",
    );
    trace.stage("snapshot_parse_and_validation");
    let current = || snapshot_reuse::current(params, &request, true);
    if !current() {
        trace.detail(
            "cancellation",
            "Source changed or a newer snapshot was requested",
        );
        trace.finish("canceled");
        return None;
    }
    let (request, mut trace) = match snapshot_reuse::try_submit(params, request, trace) {
        Ok(()) => return None,
        Err(ordinary) => ordinary,
    };
    let current = || snapshot_reuse::current(params, &request, true);
    params.shared.view.lock_unpoisoned().parts[request.slot].status = "Loading preset…".into();
    let part = params.selection.read_unpoisoned().parts[request.slot].clone();
    let rack_streaming = params.selection.read_unpoisoned().streaming;
    let load = LoadRequest {
        path: part.path.clone().into(),
        program: part.program,
        sample_rate: params.shared.rate(),
        snapshot: (!request.path.is_empty()).then(|| request.path.clone().into()),
        control_values: Vec::new(),
        uvi_state: None,
        mpe: part.mpe,
        mpe_upper: part.mpe_upper,
        streaming: part.streaming(rack_streaming),
        dynamics_start: u8::try_from(part.dynamics).ok().filter(|v| *v < 128),
        threads: match params.shared.libraries.settings().threads {
            library::ThreadSetting::Single => None,
            library::ThreadSetting::Auto => Some(crate::sound::ThreadChoice::Auto),
            library::ThreadSetting::Fixed(n) => Some(crate::sound::ThreadChoice::Fixed(n.into())),
        },
    };
    let result = V2Loader.prepare_serviced(
        &load, &mut |_| {}, &|| !current(), &mut || control_tick(params),
    );
    if !current() {
        trace.detail(
            "cancellation",
            "Source changed or a newer snapshot was requested during validation",
        );
        trace.finish("canceled");
        return None;
    }
    match result {
        Ok(instrument) => {
            let mut selection = params.selection.write_unpoisoned();
            let pending = params.shared.snapshot_request.lock_unpoisoned();
            let valid = pending.is_none()
                && params.shared.part(request.slot).unwrap().snapshot_sequence.load(Ordering::Acquire) == request.sequence
                && params
                    .shared
                    .part(request.slot)
                    .unwrap()
                    .generation
                    .load(Ordering::Acquire)
                    == request.generation;
            let Some(part) = selection
                .parts
                .get_mut(request.slot)
                .filter(|p| valid && snapshot_reuse::source_current(params, &request, &p.source()))
            else {
                drop(pending);
                drop(selection);
                trace.detail(
                    "cancellation",
                    "Source changed or a newer snapshot was requested before commit",
                );
                trace.finish("canceled");
                return None;
            };
            part.select_snapshot(request.path);
            let source = part.source();
            drop(pending);
            drop(selection);
            trace.finish("validated");
            Some((request.slot, source, instrument))
        }
        Err(error) => {
            let selection = params.selection.read_unpoisoned();
            let pending = params.shared.snapshot_request.lock_unpoisoned();
            let valid = params.shared.part(request.slot).unwrap().snapshot_sequence.load(Ordering::Acquire) == request.sequence
                && params
                .shared
                .part(request.slot)
                .unwrap()
                .generation
                .load(Ordering::Acquire)
                == request.generation
                && selection
                    .parts
                    .get(request.slot)
                    .is_some_and(|p| snapshot_reuse::source_current(params, &request, &p.source()))
                && pending.is_none();
            if !valid {
                drop(pending);
                drop(selection);
                trace.detail(
                    "cancellation",
                    "Source changed or a newer snapshot was requested before reporting failure",
                );
                trace.finish("canceled");
                return None;
            }
            let message = format!("{error:#}");
            // Linearize rejection against source changes and newer requests.
            // LoadTrace only enqueues its journal record; it does no file IO.
            trace.fail(&message);
            let report = trace.finish("failed");
            let mut view = params.shared.view.lock_unpoisoned();
            view.parts[request.slot].status = format!("Preset was not loaded: {message}");
            view.parts[request.slot].trace = Some(report);
            None
        }
    }
}

/// The loader: serialized, polled by the audio thread about ten times a second.
pub struct Load;

/// Services of the already committed owners; never begins another load or
/// acquires candidate state. Also called while waiting on private preparation.
fn service_current_ui(params: &SamplerParams) {
    let shared = &params.shared;
    let _effects = shared.ui_activity.span(ui_activity::Phase::WorkerScriptEffects);
    shared.apply_effects();
    shared.with_parts(|parts| {
        for part in parts {
            if let Some(ingress) = part.ingress.lock_unpoisoned().as_mut()
                && ingress.refresh()
            {
                part.scalar_revision.fetch_add(1, Ordering::Release);
            }
        }
    });
}

/// The mixer, routing, scans, discard drain and UI handoffs. None of it needs the
/// loader, so it also runs from the load wait: a mute or fader move made while a
/// preset prepares reaches the audio thread within one wait tick.
fn control_tick(params: &SamplerParams) {
    let shared = &params.shared;
    shared.flush_ready();
    service_current_ui(params);
    // UVI publication retains its existing source/lock policy.
    shared.refresh_uvi(params);
    shared.trim_streams(params.selection.read_unpoisoned().memory_budget_mb);
    // Dropping what the audio thread replaced is this thread's job.
    while shared.discard.pop().is_some() {}
    shared
        .midi_thru
        .store(params.selection.read_unpoisoned().midi_thru, Ordering::Release);
    poll_libraries(shared);
    route(params);
    align(params);
    push_mix(params);
    refresh_problems(shared);
}

fn push_mix(params: &SamplerParams) {
    let shared = &params.shared;
    let selection = params.selection.read_unpoisoned();
    let mut mix = mix(&selection);
    if let Some(plan) = &shared.published.lock_unpoisoned().0 {
        mix.timing = plan.clone();
    }
    let view = shared.view.lock_unpoisoned();
    mix.articulation_routes = selection
        .parts
        .iter()
        .enumerate()
        .map(|(slot, p)| {
            let instrument = view.parts.get(slot)?.instrument.as_deref()?;
            let (keys, switching) = p
                .articulation_overlay
                .routing(instrument, p.switching)
                .ok()?;
            Some(Arc::new(crate::sound::articulation::Routing {
                keys,
                switching,
            }))
        })
        .collect();
    let _ = shared.controls.force_push(mix);
}

impl BackgroundTask for Load {
    type Params = SamplerParams;
    const SERIALIZED: bool = true;
    fn run(self, params: &SamplerParams) {
        let shared = &params.shared;
        shared.ui_activity.add(UiCount::WorkerTicks, 1);
        shared.flush_ready();
        service_current_ui(params);
        snapshot_reuse::poll(params);
        load_multi(params);
        let selection = params.selection.read_unpoisoned().clone();
        shared.ensure_parts(selection.parts.len());
        shared.prepare_growth();
        control_tick(params);
        let mut snapshot = prepare_snapshot(params);
        let mut loaded = false;
        for slot in 0..shared.grown.load(Ordering::Acquire) as usize {
            let prepared = if snapshot.as_ref().is_some_and(|(s, source, _)| {
                *s == slot && params.selection.read_unpoisoned().parts[slot].source() == *source
            }) {
                snapshot.take().map(|(_, _, loaded)| loaded)
            } else {
                None
            };
            loaded |= load_part(params, slot, prepared);
        }
        if loaded {
            align(params);
            // New trees: route their nodes and size their settings.
            route(params);
            push_mix(params);
        }
        refresh_problems(shared);
    }
}

/// Replace the rack with a requested multi.
fn load_multi(params: &SamplerParams) {
    let shared = &params.shared;
    let Some(path) = shared.multi_request.lock_unpoisoned().take() else {
        return;
    };
    let before = params.selection.read_unpoisoned().clone();
    let result = SavedMulti::read(Path::new(&path)).map(|m| {
        let status = format!("{} · {} instruments", m.name, m.parts.len());
        (m.parts, status)
    });
    let status = match result {
        Ok((parts, status)) => {
            let mut current = params.selection.write_unpoisoned();
            if *current == before {
                current.order = (0..parts.len() as u32).collect();
                current.parts = parts;
                current.multi = path;
                shared.focus_request.store(0, Ordering::Release);
                status
            } else {
                "Multi load canceled because the rack changed".into()
            }
        }
        Err(e) => format!("Multi load failed: {e:#}"),
    };
    shared.view.lock_unpoisoned().multi_status = status;
}

/// A finished library scan replaces what the browser lists.
fn poll_libraries(shared: &Shared) {
    let installed = shared.view.lock_unpoisoned().scanned;
    let Some((generation, scanned)) = shared.libraries.poll(installed) else {
        return;
    };
    let mut view = shared.view.lock_unpoisoned();
    view.scanned = generation;
    match scanned {
        Some(scanned) => {
            view.status = format!(
                "{} libraries · {} presets",
                scanned.shelf.libraries.len(),
                scanned.files.len()
            );
            if let Some(imported) = &scanned.imported {
                view.status += &match imported.len() {
                    0 => " · no new installed libraries".to_owned(),
                    n => format!(" · {n} installed library folders"),
                };
            }
            view.shelf = scanned.shelf;
            view.files = scanned.files;
            view.artwork = scanned.artwork;
        }
        None => view.status = "Library scan canceled".into(),
    }
}

/// Prepare `slot`'s part when its source or the sample rate changed and hand
/// it to the audio thread. Returns whether a load finished.
fn load_part(
    params: &SamplerParams,
    slot: usize,
    prepared: Option<crate::sound::Loaded<Option<Box<CorePart>>>>,
) -> bool {
    load_part_with(params, slot, prepared, |request, progress, canceled| {
        V2Loader.prepare_serviced(
            request, progress, canceled, &mut || control_tick(params),
        )
    })
}

/// Leaves a slot retryable and honestly labelled when a load unwinds.
struct LoadGuard<'a> {
    shared: &'a Shared,
    slot: usize,
}

impl Drop for LoadGuard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking()
            && let Some(v) = self.shared.view.lock_unpoisoned().parts.get_mut(self.slot)
        {
            v.loading = false;
            v.attempted = None;
            v.status = "Load failed: internal error".into();
        }
    }
}

fn load_part_with(
    params: &SamplerParams,
    slot: usize,
    prepared: Option<crate::sound::Loaded<Option<Box<CorePart>>>>,
    mut prepare: impl FnMut(
        &LoadRequest,
        &mut dyn FnMut(Progress),
        &(dyn Fn() -> bool + Sync),
    ) -> Result<crate::sound::Loaded<Option<Box<CorePart>>>, CoreError>,
) -> bool {
    let shared = &params.shared;
    let atoms = shared.part(slot).unwrap();
    let (part, rack_streaming) = {
        let selection = params.selection.read_unpoisoned();
        (
            selection.parts.get(slot).cloned().unwrap_or_default(),
            selection.streaming,
        )
    };
    let streaming = part.streaming(rack_streaming);
    let rate = shared.rate();
    let target = (
        part.path.clone(),
        part.program,
        part.snapshot.clone(),
        rate.to_bits(),
        part.mpe,
        part.mpe_upper,
        part.dynamics,
        streaming,
    );
    let (previous_status, recall_revision, ticket) = {
        let mut view = shared.view.lock_unpoisoned();
        let v = &mut view.parts[slot];
        if v.attempted.as_ref() == Some(&target) {
            return false;
        }
        if v.active_target.as_ref() == Some(&target) {
            // Back to what is still playing: nothing to rebuild, and a failed
            // candidate may be picked again.
            v.attempted = Some(target);
            return false;
        }
        let status = v.status.clone();
        // The current instrument remains playable and editable while its
        // replacement is prepared privately. A failed candidate owns no epoch.
        v.attempted = Some(target);
        v.loading = !part.path.is_empty();
        v.status = "Loading…".into();
        v.ticket += 1;
        (status, v.recall_revision, v.ticket)
    };
    // A panic below must not leave the slot at "Loading…" with a dedup key that blocks a retry.
    let _contain = LoadGuard { shared, slot };
    atoms.load_progress.store(0, Ordering::Relaxed);
    let active_generation = atoms.generation.load(Ordering::Acquire);
    if part.path.is_empty() {
        let generation = {
            let mut ingress = atoms.ingress.lock_unpoisoned();
            let epoch = atoms.generation.fetch_add(1, Ordering::AcqRel) + 1;
            *ingress = None;
            atoms.engine_meters.lock_unpoisoned().clear();
            atoms.uvi_meters.lock_unpoisoned().clear();
            atoms.uvi_meter_interest.store(0, Ordering::Release);
            *atoms.waveforms.lock_unpoisoned() = None;
            epoch
        };
        // Everything the replaced instrument left in the cells goes; its threads
        // are joined here, before `view` is taken.
        let retired = (
            std::mem::take(&mut *atoms.stream.lock_unpoisoned()),
            std::mem::take(&mut *atoms.scripts.lock_unpoisoned()),
            std::mem::take(&mut *atoms.controls.lock_unpoisoned()),
            std::mem::take(&mut *atoms.engine_bindings.lock_unpoisoned()),
            std::mem::take(&mut *atoms.node_meters.lock_unpoisoned()),
            std::mem::take(&mut *atoms.effect_controls.lock_unpoisoned()),
            std::mem::take(&mut *atoms.snapshot_pending.lock_unpoisoned()),
        );
        drop(retired);
        atoms.resident_bytes.store(0, Ordering::Relaxed);
        atoms.full_bytes.store(0, Ordering::Relaxed);
        let mut view = shared.view.lock_unpoisoned();
        let attempted = view.parts[slot].attempted.take();
        view.parts[slot] = PartView {
            program: part.program,
            generation,
            attempted,
            recall_revision,
            ..Default::default()
        };
        drop(view);
        shared.publish_part((slot, generation, None));
        return true;
    }
    let source = part.source();
    let canceled = || {
        ({
            let selection = params.selection.read_unpoisoned();
            selection.parts.get(slot).is_none_or(|p| {
                p.source() != source
                    || p.streaming(selection.streaming) != streaming
                    || p.mpe != part.mpe
                    || p.mpe_upper != part.mpe_upper
                    || p.dynamics != part.dynamics
            })
        }) || shared.view.lock_unpoisoned().parts[slot].recall_revision != recall_revision
            || atoms.generation.load(Ordering::Acquire) != active_generation
            || shared.rate.load(Ordering::Acquire) != rate.to_bits()
    };
    let mut trace =
        crate::diagnostics::LoadTrace::new(Path::new(&part.path), part.program, Some(slot));
    trace.detail("instance_id", shared.instance_id);
    trace.detail("sample_rate", rate);
    trace.stage("prepare");
    // A bad saved blob must not make the preset unloadable: load defaults and say so.
    let uvi_state = if part.uvi_state.is_empty()
        || part.uvi_state_source != serde_json::to_string(&source).unwrap()
    {
        None
    } else if part.uvi_state.len() > MAX_SAVED_STATE {
        trace.detail("state_recovery", "Saved UVI UI state exceeds 8 MiB, using defaults");
        None
    } else {
        serde_json::from_str(&part.uvi_state)
            .map_err(|_| trace.detail("state_recovery", "Saved UVI UI state is malformed, using defaults"))
            .ok()
    };
    let request = LoadRequest {
        uvi_state,
        path: part.path.clone().into(),
        program: part.program,
        sample_rate: rate,
        mpe: part.mpe,
        mpe_upper: part.mpe_upper,
        streaming,
        snapshot: (!part.snapshot.is_empty()).then(|| part.snapshot.clone().into()),
        dynamics_start: u8::try_from(part.dynamics).ok().filter(|v| *v < 128),
        control_values: part
            .control_values
            .iter()
            .map(|c| (c.id(), c.value))
            .collect(),
        threads: match shared.libraries.settings().threads {
            crate::library::ThreadSetting::Single => None,
            crate::library::ThreadSetting::Auto => Some(crate::sound::ThreadChoice::Auto),
            crate::library::ThreadSetting::Fixed(n) => {
                Some(crate::sound::ThreadChoice::Fixed(n.into()))
            }
        },
    };
    // Persistence and the commit follow the loader's DONE: the bar holds at
    // 99% until the part is published.
    let mut progress = |p: Progress| {
        let held = u32::from(p.0).min(u32::from(Progress::DONE.0) - 10);
        atoms.load_progress.store(held, Ordering::Relaxed)
    };
    let mut prepared = prepared;
    let mut load = |progress: &mut dyn FnMut(Progress)| match prepared.take() {
        Some(loaded) if !canceled() => Ok(loaded),
        Some(_) => Err(CoreError::Canceled),
        None => prepare(&request, progress, &canceled),
    };
    let saved_script = if part.script_state_source == serde_json::to_string(&source).unwrap() {
        part.script_state.as_str()
    } else {
        ""
    };
    let finish = |mut loaded: crate::sound::Loaded<Option<Box<CorePart>>>, saved: &str| {
        {
            let persistence = sampler_kontakt::audit::Span::new("persistence_prepare");
            if let Some(runtime) = loaded.part.as_mut() {
                runtime.prepare_persistence(&loaded.scripts.views, saved)?;
                for (id, value) in &mut loaded.controls {
                    if let Some(current) = runtime.persistent_control_value(*id) {
                        *value = current;
                    }
                }
            }
            drop(persistence);
            let _span = sampler_kontakt::audit::Span::new("mapping_index");
            let mapping_index = loaded.instrument.as_ref()
                .map(|instrument| Arc::new(crate::ui::MappingIndex::new(instrument)));
            let resource_revision = loaded.scripts.resource_path.as_deref()
                .zip(loaded.scripts.resources.as_ref())
                .and_then(|(owner, resources)| {
                    crate::sound::resource_revision::ResourceRevision::capture(
                        owner, part.program, resources,
                    ).map(Arc::new)
                });
            if canceled() {
                return Err(CoreError::Canceled);
            }
            Ok::<_, CoreError>((loaded, mapping_index, resource_revision))
        }
    };
    let mut result = load(&mut progress).and_then(|loaded| finish(loaded, saved_script));
    if !saved_script.is_empty()
        && let Err(e) = &result
        && !matches!(e, CoreError::Canceled)
    {
        // The restore may have half-applied; rebuild a pristine part with defaults.
        trace.detail("state_recovery", format!("Saved script state rejected ({e}), using defaults"));
        result = load(&mut progress).and_then(|loaded| finish(loaded, ""));
    }
    let result = if canceled() {
        Err(CoreError::Canceled)
    } else {
        result
    };
    let _commit = sampler_kontakt::audit::Span::new("commit_view_locked");
    let mut view = shared.view.lock_unpoisoned();
    // A newer committed owner must never have its model changed by this job.
    if atoms.generation.load(Ordering::Acquire) != active_generation {
        return false;
    }
    let v = &mut view.parts[slot];
    v.loading = false;
    if v.recall_revision != recall_revision || v.ticket != ticket {
        v.status = previous_status;
        return false;
    }
    match result {
        Ok((mut loaded, mapping_index, resource_revision)) => {
            // Commit ownership only after preparation and persistence succeed.
            // Serialize the fence with off-audio command producers.
            let generation = {
                let mut ingress = atoms.ingress.lock_unpoisoned();
                let epoch = atoms.generation.fetch_add(1, Ordering::AcqRel) + 1;
                *ingress = None;
                atoms.engine_meters.lock_unpoisoned().clear();
                atoms.uvi_meters.lock_unpoisoned().clear();
                atoms.uvi_meter_interest.store(0, Ordering::Release);
                *atoms.waveforms.lock_unpoisoned() = None;
                epoch
            };
            atoms
                .keep_resident
                .store(streaming == Streaming::RamOnly, Ordering::Relaxed);
            let attempted = v.attempted.take();
            *v = PartView {
                program: part.program,
                active_target: attempted.clone(),
                attempted,
                active_source: Some(source.clone()),
                recall_revision,
                ..Default::default()
            };
            if loaded.scripts.uvi.is_some() {
                loaded.scripts.uvi_source = Some(source.clone());
            }
            for missing in &loaded.report.missing {
                trace.missing(missing, loaded.instrument.as_deref());
            }
            if let Some(instrument) = loaded.instrument.as_deref() {
                // ponytail: compiles each KSP script a second time for its statuses; reuse the load's compile if load time shows it.
                trace.ksp_symbols(&sampler_kontakt::parity_summaries(instrument));
            }
            if let Some(startup) = &loaded.report.stream_startup {
                trace.detail("stream_startup", serde_json::to_value(startup).unwrap_or_default());
                if let Some(warning) = startup.warning() {
                    trace.issue("prepare", "attack_preload_budget", warning);
                }
            }
            v.fault_inbox = loaded.part.as_ref().map(|p| p.fault_inbox.clone());
            let mut runtime_log = trace.runtime_log();
            if runtime_log.lua(&loaded.report.uvi_faults) {
                trace.detail("runtime_diagnostics", runtime_log.summary());
            }
            v.runtime_log = Some(runtime_log);
            let d = &loaded.report.decoded;
            trace.detail("zones", d.zones);
            trace.detail("groups", d.groups);
            trace.detail("samples", d.samples);
            trace.detail("scripts", d.scripts);
            let full = d.full_bytes;
            let missing = loaded.report.missing.len();
            v.status = format!("{} · {} zones · {} groups", d.format, d.zones, d.groups);
            if missing > 0 {
                v.status += &format!(" · {missing} not translated");
            }
            v.active = loaded.report.name.clone();
            v.tree = Some(Arc::new(loaded.tree));
            v.report = Some(Arc::new(loaded.report));
            v.generation = generation;
            v.interfaces = loaded.interfaces.into();
            if let Some(uvi)=&loaded.scripts.uvi {
                let snapshot=uvi.snapshot();
                v.publish_interface(snapshot.interface());
                v.uvi_snapshot=Some(snapshot);
            }
            v.resource_path = loaded.scripts.resource_path.clone();
            v.resource_revision = resource_revision;
            if let Some(part) = loaded.part.as_mut() {
                part.epoch = generation;
                v.effects =
                    atoms.publish_effect_controls(generation, part, loaded.instrument.as_deref());
                *atoms.engine_bindings.lock_unpoisoned() = part.engine_bindings.clone();
                v.host_bindings = part.host_bindings.clone();
                if !part.waveform_sources.is_empty()
                    && let Some(ingress) = &part.ui_controls
                {
                    let wake = Arc::downgrade(&atoms);
                    *atoms.waveforms.lock_unpoisoned() = crate::sound::waveform::Provider::start(
                        ingress.plan(),
                        std::mem::take(&mut part.waveform_sources),
                        move || {
                            if let Some(atoms) = wake.upgrade()
                                && atoms.generation.load(Ordering::Acquire) == generation
                            {
                                atoms.scalar_revision.fetch_add(1, Ordering::Release);
                            }
                        },
                    )
                    .ok();
                }
                *atoms.ingress.lock_unpoisoned() = part.ui_controls.take();
            } else {
                *atoms.effect_controls.lock_unpoisoned() =
                    effect_controls::EffectControls::default();
                *atoms.engine_bindings.lock_unpoisoned() = Arc::from(Vec::new());
            }
            let nodes = v.tree.as_ref().map_or(1, |t| t.nodes.len());
            *atoms.node_meters.lock_unpoisoned() = (0..nodes).map(|_| Default::default()).collect();
            *atoms.controls.lock_unpoisoned() = loaded
                .controls
                .iter()
                .map(|&(id, value)| ControlCell::new(id, value))
                .collect();
            v.instrument = loaded.instrument;
            v.mapping_index = mapping_index;
            v.keys = loaded.scripts.keys();
            // Dropping the old script thread / streamer joins threads: do it after `view` is released.
            let old_scripts = std::mem::replace(&mut *atoms.scripts.lock_unpoisoned(), loaded.scripts);
            let held = loaded.stream.as_ref().map(|s| s.resident_bytes());
            atoms
                .resident_bytes
                .store(held.unwrap_or(full), Ordering::Relaxed);
            atoms.full_bytes.store(full, Ordering::Relaxed);
            let old_stream = std::mem::replace(&mut *atoms.stream.lock_unpoisoned(), loaded.stream);
            v.trace = Some(trace.finish(if missing > 0 { "partial" } else { "loaded" }));
            atoms
                .load_progress
                .store(u32::from(Progress::DONE.0), Ordering::Relaxed);
            drop(view);
            drop((old_scripts, old_stream));
            shared.publish_part((slot, generation, loaded.part));
            true
        }
        Err(CoreError::Canceled) => {
            // Load again on the next pass, whatever the part is then.
            v.attempted = None;
            v.status = previous_status;
            false
        }
        Err(e) => {
            v.status = format!("Load failed: {e}");
            trace.fail(e.to_string());
            v.trace = Some(trace.finish("failed"));
            true
        }
    }
}

/// Copy the audio thread's problem counters into the parts' load reports.
fn refresh_problems(shared: &Shared) {
    let problems = shared.with_parts(|parts| {
        parts
            .iter()
            .map(|p| {
                // The fault's callback resolves to text here, off the audio thread.
                let faults: Vec<sampler_core::ScriptFault> =
                    match p.problems().fault_program.checked_sub(1) {
                        Some(program) => {
                            let rt = p.problems();
                            vec![sampler_core::ScriptFault {
                                callback: sampler_ksp::callback_of(
                                    &p.scripts.lock_unpoisoned().views,
                                    program as usize,
                                ),
                                error: format!(
                                    "{:?}",
                                    sampler_core::Error::ALL[(rt.fault_error as usize).min(11)]
                                ),
                            }]
                        }
                        None => Vec::new(),
                    };
                let lua = p
                    .scripts
                    .lock_unpoisoned()
                    .uvi
                    .as_ref()
                    .map(|ui| ui.fault_counts());
                (p.problems(), faults, lua)
            })
            .collect::<Vec<_>>()
    });
    let mut view = shared.view.lock_unpoisoned();
    for (v, (problems, faults, lua)) in view.parts.iter_mut().zip(problems) {
        if let Some(log) = v.runtime_log.as_mut() {
            let mut changed = log.counters(problems);
            if let Some(inbox) = &v.fault_inbox {
                while let Some((callback, outcome, context)) = inbox.queue.pop() {
                    log.fault_context(callback, outcome, context.as_ref(), v.instrument.as_deref());
                    changed = true;
                }
                let dropped = inbox.take_dropped();
                if dropped > 0 {
                    log.lost(dropped);
                    changed = true;
                }
            }
            if let Some(lua) = &lua {
                changed |= log.lua(lua);
            }
            if changed && let Some(trace) = &mut v.trace {
                Arc::make_mut(trace)["runtime_diagnostics"] = log.summary();
            }
        }
        if let Some(report) = v.report.as_mut().filter(|r| r.runtime != problems) {
            let report = Arc::make_mut(report);
            report.runtime = problems;
            report.faults = faults.iter().map(ToString::to_string).collect();
            report.why_silent = (problems.silent_notes > 0)
                .then(|| sampler_core::SilentNote::unpack(problems.silent).message(&faults));
        }
    }
}

/// Exact host note input, or why not.
enum ExactInput {
    Routed(CoreEvent, u8),
    Unsupported,
}

fn host_pattern(address: ExactNoteAddress, clap: bool) -> Option<HostPattern> {
    if matches!(address.port, ExactAddress::InvalidRaw(_))
        || matches!(address.channel, ExactAddress::InvalidRaw(_))
        || matches!(address.key, ExactAddress::InvalidRaw(_))
        || matches!(address.note_id, ExactAddress::InvalidRaw(_))
    {
        return None;
    }
    Some(HostPattern {
        port: address.port.raw_i32(),
        channel: address.channel.raw_i32(),
        key: address.key.raw_i32(),
        id: address.note_id.raw_i32(),
        clap,
    })
}

/// Host actions retain their original tuple; this hint cannot represent a
/// wildcard and is not an admission filter for an already captured owner.
fn host_action_port(pattern: HostPattern) -> u8 {
    u8::try_from(pattern.port).unwrap_or(0)
}

/// A CLAP or VST3 note event as core input; `None` leaves it to its typed fallback.
fn exact_host_input(exact: ExactEventRef<'_>) -> Option<ExactInput> {
    let note = |kind, address: ExactNoteAddress, velocity: f64, clap, tune: f64| {
        let Some(pattern) = host_pattern(address, clap) else {
            return ExactInput::Unsupported;
        };
        match kind {
            ExactNoteKind::On => {
                let (Ok(port), Ok(channel), Ok(key)) = (
                    u8::try_from(pattern.port),
                    u8::try_from(pattern.channel),
                    u8::try_from(pattern.key),
                ) else {
                    return ExactInput::Unsupported;
                };
                if channel >= 16
                    || key >= 128
                    || !(0.0..=1.0).contains(&velocity)
                    || !tune.is_finite()
                {
                    return ExactInput::Unsupported;
                }
                let note = HostNote {
                    port,
                    channel,
                    key,
                    id: pattern.id,
                    clap,
                };
                ExactInput::Routed(
                    CoreEvent::NoteOn {
                        note,
                        velocity,
                        tune,
                    },
                    port,
                )
            }
            // Concrete ports remain useful route hints; wildcards are resolved
            // against captured native host tuples by the core, never port zero.
            ExactNoteKind::Off if (0.0..=1.0).contains(&velocity) =>
                ExactInput::Routed(CoreEvent::NoteOffVelocity(pattern, velocity), host_action_port(pattern)),
            ExactNoteKind::Choke => ExactInput::Routed(CoreEvent::Choke(pattern), host_action_port(pattern)),
            _ => ExactInput::Unsupported,
        }
    };
    let expression =
        |address, clap, x: Option<NoteExpression>| match (host_pattern(address, clap), x) {
            (Some(pattern), Some(x)) => ExactInput::Routed(CoreEvent::Expression(pattern, x), host_action_port(pattern)),
            _ => ExactInput::Unsupported,
        };
    let unit = |v: f64| (0.0..=1.0).contains(&v).then_some(v);
    Some(match *exact.body() {
        ExactEventBody::Ump { port, packet } => {
            let Ok(port)=u8::try_from(port) else { return Some(ExactInput::Unsupported); };
            if port>=<Plugin as moose::core::PluginRuntime>::info().midi_input_ports { return Some(ExactInput::Unsupported); }
            let words=packet.storage();
            let expected=match words[0]>>28 {1|2=>1,4=>2,_=>return Some(ExactInput::Unsupported)};
            if packet.word_count()!=expected { return Some(ExactInput::Unsupported); }
            ExactInput::Routed(CoreEvent::Ump([words[0],if expected==2 {words[1]} else {0}]),port)
        }
        ExactEventBody::Midi1 { port, message } => {
            let Ok(port)=u8::try_from(port) else { return Some(ExactInput::Unsupported); };
            if port>=<Plugin as moose::core::PluginRuntime>::info().midi_input_ports { return Some(ExactInput::Unsupported); }
            let bytes=message.bytes();let status=bytes[0];
            let count=match status {0x80..=0xbf|0xe0..=0xef|0xf2=>3,0xc0..=0xdf|0xf1|0xf3=>2,
                0xf6|0xf8|0xfa..=0xfc|0xfe|0xff=>1,_=>return Some(ExactInput::Unsupported)};
            // CLAP carries a fixed three-byte storage object even when this
            // status has only one or two semantic bytes. Trailing storage is
            // neither another message nor an additional MIDI data byte.
            if bytes.len()<count || bytes[1..count].iter().any(|b|*b>=128) {return Some(ExactInput::Unsupported);}
            let a=if count>1 {u32::from(bytes[1])} else {0};let b=if count>2 {u32::from(bytes[2])} else {0};
            let kind=if status<0xf0 {2} else {1};
            ExactInput::Routed(CoreEvent::Ump([kind<<28|u32::from(status)<<16|a<<8|b,0]),port)
        }
        ExactEventBody::Note {
            kind,
            address,
            velocity,
        } => note(kind, address, velocity, true, 0.),
        // VST3 tuning is cents. Length is metadata: an explicit NoteOff ends the note.
        ExactEventBody::DetailedNote {
            kind,
            address,
            velocity,
            tuning,
            ..
        } => note(
            kind,
            address,
            f64::from(velocity),
            false,
            f64::from(tuning) / 100.,
        ),
        // CLAP: 0 volume (linear 0..=4), 1 pan (0..=1), 2 tuning (semitones),
        // 5 brightness, 6 pressure.
        ExactEventBody::NoteExpression {
            expression_id,
            address,
            value,
        } => expression(
            address,
            true,
            match expression_id {
                0 if (0.0..=4.0).contains(&value) => Some(NoteExpression::Gain(value)),
                1 => unit(value).map(|v| NoteExpression::Pan(v * 2. - 1.)),
                2 if (-120.0..=120.0).contains(&value) => Some(NoteExpression::Tune(value)),
                5 => unit(value).map(NoteExpression::Brightness),
                6 => unit(value).map(NoteExpression::Pressure),
                _ => None,
            },
        ),
        // VST3, normalized: 0 volume (0.25 is unity, 1 is +12 dB), 1 pan,
        // 2 tuning (0.5 centre, ±120 semitones), 5 brightness.
        ExactEventBody::NormalizedNoteExpression {
            expression_id,
            address,
            value,
        } => expression(
            address,
            false,
            unit(value).and_then(|v| match expression_id {
                0 => Some(NoteExpression::Gain(v * 4.)),
                1 => Some(NoteExpression::Pan(v * 2. - 1.)),
                2 => Some(NoteExpression::Tune((v - 0.5) * 240.)),
                5 => Some(NoteExpression::Brightness(v)),
                _ => None,
            }),
        ),
        // Anonymous VST3 poly pressure has a faithful per-key typed fallback.
        ExactEventBody::DetailedPolyPressure { .. } if exact.fallback().is_some() => return None,
        ExactEventBody::DetailedPolyPressure { address, pressure } => expression(
            address,
            false,
            unit(f64::from(pressure)).map(NoteExpression::Pressure),
        ),
        _ => return None,
    })
}

fn input_offset(event: &LosslessEventRef<'_>) -> u32 {
    match event {
        LosslessEventRef::Typed(e) => e.sample_offset,
        LosslessEventRef::Exact(e) => e.sample_offset(),
    }
}

fn relay_typed_input(e: &Event, cx: &mut ProcessContext, thru: bool) {
    if thru
        && matches!(
            e.body,
            EventBody::NoteOn { .. }
                | EventBody::NoteOff { .. }
                | EventBody::PitchBend { .. }
                | EventBody::ControlChange { .. }
        )
    {
        let mut out = *e;
        out.port = 0;
        cx.output_events.push(out);
    }
}

/// Semantic-only UI observation; it never admits a note or invokes a script.
/// Exact packets use it for their bounded typed companions, then route the
/// original packet once rather than re-encoding each semantic companion.
fn observe_typed_input(s:&mut Dsp,p:&SamplerParams,e:&Event) {
    let shared = &p.shared;
    s.held_keys.typed(e);
    s.held_keys.publish(shared);
    match e.body {
        EventBody::NoteOn {
            channel,
            note,
            ..
        } => {
            shared.record_learn(e.port, channel, note);
        }
        EventBody::NoteOn2 {
            channel,
            note,
            ..
        } => {
            shared.record_learn(e.port, channel, note);
        }
        EventBody::ControlChange { cc: 1, value, .. } => {
            shared.modulation.store(u32::from(value), Ordering::Relaxed)
        }
        EventBody::PitchBend { value, .. } => {
            shared.bend.store(u32::from(value), Ordering::Relaxed)
        }
        _ => {}
    }
}

/// The same host start/stop law at block start and timestamped transitions.
/// Stop preserves ordinary release callbacks and tails; panic remains a hard cut.
fn host_playing_transition(s: &mut Dsp, shared: &Shared, playing: bool, panic: bool) {
    let stopped = s.playing && !playing;
    s.playing = playing;
    if panic {
        shared.reset_midi();
        s.held_keys.clear();
        s.core.panic();
        s.audition.fill((0, 0));
    } else if stopped {
        shared.reset_midi();
        s.held_keys.clear();
        s.core.release_all_notes();
        s.audition.fill((0, 0));
    }
}

fn core_transport(host: &TransportInfo) -> Transport {
    Transport { playing: host.playing, tempo: host.tempo, beats: host.position_beats,
        signature: (host.time_sig_num, host.time_sig_den) }
}

/// A typed host MIDI event: shown on the keyboard and wheels, played as UMP.
fn feed_typed_input(
    s: &mut Dsp,
    p: &SamplerParams,
    e: &Event,
    cx: &mut ProcessContext,
    thru: bool,
 ) {
    if let EventBody::Transport(host) = &e.body {
        // The process loop has already rendered to e.sample_offset. A host
        // snapshot is not a MIDI packet and must never enter channel encoding.
        s.core.update_transport(core_transport(host));
        host_playing_transition(s, &p.shared, host.playing, false);
        return;
    }
    if let EventBody::ParamChange { id, value } = e.body {
        if let Some(address) = automation::HostAutomation::address(id) {
            if !s.core.host_parameter(address, value) {
                s.unsupported += 1;
            }
        } else if [p.attack.id(), p.release.id(), p.cutoff.id()].contains(&id) {
            p.set_plain(id, value);
            s.core.set_performance(
                p.attack.raw_target(),
                p.release.raw_target(),
                p.cutoff.raw_target(),
            );
        }
        return;
    }
    relay_typed_input(e, cx, thru);
    observe_typed_input(s,p,e);
    let words = moose::core::ump::encode_ump_channel_voice_1(&e.body)
        .or_else(|| moose::core::ump::encode_ump_channel_voice_2(&e.body));
    match words {
        Some([a, b, ..]) => s.core.event(e.port, CoreEvent::Ump([a, b])),
        None => s.unsupported += 1,
    }
}

/// An exact host note event into the core; a refused or unrouted note-on
/// ends at once, so the host never waits on it.
fn feed_exact_input(
    s: &mut Dsp,
    p: &SamplerParams,
    event: CoreEvent,
    port: u8,
    offset: u32,
    cx: &mut ProcessContext,
) {
    s.held_keys.exact(event);
    s.held_keys.publish(&p.shared);
    s.core.event(port, event);
    match event {
        CoreEvent::NoteOn { note, .. } => {
            p.shared.record_learn(port, note.channel, note.key);
            if note.clap && !s.core.owns(note) && !end_host_note(cx, note, offset) {
                s.end_rejections += 1;
            }
        }
        _ => {}
    }
}

/// Return `note`'s exact identity to the host. False if the host's output list refused it.
fn end_host_note(cx: &mut ProcessContext, note: HostNote, offset: u32) -> bool {
    !note.clap
        || cx
            .output_events
            .try_push_exact(ExactEvent::new(
                offset,
                ExactEventBody::Note {
                    kind: ExactNoteKind::End,
                    address: ExactNoteAddress::from_raw_signed(
                        i16::from(note.port),
                        i16::from(note.channel),
                        i16::from(note.key),
                        note.id,
                    ),
                    velocity: 0.,
                },
            ))
            .is_ok()
}

pub struct Dsp {
    core: V2Core,
    /// Physical host presses, independent of scripts, sustain and voice lifetime.
    held_keys: held_keys::HeldKeys,
    /// The mix last applied, re-applied to each installed part.
    mix: Mix,
    until_poll: usize,
    /// Per slot, frames until the audition note ends, and the note.
    audition: Vec<(usize, u8)>,
    /// Recent load: rises with any block's, falls over some 50 blocks.
    load: f32,
    shared_parts: Vec<Arc<PartShared>>,
    unsupported: u64,
    end_rejections: u64,
    playing: bool,
    master_gain: MasterGain,
    #[cfg(test)]
    callback_audit: Option<CallbackAudit>,
}

/// Diagnostic clock groups exist only in test builds. Publishing/serialization
/// happens in the receipt fixture after process() has returned.
#[cfg(test)]
#[derive(Default)]
struct CallbackAudit {
    wall_ns: [u64; 10],
    caller_cpu_ns: [u64; 10],
    calls: [u64; 10],
}

#[cfg(test)]
struct CallbackAuditStamp {
    wall: Instant,
    caller_cpu_ns: u64,
}

#[cfg(all(test, target_os = "linux"))]
fn callback_thread_nanos() -> u64 {
    let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: the test-only clock call receives a valid writable timespec.
    assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) }, 0);
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}

#[cfg(all(test, not(target_os = "linux")))]
fn callback_thread_nanos() -> u64 { 0 }

#[cfg(test)]
fn callback_audit_mark(
    audit: &mut Option<CallbackAudit>,
    stamp: &mut Option<CallbackAuditStamp>,
    group: usize,
) {
    if let (Some(audit), Some(previous)) = (audit.as_mut(), stamp.as_mut()) {
        let now = CallbackAuditStamp { wall: Instant::now(), caller_cpu_ns: callback_thread_nanos() };
        audit.wall_ns[group] = audit.wall_ns[group].saturating_add(
            now.wall.duration_since(previous.wall).as_nanos().min(u128::from(u64::MAX)) as u64,
        );
        audit.caller_cpu_ns[group] = audit.caller_cpu_ns[group]
            .saturating_add(now.caller_cpu_ns.saturating_sub(previous.caller_cpu_ns));
        audit.calls[group] = audit.calls[group].saturating_add(1);
        *previous = now;
    }
}

/// Keep sample-accurate smoothing while reusing the transform of an identical
/// dB value. The host event loop splits blocks at automation boundaries.
#[derive(Default)]
struct MasterGain {
    previous: Option<(u32, f32)>,
}

impl MasterGain {
    fn fill(&mut self, volume: &FloatParam, output: &mut [f32]) {
        volume.read_into(output);
        let Some(first) = output.first().copied() else { return };
        let bits = first.to_bits();
        if output.iter().all(|sample| sample.to_bits() == bits) {
            let linear = match self.previous {
                Some((previous, linear)) if previous == bits => linear,
                _ => {
                    let linear = db_to_linear(first);
                    self.previous = Some((bits, linear));
                    linear
                }
            };
            output.fill(linear);
        } else {
            let last = output.last().unwrap().to_bits();
            for sample in output.iter_mut() { *sample = db_to_linear(*sample); }
            self.previous = Some((last, *output.last().unwrap()));
        }
    }
}

impl Default for Dsp {
    fn default() -> Self {
        Self {
            core: V2Core::default(),
            held_keys: held_keys::HeldKeys::default(),
            mix: Mix::default(),
            until_poll: 0,
            audition: vec![(0, 0); RACK_SLOTS],
            load: 0.0,
            shared_parts: Vec::new(),
            unsupported: 0,
            end_rejections: 0,
            playing: false,
            master_gain: MasterGain::default(),
            #[cfg(test)]
            callback_audit: None,
        }
    }
}

/// Storage for a larger rack, made on the loader. Adoption swaps the playing
/// parts in; the emptied storage returns to the loader to be dropped.
struct Growth {
    core: V2Core,
    audition: Vec<(usize, u8)>,
    shared_parts: Vec<Arc<PartShared>>,
}

impl Growth {
    fn new(parts: Vec<Arc<PartShared>>, rate: f64) -> Self {
        let count = parts.len();
        Self {
            core: V2Core::with_parts(count, rate),
            audition: vec![(0, 0); count],
            shared_parts: parts,
        }
    }

    fn adopt(&mut self, dsp: &mut Dsp) {
        dsp.core.adopt(&mut self.core);
        self.audition[..dsp.audition.len()].copy_from_slice(&dsp.audition);
        std::mem::swap(&mut dsp.audition, &mut self.audition);
        std::mem::swap(&mut dsp.shared_parts, &mut self.shared_parts);
    }
}

fn part_atoms<'a>(
    parts: &'a [Arc<PartShared>],
    shared: &'a Shared,
    slot: usize,
) -> Option<&'a PartShared> {
    parts
        .get(slot)
        .or_else(|| shared.initial_parts.get(slot))
        .map(Arc::as_ref)
}

pub struct Sampler;

impl PluginLogic for Sampler {
    type Params = SamplerParams;
    type DspState = Dsp;

    fn bus_layouts() -> Vec<BusLayout> {
        const NAMES: [&str; BUSES] = [
            "st.1", "st.2", "st.3", "st.4", "st.5", "st.6", "st.7", "st.8", "st.9", "st.10",
            "st.11", "st.12", "st.13", "st.14", "st.15", "st.16",
        ];
        vec![NAMES.into_iter().fold(BusLayout::new(), |l, name| {
            l.with_output(name, ChannelConfig::Stereo)
        })]
    }

    fn reset(s: &mut Dsp, p: &SamplerParams, c: &AudioConfig) {
        s.core.reset(c.sample_rate);
        // Parts are prepared at a sample rate: the loader reloads them at this one.
        p.shared
            .rate
            .store(c.sample_rate.to_bits(), Ordering::Release);
        s.until_poll = 0;
        s.audition.fill((0, 0));
        s.playing = false;
        p.shared.reset_midi();
        s.held_keys.clear();
        p.shared.voices.store(0, Ordering::Relaxed);
        p.shared.audible.store(0, Ordering::Relaxed);
    }

    fn process(
        s: &mut Dsp,
        p: &SamplerParams,
        b: &mut AudioBuffer,
        events: &EventList,
        cx: &mut ProcessContext,
    ) -> ProcessStatus {
        let started = Instant::now();
        #[cfg(test)]
        let mut callback_stamp = s.callback_audit.as_ref().map(|_| CallbackAuditStamp {
            wall: started,
            caller_cpu_ns: callback_thread_nanos(),
        });
        let shared = &p.shared;
        if !shared.discard.is_full()
            && let Some(mut growth) = shared.growth.pop()
        {
            if growth.core.parts() > s.core.parts() {
                growth.adopt(s);
            }
            shared.grown.store(s.core.parts() as u64, Ordering::Release);
            let _ = shared.discard.push(Retired {
                growth: Some(growth),
                ..Default::default()
            });
            if let Some(tasks) = cx.tasks::<Load>() {
                tasks.spawn_coalescing(Load);
            }
        }
        let rate = s.core.sample_rate();
        let frames = b.num_samples();
        if s.until_poll <= frames {
            if let Some(tasks) = cx.tasks::<Load>() {
                tasks.spawn_coalescing(Load);
            }
            for slot in 0..s.core.parts() {
                let atoms = part_atoms(&s.shared_parts, shared, slot).unwrap();
                atoms.store_problems(s.core.problems(slot));
                atoms.clock.store(s.core.clock(slot), Ordering::Relaxed);
                let playing = s.core.articulation(slot).map_or(u32::MAX, |a| a as u32);
                atoms.articulation.store(playing, Ordering::Relaxed);
                if s.core.epoch(slot) == atoms.generation.load(Ordering::Acquire) {
                    atoms.refresh_controls(|id| s.core.control_value(slot, id));
                    atoms.refresh_effect_controls(|address| s.core.effect_value(slot, address));
                    atoms.refresh_widget_meters(s.core.epoch(slot), |address| {
                        s.core.widget_meter(slot, address)
                    });
                }
            }
            s.until_poll = (rate * 0.1) as usize;
        } else {
            s.until_poll -= frames;
        }
        if !shared.discard.is_full()
            && let Some(mut mix) = shared.controls.pop()
        {
            s.core.set_mix(&mix);
            std::mem::swap(&mut s.mix, &mut mix);
            let _ = shared.discard.push(Retired {
                mix: Some(mix),
                ..Default::default()
            });
        }
        // Replaced parts go back to the loader to be dropped; stop while it cannot take more.
        while !shared.discard.is_full() {
            let Some((slot, generation, part)) = shared.ready.pop() else {
                break;
            };
            let current = part_atoms(&s.shared_parts, shared, slot)
                .is_some_and(|atoms| atoms.generation.load(Ordering::Acquire) == generation);
            let retired = if current && slot < s.core.parts() {
                let old = s.core.install(slot, part);
                s.core.set_mix(&s.mix);
                Retired {
                    part: Some(old),
                    ..Default::default()
                }
            } else {
                Retired {
                    stale: part,
                    ..Default::default()
                }
            };
            let _ = shared.discard.push(retired);
        }
        while !shared.discard.is_full()
            && let Some(old) = s.core.take_expired()
        {
            let _ = shared.discard.push(Retired {
                part: Some(old),
                ..Default::default()
            });
        }
        #[cfg(test)]
        callback_audit_mark(&mut s.callback_audit, &mut callback_stamp, 0);
        // Port v1 per-part engine defaults before any keyboard or host note starts.
        s.core.set_performance(
            p.attack.raw_target(),
            p.release.raw_target(),
            p.cutoff.raw_target(),
        );
        s.core.begin_block(&BlockInfo {
            frames,
            offline: cx.process_mode.is_offline(),
            transport: core_transport(cx.transport),
        });
        host_playing_transition(s, shared, cx.transport.playing,
            shared.panic.swap(false, Ordering::AcqRel));
        while let Some((slot, articulation)) = shared.articulation_edits.pop() {
            s.core.select_articulation(slot, articulation);
        }
        while let Some((slot, play)) = shared.keyboard.pop() {
            if slot == EVERY_PART {
                // As host MIDI on port A, channel 1.
                s.core.event(0, play.event());
            } else {
                s.core.play(slot, play.event());
            }
        }
        let selected = shared.selected.load(Ordering::Relaxed) as usize;
        if shared.audition.swap(false, Ordering::AcqRel) && selected < s.core.parts() {
            let requested = shared.audition_note.swap(128, Ordering::Relaxed);
            let note = if requested < 128 { requested as u8 } else { 60 };
            let (left, playing) = s.audition[selected];
            if left > 0 {
                s.core.play(selected, CoreEvent::midi1(0x80, playing, 0));
            }
            s.core.play(selected, CoreEvent::midi1(0x90, note, 100));
            s.audition[selected] = ((rate * 1.5) as usize, note);
        }

        #[cfg(test)]
        callback_audit_mark(&mut s.callback_audit, &mut callback_stamp, 1);
        let channels = b.num_output_channels();
        let thru = shared.midi_thru.load(Ordering::Relaxed);
        let mut peak = [0f32; 2];
        let mut gains = [0f32; MAX_BLOCK];
        let scope = shared.scope.source.load(Ordering::Relaxed);
        let mut at = 0;
        let mut incoming = events.lossless_iter().peekable();
        loop {
            while incoming
                .peek()
                .is_some_and(|e| at >= frames || input_offset(e) as usize <= at)
            {
                match incoming.next().unwrap() {
                    LosslessEventRef::Typed(e) => feed_typed_input(s, p, e, cx, thru),
                    LosslessEventRef::Exact(exact) => match exact_host_input(exact) {
                        Some(ExactInput::Routed(event, port)) => {
                            for e in exact.fallback().into_iter().chain(exact.companions()) {
                                relay_typed_input(e, cx, thru);
                                if matches!(event,CoreEvent::Ump(_)) { observe_typed_input(s,p,e); }
                            }
                            feed_exact_input(s, p, event, port, exact.sample_offset(), cx);
                        }
                        Some(ExactInput::Unsupported) => s.unsupported += 1,
                        None => {
                            for e in exact.fallback().into_iter().chain(exact.companions()) {
                                feed_typed_input(s, p, e, cx, thru);
                            }
                        }
                    },
                }
            }
            #[cfg(test)]
            callback_audit_mark(&mut s.callback_audit, &mut callback_stamp, 2);
            if at >= frames {
                break;
            }
            let due = incoming
                .peek()
                .map_or(frames, |e| (input_offset(e) as usize).min(frames));
            let len = (due - at).min(MAX_BLOCK);
            for (part, (left, note)) in s.audition.iter_mut().enumerate().take(s.core.parts()) {
                if *left > 0 {
                    *left = left.saturating_sub(len);
                    if *left == 0 {
                        s.core.play(part, CoreEvent::midi1(0x80, *note, 0));
                    }
                }
            }
            s.master_gain.fill(&p.volume, &mut gains[..len]);
            #[cfg(test)]
            callback_audit_mark(&mut s.callback_audit, &mut callback_stamp, 3);
            let ports = s.core.bus_ports().map(usize::from);
            s.core.set_tap(scope.checked_sub(1));
            let Rendered { buses, live } = s.core.render(len);
            #[cfg(test)]
            callback_audit_mark(&mut s.callback_audit, &mut callback_stamp, 4);
            if scope == SCOPE_MASTER {
                let mut mono = [0f32; MAX_BLOCK];
                for (_, x) in buses.iter().enumerate().filter(|(bus, _)| live[*bus]) {
                    for (i, (m, gain)) in mono[..len].iter_mut().zip(&gains[..len]).enumerate() {
                        *m += (x[0][i] + x[1][i]) * 0.5 * gain;
                    }
                }
                shared.scope.push(&mono[..len]);
            }
            for channel in 0..channels {
                b.output(channel)[at..at + len].fill(0.0);
            }
            for (bus, x) in buses.iter().enumerate().filter(|(bus, _)| live[*bus]) {
                let port = ports[bus];
                let route = cx
                    .bus_routing
                    .output(port)
                    .map(|r| (r.channel_start(), r.channel_count()));
                let (start, count) = route.unwrap_or(if port == 0 {
                    (0, channels.min(2))
                } else {
                    (0, 0)
                });
                for channel in (0..count.min(2)).filter(|c| start + c < channels) {
                    let out = &mut b.output(start + channel)[at..at + len];
                    for (i, (o, gain)) in out.iter_mut().zip(&gains[..len]).enumerate() {
                        let value = if count == 1 {
                            (x[0][i] + x[1][i]) * 0.5
                        } else {
                            x[channel][i]
                        };
                        *o += value * gain;
                        peak[channel] = peak[channel].max(o.abs());
                    }
                }
            }
            if s.core.trace_master(&gains[..len]) {
                let mut observed = [[0f32; 2]; MAX_BLOCK];
                for port in 0..BUSES {
                    let route = cx
                        .bus_routing
                        .output(port)
                        .map(|r| (r.channel_start(), r.channel_count()));
                    let (start, count) = route.unwrap_or(if port == 0 {
                        (0, channels.min(2))
                    } else {
                        (0, 0)
                    });
                    let count = count.min(2).min(channels.saturating_sub(start));
                    if count == 0 {
                        continue;
                    }
                    observed[..len].fill([0.; 2]);
                    for channel in 0..count {
                        for (frame, sample) in observed[..len]
                            .iter_mut()
                            .zip(&b.output(start + channel)[at..at + len])
                        {
                            frame[channel] = *sample;
                        }
                    }
                    s.core.trace_output(port, &observed[..len], count as u8);
                }
            }
            if let Some(tapped) = s.core.tapped(len) {
                shared.scope.push(tapped);
            }
            at += len;
            #[cfg(test)]
            callback_audit_mark(&mut s.callback_audit, &mut callback_stamp, 5);
        }
        for slot in 0..s.core.parts() {
            let epoch = s.core.epoch(slot);
            let atoms = part_atoms(&s.shared_parts, shared, slot).unwrap();
            if shared.editor_watch.load(Ordering::Relaxed) == slot
                && atoms.generation.load(Ordering::Acquire) == epoch
            {
                for (to, tap) in atoms.editor_taps.iter().zip(s.core.voice_taps(slot)) {
                    to.store(
                        tap.map_or(0, |t| crate::sound::edits::Tap::from_native(t).pack()),
                        Ordering::Relaxed,
                    );
                }
            }
            let revision = s.core.ui_revision(slot);
            if atoms.generation.load(Ordering::Acquire) == epoch
                && revision != atoms.native_revision.load(Ordering::Relaxed)
            {
                atoms.refresh_controls(|id| s.core.control_value(slot, id));
                atoms.refresh_effect_controls(|address| s.core.effect_value(slot, address));
                atoms.native_revision.store(revision, Ordering::Relaxed);
            }
            s.core.take_effects(slot, &mut |instance, effect| {
                shared
                    .effects
                    .push((slot, epoch, instance, *effect))
                    .is_ok()
            });
        }
        #[cfg(test)]
        callback_audit_mark(&mut s.callback_audit, &mut callback_stamp, 6);
        let offset = frames.saturating_sub(1) as u32;
        let refused = s
            .core
            .end_block(frames, &mut |note| end_host_note(cx, note, offset));
        s.end_rejections += refused;
        #[cfg(test)]
        callback_audit_mark(&mut s.callback_audit, &mut callback_stamp, 7);
        shared.blocks.fetch_add(1, Ordering::Relaxed);
        cx.set_meter(P::Level, peak[0].max(peak[1]).min(1.0));
        if frames > 0 && rate > 0. {
            let fall = 0.1f32.powf(frames as f32 / rate as f32);
            let m = &shared.meters;
            let peaks = s.core.peaks_mut();
            for (slot, peak) in peaks.parts.iter().copied().enumerate() {
                if let Some(atoms) = part_atoms(&s.shared_parts, shared, slot) {
                    Meters::publish(&atoms.meter, peak, fall, &atoms.clip);
                }
            }
            let peaks_parts = peaks.parts.len();
            for slot in 0..peaks_parts.min(s.core.parts()) {
                let Some(atoms) = part_atoms(&s.shared_parts, shared, slot) else {
                    continue;
                };
                // Raw UVI taps must be published before take_node_peaks
                // consumes the same physical output observations below.
                atoms.refresh_uvi_meters(s.core.epoch(slot), |request| {
                    s.core.uvi_meter_observation(slot, request)
                });
                s.core.finish_uvi_meter_observation(slot);
                let Ok(meters) = atoms.node_meters.try_lock_unpoisoned() else {
                    continue;
                };
                let unlit = AtomicBool::new(false);
                s.core.take_node_peaks(slot, &mut |node, peak| {
                    if let Some(meter) = meters.get(node) {
                        Meters::publish(meter, peak, fall, &unlit);
                    }
                });
            }
            let peaks = s.core.peaks_mut();
            for ((meter, peak), clip) in m.buses.iter().zip(peaks.buses).zip(&m.clips.buses) {
                Meters::publish(meter, peak, fall, clip);
            }
            Meters::publish(&m.master, peak, fall, &m.clips.master);
            peaks.parts.fill([0.0; 2]);
            peaks.buses.fill([0.0; 2]);
        }
        #[cfg(test)]
        callback_audit_mark(&mut s.callback_audit, &mut callback_stamp, 8);
        let voices = s.core.voices();
        shared.voices.store(voices.active as u64, Ordering::Relaxed);
        shared
            .audible
            .store(voices.audible as u64, Ordering::Relaxed);
        shared.dropouts.store(voices.dropouts, Ordering::Relaxed);
        shared
            .unsupported_input
            .store(s.unsupported, Ordering::Relaxed);
        shared
            .end_rejections
            .store(s.end_rejections, Ordering::Relaxed);
        #[cfg(test)]
        callback_audit_mark(&mut s.callback_audit, &mut callback_stamp, 9);
        if frames > 0 && rate > 0. {
            // Positive `f32` bits order like the values: the UI swaps out the peak since it last looked.
            let busy = started.elapsed();
            shared
                .busy_ns
                .fetch_add(busy.as_nanos() as u64, Ordering::Relaxed);
            shared
                .span_ns
                .fetch_add((frames as f64 * 1e9 / rate) as u64, Ordering::Relaxed);
            let load = (busy.as_secs_f64() * rate / frames as f64) as f32;
            shared
                .cpu
                .fetch_max(u64::from(load.to_bits()), Ordering::Relaxed);
            s.load = load.max(s.load * 0.98 + load * 0.02);
        }
        ProcessStatus::Normal
    }

    fn editor(params: Arc<SamplerParams>) -> Box<dyn Editor> {
        crate::ui::editor(params)
    }

    fn latency(s: &Dsp) -> u32 {
        s.core.latency()
    }
}

moose::plugin! { logic:Sampler, params:SamplerParams, tasks:[Load] }

#[cfg(all(test, target_os = "linux", target_env = "gnu"))]
#[path = "allocation_audit.rs"]
pub(crate) mod allocation_audit;

#[cfg(test)]
pub(crate) mod tests {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    use super::allocation_audit;
    use super::*;

    #[test]
    fn poisoned_editor_state_recovers_without_reset_or_frame_panics() {
        let p = SamplerParams::new();
        let poison = |f: &mut dyn FnMut()| {
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err());
        };
        poison(&mut || {
            let mut view = p.shared.view.lock_unpoisoned();
            view.status = "retained editor state".into();
            panic!("synthetic original UI fault");
        });
        poison(&mut || {
            let mut selection = p.selection.write_unpoisoned();
            selection.parts = vec![Part {
                program: 77,
                ..Default::default()
            }];
            panic!("synthetic selection fault");
        });
        poison(&mut || {
            let _parts = p.shared.parts.lock_unpoisoned();
            panic!("synthetic registry fault");
        });
        assert!(p.shared.view.is_poisoned());
        for _ in 0..64 {
            p.reload_ui_controls();
            assert_eq!(p.shared.with_parts(|parts| parts.len()), RACK_SLOTS);
        }
        let report = p.diagnostic_report();
        assert_eq!(report["rack"]["parts"][0]["program"], 77);
        assert_eq!(
            p.shared.view.lock_unpoisoned().status,
            "retained editor state"
        );
        assert!(!p.shared.view.is_poisoned());
        assert!(!p.selection.is_poisoned());
        assert!(!p.shared.parts.is_poisoned());
    }

    #[test]
    fn poisoned_nonblocking_controls_resume_publication() {
        let part = PartShared::default();
        let id = sampler_ui_ir::ControlId(1);
        *part.controls.lock_unpoisoned() = vec![ControlCell::new(id, 0.25)].into();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = part.controls.lock_unpoisoned();
                panic!("synthetic control publication fault");
            }))
            .is_err()
        );
        assert_eq!(allocations(|| part.refresh_controls(|_| Some(0.75))), 0);
        assert_eq!(part.control_values(), [(id, 0.75)]);
        assert!(!part.controls.is_poisoned());
    }

    #[test]
    fn storage_failures_survive_the_plugin_report_without_heap_work() {
        let shared = PartShared::default();
        let expected = RuntimeProblems {
            stream_capacity: 1,
            stream_disconnected: 2,
            stream_failed: 3,
            stream_errors: 4,
            offline_failures: 5,
            cold_starts: 126,
            voice_pool_refusals: 127,
            waiting_voice_replacements: 128,
            ..Default::default()
        };
        assert_eq!(
            allocations(|| {
                shared.store_problems(expected);
                assert_eq!(shared.problems(), expected);
            }),
            0
        );
    }

    /// Matches Kontakt's matched-level reference: a fresh instance's master is
    /// unity, so a full-scale neutral sample leaves at the level the core renders it.
    #[test]
    fn a_fresh_master_is_unity() {
        let p = SamplerParams::new();
        assert_eq!(p.volume.read(), 0.0, "master dB");
        assert_eq!(db_to_linear(p.volume.read()), 1.0, "master gain");
        let full_scale = 1.0_f32;
        assert_eq!(full_scale * db_to_linear(p.volume.read()), full_scale);
    }

    #[test]
    fn master_gain_bulk_reads_preserve_smoothed_samples_and_automation_boundaries() {
        let reference = SamplerParams::new();
        let cached = SamplerParams::new();
        reference.volume.set_sample_rate(48000.0);
        cached.volume.set_sample_rate(48000.0);
        let mut gain = MasterGain::default();
        for target in [0.0, -60.0, 6.0, -3.5, 0.0, -0.0, -60.0] {
            reference.volume.set_value(target);
            cached.volume.set_value(target);
            for frames in [1, 3, 64, 128, 17, 128, 128, 128, 128, 128, 128] {
                let mut actual = [0.; MAX_BLOCK];
                gain.fill(&cached.volume, &mut actual[..frames]);
                for &sample in &actual[..frames] {
                    let expected: f32 = db_to_linear(reference.volume.read());
                    assert_eq!(sample.to_bits(), expected.to_bits(), "target {target}, {frames}-frame boundary");
                }
                let a: f32 = cached.volume.current();
                let b: f32 = reference.volume.current();
                assert_eq!(a.to_bits(), b.to_bits(), "both smoothers advance by exactly the same sample count");
            }
        }
    }

    #[test]
    #[ignore = "timing receipt: run optimized and without competing workloads"]
    fn master_gain_cpu_receipt() {
        use std::hint::black_box;
        let parameter = SamplerParams::new();
        parameter.volume.set_sample_rate(48000.0);
        let blocks = 100_000;
        for moving in [false, true] {
            for cached in [false, true] {
                parameter.volume.set_value(0.0);
                parameter.volume.snap_smoother();
                let mut gain = MasterGain::default();
                let mut out = [0.; MAX_BLOCK];
                let began = Instant::now();
                for block in 0..blocks {
                    if moving { parameter.volume.set_value(if block % 2 == 0 { -30.0 } else { -6.0 }); }
                    if cached { gain.fill(&parameter.volume, &mut out); }
                    else { for sample in &mut out { *sample = db_to_linear(parameter.volume.read()); } }
                    black_box(&out);
                }
                eprintln!("master_gain moving={moving} cached={cached} blocks={blocks} frames={} elapsed_ns={}", MAX_BLOCK, began.elapsed().as_nanos());
            }
        }
    }
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::Cell,
    };

    struct Counting;
    thread_local! {
        static COUNTING: Cell<bool> = const { Cell::new(false) };
        static CALLS: Cell<usize> = const { Cell::new(0) };
        static ALLOCATED: Cell<usize> = const { Cell::new(0) };
        static FREED: Cell<usize> = const { Cell::new(0) };
        static LIVE: Cell<isize> = const { Cell::new(0) };
        static PEAK: Cell<isize> = const { Cell::new(0) };
    }
    fn count() {
        if COUNTING.with(Cell::get) {
            CALLS.with(|n| n.set(n.get() + 1));
        }
    }
    // SAFETY: forwards every call unchanged to the system allocator.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count();
            let ptr = unsafe { System.alloc(layout) };
            #[cfg(all(target_os = "linux", target_env = "gnu"))]
            allocation_audit::record(ptr, layout.size(), false);
            if !ptr.is_null() && COUNTING.with(Cell::get) {
                ALLOCATED.with(|n| n.set(n.get() + layout.size()));
                LIVE.with(|n| {
                    n.set(n.get() + layout.size() as isize);
                    PEAK.with(|peak| peak.set(peak.get().max(n.get())));
                });
            }
            ptr
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            count();
            #[cfg(all(target_os = "linux", target_env = "gnu"))]
            allocation_audit::record(ptr, layout.size(), true);
            if COUNTING.with(Cell::get) {
                FREED.with(|n| n.set(n.get() + layout.size()));
                LIVE.with(|n| n.set(n.get() - layout.size() as isize));
            }
            unsafe { System.dealloc(ptr, layout) }
        }
    }
    #[global_allocator]
    static GLOBAL: Counting = Counting;

    /// Allocations and frees `f` makes on this thread.
    pub(crate) fn allocations(f: impl FnOnce()) -> usize {
        let before = CALLS.with(Cell::get);
        COUNTING.with(|c| c.set(true));
        f();
        COUNTING.with(|c| c.set(false));
        CALLS.with(Cell::get) - before
    }

    pub(crate) fn peak_allocated(f: impl FnOnce()) -> usize {
        LIVE.with(|n| n.set(0));
        PEAK.with(|n| n.set(0));
        allocations(f);
        PEAK.with(Cell::get).max(0) as usize
    }

    #[test]
    fn plugin_contract() {
        assert!(
            SamplerParams::new()
                .param_infos()
                .iter()
                .all(|p| p.midi_map.is_none()),
            "global MIDI parameter bindings would collapse port/channel routing"
        );
        moose_test::assert_valid_info::<Plugin>();
        moose_test::assert_has_editor::<Plugin>();
        moose_test::assert_state_round_trip::<Plugin>();
    }

    #[test]
    fn w10_uvi_favorite_uuid_resolution_keeps_missing_and_ambiguous_identity() {
        let mut shelf = library::Shelf::default();
        let (old, moved, copy) = (
            PathBuf::from("/banks/Old.ufs"),
            PathBuf::from("/banks/Moved.ufs"),
            PathBuf::from("/banks/Copy.ufs"),
        );
        shelf.bank_ids.insert(old.clone(), [1; 16]);
        let mut star = UviFavorite::at(&old.join("Keys/Piano.uvip"), &shelf).unwrap();
        let saved = star.serialize();
        star = UviFavorite::deserialize(&saved).unwrap();
        shelf.bank_ids.insert(old.clone(), [2; 16]);
        assert!(
            star.resolve(&shelf, &[old.join(&star.member)]).is_none(),
            "a replacement bank cannot steal an unresolved favourite"
        );
        assert_eq!(star.bank, old.to_str().unwrap());
        shelf.bank_ids.insert(moved.clone(), [1; 16]);
        shelf.bank_ids.insert(copy.clone(), [1; 16]);
        let mut files = vec![
            old.join(&star.member),
            moved.join(&star.member),
            copy.join(&star.member),
        ];
        files.sort();
        assert!(
            star.resolve(&shelf, &files).is_none(),
            "two relocated UUID copies are ambiguous"
        );
        shelf.bank_ids.remove(&copy);
        assert_eq!(star.resolve(&shelf, &files), Some(moved.join(&star.member)));
        shelf.bank_ids.insert(copy.clone(), [1; 16]);
        assert_eq!(
            star.resolve(&shelf, &files),
            Some(moved.join(&star.member)),
            "a still-valid saved locator wins over another copy"
        );
        let same = UviFavorite::at(&copy.join(&star.member), &shelf).unwrap();
        assert!(
            star.same_program(&same),
            "the locator does not change authored identity"
        );
        let other = UviFavorite::at(&copy.join("Keys/EP.uvip"), &shelf).unwrap();
        assert!(!star.same_program(&other));
        let replaced = UviFavorite::at(&old.join(&star.member), &shelf).unwrap();
        assert!(!star.same_program(&replaced));
        assert!(
            star.resolve(&shelf, &[]).is_none(),
            "a missing member cannot be synthesized"
        );
        for invalid in ["../Outside.uvip", "/Outside.uvip", "Keys/../Outside.uvip"] {
            let mut bad = same.clone();
            bad.member = invalid.into();
            assert!(
                bad.resolve(&shelf, &files).is_none(),
                "saved members must stay inside their bank"
            );
        }
    }

    #[test]
    fn rack_state_round_trip() {
        let state = Selection {
            parts: vec![
                Part {
                    path: "first.nki".into(),
                    program: 2,
                    channel: 2,
                    gain: -9.,
                    tune: -3.5,
                    mute: true,
                    ..Default::default()
                },
                Part {
                    path: "second.nki".into(),
                    nodes: vec![NodeMix {
                        gain: -3.,
                        output: crate::sound::tree::NodeOutput::Pair(4),
                        manual: true,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            order: vec![1, 0],
            buses: vec![Bus {
                name: "Drums".into(),
                port: 3,
                ..Default::default()
            }],
            outputs: 2,
            uvi_favorites: vec![UviFavorite {
                bank: "/banks/Piano.ufs".into(),
                member: "Keys/Piano.uvip".into(),
                high: u64::MAX,
                low: 0x0123456789abcdef,
            }],
            ..Default::default()
        };
        assert!(Selection::deserialize(&state.serialize()).unwrap() == state);
    }

    #[test]
    fn saved_multi_round_trips_and_refuses_other_formats() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rack.kontra-multi");
        let selection = Selection {
            parts: vec![
                Part {
                    path: "a.nki".into(),
                    ..Default::default()
                },
                Part::default(),
            ],
            order: vec![0, 1],
            ..Default::default()
        };
        SavedMulti::of("Evening", &selection).save(&path).unwrap();
        let back = SavedMulti::read(&path).unwrap();
        assert_eq!(
            (back.name.as_str(), back.parts.len()),
            ("Evening", 1),
            "empty parts are not saved"
        );
        std::fs::write(
            &path,
            r#"{"format":"kontra-multi","version":1,"name":"x","parts":[]}"#,
        )
        .unwrap();
        assert!(SavedMulti::read(&path).is_err(), "v1 multis are not read");
    }

    /// Run the loader until `done` holds, at most a few seconds.
    fn load_until(params: &SamplerParams, done: impl Fn(&SamplerParams) -> bool) {
        for _ in 0..200 {
            Load.run(params);
            if done(params) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("loader did not finish");
    }

    #[test]
    fn loader_hands_parts_over_and_the_audio_thread_installs_them() {
        let dir = tempfile::tempdir().unwrap();
        let sample = dir.path().join("tone.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&sample, spec).unwrap();
        for i in 0..48000 {
            w.write_sample(((i as f32 * 0.05).sin() * 20000.0) as i16)
                .unwrap();
        }
        w.finalize().unwrap();
        // Plain WAV loads have no imported instrument metadata. Exercise the
        // production instrument reader and its worker index with an authored
        // UVI program referencing the small synthetic sample above.
        let path = dir.path().join("tone.uvip");
        std::fs::write(&path, r#"<UVI4><Program Name="Authored worker mapping"><Layers><Layer Name="Tone"><Keygroups><Keygroup Name="C4" LowKey="0" HighKey="108"><Oscillators><SamplePlayer SamplePath="tone.wav" BaseNote="60"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>"#).unwrap();
        let params = SamplerParams::new();
        params.selection.write_unpoisoned().parts = vec![Part {
            path: path.display().to_string(),
            // This checks resident handover and immediate allocation-free audio,
            // as the original plain-WAV fixture did. Auto streaming deliberately
            // leaves ordinary UVI heads cold until the decoder services them.
            streaming: Some(Streaming::RamOnly),
            ..Default::default()
        }];
        load_until(&params, |p| {
            p.shared.view.lock_unpoisoned().parts[0].tree.is_some()
        });
        let view = params.shared.view.lock_unpoisoned();
        let v = &view.parts[0];
        assert!(
            !v.loading && v.report.as_ref().is_some_and(|r| r.decoded.zones == 1),
            "{}",
            v.status
        );
        let index = v.mapping_index.clone().expect("load worker prepared the mapping index");
        let instrument = v.instrument.clone().unwrap();
        let generation = v.generation;
        drop(view);
        assert!(!load_part(&params, 0, None), "unchanged source does not rebuild mapping");
        {
            let view = params.shared.view.lock_unpoisoned();
            let current = &view.parts[0];
            assert!(Arc::ptr_eq(&index, current.mapping_index.as_ref().unwrap()));
            assert!(Arc::ptr_eq(&instrument, current.instrument.as_ref().unwrap()));
            assert_eq!(current.generation, generation);
        }

        let mut dsp = Dsp::default();
        let (slot, generation, part) = params.shared.ready.pop().expect("a part was handed over");
        assert_eq!(
            (slot, generation),
            (
                0,
                params
                    .shared
                    .part(0)
                    .unwrap()
                    .generation
                    .load(Ordering::Relaxed)
            )
        );
        dsp.core.install(slot, part);
        dsp.core.play(0, CoreEvent::midi1(0x90, 60, 100));
        let mut mix = mix(&params.selection.read_unpoisoned());
        let installed = allocations(|| {
            dsp.core.set_mix(&mix);
            let r = dsp.core.render(64);
            assert!(r.live[0]);
        });
        assert_eq!(installed, 0, "mixing and rendering do not allocate");
        mix.parts[0].mute = true;
        dsp.core.set_mix(&mix);
        assert!(
            !dsp.core.render(64).buses[0][0][..64]
                .iter()
                .any(|x| x.abs() > 0.0),
            "muted"
        );

        // Clearing the part hands over an empty slot.
        params.selection.write_unpoisoned().parts[0].path.clear();
        load_until(&params, |p| {
            p.shared.view.lock_unpoisoned().parts[0].tree.is_none()
        });
        let (_, _, part) = std::iter::from_fn(|| params.shared.ready.pop())
            .last()
            .unwrap();
        assert!(part.is_none());
        assert!(params.shared.view.lock_unpoisoned().parts[0].mapping_index.is_none());
        assert!(params.shared.view.lock_unpoisoned().parts[0].active_source.is_none());
    }

    #[test]
    fn a_failed_load_reports_why() {
        let params = SamplerParams::new();
        params.selection.write_unpoisoned().parts = vec![Part {
            path: "/nonexistent/x.nki".into(),
            ..Default::default()
        }];
        Load.run(&params);
        let view = params.shared.view.lock_unpoisoned();
        assert!(
            view.parts[0].status.starts_with("Load failed"),
            "{}",
            view.parts[0].status
        );
        assert!(view.parts[0].trace.is_some());
    }

    pub(super) fn transaction_candidate(name: &str) -> crate::sound::Loaded<Option<Box<CorePart>>> {
        transaction_candidate_with_source(name,
            "on init\n declare ui_knob $value(0,1000,1)\n $value := 250\n end on")
    }

    pub(super) fn transaction_candidate_with_source(name: &str, source: &str) -> crate::sound::Loaded<Option<Box<CorePart>>> {
        use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};
        let script = sampler_ksp::compile(
            source,
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[],
        )
        .unwrap();
        let face = script.ui(&|_| None).unwrap();
        let view = script.view();
        let pcm = Pcm::new(48000, vec![[0.5; 2]; 48000].into_boxed_slice()).unwrap();
        let plan = script
            .bind(
                Prepared::new(
                    48000,
                    vec![pcm],
                    vec![Region {
                        sample: 0,
                        key_low: 0,
                        key_high: 127,
                        root_key: None,
                        velocity_low: 0.,
                        velocity_high: 1.,
                        gain: 1.,
                        envelope: Envelope::default(),
                        playback: Playback::default(),
                    }],
                    // Prepared bounds the expanded key index, not groups or
                    // region count. A full-range region needs 128 candidates.
                    128,
                )
                .unwrap(),
            )
            .unwrap();
        let controls = plan
            .controls()
            .iter()
            .map(|control| (sampler_ui_ir::ControlId(control.id.0), 250.))
            .collect();
        let limits = Limits::for_plan(&plan, 8, 4);
        let tree = MixTree::instrument(name);
        crate::sound::Loaded {
            part: Some(Box::new(
                CorePart::new(Runtime::new(plan, limits).unwrap(), tree.clone()).unwrap(),
            )),
            tree,
            report: LoadReport {
                name: name.into(),
                ..Default::default()
            },
            interfaces: vec![face],
            controls,
            instrument: Some(Arc::new(sampler_ir::Instrument::default())),
            scripts: crate::sound::ScriptUi {
                views: vec![view],
                ..Default::default()
            },
            stream: None,
        }
    }

    pub(super) fn transaction_fixture() -> (SamplerParams, V2Core) {
        let params = SamplerParams::new();
        params.selection.write_unpoisoned().parts = vec![Part {
            path: "active.nki".into(),
            ..Default::default()
        }];
        assert!(load_part(&params, 0, Some(transaction_candidate("Active"))));
        let (slot, _, part) = params.shared.ready.pop().unwrap();
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(slot, part);
        core.play(0, CoreEvent::midi1(0x90, 60, 100));
        assert!(core.render(64).live[0]);
        (params, core)
    }

    #[test]
    fn a_waiting_load_services_real_retained_ui_callbacks_and_keeps_audio_playing() {
        let params = SamplerParams::new();
        params.selection.write_unpoisoned().parts = vec![Part {
            path: "active.nki".into(), ..Default::default()
        }];
        let candidate = transaction_candidate_with_source("Active", concat!(
            "on init declare ui_knob $value(0,1000,1) $value := 250 ",
            "declare ui_label $status(1,1) end on ",
            "on ui_control($value) set_text($status,\"still live\") end on",
        ));
        let control = candidate.controls[0].0;
        assert!(load_part(&params, 0, Some(candidate)));
        let (slot, epoch, part) = params.shared.ready.pop().unwrap();
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(slot, part);
        core.play(0, CoreEvent::midi1(0x90, 60, 100));
        params.selection.write_unpoisoned().parts[0].path = "private-next.nki".into();
        assert!(params.shared.set_control_at(0, epoch, control, 750.));
        let (progress_tx, progress_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let mut services = 0;
        let mut published = false;
        std::thread::scope(|scope| {
            scope.spawn(move || {
                // This worker cannot report progress or finish until the old
                // owner's real callback and publication have advanced twice.
                release_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
                progress_tx.send(Progress::DONE).unwrap();
            });
            crate::sound::v2::load_wait::relay(progress_rx, &mut |_| {}, Some(&mut || {
                services += 1;
                core.begin_block(&crate::sound::BlockInfo { frames:64, ..Default::default() });
                assert!(core.render(64).buses[0][0].iter().any(|v| *v != 0.));
                core.end_block(64, &mut |_| true);
                core.take_effects(0, &mut |instance, effect| {
                    params.shared.effects.push((0, epoch, instance, *effect)).is_ok()
                });
                service_current_ui(&params);
                let view = params.shared.view.lock_unpoisoned();
                let part = &view.parts[0];
                assert_eq!(part.generation, epoch);
                assert_eq!(part.active, "Active");
                published |= part.updates.iter().any(|patch| patch.widgets.iter()
                    .any(|(_, widget)| widget.text == "still live"));
                if published && services >= 2 { let _ = release_tx.try_send(()); }
            }));
        });
        assert!(published);
        assert!(services >= 2);
        assert_eq!(core.epoch(0), epoch);
        assert!(params.shared.ready.pop().is_none());
    }

    #[test]
    fn failed_replacement_preserves_audio_view_controls_and_ownership() {
        let (params, mut core) = transaction_fixture();
        let atoms = params.shared.part(0).unwrap();
        let epoch = atoms.generation.load(Ordering::Acquire);
        let controls = atoms.controls.lock_unpoisoned().clone();
        let plan = atoms.ingress.lock_unpoisoned().as_ref().unwrap().plan();
        let previous = params.shared.view.lock_unpoisoned().parts[0].clone();
        params.selection.write_unpoisoned().parts[0].path = "/nonexistent/replacement.nki".into();
        assert!(load_part(&params, 0, None));
        assert!(
            params.shared.ready.pop().is_none(),
            "failure cannot unload the active audio"
        );
        assert_eq!(atoms.generation.load(Ordering::Acquire), epoch);
        assert_eq!(core.epoch(0), epoch);
        assert_eq!(
            atoms.ingress.lock_unpoisoned().as_ref().unwrap().plan(),
            plan
        );
        assert!(Arc::ptr_eq(&controls, &atoms.controls.lock_unpoisoned()));
        let current = params.shared.view.lock_unpoisoned().parts[0].clone();
        assert_eq!(current.active, "Active");
        assert_eq!(current.active_source, previous.active_source);
        assert_eq!(current.generation, epoch);
        assert!(!current.loading);
        assert!(current.status.starts_with("Load failed"));
        assert!(Arc::ptr_eq(
            previous.report.as_ref().unwrap(),
            current.report.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(
            previous.instrument.as_ref().unwrap(),
            current.instrument.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(
            previous.mapping_index.as_ref().unwrap(),
            current.mapping_index.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(&previous.interfaces, &current.interfaces));
        assert!(params.shared.set_control_at(0, epoch, controls[0].id, 750.));
        assert!(
            core.render(64).live[0],
            "held audio survives a failed replacement"
        );
        assert!(
            !load_part(&params, 0, None),
            "the failed source is not retried every poll"
        );
        {
            let mut selection = params.selection.write_unpoisoned();
            let pending = &mut selection.parts[0];
            pending.script_state = "pending source state".into();
            pending.script_state_source = "pending source identity".into();
            pending.control_values = vec![SavedControl::new(sampler_ui_ir::ControlId(999), 42.)];
        }
        params.capture_ui_controls();
        let saved = params.selection.read_unpoisoned().parts[0].clone();
        assert_eq!(saved.script_state, "pending source state");
        assert_eq!(saved.script_state_source, "pending source identity");
        assert_eq!(saved.control_values.len(), 1);
        assert_eq!(saved.control_values[0].id(), sampler_ui_ir::ControlId(999));
    }

    #[test]
    fn same_source_recall_preserves_candidate_state_during_private_preparation() {
        let (params, mut core) = transaction_fixture();
        let atoms = params.shared.part(0).unwrap();
        let epoch = atoms.generation.load(Ordering::Acquire);
        let control = atoms.controls.lock_unpoisoned()[0].id;
        assert_eq!(core.control_value(0, control), Some(250.));
        {
            let mut selection = params.selection.write_unpoisoned();
            let recalled = &mut selection.parts[0];
            recalled.control_values = vec![SavedControl::new(control, 800.)];
            recalled.script_state = "recalled state must stay untouched".into();
            recalled.script_state_source = serde_json::to_string(&recalled.source()).unwrap();
        }
        let assert_recalled = || {
            let recalled = params.selection.read_unpoisoned().parts[0].clone();
            assert_eq!(recalled.control_values[0].value, 800.);
            assert_eq!(recalled.script_state, "recalled state must stay untouched");
        };
        params.reload_ui_controls();
        // A host can save synchronously after post_load, before the loader runs.
        params.capture_ui_controls();
        assert_recalled();
        assert!(!load_part_with(&params, 0, None, |request, _, canceled| {
            assert!(!canceled());
            assert_eq!(request.control_values, vec![(control, 800.)]);
            assert!(params.shared.view.lock_unpoisoned().parts[0].loading);
            params.capture_ui_controls();
            let recalled = params.selection.read_unpoisoned().parts[0].clone();
            assert_eq!(recalled.control_values[0].value, 800.);
            assert_eq!(recalled.script_state, "recalled state must stay untouched");
            assert_eq!(
                recalled.script_state_source,
                serde_json::to_string(&recalled.source()).unwrap()
            );
            assert_eq!(core.control_value(0, control), Some(250.));
            assert_eq!(atoms.generation.load(Ordering::Acquire), epoch);
            Err(CoreError::Canceled)
        }));
        assert!(params.shared.ready.pop().is_none());
        params.capture_ui_controls();
        assert_recalled();
        assert_eq!(core.epoch(0), epoch);
        assert!(load_part_with(&params, 0, None, |_, _, _| {
            Err(CoreError::Invalid("failed recalled candidate".into()))
        }));
        params.capture_ui_controls();
        assert_recalled();

        // A second same-source recall invalidates the privately prepared job
        // even though its source, settings, and active audio epoch are unchanged.
        params.selection.write_unpoisoned().parts[0].script_state.clear();
        params.reload_ui_controls();
        assert!(!load_part_with(&params, 0, None, |_, _, canceled| {
            assert!(!canceled());
            params.selection.write_unpoisoned().parts[0].control_values =
                vec![SavedControl::new(control, 900.)];
            params.reload_ui_controls();
            assert!(canceled());
            Ok(transaction_candidate("Stale recalled state"))
        }));
        assert!(params.shared.ready.pop().is_none());
        assert_eq!(atoms.generation.load(Ordering::Acquire), epoch);
        params.capture_ui_controls();
        assert_eq!(
            params.selection.read_unpoisoned().parts[0].control_values[0].value,
            900.
        );

        // Successful commit acquires the recalled identity and resumes capture.
        params.reload_ui_controls();
        assert!(load_part(&params, 0, Some(transaction_candidate("Recalled"))));
        let (slot, recalled_epoch, part) = params.shared.ready.pop().unwrap();
        assert!(recalled_epoch > epoch);
        core.install(slot, part);
        assert!(params.shared.set_control_at(0, recalled_epoch, control, 600.));
        core.render(64);
        atoms.refresh_controls(|id| core.control_value(0, id));
        params.capture_ui_controls();
        assert_eq!(
            params.selection.read_unpoisoned().parts[0].control_values[0].value,
            600.
        );
        assert!(
            params.shared.view.lock_unpoisoned().parts[0]
                .recalled_source
                .is_none()
        );
    }

    #[test]
    fn canceled_replacement_keeps_the_old_owner_and_rejects_stale_edits_after_commit() {
        let (params, mut core) = transaction_fixture();
        let atoms = params.shared.part(0).unwrap();
        let epoch = atoms.generation.load(Ordering::Acquire);
        let old = params.shared.view.lock_unpoisoned().parts[0].clone();
        let control = atoms.controls.lock_unpoisoned()[0].id;
        let mut active_selection = params.selection.read_unpoisoned().clone();
        let active_source = active_selection.parts[0].source();
        params.selection.write_unpoisoned().parts[0].path = "candidate.nki".into();
        assert!(!load_part_with(&params, 0, None, |_, _, canceled| {
            assert!(!canceled());
            assert!(params.shared.view.lock_unpoisoned().parts[0].loading);
            assert_eq!(atoms.generation.load(Ordering::Acquire), epoch);
            assert!(params.shared.set_control_at(0, epoch, control, 500.));
            core.render(64);
            atoms.refresh_controls(|id| core.control_value(0, id));
            params.shared.capture_ui_controls(&mut active_selection);
            let captured = &active_selection.parts[0];
            assert_eq!(captured.source(), active_source);
            assert_eq!(
                captured.script_state_source,
                serde_json::to_string(&active_source).unwrap()
            );
            assert!(!captured.script_state.is_empty());
            assert!(
                captured
                    .control_values
                    .iter()
                    .any(|value| value.id() == control && value.value == 500.)
            );
            params.capture_ui_controls();
            assert!(
                params.selection.read_unpoisoned().parts[0]
                    .control_values
                    .is_empty(),
                "pending candidate cannot receive the live old instrument's state"
            );
            params.selection.write_unpoisoned().parts[0].path = "newest.nki".into();
            assert!(canceled());
            Ok(transaction_candidate("Superseded"))
        }));
        assert!(params.shared.ready.pop().is_none());
        assert_eq!(atoms.generation.load(Ordering::Acquire), epoch);
        let current = params.shared.view.lock_unpoisoned().parts[0].clone();
        assert_eq!(current.status, old.status);
        assert_eq!(current.active, old.active);
        assert_eq!(current.active_source, old.active_source);
        assert!(current.attempted.is_none());
        assert!(core.render(64).live[0]);
        assert!(load_part(&params, 0, Some(transaction_candidate("Newest"))));
        let (slot, newest, part) = params.shared.ready.pop().unwrap();
        assert!(newest > epoch);
        assert!(!params.shared.set_control_at(0, epoch, control, 100.));
        core.install(slot, part);
        assert_eq!(core.epoch(0), newest);
        assert_eq!(
            params.shared.view.lock_unpoisoned().parts[0].active,
            "Newest"
        );
        assert_eq!(
            params.shared.view.lock_unpoisoned().parts[0].active_source,
            Some(("newest.nki".into(), 0, String::new()))
        );
        params
            .shared
            .publish_part((0, epoch, transaction_candidate("Stale completion").part));
        assert!(params.shared.ready.pop().is_none());
        assert_eq!(core.epoch(0), newest);
    }

    #[test]
    fn keyboard_plays_as_midi_on_channel_one() {
        assert_eq!(Play::Note(60, 100).event(), CoreEvent::midi1(0x90, 60, 100));
        assert_eq!(Play::Note(60, 0).event(), CoreEvent::midi1(0x80, 60, 0));
        assert_eq!(Play::Bend(8192).event(), CoreEvent::Ump([0x20e0_0040, 0]));
        assert_eq!(Play::Mod(64).event(), CoreEvent::midi1(0xb0, 1, 64));
    }

    /// Compare an installed script's empty and real control environments;
    /// output only aggregate allocation/eval metrics.
    #[test]
    #[ignore]
    fn probe_ksp_init() {
        let path = std::path::PathBuf::from(std::env::var("PROBE_PATH").expect("PROBE_PATH"));
        let kontakt = sampler_kontakt::read(&path).unwrap();
        let groups = kontakt
            .instrument
            .groups
            .iter()
            .map(|g| g.name.clone())
            .collect::<Vec<_>>();
        let mut resources = sampler_kontakt::Resources::of(&path);
        for (slot, behavior) in kontakt.instrument.behaviors.iter().enumerate() {
            if behavior.language != sampler_ir::Language::Ksp {
                continue;
            }
            for real in [false, true] {
                let view = if real {
                    sampler_ksp::nckp::view_name(&behavior.source)
                        .and_then(|name| {
                            resources.read(&format!("Resources/performance_view/{name}.nckp"))
                        })
                        .and_then(|bytes| sampler_ksp::nckp::parse(&bytes).ok())
                        .map(|v| v.0)
                        .unwrap_or_default()
                } else {
                    Default::default()
                };
                let environment = sampler_ksp::Environment {
                    groups: groups.clone(),
                    slot: slot as u8,
                    performance_view: view,
                    ..Default::default()
                };
                for cell in [&CALLS, &ALLOCATED, &FREED] {
                    cell.with(|n| n.set(0));
                }
                LIVE.with(|n| n.set(0));
                PEAK.with(|n| n.set(0));
                let begin = Instant::now();
                COUNTING.with(|c| c.set(true));
                let _initialized = sampler_ksp::initialize(
                    &behavior.source,
                    sampler_ksp::Limits::LIBRARY,
                    &environment,
                )
                .unwrap();
                COUNTING.with(|c| c.set(false));
                println!(
                    "AUDIT {}",
                    serde_json::json!({"stage":"init_allocations", "slot":slot, "real_view":real, "ms":begin.elapsed().as_secs_f64()*1000., "calls":CALLS.with(Cell::get), "allocated_bytes":ALLOCATED.with(Cell::get), "freed_bytes":FREED.with(Cell::get), "live_bytes":LIVE.with(Cell::get), "peak_live_bytes":PEAK.with(Cell::get)})
                );
            }
        }
    }

    /// Source and instruction-layout sizes without exporting authored script text.
    #[test]
    #[ignore]
    fn probe_ksp_sizes() {
        let path = std::path::PathBuf::from(std::env::var("PROBE_PATH").expect("PROBE_PATH"));
        let kontakt = sampler_kontakt::read(&path).unwrap();
        let source_bytes: Vec<_> = kontakt
            .instrument
            .behaviors
            .iter()
            .filter(|b| b.language == sampler_ir::Language::Ksp)
            .map(|b| b.source.len())
            .collect();
        println!(
            "AUDIT {}",
            serde_json::json!({"stage":"ksp_sizes", "source_bytes":source_bytes,
            "instruction_bytes":std::mem::size_of::<sampler_core::Instruction>()})
        );
    }

    /// Numeric-only audit: real loader, publication, C4 audio, retained editor RSS.
    #[test]
    #[ignore]
    fn probe_load() {
        let path = std::env::var("PROBE_PATH").expect("PROBE_PATH");
        let program = std::env::var("PROBE_PROGRAM")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(0);
        let proc_kb = |key: &str| -> u64 {
            std::fs::read_to_string("/proc/self/status")
                .unwrap()
                .lines()
                .find_map(|l| l.strip_prefix(key)?.split_whitespace().next()?.parse().ok())
                .unwrap_or(0)
        };
        let params = std::sync::Arc::new(SamplerParams::new());
        params.selection.write_unpoisoned().parts = vec![Part {
            path: path.clone(),
            program,
            ..Default::default()
        }];
        let rss0 = proc_kb("VmRSS:");
        let mut dsp = Dsp::default();
        dsp.core.set_mix(&mix(&params.selection.read_unpoisoned()));
        let t0 = Instant::now();
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        let allocation_output = std::env::var_os("PROBE_ALLOCS");
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        if allocation_output.is_some() {
            allocation_audit::start();
        }
        let worker = {
            let p = params.clone();
            std::thread::spawn(move || {
                Load.run(&p);
                t0.elapsed()
            })
        };
        let mut publication_ms = None;
        let mut first_audio_ms = None;
        let mut first_audio_frame = None;
        let mut installed_ms = None;
        let mut peak = 0f32;
        let mut blocks = 0u32;
        let audit_onset = std::env::var_os("PROBE_ONSET").is_some();
        let mut onset = Vec::new();
        loop {
            if publication_ms.is_none()
                && params.shared.view.lock_unpoisoned().parts[0].tree.is_some()
            {
                publication_ms = Some(t0.elapsed().as_secs_f64() * 1000.);
            }
            while let Some((slot, _, part)) = params.shared.ready.pop() {
                let present = part.is_some();
                dsp.core.install(slot, part);
                if present {
                    installed_ms = Some(t0.elapsed().as_secs_f64() * 1000.);
                    dsp.core.begin_block(&BlockInfo { frames: 64, ..Default::default() });
                    if audit_onset {
                        onset.push(serde_json::json!({"event":"installed","at_ms":installed_ms,"state":dsp.core.onset_audit()}));
                    }
                    for cc in [1, 11] {
                        let start = Instant::now();
                        dsp.core.play(0, CoreEvent::midi1(0xb0, cc, 127));
                        if audit_onset {
                            onset.push(serde_json::json!({"event":"cc","cc":cc,"elapsed_ms":start.elapsed().as_secs_f64()*1000.,"at_ms":t0.elapsed().as_secs_f64()*1000.,"state":dsp.core.onset_audit()}));
                        }
                    }
                    let start = Instant::now();
                    dsp.core.play(0, CoreEvent::midi1(0x90, 60, 100));
                    if audit_onset {
                        onset.push(serde_json::json!({"event":"note_admitted","elapsed_ms":start.elapsed().as_secs_f64()*1000.,"at_ms":t0.elapsed().as_secs_f64()*1000.,"state":dsp.core.onset_audit()}));
                    }
                }
            }
            if installed_ms.is_some() {
                if blocks > 0 { dsp.core.begin_block(&BlockInfo { frames: 64, ..Default::default() }); }
                let render_start = Instant::now();
                let rendered = dsp.core.render(64);
                for bus in rendered.buses {
                    for channel in bus {
                        for x in &channel[..64] {
                            peak = peak.max(x.abs());
                        }
                    }
                }
                if peak > 1e-7 && first_audio_ms.is_none() {
                    first_audio_ms = Some(t0.elapsed().as_secs_f64() * 1000.);
                    first_audio_frame = Some(blocks * 64);
                }
                if audit_onset && blocks < 16 {
                    onset.push(serde_json::json!({"event":"render","block":blocks,"elapsed_ms":render_start.elapsed().as_secs_f64()*1000.,"at_ms":t0.elapsed().as_secs_f64()*1000.,"peak":peak,"state":dsp.core.onset_audit()}));
                }
                blocks += 1;
            }
            if worker.is_finished() && (installed_ms.is_none() || blocks >= 375) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(1333));
        }
        let total = worker.join().unwrap();
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        if let Some(output) = allocation_output {
            allocation_audit::finish(std::path::Path::new(&output));
        }
        let rss_done = proc_kb("VmRSS:");
        let hwm_done = proc_kb("VmHWM:");
        let ui = crate::ui::audit_frames(&params);
        let ui_wall_ms = ui["build_ms"].as_f64().unwrap();
        let rss_settled = (ui["rss_live_mb"].as_f64().unwrap() * 1024.) as u64;
        let hwm = proc_kb("VmHWM:");
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        let trimmed_rss = {
            unsafe {
                libc::malloc_trim(0);
            }
            proc_kb("VmRSS:")
        };
        #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
        let trimmed_rss = rss_settled;

        let atoms = params.shared.part(0).unwrap();
        let stream = atoms.stream.lock_unpoisoned();
        let stream = stream.as_ref().map(|s| serde_json::json!({"heads":s.report.head_bytes,"head_frames":s.report.head_frames,"pool_bytes":s.report.pool_bytes,"full_bytes":s.report.full_bytes,"resident_bytes":s.resident_bytes()}));
        let view = params.shared.view.lock_unpoisoned();
        let decoded = view.parts[0].report.as_ref().map(|r| r.audit_counts());
        drop(view);
        let extra = serde_json::json!({"stream":stream,"decoded":decoded,"problems":format!("{:?}",dsp.core.problems(0))});
        let trace = params.shared.view.lock_unpoisoned().parts[0]
            .trace
            .as_deref()
            .cloned();
        println!(
            "PROBE {}",
            serde_json::json!({
                "path":path,"program":program,"publication_ms":publication_ms,
                "installed_ms":installed_ms,"first_audio_ms":first_audio_ms,"first_audio_frames":first_audio_frame,"peak":peak,
                "load_run_ms":total.as_secs_f64()*1000.,"ui_wall_ms":ui_wall_ms,"ui":ui,
                "rss0_mb":rss0 as f64/1024.,"rss_done_mb":rss_done as f64/1024.,"hwm_done_mb":hwm_done as f64/1024.,
                "rss_settled_mb":rss_settled as f64/1024.,"rss_after_trim_mb":trimmed_rss as f64/1024.,"hwm_mb":hwm as f64/1024.,"trace":trace,
                "extra":extra,"onset":onset
            })
        );
    }

    #[test]
    fn exact_native_port_tuples_reach_the_real_core_after_part_admission_is_remapped() {
        use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};
        let decode = |body| {
            let mut events = moose::core::EventList::with_capacity(1);
            events.try_push_exact(ExactEvent::new(0, body)).unwrap();
            let LosslessEventRef::Exact(event) = events.lossless_iter().next().unwrap() else {
                panic!("exact native event lost its payload")
            };
            match exact_host_input(event).unwrap() {
                ExactInput::Routed(event, port) => (event, port),
                ExactInput::Unsupported => panic!("authored native event refused"),
            }
        };
        for clap in [true, false] {
            let pcm = Pcm::new(48000, vec![[0.25; 2]; 4096].into_boxed_slice()).unwrap();
            let region = Region { sample: 0, key_low: 60, key_high: 60, root_key: None,
                velocity_low: 0., velocity_high: 1., gain: 1.,
                envelope: Envelope::default(), playback: Playback::default() };
            let plan = Prepared::new(48000, vec![pcm], vec![region], 1).unwrap();
            let limits = Limits::for_plan(&plan, 8, 16);
            let mut core = V2Core::with_parts(1, 48000.);
            core.install(0, Some(Box::new(CorePart::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("exact native")).unwrap())));
            let mut mix = Mix::default(); mix.parts[0].port = 2; mix.parts[0].channel = 3; core.set_mix(&mix);
            let address = ExactNoteAddress::from_raw_signed(2, 3, 60, 117);
            let note = HostNote { port: 2, channel: 3, key: 60, id: 117, clap };
            let body = if clap { ExactEventBody::Note { kind: ExactNoteKind::On, address, velocity: 0.75 } }
                else { ExactEventBody::DetailedNote { kind: ExactNoteKind::On, address, velocity: 0.75, tuning: 0., length: None } };
            let (event, port) = decode(body); assert_eq!(port, 2); core.event(port, event);
            assert!(core.owns(note)); assert!(core.render(1).buses[0][0][0] > 0.);
            let mut changed = mix.clone(); changed.parts[0].port = 4; changed.parts[0].channel = 7; core.set_mix(&changed);
            let body = if clap { ExactEventBody::NoteExpression { expression_id: 0, address, value: 0. } }
                else { ExactEventBody::NormalizedNoteExpression { expression_id: 0, address, value: 0. } };
            let (event, port) = decode(body); assert_eq!(port, 2); core.event(port, event);
            assert!(core.render(32).buses[0][0][..32].iter().all(|v| *v == 0.));
            assert_eq!(core.voices().active, 1, "held note expression must not retrigger or terminate its owner");
            let body = if clap { ExactEventBody::Note { kind: ExactNoteKind::Off, address, velocity: 0.5 } }
                else { ExactEventBody::DetailedNote { kind: ExactNoteKind::Off, address, velocity: 0.5, tuning: 0., length: None } };
            let (event, port) = decode(body); assert_eq!(port, 2); core.event(port, event); core.render(128);
            let mut ended = Vec::new(); core.end_block(128, &mut |note| { ended.push(note); true });
            assert!(!core.owns(note)); assert_eq!(core.voices().active, 0);
            assert_eq!(ended, if clap { vec![note] } else { vec![] });
            core.end_block(1, &mut |_| panic!("duplicate native end"));
        }
    }

}

mod effect_controls;
#[cfg(test)]
mod loop_audit;
#[cfg(test)]
mod cpu_receipt;
#[cfg(test)]
mod persistence_tests;
#[cfg(test)]
mod pressed_tests;
#[cfg(test)]
mod uvi_save_tests;
#[cfg(test)]
mod lifecycle_audit_tests;

#[cfg(test)]
mod settings_parity_tests {
    use super::*;
    #[test]
    fn v1_snapshot_requests_validate_before_mutating() {
        let part = Part {
            path: "/unavailable-kontakto/base.nki".into(),
            snapshot: "/unavailable-kontakto/old.nksn".into(),
            gain: -4.,
            channel: 3,
            ..Default::default()
        };
        let p = SamplerParams::new();
        p.selection.write_unpoisoned().parts = vec![part.clone()];
        let generation = p.shared.part(0).unwrap().generation.load(Ordering::Acquire);
        assert!(
            p.shared
                .queue_snapshot(0, &part, "/unavailable-kontakto/missing.nksn".into(), true)
        );
        assert_eq!(
            p.shared.part(0).unwrap().generation.load(Ordering::Acquire),
            generation,
            "an unvalidated request cannot invalidate the active bank's services"
        );
        assert!(prepare_snapshot(&p).is_none());
        assert!(p.selection.read_unpoisoned().parts[0] == part);
        let report = p.shared.view.lock_unpoisoned().parts[0]
            .trace
            .clone()
            .unwrap();
        assert_eq!(report["details"]["operation"], "snapshot_validation");
        assert_eq!(report["status"], "failed");
        assert!(report["failure"].is_string());
        crate::diagnostics::flush(std::time::Duration::from_secs(5)).unwrap();
        assert!(
            crate::diagnostics::snapshot()
                .events
                .iter()
                .any(
                    |event| event.load_id.as_deref() == report["load_id"].as_str()
                        && event.event == "load_finished"
                        && event.details["status"] == "failed"
                ),
            "failed validation is retained in the diagnostic journal"
        );
        assert!(
            p.shared.view.lock_unpoisoned().parts[0]
                .status
                .contains("Preset was not loaded")
        );
        assert!(
            p.shared
                .queue_snapshot(0, &part, "/unavailable-kontakto/stale.nksn".into(), true)
        );
        p.selection.write_unpoisoned().parts[0].snapshot = "replacement.nksn".into();
        p.shared.view.lock_unpoisoned().parts[0].status = "replacement".into();
        assert!(prepare_snapshot(&p).is_none());
        assert_eq!(
            p.shared.view.lock_unpoisoned().parts[0].status,
            "replacement"
        );
        crate::diagnostics::flush(std::time::Duration::from_secs(5)).unwrap();
        assert!(
            crate::diagnostics::snapshot()
                .events
                .iter()
                .any(|event| event.event == "load_finished"
                    && event.details["status"] == "canceled"
                    && event.details["details"]["snapshot"] == "/unavailable-kontakto/stale.nksn")
        );
        let multi = Part {
            path: "ensemble.nkm".into(),
            ..Default::default()
        };
        assert!(!p.shared.queue_snapshot(0, &multi, "preset.nksn".into(), true));
        let program = Part { program: 1, ..part };
        assert!(!p.shared.queue_snapshot(0, &program, "preset.nksn".into(), true));
    }

    #[test]
    fn v1_part_playback_choices_survive_v2_state() {
        // Unknown fields were silently discarded before restoring these controls.
        let input = serde_json::json!({"streaming": "RamOnly", "mpe_upper": true,
            "snapshot": "/library/Snapshots/Soft.nksn"});
        let part: Part = serde_json::from_value(input.clone()).unwrap();
        let saved = serde_json::to_value(&part).unwrap();
        for key in ["streaming", "mpe_upper", "snapshot"] {
            assert_eq!(saved.get(key), input.get(key), "lost {key}");
        }
        let mut buf = Vec::new();
        part.write_field(&mut buf);
        let restored =
            Part::read_field(&mut moose::core::custom_state::StateCursor::new(&buf)).unwrap();
        assert!(part == restored);
    }
}

// Port v1 0cb7a8a0:src/plugin.rs:3286+: source-guarded completed jobs,
// manual controls, readiness checks, fresh scripts and latency settling.
const LATENCY_SETTLE: std::time::Duration = std::time::Duration::from_millis(1500);
fn align(params: &SamplerParams) {
    let shared = &params.shared;
    let done = shared.measure.take();
    if !done.is_empty() {
        let mut statuses = Vec::new();
        let mut current = params.selection.write_unpoisoned();
        for (slot, generation, source, result) in done {
            if !shared
                .part(slot)
                .is_some_and(|p| p.generation.load(Ordering::Acquire) == generation)
            {
                continue;
            }
            let Some(p) = current
                .parts
                .get_mut(slot)
                .filter(|p| crate::timing::source(&p.path, p.program, &p.snapshot) == source)
            else {
                continue;
            };
            let Some(result) = result else {
                statuses.push((slot, String::new()));
                continue;
            };
            match result {
                Ok(t) => {
                    p.timing = crate::timing::Timing {
                        override_ms: p.timing.override_ms,
                        exclude: p.timing.exclude,
                        ..t
                    };
                    statuses.push((slot, String::new()));
                }
                Err(e) => {
                    p.timing = crate::timing::Timing {
                        source,
                        override_ms: p.timing.override_ms,
                        exclude: p.timing.exclude,
                        ..Default::default()
                    };
                    statuses.push((slot, format!("Timing not measured: {e}")));
                }
            }
        }
        drop(current);
        let mut view = shared.view.lock_unpoisoned();
        for (slot, status) in statuses {
            if let Some(v) = view.parts.get_mut(slot) {
                v.timing_status = status;
            }
        }
    }
    let selection = params.selection.read_unpoisoned().clone();
    if !selection.auto_align {
        shared.measure.cancel();
    }
    if selection.auto_align && !shared.measure.busy() {
        for (slot, part) in selection.parts.iter().enumerate() {
            if part.path.is_empty()
                || part
                    .timing
                    .measured(&part.path, part.program, &part.snapshot)
            {
                continue;
            }
            let ready = {
                let view = shared.view.lock_unpoisoned();
                view.parts.get(slot).is_some_and(|v| {
                    !v.loading
                        && v.active_source
                            .as_ref()
                            .is_some_and(|(path, program, snapshot, ..)| {
                                path == &part.path
                                    && *program == part.program
                                    && snapshot == &part.snapshot
                            })
                        && v.report.is_some()
                })
            };
            if !ready {
                continue;
            }
            let Some(atoms) = shared.part(slot) else {
                continue;
            };
            let generation = atoms.generation.load(Ordering::Acquire);
            let request = LoadRequest {
                path: part.path.clone().into(),
                program: part.program,
                sample_rate: shared.rate(),
                snapshot: (!part.snapshot.is_empty()).then(|| part.snapshot.clone().into()),
                control_values: part
                    .control_values
                    .iter()
                    .map(|c| (c.id(), c.value))
                    .collect(),
                uvi_state: serde_json::from_str(&part.uvi_state).ok(),
                dynamics_start: u8::try_from(part.dynamics).ok().filter(|v| *v < 128),
                // Like v1's fresh measuring engine: manager-channel takes;
                // disk pages wait offline, and rendering uses one worker.
                mpe: false,
                mpe_upper: false,
                streaming: Streaming::Auto,
                threads: Some(crate::sound::ThreadChoice::Fixed(1)),
            };
            let result =
                shared
                    .measure
                    .start(slot, generation, request, part.edits.clone(), move || {
                        atoms.generation.load(Ordering::Acquire) != generation
                    });
            shared.view.lock_unpoisoned().parts[slot].timing_status = match result {
                Ok(()) => "Measuring timing…".into(),
                Err(e) => format!("Timing not measured: {e}"),
            };
            break;
        }
    }
    let plan = timing_plan(&selection);
    let mut published = shared.published.lock_unpoisoned();
    let (sent, waiting) = &mut *published;
    if sent.as_ref().is_some_and(|old| old.as_ref() == &plan) {
        *waiting = None;
        return;
    }
    let told = |p: &crate::timing::Plan| if p.on { p.latency_ms } else { 0. };
    let now = Instant::now();
    let settled = match (&sent, &waiting) {
        (None, _) => true,
        (Some(s), _) if told(s) == told(&plan) => true,
        (_, Some((ms, since))) if *ms == told(&plan) => {
            now.duration_since(*since) >= LATENCY_SETTLE
        }
        _ => {
            *waiting = Some((told(&plan), now));
            false
        }
    };
    if settled {
        *sent = Some(Arc::new(plan));
        *waiting = None;
        shared
            .reported
            .store(told(sent.as_ref().unwrap()).to_bits(), Ordering::Relaxed);
    }
}

/// Port v1 timing_for/plan; stale patch and snapshot measurements are excluded.
pub(crate) fn timing_for(p: &Part) -> std::borrow::Cow<'_, crate::timing::Timing> {
    if p.timing.measured(&p.path, p.program, &p.snapshot) {
        std::borrow::Cow::Borrowed(&p.timing)
    } else {
        std::borrow::Cow::Owned(crate::timing::Timing {
            override_ms: p.timing.override_ms,
            exclude: p.timing.exclude,
            ..Default::default()
        })
    }
}
fn timing_plan(s: &Selection) -> crate::timing::Plan {
    let latency_ms = crate::timing::reported_ms(
        s.parts
            .iter()
            .filter(|p| !p.path.is_empty())
            .map(|p| timing_for(p).latest()),
    );
    crate::timing::Plan {
        on: s.auto_align,
        transport_only: s.align_transport_only,
        latency_ms,
        parts: (0..s.parts.len().max(RACK_SLOTS))
            .map(|n| {
                std::sync::Arc::new(s.parts.get(n).filter(|p| !p.path.is_empty()).map_or_else(
                    crate::timing::Holds::default,
                    |p| {
                        let names: Vec<&str> =
                            p.timing.arts.iter().map(|a| a.identity.as_str()).collect();
                        crate::timing::Holds::of(&timing_for(p), &names, latency_ms)
                    },
                ))
            })
            .collect(),
    }
}

#[cfg(test)]
mod timing_loader_tests {
    use super::*;
    #[test]
    fn v1_auto_align_finished_jobs_reject_stale_generations_and_snapshots() {
        let p = SamplerParams::new();
        let atoms = p.shared.part(0).unwrap();
        atoms.generation.store(7, Ordering::Release);
        let source = crate::timing::source("synthetic.nki", 2, "saved.nksn");
        p.selection.write_unpoisoned().parts = vec![Part {
            path: "synthetic.nki".into(),
            program: 2,
            snapshot: "saved.nksn".into(),
            timing: crate::timing::Timing {
                override_ms: Some(95.),
                exclude: true,
                ..Default::default()
            },
            ..Default::default()
        }];
        let result = crate::timing::Timing {
            source: source.clone(),
            declared: Some(47.),
            ..Default::default()
        };
        for (generation, identity) in [
            (6, source.clone()),
            (7, crate::timing::source("synthetic.nki", 2, "other.nksn")),
        ] {
            p.shared.measure.done.lock_unpoisoned().push((
                0,
                generation,
                identity,
                Some(Ok(result.clone())),
            ));
            align(&p);
            assert!(
                p.selection.read_unpoisoned().parts[0]
                    .timing
                    .source
                    .is_empty(),
                "stale result changed this part"
            );
        }
        p.shared
            .measure
            .done
            .lock_unpoisoned()
            .push((0, 7, source.clone(), Some(Ok(result))));
        align(&p);
        let s = p.selection.read_unpoisoned();
        let t = &s.parts[0].timing;
        assert_eq!(t.source, source);
        assert_eq!(t.declared, Some(47.));
        assert_eq!(t.override_ms, Some(95.));
        assert!(t.exclude);
    }
    #[test]
    fn v1_auto_align_publishes_changed_latency_only_after_settling() {
        let p = SamplerParams::new();
        {
            let mut s = p.selection.write_unpoisoned();
            s.auto_align = true;
            s.parts = vec![Part {
                path: "synthetic.nki".into(),
                timing: crate::timing::Timing {
                    override_ms: Some(10.),
                    ..Default::default()
                },
                ..Default::default()
            }];
        }
        align(&p);
        assert_eq!(
            f32::from_bits(p.shared.reported.load(Ordering::Relaxed)),
            10.
        );
        p.selection.write_unpoisoned().parts[0].timing.override_ms = Some(20.);
        align(&p);
        assert_eq!(
            f32::from_bits(p.shared.reported.load(Ordering::Relaxed)),
            10.
        );
        p.shared.published.lock_unpoisoned().1 = Some((20., Instant::now() - LATENCY_SETTLE));
        align(&p);
        assert_eq!(
            f32::from_bits(p.shared.reported.load(Ordering::Relaxed)),
            20.
        );
        assert_eq!(
            p.shared
                .published
                .lock_unpoisoned()
                .0
                .as_ref()
                .unwrap()
                .latency_ms,
            20.
        );
    }
    #[test]
    fn v1_auto_align_loader_measures_a_fresh_generated_wave_and_persists_it() {
        let dir = std::env::temp_dir().join(format!("kontra-timing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("C3.wav");
        let data: Vec<u8> = (0..72000)
            .flat_map(|n| {
                (((std::f64::consts::TAU * 261.625565 * n as f64 / 48000.).sin() * 12000.) as i16)
                    .to_le_bytes()
            })
            .collect();
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((36 + data.len() as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(48000u32.to_le_bytes());
        wav.extend(96000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((data.len() as u32).to_le_bytes());
        wav.extend(data);
        std::fs::write(&path, wav).unwrap();
        let p = SamplerParams::new();
        {
            let mut s = p.selection.write_unpoisoned();
            s.auto_align = true;
            s.parts.push(Part {
                path: path.to_string_lossy().into(),
                timing: crate::timing::Timing {
                    override_ms: Some(12.),
                    ..Default::default()
                },
                ..Default::default()
            });
            s.order.push(0);
        }
        let began = Instant::now();
        loop {
            Load.run(&p);
            if p.selection.read_unpoisoned().parts[0].timing.measured(
                &path.to_string_lossy(),
                0,
                "",
            ) {
                break;
            }
            assert!(
                began.elapsed().as_secs() < 15,
                "auto-align never measured a loaded part"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let s = p.selection.read_unpoisoned();
        let t = &s.parts[0].timing;
        assert_eq!(t.override_ms, Some(12.));
        assert!(
            t.loaded.first.iter().all(Option::is_some),
            "all velocity buckets were actually rendered"
        );
        let restored = Selection::deserialize(&s.serialize()).unwrap();
        assert_eq!(restored.parts[0].timing, *t);
        drop(s);
        drop(p);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod exact_packet_tests;

#[cfg(test)]
mod transport_event_tests;

#[cfg(test)]
mod uvi_registry_part_tests {
    use super::*;
    /// F-01: a commit holds `view` while it writes `scripts`; the UVI refresh (host save,
    /// loader tick) must never wait for `view` while holding `scripts`.
    #[test]
    fn uvi_refresh_does_not_hold_scripts_while_waiting_for_view() {
        use sampler_uvi::scripted::ScriptThread;
        use std::time::Duration;
        let xml="<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[Knob('K',0.25,0,1)]]></script></ScriptProcessor></EventProcessors></Program></UVI4>";
        let (_thread,loaded)=ScriptThread::spawn(xml.into(),(),Default::default()).unwrap();
        let params=SamplerParams::new();params.shared.ensure_parts(1);
        params.selection.write_unpoisoned().parts=vec![Part{path:"order.uvip".into(),..Default::default()}];
        let atoms=params.shared.part(0).unwrap();atoms.generation.store(3,Ordering::Release);
        {
            let mut scripts=atoms.scripts.lock_unpoisoned();
            scripts.uvi=Some(loaded.ui.clone());
            scripts.uvi_source=Some(params.selection.read_unpoisoned().parts[0].source());
            scripts.uvi_revision=loaded.ui.revision().wrapping_add(1);
        }
        params.shared.view.lock_unpoisoned().parts[0].generation=3;
        std::thread::scope(|scope|{
            let commit=params.shared.view.lock_unpoisoned();
            let refresh=scope.spawn(||params.shared.refresh_uvi(&params));
            std::thread::sleep(Duration::from_millis(150));
            assert!(!refresh.is_finished(),"refresh waits for the view");
            assert!(atoms.scripts.try_lock().is_ok(),"commit's next step (scripts) must not be blocked by refresh");
            drop(commit);
            refresh.join().unwrap();
        });
        assert!(params.shared.view.lock_unpoisoned().parts[0].uvi_snapshot.is_some());
    }

    #[test]
    fn uvi_dynamic_registry_joins_part_display_save_epoch_and_drawn_owner(){
        use sampler_uvi::scripted::ScriptThread;
        use std::time::{Duration,Instant};
        let xml="<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[local create=Button('Create');create.changed=function() if made then return end;made=true;Knob('Dynamic',0.25,0,1) end]]></script></ScriptProcessor></EventProcessors></Program></UVI4>";
        let (_thread,loaded)=ScriptThread::spawn(xml.into(),(),Default::default()).unwrap();
        let params=SamplerParams::new();params.shared.ensure_parts(1);
        params.selection.write_unpoisoned().parts=vec![Part{path:"registry.uvip".into(),..Default::default()}];
        let actual=params.shared.part(0).unwrap();actual.generation.store(17,Ordering::Release);
        let initial=loaded.ui.snapshot();let sampler_ui_ir::Binding::Control(create)=initial.interface().widgets[0].binding else{panic!("real Button")};
        {
            let mut scripts=actual.scripts.lock_unpoisoned();scripts.uvi=Some(loaded.ui.clone());scripts.uvi_source=Some(params.selection.read_unpoisoned().parts[0].source());
            let mut view=params.shared.view.lock_unpoisoned();view.parts[0].interfaces=vec![initial.interface().as_ref().clone()].into();view.parts[0].generation=17;
        }
        *actual.controls.lock_unpoisoned()=vec![ControlCell::new(create,0.)].into();
        assert!(params.shared.set_uvi_control_at(0,17,&initial,create,1.));
        let deadline=Instant::now()+Duration::from_secs(3);
        let dynamic=loop{
            let face=loaded.ui.interface();if let Some(widget)=face.widgets.iter().find(|w|w.name=="Dynamic") {let sampler_ui_ir::Binding::Control(id)=widget.binding else{panic!("real dynamic Knob")};break id;}
            assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(2));
        };
        params.shared.refresh_uvi(&params);
        let drawn=params.shared.view.lock_unpoisoned().parts[0].uvi_snapshot.clone().unwrap();
        assert!(loaded.ui.owns_snapshot(&drawn));assert!(actual.display_values().contains(&(dynamic,0.25)));
        assert!(actual.control_values().contains(&(dynamic,0.25)),"save must include actual dynamic roster");
        assert!(!params.shared.set_uvi_control_at(0,16,&drawn,dynamic,0.75));
        assert!(!params.shared.set_uvi_control_at(0,17,&initial,dynamic,0.75));
        let (_other_thread,other)=ScriptThread::spawn(xml.into(),(),Default::default()).unwrap();
        actual.scripts.lock_unpoisoned().uvi=Some(other.ui.clone());
        assert!(!params.shared.set_uvi_control_at(0,17,&drawn,dynamic,0.75),"same Part epoch cannot substitute a different real VM bridge");
        assert!(!actual.control_values().iter().any(|(id,_)|*id==dynamic),"retired original cell is not saved through the old static mirror");
    }
}


#[cfg(all(test,feature="shots"))]
mod uvi_gesture_baseline_tests {
    use super::*;
    use crate::ui::tests::Harness;
    use moose::mui::mui::prelude::{Buttons,Input,Mods,Point,PointerInput};
    use sampler_uvi::scripted::{DiagnosticPoll,UiBridge};
    use std::{sync::Arc,time::{Duration,Instant}};
    struct Fixture {
        _directory:tempfile::TempDir,params:Arc<SamplerParams>,core:V2Core,
        bridge:Arc<UiBridge>,editor:Harness,point:Point,down:bool,mods:Mods,unity:Vec<f32>,
    }
    impl Fixture {
        fn new(on_down:bool)->Self {
            let scratch=Path::new(env!("CARGO_MANIFEST_DIR")).join("artifacts/ui-test-scratch");
            std::fs::create_dir_all(&scratch).unwrap();
            let directory=tempfile::Builder::new().prefix("uvi-gesture-").tempdir_in(&scratch).unwrap();
            let mut writer=hound::WavWriter::create(directory.path().join("held.wav"),hound::WavSpec{channels:1,sample_rate:48000,bits_per_sample:16,sample_format:hound::SampleFormat::Int}).unwrap();
            for _ in 0..32768 {writer.write_sample(16384i16).unwrap();}writer.finalize().unwrap();
            let script=format!(r#"
              makePerformanceView();setSize(240,120)
              local button=Button{{'Gesture',bounds={{10,10,100,30}}}}
              button.triggeredOnMouseDown={}
              local status=Label{{'Status',bounds={{10,65,220,25}}}};status.text='0|idle'
              local calls=0
              button.changed=function(self,mods)
                calls=calls+1
                status.text=tostring(calls)..'|'..(mods and (tostring(mods.shiftDown)..'|'..tostring(mods.commandDown)..'|'..tostring(mods.altDown)) or 'nil')
                Program:setParameter('Gain',mods and mods.shiftDown and 0.25 or 0.75)
              end
            "#,if on_down{"true"}else{"false"});
            let xml=format!("<UVI4><Program Name='Actual gesture fixture' Gain='1'><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors><Layers><Layer Name='Tone' Gain='1'><Keygroups><Keygroup Name='Held' LowKey='60' HighKey='60' Gain='1'><Oscillators><SamplePlayer SamplePath='held.wav' BaseNote='60'/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>");
            let preset=directory.path().join("gesture.uvip");std::fs::write(&preset,xml).unwrap();
            let params=Arc::new(SamplerParams::new());
            {let mut selection=params.selection.write_unpoisoned();selection.parts=vec![Part{path:preset.display().to_string(),streaming:Some(Streaming::RamOnly),view:3,..Default::default()}];selection.order=vec![0];}
            // Same public normal loader/ready/install path as production, not
            // an inspector script attachment or fabricated control registration.
            let deadline=Instant::now()+Duration::from_secs(5);
            loop {Load.run(&params);if params.shared.view.lock_unpoisoned().parts[0].tree.is_some(){break;}assert!(Instant::now()<deadline,"normal authored source failed to load");std::thread::sleep(Duration::from_millis(2));}
            let actual=params.shared.part(0).unwrap();let bridge=actual.scripts.lock_unpoisoned().uvi.clone().expect("actual normal UVI owner");
            let (slot,epoch,part)=params.shared.ready.pop().expect("normal loader produced actual Part");assert_eq!(slot,0);assert!(part.is_some());
            let rate=f64::from_bits(params.shared.rate.load(Ordering::Acquire));let mut core=V2Core::with_parts(1,rate);core.install(slot,part);core.set_mix(&mix(&params.selection.read_unpoisoned()));
            core.play(0,CoreEvent::midi1(0x90,60,100));
            for _ in 0..4 {let _=core.render(64);}
            let unity=core.render(64).buses[0][0][..64].to_vec();assert!(unity.iter().any(|sample|sample.abs()>0.01),"normal held source must sound before gesture");
            assert_eq!(core.runtime_stats(0).unwrap().voices,1);
            let editor=Harness::new(&params,1180.,780.);let widget=bridge.interface().widgets.iter().position(|widget|widget.name=="Gesture").expect("actual authored Gesture declaration");let id=format!("part-0-epoch-{epoch}-script-0-ir-{widget}");
            let bounds=editor.ui.scene().unwrap().surface(&id).unwrap_or_else(||panic!("actual authored button not drawn: {id}")).frame;
            let point=Point::new(bounds.x+bounds.size.width/2.,bounds.y+bounds.size.height/2.);
            Self{_directory:directory,params,core,bridge,editor,point,down:false,mods:Mods{shift:true,ctrl:true,alt:true,cmd:true},unity}
        }
        fn tick(&mut self) {
            self.editor.tick(Input{pointer:PointerInput{pos:Some(self.point),buttons:if self.down{Buttons::PRIMARY}else{Buttons::default()},mods:self.mods,..Default::default()},..Default::default()});
            // Zero PCM service drains the REAL normal Driver queue without
            // advancing the held source or manufacturing a rendered sample.
            let _=self.core.render(0);service_current_ui(&self.params);self.params.shared.refresh_uvi(&self.params);
            std::thread::sleep(Duration::from_millis(1));
        }
        fn status(&self)->String {self.bridge.interface().widgets.iter().find(|widget|widget.name=="Status").unwrap().text.clone()}
        fn first_callback(&mut self)->String {
            let deadline=Instant::now()+Duration::from_secs(3);
            loop {self.tick();let status=self.status();if status.starts_with("1|"){return status;}assert!(Instant::now()<deadline,"documented activation boundary reached but real changed callback did not execute; status={status}");}
        }
        fn drain_actual_owner(&mut self) {
            let mut receipt=self.bridge.request_diagnostics(Duration::from_secs(3)).unwrap();let deadline=Instant::now()+Duration::from_secs(3);let mut complete=false;
            loop {self.tick();match receipt.poll(){DiagnosticPoll::Pending=>{},DiagnosticPoll::Complete(_)=>complete=true,other=>panic!("actual owner diagnostic failed: {other:?}")};if complete&&receipt.commands_drained(){break;}assert!(Instant::now()<deadline,"real ordinary Driver did not pass the ordered command marker");}
        }
        fn assert_quarter_gain(&mut self){
            self.drain_actual_owner();let rendered=self.core.render(64);let actual=&rendered.buses[0][0][..64];
            for (sample,unity) in actual.iter().zip(&self.unity){assert!((f64::from(*sample)-f64::from(*unity)*0.25).abs()<2e-5,"gesture callback must set actual held PCM quarter gain: got{sample}, unity{unity}");}
            let stats=self.core.runtime_stats(0).unwrap();assert_eq!(stats.voices,1);assert_eq!(stats.nonfinite_frames,0);assert_eq!(stats.voice_pool_refusals,0);
        }
    }
    #[test]
    fn normal_uvi_pointer_modifiers_reach_release_callback_and_held_pcm(){
        let mut fixture=Fixture::new(false);fixture.down=true;for _ in 0..8 {fixture.tick();}
        assert_eq!(fixture.status(),"0|idle","default activation must not fire on press/held");
        fixture.down=false;let status=fixture.first_callback();
        assert_eq!(status,"1|true|true|true","real pointer modifier booleans must reach the original callback, not nil or KSP bits");
        fixture.assert_quarter_gain();
    }
    #[test]
    fn normal_uvi_trigger_on_mouse_down_changes_held_pcm_before_release_once(){
        let mut fixture=Fixture::new(true);fixture.down=true;for _ in 0..4 {fixture.tick();}fixture.drain_actual_owner();
        assert_eq!(fixture.status(),"1|true|true|true","documented press activation must run before physical release after the actual owner marker");fixture.assert_quarter_gain();
        for _ in 0..20 {fixture.tick();}assert_eq!(fixture.status(),"1|true|true|true","held frames must not retrigger");
        fixture.down=false;for _ in 0..8 {fixture.tick();}fixture.drain_actual_owner();
        assert_eq!(fixture.status(),"1|true|true|true","release must not duplicate a press activation");
    }
}
#[cfg(all(test,feature="shots"))]
mod uvi_gesture_policy_tests {
    use super::*;
    use crate::ui::tests::Harness;
    use moose::mui::mui::prelude::{Buttons,Input,Mods,Point,PointerInput};
    use sampler_uvi::scripted::{DiagnosticPoll,UiBridge};
    use std::{sync::Arc,time::{Duration,Instant}};
    struct Fixture {
        _directory:tempfile::TempDir,params:Arc<SamplerParams>,core:V2Core,
        bridge:Arc<UiBridge>,editor:Harness,point:Point,down:bool,mods:Mods,unity:Vec<f32>,id:String,width:f64,
    }
    impl Fixture {
        fn new(on_down:bool)->Self {
            let scratch=Path::new(env!("CARGO_MANIFEST_DIR")).join("artifacts/ui-test-scratch");
            std::fs::create_dir_all(&scratch).unwrap();
            let directory=tempfile::Builder::new().prefix("uvi-gesture-").tempdir_in(&scratch).unwrap();
            let mut writer=hound::WavWriter::create(directory.path().join("held.wav"),hound::WavSpec{channels:1,sample_rate:48000,bits_per_sample:16,sample_format:hound::SampleFormat::Int}).unwrap();
            for _ in 0..32768 {writer.write_sample(16384i16).unwrap();}writer.finalize().unwrap();
            let script=format!(r#"
              makePerformanceView();setSize(240,120)
              local button=Button{{'Gesture',bounds={{10,10,100,30}}}}
              button.triggeredOnMouseDown={}
              local status=Label{{'Status',bounds={{10,65,220,25}}}};status.text='0|idle'
              local calls=0
              button.changed=function(self,mods)
                calls=calls+1
                self.triggeredOnMouseDown=false;self.width=130
                status.text=tostring(calls)..'|'..(mods and (tostring(mods.shiftDown)..'|'..tostring(mods.commandDown)..'|'..tostring(mods.altDown)) or 'nil')
                Program:setParameter('Gain',mods and mods.shiftDown and 0.25 or 0.75)
              end
              function onController(e) if e.controller==3 then button.triggeredOnMouseDown=true;button.width=130 end end
            "#,if on_down{"true"}else{"false"});
            let xml=format!("<UVI4><Program Name='Actual gesture fixture' Gain='1'><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors><Layers><Layer Name='Tone' Gain='1'><Keygroups><Keygroup Name='Held' LowKey='60' HighKey='60' Gain='1'><Oscillators><SamplePlayer SamplePath='held.wav' BaseNote='60'/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>");
            let preset=directory.path().join("gesture.uvip");std::fs::write(&preset,xml).unwrap();
            let params=Arc::new(SamplerParams::new());
            {let mut selection=params.selection.write_unpoisoned();selection.parts=vec![Part{path:preset.display().to_string(),streaming:Some(Streaming::RamOnly),view:3,..Default::default()}];selection.order=vec![0];}
            // Same public normal loader/ready/install path as production, not
            // an inspector script attachment or fabricated control registration.
            let deadline=Instant::now()+Duration::from_secs(5);
            loop {Load.run(&params);if params.shared.view.lock_unpoisoned().parts[0].tree.is_some(){break;}assert!(Instant::now()<deadline,"normal authored source failed to load");std::thread::sleep(Duration::from_millis(2));}
            let actual=params.shared.part(0).unwrap();let bridge=actual.scripts.lock_unpoisoned().uvi.clone().expect("actual normal UVI owner");
            let (slot,epoch,part)=params.shared.ready.pop().expect("normal loader produced actual Part");assert_eq!(slot,0);assert!(part.is_some());
            let rate=f64::from_bits(params.shared.rate.load(Ordering::Acquire));let mut core=V2Core::with_parts(1,rate);core.install(slot,part);core.set_mix(&mix(&params.selection.read_unpoisoned()));
            core.play(0,CoreEvent::midi1(0x90,60,100));
            for _ in 0..4 {let _=core.render(64);}
            let unity=core.render(64).buses[0][0][..64].to_vec();assert!(unity.iter().any(|sample|sample.abs()>0.01),"normal held source must sound before gesture");
            assert_eq!(core.runtime_stats(0).unwrap().voices,1);
            let editor=Harness::new(&params,1180.,780.);let widget=bridge.interface().widgets.iter().position(|widget|widget.name=="Gesture").expect("actual authored Gesture declaration");let id=format!("part-0-epoch-{epoch}-script-0-ir-{widget}");
            let bounds=editor.ui.scene().unwrap().surface(&id).unwrap_or_else(||panic!("actual authored button not drawn: {id}")).frame;
            let point=Point::new(bounds.x+bounds.size.width/2.,bounds.y+bounds.size.height/2.);
            Self{_directory:directory,params,core,bridge,editor,point,down:false,mods:Mods{shift:true,ctrl:true,alt:true,cmd:true},unity,id,width:bounds.size.width}
        }
        fn tick(&mut self) {
            self.editor.tick(Input{pointer:PointerInput{pos:Some(self.point),buttons:if self.down{Buttons::PRIMARY}else{Buttons::default()},mods:self.mods,..Default::default()},..Default::default()});
            // Zero PCM service drains the REAL normal Driver queue without
            // advancing the held source or manufacturing a rendered sample.
            let _=self.core.render(0);service_current_ui(&self.params);self.params.shared.refresh_uvi(&self.params);
            std::thread::sleep(Duration::from_millis(1));
        }
        fn status(&self)->String {self.bridge.interface().widgets.iter().find(|widget|widget.name=="Status").unwrap().text.clone()}
        fn first_callback(&mut self)->String {
            let deadline=Instant::now()+Duration::from_secs(3);
            loop {self.tick();let status=self.status();if status.starts_with("1|"){return status;}assert!(Instant::now()<deadline,"documented activation boundary reached but real changed callback did not execute; status={status}");}
        }
        fn drain_actual_owner(&mut self) {
            let mut receipt=self.bridge.request_diagnostics(Duration::from_secs(3)).unwrap();let deadline=Instant::now()+Duration::from_secs(3);let mut complete=false;
            loop {self.tick();match receipt.poll(){DiagnosticPoll::Pending=>{},DiagnosticPoll::Complete(_)=>complete=true,other=>panic!("actual owner diagnostic failed: {other:?}")};if complete&&receipt.commands_drained(){break;}assert!(Instant::now()<deadline,"real ordinary Driver did not pass the ordered command marker");}
        }
        fn assert_quarter_gain(&mut self){
            self.drain_actual_owner();let rendered=self.core.render(64);let actual=&rendered.buses[0][0][..64];
            for (sample,unity) in actual.iter().zip(&self.unity){assert!((f64::from(*sample)-f64::from(*unity)*0.25).abs()<2e-5,"gesture callback must set actual held PCM quarter gain: got{sample}, unity{unity}");}
            let stats=self.core.runtime_stats(0).unwrap();assert_eq!(stats.voices,1);assert_eq!(stats.nonfinite_frames,0);assert_eq!(stats.voice_pool_refusals,0);
        }
    }

    impl Fixture {
        fn adopted_phase(&mut self,on_press:bool){
            let deadline=Instant::now()+Duration::from_secs(3);
            loop{
                self.tick();
                let owner=self.bridge.interface();let button=owner.widgets.iter().find(|widget|widget.name=="Gesture").unwrap();
                let drawn=self.editor.ui.scene().unwrap().surface(&self.id).unwrap();
                if button.activate_on_press==on_press&&drawn.frame.size.width>self.width*1.2{break;}
                assert!(Instant::now()<deadline,"actual owner/new schema AND widened rendered Face were not adopted");
            }
            self.drain_actual_owner();
        }
    }
    #[test]
    fn normal_uvi_press_callback_phase_change_cannot_replay_its_release(){
        let mut f=Fixture::new(true);let original=f.bridge.snapshot();f.down=true;
        assert_eq!(f.first_callback(),"1|true|true|true","real press callback must complete through the normal owner");
        f.adopted_phase(false);assert!(!original.same_schema(&f.bridge.snapshot()),"actual phase schema changed while physically held");
        for _ in 0..8{f.tick();}assert_eq!(f.status(),"1|true|true|true");f.assert_quarter_gain();
        f.down=false;for _ in 0..8{f.tick();}f.drain_actual_owner();
        assert_eq!(f.status(),"1|true|true|true","a consumed physical press must stay consumed through current release-mode Face adoption");f.assert_quarter_gain();
        f.down=true;for _ in 0..8{f.tick();}assert_eq!(f.status(),"1|true|true|true","fresh release-mode click waits for release");
        f.down=false;for _ in 0..8{f.tick();}f.drain_actual_owner();assert_eq!(f.status(),"2|true|true|true","the next genuine click remains usable exactly once");f.assert_quarter_gain();
    }
    #[test]
    fn normal_uvi_release_to_press_change_cancels_held_gesture_without_synthetic_down(){
        let mut f=Fixture::new(false);f.down=true;for _ in 0..8{f.tick();}assert_eq!(f.status(),"0|idle");
        f.core.play(0,CoreEvent::midi1(0xb0,3,127));let _=f.core.render(0);f.adopted_phase(true);
        for _ in 0..8{f.tick();}assert_eq!(f.status(),"0|idle","a policy change cannot manufacture a missing pointer-down");
        f.down=false;for _ in 0..8{f.tick();}f.drain_actual_owner();assert_eq!(f.status(),"0|idle","changed-policy held gesture is canceled");
        f.down=true;for _ in 0..8{f.tick();}f.drain_actual_owner();assert_eq!(f.status(),"1|true|true|true");
        f.down=false;for _ in 0..8{f.tick();}f.drain_actual_owner();assert_eq!(f.status(),"1|true|true|true");f.assert_quarter_gain();
    }
}
