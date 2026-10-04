use crate::articulate::{self, Articulate, In, Mpe, Route, Router};
use crate::{artwork, library};
use crate::{
    engine::{
        BUSES, Bank, BusControls, Engine, Heads, MAX_BLOCK, Mix, NO_AUX, PartControls, RACK_SLOTS, Rack, Residency,
        Streaming, TUNE_RANGE,
        overrides::{Edits, Override, Probe},
    },
    fx::{DIRECT, FxProcessor, OUTS},
    routing,
    import::{self, Instrument},
    timing::{self, Align, Holds, Plan, Timing},
    ksp::{Interface, KeyState, Live, LiveFault, Persisted, Refresh, Runtime},
};
use crossbeam_queue::ArrayQueue;
#[cfg(test)]
use crate::engine::load_scripts;
use moose::mui::mui::scene::Image;
use moose::prelude::*;
use moose::core::{ExactAddress, ExactEvent, ExactEventBody, ExactEventRef, ExactNoteAddress, ExactNoteKind, LosslessEventRef};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{
        Mutex, RwLock,
        atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    time::Instant,
};

#[cfg(feature = "uvi")]
pub(crate) mod uvi_ui;
#[cfg(feature = "uvi")]
mod uvi;
#[cfg(feature = "uvi")]
mod uvi_control;
#[cfg(feature = "uvi")]
mod uvi_state;
#[cfg(feature = "uvi")]
pub(crate) mod uvi_load;
#[cfg(feature = "uvi")]
mod uvi_delay;
#[cfg(all(test, feature = "uvi"))]
mod uvi_integration_tests;
#[cfg(feature = "uvi")]
const UVI_LEAD_PACKETS: usize = 16;

/// Opaque native persistence, shared by rack/UI clones. Debug never prints
/// authored values. The wire format remains the existing byte vector.
#[derive(Clone, Default, PartialEq)]
pub struct NativeState(Arc<Vec<u8>>);
impl NativeState {
    pub fn is_empty(&self) -> bool { self.0.is_empty() }
    pub fn clear(&mut self) { *self = Self::default(); }
}
impl From<Vec<u8>> for NativeState {
    fn from(bytes: Vec<u8>) -> Self { Self(Arc::new(bytes)) }
}
impl AsRef<[u8]> for NativeState {
    fn as_ref(&self) -> &[u8] { self.0.as_slice() }
}
impl std::fmt::Debug for NativeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeState").field("bytes", &self.0.len()).finish()
    }
}
impl serde::Serialize for NativeState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(self.0.as_ref(), serializer)
    }
}
impl<'de> serde::Deserialize<'de> for NativeState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        <Vec<u8> as serde::Deserialize>::deserialize(deserializer).map(Self::from)
    }
}
impl StateField for NativeState {
    fn write_field(&self, buf: &mut Vec<u8>) { self.0.as_ref().write_field(buf); }
    fn read_field(cursor: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        Vec::<u8>::read_field(cursor).map(Self::from)
    }
}

#[derive(State, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Part {
    pub path: String,
    pub group: u32,
    pub port: u8,
    pub output: u8,
    pub channel: i16,
    pub gain: f32,
    pub pan: f32,
    /// Semitones, cents as the fraction.
    pub tune: f32,
    pub mute: bool,
    pub solo: bool,
    pub program: u32,
    /// Script persistent variables as JSON (`Vec<Persisted>`); empty uses the instrument's saved values.
    pub script_state: String,
    /// Requested convolution controls; old host states default to the authored values.
    pub ir_settings: Vec<crate::fx::IrSlotSettings>,
    /// The name the player gave the part; empty shows the instrument's.
    pub name: String,
    /// The rack shows only the part's header, one slim line of it.
    pub collapsed: bool,
    /// The part's height in the rack, header and all, when the player sized
    /// it; 0 shows all of its performance view.
    pub height: f32,
    /// Output bus (0..[`BUSES`]) the part also sends to, post-fader; -1 for none.
    pub aux: i16,
    /// Level of that send in dB (-60..=6).
    pub aux_gain: f32,
    /// How notes pick the instrument's articulations, and its remapped keyswitches.
    pub articulate: Articulate,
    pub mpe: Mpe,
    /// Where the part's samples play from; `None` follows [`Selection::streaming`].
    pub streaming: Option<Streaming>,
    /// The player's envelope, filter and EQ edits over the instrument's own
    /// values; empty plays them as the library has them.
    pub edits: Edits,
    /// How late the part sounds, measured, and the player's override (see
    /// [`Selection::auto_align`]).
    pub timing: Timing,
    /// The player picked [`Part::output`]: automatic routing leaves it.
    pub output_manual: bool,
    /// Per output channel the instrument plays to past its own output (a
    /// mic mixer's "Out 2"), the bus it goes to, -1 with the part; kept by
    /// "One per mic" routing (`routing.rs`), empty otherwise.
    pub mic_buses: Vec<i16>,
    /// What plays on each of those channels, as the library names it.
    pub mic_names: Vec<String>,
    /// Which performance view the part shows: 0 follows the app's setting,
    /// 1 the library's original, 2 KONTRA's own controls, 3 the original
    /// vectorized ([`crate::library::ViewMode`]).
    pub view: u8,
    /// Snapshot applied to `path`, which remains its explicit base NKI.
    /// Appended so pre-keyed host states keep their positional field order.
    pub snapshot: String,
    /// Supported engine edits applied by authored callbacks, in last-write order.
    pub engine_state: Vec<crate::ksp::engine::NativeEdit>,
    /// Coupled Delay Time/unit caches, appended for older positional host states.
    pub delay_state: Vec<crate::fx::DelayState>,
    /// Native bank/member identity; appended to retain positional host states.
    /// An absent backend remains identifiable when its feature is disabled.
    pub uvi: Option<library::UviSource>,
    /// Opaque native program state, retained even by builds without UVI.
    /// Appended for positional host-state compatibility.
    pub uvi_state: NativeState,
}
impl Part {
    pub(crate) fn is_empty(&self) -> bool {
        self.path.is_empty() && self.uvi.is_none()
    }
    pub(crate) fn source(&self) -> (String, u32, String) {
        (self.path.clone(), self.program, self.snapshot.clone())
    }
    fn matches_source(&self, source: &(String, u32, String)) -> bool {
        self.path == source.0 && self.program == source.1 && self.snapshot == source.2
    }
    pub(crate) fn snapshot_base(&self) -> bool {
        self.program == 0 && Path::new(&self.path).extension().is_some_and(|e| e.eq_ignore_ascii_case("nki"))
    }
    fn select_snapshot(&mut self, path: String) {
        self.snapshot = path;
        self.uvi_state.clear();
        self.script_state.clear();
        self.ir_settings.clear();
        self.engine_state.clear();
        self.delay_state.clear();
        self.edits = Edits::default();
    }
    /// Where the part's samples play from, given the rack's setting.
    pub fn streaming(&self, rack: Streaming) -> Streaming {
        self.streaming.unwrap_or(rack)
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

impl StateField for library::UviSource {
    fn write_field(&self, buf: &mut Vec<u8>) { serde_json::to_string(self).unwrap().write_field(buf); }
    fn read_field(cursor: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        serde_json::from_str(&String::read_field(cursor)?).ok()
    }
}
impl StateField for library::UviRequest {
    fn write_field(&self, buf: &mut Vec<u8>) { serde_json::to_string(self).unwrap().write_field(buf); }
    fn read_field(cursor: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        serde_json::from_str(&String::read_field(cursor)?).ok()
    }
}
impl StateField for Edits {
    fn write_field(&self, buf: &mut Vec<u8>) {
        serde_json::to_string(self).unwrap_or_default().write_field(buf);
    }
    fn read_field(cursor: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        Some(serde_json::from_str(&String::read_field(cursor)?).unwrap_or_default())
    }
}
impl StateField for Timing {
    fn write_field(&self, buf: &mut Vec<u8>) {
        serde_json::to_string(self).unwrap_or_default().write_field(buf);
    }
    fn read_field(cursor: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        Some(serde_json::from_str(&String::read_field(cursor)?).unwrap_or_default())
    }
}
impl StateField for crate::ksp::engine::NativeEdit {
    fn write_field(&self, buf: &mut Vec<u8>) {
        serde_json::to_string(self).unwrap_or_default().write_field(buf);
    }
    fn read_field(cursor: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        serde_json::from_str(&String::read_field(cursor)?).ok()
    }
}
impl StateField for crate::fx::IrSlotSettings {
    fn write_field(&self, buf: &mut Vec<u8>) {
        serde_json::to_string(self).unwrap_or_default().write_field(buf);
    }
    fn read_field(cursor: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        serde_json::from_str(&String::read_field(cursor)?).ok()
    }
}
impl StateField for crate::fx::DelayState {
    fn write_field(&self, buf: &mut Vec<u8>) {
        serde_json::to_string(self).unwrap_or_default().write_field(buf);
    }
    fn read_field(cursor: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        serde_json::from_str(&String::read_field(cursor)?).ok()
    }
}
impl Default for Part {
    fn default() -> Self {
        Self {
            path: String::new(),
            program: 0,
            group: u32::MAX,
            port: 0,
            output: 0,
            channel: -1,
            gain: 0.,
            pan: 0.,
            tune: 0.,
            mute: false,
            solo: false,
            script_state: String::new(),
            ir_settings: Vec::new(),
            name: String::new(),
            collapsed: false,
            height: 0.,
            aux: -1,
            aux_gain: 0.,
            articulate: Articulate::default(),
            mpe: Mpe::default(),
            streaming: None,
            edits: Edits::default(),
            timing: Timing::default(),
            output_manual: false,
            mic_buses: Vec::new(),
            mic_names: Vec::new(),
            view: 0,
            snapshot: String::new(),
            engine_state: Vec::new(),
            delay_state: Vec::new(),
            uvi: None,
            uvi_state: NativeState::default(),
        }
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
#[derive(State, Default, Clone, PartialEq)]
pub struct Selection {
    /// The library folder projects once kept; the app's settings keep the
    /// library folders now ([`library::Settings`]). Kept so old projects open.
    pub root: String,
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
    /// Where samples play from, for parts that do not choose
    /// ([`Part::streaming`]); a change reloads the parts it affects.
    pub streaming: Streaming,
    /// Auto-align timing (experimental): each part's articulations are
    /// measured, the latest is reported to the host as latency, and notes
    /// are held back so every attack lands on the grid (`timing.rs`).
    pub auto_align: bool,
    /// Hold notes back only while the host's transport plays: played live
    /// with it stopped, a part sounds as late as its library does.
    pub align_transport_only: bool,
    /// How parts are routed to buses and host ports ([`routing::Outputs`]).
    pub outputs: u8,
    /// Appended: pre-keyed Kontakt sessions retain positional field ordering.
    pub uvi_favorites: Vec<library::UviSource>,
    pub uvi_recent: Vec<library::UviSource>,
    /// Requested, not loaded: retained while the live adapter is unavailable.
    pub uvi_requested: Option<library::UviRequest>,
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
            (self.parts.iter()).any(|p| !p.is_empty() && p.port == port && p.channel == channel)
        };
        (0..4u8)
            .flat_map(|port| (0..16i16).map(move |channel| (port, channel)))
            .find(|&(port, channel)| !taken(port, channel))
            .unwrap_or((0, -1))
    }
}

#[derive(Params)]
#[params(output_port_name = "port_name", output_port_names_revision = "port_names_revision", pre_save = "capture_native_state")]
pub struct SamplerParams {
    #[param(name="Volume",range="linear(-60, 6)",default=-12.0,unit="dB",smooth="exp(5)")]
    pub volume: FloatParam,
    #[param(name = "Attack", range = "log(0.0001, 5)", default = 0.002, unit = "s")]
    pub attack: FloatParam,
    #[param(name = "Release", range = "log(0.001, 10)", default = 0.15, unit = "s")]
    pub release: FloatParam,
    #[param(
        name = "Tone",
        range = "log(20, 20000)",
        default = 20000.0,
        unit = "Hz"
    )]
    pub cutoff: FloatParam,
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
    fn capture_native_state(&self) {
        #[cfg(feature = "uvi")]
        let mut selection = self.selection.read().unwrap().clone();
        #[cfg(feature = "uvi")]
        if let Err(error) = self.capture_uvi_state(&mut selection) {
            // Params pre_save cannot return an error; retain the last successful state.
            crate::diagnostics::event(crate::diagnostics::LogLevel::Error, "uvi", "state_capture_failed",
                serde_json::json!({"reason":error.to_string()}));
        }
    }

    /// Off audio: explicit rack saves require a fresh, coherent native snapshot.
    #[cfg(feature = "uvi")]
    pub(crate) fn capture_uvi_state(&self, selection: &mut Selection) -> anyhow::Result<()> {
        uvi_state::capture(self, selection)
    }

    /// Serialized loader only. The controller and its blocking destructor stay
    /// here until an audio bridge has a separately owned, fixed packet port.
    #[cfg(feature = "uvi")]
    pub(crate) fn with_prepared_uvi_worker<T>(&self, use_worker: impl FnOnce(&library::UviRequest, u64, u64, &mut crate::uvi::worker::Worker) -> T) -> Option<T> {
        let key = uvi_load_key(self, &self.selection.read().unwrap())?;
        let mut prepared = self.shared.uvi_prepared.lock().unwrap();
        let prepared = prepared.as_mut().filter(|prepared| prepared.key == key)?;
        let worker = prepared.worker.as_mut().filter(|worker| worker.status() == crate::uvi::worker::Status::Ready)?;
        Some(use_worker(&key.request, key.epoch, prepared.generation, worker))
    }
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
        drain_audio_diagnostics(self);
        let selection = self.selection.read().unwrap().clone();
        self.shared.ensure_parts(selection.parts.len());
        let view = self.shared.view.lock().unwrap();
        let parts: Vec<_> = selection.parts.iter().enumerate().map(|(slot, part)| {
            let v = &view.parts[slot];
            serde_json::json!({
                "slot":slot, "path":part.path, "program":part.program, "name":part.name,
                "port":part.port, "channel":part.channel, "output":part.output,
                "aux":part.aux, "mic_buses":part.mic_buses, "mic_names":part.mic_names,
                "gain":part.gain, "pan":part.pan, "tune":part.tune, "mute":part.mute, "solo":part.solo,
                "articulation":part.articulate, "mpe":part.mpe, "timing":part.timing,
                "streaming":part.streaming(selection.streaming), "generation":self.shared.part(slot).unwrap().generation.load(Ordering::Relaxed),
                "script_epoch":v.script_epoch, "status":v.status, "load":v.load_report.as_deref(),
                "runtime_status":v.runtime_status,
            })
        }).collect();
        let audio = self.shared.diagnostic_latest.lock().unwrap().clone();
        let context = serde_json::json!({
            "instance_id":self.shared.instance_id, "build":crate::build_info::BUILD,
            "host":{"sample_rate":self.shared.rate(), "audio":audio},
            "rack":{"parts":parts, "buses":selection.buses, "outputs":selection.outputs,
                "auto_align":selection.auto_align, "align_transport_only":selection.align_transport_only,
                "midi_thru":selection.midi_thru},
            "audio_snapshots_dropped":self.shared.diagnostic_dropped.load(Ordering::Relaxed),
            "keyboard":{"heard":self.shared.heard.iter().map(|v| v.load(Ordering::Relaxed)).collect::<Vec<_>>(),
                "played":self.shared.played.iter().map(|v| v.load(Ordering::Relaxed)).collect::<Vec<_>>()},
            "memory":{"resident_bytes":crate::engine::resident_bytes(), "budget_bytes":crate::engine::memory_budget()},
        });
        drop(view);
        let mut context = context;
        #[cfg(feature = "uvi")]
        {
            let runtime_started=Instant::now();
            let runtime_budget=std::time::Duration::from_millis(500);
            let prepared = self.shared.uvi_prepared.lock().unwrap();
            context["uvi"] = serde_json::json!({
                "requested":selection.uvi_requested, "live_installed":self.shared.with_parts(|parts|
                    parts.iter().any(|p| p.uvi_generation.load(Ordering::Acquire) != 0 && !p.uvi_failed.load(Ordering::Acquire))),
                "prepared":prepared.as_ref().map(|p| serde_json::json!({
                    "source":p.key.request.source, "slot":p.key.request.slot, "new":p.key.request.new,
                    "epoch":p.key.epoch, "generation":p.generation, "sample_rate":f64::from_bits(p.key.rate),
                    "worker":p.worker.as_ref().map(|worker| worker.runtime_diagnostic_report(runtime_budget.saturating_sub(runtime_started.elapsed()))),
                    "status":p.status, "initialized":Some(&p.key) == uvi_load_key(self, &selection).as_ref()
                        && p.worker.as_ref().is_some_and(|worker| worker.status() == crate::uvi::worker::Status::Ready),
                })),
                "rack_workers":self.shared.uvi_controls.lock().unwrap().runtime_diagnostic_report(runtime_budget.saturating_sub(runtime_started.elapsed())),
                "block_frames":crate::uvi::worker::BLOCK_FRAMES, "queue_capacity":crate::uvi::worker::QUEUE_CAPACITY,
            });
            drop(prepared);
            // Leave room in the existing 4MiB support context for host/Kontakt
            // reports. The full stamped node snapshot remains on each worker.
            uvi_control::bound_node_reports(&mut context["uvi"], 2 * 1024 * 1024);
        }
        context["log_flush_error"] = serde_json::json!(crate::diagnostics::flush(std::time::Duration::from_secs(2)).err());
        context
    }
}

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

/// Fixed-size playback state; the audio thread only copies it into a queue.
#[derive(Clone, Copy, Default, serde::Serialize)]
struct PartDiagnostics {
    generation: u64,
    script_epoch: u64,
    voices: usize,
    audible: usize,
    underruns: u64,
    dropped_commands: u64,
    host_note_drops: u64,
    held_keys: [[u64; 2]; 16],
    sustain_cc: [u8; 16],
    sostenuto_cc: [u8; 16],
    pending_commands: usize,
    pending_writes: usize,
    pending_releases: usize,
}

#[derive(Clone, serde::Serialize)]
struct AudioDiagnostics {
    block: u64,
    sample_rate: f64,
    block_size: usize,
    output_channels: usize,
    offline: bool,
    output_buses: [(usize, usize); BUSES],
    parts: Vec<PartDiagnostics>,
    unsupported_note_brightness: u64,
    unsupported_host_expression: u64,
    alignment_overflows: u64,
    host_note_end_rejections: u64,
}

impl AudioDiagnostics {
    fn with_parts(count: usize) -> Self {
        Self { block: 0, sample_rate: 0.0, block_size: 0, output_channels: 0,
            offline: false, output_buses: [(0, 0); BUSES], parts: vec![PartDiagnostics::default(); count], unsupported_note_brightness: 0, unsupported_host_expression: 0, alignment_overflows: 0, host_note_end_rejections: 0 }
    }
}

/// Stable per-part atoms shared with the editor/loader. Audio keeps its own
/// prepared Arc vector and never locks the growable registry.
#[derive(Default)]
pub(crate) struct PartShared {
    #[cfg(feature = "uvi")]
    pub(crate) uvi_generation: AtomicU64,
    #[cfg(feature = "uvi")]
    uvi_part_generation: AtomicU64,
    #[cfg(feature = "uvi")]
    uvi_failed: AtomicBool,
    #[cfg(feature = "uvi")]
    uvi_state_frame: AtomicU64,
    pub(crate) generation: AtomicU64,
    /// Out of [`crate::engine::LOAD_DONE`], rising within each load.
    pub(crate) load_progress: AtomicU32,
    pub(crate) meter: [AtomicU32; 2],
    pub(crate) clip: AtomicBool,
    underruns: AtomicU64,
    mics: [AtomicU16; OUTS],
    measuring: AtomicBool,
}

pub struct Shared {
    instance_id: u64,
    diagnostic_audio: ArrayQueue<AudioDiagnostics>,
    diagnostic_free: ArrayQueue<AudioDiagnostics>,
    diagnostic_latest: Mutex<Option<AudioDiagnostics>>,
    diagnostic_dropped: AtomicU64,
    ready: ArrayQueue<(usize, u64, Handoff)>,
    pending_ready: Mutex<std::collections::VecDeque<(usize, u64, Handoff)>>,
    discard: ArrayQueue<Retired>,
    /// Loader-only staging. A Worker owns a blocking join, never a Dsp payload.
    #[cfg(feature = "uvi")]
    uvi_prepared: Mutex<Option<UviPrepared>>,
    #[cfg(feature = "uvi")]
    uvi_epoch: AtomicU64,
    #[cfg(feature = "uvi")]
    uvi_generation: AtomicU64,
    #[cfg(feature = "uvi")]
    uvi_max_host_frames: AtomicUsize,
    #[cfg(feature = "uvi")]
    uvi_delays: ArrayQueue<UviDelayHandoff>,
    #[cfg(feature = "uvi")]
    uvi_delay_prepared: Mutex<Option<UviDelayPreparation>>,
    #[cfg(feature = "uvi")]
    uvi_delay_wanted: AtomicU64,
    #[cfg(feature = "uvi")]
    uvi_delay_installed: AtomicU64,
    /// Admitted native buffering latency + maximum host frames, independent of
    /// worker epoch. Host resets must not retract latency during replacement.
    #[cfg(feature = "uvi")]
    uvi_latency_admission: AtomicU64,
    #[cfg(feature = "uvi")]
    uvi_controls: Arc<Mutex<uvi_control::Registry>>,
    #[cfg(feature = "uvi")]
    uvi_state_capture: Mutex<()>,
    #[cfg(feature = "uvi")]
    uvi_edits: ArrayQueue<(usize, crate::uvi::worker::Stamp, crate::uvi::host::UiEdit)>,
    /// Persistence snapshots: the loader lends one per scripted slot, the audio thread fills it in place and returns it.
    snapshot_requests: ArrayQueue<(usize, u64, Box<PersistenceSnapshot>)>,
    /// Refreshed snapshots, and whether any value in them changed.
    snapshots: ArrayQueue<(usize, u64, Box<PersistenceSnapshot>, bool)>,
    /// Live script views, lent and refreshed the same way. The source epoch
    /// travels with each buffer so replacements cannot refresh an old shape.
    live_requests: ArrayQueue<(usize, u64, Box<Live>)>,
    lives: ArrayQueue<(usize, u64, Box<Live>)>,
    /// One non-audio publisher; the timestamp distinguishes editor frames from loader polling.
    live_publish: Mutex<(Option<Instant>, usize)>,
    #[cfg(test)]
    load_gate: Mutex<Option<(usize, Arc<std::sync::Barrier>)>>,
    #[cfg(test)]
    restore_gate: Mutex<Option<(usize, Arc<std::sync::Barrier>)>>,
    #[cfg(test)]
    publish_gate: Mutex<Option<Arc<std::sync::Barrier>>>,
    #[cfg(test)]
    snapshot_gate: Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Edits of script controls from the performance view.
    edits: ArrayQueue<Edit>,
    file_selections: ArrayQueue<FileSelection>,
    /// IR loads requested by scripts: part, instrument generation, script epoch.
    ir_requests: ArrayQueue<(usize, u64, u64, crate::engine::IrRequest)>,
    ir_ready: ArrayQueue<(usize, u64, IrHandoff)>,
    zone_requests: ArrayQueue<ZoneRequest>,
    zone_ready: ArrayQueue<ZoneReady>,
    zone_pending: Mutex<Option<ZoneRequest>>,
    zone_chains: Mutex<Vec<ZoneChain>>,
    array_requests: ArrayQueue<(usize, u64, u64, Box<crate::ksp::ArrayJob>)>,
    array_ready: ArrayQueue<(usize, u64, u64, Box<crate::ksp::ArrayJob>)>,
    array_retired: ArrayQueue<(usize, u64, u64, Box<crate::ksp::ArrayJob>)>,
    /// Host sample rate (`f64` bits) that effect processors are built for.
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
    /// Each slot's articulation and MPE routing, sent with [`Shared::controls`].
    routes: ArrayQueue<Vec<Route>>,
    /// Peak meters the audio thread keeps current; read them at paint time.
    pub meters: Meters,
    /// The part and group the sound editor shows, as the audio thread plays it.
    pub(crate) probe: Probe,
    /// One strip's signal for a spectrum on screen.
    pub(crate) scope: Scope,
    /// Blocks processed: a stopped host stops counting.
    pub(crate) blocks: AtomicU64,
    /// Override changes for the audio thread, by rack slot.
    overrides: ArrayQueue<(usize, Override)>,
    /// Each slot's overrides as last sent (loader and editor threads only).
    sent: Mutex<Vec<Vec<Override>>>,
    /// Each slot's smart memory and the generation it serves (loader only).
    residency: Mutex<Vec<Option<(u64, Box<Residency>)>>>,
    /// Resized heads back from the audio thread: slot, generation, heads.
    heads: ArrayQueue<(usize, u64, Heads)>,
    parts: Mutex<Vec<Arc<PartShared>>>,
    initial_parts: [Arc<PartShared>; RACK_SLOTS],
    growth: ArrayQueue<Box<PreparedGrowth>>,
    grown: AtomicU64,
    growth_prepared: AtomicU64,
    pub(crate) audition: AtomicBool,
    pub(crate) audition_note: AtomicU64,
    pub(crate) selected: AtomicU64,
    pub(crate) focus_request: AtomicU64,
    pub(crate) panic: AtomicBool,
    pub(crate) midi_thru: AtomicBool,
    pub(crate) multi_request: Mutex<Option<String>>,
    /// Latest rack-wide snapshot selection; a newer request supersedes
    /// validation in any slot. Separate from audio persistence snapshots.
    pub(crate) snapshot_request: Mutex<Option<SnapshotRequest>>,
    /// The app's library folders and the scan of them.
    pub(crate) libraries: library::Scanner,
    pub(crate) view: Mutex<View>,
    /// Voices sounding across the rack, reported by the audio thread.
    pub(crate) voices: AtomicU64,
    #[cfg(feature = "uvi")]
    pub(crate) uvi_voices: AtomicU64,
    /// Of those, the ones not muted by their scripts: the ones rendered.
    pub(crate) audible: AtomicU64,
    /// Set by every editor display tick: an editor is open to show the
    /// scripts' live views, which the audio thread otherwise need not copy.
    pub(crate) watched: AtomicBool,
    /// Audio thread load (`f32` bits): render time over block time, peak-held.
    pub(crate) cpu: AtomicU64,
    /// Render time and the audio time it rendered, in nanoseconds, summed:
    /// the meter shows their ratio over its window, as Kontakt does.
    pub(crate) busy_ns: AtomicU64,
    pub(crate) span_ns: AtomicU64,
    /// Voice blocks whose streamed samples were not read in time (played
    /// silent) plus script engine calls dropped by a full queue, summed over
    /// the parts' engines since each was created.
    pub(crate) dropouts: AtomicU64,
    /// Per slot, streamed frames that arrived late, for its smart memory.

    /// The timing plan for the audio thread; the latest wins.
    plan: ArrayQueue<Plan>,
    /// Loader only: the plan last sent, and a new latency waiting to settle.
    published: Mutex<(Option<Plan>, Option<(f32, Instant)>)>,
    /// Timing measurements running and finished.
    measure: Arc<Measure>,
    /// The latency the host is told, ms (`f32` bits); 0 while auto-align is off.
    pub(crate) reported: AtomicU32,
    /// What each part's instrument plays past its own output ([`routing::Mics`]),
    /// published by the audio thread.

    /// Host port names; the loader publishes, the host's main thread reads.
    port_names: Mutex<routing::PortNames>,
    /// Bumped with each publication: format wrappers poll it and tell the host.
    port_names_revision: AtomicU64,
    // Last: bank/stream queues and their workers retire before the logging worker.
    _diagnostics: crate::diagnostics::DiagnosticLease,
    // The crash marker retires after every retained worker and diagnostics lease.
    _crash_session: Mutex<Option<crate::support::CrashSessionGuard>>,
}
/// How long an editor edit outranks a live view that disagrees with it.
const EDIT_SETTLE: std::time::Duration = std::time::Duration::from_millis(500);

/// Keep recent `edited` values over a live view begun before the scripts
/// had them; an edit the view agrees with, or an old one, is done.
fn settle_edits(edited: &mut Vec<(usize, i32, Instant)>, interface: Option<&crate::ksp::Interface>) {
    edited.retain(|&(control, value, at)| {
        at.elapsed() <= EDIT_SETTLE && interface.and_then(|i| i.controls.get(control)).is_some_and(|c|
            c.properties.get("$CONTROL_PAR_VALUE") != Some(&crate::ksp::Value::Int(value)))
    });
}

/// Parts' timing measurements, each on a thread of its own.
#[derive(Default)]
struct Measure {

    /// Slot and what was measured, or why it could not be.
    done: Mutex<Vec<(usize, String, Result<Timing, String>)>>,
}
#[derive(PartialEq, Eq)]
pub(crate) struct LiveDiagnostics {
    epoch: u64,
    faults: Vec<LiveFault>,
    fault_occurrences_omitted: u64,
    notes: Vec<&'static str>,
}

pub(crate) struct ScriptPage {
    pub(crate) slot: usize,
    pub(crate) title: String,
    pub(crate) interface: Arc<Interface>,
    last_view: Mutex<Option<Arc<Interface>>>,
}

/// Prepared on Load; frame snapshots share these unused page buffers instead
/// of copying every page's controls. Audio only receives one existing Live.
#[derive(Default)]
pub(crate) struct ScriptPages {
    pub(crate) views: Vec<ScriptPage>,
    buffers: Mutex<Vec<Box<Live>>>,
}

pub(crate) fn script_pages(rt: Option<&Runtime>) -> Arc<ScriptPages> {
    let mut pages = ScriptPages::default();
    if let Some(rt) = rt.filter(|rt| rt.performance_slots().count() > 1) {
        for (slot, title) in rt.performance_slots() {
            let live = Box::new(rt.live_for_slot(Some(slot)));
            pages.views.push(ScriptPage {
                slot, title: if title.is_empty() { format!("Script {}", slot + 1) } else { title.into() },
                interface: Arc::new(live.interface.as_ref().unwrap().clone()),
                last_view: Mutex::new(None),
            });
            // The first page has a separate buffer in PartView::live.
            if pages.views.len() > 1 { pages.buffers.get_mut().unwrap().push(live); }
        }
    }
    Arc::new(pages)
}

fn first_script_live(rt: &Runtime) -> Box<Live> {
    Box::new(rt.live_for_slot(rt.performance_slots().next().map(|(slot, _)| slot)))
}

#[derive(Default, Clone)]
pub(crate) struct PartView {
    #[cfg(feature = "uvi")]
    pub(crate) uvi_ui: Option<Arc<uvi_ui::Published>>,
    #[cfg(feature = "uvi")]
    pub(crate) uvi_activation: Option<uvi_load::Activation>,
    pub(crate) program: u32,
    pub(crate) interface: Option<Arc<crate::ksp::Interface>>,
    pub(crate) interface_status: String,
    pub(crate) load_report: Option<Arc<serde_json::Value>>,
    pub(crate) runtime_status: String,
    pub(crate) diagnostics_lent: Option<Instant>,
    /// Latest bounded raw diagnostics; formatting never delays live-buffer recycling.
    pub(crate) live_diagnostics: Option<Arc<LiveDiagnostics>>,
    pub(crate) diagnostics_dirty: bool,
    pub(crate) wallpaper: Option<Arc<artwork::Picture>>,
    /// Control pictures the scripts name, by name.
    pub(crate) pictures: Arc<HashMap<String, Arc<artwork::Picture>>>,
    pub(crate) wallpaper_status: String,
    pub(crate) attempted: Option<(String, u32, String)>,
    /// How the attempted load placed samples.
    pub(crate) streaming: Streaming,
    pub(crate) instrument: Option<Arc<Instrument>>,
    pub(crate) status: String,
    pub(crate) active: String,
    pub(crate) bytes: usize,
    /// Bytes the smart memory handed back so far.
    pub(crate) freed: u64,
    /// Rate of the effects handed to the audio thread; 0 when none were.
    pub(crate) fx_rate: f64,
    /// Impulse responses the runtime's `on init` loaded, built into the effects.
    pub(crate) irs: Vec<crate::fx::ScriptIr>,
    /// `Part::script_state` the audio thread's runtime matches (loaded from or last saved).
    pub(crate) script_state: String,
    pub(crate) ir_settings: Vec<crate::fx::IrSlotSettings>,
    pub(crate) engine_state: Arc<[crate::ksp::engine::NativeEdit]>,
    pub(crate) delay_state: Arc<[crate::fx::DelayState]>,
    /// Epoch of the runtime last handed to the audio thread.
    pub(crate) script_epoch: u64,
    /// Snapshot buffer for this slot's runtime while the loader holds it.
    pub(crate) snapshot: Option<Box<PersistenceSnapshot>>,
    /// When `snapshot` was last lent out.
    pub(crate) snapshot_lent: Option<Instant>,
    /// Live view buffer for this slot's runtime while the loader holds it.
    pub(crate) live: Option<Box<Live>>,
    pub(crate) live_revisions: Option<(u64, u64)>,
    /// Published control stamps, shared cheaply by frame snapshots; never touched by audio.
    pub(crate) live_control_versions: Arc<[(u64, u64)]>,
    #[cfg(test)]
    pub(crate) publication_rows: Option<(usize, usize, bool)>,
    /// Script slot that `interface` belongs to.
    pub(crate) script_slot: usize,
    pub(crate) script_pages: Arc<ScriptPages>,
    /// Samples are being read for this slot.
    pub(crate) loading: bool,
    /// Keyboard colors and names the scripts set, kept current while they run.
    pub(crate) keys: Arc<BTreeMap<u8, KeyState>>,
    /// Why the part's timing could not be measured.
    pub(crate) timing_status: String,
    /// Controls set from the editor, their value and when: a live view the
    /// scripts began before the edit reached them still shows the old value,
    /// and would flick the control back for a frame.
    pub(crate) edited: Vec<(usize, i32, Instant)>,
}
impl PartView {
    #[cfg(all(test, feature = "uvi"))]
    pub(crate) fn authored_uvi(source: library::UviSource, published: Arc<uvi_ui::Published>) -> Self {
        let stamp = published.stamp;
        Self {
            uvi_activation: Some(uvi_load::Activation { source, epoch: stamp.epoch, generation: stamp.generation,
                saved_state: NativeState::default(), part_generation: 0, rate: 48000, max_host_frames: MAX_BLOCK, published: true }),
            uvi_ui: Some(published), status: "UVI instrument".into(), ..Default::default()
        }
    }
    #[cfg(feature = "uvi")]
    pub(crate) fn uvi_matches(&self, source: &library::UviSource, stamp: crate::uvi::worker::Stamp) -> bool {
        self.uvi_activation.as_ref().is_some_and(|a| a.source == *source
            && (a.epoch, a.generation) == (stamp.epoch, stamp.generation))
    }
    /// Pending scalar values affect drawing without copying or mutating the
    /// callback-derived interface retained by readers.
    pub(crate) fn control_value(&self, control: usize) -> Option<f64> {
        let c = self.interface.as_ref()?.controls.get(control)?;
        if let Some((_, value)) = self.edited_values().find(|&(n, _)| n == control) {
            return Some(f64::from(value));
        }
        match c.properties.get("$CONTROL_PAR_VALUE")? {
            crate::ksp::Value::Int(n) => Some(f64::from(*n)),
            crate::ksp::Value::Real(r) => Some(*r),
            _ => None,
        }
    }

    /// Timestamps settle edits; only control/value pairs invalidate drawing.
    pub(crate) fn edited_values(&self) -> impl Iterator<Item = (usize, i32)> + '_ {
        self.edited.iter().map(|&(n, value, _)| (n, value))
    }
}
#[derive(Clone)]
pub(crate) struct View {
    /// Last script epoch handed out; tags runtimes so stale persistence snapshots are ignored.
    pub(crate) script_epoch: u64,
    pub(crate) multi_status: String,
    pub(crate) artwork: HashMap<String, Arc<Image>>,
    /// The libraries found, and which scan found them ([`library::Scanner::wanted`]).
    pub(crate) shelf: Arc<library::Shelf>,
    pub(crate) scanned: u64,
    pub(crate) files: Arc<Vec<PathBuf>>,
    pub(crate) parts: Vec<PartView>,
    pub(crate) status: String,
    pub(crate) uvi_attempted: Option<library::UviRequest>,
    pub(crate) uvi_status: String,
    #[cfg(feature = "uvi")]
    pub(crate) uvi_ui: Option<Arc<uvi_ui::Published>>,
    /// When an editor last showed the rack (see [`Shared::watched`]).
    pub(crate) watched_at: Option<Instant>,
}
impl Default for Shared {
    fn default() -> Self {
        let crash_session = crate::support::start_plugin_session();
        let initial_parts: [Arc<PartShared>; RACK_SLOTS] = std::array::from_fn(|_| Arc::default());
        let diagnostic_free = ArrayQueue::new(4);
        for _ in 0..2 { diagnostic_free.push(AudioDiagnostics::with_parts(RACK_SLOTS)).ok().unwrap(); }
        let shared = Self {
            _crash_session: Mutex::new(crash_session),
            instance_id: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed),
            diagnostic_audio: ArrayQueue::new(2),
            diagnostic_free,
            diagnostic_latest: Mutex::new(None),
            diagnostic_dropped: AtomicU64::new(0),
            ready: ArrayQueue::new(64),
            pending_ready: Mutex::default(),
            discard: ArrayQueue::new(64),
            #[cfg(feature = "uvi")]
            uvi_prepared: Mutex::default(),
            #[cfg(feature = "uvi")]
            uvi_epoch: AtomicU64::new(1),
            #[cfg(feature = "uvi")]
            uvi_generation: AtomicU64::new(0),
            #[cfg(feature = "uvi")]
            uvi_max_host_frames: AtomicUsize::new(MAX_BLOCK),
            #[cfg(feature = "uvi")]
            uvi_delays: ArrayQueue::new(2),
            #[cfg(feature = "uvi")]
            uvi_delay_prepared: Mutex::default(),
            #[cfg(feature = "uvi")]
            uvi_delay_wanted: AtomicU64::new(0),
            #[cfg(feature = "uvi")]
            uvi_delay_installed: AtomicU64::new(0),
            #[cfg(feature = "uvi")]
            uvi_latency_admission: AtomicU64::new(0),
            #[cfg(feature = "uvi")]
            uvi_controls: Arc::default(),
            #[cfg(feature = "uvi")]
            uvi_state_capture: Mutex::new(()),
            #[cfg(feature = "uvi")]
            uvi_edits: ArrayQueue::new(256),
            snapshot_requests: ArrayQueue::new(2 * RACK_SLOTS),
            snapshots: ArrayQueue::new(2 * RACK_SLOTS),
            live_requests: ArrayQueue::new(2 * RACK_SLOTS),
            lives: ArrayQueue::new(2 * RACK_SLOTS),
            live_publish: Mutex::new((None, 0)),
            #[cfg(test)]
            load_gate: Mutex::new(None),
            #[cfg(test)]
            restore_gate: Mutex::new(None),
            #[cfg(test)]
            publish_gate: Mutex::new(None),
            #[cfg(test)]
            snapshot_gate: Mutex::new(None),
            edits: ArrayQueue::new(256),
            file_selections: ArrayQueue::new(16),
            ir_requests: ArrayQueue::new(64),
            ir_ready: ArrayQueue::new(64),
            zone_requests: ArrayQueue::new(crate::engine::zone::QUEUE),
            zone_ready: ArrayQueue::new(64),
            zone_pending: Mutex::default(),
            zone_chains: Mutex::default(),
            array_requests: ArrayQueue::new(64),
            array_ready: ArrayQueue::new(64),
            array_retired: ArrayQueue::new(64),
            rate: AtomicU64::new(48000f64.to_bits()),
            key_owners: std::array::from_fn(|_| AtomicU64::new(0)),
            played: std::array::from_fn(|_| AtomicU8::new(0)),
            heard: std::array::from_fn(|_| AtomicU8::new(0)),
            keyboard: ArrayQueue::new(256),
            bend: AtomicU32::new(8192),
            modulation: AtomicU32::new(0),
            controls: ArrayQueue::new(1),
            routes: ArrayQueue::new(1),
            meters: Meters::default(),
            probe: Probe::default(),
            scope: Scope::default(),
            blocks: AtomicU64::new(0),
            overrides: ArrayQueue::new(1024),
            residency: Mutex::new((0..RACK_SLOTS).map(|_| None).collect()),
            // One batch in flight per slot: never full.
            heads: ArrayQueue::new(RACK_SLOTS),
            sent: Mutex::new((0..RACK_SLOTS).map(|_| Vec::new()).collect()),
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
            voices: AtomicU64::new(0),
            #[cfg(feature = "uvi")]
            uvi_voices: AtomicU64::new(0),
            audible: AtomicU64::new(0),
            watched: AtomicBool::new(false),
            cpu: AtomicU64::new(0),
            busy_ns: AtomicU64::new(0),
            span_ns: AtomicU64::new(0),
            dropouts: AtomicU64::new(0),

            plan: ArrayQueue::new(1),
            published: Mutex::default(),
            measure: Arc::default(),
            reported: AtomicU32::new(0),

            port_names: Mutex::default(),
            port_names_revision: AtomicU64::new(0),
            _diagnostics: crate::diagnostics::acquire(),
            view: Mutex::new(View {
                script_epoch: 0,
                multi_status: String::new(),
                artwork: Default::default(),
                shelf: Arc::default(),
                scanned: 0,
                files: Arc::default(),
                parts: (0..RACK_SLOTS).map(|_| PartView::default()).collect(),
                status: "Choose a library and select a preset".into(),
                uvi_attempted: None,
                uvi_status: String::new(),
                #[cfg(feature = "uvi")]
                uvi_ui: None,
                watched_at: None,
            }),
        };
        if let Some(guard) = shared._crash_session.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut() { guard.mark_initialized(); }
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
/// spectrum shows and clears otherwise: closed, it costs one load a block.
/// Lock-free: a reader may see a block half-written, which a spectrum
/// cannot tell from the signal.
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
#[derive(Default)]
pub struct Meters {
    /// Rack slots, after the part's gain, pan, mute and solo.

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
            let level = if level < 1e-4 || !level.is_finite() { 0. } else { level };
            m.store(level.to_bits(), Ordering::Relaxed);
        }
    }
}
fn db_gain(db: f32) -> f32 {
    if db.is_finite() {
        db_to_linear(db.clamp(-60., 6.))
    } else {
        1.
    }
}
/// What each rack slot's MIDI goes through before its scripts.
pub(crate) fn routes(selection: &Selection) -> Vec<Route> {
    (0..selection.parts.len().max(RACK_SLOTS)).map(|n| {
        selection
            .parts
            .get(n)
            .map_or_else(Route::default, |p| Route::new(&p.path, &p.articulate, &p.mpe))
    }).collect()
}
/// What the audio thread mixes by, from the persisted rack.
pub(crate) fn mix(selection: &Selection) -> Mix {
    Mix {
        parts: rack_controls(selection),
        buses: std::array::from_fn(|n| {
            let b = selection.bus(n);
            BusControls {
                gain: db_gain(b.gain),
                pan: if b.pan.is_finite() { b.pan.clamp(-1., 1.) } else { 0. },
                mute: b.mute,
                solo: b.solo,
                port: if (0..BUSES as i16).contains(&b.port) { b.port as u8 } else { n as u8 },
            }
        }),
    }
}
pub(crate) fn rack_controls(selection: &Selection) -> Vec<PartControls> {
    (0..selection.parts.len().max(RACK_SLOTS)).map(|n| {
        selection
            .parts
            .get(n)
            .map(|p| PartControls {
                port: p.port.min(3),
                output: p.output.min(BUSES as u8 - 1),
                channel: p.channel.clamp(-1, 15),
                gain: db_gain(p.gain),
                pan: if p.pan.is_finite() {
                    p.pan.clamp(-1., 1.)
                } else {
                    0.
                },
                tune: if p.tune.is_finite() {
                    p.tune.clamp(-TUNE_RANGE, TUNE_RANGE)
                } else {
                    0.
                },
                mute: p.mute,
                solo: p.solo,
                aux: if (0..BUSES as i16).contains(&p.aux) { p.aux as u8 } else { NO_AUX },
                aux_gain: db_gain(p.aux_gain),
                outs: std::array::from_fn(|c| match p.mic_buses.get(c) {
                    Some(&b) if (0..BUSES as i16).contains(&b) => b as u8,
                    _ => NO_AUX,
                }),
            })
            .unwrap_or_default()
    }).collect()
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
/// A script control edit for the runtime of `part` tagged `epoch`.
// Inline paths keep selection delivery and stale-epoch rejection free of audio allocations/frees.
struct FileSelection {
    part: usize, epoch: u64, slot: usize, control: usize,
    path: [u8; 1280], len: usize,
}

struct Edit {
    part: usize,
    epoch: u64,
    slot: usize,
    control: usize,
    value: i32,
}
/// Loader-built state for one rack slot, installed by the audio thread.
struct IrHandoff {
    ir: Option<crate::fx::PreparedIr>,
    epoch: u64,
    rate: f64,
    request: crate::engine::IrRequest,
}

enum Handoff {
    #[cfg(feature = "uvi")]
    Uvi(uvi_control::Audio),
    /// A new instrument, or an empty slot, with its effects and initialized scripts.
    Part {
        bank: Option<Box<Bank>>,
        fx: FxProcessor,
        script: Option<Box<Runtime>>,
        epoch: u64,
    },
    /// Effects rebuilt for a new host sample rate; the bank stays.
    Fx(FxProcessor),
    /// Restored scripts; synchronous zone init may also prepare new geometry.
    Script {
        script: Option<Box<Runtime>>,
        bank: Option<Box<Bank>>,
        epoch: u64,
    },
    /// The part's samples loaded whole (RAM only): replaces its streaming
    /// bank under the playing voices.
    Bank(Box<Bank>),
    /// Sample heads the smart memory resized.
    Heads(Heads),
}
impl Handoff {
    fn replaces_player(&self) -> bool {
        match self {
            Self::Part { .. } => true,
            #[cfg(feature = "uvi")]
            Self::Uvi(_) => true,
            _ => false,
        }
    }
}
/// What the audio thread replaced, freed on the loader thread.
#[expect(dead_code, reason = "held only to be dropped off the audio thread")]
#[derive(Default)]
struct Retired {
    #[cfg(feature = "uvi")]
    uvi: Option<uvi_control::Audio>,
    #[cfg(feature = "uvi")]
    uvi_delays: Option<Box<uvi_delay::Prepared>>,
    bank: Option<Box<Bank>>,
    fx: Option<FxProcessor>,
    script: Option<Box<Runtime>>,
    heads: Option<Heads>,
    ir: Option<crate::fx::PreparedIr>,
    growth: Option<Box<PreparedGrowth>>,
    controls: Option<Mix>,
    routes: Option<Vec<Route>>,
    plan: Option<Plan>,
    diagnostic: Option<AudioDiagnostics>,
    zone: Option<Box<crate::engine::zone::Prepared>>,
    zone_preload: Option<(usize,u64,u64,Box<Bank>)>,
}

type ZoneRequest = (usize,u64,u64,crate::engine::zone::Job);
type ZoneReady = (usize,u64,u64,Box<crate::engine::zone::Prepared>);
struct ZoneChain {
    part: usize, generation: u64, epoch: u64,
    context: std::sync::Weak<crate::engine::zone::Context>,
    state: Arc<crate::engine::zone::State>,
}

/// The serialized service worker batches consecutive edits without discarding
/// their unique completion IDs. Playback preparation never runs on audio.
fn load_zone_maps(params: &SamplerParams) {
    let current = |part: usize,generation: u64,epoch: u64| {
        let view = params.shared.view.lock().unwrap();
        view.parts.get(part).is_some_and(|v| v.script_epoch == epoch
            && params.shared.part(part).is_some_and(|p|p.generation.load(Ordering::Acquire)==generation)
            && v.instrument.as_ref().is_some_and(|i|crate::cache::current(&i.dependencies)))
    };
    let mut chains = params.shared.zone_chains.lock().unwrap();
    chains.retain(|chain| chain.context.strong_count() != 0 && current(chain.part,chain.generation,chain.epoch));
    while !params.shared.zone_ready.is_full() {
        let first = params.shared.zone_pending.lock().unwrap().take().or_else(||params.shared.zone_requests.pop());
        let Some((part,generation,epoch,job)) = first else { break };
        if !current(part,generation,epoch) { continue; }
        let context = Arc::downgrade(&job.context);
        let mut jobs = vec![job];
        while jobs.len() < crate::engine::zone::QUEUE {
            let Some((p,g,e,next)) = params.shared.zone_requests.pop() else { break };
            if (p,g,e)==(part,generation,epoch) && Arc::ptr_eq(&next.context,&jobs[0].context) { jobs.push(next); }
            else { *params.shared.zone_pending.lock().unwrap() = Some((p,g,e,next)); break; }
        }
        let chain = chains.iter().position(|c| (c.part,c.generation,c.epoch)==(part,generation,epoch) && c.context.ptr_eq(&context));
        let base = chain.map_or_else(||jobs[0].base.clone(),|i|chains[i].state.clone());
        let prepared = crate::engine::zone::Prepared::build(jobs,base,&||!current(part,generation,epoch));
        if !current(part,generation,epoch) { continue; }
        if let Some(error) = &prepared.error {
            let source = params.shared.view.lock().unwrap().parts[part].instrument.clone();
            if let Some(source) = source { crate::diagnostics::resource(&source.path,"zone mapping",error); }
        }
        if prepared.success {
            let state = prepared.resulting_state();
            match chain {
                Some(i) => chains[i].state = state,
                None => chains.push(ZoneChain {part,generation,epoch,context,state}),
            }
        }
        params.shared.zone_ready.push((part,generation,epoch,prepared)).ok().unwrap();
    }
}

/// Installation and callback delivery are distinct: every status1 follows the
/// snapshot swap. Limit delivery to64 IDs per block, retaining the rest.
fn finish_zone_maps(s: &mut Dsp,p: &SamplerParams) {
    if s.zone_completion.is_none() && !p.shared.discard.is_full() {
        s.zone_completion = p.shared.zone_ready.pop();
    }
    let Some((part,generation,epoch,prepared)) = &mut s.zone_completion else { return };
    let current = s.rack.parts.get(*part).is_some() && *epoch == s.script_epoch[*part]
        && part_atoms(&s.shared_parts,&p.shared,*part).is_some_and(|p|p.generation.load(Ordering::Acquire)==*generation)
        && s.installed_generation[*part] == *generation;
    if !current {
        if !p.shared.discard.is_full() {
            let (_,_,_,prepared) = s.zone_completion.take().unwrap();
            p.shared.discard.push(Retired {zone:Some(prepared),..Default::default()}).ok().unwrap();
        }
        return;
    }
    if !prepared.installed && prepared.completed == 0 {
        if p.shared.discard.is_full() { return; }
        prepared.success = s.rack.parts[*part].install_zone_map(prepared);
        // Mark a failed attempt too, so it never installs after callbacks began.
        prepared.installed = true;
    }
    for _ in 0..64 {
        let Some(edit) = prepared.edits.get(prepared.completed).copied() else { break };
        if !s.rack.parts[*part].finish_zone_edit(edit,prepared.success) { break; }
        prepared.completed += 1;
    }
    if prepared.completed == prepared.edits.len() && !p.shared.discard.is_full() {
        let (_,_,_,prepared) = s.zone_completion.take().unwrap();
        p.shared.discard.push(Retired {zone:Some(prepared),..Default::default()}).ok().unwrap();
    }
}

/// Array reads, writes and disposal share the serialized file-service worker.
fn load_arrays(params: &SamplerParams) {
    let current = |part: usize, generation: u64, epoch: u64| {
        generation == params.shared.part(part).unwrap().generation.load(Ordering::Acquire)
            && params.shared.view.lock().unwrap().parts[part].script_epoch == epoch
    };
    while !params.shared.array_ready.is_full() {
        let Some((part, generation, epoch, mut request)) = params.shared.array_retired.pop() else { break };
        if !current(part, generation, epoch) { continue }
        if request.error() == Some("load_array_str: prepared string capacity exceeded") {
            let source = params.shared.view.lock().unwrap().parts[part].instrument.clone();
            if let Some(source) = source {
                crate::diagnostics::resource(&source.path, request.path(), request.error().unwrap());
            }
        }
        request.recycle();
        params.shared.array_ready.push((part, generation, epoch, request)).ok().unwrap();
    }
    while !params.shared.array_ready.is_full() {
        let Some((part, generation, epoch, mut request)) = params.shared.array_requests.pop() else { break };
        if !current(part, generation, epoch) {
            if request.is_write() {
                crate::diagnostics::event(crate::diagnostics::LogLevel::Info,"ksp","array-save-canceled",
                    serde_json::json!({"path":request.path(),"part":part,"generation":generation,"script_epoch":epoch}));
            }
            continue
        }
        let source = params.shared.view.lock().unwrap().parts[part].instrument.clone();
        let success = request.perform();
        if !success {
            if let Some(source) = &source {
                crate::diagnostics::resource(&source.path, request.path(), request.error().unwrap_or("NKA file operation failed"));
            }
        }
        // A save already committed remains a real file write if its runtime is
        // replaced during I/O. Record that outcome; never deliver it to a new slot.
        let still_current = current(part,generation,epoch);
        if request.is_write() && success {
            crate::diagnostics::event(crate::diagnostics::LogLevel::Info,"ksp","array-file-saved",
                serde_json::json!({"path":request.path(),"part":part,"generation":generation,
                    "script_epoch":epoch,"completion_stale":!still_current}));
        }
        if !still_current { continue }
        params.shared.array_ready.push((part, generation, epoch, request)).ok().unwrap();
    }
}

/// Resolve and build requested convolution replacements off the audio thread.
fn load_irs(params: &SamplerParams) {
    while !params.shared.ir_ready.is_full() {
        let Some((part, generation, epoch, request)) = params.shared.ir_requests.pop() else { break };
        let source = {
            let view = params.shared.view.lock().unwrap();
            let v = &view.parts[part];
            (v.script_epoch == epoch && generation == params.shared.part(part).unwrap().generation.load(Ordering::Acquire))
                .then(|| v.instrument.clone().map(|i| (i, v.irs.clone(), v.program))).flatten()
        };
        let Some((instrument, mut irs, program)) = source else { continue };
        let rate = params.shared.rate();
        let mut resolved = None;
        let load = if request.id < 0 { Ok(None) } else {
            match crate::resources::ir_sample(&instrument.path, request.file()) {
                Some(file) => {
                    resolved = Some(file.clone());
                    crate::fx::ScriptIr::load(request.rack, request.slot, file)
                        .map(Some).map_err(|e| format!("{e:#}"))
                }
                None => Err("impulse response could not be resolved in the library resources".into()),
            }
        };
        let report = |code, error: &str| {
            crate::diagnostics::resource(&instrument.path, request.file(), error);
            crate::diagnostics::runtime(&instrument.path, program, part, epoch, None, &[serde_json::json!({
                "code":code, "message":error, "requested":request.file(), "resolved":resolved,
                "rack":format!("{:?}", request.rack), "slot":request.slot,
            })]);
        };
        let ir = match load {
            Ok(loaded) => {
                if let Some(loaded) = loaded {
                    irs.retain(|l| (l.rack, l.slot) != (request.rack, request.slot) || !matches!(l.load, crate::fx::Load::Ir { .. }));
                    irs.push(loaded);
                }
                irs.retain(|l| (l.rack, l.slot) != (request.rack, request.slot) || !matches!(l.load, crate::fx::Load::Convolution(_)));
                irs.push(crate::fx::ScriptIr { rack: request.rack, slot: request.slot, load: crate::fx::Load::Convolution(request.settings) });
                if request.settings.values[1] != request.settings.values[2] {
                    crate::diagnostics::runtime(&instrument.path, program, part, epoch, None, &[serde_json::json!({
                        "code":"convolution_uniform_size", "message":"Separate early/late boundary is unavailable; the last size edit stretches the whole impulse response",
                        "rack":format!("{:?}", request.rack), "slot":request.slot, "values":request.settings.values,
                    })]);
                }
                let ir = instrument.fx.prepare_ir(request.rack, request.slot, rate as f32, MAX_BLOCK, &irs);
                if ir.is_none() { report("ir_build_failed", "convolution slot or its impulse response is unavailable"); }
                ir
            }
            Err(error) => { report("ir_load_failed", &error); None }
        };
        let mut view = params.shared.view.lock().unwrap();
        let v = &mut view.parts[part];
        if v.script_epoch != epoch || generation != params.shared.part(part).unwrap().generation.load(Ordering::Acquire) {
            continue;
        }
        let loaded = ir.is_some();
        // Only this serialized worker produces; the audio thread only pops.
        params.shared.ir_ready.push((part, generation, IrHandoff { ir, epoch, rate, request })).ok().unwrap();
        if loaded { v.irs = irs; }
    }
}
/// Persistent script values to restore: the host's saved state, else the instrument's.
fn persisted(saved: &str, i: &Instrument) -> Vec<Persisted> {
    serde_json::from_str(saved).unwrap_or_else(|_| i.script_state.clone())
}
#[derive(Clone)]
pub(crate) struct PersistenceSnapshot {
    script: Vec<Persisted>,
    /// Worker-prepared slot list: audio updates values in place, including
    /// edits whose new kernel has not finished building yet.
    ir: Vec<crate::fx::IrSlotSettings>,
    native: crate::engine::native_state::NativeSnapshot,
}
/// Initialize `i`'s scripts off the audio thread, with a snapshot buffer shaped for them.
fn scripts(
    i: &Instrument,
    saved: &str,
    ir_settings: &[crate::fx::IrSlotSettings],
    engine_state: &[crate::ksp::engine::NativeEdit],
    rate: f64,
) -> (
    Option<Box<Runtime>>,
    Option<Box<PersistenceSnapshot>>,
    Vec<String>,
) {
    scripts_with_delays(i, saved, ir_settings, engine_state, &[], rate)
}

fn scripts_with_delays(
    i: &Instrument, saved: &str, ir_settings: &[crate::fx::IrSlotSettings],
    engine_state: &[crate::ksp::engine::NativeEdit], delay_state: &[crate::fx::DelayState], rate: f64,
) -> (Option<Box<Runtime>>, Option<Box<PersistenceSnapshot>>, Vec<String>) {
    let (script, errors) = crate::engine::load_scripts_with_delay_state(i, persisted(saved, i), rate, ir_settings, engine_state, delay_state);
    // Nothing persistent: no snapshots to trade with the audio thread.
    let snapshot = script
        .as_ref()
        .map(|rt| PersistenceSnapshot { script: rt.persistence(), ir: i.fx.ir_settings_with(&rt.init_irs), native: rt.native_state.snapshot() })
        .filter(|s| !s.native.is_empty() || !s.ir.is_empty() || s.script.iter().any(|p| !p.is_empty()))
        .map(Box::new);
    (script, snapshot, errors)
}
/// Epoch for `script`, about to be handed off from `slot`; forgets the old runtime's buffers.
fn next_epoch(
    view: &mut View,
    slot: usize,
    snapshot: Option<Box<PersistenceSnapshot>>,
    live: Option<Box<Live>>,
) -> u64 {
    view.script_epoch += 1;
    let v = &mut view.parts[slot];
    v.script_epoch = view.script_epoch;
    v.snapshot = snapshot;
    v.snapshot_lent = None;
    v.live = live;
    v.live_revisions = None;
    v.live_control_versions = Arc::default();
    v.live_diagnostics = None;
    v.diagnostics_dirty = false;
    view.script_epoch
}
/// Copies prepared outside the view lock. A delta owns only changed controls;
/// the unique published interface can donate its unchanged controls allocation.
struct InterfaceUpdate {
    interface: Option<Interface>,
    rows: Option<Vec<(usize, crate::ksp::Control)>>,
    versions: Arc<[(u64, u64)]>,
}

fn prepare_interface(live: &Live, previous: Option<&Interface>, versions: &[(u64, u64)]) -> InterfaceUpdate {
    let next: Arc<[(u64, u64)]> = live.control_versions().collect::<Vec<_>>().into();
    let Some(source) = &live.interface else {
        return InterfaceUpdate { interface: None, rows: None, versions: next };
    };
    let Some(previous) = previous.filter(|old|
        old.controls.len() == source.controls.len() && versions.len() == source.controls.len()
            && next.len() == source.controls.len()) else {
        return InterfaceUpdate { interface: Some(source.clone()), rows: None, versions: next };
    };
    let rows = source.controls.iter().zip(&previous.controls).enumerate().filter_map(|(n, (new, old))| {
        // Visible menu rows depend on arbitrary script variables, not just the
        // control's metadata/value stamps. No full property-map comparisons.
        (versions[n] != next[n] || old.menu != new.menu).then(|| (n, new.clone()))
    }).collect();
    let header = Interface {
        performance: source.performance, width: source.width, height: source.height,
        title: source.title.clone(), wallpaper: source.wallpaper.clone(),
        wallpaper_state: source.wallpaper_state, skin_offset: source.skin_offset,
        background_color: source.background_color,
        fonts: source.fonts.clone(), controls: Vec::new(),
        diagnostics: source.diagnostics.clone(), listeners: source.listeners.clone(),
    };
    InterfaceUpdate { interface: Some(header), rows: Some(rows), versions: next }
}

impl Shared {
    #[cfg(feature = "uvi")]
    pub(crate) fn uvi_activation_epoch(&self) -> u64 {
        self.uvi_epoch.load(Ordering::Acquire)
    }
    #[cfg(feature = "uvi")]
    pub(crate) fn edit_uvi(&self, slot: usize, stamp: crate::uvi::worker::Stamp, edit: crate::uvi::host::UiEdit) -> bool {
        if stamp.epoch != self.uvi_epoch.load(Ordering::Acquire)
            || self.part(slot).is_none_or(|p| p.uvi_generation.load(Ordering::Acquire) != stamp.generation
                || p.uvi_failed.load(Ordering::Acquire)
                || p.uvi_part_generation.load(Ordering::Acquire) != p.generation.load(Ordering::Acquire)) {
            return false;
        }
        self.uvi_edits.push((slot, stamp, edit)).is_ok()
    }
    /// Publish visible script state on the editor thread, with a worker fallback
    /// after the editor closes. Neither caller may keep this guard while loading.
    pub(crate) fn publish_live(&self, editor: bool) {
        let Ok(mut owner) = self.live_publish.try_lock() else { return };
        let now = Instant::now();
        if editor {
            owner.0 = Some(now);
            self.view.lock().unwrap().watched_at = Some(now);
        } else {
            if self.watched.swap(false, Ordering::Relaxed) {
                self.view.lock().unwrap().watched_at = Some(now);
            }
            if owner.0.is_some_and(|at| at.elapsed() < LIVE_WATCH) { return; }
        }
        // One completed interface per editor frame. Prepare changed rows off
        // the view lock; keep retained reader snapshots immutable. Unseen
        // diagnostics may drain the bounded rack queue in one worker pass.
        for _ in 0..if editor { 1 } else { self.view.lock().unwrap().parts.len() } {
            let Some((slot, epoch, live)) = self.lives.pop() else { break };
            let revisions = live.revisions();
            let (keys_changed, previous, previous_diagnostics) = {
                let view = self.view.lock().unwrap();
                let v = &view.parts[slot];
                if epoch == 0 || epoch != v.script_epoch { continue; }
                if !v.script_pages.views.is_empty() && v.script_slot != live.slot {
                    v.script_pages.buffers.lock().unwrap().push(live);
                    continue;
                }
                let changed = live.refresh_interface && v.live_revisions.is_none_or(|old| old.0 != revisions.0);
                (live.refresh_interface && v.live_revisions.is_none_or(|old| old.1 != revisions.1),
                    changed.then(|| (v.interface.clone(), if v.live_revisions.is_some() && v.script_slot == live.slot { v.live_control_versions.clone() } else { Arc::default() })), v.live_diagnostics.clone())
            };
            // This temporary reader exists only during off-lock preparation;
            // release it before testing whether the published Arc is unique.
            let mut interface = previous.map(|(old, versions)| {
                prepare_interface(&live, old.as_deref(), &versions)
            });
            let keys = keys_changed.then(|| Arc::new(live.keys.clone()));
            let diagnostics = previous_diagnostics.filter(|old|
                old.epoch == epoch && old.faults == live.faults && old.fault_occurrences_omitted == live.fault_occurrences_omitted && old.notes == live.notes
            ).unwrap_or_else(|| Arc::new(LiveDiagnostics {
                epoch, faults: live.faults.clone(), fault_occurrences_omitted: live.fault_occurrences_omitted, notes: live.notes.clone(),
            }));
            #[cfg(test)]
            if let Some(gate) = self.publish_gate.lock().unwrap().take() {
                gate.wait();
                gate.wait();
            }
            // Drop replaced maps, strings and old snapshots after unlocking.
            let mut retired_rows = Vec::with_capacity(interface.as_ref().and_then(|u| u.rows.as_ref()).map_or(0, Vec::len));
            let mut retired_interface = None;
            let mut retired_arc = None;
            let mut retired_versions = None;
            #[cfg(test)]
            let mut publication_rows = None;
            let mut view = self.view.lock().unwrap();
            if epoch == 0 || epoch != view.parts[slot].script_epoch { continue; }
            if !view.parts[slot].script_pages.views.is_empty() && view.parts[slot].script_slot != live.slot {
                view.parts[slot].script_pages.buffers.lock().unwrap().push(live);
                continue;
            }
            if let Some(update) = &mut interface {
                if let Some(rows) = update.rows.take() {
                    let previous = view.parts[slot].interface.take();
                    match previous.map(Arc::try_unwrap) {
                        Some(Ok(mut old)) => {
                            let new = update.interface.as_mut().expect("delta has an interface");
                            new.controls = std::mem::take(&mut old.controls);
                            #[cfg(test)]
                            { publication_rows = Some((rows.len(), new.controls.len(), true)); }
                            for (n, control) in rows {
                                retired_rows.push(std::mem::replace(&mut new.controls[n], control));
                            }
                            retired_interface = Some(old);
                        }
                        previous => {
                            // A reader still owns the old snapshot. Restore it
                            // immediately, then copy the fallback outside the lock.
                            view.parts[slot].interface = previous.and_then(Result::err);
                            drop(view);
                            retired_rows.extend(rows.into_iter().map(|(_, control)| control));
                            update.interface = live.interface.clone();
                            view = self.view.lock().unwrap();
                            if epoch == 0 || epoch != view.parts[slot].script_epoch { continue; }
                            if !view.parts[slot].script_pages.views.is_empty() && view.parts[slot].script_slot != live.slot {
                                view.parts[slot].script_pages.buffers.lock().unwrap().push(live);
                                continue;
                            }
                        }
                    }
                }
                #[cfg(test)]
                if publication_rows.is_none() {
                    let count = update.interface.as_ref().map_or(0, |i| i.controls.len());
                    publication_rows = Some((count, count, false));
                }
                let new = update.interface.take().map(Arc::new);
                retired_arc = std::mem::replace(&mut view.parts[slot].interface, new);
                retired_versions = Some(std::mem::replace(&mut view.parts[slot].live_control_versions, update.versions.clone()));
            }
            let v = &mut view.parts[slot];
            #[cfg(test)]
            if publication_rows.is_some() { v.publication_rows = publication_rows; }
            if live.refresh_interface {
                // Pending edits are a view overlay. Matching callbacks acknowledge
                // them; refused values become visible when their grace expires.
                settle_edits(&mut v.edited, live.interface.as_ref());
            }
            if live.refresh_interface { v.live_revisions = Some(revisions); }
            if let Some(keys) = keys { v.keys = keys; }
            if v.live_diagnostics.as_ref().is_none_or(|old| !Arc::ptr_eq(old, &diagnostics)) {
                v.live_diagnostics = Some(diagnostics);
                v.diagnostics_dirty = true;
            }
            v.script_slot = live.slot;
            v.live = Some(live);
            drop(view);
            drop((retired_rows, retired_interface, retired_arc, retired_versions));
        }
        let mut view = self.view.lock().unwrap();
        let shown = view.watched_at.is_some_and(|at| at.elapsed() < LIVE_WATCH);
        let count = view.parts.len().min(self.grown.load(Ordering::Acquire) as usize);
        if count == 0 { return }
        let start = owner.1 % count;
        for step in 0..count {
            let slot = (start + step) % count;
            owner.1 = (slot + 1) % count;
            let v = &mut view.parts[slot];
            if v.live.is_none() {
                let mut buffers = v.script_pages.buffers.lock().unwrap();
                if let Some(at) = buffers.iter().position(|live| live.slot == v.script_slot) {
                    let mut live = buffers.swap_remove(at);
                    live.interface_current = false;
                    v.live = Some(live);
                }
            }
            if (shown || v.diagnostics_lent.is_none_or(|at| at.elapsed().as_secs() >= 1))
                && let Some(mut live) = v.live.take()
            {
                live.refresh_interface = shown;
                match self.live_requests.push((slot, v.script_epoch, live)) {
                    Ok(()) => v.diagnostics_lent = Some(now),
                    Err((_, _, live)) => {
                        v.live = Some(live);
                        owner.1 = slot;
                        break;
                    }
                }
            }
        }
    }

    pub(crate) fn select_script_page(&self, part: usize, epoch: u64, slot: usize) -> bool {
        let mut view = self.view.lock().unwrap();
        let Some(v) = view.parts.get_mut(part) else { return false };
        if epoch == 0 || epoch != v.script_epoch { return false; }
        let Some(page) = v.script_pages.views.iter().find(|p| p.slot == slot) else { return false };
        if v.script_slot == slot { return false; }
        let next = page.last_view.lock().unwrap().take().unwrap_or_else(|| page.interface.clone());
        let retired = v.interface.replace(next);
        let retired = if let Some(previous) = v.script_pages.views.iter().find(|p| p.slot == v.script_slot) {
            std::mem::replace(&mut *previous.last_view.lock().unwrap(), retired)
        } else { retired };
        v.script_slot = slot;
        v.live_revisions = None;
        v.live_control_versions = Arc::default();
        v.edited.clear();
        if let Some(live) = v.live.take() { v.script_pages.buffers.lock().unwrap().push(live); }
        drop(view);
        drop(retired);
        true
    }

    /// Formatting and journal I/O stay on Load; a busy loader cannot retain
    /// the live view's reusable audio buffer while the editor is open.
    fn drain_live_diagnostics(&self) {
        for slot in 0..self.with_parts(|p| p.len()) {
            let pending = {
                let mut view = self.view.lock().unwrap();
                let v = &mut view.parts[slot];
                if !std::mem::take(&mut v.diagnostics_dirty) { continue; }
                v.live_diagnostics.clone().filter(|d| d.epoch != 0 && d.epoch == v.script_epoch)
                    .zip(v.load_report.clone())
                    .map(|(diagnostics, report)| (diagnostics, report,
                        v.instrument.clone(), v.program))
            };
            let Some((diagnostics, previous, instrument, program)) = pending else { continue };
            let mut runtime = serde_json::json!({"faults":diagnostics.faults,"fault_occurrences_omitted":diagnostics.fault_occurrences_omitted,"notes":diagnostics.notes});
            if let Some(instrument) = &instrument {
                for fault in runtime["faults"].as_array_mut().into_iter().flatten() {
                    if let Some(excerpt) = previous["runtime"]["faults"].as_array().into_iter().flatten()
                        .find(|old| old["slot"] == fault["slot"] && old["line"] == fault["line"])
                        .and_then(|old| old.get("source_excerpt"))
                    {
                        fault["source_excerpt"] = excerpt.clone();
                        continue;
                    }
                    if let (Some(slot), Some(line)) = (fault["slot"].as_u64(), fault["line"].as_u64())
                        && let Some(source) = slot.checked_sub(1).and_then(|slot| instrument.scripts.get(slot as usize))
                        && let Some(excerpt) = crate::diagnostics::script_excerpt(source, slot as u32, line as u32, None)
                    { fault["source_excerpt"] = excerpt; }
                }
            }
            if previous["runtime"] == runtime { continue; }
            let mut new_issues = Vec::new();
            for fault in runtime["faults"].as_array().into_iter().flatten() {
                let known = previous["runtime"]["faults"].as_array().into_iter().flatten().any(|old|
                    old["slot"] == fault["slot"] && old["line"] == fault["line"] && old["message"] == fault["message"] && old["context"] == fault["context"]
                );
                if !known { new_issues.push(fault.clone()); }
            }
            for note in runtime["notes"].as_array().into_iter().flatten() {
                if !previous["runtime"]["notes"].as_array().is_some_and(|old| old.contains(note)) {
                    new_issues.push(serde_json::json!({"message":note}));
                }
            }
            let omitted = (diagnostics.fault_occurrences_omitted != 0).then(|| format!("KSP fault diagnostics omitted {} executions at additional locations", diagnostics.fault_occurrences_omitted));
            if let Some(message) = &omitted
                && previous["runtime"]["fault_occurrences_omitted"].as_u64().unwrap_or(0) != diagnostics.fault_occurrences_omitted
            {
                new_issues.push(serde_json::json!({"code":"fault_diagnostics_omitted","message":message,"fault_occurrences_omitted":diagnostics.fault_occurrences_omitted}));
            }
            let status = diagnostics.faults.iter().map(|f| f.to_string())
                .chain(diagnostics.notes.iter().map(|n| (*n).to_owned())).chain(omitted).collect::<Vec<_>>().join("\n");
            let load_id = previous["script_restore"]["load_id"].as_str().or_else(|| previous["load_id"].as_str()).map(str::to_owned);
            let mut report = (*previous).clone();
            if report["status"] == "loaded" && (!diagnostics.faults.is_empty() || diagnostics.fault_occurrences_omitted != 0 || !diagnostics.notes.is_empty()) {
                report["status"] = "partial".into();
            }
            report["runtime"] = runtime;
            let report = Arc::new(report);
            let mut view = self.view.lock().unwrap();
            let v = &mut view.parts[slot];
            if v.script_epoch != diagnostics.epoch
                || v.live_diagnostics.as_ref().is_none_or(|latest| !Arc::ptr_eq(latest, &diagnostics))
                || v.load_report.as_ref().is_none_or(|latest| !Arc::ptr_eq(latest, &previous))
            {
                if v.script_epoch == diagnostics.epoch { v.diagnostics_dirty = true; }
                continue;
            }
            v.load_report = Some(report);
            v.runtime_status = status;
            drop(view);
            if let Some(instrument) = instrument { crate::diagnostics::runtime(&instrument.path, program, slot, diagnostics.epoch, load_id.as_deref(), &new_issues); }
        }
    }

    fn rate(&self) -> f64 {
        f64::from_bits(self.rate.load(Ordering::Acquire))
    }

    /// Set script control `control` of `part`'s performance view and run its
    /// `on ui_control`; the view shows the value until the scripts report back.
    pub(crate) fn select_control_file(&self, part: usize, epoch: u64, slot: usize, control: usize, path: &str) -> bool {
        if path.len() > 1280 || path.chars().count() > 320 { return false }
        let view = self.view.lock().unwrap();
        let Some(v) = view.parts.get(part) else { return false };
        if epoch == 0 || v.script_epoch != epoch || v.script_slot != slot
            || !v.interface.as_ref().and_then(|u| u.controls.get(control)).is_some_and(|c| c.kind == "ui_file_selector") { return false }
        let mut selected = FileSelection { part, epoch, slot, control, path: [0; 1280], len: path.len() };
        selected.path[..path.len()].copy_from_slice(path.as_bytes());
        self.file_selections.push(selected).is_ok()
    }

    pub(crate) fn edit_control(&self, part: usize, control: usize, value: i32) {
        if part >= self.grown.load(Ordering::Acquire) as usize { return }
        let mut view = self.view.lock().unwrap();
        let Some(v) = view.parts.get_mut(part) else { return };
        let edit = Edit {
            part,
            epoch: v.script_epoch,
            slot: v.script_slot,
            control,
            value,
        };
        if self.edits.push(edit).is_err() {
            return;
        }
        v.edited.retain(|e| e.0 != control);
        v.edited.push((control, value, Instant::now()));
    }

    /// Send the audio thread what changed in the parts' edits since last
    /// sent. A full queue leaves the slot to be sent again next time:
    /// setting an override is idempotent.
    pub(crate) fn sync_overrides(&self, selection: &Selection) {
        let mut sent = self.sent.lock().unwrap();
        for (slot, had) in sent.iter_mut().enumerate().take(self.grown.load(Ordering::Acquire) as usize) {
            let want = selection.parts.get(slot).map_or(&[][..], |p| &p.edits.0[..]);
            if want == &had[..] {
                continue;
            }
            let gone = had
                .iter()
                .filter(|o| !want.iter().any(|w| (w.group, w.param) == (o.group, o.param)))
                .map(|o| Override { offset: 0.0, ..*o });
            let new = want.iter().filter(|w| !had.contains(w)).copied();
            let all = gone.chain(new).all(|o| self.overrides.push((slot, o)).is_ok());
            if all {
                *had = want.to_vec();
            }
        }
    }

    /// Queue a snapshot against this exact source. Parsing and mutation stay
    /// on the existing worker; a later source request cancels its result.
    pub(crate) fn queue_snapshot(&self, slot: usize, part: &Part, path: String) -> bool {
        if !part.snapshot_base()
        { return false; }
        let mut request = self.snapshot_request.lock().unwrap();
        let Some(atoms) = self.part(slot) else { return false };
        let generation = atoms.generation.load(Ordering::Acquire);
        *request = Some(SnapshotRequest { slot, source: part.source(), path, generation });
        true
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
        self.key_owners[note as usize].store(slot as u64, Ordering::Release);
        self.played[note as usize].store(velocity, Ordering::Relaxed);
        if self.keyboard.push((slot, Play::Note(note, velocity))).is_err() {
            self.panic.store(true, Ordering::Release);
        }
    }

    /// Stop `note` on whichever slot the on-screen keyboard started it.
    pub(crate) fn release_key(&self, note: u8) {
        let was_down = self.played[note as usize].swap(0, Ordering::AcqRel) != 0;
        let owner = self.key_owners[note as usize].load(Ordering::Acquire);
        if was_down
            && self.keyboard.push((owner as usize, Play::Note(note, 0))).is_err()
        {
            self.panic.store(true, Ordering::Release);
        }
    }

    fn reset_midi(&self) {
        while self.keyboard.pop().is_some() {}
        for owner in &self.key_owners { owner.store(0, Ordering::Release); }
        for lit in self.played.iter().chain(&self.heard) { lit.store(0, Ordering::Relaxed); }
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

    /// Play `note` (or the selected part's first root) for a moment.
    pub(crate) fn audition(&self, note: Option<u8>) {
        if let Some(note) = note {
            self.audition_note.store(u64::from(note), Ordering::Relaxed);
        }
        self.audition.store(true, Ordering::Release);
    }
}
/// A rack KONTRA saved: a `.kontra-multi` file, JSON. An NKM would have
/// to embed every instrument's program, which nothing here writes, so this
/// names the instruments instead:
///
/// ```json
/// { "format": "kontra-multi", "version": 1, "name": "Evening",
///   "parts": [ { "path": "/…/Piano.nki", "program": 0,
///                "channel": -1, "port": 0, "output": 0,
///                "gain": 0.0, "pan": 0.0, "tune": 0.0,
///                "mute": false, "solo": false, "collapsed": false,
///                "name": "", "group": 4294967295, "script_state": "" } ] }
/// ```
///
/// Parts are in rack order. `channel` is 0..=15 or -1 for omni; `port`
/// 0..=3 is MIDI port A..D; `output` 0..=7 the stereo output; `gain` dB
/// (-60..=6); `pan` -1..=1; `tune` semitones with cents as the fraction
/// (±36); `program` picks the program inside an NKM `path`; `name` empty
/// shows the instrument's; `group` `u32::MAX` means every group;
/// `script_state` holds the script controls' persistent values as JSON,
/// empty for the instrument's own. A missing field takes its default.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SavedMulti {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub parts: Vec<Part>,
}

impl SavedMulti {
    const FORMAT: &str = "kontra-multi";

    /// The rack in `selection`, in its order.
    pub fn of(name: &str, selection: &Selection) -> Self {
        Self {
            format: Self::FORMAT.into(),
            version: 1,
            name: name.into(),
            parts: selection
                .order
                .iter()
                .filter_map(|n| selection.parts.get(*n as usize))
                .filter(|p| !p.is_empty())
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
        anyhow::ensure!(
            [Self::FORMAT, "kontakto-multi"].contains(&multi.format.as_str()),
            "Not a KONTRA multi"
        );
        anyhow::ensure!(multi.version <= 1, "Saved by a newer KONTRA");
        Ok(multi)
    }
}

fn trace_effects(trace: &mut crate::diagnostics::LoadTrace, instrument: &Instrument) {
    for warning in instrument.fx.warnings() { trace.issue("effects", "unsupported_effect", warning); }
    for (group, g) in instrument.groups.iter().enumerate() {
        for warning in crate::engine::filter::unsupported_at(&g.fx, g.amp_split_slot) {
            trace.issue("effects", "unsupported_group_effect", format!("Group {group} ({}): {warning}", g.name));
        }
    }
}

fn drain_audio_diagnostics(params: &SamplerParams) {
    // Report/export, Load and the independent snapshot lane may all drain.
    // Serialize dequeue with baseline updates so an older snapshot cannot
    // overtake the newer one whose cumulative counters it should precede.
    let mut latest = params.shared.diagnostic_latest.lock().unwrap();
    while let Some(audio) = params.shared.diagnostic_audio.pop() {
        if latest.as_ref().is_some_and(|old| old.block >= audio.block) {
            let _ = params.shared.diagnostic_free.force_push(audio); continue
        }
        let previous = latest.replace(audio.clone());
        if previous.as_ref().is_none_or(|old| (old.sample_rate, old.block_size, old.output_channels, old.offline, old.output_buses)
            != (audio.sample_rate, audio.block_size, audio.output_channels, audio.offline, audio.output_buses)) {
            crate::diagnostics::event(crate::diagnostics::LogLevel::Info, "audio", "host_audio_config", serde_json::json!({
                "instance_id":params.shared.instance_id, "sample_rate":audio.sample_rate, "block_size":audio.block_size,
                "output_channels":audio.output_channels, "output_buses":audio.output_buses, "offline":audio.offline,
            }));
        }
        let brightness = audio.unsupported_note_brightness.saturating_sub(previous.as_ref().map_or(0, |old| old.unsupported_note_brightness));
        if brightness != 0 {
            crate::diagnostics::event(crate::diagnostics::LogLevel::Warning, "midi", "unsupported_note_brightness", serde_json::json!({
                "instance_id":params.shared.instance_id, "delta":brightness, "total":audio.unsupported_note_brightness,
                "reason":"Host note-expression brightness has no verified CC74 value law; an unlinked brightness event after event-buffer overflow also has unknown provenance. It was not applied to another note or to channel CC74.",
            }));
        }
        let ends=audio.host_note_end_rejections.saturating_sub(previous.as_ref().map_or(0,|old| old.host_note_end_rejections));
        if ends!=0 {
            crate::diagnostics::event(crate::diagnostics::LogLevel::Warning,"midi","host_note_end_rejected",serde_json::json!({
                "instance_id":params.shared.instance_id,"delta":ends,"total":audio.host_note_end_rejections,
                "reason":"The host output-event queue rejected NOTE_END. Sounding owners remain retained for retry; a no-sound switch/unmatched route could not return its identity.",
            }));
        }
        let aligned=audio.alignment_overflows.saturating_sub(previous.as_ref().map_or(0,|old| old.alignment_overflows));
        if aligned!=0 {
            crate::diagnostics::event(crate::diagnostics::LogLevel::Warning,"midi","alignment_queue_overflow",serde_json::json!({
                "instance_id":params.shared.instance_id,"delta":aligned,"total":audio.alignment_overflows,
                "reason":"A prepared alignment event queue or exact-owner list filled; excess events could not retain their calibrated delay/ownership.",
            }));
        }
        let unsupported = audio.unsupported_host_expression.saturating_sub(previous.as_ref().map_or(0, |old| old.unsupported_host_expression));
        if unsupported != 0 {
            crate::diagnostics::event(crate::diagnostics::LogLevel::Warning, "midi", "unsupported_host_expression", serde_json::json!({
                "instance_id":params.shared.instance_id, "delta":unsupported, "total":audio.unsupported_host_expression,
                "reason":"Native note event or expression has an unsupported or invalid value law/address, or its exact provenance was lost to input-buffer overflow. It was not projected onto another note's key row.",
            }));
        }
        for (part, current) in audio.parts.iter().enumerate() {
            // Rack slots retain their Engine across instrument/script reloads;
            // these counters have Engine lifetime, not generation lifetime.
            let before = previous.as_ref().and_then(|old| old.parts.get(part)).copied();
            let underruns = current.underruns.saturating_sub(before.map_or(0, |p| p.underruns));
            let dropped = current.dropped_commands.saturating_sub(before.map_or(0, |p| p.dropped_commands));
            let owners = current.host_note_drops.saturating_sub(before.map_or(0, |p| p.host_note_drops));
            if owners != 0 {
                crate::diagnostics::event(crate::diagnostics::LogLevel::Warning, "midi", "host_note_owner_exhausted", serde_json::json!({
                    "instance_id":params.shared.instance_id, "part":part, "delta":owners, "total":current.host_note_drops,
                    "reason":"Bounded host-note ownership is full or a concrete tuple was duplicated before NOTE_END; the new note was dropped without stealing another identity.",
                }));
            }
            if underruns == 0 && dropped == 0 { continue }
            crate::diagnostics::event(crate::diagnostics::LogLevel::Warning, "engine", "playback_drops", serde_json::json!({
                "instance_id":params.shared.instance_id, "part":part, "generation":current.generation,
                "script_epoch":current.script_epoch, "underruns_delta":underruns, "dropped_commands_delta":dropped,
                "state":current, "sample_rate":audio.sample_rate, "block_size":audio.block_size,
                "reason":"Streamed frames arrived late or bounded script command queues overflowed; see the separate delta counters",
            }));
        }
        if let Some(previous) = previous { let _ = params.shared.diagnostic_free.force_push(previous); }
    }
}

/// The simplified CC74 companion cannot supply a host expression's value law
/// or note ID. Keep it unsupported, rather than applying it as absolute MIDI2
/// CC74 to a newer same-pitch note. Direct registered MIDI2 CC74 stays admitted.
#[cfg(test)]
fn unsupported_host_brightness(events: &EventList, index: usize) -> bool {
    if !events.get(index).is_some_and(|event| matches!(event.body,
        EventBody::PerNoteCC { cc:74, registered:true, .. })) { return false }
    match events.exact_for_event(index).map(|event| event.body()) {
        Some(moose::core::ExactEventBody::NoteExpression { expression_id:5, .. }
            | moose::core::ExactEventBody::NormalizedNoteExpression { expression_id:5, .. }) => true,
        // A full exact lane makes the adapter emit an unlinked semantic
        // fallback. Capacity exhaustion must not erase the safety boundary.
        None => events.overflow().is_some(),
        _ => false,
    }
}

/// Worker-prepared snapshots, at most once per second of audio; no logging here.
fn capture_audio_diagnostics(s: &mut Dsp, p: &SamplerParams, frames: usize, channels: usize, offline: bool, cx: &ProcessContext) {
    if s.until_diagnostics > frames { s.until_diagnostics -= frames; return }
    s.until_diagnostics = s.rack.parts[0].rate().max(1.0) as usize;
    if p.shared.discard.is_full() { return }
    // The callback is the only producer. Make room before taking a free
    // buffer, because the worker may have recycled buffers while this queue
    // still contains two unread snapshots.
    let spare = if p.shared.diagnostic_audio.is_full() {
        let old = p.shared.diagnostic_audio.pop();
        if old.is_some() { p.shared.diagnostic_dropped.fetch_add(1, Ordering::Relaxed); }
        old.or_else(|| p.shared.diagnostic_free.pop())
    } else {
        p.shared.diagnostic_free.pop().or_else(|| {
            let old = p.shared.diagnostic_audio.pop();
            if old.is_some() { p.shared.diagnostic_dropped.fetch_add(1, Ordering::Relaxed); }
            old
        })
    };
    let Some(mut spare) = spare else { return };
    if spare.parts.len() != s.rack.parts.len() {
        if p.shared.discard.push(Retired { diagnostic: Some(spare), ..Default::default() }).is_err() {
            unreachable!("diagnostic spare must have retirement space");
        }
        return;
    }
    let audio = &mut s.diagnostic;
    audio.block = p.shared.blocks.load(Ordering::Relaxed); audio.sample_rate = s.rack.parts[0].rate();
    audio.block_size = frames; audio.output_channels = channels; audio.offline = offline;
    audio.unsupported_note_brightness = s.unsupported_note_brightness;
    audio.unsupported_host_expression = s.unsupported_host_expression;
    audio.alignment_overflows=s.align.overflows();
    audio.host_note_end_rejections=s.host_note_end_rejections;
    audio.output_buses = std::array::from_fn(|bus| cx.bus_routing.output(bus).map_or(if bus == 0 { (0, channels.min(2)) } else { (0, 0) }, |r| (r.channel_start(), r.channel_count())));
    for (part, current) in audio.parts.iter_mut().enumerate() {
        let e = &s.rack.parts[part]; let [pending_commands, pending_writes, pending_releases] = e.pending_work();
        *current = PartDiagnostics { generation:s.installed_generation[part], script_epoch:s.script_epoch[part],
            voices:e.active_voices(), audible:e.audible_voices(), underruns:e.underruns().saturating_add({
                #[cfg(feature = "uvi")]
                { s.uvi.get(part).and_then(Option::as_ref).map_or(0, |a| a.slot().underruns()) }
                #[cfg(not(feature = "uvi"))]
                { 0 }
            }), dropped_commands:e.dropped_commands(), host_note_drops:e.host_note_drops(),
            held_keys:std::array::from_fn(|channel| std::array::from_fn(|half| (0..64).fold(0, |keys, note|
                keys | (u64::from(e.key_down(channel as u8, (half * 64 + note) as u8)) << note)))),
            sustain_cc:std::array::from_fn(|channel| e.cc_state()[channel][64]),
            sostenuto_cc:std::array::from_fn(|channel| e.cc_state()[channel][66]),
            pending_commands, pending_writes, pending_releases };
    }
    std::mem::swap(&mut s.diagnostic, &mut spare);
    p.shared.diagnostic_audio.push(spare).ok().unwrap();
    if let Some(tasks) = cx.tasks::<AudioDiagnosticsTask>() {
        tasks.spawn_coalescing(AudioDiagnosticsTask);
    }
}

pub(crate) struct SnapshotRequest {
    slot: usize,
    source: (String, u32, String),
    path: String,
    generation: u64,
}

/// Validate on the loader before replacing any saved or playing state.
fn prepare_snapshot(params: &SamplerParams) -> Option<(usize, (String, u32, String), Arc<Instrument>)> {
    let request = {
        let mut pending = params.shared.snapshot_request.lock().unwrap();
        if pending.as_ref()?.slot >= params.shared.grown.load(Ordering::Acquire) as usize { return None }
        pending.take()?
    };
    let mut trace = crate::diagnostics::LoadTrace::new(Path::new(&request.source.0), request.source.1, Some(request.slot));
    trace.detail("instance_id", params.shared.instance_id);
    trace.detail("operation", "snapshot_validation");
    trace.detail("snapshot", request.path.clone());
    trace.detail("scope", "container parsing, explicit base matching and supported state import; audio not installed");
    trace.stage("snapshot_parse_and_validation");
    let current = || {
        params.shared.part(request.slot).unwrap().generation.load(Ordering::Acquire) == request.generation
            && params.selection.read().unwrap().parts.get(request.slot)
                .is_some_and(|p| p.matches_source(&request.source))
            && params.shared.snapshot_request.lock().unwrap().is_none()
    };
    if !current() {
        trace.detail("cancellation", "Source changed or a newer snapshot was requested");
        trace.finish("canceled");
        return None;
    }
    params.shared.view.lock().unwrap().parts[request.slot].status = "Loading snapshot…".into();
    let result = import::read_snapshot(Path::new(&request.source.0), Path::new(&request.path));
    if !current() {
        trace.detail("cancellation", "Source changed or a newer snapshot was requested during validation");
        trace.finish("canceled");
        return None;
    }
    match result {
        Ok(instrument) => {
            let mut selection = params.selection.write().unwrap();
            let pending = params.shared.snapshot_request.lock().unwrap();
            let valid = pending.is_none()
                && params.shared.part(request.slot).unwrap().generation.load(Ordering::Acquire) == request.generation;
            let Some(part) = selection.parts.get_mut(request.slot).filter(|p| valid && p.matches_source(&request.source)) else {
                drop(pending);
                drop(selection);
                trace.detail("cancellation", "Source changed or a newer snapshot was requested before commit");
                trace.finish("canceled");
                return None;
            };
            part.select_snapshot(request.path);
            let source = part.source();
            drop(pending);
            drop(selection);
            trace.finish("validated");
            Some((request.slot, source, Arc::new(instrument)))
        }
        Err(error) => {
            let selection = params.selection.read().unwrap();
            let pending = params.shared.snapshot_request.lock().unwrap();
            let valid = params.shared.part(request.slot).unwrap().generation.load(Ordering::Acquire) == request.generation
                && selection.parts.get(request.slot).is_some_and(|p| p.matches_source(&request.source))
                && pending.is_none();
            if !valid {
                drop(pending);
                drop(selection);
                trace.detail("cancellation", "Source changed or a newer snapshot was requested before reporting failure");
                trace.finish("canceled");
                return None;
            }
            let message = format!("{error:#}");
            // Linearize rejection against source changes and newer requests.
            // LoadTrace only enqueues its journal record; it does no file IO.
            trace.fail(&message);
            let report = trace.finish("failed");
            let mut view = params.shared.view.lock().unwrap();
            view.parts[request.slot].status = format!("Snapshot was not loaded: {message}");
            view.parts[request.slot].load_report = Some(report);
            None
        }
    }
}

/// Periodic audio snapshots must not wait behind this instance's serialized
/// instrument loader. Scheduling is bounded and coalesced; shared pool pressure
/// can still overwrite snapshots, which diagnostic_dropped continues to count.
pub struct AudioDiagnosticsTask;
impl BackgroundTask for AudioDiagnosticsTask {
    type Params = SamplerParams;
    const SERIALIZED: bool = true;
    fn run(self, params: &SamplerParams) { drain_audio_diagnostics(params); }
}

fn catalog_uvi_receipt(view: &mut View, requested: Option<&library::UviRequest>) {
    if cfg!(feature = "uvi") && view.uvi_attempted.as_ref().is_some_and(|attempted| Some(attempted) == requested) {
        return;
    }
    view.uvi_attempted = None;
    view.uvi_status.clear();
}

pub struct Load;

#[cfg(feature = "uvi")]
#[derive(Clone, PartialEq, Eq)]
struct UviLoadKey {
    request: library::UviRequest,
    epoch: u64,
    rate: u64,
    max_host_frames: usize,
    catalog: u64,
    /// Exact active Kontakt identity/generation, without changing that engine.
    target: Option<((String, u32, String), u64)>,
    target_uvi: Option<library::UviSource>,
}

#[cfg(feature = "uvi")]
struct UviPrepared {
    key: UviLoadKey,
    generation: u64,
    worker: Option<crate::uvi::worker::Worker>,
    status: &'static str,
    ui: Option<uvi_ui::Mailbox>,
}

#[cfg(feature = "uvi")]
fn uvi_load_key(params: &SamplerParams, selection: &Selection) -> Option<UviLoadKey> {
    let request = selection.uvi_requested.clone()?;
    let target = request.slot.and_then(|slot| {
        let slot = slot as usize;
        Some((selection.parts.get(slot)?.source(), params.shared.part(slot)?.generation.load(Ordering::Acquire)))
    });
    let target_uvi = request.slot.and_then(|slot| selection.parts.get(slot as usize)).and_then(|p| p.uvi.clone());
    Some(UviLoadKey { request, epoch: params.shared.uvi_epoch.load(Ordering::Acquire),
        rate: params.shared.rate.load(Ordering::Acquire),
        max_host_frames: params.shared.uvi_max_host_frames.load(Ordering::Acquire),
        catalog: params.shared.libraries.wanted(),
        target_uvi, target })
}

#[cfg(feature = "uvi")]
fn prepare_uvi(params: &SamplerParams, selection: &Selection) {
    use crate::uvi::worker::{Status, Worker};
    const STARTING: &str = "Loading the UVI instrument; the current instrument is still playing.";
    const READY: &str = "Preparing the UVI player for the rack…";
    const FAILED: &str = "The UVI instrument could not be loaded; the current instrument is still playing.";
    let key = uvi_load_key(params, selection);
    let retired = {
        let mut prepared = params.shared.uvi_prepared.lock().unwrap();
        if prepared.as_ref().map(|p| &p.key) != key.as_ref() { prepared.take() } else { None }
    };
    // Stop/join and all graph/Lua/resource destruction run only on Load.
    drop(retired);
    let Some(key) = key else {
        let mut view = params.shared.view.lock().unwrap();
        if params.selection.read().unwrap().uvi_requested.is_none() {
            view.uvi_attempted = None; view.uvi_status.clear(); view.uvi_ui = None;
        }
        return;
    };
    let missing = params.shared.uvi_prepared.lock().unwrap().is_none();
    if missing {
        let rate = f64::from_bits(key.rate);
        let configured = if key.request.new && key.request.slot.is_some()
            || key.request.slot.is_some() && key.target.is_none() {
            Err("The UVI destination changed. Select its program again.")
        } else if !(8000. ..=192000.).contains(&rate) || rate.fract() != 0. {
            Err("The current sample rate is unsupported by this UVI instrument.")
        } else {
            params.shared.libraries.uvi_worker_config(&key.request.source, rate as u32)
        };
        if uvi_load_key(params, &params.selection.read().unwrap()) != Some(key.clone()) { return; }
        let generation = params.shared.uvi_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let mut trace = crate::diagnostics::LoadTrace::new(&key.request.source.bank, 0, key.request.slot.map(|slot| slot as usize));
        trace.detail("backend", "uvi");
        trace.detail("member", key.request.source.member.clone());
        trace.detail("epoch", key.epoch);
        trace.detail("generation", generation);
        trace.stage("uvi_controller_setup");
        let (worker, status, ui) = match configured {
            Ok(config) => {
                let assets = match crate::uvi::ui_assets::UiAssets::open(&config) {
                    Ok(assets) => Some(assets),
                    Err(error) => { trace.issue("ui", "uvi_artwork_authority_unavailable", format!("{error:#}")); None }
                };
                match Worker::start_hosted(config, key.epoch, generation) {
                    Ok(worker) => (Some(worker), STARTING, Some(uvi_ui::Mailbox::new(
                        crate::uvi::worker::Stamp { epoch: key.epoch, generation, frame: 0 }, assets))),
                    Err(error) => { trace.fail(format!("Starting UVI worker: {error:#}")); (None, FAILED, None) },
                }
            },
            Err(reason) => { trace.fail(reason); (None, reason, None) },
        };
        trace.finish(if worker.is_some() { "worker_started" } else { "failed" });
        let prepared = UviPrepared { key: key.clone(), generation, worker, status, ui };
        if uvi_load_key(params, &params.selection.read().unwrap()) != Some(key.clone()) { drop(prepared); return; }
        *params.shared.uvi_prepared.lock().unwrap() = Some(prepared);
    }
    let (status, failed, ui) = {
        let mut prepared = params.shared.uvi_prepared.lock().unwrap();
        let p = prepared.as_mut().unwrap();
        let failed = match p.worker.as_ref().map(Worker::status) {
            Some(Status::Ready) => { p.status = READY; None },
            Some(Status::Failed | Status::Stopped) => { p.status = FAILED; p.worker.take() },
            _ => None,
        };
        let ui = match (&p.worker, &mut p.ui) {
            (Some(worker), Some(ui)) => ui.poll(worker),
            _ => None,
        };
        let display = if p.status == STARTING {
            p.worker.as_ref().and_then(Worker::initialization_progress)
                .map(|progress| format!("{}; the current instrument is still playing.", uvi_load::loading_status(Some(progress))))
                .unwrap_or_else(|| p.status.to_owned())
        } else { p.status.to_owned() };
        (display, failed, ui)
    };
    if let Some(worker) = &failed {
        crate::diagnostics::event(crate::diagnostics::LogLevel::Error, "uvi", "uvi_staging_failed",
            serde_json::json!({"path":key.request.source.bank, "member":key.request.source.member,
                "epoch":key.epoch, "reason":worker.private_failure()}));
    }
    drop(failed);
    if uvi_load_key(params, &params.selection.read().unwrap()) != Some(key.clone()) { return; }
    let mut view = params.shared.view.lock().unwrap();
    if params.selection.read().unwrap().uvi_requested.as_ref() == Some(&key.request)
        && params.shared.uvi_epoch.load(Ordering::Acquire) == key.epoch {
        if view.uvi_attempted.as_ref() != Some(&key.request) { view.uvi_ui = None; }
        view.uvi_attempted = Some(key.request);
        if let Some(ui) = ui { view.uvi_ui = Some(ui); }
        if view.uvi_status != status { view.uvi_status = status; }
    }
}

#[cfg(feature = "uvi")]
struct UviDelayHandoff { epoch: u64, ticket: u64, storage: Box<uvi_delay::Prepared> }

#[cfg(feature = "uvi")]
struct UviDelayPreparation {
    context: (u64, usize, usize),
    outcome: Result<(u32, u64), uvi_delay::Error>,
}

/// Different callback queues are unordered. A native endpoint is published only
/// after the callback acknowledged storage for this epoch and complete rack.
#[cfg(feature = "uvi")]
fn uvi_delay_ready(params: &SamplerParams) -> Result<bool, uvi_delay::Error> {
    let epoch = params.shared.uvi_epoch.load(Ordering::Acquire);
    let count = params.shared.with_parts(|parts| parts.len());
    if count > params.shared.grown.load(Ordering::Acquire) as usize { return Ok(false) }
    let maximum = params.shared.uvi_max_host_frames.load(Ordering::Acquire);
    let context = (epoch, count, maximum);
    let mut prepared = params.shared.uvi_delay_prepared.lock().unwrap();
    if let Some(previous) = prepared.as_ref()
        && previous.context == context {
        let (latency, ticket) = previous.outcome?;
        params.shared.uvi_latency_admission.store(((maximum as u64) << 32) | u64::from(latency), Ordering::Release);
        return Ok(params.shared.uvi_delay_installed.load(Ordering::Acquire) == ticket);
    }
    let allocated = crate::uvi::bridge::Bridge::buffering_latency(maximum, UVI_LEAD_PACKETS)
        .map_err(|_| uvi_delay::Error::InvalidLayout)
        .and_then(|latency| {
            let mut storage = uvi_delay::Prepared::new(count, latency as usize)?;
            if !storage.set_all(latency) { return Err(uvi_delay::Error::InvalidLayout) }
            Ok((latency, storage))
        });
    let (latency, storage) = match allocated {
        Ok(allocated) => allocated,
        Err(error) => {
            *prepared = Some(UviDelayPreparation { context, outcome: Err(error) });
            return Err(error);
        }
    };
    params.shared.uvi_latency_admission.store(((maximum as u64) << 32) | u64::from(latency), Ordering::Release);
    let ticket = params.shared.uvi_delay_wanted.fetch_add(1, Ordering::AcqRel) + 1;
    let next = UviDelayHandoff { epoch, ticket, storage: Box::new(storage) };
    let _ = params.shared.uvi_delays.force_push(next);
    *prepared = Some(UviDelayPreparation { context, outcome: Ok((latency, ticket)) });
    Ok(false)
}

/// Scalar-only host report. Actual mixer delays still follow live endpoints.
#[cfg(feature = "uvi")]
fn admitted_uvi_latency(shared: &Shared, maximum: usize) -> u32 {
    let admitted = shared.uvi_latency_admission.load(Ordering::Acquire);
    if admitted >> 32 == maximum as u64 { admitted as u32 } else { 0 }
}

/// Preserve physical compensation during replacement only when the already
/// adopted storage can actually supply the retained host-reported delay.
#[cfg(feature = "uvi")]
fn physical_uvi_latency(s: &Dsp, live: u32) -> u32 {
    if s.uvi_delays.as_ref().is_some_and(|delays|
        delays.slots() == s.uvi.len() && delays.latency_frames() == s.uvi_reported_latency) {
        live.max(s.uvi_reported_latency)
    } else { live }
}

/// Commit the requested identity only after native initialization succeeded.
/// The controller remains on Load; its endpoint is published separately after
/// common mixer delay storage has been acknowledged by the audio thread.
#[cfg(feature = "uvi")]
fn install_prepared_uvi(params: &SamplerParams) {
    use crate::uvi::worker::Status;
    let ready = {
        let prepared = params.shared.uvi_prepared.lock().unwrap();
        prepared.as_ref().filter(|p| p.worker.as_ref().is_some_and(|w| w.status() == Status::Ready))
            .map(|p| p.key.clone())
    };
    let Some(key) = ready else { return };
    let before = params.selection.read().unwrap().clone();
    if uvi_load_key(params, &before) != Some(key.clone()) { return }
    if !key.request.new && key.request.slot.is_none()
        && let Some(slot) = before.parts.iter().enumerate().find_map(|(slot, p)| {
            (p.uvi.as_ref() == Some(&key.request.source)
                && params.shared.part(slot).is_some_and(|a| !a.uvi_failed.load(Ordering::Acquire)
                    && a.uvi_generation.load(Ordering::Acquire) != 0)).then_some(slot)
        }) {
        let mut current = params.selection.write().unwrap();
        if uvi_load_key(params, &current) != Some(key) { return }
        current.uvi_requested = None;
        params.shared.focus_request.store(slot as u64, Ordering::Release);
        drop(current);
        params.shared.uvi_prepared.lock().unwrap().take();
        return;
    }
    let retry = (!key.request.new).then(|| before.parts.iter().position(|p|
        p.uvi.as_ref() == Some(&key.request.source))).flatten();
    let slot = key.request.slot.map(|s| s as usize).or(retry).unwrap_or_else(||
        before.parts.iter().position(Part::is_empty).unwrap_or(before.parts.len()));
    params.shared.ensure_parts(slot + 1);
    let atoms = params.shared.part(slot).unwrap();
    let mut current = params.selection.write().unwrap();
    if uvi_load_key(params, &current) != Some(key.clone()) { return }
    let mut prepared = params.shared.uvi_prepared.lock().unwrap().take().unwrap();
    let generation = prepared.generation;
    let part_generation = atoms.generation.load(Ordering::Acquire).wrapping_add(1);
    let rate = f64::from_bits(key.rate) as u32;
    let result = params.shared.uvi_controls.lock().unwrap().adopt_prepared(
        prepared.worker.take().unwrap(), prepared.ui.take().unwrap(), key.request.source.clone(),
        key.epoch, generation, part_generation, slot, rate, key.max_host_frames, UVI_LEAD_PACKETS);
    if result.is_err() {
        drop(current);
        params.shared.view.lock().unwrap().uvi_status = "The UVI player could not be prepared.".into();
        return;
    }
    let mut part = if key.request.slot.is_some() || retry.is_some() { current.parts[slot].clone() }
        else {
            let settings = params.shared.libraries.settings();
            let (port, channel) = settings.new_input.unwrap_or_else(|| current.next_input());
            Part { port, channel, output: settings.new_output.unwrap_or(0),
                output_manual: settings.new_output.is_some(), ..Default::default() }
        };
    part.path.clear(); part.snapshot.clear(); part.program = 0;
    part.uvi = Some(key.request.source.clone());
    part.uvi_state.clear();
    part.name.clear();
    part.group = u32::MAX; part.articulate = Default::default(); part.mpe = Default::default();
    part.tune = 0.;
    part.edits = Default::default(); part.script_state.clear(); part.ir_settings.clear();
    part.engine_state.clear(); part.delay_state.clear();
    if slot == current.parts.len() { current.parts.push(part); } else { current.parts[slot] = part; }
    if !current.order.contains(&(slot as u32)) { current.order.push(slot as u32); }
    current.uvi_requested = None;
    atoms.generation.store(part_generation, Ordering::Release);
    params.shared.focus_request.store(slot as u64, Ordering::Release);
    drop(current);
    let mut view = params.shared.view.lock().unwrap();
    view.parts[slot] = PartView {
        uvi_activation: Some(uvi_load::Activation { source: key.request.source, saved_state: NativeState::default(), epoch: key.epoch,
            generation, part_generation, rate, max_host_frames: key.max_host_frames, published: false }),
        status: "Preparing UVI playback…".into(), loading: true, ..Default::default()
    };
    view.uvi_attempted = None; view.uvi_status.clear(); view.uvi_ui = None;
}

impl BackgroundTask for Load {
    type Params = SamplerParams;
    const SERIALIZED: bool = true;
    fn run(self, params: &SamplerParams) {
        drain_audio_diagnostics(params);
        params.shared.flush_ready();
        load_arrays(params);
        load_zone_maps(params);
        let mut freed = false;
        while let Some(mut retired) = params.shared.discard.pop() {
            if let Some((part,generation,epoch,mut bank)) = retired.zone_preload.take() {
                let current = || {
                    let view=params.shared.view.lock().unwrap();
                    view.parts.get(part).is_some_and(|v|v.script_epoch==epoch
                        && params.shared.part(part).is_some_and(|p|p.generation.load(Ordering::Acquire)==generation)
                        && v.instrument.as_ref().is_some_and(|i|crate::cache::current(&i.dependencies)))
                };
                if current() {
                    let chains=params.shared.zone_chains.lock().unwrap();
                    let latest=chains.iter().rev().find(|c|(c.part,c.generation,c.epoch)==(part,generation,epoch)
                        && c.context.upgrade().is_some_and(|context|bank.zone_preload_matches(&context)));
                    let result=latest.map_or(Ok(()),|chain| {
                        let context=chain.context.upgrade().unwrap();
                        bank.rebase_zone_preload(&context,chain.state.clone(),&||!current())
                    });
                    if result.is_ok() && current() {
                        params.shared.publish_part((part,generation,Handoff::Bank(bank)));
                    } else if let Err(error)=result {
                        let source=params.shared.view.lock().unwrap().parts[part].instrument.clone();
                        if let Some(source)=source { crate::diagnostics::resource(&source.path,"zone preload",&format!("{error:#}")); }
                    }
                }
            }
            freed = true;
        }
        if freed {
            crate::audio::trim_heap();
        }
        let requested = { params.shared.multi_request.lock().unwrap().take() };
        if let Some(path) = requested {
            let before = params.selection.read().unwrap().clone();
            // A saved rack restores every setting; an NKM only its programs.
            let result = if import::is_saved_multi(Path::new(&path)) {
                SavedMulti::read(Path::new(&path)).map(|m| {
                    let status = format!("{} · {} instruments", m.name, m.parts.len());
                    (m.parts, status)
                })
            } else {
                import::read_multi(Path::new(&path)).map(|m| {
                    let parts = m
                        .parts
                        .iter()
                        .map(|p| Part {
                            path: path.clone(),
                            program: p.program,
                            ..Default::default()
                        })
                        .collect::<Vec<_>>();
                    let status = format!(
                        "{} · {} instruments · original multi routing/scripts unavailable",
                        m.name,
                        parts.len()
                    );
                    (parts, status)
                })
            }
            ;
            if params.shared.multi_request.lock().unwrap().is_some() {
                return;
            }
            match result {
                Ok((parts, status)) => {
                    let status = {
                        let mut current = params.selection.write().unwrap();
                        if *current == before {
                            current.order = (0..parts.len() as u32).collect();
                            current.parts = parts;
                            current.multi = path;
                            params.shared.focus_request.store(0, Ordering::Release);
                            status
                        } else {
                            "Multi load canceled because the rack changed".into()
                        }
                    };
                    params.shared.view.lock().unwrap().multi_status = status;
                }
                Err(e) => {
                    params.shared.view.lock().unwrap().multi_status =
                        format!("Multi load failed: {e:#}")
                }
            }
        }
        let prepared_snapshot = prepare_snapshot(params);
        let mut selection = params.selection.read().unwrap().clone();
        params.shared.ensure_parts(selection.parts.len());
        #[cfg(feature = "uvi")]
        {
            prepare_uvi(params, &selection);
            install_prepared_uvi(params);
            selection = params.selection.read().unwrap().clone();
            params.shared.ensure_parts(selection.parts.len());
            params.shared.prepare_growth();
            uvi_load::service(params);
        }
        #[cfg(not(feature = "uvi"))]
        {
        let uvi_changed = params.shared.view.lock().unwrap().uvi_attempted != selection.uvi_requested;
        if uvi_changed {
            let status = selection.uvi_requested.as_ref().map(|request| {
                match params.shared.libraries.inspect_uvi(&request.source) {
                    Ok(()) => "UVI program passed graph preflight; live UVI playback is not available yet.".to_owned(),
                    Err(reason) => reason.to_owned(),
                }
            }).unwrap_or_default();
            let mut view = params.shared.view.lock().unwrap();
            if params.selection.read().unwrap().uvi_requested == selection.uvi_requested {
                view.uvi_attempted = selection.uvi_requested.clone();
                view.uvi_status = status;
            }
        }
        }
        params.shared.prepare_growth();
        params
            .shared
            .midi_thru
            .store(selection.midi_thru, Ordering::Release);
        // A finished library scan replaces what the browser lists.
        let installed = params.shared.view.lock().unwrap().scanned;
        if let Some((generation, scanned)) = params.shared.libraries.poll(installed) {
            let mut view = params.shared.view.lock().unwrap();
            view.scanned = generation;
            match scanned {
                Some(scanned) => {
                    catalog_uvi_receipt(&mut view, params.selection.read().unwrap().uvi_requested.as_ref());
                    let presets = scanned.files.len() + scanned.shelf.uvi.values().map(|bank| bank.presets.len()).sum::<usize>();
                    view.status = format!("{} libraries · {presets} presets", scanned.shelf.libraries.len());
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
        route(params);
        {
            let current = params.selection.read().unwrap();
            params.shared.sync_overrides(&current);
            let _ = params.shared.controls.force_push(mix(&current));

            let _ = params.shared.routes.force_push(routes(&current));
            params
                .shared
                .midi_thru
                .store(current.midi_thru, Ordering::Release);
        }
        for slot in 0..params.shared.grown.load(Ordering::Acquire) as usize {
            let atoms = params.shared.part(slot).unwrap();
            let part = selection.parts.get(slot).cloned().unwrap_or_default();
            #[cfg(feature = "uvi")]
            if part.uvi.is_some() { continue; }
            let target = part.source();
            let streaming = part.streaming(selection.streaming);
            let selected_snapshot = prepared_snapshot.as_ref().is_some_and(|(at, source, _)| *at == slot && *source == target);
            // The host restored different script values for a loaded part: rebuild only its scripts.
            let restore = {
                let view = params.shared.view.lock().unwrap();
                let v = &view.parts[slot];
                v.instrument.clone().filter(|i| {
                    !selected_snapshot && v.attempted.as_ref() == Some(&target)
                        && v.fx_rate != 0.
                        && !i.scripts.is_empty()
                        && (part.script_state != v.script_state || part.ir_settings != v.ir_settings || part.engine_state.as_slice() != v.engine_state.as_ref() || part.delay_state.as_slice() != v.delay_state.as_ref())
                }).map(|i| (i, atoms.generation.load(Ordering::Acquire), v.script_epoch))
            };
            if let Some((instrument, generation, previous_epoch)) = restore {
                let mut trace = crate::diagnostics::LoadTrace::new(&instrument.path, part.program, Some(slot));
                trace.detail("instance_id", params.shared.instance_id);
                trace.detail("operation", "script_restore");
                trace.stage("scripts");
                let (script, snapshot, errors) =
                    scripts_with_delays(&instrument, &part.script_state, &part.ir_settings, &part.engine_state, &part.delay_state, params.shared.rate());
                for e in &errors { trace.script_issue("initialization_failed", e, &instrument.scripts); }
                if let Some(rt) = script.as_deref() {
                    trace.script_runtime(rt, &instrument.scripts);
                }
                let zone_bank=if let Some(rt)=script.as_deref().filter(|rt|rt.init_zone_edits.iter().any(|e|e.id<0)) {
                    trace.stage("zone_init");
                    let own=params.shared.view.lock().unwrap().parts[slot].bytes;
                    let resident=crate::engine::resident_bytes().saturating_sub(own);
                    let budget=crate::engine::MEMORY_LIMIT.min(crate::engine::memory_budget().saturating_sub(resident));
                    let canceled=||atoms.generation.load(Ordering::Acquire)!=generation;
                    trace.detail("memory_budget_bytes",budget);
                    match Bank::load_cancelable(&instrument,budget,streaming,&rt.init_controllers,&AtomicU32::new(0),&canceled)
                        .and_then(|mut bank|{bank.prepare_zone_init(rt.service_epoch,&rt.init_zone_edits,&canceled)?;Ok(Box::new(bank))}) {
                        Ok(bank)=>Some(bank),
                        Err(error)=>{
                            trace.fail(format!("{error:#}")); let report=trace.finish("failed");
                            let mut view=params.shared.view.lock().unwrap();
                            let current=params.selection.read().unwrap();
                            let v=&mut view.parts[slot];
                            let fresh=current.parts.get(slot).is_some_and(|p|p.matches_source(&target)
                                && p.script_state==part.script_state && p.ir_settings==part.ir_settings
                                && p.engine_state==part.engine_state && p.delay_state==part.delay_state)
                                && atoms.generation.load(Ordering::Acquire)==generation
                                && v.script_epoch==previous_epoch && v.attempted.as_ref()==Some(&target)
                                && v.instrument.as_ref().is_some_and(|i|Arc::ptr_eq(i,&instrument));
                            if fresh {v.interface_status=format!("Zone init restore failed: {error:#}");v.load_report=Some(report);}
                            continue;
                        }
                    }
                } else {None};
                let live = script.as_deref().map(first_script_live);
                let pages = script_pages(script.as_deref());
                let irs = script.as_deref().map_or(Vec::new(), |rt| rt.init_irs.clone());
                let fx_rate = {
                    let view = params.shared.view.lock().unwrap();
                    (irs != view.parts[slot].irs).then_some(view.parts[slot].fx_rate)
                };
                let fx = fx_rate.map(|rate| {
                    trace.stage("effects");
                    trace_effects(&mut trace, &instrument);
                    crate::engine::effects(&instrument, script.as_deref(), rate as f32)
                });
                let interface_status = errors.join("\n");
                #[cfg(test)]
                if let Some(gate) = {
                    let mut gate = params.shared.restore_gate.lock().unwrap();
                    gate.as_ref().is_some_and(|(at, _)| *at == slot)
                        .then(|| gate.take().unwrap().1)
                } { gate.wait(); gate.wait(); }
                let mut view = params.shared.view.lock().unwrap();
                // Rebuilding scripts/FX can outlive a newer host restore or
                // source selection. Check its exact saved state, not rack gain
                // or editor settings, before giving old work a fresh epoch.
                let current = params.selection.read().unwrap();
                let v = &view.parts[slot];
                let fresh = current.parts.get(slot).is_some_and(|p| p.matches_source(&target)
                    && p.script_state == part.script_state && p.ir_settings == part.ir_settings
                    && p.engine_state == part.engine_state && p.delay_state == part.delay_state)
                    && atoms.generation.load(Ordering::Acquire) == generation
                    && v.script_epoch == previous_epoch && v.attempted.as_ref() == Some(&target)
                    && v.instrument.as_ref().is_some_and(|i| Arc::ptr_eq(i, &instrument));
                if !fresh {
                    drop(current); drop(view);
                    trace.finish("canceled");
                    continue;
                }
                let epoch = next_epoch(&mut view, slot, snapshot, live);
                view.parts[slot].script_pages = pages;
                if let Some(selected) = view.parts[slot].live.as_ref().map(|live| live.slot) { view.parts[slot].script_slot = selected; }
                view.parts[slot].script_state = part.script_state.clone();
                view.parts[slot].ir_settings = part.ir_settings.clone();
                view.parts[slot].engine_state = part.engine_state.clone().into();
                view.parts[slot].delay_state = part.delay_state.clone().into();
                view.parts[slot].interface_status = interface_status;
                view.parts[slot].runtime_status.clear();
                if let Some(bank)=zone_bank.as_deref() {view.parts[slot].bytes=bank.bytes;}
                if let Some(fx) = fx {
                    view.parts[slot].irs = irs;
                    let _ = params.shared.publish_part((
                        slot,
                        generation,
                        Handoff::Fx(fx),
                    ));
                }
                let _ = params.shared.publish_part((
                    slot,
                    generation,
                    Handoff::Script { script, bank:zone_bank, epoch },
                ));
                drop(current); drop(view);
                let report = trace.finish("loaded");
                let mut view = params.shared.view.lock().unwrap();
                if view.parts[slot].script_epoch == epoch {
                    if let Some(load) = &mut view.parts[slot].load_report {
                        let load = Arc::make_mut(load);
                        load["runtime"] = serde_json::Value::Null;
                        if report["status"] == "partial" && load["status"] == "loaded" { load["status"] = "partial".into(); }
                        load["script_restore"] = (*report).clone();
                    } else { view.parts[slot].load_report = Some(report); }
                }
                continue;
            }
            let loaded = {
                let view = params.shared.view.lock().unwrap();
                let v = &view.parts[slot];
                !selected_snapshot && v.attempted.as_ref() == Some(&target) && v.streaming == streaming
            };
            if loaded {
                continue;
            }
            {
                let mut view = params.shared.view.lock().unwrap();
                let v = &mut view.parts[slot];
                (v.attempted, v.streaming) = (Some(target.clone()), streaming);
                v.status = "Loading import…".into();
                v.load_report = None;
                v.runtime_status.clear();
                atoms.load_progress.store(0, Ordering::Relaxed);
                v.loading = true;
                v.script_epoch = 0;
                v.script_pages = Arc::default();
                v.edited.clear();
                v.snapshot = None;
                v.live = None;
                v.live_revisions = None;
                v.live_control_versions = Arc::default();
                v.live_diagnostics = None;
                v.diagnostics_dirty = false;
            };
            let generation = atoms.generation.fetch_add(1, Ordering::AcqRel) + 1;
            let canceled = || {
                let current = params.selection.read().unwrap();
                current.parts.get(slot).is_none_or(|p| {
                    !p.matches_source(&target)
                        || p.streaming != part.streaming || p.streaming(current.streaming) != streaming
                })
                    || atoms.generation.load(Ordering::Acquire) != generation
            };
            if part.path.is_empty() {
                params.shared.view.lock().unwrap().parts[slot] = PartView {
                    attempted: Some(target),
                    streaming,
                    ..Default::default()
                };
                let _ = params.shared.publish_part((
                    slot,
                    generation,
                    Handoff::Part {
                        bank: None,
                        fx: FxProcessor::default(),
                        script: None,
                        epoch: 0,
                    },
                ));
                continue;
            }
            let mut trace = crate::diagnostics::LoadTrace::new(Path::new(&part.path), part.program, Some(slot));
            trace.detail("instance_id", params.shared.instance_id);
            if !part.snapshot.is_empty() { trace.detail("snapshot", part.snapshot.as_str()); }
            trace.detail("sample_rate", params.shared.rate());
            trace.detail("streaming_requested", format!("{streaming:?}"));
            let set_stage = |trace: &mut crate::diagnostics::LoadTrace, name: &'static str| {
                trace.stage(name);
                params.shared.view.lock().unwrap().parts[slot].status = format!("Loading {name}…");
            };
            let result = (|| -> anyhow::Result<_> {
                set_stage(&mut trace, "import");
                #[cfg(test)]
                {
                    let gate = {
                        let mut gate = params.shared.load_gate.lock().unwrap();
                        if gate.as_ref().is_some_and(|(at, _)| *at == slot) { gate.take() } else { None }
                    };
                    if let Some((_, gate)) = gate {
                        gate.wait();
                        gate.wait();
                    }
                }
                anyhow::ensure!(!canceled(), "Instrument load canceled");
                let instrument = if let Some((_, _, instrument)) = prepared_snapshot.as_ref().filter(|(at, source, _)| *at == slot && *source == target) {
                    Arc::clone(instrument)
                } else if part.snapshot.is_empty() {
                    import::shared_program(Path::new(&part.path), part.program)?
                } else {
                    anyhow::ensure!(part.snapshot_base(), "Snapshot requires an explicit base NKI");
                    Arc::new(import::read_snapshot(Path::new(&part.path), Path::new(&part.snapshot))?)
                };
                trace.detail("groups", instrument.groups.len());
                trace.detail("zones_total", instrument.zones.len());
                trace.detail("script_slots", instrument.scripts.len());
                trace.detail("missing_samples", instrument.missing_samples.len());
                for w in &instrument.warnings { trace.issue("import", crate::diagnostics::code(w), w); }
                for name in &instrument.missing_samples { trace.issue("samples", "missing", name); }
                anyhow::ensure!(!canceled(), "Instrument load canceled");
                // Progress by phase: parsed 5%, scripts 10%, the bank the rest.
                let progress = &atoms.load_progress;
                progress.fetch_max(crate::engine::LOAD_DONE / 20, Ordering::Relaxed);
                set_stage(&mut trace, "scripts_and_sample_headers");
                let ((script, snapshot, script_errors), bare) = std::thread::scope(|scope| {
                    let (rate, instrument, state) = (params.shared.rate(), &instrument, &part.script_state);
                    let ir_settings = &part.ir_settings;
                    let engine_state = &part.engine_state;
                    let delay_state = &part.delay_state;
                    let scripts = scope.spawn(move || scripts_with_delays(instrument, state, ir_settings, engine_state, delay_state, rate));
                    let bare = (!instrument.zones.is_empty()).then(|| Bank::load_bare_cancelable(instrument, &canceled));
                    let scripts = scripts.join().unwrap_or_else(|e| std::panic::resume_unwind(e));
                    (scripts, bare)
                });
                for e in script_errors { trace.script_issue("initialization_failed", e, &instrument.scripts); }
                if let Some(rt) = script.as_deref() {
                    trace.script_runtime(rt, &instrument.scripts);
                }
                anyhow::ensure!(!canceled(), "Instrument load canceled");
                progress.fetch_max(crate::engine::LOAD_DONE / 10, Ordering::Relaxed);
                // Publish controls now; decode their artwork after audio is ready.
                let needs_art = {
                    let view = params.shared.view.lock().unwrap();
                    let v = &view.parts[slot];
                    !part.snapshot.is_empty() || v.program != part.program || v.instrument.as_ref().is_none_or(|i| i.path != instrument.path)
                };
                let parsed = needs_art.then(|| script_interface(script.as_deref()));
                let art = {
                    let mut view = params.shared.view.lock().unwrap();
                    let v = &mut view.parts[slot];
                    v.instrument = Some(instrument.clone());
                    v.program = part.program;
                    parsed.map(|parsed| {
                        v.interface = parsed.interface.clone();
                        v.script_slot = parsed.slot;
                        v.interface_status = parsed.status;
                        v.keys = parsed.keys;
                        v.pictures = Arc::default();
                        v.wallpaper = None;
                        v.wallpaper_status.clear();
                        parsed.interface
                    })
                };
                if instrument.zones.is_empty() {
                    trace.issue("samples", "unsupported", "Controller instrument has no sample bank; standalone controller playback is unavailable");
                    return Ok((instrument, None, script, snapshot, None, art));
                }
                // Every group plays; the stored group only selects what the mapping inspector shows.
                if part.group == u32::MAX {
                    let group = instrument.first_playable_group().unwrap_or(0);
                    let mut current = params.selection.write().unwrap();
                    if let Some(c) = current.parts.get_mut(slot).filter(|c| {
                        c.matches_source(&target) && c.group == u32::MAX
                    }) {
                        c.group = group as u32;
                    }
                }
                // Samples resident in the whole process (every part and
                // plugin instance, shared data once) but this part's own,
                // about to be replaced.
                let own = params.shared.view.lock().unwrap().parts[slot].bytes;
                let resident = crate::engine::resident_bytes().saturating_sub(own);
                // The part gets what the process budget has left; a tight
                // budget streams more instead of failing.
                let budget = crate::engine::MEMORY_LIMIT
                    .min(crate::engine::memory_budget().saturating_sub(resident));
                trace.detail("memory_budget_bytes", budget);
                let controllers = script.as_deref().map_or(&[][..], |rt| &rt.init_controllers);
                let mut bank = bare.transpose()?.map(Box::new);
                let zone_epoch=script.as_deref().map_or(0,|rt|rt.service_epoch);
                let zone_init=script.as_deref().map_or(Vec::new(),|rt|rt.init_zone_edits.clone());
                if let Some(bank)=bank.as_deref_mut() {bank.prepare_zone_init(zone_epoch,&zone_init,&canceled)?;}
                let preload = Some((budget, controllers.to_vec(),zone_epoch,zone_init));
                Ok((instrument, bank, script, snapshot, preload, art))
            })();
            if canceled() {
                let report = trace.finish("canceled");
                let mut view = params.shared.view.lock().unwrap();
                view.parts[slot].loading = false;
                view.parts[slot].load_report = Some(report);
                continue;
            }
            // Building UI snapshots and convolution/FX state can be large.
            // Keep both outside the editor's view lock, including controller patches.
            if result.is_ok() { set_stage(&mut trace, "effects"); }
            let result = result.map(|(instrument, bank, script, snapshot, preload, art)| {
                let live = script.as_deref().map(first_script_live);
                let pages = script_pages(script.as_deref());
                let rate = params.shared.rate();
                let irs = script.as_deref().map_or(Vec::new(), |rt| rt.init_irs.clone());
                trace_effects(&mut trace, &instrument);
                let fx = crate::engine::effects(&instrument, script.as_deref(), rate as f32);
                (instrument, bank, script, snapshot, preload, art, live, pages, fx, rate, irs)
            });
            if canceled() {
                let report = trace.finish("canceled");
                let mut view = params.shared.view.lock().unwrap();
                view.parts[slot].loading = false;
                view.parts[slot].load_report = Some(report);
                continue;
            }
            let status = match &result {
                Ok((_, bank, _, _, _, _, _, _, _, _, _)) => {
                    if let Some(b) = bank.as_deref() {
                        trace.detail("samples_loaded", b.sample_count());
                        trace.detail("samples_streamed", b.streamed_samples());
                        trace.detail("zones_playable", b.zones().len());
                        trace.detail("zones_skipped", b.skipped_zones);
                        trace.detail("zone_skip_counts", serde_json::json!(b.zone_skip_counts));
                        trace.detail("resident_bytes", b.bytes);
                        if let Some(w) = &b.warning { trace.issue("samples", "streaming_warning", w); }
                        for e in &b.issues { trace.issue("samples", "zone_skipped", e); }
                        if b.skipped_zones > b.issues.len() {
                            trace.issue("samples", "diagnostics_truncated", format!("{} zones skipped; the bank retains only the first {} failure examples", b.skipped_zones, b.issues.len()));
                        }
                    }
                    "loaded"
                }
                Err(e) => { trace.fail(format!("{e:#}")); "failed" }
            };
            let report = trace.finish(status);
            let mut view = params.shared.view.lock().unwrap();
            view.parts[slot].loading = false;
            view.parts[slot].load_report = Some(report);
            view.parts[slot].runtime_status.clear();
            view.parts[slot].diagnostics_lent = None;
            match result {
                Ok((instrument, bank, script, snapshot, preload, art, live, pages, fx, rate, irs)) => {
                    let epoch = if script.is_some() {
                        next_epoch(&mut view, slot, snapshot, live)
                    } else {
                        0
                    };
                    view.parts[slot].script_pages = pages;
                    if let Some(selected) = view.parts[slot].live.as_ref().map(|live| live.slot) { view.parts[slot].script_slot = selected; }
                    let mut bank = bank;
                    let residency = bank.as_mut().and_then(|b| b.take_residency());
                    params.shared.residency.lock().unwrap()[slot] =
                        residency.map(|r| (generation, r));
                    let v = &mut view.parts[slot];
                    v.script_state = part.script_state.clone();
                    v.ir_settings = part.ir_settings.clone();
                    v.engine_state = part.engine_state.clone().into();
                    v.delay_state = part.delay_state.clone().into();
                    v.active = instrument.name.clone();
                    v.bytes = bank.as_ref().map(|b| b.bytes).unwrap_or(0);
                    v.freed = 0;
                    v.status = bank.as_deref().map(bank_status).unwrap_or_else(|| {
                        "Controller instrument · KSP playback unavailable".into()
                    });
                    v.fx_rate = rate;
                    v.irs = irs;
                    let _ = params.shared.publish_part((
                        slot,
                        generation,
                        Handoff::Part {
                            bank,
                            fx,
                            script,
                            epoch,
                        },
                    ));
                    if preload.is_some() {
                        v.status += " · preloading…";
                    }
                    drop(view);
                    if let Some(interface) = art {
                        let parent_id = params.shared.view.lock().unwrap().parts[slot].load_report.as_ref().map(|r| r["load_id"].clone());
                        let mut trace = crate::diagnostics::LoadTrace::new(&instrument.path, part.program, Some(slot));
                        trace.detail("instance_id", params.shared.instance_id);
                        trace.detail("operation", "artwork");
                        trace.detail("parent_load_id", parent_id);
                        trace.stage("artwork");
                        let interface = interface.as_deref();
                        let wallpaper = artwork::performance(&instrument, interface);
                        let pages = params.shared.view.lock().unwrap().parts[slot].script_pages.clone();
                        let names: Vec<_> = interface.into_iter().chain(pages.views.iter().map(|p| p.interface.as_ref())).flat_map(artwork::picture_names).collect();
                        let (pictures, errors) = artwork::pictures_report(&instrument.path, names.iter().map(|name| name.as_ref()));
                        for e in errors { trace.issue("artwork", crate::diagnostics::code(&e), e); }
                        if let Err(e) = &wallpaper { trace.issue("artwork", crate::diagnostics::code(e), e); }
                        trace.detail("pictures_loaded", pictures.len());
                        trace.detail("performance_view", interface.is_some_and(|u| u.performance));
                        trace.detail("controls", interface.map_or(0, |u| u.controls.len()));
                        for warning in interface.into_iter().chain(pages.views.iter().map(|p| p.interface.as_ref())).flat_map(crate::ui::font_fallbacks) {
                            trace.issue("ui", "font_fallback", warning);
                        }
                        for c in interface.into_iter().flat_map(|u| &u.controls) {
                            if let Some(reason) = crate::diagnostics::widget_limit(&c.kind) {
                                trace.issue("ui", "unsupported", format!("{} ({}): {reason}", c.kind, c.variable));
                            }
                        }
                        let report = trace.finish(if canceled() { "canceled" } else { "loaded" });
                        if canceled() { continue; }
                        let mut view = params.shared.view.lock().unwrap();
                        let v = &mut view.parts[slot];
                        if let Some(load) = &mut v.load_report {
                            let load = Arc::make_mut(load);
                            if report["status"] != "loaded" && load["status"] == "loaded" { load["status"] = "partial".into(); }
                            load["artwork"] = (*report).clone();
                        }
                        v.pictures = Arc::new(pictures);
                        (v.wallpaper, v.wallpaper_status) = match wallpaper {
                            Ok(image) => (image, String::new()),
                            Err(e) => (None, e),
                        };
                    }
                    // The preload: the full bank takes over from the bare one,
                    // playing voices carrying on (`Engine::upgrade_bank`).
                    let Some((budget, controllers,zone_epoch,zone_init)) = preload else { continue };
                    let parent_id = params.shared.view.lock().unwrap().parts[slot].load_report.as_ref().map(|r| r["load_id"].clone());
                    let mut trace = crate::diagnostics::LoadTrace::new(&instrument.path, part.program, Some(slot));
                    trace.detail("instance_id", params.shared.instance_id);
                    trace.detail("operation", "preload");
                    trace.detail("parent_load_id", parent_id);
                    trace.stage("samples");
                    let bank = Bank::load_cancelable(
                        &instrument,
                        budget,
                        Streaming::Auto,
                        &controllers,
                        &AtomicU32::new(0),
                        &canceled,
                    ).and_then(|mut bank| {bank.prepare_zone_init(zone_epoch,&zone_init,&canceled)?;Ok(bank)});
                    if canceled() { trace.finish("canceled"); continue; }
                    let status = match &bank {
                        Ok(bank) => {
                            trace.detail("resident_bytes", bank.bytes);
                            trace.detail("samples_loaded", bank.sample_count());
                            trace.detail("zones_skipped", bank.skipped_zones);
                            trace.detail("zone_skip_counts", serde_json::json!(bank.zone_skip_counts));
                            if let Some(w) = &bank.warning { trace.issue("samples", "streaming_warning", w); }
                            for e in &bank.issues { trace.issue("samples", "zone_skipped", e); }
                            "loaded"
                        }
                        Err(e) => { trace.fail(format!("{e:#}")); "failed" }
                    };
                    let report = trace.finish(status);
                    let mut view = params.shared.view.lock().unwrap();
                    let v = &mut view.parts[slot];
                    if let Some(load) = &mut v.load_report {
                        let load = Arc::make_mut(load);
                        if report["status"] != "loaded" && load["status"] == "loaded" { load["status"] = "partial".into(); }
                        load["preload"] = (*report).clone();
                    }
                    match bank {
                        Ok(mut bank) => {
                            let residency = bank.take_residency();
                            (v.bytes, v.status) = (bank.bytes, bank_status(&bank));
                            let bank = Handoff::Bank(Box::new(bank));
                            let _ = params.shared.publish_part((slot, generation, bank));
                            // After the bank in the queue: its heads resize the full bank.
                            params.shared.residency.lock().unwrap()[slot] =
                                residency.map(|r| (generation, r));
                        }
                        Err(e) => {
                            v.status = v.status.replace(" · preloading…", "");
                            v.status += &format!(" · preload failed, everything streams: {e:#}");
                        }
                    }
                    // RAM only: the part plays, streaming, while every sample
                    // loads whole; the resident bank then takes over and
                    // playing voices carry on from it.
                    // ponytail: the streaming bank stays resident until the fill
                    // lands (its preload twice over at peak); fill per sample
                    // into the playing bank if that peak matters.
                    if streaming == Streaming::RamOnly {
                        v.status += " · loading into RAM…";
                        let parent_id = v.load_report.as_ref().map(|r| r["load_id"].clone());
                        drop(view);
                        let mut trace = crate::diagnostics::LoadTrace::new(&instrument.path, part.program, Some(slot));
                        trace.detail("instance_id", params.shared.instance_id);
                        trace.detail("operation", "ram_fill");
                        trace.detail("parent_load_id", parent_id);
                        trace.stage("samples");
                        let bank = Bank::load_cancelable(
                            &instrument,
                            budget,
                            Streaming::RamOnly,
                            &controllers,
                            &AtomicU32::new(0),
                            &canceled,
                        ).and_then(|mut bank| {bank.prepare_zone_init(zone_epoch,&zone_init,&canceled)?;Ok(bank)});
                        // Superseded while filling: the newer load has its own bank.
                        if canceled() {
                            trace.finish("canceled");
                            continue;
                        }
                        let status = match &bank {
                            Ok(bank) => {
                                trace.detail("resident_bytes", bank.bytes);
                                trace.detail("samples_loaded", bank.sample_count());
                                trace.detail("zones_skipped", bank.skipped_zones);
                                trace.detail("zone_skip_counts", serde_json::json!(bank.zone_skip_counts));
                                if let Some(w) = &bank.warning { trace.issue("samples", "streaming_warning", w); }
                                for e in &bank.issues { trace.issue("samples", "zone_skipped", e); }
                                "loaded"
                            }
                            Err(e) => { trace.fail(format!("{e:#}")); "failed" }
                        };
                        let report = trace.finish(status);
                        let mut view = params.shared.view.lock().unwrap();
                        let v = &mut view.parts[slot];
                        if let Some(load) = &mut v.load_report {
                            let load = Arc::make_mut(load);
                            if report["status"] != "loaded" && load["status"] == "loaded" { load["status"] = "partial".into(); }
                            load["ram_fill"] = (*report).clone();
                        }
                        match bank {
                            Ok(bank) => {
                                // Loaded whole: nothing left to resize.
                                params.shared.residency.lock().unwrap()[slot] = None;
                                (v.bytes, v.status) = (bank.bytes, bank_status(&bank));
                                let bank = Handoff::Bank(Box::new(bank));
                                let _ = params.shared.publish_part((slot, generation, bank));
                            }
                            Err(e) => {
                                v.status = v.status.replace(" · loading into RAM…", "");
                                v.status += &format!(" · RAM fill failed, streaming: {e:#}");
                            }
                        }
                    }
                }
                Err(e) => {
                    let v = &mut view.parts[slot];
                    v.status = format!("Load failed: {e:#}");
                    v.fx_rate = 0.;
                }
            }
        }
        load_irs(params);
        load_arrays(params);
        load_zone_maps(params);
        if params.shared.diagnostic_free.is_empty() {
            let count = params.shared.grown.load(Ordering::Acquire) as usize;
            for _ in 0..2 { let _ = params.shared.diagnostic_free.force_push(AudioDiagnostics::with_parts(count)); }
        }
        // The host changed sample rate since these effects were built: rebuild them here, off the audio thread.
        let rate = params.shared.rate();
        for slot in 0..params.shared.grown.load(Ordering::Acquire) as usize {
            let stale = {
                let view = params.shared.view.lock().unwrap();
                let v = &view.parts[slot];
                v.instrument
                    .clone()
                    .filter(|_| v.fx_rate != 0. && v.fx_rate != rate)
                    .map(|i| (i, v.irs.clone()))
            };
            let Some((instrument, irs)) = stale else { continue };
            let fx = instrument.fx.processor_for_groups(rate as f32, MAX_BLOCK, &irs, &instrument.groups);
            params.shared.view.lock().unwrap().parts[slot].fx_rate = rate;
            let _ = params.shared.publish_part((
                slot,
                params.shared.part(slot).unwrap().generation.load(Ordering::Acquire),
                Handoff::Fx(fx),
            ));
        }
        // Save script values the audio thread reported, then lend the buffers out again.
        while let Some((slot, epoch, mut snapshot, mut changed)) = params.shared.snapshots.pop() {
            let mut view = params.shared.view.lock().unwrap();
            let v = &mut view.parts[slot];
            if epoch == 0 || epoch != v.script_epoch {
                drop(view);
                continue;
            }
            // File paths stay on the worker. The audio snapshot only updates
            // the small requested parameter values in these prepared records.
            for value in &mut snapshot.ir {
                if let Some(file) = v.irs.iter().rev().find_map(|l| match &l.load {
                    crate::fx::Load::Ir { file, .. } if (l.rack, l.slot) == (value.rack, value.slot) => Some(file), _ => None,
                }) && value.file.as_ref() != Some(file) {
                    value.file = Some(file.clone());
                    changed = true;
                }
            }
            // Unchanged: the saved JSON already holds it, unless nothing is
            // saved yet. Serializing megabytes of script tables ten times a
            // second was most of the loader's time.
            if !changed && !v.script_state.is_empty() {
                let retired = v.snapshot.replace(snapshot);
                drop(view);
                drop(retired);
                continue;
            }
            // Formatting and cloning large persistent arrays must not hold the
            // same mutex every editor frame needs to read its live controls.
            drop(view);
            #[cfg(test)]
            if let Some(gate) = params.shared.snapshot_gate.lock().unwrap().take() {
                gate.wait();
                gate.wait();
            }
            if !crate::ksp::settle_persistence(&mut snapshot.script) {
                let mut view = params.shared.view.lock().unwrap();
                let retired = if view.parts[slot].script_epoch == epoch {
                    view.parts[slot].snapshot.replace(snapshot)
                } else { None };
                drop(view);
                drop(retired);
                continue;
            }
            let json = serde_json::to_string(&snapshot.script).unwrap_or_default();
            let ir_settings = snapshot.ir.clone();
            let engine_state = snapshot.native.saved();
            let delay_state = snapshot.native.saved_delays();
            if snapshot.native.misses != snapshot.native.reported_misses {
                crate::diagnostics::event(crate::diagnostics::LogLevel::Error, "engine", "native_state_unprepared",
                    serde_json::json!({"part":slot,"script_epoch":epoch,"count":snapshot.native.misses,"parameter":snapshot.native.last_miss}));
                snapshot.native.reported_misses = snapshot.native.misses;
            }
            // Prepare the view's owned copies before entering its commit lock.
            let view_json = json.clone();
            let view_ir = ir_settings.clone();
            let view_native = engine_state.clone().into();
            let view_delay = delay_state.clone().into();
            let mut view = params.shared.view.lock().unwrap();
            let v = &mut view.parts[slot];
            // A replacement can publish while this worker formats the snapshot.
            if epoch != v.script_epoch {
                drop(view);
                continue;
            }
            let retired_snapshot = v.snapshot.replace(snapshot);
            if json == v.script_state && ir_settings == v.ir_settings && engine_state.as_slice() == v.engine_state.as_ref() && delay_state.as_slice() == v.delay_state.as_ref() {
                drop(view);
                drop(retired_snapshot);
                continue;
            }
            // The part as the rack names it: the instrument's own path is
            // canonical, and a relative or symlinked part path never matched
            // it, so the saved values never reached the part and the next
            // round rebuilt its scripts from stale ones, ten times a second.
            let target = v.attempted.clone();
            let retired = (
                std::mem::replace(&mut v.script_state, view_json),
                std::mem::replace(&mut v.ir_settings, view_ir),
                std::mem::replace(&mut v.engine_state, view_native),
                std::mem::replace(&mut v.delay_state, view_delay),
            );
            drop(view);
            drop((retired, retired_snapshot));
            let mut current = params.selection.write().unwrap();
            let retired = if let Some(p) = current.parts.get_mut(slot)
                .filter(|p| target.as_ref() == Some(&p.source()))
            {
                Some((
                    std::mem::replace(&mut p.script_state, json),
                    std::mem::replace(&mut p.ir_settings, ir_settings),
                    std::mem::replace(&mut p.engine_state, engine_state),
                    std::mem::replace(&mut p.delay_state, delay_state),
                ))
            } else { None };
            drop(current);
            drop(retired);
        }
        // Visible editors publish and recycle directly, even while this task loads another part.
        params.shared.publish_live(false);
        params.shared.drain_live_diagnostics();
        smart_memory(&params.shared);
        align(params);
        for slot in 0..params.shared.grown.load(Ordering::Acquire) as usize {
            let mut view = params.shared.view.lock().unwrap();
            // Once a second: saving is all it is for, and big script tables
            // cost the audio thread a while to copy.
            let v = &mut view.parts[slot];
            if v.snapshot_lent.is_none_or(|t| t.elapsed() >= SNAPSHOT_EVERY)
                && let Some(mut snapshot) = v.snapshot.take()
            {
                snapshot.native.rewind();
                match params.shared.snapshot_requests.push((slot, v.script_epoch, snapshot)) {
                    Ok(()) => v.snapshot_lent = Some(Instant::now()),
                    Err((_, _, snapshot)) => v.snapshot = Some(snapshot),
                }
            }
        }
    }
}
enum ExactInput { Routed(In, u8), Brightness, Unsupported }

fn host_pattern(address: ExactNoteAddress, clap: bool) -> Option<crate::engine::HostPattern> {
    if matches!(address.port, ExactAddress::InvalidRaw(_)) || matches!(address.channel, ExactAddress::InvalidRaw(_))
        || matches!(address.key, ExactAddress::InvalidRaw(_)) || matches!(address.note_id, ExactAddress::InvalidRaw(_)) { return None; }
    Some(crate::engine::HostPattern { port:address.port.raw_i32(), channel:address.channel.raw_i32(),
        key:address.key.raw_i32(), id:address.note_id.raw_i32(), clap })
}

fn exact_host_input(exact: ExactEventRef<'_>) -> Option<ExactInput> {
    use crate::engine::{HostExpression, HostNote};
    let on = |kind, address:ExactNoteAddress, velocity:f64, clap, tune:f32| {
        let Some(pattern) = host_pattern(address, clap) else { return ExactInput::Unsupported };
        match kind {
            ExactNoteKind::On => {
                let (Ok(port), Ok(channel), Ok(key)) = (u8::try_from(pattern.port), u8::try_from(pattern.channel), u8::try_from(pattern.key)) else { return ExactInput::Unsupported };
                if channel >= 16 || key >= 128 || !velocity.is_finite() || !(0.0..=1.0).contains(&velocity) || !tune.is_finite() { return ExactInput::Unsupported; }
                ExactInput::Routed(In::HostOn(HostNote { port, channel, key, id:pattern.id, clap }, (velocity * 127.).round() as u8, tune), port)
            }
            ExactNoteKind::Off => ExactInput::Routed(In::HostOff(pattern), 0),
            ExactNoteKind::Choke => ExactInput::Routed(In::HostChoke(pattern), 0),
            _ => ExactInput::Unsupported,
        }
    };
    Some(match *exact.body() {
        ExactEventBody::Note { kind, address, velocity } => on(kind,address,velocity,true,0.),
        // VST3 tuning is cents; the engine's expression is semitones. Carry
        // it with the onset so only its newly admitted host owner receives it.
        // Length is unused metadata with no SDK-defined signed range. It never
        // schedules release here; explicit NoteOff remains mandatory.
        ExactEventBody::DetailedNote { kind, address, velocity, tuning, .. } => {
            on(kind,address,f64::from(velocity),false,tuning / 100.)
        },
        ExactEventBody::NoteExpression { expression_id:5, .. }
        | ExactEventBody::NormalizedNoteExpression { expression_id:5, .. } => ExactInput::Brightness,
        ExactEventBody::NoteExpression { expression_id, address, value } => {
            let Some(pattern) = host_pattern(address,true) else { return Some(ExactInput::Unsupported) };
            let x = match expression_id {
                0 if value.is_finite() && (0.0..=4.0).contains(&value) => HostExpression::Gain(value as f32),
                1 if value.is_finite() && (0.0..=1.0).contains(&value) => HostExpression::Pan((value * 2. - 1.) as f32),
                2 if value.is_finite() && (-120.0..=120.0).contains(&value) => HostExpression::Tune(value as f32),
                _ => return Some(ExactInput::Unsupported),
            };
            ExactInput::Routed(In::HostExpression(pattern,x),0)
        }
        // The adapter's faithful VST3 poly-pressure fallback exists only for
        // anonymous IDs. Preserve that established per-key path; concrete IDs
        // cannot borrow it and are diagnosed until their value law is added.
        ExactEventBody::DetailedPolyPressure { .. } if exact.fallback().is_some() => return None,
        ExactEventBody::NormalizedNoteExpression { .. } | ExactEventBody::DetailedPolyPressure { .. } => ExactInput::Unsupported,
        _ => return None,
    })
}

fn input_offset(event: &LosslessEventRef<'_>) -> u32 {
    match event { LosslessEventRef::Typed(e) => e.sample_offset, LosslessEventRef::Exact(e) => e.sample_offset() }
}

#[cfg(feature = "uvi")]
fn native_delivery<'a>(native: &'a mut [Option<uvi_control::Audio>], parts: &'a [Arc<PartShared>], shared: &'a Shared)
    -> impl FnMut(usize, u8, In, &Router, bool) -> bool + 'a {
    move |slot, port, ev, router, reached| {
        let Some(audio) = native.get_mut(slot).and_then(Option::as_mut) else { return false };
        let failed = audio.slot_mut().feed(ev, port, reached, router.mpe_enabled()).is_err();
        if reached && !router.external_input_supported() { audio.slot_mut().abort_activation(); }
        if failed || reached && !router.external_input_supported() {
            part_atoms(parts, shared, slot).unwrap().uvi_failed.store(true, Ordering::Release);
        }
        true
    }
}
fn native_key_held(s: &Dsp, channel: u8, key: u8) -> bool {
    #[cfg(feature = "uvi")]
    return s.uvi.iter().flatten().any(|a| a.slot().host_key_held(channel, key));
    #[cfg(not(feature = "uvi"))]
    { let _ = (s, channel, key); false }
}

fn feed_host_input(s: &mut Dsp, p: &SamplerParams, ev: In, port: u8, offset: u32, holding: bool, rate: f64) {
    let lit = |note:u8, velocity| { if let Some(lit) = p.shared.heard.get(note as usize) { lit.store(velocity,Ordering::Relaxed); } };
    match ev {
        In::NoteOn(_,note,velocity) | In::HostOn(crate::engine::HostNote { key:note, .. },velocity,_) => lit(note,velocity.max(1)),
        In::NoteOff(_,note) => lit(note,0),
        In::HostOff(pattern) | In::HostChoke(pattern) if (0..128).contains(&pattern.key) => lit(pattern.key as u8,0),
        In::Cc(_,120|123,_) => (0..128).for_each(|note| lit(note,0)),
        In::Bend(_,value) => p.shared.bend.store(u32::from(value),Ordering::Relaxed),
        In::Cc(_,1,value) => p.shared.modulation.store(u32::from(value),Ordering::Relaxed),
        _ => {},
    }
    #[cfg(feature = "uvi")]
    {
        let mut external = native_delivery(&mut s.uvi, &s.shared_parts, &p.shared);
        if holding {
            s.align.arrive_with(&mut s.rack, &mut s.routers, port, ev,
                s.align.clock + u64::from(offset), rate, &s.native_slots, &mut external);
        } else { articulate::dispatch_with(&mut s.rack, &mut s.routers, port, ev, &mut external); }
    }
    #[cfg(not(feature = "uvi"))]
    if holding { s.align.arrive(&mut s.rack,&mut s.routers,port,ev,s.align.clock + u64::from(offset),rate); }
    else { articulate::dispatch(&mut s.rack,&mut s.routers,port,ev); }
    if let In::HostOff(pattern) | In::HostChoke(pattern) = ev {
        for key in (0..128).filter(|key| pattern.key == -1 || pattern.key == *key) {
            let held = (0..16).any(|channel| s.rack.parts.iter().any(|e| e.host_key_held(channel,key as u8))
                || native_key_held(s, channel, key as u8)
                || holding && s.align.host_key_held(channel,key as u8));
            p.shared.heard[key as usize].store(u8::from(held),Ordering::Relaxed);
        }
    }
}

fn relay_typed_input(e: &Event, cx: &mut ProcessContext, thru: bool) {
    if thru && matches!(e.body,EventBody::NoteOn { .. } | EventBody::NoteOff { .. } | EventBody::PitchBend { .. } | EventBody::ControlChange { .. }) {
        let mut out = *e; out.port = 0; cx.output_events.push(out);
    }
}

fn feed_typed_input(s: &mut Dsp, p: &SamplerParams, e: &Event, overflow: bool, cx: &mut ProcessContext, thru: bool, holding: bool, rate: f64) {
    relay_typed_input(e,cx,thru);
    let Some(ev) = In::from_event(&e.body) else { return };
    if overflow && matches!(ev,In::NoteTune(..)|In::NoteGain(..)|In::NotePan(..)|In::NotePressure(..)|In::NoteBrightness(..)) {
        if matches!(ev,In::NoteBrightness(..)) { s.unsupported_note_brightness = s.unsupported_note_brightness.saturating_add(1); }
        else { s.unsupported_host_expression = s.unsupported_host_expression.saturating_add(1); }
        return;
    }
    feed_host_input(s,p,ev,e.port,e.sample_offset,holding,rate);
}

fn finish_host_notes(s: &mut Dsp, cx: &mut ProcessContext, offset: u32) {
    for e in &mut s.rack.parts { e.mark_host_notes(); }
    for part in 0..s.rack.parts.len() {
        let mut index = 0;
        while let Some((note,pinned)) = s.rack.parts[part].host_note_at(index) {
            if pinned || s.rack.parts.iter().any(|e| e.host_note_pending(note)) || s.align.host_note_waiting(note)
                || native_note_pending(s, note) {
                #[cfg(feature = "uvi")]
                s.uvi_end_fence.mark_pending(note);
                index += 1; continue;
            }
            #[cfg(feature = "uvi")]
            if !s.uvi_end_fence.permits(note, s.align.clock.saturating_add(u64::from(offset)), s.uvi_latency) {
                index += 1; continue;
            }
            let accepted = !note.clap || cx.output_events.try_push_exact(ExactEvent::new(offset,ExactEventBody::Note {
                kind:ExactNoteKind::End, address:ExactNoteAddress::from_raw_signed(i16::from(note.port),i16::from(note.channel),i16::from(note.key),note.id), velocity:0.,
            })).is_ok();
            if !accepted { s.host_note_end_rejections=s.host_note_end_rejections.saturating_add(1); return; }
            for e in &mut s.rack.parts { e.retire_host_note(note); }
            s.align.retire_host_note(note);
            retire_native_note(s, note);
        }
    }
    // A disabled/remapped-away input can have a held alignment record but
    // no engine root. Close it after key-up without leaking adapter owners.
    let mut index=0;
    while let Some((note,held))=s.align.host_note_at(index) {
        if held || s.align.host_note_waiting(note) || s.rack.parts.iter().any(|e| e.host_note_present(note))
            || native_note_present(s, note) { index+=1; continue; }
        let accepted=!note.clap || cx.output_events.try_push_exact(ExactEvent::new(offset,ExactEventBody::Note {
            kind:ExactNoteKind::End,address:ExactNoteAddress::from_raw_signed(i16::from(note.port),i16::from(note.channel),i16::from(note.key),note.id),velocity:0.,
        })).is_ok();
        if accepted { s.align.retire_host_note(note); } else { s.host_note_end_rejections=s.host_note_end_rejections.saturating_add(1); return; }
    }
    #[cfg(feature = "uvi")]
    for player in 0..s.uvi.len() + s.retiring_uvi.len() {
        let mut index = 0;
        loop {
            let audio = if player < s.uvi.len() { &s.uvi[player] } else { &s.retiring_uvi[player - s.uvi.len()] };
            let Some((note, pinned)) = audio.as_ref().and_then(|a| a.slot().host_note_at(index)) else { break; };
            if pinned || native_note_pending(s, note) || s.rack.parts.iter().any(|e| e.host_note_pending(note))
                || s.align.host_note_waiting(note) { s.uvi_end_fence.mark_pending(note); index += 1; continue; }
            if s.rack.parts.iter().any(|e| e.host_note_present(note))
                && !s.uvi_end_fence.permits(note, s.align.clock.saturating_add(u64::from(offset)), s.uvi_latency) {
                index += 1; continue;
            }
            let accepted = !note.clap || cx.output_events.try_push_exact(ExactEvent::new(offset, ExactEventBody::Note {
                kind: ExactNoteKind::End, address: ExactNoteAddress::from_raw_signed(i16::from(note.port),
                    i16::from(note.channel), i16::from(note.key), note.id), velocity: 0.,
            })).is_ok();
            if !accepted { s.host_note_end_rejections = s.host_note_end_rejections.saturating_add(1); return; }
            for engine in &mut s.rack.parts { engine.retire_host_note(note); }
            s.align.retire_host_note(note);
            retire_native_note(s, note);
        }
    }
}

fn native_underruns(s: &Dsp, slot: usize) -> u64 {
    #[cfg(feature = "uvi")]
    return s.uvi.get(slot).and_then(Option::as_ref).map_or(0, |a| a.slot().underruns());
    #[cfg(not(feature = "uvi"))]
    { let _ = (s, slot); 0 }
}

fn native_note_present(s: &Dsp, note: crate::engine::HostNote) -> bool {
    #[cfg(feature = "uvi")]
    return s.uvi.iter().chain(&s.retiring_uvi).flatten().any(|a| a.slot().has_host_note(note));
    #[cfg(not(feature = "uvi"))]
    { let _ = (s, note); false }
}
fn native_note_pending(s: &Dsp, note: crate::engine::HostNote) -> bool {
    #[cfg(feature = "uvi")]
    return s.uvi.iter().chain(&s.retiring_uvi).flatten().any(|a| a.slot().host_note_pending(note));
    #[cfg(not(feature = "uvi"))]
    { let _ = (s, note); false }
}
fn retire_native_note(s: &mut Dsp, note: crate::engine::HostNote) {
    #[cfg(feature = "uvi")]
    {
        for audio in s.uvi.iter_mut().chain(&mut s.retiring_uvi).flatten() { audio.slot_mut().retire_host_note(note); }
        s.uvi_end_fence.retire(note);
    }
    #[cfg(not(feature = "uvi"))]
    let _ = (s, note);
}

pub struct Dsp {
    /// Boxed: the rack is ~300 KB, too big for a host thread's stack.
    rack: Box<Rack>,
    #[cfg(feature = "uvi")]
    uvi: Vec<Option<uvi_control::Audio>>,
    /// Displaced endpoints retain canonical note tuples until End is accepted.
    #[cfg(feature = "uvi")]
    retiring_uvi: Vec<Option<uvi_control::Audio>>,
    #[cfg(feature = "uvi")]
    uvi_latency: u32,
    #[cfg(feature = "uvi")]
    uvi_reported_latency: u32,
    #[cfg(feature = "uvi")]
    uvi_underruns_retired: u64,
    #[cfg(feature = "uvi")]
    uvi_delays: Option<Box<uvi_delay::Prepared>>,
    #[cfg(feature = "uvi")]
    uvi_end_fence: uvi_delay::Ends,
    #[cfg(feature = "uvi")]
    native_slots: Vec<bool>,
    until_poll: usize,
    audition_left: Vec<usize>,
    /// Epoch of each slot's installed runtime, returned with its persistence snapshots.
    script_epoch: Vec<u64>,
    installed_generation: Vec<u64>,
    /// The lent live view and persistence snapshot being refreshed, a budget
    /// a block: slot, epoch and [`Runtime::changes`] at the start, buffer, progress.
    live: Option<Lent<Box<Live>>>,
    snapshot: Option<Lent<Box<PersistenceSnapshot>>>,
    zone_completion: Option<ZoneReady>,
    /// Per slot, epoch and changes the buffers last refreshed whole hold:
    /// while the scripts do not run they are current and need no refresh.
    live_seen: Vec<(u64, u64)>,
    snapshot_seen: Vec<(u64, u64)>,
    routers: Vec<Router>,
    /// The channel each on-screen key started on, so its note-off follows it
    /// even if the part's instrument changed while the key was held.
    key_channels: KeyChannels,
    /// The slots each on-screen key played on [`EVERY_PART`], one bit each:
    /// its note-off goes to those and no others.
    key_slots: KeySlots,
    /// Recent load: rises with any block's, falls over some 50 blocks.
    load: f32,
    /// Notes held back so every part's attacks land on the grid.
    align: Align,
    until_diagnostics: usize,
    shared_parts: Vec<Arc<PartShared>>,
    diagnostic: AudioDiagnostics,
    unsupported_note_brightness: u64,
    unsupported_host_expression: u64,
    host_note_end_rejections: u64,
    // Created off-thread with the rack, never acquired or replaced by reset/process.
    _diagnostics: crate::diagnostics::DiagnosticLease,
    /// Declared after endpoints: host teardown drops every port before its
    /// last control lease can stop/join the playback threads.
    #[cfg(feature = "uvi")]
    _uvi_controllers: Option<Arc<Mutex<uvi_control::Registry>>>,
}
impl Default for Dsp {
    fn default() -> Self {
        Self { rack: Box::default(),
            #[cfg(feature = "uvi")]
            uvi: (0..RACK_SLOTS).map(|_| None).collect(),
            #[cfg(feature = "uvi")]
            retiring_uvi: (0..64).map(|_| None).collect(),
            #[cfg(feature = "uvi")]
            uvi_latency: 0,
            #[cfg(feature = "uvi")]
            uvi_reported_latency: 0,
            #[cfg(feature = "uvi")]
            uvi_underruns_retired: 0,
            #[cfg(feature = "uvi")]
            uvi_delays: None,
            #[cfg(feature = "uvi")]
            uvi_end_fence: uvi_delay::Ends::default(),
            #[cfg(feature = "uvi")]
            native_slots: vec![false; RACK_SLOTS],
            until_poll: 0, audition_left: vec![0; RACK_SLOTS],
            script_epoch: vec![0; RACK_SLOTS], installed_generation: vec![0; RACK_SLOTS],
            live: None, snapshot: None, zone_completion: None, live_seen: vec![(0, 0); RACK_SLOTS],
            snapshot_seen: vec![(0, 0); RACK_SLOTS], routers: (0..RACK_SLOTS).map(|_| Router::default()).collect(),
            key_channels: KeyChannels::default(), key_slots: KeySlots::default(), load: 0.0,
            align: Align::default(), until_diagnostics: 0, shared_parts: Vec::new(),
            diagnostic: AudioDiagnostics::with_parts(RACK_SLOTS), unsupported_note_brightness: 0, unsupported_host_expression: 0, host_note_end_rejections: 0, _diagnostics: crate::diagnostics::acquire(),
            #[cfg(feature = "uvi")]
            _uvi_controllers: None,
        }
    }
}

/// Every allocation needed by a larger rack is made on the serialized loader.
/// Adoption swaps existing voices, routers and held notes into these buffers;
/// the emptied containers return to that worker for destruction.
struct PreparedGrowth {
    #[cfg(feature = "uvi")]
    native_slots: Vec<bool>,
    #[cfg(feature = "uvi")]
    uvi: Vec<Option<uvi_control::Audio>>,
    #[cfg(feature = "uvi")]
    retiring_uvi: Vec<Option<uvi_control::Audio>>,
    rack: Box<Rack>, align: Align, routers: Vec<Router>,
    audition_left: Vec<usize>, script_epoch: Vec<u64>, installed_generation: Vec<u64>,
    live_seen: Vec<(u64, u64)>, snapshot_seen: Vec<(u64, u64)>,
    key_slots: KeySlots, shared_parts: Vec<Arc<PartShared>>, diagnostic: AudioDiagnostics,
}
impl PreparedGrowth {
    fn new(parts: Vec<Arc<PartShared>>, rate: f64) -> Self {
        let count = parts.len();
        let mut rack = Box::new(Rack::with_slots(count)); rack.reset(rate);
        Self {
            #[cfg(feature = "uvi")]
            uvi: (0..count).map(|_| None).collect(),
            #[cfg(feature = "uvi")]
            native_slots: vec![false; count],
            #[cfg(feature = "uvi")]
            retiring_uvi: (0..count.max(64)).map(|_| None).collect(),
            rack, align: Align::with_slots(count), routers: (0..count).map(|_| Router::default()).collect(),
            audition_left: vec![0; count], script_epoch: vec![0; count], installed_generation: vec![0; count],
            live_seen: vec![(0, 0); count], snapshot_seen: vec![(0, 0); count],
            key_slots: KeySlots(std::array::from_fn(|_| vec![false; count])), shared_parts: parts,
            diagnostic: AudioDiagnostics::with_parts(count) }
    }
    fn adopt(&mut self, dsp: &mut Dsp) {
        let rate = dsp.rack.parts[0].rate();
        for engine in &mut self.rack.parts[dsp.rack.parts.len()..] { engine.reset(rate); }
        dsp.rack.adopt_parts(&mut self.rack); dsp.align.adopt_parts(&mut self.align);
        macro_rules! preserve { ($($field:ident),*) => {$({
            for (old, new) in dsp.$field.iter_mut().zip(&mut self.$field) { std::mem::swap(old, new); }
            std::mem::swap(&mut dsp.$field, &mut self.$field);
        })*}; }
        preserve!(routers, audition_left, script_epoch, installed_generation, live_seen, snapshot_seen);
        #[cfg(feature = "uvi")]
        preserve!(uvi, retiring_uvi, native_slots);
        for (old, new) in dsp.key_slots.0.iter_mut().zip(&mut self.key_slots.0) {
            new[..old.len()].copy_from_slice(old); std::mem::swap(old, new);
        }
        std::mem::swap(&mut dsp.shared_parts, &mut self.shared_parts);
        std::mem::swap(&mut dsp.diagnostic, &mut self.diagnostic);
    }
}

fn part_atoms<'a>(parts: &'a [Arc<PartShared>], shared: &'a Shared, slot: usize) -> Option<&'a PartShared> {
    parts.get(slot).or_else(|| shared.initial_parts.get(slot)).map(Arc::as_ref)
}

struct KeySlots([Vec<bool>; 128]);
impl Default for KeySlots {
    fn default() -> Self {
        Self(std::array::from_fn(|_| vec![false; RACK_SLOTS]))
    }
}
struct KeyChannels([u8; 128]);
impl Default for KeyChannels {
    fn default() -> Self {
        Self([0; 128])
    }
}
/// A lent buffer being refreshed: slot, epoch and changes at the start,
/// the buffer, progress.
type Lent<T> = (usize, (u64, u64), T, Refresh);
/// Script values copied into a lent buffer per block: large tables take
/// several blocks rather than one long one.
const REFRESH_BUDGET: usize = 16384;
const SNAPSHOT_EVERY: std::time::Duration = std::time::Duration::from_secs(1);
/// Live views keep refreshing this long after the editor's last display tick.
const LIVE_WATCH: std::time::Duration = std::time::Duration::from_secs(2);
const LIVE_BUDGET: usize = 512;
pub struct Sampler;

/// Channel that makes the on-screen keyboard reach the part's first zone.
fn preview_channel(e: &Engine) -> u8 {
    e.bank()
        .and_then(|b| {
            b.zones()
                .first()
                .map(|z| b.groups()[z.group].channel.max(0) as u8)
        })
        .unwrap_or(0)
}

/// Mid-range velocity of the first zone on `note`.
fn preview_velocity(e: &Engine, note: u8) -> u8 {
    let zone = e.bank().and_then(|b| {
        b.zones()
            .iter()
            .find(|z| (z.low_key..=z.high_key).contains(&note))
    });
    zone.map_or(100, |z| {
        ((u16::from(z.low_velocity) + u16::from(z.high_velocity)) / 2).max(1) as u8
    })
}

/// One round of every part's smart memory (see `engine/residency.rs`):
/// take back the heads the audio thread swapped, send the next ones, and
/// show what is resident and what was freed.
fn smart_memory(shared: &Shared) {
    let mut residency = shared.residency.lock().unwrap();
    while let Some((slot, generation, heads)) = shared.heads.pop() {
        if let Some((current, r)) = &mut residency[slot]
            && *current == generation
        {
            r.returned(heads);
        }
    }
    for (slot, entry) in residency.iter_mut().enumerate() {
        let Some((generation, r)) = entry else { continue };
        if shared.part(slot).unwrap().generation.load(Ordering::Acquire) != *generation {
            *entry = None;
            continue;
        }
        if let Some(heads) = r.poll(shared.part(slot).unwrap().underruns.load(Ordering::Relaxed))
            && let Err((.., Handoff::Heads(heads))) = shared.ready.push((slot, *generation, Handoff::Heads(heads)))
        {
            r.returned(heads);
        }
        let v = &mut shared.view.lock().unwrap().parts[slot];
        (v.bytes, v.freed) = (r.resident(), r.freed() as u64);
    }
}

/// Route parts as the mixer's "Outputs" choice says and publish the host
/// port names once they settle.
fn route(params: &SamplerParams) {
    let shared = &params.shared;
    // `reroute` takes `view`, which the editor holds while it reads the
    // selection: never hold the selection's lock across it.
    let mut routed = params.selection.read().unwrap().clone();
    let before = routed.clone();
    shared.reroute(&mut routed);
    let names = {
        let mut current = params.selection.write().unwrap();
        // Changed meanwhile: the next pass routes the new selection.
        if routed != before && *current == before {
            *current = routed;
        }
        routing::port_names(&current)
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
        while parts.len() < count { parts.push(Arc::default()); }
        drop(parts);
        let mut view = self.view.lock().unwrap();
        if view.parts.len() < count { view.parts.resize_with(count, PartView::default); }
        drop(view);
        let mut sent = self.sent.lock().unwrap();
        while sent.len() < count { sent.push(Vec::new()); }
        drop(sent);
        let mut residency = self.residency.lock().unwrap();
        while residency.len() < count { residency.push(None); }
    }

    /// Loader only. Keep different parts when the bounded callback queue is
    /// full; replacing a source retires its stale pending payloads here.
    fn publish_part(&self, item: (usize, u64, Handoff)) {
        let mut pending = self.pending_ready.lock().unwrap();
        let (slot, generation, next) = &item;
        pending.retain(|(old_slot, old_generation, old)| old_slot != slot
            || (old_generation == generation && !next.replaces_player()
                && std::mem::discriminant(old) != std::mem::discriminant(next)));
        pending.push_back(item);
        self.flush_ready_inner(&mut pending);
    }

    fn flush_ready(&self) {
        self.flush_ready_inner(&mut self.pending_ready.lock().unwrap());
    }

    fn flush_ready_inner(&self, pending: &mut std::collections::VecDeque<(usize, u64, Handoff)>) {
        while let Some(item) = pending.pop_front() {
            if self.part(item.0).is_none_or(|p| p.generation.load(Ordering::Acquire) != item.1) { continue }
            if let Err(item) = self.ready.push(item) { pending.push_front(item); break }
        }
    }

    /// Loader only: all larger callback storage is prepared and retired here.
    fn prepare_growth(&self) {
        let count = self.with_parts(|parts| parts.len());
        if count <= self.growth_prepared.load(Ordering::Acquire) as usize { return }
        let parts = self.with_parts(|parts| parts.to_vec());
        let count = parts.len();
        let prepared = Box::new(PreparedGrowth::new(parts, self.rate()));
        let _ = self.growth.force_push(prepared);
        self.growth_prepared.store(count as u64, Ordering::Release);
        while self.diagnostic_free.pop().is_some() {}
        for _ in 0..2 { let _ = self.diagnostic_free.force_push(AudioDiagnostics::with_parts(count)); }
    }

    /// [`routing::apply`] with what the audio thread last saw the parts'
    /// instruments route past their outputs, named from the instruments.
    pub(crate) fn reroute(&self, selection: &mut Selection) {
        let mics = self.with_parts(|parts| parts.iter().map(|p| p.mics.each_ref().map(|c| c.load(Ordering::Relaxed))).collect::<Vec<_>>());
        let view = self.view.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        routing::apply(selection, &mics, |slot, code| {
            let instrument = view.parts.get(slot).and_then(|v| v.instrument.as_deref());
            let name = instrument.and_then(|i| match code {
                0x100.. => i.groups.get(usize::from(code - 0x100)).map(|g| g.name.clone()),
                b => i.fx.buses.iter().find(|x| x.index == usize::from(b - 1)).map(|x| x.name.clone()),
            });
            name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| match code {
                0x100.. => "Direct".into(),
                b => format!("Bus {b}"),
            })
        });
    }
}
/// What an instrument routes to each output channel past its own output
/// ([`routing::Mics`]): its first instrument bus there, else its first group.
pub(crate) fn outs_of(engine: &Engine) -> [u16; OUTS] {
    let mut outs = engine.fx().routed().map(|b| b.map_or(0, |b| 1 + u16::from(b)));
    let settings = engine.bank().map_or(&[][..], |b| &b.settings[..]);
    for (g, s) in settings.iter().enumerate() {
        if let Some(c) = s.bus.and_then(|b| b.checked_sub(DIRECT))
            && let Some(o) = outs.get_mut(usize::from(c))
            && *o == 0
        {
            *o = 0x100 + g.min(0xfeff) as u16;
        }
    }
    outs
}
/// How long a new latency must hold before the host hears of it: hosts
/// restart processing for each change, and parts loading one by one would
/// otherwise change it once per part.
const LATENCY_SETTLE: std::time::Duration = std::time::Duration::from_millis(1500);
/// Auto-align on the loader: keep finished measurements with their parts,
/// measure parts not yet measured, and send the audio thread the plan.
fn align(params: &SamplerParams) {
    let shared = &params.shared;
    let done = std::mem::take(&mut *shared.measure.done.lock().unwrap());
    if !done.is_empty() {
        let mut statuses = Vec::new();
        let mut current = params.selection.write().unwrap();
        for (slot, source, result) in done {
            let Some(p) = current.parts.get_mut(slot).filter(|p| timing::source(&p.path, p.program) == source) else {
                continue;
            };
            match result {
                Ok(t) => {
                    p.timing = Timing { override_ms: p.timing.override_ms, exclude: p.timing.exclude, ..t };
                    statuses.push((slot, String::new()));
                }
                // Not measured again until the part changes.
                Err(e) => {
                    p.timing.source = source;
                    statuses.push((slot, format!("Timing not measured: {e}")));
                }
            }
        }
        drop(current);
        let mut view = shared.view.lock().unwrap();
        for (slot, status) in statuses {
            view.parts[slot].timing_status = status;
        }
    }
    let selection = params.selection.read().unwrap().clone();
    if selection.auto_align {
        for (slot, part) in selection.parts.iter().enumerate() {
            if part.path.is_empty() || part.timing.measured(&part.path, part.program) {
                continue;
            }
            let instrument = {
                let view = shared.view.lock().unwrap();
                let v = &view.parts[slot];
                let ready = v.attempted.as_ref().is_some_and(|(path, program, snapshot)| path == &part.path && *program == part.program && snapshot == &part.snapshot) && !v.loading && v.bytes > 0;
                v.instrument.clone().filter(|i| ready && i.path == Path::new(&part.path) && v.program == part.program)
            };
            let Some(instrument) = instrument else { continue };
            let atoms = shared.part(slot).unwrap();
            if atoms.measuring.swap(true, Ordering::AcqRel) {
                continue;
            }
            let measure = shared.measure.clone();
            let measured_part = atoms.clone();
            let source = timing::source(&part.path, part.program);
            let spawned = std::thread::Builder::new().name("kontra-timing".into()).spawn(move || {
                let result = (|| -> anyhow::Result<Timing> {
                    let budget = crate::engine::MEMORY_LIMIT.min(crate::engine::memory_budget());
                    let mut e = timing::engine_for(&instrument, 48_000., budget)?;
                    let arts = timing::found(&instrument, &e);
                    let declared = timing::declared(e.script());
                    let note = timing::probe_note(&instrument, &arts);
                    let (loaded, arts) = timing::measure(&mut e, &arts, note);
                    Ok(Timing { source: source.clone(), loaded, arts, declared, ..Timing::default() })
                })();
                measure.done.lock().unwrap().push((slot, source, result.map_err(|e| format!("{e:#}"))));
                measured_part.measuring.store(false, Ordering::Release);
            });
            if spawned.is_err() {
                atoms.measuring.store(false, Ordering::Release);
            }
        }
    }
    let plan = plan(&selection);
    let mut published = shared.published.lock().unwrap();
    let (sent, waiting) = &mut *published;
    if sent.as_ref() == Some(&plan) {
        *waiting = None;
        return;
    }
    // The latency the host would be told.
    let told = |p: &Plan| if p.on { p.latency_ms } else { 0. };
    let now = Instant::now();
    let settled = match (&sent, &waiting) {
        (None, _) => true,
        (Some(s), _) if told(s) == told(&plan) => true,
        (_, Some((ms, since))) if *ms == told(&plan) => now.duration_since(*since) >= LATENCY_SETTLE,
        _ => {
            *waiting = Some((told(&plan), now));
            false
        }
    };
    if settled {
        *sent = Some(plan.clone());
        *waiting = None;
        shared.reported.store(told(&plan).to_bits(), Ordering::Relaxed);
        let _ = shared.plan.force_push(plan);
    }
}
fn timing_for(p: &Part) -> std::borrow::Cow<'_, Timing> {
    if p.timing.measured(&p.path, p.program) {
        std::borrow::Cow::Borrowed(&p.timing)
    } else {
        // Keep the part-level controls, but discard measurements for another patch.
        std::borrow::Cow::Owned(Timing {
            override_ms: p.timing.override_ms,
            exclude: p.timing.exclude,
            ..Timing::default()
        })
    }
}

/// What the audio thread aligns by, from the persisted rack.
pub(crate) fn plan(selection: &Selection) -> Plan {
    let parts = || selection.parts.iter().filter(|p| !p.is_empty());
    let latency_ms = timing::reported_ms(parts().map(|p| timing_for(p).latest()));
    Plan {
        on: selection.auto_align,
        transport_only: selection.align_transport_only,
        latency_ms,
        parts: (0..selection.parts.len().max(RACK_SLOTS)).map(|n| {
            selection.parts.get(n).filter(|p| !p.is_empty()).map_or_else(Holds::default, |p| {
                let a = &p.articulate;
                let names: Vec<&str> =
                    if a.source == p.path { a.articulations.iter().map(|a| a.name.as_str()).collect() } else { Vec::new() };
                Holds::of(timing_for(p).as_ref(), &names, latency_ms)
            })
        }).collect(),
    }
}
fn bank_status(bank: &Bank) -> String {
    let mut status = format!(
        "{} samples · {} streamed · {:.0} MB",
        bank.sample_count(),
        bank.streamed_samples(),
        bank.bytes as f64 / 1048576.0
    );
    if bank.skipped_zones > 0 {
        status += &format!(
            " · {} zones skipped ({})",
            bank.skipped_zones, bank.zone_skip_counts.summary()
        );
    }
    if let Some(warning) = &bank.warning {
        status += &format!(" · {warning}");
    }
    status
}
impl PluginLogic for Sampler {
    type Params = SamplerParams;
    type DspState = Dsp;
    fn bus_layouts() -> Vec<BusLayout> {
        const NAMES: [&str; BUSES] = [
            "st.1", "st.2", "st.3", "st.4", "st.5", "st.6", "st.7", "st.8", "st.9", "st.10",
            "st.11", "st.12", "st.13", "st.14", "st.15", "st.16",
        ];
        vec![
            NAMES
                .into_iter()
                .fold(BusLayout::new(), |l, name| l.with_output(name, ChannelConfig::Stereo)),
        ]
    }
    fn reset(s: &mut Dsp, p: &SamplerParams, c: &AudioConfig) {
        s.rack.reset(c.sample_rate);
        p.shared
            .rate
            .store(c.sample_rate.to_bits(), Ordering::Release);
        #[cfg(feature = "uvi")]
        {
            if s._uvi_controllers.is_none() {
                s._uvi_controllers = Some(p.shared.uvi_controls.clone());
            }
            p.shared.uvi_max_host_frames.store(c.max_block_size, Ordering::Release);
            p.shared.uvi_epoch.fetch_add(1, Ordering::AcqRel);
            p.shared.uvi_delay_installed.store(0, Ordering::Release);
            s.uvi_reported_latency = if (8000. ..=192000.).contains(&c.sample_rate) && c.sample_rate.fract() == 0. {
                admitted_uvi_latency(&p.shared, c.max_block_size)
            } else { 0 };
            s.uvi_latency = physical_uvi_latency(s, 0);
            s.uvi_end_fence.clear();
            if let Some(delays) = &mut s.uvi_delays { delays.clear(); }
            for (slot, audio) in s.uvi.iter_mut().enumerate() {
                if let Some(audio) = audio { audio.slot_mut().abort_activation(); }
                if let Some(atoms) = part_atoms(&s.shared_parts, &p.shared, slot) {
                    atoms.uvi_generation.store(0, Ordering::Release);
                }
            }
        }
        s.until_poll = 0;
        s.until_diagnostics = 0;
        s.audition_left.fill(0);
        for row in &mut s.key_slots.0 { row.fill(false); }
        s.align.clear();
        for router in &mut s.routers { router.reset_midi(); }
        // The voices are gone, and the host's releases for them may be too.
        p.shared.reset_midi();
        // So are the sound editor's voice dots.
        for tap in &p.shared.probe.voices {
            tap.store(0, Ordering::Relaxed);
        }
    }
    fn process(
        s: &mut Dsp,
        p: &SamplerParams,
        b: &mut AudioBuffer,
        events: &EventList,
        cx: &mut ProcessContext,
    ) -> ProcessStatus {
        let started = Instant::now();
        if !p.shared.discard.is_full() && let Some(mut growth) = p.shared.growth.pop() {
            if growth.rack.parts.len() > s.rack.parts.len() { growth.adopt(s); }
            p.shared.grown.store(s.rack.parts.len() as u64, Ordering::Release);
            p.shared.discard.push(Retired { growth: Some(growth), ..Default::default() }).ok().unwrap();
            // Separate queues have no cross-queue ordering: only this
            // acknowledgment permits the loader to publish new-slot work.
            if let Some(tasks) = cx.tasks::<Load>() { tasks.spawn_coalescing(Load); }
        }
        #[cfg(feature = "uvi")]
        if !p.shared.discard.is_full() && let Some(next) = p.shared.uvi_delays.pop() {
            let current = next.epoch == p.shared.uvi_epoch.load(Ordering::Acquire)
                && next.ticket == p.shared.uvi_delay_wanted.load(Ordering::Acquire)
                && next.storage.slots() == s.rack.parts.len();
            let retired = if current {
                let mut storage = next.storage;
                if let Some(previous) = &mut s.uvi_delays {
                    if previous.adopt_growth(&mut storage) { Some(storage) }
                    else { Some(std::mem::replace(previous, storage)) }
                } else { s.uvi_delays = Some(storage); None }
            } else { Some(next.storage) };
            p.shared.discard.push(Retired { uvi_delays: retired, ..Default::default() }).ok().unwrap();
            if current {
                p.shared.uvi_delay_installed.store(next.ticket, Ordering::Release);
                if let Some(tasks) = cx.tasks::<Load>() { tasks.spawn_coalescing(Load); }
            }
        }
        let rate = s.rack.parts[0].rate();
        let frames = b.num_samples();
        // Offline, a render waits for the disk and keeps every tail; live,
        // tails go before the deadline does.
        let offline = cx.process_mode.is_offline();
        for engine in &mut s.rack.parts {
            engine.blocking_streams = offline;
        }
        if s.until_poll <= frames {
            if let Some(tasks) = cx.tasks::<Load>() {
                tasks.spawn_coalescing(Load);
            }
            for (slot, engine) in s.rack.parts.iter().enumerate() {
                let atoms = part_atoms(&s.shared_parts, &p.shared, slot).unwrap();
                for (m, out) in atoms.mics.iter().zip(outs_of(engine)) {
                    m.store(out, Ordering::Relaxed);
                }
            }
            s.until_poll = (rate * 0.1) as usize;
        } else {
            s.until_poll -= frames;
        }
        if !p.shared.discard.is_full() && let Some(controls) = p.shared.controls.pop() {
            s.rack.set_controls(&controls);
            p.shared.discard.push(Retired { controls: Some(controls), ..Default::default() }).ok().unwrap();
        }
        if !p.shared.discard.is_full() && let Some(plan) = p.shared.plan.pop() {
            let old = std::mem::replace(&mut s.align.plan, plan);
            p.shared.discard.push(Retired { plan: Some(old), ..Default::default() }).ok().unwrap();
        }
        // Held back only while aligning; once not, what was held plays at once.
        let holding = s.align.holding(cx.transport.playing);
        if !p.shared.discard.is_full() && let Some(routes) = p.shared.routes.pop() {
            for (r, route) in s.routers.iter_mut().zip(routes.iter().copied()) { r.set_route(route); }
            p.shared.discard.push(Retired { routes: Some(routes), ..Default::default() }).ok().unwrap();
        }
        // Retired banks, effects and scripts go back to the loader thread to be
        // freed; stop while it cannot take more.
        while !p.shared.discard.is_full() {
            #[cfg(feature = "uvi")]
            if s.retiring_uvi.iter().all(Option::is_some) { break; }
            let Some((slot, generation, handoff)) = p.shared.ready.pop() else {
                break;
            };
            let engine = &mut s.rack.parts[slot];
            let current = generation == part_atoms(&s.shared_parts, &p.shared, slot).unwrap().generation.load(Ordering::Acquire);
            #[cfg(feature = "uvi")]
            let current = current && match &handoff {
                Handoff::Uvi(audio) => audio.epoch() == p.shared.uvi_epoch.load(Ordering::Acquire)
                    && audio.destination() == slot && audio.part_generation() == generation
                    && s.uvi_delays.as_ref().is_some_and(|d| d.slots() == s.uvi.len()
                        && d.latency_frames() == audio.latency_frames()),
                _ => true,
            };
            if current && handoff.replaces_player() {
                s.align.abort_slot(slot);
                #[cfg(feature = "uvi")]
                if let Some(delays) = &mut s.uvi_delays { delays.clear_slot(slot); }
            }
            #[cfg(feature = "uvi")]
            if current && handoff.replaces_player() {
                if let Some(mut previous) = s.uvi[slot].take() {
                    previous.slot_mut().abort_activation();
                    *s.retiring_uvi.iter_mut().find(|p| p.is_none()).unwrap() = Some(previous);
                }
                s.native_slots[slot] = false;
                part_atoms(&s.shared_parts, &p.shared, slot).unwrap().uvi_generation.store(0, Ordering::Release);
            }
            let retired = match handoff {
                #[cfg(feature = "uvi")]
                Handoff::Uvi(audio) if current => {
                    engine.reset(rate);
                    s.script_epoch[slot] = 0;
                    s.installed_generation[slot] = generation;
                    s.uvi_latency = audio.latency_frames();
                    let native_generation = audio.generation();
                    s.uvi[slot] = Some(audio);
                    s.native_slots[slot] = true;
                    let atoms = part_atoms(&s.shared_parts, &p.shared, slot).unwrap();
                    atoms.uvi_state_frame.store(0, Ordering::Release);
                    atoms.uvi_failed.store(false, Ordering::Release);
                    atoms.uvi_part_generation.store(generation, Ordering::Release);
                    atoms.uvi_generation.store(native_generation, Ordering::Release);
                    Retired { bank: engine.set_bank(None), script: engine.set_script(None),
                        fx: Some(engine.set_fx(FxProcessor::default())), ..Default::default() }
                }
                #[cfg(feature = "uvi")]
                Handoff::Uvi(audio) => Retired { uvi: Some(audio), ..Default::default() },
                Handoff::Part {
                    bank,
                    fx,
                    script,
                    epoch,
                } if current => {
                    engine.reset(rate);
                    s.script_epoch[slot] = epoch;
                    s.installed_generation[slot] = generation;
                    Retired {
                        script: engine.set_script(script),
                        bank: engine.set_bank(bank),
                        fx: Some(engine.set_fx(fx)),
                        heads: None,
                        ir: None,
                        ..Retired::default()
                    }
                }
                Handoff::Fx(fx) if current => Retired {
                    fx: Some(engine.set_fx(fx)),
                    ..Retired::default()
                },
                Handoff::Bank(bank) if current => {
                    if engine.zone_upgrade_ready(&bank) {
                        Retired { bank:engine.upgrade_bank(bank),..Retired::default() }
                    } else {
                        Retired { zone_preload:Some((slot,generation,s.script_epoch[slot],bank)),..Retired::default() }
                    }
                },
                Handoff::Script { script, bank, epoch } if current => {
                    engine.reset(rate);
                    s.script_epoch[slot] = epoch;
                    let mut retired=Retired {script:engine.set_script(script),..Retired::default()};
                    if let Some(bank)=bank {retired.bank=engine.set_bank(Some(bank));}
                    retired
                }
                Handoff::Heads(mut heads) if current => {
                    engine.swap_heads(&mut heads);
                    match p.shared.heads.push((slot, generation, heads)) {
                        Ok(()) => Retired::default(),
                        Err((.., heads)) => Retired {
                            heads: Some(heads),
                            ..Retired::default()
                        },
                    }
                }
                Handoff::Part {
                    bank, fx, script, ..
                } => Retired {
                    bank,
                    fx: Some(fx),
                    script,
                    heads: None,
                    ir: None,
                    ..Retired::default()
                },
                Handoff::Heads(heads) => Retired {
                    heads: Some(heads),
                    ..Retired::default()
                },
                Handoff::Fx(fx) => Retired {
                    fx: Some(fx),
                    ..Retired::default()
                },
                Handoff::Bank(bank) => Retired {
                    bank: Some(bank),
                    ..Retired::default()
                },
                Handoff::Script { script, bank, .. } => Retired {
                    script,bank,
                    ..Retired::default()
                },
            };
            let _ = p.shared.discard.push(retired);
        }
        #[cfg(feature = "uvi")]
        {
            s.uvi_reported_latency = admitted_uvi_latency(&p.shared, p.shared.uvi_max_host_frames.load(Ordering::Acquire));
            let latency = s.uvi.iter().flatten().filter(|a| a.epoch() == p.shared.uvi_epoch.load(Ordering::Acquire))
                .map(uvi_control::Audio::latency_frames).max().unwrap_or(0);
            let latency = physical_uvi_latency(s, latency);
            if latency != s.uvi_latency {
                s.uvi_latency = latency;
                if let Some(delays) = &mut s.uvi_delays { delays.clear(); }
                s.uvi_end_fence.clear();
            }
        }
        while !p.shared.discard.is_full() {
            let Some((slot, generation, IrHandoff { ir, epoch, rate: ir_rate, request })) = p.shared.ir_ready.pop() else { break };
            let engine = &mut s.rack.parts[slot];
            let current = generation == part_atoms(&s.shared_parts, &p.shared, slot).unwrap().generation.load(Ordering::Acquire) && epoch == s.script_epoch[slot];
            let settings_current = engine.fx().ir_settings(request.rack, request.slot).is_none_or(|s| s == request.settings);
            let ir = if !current { ir } else if ir_rate == engine.rate() && settings_current {
                engine.finish_ir(request.script_slot, request.id, ir)
            } else {
                if let Err(request) = engine.retry_ir_request(request) {
                    engine.finish_ir(request.script_slot, request.id, None);
                }
                ir
            };
            let _ = p.shared.discard.push(Retired { ir, ..Retired::default() });
        }
        finish_zone_maps(s,p);
        while !p.shared.array_retired.is_full() {
            let Some((slot, generation, epoch, request)) = p.shared.array_ready.pop() else { break };
            let current = generation == part_atoms(&s.shared_parts, &p.shared, slot).unwrap().generation.load(Ordering::Acquire)
                && epoch == s.script_epoch[slot];
            let request = if current {
                match s.rack.parts[slot].finish_array_job(request) {
                    Ok(()) => continue,
                    Err(request) => request,
                }
            } else { request };
            p.shared.array_retired.push((slot, generation, epoch, request)).ok().unwrap();
        }
        let scripted = s.rack.parts.iter().filter(|e| e.script().is_some()).count();
        for engine in &mut s.rack.parts {
            engine.begin_audio_block(frames, scripted, offline);
        }
        s.rack.set_transport(cx.transport.playing, cx.transport.tempo, cx.transport.position_beats,
            (cx.transport.time_sig_num, cx.transport.time_sig_den));
        #[cfg(feature = "uvi")]
        {
            for (slot, audio) in s.uvi.iter_mut().enumerate() {
                if let Some(audio) = audio {
                    let player = audio.slot_mut();
                    let consumed_start = player.frame();
                    // Include this callback's partial native packet before dequeuing controls.
                    // A snapshot waits for real PCM processing at this boundary.
                    let boundary = consumed_start.saturating_add(frames as u64)
                        .div_ceil(crate::uvi::worker::BLOCK_FRAMES as u64)
                        .saturating_mul(crate::uvi::worker::BLOCK_FRAMES as u64);
                    part_atoms(&s.shared_parts, &p.shared, slot).unwrap()
                        .uvi_state_frame.store(boundary, Ordering::Release);
                    if player.collect_completions(consumed_start).is_err()
                        || player.set_host_transport(cx.transport.playing, cx.transport.position_beats,
                            cx.transport.tempo).is_err() {
                        part_atoms(&s.shared_parts, &p.shared, slot).unwrap().uvi_failed.store(true, Ordering::Release);
                    }
                }
            }
            while let Some((slot, stamp, edit)) = p.shared.uvi_edits.pop() {
                if let Some(audio) = s.uvi.get_mut(slot).and_then(Option::as_mut)
                    && (audio.epoch(), audio.generation()) == (stamp.epoch, stamp.generation)
                    && stamp.epoch == p.shared.uvi_epoch.load(Ordering::Acquire)
                    && audio.part_generation() == part_atoms(&s.shared_parts, &p.shared, slot).unwrap().generation.load(Ordering::Acquire)
                    && audio.slot_mut().push_ui(edit).is_err() {
                    part_atoms(&s.shared_parts, &p.shared, slot).unwrap().uvi_failed.store(true, Ordering::Release);
                }
            }
        }
        if !holding && s.align.next_due().is_some() {
            #[cfg(feature = "uvi")]
            s.align.flush_with(&mut s.rack, &mut s.routers,
                &mut native_delivery(&mut s.uvi, &s.shared_parts, &p.shared));
            #[cfg(not(feature = "uvi"))]
            s.align.flush(&mut s.rack, &mut s.routers);
        }
        while let Some((slot, o)) = p.shared.overrides.pop() {
            if let Some(engine) = s.rack.parts.get_mut(slot) {
                engine.set_override(o);
            }
        }
        while let Some(e) = p.shared.file_selections.pop() {
            if e.epoch != 0 && s.script_epoch.get(e.part) == Some(&e.epoch)
                && let Some(engine) = s.rack.parts.get_mut(e.part)
                && let Ok(path) = std::str::from_utf8(&e.path[..e.len])
            {
                engine.ui_file_selection(e.slot, e.control, path);
            }
        }
        while let Some(e) = p.shared.edits.pop() {
            if e.epoch != 0 && s.script_epoch.get(e.part) == Some(&e.epoch) {
                s.rack.parts[e.part].ui_control(e.slot, e.control, e.value);
                s.routers[e.part].forget();
                let picked = s.routers[e.part].articulation_of_control(e.slot, e.control);
                s.align.picked(e.part, picked);
            }
        }
        // Refresh lent live views and persistence snapshots in place, one at
        // a time and a budget a block; the loader shows and saves them.
        let version = |s: &Dsp, slot: usize| {
            let changes = s.rack.parts[slot].script().map_or(0, Runtime::changes);
            (s.script_epoch[slot], changes)
        };
        if s.live.is_none() && !p.shared.lives.is_full() {
            s.live = (p.shared.live_requests.pop())
                .map(|(slot, epoch, live)| (slot, (epoch, version(s, slot).1), live, Refresh::default()));
        }
        if let Some((slot, seen, live, at)) = &mut s.live {
            let done = seen.0 != s.script_epoch[*slot]
                || (s.rack.parts[*slot].script()).is_none_or(|rt| {
                    rt.refresh_diagnostics(live);
                    (*seen == s.live_seen[*slot] && live.refresh_interface && live.interface_current)
                        || rt.refresh_live_within(live, at, LIVE_BUDGET)
                });
            if done && let Some((slot, seen, live, refresh)) = s.live.take() {
                s.live_seen[slot] = seen;
                let _ = p.shared.lives.push((slot, seen.0, live));
                // Publish changed views promptly; unchanged lent buffers still
                // return to the worker on its ordinary diagnostics poll.
                if refresh.changed && let Some(tasks) = cx.tasks::<Load>() {
                    tasks.spawn_coalescing(Load);
                }
            }
        }
        for (e, r) in s.rack.parts.iter_mut().zip(&s.routers) {
            e.attack = p.attack.value();
            e.release = p.release.value();
            e.cutoff = p.cutoff.value() * r.cutoff_scale();
        }
        if p.shared.panic.swap(false, Ordering::AcqRel) {
            p.shared.reset_midi();
            for row in &mut s.key_slots.0 { row.fill(false); }
            s.align.clear();
            for router in &mut s.routers { router.reset_midi(); }
            s.rack.panic();
            #[cfg(feature = "uvi")]
            {
                for audio in s.uvi.iter_mut().chain(&mut s.retiring_uvi).flatten() { audio.slot_mut().panic(); }
                p.shared.uvi_epoch.fetch_add(1, Ordering::AcqRel);
                p.shared.uvi_delay_installed.store(0, Ordering::Release);
                for slot in 0..s.uvi.len() {
                    part_atoms(&s.shared_parts, &p.shared, slot).unwrap().uvi_generation.store(0, Ordering::Release);
                }
                if let Some(tasks) = cx.tasks::<Load>() { tasks.spawn_coalescing(Load); }
                if let Some(delays) = &mut s.uvi_delays { delays.clear(); }
                s.uvi_end_fence.clear();
            }
            s.audition_left.fill(0);
        }
        while let Some((slot, play)) = p.shared.keyboard.pop() {
            if slot == EVERY_PART {
                // As host MIDI on port A, channel 1 plays it.
                let (rack, routers) = (&mut s.rack, &mut s.routers);
                #[cfg(feature = "uvi")]
                let mut external = native_delivery(&mut s.uvi, &s.shared_parts, &p.shared);
                #[cfg(not(feature = "uvi"))]
                let mut external = |_:usize, _:u8, _:In, _:&Router, _:bool| false;
                match play {
                    Play::Note(note, 0) => {
                        let targets = &mut s.key_slots.0[note as usize & 127];
                        articulate::dispatch_to_with(rack, routers, targets.iter_mut().enumerate().filter_map(|(slot, reached)| std::mem::take(reached).then_some(slot)), 0, In::NoteOff(0, note), &mut external);
                    }
                    Play::Note(note, velocity) => {
                        articulate::dispatch_record_with(rack, routers, 0, In::NoteOn(0, note, velocity), &mut s.key_slots.0[note as usize & 127], &mut external);
                    }
                    Play::Bend(value) => drop(articulate::dispatch_with(rack, routers, 0, In::Bend(0, value), &mut external)),
                    Play::Mod(value) => drop(articulate::dispatch_with(rack, routers, 0, In::Cc(0, 1, value), &mut external)),
                }
                continue;
            }
            let Some(engine) = s.rack.parts.get(slot) else { continue };
            let channel = preview_channel(engine);
            let ev = match play {
                Play::Note(note, 0) => In::NoteOff(s.key_channels.0[note as usize & 127], note),
                Play::Note(note, velocity) => {
                    s.key_channels.0[note as usize & 127] = channel;
                    In::NoteOn(channel, note, velocity)
                }
                Play::Bend(value) => In::Bend(channel, value),
                Play::Mod(value) => In::Cc(channel, 1, value),
            };
            #[cfg(feature = "uvi")]
            articulate::play_with(&mut s.rack, &mut s.routers, slot, 0, ev,
                &mut native_delivery(&mut s.uvi, &s.shared_parts, &p.shared));
            #[cfg(not(feature = "uvi"))]
            articulate::play(&mut s.rack, &mut s.routers, slot, ev);
        }
        // With no part selected there is none to audition.
        let selected = p.shared.selected.load(Ordering::Relaxed) as usize;
        if p.shared.audition.swap(false, Ordering::AcqRel) && selected < s.rack.parts.len() {
            let slot = selected;
            let e = &mut s.rack.parts[slot];
            for channel in 0..16 {
                e.cc(channel, 120, 0);
            }
            let requested = p.shared.audition_note.swap(128, Ordering::Relaxed);
            let note = if requested < 128 {
                requested as u8
            } else {
                e.bank()
                    .and_then(|b| b.zones().first())
                    .map_or(60, |z| z.root)
            };
            let (channel, velocity) = (preview_channel(e), preview_velocity(e, note));
            #[cfg(feature = "uvi")]
            if let Some(audio) = &mut s.uvi[slot] {
                let player = audio.slot_mut();
                let _ = player.feed(In::Cc(channel, 123, 0), 0, true, false);
                if player.feed(In::NoteOn(channel, note, velocity), 0, true, false).is_err() {
                    part_atoms(&s.shared_parts, &p.shared, slot).unwrap().uvi_failed.store(true, Ordering::Release);
                }
            } else { e.note_on(channel, note, velocity); }
            #[cfg(not(feature = "uvi"))]
            e.note_on(channel, note, velocity);
            s.audition_left[slot] = (rate * 1.5) as usize;
        }

        let channels = b.num_output_channels();
        let thru = p.shared.midi_thru.load(Ordering::Relaxed);
        let mut peak = [0f32; 2];
        let mut gains = [0f32; MAX_BLOCK];
        let scope = p.shared.scope.source.load(Ordering::Relaxed);
        let mut at = 0;
        let mut incoming = events.lossless_iter().peekable();
        loop {
            // Exact events are consumed once; linked semantic companions do
            // not replace the host tuple or fan an old ID into a new key row.
            while incoming.peek().is_some_and(|e| at >= frames || input_offset(e) as usize <= at) {
                match incoming.next().unwrap() {
                    LosslessEventRef::Typed(e) => feed_typed_input(s,p,e,events.overflow().is_some(),cx,thru,holding,rate),
                    LosslessEventRef::Exact(exact) => match exact_host_input(exact) {
                        Some(ExactInput::Routed(ev,port)) => {
                            if let Some(e) = exact.fallback() { relay_typed_input(e,cx,thru); }
                            for e in exact.companions() { relay_typed_input(e,cx,thru); }
                            feed_host_input(s,p,ev,port,exact.sample_offset(),holding,rate);
                            // A switch or unmatched route creates no sounding
                            // owner. Return that exact identity immediately.
                            if let In::HostOn(note, ..) = ev
                                && note.clap && !s.align.host_note_waiting(note)
                                && !s.rack.parts.iter().any(|e| e.host_note_present(note)) && !native_note_present(s, note) {
                                if cx.output_events.try_push_exact(ExactEvent::new(exact.sample_offset(),ExactEventBody::Note {
                                    kind:ExactNoteKind::End,address:ExactNoteAddress::from_raw_signed(i16::from(note.port),i16::from(note.channel),i16::from(note.key),note.id),velocity:0.,
                                })).is_err() { s.host_note_end_rejections=s.host_note_end_rejections.saturating_add(1); }
                            }
                        }
                        Some(ExactInput::Brightness) => s.unsupported_note_brightness = s.unsupported_note_brightness.saturating_add(1),
                        Some(ExactInput::Unsupported) => s.unsupported_host_expression = s.unsupported_host_expression.saturating_add(1),
                        None => {
                            if let Some(e) = exact.fallback() { feed_typed_input(s,p,e,false,cx,thru,holding,rate); }
                            for e in exact.companions() { feed_typed_input(s,p,e,false,cx,thru,holding,rate); }
                        }
                    },
                }
            }
            if at >= frames {
                break;
            }
            let now = s.align.clock + at as u64;
            let mut due = incoming.peek().map_or(frames,|e| (input_offset(e) as usize).min(frames));
            if holding {
                #[cfg(feature = "uvi")]
                s.align.release_with(now, &mut s.rack, &mut s.routers,
                    &mut native_delivery(&mut s.uvi, &s.shared_parts, &p.shared));
                #[cfg(not(feature = "uvi"))]
                s.align.release(now, &mut s.rack, &mut s.routers);
                if let Some(held) = s.align.next_due() {
                    due = due.min(at + (held - now).min(frames as u64) as usize);
                }
            }
            let len = (due - at).min(MAX_BLOCK);
            for (slot, (e, left)) in s.rack.parts.iter_mut().zip(&mut s.audition_left).enumerate() {
                if *left > 0 {
                    *left = left.saturating_sub(len);
                    if *left == 0 {
                        for channel in 0..16 {
                            e.cc(channel, 123, 0);
                        }
                        #[cfg(feature = "uvi")]
                        if let Some(audio) = &mut s.uvi[slot] {
                            for channel in 0..16 { let _ = audio.slot_mut().feed(In::Cc(channel, 123, 0), 0, true, false); }
                        }
                        #[cfg(not(feature = "uvi"))]
                        let _ = slot;
                    }
                }
            }
            for gain in &mut gains[..len] {
                *gain = db_to_linear(p.volume.read());
            }
            let ports = s.rack.bus_controls.map(|c| usize::from(c.port));
            s.rack.tap = scope.checked_sub(1).filter(|&slot| slot < s.rack.parts.len());
            #[cfg(feature = "uvi")]
            let (buses, live) = {
                let native = &mut s.uvi;
                let shared_parts = &s.shared_parts;
                let mut source = |slot: usize, left: &mut [f32], right: &mut [f32]| {
                    let Some(audio) = &mut native[slot] else { return false };
                    if audio.slot_mut().process_mode(left, right, offline).is_err() {
                        part_atoms(shared_parts, &p.shared, slot).unwrap().uvi_failed.store(true, Ordering::Release);
                    }
                    true
                };
                if s.uvi_latency != 0 && let Some(delays) = &mut s.uvi_delays {
                    s.rack.render_live_with_delay(len, &mut source, delays.as_mut_slice())
                } else { s.rack.render_live_with(len, &mut source) }
            };
            #[cfg(not(feature = "uvi"))]
            let (buses, live) = s.rack.render_live(len);
            if scope == SCOPE_MASTER {
                let mut mono = [0f32; MAX_BLOCK];
                for (_, x) in buses.iter().enumerate().filter(|(bus, _)| live[*bus]) {
                    for (i, (m, gain)) in mono[..len].iter_mut().zip(&gains[..len]).enumerate() {
                        *m += (x[0][i] + x[1][i]) * 0.5 * gain;
                    }
                }
                p.shared.scope.push(&mono[..len]);
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
            if s.rack.tap.is_some() {
                p.shared.scope.push(&s.rack.tapped[..len]);
            }
            at += len;
        }
        finish_host_notes(s,cx,frames.saturating_sub(1) as u32);
        #[cfg(feature = "uvi")]
        for retired in &mut s.retiring_uvi {
            if p.shared.discard.is_full() { break }
            if retired.as_ref().is_some_and(|audio| !audio.slot().has_host_owners()) {
                s.uvi_underruns_retired = s.uvi_underruns_retired.saturating_add(retired.as_ref().unwrap().slot().underruns());
                p.shared.discard.push(Retired { uvi: retired.take(), ..Default::default() }).ok().unwrap();
            }
        }
        if s.snapshot.is_none() && !p.shared.snapshots.is_full() {
            s.snapshot = (p.shared.snapshot_requests.pop())
                .map(|(slot, epoch, saved)| (slot, (epoch, version(s, slot).1), saved, Refresh::default()));
        }
        if let Some((slot, seen, saved, at)) = &mut s.snapshot {
            let done = seen.0 != s.script_epoch[*slot]
                || *seen == s.snapshot_seen[*slot]
                || (s.rack.parts[*slot].script())
                    .is_none_or(|rt| rt.refresh_persistence_within(&mut saved.script, at, REFRESH_BUDGET)
                        && rt.native_state.refresh(&mut saved.native, 256));
            if done && let Some((slot, seen, saved, at)) = s.snapshot.take() {
                let mut saved = saved;
                let mut changed = at.changed || saved.native.changed;
                for value in saved.ir.iter_mut().filter(|_| seen.0 == s.script_epoch[slot]) {
                    if let Some(settings) = s.rack.parts[slot].fx().ir_settings(value.rack, value.slot) {
                        changed |= value.settings != settings;
                        value.settings = settings;
                    }
                }
                s.snapshot_seen[slot] = seen;
                let _ = p.shared.snapshots.push((slot, seen.0, saved, changed));
            }
        }
        // Parameter writes have rendered: capture the block's latest settings.
        let mut arrays_queued = false;
        for (part, engine) in s.rack.parts.iter_mut().enumerate() {
            let generation = part_atoms(&s.shared_parts, &p.shared, part).unwrap().generation.load(Ordering::Acquire);
            while !p.shared.array_requests.is_full() {
                let Some(request) = engine.pop_array_job() else { break };
                p.shared.array_requests.push((part, generation, s.script_epoch[part], request)).ok().unwrap();
                arrays_queued = true;
            }
            while !p.shared.array_retired.is_full() {
                let Some(request) = engine.pop_retired_array_job() else { break };
                p.shared.array_retired.push((part, generation, s.script_epoch[part], request)).ok().unwrap();
                arrays_queued = true;
            }
            while !p.shared.zone_requests.is_full() {
                let Some(request) = engine.pop_zone_job() else { break };
                p.shared.zone_requests.push((part,generation,s.script_epoch[part],request)).ok().unwrap();
                arrays_queued = true;
            }
            while !p.shared.ir_requests.is_full() {
                let Some(request) = engine.pop_ir_request() else { break };
                let generation = part_atoms(&s.shared_parts, &p.shared, part).unwrap().generation.load(Ordering::Acquire);
                let _ = p.shared.ir_requests.push((part, generation, s.script_epoch[part], request));
            }
        }
        if arrays_queued && let Some(tasks) = cx.tasks::<Load>() { tasks.spawn_coalescing(Load); }
        p.shared.blocks.fetch_add(1, Ordering::Relaxed);
        s.align.clock += frames as u64;
        cx.set_meter(P::Level, peak[0].max(peak[1]).min(1.0));
        if frames > 0 && rate > 0. {
            let fall = 0.1f32.powf(frames as f32 / rate as f32);
            let m = &p.shared.meters;
            let peaks = &mut s.rack.peaks;
            for (slot, peak) in peaks.parts.iter().copied().enumerate() {
                let atoms = part_atoms(&s.shared_parts, &p.shared, slot).unwrap();
                Meters::publish(&atoms.meter, peak, fall, &atoms.clip);
            }
            for ((meter, peak), clip) in m.buses.iter().zip(peaks.buses).zip(&m.clips.buses) {
                Meters::publish(meter, peak, fall, clip);
            }
            Meters::publish(&m.master, peak, fall, &m.clips.master);
            peaks.parts.fill([0.0; 2]); peaks.buses.fill([0.0; 2]);
        }
        let watch = p.shared.probe.watch.load(Ordering::Relaxed);
        if let Some((slot, group)) = Probe::watched(watch).filter(|(slot, _)| *slot < s.rack.parts.len()) {
            let published = if s.rack.parts[slot].publish(group, &p.shared.probe) { watch } else { 0 };
            p.shared.probe.published.store(published, Ordering::Relaxed);
        }
        let voices: usize = s.rack.parts.iter().map(Engine::active_voices).sum();
        #[cfg(feature = "uvi")]
        let voices = {
            let native: usize = s.uvi.iter().flatten().map(|a| a.slot().active_voices() as usize).sum();
            p.shared.uvi_voices.store(native as u64, Ordering::Relaxed);
            voices.saturating_add(native)
        };
        p.shared.voices.store(voices as u64, Ordering::Relaxed);
        let audible: usize = s.rack.parts.iter().map(Engine::audible_voices).sum();
        p.shared.audible.store(audible as u64, Ordering::Relaxed);
        let dropouts: u64 = (s.rack.parts.iter())
            .map(|e| e.underruns() + e.dropped_commands())
            .sum();
        #[cfg(feature = "uvi")]
        let dropouts = dropouts.saturating_add(s.uvi_underruns_retired)
            .saturating_add(s.uvi.iter().chain(&s.retiring_uvi).flatten().map(|a| a.slot().underruns()).sum());
        p.shared.dropouts.store(dropouts, Ordering::Relaxed);
        for (slot, e) in s.rack.parts.iter().enumerate() {
            part_atoms(&s.shared_parts, &p.shared, slot).unwrap().underruns.store(e.underruns().saturating_add(native_underruns(s, slot)), Ordering::Relaxed);
        }
        if frames > 0 && rate > 0. {
            // Positive `f32` bits order like the values: the UI swaps out the peak since it last looked.
            let busy = started.elapsed();
            p.shared.busy_ns.fetch_add(busy.as_nanos() as u64, Ordering::Relaxed);
            p.shared.span_ns.fetch_add((frames as f64 * 1e9 / rate) as u64, Ordering::Relaxed);
            let load = (busy.as_secs_f64() * rate / frames as f64) as f32;
            p.shared
                .cpu
                .fetch_max(u64::from(load.to_bits()), Ordering::Relaxed);
            s.load = load.max(s.load * 0.98 + load * 0.02);
            for e in &mut s.rack.parts {
                e.load = s.load;
            }
        }
        capture_audio_diagnostics(s, p, frames, channels, offline, cx);
        ProcessStatus::Normal
    }
    fn editor(params: Arc<SamplerParams>) -> Box<dyn Editor> {
        crate::ui::editor(params)
    }
    /// Auto-align's latency: the latest part's attack (see `timing.rs`).
    fn latency(s: &Dsp) -> u32 {
        let latency = s.align.plan.latency(s.rack.parts[0].rate());
        #[cfg(feature = "uvi")]
        return latency.saturating_add(s.uvi_reported_latency);
        #[cfg(not(feature = "uvi"))]
        latency
    }
}
/// What the UI shows of initialized scripts: the performance view (the last slot with one),
/// its script slot, their issues, and the keyboard the scripts set.
pub(crate) fn script_interface(rt: Option<&Runtime>) -> ScriptView {
    let Some(rt) = rt else {
        return ScriptView::default();
    };
    let live = first_script_live(rt);
    ScriptView {
        interface: live.interface.map(Arc::new),
        slot: live.slot,
        status: rt.diagnostics().join("\n"),
        keys: Arc::new(live.keys),
    }
}
#[derive(Default)]
pub(crate) struct ScriptView {
    pub(crate) interface: Option<Arc<Interface>>,
    pub(crate) slot: usize,
    pub(crate) status: String,
    pub(crate) keys: Arc<BTreeMap<u8, KeyState>>,
}

moose::plugin! { logic:Sampler, params:SamplerParams, tasks:[Load, AudioDiagnosticsTask] }

/// This thread's CPU time in seconds (Linux), which a busy machine's
/// preemption does not inflate the way wall time does; 0 elsewhere.
/// Minor page faults, voluntary and involuntary context switches this
/// thread has taken (Linux; zeros elsewhere).
fn thread_usage() -> [i64; 3] {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: getrusage fills one rusage for the calling thread.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        unsafe { libc::getrusage(libc::RUSAGE_THREAD, &mut usage) };
        [usage.ru_minflt, usage.ru_nvcsw, usage.ru_nivcsw]
    }
    #[cfg(not(target_os = "linux"))]
    [0; 3]
}
fn thread_cpu() -> f64 {
    cpu_clock(3)
}
/// CPU seconds of `clock`: 2 is the whole process, 3 this thread (Linux).
fn cpu_clock(clock: i32) -> f64 {
    #[cfg(target_os = "linux")]
    {
        #[repr(C)]
        struct Timespec {
            s: i64,
            ns: i64,
        }
        unsafe extern "C" {
            fn clock_gettime(clock: i32, t: *mut Timespec) -> i32;
        }
        let mut t = Timespec { s: 0, ns: 0 };
        // SAFETY: clock_gettime writes one timespec for a CPU-time clock.
        unsafe { clock_gettime(clock, &mut t) };
        t.s as f64 + t.ns as f64 * 1e-9
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = clock;
        0.0
    }
}
/// This process's resident memory, MiB (Linux; 0 elsewhere).
/// Memory the process holds, resident or swapped out: on a machine short
/// of RAM the resident part alone says more about the others than about us.
fn rss_mib() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kib = |field: &str| {
        let line = status.lines().find(|l| l.starts_with(field))?;
        line.split_whitespace().nth(1)?.parse::<f64>().ok()
    };
    kib("VmRSS:").unwrap_or(0.) / 1024. + kib("VmSwap:").unwrap_or(0.) / 1024.
}
/// This thread's user-space instruction or cycle count from the CPU's
/// counters (Linux x86-64, where `perf_event_open` is allowed). Instructions
/// are the same run to run however busy the machine is, so small changes
/// compare where timings drown in noise.
struct Counter(i32);
impl Counter {
    fn open(config: u64) -> Option<Self> {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            unsafe extern "C" {
                fn syscall(n: i64, ...) -> i64;
            }
            // perf_event_attr, version 5 (112 bytes): hardware `config`,
            // user space only (exclude_kernel, exclude_hv), enabled.
            let mut attr = [0u64; 14];
            attr[0] = 112 << 32;
            attr[1] = config;
            attr[5] = (1 << 5) | (1 << 6);
            // SAFETY: perf_event_open(attr, this thread, any CPU, no group, no flags).
            let fd = unsafe { syscall(298, attr.as_ptr(), 0i32, -1i32, -1i32, 0u64) };
            (fd >= 0).then_some(Self(fd as i32))
        }
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        {
            let _ = config;
            None
        }
    }

    fn read(&self) -> u64 {
        unsafe extern "C" {
            fn read(fd: i32, buf: *mut u64, n: usize) -> isize;
        }
        let mut value = 0;
        // SAFETY: reads one u64 count from the counter's descriptor.
        unsafe { read(self.0, &mut value, 8) };
        value
    }
}
/// Host-shaped timing: the parts at `paths` in the rack, `Sampler::process`
/// called at real-time pace for 512-frame blocks at 48 kHz on sixteen
/// active stereo ports, with the loader running every 100 ms as the host's
/// task does. First `seconds` idle, then `seconds` with `notes` held notes
/// on every part, one replaced every `1 s / notes`. Prints block times and,
/// where the CPU's counters are readable, instructions per block.
/// How the playing voices could share work: muted, filtered, envelope
/// stage, and how many distinct resampling phases and sources they have.
fn census(voices: &[crate::engine::VoiceInfo]) {
    use std::collections::HashMap;
    let audible: Vec<_> = voices.iter().filter(|v| v.gain > 0.0).collect();
    let mut phases: HashMap<String, usize> = HashMap::new();
    for v in &audible {
        *phases.entry(format!("{:?}", v.phase)).or_default() += 1;
    }
    let key = |v: &crate::engine::VoiceInfo| ((v.step * 4294967296.0) as u64, ((v.pos.fract()) * 4294967296.0) as u64);
    let count = |f: &dyn Fn(&&&crate::engine::VoiceInfo) -> bool, k: &dyn Fn(&crate::engine::VoiceInfo) -> String| {
        let mut m: HashMap<String, usize> = HashMap::new();
        for v in audible.iter().filter(|v| f(v)) {
            *m.entry(k(v)).or_default() += 1;
        }
        m.len()
    };
    let unfiltered = audible.iter().filter(|v| !v.filtered).count();
    let mut steps: Vec<(f64, bool)> = audible.iter().map(|v| (v.step, v.pos.fract() == 0.0)).collect();
    steps.sort_by(|a, b| a.0.total_cmp(&b.0));
    steps.dedup();
    println!("  steps (step, integer position): {:?}", &steps[..steps.len().min(8)]);
    println!(
        "  census: {} voices · {} muted · {} audible ({} filtered) · stages {:?} · unfiltered: {} step groups, {} step+phase groups, {} identical-source groups · all audible: {} step+phase groups",
        voices.len(),
        voices.len() - audible.len(),
        audible.len(),
        audible.len() - unfiltered,
        phases,
        count(&|v| !v.filtered, &|v| format!("{}", key(v).0)),
        count(&|v| !v.filtered, &|v| format!("{:?}", key(v))),
        count(&|v| !v.filtered, &|v| format!("{} {} {}", v.sample, v.pos, v.step)),
        count(&|_| true, &|v| format!("{:?}", key(v))),
    );
}

/// The real worker, edit queue and audio live-view budget under 60 Hz UI edits.
pub(crate) fn bench_ui_worker(engine: Engine, instrument: Arc<Instrument>, program: u32, control: usize, low: i32, high: i32, frames: usize, edits: usize, mut observer: Option<&mut dyn FnMut(&Arc<SamplerParams>) -> anyhow::Result<()>>) -> anyhow::Result<(serde_json::Value, Engine)> {
    use moose::core::bus_routing::{BusActivation, BusRouting};
    use std::time::Duration;
    let params = Arc::new(SamplerParams::new());
    let rt = engine.script().unwrap();
    let live = Box::new(rt.live());
    let saved = Box::new(PersistenceSnapshot { script: rt.persistence(), ir: instrument.fx.ir_settings_with(&rt.init_irs), native: rt.native_state.snapshot() });
    let json = serde_json::to_string(&saved.script)?;
    let part = Part { path: instrument.path.to_string_lossy().into_owned(), program, script_state: json.clone(), ir_settings: saved.ir.clone(), ..Part::default() };
    let streaming = part.streaming(params.selection.read().unwrap().streaming);
    params.selection.write().unwrap().parts = vec![part.clone()];
    {
        let mut view = params.shared.view.lock().unwrap();
        let v = &mut view.parts[0];
        v.attempted = Some(part.source());
        v.instrument = Some(instrument.clone());
        v.streaming = streaming;
        v.fx_rate = 48000.;
        v.script_epoch = 1;
        v.script_state = json;
        v.ir_settings = part.ir_settings;
        v.irs = rt.init_irs.clone();
        v.interface = live.interface.clone().map(Arc::new);
        v.script_slot = live.slot;
        v.live = Some(live);
        v.snapshot = Some(saved);
    }
    let mut dsp = Dsp::default();
    Sampler::reset(&mut dsp, &params, &AudioConfig::new(48000., frames));
    dsp.rack.parts[0] = engine;
    dsp.script_epoch[0] = 1;
    params.shared.watched.store(true, Ordering::Relaxed);
    Load.run(&params);
    moose::core::tasks::warm_pool();
    let load_times = Arc::new(Mutex::new(Vec::new()));
    let publications = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let worker = {
        let (params, times, publications) = (params.clone(), load_times.clone(), publications.clone());
        moose::core::tasks::TaskSpawner::<Load>::new_serialized(move |task| {
            let at = Instant::now();
            task.run(&params);
            times.lock().unwrap().push(at.elapsed().as_secs_f64() * 1e3);
            publications.fetch_add(1, Ordering::Release);
        })
    };
    let mut tasks = moose::core::tasks::TaskSpawnerBundle::new();
    tasks.push(worker.clone());
    let tasks = tasks.into_any().unwrap();
    #[cfg(test)]
    if std::env::var_os("KONTRA_UI_BENCH_CONCURRENT_AUDIO").is_some() {
        let result = bench_ui_concurrent_audio(dsp, &params, &tasks, control, low, high, frames, edits, observer.take(), &load_times);
        drop(tasks);
        drop(worker);
        return result;
    }
    let transport = TransportInfo::default();
    let mut data = vec![vec![0.0f32; frames]; 2 * BUSES];
    let mut outgoing = EventList::with_capacity(0);
    let none = EventList::with_capacity(0);
    let (mut edit, mut next_edit) = (0, Duration::ZERO);
    let (mut waits, mut edit_times, mut completions, mut gaps) = (Vec::new(), Vec::new(), 0, Vec::new());
    let (start, mut last_completion) = (Instant::now(), Instant::now());
    let mut seen = dsp.live_seen[0];
    let mut published = 0;
    let mut process_times = Vec::new();
    let mut pending_edit = None;
    let mut publication_latencies = Vec::new();
    let (mut next_observe, mut last_observe) = (Duration::ZERO, start);
    let mut observer_gaps = Vec::new();
    let blocks = ((edits as f64 / 60.0 + 1.0) * 48000.0 / frames as f64).ceil() as usize;
    for block in 0..blocks {
        params.shared.watched.store(true, Ordering::Relaxed);
        if edit < edits && start.elapsed() >= next_edit {
            let at = Instant::now();
            let retained = params.shared.view.lock().unwrap().parts[0].interface.clone();
            waits.push(at.elapsed().as_secs_f64() * 1e3);
            let value = (i64::from(low) + (i64::from(high) - i64::from(low)) * (edit % 101) as i64 / 100) as i32;
            let at = Instant::now();
            params.shared.edit_control(0, control, value);
            pending_edit = Some(at);
            edit_times.push(at.elapsed().as_secs_f64() * 1e3);
            drop(retained);
            edit += 1;
            next_edit = Duration::from_secs_f64(edit as f64 / 60.0);
        }
        let mut channels: Vec<_> = data.iter_mut().map(|v| v.as_mut_slice()).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, frames);
        let mut routing = BusRouting::new();
        for _ in 0..BUSES { routing.push_output(2, BusActivation::Active); }
        let mut context = ProcessContext::new(&transport, 48000., frames, &mut outgoing).with_bus_routing(routing).with_tasks(&tasks);
        let at = Instant::now();
        Sampler::process(&mut dsp, &params, &mut buffer, &none, &mut context);
        process_times.push(at.elapsed().as_secs_f64() * 1e3);
        let ready = publications.load(Ordering::Acquire);
        // The editor owns publication once its first frame marks the heartbeat.
        // Pump real frames independently of Load; its completions cannot wake an
        // editor that is now responsible for draining and re-lending live views.
        let observing = observer.is_some();
        let frame_due = observing && start.elapsed() >= next_observe;
        if frame_due {
            let at = Instant::now();
            observer.as_mut().unwrap()(&params)?;
            observer_gaps.push((at - last_observe).as_secs_f64() * 1e3);
            last_observe = at;
            next_observe = Duration::from_secs_f64(((start.elapsed().as_secs_f64() * 60.).floor() + 1.) / 60.);
        }
        if frame_due || (!observing && ready != published) {
            // Only a callback-derived Live value can settle the optimistic edit.
            // Coalesced drags measure delivery of the latest edit, including
            // observer rendering when one is installed.
            if pending_edit.is_some() && !params.shared.view.lock().unwrap().parts[0].edited.iter().any(|e| e.0 == control) {
                publication_latencies.push(pending_edit.take().unwrap().elapsed().as_secs_f64() * 1e3);
            }
        }
        published = ready;
        if dsp.live_seen[0] != seen {
            seen = dsp.live_seen[0];
            completions += 1;
            gaps.push(last_completion.elapsed().as_secs_f64() * 1e3);
            last_completion = Instant::now();
        }
        let deadline = start + Duration::from_secs_f64((block + 1) as f64 * frames as f64 / 48000.0);
        if let Some(left) = deadline.checked_duration_since(Instant::now()) { std::thread::sleep(left); }
    }
    // Retiring the lane waits for its running handler before releasing Params.
    drop(tasks);
    drop(worker);
    let summary = |mut values: Vec<f64>| {
        values.sort_by(f64::total_cmp);
        if values.is_empty() { return serde_json::json!({}); }
        serde_json::json!({"mean":values.iter().sum::<f64>() / values.len() as f64,"p50":values[values.len()/2],"max":values[values.len()-1]})
    };
    let load_times = load_times.lock().unwrap().clone();
    Ok((serde_json::json!({"host_frames":frames,"edits_delivered":edit,"live_refresh_completions":completions,
        "live_completion_gap_ms":summary(gaps),"view_mutex_acquire_ms":summary(waits),"shared_edit_ms":summary(edit_times),
        "load_runs":load_times.len(),"load_wall_ms":summary(load_times),"audio_process_ms":summary(process_times),
        "editor_frames":observer_gaps.len(),"editor_frame_gap_ms":summary(observer_gaps),"editor_requested_hz":if observer.is_some() {60} else {0},
        "latest_edits_published":publication_latencies.len(),"latest_edit_to_publication_observer_ms":summary(publication_latencies)}), std::mem::take(&mut dsp.rack.parts[0])))
}

/// Opt-in actual host pacing: audio and the editor run independently, while
/// retaining the same serialized Load lane and immutable publication path.
#[cfg(test)]
fn bench_ui_concurrent_audio(mut dsp: Dsp, params: &Arc<SamplerParams>, tasks: &moose::core::tasks::AnyTaskSpawner,
    control: usize, low: i32, high: i32, frames: usize, edits: usize,
    mut observer: Option<&mut dyn FnMut(&Arc<SamplerParams>) -> anyhow::Result<()>>, load_times: &Arc<Mutex<Vec<f64>>>)
    -> anyhow::Result<(serde_json::Value, Engine)> {
    use moose::core::bus_routing::{BusActivation, BusRouting};
    use std::time::Duration;
    let chord = std::env::var("KONTRA_UI_BENCH_CHORD").ok().map(|notes| notes.split(',')
        .map(|note| note.trim().parse::<u8>()).collect::<Result<Vec<_>, _>>()).transpose()?.unwrap_or_default();
    anyhow::ensure!(chord.iter().all(|note| *note < 128), "benchmark chord notes must be 0..127");
    let summary = |mut samples: Vec<f64>| {
        samples.sort_by(f64::total_cmp);
        if samples.is_empty() { return serde_json::json!({}); }
        serde_json::json!({"n":samples.len(),"mean":samples.iter().sum::<f64>()/samples.len() as f64,
            "p50":samples[samples.len()/2],"p99":samples[((samples.len()-1) as f64*0.99).ceil() as usize],"max":samples[samples.len()-1]})
    };
    let stop = AtomicBool::new(false);
    // Drop precedes Scope's join on both observer errors and panics.
    struct Stop<'a>(&'a AtomicBool);
    impl Drop for Stop<'_> { fn drop(&mut self) { self.0.store(true, Ordering::Release); } }
    let start = Instant::now();
    let warmup = Duration::from_millis(250);
    let duration = warmup + Duration::from_secs_f64(edits as f64/60. + 1.);
    std::thread::scope(|scope| {
        let audio = scope.spawn(|| {
            let mut data = vec![vec![0.0f32; frames]; 2*BUSES];
            let mut outgoing = EventList::with_capacity(0);
            let mut incoming = EventList::with_capacity(chord.len());
            let transport = TransportInfo::default();
            let mut process_times = Vec::new();
            let (mut block, mut peak_voices, mut late_starts, mut missed) = (0, 0, 0, 0);
            let mut peak = 0.0f32;
            let (mut edit_peak_voices, mut audible_blocks, mut nonfinite_samples) = (0, 0, 0);
            let (mut gaps, mut last_completion, mut seen) = (Vec::new(), start, dsp.live_seen[0]);
            while !stop.load(Ordering::Acquire) {
                let scheduled = start + Duration::from_secs_f64(block as f64*frames as f64/48000.);
                if let Some(left) = scheduled.checked_duration_since(Instant::now()) { std::thread::sleep(left); }
                if stop.load(Ordering::Acquire) { break; }
                if Instant::now().saturating_duration_since(scheduled).as_secs_f64()*1000. > frames as f64/48. { late_starts += 1; }
                incoming.clear();
                if block == 0 {
                    for &note in &chord { incoming.push(Event::new(0, EventBody::NoteOn {group:0,channel:0,note,velocity:100})); }
                }
                let mut channels: Vec<_> = data.iter_mut().map(|channel| channel.as_mut_slice()).collect();
                let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, frames);
                let mut routing = BusRouting::new();
                for _ in 0..BUSES { routing.push_output(2, BusActivation::Active); }
                let mut context = ProcessContext::new(&transport, 48000., frames, &mut outgoing).with_bus_routing(routing).with_tasks(tasks);
                let at = Instant::now();
                Sampler::process(&mut dsp, params, &mut buffer, &incoming, &mut context);
                let elapsed = at.elapsed().as_secs_f64()*1000.;
                missed += usize::from(elapsed > frames as f64/48.);
                process_times.push(elapsed);
                peak_voices = peak_voices.max(dsp.rack.parts[0].active_voices());
                if start.elapsed() >= warmup && start.elapsed() < warmup + Duration::from_secs_f64(edits as f64/60.) {
                    edit_peak_voices = edit_peak_voices.max(dsp.rack.parts[0].active_voices());
                }
                let mut audible = false;
                for sample in data.iter().flatten() {
                    nonfinite_samples += usize::from(!sample.is_finite());
                    peak = peak.max(sample.abs());
                    audible |= sample.abs()>1e-8;
                }
                audible_blocks += usize::from(audible);
                if dsp.live_seen[0] != seen {
                    seen = dsp.live_seen[0];
                    gaps.push(last_completion.elapsed().as_secs_f64()*1000.);
                    last_completion = Instant::now();
                }
                block += 1;
            }
            let report = serde_json::json!({"blocks":block,"deadline_ms":frames as f64/48.,"process_deadline_misses":missed,
                "late_starts_over_one_block":late_starts,"peak_voices":peak_voices,"peak_voices_during_edits":edit_peak_voices,
                "audible_blocks":audible_blocks,"nonfinite_samples":nonfinite_samples,"output_peak":peak,
                "underruns":dsp.rack.parts[0].underruns(),"dropped_commands":dsp.rack.parts[0].dropped_commands()});
            (dsp, process_times, gaps, report)
        });
        let _stop_on_exit = Stop(&stop);
        let (mut edit, mut next_edit, mut next_observe) = (0, warmup, Duration::ZERO);
        let (mut waits, mut edit_times, mut acknowledgements, mut observer_times) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut pending = None;
        while start.elapsed() < duration {
            params.shared.watched.store(true, Ordering::Relaxed);
            if edit < edits && start.elapsed() >= next_edit {
                let at = Instant::now();
                let retained = params.shared.view.lock().unwrap().parts[0].interface.clone();
                waits.push(at.elapsed().as_secs_f64()*1000.);
                let value = (i64::from(low)+(i64::from(high)-i64::from(low))*(edit%101) as i64/100) as i32;
                let at = Instant::now();
                params.shared.edit_control(0, control, value);
                edit_times.push(at.elapsed().as_secs_f64()*1000.);
                pending = Some(at);
                drop(retained);
                edit += 1;
                next_edit = warmup + Duration::from_secs_f64(edit as f64/60.);
            }
            if start.elapsed() >= next_observe {
                let at = Instant::now();
                if let Some(observer) = observer.as_mut() { observer(params)?; } else { params.shared.publish_live(true); }
                observer_times.push(at.elapsed().as_secs_f64()*1000.);
                if pending.is_some() && !params.shared.view.lock().unwrap().parts[0].edited.iter().any(|edit| edit.0 == control) {
                    acknowledgements.push(pending.take().unwrap().elapsed().as_secs_f64()*1000.);
                }
                next_observe = Duration::from_secs_f64(((start.elapsed().as_secs_f64()*60.).floor()+1.)/60.);
            }
            let due = if edit < edits { next_observe.min(next_edit) } else { next_observe };
            if let Some(left) = due.checked_sub(start.elapsed()) { std::thread::sleep(left.min(Duration::from_millis(2))); }
        }
        stop.store(true, Ordering::Release);
        let (mut dsp, process_times, completion_gaps, audio_report) = audio.join().map_err(|_| anyhow::anyhow!("benchmark audio thread panicked"))?;
        let load_times = load_times.lock().unwrap().clone();
        let mut report = serde_json::json!({"concurrent_audio":true,"held_chord":chord,"host_frames":frames,"edits_delivered":edit,
            "audio_process_ms":summary(process_times),"audio":audio_report,"load_runs":load_times.len(),"load_wall_ms":summary(load_times),
            "live_completion_gap_ms":summary(completion_gaps),"view_mutex_acquire_ms":summary(waits),"shared_edit_ms":summary(edit_times),
            "editor_frames":observer_times.len(),"observer_wall_ms":summary(observer_times),"editor_requested_hz":60,
            "latest_edits_published":acknowledgements.len(),"latest_edit_to_publication_observer_ms":summary(acknowledgements)});
        report["warmup_ms"] = serde_json::json!(warmup.as_secs_f64()*1000.);
        if !chord.is_empty() {
            anyhow::ensure!(audio_report["peak_voices_during_edits"].as_u64().unwrap_or(0)>0 && audio_report["output_peak"].as_f64().unwrap_or(0.)>0.,
                "held-chord benchmark must play actual nonzero sample output: {audio_report}");
        }
        dsp.rack.parts[0].panic();
        Ok((report, std::mem::take(&mut dsp.rack.parts[0])))
    })
}

/// Profile real script control edits separately from view copying and saved-state
/// serialization. Sample headers and effects are installed, but no notes play.
pub fn bench_ui_control(path: &Path, variable: &str, edits: usize) -> anyhow::Result<()> {
    anyhow::ensure!((1..=10000).contains(&edits), "edits must be 1..10000");
    let instrument = import::read(path)?;
    let (runtime, errors) = crate::engine::load_scripts(&instrument, instrument.script_state.clone(), 48000.0);
    anyhow::ensure!(errors.is_empty(), "Script initialization failed: {errors:?}");
    let runtime = runtime.ok_or_else(|| anyhow::anyhow!("Instrument has no scripts"))?;
    let mut live = runtime.live();
    let interface = live.interface.as_ref().ok_or_else(|| anyhow::anyhow!("No performance view"))?;
    let control = interface.controls.iter().position(|c| c.variable == variable)
        .ok_or_else(|| anyhow::anyhow!("Control {variable} not found"))?;
    let range = |name: &str, default| match interface.controls[control].properties.get(name) {
        Some(crate::ksp::Value::Int(value)) => *value, _ => default,
    };
    let (low, high) = (range("$CONTROL_PAR_MIN_VALUE", 0), range("$CONTROL_PAR_MAX_VALUE", 1000000));
    let mut saved = runtime.persistence();
    let controls = interface.controls.len();
    let array_cells: usize = interface.controls.iter().map(|c| match c.properties.get("$CONTROL_PAR_VALUE") {
        Some(crate::ksp::Value::IntArray(v)) => v.len(),
        Some(crate::ksp::Value::RealArray(v)) => v.len(),
        Some(crate::ksp::Value::Array(v)) => v.len(),
        _ => 0,
    }).sum();
    let mut engine = Engine::default();
    engine.set_bank(Some(Box::new(Bank::load_bare(&instrument)?)));
    engine.set_fx(crate::engine::effects(&instrument, Some(&runtime), 48000.0));
    engine.set_script(Some(runtime));
    let (mut left, mut right) = ([0.0f32; 128], [0.0f32; 128]);
    let mut times = BTreeMap::<&str, Vec<f64>>::new();
    let mut record = |stage, time: Instant| {
        times.entry(stage).or_default().push(time.elapsed().as_secs_f64() * 1e3);
    };
    // The same mutex scope as the publication worker, without involving drawing.
    let view = Mutex::new(live.interface.clone().map(Arc::new));
    let (mut json_bytes, mut live_passes, mut saved_passes, mut changed_edits) = (0, 0, 0, 0);
    for edit in 0..edits {
        let value = (i64::from(low) + (i64::from(high) - i64::from(low)) * (edit % 101) as i64 / 100) as i32;
        engine.begin_audio_block(128, 1, false);
        let at = Instant::now();
        engine.ui_control(live.slot, control, value);
        record("callback_ms", at);
        let at = Instant::now();
        // 21.3 ms of audio allows authored macro wait(2000) continuations to run.
        for _ in 0..8 { engine.render(&mut left, &mut right); }
        record("deferred_callbacks_and_idle_effects_ms", at);
        let rt = engine.script().unwrap();
        let at = Instant::now();
        let mut refresh = Refresh::default();
        loop {
            live_passes += 1;
            if rt.refresh_live_within(&mut live, &mut refresh, LIVE_BUDGET) { break }
        }
        record("live_refresh_ms", at);
        let at = Instant::now();
        let mut locked = view.lock().unwrap();
        let copied = live.interface.clone();
        if locked.as_deref() != copied.as_ref() { *locked = copied.map(Arc::new); }
        drop(locked);
        record("live_publication_lock_ms", at);
        let at = Instant::now();
        // A retained drawing snapshot makes Arc::make_mut copy the full view
        // in Shared::edit_control; keep one here to measure that path.
        let mut locked = view.lock().unwrap();
        let retained = locked.clone();
        if let Some(c) = locked.as_mut().and_then(|i| Arc::make_mut(i).controls.get_mut(control)) {
            c.properties.insert("$CONTROL_PAR_VALUE".into(), crate::ksp::Value::Int(value));
        }
        drop(locked);
        record("optimistic_edit_lock_ms", at);
        drop(retained);
        let at = Instant::now();
        let mut refresh = Refresh::default();
        loop {
            saved_passes += 1;
            if rt.refresh_persistence_within(&mut saved, &mut refresh, REFRESH_BUDGET) { break }
        }
        changed_edits += usize::from(refresh.changed);
        record("persistence_refresh_ms", at);
        let at = Instant::now();
        let locked = view.lock().unwrap();
        if crate::ksp::settle_persistence(&mut saved) {
            let json = serde_json::to_string(&saved)?;
            json_bytes = json.len();
            std::hint::black_box(json);
        }
        drop(locked);
        record("persistence_serialize_lock_ms", at);
    }
    drop(record);
    let stages: BTreeMap<_, _> = times.into_iter().map(|(stage, mut values)| {
        values.sort_by(f64::total_cmp);
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        (stage, serde_json::json!({"mean":mean,"p50":values[values.len()/2],"p99":values[(values.len()*99/100).min(values.len()-1)],"max":values[values.len()-1]}))
    }).collect();
    let diagnostics = engine.script().unwrap().diagnostics();
    let instrument = Arc::new(instrument);
    let mut workers = Vec::new();
    for frames in [64, 128, 512] {
        let (report, returned) = bench_ui_worker(engine, instrument.clone(), 0, control, low, high, frames, edits, None)?;
        workers.push(report);
        engine = returned;
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "instrument":instrument.name,"control":variable,"edits":edits,"controls":controls,
        "persistent_json_bytes":json_bytes,"changed_edits":changed_edits,"array_cells":array_cells,
        "live_refresh_passes_per_edit":live_passes as f64/edits as f64,
        "persistence_refresh_passes_per_edit":saved_passes as f64/edits as f64,
        "stages_ms":stages,"diagnostics":diagnostics,
        "concurrent_workers":workers
    }))?);
    Ok(())
}

pub fn bench_host(paths: &[String], seconds: f64, notes: usize) -> anyhow::Result<()> {
    use moose::core::bus_routing::{BusActivation, BusRouting};
    use std::time::Duration;
    let frames: usize = paths
        .iter()
        .find_map(|p| p.strip_prefix("--frames="))
        .map(str::parse)
        .transpose()?
        .unwrap_or(512);
    let rate: f64 = paths
        .iter()
        .find_map(|p| p.strip_prefix("--rate="))
        .map(str::parse)
        .transpose()?
        .unwrap_or(48000.0);
    anyhow::ensure!((1..=32768).contains(&frames), "Invalid host buffer size");
    anyhow::ensure!(
        rate.is_finite() && (8000.0..=384000.0).contains(&rate),
        "Invalid sample rate"
    );
    anyhow::ensure!(
        seconds.is_finite() && seconds * rate >= frames as f64 && notes <= 128,
        "Invalid benchmark duration/note count"
    );
    for option in paths.iter().filter(|p| p.starts_with("--")) {
        anyhow::ensure!(
            matches!(option.as_str(), "--ram-only" | "--chords" | "--fifo")
                || option.starts_with("--frames=")
                || option.starts_with("--rate="),
            "Unknown benchmark option {option}"
        );
    }
    let p = Arc::new(SamplerParams::new());
    let ram_only = paths.iter().any(|p| p == "--ram-only");
    // Quantized chords, as a DAW plays a grid: every 250 ms each part (on its
    // own available MIDI route) releases its last chord and starts 3 or 4 notes at once.
    // Parts beyond the host's 64 MIDI routes share valid port-D channels.
    let chords = paths.iter().any(|p| p == "--chords");
    let fifo = paths.iter().any(|p| p == "--fifo");
    #[cfg(not(target_os = "linux"))]
    anyhow::ensure!(!fifo, "--fifo is only supported on Linux");
    let paths: Vec<_> = paths.iter().filter(|p| !p.starts_with("--")).cloned().collect();
    let paths = &paths[..];
    anyhow::ensure!(
        !paths.is_empty(),
        "Benchmark requires at least one instrument"
    );
    let mut selection = p.selection.write().unwrap();
    if ram_only {
        selection.streaming = Streaming::RamOnly;
    }
    selection.parts = paths
        .iter()
        .enumerate()
        .map(|(i, path)| Part {
            path: path.clone(),
            channel: if chords { (i % 16) as i16 } else { -1 },
            port: if chords { (i / 16).min(3) as u8 } else { 0 },
            ..Default::default()
        })
        .collect();
    drop(selection);
    let mut dsp = Dsp::default();
    let transport = TransportInfo::default();
    let mut data = vec![vec![0f32; frames]; 2 * BUSES];
    let mut outgoing = EventList::with_capacity(64);
    let (instructions, cycles) = (Counter::open(1), Counter::open(0));
    let mut process = |dsp: &mut Dsp, events: &EventList| {
        let mut channels: Vec<_> = data.iter_mut().map(|v| v.as_mut_slice()).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, frames);
        let mut routing = BusRouting::new();
        for _ in 0..BUSES {
            routing.push_output(2, BusActivation::Active);
        }
        outgoing.clear();
        let mut cx = ProcessContext::new(&transport, rate, frames, &mut outgoing)
            .with_bus_routing(routing);
        let count = |c: &Option<Counter>| c.as_ref().map_or(0, Counter::read);
        let (started, cpu) = (Instant::now(), thread_cpu());
        let before = (count(&instructions), count(&cycles));
        Sampler::process(dsp, &p, &mut buffer, events, &mut cx);
        let millions = (count(&instructions) - before.0) as f64 * 1e-6;
        let spent = (count(&cycles) - before.1) as f64 * 1e-6;
        (started.elapsed().as_secs_f64() * 1e3, (thread_cpu() - cpu) * 1e3, millions, spent)
    };
    Sampler::reset(
        &mut dsp,
        &p,
        &AudioConfig::new(rate, frames),
    );
    let none = EventList::with_capacity(0);
    let (load_start, load_cpu) = (Instant::now(), cpu_clock(2));
    Load.run(&p);
    for _ in 0..4 {
        process(&mut dsp, &none);
        Load.run(&p);
    }
    let loaded = (dsp.rack.parts.iter().take(paths.len())).filter(|e| e.bank().is_some()).count();
    anyhow::ensure!(loaded == paths.len(), "only {loaded} of {} parts loaded", paths.len());
    let part_keys: Vec<Vec<u8>> = (0..paths.len())
        .map(|i| {
            let b = dsp.rack.parts[i].bank().unwrap();
            let low = b.zones().iter().map(|z| z.low_key).min().unwrap_or(48);
            let high = b.zones().iter().map(|z| z.high_key).max().unwrap_or(72);
            // Keyswitches sit low: play the upper part of the range.
            (low.max(36)..=high.min(96)).collect()
        })
        .collect();
    let keys = part_keys[0].clone();
    let mut chord_held: Vec<Vec<u8>> = vec![Vec::new(); paths.len()];
    let beat = (rate * 0.25) as usize;
    anyhow::ensure!(part_keys.iter().all(|k| !k.is_empty()), "No playable benchmark keys in 36..96");
    let stop = Arc::new(AtomicBool::new(false));
    let loader = {
        let (p, stop) = (p.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                Load.run(&p);
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };
    // As a host's audio thread: SCHED_FIFO 85 for this thread alone, so the
    // loader, streamer and other processes cannot preempt it.
    #[cfg(target_os = "linux")]
    if fifo {
        let param = libc::sched_param { sched_priority: 85 };
        // SAFETY: sets this thread's own policy; tid 0 is the caller.
        let failed = unsafe { libc::sched_setscheduler(0, libc::SCHED_FIFO, &param) } != 0;
        anyhow::ensure!(!failed, "SCHED_FIFO refused: {}", std::io::Error::last_os_error());
    }
    let block = Duration::from_secs_f64(frames as f64 / rate);
    let blocks = (seconds * rate) as usize / frames;
    let every = (rate / notes.max(1) as f64) as usize;
    let (mut held, mut started) = (std::collections::VecDeque::new(), 0usize);
    let mut events = EventList::with_capacity(64);
    // Idle instructions per block, so playing ones divide into a cost per voice.
    let (mut idle, mut idle_cycles) = (0., 0.);
    println!(
        "resident after load: {:.0} MiB · loaded in {:.2} s, {:.2} s CPU",
        rss_mib(),
        load_start.elapsed().as_secs_f64(),
        cpu_clock(2) - load_cpu
    );
    for v in &p.shared.view.lock().unwrap().parts[..paths.len()] {
        println!("  {}", v.status);
    }
    // Idle again after playing: voices and streams have ended and must cost nothing.
    for (phase, playing) in [("idle", false), ("playing", true), ("after", false)] {
        if phase != "idle" && notes == 0 {
            break;
        }
        let process_cpu = cpu_clock(2);
        let (mut times, mut cpus) = (Vec::with_capacity(blocks), Vec::with_capacity(blocks));
        let (mut counts, mut cycle_counts) = (Vec::with_capacity(blocks), Vec::with_capacity(blocks));
        let (mut faults, mut onsets) = (Vec::with_capacity(blocks), Vec::with_capacity(blocks));
        let (mut voices, mut cpu, mut voice_blocks, mut audible_blocks) = (0, 0f32, 0usize, 0usize);
        let (start, mut pace) = (Instant::now(), crate::engine::Pace::start());
        for b in 0..blocks {
            events.clear();
            let frame = b * frames;
            let mut beats = 0;
            while playing && chords && started * beat < frame + frames {
                let at = (started * beat).saturating_sub(frame) as u32;
                for (part, keys) in part_keys.iter().enumerate() {
                    let channel = (part % 16) as u8;
                    let port = (part / 16).min(3) as u8;
                    for key in chord_held[part].drain(..) {
                        events.push(Event::on_port(at, port, EventBody::NoteOff { group: 0, channel, note: key, velocity: 0 }));
                    }
                    for n in 0..3 + (started + part) % 2 {
                        let key = keys[(started * 5 + n * 4) % keys.len()];
                        events.push(Event::on_port(at, port, EventBody::NoteOn { group: 0, channel, note: key, velocity: 90 }));
                        chord_held[part].push(key);
                    }
                }
                started += 1;
                beats += 1;
            }
            while playing && !chords && started * every < frame + frames {
    anyhow::ensure!(part_keys.iter().all(|k| !k.is_empty()), "No playable benchmark keys in 36..96");
                let note = |on: bool, key: u8| {
                    let body = if on {
                        EventBody::NoteOn { group: 0, channel: 0, note: key, velocity: 100 }
                    } else {
                        EventBody::NoteOff { group: 0, channel: 0, note: key, velocity: 0 }
                    };
                    Event::new(0, body)
                };
                if held.len() >= notes.min(keys.len())
                    && let Some(key) = held.pop_front()
                {
                    events.push(note(false, key));
                }
                let key = keys[(started * 7) % keys.len()];
                events.push(note(true, key));
                held.push_back(key);
                started += 1;
            }
            let usage_before = thread_usage();
            let (wall, cpu_ms, millions, spent) = process(&mut dsp, &events);
            let usage = thread_usage();
            faults.push(std::array::from_fn::<i64, 3, _>(|i| usage[i] - usage_before[i]));
            onsets.push(beats > 0);
            cycle_counts.push(spent);
            times.push(wall);
            cpus.push(cpu_ms);
            counts.push(millions);
            let now = p.shared.voices.load(Ordering::Relaxed);
            (voices, voice_blocks) = (voices.max(now), voice_blocks + now as usize);
            audible_blocks += dsp.rack.parts.iter().map(|e| e.audible_voices()).sum::<usize>();
            cpu = cpu.max(f32::from_bits(p.shared.cpu.swap(0, Ordering::Relaxed) as u32));
            if playing && b % (blocks / 4).max(1) == blocks / 8 {
                census(&dsp.rack.parts[0].voice_census());
            }
            pace.until(block * (b + 1) as u32);
        }
        if playing {
            let mut off = EventList::with_capacity(64);
            for key in held.drain(..) {
                off.push(Event::new(0, EventBody::NoteOff { group: 0, channel: 0, note: key, velocity: 0 }));
            }
            for (part, keys) in chord_held.iter_mut().enumerate() {
                for key in keys.drain(..) {
                    off.push(Event::new(0, EventBody::NoteOff { group: 0, channel: part as u8, note: key, velocity: 0 }));
                }
            }
            process(&mut dsp, &off);
        }
        let deadline = frames as f64 / rate * 1e3;
        let mean_voices = voice_blocks as f64 / times.len() as f64;
        let mean_audible = audible_blocks as f64 / times.len() as f64;
        let whole = (cpu_clock(2) - process_cpu) / start.elapsed().as_secs_f64();
        let missed = times.iter().filter(|&&ms| ms > deadline).count();
        println!(
            "{phase}: {} blocks · mean {mean_voices:.0} ({mean_audible:.0} audible), peak {voices} voices · peak reported CPU {:.1}% · whole process {:.1}% of a core · RSS+swap {:.0} MiB, samples {:.0} MiB · {} dropouts",
            times.len(),
            cpu * 100.,
            whole * 100.,
            rss_mib(),
            crate::engine::resident_bytes() as f64 / (1 << 20) as f64,
            p.shared.dropouts.load(Ordering::Relaxed),
        );
        let freed: u64 = p.shared.view.lock().unwrap().parts.iter().map(|v| v.freed).sum();
        println!("  smart memory: {} MiB freed", freed >> 20);
        println!("  callback deadline misses: {missed}");
        if counts.iter().any(|&c| c > 0.) {
            let mut sorted = counts.clone();
            sorted.sort_by(f64::total_cmp);
            let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
            let mean = counts.iter().sum::<f64>() / counts.len() as f64;
            let mean_cycles = cycle_counts.iter().sum::<f64>() / cycle_counts.len() as f64;
            let per_voice = if playing && mean_voices >= 1. {
                let voice_frames = mean_voices * frames as f64;
                let audible_frames = mean_audible.max(1.) * frames as f64;
                format!(
                    " · {:.1} k per voice · {:.1} instructions, {:.1} cycles per voice-frame · {:.1} cycles per audible voice-frame",
                    (mean - idle) * 1e3 / mean_voices,
                    (mean - idle) * 1e6 / voice_frames,
                    (mean_cycles - idle_cycles) * 1e6 / voice_frames,
                    (mean_cycles - idle_cycles) * 1e6 / audible_frames,
                )
            } else {
                if phase == "idle" {
                    (idle, idle_cycles) = (mean, mean_cycles);
                }
                String::new()
            };
            println!(
                "  instructions: mean {mean:.3} M/block · p50 {:.3} · p99 {:.3} · max {:.3}{per_voice}",
                at(0.5),
                at(0.99),
                at(1.0)
            );
        }
        for (clock, times) in [("wall", &times), ("thread CPU", &cpus)] {
            let mut sorted = times.clone();
            sorted.sort_by(f64::total_cmp);
            let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
            let mean = times.iter().sum::<f64>() / times.len() as f64;
            println!(
                "  {clock}: mean {mean:.3} ms · p50 {:.3} · p99 {:.3} · max {:.3} ({:.1}× mean) · {:.1}% of the {deadline:.2} ms deadline",
                at(0.5),
                at(0.99),
                at(1.0),
                at(1.0) / mean,
                mean / deadline * 100.,
            );
        }
        if onsets.iter().any(|&o| o) {
            // Blocks that start notes against the rest: the note-on cost.
            for (name, onset) in [("note-on blocks", true), ("other blocks", false)] {
                let pick = |v: &[f64]| -> Vec<f64> { v.iter().zip(&onsets).filter(|(_, o)| **o == onset).map(|(t, _)| *t).collect() };
                let (mut t, mut m) = (pick(&cpus), pick(&counts));
                let f: Vec<f64> = faults.iter().zip(&onsets).filter(|(_, o)| **o == onset).map(|(f, _)| f[0] as f64).collect();
                let switches: (i64, i64) = faults.iter().zip(&onsets).filter(|(_, o)| **o == onset).fold((0, 0), |a, (f, _)| (a.0 + f[1], a.1 + f[2]));
                let off: Vec<f64> = times.iter().zip(&cpus).zip(&onsets).filter(|(_, o)| **o == onset).map(|((w, c), _)| w - c).collect();
                t.sort_by(f64::total_cmp);
                m.sort_by(f64::total_cmp);
                let at = |v: &[f64], q: f64| v.get(((v.len().max(1) - 1) as f64 * q) as usize).copied().unwrap_or(0.);
                let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
                println!(
                    "  {name} ({}): CPU mean {:.3} ms · p99 {:.3} · max {:.3} · instructions mean {:.3} M · p99 {:.3} · max {:.3} · minor faults mean {:.1} · max {:.0} · off-CPU (wall − CPU) mean {:.3} ms · max {:.3} · context switches {} voluntary, {} involuntary",
                    t.len(),
                    mean(&t),
                    at(&t, 0.99),
                    at(&t, 1.),
                    mean(&m),
                    at(&m, 0.99),
                    at(&m, 1.),
                    mean(&f),
                    f.iter().copied().fold(0., f64::max),
                    mean(&off),
                    off.iter().copied().fold(0., f64::max),
                    switches.0,
                    switches.1,
                );
            }
        }
        let times = &cpus;
        let mut sorted = times.clone();
        sorted.sort_by(f64::total_cmp);
        let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
        // Where the slow blocks fall: evenly spaced ones are a timer.
        let slow: Vec<usize> = (0..times.len()).filter(|&i| times[i] > 2. * at(0.5).max(0.01)).collect();
        let gaps: Vec<usize> = slow.windows(2).map(|w| w[1] - w[0]).collect();
        println!(
            "  {} blocks over 2× median; first at {:?}; gaps {:?}",
            slow.len(),
            &slow[..slow.len().min(12)],
            &gaps[..gaps.len().min(24)]
        );
    }
    stop.store(true, Ordering::Relaxed);
    let _ = loader.join();
    for (part, engine) in dsp.rack.parts.iter().enumerate() {
        if let Some(rt) = engine.script() {
            for diagnostic in rt.diagnostics() {
                eprintln!("part {} script: {diagnostic}", part + 1);
            }
        }
    }
    Ok(())
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[cfg(feature = "uvi")]
    #[test]
    fn native_latency_admission_survives_host_resets_without_live_endpoints() {
        let params = SamplerParams::new();
        let mut dsp = Dsp::default();
        let config = AudioConfig::new(48000., 256);
        let latency = crate::uvi::bridge::Bridge::buffering_latency(256, UVI_LEAD_PACKETS).unwrap();
        params.shared.uvi_latency_admission.store((256u64 << 32) | u64::from(latency), Ordering::Release);
        let mut left = [0.; 256];
        let mut right = [0.; 256];
        let mut channels = [left.as_mut_slice(), right.as_mut_slice()];
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, 256);
        let events = EventList::with_capacity(0);
        let mut output = EventList::with_capacity(16);
        let transport = TransportInfo::default();
        let mut cx = ProcessContext::new(&transport, 48000., 256, &mut output);
        // Model a host restarting whenever the report changes: epochs advance,
        // no replacement endpoint is present yet, and reported latency holds.
        for _ in 0..3 {
            Sampler::reset(&mut dsp, &params, &config);
            assert_eq!(dsp.uvi_latency, 0, "mixer has no current endpoint");
            assert_eq!(Sampler::latency(&dsp), latency);
            assert_eq!(allocations(|| { Sampler::process(&mut dsp, &params, &mut buffer, &events, &mut cx); }), 0);
            assert_eq!(Sampler::latency(&dsp), latency);
        }
        let mut delays = uvi_delay::Prepared::new(dsp.uvi.len(), latency as usize).unwrap();
        assert!(delays.set_all(latency));
        dsp.uvi_delays = Some(Box::new(delays));
        Sampler::reset(&mut dsp, &params, &config);
        assert_eq!(dsp.uvi_latency, latency, "matching adopted storage preserves physical compensation during replacement");
        assert_eq!(allocations(|| { Sampler::process(&mut dsp, &params, &mut buffer, &events, &mut cx); }), 0);
        assert_eq!(dsp.uvi_latency, latency);
        Sampler::reset(&mut dsp, &params, &AudioConfig::new(48000., 512));
        assert_eq!(dsp.uvi_latency, 0, "old delay storage cannot supply a changed configuration");
        assert_eq!(Sampler::latency(&dsp), 0, "changed maximum requires matching delay admission");
        Sampler::reset(&mut dsp, &params, &config);
        assert_eq!(Sampler::latency(&dsp), latency);
        // The ordinary loader observes that no native source remains selected.
        uvi_load::service(&params);
        Sampler::process(&mut dsp, &params, &mut buffer, &events, &mut cx);
        assert_eq!(Sampler::latency(&dsp), 0);
        assert_eq!(dsp.uvi_latency, 0);
        assert_eq!(params.shared.uvi_latency_admission.load(Ordering::Acquire), 0);
    }

    #[test]
    fn opaque_native_state_survives_json_and_host_codec_without_backend_types() {
        use moose::core::custom_state::State;
        let part = Part { uvi_state:vec![0,1,255,128].into(), ..Default::default() };
        assert!(Part::deserialize(&State::serialize(&part)) == Some(part.clone()));
        let clone=part.clone();
        assert!(Arc::ptr_eq(&part.uvi_state.0,&clone.uvi_state.0), "UI selection clones share opaque bytes");
        assert_eq!(format!("{:?}",part.uvi_state),"NativeState { bytes: 4 }");
        let mut old_bytes=Vec::new(); vec![0u8,1,255,128].write_field(&mut old_bytes);
        let mut new_bytes=Vec::new(); part.uvi_state.write_field(&mut new_bytes);
        assert_eq!(new_bytes,old_bytes,"native byte wrapper retains the Vec wire codec");
        let json=serde_json::to_string(&part).unwrap();
        assert_eq!(serde_json::to_string(&part.uvi_state).unwrap(),"[0,1,255,128]");
        assert!(serde_json::from_str::<Part>(&json).unwrap() == part);
        assert!(serde_json::from_str::<Part>("{}").unwrap().uvi_state.is_empty());
    }

    #[test]
    fn performance_pages_keep_slot_callbacks_and_late_buffers_separate() {
        let first = "on init\nmake_perfview\nset_script_title(\"Performance\")\nset_ui_height_px(200)\nset_skin_offset(0)\ndeclare ui_knob $amount(0,100,1)\ndeclare ui_label $caption(1,1)\nset_text($caption,\"Layer one\")\nend on\non ui_control($amount)\nset_text($caption,\"Performance edited\")\nend on";
        let second = "on init\nmake_perfview\nset_script_title(\"FX Rack\")\nset_ui_height_px(160)\nset_skin_offset(268)\ndeclare ui_knob $amount(0,100,1)\ndeclare ui_label $caption(1,1)\nset_text($caption,\"Effect\")\nend on\non ui_control($amount)\nset_text($caption,\"FX edited\")\nend on";
        let mut engine = crate::ksp::LogEngine::new(Vec::new(),48000.);
        let (mut rt, errors) = Runtime::with_scripts(&[first,second], &mut engine,8,Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        let parsed = script_interface(Some(&rt));
        assert_eq!(parsed.slot,0,"host opens the first authored page");
        let p = SamplerParams::new();
        {
            let mut view = p.shared.view.lock().unwrap();
            let v = &mut view.parts[0];
            v.interface = parsed.interface;
            v.script_pages = script_pages(Some(&rt));
            v.script_epoch = 1;
            v.script_slot = 0;
            v.live = Some(first_script_live(&rt));
            assert_eq!(v.script_pages.views.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(),["Performance","FX Rack"]);
        }
        p.shared.publish_live(true);
        let (_, epoch, mut old) = p.shared.live_requests.pop().unwrap();
        assert_eq!(old.slot,0);
        assert!(p.shared.select_script_page(0,epoch,1));
        p.shared.edit_control(0,0,67);
        let edit = p.shared.edits.pop().unwrap();
        assert_eq!(edit.slot,1,"the same control index belongs to the selected script slot");
        assert_eq!(allocations(|| {
            rt.ui_control(&mut engine,edit.slot,edit.control,edit.value);
            rt.refresh_live(&mut old);
        }),0,"callbacks and prepared page refresh remain allocation-free");
        p.shared.lives.push((0,epoch,old)).ok().unwrap();
        p.shared.publish_live(true);
        let (_, _, mut selected) = p.shared.live_requests.pop().unwrap();
        assert_eq!(selected.slot,1,"late old-page completions cannot replace the chosen page");
        assert!(!selected.interface_current,"a selected cached page must read current callback state");
        assert_eq!(allocations(|| { rt.refresh_live(&mut selected); }),0);
        p.shared.lives.push((0,epoch,selected)).ok().unwrap();
        p.shared.publish_live(true);
        {
            let view = p.shared.view.lock().unwrap();
            let v = &view.parts[0];
            let interface = v.interface.as_ref().unwrap();
            assert_eq!(v.script_slot,1);
            assert_eq!((interface.height,interface.skin_offset),(160,268));
            assert_eq!(interface.controls[1].properties["$CONTROL_PAR_TEXT"],crate::ksp::Value::Text("FX edited".into()));
            assert_eq!(v.control_value(0),Some(67.));
        }
        assert!(p.shared.select_script_page(0,epoch,0));
        assert_eq!(rt.interface(0).controls[0].properties["$CONTROL_PAR_VALUE"],crate::ksp::Value::Int(0));
        p.shared.view.lock().unwrap().parts[0].script_epoch = 2;
        assert!(!p.shared.select_script_page(0,epoch,1),"buttons retained from a replaced instrument cannot select its successor's pages");
    }


    #[test]
    fn snapshot_requests_validate_before_mutating_and_preserve_older_host_state() {
        use moose::core::custom_state::State;
        let part = Part { path: "/unavailable-kontakto/base.nki".into(), program: 0,
            script_state: r#"[{"retained":1}]"#.into(), gain: -4., channel: 3,
            snapshot: "/unavailable-kontakto/old.nksn".into(), ..Default::default() };
        let bytes = State::serialize(&part);
        assert!(Part::deserialize(&bytes) == Some(part.clone()));
        let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let mut frames = Vec::new();
        let mut at = 8;
        for _ in 0..count {
            let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            frames.push(bytes[at..at + 8 + len].to_vec());
            at += 8 + len;
        }
        // Fields before snapshot retain their original positional order.
        // Later appended fields also default when restoring that older state.
        let mut snapshot_field = Vec::new();
        part.snapshot.write_field(&mut snapshot_field);
        let old_count = frames.iter().position(|frame| frame[8..] == snapshot_field).unwrap();
        frames.truncate(old_count);
        let mut old_keyed = bytes[..8].to_vec();
        old_keyed[4..8].copy_from_slice(&(old_count as u32).to_le_bytes());
        let mut legacy = (old_count as u32).to_le_bytes().to_vec();
        for frame in frames { old_keyed.extend(&frame); legacy.extend(&frame[4..]); }
        let mut old = part.clone(); old.snapshot.clear();
        assert!(Part::deserialize(&old_keyed) == Some(old.clone()));
        assert!(Part::deserialize(&legacy) == Some(old));
        assert!(serde_json::from_str::<Part>("{}").unwrap().snapshot.is_empty());

        let p = SamplerParams::new();
        p.selection.write().unwrap().parts = vec![part.clone()];
        let generation = p.shared.part(0).unwrap().generation.load(Ordering::Acquire);
        assert!(p.shared.queue_snapshot(0, &part, "/unavailable-kontakto/missing.nksn".into()));
        assert_eq!(p.shared.part(0).unwrap().generation.load(Ordering::Acquire), generation, "an unvalidated request cannot invalidate the active bank's services");
        assert!(prepare_snapshot(&p).is_none());
        assert!(p.selection.read().unwrap().parts[0] == part);
        let report = p.shared.view.lock().unwrap().parts[0].load_report.clone().unwrap();
        assert_eq!(report["details"]["operation"], "snapshot_validation");
        assert_eq!(report["status"], "failed");
        assert!(report["failure"].is_string());
        crate::diagnostics::flush(std::time::Duration::from_secs(5)).unwrap();
        assert!(crate::diagnostics::snapshot().events.iter().any(|event|
            event.load_id.as_deref() == report["load_id"].as_str() && event.event == "load_finished"
                && event.details["status"] == "failed"), "failed validation is retained in the diagnostic journal");
        assert!(p.shared.view.lock().unwrap().parts[0].status.contains("Snapshot was not loaded"));
        assert!(p.shared.queue_snapshot(0, &part, "/unavailable-kontakto/stale.nksn".into()));
        p.selection.write().unwrap().parts[0].snapshot = "replacement.nksn".into();
        p.shared.view.lock().unwrap().parts[0].status = "replacement".into();
        assert!(prepare_snapshot(&p).is_none());
        assert_eq!(p.shared.view.lock().unwrap().parts[0].status, "replacement");
        crate::diagnostics::flush(std::time::Duration::from_secs(5)).unwrap();
        assert!(crate::diagnostics::snapshot().events.iter().any(|event|
            event.event == "load_finished" && event.details["status"] == "canceled"
                && event.details["details"]["snapshot"] == "/unavailable-kontakto/stale.nksn"));
        let multi = Part { path: "ensemble.nkm".into(), ..Default::default() };
        assert!(!p.shared.queue_snapshot(0, &multi, "preset.nksn".into()));
        let program = Part { program: 1, ..part };
        assert!(!p.shared.queue_snapshot(0, &program, "preset.nksn".into()));
    }

    #[test]
    fn script_fault_excerpts_preserve_malformed_source_and_runtime_arguments_off_audio() {
        let _diagnostics = crate::diagnostics::acquire();
        let instrument = Arc::new(Instrument {
            path: "/private/excerpt-owner/Example.nki".into(),
            scripts: vec![
                "on init\nmessage(\"unterminated)\nend on".into(),
                "on init\ndeclare ui_knob $bad(0,256,1)\nend on\non ui_control($bad)\nset_key_color($bad,$KEY_COLOR_RED)\nend on".into(),
            ], ..Default::default()
        });
        let (mut rt, errors) = crate::engine::load_scripts(&instrument, Vec::new(), 48_000.);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Unterminated KSP string at line 2"), "{errors:?}");
        let mut trace = crate::diagnostics::LoadTrace::new(&instrument.path, 0, Some(0));
        trace.script_issue("initialization_failed", &errors[0], &instrument.scripts);
        let initial = trace.finish("partial");
        let parse = &initial["issues"][0];
        assert_eq!((parse["script_slot"].as_u64(), parse["line"].as_u64()), (Some(0), Some(2)));
        assert!(crate::diagnostics::excerpt_text(parse).unwrap().contains(">      2 | message(\"unterminated)"));

        let rt = rt.as_mut().unwrap();
        let mut live = rt.live();
        let mut engine = crate::ksp::LogEngine::default();
        assert_eq!(allocations(|| {
            rt.ui_control(&mut engine, 1, 0, 128);
            rt.refresh_diagnostics(&mut live);
        }), 0, "only the worker materializes source excerpts");
        assert_eq!(live.faults.len(), 1);
        let shared = Shared::default();
        {
            let mut view = shared.view.lock().unwrap();
            let part = &mut view.parts[0];
            part.instrument = Some(instrument);
            part.script_epoch = 9;
            part.diagnostics_dirty = true;
            part.load_report = Some(initial.clone());
            part.live_diagnostics = Some(Arc::new(LiveDiagnostics {
                epoch: 9, faults: live.faults.clone(), fault_occurrences_omitted: 0, notes: Vec::new(),
            }));
        }
        shared.drain_live_diagnostics();
        let report = shared.view.lock().unwrap().parts[0].load_report.clone().unwrap();
        let fault = &report["runtime"]["faults"][0];
        assert_eq!(fault["context"]["MidiNote"]["value"], 128);
        assert_eq!(fault["context"]["MidiNote"]["builtin"], "set_key_color");
        assert_eq!(fault["source_excerpt"]["script_slot"], 2, "runtime slot 2 must select source index 1");
        assert!(crate::diagnostics::excerpt_text(fault).unwrap().contains(">      5 | set_key_color($bad,$KEY_COLOR_RED)"));
        assert!(crate::diagnostics::snapshot().events.iter().any(|event|
            event.load_id.as_deref() == initial["load_id"].as_str() && event.event == "runtime_issue"
                && event.script_slot == Some(1)
                && event.details["context"]["MidiNote"]["value"] == 128
                && crate::diagnostics::excerpt_text(&event.details).is_some()), "the journal event retains source and argument context");
        let mut safe = serde_json::json!({"path":"/private/excerpt-owner/Example.nki", "report":report.as_ref(), "script_source":"full private payload", "access_key":"not-code"});
        crate::diagnostics::clean(&mut safe, true);
        assert!(!safe.to_string().contains("/private/excerpt-owner") && !safe.to_string().contains("full private payload") && !safe.to_string().contains("not-code"));
        assert!(crate::diagnostics::excerpt_text(&safe["report"]["runtime"]["faults"][0]).is_some(), "authorized bounded excerpts survive copy/export sanitization");
        let long = format!("{}bad()\nend on", "é".repeat(2000));
        let excerpt = crate::diagnostics::script_excerpt(&long, 1, 1, Some(2001)).unwrap();
        assert_eq!(excerpt["truncated"], true);
        assert!(excerpt["text"].as_str().unwrap().contains("bad()") && excerpt["text"].as_str().unwrap().contains('^'));
        assert!(serde_json::to_vec(&excerpt).unwrap().len() < 4096);
    }

    #[test]
    fn live_diagnostics_preserve_failed_load_status_and_failure_cause() {
        let shared = Shared::default();
        for initial in ["failed", "canceled", "loaded", "partial"] {
            {
                let mut view = shared.view.lock().unwrap();
                let part = &mut view.parts[0];
                part.script_epoch = 1;
                part.diagnostics_dirty = true;
                part.live_diagnostics = Some(Arc::new(LiveDiagnostics {
                    epoch: 1, faults: Vec::new(), fault_occurrences_omitted: 17, notes: vec!["Existing runtime feature remains unsupported"],
                }));
                part.load_report = Some(Arc::new(serde_json::json!({"status": initial, "failure": "Foreign base rejected"})));
            }
            shared.drain_live_diagnostics();
            let view = shared.view.lock().unwrap();
            let report = view.parts[0].load_report.as_ref().unwrap();
            assert_eq!(report["status"], if initial == "loaded" { "partial" } else { initial });
            assert_eq!(report["failure"], "Foreign base rejected");
            assert_eq!(report["runtime"]["notes"][0], "Existing runtime feature remains unsupported");
            assert_eq!(report["runtime"]["fault_occurrences_omitted"], 17);
            assert!(view.parts[0].runtime_status.contains("omitted 17 executions"));
        }
    }

    /// Opt-in real worker proof; only paths are supplied by the local owner.
    #[test]
    #[ignore = "requires a local NKI, three matching snapshots and one foreign snapshot"]
    fn factory_snapshots_load_through_the_production_worker() {
        use moose::core::bus_routing::{BusActivation, BusRouting};
        let base = std::env::var("KONTRA_SNAPSHOT_BASE").expect("KONTRA_SNAPSHOT_BASE");
        let snapshots: Vec<_> = std::env::split_paths(&std::env::var_os("KONTRA_SNAPSHOTS").expect("KONTRA_SNAPSHOTS")).collect();
        assert_eq!(snapshots.len(), 3);
        let foreign = std::env::var("KONTRA_FOREIGN_SNAPSHOT").expect("KONTRA_FOREIGN_SNAPSHOT");
        let p = SamplerParams::new();
        {
            let mut selection = p.selection.write().unwrap();
            selection.parts = vec![Part { path: base.clone(), gain: -2., channel: 0, ..Default::default() }];
            selection.order = vec![0];
        }
        let mut dsp = Dsp::default();
        let transport = TransportInfo::default();
        let mut midi_out = EventList::with_capacity(4);
        let mut routing = BusRouting::new(); routing.push_output(2, BusActivation::Active);
        let mut cx = ProcessContext::new(&transport, 48000., 128, &mut midi_out).with_bus_routing(routing);
        let mut out = vec![vec![0f32; 128]; 2];
        let mut refs: Vec<_> = out.iter_mut().map(|o| o.as_mut_slice()).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut refs, 128);
        let none = EventList::with_capacity(0);
        let mut epoch = 0;
        for snapshot in snapshots {
            let before = p.selection.read().unwrap().parts[0].clone();
            let path = snapshot.to_string_lossy().into_owned();
            assert!(p.shared.queue_snapshot(0, &before, path.clone()));
            assert!(p.selection.read().unwrap().parts[0] == before, "queued selection does not modify active state");
            Load.run(&p);
            let part = p.selection.read().unwrap().parts[0].clone();
            assert_eq!((&part.path, &part.snapshot, part.channel, part.gain), (&base, &path, 0, -2.));
            assert_eq!(p.shared.view.lock().unwrap().parts[0].attempted, Some(part.source()));
            assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &none, &mut cx); }), 0,
                "installing the prepared snapshot allocated or freed on audio");
            assert!(dsp.script_epoch[0] > epoch); epoch = dsp.script_epoch[0];
            assert!(dsp.rack.parts[0].bank().is_some());
            let instrument = p.shared.view.lock().unwrap().parts[0].instrument.clone().unwrap();
            assert_eq!(instrument.name, snapshot.file_stem().unwrap().to_string_lossy());
            let (expected, _, errors) = scripts(&instrument, "", &[], &[], 48000.);
            assert!(errors.is_empty(), "{errors:?}");
            assert!(dsp.rack.parts[0].script().unwrap().persistence() == expected.unwrap().persistence(), "snapshot persistence is the worker-installed state");
            assert_eq!(allocations(|| {
                for _ in 0..32 { Sampler::process(&mut dsp, &p, &mut buffer, &none, &mut cx); }
            }), 0, "loaded snapshot processing allocated or freed on audio");
            let multi = SavedMulti::of("snapshot", &p.selection.read().unwrap());
            let restored: SavedMulti = serde_json::from_str(&serde_json::to_string(&multi).unwrap()).unwrap();
            assert_eq!(restored.parts[0].snapshot, path);
            println!("worker snapshot {}: epoch {}, controls {}, groups {}, audio heap 0", instrument.name, epoch,
                p.shared.view.lock().unwrap().parts[0].interface.as_ref().map_or(0, |i| i.controls.len()), instrument.groups.len());
        }
        // Commit the active runtime's ordinary persistence before testing a
        // rejected selection. Load also services normal audio handoffs, whose
        // legitimate publication must not be confused with preset replacement.
        let mut settled = false;
        for _ in 0..32 {
            assert_eq!(allocations(|| {
                for _ in 0..16 { Sampler::process(&mut dsp, &p, &mut buffer, &none, &mut cx); }
            }), 0);
            Load.run(&p);
            let expected = serde_json::to_string(&dsp.rack.parts[0].script().unwrap().persistence()).unwrap();
            if p.selection.read().unwrap().parts[0].script_state == expected {
                settled = true;
                break;
            }
        }
        assert!(settled, "active snapshot persistence did not settle within 512 blocks");
        let before = p.selection.read().unwrap().parts[0].clone();
        let bank = dsp.rack.parts[0].bank().unwrap() as *const Bank;
        let generation = p.shared.part(0).unwrap().generation.load(Ordering::Acquire);
        let epoch = dsp.script_epoch[0];
        assert!(p.shared.queue_snapshot(0, &before, foreign));
        Load.run(&p);
        let after = p.selection.read().unwrap().parts[0].clone();
        let old = serde_json::to_value(&before).unwrap();
        let new = serde_json::to_value(&after).unwrap();
        let changed: Vec<_> = old.as_object().unwrap().iter()
            .filter_map(|(name, value)| (new.get(name) != Some(value)).then_some(name)).collect();
        assert!(after == before, "foreign snapshot changed fields {changed:?}; script bytes {} -> {}, IR slots {} -> {}; source retained {}; generation {} -> {}, epoch {} -> {}, bank retained {}",
            before.script_state.len(), after.script_state.len(), before.ir_settings.len(), after.ir_settings.len(),
            before.source() == after.source(), generation, p.shared.part(0).unwrap().generation.load(Ordering::Acquire), epoch, dsp.script_epoch[0],
            dsp.rack.parts[0].bank().unwrap() as *const Bank == bank);
        assert!(p.shared.view.lock().unwrap().parts[0].status.contains("Snapshot requires base instrument"));
        let report = p.shared.view.lock().unwrap().parts[0].load_report.clone().unwrap();
        assert_eq!(report["details"]["operation"], "snapshot_validation");
        assert_eq!(report["status"], "failed");
        crate::diagnostics::flush(std::time::Duration::from_secs(5)).unwrap();
        assert!(crate::diagnostics::snapshot().events.iter().any(|event|
            event.load_id.as_deref() == report["load_id"].as_str() && event.event == "load_finished"
                && event.details["status"] == "failed"));
        Sampler::process(&mut dsp, &p, &mut buffer, &none, &mut cx);
        assert_eq!(dsp.rack.parts[0].bank().unwrap() as *const Bank, bank);
        assert_eq!(p.shared.part(0).unwrap().generation.load(Ordering::Acquire), generation);
        assert_eq!(dsp.script_epoch[0], epoch);
        println!("foreign snapshot rejected without replacing active state, bank, generation or script epoch");
    }

    #[test]
    fn persistence_formatting_releases_the_editor_lock_and_rejects_replaced_epochs() {
        use std::sync::Barrier;
        let source = "on init\ndeclare %table[16384]\n%table[16383] := 721\nmake_persistent(%table)\nend on";
        let (rt, errors) = Runtime::with_scripts(&[source], &mut crate::ksp::LogEngine::default(), 0, Vec::new());
        assert!(errors.iter().all(Option::is_none));
        let expected = serde_json::to_string(&rt.persistence()).unwrap();
        for replace in [false, true] {
            let p = Arc::new(SamplerParams::new());
            let part = Part { script_state: "before".into(), ..Default::default() };
            p.selection.write().unwrap().parts = vec![part.clone()];
            let streaming = {
                let selection = p.selection.read().unwrap();
                part.streaming(selection.streaming)
            };
            {
                let mut view = p.shared.view.lock().unwrap();
                for v in &mut view.parts {
                    v.attempted = Some(part.source());
                    v.streaming = streaming;
                }
                let v = &mut view.parts[0];
                v.script_epoch = 1;
                v.script_state = part.script_state;
                v.snapshot_lent = Some(Instant::now());
            }
            let snapshot = Box::new(PersistenceSnapshot {
                script: rt.persistence(), ir: Vec::new(), native: rt.native_state.snapshot(),
            });
            let address = (&*snapshot as *const PersistenceSnapshot) as usize;
            p.shared.snapshots.push((0, 1, snapshot, true)).ok().unwrap();
            let gate = Arc::new(Barrier::new(2));
            *p.shared.snapshot_gate.lock().unwrap() = Some(gate.clone());
            let worker = {
                let p = p.clone();
                std::thread::spawn(move || Load.run(&p))
            };
            gate.wait();
            // The worker is paused at the persistence formatting boundary.
            // A frame must read the view without waiting for its large arrays.
            let unlocked = p.shared.view.try_lock().is_ok();
            if unlocked && replace {
                let mut view = p.shared.view.lock().unwrap();
                view.parts[0].script_epoch = 2;
                view.parts[0].script_state = "replacement".into();
                drop(view);
                p.selection.write().unwrap().parts[0].script_state = "replacement".into();
            }
            gate.wait();
            worker.join().unwrap();
            assert!(unlocked, "persistence formatting must not hold the editor view lock");
            let view = p.shared.view.lock().unwrap();
            let v = &view.parts[0];
            if replace {
                assert_eq!(v.script_epoch, 2);
                assert_eq!(v.script_state, "replacement");
                assert!(v.snapshot.is_none(), "an old epoch's buffer cannot enter its replacement");
                assert_eq!(p.selection.read().unwrap().parts[0].script_state, "replacement");
            } else {
                assert_eq!(v.script_state, expected);
                assert_eq!(p.selection.read().unwrap().parts[0].script_state, expected);
                assert_eq!((&**v.snapshot.as_ref().unwrap() as *const PersistenceSnapshot) as usize, address,
                    "the prepared audio snapshot buffer is recycled, preserving host JSON");
            }
        }
    }

    #[test]
    fn publisher_reuses_unique_rows_and_preserves_retained_snapshots_and_menus() {
        use crate::ksp::Value;
        let script = r#"on init
make_perfview
declare ui_switch $page
declare ui_label $caption(1,1)
set_text($caption, "start")
declare ui_menu $choices
add_menu_item($choices, "first", 0)
add_menu_item($choices, "second", 1)
set_menu_item_visibility(get_ui_id($choices), 1, 0)
declare ui_text_edit @fixed
@fixed := "fixed"
end on
on ui_control($page)
set_text($caption, "page")
set_control_par(get_ui_id($caption), $CONTROL_PAR_FONT_TYPE, 23)
set_skin_offset($page * 10)
set_menu_item_visibility(get_ui_id($choices), 1, $page)
end on"#;
        let mut engine = crate::ksp::LogEngine::new(Vec::new(), 48_000.);
        let (mut rt, errors) = Runtime::with_scripts(&[script], &mut engine, 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        let p = SamplerParams::new();
        p.shared.view.lock().unwrap().parts[0].script_epoch = 1;
        p.shared.lives.push((0, 1, Box::new(rt.live()))).ok().unwrap();
        p.shared.publish_live(true);
        let (allocation, fixed_text, weak) = {
            let view = p.shared.view.lock().unwrap();
            let source = view.parts[0].interface.as_ref().unwrap();
            let Value::Text(text) = &source.controls[3].properties["$CONTROL_PAR_VALUE"] else { panic!("fixed text") };
            (source.controls.as_ptr() as usize, text.as_ptr() as usize, Arc::downgrade(source))
        };
        let (slot, epoch, mut live) = p.shared.live_requests.pop().unwrap();
        assert_eq!(allocations(|| {
            rt.ui_control(&mut engine, 0, 0, 1);
            rt.refresh_live(&mut live);
        }), 0, "row publication adds no audio allocation or freeing");
        let expected = live.interface.clone().unwrap();
        p.shared.lives.push((slot, epoch, live)).ok().unwrap();
        p.shared.publish_live(true);
        let retained = {
            let view = p.shared.view.lock().unwrap();
            let source = view.parts[0].interface.as_ref().unwrap();
            assert_eq!(source.as_ref(), &expected, "all callback metadata, fonts, menus and wallpaper match the completed Live");
            assert_eq!(source.controls.as_ptr() as usize, allocation, "unique source donates its controls allocation");
            let Value::Text(text) = &source.controls[3].properties["$CONTROL_PAR_VALUE"] else { panic!("fixed text") };
            assert_eq!(text.as_ptr() as usize, fixed_text, "an unchanged property map is not copied");
            assert!(view.parts[0].publication_rows.unwrap().2);
            source.clone()
        };
        assert!(weak.upgrade().is_none(), "a Weak panel cache does not prevent unique row reuse");
        let (slot, epoch, mut live) = p.shared.live_requests.pop().unwrap();
        rt.ui_control(&mut engine, 0, 0, 0);
        rt.refresh_live(&mut live);
        let next = live.interface.clone().unwrap();
        p.shared.lives.push((slot, epoch, live)).ok().unwrap();
        p.shared.publish_live(true);
        {
            let view = p.shared.view.lock().unwrap();
            let source = view.parts[0].interface.as_ref().unwrap();
            assert_eq!(source.as_ref(), &next);
            assert_eq!(retained.as_ref(), &expected, "retained readers keep the previous callback snapshot");
            assert_ne!(source.controls.as_ptr(), retained.controls.as_ptr(), "retained readers get a full immutable fallback");
            assert!(!view.parts[0].publication_rows.unwrap().2);
        }
        // Menu rows can change from another variable while this control's own
        // metadata and value stamps stay equal. They must still be published.
        let mut live = rt.live();
        let previous = live.interface.clone().unwrap();
        let versions = live.control_versions().collect::<Vec<_>>();
        live.interface.as_mut().unwrap().controls[2].menu[0].0 = "renamed".into();
        let update = prepare_interface(&live, Some(&previous), &versions);
        let rows = update.rows.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 2);
        assert_eq!(rows[0].1.menu[0].0, "renamed", "menu visibility/text is independent of per-control stamps");
    }

    #[test]
    fn editor_publishes_and_recycles_live_views_while_another_part_loads() {
        use crate::ksp::Value;
        use std::sync::Barrier;
        let source = "on init\nmake_perfview\ndeclare ui_slider $s(0,100)\ndeclare ui_label $l(1,1)\nset_text($l, \"start\")\nend on\non ui_control($s)\nset_text($l, \"value \" & $s)\nend on";
        let (rt, errors) = Runtime::with_scripts(&[source], &mut crate::ksp::LogEngine::default(), 0, Vec::new());
        assert!(errors.iter().all(Option::is_none));
        let p = Arc::new(SamplerParams::new());
        p.selection.write().unwrap().parts = vec![
            Part::default(),
            Part { path: "/missing-kontra-live-publication/second.nki".into(), ..Default::default() },
        ];
        let live = Box::new(rt.live());
        let address = (&*live as *const Live) as usize;
        let streaming = {
            let selection = p.selection.read().unwrap();
            selection.parts[0].streaming(selection.streaming)
        };
        {
            let mut view = p.shared.view.lock().unwrap();
            view.script_epoch = 1;
            let v = &mut view.parts[0];
            v.attempted = Some((String::new(), 0, String::new()));
            v.streaming = streaming;
            v.script_epoch = 1;
            v.interface = live.interface.clone().map(Arc::new);
            v.load_report = Some(Arc::new(serde_json::json!({"status":"loaded","runtime":null})));
            v.live = Some(live);
        }
        let mut dsp = Dsp::default();
        dsp.script_epoch[0] = 1;
        dsp.until_poll = usize::MAX;
        dsp.rack.parts[0].set_script(Some(Box::new(rt)));
        p.shared.publish_live(true);
        let bank_gate = Arc::new(Barrier::new(2));
        *p.shared.load_gate.lock().unwrap() = Some((1, bank_gate.clone()));
        let loader = {
            let p = p.clone();
            std::thread::spawn(move || Load.run(&p))
        };
        bank_gate.wait();
        assert!(p.shared.view.lock().unwrap().parts[1].loading);

        let label = |p: &SamplerParams| p.shared.view.lock().unwrap().parts[0].interface.as_ref().unwrap()
            .controls[1].properties["$CONTROL_PAR_TEXT"].clone();
        let transport = TransportInfo::default();
        let mut outgoing = EventList::with_capacity(0);
        let none = EventList::with_capacity(0);
        let mut cx = ProcessContext::new(&transport, 48000., 64, &mut outgoing);
        let mut left = [0f32;64];
        let mut right = [0f32;64];
        let mut outputs = [&mut left[..], &mut right[..]];
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut outputs, 64);
        for value in [42, 84] {
            let retained = p.shared.view.lock().unwrap().parts[0].interface.clone().unwrap();
            let source_value = retained.controls[0].properties["$CONTROL_PAR_VALUE"].clone();
            p.shared.edit_control(0, 0, value);
            {
                let view = p.shared.view.lock().unwrap();
                assert!(Arc::ptr_eq(&retained, view.parts[0].interface.as_ref().unwrap()), "an edit copies no interface under the view lock");
                assert_eq!(view.parts[0].control_value(0), Some(f64::from(value)), "the edit is visible immediately");
                assert_eq!(retained.controls[0].properties["$CONTROL_PAR_VALUE"], source_value);
            }
            assert!(crate::ui::watch_live_change(&p, || {
                assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &none, &mut cx); }), 0);
                let (slot, epoch, mut live) = p.shared.lives.pop().expect("audio completes a live view");
                assert_eq!((&*live as *const Live) as usize, address, "the same prepared buffer is recycled");
                live.faults.push(LiveFault { slot: 0, line: 1, message: "synthetic runtime fault", context: None, last_action: None, count: value as u32 });
                p.shared.lives.push((slot, epoch, live)).ok().unwrap();
            }), "completed script changes wake an otherwise idle editor before build");
            assert_eq!(label(&p), Value::Text(format!("value {value}")), "callback-derived labels publish without Load");
            assert!(p.shared.view.lock().unwrap().parts[0].edited.is_empty());
            assert_eq!(p.shared.live_requests.len(), 1, "re-lending never waits for diagnostics or loading");
            assert_ne!(retained.controls[1].properties["$CONTROL_PAR_TEXT"], Value::Text(format!("value {value}")));
            assert!(p.shared.view.lock().unwrap().watched_at.is_some_and(|at| at.elapsed() < LIVE_WATCH));
            assert!(p.shared.view.lock().unwrap().parts[0].load_report.as_ref().unwrap()["runtime"].is_null(), "UI never formats JSON");
            assert!(!loader.is_finished(), "another part's loader remains blocked");
        }
        let retained = p.shared.view.lock().unwrap().parts[0].interface.clone().unwrap();
        assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &none, &mut cx); }), 0);
        let (slot, epoch, mut live) = p.shared.lives.pop().unwrap();
        live.faults.push(LiveFault { slot: 0, line: 1, message: "synthetic runtime fault", context: None, last_action: None, count: 84 });
        p.shared.lives.push((slot, epoch, live)).ok().unwrap();
        p.shared.publish_live(true);
        assert!(Arc::ptr_eq(&retained, p.shared.view.lock().unwrap().parts[0].interface.as_ref().unwrap()),
            "unchanged revisions retain the published interface");
        // Equal revisions preserve an optimistic value until its grace
        // period ends, then restore a value the script refused exactly once.
        p.shared.edit_control(0, 0, 99);
        p.shared.edits.pop().unwrap(); // Simulate an edit rejected by the script.
        let optimistic = p.shared.view.lock().unwrap().parts[0].interface.clone().unwrap();
        assert_eq!(p.shared.view.lock().unwrap().parts[0].control_value(0), Some(99.));
        assert_eq!(optimistic.controls[0].properties["$CONTROL_PAR_VALUE"], Value::Int(84));
        let (slot, _, live) = p.shared.live_requests.pop().unwrap();
        p.shared.lives.push((slot, 1, live)).ok().unwrap();
        p.shared.publish_live(true);
        assert!(Arc::ptr_eq(&optimistic, p.shared.view.lock().unwrap().parts[0].interface.as_ref().unwrap()),
            "a pending edit does not recopy unchanged metadata");
        p.shared.view.lock().unwrap().parts[0].edited[0].2 = Instant::now() - EDIT_SETTLE * 2;
        let (slot, _, live) = p.shared.live_requests.pop().unwrap();
        p.shared.lives.push((slot, 1, live)).ok().unwrap();
        p.shared.publish_live(true);
        assert_eq!(p.shared.view.lock().unwrap().parts[0].interface.as_ref().unwrap().controls[0].properties["$CONTROL_PAR_VALUE"], Value::Int(84));
        assert!(p.shared.view.lock().unwrap().parts[0].edited.is_empty(), "refused optimistic edits eventually settle");
        assert_eq!(p.shared.view.lock().unwrap().parts[0].control_value(0), Some(84.));
        assert!(Arc::ptr_eq(&optimistic, p.shared.view.lock().unwrap().parts[0].interface.as_ref().unwrap()), "refusal restores the source value without recopying it");
        bank_gate.wait();
        loader.join().unwrap();
        assert_eq!(p.shared.view.lock().unwrap().parts[0].load_report.as_ref().unwrap()["runtime"]["faults"][0]["count"], 84,
            "Load eventually formats the latest raw diagnostics");

        // Replace the runtime after a view is prepared but before it commits.
        let (slot, _, mut live) = p.shared.live_requests.pop().unwrap();
        dsp.rack.parts[0].ui_control(0, 0, 13);
        dsp.rack.parts[0].script().unwrap().refresh_live(&mut live);
        p.shared.lives.push((slot, 1, live)).ok().unwrap();
        let publication_gate = Arc::new(Barrier::new(2));
        *p.shared.publish_gate.lock().unwrap() = Some(publication_gate.clone());
        let publisher = {
            let p = p.clone();
            std::thread::spawn(move || p.shared.publish_live(true))
        };
        publication_gate.wait();
        let replacement = Box::new(dsp.rack.parts[0].script().unwrap().live());
        next_epoch(&mut p.shared.view.lock().unwrap(), 0, None, Some(replacement));
        publication_gate.wait();
        publisher.join().unwrap();
        assert_eq!(label(&p), Value::Text("value 84".into()), "prepared old-epoch views cannot overwrite a restore");
        assert_eq!(p.shared.live_requests.len(), 1, "only the new runtime's buffer is lent");

        let (slot, _, mut live) = p.shared.live_requests.pop().unwrap();
        live.interface.as_mut().unwrap().controls[1].properties.insert("$CONTROL_PAR_TEXT".into(), Value::Text("hidden update".into()));
        p.shared.lives.push((slot, 2, live)).ok().unwrap();
        p.shared.publish_live(false);
        assert_eq!(p.shared.lives.len(), 1, "worker leaves visible publication to the editor");
        p.shared.live_publish.lock().unwrap().0 = Some(Instant::now() - LIVE_WATCH * 2);
        {
            let mut view = p.shared.view.lock().unwrap();
            view.watched_at = Some(Instant::now() - LIVE_WATCH * 2);
            view.parts[0].diagnostics_lent = None;
        }
        p.shared.publish_live(false);
        assert_eq!(label(&p), Value::Text("hidden update".into()), "closed editors retain worker fallback");
        assert!(!p.shared.live_requests.pop().unwrap().2.refresh_interface, "hidden views only refresh diagnostics");
    }

    #[test]
    fn file_selector_callbacks_load_real_presets_and_reject_stale_selections_without_allocating() {
        let dir = std::env::temp_dir().join(format!("kontra-selector-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, value) in [("Quiet.nka", 250000), ("Loud.nka", 1000000)] {
            std::fs::write(dir.join(name), format!("%preset\n{value}\n")).unwrap();
        }
        let source = format!(r#"on init
make_perfview
declare ui_file_selector $files
declare ui_label $result(1,1)
declare %preset[1]
declare $id
set_control_par_str(get_ui_id($files),$CONTROL_PAR_BASEPATH,"{}")
set_control_par(get_ui_id($files),$CONTROL_PAR_FILE_TYPE,$NI_FILE_TYPE_ARRAY)
end on
on ui_control($files)
set_text($result,fs_get_filename(get_ui_id($files),0) & ":" & fs_get_filename(get_ui_id($files),1))
$id := load_array_str(%preset,fs_get_filename(get_ui_id($files),2))
end on
on async_complete
if ($NI_ASYNC_ID=$id and $NI_ASYNC_EXIT_STATUS=1)
set_engine_par($ENGINE_PAR_VOLUME,%preset[0],-1,-1,-1)
end if
end on"#,dir.display());
        let instrument = Instrument { scripts: vec![source], ..Default::default() };
        let (rt, errors) = crate::engine::load_scripts(&instrument, Vec::new(), 48000.);
        assert!(errors.is_empty(), "{errors:?}");
        let rt = rt.unwrap();
        let p = SamplerParams::new();
        let mut dsp = Dsp::default();
        dsp.script_epoch[0] = 1;
        {
            let mut view = p.shared.view.lock().unwrap();
            view.parts[0].script_epoch = 1;
            view.parts[0].interface = Some(Arc::new(rt.interface(0)));
        }
        let bank = crate::engine::Bank::from_samples(vec![crate::import::Group::default()],
            vec![crate::import::Zone::default()], vec![(std::path::PathBuf::new(), crate::audio::Sample {
                rate:48000, frames:vec![[0.25;2];48000],
            })]).unwrap();
        dsp.rack.parts[0].set_bank(Some(Box::new(bank)));
        dsp.rack.parts[0].set_script(Some(rt));
        dsp.rack.parts[0].note_on(0,60,100);
        let mut outputs = vec![vec![0.;64];2];
        let mut refs:Vec<_> = outputs.iter_mut().map(Vec::as_mut_slice).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[],&mut refs,64);
        let transport = TransportInfo::default();
        let mut midi_out = EventList::with_capacity(0);
        let mut cx = ProcessContext::new(&transport,48000.,64,&mut midi_out);
        let events = EventList::with_capacity(0);
        let mut peaks = Vec::new();
        for name in ["Quiet.nka","Loud.nka"] {
            let path = dir.join(name).to_string_lossy().into_owned();
            assert!(p.shared.select_control_file(0,1,0,0,&path));
            assert_eq!(allocations(|| { Sampler::process(&mut dsp,&p,&mut buffer,&events,&mut cx); }),0);
            let u = dsp.rack.parts[0].script().unwrap().interface(0);
            assert_eq!(u.controls[1].properties["$CONTROL_PAR_TEXT"],crate::ksp::Value::Text(format!("{}:{name}",name.trim_end_matches(".nka"))));
            assert_eq!(u.controls[0].properties["$CONTROL_PAR_FILEPATH"],crate::ksp::Value::Text(path));
            assert_eq!(p.shared.array_requests.len(),1);
            load_arrays(&p);
            assert_eq!(allocations(|| { for _ in 0..10 { Sampler::process(&mut dsp,&p,&mut buffer,&events,&mut cx); } }),0);
            assert_eq!(dsp.rack.parts[0].script().unwrap().interface(0).controls[0].properties["$CONTROL_PAR_FILE_TYPE"],crate::ksp::Value::Int(2));
            peaks.push(buffer.output(0).iter().copied().map(f32::abs).fold(0.,f32::max));
            load_arrays(&p);
        }
        assert!(peaks[1]>peaks[0]*4.,"selected presets must change audio, not just filenames: {peaks:?}");
        assert!(!p.shared.select_control_file(0,2,0,0,"/stale.nka"));
        assert!(!p.shared.select_control_file(0,1,0,1,"/not-selector.nka"));
        assert!(!p.shared.select_control_file(0,1,0,0,&"x".repeat(321)));
        assert!(p.shared.select_control_file(0,1,0,0,"/stale.nka"));
        dsp.script_epoch[0]=2;
        assert_eq!(allocations(|| { Sampler::process(&mut dsp,&p,&mut buffer,&events,&mut cx); }),0);
        assert!(p.shared.array_requests.is_empty(),"queued old selections must not execute on a replacement script");
        assert!(dsp.rack.parts[0].script().unwrap().diagnostics().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn async_array_reads_use_real_paths_bound_installation_and_reject_stale_epochs_without_allocating() {
        let dir = std::env::temp_dir().join(format!("kontra-async-array-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, value) in [("first.nka", 42), ("second.nka", 84)] {
            std::fs::write(dir.join(name), format!("%data\n{}", format!("{value}\n").repeat(600))).unwrap();
        }
        let source = format!(r#"on init
make_perfview
declare ui_table %data[600](1,1,100)
declare ui_switch $load
declare ui_label $result(1,1)
declare $id
declare $completed
make_persistent($completed)
end on
on ui_control($load)
if ($load=0)
$id := load_array_str(%data,"{}/first.nka")
else
$id := load_array_str(%data,"{}/second.nka")
end if
end on
on async_complete
inc($completed)
set_text($result,$NI_ASYNC_EXIT_STATUS & ":" & %data[0] & ":" & %data[599])
end on"#, dir.display(), dir.display());
        let instrument = Instrument { scripts: vec![source], ..Default::default() };
        let (rt, errors) = crate::engine::load_scripts(&instrument, Vec::new(), 48000.);
        assert!(errors.is_empty(), "{errors:?}");
        let p = SamplerParams::new();
        let mut dsp = Dsp::default();
        dsp.script_epoch[0] = 1;
        let rt = rt.unwrap();
        let mut retained = rt.live();
        {
            let mut view = p.shared.view.lock().unwrap();
            view.parts[0].script_epoch = 1;
            view.parts[0].interface = Some(Arc::new(rt.interface(0)));
        }
        dsp.rack.parts[0].set_script(Some(rt));
        let mut outputs = vec![vec![0.; 64]; 2];
        let mut refs: Vec<_> = outputs.iter_mut().map(Vec::as_mut_slice).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut refs, 64);
        let transport = TransportInfo::default();
        let mut midi_out = EventList::with_capacity(0);
        let mut cx = ProcessContext::new(&transport, 48000., 64, &mut midi_out);
        let events = EventList::with_capacity(0);
        let tick = |dsp: &mut Dsp, buffer: &mut AudioBuffer, cx: &mut ProcessContext| {
            Sampler::process(dsp, &p, buffer, &events, cx);
        };
        p.shared.edit_control(0, 1, 1);
        assert_eq!(allocations(|| tick(&mut dsp, &mut buffer, &mut cx)), 0);
        assert_eq!(p.shared.array_requests.len(), 1);
        assert_eq!(dsp.rack.parts[0].script().unwrap().persistence()[0]["$completed"], crate::ksp::Value::Int(0));
        load_arrays(&p);
        assert_eq!(allocations(|| tick(&mut dsp, &mut buffer, &mut cx)), 0);
        assert_eq!(dsp.rack.parts[0].script().unwrap().persistence()[0]["$completed"], crate::ksp::Value::Int(0), "600 cells do not copy in one block");
        assert_eq!(allocations(|| {
            tick(&mut dsp, &mut buffer, &mut cx);
            tick(&mut dsp, &mut buffer, &mut cx);
        }), 0);
        assert_eq!(dsp.rack.parts[0].script().unwrap().interface(0).controls[2].properties["$CONTROL_PAR_TEXT"], crate::ksp::Value::Text("1:84:84".into()));
        assert_eq!(allocations(|| {
            assert!(dsp.rack.parts[0].script().unwrap().refresh_live(&mut retained));
        }), 0);
        assert_eq!(retained.interface.as_ref().unwrap().controls[0].properties["$CONTROL_PAR_VALUE"],
            crate::ksp::Value::IntArray(vec![84;600]),
            "retained Live table follows typed memory mutation revisions");
        load_arrays(&p); // Free parse buffers on the worker and return the prepared job.
        tick(&mut dsp, &mut buffer, &mut cx);
        assert_eq!(allocations(|| {
            for value in [0, 1, 0] { dsp.rack.parts[0].ui_control(0, 1, value); }
            tick(&mut dsp, &mut buffer, &mut cx);
        }), 0);
        assert_eq!(p.shared.array_requests.len(), 2, "one destination has two prepared jobs");
        assert!(dsp.rack.parts[0].script().unwrap().diagnostics().iter().any(|s| s.contains("prepared request queue")));
        load_arrays(&p);
        assert_eq!(allocations(|| {
            for _ in 0..3 { tick(&mut dsp, &mut buffer, &mut cx); }
        }), 0);
        assert_eq!(dsp.rack.parts[0].script().unwrap().interface(0).controls[2].properties["$CONTROL_PAR_TEXT"], crate::ksp::Value::Text("1:42:42".into()));
        assert_eq!(allocations(|| { dsp.rack.parts[0].script().unwrap().refresh_live(&mut retained); }), 0);
        assert_eq!(retained.interface.as_ref().unwrap().controls[0].properties["$CONTROL_PAR_VALUE"],
            crate::ksp::Value::IntArray(vec![42;600]),
            "a subsequent file invalidates the same retained table snapshot");
        assert_eq!(allocations(|| {
            for _ in 0..3 { tick(&mut dsp, &mut buffer, &mut cx); }
        }), 0);
        assert_eq!(dsp.rack.parts[0].script().unwrap().persistence()[0]["$completed"], crate::ksp::Value::Int(4), "each accepted or rejected job completes once");
        assert_eq!(dsp.rack.parts[0].script().unwrap().interface(0).controls[2].properties["$CONTROL_PAR_TEXT"], crate::ksp::Value::Text("1:84:84".into()), "different paths on the same variable read different files");
        load_arrays(&p);
        tick(&mut dsp, &mut buffer, &mut cx);
        dsp.rack.parts[0].ui_control(0, 1, 0);
        tick(&mut dsp, &mut buffer, &mut cx);
        load_arrays(&p);
        let (new_rt, _) = crate::engine::load_scripts(&instrument, Vec::new(), 48000.);
        let mut retired = None;
        p.shared.view.lock().unwrap().parts[0].script_epoch = 2;
        assert_eq!(allocations(|| {
            retired = dsp.rack.parts[0].set_script(new_rt);
            dsp.script_epoch[0] = 2;
            tick(&mut dsp, &mut buffer, &mut cx);
        }), 0);
        load_arrays(&p);
        assert_eq!(dsp.rack.parts[0].script().unwrap().persistence()[0]["$completed"], crate::ksp::Value::Int(0), "stale results never enter the replacement script");
        drop(retired);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn async_array_writes_capture_call_time_values_fail_honestly_and_cancel_stale_jobs_without_allocating() {
        let dir = std::env::temp_dir().join(format!("kontra-async-save-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = format!(r#"on init
make_perfview
declare ui_menu $save
add_menu_item($save,"Write",1)
add_menu_item($save,"Fail",2)
add_menu_item($save,"Queue",3)
add_menu_item($save,"Stale",4)
add_menu_item($save,"Committed",5)
declare %integers[3] := (42,-3,7)
declare ?reals[2] := (0.25,-1.5)
declare !names[2]
!names[0] := "Ω"
!names[1] := "😀"
declare $done
declare $success
make_persistent($done)
make_persistent($success)
end on
on ui_control($save)
select ($save)
case 1
save_array_str(%integers,"{0}/integers.nka")
save_array_str(?reals,"{0}/reals.nka")
save_array_str(!names,"{0}/names.nka")
%integers[0] := 999
?reals[0] := 99.0
!names[0] := "later"
case 2
save_array_str(%integers,"{0}/missing/failed.nka")
case 3
%integers[0] := 50
save_array_str(%integers,"{0}/queued.nka")
%integers[0] := 51
save_array_str(%integers,"{0}/queued.nka")
%integers[0] := 52
save_array_str(%integers,"{0}/queued.nka")
case 4
save_array_str(%integers,"{0}/stale.nka")
case 5
save_array_str(%integers,"{0}/committed.nka")
end select
end on
on async_complete
inc($done)
$success := $success+$NI_ASYNC_EXIT_STATUS
end on"#,dir.display());
        let instrument = Instrument {scripts:vec![source],..Default::default()};
        let (rt,errors)=crate::engine::load_scripts(&instrument,Vec::new(),48000.);
        assert!(errors.is_empty(),"{errors:?}");
        let p=SamplerParams::new();
        let mut dsp=Dsp::default();
        dsp.script_epoch[0]=1;
        {
            let mut view=p.shared.view.lock().unwrap();
            view.parts[0].script_epoch=1;
            view.parts[0].interface=Some(Arc::new(rt.as_deref().unwrap().interface(0)));
        }
        dsp.rack.parts[0].set_script(rt);
        let mut outputs=vec![vec![0.;64];2];
        let mut refs:Vec<_>=outputs.iter_mut().map(Vec::as_mut_slice).collect();
        let mut buffer=AudioBuffer::from_slices_checked(&[],&mut refs,64);
        let transport=TransportInfo::default();
        let mut midi_out=EventList::with_capacity(0);
        let mut cx=ProcessContext::new(&transport,48000.,64,&mut midi_out);
        let events=EventList::with_capacity(0);
        let tick=|dsp:&mut Dsp,buffer:&mut AudioBuffer,cx:&mut ProcessContext|{
            Sampler::process(dsp,&p,buffer,&events,cx);
        };
        p.shared.edit_control(0,0,1);
        assert_eq!(allocations(||tick(&mut dsp,&mut buffer,&mut cx)),0);
        assert!(!dir.join("integers.nka").exists(),"audio only snapshots and queues");
        load_arrays(&p);
        assert_eq!(std::fs::read(dir.join("integers.nka")).unwrap(),b"%integers\n42\n-3\n7\n");
        assert_eq!(std::fs::read(dir.join("reals.nka")).unwrap(),b"?reals\n0.25\n-1.5\n");
        assert_eq!(std::fs::read(dir.join("names.nka")).unwrap(),"!names\nΩ\n😀\n".as_bytes());
        assert_eq!(allocations(||for _ in 0..3 {tick(&mut dsp,&mut buffer,&mut cx);}),0);
        assert_eq!(dsp.rack.parts[0].script().unwrap().persistence()[0]["$success"],crate::ksp::Value::Int(3));
        load_arrays(&p);tick(&mut dsp,&mut buffer,&mut cx);
        assert_eq!(allocations(||{
            dsp.rack.parts[0].ui_control(0,0,2);tick(&mut dsp,&mut buffer,&mut cx);
        }),0);
        load_arrays(&p);
        assert!(!dir.join("missing").exists());
        assert_eq!(allocations(||tick(&mut dsp,&mut buffer,&mut cx)),0);
        assert!(dsp.rack.parts[0].script().unwrap().diagnostics().iter().any(|s|s.starts_with("save_array_str: file could not be written")));
        load_arrays(&p);tick(&mut dsp,&mut buffer,&mut cx);
        assert_eq!(allocations(||{
            dsp.rack.parts[0].ui_control(0,0,3);tick(&mut dsp,&mut buffer,&mut cx);
        }),0);
        assert_eq!(p.shared.array_requests.len(),2);
        load_arrays(&p);
        assert_eq!(std::fs::read(dir.join("queued.nka")).unwrap(),b"%integers\n51\n-3\n7\n");
        assert_eq!(allocations(||for _ in 0..2 {tick(&mut dsp,&mut buffer,&mut cx);}),0);
        let saved=dsp.rack.parts[0].script().unwrap().persistence();
        assert_eq!(saved[0]["$done"],crate::ksp::Value::Int(7));
        assert_eq!(saved[0]["$success"],crate::ksp::Value::Int(5));
        assert!(dsp.rack.parts[0].script().unwrap().diagnostics().iter().any(|s|s.starts_with("save_array_str: prepared request queue")));
        load_arrays(&p);tick(&mut dsp,&mut buffer,&mut cx);
        assert_eq!(allocations(||{
            dsp.rack.parts[0].ui_control(0,0,4);tick(&mut dsp,&mut buffer,&mut cx);
        }),0);
        let (rt,_)=crate::engine::load_scripts(&instrument,Vec::new(),48000.);
        let mut retired=None;
        p.shared.view.lock().unwrap().parts[0].script_epoch=2;
        assert_eq!(allocations(||{
            retired=dsp.rack.parts[0].set_script(rt);dsp.script_epoch[0]=2;
        }),0);
        load_arrays(&p);
        assert!(!dir.join("stale.nka").exists(),"stale queued write never begins");
        drop(retired);
        assert_eq!(allocations(||{
            dsp.rack.parts[0].ui_control(0,0,5);tick(&mut dsp,&mut buffer,&mut cx);
        }),0);
        load_arrays(&p);
        let committed=std::fs::read(dir.join("committed.nka")).unwrap();
        let (rt,_)=crate::engine::load_scripts(&instrument,Vec::new(),48000.);
        let mut retired=None;
        p.shared.view.lock().unwrap().parts[0].script_epoch=3;
        assert_eq!(allocations(||{
            retired=dsp.rack.parts[0].set_script(rt);dsp.script_epoch[0]=3;
            tick(&mut dsp,&mut buffer,&mut cx);
        }),0);
        load_arrays(&p);
        assert_eq!(std::fs::read(dir.join("committed.nka")).unwrap(),committed,"committed save is not rolled back when completion becomes stale");
        assert_eq!(dsp.rack.parts[0].script().unwrap().persistence()[0]["$done"],crate::ksp::Value::Int(0));
        drop(retired);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sound_off_preserves_later_note_cleanup_without_audio_allocations() {
        let source = "on init\ndeclare $held\ndeclare $released\nmake_persistent($held)\nmake_persistent($released)\nend on\non note\ninc($held)\nend on\non release\nwait(1000000)\ndec($held)\ninc($released)\nplay_note(72,100,0,0)\nend on";
        let mut init = crate::ksp::LogEngine::default();
        let (rt, errors) = Runtime::with_scripts(&[source], &mut init, 0, Vec::new());
        assert!(errors.iter().all(Option::is_none));
        let mut engine = Engine::default();
        engine.set_script(Some(Box::new(rt)));
        let mut left = [0f32;64];
        let mut right = [0f32;64];
        assert_eq!(allocations(|| {
            engine.note_on(0,60,100);
            engine.cc(0,120,0);
            engine.note_off(0,60);
            engine.render(&mut left,&mut right);
            engine.note_on(0,60,100);
            engine.cc(0,120,0);
            engine.panic();
            engine.render(&mut left,&mut right);
        }),0);
        let rt = engine.script().unwrap();
        assert_eq!(rt.persistence()[0]["$held"],crate::ksp::Value::Int(0));
        assert_eq!(rt.persistence()[0]["$released"],crate::ksp::Value::Int(2));
        assert_eq!(rt.env.events.live_count(),0);
        assert_eq!(engine.pending_work(),[0,0,0]);
        assert!(rt.diagnostics().is_empty(),"{:?}",rt.diagnostics());
    }

    #[test]
    fn controller_resets_restore_authored_device_defaults_without_audio_allocations() {
        use crate::{audio::Sample, import::{Group, Instrument, Loop, ModAssignment, ModSource, ModTarget, Zone}};
        let source = "on init\ndeclare ui_switch $setting\nset_controller(1,77)\nset_controller(11,12)\nset_controller(64,127)\nset_controller(100,5)\nset_controller(110,127)\nset_controller(111,127)\nset_controller(113,63)\nend on";
        for reset in 0..3 {
            let group = Group {
                mods: [110, 111, 113].map(|cc| ModAssignment {
                    name: format!("internal CC{cc}"), source: ModSource::MidiCc(cc),
                    target: ModTarget::Volume, intensity: 1., invert: false,
                    lag_ms: 0, shaper: None,
                }).into(),
                ..Default::default()
            };
            let zone = Zone {
                loop_range: Some(Loop { start: 0, end: 128, alternating: false, until_release: false, crossfade: 0 }),
                ..Default::default()
            };
            let instrument = Instrument { groups: vec![group], scripts: vec![source.into()], ..Default::default() };
            let (rt, errors) = crate::engine::load_scripts(&instrument, Vec::new(), 48000.);
            assert!(errors.is_empty(), "{errors:?}");
            let rt = rt.unwrap();
            for pair in [(110, 127), (111, 127), (113, 63)] {
                assert!(rt.init_controllers.contains(&pair), "host init must record device defaults");
            }
            let bank = Bank::from_samples(instrument.groups, vec![zone], vec![(PathBuf::new(),
                Sample { rate: 48000, frames: vec![[0.5, 0.25]; 128] })]).unwrap();
            let mut engine = Engine::default();
            engine.set_bank(Some(Box::new(bank)));
            engine.set_script(Some(rt));
            engine.ui_control(0, 0, 1);
            let mut left = [0f32; 64];
            let mut right = [0f32; 64];
            for cc in [1, 11, 64, 100, 110, 111, 113] { engine.cc(0, cc, 0); }
            engine.render(&mut left, &mut right);
            assert_eq!(allocations(|| {
                match reset {
                    0 => engine.cc(0, 121, 0),
                    1 => engine.panic(),
                    _ => engine.reset(44100.),
                }
                engine.render(&mut left, &mut right);
                engine.note_on(0, 60, 100);
                engine.render(&mut left, &mut right);
            }), 0);
            for (cc, expected) in [(110, 127), (111, 127), (113, 63)] {
                assert_eq!(engine.cc_state()[0][cc], expected, "device CC{cc}, reset {reset}");
                assert_eq!(engine.script().unwrap().env.input.cc[cc], i32::from(expected));
            }
            // Host reset replays init controllers; MIDI reset/Panic retain
            // their mandated performance defaults even if init set another value.
            let standard = if reset == 2 { [77, 12, 127, 5] } else { [0, 127, 0, 127] };
            for (cc, expected) in [1, 11, 64, 100].into_iter().zip(standard) {
                assert_eq!(engine.cc_state()[0][cc], expected, "standard CC{cc}, reset {reset}");
                assert_eq!(engine.script().unwrap().env.input.cc[cc], i32::from(expected));
            }
            assert!(left.iter().any(|x| x.abs() > 0.001), "fresh notes must recover, reset {reset}");
            assert!(engine.voice_census().iter().any(|v| v.gain > 0.), "fresh voices have real gain");
            assert_eq!(engine.script().unwrap().interface(0).controls[0].properties["$CONTROL_PAR_VALUE"], crate::ksp::Value::Int(1));
        }
    }

    #[test]
    fn panic_and_host_reset_release_script_buffers_without_audio_allocations() {
        let source = "on init\ndeclare $held\ndeclare $released\ndeclare ui_switch $setting\nmake_persistent($held)\nmake_persistent($released)\nend on\non note\ninc($held)\nend on\non release\nwait(1000000)\ndec($held)\ninc($released)\nplay_note(72,100,0,0)\nend on";
        let mut init = crate::ksp::LogEngine::default();
        let (rt, errors) = Runtime::with_scripts(&[source], &mut init, 0, Vec::new());
        assert!(errors.iter().all(Option::is_none));
        let mut engine = Engine::default();
        engine.set_script(Some(Box::new(rt)));
        engine.ui_control(0,0,1);
        let mut left = [0f32;64];
        let mut right = [0f32;64];
        assert_eq!(allocations(|| {
            engine.note_on(0,60,100);
            engine.panic();
            engine.render(&mut left,&mut right);
            engine.note_on(0,60,100);
            engine.reset(48000.0);
            engine.render(&mut left,&mut right);
        }),0);
        let rt = engine.script().unwrap();
        assert_eq!(rt.persistence()[0]["$held"],crate::ksp::Value::Int(0));
        assert_eq!(rt.persistence()[0]["$released"],crate::ksp::Value::Int(2));
        assert_eq!(rt.interface(0).controls[0].properties["$CONTROL_PAR_VALUE"],crate::ksp::Value::Int(1));
        assert_eq!(engine.pending_work(),[0,0,0]);
        assert!(rt.diagnostics().is_empty(),"{:?}",rt.diagnostics());
    }

    #[test]
    fn repeated_debug_string_growth_preserves_notes_without_audio_allocations() {
        let source = "on init\ndeclare @debug\ndeclare $notes\nmake_persistent($notes)\nend on\non note\n@debug := @debug & \"😀\"\nmessage(@debug)\ninc($notes)\nend on";
        let mut init = crate::ksp::LogEngine::default();
        let (rt, errors) = Runtime::with_scripts(&[source], &mut init, 0, Vec::new());
        assert!(errors.iter().all(Option::is_none));
        let mut engine = Engine::default();
        engine.set_script(Some(Box::new(rt)));
        let mut left = [0f32;64];
        let mut right = [0f32;64];
        assert_eq!(allocations(|| {
            for _ in 0..800 {
                engine.note_on(0,60,100);
                engine.note_off(0,60);
                engine.render(&mut left,&mut right);
            }
        }),0);
        let rt = engine.script().unwrap();
        assert_eq!(rt.persistence()[0]["$notes"],crate::ksp::Value::Int(800));
        assert_eq!(rt.last_message(),"😀".repeat(320));
        assert!(rt.diagnostics().is_empty(),"{:?}",rt.diagnostics());
    }

    #[test]
    fn replacement_runtimes_reject_queued_old_views_and_snapshots_without_audio_allocations() {
        use crate::ksp::Value;
        let runtime = |count, name: &str, saved| {
            let mut source = format!("on init\nmake_perfview\ndeclare $saved := {saved}\nmake_persistent($saved)\nset_control_par_str($INST_WALLPAPER_ID,$CONTROL_PAR_PICTURE,\"{name}\")\ndeclare ui_slider $s(0,100)\n");
            for n in 1..count { source.push_str(&format!("declare ui_label $l{n}(1,1)\n")); }
            source.push_str("end on\non ui_control($s)\n$saved := $s\nend on");
            let (rt, errors) = Runtime::with_scripts(&[source], &mut crate::ksp::LogEngine::default(), 0, Vec::new());
            assert!(errors.iter().all(Option::is_none));
            rt
        };
        // Actual replacement sizes: Areia's 847 controls and CHORUS's 572.
        let old = runtime(847, "old", 11);
        let new = runtime(572, "new", 22);
        let old_live = Box::new(old.live());
        let old_address = (&*old_live as *const Live) as usize;
        let old_saved = Box::new(PersistenceSnapshot { script: old.persistence(), ir: Vec::new(), native: old.native_state.snapshot() });
        let saved_address = (&*old_saved as *const PersistenceSnapshot) as usize;
        let new_live = Box::new(new.live());
        let new_interface = Arc::new(new_live.interface.clone().unwrap());
        let new_saved = Box::new(PersistenceSnapshot { script: new.persistence(), ir: Vec::new(), native: new.native_state.snapshot() });
        let p = SamplerParams::new();
        {
            let mut view = p.shared.view.lock().unwrap();
            view.script_epoch = 1;
            view.parts[0].script_epoch = 1;
            view.parts[0].live = Some(old_live);
        }
        p.shared.publish_live(true); // Old buffer is queued before installation.
        p.shared.snapshot_requests.push((0, 1, old_saved)).ok().unwrap();
        {
            let mut view = p.shared.view.lock().unwrap();
            assert_eq!(next_epoch(&mut view, 0, None, Some(new_live)), 2);
            view.parts[0].interface = Some(new_interface.clone());
        }
        let mut dsp = Dsp::default();
        dsp.until_poll = usize::MAX;
        dsp.script_epoch[0] = 1;
        dsp.rack.parts[0].set_script(Some(Box::new(old)));
        p.shared.ready.push((0, 0, Handoff::Script { script: Some(Box::new(new)), bank:None, epoch: 2 })).ok().unwrap();
        let transport = TransportInfo::default();
        let mut outgoing = EventList::with_capacity(0);
        let none = EventList::with_capacity(0);
        let mut cx = ProcessContext::new(&transport, 48000., 64, &mut outgoing);
        let mut left = [0f32;64];
        let mut right = [0f32;64];
        let mut outputs = [&mut left[..], &mut right[..]];
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut outputs, 64);
        assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &none, &mut cx); }), 0);
        let (slot, epoch, live) = p.shared.lives.pop().unwrap();
        assert_eq!((slot, epoch), (0, 1), "a stale buffer retains its source epoch");
        assert_eq!((&*live as *const Live) as usize, old_address);
        assert_eq!(live.interface.as_ref().unwrap().controls.len(), 847);
        assert_eq!(live.interface.as_ref().unwrap().wallpaper, "old", "rejected buffers are never refreshed");
        p.shared.lives.push((slot, epoch, live)).ok().unwrap();
        let (slot, epoch, saved, changed) = p.shared.snapshots.pop().unwrap();
        assert_eq!((slot, epoch, changed), (0, 1, false));
        assert_eq!((&*saved as *const PersistenceSnapshot) as usize, saved_address);
        assert_eq!(saved.script[0]["$saved"], Value::Int(11));
        drop(saved); // Stale snapshots are retired on this worker/test thread.
        p.shared.publish_live(true); // Rejects the old view and lends the new one.
        assert!(Arc::ptr_eq(&new_interface, p.shared.view.lock().unwrap().parts[0].interface.as_ref().unwrap()));
        assert!(p.shared.lives.is_empty() && p.shared.snapshots.is_empty());
        assert_eq!(p.shared.live_requests.len(), 1, "the stale buffer is returned exactly once");
        p.shared.snapshot_requests.push((0, 2, new_saved)).ok().unwrap();
        assert_eq!(allocations(|| {
            dsp.rack.parts[0].ui_control(0, 0, 37);
            for _ in 0..100 { Sampler::process(&mut dsp, &p, &mut buffer, &none, &mut cx); }
        }), 0);
        let (_, epoch, saved, changed) = p.shared.snapshots.pop().unwrap();
        assert_eq!(epoch, 2);
        assert!(changed);
        assert_eq!(saved.script[0]["$saved"], Value::Int(37), "new snapshots refresh normally");
        p.shared.publish_live(true);
        let view = p.shared.view.lock().unwrap();
        let interface = view.parts[0].interface.as_ref().unwrap();
        assert_eq!(interface.controls.len(), 572);
        assert_eq!(interface.wallpaper, "new");
        assert_eq!(interface.controls[0].properties["$CONTROL_PAR_VALUE"], Value::Int(37));
    }

    #[test]
    fn changed_live_views_wake_publication_without_allocating_or_busy_polling() {
        use moose::core::tasks::{TaskSpawner, TaskSpawnerBundle};
        let source = "on init\nmake_perfview\ndeclare ui_slider $s(0,100)\nend on";
        let mut engine = crate::ksp::LogEngine::default();
        let (rt, errors) = Runtime::with_scripts(&[source], &mut engine, 0, Vec::new());
        assert!(errors.iter().all(Option::is_none));
        let p = SamplerParams::new();
        let mut dsp = Dsp::default();
        dsp.script_epoch[0] = 1;
        dsp.until_poll = usize::MAX;
        let live = Box::new(rt.live());
        dsp.rack.parts[0].set_script(Some(Box::new(rt)));
        dsp.rack.parts[0].ui_control(0, 0, 42);
        p.shared.live_requests.push((0, dsp.script_epoch[0], live)).ok().unwrap();
        moose::core::tasks::warm_pool();
        let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut tasks = TaskSpawnerBundle::new();
        let counted = runs.clone();
        tasks.push(TaskSpawner::<Load>::new_serialized(move |_| { counted.fetch_add(1, Ordering::Release); }));
        let tasks = tasks.into_any().unwrap();
        let transport = TransportInfo::default();
        let mut midi = EventList::with_capacity(0);
        let none = EventList::with_capacity(0);
        let mut cx = ProcessContext::new(&transport, 48000., 64, &mut midi).with_tasks(&tasks);
        let mut left = [0f32;64];
        let mut right = [0f32;64];
        let mut outputs = [&mut left[..], &mut right[..]];
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut outputs, 64);
        assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &none, &mut cx); }), 0);
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        while runs.load(Ordering::Acquire) == 0 && Instant::now() < deadline { std::thread::yield_now(); }
        assert_eq!(runs.load(Ordering::Acquire), 1, "changed view wakes the real task lane");
        for _ in 0..8 {
            let (slot, _, live) = p.shared.lives.pop().unwrap();
            p.shared.live_requests.push((slot, dsp.script_epoch[slot], live)).ok().unwrap();
            assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &none, &mut cx); }), 0);
        }
        drop(tasks);
        assert_eq!(runs.load(Ordering::Acquire), 1, "unchanged returned buffers must not wake the worker again");
    }

    /// A click's value holds over a live view begun before the scripts had
    /// it, so the control does not flick back for a frame.
    #[test]
    fn an_edit_outranks_a_live_view_that_has_not_seen_it() {
        use crate::ksp::{Control, Interface, Value};
        let shown = |v: i32| {
            let mut i = Interface::default();
            i.controls.push(Control {
                id: 0,
                variable: "$b".into(),
                kind: "ui_button".into(),
                properties: [("$CONTROL_PAR_VALUE".to_owned(), Value::Int(v))].into(),
                menu: Vec::new(),
            });
            i
        };
        let mut part = PartView { interface: Some(Arc::new(shown(0))),
            edited: vec![(0, 1, Instant::now())], ..Default::default() };
        let retained = part.interface.clone().unwrap();
        settle_edits(&mut part.edited, part.interface.as_deref());
        assert_eq!((part.control_value(0), part.edited.len()), (Some(1.), 1), "a stale view shows the overlay");
        assert_eq!(retained.controls[0].properties["$CONTROL_PAR_VALUE"], Value::Int(0), "callback snapshots remain immutable");
        part.interface = Some(Arc::new(shown(1)));
        settle_edits(&mut part.edited, part.interface.as_deref());
        assert!(part.edited.is_empty(), "a view that has it ends the edit");
        part.interface = Some(Arc::new(shown(0)));
        part.edited = vec![(0, 1, Instant::now() - EDIT_SETTLE * 2)];
        settle_edits(&mut part.edited, part.interface.as_deref());
        assert_eq!((part.control_value(0), part.edited.len()), (Some(0.), 0), "a script that kept its value wins in the end");
    }

    /// Routing never holds the selection while it waits for `view`, which
    /// the editor holds while reading the selection: both would stop.
    #[test]
    fn routing_waits_for_the_view_without_holding_the_selection() {
        let p = Arc::new(SamplerParams::new());
        let view = p.shared.view.lock().unwrap();
        let router = {
            let p = p.clone();
            std::thread::spawn(move || route(&p))
        };
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        let mut readable = true;
        while Instant::now() < deadline {
            readable &= p.selection.try_read().is_ok();
            std::thread::yield_now();
        }
        drop(view);
        router.join().unwrap();
        assert!(readable, "routing held the selection while it waited for the view");
    }
    use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell};

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
    #[cfg(feature = "uvi")]
    fn staged_uvi_worker_is_owned_off_audio_and_rejects_cancel_reset_and_stale_destination() {
        let dir = std::env::temp_dir().join(format!("kontra-uvi-staged-ready-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bank = dir.join("authored.ufs");
        crate::library::tests::authored_uvi_bank(&bank, 4);
        let mut params = SamplerParams::new();
        params.shared.libraries = crate::library::tests::authored_uvi_scanner(&dir);
        let part = Part { path: "/playing/original.nki".into(), snapshot: "/playing/original.nksn".into(), ..Default::default() };
        let request = library::UviRequest { source: library::UviSource { bank, bank_uuid: [4; 16], member: "Piano.uvip".into() }, slot: Some(0), new: false };
        *params.selection.write().unwrap() = Selection { parts: vec![part.clone()], uvi_requested: Some(request.clone()), ..Default::default() };
        params.shared.part(0).unwrap().generation.store(9, Ordering::Release);
        let ready = |params: &SamplerParams| {
            let start = Instant::now();
            loop {
                prepare_uvi(params, &params.selection.read().unwrap().clone());
                if let Some(identity) = params.with_prepared_uvi_worker(|_, epoch, generation, worker| {
                    assert!(worker.stats().initialization_ns > 0); (epoch, generation)
                }) { break identity; }
                assert!(start.elapsed().as_secs() < 5, "authored worker did not initialize: {}", params.shared.view.lock().unwrap().uvi_status);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        let first = ready(&params);
        {
            let mut view=params.shared.view.lock().unwrap();
            let before=view.uvi_status.clone();
            catalog_uvi_receipt(&mut view,Some(&request));
            assert_eq!(view.uvi_attempted.as_ref(),Some(&request));
            assert_eq!(view.uvi_status,before,"matching metadata receipt retains staged status and panel identity");
        }
        prepare_uvi(&params,&params.selection.read().unwrap().clone());
        assert_eq!(params.with_prepared_uvi_worker(|_,epoch,generation,_| (epoch,generation)),Some(first),
            "catalog metadata receipt does not replace a matching initialized worker");
        assert!(params.shared.ready.is_empty(), "a staged controller never enters the Kontakt callback queue");
        assert!(params.selection.read().unwrap().parts == [part.clone()]);
        assert_eq!(params.shared.part(0).unwrap().generation.load(Ordering::Acquire), 9);
        // Same-rate reset still changes the activation identity, with no join in reset.
        let mut dsp = Dsp::default();
        let config = AudioConfig::new(48000., 256);
        assert_eq!(allocations(|| Sampler::reset(&mut dsp, &params, &config)), 0);
        assert!(params.with_prepared_uvi_worker(|_, _, _, _| ()).is_none());
        let reset = ready(&params);
        assert_ne!(first, reset);
        params.selection.write().unwrap().parts[0].snapshot = "/playing/changed.nksn".into();
        assert!(params.with_prepared_uvi_worker(|_, _, _, _| ()).is_none());
        assert_ne!(ready(&params), reset);
        params.selection.write().unwrap().uvi_requested = None;
        prepare_uvi(&params, &params.selection.read().unwrap().clone());
        assert!(params.shared.uvi_prepared.lock().unwrap().is_none());
        // A request superseded before provider completion cannot publish or start.
        let stale = Selection { parts: vec![part], uvi_requested: Some(request), ..Default::default() };
        let generation = params.shared.uvi_generation.load(Ordering::Acquire);
        prepare_uvi(&params, &stale);
        assert!(params.shared.uvi_prepared.lock().unwrap().is_none());
        assert_eq!(params.shared.uvi_generation.load(Ordering::Acquire), generation);
        assert!(params.shared.ready.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[cfg(feature = "uvi")]
    fn staged_uvi_failure_keeps_the_existing_source_and_retires_the_controller() {
        let dir = std::env::temp_dir().join(format!("kontra-uvi-staged-failure-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bank = dir.join("authored.ufs");
        crate::library::tests::authored_uvi_bank_with_source(&bank, 5, br#"<Program><EventProcessors><ScriptProcessor><script>error('original authored initialization failure')</script></ScriptProcessor></EventProcessors><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" StartPhase="0.25" BaseNote="60"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#);
        let mut params = SamplerParams::new();
        params.shared.libraries = crate::library::tests::authored_uvi_scanner(&dir);
        let part = Part { path: "/playing/original.nki".into(), ..Default::default() };
        *params.selection.write().unwrap() = Selection { parts: vec![part.clone()], uvi_requested: Some(library::UviRequest {
            source: library::UviSource { bank, bank_uuid: [5; 16], member: "Piano.uvip".into() }, slot: Some(0), new: false,
        }), ..Default::default() };
        let source = params.selection.read().unwrap().uvi_requested.as_ref().unwrap().source.clone();
        assert!(params.shared.libraries.uvi_worker_config(&source, 48000).is_ok(), "failure must come from initialization, not provider rejection");
        let start = Instant::now();
        loop {
            prepare_uvi(&params, &params.selection.read().unwrap().clone());
            if params.shared.uvi_prepared.lock().unwrap().as_ref().is_some_and(|prepared| prepared.worker.is_none()) { break; }
            assert!(start.elapsed().as_secs() < 5);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let generation = params.shared.uvi_generation.load(Ordering::Acquire);
        prepare_uvi(&params, &params.selection.read().unwrap().clone());
        assert_eq!(params.shared.uvi_generation.load(Ordering::Acquire), generation, "failure is not retried every audio poll");
        assert!(params.selection.read().unwrap().parts == [part]);
        assert!(params.shared.ready.is_empty());
        assert!(!params.shared.view.lock().unwrap().uvi_status.contains("original authored initialization failure"), "raw private failures stay out of public status");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn uvi_requests_round_trip_without_changing_legacy_kontakt_state() {
        use moose::core::custom_state::State;
        let source = library::UviSource { bank: "/banks/original.ufs".into(), bank_uuid: [7; 16], member: "Root/Keys/Piano.uvip".into() };
        let selection = Selection {
            parts: vec![Part { path: "/libraries/piano.nki".into(), snapshot: "/libraries/piano.nksn".into(), ..Default::default() }],
            order: vec![0], favorites: vec!["/libraries/piano.nki".into()], recent: vec!["/libraries/piano.nki".into()],
            uvi_favorites: vec![source.clone()], uvi_recent: vec![source.clone()],
            uvi_requested: Some(library::UviRequest { source, slot: Some(0), new: false }), ..Default::default()
        };
        let bytes = selection.serialize();
        assert!(Selection::deserialize(&bytes) == Some(selection.clone()));
        let old_count = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) - 3;
        let mut old_keyed = bytes[..8].to_vec();
        old_keyed[4..8].copy_from_slice(&old_count.to_le_bytes());
        let mut legacy = old_count.to_le_bytes().to_vec();
        let mut at = 8;
        for _ in 0..old_count {
            let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            old_keyed.extend_from_slice(&bytes[at..at + 8 + len]);
            legacy.extend_from_slice(&bytes[at + 4..at + 8 + len]);
            at += 8 + len;
        }
        let mut old = selection;
        old.uvi_favorites.clear(); old.uvi_recent.clear(); old.uvi_requested = None;
        assert!(Selection::deserialize(&old_keyed) == Some(old.clone()));
        assert!(Selection::deserialize(&legacy) == Some(old));
    }

    #[test]
    fn rack_state_round_trip() {
        use moose::core::custom_state::State;
        let state = Selection {
            root: "/libraries".into(),
            multi: String::new(),
            parts: vec![
                Part {
                    path: "first.nkm".into(),
                    program: 2,
                    channel: 2,
                    gain: -9.,
                    pan: 0.25,
                    tune: -3.5,
                    mute: true,
                    ..Default::default()
                },
                Part {
                    path: "second.nki".into(),
                    group: 3,
                    solo: true,
                    streaming: Some(Streaming::Auto),
                    timing: Timing { override_ms: Some(120.), exclude: true, ..Default::default() },
                    output: 3,
                    output_manual: true,
                    mic_buses: vec![-1, 4],
                    mic_names: vec![String::new(), "Close".into()],
                    view: 2,
                    ..Default::default()
                },
            ],
            outputs: 2,
            order: vec![1, 0],
            midi_thru: true,
            favorites: vec!["/libraries/Solo/a.nki".into()],
            recent: vec!["second.nki".into(), "first.nkm".into()],
            uvi_favorites: Vec::new(),
            uvi_recent: Vec::new(),
            uvi_requested: None,
            qwerty: true,
            browser_width: 300.,
            browser_split: 0.4,
            appearance: 2,
            sharp_artwork: true,
            sticky_off: true,
            streaming: Streaming::RamOnly,
            auto_align: true,
            align_transport_only: true,
            buses: vec![
                Bus::default(),
                Bus {
                    name: "Brass".into(),
                    gain: -6.,
                    pan: -0.5,
                    mute: true,
                    solo: true,
                    port: 3,
                },
            ],
        };
        assert!(Selection::deserialize(&state.serialize()).unwrap() == state);
        assert_eq!(rack_controls(&state)[0].tune, -3.5, "the part's tune reaches the engine");
        let mix = mix(&state);
        assert_eq!(mix.buses[0], BusControls::on(0), "untouched buses play through their own port");
        assert_eq!(mix.buses[15], BusControls::on(15));
        assert_eq!((mix.buses[1].port, mix.buses[1].mute, mix.buses[1].solo), (3, true, true));
        assert!((mix.buses[1].gain - db_to_linear(-6.)).abs() < 1e-6);
        assert_eq!((state.bus(1).label(1), state.bus(7).label(7)), ("Brass".into(), "st.8".into()));
    }
    #[test]
    fn new_parts_take_the_next_free_midi_channel() {
        let part = |port, channel| Part {
            path: "a.nki".into(),
            port,
            channel,
            ..Default::default()
        };
        let mut rack = Selection::default();
        assert_eq!(rack.next_input(), (0, 0), "the first part plays on A1");
        rack.parts = vec![part(0, 0), part(0, -1), Part::default(), part(0, 2)];
        assert_eq!(rack.next_input(), (0, 1), "omni and empty slots take no channel");
        rack.parts = (0..16).map(|c| part(0, c)).collect();
        assert_eq!(rack.next_input(), (1, 0), "A full: B1");
        rack.parts = (0..64).map(|n| part(n as u8 / 16, n % 16)).collect();
        assert_eq!(rack.next_input(), (0, -1), "all 64 taken: omni");
    }
    #[test]
    fn saved_multi_round_trips_through_the_browser_and_loader() {
        let root = std::env::temp_dir().join(format!("kontakto-multi-{}", std::process::id()));
        let path = root.join("Multis").join("Evening.kontra-multi");
        let rack = Selection {
            root: root.to_string_lossy().into_owned(),
            parts: vec![
                Part {
                    path: "/libraries/Keys/Piano.nki".into(),
                    snapshot: "/libraries/Keys/Warm.nksn".into(),
                    channel: 3,
                    port: 1,
                    output: 2,
                    gain: -6.5,
                    pan: -0.5,
                    tune: 7.02,
                    collapsed: true,
                    name: "Left hand".into(),
                    script_state: r#"[{"name":"$legato","value":1}]"#.into(),
                    ..Default::default()
                },
                Part {
                    path: "/libraries/Strings/Ensemble.nkm".into(),
                    program: 2,
                    mute: true,
                    ..Default::default()
                },
            ],
            order: vec![1, 0],
            ..Default::default()
        };
        SavedMulti::of("Evening", &rack).save(&path).unwrap();
        assert!(import::is_multi(&path));
        assert_eq!(import::presets(&root).unwrap(), std::slice::from_ref(&path), "the browser lists it");

        let p = SamplerParams::new();
        p.selection.write().unwrap().root = rack.root.clone();
        p.shared.queue_multi(path.to_string_lossy().into_owned());
        Load.run(&p);
        let loaded = p.selection.read().unwrap().clone();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(loaded.parts == [rack.parts[1].clone(), rack.parts[0].clone()], "every setting, in rack order");
        assert_eq!(loaded.order, [0, 1]);
        assert_eq!(loaded.multi, path.to_string_lossy());
        assert!(p.shared.view.lock().unwrap().multi_status.starts_with("Evening · 2 instruments"));
    }
    #[test]
    fn failed_load_keeps_its_stage_and_reason_for_the_info_tab() {
        let p = SamplerParams::new();
        p.selection.write().unwrap().parts = vec![Part { path: "/missing-kontra-test/instrument.nki".into(), ..Default::default() }];
        Load.run(&p);
        let view = p.shared.view.lock().unwrap();
        let report = view.parts[0].load_report.as_ref().expect("failed loads have reports too");
        assert_eq!(report["status"], "failed");
        assert_eq!(report["issues"][0]["stage"], "import");
        assert!(!report["issues"][0]["message"].as_str().unwrap().is_empty());
        assert!(report["log_path"].as_str().is_some());
    }

    #[test]
    fn delayed_script_restore_rejects_replaced_source_and_newer_saved_state() {
        use crate::ksp::Value;
        use std::sync::Barrier;
        let path=std::env::temp_dir().join(format!("kontra-zone-restore-{}.wav",std::process::id()));
        let mut wav=hound::WavWriter::create(&path,hound::WavSpec {channels:2,sample_rate:48000,
            bits_per_sample:32,sample_format:hound::SampleFormat::Float}).unwrap();
        for _ in 0..2048 {wav.write_sample(0.125f32).unwrap();wav.write_sample(0.125f32).unwrap();}wav.finalize().unwrap();
        let cache=crate::fx::DelayState {rack:crate::fx::Rack::Insert,slot:0,
            values:[3.0,1.0,123.0,2.0],legacy:false};
        let mut delay_bytes:Vec<u8>=[3.0f32,0.0,0.0,0.5,1.0,123.0,2.0]
            .into_iter().flat_map(f32::to_le_bytes).collect();delay_bytes.push(0);
        for with_zone_bank in [false,true] {
        let init=if with_zone_bank {"set_snapshot_type(3)\nset_zone_par(0,$ZONE_PAR_GROUP,0)\nset_zone_par(0,$ZONE_PAR_LOW_KEY,60)\nset_zone_par(0,$ZONE_PAR_HIGH_KEY,60)\n"}else{""};
        let instrument = Arc::new(Instrument { path: "/virtual/restore-freshness.nki".into(),
            groups:if with_zone_bank {vec![crate::import::Group::default()]}else{Vec::new()},
            zones:if with_zone_bank {vec![crate::import::Zone {sample:path.clone(),..Default::default()}]}else{Vec::new()},
            scripts: vec![format!("on init\n{init}make_perfview\ndeclare ui_slider $saved(0,100)\nmake_persistent($saved)\n$saved := 1\ndeclare ui_label $delay(1,1)\nset_text($delay,get_engine_par_disp($ENGINE_PAR_DL_TIME,-1,0,$NI_INSERT_BUS))\nend on")],
            fx:crate::fx::ProgramFx {insert:crate::fx::Chain {slots:vec![crate::fx::Effect {
                slot:0,kind:crate::fx::Kind::Delay,version:0,bypass:false,output_gain:1.0,dry_level:0.0,
                params:crate::fx::params::parse(crate::fx::Kind::Delay,&delay_bytes),
            }]},..Default::default()},
            ..Default::default() });
        let (rt, _, errors) = scripts_with_delays(&instrument, "", &[], &[], &[cache],48000.);
        assert!(errors.is_empty(), "{errors:?}");
        let mut rt = rt.unwrap();
        let initial = serde_json::to_string(&rt.persistence()).unwrap();
        rt.ui_control(&mut crate::ksp::LogEngine::default(), 0, 0, 7);
        let restoring = serde_json::to_string(&rt.persistence()).unwrap();
        rt.ui_control(&mut crate::ksp::LogEngine::default(), 0, 0, 9);
        let newer = serde_json::to_string(&rt.persistence()).unwrap();
        // The last case changes only a rack gain: it must not cancel a valid
        // restore whose source and saved script/IR/native state still match.
        for change in 0..6 {
            let p = Arc::new(SamplerParams::new());
            let part = Part { path: instrument.path.to_string_lossy().into_owned(),
                script_state: restoring.clone(),delay_state:vec![cache], ..Default::default() };
            let streaming = part.streaming(p.selection.read().unwrap().streaming);
            p.selection.write().unwrap().parts = vec![part.clone()];
            let initial_epoch = {
                let mut view = p.shared.view.lock().unwrap();
                for v in &mut view.parts { v.attempted = Some(Part::default().source()); v.streaming = streaming; }
                let epoch = next_epoch(&mut view, 0, None, None);
                let v = &mut view.parts[0];
                v.attempted = Some(part.source()); v.instrument = Some(instrument.clone());
                v.fx_rate = 48000.; v.script_state = initial.clone();
                v.delay_state=part.delay_state.clone().into();
                epoch
            };
            let generation = p.shared.part(0).unwrap().generation.load(Ordering::Acquire);
            let gate = Arc::new(Barrier::new(2));
            *p.shared.restore_gate.lock().unwrap() = Some((0, gate.clone()));
            let worker = { let p = p.clone(); std::thread::spawn(move || Load.run(&p)) };
            gate.wait();
            assert!(p.shared.view.try_lock().is_ok(), "restore preparation cannot hold the editor lock");
            match change {
                0 => p.selection.write().unwrap().parts[0].path = "/virtual/replacement.nki".into(),
                1 => p.selection.write().unwrap().parts[0].script_state = newer.clone(),
                2 => { p.shared.part(0).unwrap().generation.fetch_add(1, Ordering::AcqRel); }
                3 => p.selection.write().unwrap().parts.clear(),
                4 => p.selection.write().unwrap().parts[0].delay_state[0]=crate::fx::DelayState {
                    rack: crate::fx::Rack::Insert, slot: 0, values: [10.0, 1.0, 1000.0, 3.0], legacy: false,
                },
                _ => p.selection.write().unwrap().parts[0].gain = -6.,
            }
            gate.wait(); worker.join().unwrap();
            let mut restored = None;
            while let Some((slot, handoff_generation, handoff)) = p.shared.ready.pop() {
                if slot == 0 && let Handoff::Script { script, bank, epoch } = handoff { restored = Some((handoff_generation, script, bank, epoch)); }
            }
            let view = p.shared.view.lock().unwrap();
            let v = &view.parts[0];
            if change < 5 {
                assert!(restored.is_none(), "stale restore cannot enter the callback queue ({change})");
                assert_eq!(v.script_epoch, initial_epoch, "stale preparation cannot acquire a fresh epoch");
                assert_eq!(v.script_state, initial, "stale preparation cannot replace published state");
                assert_eq!(v.bytes,0,"stale paired bank cannot replace the published resident count");
                assert_eq!(v.delay_state.as_ref(),&[cache],"stale preparation cannot replace published physical caches");
            } else {
                let (handoff_generation, script, bank, epoch) = restored.expect("unrelated gain preserves the pending restore");
                assert_eq!(handoff_generation, generation); assert_ne!(epoch, initial_epoch);
                let script=script.unwrap();
                assert_eq!(script.interface(0).controls[0].properties["$CONTROL_PAR_VALUE"], Value::Int(7));
                assert_eq!(script.interface(0).controls[1].properties["$CONTROL_PAR_TEXT"],Value::Text("3".into()),"paired runtime init reads its restored physical Delay state");
                assert_eq!(script.native_state.snapshot().saved_delays(),vec![cache]);
                assert_eq!(v.script_state, restoring);
                assert_eq!(bank.is_some(),with_zone_bank,"prepared bank and restored runtime publish together");
                if let Some(bank)=bank {
                    use crate::ksp::engine::ZonePar;
                    assert_eq!(bank.zone_par(0,ZonePar::Group),Some(0));
                    assert_eq!(bank.zone_par(0,ZonePar::LowKey),Some(60));
                    assert_eq!(bank.zone_par(0,ZonePar::HighKey),Some(60));
                }
            }
            if change == 1 { assert_eq!(p.selection.read().unwrap().parts[0].script_state, newer); }
            if change == 4 {assert_eq!(p.selection.read().unwrap().parts[0].delay_state[0].values,[10.0,1.0,1000.0,3.0],"newer host cache survives rejection of the paired restore");}
        }
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn delay_physical_caches_survive_host_binary_json_worker_restore_and_unit_switch_without_heap() {
        use crate::fx::{Chain, Effect, Kind, Params, Rack, FxParam};
        use crate::ksp::Value;
        use moose::core::custom_state::State;
        let mut bytes: Vec<u8> = [3.0f32, 0.0, 0.0, 0.5, 1.0, 123.0, 2.0]
            .into_iter().flat_map(f32::to_le_bytes).collect(); bytes.push(0);
        let i = Arc::new(Instrument { path: "/virtual/Delay.nki".into(), scripts: vec![r#"on init
 declare ui_slider $go(0,2)
 declare ui_label $read(1,1)
 set_text($read,get_engine_par_disp($ENGINE_PAR_DL_TIME,-1,0,$NI_INSERT_BUS))
end on
on ui_control($go)
 if ($go = 1)
  set_engine_par($ENGINE_PAR_DL_TIME_UNIT,$NI_SYNC_UNIT_ABS,-1,0,$NI_INSERT_BUS)
  set_engine_par($ENGINE_PAR_DL_TIME,823567,-1,0,$NI_INSERT_BUS)
  set_engine_par($ENGINE_PAR_DL_TIME_UNIT,$NI_SYNC_UNIT_QUARTER,-1,0,$NI_INSERT_BUS)
  set_engine_par($ENGINE_PAR_DL_TIME,818182,-1,0,$NI_INSERT_BUS)
 else
  set_engine_par($ENGINE_PAR_DL_TIME_UNIT,$NI_SYNC_UNIT_ABS,-1,0,$NI_INSERT_BUS)
  set_text($read,get_engine_par_disp($ENGINE_PAR_DL_TIME,-1,0,$NI_INSERT_BUS))
 end if
end on"#.into()], fx: crate::fx::ProgramFx { insert: Chain { slots: vec![Effect {
            slot: 0, kind: Kind::Delay, version: 0, bypass: false, output_gain: 1.0,
            dry_level: 0.0, params: crate::fx::params::parse(Kind::Delay, &bytes),
        }] }, ..Default::default() }, ..Default::default() });
        let (rt, snapshot, errors) = scripts_with_delays(&i, "", &[], &[], &[], 48000.0);
        assert!(errors.is_empty(), "{errors:?}");
        let mut snapshot = snapshot.unwrap();
        let mut e = Engine::default();
        e.set_bank(Some(Box::new(Bank::from_samples(Vec::new(), Vec::new(), Vec::new()).unwrap())));
        e.set_fx(crate::engine::effects(&i, rt.as_deref(), 48000.0)); e.set_script(rt);
        let mut l=[0.0;128]; let mut r=[0.0;128];
        assert_eq!(allocations(|| {
            e.ui_control(0,0,1); e.render(&mut l,&mut r);
            while !e.script().unwrap().native_state.refresh(&mut snapshot.native,1) {}
        }),0);
        assert!(snapshot.native.changed);
        let delays=snapshot.native.saved_delays();
        assert_eq!(delays.len(),1,"only the successfully changed real slot is saved");
        assert!((delays[0].values[2]-1000.0).abs()<0.01);
        assert_eq!(delays[0].values[0],10.0);
        let p=SamplerParams::new();
        let part=Part { path:i.path.to_string_lossy().into_owned(), ..Default::default() };
        p.selection.write().unwrap().parts=vec![part.clone()];
        {
            let mut view=p.shared.view.lock().unwrap(); let v=&mut view.parts[0];
            v.instrument=Some(i.clone()); v.fx_rate=48000.0; v.script_epoch=1;
            v.attempted=Some(part.source());
        }
        p.shared.snapshots.push((0,1,snapshot,true)).ok().unwrap(); Load.run(&p);
        let saved=p.selection.read().unwrap().parts[0].clone();
        assert_eq!(saved.delay_state,delays,"real snapshot worker publishes physical state");
        let binary=State::serialize(&saved);
        let read=Part::deserialize(&binary).unwrap();
        assert!(read==saved,"host binary keeps appended physical state");
        let count=u32::from_le_bytes(binary[4..8].try_into().unwrap());
        let mut at=8; let mut older=binary[..8].to_vec();
        let mut positional=(count-3).to_le_bytes().to_vec();
        for _ in 0..count-3 {
            let len=u32::from_le_bytes(binary[at+4..at+8].try_into().unwrap()) as usize;
            older.extend(&binary[at..at+8+len]); positional.extend(&binary[at+4..at+8+len]); at+=8+len;
        }
        older[4..8].copy_from_slice(&(count-3).to_le_bytes());
        let mut old_expected=saved.clone(); old_expected.delay_state.clear(); old_expected.uvi=None; old_expected.uvi_state.clear();
        assert!(Part::deserialize(&older)==Some(old_expected.clone()),"older keyed state defaults new caches");
        assert!(Part::deserialize(&positional)==Some(old_expected),"older positional state retains prior fields");
        let saved:Part=serde_json::from_str(&serde_json::to_string(&read).unwrap()).unwrap();
        let (rt,_,errors)=scripts_with_delays(&i,&saved.script_state,&saved.ir_settings,&saved.engine_state,&saved.delay_state,48000.0);
        assert!(errors.is_empty(),"{errors:?}");
        let mut restored=Engine::default();
        restored.set_bank(Some(Box::new(Bank::from_samples(Vec::new(),Vec::new(),Vec::new()).unwrap())));
        restored.set_fx(crate::engine::effects(&i,rt.as_deref(),48000.0)); restored.set_script(rt);
        assert_eq!(restored.script().unwrap().interface(0).controls[1].properties["$CONTROL_PAR_TEXT"],Value::Text("10".into()),"init reads restored synchronized state");
        assert_eq!(allocations(|| {
            restored.ui_control(0,0,2); restored.render(&mut l,&mut r);
        }),0);
        assert_eq!(restored.script().unwrap().interface(0).controls[1].properties["$CONTROL_PAR_TEXT"],Value::Text("1000.0".into()));
        let time=FxParam::Field(Kind::Delay,0);
        assert!((restored.fx().param(Rack::Insert,0,time).unwrap()-823567.0/1e6).abs()<1e-6);
        // Rate/effect rebuilds use the same retained physical state, preserving
        // caches more recently changed than the last worker snapshot.
        let rebuilt=crate::engine::effects(&i,restored.script(),48000.0);
        let mut retired=None;
        assert_eq!(allocations(|| { retired=Some(restored.set_fx(rebuilt)); }),0);
        drop(retired);
        assert!((restored.fx().param(Rack::Insert,0,time).unwrap()-823567.0/1e6).abs()<1e-6);
        let legacy=serde_json::from_str::<Part>("{}").unwrap(); assert!(legacy.delay_state.is_empty());
        let (legacy_rt,_,errors)=scripts(&i,"",&[],&[],48000.0);
        assert!(errors.is_empty()); assert!(legacy_rt.unwrap().native_state.snapshot().saved_delays().is_empty());
        let mut selected=saved.clone(); selected.select_snapshot("new.nksn".into());
        assert!(selected.delay_state.is_empty());
        let mut wrong=saved.delay_state[0]; wrong.slot=7;
        let (wrong_rt,_,errors)=scripts_with_delays(&i,"",&[],&[],&[wrong],48000.0);
        assert_eq!(errors.len(),1); assert!(wrong_rt.unwrap().native_state.snapshot().saved_delays().is_empty());
        let mut other=Instrument { path:i.path.clone(), scripts:i.scripts.clone(), ..Default::default() };
        other.fx.insert.slots.push(Effect { slot:0,kind:Kind::Gainer,version:0,bypass:false,output_gain:1.0,dry_level:0.0,params:Params::Gainer(crate::fx::params::Gainer{gain:1.0}) });
        let (wrong_rt,_,errors)=scripts_with_delays(&other,"",&[],&[],&saved.delay_state,48000.0);
        assert_eq!(errors.len(),1); assert!(wrong_rt.unwrap().native_state.snapshot().saved_delays().is_empty());
    }

    #[test]
    fn native_engine_edits_restore_authored_readback_without_audio_allocations() {
        use crate::fx::{Chain, Effect, Kind, Params, FxParam, Rack};
        use crate::ksp::{EnginePar, Value};
        let effect = |slot, kind, params, bypass| Effect { slot, kind, params, bypass,
            version: 0, output_gain: 1., dry_level: 0. };
        let mut i = Instrument { path: "/virtual/native-engine-state.nki".into(), scripts: vec![r#"on init
make_perfview
declare ui_slider $amount(0,500000)
declare ui_switch $on
make_persistent($on)
$amount := get_engine_par($ENGINE_PAR_SENDLEVEL_0,-1,7,0)
end on
on persistence_changed
$amount := get_engine_par($ENGINE_PAR_SENDLEVEL_0,-1,7,0)
end on
on ui_control($amount)
set_engine_par($ENGINE_PAR_SENDLEVEL_0,$amount,-1,7,0)
end on
on ui_control($on)
set_engine_par($ENGINE_PAR_SEND_EFFECT_BYPASS,1-$on,-1,7,$NI_INSERT_BUS)
{ A send tap has no DSP dry-level setter: it must not be persisted. }
set_engine_par($ENGINE_PAR_SEND_EFFECT_DRY_LEVEL,1000000,-1,7,$NI_INSERT_BUS)
end on"#.into()], ..Default::default() };
        i.fx.insert = Chain { slots: vec![effect(7, Kind::SendLevels,
            Params::SendLevels(crate::fx::params::SendLevels { sends: vec![0.0625;8], outputs: Vec::new() }), true)] };
        i.fx.send = Chain { slots: vec![effect(0, Kind::Reverb, Params::Reverb(crate::fx::params::Reverb::DEFAULT), false)] };
        let i = Arc::new(i);
        let (rt, snapshot, errors) = scripts(&i, "", &[], &[], 48000.);
        assert!(errors.is_empty(), "{errors:?}");
        let rt = rt.unwrap();
        assert!(rt.engine_state().is_empty(), "prepared defaults are not saved edits");
        let mut snapshot = snapshot.unwrap();
        let mut engine = Engine::default();
        engine.set_bank(Some(Box::new(Bank::from_samples(Vec::new(),Vec::new(),Vec::new()).unwrap())));
        engine.set_fx(crate::engine::effects(&i, Some(&rt), 48000.));
        engine.set_script(Some(rt));
        let mut left = [0.;64]; let mut right = [0.;64];
        assert_eq!(allocations(|| {
            engine.ui_control(0,1,1);
            engine.ui_control(0,0,125000);
            engine.render(&mut left,&mut right);
            let rt = engine.script().unwrap();
            let mut at = Refresh::default();
            while !rt.refresh_persistence_within(&mut snapshot.script,&mut at,1) {}
            while !rt.native_state.refresh(&mut snapshot.native,1) {}
        }),0);
        assert!(snapshot.native.changed);
        let expected_send = engine.fx().param(Rack::Insert,7,FxParam::SendLevel(0)).unwrap();
        assert_eq!(engine.fx().param(Rack::Insert,7,FxParam::Bypass),Some(0.));
        let rebuilt = crate::engine::effects(&i,engine.script(),48000.);
        let mut retired = None;
        assert_eq!(allocations(|| { retired = Some(engine.set_fx(rebuilt)); }),0);
        drop(retired); // A worker disposes rebuilt/retired DSP allocations.
        assert_eq!(engine.fx().param(Rack::Insert,7,FxParam::SendLevel(0)),Some(expected_send));
        assert_eq!(engine.fx().param(Rack::Insert,7,FxParam::Bypass),Some(0.));
        let p = SamplerParams::new();
        {
            let mut view = p.shared.view.lock().unwrap(); let v = &mut view.parts[0];
            v.instrument = Some(i.clone()); v.fx_rate = 48000.; v.script_epoch = 1;
            v.attempted = Some((i.path.to_string_lossy().into_owned(),0,String::new()));
        }
        p.selection.write().unwrap().parts = vec![Part { path:i.path.to_string_lossy().into_owned(), ..Default::default() }];
        p.shared.snapshots.push((0,1,snapshot,true)).ok().unwrap();
        Load.run(&p);
        let saved = p.selection.read().unwrap().parts[0].clone();
        assert_eq!(saved.engine_state.len(),2,"only the successful changed send/bypass writes persist");
        use moose::core::custom_state::State;
        assert!(Part::deserialize(&State::serialize(&saved)) == Some(saved.clone()),"host binary state preserves native edits");
        let mut new_snapshot = saved.clone(); new_snapshot.select_snapshot("next.nksn".into());
        assert!(new_snapshot.engine_state.is_empty(),"explicit snapshot selection uses its own engine state");
        assert!(serde_json::from_str::<Part>("{}").unwrap().engine_state.is_empty(),"old states use authored defaults");
        let saved: Part = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        let (rt,_,errors) = scripts(&i,&saved.script_state,&saved.ir_settings,&saved.engine_state,48000.);
        assert!(errors.is_empty(),"{errors:?}"); let rt = rt.unwrap();
        assert_eq!(rt.interface(0).controls[0].properties["$CONTROL_PAR_VALUE"],Value::Int(125000));
        assert_eq!(rt.interface(0).controls[1].properties["$CONTROL_PAR_VALUE"],Value::Int(1));
        let mut restored = Engine::default(); restored.set_bank(Some(Box::new(Bank::from_samples(Vec::new(),Vec::new(),Vec::new()).unwrap()))); restored.set_fx(crate::engine::effects(&i,Some(&rt),48000.)); restored.set_script(Some(rt));
        assert_eq!(restored.fx().param(Rack::Insert,7,FxParam::SendLevel(0)),Some(expected_send));
        assert_eq!(restored.fx().param(Rack::Insert,7,FxParam::Bypass),Some(0.));
        // Unsupported foreign shape is reported and cannot seed another target.
        let bad = EnginePar { id:crate::engine::engine_par::SENDLEVEL_0, group:-1,slot:99,generic:0 };
        let (rt,errors) = crate::engine::load_scripts_with_state(&i,Vec::new(),48000.,&[],&[crate::ksp::engine::NativeEdit { par:bad,value:1000000 }]);
        assert_eq!(errors.len(),1); assert!(rt.unwrap().engine_state().is_empty());
    }

    #[test]
    #[ignore = "requires the owner's local Una Corda and Analog Strings libraries"]
    fn una_corda_space_native_state_round_trips_all_three_authored_patches() {
        use crate::fx::{Rack, FxParam};
        use crate::ksp::Value;
        for name in ["Pure", "Felt", "Cotton"] {
            let path = Path::new(import::LIBRARY_ROOT).join(format!("Una Corda Library/Instruments/Una Corda {name}.nki"));
            let i = import::read(&path).unwrap();
            let (rt, errors) = crate::engine::load_scripts(&i, i.script_state.clone(), 48000.);
            assert!(errors.is_empty(),"{name}: {errors:?}"); let rt = rt.unwrap();
            eprintln!("Una Corda {name} native edit capacity {:?}",rt.engine_state_capacity());
            let ui = rt.interface(0);
            let amount = ui.controls.iter().position(|c| c.variable == "$Mas_sliSpace").unwrap();
            let amount_fx = ui.controls.iter().position(|c| c.variable == "$T3_sliAmount").unwrap();
            let on = ui.controls.iter().position(|c| c.variable == "$Mas_swiSpace").unwrap();
            let mut engine = Engine::default();
            engine.set_bank(Some(Box::new(Bank::from_samples(i.groups.clone(),Vec::new(),Vec::new()).unwrap())));
            engine.set_fx(crate::engine::effects(&i,Some(&rt),48000.)); engine.set_script(Some(rt));
            assert_eq!(allocations(|| {
                engine.ui_control(0,on,1); engine.ui_control(0,amount,125000);
                engine.render(&mut [0.;128],&mut [0.;128]);
            }),0);
            let native = engine.script().unwrap().engine_state();
            assert_eq!(native.len(),2,"{name}: only Space send and bypass changed");
            let script = engine.script().unwrap().persistence();
            assert!(script.iter().all(|p| !p.contains_key("$Mas_sliSpace") && !p.contains_key("$T3_sliAmount")),"Space depends on native state, not persistent amount variables");
            let ir = i.fx.ir_settings_with(&engine.script().unwrap().init_irs);
            let saved = Part { engine_state:native, script_state:serde_json::to_string(&script).unwrap(), ir_settings:ir, ..Default::default() };
            let saved: Part = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
            let (rt,_,errors) = scripts(&i,&saved.script_state,&saved.ir_settings,&saved.engine_state,48000.);
            assert!(errors.is_empty(),"{name}: {errors:?}"); let rt = rt.unwrap();
            for uid in [amount,amount_fx] { assert_eq!(rt.interface(0).controls[uid].properties["$CONTROL_PAR_VALUE"],Value::Int(125000),"{name}: authored getter readback"); }
            assert_eq!(rt.interface(0).controls[on].properties["$CONTROL_PAR_VALUE"],Value::Int(1));
            let mut restored = Engine::default(); restored.set_fx(crate::engine::effects(&i,Some(&rt),48000.)); restored.set_script(Some(rt));
            for (rack,slot,par) in [(Rack::Insert,7,FxParam::SendLevel(0)),(Rack::Insert,7,FxParam::Bypass),(Rack::Send,0,FxParam::Wet),(Rack::Send,0,FxParam::Dry)] {
                assert_eq!(restored.fx().param(rack,slot,par),engine.fx().param(rack,slot,par),"{name}: DSP state");
            }
            let mut before = engine.set_fx(crate::fx::ProgramFx::default().processor(48000.,128));
            let mut after = restored.set_fx(crate::fx::ProgramFx::default().processor(48000.,128));
            before.clear(); after.clear(); let mut tail = 0f64;
            for block in 0..256 {
                let mut l = [0.;128]; let mut r = [0.;128];
                if block == 0 { l[0] = 0.01; r[0] = 0.01; }
                let mut l2 = l; let mut r2 = r;
                before.process(&mut l,&mut r); after.process(&mut l2,&mut r2);
                assert_eq!(l,l2,"{name}: restored impulse left block {block}"); assert_eq!(r,r2,"{name}: restored impulse right block {block}");
                if block != 0 { tail += l.iter().map(|x| (*x as f64).powi(2)).sum::<f64>(); }
            }
            assert!(tail > 0.,"{name}: restored Space remains audible");
        }
        let path = Path::new(import::LIBRARY_ROOT).join("ANALOG STRINGS/Instruments/ANALOG STRINGS.nki");
        let i = import::read(&path).unwrap();
        let (rt,errors) = crate::engine::load_scripts(&i,i.script_state.clone(),48000.);
        assert!(errors.is_empty(),"{errors:?}");
        let rt = rt.unwrap();
        let (count, preparation) = crate::engine::ScriptSetup::audit_native_preparation(&i,&rt);
        assert_eq!(count,rt.engine_state_capacity().0);
        eprintln!("Analog Strings native edit capacity {:?}, preparation {:?}",rt.engine_state_capacity(),preparation);
    }

    #[test]
    #[ignore = "requires the owner's local Una Corda library"]
    fn una_corda_menus_load_real_ir_files_after_init() {
        let path = Path::new(import::LIBRARY_ROOT).join("Una Corda Library/Instruments/Una Corda Cotton.nki");
        let i = Arc::new(import::read(&path).unwrap());
        let (rt, errors) = crate::engine::load_scripts(&i, i.script_state.clone(), 48000.);
        assert!(errors.is_empty(), "{errors:?}");
        let rt = rt.unwrap();
        let p = SamplerParams::new();
        {
            let mut view = p.shared.view.lock().unwrap();
            let v = &mut view.parts[0];
            v.instrument = Some(i.clone()); v.script_epoch = 1; v.fx_rate = 48000.; v.irs = rt.init_irs.clone();
        }
        let mut engine = Engine::default();
        engine.set_bank(Some(Box::new(Bank::from_samples(i.groups.clone(), Vec::new(), Vec::new()).unwrap())));
        engine.set_fx(crate::engine::effects(&i, Some(&rt), 48000.));
        engine.set_script(Some(rt));
        let mix = |e: &Engine| [crate::fx::FxParam::Wet, crate::fx::FxParam::Dry, crate::fx::FxParam::Bypass]
            .map(|par| e.fx().param(crate::fx::Rack::Send, 0, par));
        let saved_mix = mix(&engine);
        for (name, value, file) in [
            ("$T3_mnuType", 1, "GI_UC_IR_Vintage_EMT140_Dark.ncw"),
            ("$T3_mnuReverb", 1, "GI_UC_IR_Room_Intimate Chamber.ncw"),
        ] {
            let control = engine.script().unwrap().interface(0).controls.iter().position(|c| c.variable == name).unwrap();
            engine.ui_control(0, control, value);
            for _ in 0..4 { engine.render(&mut [0.; 128], &mut [0.; 128]); }
            let request = engine.pop_ir_request().expect("the actual menu queues a live IR load");
            assert_eq!(request.file(), file);
            println!("{name}={value} requests {}", request.file());
            p.shared.ir_requests.push((0, 0, 1, request)).ok().unwrap();
            assert!(engine.pop_ir_request().is_none());
            load_irs(&p);
            let (_, _, IrHandoff { ir, request, .. }) = p.shared.ir_ready.pop().unwrap();
            assert!(ir.is_some(), "the real NCW is decoded and its replacement DSP is built");
            let retired = engine.finish_ir(request.script_slot, request.id, ir);
            assert!(retired.is_some(), "the new DSP replaced the old one");
            let view = p.shared.view.lock().unwrap();
            let loaded = view.parts[0].irs.iter().find(|l| l.rack == crate::fx::Rack::Send && l.slot == 0 && matches!(l.load, crate::fx::Load::Ir { .. })).unwrap();
            let crate::fx::Load::Ir { file: loaded_file, ir: impulse } = &loaded.load else { panic!("IR") };
            assert_eq!(loaded_file.file_name().unwrap().to_string_lossy().to_ascii_lowercase(), file.to_ascii_lowercase());
            println!("installed {} ({} frames at {} Hz)", loaded_file.display(), impulse.0.frames.len(), impulse.0.rate);
            assert_eq!(mix(&engine), saved_mix, "switching spaces preserves live return settings");
        }
        for (name, value, field, expected) in [
            ("$T3_sliSize", 0, 2, 0),
            ("$T3_sliSize", 500000, 2, 500000),
            ("$T3_sliSize", 1000000, 2, 1000000),
            ("$T3_sliDistance", 5722 * 5, 0, 250000),
            ("$T3_sliDistance", 5722 * 40, 0, 606445),
            ("$T3_sliDistance", 5722 * 100, 0, 783203),
        ] {
            let control = engine.script().unwrap().interface(0).controls.iter().position(|c| c.variable == name).unwrap();
            engine.ui_control(0, control, value);
            for _ in 0..4 { engine.render(&mut [0.; 128], &mut [0.; 128]); }
            let request = engine.pop_ir_request().expect("the actual knob queues a settings rebuild");
            assert!(request.id < 0 && request.file().is_empty());
            assert_eq!((request.settings.values[field] * 1e6).round() as i32, expected);
            println!("{name}={value}: requested engine value {expected}");
            p.shared.ir_requests.push((0, 0, 1, request)).ok().unwrap();
            assert!(engine.pop_ir_request().is_none());
            load_irs(&p);
            let (_, _, IrHandoff { ir, request, .. }) = p.shared.ir_ready.pop().unwrap();
            assert!(ir.is_some());
            assert!(engine.finish_ir(request.script_slot, request.id, ir).is_some());
            assert_eq!((engine.fx().param(crate::fx::Rack::Send, 0, crate::fx::FxParam::Convolution(field as u8)).unwrap() * 1e6).round() as i32, expected);
            assert_eq!(mix(&engine), saved_mix);
            let view = p.shared.view.lock().unwrap();
            assert!(view.parts[0].irs.iter().any(|l| matches!(&l.load, crate::fx::Load::Ir { file, .. } if file.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case("GI_UC_IR_Room_Intimate Chamber.ncw"))));
        }
        let notes = engine.script().unwrap().diagnostics();
        println!("remaining diagnostics: {notes:?}");
        assert!(!notes.iter().any(|n| n.contains("load_ir_sample")), "{notes:?}");
        assert!(!notes.iter().any(|n| n.contains("parameter not implemented")), "{notes:?}");
        let initial = Part { path: path.to_string_lossy().into_owned(), ..Default::default() };
        let streaming = initial.streaming(p.selection.read().unwrap().streaming);
        p.selection.write().unwrap().parts = vec![initial];
        {
            let mut view = p.shared.view.lock().unwrap();
            view.parts[0].attempted = Some((path.to_string_lossy().into_owned(), 0, String::new()));
            view.parts[0].streaming = streaming;
        }
        p.shared.snapshots.push((0, 1, Box::new(PersistenceSnapshot {
            script: engine.script().unwrap().persistence(),
            ir: vec![crate::fx::IrSlotSettings { rack: crate::fx::Rack::Send, slot: 0, settings: engine.fx().ir_settings(crate::fx::Rack::Send, 0).unwrap(), file: None }],
            native: engine.script().unwrap().native_state.snapshot(),
        }), true)).ok().unwrap();
        Load.run(&p);
        let part = p.selection.read().unwrap().parts[0].clone();
        assert_eq!(part.ir_settings.len(), 1);
        assert!(part.ir_settings[0].file.as_ref().unwrap().file_name().unwrap().to_string_lossy().eq_ignore_ascii_case("GI_UC_IR_Room_Intimate Chamber.ncw"));
        // Both DAW custom state and multi/export JSON preserve the typed field.
        let mut bytes = Vec::new();
        part.write_field(&mut bytes);
        let saved = Part::read_field(&mut moose::core::custom_state::StateCursor::new(&bytes)).unwrap();
        assert!(saved == part);
        let saved: Part = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert!(serde_json::from_str::<Part>("{}").unwrap().ir_settings.is_empty());
        let (restored, _, errors) = scripts(&i, &saved.script_state, &saved.ir_settings, &saved.engine_state, 44100.);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(restored.as_ref().unwrap().init_irs.iter().any(|l| matches!(&l.load, crate::fx::Load::Ir { file, .. } if file.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case("GI_UC_IR_Room_Intimate Chamber.ncw"))), "reload retains the selected room's actual impulse response");
        let fx = crate::engine::effects(&i, restored.as_deref(), 44100.);
        engine.reset(44100.);
        engine.set_fx(fx);
        engine.set_script(restored);
        for _ in 0..4 { engine.render(&mut [0.; 128], &mut [0.; 128]); }
        let restored = engine.fx().ir_settings(crate::fx::Rack::Send, 0).unwrap();
        assert_eq!((restored.values[2] * 1e6).round() as i32, 1000000);
        assert_eq!((restored.values[0] * 1e6).round() as i32, 783203);
        println!("Size and Distance restored from DAW/multi state at 44100 Hz");
    }

    #[test]
    fn zone_remaps_install_real_playback_before_async_resume_and_reject_stale_jobs_without_heap() {
        use crate::{audio::Sample, import::{Group, Zone, Wavetable}, ksp::engine::ZonePar};
        let groups = vec![Group { source_mode: Some(9), wavetable: Some(Wavetable::default()), ..Group::default() }, Group::default()];
        let path=std::env::temp_dir().join(format!("kontra-zone-remap-{}.wav",std::process::id()));
        let zones = vec![Zone {sample:path.clone(),group:1,low_key:55,high_key:55,..Zone::default()},
            Zone {sample:path.clone(),group:1,low_key:59,high_key:59,..Zone::default()}];
        let frames: Vec<_> = (0..4096).map(|i|[(std::f32::consts::TAU*(i%2048) as f32/2048.).sin()*0.125;2]).collect();
        let mut wav=Vec::new();
        wav.extend_from_slice(b"RIFF");wav.extend_from_slice(&(36u32+frames.len() as u32*8).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&3u16.to_le_bytes());wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&48000u32.to_le_bytes());wav.extend_from_slice(&384000u32.to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());wav.extend_from_slice(&32u16.to_le_bytes());
        wav.extend_from_slice(b"data");wav.extend_from_slice(&(frames.len() as u32*8).to_le_bytes());
        for frame in &frames {for value in frame {wav.extend_from_slice(&value.to_le_bytes());}}
        std::fs::write(&path,wav).unwrap();
        let proof=std::env::var_os("KONTRA_ZONE_PROOF_DIR").map(std::path::PathBuf::from);
        if let Some(dir)=&proof {std::fs::create_dir_all(dir).unwrap();std::fs::copy(&path,dir.join("authored-source.wav")).unwrap();}
        let save=|name:&str,l:&[f32],r:&[f32]| {
            if let Some(dir)=&proof {
                let mut out=hound::WavWriter::create(dir.join(name),hound::WavSpec {channels:2,sample_rate:48000,bits_per_sample:32,sample_format:hound::SampleFormat::Float}).unwrap();
                for (&l,&r) in l.iter().zip(r) {out.write_sample(l).unwrap();out.write_sample(r).unwrap();}out.finalize().unwrap();
            }
        };
        let bank = |zones: Vec<Zone>| Bank::from_samples(groups.clone(),zones,
            vec![(path.clone(),Sample {rate:48000,frames:frames.clone()})]).unwrap();
        let script = r#"on init
set_snapshot_type(3)
declare ui_switch $select
declare ui_label $status(1,1)
declare %jobs[3]
declare $done
declare $resumed
declare $success
make_persistent($done)
make_persistent($resumed)
make_persistent($success)
end on
on ui_control($select)
%jobs[0] := set_zone_par(1,$ZONE_PAR_GROUP,0)
%jobs[1] := set_zone_par(1,$ZONE_PAR_LOW_KEY,60)
%jobs[2] := set_zone_par(1,$ZONE_PAR_HIGH_KEY,60)
set_text($status,"pending")
wait_async(%jobs[0])
wait_async(%jobs[1])
wait_async(%jobs[2])
inc($resumed)
set_text($status,get_zone_par(1,$ZONE_PAR_GROUP) & ":" & get_zone_par(1,$ZONE_PAR_LOW_KEY) & ":" & get_zone_par(1,$ZONE_PAR_HIGH_KEY))
end on
on async_complete
inc($done)
$success := $success+$NI_ASYNC_EXIT_STATUS
end on"#;
        let i = Instrument { groups:groups.clone(),zones:zones.clone(),scripts:vec![script.into()],..Instrument::default() };
        let (rt,errors) = crate::engine::load_scripts(&i,Vec::new(),48000.);
        assert!(errors.is_empty(),"{errors:?}");
        let p = SamplerParams::default();
        let mut s = Dsp::default();
        s.script_epoch[0]=7;
        s.installed_generation[0]=0;
        { let mut v=p.shared.view.lock().unwrap();v.parts[0].instrument=Some(Arc::new(i));v.parts[0].script_epoch=7; }
        let source=p.shared.view.lock().unwrap().parts[0].instrument.clone().unwrap();
        s.rack.parts[0].set_bank(Some(Box::new(Bank::load(&source).unwrap())));
        s.rack.parts[0].set_script(rt);
        let mut old_reference=Engine::default();old_reference.set_bank(Some(Box::new(bank(zones.clone()))));
        s.rack.parts[0].note_on(0,59,100);old_reference.note_on(0,59,100);
        s.rack.parts[0].render(&mut [0.;128],&mut [0.;128]);old_reference.render(&mut [0.;128],&mut [0.;128]);
        let scalar=|e:&Engine,name:&str|e.script().unwrap().persistence()[0][name].clone();
        assert_eq!(allocations(||s.rack.parts[0].ui_control(0,0,1)),0);
        assert_eq!(scalar(&s.rack.parts[0],"$resumed"),crate::ksp::Value::Int(0));
        assert_eq!(s.rack.parts[0].bank().unwrap().zone_par(1,ZonePar::Group),Some(1));
        assert_eq!(allocations(|| {
            while let Some(job)=s.rack.parts[0].pop_zone_job() { p.shared.zone_requests.push((0,0,7,job)).ok().unwrap(); }
        }),0);
        assert_eq!(p.shared.zone_requests.len(),3);
        let mut ram_fill=Box::new(Bank::load(&source).unwrap());
        assert!(!s.rack.parts[0].zone_upgrade_ready(&ram_fill),"pending writes postpone a baseline RAM fill");
        load_zone_maps(&p); // real serialized preparation, outside the audio allocation gate
        assert_eq!(scalar(&s.rack.parts[0],"$done"),crate::ksp::Value::Int(0),"preparation alone cannot complete IDs");
        assert_eq!(allocations(||finish_zone_maps(&mut s,&p)),0);
        let e=&s.rack.parts[0];
        assert_eq!(scalar(e,"$done"),crate::ksp::Value::Int(3));
        assert_eq!(scalar(e,"$success"),crate::ksp::Value::Int(3));
        assert_eq!(scalar(e,"$resumed"),crate::ksp::Value::Int(1));
        assert_eq!(e.script().unwrap().interface(0).controls[1].properties["$CONTROL_PAR_TEXT"],crate::ksp::Value::Text("0:60:60".into()));
        for (par,value) in [(ZonePar::Group,0),(ZonePar::LowKey,60),(ZonePar::HighKey,60)] {assert_eq!(e.bank().unwrap().zone_par(1,par),Some(value));}
        assert!(e.script().unwrap().diagnostics().is_empty(),"{:?}",e.script().unwrap().diagnostics());
        // Independent reference: old sampler keeps playing; a separately
        // authored WT mapping contributes the new oscillator at MIDI60.
        let mut mapped=zones.clone();mapped[1].group=0;mapped[1].low_key=60;mapped[1].high_key=60;
        let mut new_reference=Engine::default();new_reference.set_bank(Some(Box::new(bank(mapped))));
        let (mut al,mut ar,mut oldl,mut oldr,mut newl,mut newr)=([0.;2048],[0.;2048],[0.;2048],[0.;2048],[0.;2048],[0.;2048]);
        assert_eq!(allocations(|| {
            s.rack.parts[0].note_on(0,60,100);new_reference.note_on(0,60,100);
            s.rack.parts[0].render(&mut al,&mut ar);old_reference.render(&mut oldl,&mut oldr);new_reference.render(&mut newl,&mut newr);
        }),0);
        assert!(newl.iter().any(|v|v.abs()>0.01),"remapped WT really produces PCM");
        assert!(al.iter().zip(oldl.iter().zip(&newl)).chain(ar.iter().zip(oldr.iter().zip(&newr))).all(|(a,(old,new))|(a-old-new).abs()<1e-5),"new WT mapping plus untouched old sampler voice matches independent reference");
        assert!(s.rack.parts[0].voice_census().iter().any(|v|v.group==0 && v.wavetable.is_some()));
        save("remapped-actual.wav",&al,&ar);save("old-sampler-reference.wav",&oldl,&oldr);save("new-wavetable-reference.wav",&newl,&newr);
        let expected_l:Vec<_>=oldl.iter().zip(&newl).map(|(a,b)|a+b).collect();let expected_r:Vec<_>=oldr.iter().zip(&newr).map(|(a,b)|a+b).collect();
        save("remapped-independent-reference.wav",&expected_l,&expected_r);
        println!("mapped readback=0:60:60, waits=3, successful IDs=3; old sampler + new WT PCM matches independent sum");
        // A RAM fill built from original source mappings must be rebased,
        // retaining both mapped keys and the already playing WT window.
        { let chains=p.shared.zone_chains.lock().unwrap();let chain=chains.last().unwrap();
          ram_fill.rebase_zone_preload(&chain.context.upgrade().unwrap(),chain.state.clone(),&||false).unwrap(); }
        assert!(s.rack.parts[0].zone_upgrade_ready(&ram_fill));
        let mut old_bank=None;
        assert_eq!(allocations(|| {
            old_bank=s.rack.parts[0].upgrade_bank(ram_fill);
            s.rack.parts[0].render(&mut al,&mut ar);old_reference.render(&mut oldl,&mut oldr);new_reference.render(&mut newl,&mut newr);
        }),0);
        assert!(old_bank.is_some());drop(old_bank);
        assert_eq!(s.rack.parts[0].bank().unwrap().zone_par(1,ZonePar::LowKey),Some(60));
        assert!(al.iter().zip(oldl.iter().zip(&newl)).chain(ar.iter().zip(oldr.iter().zip(&newr))).all(|(a,(old,new))|(a-old-new).abs()<1e-5),"RAM fill retains remapped playback and active voice phase");
        save("upgraded-actual.wav",&al,&ar);
        let expected_l:Vec<_>=oldl.iter().zip(&newl).map(|(a,b)|a+b).collect();let expected_r:Vec<_>=oldr.iter().zip(&newr).map(|(a,b)|a+b).collect();save("upgraded-independent-reference.wav",&expected_l,&expected_r);
        // Also prepare a real zero-head sampler bank into a complete resident
        // WT table: the file decoder, not a synthetic silence fallback, runs.
        let bare=Bank::load_bare(&source).unwrap();
        let jobs=[(ZonePar::Group,0),(ZonePar::LowKey,60),(ZonePar::HighKey,60)].into_iter().enumerate().map(|(id,(par,value))|
            bare.zone_job(crate::ksp::engine::ZoneEdit {zone:1,par,value,slot:0,id:id as i32}).unwrap()).collect::<Vec<_>>();
        let base=jobs[0].base.clone();let mut prepared=crate::engine::zone::Prepared::build(jobs,base,&||false);
        assert!(prepared.success,"{:?}",prepared.error);
        let mut bare=bare;assert!(bare.install_zone_map(&mut prepared));
        let mut bare_play=Engine::default();bare_play.set_bank(Some(Box::new(bare)));
        let mut fresh_reference=Engine::default();let mut mapped=zones.clone();mapped[1].group=0;mapped[1].low_key=60;mapped[1].high_key=60;
        fresh_reference.set_bank(Some(Box::new(bank(mapped))));
        assert_eq!(allocations(|| {bare_play.note_on(0,60,100);fresh_reference.note_on(0,60,100);
            bare_play.render(&mut al,&mut ar);fresh_reference.render(&mut newl,&mut newr);}),0);
        assert!(al.iter().zip(&newl).chain(ar.iter().zip(&newr)).all(|(a,b)|(a-b).abs()<1e-5));
        save("zero-head-wavetable-actual.wav",&al,&ar);save("zero-head-independent-reference.wav",&newl,&newr);
        // Sustain belongs to the note's channel for both source families.
        let mut mapped=zones.clone();mapped[1].group=0;mapped[1].low_key=60;mapped[1].high_key=60;
        let mut channels=Engine::default();channels.set_bank(Some(Box::new(bank(mapped))));
        assert_eq!(allocations(|| {
            channels.note_on(1,60,100);channels.note_on(2,55,100);
            channels.cc(1,64,127);channels.cc(2,64,127);
            channels.note_off(1,60);channels.note_off(2,55);
            channels.render(&mut [0.;32],&mut [0.;32]);
        }),0);
        let voices=channels.voice_census();assert_eq!(voices.len(),2);assert!(voices.iter().all(|v|!v.released));
        assert!(voices.iter().any(|v|v.channel==1 && v.group==0 && v.wavetable.is_some()));
        assert!(voices.iter().any(|v|v.channel==2 && v.group==1 && v.wavetable.is_none()));
        assert_eq!(allocations(|| {channels.cc(1,64,0);channels.render(&mut [0.;32],&mut [0.;32]);}),0);
        let voices=channels.voice_census();assert!(voices.iter().any(|v|v.channel==1 && v.released));assert!(voices.iter().any(|v|v.channel==2 && !v.released));
        assert_eq!(allocations(|| {channels.cc(2,64,0);channels.render(&mut [0.;32],&mut [0.;32]);}),0);
        assert!(channels.voice_census().iter().all(|v|v.released));
        println!("normal + WT channel ownership/sustain isolation: PASS; all audio allocation gates=0");
        // A new source bank may share dimensions, but an old prepared job
        // must not edit it or claim successful completion.
        while p.shared.discard.pop().is_some() {}
        assert_eq!(allocations(||s.rack.parts[0].ui_control(0,0,0)),0);
        while let Some(job)=s.rack.parts[0].pop_zone_job() {p.shared.zone_requests.push((0,0,7,job)).ok().unwrap();}
        load_zone_maps(&p);
        let replacement=Box::new(bank(zones.clone()));
        let mut retired=None;
        assert_eq!(allocations(|| {retired=s.rack.parts[0].set_bank(Some(replacement));finish_zone_maps(&mut s,&p);}),0);
        assert_eq!(s.rack.parts[0].bank().unwrap().zone_par(1,ZonePar::Group),Some(1));
        assert_eq!(scalar(&s.rack.parts[0],"$done"),crate::ksp::Value::Int(6));
        assert_eq!(scalar(&s.rack.parts[0],"$success"),crate::ksp::Value::Int(3),"stale source completions fail");
        drop(retired);
        // Runtime epoch replacement suppresses delivery entirely.
        while p.shared.discard.pop().is_some() {}
        s.rack.parts[0].ui_control(0,0,1);
        while let Some(job)=s.rack.parts[0].pop_zone_job(){p.shared.zone_requests.push((0,0,7,job)).ok().unwrap();}
        load_zone_maps(&p);
        s.script_epoch[0]=8;
        assert_eq!(allocations(||finish_zone_maps(&mut s,&p)),0);
        assert_eq!(scalar(&s.rack.parts[0],"$done"),crate::ksp::Value::Int(6));
        assert_eq!(s.rack.parts[0].bank().unwrap().zone_par(1,ZonePar::Group),Some(1));
        // Offline hosts and the Part script-before-bank install order use
        // the same preparation boundary; init is synchronous and returns -1.
        let init_source=Instrument {groups:groups.clone(),zones:zones.clone(),scripts:vec![r#"on init
set_snapshot_type(3)
declare ui_label $status(1,1)
declare %jobs[3]
declare $done
make_persistent($done)
%jobs[0] := set_zone_par(1,$ZONE_PAR_GROUP,0)
%jobs[1] := set_zone_par(1,$ZONE_PAR_LOW_KEY,60)
%jobs[2] := set_zone_par(1,$ZONE_PAR_HIGH_KEY,60)
set_text($status,"pending")
wait_async(%jobs[0])
wait_async(%jobs[1])
wait_async(%jobs[2])
set_text($status,get_zone_par(1,$ZONE_PAR_GROUP) & ":" & get_zone_par(1,$ZONE_PAR_LOW_KEY) & ":" & get_zone_par(1,$ZONE_PAR_HIGH_KEY) & ":" & %jobs[0] & ":" & %jobs[1] & ":" & %jobs[2])
end on
on async_complete
inc($done)
end on"#.into()],..Instrument::default()};
        let (rt,errors)=crate::engine::load_scripts(&init_source,Vec::new(),48000.);
        assert!(errors.is_empty(),"{errors:?}");
        let mut initial=Bank::load(&init_source).unwrap();initial.apply_script_zone_init(rt.as_deref().unwrap()).unwrap();
        assert_eq!(initial.zone_par(1,ZonePar::Group),Some(0));
        let mut offline=Engine::default();offline.set_script(rt);
        let initial=Box::new(initial);
        assert_eq!(allocations(|| {offline.set_bank(Some(initial));}),0);
        assert!(!offline.service_zone_edits().unwrap(),"synchronous init did not enqueue live async work");
        assert_eq!(offline.script().unwrap().interface(0).controls[0].properties["$CONTROL_PAR_TEXT"],crate::ksp::Value::Text("0:60:60:-1:-1:-1".into()));
        assert_eq!(scalar(&offline,"$done"),crate::ksp::Value::Int(0),"init has no async_complete callbacks");
        assert!(offline.script().unwrap().diagnostics().is_empty(),"{:?}",offline.script().unwrap().diagnostics());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn live_ir_loads_complete_after_install_without_audio_allocations() {
        use crate::{audio::Sample, fx::{Chain, Effect, Kind, Load as FxLoad, Params as FxParams, params::{Convolution, Impulse, IrBand}}, import::Group};
        let dir = std::env::temp_dir().join(format!("kontra-live-ir-{}", std::process::id()));
        let resources = dir.join("Resources/ir_samples");
        std::fs::create_dir_all(&resources).unwrap();
        let mut wav = Vec::new();
        wav.extend(b"RIFF"); wav.extend(44u32.to_le_bytes()); wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes()); wav.extend(3u16.to_le_bytes()); wav.extend(1u16.to_le_bytes());
        wav.extend(48000u32.to_le_bytes()); wav.extend(192000u32.to_le_bytes());
        wav.extend(4u16.to_le_bytes()); wav.extend(32u16.to_le_bytes()); wav.extend(b"data");
        wav.extend(8u32.to_le_bytes()); wav.extend(0.5f32.to_le_bytes()); wav.extend(0.25f32.to_le_bytes());
        std::fs::write(resources.join("Room.wav"), wav).unwrap();
        std::fs::write(resources.join("Broken.wav"), b"invalid audio").unwrap();
        let band = IrBand { length_ratio: 1., low_cut_hz: 20., high_cut_hz: 24000. };
        let i = Instrument {
            path: dir.join("Instrument.nki"), groups: vec![Group::default()],
            fx: crate::fx::ProgramFx { insert: Chain { slots: vec![Effect {
                slot: 0, kind: Kind::Convolution, version: 0, bypass: false, output_gain: 1., dry_level: 0.,
                params: FxParams::Convolution(Box::new(Convolution {
                    unknown: [0.; 2], predelay_ms: 0., early: band, late: band, unknown_9: 0.,
                    flags: [false; 5], curve_x: Vec::new(), curve_db: Vec::new(), ir_index: -1,
                    ir_file: None, ir_error: None,
                    ir: Some(Impulse(Arc::new(Sample { rate: 48000, frames: vec![[1.; 2]] }))),
                })),
            }] }, ..Default::default() },
            scripts: vec![r#"on init
make_perfview
declare ui_slider $room(0, 2)
declare ui_slider $size(0, 1000000)
declare ui_slider $distance(0, 1000000)
declare ui_button $reverse
declare ui_button $automatic
declare $read_reverse
declare $read_automatic
make_persistent($read_reverse)
make_persistent($read_automatic)
$reverse := get_engine_par($ENGINE_PAR_IRC_REVERSE,-1,0,$NI_INSERT_BUS)
$automatic := get_engine_par($ENGINE_PAR_IRC_AUTO_GAIN,-1,0,$NI_INSERT_BUS)
$read_reverse := $reverse
$read_automatic := $automatic
declare $id
end on
on ui_control($room)
if ($room = 1)
$id := load_ir_sample("room", 0, $NI_INSERT_BUS)
else
if ($room = 2)
$id := load_ir_sample("broken", 0, $NI_INSERT_BUS)
else
$id := load_ir_sample("missing", 0, $NI_INSERT_BUS)
end if
end if
end on
on ui_control($size)
set_engine_par($ENGINE_PAR_IRC_LENGTH_RATIO_LR, $size, -1, 0, $NI_INSERT_BUS)
end on
on ui_control($distance)
set_engine_par($ENGINE_PAR_IRC_PREDELAY, $distance, -1, 0, $NI_INSERT_BUS)
end on
on ui_control($reverse)
set_engine_par($ENGINE_PAR_IRC_REVERSE,$reverse,-1,0,$NI_INSERT_BUS)
$read_reverse := get_engine_par($ENGINE_PAR_IRC_REVERSE,-1,0,$NI_INSERT_BUS)
end on
on ui_control($automatic)
set_engine_par($ENGINE_PAR_IRC_AUTO_GAIN,$automatic,-1,0,$NI_INSERT_BUS)
$read_automatic := get_engine_par($ENGINE_PAR_IRC_AUTO_GAIN,-1,0,$NI_INSERT_BUS)
end on
on async_complete
message($NI_ASYNC_ID & ":" & $NI_ASYNC_EXIT_STATUS)
end on"#.into()],
            ..Default::default()
        };
        let (rt, errors) = crate::engine::load_scripts(&i, Vec::new(), 48000.);
        assert!(errors.is_empty(), "{errors:?}");
        let p = SamplerParams::new();
        let mut dsp = Dsp::default();
        dsp.script_epoch[0] = 1;
        dsp.rack.parts[0].set_bank(Some(Box::new(Bank::from_samples(i.groups.clone(), Vec::new(), Vec::new()).unwrap())));
        dsp.rack.parts[0].set_fx(crate::engine::effects(&i, rt.as_deref(), 48000.));
        dsp.rack.parts[0].set_script(rt);
        {
            let mut view = p.shared.view.lock().unwrap();
            let v = &mut view.parts[0];
            v.instrument = Some(Arc::new(i)); v.script_epoch = 1; v.fx_rate = 48000.;
        }
        let mut outputs = vec![vec![0.; 64]; 2];
        let mut refs: Vec<_> = outputs.iter_mut().map(Vec::as_mut_slice).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut refs, 64);
        let transport = TransportInfo::default();
        let mut midi_out = EventList::with_capacity(0);
        let mut cx = ProcessContext::new(&transport, 48000., 64, &mut midi_out);
        let events = EventList::with_capacity(0);
        let tick = |dsp: &mut Dsp, buffer: &mut AudioBuffer, cx: &mut ProcessContext| {
            Sampler::process(dsp, &p, buffer, &events, cx);
        };
        assert_eq!(allocations(|| {
            dsp.rack.parts[0].ui_control(0, 0, 1);
            tick(&mut dsp, &mut buffer, &mut cx);
        }), 0, "queuing the IR allocated or freed on the audio thread");
        assert_eq!(dsp.rack.parts[0].script().unwrap().last_message(), "", "completion waits for installation");
        assert_eq!(p.shared.ir_requests.len(), 1);
        let pending = p.shared.ir_requests.pop().unwrap();
        for _ in 0..p.shared.ready.capacity() {
            p.shared.ready.push((0, 0, Handoff::Fx(FxProcessor::default()))).ok().unwrap();
            p.shared.ir_ready.push((0, 0, IrHandoff { ir: None, epoch: 0, rate: 48000., request: pending.3.clone() })).ok().unwrap();
        }
        p.shared.ir_requests.push(pending).ok().unwrap();
        load_irs(&p);
        assert!(p.shared.ready.is_full(), "IR loads never evict queued instrument or FX handoffs");
        assert_eq!(p.shared.ir_requests.len(), 1, "backpressure leaves the request pending");
        while p.shared.ready.pop().is_some() {}
        while p.shared.ir_ready.pop().is_some() {}
        load_irs(&p);
        assert_eq!(dsp.rack.parts[0].script().unwrap().last_message(), "");
        assert_eq!(allocations(|| tick(&mut dsp, &mut buffer, &mut cx)), 0, "installing or completing the IR allocated or freed on the audio thread");
        assert_eq!(dsp.rack.parts[0].script().unwrap().last_message(), "1:1");
        assert_eq!(p.shared.discard.len(), 1, "the worker receives the old kernel");
        while p.shared.discard.pop().is_some() {}
        // Native switches use 0/1, coalesce through the same worker path,
        // and read back before/after installation without RT allocation.
        assert_eq!(allocations(|| {
            dsp.rack.parts[0].ui_control(0, 3, 1);
            dsp.rack.parts[0].ui_control(0, 4, 1);
            tick(&mut dsp, &mut buffer, &mut cx);
        }), 0);
        for name in ["$read_reverse", "$read_automatic"] {
            assert_eq!(dsp.rack.parts[0].script().unwrap().persistence()[0][name], crate::ksp::Value::Int(1));
        }
        assert_eq!(p.shared.ir_requests.len(), 1, "two switches need one IR rebuild");
        load_irs(&p);
        assert_eq!(allocations(|| tick(&mut dsp, &mut buffer, &mut cx)), 0);
        let installed = dsp.rack.parts[0].fx().ir_settings(crate::fx::Rack::Insert, 0).unwrap();
        assert_eq!((installed.reverse, installed.auto_gain), (Some(true), Some(true)));
        let (instrument, loads) = {
            let view = p.shared.view.lock().unwrap();
            (view.parts[0].instrument.as_ref().unwrap().clone(), view.parts[0].irs.clone())
        };
        let saved = instrument.fx.ir_settings_with(&loads);
        let json = serde_json::to_string(&saved).unwrap();
        let saved: Vec<crate::fx::IrSlotSettings> = serde_json::from_str(&json).unwrap();
        let (restored, errors) = crate::engine::load_scripts_with_state(&instrument, Vec::new(), 48000., &saved, &[]);
        assert!(errors.is_empty(), "{errors:?}");
        for name in ["$read_reverse", "$read_automatic"] {
            assert_eq!(restored.as_ref().unwrap().persistence()[0][name], crate::ksp::Value::Int(1), "restored switch");
        }
        let mut rebuilt = crate::engine::effects(&instrument, restored.as_deref(), 48000.);
        let (mut l, mut r) = ([1.,0.,0.,0.], [1.,0.,0.,0.]);
        assert_eq!(allocations(|| rebuilt.process(&mut l, &mut r)), 0);
        let gain = (0.5f32 / (0.5f32.powi(2) + 0.25f32.powi(2))).sqrt();
        for out in [l, r] {
            assert!((out[0] - 0.25 * gain).abs() < 1e-6);
            assert!((out[1] - 0.5 * gain).abs() < 1e-6, "restored Reverse + Auto Gain change the impulse");
        }
        let legacy: crate::fx::params::IrSettings = serde_json::from_str(r#"{"values":[0,0.5,0.5],"size":0.5}"#).unwrap();
        assert_eq!((legacy.reverse, legacy.auto_gain), (None, None));
        let mut native = instrument.fx.clone();
        let FxParams::Convolution(c) = &mut native.insert.slots[0].params else { unreachable!() };
        c.flags[0] = true;
        c.flags[1] = true;
        let legacy_load = crate::fx::ScriptIr { rack: crate::fx::Rack::Insert, slot: 0, load: FxLoad::Convolution(legacy) };
        let legacy_processor = native.processor_with(48000., 64, &[legacy_load]);
        let legacy_settings = legacy_processor.ir_settings(crate::fx::Rack::Insert, 0).unwrap();
        assert_eq!((legacy_settings.reverse, legacy_settings.auto_gain), (Some(true), Some(true)), "old host states retain native switches");
        while p.shared.discard.pop().is_some() {}
        let before = p.shared.view.lock().unwrap().parts[0].irs.clone();
        assert!(matches!(before[0].load, FxLoad::Ir { .. }));
        dsp.rack.parts[0].ui_control(0, 0, 0);
        tick(&mut dsp, &mut buffer, &mut cx);
        load_irs(&p);
        assert_eq!(allocations(|| tick(&mut dsp, &mut buffer, &mut cx)), 0);
        assert_eq!(dsp.rack.parts[0].script().unwrap().last_message(), "2:0");
        assert!(p.shared.view.lock().unwrap().parts[0].irs == before, "failed loads keep the IR");
        // Knob edits coalesce into one worker rebuild, retaining the loaded IR
        // and exact requested values without issuing an async-file completion.
        let snapshot = PersistenceSnapshot {
            script: dsp.rack.parts[0].script().unwrap().persistence(),
            ir: vec![crate::fx::IrSlotSettings { rack: crate::fx::Rack::Insert, slot: 0, settings: crate::fx::params::IrSettings::DEFAULT, file: None }],
            native: dsp.rack.parts[0].script().unwrap().native_state.snapshot(),
        };
        p.shared.snapshot_requests.push((0, dsp.script_epoch[0], Box::new(snapshot))).ok().unwrap();
        assert_eq!(allocations(|| {
            dsp.rack.parts[0].ui_control(0, 1, 1000000);
            dsp.rack.parts[0].ui_control(0, 2, 250000);
            tick(&mut dsp, &mut buffer, &mut cx);
        }), 0);
        let (_, _, snapshot, changed) = p.shared.snapshots.pop().expect("host state snapshot before worker completion");
        assert!(changed);
        assert_eq!(snapshot.ir[0].settings.values, [0.25, 0.5, 1.]);
        assert_eq!(p.shared.ir_requests.len(), 1);
        let settings = p.shared.ir_requests.pop().unwrap();
        assert!(settings.3.id < 0 && settings.3.file().is_empty());
        assert_eq!(settings.3.settings.values, [0.25, 0.5, 1.]);
        p.shared.ir_requests.push(settings).ok().unwrap();
        load_irs(&p);
        // A newer edit arriving while the worker builds must win. Execute that
        // callback before the handoff so the obsolete kernel is rejected.
        assert_eq!(allocations(|| {
            dsp.rack.parts[0].ui_control(0, 1, 0);
            dsp.rack.parts[0].ui_control(0, 3, 0);
            dsp.rack.parts[0].render(&mut [0.; 64], &mut [0.; 64]);
            tick(&mut dsp, &mut buffer, &mut cx);
        }), 0);
        assert_eq!(p.shared.ir_requests.len(), 1, "stale settings rebuild at the latest requested values");
        load_irs(&p);
        assert_eq!(allocations(|| tick(&mut dsp, &mut buffer, &mut cx)), 0);
        assert_eq!(dsp.rack.parts[0].fx().ir_settings(crate::fx::Rack::Insert, 0).unwrap().values, [0.25, 0.5, 0.]);
        assert_eq!(dsp.rack.parts[0].fx().ir_settings(crate::fx::Rack::Insert, 0).unwrap().reverse, Some(false), "newer switch wins over the stale prepared kernel");
        assert_eq!(dsp.rack.parts[0].script().unwrap().persistence()[0]["$read_reverse"], crate::ksp::Value::Int(0));
        assert_eq!(dsp.rack.parts[0].script().unwrap().last_message(), "2:0");
        dsp.rack.parts[0].ui_control(0, 0, 2);
        tick(&mut dsp, &mut buffer, &mut cx);
        load_irs(&p);
        assert_eq!(allocations(|| tick(&mut dsp, &mut buffer, &mut cx)), 0);
        assert_eq!(dsp.rack.parts[0].script().unwrap().last_message(), "3:0");
        crate::diagnostics::flush(std::time::Duration::from_secs(5)).unwrap();
        let journal = std::fs::read_to_string(crate::diagnostics::log_path().unwrap()).unwrap();
        assert!(journal.lines().any(|line| {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return false };
            let data = &v["data"];
            data["code"] == "ir_load_failed" && data["requested"] == "broken" && data["resolved"].as_str().is_some_and(|p| p.ends_with("Broken.wav"))
                && v["part"] == 0 && data["message"].as_str().is_some_and(|s| s.contains("decoding failed") && s.contains("no suitable format reader"))
        }), "the worker preserves the requested name, resolved path, part and decoder's failure reason");
        dsp.rack.parts[0].ui_control(0, 0, 1);
        tick(&mut dsp, &mut buffer, &mut cx);
        load_irs(&p);
        p.shared.rate.store(44100f64.to_bits(), Ordering::Release);
        dsp.rack.parts[0].reset(44100.);
        assert_eq!(allocations(|| tick(&mut dsp, &mut buffer, &mut cx)), 0);
        assert_eq!(dsp.rack.parts[0].script().unwrap().last_message(), "3:0", "wrong-rate kernels are retried without installation or premature completion");
        assert_eq!(p.shared.ir_requests.len(), 1);
        load_irs(&p);
        assert_eq!(allocations(|| tick(&mut dsp, &mut buffer, &mut cx)), 0);
        assert_eq!(dsp.rack.parts[0].script().unwrap().last_message(), "4:1", "the original request ID completes after rebuilding at the new rate");
        let flags = dsp.rack.parts[0].fx().ir_settings(crate::fx::Rack::Insert, 0).unwrap();
        assert_eq!((flags.reverse, flags.auto_gain), (Some(false), Some(true)), "rate rebuild retains latest switches");
        dsp.rack.parts[0].ui_control(0, 0, 1);
        tick(&mut dsp, &mut buffer, &mut cx);
        load_irs(&p);
        dsp.script_epoch[0] = 2;
        assert_eq!(allocations(|| tick(&mut dsp, &mut buffer, &mut cx)), 0);
        assert_eq!(dsp.rack.parts[0].script().unwrap().last_message(), "4:1", "a replaced script never gets a stale completion");
        dsp.script_epoch[0] = 1;
        dsp.rack.parts[0].ui_control(0, 0, 1);
        tick(&mut dsp, &mut buffer, &mut cx);
        p.shared.part(0).unwrap().generation.store(1, Ordering::Release);
        load_irs(&p);
        assert!(p.shared.ir_ready.is_empty(), "replaced instruments discard queued loads");
        dsp.rack.parts[0].ui_control(0, 0, 1);
        dsp.rack.parts[0].set_script(None);
        assert!(dsp.rack.parts[0].pop_ir_request().is_none(), "script replacement clears requests before they receive the new epoch");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn host_transport_reaches_ksp_with_sample_position_pause_seek_and_tempo_without_allocating() {
        let source = r#"on init
make_perfview
declare ui_label $position(1,1)
declare ui_label $transition(1,1)
declare $starts := 0
declare $stops := 0
set_listener($NI_SIGNAL_TRANSP_START,1)
set_listener($NI_SIGNAL_TRANSP_STOP,1)
end on
on listener
if ($NI_SIGNAL_TYPE = $NI_SIGNAL_TRANSP_START)
inc($starts)
else
inc($stops)
end if
set_text($transition,$starts & ":" & $stops & ":" & $NI_SONG_POSITION & ":" & $DURATION_QUARTER)
end on
on note
ignore_event($EVENT_ID)
set_text($position,$NI_SONG_POSITION & ":" & $NI_TRANSPORT_RUNNING & ":" & $DURATION_QUARTER & ":" & $SIGNATURE_NUM & "/" & $SIGNATURE_DENOM & ":" & $DURATION_BAR)
end on"#;
        let mut log = crate::ksp::LogEngine::new(Vec::new(), 48_000.);
        let (rt, errors) = Runtime::with_scripts(&[source], &mut log, 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        let p = SamplerParams::new();
        let mut dsp = Dsp::default();
        dsp.rack.parts[0].set_script(Some(Box::new(rt)));
        let mut outputs = [vec![0.;128], vec![0.;128]];
        let mut refs: Vec<_> = outputs.iter_mut().map(Vec::as_mut_slice).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut refs, 128);
        let mut events = EventList::with_capacity(1);
        events.push(Event::on_port(120, 0, EventBody::NoteOn { group: 0, channel: 0, note: 60, velocity: 100 }));
        let mut outgoing = EventList::with_capacity(4);
        // Each host position is authoritative: a seek never accrues the gap
        // since the previous snapshot, and stopped transport stays fixed.
        for (playing, tempo, beats, signature, position, transition) in [
            (true, 120., 2., (3,4), "1924:1:500000:3/4:1500000", "1:0:1920:500000"),
            (true, 60., 10., (7,8), "9602:1:1000000:7/8:3500000", "1:0:1920:500000"),
            (false, 90., 3.5, (7,8), "3360:0:666666:7/8:0", "1:1:3360:666666"),
            (false, 90., -1.25, (0,0), "-1200:0:666666:7/8:0", "1:1:3360:666666"),
            (true, 90., 100., (4,4), "96003:1:666666:4/4:2666666", "2:1:96000:666666"),
        ] {
            let transport = TransportInfo { playing, tempo, position_beats: beats, time_sig_num: signature.0, time_sig_den: signature.1, ..TransportInfo::default() };
            let mut cx = ProcessContext::new(&transport, 48_000., 128, &mut outgoing);
            assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx); }), 0,
                "host timing updates and transport listeners must not allocate or free");
            let interface = dsp.rack.parts[0].script().unwrap().interface(0);
            assert_eq!(interface.controls[0].properties["$CONTROL_PAR_TEXT"], crate::ksp::Value::Text(position.into()));
            assert_eq!(interface.controls[1].properties["$CONTROL_PAR_TEXT"], crate::ksp::Value::Text(transition.into()),
                "seeks/tempo changes must not fabricate start/stop transitions");
        }
        assert!(dsp.rack.parts[0].script().unwrap().diagnostics().is_empty());
    }

    #[test]
    fn internal_note_controllers_forward_wait_ignore_and_retune_without_audio_allocations() {
        // NI's two-slot per-note pitch-bend example, with an intervening
        // callback exercising the same forwarding rules as controllers.
        let send = r#"on init
declare ui_button $send
declare ui_slider $unexpected(0,100)
end on
on ui_control($send)
set_note_controller(0,60,127)
set_note_controller(511,61,7)
set_note_controller($VNC_PITCH_BEND,60,-4096)
set_note_controller(23,60,3)
end on
on note_controller
inc($unexpected)
end on"#;
        let filter = r#"on init
declare ui_label $delayed(1,1)
end on
on note_controller
if ($NC_NUM = 23)
ignore_controller
else
wait(1000)
set_text($delayed,$NC_NUM & ":" & $NC_NOTE & ":" & $NC_VALUE & ":" & $MIDI_CHANNEL)
end if
end on"#;
        let receive = r#"on init
make_perfview
declare %events[128]
declare ui_slider $count(0,10000)
declare ui_slider $registered(0,127)
declare ui_slider $assignable(0,127)
declare ui_slider $bend(-8192,8191)
declare ui_slider $callback_type(0,1)
end on
on note
%events[$EVENT_NOTE] := $EVENT_ID
end on
on note_controller
inc($count)
if ($NI_CALLBACK_TYPE = $NI_CB_TYPE_NOTE_CONTROLLER)
$callback_type := 1
end if
select ($NC_NUM)
case 0
$registered := $NC_VALUE
case 511
$assignable := $NC_VALUE
case $VNC_PITCH_BEND
$bend := $NC_VALUE
change_tune(%events[$NC_NOTE],$NC_VALUE * 10,0)
end select
end on"#;
        let mut engine = crate::ksp::LogEngine::new(Vec::new(), 48_000.);
        let (mut rt, errors) = Runtime::with_scripts(&[send, filter, receive], &mut engine, 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        rt.set_midi_channel(5);
        rt.note_on(&mut engine, 0, 60, 100);
        engine.calls.clear();
        engine.calls.reserve(256);
        let mut live = rt.live();
        assert_eq!(allocations(|| {
            for _ in 0..100 {
                rt.ui_control(&mut engine, 0, 0, 1);
                rt.process(&mut engine, 49);
                rt.refresh_live(&mut live);
            }
        }), 0, "first-use and repeated internal per-note messages must not allocate or free");
        let interface = live.interface.as_ref().unwrap();
        for (index, expected) in [300, 127, 7, -4096, 1].into_iter().enumerate() {
            assert_eq!(interface.controls[index].properties["$CONTROL_PAR_VALUE"], crate::ksp::Value::Int(expected));
        }
        assert_eq!(rt.interface(1).controls[0].properties["$CONTROL_PAR_TEXT"], crate::ksp::Value::Text("512:60:-4096:0".into()),
            "UI-generated messages retain their own channel and controller context across wait");
        assert_eq!(rt.interface(0).controls[1].properties["$CONTROL_PAR_VALUE"], crate::ksp::Value::Int(0), "generated messages start after the sending slot");
        assert_eq!(engine.calls.len(), 100);
        assert!(engine.calls.iter().all(|call| matches!(call,
            crate::ksp::EngineCall::SetPar { par: crate::ksp::VoicePar::TuneMc, value: -40960, .. })),
            "only the receiving script applies per-note tuning to the existing voice");
        assert!(rt.diagnostics().is_empty(), "{:?}", rt.diagnostics());
    }

    #[test]
    fn delayed_label_style_reaches_retained_live_without_audio_allocations() {
        let script = r#"on init
make_perfview
declare ui_switch $page
declare ui_label $caption(1,1)
set_text($caption, "thresh.")
end on
on ui_control($page)
set_control_par(get_ui_id($caption), $CONTROL_PAR_FONT_TYPE, 23)
set_control_par(get_ui_id($caption), $CONTROL_PAR_TEXTPOS_Y, 0)
set_control_par(get_ui_id($caption), $CONTROL_PAR_TEXT_ALIGNMENT, 1)
set_control_par(get_ui_id($caption), $CONTROL_PAR_TEXT_COLOR, 8421504)
end on"#;
        let mut engine = crate::ksp::LogEngine::new(Vec::new(), 48_000.);
        let (mut rt, errors) = Runtime::with_scripts(&[script], &mut engine, 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        let mut live = rt.live();
        let fields = ["$CONTROL_PAR_FONT_TYPE", "$CONTROL_PAR_TEXTPOS_Y", "$CONTROL_PAR_TEXT_ALIGNMENT", "$CONTROL_PAR_TEXT_COLOR"];
        for field in fields {
            assert!(!matches!(live.interface.as_ref().unwrap().controls[1].properties.get(field), Some(crate::ksp::Value::Int(_) | crate::ksp::Value::Real(_) | crate::ksp::Value::Text(_))), "unset metadata keeps the renderer's default: {field}");
        }
        assert_eq!(allocations(|| {
            rt.ui_control(&mut engine, 0, 0, 1);
            rt.refresh_live(&mut live);
        }), 0, "first publication of delayed metadata must neither allocate nor free");
        let caption = &live.interface.as_ref().unwrap().controls[1];
        for (field, expected) in fields.into_iter().zip([23, 0, 1, 8421504]) {
            assert_eq!(caption.properties.get(field), Some(&crate::ksp::Value::Int(expected)), "retained GUI snapshot must match the delayed callback: {field}");
        }
        assert_eq!(live, rt.live(), "retained and newly prepared views agree");
    }

    #[test]
    fn control_edits_run_the_script_and_report_back() {
        let script = "on init\nmake_perfview\ndeclare ui_switch $legato\nmake_persistent($legato)\ndeclare ui_label $l(1,1)\ndeclare %bad[1]\nend on\non ui_control($legato)\n%bad[3] := 1\nset_text($l, \"Legato\")\nset_key_color(36, $KEY_COLOR_BLUE)\nend on";
        let mut engine = crate::ksp::LogEngine::new(Vec::new(), 48_000.0);
        let (rt, errors) = Runtime::with_scripts(&[script], &mut engine, 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        let p = SamplerParams::new();
        let mut dsp = Dsp::default();
        {
            let mut view = p.shared.view.lock().unwrap();
            let epoch = next_epoch(
                &mut view,
                0,
                Some(Box::new(PersistenceSnapshot { script: rt.persistence(), ir: Vec::new(), native: rt.native_state.snapshot() })),
                Some(Box::new(rt.live())),
            );
            let parsed = script_interface(Some(&rt));
            view.parts[0].interface = parsed.interface;
            dsp.script_epoch[0] = epoch;
        }
        dsp.rack.parts[0].set_script(Some(Box::new(rt)));

        p.shared.edit_control(0, 0, 1);
        let shown = p.shared.view.lock().unwrap().parts[0]
            .interface
            .clone()
            .unwrap();
        assert_eq!(
            p.shared.view.lock().unwrap().parts[0].control_value(0),
            Some(1.),
            "the view shows an edit at once"
        );
        assert_eq!(shown.controls[0].properties["$CONTROL_PAR_VALUE"], crate::ksp::Value::Int(0),
            "the pending edit preserves callback metadata");
        let live = p.shared.view.lock().unwrap().parts[0].live.take().unwrap();
        p.shared.live_requests.push((0, dsp.script_epoch[0], live)).ok().unwrap();

        let mut outputs = vec![vec![0f32; 64]; 2];
        let mut refs: Vec<_> = outputs.iter_mut().map(|o| o.as_mut_slice()).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut refs, 64);
        let transport = TransportInfo::default();
        let mut midi_out = EventList::with_capacity(4);
        let mut cx = ProcessContext::new(&transport, 48000., 64, &mut midi_out);
        Sampler::process(
            &mut dsp,
            &p,
            &mut buffer,
            &EventList::with_capacity(1),
            &mut cx,
        );

        // Refreshing a lent view and snapshot neither allocates nor frees.
        let events = EventList::with_capacity(1);
        p.shared.edit_control(0, 0, 1);
        Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx);
        let (slot, _, live) = p.shared.lives.pop().unwrap();
        p.shared.live_requests.push((slot, dsp.script_epoch[slot], live)).ok().unwrap();
        let saved = Box::new(PersistenceSnapshot { script: dsp.rack.parts[0].script().unwrap().persistence(), ir: Vec::new(), native: dsp.rack.parts[0].script().unwrap().native_state.snapshot() });
        p.shared.snapshot_requests.push((0, dsp.script_epoch[0], saved)).ok().unwrap();
        // As if the scripts changed since: both refresh in full.
        (dsp.live_seen[0], dsp.snapshot_seen[0]) = ((u64::MAX, 0), (u64::MAX, 0));
        let calls = allocations(|| {
            for _ in 0..4 {
                Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx);
            }
        });
        assert_eq!(calls, 0, "the audio thread allocated or freed");
        assert!(p.shared.snapshots.pop().is_some(), "the snapshot comes back");
        let (slot, epoch, live) = p.shared.lives.pop().expect("the live view comes back");
        assert_eq!((slot, epoch), (0, dsp.script_epoch[0]));
        assert!(!live.faults.is_empty(), "runtime faults are copied into the lent buffer without allocating");
        let interface = live.interface.as_ref().unwrap();
        assert_eq!(
            interface.controls[1].properties["$CONTROL_PAR_TEXT"],
            crate::ksp::Value::Text("Legato".into())
        );
        assert_eq!(
            live.keys[&36].color,
            Some(crate::ksp::Value::Text("$KEY_COLOR_BLUE".into()))
        );
        let saved = dsp.rack.parts[0].script().unwrap().persistence();
        assert_eq!(
            saved[0]["$legato"],
            crate::ksp::Value::Int(1),
            "edits persist"
        );
        p.shared.panic.store(true, Ordering::Release);
        p.shared.bend.store(10000, Ordering::Relaxed);
        p.shared.modulation.store(127, Ordering::Relaxed);
        assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx); }), 0,
            "Panic must not allocate or free on the audio thread");
        assert_eq!(p.shared.bend.load(Ordering::Relaxed), 8192);
        assert_eq!(p.shared.modulation.load(Ordering::Relaxed), 0);
        assert_eq!(dsp.rack.parts[0].script().unwrap().persistence()[0]["$legato"], crate::ksp::Value::Int(1),
            "Panic must preserve edited instrument controls");
    }
    #[test]
    fn exact_host_brightness_does_not_alias_a_new_note_or_escape_through_overflow_without_heap() {
        use moose::core::{ExactEvent, ExactEventBody, ExactNoteAddress, ExactNoteKind};
        use crate::modulation::{ModAssignment, ModSource, ModTarget};
        let p = SamplerParams::new();
        let setup = || {
            let group = import::Group { mods:vec![ModAssignment { name:"CC74_VOLUME".into(),
                source:ModSource::MidiCc(74), target:ModTarget::Volume, intensity:1., invert:false,
                lag_ms:0, shaper:None }], ..Default::default() };
            let bank = Bank::from_samples(vec![group],vec![import::Zone::default()],vec![(PathBuf::new(),
                crate::audio::Sample { rate:48000, frames:vec![[0.25;2];4096] })]).unwrap();
            let mut dsp = Dsp::default();
            dsp.rack.parts[0].set_bank(Some(Box::new(bank)));
            dsp.rack.parts[0].cc(0,74,32);
            dsp
        };
        let brightness = Event::on_port(64,0,EventBody::PerNoteCC {
            group:0, channel:0, note:60, cc:74, value:u32::MAX, registered:true });
        let address = |id| ExactNoteAddress::from_raw_signed(0,0,60,id);
        let make_events = |expression:bool| {
            let mut events = EventList::with_capacity(8);
            for (at,id,kind,body) in [
                (0,10,ExactNoteKind::On,EventBody::NoteOn { group:0, channel:0, note:60, velocity:100 }),
                (16,10,ExactNoteKind::Off,EventBody::NoteOff { group:0, channel:0, note:60, velocity:0 }),
                (16,11,ExactNoteKind::On,EventBody::NoteOn { group:0, channel:0, note:60, velocity:100 }),
            ] {
                let token = events.try_push_exact_token(ExactEvent::new(at,ExactEventBody::Note {
                    kind,address:address(id),velocity:100./127. })).unwrap();
                events.try_push_exact_companion(token,Event::on_port(at,0,body)).unwrap();
            }
            if expression {
                // The old released host note is still a legitimate expression
                // target; its simplified companion must not affect note11.
                let token = events.try_push_exact_token(ExactEvent::new(64,ExactEventBody::NoteExpression {
                    expression_id:5,address:address(10),value:1. })).unwrap();
                events.try_push_exact_companion(token,brightness).unwrap();
                assert!(unsupported_host_brightness(&events,3));
            }
            events
        };
        let render = |dsp:&mut Dsp,params:&SamplerParams,events:&EventList| {
            let (mut left,mut right) = ([0.;128],[0.;128]);
            let mut output = [&mut left[..],&mut right[..]];
            let mut buffer = AudioBuffer::from_slices_checked(&[],&mut output,128);
            let transport = TransportInfo::default();
            let mut midi_out = EventList::with_capacity(0);
            let mut cx = ProcessContext::new(&transport,48000.,128,&mut midi_out);
            assert_eq!(allocations(|| { Sampler::process(dsp,params,&mut buffer,events,&mut cx); }),0);
            (left,right)
        };
        let (mut actual,mut expected) = (setup(),setup());
        let with_expression = make_events(true);
        let plain = make_events(false);
        let reference = SamplerParams::new();
        assert_eq!(render(&mut actual,&p,&with_expression),render(&mut expected,&reference,&plain));
        assert_eq!(actual.unsupported_note_brightness,1);
        assert_eq!(actual.rack.parts[0].cc_state()[0][74],32);
        drain_audio_diagnostics(&p);
        actual.until_diagnostics = 0;
        render(&mut actual,&p,&EventList::with_capacity(0));
        drain_audio_diagnostics(&p);
        assert_eq!(p.shared.diagnostic_latest.lock().unwrap().as_ref().unwrap().unsupported_note_brightness,1);

        let mut raw = EventList::with_capacity(1);
        raw.push(brightness);
        assert!(!unsupported_host_brightness(&raw,0),"direct registered MIDI2 brightness remains supported");
        let mut normalized = EventList::with_capacity(1);
        let token = normalized.try_push_exact_token(ExactEvent::new(64,ExactEventBody::NormalizedNoteExpression {
            expression_id:5,address:address(11),value:1. })).unwrap();
        normalized.try_push_exact_companion(token,brightness).unwrap();
        assert!(unsupported_host_brightness(&normalized,0));
        let mut overflow = EventList::with_capacity(1);
        let exact = ExactEvent::new(64,ExactEventBody::NoteExpression { expression_id:5,address:address(10),value:1. });
        overflow.try_push_exact_token(exact).unwrap();
        assert!(overflow.try_push_exact_token(exact).is_err());
        // Adapter fallback after the exact lane fills has no origin metadata.
        overflow.push(brightness);
        assert!(overflow.exact_for_event(0).is_none());
        assert!(unsupported_host_brightness(&overflow,0));
    }

    #[test]
    fn vst3_length_hint_keeps_exact_note_until_explicit_off_without_heap() {
        use crate::{audio::Sample, import::{Group,Zone}};
        let address=ExactNoteAddress::from_vst3_signed(0,4,60,42);
        let mut reference=None;
        for length in [0,16,12000,-1,i32::MIN] {
            let params=SamplerParams::new();
            let mut dsp=Dsp::default(); dsp.until_poll=usize::MAX;
            dsp.rack.parts[0].reset(48000.);
            let bank=Bank::from_samples(vec![Group::default()],vec![Zone::default()],vec![(PathBuf::new(),
                Sample { rate:48000,frames:vec![[0.2;2];4096] })]).unwrap();
            dsp.rack.parts[0].set_bank(Some(Box::new(bank)));
            let mut on=EventList::with_capacity(4);
            on.try_push_exact(ExactEvent::new(64,ExactEventBody::DetailedNote {
                kind:ExactNoteKind::On,address,velocity:0.8,tuning:0.,length:Some(length) })).unwrap();
            assert!(on.get(0).is_none(),"this host note must exercise exact-only ingress");
            let mut off=EventList::with_capacity(2);
            off.try_push_exact(ExactEvent::new(32,ExactEventBody::DetailedNote {
                kind:ExactNoteKind::Off,address,velocity:0.,tuning:0.,length:None })).unwrap();
            let none=EventList::with_capacity(0);
            let mut outgoing=EventList::with_capacity(16);
            let transport=TransportInfo::default();
            let mut render=|dsp:&mut Dsp,events:&EventList| {
                let (mut left,mut right)=([0.;128],[0.;128]);
                let mut channels=[&mut left[..],&mut right[..]];
                let mut buffer=AudioBuffer::from_slices_checked(&[],&mut channels,128);
                let mut cx=ProcessContext::new(&transport,48000.,128,&mut outgoing);
                Sampler::process(dsp,&params,&mut buffer,events,&mut cx);
                (left,right)
            };
            let mut pcm=([0.;128],[0.;128]);
            assert_eq!(allocations(|| { pcm=render(&mut dsp,&on); }),0);
            assert!(pcm.0[..64].iter().all(|x| *x==0.),"host sample offset was lost");
            assert!(pcm.0[64..].iter().any(|x| x.abs()>1e-6),"exact VST3 note produced no audio");
            if let Some(expected)=reference { assert_eq!(pcm,expected,"optional length changed onset audio"); }
            else { reference=Some(pcm); }
            assert_eq!(allocations(|| { render(&mut dsp,&none); }),0);
            assert!(dsp.rack.parts[0].host_key_held(4,60),"length hint released the exact host owner");
            assert!(dsp.rack.parts[0].voice_census().iter().any(|v| !v.released));
            assert_eq!(allocations(|| { render(&mut dsp,&off); }),0);
            assert!(!dsp.rack.parts[0].host_key_held(4,60),"explicit NoteOff failed to close the owner");
            assert_eq!(dsp.unsupported_host_expression,0);
        }
        for (length,velocity,tuning) in [(Some(16),f32::NAN,0.),(Some(16),0.8,f32::NAN)] {
            let mut invalid=EventList::with_capacity(1);
            invalid.try_push_exact(ExactEvent::new(0,ExactEventBody::DetailedNote {
                kind:ExactNoteKind::On,address,velocity,tuning,length })).unwrap();
            let LosslessEventRef::Exact(event)=invalid.lossless_iter().next().unwrap() else { panic!("missing exact event") };
            assert!(matches!(exact_host_input(event),Some(ExactInput::Unsupported)),"invalid attack became playable");
        }
    }

    #[test]
    fn vst3_onset_tuning_keeps_anonymous_and_scripted_timed_owners_without_heap() {
        use crate::{audio::Sample, import::{Group,Zone}, engine::{HostExpression,HostPattern}};
        for anonymous in [false,true] { for scripted in [false,true] { for holding in [false,true] {
            let params=SamplerParams::new(); let mut dsp=Dsp::default(); dsp.until_poll=usize::MAX;
            dsp.rack.parts[0].reset(48000.);
            let bank=Bank::from_samples(vec![Group::default()],vec![Zone::default()],vec![(PathBuf::new(),
                Sample { rate:48000, frames:(0..24000).map(|n| [0.2+n as f32/240000.;2]).collect() })]).unwrap();
            dsp.rack.parts[0].set_bank(Some(Box::new(bank)));
            if scripted {
                let script="on note\nignore_event($EVENT_ID)\nwait(1000)\nplay_note($EVENT_NOTE,$EVENT_VELOCITY,0,-1)\nend on";
                let (rt,errors)=Runtime::with_scripts(&[script],&mut crate::ksp::LogEngine::new(Vec::new(),48000.),0,Vec::new());
                assert!(errors.iter().all(Option::is_none)); dsp.rack.parts[0].set_script(Some(Box::new(rt)));
            }
            dsp.align.plan.on=holding; dsp.align.plan.transport_only=false;
            dsp.align.plan.parts[0]=Holds::of(&timing::Timing { override_ms:Some(0.),..Default::default() },&[],10.);
            let address=|id| ExactNoteAddress::from_vst3_signed(0,4,60,id);
            let ids=if anonymous { [-1,-1] } else { [42,43] };
            let mut on=EventList::with_capacity(4);
            for (offset,id,cents) in [(16,ids[0],250.),(64,ids[1],-350.)] {
                on.try_push_exact(ExactEvent::new(offset,ExactEventBody::DetailedNote {
                    kind:ExactNoteKind::On,address:address(id),velocity:0.8,tuning:cents,length:Some(16) })).unwrap();
            }
            if !anonymous {
                // A rejected duplicate must not retune the original root.
                on.try_push_exact(ExactEvent::new(80,ExactEventBody::DetailedNote {
                    kind:ExactNoteKind::On,address:address(42),velocity:0.8,tuning:1200.,length:None })).unwrap();
            }
            let none=EventList::with_capacity(0); let mut outgoing=EventList::with_capacity(32);
            let transport=TransportInfo::default();
            let mut render=|dsp:&mut Dsp,events:&EventList| {
                let (mut left,mut right)=([0.;128],[0.;128]);
                let mut channels=[&mut left[..],&mut right[..]];
                let mut buffer=AudioBuffer::from_slices_checked(&[],&mut channels,128);
                let mut cx=ProcessContext::new(&transport,48000.,128,&mut outgoing);
                Sampler::process(dsp,&params,&mut buffer,events,&mut cx); left
            };
            let mut first_sound=None;
            for block in 0..8 {
                let mut pcm=[0.;128];
                assert_eq!(allocations(|| { pcm=render(&mut dsp,if block==0 { &on } else { &none }); }),0);
                if first_sound.is_none() { first_sound=pcm.iter().position(|x| x.abs()>1e-6).map(|n| block*128+n); }
            }
            let earliest=16+usize::from(holding)*480+usize::from(scripted)*48;
            let first_sound=first_sound.expect("valid nonzero VST3 tuning silenced the note");
            assert!((earliest..earliest+32).contains(&first_sound),"offset/alignment/script wait lost: {first_sound} vs {earliest}");
            let voices=dsp.rack.parts[0].voice_census();
            assert_eq!(voices.len(),2,"duplicate ID created or retuned a root");
            for semitones in [2.5,-3.5] {
                let expected=2f64.powf(semitones/12.);
                assert_eq!(voices.iter().filter(|v| (v.step-expected).abs()<1e-6).count(),1,
                    "cents conversion or overlapping ownership lost: anonymous={anonymous},scripted={scripted},holding={holding}");
            }
            assert_eq!(dsp.unsupported_host_expression,0);
            if !anonymous {
                let pattern=HostPattern { port:0,channel:4,key:60,id:42,clap:false };
                assert_eq!(allocations(|| feed_host_input(&mut dsp,&params,In::HostExpression(pattern,HostExpression::Tune(4.25)),0,0,holding,48000.)),0);
                for _ in 0..4 { assert_eq!(allocations(|| { render(&mut dsp,&none); }),0); }
                let voices=dsp.rack.parts[0].voice_census();
                for semitones in [4.25,-3.5] {
                    assert_eq!(voices.iter().filter(|v| (v.step-2f64.powf(semitones/12.)).abs()<1e-6).count(),1,
                        "later owner expression changed its same-key neighbor");
                }
            }
            let mut off=EventList::with_capacity(2);
            for id in if anonymous { &ids[..1] } else { &ids[..] } {
                off.try_push_exact(ExactEvent::new(16,ExactEventBody::DetailedNote {
                    kind:ExactNoteKind::Off,address:address(*id),velocity:0.,tuning:f32::NAN,length:None })).unwrap();
            }
            for block in 0..5 { assert_eq!(allocations(|| { render(&mut dsp,if block==0 { &off } else { &none }); }),0); }
            assert!(!dsp.rack.parts[0].host_key_held(4,60),"explicit NoteOff failed to close tuned owners");
        } } }
    }

    #[test]
    fn vst3_extreme_finite_onset_tuning_keeps_processing_bounded_without_heap() {
        use crate::{audio::Sample, import::{Group,Zone,Wavetable}};
        for wavetable in [false,true] { for cents in [f32::MAX,-f32::MAX] {
            let params=SamplerParams::new(); let mut dsp=Dsp::default(); dsp.until_poll=usize::MAX;
            dsp.rack.parts[0].reset(48000.);
            let group=Group { wavetable:wavetable.then_some(Wavetable { quality:2,form1_type:16,
                form1:0.5,inharmonic:0.5,..Default::default() }),..Default::default() };
            let bank=Bank::from_samples(vec![group],vec![Zone::default()],vec![(PathBuf::new(),
                Sample { rate:48000,frames:vec![[0.2;2];49152] })]).unwrap();
            dsp.rack.parts[0].set_bank(Some(Box::new(bank)));
            let address=ExactNoteAddress::from_vst3_signed(0,4,60,42);
            let mut on=EventList::with_capacity(1);
            on.try_push_exact(ExactEvent::new(16,ExactEventBody::DetailedNote {
                kind:ExactNoteKind::On,address,velocity:0.8,tuning:cents,length:Some(i32::MIN) })).unwrap();
            let mut off=EventList::with_capacity(1);
            off.try_push_exact(ExactEvent::new(32,ExactEventBody::DetailedNote {
                kind:ExactNoteKind::Off,address,velocity:0.,tuning:0.,length:None })).unwrap();
            let none=EventList::with_capacity(0); let mut outgoing=EventList::with_capacity(16);
            let transport=TransportInfo::default();
            let mut render=|dsp:&mut Dsp,events:&EventList| {
                let (mut left,mut right)=([0.;128],[0.;128]);
                let mut channels=[&mut left[..],&mut right[..]];
                let mut buffer=AudioBuffer::from_slices_checked(&[],&mut channels,128);
                let mut cx=ProcessContext::new(&transport,48000.,128,&mut outgoing);
                Sampler::process(dsp,&params,&mut buffer,events,&mut cx);
                assert!(left.iter().chain(&right).all(|x| x.is_finite()),"extreme finite tuning poisoned PCM");
            };
            for block in 0..8 {
                assert_eq!(allocations(|| render(&mut dsp,if block==0 { &on } else { &none })),0);
                let voices=dsp.rack.parts[0].voice_census();
                assert_eq!(voices.len(),1,"finite onset tuning was rejected");
                // The existing sample traversal cap and overflow-safe wavetable
                // clock bound actual motion, independently of the cached ratio.
                assert!(voices[0].pos.is_finite());
                assert!((0. ..=if wavetable { 2048. } else { 128.*8.*32. }).contains(&voices[0].pos));
            }
            assert!(dsp.rack.parts[0].host_key_held(4,60));
            assert_eq!(dsp.unsupported_host_expression,0);
            assert_eq!(allocations(|| render(&mut dsp,&off)),0);
            assert!(!dsp.rack.parts[0].host_key_held(4,60),"explicit Off lost the extreme-tuned owner");
        } }
    }

    #[test]
    fn exact_host_ids_keep_old_expression_and_emit_end_after_every_part_without_heap() {
        use crate::{audio::Sample, import::{Group,Zone}};
        let params=SamplerParams::new();
        let mut dsp=Dsp::default(); dsp.until_poll=usize::MAX;
        for (part,size) in [(0,256),(1,24000)] {
            dsp.rack.parts[part].reset(48000.);
            let bank=Bank::from_samples(vec![Group::default()],vec![Zone::default()],vec![(PathBuf::new(),Sample { rate:48000,frames:vec![[0.2;2];size] })]).unwrap();
            dsp.rack.parts[part].set_bank(Some(Box::new(bank)));
        }
        let address=|id| ExactNoteAddress::from_raw_signed(0,4,60,id);
        let mut on=EventList::with_capacity(4);
        on.try_push_exact(ExactEvent::new(0,ExactEventBody::Note { kind:ExactNoteKind::On,address:address(10),velocity:0.8 })).unwrap();
        let mut reuse=EventList::with_capacity(8);
        reuse.try_push_exact(ExactEvent::new(0,ExactEventBody::Note { kind:ExactNoteKind::Off,address:address(10),velocity:0. })).unwrap();
        reuse.try_push_exact(ExactEvent::new(0,ExactEventBody::Note { kind:ExactNoteKind::On,address:address(11),velocity:0.8 })).unwrap();
        let token=reuse.try_push_exact_token(ExactEvent::new(0,ExactEventBody::NoteExpression { expression_id:2,address:address(10),value:12. })).unwrap();
        reuse.try_push_exact_companion(token,Event::on_port(0,0,EventBody::PerNotePitchBend { group:0,channel:4,note:60,value:moose::core::midi::per_note_bend_from_semitones(12.) })).unwrap();
        let mut off=EventList::with_capacity(2);
        off.try_push_exact(ExactEvent::new(0,ExactEventBody::Note { kind:ExactNoteKind::Off,address:address(-1),velocity:0. })).unwrap();
        let none=EventList::with_capacity(1);
        let mut outgoing=EventList::with_capacity(128);
        let transport=TransportInfo::default();
        let mut cx=ProcessContext::new(&transport,48000.,128,&mut outgoing);
        let (mut left,mut right)=([0.;128],[0.;128]);
        let mut channels=[&mut left[..],&mut right[..]];
        let mut buffer=AudioBuffer::from_slices_checked(&[],&mut channels,128);
        assert_eq!(allocations(|| {
            Sampler::process(&mut dsp,&params,&mut buffer,&on,&mut cx);
            Sampler::process(&mut dsp,&params,&mut buffer,&reuse,&mut cx);
        }),0);
        let voices=dsp.rack.parts[1].voice_census();
        assert!(voices.iter().any(|v| v.released && v.step>1.9));
        assert!(voices.iter().any(|v| !v.released && (v.step-1.).abs()<0.01),"old-id expression changed the new same-key note");
        assert!(params.shared.heard[60].load(Ordering::Relaxed)>0,"old key-up cleared the newer host key");
        assert_eq!(dsp.unsupported_host_expression,0);
        // Part 0's attack has ended; part 1's old release still owns id 10.
        assert!(!cx.output_events.lossless_iter().any(|event| matches!(event,LosslessEventRef::Exact(e) if matches!(e.body(),ExactEventBody::Note { kind:ExactNoteKind::End,.. }))));
        assert_eq!(allocations(|| {
            Sampler::process(&mut dsp,&params,&mut buffer,&off,&mut cx);
            for _ in 0..128 { Sampler::process(&mut dsp,&params,&mut buffer,&none,&mut cx); }
        }),0);
        let ended:Vec<_>=cx.output_events.lossless_iter().filter_map(|event| match event {
            LosslessEventRef::Exact(e) => match *e.body() { ExactEventBody::Note { kind:ExactNoteKind::End,address,.. } => Some(address),_=>None },_=>None,
        }).collect();
        assert_eq!(ended.len(),2);
        for id in [10,11] { assert!(ended.contains(&address(id))); }
        assert!(dsp.rack.parts.iter().all(|e| e.host_note_at(0).is_none()));
        for holding in [false,true] {
            dsp.align.plan.on=holding;
            let unmatched=ExactNoteAddress::from_raw_signed(7,4,60,12+i32::from(holding));
            let mut absent=EventList::with_capacity(2);
            absent.try_push_exact(ExactEvent::new(0,ExactEventBody::Note { kind:ExactNoteKind::On,address:unmatched,velocity:0.8 })).unwrap();
            assert_eq!(allocations(|| { Sampler::process(&mut dsp,&params,&mut buffer,&absent,&mut cx); }),0);
            assert!(cx.output_events.lossless_iter().any(|event| matches!(event,LosslessEventRef::Exact(e) if matches!(e.body(),ExactEventBody::Note { kind:ExactNoteKind::End,address,.. } if *address==unmatched))),"a no-sound route retained the host identity");
        }
        let mut art=crate::articulate::Articulate::default();
        art.sync("switch",&[("Switch".into(),Some(60),None)]);
        for router in &mut dsp.routers { router.set_route(crate::articulate::Route::new("switch",&art,&crate::articulate::Mpe::default())); }
        let switch=ExactNoteAddress::from_raw_signed(0,4,60,14);
        let mut switched=EventList::with_capacity(2);
        switched.try_push_exact(ExactEvent::new(0,ExactEventBody::Note { kind:ExactNoteKind::On,address:switch,velocity:0.8 })).unwrap();
        assert_eq!(allocations(|| { Sampler::process(&mut dsp,&params,&mut buffer,&switched,&mut cx); }),0);
        assert!(cx.output_events.lossless_iter().any(|event| matches!(event,LosslessEventRef::Exact(e) if matches!(e.body(),ExactEventBody::Note { kind:ExactNoteKind::End,address,.. } if *address==switch))),"aligned keyswitch retained the host identity");
    }

    #[test]
    fn exact_note_end_backpressure_retains_owners_for_one_retry_without_heap() {
        use crate::engine::{HostNote,HostPattern};
        let params=SamplerParams::new(); let mut dsp=Dsp::default();
        let first=HostNote { port:0,channel:4,key:60,id:10,clap:true };
        let second=HostNote { id:11,..first };
        let pattern=HostPattern { port:0,channel:4,key:60,id:-1,clap:true };
        let transport=TransportInfo::default(); let mut output=EventList::with_capacity(1);
        let mut cx=ProcessContext::new(&transport,48000.,128,&mut output);
        assert_eq!(allocations(|| {
            feed_host_input(&mut dsp,&params,In::HostOn(first,100,0.),0,0,false,48000.);
            feed_host_input(&mut dsp,&params,In::HostOn(second,100,0.),0,0,false,48000.);
            feed_host_input(&mut dsp,&params,In::HostOff(pattern),0,0,false,48000.);
            finish_host_notes(&mut dsp,&mut cx,127);
            assert_eq!(dsp.host_note_end_rejections,1,"repeated failures scanned the full owner pool");
            assert!(dsp.rack.parts.iter().all(|e| !e.host_note_present(first) && e.host_note_present(second)));
            cx.output_events.clear();
            finish_host_notes(&mut dsp,&mut cx,127);
            assert_eq!(dsp.host_note_end_rejections,1);
            assert!(dsp.rack.parts.iter().all(|e| !e.host_note_present(second)));
        }),0);
    }

    #[test]
    fn delayed_exact_expression_retains_owner_until_safe_same_id_reuse() {
        use crate::{audio::Sample, import::{Group,Zone}, engine::{HostNote,HostPattern,HostExpression}};
        let params=SamplerParams::new(); let mut dsp=Dsp::default();
        dsp.rack.parts[0].reset(48000.);
        let bank=Bank::from_samples(vec![Group::default()],vec![Zone::default()],vec![(PathBuf::new(),Sample { rate:48000,frames:vec![[0.2;2];32] })]).unwrap();
        dsp.rack.parts[0].set_bank(Some(Box::new(bank)));
        dsp.align.plan.parts[0]=Holds::of(&timing::Timing { override_ms:Some(0.),..Default::default() },&[],10.);
        let note=HostNote { port:0,channel:4,key:60,id:10,clap:true };
        let pattern=HostPattern { port:0,channel:4,key:60,id:10,clap:true };
        let transport=TransportInfo::default(); let mut output=EventList::with_capacity(128);
        let mut cx=ProcessContext::new(&transport,48000.,128,&mut output);
        let (mut left,mut right)=([0.;64],[0.;64]);
        assert_eq!(allocations(|| {
            feed_host_input(&mut dsp,&params,In::HostOn(note,100,0.),0,0,true,48000.);
            feed_host_input(&mut dsp,&params,In::HostOff(pattern),0,96,true,48000.);
            feed_host_input(&mut dsp,&params,In::HostExpression(pattern,HostExpression::Gain(0.)),0,240,true,48000.);
            dsp.align.release(600,&mut dsp.rack,&mut dsp.routers);
            dsp.rack.parts[0].render(&mut left,&mut right);
            finish_host_notes(&mut dsp,&mut cx,63);
            assert_eq!(dsp.rack.parts[0].active_voices(),0);
            assert!(dsp.align.host_note_waiting(note),"delayed old expression lost its owner pin");
            assert!(dsp.rack.parts[0].host_note_present(note));
            assert!(!dsp.rack.parts[0].admit_host_note(note),"same ID became reusable before delayed work completed");
            assert!(!cx.output_events.lossless_iter().any(|event| matches!(event,LosslessEventRef::Exact(e) if matches!(e.body(),ExactEventBody::Note { kind:ExactNoteKind::End,.. }))));
            dsp.align.release(720,&mut dsp.rack,&mut dsp.routers);
            finish_host_notes(&mut dsp,&mut cx,63);
            assert!(!dsp.align.host_note_waiting(note));
            assert!(!dsp.rack.parts[0].host_note_present(note));
            feed_host_input(&mut dsp,&params,In::HostOn(note,100,0.),0,0,false,48000.);
            dsp.rack.parts[0].render(&mut left,&mut right);
            assert!(left.iter().any(|sample| sample.abs()>1e-6),"old gain expression leaked into reused host ID");
        }),0);
    }

    /// The keys lit by the host's notes go out with its all-notes-off and
    /// all-sound-off (what a host sends on stop), and when it resets the
    /// plugin, with or without parts to play them.
    #[test]
    fn host_notes_light_until_the_host_lets_them_go() {
        let p = SamplerParams::new();
        let mut dsp = Dsp::default();
        let run = |dsp: &mut Dsp, bodies: &[EventBody]| {
            let mut events = EventList::with_capacity(bodies.len().max(1));
            for &body in bodies {
                events.push(Event::on_port(0, 0, body));
            }
            let mut outputs = vec![vec![0f32; 64]; 2];
            let mut refs: Vec<_> = outputs.iter_mut().map(|o| o.as_mut_slice()).collect();
            let mut buffer = AudioBuffer::from_slices_checked(&[], &mut refs, 64);
            let transport = TransportInfo::default();
            let mut midi_out = EventList::with_capacity(4);
            let mut cx = ProcessContext::new(&transport, 48000., 64, &mut midi_out);
            Sampler::process(dsp, &p, &mut buffer, &events, &mut cx);
        };
        let lit = |p: &SamplerParams| (0..128).filter(|&n| p.shared.heard[n].load(Ordering::Relaxed) > 0).count();
        let chord: Vec<_> = (69..81)
            .map(|note| EventBody::NoteOn { group: 0, channel: 0, note, velocity: 100 })
            .collect();
        for cc in [123, 120] {
            run(&mut dsp, &chord);
            assert_eq!(lit(&p), 12, "the host's notes light their keys");
            run(&mut dsp, &[EventBody::ControlChange { group: 0, channel: 3, cc, value: 0 }]);
            assert_eq!(lit(&p), 0, "CC{cc} lets them go");
        }
        run(&mut dsp, &chord);
        Sampler::reset(&mut dsp, &p, &AudioConfig::new(48000., 64));
        assert_eq!(lit(&p), 0, "a reset forgets them");
    }
    /// With no part selected the on-screen keys play like host MIDI on A1:
    /// every part listening there sounds, omni ones included, and each
    /// key's release reaches only the parts its press did.
    #[test]
    fn unselected_keys_play_every_part_midi_would() {
        use crate::{audio::Sample, import::{Group, Loop, Zone}};
        let mut dsp = Dsp::default();
        let p = SamplerParams::new();
        for (slot, channel) in [(0, -1), (1, -1), (2, 0), (3, 5)] {
            let zone = Zone {
                loop_range: Some(Loop { start: 0, end: 100, alternating: false, until_release: false, crossfade: 0 }),
                ..Zone::default()
            };
            let sample = Sample { rate: 48000, frames: vec![[0.5, 0.25]; 100] };
            let bank = Bank::from_samples(vec![Group::default()], vec![zone], vec![(PathBuf::new(), sample)]);
            dsp.rack.parts[slot].set_bank(Some(Box::new(bank.unwrap())));
            dsp.rack.controls[slot].channel = channel;
        }
        let mut outputs = vec![vec![0f32; 256]; 2];
        let mut refs: Vec<_> = outputs.iter_mut().map(|o| o.as_mut_slice()).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut refs, 256);
        let transport = TransportInfo::default();
        let mut midi_out = EventList::with_capacity(4);
        let mut cx = ProcessContext::new(&transport, 48000., 256, &mut midi_out);
        let mut run = |dsp: &mut Dsp, events: &EventList, blocks: usize| {
            for _ in 0..blocks {
                Sampler::process(dsp, &p, &mut buffer, events, &mut cx);
            }
        };
        let none = EventList::with_capacity(0);
        let voices = |dsp: &Dsp| [0, 1, 2, 3].map(|s| dsp.rack.parts[s].active_voices() > 0);

        p.shared.press_key(EVERY_PART, 60, 100);
        run(&mut dsp, &none, 1);
        assert_eq!(voices(&dsp), [true, true, true, false], "omni and channel 1 play, channel 6 does not");

        // Channel 6 plays the same key from the host (on the omni parts
        // too); the on-screen key's release stops only what it started.
        let mut host = EventList::with_capacity(1);
        host.push(Event::new(0, EventBody::NoteOn { group: 0, channel: 5, note: 60, velocity: 100 }));
        run(&mut dsp, &host, 1);
        p.shared.release_key(60);
        run(&mut dsp, &none, 400);
        let count = |dsp: &Dsp| [0, 1, 2, 3].map(|s| dsp.rack.parts[s].active_voices());
        assert_eq!(count(&dsp), [1, 1, 0, 1]);

        // A selected part plays alone.
        p.shared.press_key(2, 64, 100);
        run(&mut dsp, &none, 1);
        assert_eq!(count(&dsp), [1, 1, 1, 1]);
    }
    #[test]
    fn process_routes_bus_and_midi_thru() {
        use crate::{
            audio::Sample,
            import::{Group, Loop, Zone},
        };
        use moose::core::bus_routing::{BusActivation, BusRouting};
        let mut dsp = Dsp::default();
        let p = SamplerParams::new();
        p.shared.midi_thru.store(true, Ordering::Relaxed);
        let group = Group {
            name: "test".into(),
            ..Group::default()
        };
        let zone = Zone {
            loop_range: Some(Loop {
                start: 0,
                end: 100,
                alternating: false,
                until_release: false,
                crossfade: 0,
            }),
            ..Zone::default()
        };
        dsp.rack.parts[1].set_bank(Some(Box::new(
            Bank::from_samples(
                vec![group],
                vec![zone],
                vec![(
                    PathBuf::new(),
                    Sample {
                        rate: 48000,
                        frames: vec![[0.5, 0.25]; 100],
                    },
                )],
            )
            .unwrap(),
        )));
        dsp.rack.controls[1].port = 1;
        dsp.rack.controls[1].output = 2;
        let mut events = EventList::with_capacity(4);
        events.push(Event::on_port(
            16,
            1,
            EventBody::NoteOn {
                group: 0,
                channel: 0,
                note: 60,
                velocity: 127,
            },
        ));
        let mut outputs = vec![vec![0f32; 128]; 6];
        let mut refs: Vec<_> = outputs.iter_mut().map(|o| o.as_mut_slice()).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut refs, 128);
        let transport = TransportInfo::default();
        let mut midi_out = EventList::with_capacity(4);
        let mut routes = BusRouting::new();
        for _ in 0..3 {
            routes.push_output(2, BusActivation::Active);
        }
        let mut cx =
            ProcessContext::new(&transport, 48000., 128, &mut midi_out).with_bus_routing(routes);
        Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx);
        for ch in 0..4 {
            assert!(outputs[ch].iter().all(|x| *x == 0.));
        }
        assert!(outputs[4][..16].iter().all(|x| *x == 0.));
        assert!(outputs[4][127] > 0.);
        assert!((outputs[4][127] - 2. * outputs[5][127]).abs() < 1e-6);
        assert_eq!(midi_out.len(), 1);
        assert_eq!(midi_out.get(0).unwrap().port, 0);
        assert_eq!(midi_out.get(0).unwrap().sample_offset, 16);
    }
    /// Plugin instances in one process (as a DAW hosts them) and parts
    /// within one hold a sample's frames once.
    #[test]
    #[ignore = "requires the owner's local Afflatus library"]
    fn instances_share_sample_memory() {
        let path = Path::new(import::LIBRARY_ROOT)
            .join("Afflatus Chapter II Brass/Instruments/4. Experimental/Mega Brass.nki")
            .to_string_lossy()
            .into_owned();
        let instance = |parts: usize| {
            let p = SamplerParams::new();
            let part = Part { path: path.clone(), ..Default::default() };
            p.selection.write().unwrap().parts = std::iter::repeat_n(part, parts).collect();
            Load.run(&p);
            for v in &p.shared.view.lock().unwrap().parts[..parts] {
                println!("  {}", v.status);
            }
            p
        };
        let mib = |b: usize| b as f64 / (1 << 20) as f64;
        let first = instance(1);
        let one = crate::engine::resident_bytes();
        println!("1 instance, 1 part: samples {:.0} MiB, RSS {:.0} MiB", mib(one), rss_mib());
        let second = instance(2);
        let three = crate::engine::resident_bytes();
        println!("2 instances, 3 parts: samples {:.0} MiB, RSS {:.0} MiB", mib(three), rss_mib());
        let banks: usize = [&first, &second]
            .iter()
            .map(|p| p.shared.view.lock().unwrap().parts.iter().map(|v| v.bytes).sum::<usize>())
            .sum();
        assert!(banks > 2 * one, "three banks hold the samples");
        assert!(three < one + one / 20, "{three} bytes resident for three copies of {one}");
    }

    #[test]
    #[ignore = "requires the owner's local Vista library"]
    fn worker_loads_and_removes_real_rack_parts() {
        let p = SamplerParams::new();
        let path = Path::new(import::LIBRARY_ROOT)
            .join("Performance Samples Vista/Instruments/Bonus/Vista - Harp.nki")
            .to_string_lossy()
            .into_owned();
        p.selection.write().unwrap().parts = vec![
            Part {
                path: path.clone(),
                group: 0,
                ..Default::default()
            },
            // Not canonical, as relative or symlinked paths are not: saved
            // script values must still reach the part.
            Part {
                path: path.replacen("/Instruments/", "/Instruments/../Instruments/", 1),
                group: 1,
                output: 1,
                ..Default::default()
            },
        ];
        Load.run(&p);
        assert!(
            p.shared.view.lock().unwrap().parts[0].wallpaper.is_some(),
            "Vista named NKR wallpaper loads on the worker"
        );
        {
            let view = p.shared.view.lock().unwrap();
            let interface = view.parts[0].interface.as_ref().expect("Vista KSP init");
            assert_eq!(interface.controls.len(), 47);
            assert_eq!(interface.height, 180);
            assert!(interface.controls.iter().any(|c|matches!(c.properties.get("$CONTROL_PAR_TEXT"),Some(crate::ksp::Value::Text(s)) if s=="Mic Mixer")));
        }
        let mut dsp = Dsp::default();
        let transport = TransportInfo::default();
        let mut events = EventList::with_capacity(4);
        events.push(Event::new(
            0,
            EventBody::NoteOn {
                group: 0,
                channel: 0,
                note: 60,
                velocity: 100,
            },
        ));
        let mut outgoing = EventList::with_capacity(4);
        let mut data = vec![vec![0f32; 2048]; 4];
        let mut channels: Vec<_> = data.iter_mut().map(|v| v.as_mut_slice()).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, 2048);
        let mut routing = moose::core::bus_routing::BusRouting::new();
        for _ in 0..2 {
            routing.push_output(2, moose::core::bus_routing::BusActivation::Active);
        }
        let mut cx =
            ProcessContext::new(&transport, 48000., 2048, &mut outgoing).with_bus_routing(routing);
        Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx);
        assert!(dsp.rack.parts[0].bank().is_some() && dsp.rack.parts[1].bank().is_some());
        for part in &dsp.rack.parts[..2] {
            assert_eq!(part.bank().unwrap().zones().len(), 2000, "file-handle pressure must not discard harp zones");
        }
        for ch in [0, 2] {
            assert!(buffer.output(ch).iter().all(|x| x.is_finite()));
            assert!(buffer.output(ch).iter().any(|x| x.abs() > 0.00001));
        }
        p.selection.write().unwrap().parts[0] = Part::default();
        Load.run(&p);
        Sampler::process(
            &mut dsp,
            &p,
            &mut buffer,
            &EventList::with_capacity(0),
            &mut cx,
        );
        assert!(dsp.rack.parts[0].bank().is_none());
        assert!(dsp.rack.parts[1].bank().is_some());
        assert!(dsp.rack.parts[1].active_voices() > 0);
        // Scripts run on the audio thread; their persistent values reach the host state.
        assert!(dsp.rack.parts[1].script().is_some());
        Load.run(&p);
        Sampler::process(
            &mut dsp,
            &p,
            &mut buffer,
            &EventList::with_capacity(0),
            &mut cx,
        );
        Load.run(&p);
        let saved = p.selection.read().unwrap().parts[1].script_state.clone();
        assert!(saved.starts_with("[{"), "the instrument's values are saved");
        // Restored host state rebuilds only the scripts: "[]" restores nothing, leaving declared defaults.
        let mut round_trip = |state: &str| {
            let epoch = dsp.script_epoch[1];
            p.selection.write().unwrap().parts[1].script_state = state.into();
            Load.run(&p);
            Sampler::process(
                &mut dsp,
                &p,
                &mut buffer,
                &EventList::with_capacity(0),
                &mut cx,
            );
            assert!(dsp.script_epoch[1] > epoch);
            assert!(dsp.rack.parts[1].bank().is_some() && dsp.rack.parts[1].script().is_some());
            Load.run(&p);
            Sampler::process(
                &mut dsp,
                &p,
                &mut buffer,
                &EventList::with_capacity(0),
                &mut cx,
            );
            Load.run(&p);
            p.selection.read().unwrap().parts[1].script_state.clone()
        };
        assert!(
            round_trip("[]") != saved,
            "declared defaults differ from the saved values"
        );
        assert!(round_trip(&saved) == saved, "saved values restore exactly");
        // A host rate change rebuilds the effects on the loader and keeps the bank.
        Sampler::reset(&mut dsp, &p, &AudioConfig::new(44100., 2048));
        Load.run(&p);
        assert_eq!(p.shared.view.lock().unwrap().parts[1].fx_rate, 44100.);
        assert!(!p.shared.ready.is_empty());
        Sampler::process(
            &mut dsp,
            &p,
            &mut buffer,
            &EventList::with_capacity(0),
            &mut cx,
        );
        assert!(p.shared.ready.is_empty());
        assert!(dsp.rack.parts[1].bank().is_some());
    }
    #[test]
    #[ignore = "requires the owner's local Chorus multi"]
    fn real_multi_opens_embedded_instruments() {
        let p = SamplerParams::new();
        p.shared.queue_multi(format!(
            "{}/Audio Imperia CHORUS/Multis/10 Chorus - Ensemble - Traditional Syllables.nkm",
            import::LIBRARY_ROOT
        ));
        Load.run(&p);
        let s = p.selection.read().unwrap();
        assert_eq!(
            s.parts.iter().map(|p| p.program).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(s.parts.iter().all(|p| p.path == s.multi));
        let view = p.shared.view.lock().unwrap();
        assert!(view.parts[0].instrument.as_ref().unwrap().zones.is_empty());
        assert!(
            view.parts[0]
                .instrument
                .as_ref()
                .unwrap()
                .missing_samples
                .is_empty()
        );
        assert!(
            view.parts[1]
                .instrument
                .as_ref()
                .unwrap()
                .name
                .contains("Women")
        );
        assert!(
            view.parts[2]
                .instrument
                .as_ref()
                .unwrap()
                .name
                .contains("Men")
        );
        assert!(view.parts[0].status.starts_with("Controller instrument"));
    }
    #[test]
    fn failed_multi_keeps_the_rack() {
        let p = SamplerParams::new();
        let before = p.selection.read().unwrap().clone();
        p.shared.queue_multi("/missing.nkm".into());
        Load.run(&p);
        assert!(*p.selection.read().unwrap() == before);
        assert!(
            p.shared
                .view
                .lock()
                .unwrap()
                .multi_status
                .starts_with("Multi load failed")
        );
    }

    // ---- Auto-align ----------------------------------------------------

    /// A bank whose zones answer velocities `low..=high` with `pad` frames of
    /// silence, then a steady tone: an attack exactly `pad` frames late.
    /// Keys 48..=72 only, so keyswitches below play nothing.
    fn late_bank(zones: &[(u8, u8, usize)]) -> Box<Bank> {
        use crate::{audio::Sample, import::{Group, Zone}};
        let group = Group { name: "late".into(), ..Group::default() };
        let (zones, samples) = zones
            .iter()
            .enumerate()
            .map(|(n, &(low, high, pad))| {
                let path = PathBuf::from(format!("{n}"));
                let mut frames = vec![[0.0f32; 2]; pad];
                frames.extend((0..9600).map(|i| {
                    let x = (i as f32 * 0.03).sin() * 0.5 + 0.1;
                    [x, x]
                }));
                let zone = Zone { sample: path.clone(), low_velocity: low, high_velocity: high, low_key: 48, high_key: 72, ..Zone::default() };
                (zone, (path, Sample { rate: 48000, frames }))
            })
            .unzip();
        Box::new(Bank::from_samples(vec![group], zones, samples).unwrap())
    }

    /// Render `frames` in blocks of 128 with note-ons `(frame, channel,
    /// note, velocity)` on port 0, allocation-free; output channels, two per bus.
    fn render_notes(dsp: &mut Dsp, p: &SamplerParams, notes: &[(usize, u8, u8, u8)], frames: usize) -> Vec<Vec<f32>> {
        use moose::core::bus_routing::{BusActivation, BusRouting};
        const BLOCK: usize = 128;
        let mut out = vec![Vec::new(); 4];
        let transport = TransportInfo::default();
        for start in (0..frames).step_by(BLOCK) {
            let mut events = EventList::with_capacity(8);
            for &(at, channel, note, velocity) in notes.iter().filter(|n| (start..start + BLOCK).contains(&n.0)) {
                events.push(Event::on_port(
                    (at - start) as u32,
                    0,
                    EventBody::NoteOn { group: 0, channel, note, velocity },
                ));
            }
            let mut block = vec![vec![0f32; BLOCK]; 4];
            let mut refs: Vec<_> = block.iter_mut().map(|o| o.as_mut_slice()).collect();
            let mut buffer = AudioBuffer::from_slices_checked(&[], &mut refs, BLOCK);
            let mut midi_out = EventList::with_capacity(4);
            let mut routes = BusRouting::new();
            routes.push_output(2, BusActivation::Active);
            routes.push_output(2, BusActivation::Active);
            let mut cx = ProcessContext::new(&transport, 48000., BLOCK, &mut midi_out).with_bus_routing(routes);
            let calls = allocations(|| {
                Sampler::process(dsp, p, &mut buffer, &events, &mut cx);
            });
            assert_eq!(calls, 0, "the audio thread allocated or freed, holding notes back");
            for (o, b) in out.iter_mut().zip(&block) {
                o.extend_from_slice(b);
            }
        }
        out
    }

    fn first_sound(x: &[f32]) -> Option<usize> {
        x.iter().position(|v| *v != 0.0)
    }

    fn attack(first_ms: f32) -> timing::Delay {
        timing::Delay { first: [Some(first_ms); 3], legato: [Some(first_ms); 3], ..Default::default() }
    }

    /// A library's mic mixer sends its "Tree" bus to "Out 2" and its
    /// "Close" group straight to "Out 3" (`$ENGINE_PAR_OUTPUT_CHANNEL`). In
    /// "One per mic" each gets a bus and host port of its own, named after
    /// it, and plays there; the rest stays on the part's own.
    #[test]
    fn mic_outputs_get_their_own_buses_and_ports() {
        use crate::{audio::Sample, import::{Group, Instrument, Loop, Zone}};
        use moose::core::bus_routing::{BusActivation, BusRouting};
        let groups: Vec<Group> = ["Close", "Tree", "Main"].map(|n| Group { name: n.into(), ..Group::default() }).into();
        let looped = Some(Loop { start: 0, end: 100, alternating: false, until_release: false, crossfade: 0 });
        let zones = (0..3)
            .map(|g| Zone { group: g, sample: PathBuf::from(g.to_string()), loop_range: looped.clone(), ..Zone::default() })
            .collect();
        let samples = [0.1f32, 0.2, 0.3]
            .iter()
            .enumerate()
            .map(|(g, &v)| (PathBuf::from(g.to_string()), Sample { rate: 48000, frames: vec![[v, v]; 100] }))
            .collect();
        let mut i = Instrument { name: "Harp".into(), groups: groups.clone(), ..Default::default() };
        i.fx.buses = vec![crate::fx::Bus { index: 0, name: "Tree".into(), volume: 1.0, pan: 0.0, output: -1, chain: Default::default() }];
        i.scripts = vec!["on init
set_engine_par($ENGINE_PAR_OUTPUT_CHANNEL, $NI_BUS_OFFSET, 1, -1, -1)
set_engine_par($ENGINE_PAR_OUTPUT_CHANNEL, 1, -1, -1, $NI_BUS_OFFSET)
set_engine_par($ENGINE_PAR_OUTPUT_CHANNEL, 2, 0, -1, -1)
end on"
            .into()];
        let (rt, errors) = load_scripts(&i, Vec::new(), 48000.);
        assert!(errors.is_empty(), "{errors:?}");
        let mut dsp = Dsp::default();
        let p = SamplerParams::new();
        let e = &mut dsp.rack.parts[0];
        e.set_bank(Some(Box::new(Bank::from_samples(groups, zones, samples).unwrap())));
        e.set_script(rt);
        let fx = i.fx.processor(e.rate() as f32, MAX_BLOCK);
        e.set_fx(fx);

        const BLOCK: usize = 256;
        let transport = TransportInfo::default();
        let block = |dsp: &mut Dsp, note: bool| {
            let mut events = EventList::with_capacity(2);
            if note {
                events.push(Event::on_port(0, 0, EventBody::NoteOn { group: 0, channel: 0, note: 60, velocity: 127 }));
            }
            let mut out = vec![vec![0f32; BLOCK]; 8];
            let mut refs: Vec<_> = out.iter_mut().map(|o| o.as_mut_slice()).collect();
            let mut buffer = AudioBuffer::from_slices_checked(&[], &mut refs, BLOCK);
            let mut midi_out = EventList::with_capacity(4);
            let mut routes = BusRouting::new();
            for _ in 0..4 {
                routes.push_output(2, BusActivation::Active);
            }
            let mut cx = ProcessContext::new(&transport, 48000., BLOCK, &mut midi_out).with_bus_routing(routes);
            Sampler::process(dsp, &p, &mut buffer, &events, &mut cx);
            out
        };
        // A block tells the loader what the instrument routes where.
        block(&mut dsp, false);
        p.shared.view.lock().unwrap().parts[0].instrument = Some(Arc::new(i));
        let mut sel = Selection {
            outputs: routing::Outputs::Mic as u8,
            parts: vec![Part { path: "/lib/Harp.nki".into(), ..Default::default() }],
            order: vec![0],
            ..Default::default()
        };
        p.shared.reroute(&mut sel);
        assert_eq!(sel.parts[0].mic_buses[..4], [-1, 1, 2, -1]);
        assert_eq!(routing::port_names(&sel)[..4], ["Harp", "Harp Tree", "Harp Close", "st.4"]);

        let _ = p.shared.controls.force_push(mix(&sel));
        block(&mut dsp, true);
        let out = block(&mut dsp, false);
        let at = |ch: usize| out[ch][BLOCK - 1];
        assert!(at(0) > 0. && at(2) > 0. && at(4) > 0., "{} {} {}", at(0), at(2), at(4));
        assert!((at(2) - 2. * at(4)).abs() < 1e-5, "Tree (0.2) is twice Close (0.1): {} {}", at(2), at(4));
        assert_eq!(at(6), 0.);

        // Back to one per instrument: the mics play with the part again.
        sel.outputs = routing::Outputs::Instrument as u8;
        p.shared.reroute(&mut sel);
        let _ = p.shared.controls.force_push(mix(&sel));
        let out = block(&mut dsp, false);
        assert!(out[2][BLOCK - 1] == 0. && out[4][BLOCK - 1] == 0.);
        assert!(out[0][BLOCK - 1] > at(0));
    }

    /// Two parts 10 and 30 ms late: the host is told 30 ms, the first part's
    /// notes wait 20, and both attacks land together, 30 ms after the note.
    /// The wait is sample-exact: the held part renders bit for bit what it
    /// renders unheld, 960 frames later.
    #[test]
    fn two_parts_with_different_attacks_land_together() {
        let setup = |on: bool| {
            let mut dsp = Dsp::default();
            let p = SamplerParams::new();
            for (slot, pad) in [(0, 480), (1, 1440)] {
                dsp.rack.parts[slot].set_bank(Some(late_bank(&[(0, 127, pad)])));
                dsp.rack.controls[slot].channel = slot as i16;
                dsp.rack.controls[slot].output = slot as u8;
            }
            let selection = Selection {
                auto_align: on,
                parts: [10.0, 30.0]
                    .map(|ms| Part {
                        path: "late.nki".into(),
                        timing: Timing { source: timing::source("late.nki", 0), loaded: attack(ms), ..Default::default() },
                        ..Default::default()
                    })
                    .into(),
                ..Default::default()
            };
            let _ = p.shared.plan.force_push(plan(&selection));
            (dsp, p)
        };
        let notes = [(100, 0, 60, 100), (100, 1, 60, 100)];
        let (mut dsp, p) = setup(true);
        let held = render_notes(&mut dsp, &p, &notes, 4096);
        assert_eq!(Sampler::latency(&dsp), 1440, "reported through the plugin API");
        let (mut dry, q) = setup(false);
        let unheld = render_notes(&mut dry, &q, &notes, 4096);
        assert_eq!(Sampler::latency(&dry), 0);
        // Unheld, the attacks are 20 ms apart; held, both at note + 30 ms.
        assert_eq!((first_sound(&unheld[0]), first_sound(&unheld[2])), (Some(100 + 480), Some(100 + 1440)));
        assert_eq!((first_sound(&held[0]), first_sound(&held[2])), (Some(100 + 1440), Some(100 + 1440)));
        assert_eq!(held[0][960..], unheld[0][..4096 - 960]);
        assert_eq!(held[2], unheld[2]);
    }

    #[test]
    fn a_replaced_patch_ignores_stale_measurements_but_keeps_its_manual_offset() {
        let selection = Selection {
            auto_align: true,
            parts: vec![
                Part {
                    path: "new.nki".into(),
                    timing: Timing {
                        source: timing::source("old.nki", 0),
                        loaded: attack(100.0),
                        override_ms: Some(80.0),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                Part {
                    path: "anchor.nki".into(),
                    timing: Timing {
                        source: timing::source("anchor.nki", 0),
                        loaded: attack(200.0),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let plan = plan(&selection);
        assert_eq!(plan.latency_ms, 200.0);
        let mut s = timing::Scheduler::default();
        assert_eq!(
            s.arrive(articulate::In::NoteOn(0, 60, 100), 0, &plan.parts[0], 48_000.0, &Router::default()),
            None
        );
        assert_eq!(s.next_due(), Some(120 * 48), "the stale 100 ms measurement is ignored; the manual 80 ms offset remains");
    }

    /// One part, two articulations on their own channels whose samples are
    /// 5 and 40 ms late: each note waits by its own articulation's delay
    /// and both attacks land on the grid, 40 ms after their notes.
    #[test]
    fn two_articulations_with_different_attacks_land_on_the_grid() {
        let articulate = Articulate {
            source: "arts.nki".into(),
            mode: articulate::Mode::Channel,
            articulations: ["Short", "Long"]
                .iter()
                .enumerate()
                .map(|(n, name)| articulate::Articulation {
                    name: (*name).into(),
                    key: Some(24 + n as u8),
                    channel: n as u8,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let selection = Selection {
            auto_align: true,
            parts: vec![Part {
                path: "arts.nki".into(),
                articulate: articulate.clone(),
                timing: Timing {
                    source: timing::source("arts.nki", 0),
                    loaded: attack(40.0),
                    arts: vec![
                        timing::Delay { name: "Short".into(), ..attack(5.0) },
                        timing::Delay { name: "Long".into(), ..attack(40.0) },
                    ],
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let play = |channel: u8, velocity: u8| {
            let mut dsp = Dsp::default();
            let p = SamplerParams::new();
            // Soft notes play the 5 ms zone, loud ones the 40 ms zone.
            dsp.rack.parts[0].set_bank(Some(late_bank(&[(0, 64, 240), (65, 127, 1920)])));
            dsp.rack.controls[0].channel = -1;
            dsp.routers[0].set_route(Route::new("arts.nki", &articulate, &Mpe::default()));
            dsp.align.plan = plan(&selection);
            assert_eq!(Sampler::latency(&dsp), 1920);
            first_sound(&render_notes(&mut dsp, &p, &[(1000, channel, 60, velocity)], 6000)[0])
        };
        assert_eq!(play(0, 40), Some(1000 + 1920), "the short articulation waits 35 ms");
        assert_eq!(play(1, 100), Some(1000 + 1920), "the long one not at all");
    }

    /// Off by default: nothing is held and the host is told nothing.
    #[test]
    fn auto_align_is_off_by_default() {
        let p = plan(&Selection::default());
        assert!(!p.on);
        assert_eq!(p.latency(48000.), 0);
    }

    /// The loader measures a real part in the background and reports its
    /// latest articulation to the audio thread.
    #[test]
    #[ignore = "requires the owner's local Pacific library"]
    fn loader_measures_a_real_part() {
        let p = SamplerParams::new();
        let path = Path::new(import::LIBRARY_ROOT)
            .join("Pacific Ensemble Strings")
            .to_string_lossy()
            .into_owned();
        let nki = (import::presets(Path::new(&path)).unwrap().into_iter())
            .map(|f| f.to_string_lossy().into_owned())
            .find(|f| f.contains("Marcato") && f.ends_with(".nki"))
            .unwrap();
        {
            let mut s = p.selection.write().unwrap();
            s.auto_align = true;
            s.parts = vec![Part { path: nki.clone(), ..Default::default() }];
        }
        let started = Instant::now();
        loop {
            Load.run(&p);
            let t = p.selection.read().unwrap().parts[0].timing.clone();
            if !t.source.is_empty() {
                println!("{nki}: {} ms {} after {:?}", t.latest(), t.basis(), started.elapsed());
                assert!(t.latest() > 20.0 && t.latest() < 200.0);
                break;
            }
            assert!(started.elapsed().as_secs() < 120, "not measured");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        // Told the host once the figure has held for the debounce.
        let told = Instant::now();
        let plan = loop {
            Load.run(&p);
            if let Some(plan) = p.shared.plan.pop().filter(|p| p.latency_ms > 0.0) {
                break plan;
            }
            assert!(told.elapsed() < LATENCY_SETTLE * 2, "never reported");
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        assert!(plan.on && plan.latency_ms > 20.0);
        assert!(told.elapsed() >= LATENCY_SETTLE - std::time::Duration::from_millis(200));
    }
    #[test]
    fn paused_host_preserves_129_part_publications_and_replaces_stale_pending_generations() {
        let p = SamplerParams::new();
        p.selection.write().unwrap().parts = vec![Part::default(); 129];
        Load.run(&p);
        assert!(p.shared.view.lock().unwrap().parts[128].attempted.is_none(), "new-slot load waits for callback storage");
        while let Some((slot, ..)) = p.shared.ready.pop() { assert!(slot < RACK_SLOTS); }
        // Acknowledgment permits the worker to publish all rows even when
        // a paused host cannot consume the 64-entry callback queue yet.
        p.shared.grown.store(129, Ordering::Release);
        Load.run(&p);
        assert!(p.shared.view.lock().unwrap().parts[128].attempted.is_some());
        assert_eq!(p.shared.ready.len(), 64);
        assert_eq!(p.shared.pending_ready.lock().unwrap().len(), 49);
        let mut seen = vec![false; 129];
        loop {
            while let Some((slot, generation, ..)) = p.shared.ready.pop() {
                assert_eq!(generation, p.shared.part(slot).unwrap().generation.load(Ordering::Acquire));
                seen[slot] = true;
            }
            if p.shared.pending_ready.lock().unwrap().is_empty() { break }
            p.shared.flush_ready();
        }
        assert!(seen[16..].iter().all(|delivered| *delivered), "every new part survives backpressure");
        for slot in 0..64 { p.shared.publish_part((slot, 1, Handoff::Fx(FxProcessor::default()))); }
        p.shared.part(128).unwrap().generation.store(2, Ordering::Release);
        p.shared.publish_part((128, 2, Handoff::Bank(late_bank(&[(0, 127, 0)]))));
        p.shared.part(128).unwrap().generation.store(3, Ordering::Release);
        p.shared.publish_part((128, 3, Handoff::Bank(late_bank(&[(0, 127, 0)]))));
        let pending = p.shared.pending_ready.lock().unwrap();
        assert_eq!(pending.len(), 1, "stale large banks retire on the worker instead of accumulating");
        assert_eq!((pending[0].0, pending[0].1), (128, 3));
    }

    #[test]
    fn bounded_live_queue_refreshes_every_large_rack_part_under_continuous_recycling() {
        let p = SamplerParams::new();
        p.shared.ensure_parts(129);
        p.shared.grown.store(129, Ordering::Release);
        let (rt, errors) = Runtime::with_scripts(&["on init\nmake_perfview\nend on"], &mut crate::ksp::LogEngine::default(), 0, Vec::new());
        assert!(errors.iter().all(Option::is_none));
        for v in &mut p.shared.view.lock().unwrap().parts {
            v.script_epoch = 1;
            v.live = Some(Box::new(rt.live()));
        }
        p.shared.publish_live(true);
        let mut seen = vec![false; 129];
        for _ in 0..129 {
            let (slot, _, live) = p.shared.live_requests.pop().unwrap();
            seen[slot] = true;
            // A low-numbered part may finish every frame. It must not take
            // every newly free queue position ahead of higher-numbered parts.
            p.shared.view.lock().unwrap().parts[slot].live = Some(live);
            p.shared.publish_live(true);
        }
        assert!(seen.iter().all(|visited| *visited));
    }

    #[test]
    fn growing_to_129_parts_preserves_notes_and_releases_high_keyboard_targets_without_heap_work() {
        let p = SamplerParams::new();
        let mut dsp = Dsp::default();
        dsp.rack.parts[0].set_bank(Some(late_bank(&[(0, 127, 0)])));
        dsp.rack.parts[0].note_on(0, 59, 100);
        dsp.align.plan.on = true;
        dsp.align.plan.parts.fill(Holds::of(&Timing { override_ms: Some(0.), ..Default::default() }, &[], 10.));
        dsp.align.arrive(&mut dsp.rack, &mut dsp.routers, 0, In::NoteOn(0, 62, 100), 0, 48000.);
        assert_eq!(dsp.align.next_due(), Some(480));
        let selection = Selection { parts: (0..129).map(|slot| Part {
            path: format!("/virtual/dynamic-{slot}.nki"), channel: (slot % 16) as i16,
            ..Default::default()
        }).collect(), order: (0..129).rev().collect(), ..Default::default() };
        use moose::core::custom_state::State;
        assert!(Selection::deserialize(&selection.serialize()).unwrap() == selection);
        let multi = SavedMulti::of("dynamic", &selection);
        let restored: SavedMulti = serde_json::from_str(&serde_json::to_string(&multi).unwrap()).unwrap();
        assert_eq!(restored.parts.len(), 129);
        assert_eq!(restored.parts[0].path, selection.parts[128].path);
        p.shared.ensure_parts(129);
        let mut edited = selection.clone();
        edited.parts[128].edits.0.push(Override { group: None, param: crate::engine::overrides::Param::Attack, offset: 1. });
        p.shared.sync_overrides(&edited);
        assert!(p.shared.sent.lock().unwrap()[128].is_empty(), "new-slot edits wait for callback acknowledgment");
        p.shared.prepare_growth();
        let mut outputs = vec![vec![0f32; 128]; 2 * BUSES];
        let mut refs: Vec<_> = outputs.iter_mut().map(|o| o.as_mut_slice()).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut refs, 128);
        let transport = TransportInfo::default();
        let events = EventList::with_capacity(0);
        let mut midi_out = EventList::with_capacity(0);
        use moose::core::bus_routing::{BusActivation, BusRouting};
        let mut routing = BusRouting::new();
        for _ in 0..BUSES { routing.push_output(2, BusActivation::Active); }
        let mut cx = ProcessContext::new(&transport, 48000., 128, &mut midi_out).with_bus_routing(routing);
        assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx); }), 0);
        assert_eq!(dsp.rack.parts.len(), 129);
        assert!(dsp.rack.parts[0].key_down(0, 59), "growth preserves held notes");
        assert_eq!(dsp.rack.parts[0].active_voices(), 1);
        assert_eq!(dsp.align.next_due(), Some(480), "growth preserves queued notes");
        p.shared.sync_overrides(&edited);
        assert_eq!(p.shared.sent.lock().unwrap()[128], edited.parts[128].edits.0);
        p.shared.part(128).unwrap().generation.store(73, Ordering::Release);
        p.shared.ready.push((128, 73, Handoff::Part { bank: Some(late_bank(&[(0, 127, 0)])),
            fx: FxProcessor::default(), script: None, epoch: 9 })).ok().unwrap();
        p.shared.scope.source.store(129, Ordering::Relaxed);
        p.shared.press_key(128, 60, 100);
        assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx); }), 0);
        assert!(dsp.rack.parts[128].key_down(0, 60));
        assert_eq!(dsp.rack.tap, Some(128), "high part scope is distinct from the master sentinel");
        assert!(p.shared.part(128).unwrap().meter.iter().any(|m| f32::from_bits(m.load(Ordering::Relaxed)) > 0.));
        p.shared.release_key(60);
        p.shared.scope.source.store(SCOPE_MASTER, Ordering::Relaxed);
        assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx); }), 0);
        assert!(!dsp.rack.parts[128].key_down(0, 60), "part 128 is not the old key-owner sentinel");
        assert_eq!(dsp.rack.tap, None, "master scope remains independent of part count");
        p.shared.press_key(EVERY_PART, 61, 100);
        assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx); }), 0);
        assert!(dsp.rack.parts[0].key_down(0, 61) && dsp.rack.parts[128].key_down(0, 61));
        dsp.rack.controls[128].channel = 15;
        p.shared.release_key(61);
        dsp.until_diagnostics = 0;
        assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx); }), 0);
        assert!(!dsp.rack.parts[128].key_down(0, 61), "release uses the saved targets despite route changes");
        drain_audio_diagnostics(&p);
        let audio = p.shared.diagnostic_latest.lock().unwrap().clone().unwrap();
        assert_eq!(audio.parts.len(), 129);
        assert_eq!(audio.parts[128].generation, 73);
        assert_eq!(audio.parts[128].held_keys.len(), 16, "MIDI channel count stays independent");
        assert_eq!(audio.output_buses.len(), BUSES, "host buses stay independent of parts");
        assert_eq!(audio.output_channels, 32);
        assert_eq!(audio.output_buses[BUSES - 1], (30, 2));
        // A full publication queue may coexist with worker-recycled buffers.
        let _ = p.shared.diagnostic_free.force_push(AudioDiagnostics::with_parts(129));
        assert_eq!(allocations(|| {
            for _ in 0..4 {
                dsp.until_diagnostics = 0;
                Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx);
            }
        }), 0);
        assert_eq!(p.shared.diagnostic_audio.len(), 2);
        p.shared.panic.store(true, Ordering::Release);
        assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx); }), 0);
        assert!(dsp.rack.parts.iter().all(|e| (0..16).all(|ch| !e.key_down(ch, 59) && !e.key_down(ch, 61))));
        assert_eq!(allocations(|| {
            for _ in 0..128 { Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx); }
        }), 0);
        assert!(dsp.rack.parts.iter().all(|e| e.active_voices() == 0));
        assert!(dsp.key_slots.0.iter().all(|row| row.iter().all(|held| !held)));
    }

    #[test]
    fn audio_snapshots_wake_their_own_lane_without_a_loader_or_audio_heap_work() {
        use moose::core::tasks::{TaskSpawner, TaskSpawnerBundle};
        let p = Arc::new(SamplerParams::new());
        let mut dsp = Dsp::default();
        dsp.until_poll = usize::MAX;
        moose::core::tasks::warm_pool();
        let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = runs.clone();
        let worker = p.clone();
        let mut tasks = TaskSpawnerBundle::new();
        // No Load lane exists: a Load-only wake cannot drain this fixture.
        tasks.push(TaskSpawner::<AudioDiagnosticsTask>::new_serialized(move |task| {
            task.run(&worker);
            counted.fetch_add(1, Ordering::Release);
        }));
        let tasks = tasks.into_any().unwrap();
        let transport = TransportInfo::default();
        let mut midi = EventList::with_capacity(0);
        let events = EventList::with_capacity(0);
        let mut cx = ProcessContext::new(&transport, 48000., 64, &mut midi).with_tasks(&tasks);
        let mut left = [0f32;64]; let mut right = [0f32;64];
        let mut outputs = [&mut left[..], &mut right[..]];
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut outputs, 64);
        for block in 1..=3 {
            dsp.until_diagnostics = 0;
            assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx); }), 0);
            let deadline = Instant::now() + std::time::Duration::from_secs(2);
            while runs.load(Ordering::Acquire) < block && Instant::now() < deadline { std::thread::yield_now(); }
            assert_eq!(runs.load(Ordering::Acquire), block, "each published snapshot wakes the independent lane");
            assert_eq!(p.shared.diagnostic_latest.lock().unwrap().as_ref().unwrap().block, block as u64);
            assert!(p.shared.diagnostic_audio.is_empty());
        }
        assert_eq!(p.shared.diagnostic_dropped.load(Ordering::Relaxed), 0);
        for _ in 0..8 {
            assert_eq!(allocations(|| { Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx); }), 0);
        }
        assert_eq!(runs.load(Ordering::Acquire), 3, "blocks without a snapshot must not wake its worker");
        drop(tasks);
    }

    #[test]
    fn audio_drop_warning_deltas_survive_instrument_and_script_reload() {
        let p = SamplerParams::new();
        // Mirrors a cumulative counter carried through repeated reloads: the
        // journal must report only 25 + 15 + 6, not recount 25/40 on reload.
        for (block, generation, epoch, underruns, commands) in [
            (1, 1, 1, 25, 5), (2, 2, 2, 25, 5), (3, 3, 3, 40, 7),
            (4, 3, 4, 40, 7), (5, 4, 5, 46, 8),
        ] {
            let mut audio = AudioDiagnostics::with_parts(RACK_SLOTS);
            audio.block = block;
            audio.parts[0] = PartDiagnostics { generation, script_epoch:epoch, underruns,
                dropped_commands:commands, ..Default::default() };
            p.shared.diagnostic_audio.push(audio).ok().unwrap();
            drain_audio_diagnostics(&p);
        }
        crate::diagnostics::flush(std::time::Duration::from_secs(2)).unwrap();
        let history = crate::diagnostics::snapshot();
        let warnings: Vec<_> = history.events.iter().filter(|event|
            event.instance_id == Some(p.shared.instance_id) && event.event == "playback_drops").collect();
        assert_eq!(warnings.len(), 3, "unchanged lifetime counters cannot become new reload warnings");
        assert_eq!(warnings.iter().map(|event| event.details["underruns_delta"].as_u64().unwrap()).collect::<Vec<_>>(), [25, 15, 6]);
        assert_eq!(warnings.iter().map(|event| event.details["dropped_commands_delta"].as_u64().unwrap()).collect::<Vec<_>>(), [5, 2, 1]);
        assert_eq!(p.shared.diagnostic_latest.lock().unwrap().as_ref().unwrap().parts[0].underruns, 46);
    }

    #[test]
    fn diagnostics_audio_handoff_is_bounded_and_reports_held_channel_state() {
        let p = SamplerParams::new();
        let mut dsp = Dsp::default();
        dsp.rack.parts[0].set_bank(Some(late_bank(&[(0, 127, 0)])));
        dsp.rack.parts[0].note_on(2, 60, 100);
        dsp.rack.parts[0].cc(2, 64, 127);
        dsp.rack.parts[0].cc(2, 66, 127);
        dsp.installed_generation[0] = 41;
        p.shared.part(0).unwrap().generation.store(42, Ordering::Relaxed);
        let mut outputs = vec![vec![0f32; 128]; 2];
        let mut refs: Vec<_> = outputs.iter_mut().map(|o| o.as_mut_slice()).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut refs, 128);
        let transport = TransportInfo::default();
        let events = EventList::with_capacity(0);
        let mut midi_out = EventList::with_capacity(0);
        let mut cx = ProcessContext::new(&transport, 48000., 128, &mut midi_out);
        assert_eq!(allocations(|| {
            for _ in 0..3 {
                dsp.until_diagnostics = 0;
                Sampler::process(&mut dsp, &p, &mut buffer, &events, &mut cx);
            }
        }), 0, "diagnostic capture must not allocate, format or free on audio");
        assert_eq!(p.shared.diagnostic_audio.len(), 2);
        assert_eq!(p.shared.diagnostic_dropped.load(Ordering::Relaxed), 1);
        drain_audio_diagnostics(&p);
        let audio = p.shared.diagnostic_latest.lock().unwrap().clone().unwrap();
        assert_eq!((audio.block, audio.sample_rate, audio.block_size, audio.output_channels), (3, 48000., 128, 2));
        assert_eq!(audio.parts[0].generation, 41, "reports the playing generation, not a queued load");
        assert!(audio.parts[0].voices > 0 && audio.parts[0].held_keys[2][0] & (1 << 60) != 0);
        assert_eq!((audio.parts[0].sustain_cc[2], audio.parts[0].sostenuto_cc[2]), (127, 127));
        p.selection.write().unwrap().parts = vec![Part { path:"/missing-kontra-diagnostic-hooks/instrument.nki".into(), channel:2, ..Default::default() }];
        Load.run(&p);
        let context = p.diagnostic_report();
        assert_eq!(context["instance_id"], p.shared.instance_id);
        assert_eq!(context["rack"]["parts"][0]["channel"], 2);
        assert_eq!(context["rack"]["parts"][0]["load"]["status"], "failed");
        assert!(context["rack"]["parts"][0]["load"]["issues"][0]["message"].as_str().is_some());
        assert_eq!(context["host"]["audio"]["block"], 3);
        assert!(context["log_flush_error"].is_null());
        assert_ne!(p.shared.instance_id, SamplerParams::new().shared.instance_id);
    }

}
