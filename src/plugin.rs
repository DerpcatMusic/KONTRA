//! The plugin: persisted rack state, the loader task and the audio callback,
//! composed around the v2 sound core ([`crate::sound::v2`]).
//!
//! Threads: the host's audio thread runs [`Sampler::process`] and talks to
//! [`V2Core`] only; the serialized [`Load`] task prepares parts with
//! [`V2Loader`], routes outputs ([`routing`]) and hands parts, mixes and
//! larger storage to the audio thread through lock-free queues. Everything the
//! audio thread replaces goes back to the loader to be dropped.

use crate::sound::{
    BUSES, BlockInfo, Core, CoreError, CoreLoader, LoadRequest, MAX_BLOCK, Progress, RACK_SLOTS, Rendered, TUNE_RANGE,
    Transport,
    event::{Event as CoreEvent, HostNote, HostPattern, NoteExpression},
    mix::{BusControls, Mix, NO_AUX, PartControls, db_gain},
    report::{LoadReport, RuntimeProblems},
    tree::{MixTree, NodeMix},
    v2::{Part as CorePart, Retired as CoreRetired, V2Core, V2Loader},
};
use crate::{library, routing};
use crossbeam_queue::ArrayQueue;
use moose::core::{ExactAddress, ExactEvent, ExactEventBody, ExactEventRef, ExactNoteAddress, ExactNoteKind, LosslessEventRef};
use moose::mui::mui::scene::Image;
use moose::prelude::*;
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
    /// Pitch-bend range in semitones each way; 0 keeps the instrument's own.
    pub bend_range: u8,
}

impl Default for Part {
    fn default() -> Self {
        Self {
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
            height: 0.,
            aux: -1,
            aux_gain: 0.,
            output_manual: false,
            nodes: Vec::new(),
            mpe: false,
            bend_range: 0,
        }
    }
}

impl Part {
    /// What the loader prepares for this part.
    fn source(&self) -> (String, u32) {
        (self.path.clone(), self.program)
    }
}

impl StateField for NodeMix {
    fn write_field(&self, buf: &mut Vec<u8>) {
        serde_json::to_string(self).unwrap_or_default().write_field(buf);
    }
    fn read_field(cursor: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        Some(serde_json::from_str(&String::read_field(cursor)?).unwrap_or_default())
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
        Self { name: String::new(), gain: 0., pan: 0., mute: false, solo: false, port: -1 }
    }
}

impl Bus {
    /// The name the mixer shows for bus `n`.
    pub fn label(&self, n: usize) -> String {
        if self.name.is_empty() { format!("st.{}", n + 1) } else { self.name.clone() }
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
}

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
            (self.parts.iter()).any(|p| !p.path.is_empty() && p.port == port && p.channel == channel)
        };
        (0..4u8)
            .flat_map(|port| (0..16i16).map(move |channel| (port, channel)))
            .find(|&(port, channel)| !taken(port, channel))
            .unwrap_or((0, -1))
    }
}

#[derive(Params)]
#[params(output_port_name = "port_name", output_port_names_revision = "port_names_revision")]
pub struct SamplerParams {
    #[param(name = "Volume", range = "linear(-60, 6)", default = -12.0, unit = "dB", smooth = "exp(5)")]
    pub volume: FloatParam,
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
    /// Host output port `index`'s name, as last published (`routing.rs`).
    fn port_name(&self, index: u32) -> Option<String> {
        let names = self.shared.port_names.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        names.published.get(index as usize).cloned()
    }

    fn port_names_revision(&self) -> u64 {
        self.shared.port_names_revision.load(Ordering::Acquire)
    }

    /// Support context: call on an export worker, never paint or process.
    pub(crate) fn diagnostic_report(&self) -> serde_json::Value {
        let selection = self.selection.read().unwrap().clone();
        self.shared.ensure_parts(selection.parts.len());
        let view = self.shared.view.lock().unwrap();
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
            "host": {"sample_rate": self.shared.rate()},
            "rack": {"parts": parts, "buses": selection.buses, "outputs": selection.outputs,
                "midi_thru": selection.midi_thru},
            "unsupported_host_input": self.shared.unsupported_input.load(Ordering::Relaxed),
            "host_note_end_rejections": self.shared.end_rejections.load(Ordering::Relaxed),
            "keyboard": {"heard": self.shared.heard.iter().map(|v| v.load(Ordering::Relaxed)).collect::<Vec<_>>(),
                "played": self.shared.played.iter().map(|v| v.load(Ordering::Relaxed)).collect::<Vec<_>>()},
        });
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
    pub(crate) generation: AtomicU64,
    /// Out of [`Progress::DONE`], rising within each load.
    pub(crate) load_progress: AtomicU32,
    pub(crate) meter: [AtomicU32; 2],
    pub(crate) clip: AtomicBool,
    /// [`RuntimeProblems`] field by field, as the audio thread last saw them.
    problems: [AtomicU64; 6],
    /// The loaded part's controls; the audio thread refreshes their values.
    pub(crate) controls: Mutex<Arc<[ControlCell]>>,
    /// Per node of the loaded part's tree, its level like [`Self::meter`]
    /// (node 0, the instrument, is the part's own meter).
    pub(crate) node_meters: Mutex<Arc<[[AtomicU32; 2]]>>,
}

/// One control's value as last seen (`f64` bits).
pub(crate) struct ControlCell {
    pub(crate) id: sampler_ui_ir::ControlId,
    value: AtomicU64,
}

impl ControlCell {
    pub(crate) fn value(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::Relaxed))
    }

    fn set(&self, value: f64) {
        self.value.store(value.to_bits(), Ordering::Relaxed);
    }
}

impl PartShared {
    /// Tree node `node`'s level, silent when the part has no such node.
    pub(crate) fn node_level(&self, node: usize) -> [f32; 2] {
        if node == 0 {
            return Meters::read(&self.meter);
        }
        self.node_meters.lock().unwrap().get(node).map_or([0.0; 2], Meters::read)
    }

    /// The part's controls and their values, in id order.
    pub(crate) fn control_values(&self) -> Vec<(sampler_ui_ir::ControlId, f64)> {
        self.controls.lock().unwrap().iter().map(|c| (c.id, c.value())).collect()
    }

    /// Audio thread: copy the core's values in, unless the loader holds the lock.
    fn refresh_controls(&self, value: impl Fn(sampler_ui_ir::ControlId) -> Option<f64>) {
        if let Ok(cells) = self.controls.try_lock() {
            for cell in cells.iter() {
                if let Some(v) = value(cell.id) {
                    cell.set(v);
                }
            }
        }
    }

    pub(crate) fn problems(&self) -> RuntimeProblems {
        let [a, b, c, d, e, f] = self.problems.each_ref().map(|x| x.load(Ordering::Relaxed));
        RuntimeProblems {
            capacity_drops: a,
            underruns: b,
            nonfinite: c,
            script_overruns: d,
            narrowed_input: e,
            ignored_input: f,
        }
    }

    fn store_problems(&self, p: RuntimeProblems) {
        let values =
            [p.capacity_drops, p.underruns, p.nonfinite, p.script_overruns, p.narrowed_input, p.ignored_input];
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
    instance_id: u64,
    ready: ArrayQueue<Ready>,
    pending_ready: Mutex<std::collections::VecDeque<Ready>>,
    discard: ArrayQueue<Retired>,
    /// Host sample rate (`f64` bits) parts are prepared for.
    pub(crate) rate: AtomicU64,
    pub(crate) key_owners: [AtomicU64; 128],
    /// The velocity each key sounds at, 0 when silent: `played` on screen
    /// or from the computer keyboard, `heard` from the host's MIDI.
    pub(crate) played: [AtomicU8; 128],
    pub(crate) heard: [AtomicU8; 128],
    /// What the on-screen keyboard and wheels play, by rack slot.
    pub(crate) keyboard: ArrayQueue<(usize, Play)>,
    /// The pitch wheel (0..=16383, centre 8192) and mod wheel (0..=127) as
    /// last moved, on screen or by incoming MIDI.
    pub(crate) bend: AtomicU32,
    pub(crate) modulation: AtomicU32,
    pub(crate) controls: ArrayQueue<Mix>,
    /// Widget edits for the audio thread: rack slot, control, value.
    control_edits: ArrayQueue<(usize, sampler_ui_ir::ControlId, f64)>,
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
    /// The app's library folders and the scan of them.
    pub(crate) libraries: library::Scanner,
    /// Dialog and file-operation workers retire at plugin teardown, not GUI close.
    #[cfg(all(feature = "plugin", target_os = "linux"))]
    pub(crate) dialog_runtime: Arc<crate::ui::picker::Runtime>,
    pub(crate) view: Mutex<View>,
    /// Voices sounding across the rack, reported by the audio thread.
    pub(crate) voices: AtomicU64,
    pub(crate) audible: AtomicU64,
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
    // Last: workers retire before the logging worker.
    _diagnostics: crate::diagnostics::DiagnosticLease,
    // The crash marker retires after every retained worker and diagnostics lease.
    _crash_session: Mutex<Option<crate::support::CrashSessionGuard>>,
}

#[derive(Default, Clone)]
pub(crate) struct PartView {
    pub(crate) program: u32,
    /// The source and sample rate (bits) last prepared or being prepared.
    pub(crate) attempted: Option<(String, u32, u64)>,
    pub(crate) status: String,
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
    /// The load's log record ([`crate::diagnostics::LoadTrace`]).
    pub(crate) trace: Option<Arc<serde_json::Value>>,
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
        let initial_parts: [Arc<PartShared>; RACK_SLOTS] = std::array::from_fn(|_| Arc::default());
        let shared = Self {
            _crash_session: Mutex::new(crash_session),
            instance_id: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed),
            ready: ArrayQueue::new(64),
            pending_ready: Mutex::default(),
            discard: ArrayQueue::new(64),
            rate: AtomicU64::new(48000f64.to_bits()),
            key_owners: std::array::from_fn(|_| AtomicU64::new(0)),
            played: std::array::from_fn(|_| AtomicU8::new(0)),
            heard: std::array::from_fn(|_| AtomicU8::new(0)),
            keyboard: ArrayQueue::new(256),
            bend: AtomicU32::new(8192),
            modulation: AtomicU32::new(0),
            controls: ArrayQueue::new(1),
            control_edits: ArrayQueue::new(256),
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
            libraries: library::Scanner::default(),
            #[cfg(all(feature = "plugin", target_os = "linux"))]
            dialog_runtime: Arc::default(),
            voices: AtomicU64::new(0),
            audible: AtomicU64::new(0),
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
        if let Some(guard) = shared._crash_session.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut() {
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
            *o = f32::from_bits(self.samples[start.wrapping_add(i) % SCOPE].load(Ordering::Relaxed));
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
        meter.each_ref().map(|m| f32::from_bits(m.load(Ordering::Relaxed)))
    }

    /// Hold `peak` or let the shown level fall by `fall`; light `clip` at 0 dBFS.
    fn publish(meter: &[AtomicU32; 2], peak: [f32; 2], fall: f32, clip: &AtomicBool) {
        if peak[0] >= 1.0 || peak[1] >= 1.0 {
            clip.store(true, Ordering::Relaxed);
        }
        for (m, peak) in meter.iter().zip(peak) {
            let shown = f32::from_bits(m.load(Ordering::Relaxed)) * fall;
            let level = if peak >= shown { peak } else { shown };
            let level = if level < 1e-4 || !level.is_finite() { 0. } else { level };
            m.store(level.to_bits(), Ordering::Relaxed);
        }
    }
}

fn finite_or(x: f32, range: f32) -> f32 {
    if x.is_finite() { x.clamp(-range, range) } else { 0. }
}

/// What the audio thread mixes by, from the persisted rack.
pub(crate) fn mix(selection: &Selection) -> Mix {
    let slots = selection.parts.len().max(RACK_SLOTS);
    Mix {
        parts: rack_controls(selection),
        buses: std::array::from_fn(|n| {
            let b = selection.bus(n);
            BusControls {
                gain: db_gain(b.gain),
                pan: finite_or(b.pan, 1.),
                mute: b.mute,
                solo: b.solo,
                port: if (0..BUSES as i16).contains(&b.port) { b.port as u8 } else { n as u8 },
            }
        }),
        nodes: (0..slots).map(|n| selection.parts.get(n).map_or_else(Vec::new, |p| p.nodes.clone())).collect(),
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
                    aux: if (0..BUSES as i16).contains(&p.aux) { p.aux as u8 } else { NO_AUX },
                    aux_gain: db_gain(p.aux_gain),
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
    let mut routed = params.selection.read().unwrap().clone();
    let before = routed.clone();
    routing::apply(&mut routed, &trees);
    let names = {
        let mut current = params.selection.write().unwrap();
        // Changed meanwhile: the next pass routes the new selection.
        if routed != before && *current == before {
            *current = routed;
        }
        routing::port_names(&current, &trees)
    };
    let mut ports = shared.port_names.lock().unwrap();
    if ports.offer(names, Instant::now()) {
        shared.port_names_revision.fetch_add(1, Ordering::Release);
    }
}

impl Shared {
    /// Editor/loader only. Audio reads its prepared registry without this lock.
    pub(crate) fn with_parts<T>(&self, read: impl FnOnce(&[Arc<PartShared>]) -> T) -> T {
        read(&self.parts.lock().unwrap())
    }

    pub(crate) fn part(&self, slot: usize) -> Option<Arc<PartShared>> {
        self.parts.lock().unwrap().get(slot).cloned()
    }

    /// Prepare metadata for newly appended/restored rows off the audio thread.
    pub(crate) fn ensure_parts(&self, count: usize) {
        let mut parts = self.parts.lock().unwrap();
        while parts.len() < count {
            parts.push(Arc::default());
        }
        drop(parts);
        let mut view = self.view.lock().unwrap();
        if view.parts.len() < count {
            view.parts.resize_with(count, PartView::default);
        }
    }

    /// Each slot's output tree, as last loaded.
    pub(crate) fn trees(&self) -> Vec<Option<Arc<MixTree>>> {
        let view = self.view.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        view.parts.iter().map(|v| v.tree.clone()).collect()
    }

    /// [`routing::apply`] with the parts' trees as loaded.
    pub(crate) fn reroute(&self, selection: &mut Selection) {
        routing::apply(selection, &self.trees());
    }

    /// Loader only. Keep different parts when the bounded queue is full; a
    /// newer part for a slot replaces one still pending.
    fn publish_part(&self, item: Ready) {
        let mut pending = self.pending_ready.lock().unwrap();
        pending.retain(|(slot, ..)| *slot != item.0);
        pending.push_back(item);
        self.flush_ready_inner(&mut pending);
    }

    fn flush_ready(&self) {
        self.flush_ready_inner(&mut self.pending_ready.lock().unwrap());
    }

    fn flush_ready_inner(&self, pending: &mut std::collections::VecDeque<Ready>) {
        while let Some(item) = pending.pop_front() {
            if self.part(item.0).is_none_or(|p| p.generation.load(Ordering::Acquire) != item.1) {
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
        let _ = self.growth.force_push(Box::new(Growth::new(parts, self.rate())));
        self.growth_prepared.store(count as u64, Ordering::Release);
    }

    pub(crate) fn rate(&self) -> f64 {
        f64::from_bits(self.rate.load(Ordering::Acquire))
    }

    /// Ask the loader to replace the rack with the multi at `path`.
    pub(crate) fn queue_multi(&self, path: String) {
        self.view.lock().unwrap().multi_status = "Loading multi…".into();
        *self.multi_request.lock().unwrap() = Some(path);
    }

    /// Start `note` on `slot` (or [`EVERY_PART`]) at `velocity` (1..=127) from the on-screen keyboard.
    /// A key already down (the mouse and a computer key on one note) is let
    /// go first: every note-on has its note-off, so one release stops it.
    pub(crate) fn press_key(&self, slot: usize, note: u8, velocity: u8) {
        self.release_key(note);
        let velocity = velocity.clamp(1, 127);
        self.key_owners[note as usize & 127].store(slot as u64, Ordering::Release);
        self.played[note as usize & 127].store(velocity, Ordering::Relaxed);
        if self.keyboard.push((slot, Play::Note(note, velocity))).is_err() {
            self.panic.store(true, Ordering::Release);
        }
    }

    /// Stop `note` on whichever slot the on-screen keyboard started it.
    pub(crate) fn release_key(&self, note: u8) {
        let was_down = self.played[note as usize & 127].swap(0, Ordering::AcqRel) != 0;
        let owner = self.key_owners[note as usize & 127].load(Ordering::Acquire);
        if was_down && self.keyboard.push((owner as usize, Play::Note(note, 0))).is_err() {
            self.panic.store(true, Ordering::Release);
        }
    }

    /// Edit a control of the part in `slot` as its widget would; the
    /// script's `on ui_control` runs on the audio thread. False when the
    /// queue is full.
    pub(crate) fn set_control(&self, slot: usize, control: sampler_ui_ir::ControlId, value: f64) -> bool {
        if let Some(part) = self.part(slot)
            && let Some(cell) = part.controls.lock().unwrap().iter().find(|c| c.id == control)
        {
            cell.set(value);
        }
        self.control_edits.push((slot, control, value)).is_ok()
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

/// A rack KONTRA 2 saved: a `.kontra2-multi` file, JSON, naming the
/// instruments with every part setting:
///
/// ```json
/// { "format": "kontra2-multi", "version": 1, "name": "Evening",
///   "parts": [ { "path": "/…/Piano.nki", "program": 0, "channel": -1,
///                "port": 0, "output": 0, "gain": 0.0, "nodes": [] } ] }
/// ```
///
/// Parts are in rack order; a missing field takes its default.
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
            version: 1,
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
        anyhow::ensure!(multi.format == library::MULTI, "Not a KONTRA 2 multi");
        anyhow::ensure!(multi.version <= 1, "Saved by a newer KONTRA");
        Ok(multi)
    }
}

/// The loader: serialized, polled by the audio thread about ten times a second.
pub struct Load;

impl BackgroundTask for Load {
    type Params = SamplerParams;
    const SERIALIZED: bool = true;
    fn run(self, params: &SamplerParams) {
        let shared = &params.shared;
        shared.flush_ready();
        // Dropping what the audio thread replaced is this thread's job.
        while shared.discard.pop().is_some() {}
        load_multi(params);
        let selection = params.selection.read().unwrap().clone();
        shared.ensure_parts(selection.parts.len());
        shared.prepare_growth();
        shared.midi_thru.store(selection.midi_thru, Ordering::Release);
        poll_libraries(shared);
        route(params);
        let push_mix = || {
            let _ = shared.controls.force_push(mix(&params.selection.read().unwrap()));
        };
        push_mix();
        let mut loaded = false;
        for slot in 0..shared.grown.load(Ordering::Acquire) as usize {
            loaded |= load_part(params, slot);
        }
        if loaded {
            // New trees: route their nodes and size their settings.
            route(params);
            push_mix();
        }
        refresh_problems(shared);
    }
}

/// Replace the rack with a requested multi.
fn load_multi(params: &SamplerParams) {
    let shared = &params.shared;
    let Some(path) = shared.multi_request.lock().unwrap().take() else { return };
    let before = params.selection.read().unwrap().clone();
    let result = SavedMulti::read(Path::new(&path)).map(|m| {
        let status = format!("{} · {} instruments", m.name, m.parts.len());
        (m.parts, status)
    });
    let status = match result {
        Ok((parts, status)) => {
            let mut current = params.selection.write().unwrap();
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
    shared.view.lock().unwrap().multi_status = status;
}

/// A finished library scan replaces what the browser lists.
fn poll_libraries(shared: &Shared) {
    let installed = shared.view.lock().unwrap().scanned;
    let Some((generation, scanned)) = shared.libraries.poll(installed) else { return };
    let mut view = shared.view.lock().unwrap();
    view.scanned = generation;
    match scanned {
        Some(scanned) => {
            view.status = format!("{} libraries · {} presets", scanned.shelf.libraries.len(), scanned.files.len());
            if let Some(imported) = &scanned.imported {
                view.status += &match imported.len() {
                    0 => " · nothing new from Kontakt".to_owned(),
                    n => format!(" · {n} folders from Kontakt"),
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
fn load_part(params: &SamplerParams, slot: usize) -> bool {
    let shared = &params.shared;
    let atoms = shared.part(slot).unwrap();
    let part = params.selection.read().unwrap().parts.get(slot).cloned().unwrap_or_default();
    let rate = shared.rate();
    let target = (part.path.clone(), part.program, rate.to_bits());
    {
        let mut view = shared.view.lock().unwrap();
        let v = &mut view.parts[slot];
        if v.attempted.as_ref() == Some(&target) {
            return false;
        }
        *v = PartView {
            program: part.program,
            attempted: Some(target),
            loading: !part.path.is_empty(),
            status: "Loading…".into(),
            ..Default::default()
        };
    }
    atoms.load_progress.store(0, Ordering::Relaxed);
    let generation = atoms.generation.fetch_add(1, Ordering::AcqRel) + 1;
    if part.path.is_empty() {
        shared.view.lock().unwrap().parts[slot].status.clear();
        shared.publish_part((slot, generation, None));
        return true;
    }
    let source = part.source();
    let canceled = || {
        params.selection.read().unwrap().parts.get(slot).is_none_or(|p| p.source() != source)
            || atoms.generation.load(Ordering::Acquire) != generation
            || shared.rate.load(Ordering::Acquire) != rate.to_bits()
    };
    let mut trace = crate::diagnostics::LoadTrace::new(Path::new(&part.path), part.program, Some(slot));
    trace.detail("instance_id", shared.instance_id);
    trace.detail("sample_rate", rate);
    trace.stage("prepare");
    let request = LoadRequest { path: part.path.clone().into(), program: part.program, sample_rate: rate };
    let mut progress = |p: Progress| atoms.load_progress.store(u32::from(p.0), Ordering::Relaxed);
    let result = V2Loader.prepare(&request, &mut progress, &canceled);
    let mut view = shared.view.lock().unwrap();
    let v = &mut view.parts[slot];
    v.loading = false;
    match result {
        Ok(loaded) => {
            for line in loaded.report.lines().skip(1) {
                trace.issue("translate", crate::diagnostics::code(&line), line);
            }
            let d = &loaded.report.decoded;
            trace.detail("zones", d.zones);
            trace.detail("groups", d.groups);
            trace.detail("samples", d.samples);
            trace.detail("scripts", d.scripts);
            let missing = loaded.report.missing.len();
            v.status = format!("{} · {} zones · {} groups", d.format, d.zones, d.groups);
            if missing > 0 {
                v.status += &format!(" · {missing} not translated");
            }
            v.active = loaded.report.name.clone();
            v.tree = Some(Arc::new(loaded.tree));
            v.report = Some(Arc::new(loaded.report));
            v.interfaces = loaded.interfaces.into();
            let nodes = v.tree.as_ref().map_or(1, |t| t.nodes.len());
            *atoms.node_meters.lock().unwrap() = (0..nodes).map(|_| Default::default()).collect();
            *atoms.controls.lock().unwrap() = loaded
                .controls
                .iter()
                .map(|&(id, value)| ControlCell { id, value: AtomicU64::new(value.to_bits()) })
                .collect();
            v.instrument = loaded.instrument;
            v.trace = Some(trace.finish(if missing > 0 { "partial" } else { "loaded" }));
            atoms.load_progress.store(u32::from(Progress::DONE.0), Ordering::Relaxed);
            drop(view);
            shared.publish_part((slot, generation, loaded.part));
            true
        }
        Err(CoreError::Canceled) => {
            // Load again on the next pass, whatever the part is then.
            v.attempted = None;
            v.status.clear();
            false
        }
        Err(e) => {
            v.status = format!("Load failed: {e}");
            trace.fail(e.to_string());
            v.trace = Some(trace.finish("failed"));
            drop(view);
            shared.publish_part((slot, generation, None));
            true
        }
    }
}

/// Copy the audio thread's problem counters into the parts' load reports.
fn refresh_problems(shared: &Shared) {
    let problems = shared.with_parts(|parts| parts.iter().map(|p| p.problems()).collect::<Vec<_>>());
    let mut view = shared.view.lock().unwrap();
    for (v, problems) in view.parts.iter_mut().zip(problems) {
        if let Some(report) = v.report.as_mut().filter(|r| r.runtime != problems) {
            Arc::make_mut(report).runtime = problems;
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

/// A CLAP or VST3 note event as core input; `None` leaves it to its typed fallback.
fn exact_host_input(exact: ExactEventRef<'_>) -> Option<ExactInput> {
    let note = |kind, address: ExactNoteAddress, velocity: f64, clap, tune: f64| {
        let Some(pattern) = host_pattern(address, clap) else { return ExactInput::Unsupported };
        match kind {
            ExactNoteKind::On => {
                let (Ok(port), Ok(channel), Ok(key)) =
                    (u8::try_from(pattern.port), u8::try_from(pattern.channel), u8::try_from(pattern.key))
                else {
                    return ExactInput::Unsupported;
                };
                if channel >= 16 || key >= 128 || !(0.0..=1.0).contains(&velocity) || !tune.is_finite() {
                    return ExactInput::Unsupported;
                }
                let note = HostNote { port, channel, key, id: pattern.id, clap };
                ExactInput::Routed(CoreEvent::NoteOn { note, velocity, tune }, port)
            }
            ExactNoteKind::Off => ExactInput::Routed(CoreEvent::NoteOff(pattern), 0),
            ExactNoteKind::Choke => ExactInput::Routed(CoreEvent::Choke(pattern), 0),
            _ => ExactInput::Unsupported,
        }
    };
    let expression = |address, clap, x: Option<NoteExpression>| match (host_pattern(address, clap), x) {
        (Some(pattern), Some(x)) => ExactInput::Routed(CoreEvent::Expression(pattern, x), 0),
        _ => ExactInput::Unsupported,
    };
    let unit = |v: f64| (0.0..=1.0).contains(&v).then_some(v);
    Some(match *exact.body() {
        ExactEventBody::Note { kind, address, velocity } => note(kind, address, velocity, true, 0.),
        // VST3 tuning is cents. Length is metadata: an explicit NoteOff ends the note.
        ExactEventBody::DetailedNote { kind, address, velocity, tuning, .. } => {
            note(kind, address, f64::from(velocity), false, f64::from(tuning) / 100.)
        }
        // CLAP: 0 volume (linear 0..=4), 1 pan (0..=1), 2 tuning (semitones),
        // 5 brightness, 6 pressure.
        ExactEventBody::NoteExpression { expression_id, address, value } => expression(
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
        ExactEventBody::NormalizedNoteExpression { expression_id, address, value } => expression(
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
        ExactEventBody::DetailedPolyPressure { address, pressure } => {
            expression(address, false, unit(f64::from(pressure)).map(NoteExpression::Pressure))
        }
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
            EventBody::NoteOn { .. } | EventBody::NoteOff { .. } | EventBody::PitchBend { .. } | EventBody::ControlChange { .. }
        )
    {
        let mut out = *e;
        out.port = 0;
        cx.output_events.push(out);
    }
}

/// A typed host MIDI event: shown on the keyboard and wheels, played as UMP.
fn feed_typed_input(s: &mut Dsp, p: &SamplerParams, e: &Event, cx: &mut ProcessContext, thru: bool) {
    relay_typed_input(e, cx, thru);
    let shared = &p.shared;
    let lit = |note: u8, velocity: u8| shared.heard[note as usize & 127].store(velocity, Ordering::Relaxed);
    match e.body {
        EventBody::NoteOn { note, velocity, .. } => lit(note, velocity.max(1)),
        EventBody::NoteOn2 { note, velocity, .. } => lit(note, ((velocity >> 9) as u8).max(1)),
        EventBody::NoteOff { note, .. } | EventBody::NoteOff2 { note, .. } => lit(note, 0),
        EventBody::ControlChange { cc: 120 | 123, .. } => (0..128).for_each(|note| lit(note, 0)),
        EventBody::ControlChange { cc: 1, value, .. } => shared.modulation.store(u32::from(value), Ordering::Relaxed),
        EventBody::PitchBend { value, .. } => shared.bend.store(u32::from(value), Ordering::Relaxed),
        _ => {}
    }
    let words = moose::core::ump::encode_ump_channel_voice_1(&e.body)
        .or_else(|| moose::core::ump::encode_ump_channel_voice_2(&e.body));
    match words {
        Some([a, b, ..]) => s.core.event(e.port, CoreEvent::Ump([a, b])),
        None => s.unsupported += 1,
    }
}

/// An exact host note event into the core; a refused or unrouted note-on
/// ends at once, so the host never waits on it.
fn feed_exact_input(s: &mut Dsp, p: &SamplerParams, event: CoreEvent, port: u8, offset: u32, cx: &mut ProcessContext) {
    let heard = &p.shared.heard;
    s.core.event(port, event);
    match event {
        CoreEvent::NoteOn { note, velocity, .. } => {
            heard[usize::from(note.key)].store(((velocity * 127.).round() as u8).max(1), Ordering::Relaxed);
            if note.clap && !s.core.owns(note) && !end_host_note(cx, note, offset) {
                s.end_rejections += 1;
            }
        }
        CoreEvent::NoteOff(pattern) | CoreEvent::Choke(pattern) => {
            for key in (0..128u8).filter(|key| pattern.key == -1 || pattern.key == i32::from(*key)) {
                let held = (0..16).any(|channel| s.core.key_held(channel, key));
                heard[usize::from(key)].store(u8::from(held), Ordering::Relaxed);
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
}

impl Default for Dsp {
    fn default() -> Self {
        Self {
            core: V2Core::default(),
            mix: Mix::default(),
            until_poll: 0,
            audition: vec![(0, 0); RACK_SLOTS],
            load: 0.0,
            shared_parts: Vec::new(),
            unsupported: 0,
            end_rejections: 0,
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
        Self { core: V2Core::with_parts(count, rate), audition: vec![(0, 0); count], shared_parts: parts }
    }

    fn adopt(&mut self, dsp: &mut Dsp) {
        dsp.core.adopt(&mut self.core);
        self.audition[..dsp.audition.len()].copy_from_slice(&dsp.audition);
        std::mem::swap(&mut dsp.audition, &mut self.audition);
        std::mem::swap(&mut dsp.shared_parts, &mut self.shared_parts);
    }
}

fn part_atoms<'a>(parts: &'a [Arc<PartShared>], shared: &'a Shared, slot: usize) -> Option<&'a PartShared> {
    parts.get(slot).or_else(|| shared.initial_parts.get(slot)).map(Arc::as_ref)
}

pub struct Sampler;

impl PluginLogic for Sampler {
    type Params = SamplerParams;
    type DspState = Dsp;

    fn bus_layouts() -> Vec<BusLayout> {
        const NAMES: [&str; BUSES] = [
            "st.1", "st.2", "st.3", "st.4", "st.5", "st.6", "st.7", "st.8", "st.9", "st.10", "st.11", "st.12",
            "st.13", "st.14", "st.15", "st.16",
        ];
        vec![NAMES.into_iter().fold(BusLayout::new(), |l, name| l.with_output(name, ChannelConfig::Stereo))]
    }

    fn reset(s: &mut Dsp, p: &SamplerParams, c: &AudioConfig) {
        s.core.reset(c.sample_rate);
        // Parts are prepared at a sample rate: the loader reloads them at this one.
        p.shared.rate.store(c.sample_rate.to_bits(), Ordering::Release);
        s.until_poll = 0;
        s.audition.fill((0, 0));
        p.shared.reset_midi();
    }

    fn process(
        s: &mut Dsp,
        p: &SamplerParams,
        b: &mut AudioBuffer,
        events: &EventList,
        cx: &mut ProcessContext,
    ) -> ProcessStatus {
        let started = Instant::now();
        let shared = &p.shared;
        if !shared.discard.is_full()
            && let Some(mut growth) = shared.growth.pop()
        {
            if growth.core.parts() > s.core.parts() {
                growth.adopt(s);
            }
            shared.grown.store(s.core.parts() as u64, Ordering::Release);
            let _ = shared.discard.push(Retired { growth: Some(growth), ..Default::default() });
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
                atoms.refresh_controls(|id| s.core.control_value(slot, id));
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
            let _ = shared.discard.push(Retired { mix: Some(mix), ..Default::default() });
        }
        // Replaced parts go back to the loader to be dropped; stop while it cannot take more.
        while !shared.discard.is_full() {
            let Some((slot, generation, part)) = shared.ready.pop() else { break };
            let current = part_atoms(&s.shared_parts, shared, slot)
                .is_some_and(|atoms| atoms.generation.load(Ordering::Acquire) == generation);
            let retired = if current && slot < s.core.parts() {
                let old = s.core.install(slot, part);
                s.core.set_mix(&s.mix);
                Retired { part: Some(old), ..Default::default() }
            } else {
                Retired { stale: part, ..Default::default() }
            };
            let _ = shared.discard.push(retired);
        }
        s.core.begin_block(&BlockInfo {
            frames,
            offline: cx.process_mode.is_offline(),
            transport: Transport {
                playing: cx.transport.playing,
                tempo: cx.transport.tempo,
                beats: cx.transport.position_beats,
                signature: (cx.transport.time_sig_num, cx.transport.time_sig_den),
            },
        });
        if shared.panic.swap(false, Ordering::AcqRel) {
            shared.reset_midi();
            s.core.panic();
            s.audition.fill((0, 0));
        }
        while let Some((slot, control, value)) = shared.control_edits.pop() {
            s.core.set_control(slot, control, value);
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

        let channels = b.num_output_channels();
        let thru = shared.midi_thru.load(Ordering::Relaxed);
        let mut peak = [0f32; 2];
        let mut gains = [0f32; MAX_BLOCK];
        let scope = shared.scope.source.load(Ordering::Relaxed);
        let mut at = 0;
        let mut incoming = events.lossless_iter().peekable();
        loop {
            while incoming.peek().is_some_and(|e| at >= frames || input_offset(e) as usize <= at) {
                match incoming.next().unwrap() {
                    LosslessEventRef::Typed(e) => feed_typed_input(s, p, e, cx, thru),
                    LosslessEventRef::Exact(exact) => match exact_host_input(exact) {
                        Some(ExactInput::Routed(event, port)) => {
                            for e in exact.fallback().into_iter().chain(exact.companions()) {
                                relay_typed_input(e, cx, thru);
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
            if at >= frames {
                break;
            }
            let due = incoming.peek().map_or(frames, |e| (input_offset(e) as usize).min(frames));
            let len = (due - at).min(MAX_BLOCK);
            for (part, (left, note)) in s.audition.iter_mut().enumerate().take(s.core.parts()) {
                if *left > 0 {
                    *left = left.saturating_sub(len);
                    if *left == 0 {
                        s.core.play(part, CoreEvent::midi1(0x80, *note, 0));
                    }
                }
            }
            for gain in &mut gains[..len] {
                *gain = db_to_linear(p.volume.read());
            }
            let ports = s.core.bus_ports().map(usize::from);
            s.core.set_tap(scope.checked_sub(1));
            let Rendered { buses, live } = s.core.render(len);
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
                let route = cx.bus_routing.output(port).map(|r| (r.channel_start(), r.channel_count()));
                let (start, count) = route.unwrap_or(if port == 0 { (0, channels.min(2)) } else { (0, 0) });
                for channel in (0..count.min(2)).filter(|c| start + c < channels) {
                    let out = &mut b.output(start + channel)[at..at + len];
                    for (i, (o, gain)) in out.iter_mut().zip(&gains[..len]).enumerate() {
                        let value = if count == 1 { (x[0][i] + x[1][i]) * 0.5 } else { x[channel][i] };
                        *o += value * gain;
                        peak[channel] = peak[channel].max(o.abs());
                    }
                }
            }
            if let Some(tapped) = s.core.tapped(len) {
                shared.scope.push(tapped);
            }
            at += len;
        }
        let offset = frames.saturating_sub(1) as u32;
        let refused = s.core.end_block(frames, &mut |note| end_host_note(cx, note, offset));
        s.end_rejections += refused;
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
                let Some(atoms) = part_atoms(&s.shared_parts, shared, slot) else { continue };
                let Ok(meters) = atoms.node_meters.try_lock() else { continue };
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
        let voices = s.core.voices();
        shared.voices.store(voices.active as u64, Ordering::Relaxed);
        shared.audible.store(voices.audible as u64, Ordering::Relaxed);
        shared.dropouts.store(voices.dropouts, Ordering::Relaxed);
        shared.unsupported_input.store(s.unsupported, Ordering::Relaxed);
        shared.end_rejections.store(s.end_rejections, Ordering::Relaxed);
        if frames > 0 && rate > 0. {
            // Positive `f32` bits order like the values: the UI swaps out the peak since it last looked.
            let busy = started.elapsed();
            shared.busy_ns.fetch_add(busy.as_nanos() as u64, Ordering::Relaxed);
            shared.span_ns.fetch_add((frames as f64 * 1e9 / rate) as u64, Ordering::Relaxed);
            let load = (busy.as_secs_f64() * rate / frames as f64) as f32;
            shared.cpu.fetch_max(u64::from(load.to_bits()), Ordering::Relaxed);
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::Cell,
    };

    struct Counting;
    thread_local! {
        static COUNTING: Cell<bool> = const { Cell::new(false) };
        static CALLS: Cell<usize> = const { Cell::new(0) };
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
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            count();
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

    #[test]
    fn plugin_contract() {
        assert!(
            SamplerParams::new().param_infos().iter().all(|p| p.midi_map.is_none()),
            "global MIDI parameter bindings would collapse port/channel routing"
        );
        moose_test::assert_valid_info::<Plugin>();
        moose_test::assert_has_editor::<Plugin>();
        moose_test::assert_state_round_trip::<Plugin>();
    }

    #[test]
    fn rack_state_round_trip() {
        let state = Selection {
            parts: vec![
                Part { path: "first.nki".into(), program: 2, channel: 2, gain: -9., tune: -3.5, mute: true, ..Default::default() },
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
            buses: vec![Bus { name: "Drums".into(), port: 3, ..Default::default() }],
            outputs: 2,
            ..Default::default()
        };
        assert!(Selection::deserialize(&state.serialize()).unwrap() == state);
    }

    #[test]
    fn saved_multi_round_trips_and_refuses_other_formats() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rack.kontra2-multi");
        let selection = Selection {
            parts: vec![Part { path: "a.nki".into(), ..Default::default() }, Part::default()],
            order: vec![0, 1],
            ..Default::default()
        };
        SavedMulti::of("Evening", &selection).save(&path).unwrap();
        let back = SavedMulti::read(&path).unwrap();
        assert_eq!((back.name.as_str(), back.parts.len()), ("Evening", 1), "empty parts are not saved");
        std::fs::write(&path, r#"{"format":"kontra-multi","version":1,"name":"x","parts":[]}"#).unwrap();
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
        let path = dir.path().join("tone.wav");
        let spec = hound::WavSpec { channels: 1, sample_rate: 48000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..48000 {
            w.write_sample(((i as f32 * 0.05).sin() * 20000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
        let params = SamplerParams::new();
        params.selection.write().unwrap().parts = vec![Part { path: path.display().to_string(), ..Default::default() }];
        load_until(&params, |p| p.shared.view.lock().unwrap().parts[0].tree.is_some());
        let view = params.shared.view.lock().unwrap();
        let v = &view.parts[0];
        assert!(!v.loading && v.report.as_ref().is_some_and(|r| r.decoded.zones == 1), "{}", v.status);
        drop(view);

        let mut dsp = Dsp::default();
        let (slot, generation, part) = params.shared.ready.pop().expect("a part was handed over");
        assert_eq!((slot, generation), (0, params.shared.part(0).unwrap().generation.load(Ordering::Relaxed)));
        dsp.core.install(slot, part);
        dsp.core.play(0, CoreEvent::midi1(0x90, 60, 100));
        let mut mix = mix(&params.selection.read().unwrap());
        let installed = allocations(|| {
            dsp.core.set_mix(&mix);
            let r = dsp.core.render(64);
            assert!(r.live[0]);
        });
        assert_eq!(installed, 0, "mixing and rendering do not allocate");
        mix.parts[0].mute = true;
        dsp.core.set_mix(&mix);
        assert!(!dsp.core.render(64).buses[0][0][..64].iter().any(|x| x.abs() > 0.0), "muted");

        // Clearing the part hands over an empty slot.
        params.selection.write().unwrap().parts[0].path.clear();
        load_until(&params, |p| p.shared.view.lock().unwrap().parts[0].tree.is_none());
        let (_, _, part) = std::iter::from_fn(|| params.shared.ready.pop()).last().unwrap();
        assert!(part.is_none());
    }

    #[test]
    fn a_failed_load_reports_why() {
        let params = SamplerParams::new();
        params.selection.write().unwrap().parts = vec![Part { path: "/nonexistent/x.nki".into(), ..Default::default() }];
        Load.run(&params);
        let view = params.shared.view.lock().unwrap();
        assert!(view.parts[0].status.starts_with("Load failed"), "{}", view.parts[0].status);
        assert!(view.parts[0].trace.is_some());
    }

    #[test]
    fn keyboard_plays_as_midi_on_channel_one() {
        assert_eq!(Play::Note(60, 100).event(), CoreEvent::midi1(0x90, 60, 100));
        assert_eq!(Play::Note(60, 0).event(), CoreEvent::midi1(0x80, 60, 0));
        assert_eq!(Play::Bend(8192).event(), CoreEvent::Ump([0x20e0_0040, 0]));
        assert_eq!(Play::Mod(64).event(), CoreEvent::midi1(0xb0, 1, 64));
    }
}
