use crate::articulate::{self, Articulate, In, Mpe, Route, Router};
use crate::{artwork, library};
use crate::{
    engine::{
        BUSES, Bank, BusControls, Engine, Heads, MAX_BLOCK, Mix, NO_AUX, PartControls, RACK_SLOTS, Rack, Residency,
        Streaming, TUNE_RANGE, load_scripts,
        overrides::{Edits, Override, Probe},
    },
    fx::{DIRECT, FxProcessor, OUTS},
    routing,
    import::{self, Instrument},
    timing::{self, Align, Holds, Plan, Timing},
    ksp::{Interface, KeyState, Live, Persisted, Refresh, Runtime},
};
use crossbeam_queue::ArrayQueue;
use moose::mui::mui::scene::Image;
use moose::prelude::*;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{
        Mutex, RwLock,
        atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    time::Instant,
};

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
}
impl Part {
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
    /// Host output port `index`'s name, as last published (`routing.rs`).
    fn port_name(&self, index: u32) -> Option<String> {
        let names = self.shared.port_names.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        names.published.get(index as usize).cloned()
    }

    fn port_names_revision(&self) -> u64 {
        self.shared.port_names_revision.load(Ordering::Acquire)
    }
}

pub struct Shared {
    ready: ArrayQueue<(usize, u64, Handoff)>,
    discard: ArrayQueue<Retired>,
    /// Persistence snapshots: the loader lends one per scripted slot, the audio thread fills it in place and returns it.
    snapshot_requests: ArrayQueue<(usize, Box<Vec<Persisted>>)>,
    /// Refreshed snapshots, and whether any value in them changed.
    snapshots: ArrayQueue<(usize, u64, Box<Vec<Persisted>>, bool)>,
    /// Live script views, lent and refreshed the same way.
    live_requests: ArrayQueue<(usize, Box<Live>)>,
    lives: ArrayQueue<(usize, u64, Box<Live>)>,
    /// Edits of script controls from the performance view.
    edits: ArrayQueue<Edit>,
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
    routes: ArrayQueue<[Route; RACK_SLOTS]>,
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
    sent: Mutex<[Vec<Override>; RACK_SLOTS]>,
    /// Each slot's smart memory and the generation it serves (loader only).
    residency: Mutex<[Option<(u64, Box<Residency>)>; RACK_SLOTS]>,
    /// Resized heads back from the audio thread: slot, generation, heads.
    heads: ArrayQueue<(usize, u64, Heads)>,
    pub(crate) generation: [AtomicU64; RACK_SLOTS],
    /// Per slot, how far its load is, out of [`crate::engine::LOAD_DONE`]:
    /// parse, scripts, then samples by frames read; only rises within a load.
    pub(crate) load_progress: [AtomicU32; RACK_SLOTS],
    pub(crate) audition: AtomicBool,
    pub(crate) audition_note: AtomicU64,
    pub(crate) selected: AtomicU64,
    pub(crate) focus_request: AtomicU64,
    pub(crate) panic: AtomicBool,
    pub(crate) midi_thru: AtomicBool,
    pub(crate) multi_request: Mutex<Option<String>>,
    /// The app's library folders and the scan of them.
    pub(crate) libraries: library::Scanner,
    pub(crate) view: Mutex<View>,
    /// Voices sounding across the rack, reported by the audio thread.
    pub(crate) voices: AtomicU64,
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
    underruns: [AtomicU64; RACK_SLOTS],
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
    mics: [[AtomicU16; OUTS]; RACK_SLOTS],
    /// Host port names; the loader publishes, the host's main thread reads.
    port_names: Mutex<routing::PortNames>,
    /// Bumped with each publication: format wrappers poll it and tell the host.
    port_names_revision: AtomicU64,
}
/// Parts' timing measurements, each on a thread of its own.
#[derive(Default)]
struct Measure {
    busy: [AtomicBool; RACK_SLOTS],
    /// Slot and what was measured, or why it could not be.
    done: Mutex<Vec<(usize, String, Result<Timing, String>)>>,
}
#[derive(Default, Clone)]
pub(crate) struct PartView {
    pub(crate) program: u32,
    pub(crate) interface: Option<Arc<crate::ksp::Interface>>,
    pub(crate) interface_status: String,
    pub(crate) load_report: Option<Arc<serde_json::Value>>,
    pub(crate) runtime_status: String,
    pub(crate) diagnostics_lent: Option<Instant>,
    pub(crate) wallpaper: Option<Arc<artwork::Picture>>,
    /// Control pictures the scripts name, by name.
    pub(crate) pictures: Arc<HashMap<String, Arc<artwork::Picture>>>,
    pub(crate) wallpaper_status: String,
    pub(crate) attempted: Option<(String, u32)>,
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
    /// Epoch of the runtime last handed to the audio thread.
    pub(crate) script_epoch: u64,
    /// Snapshot buffer for this slot's runtime while the loader holds it.
    pub(crate) snapshot: Option<Box<Vec<Persisted>>>,
    /// When `snapshot` was last lent out.
    pub(crate) snapshot_lent: Option<Instant>,
    /// Live view buffer for this slot's runtime while the loader holds it.
    pub(crate) live: Option<Box<Live>>,
    /// Script slot that `interface` belongs to.
    pub(crate) script_slot: usize,
    /// Samples are being read for this slot.
    pub(crate) loading: bool,
    /// Keyboard colors and names the scripts set, kept current while they run.
    pub(crate) keys: Arc<BTreeMap<u8, KeyState>>,
    /// Why the part's timing could not be measured.
    pub(crate) timing_status: String,
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
    pub(crate) parts: [PartView; RACK_SLOTS],
    pub(crate) status: String,
    /// When an editor last showed the rack (see [`Shared::watched`]).
    pub(crate) watched_at: Option<Instant>,
}
impl Default for Shared {
    fn default() -> Self {
        Self {
            ready: ArrayQueue::new(64),
            discard: ArrayQueue::new(64),
            snapshot_requests: ArrayQueue::new(2 * RACK_SLOTS),
            snapshots: ArrayQueue::new(2 * RACK_SLOTS),
            live_requests: ArrayQueue::new(2 * RACK_SLOTS),
            lives: ArrayQueue::new(2 * RACK_SLOTS),
            edits: ArrayQueue::new(256),
            rate: AtomicU64::new(48000f64.to_bits()),
            key_owners: std::array::from_fn(|_| AtomicU64::new(128)),
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
            residency: Mutex::default(),
            // One batch in flight per slot: never full.
            heads: ArrayQueue::new(RACK_SLOTS),
            sent: Mutex::new(std::array::from_fn(|_| Vec::new())),
            generation: std::array::from_fn(|_| AtomicU64::new(0)),
            load_progress: std::array::from_fn(|_| AtomicU32::new(0)),
            audition: AtomicBool::new(false),
            audition_note: AtomicU64::new(128),
            selected: AtomicU64::new(0),
            focus_request: AtomicU64::new(128),
            panic: AtomicBool::new(false),
            midi_thru: AtomicBool::new(false),
            multi_request: Mutex::new(None),
            libraries: library::Scanner::default(),
            voices: AtomicU64::new(0),
            audible: AtomicU64::new(0),
            watched: AtomicBool::new(false),
            cpu: AtomicU64::new(0),
            busy_ns: AtomicU64::new(0),
            span_ns: AtomicU64::new(0),
            dropouts: AtomicU64::new(0),
            underruns: Default::default(),
            plan: ArrayQueue::new(1),
            published: Mutex::default(),
            measure: Arc::default(),
            reported: AtomicU32::new(0),
            mics: Default::default(),
            port_names: Mutex::default(),
            port_names_revision: AtomicU64::new(0),
            view: Mutex::new(View {
                script_epoch: 0,
                multi_status: String::new(),
                artwork: Default::default(),
                shelf: Arc::default(),
                scanned: 0,
                files: Arc::default(),
                parts: std::array::from_fn(|_| PartView::default()),
                status: "Choose a library and select a preset".into(),
                watched_at: None,
            }),
        }
    }
}
/// Samples [`Scope`] keeps: a spectrum's window and then some.
pub(crate) const SCOPE: usize = 8192;
/// [`Scope::source`] for everything sent to the host.
pub(crate) const SCOPE_MASTER: usize = RACK_SLOTS + 1;

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
    pub parts: [[AtomicU32; 2]; RACK_SLOTS],
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
    pub parts: [AtomicBool; RACK_SLOTS],
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
pub(crate) fn routes(selection: &Selection) -> [Route; RACK_SLOTS] {
    std::array::from_fn(|n| {
        selection
            .parts
            .get(n)
            .map_or_else(Route::default, |p| Route::new(&p.path, &p.articulate, &p.mpe))
    })
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
pub(crate) fn rack_controls(selection: &Selection) -> [PartControls; RACK_SLOTS] {
    std::array::from_fn(|n| {
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
    })
}
/// The slot the on-screen keyboard plays when no part is selected: every
/// part MIDI channel 1 on port A reaches, as if the host had sent it.
pub(crate) const EVERY_PART: usize = RACK_SLOTS;

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
struct Edit {
    part: usize,
    epoch: u64,
    slot: usize,
    control: usize,
    value: i32,
}
/// Loader-built state for one rack slot, installed by the audio thread.
enum Handoff {
    /// A new instrument, or an empty slot, with its effects and initialized scripts.
    Part {
        bank: Option<Box<Bank>>,
        fx: FxProcessor,
        script: Option<Box<Runtime>>,
        epoch: u64,
    },
    /// Effects rebuilt for a new host sample rate; the bank stays.
    Fx(FxProcessor),
    /// Scripts rebuilt from restored host state; the bank stays.
    Script {
        script: Option<Box<Runtime>>,
        epoch: u64,
    },
    /// The part's samples loaded whole (RAM only): replaces its streaming
    /// bank under the playing voices.
    Bank(Box<Bank>),
    /// Sample heads the smart memory resized.
    Heads(Heads),
}
/// What the audio thread replaced, freed on the loader thread.
#[expect(dead_code, reason = "held only to be dropped off the audio thread")]
#[derive(Default)]
struct Retired {
    bank: Option<Box<Bank>>,
    fx: Option<FxProcessor>,
    script: Option<Box<Runtime>>,
    heads: Option<Heads>,
}
/// Persistent script values to restore: the host's saved state, else the instrument's.
fn persisted(saved: &str, i: &Instrument) -> Vec<Persisted> {
    serde_json::from_str(saved).unwrap_or_else(|_| i.script_state.clone())
}
/// Initialize `i`'s scripts off the audio thread, with a snapshot buffer shaped for them.
fn scripts(
    i: &Instrument,
    saved: &str,
    rate: f64,
) -> (
    Option<Box<Runtime>>,
    Option<Box<Vec<Persisted>>>,
    Vec<String>,
) {
    let (script, errors) = load_scripts(i, persisted(saved, i), rate);
    // Nothing persistent: no snapshots to trade with the audio thread.
    let snapshot = script
        .as_ref()
        .map(|rt| rt.persistence())
        .filter(|s| s.iter().any(|p| !p.is_empty()))
        .map(Box::new);
    (script, snapshot, errors)
}
/// Epoch for `script`, about to be handed off from `slot`; forgets the old runtime's buffers.
fn next_epoch(
    view: &mut View,
    slot: usize,
    snapshot: Option<Box<Vec<Persisted>>>,
    live: Option<Box<Live>>,
) -> u64 {
    view.script_epoch += 1;
    let v = &mut view.parts[slot];
    v.script_epoch = view.script_epoch;
    v.snapshot = snapshot;
    v.snapshot_lent = None;
    v.live = live;
    view.script_epoch
}
impl Shared {
    fn rate(&self) -> f64 {
        f64::from_bits(self.rate.load(Ordering::Acquire))
    }

    /// Set script control `control` of `part`'s performance view and run its
    /// `on ui_control`; the view shows the value until the scripts report back.
    pub(crate) fn edit_control(&self, part: usize, control: usize, value: i32) {
        let mut view = self.view.lock().unwrap();
        let v = &mut view.parts[part];
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
        if let Some(c) = v
            .interface
            .as_mut()
            .and_then(|i| Arc::make_mut(i).controls.get_mut(control))
        {
            c.properties
                .insert("$CONTROL_PAR_VALUE".into(), crate::ksp::Value::Int(value));
        }
    }

    /// Send the audio thread what changed in the parts' edits since last
    /// sent. A full queue leaves the slot to be sent again next time:
    /// setting an override is idempotent.
    pub(crate) fn sync_overrides(&self, selection: &Selection) {
        let mut sent = self.sent.lock().unwrap();
        for (slot, had) in sent.iter_mut().enumerate() {
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
        let owner = self.key_owners[note as usize].swap(128, Ordering::AcqRel);
        self.played[note as usize].store(0, Ordering::Relaxed);
        if owner <= EVERY_PART as u64
            && self.keyboard.push((owner as usize, Play::Note(note, 0))).is_err()
        {
            self.panic.store(true, Ordering::Release);
        }
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
        anyhow::ensure!(
            [Self::FORMAT, "kontakto-multi"].contains(&multi.format.as_str()),
            "Not a KONTRA multi"
        );
        anyhow::ensure!(multi.version <= 1, "Saved by a newer KONTRA");
        Ok(multi)
    }
}

pub struct Load;
impl BackgroundTask for Load {
    type Params = SamplerParams;
    const SERIALIZED: bool = true;
    fn run(self, params: &SamplerParams) {
        let mut freed = false;
        while params.shared.discard.pop().is_some() {
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
            .and_then(|m| {
                anyhow::ensure!(
                    m.0.len() <= RACK_SLOTS,
                    "Multi exceeds the 16-instrument rack limit"
                );
                Ok(m)
            });
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
        let selection = params.selection.read().unwrap().clone();
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
        for slot in 0..RACK_SLOTS {
            let part = selection.parts.get(slot).cloned().unwrap_or_default();
            let target = (part.path.clone(), part.program);
            let streaming = part.streaming(selection.streaming);
            // The host restored different script values for a loaded part: rebuild only its scripts.
            let restore = {
                let view = params.shared.view.lock().unwrap();
                let v = &view.parts[slot];
                v.instrument.clone().filter(|i| {
                    v.attempted.as_ref() == Some(&target)
                        && v.fx_rate != 0.
                        && !i.scripts.is_empty()
                        && part.script_state != v.script_state
                })
            };
            if let Some(instrument) = restore {
                let mut trace = crate::diagnostics::LoadTrace::new(&instrument.path, part.program, Some(slot));
                trace.detail("operation", "script_restore");
                trace.stage("scripts");
                let (script, snapshot, errors) =
                    scripts(&instrument, &part.script_state, params.shared.rate());
                for e in &errors { trace.issue("scripts", "initialization_failed", e); }
                if let Some(rt) = script.as_deref() {
                    for e in rt.diagnostics() { trace.issue("scripts", crate::diagnostics::code(&e), e); }
                }
                let live = script.as_deref().map(|rt| Box::new(rt.live()));
                let irs = script.as_deref().map_or(Vec::new(), |rt| rt.init_irs.clone());
                let fx_rate = {
                    let view = params.shared.view.lock().unwrap();
                    (irs != view.parts[slot].irs).then_some(view.parts[slot].fx_rate)
                };
                let fx = fx_rate.map(|rate| {
                    trace.stage("effects");
                    crate::engine::effects(&instrument, script.as_deref(), rate as f32)
                });
                let report = trace.finish("loaded");
                let interface_status = errors.join("\n");
                let mut view = params.shared.view.lock().unwrap();
                let epoch = next_epoch(&mut view, slot, snapshot, live);
                view.parts[slot].script_state = part.script_state.clone();
                view.parts[slot].interface_status = interface_status;
                view.parts[slot].runtime_status.clear();
                if let Some(load) = &mut view.parts[slot].load_report {
                    let load = Arc::make_mut(load);
                    load["runtime"] = serde_json::Value::Null;
                    if report["status"] == "partial" { load["status"] = "partial".into(); }
                    load["script_restore"] = (*report).clone();
                } else { view.parts[slot].load_report = Some(report); }
                if let Some(fx) = fx {
                    view.parts[slot].irs = irs;
                    let _ = params.shared.ready.force_push((
                        slot,
                        params.shared.generation[slot].load(Ordering::Acquire),
                        Handoff::Fx(fx),
                    ));
                }
                let _ = params.shared.ready.force_push((
                    slot,
                    params.shared.generation[slot].load(Ordering::Acquire),
                    Handoff::Script { script, epoch },
                ));
                continue;
            }
            let loaded = {
                let view = params.shared.view.lock().unwrap();
                let v = &view.parts[slot];
                v.attempted.as_ref() == Some(&target) && v.streaming == streaming
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
                params.shared.load_progress[slot].store(0, Ordering::Relaxed);
                v.loading = true;
                v.script_epoch = 0;
                v.snapshot = None;
                v.live = None;
            };
            let generation = params.shared.generation[slot].fetch_add(1, Ordering::AcqRel) + 1;
            let canceled = || {
                let current = params.selection.read().unwrap();
                current.parts.get(slot).is_none_or(|p| {
                    p.path != part.path || p.program != part.program
                        || p.streaming != part.streaming || p.streaming(current.streaming) != streaming
                })
                    || params.shared.generation[slot].load(Ordering::Acquire) != generation
            };
            if part.path.is_empty() {
                params.shared.view.lock().unwrap().parts[slot] = PartView {
                    attempted: Some(target),
                    streaming,
                    ..Default::default()
                };
                let _ = params.shared.ready.force_push((
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
            trace.detail("sample_rate", params.shared.rate());
            trace.detail("streaming_requested", format!("{streaming:?}"));
            let set_stage = |trace: &mut crate::diagnostics::LoadTrace, name: &'static str| {
                trace.stage(name);
                params.shared.view.lock().unwrap().parts[slot].status = format!("Loading {name}…");
            };
            let result = (|| -> anyhow::Result<_> {
                set_stage(&mut trace, "import");
                anyhow::ensure!(!canceled(), "Instrument load canceled");
                let instrument = import::shared_program(Path::new(&part.path), part.program)?;
                trace.detail("groups", instrument.groups.len());
                trace.detail("zones_total", instrument.zones.len());
                trace.detail("script_slots", instrument.scripts.len());
                trace.detail("missing_samples", instrument.missing_samples.len());
                for w in &instrument.warnings { trace.issue("import", crate::diagnostics::code(w), w); }
                for name in &instrument.missing_samples { trace.issue("samples", "missing", name); }
                anyhow::ensure!(!canceled(), "Instrument load canceled");
                // Progress by phase: parsed 5%, scripts 10%, the bank the rest.
                let progress = &params.shared.load_progress[slot];
                progress.fetch_max(crate::engine::LOAD_DONE / 20, Ordering::Relaxed);
                set_stage(&mut trace, "scripts");
                let (script, snapshot, script_errors) =
                    scripts(&instrument, &part.script_state, params.shared.rate());
                for e in script_errors { trace.issue("scripts", "initialization_failed", e); }
                if let Some(rt) = script.as_deref() {
                    for d in rt.diagnostics() { trace.issue("scripts", crate::diagnostics::code(&d), d); }
                }
                anyhow::ensure!(!canceled(), "Instrument load canceled");
                set_stage(&mut trace, "artwork");
                progress.fetch_max(crate::engine::LOAD_DONE / 10, Ordering::Relaxed);
                {
                    let needs_art = {
                        let view = params.shared.view.lock().unwrap();
                        let v = &view.parts[slot];
                        v.program != part.program
                            || v.instrument
                                .as_ref()
                                .is_none_or(|i| i.path != instrument.path)
                    };
                    let parsed = needs_art.then(|| script_interface(script.as_deref()));
                    let art = parsed.as_ref().map(|parsed| {
                        let interface = parsed.interface.as_deref();
                        let wallpaper = artwork::performance(
                            &instrument,
                            interface,
                        );
                        let names =
                            interface
                                .into_iter()
                                .flat_map(|u| &u.controls)
                                .filter_map(|c| match c.properties.get("$CONTROL_PAR_PICTURE") {
                                    Some(crate::ksp::Value::Text(name)) => Some(name.as_str()),
                                    _ => None,
                                });
                        let (pictures, errors) = artwork::pictures_report(&instrument.path, names);
                        for e in errors { trace.issue("artwork", crate::diagnostics::code(&e), e); }
                        if let Err(e) = &wallpaper { trace.issue("artwork", crate::diagnostics::code(e), e); }
                        trace.detail("pictures_loaded", pictures.len());
                        trace.detail("performance_view", interface.is_some_and(|u| u.performance));
                        trace.detail("controls", interface.map_or(0, |u| u.controls.len()));
                        for c in interface.into_iter().flat_map(|u| &u.controls) {
                            if let Some(reason) = crate::diagnostics::widget_limit(&c.kind) {
                                trace.issue("ui", "unsupported", format!("{} ({}): {reason}", c.kind, c.variable));
                            }
                        }
                        (wallpaper, pictures)
                    });
                    let mut view = params.shared.view.lock().unwrap();
                    let v = &mut view.parts[slot];
                    v.instrument = Some(instrument.clone());
                    v.program = part.program;
                    if let Some(parsed) = parsed {
                        v.interface = parsed.interface;
                        v.script_slot = parsed.slot;
                        v.interface_status = parsed.status;
                        v.keys = parsed.keys;
                    }
                    if let Some((art, pictures)) = art {
                        v.pictures = Arc::new(pictures);
                        match art {
                            Ok(image) => {
                                v.wallpaper = image;
                                v.wallpaper_status.clear();
                            }
                            Err(e) => {
                                v.wallpaper = None;
                                v.wallpaper_status = e;
                            }
                        }
                    }
                }
                set_stage(&mut trace, "samples");
                if instrument.zones.is_empty() {
                    trace.issue("samples", "unsupported", "Controller instrument has no sample bank; standalone controller playback is unavailable");
                    return Ok((instrument, None, script, snapshot, None));
                }
                // Every group plays; the stored group only selects what the mapping inspector shows.
                if part.group == u32::MAX {
                    let group = instrument.first_playable_group().unwrap_or(0);
                    let mut current = params.selection.write().unwrap();
                    if let Some(c) = current.parts.get_mut(slot).filter(|c| {
                        c.path == part.path && c.program == part.program && c.group == u32::MAX
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
                // RAM only plays from a streaming bank while the RAM fills.
                let bank = Box::new(Bank::load_cancelable(
                    &instrument,
                    budget,
                    Streaming::Auto,
                    controllers,
                    &params.shared.load_progress[slot],
                    &canceled,
                )?);
                let fill = (streaming == Streaming::RamOnly).then(|| (budget, controllers.to_vec()));
                Ok((instrument, Some(bank), script, snapshot, fill))
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
            let result = result.map(|(instrument, bank, script, snapshot, fill)| {
                let live = script.as_deref().map(|rt| Box::new(rt.live()));
                let rate = params.shared.rate();
                let irs = script.as_deref().map_or(Vec::new(), |rt| rt.init_irs.clone());
                let fx = crate::engine::effects(&instrument, script.as_deref(), rate as f32);
                (instrument, bank, script, snapshot, fill, live, fx, rate, irs)
            });
            if canceled() {
                let report = trace.finish("canceled");
                let mut view = params.shared.view.lock().unwrap();
                view.parts[slot].loading = false;
                view.parts[slot].load_report = Some(report);
                continue;
            }
            let status = match &result {
                Ok((_, bank, _, _, _, _, _, _, _)) => {
                    if let Some(b) = bank.as_deref() {
                        trace.detail("samples_loaded", b.sample_count());
                        trace.detail("samples_streamed", b.streamed_samples());
                        trace.detail("zones_playable", b.zones().len());
                        trace.detail("zones_skipped", b.skipped_zones);
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
                Ok((instrument, bank, script, snapshot, fill, live, fx, rate, irs)) => {
                    let epoch = if script.is_some() {
                        next_epoch(&mut view, slot, snapshot, live)
                    } else {
                        0
                    };
                    let mut bank = bank;
                    let residency = bank.as_mut().and_then(|b| b.take_residency());
                    params.shared.residency.lock().unwrap()[slot] =
                        residency.map(|r| (generation, r));
                    let v = &mut view.parts[slot];
                    v.script_state = part.script_state.clone();
                    v.active = instrument.name.clone();
                    v.bytes = bank.as_ref().map(|b| b.bytes).unwrap_or(0);
                    v.freed = 0;
                    v.status = bank.as_deref().map(bank_status).unwrap_or_else(|| {
                        "Controller instrument · KSP playback unavailable".into()
                    });
                    v.fx_rate = rate;
                    v.irs = irs;
                    let _ = params.shared.ready.force_push((
                        slot,
                        generation,
                        Handoff::Part {
                            bank,
                            fx,
                            script,
                            epoch,
                        },
                    ));
                    // RAM only: the part plays, streaming, while every sample
                    // loads whole; the resident bank then takes over and
                    // playing voices carry on from it.
                    // ponytail: the streaming bank stays resident until the fill
                    // lands (its preload twice over at peak); fill per sample
                    // into the playing bank if that peak matters.
                    if let Some((budget, controllers)) = fill {
                        v.status += " · loading into RAM…";
                        let parent_id = v.load_report.as_ref().map(|r| r["load_id"].clone());
                        drop(view);
                        let mut trace = crate::diagnostics::LoadTrace::new(&instrument.path, part.program, Some(slot));
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
                        );
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
                            if report["status"] != "loaded" { load["status"] = "partial".into(); }
                            load["ram_fill"] = (*report).clone();
                        }
                        match bank {
                            Ok(bank) => {
                                // Loaded whole: nothing left to resize.
                                params.shared.residency.lock().unwrap()[slot] = None;
                                (v.bytes, v.status) = (bank.bytes, bank_status(&bank));
                                let bank = Handoff::Bank(Box::new(bank));
                                let _ = params.shared.ready.force_push((slot, generation, bank));
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
        // The host changed sample rate since these effects were built: rebuild them here, off the audio thread.
        let rate = params.shared.rate();
        for slot in 0..RACK_SLOTS {
            let stale = {
                let view = params.shared.view.lock().unwrap();
                let v = &view.parts[slot];
                v.instrument
                    .clone()
                    .filter(|_| v.fx_rate != 0. && v.fx_rate != rate)
                    .map(|i| (i, v.irs.clone()))
            };
            let Some((instrument, irs)) = stale else { continue };
            let fx = instrument.fx.processor_with(rate as f32, MAX_BLOCK, &irs);
            params.shared.view.lock().unwrap().parts[slot].fx_rate = rate;
            let _ = params.shared.ready.force_push((
                slot,
                params.shared.generation[slot].load(Ordering::Acquire),
                Handoff::Fx(fx),
            ));
        }
        // Save script values the audio thread reported, then lend the buffers out again.
        while let Some((slot, epoch, mut snapshot, changed)) = params.shared.snapshots.pop() {
            let mut view = params.shared.view.lock().unwrap();
            let v = &mut view.parts[slot];
            if epoch == 0 || epoch != v.script_epoch {
                continue;
            }
            // Unchanged: the saved JSON already holds it, unless nothing is
            // saved yet. Serializing megabytes of script tables ten times a
            // second was most of the loader's time.
            if !changed && !v.script_state.is_empty() {
                v.snapshot = Some(snapshot);
                continue;
            }
            if !crate::ksp::settle_persistence(&mut snapshot) {
                v.snapshot = Some(snapshot);
                continue;
            }
            let json = serde_json::to_string(&*snapshot).unwrap_or_default();
            v.snapshot = Some(snapshot);
            if json == v.script_state {
                continue;
            }
            // The part as the rack names it: the instrument's own path is
            // canonical, and a relative or symlinked part path never matched
            // it, so the saved values never reached the part and the next
            // round rebuilt its scripts from stale ones, ten times a second.
            let target = v.attempted.clone();
            v.script_state = json.clone();
            drop(view);
            let mut current = params.selection.write().unwrap();
            if let Some(p) = current
                .parts
                .get_mut(slot)
                .filter(|p| target.as_ref() == Some(&(p.path.clone(), p.program)))
            {
                p.script_state = json;
            }
        }
        // Show what the scripts changed since the last round.
        while let Some((slot, epoch, live)) = params.shared.lives.pop() {
            let mut view = params.shared.view.lock().unwrap();
            let v = &mut view.parts[slot];
            if epoch == 0 || epoch != v.script_epoch {
                continue;
            }
            if live.refresh_interface && v.interface.as_deref() != live.interface.as_ref() {
                v.interface = live.interface.clone().map(Arc::new);
            }
            if live.refresh_interface && *v.keys != live.keys {
                v.keys = Arc::new(live.keys.clone());
            }
            let runtime = serde_json::json!({"faults":live.faults,"notes":live.notes});
            let mut new_issues = Vec::new();
            if let Some(report) = &mut v.load_report {
                if report["runtime"] != runtime {
                    for fault in runtime["faults"].as_array().into_iter().flatten() {
                        let known = report["runtime"]["faults"].as_array().into_iter().flatten().any(|old|
                            old["slot"] == fault["slot"] && old["line"] == fault["line"] && old["message"] == fault["message"]
                        );
                        if !known { new_issues.push(fault.clone()); }
                    }
                    for note in runtime["notes"].as_array().into_iter().flatten() {
                        if !report["runtime"]["notes"].as_array().is_some_and(|old| old.contains(note)) {
                            new_issues.push(serde_json::json!({"message":note}));
                        }
                    }
                    let report = Arc::make_mut(report);
                    if !live.faults.is_empty() || !live.notes.is_empty() { report["status"] = "partial".into(); }
                    report["runtime"] = runtime;
                    v.runtime_status = live.faults.iter().map(|f| format!("Slot {} line {}: {} ({}x)", f.slot, f.line, f.message, f.count))
                        .chain(live.notes.iter().map(|n| (*n).to_owned())).collect::<Vec<_>>().join("\n");
                }
            }
            let path = v.instrument.as_ref().map(|i| i.path.clone());
            let load_id = v.load_report.as_ref().and_then(|r| r["script_restore"]["load_id"].as_str().or_else(|| r["load_id"].as_str())).map(str::to_owned);
            let program = v.program;
            v.script_slot = live.slot;
            v.live = Some(live);
            drop(view);
            if let Some(path) = path { crate::diagnostics::runtime(&path, program, slot, epoch, load_id.as_deref(), &new_issues); }
        }
        smart_memory(&params.shared);
        align(params);
        if params.shared.watched.swap(false, Ordering::Relaxed) {
            params.shared.view.lock().unwrap().watched_at = Some(Instant::now());
        }
        for slot in 0..RACK_SLOTS {
            let mut view = params.shared.view.lock().unwrap();
            // Live views only while an editor shows them: refreshing script
            // interfaces no one sees was most of an idle rack's audio work.
            let shown = view.watched_at.is_some_and(|t| t.elapsed() < LIVE_WATCH);
            let v = &mut view.parts[slot];
            if (shown || v.diagnostics_lent.is_none_or(|t| t.elapsed().as_secs() >= 1))
                && let Some(mut live) = v.live.take()
            {
                live.refresh_interface = shown;
                match params.shared.live_requests.push((slot, live)) {
                    Ok(()) => v.diagnostics_lent = Some(Instant::now()),
                    Err((_, live)) => v.live = Some(live),
                }
            }
            // Once a second: saving is all it is for, and big script tables
            // cost the audio thread a while to copy.
            let v = &mut view.parts[slot];
            if v.snapshot_lent.is_none_or(|t| t.elapsed() >= SNAPSHOT_EVERY)
                && let Some(snapshot) = v.snapshot.take()
            {
                match params.shared.snapshot_requests.push((slot, snapshot)) {
                    Ok(()) => v.snapshot_lent = Some(Instant::now()),
                    Err((_, snapshot)) => v.snapshot = Some(snapshot),
                }
            }
        }
    }
}
#[derive(Default)]
pub struct Dsp {
    /// Boxed: the rack is ~300 KB, too big for a host thread's stack.
    rack: Box<Rack>,
    until_poll: usize,
    audition_left: [usize; RACK_SLOTS],
    /// Epoch of each slot's installed runtime, returned with its persistence snapshots.
    script_epoch: [u64; RACK_SLOTS],
    /// The lent live view and persistence snapshot being refreshed, a budget
    /// a block: slot, epoch and [`Runtime::changes`] at the start, buffer, progress.
    live: Option<Lent<Box<Live>>>,
    snapshot: Option<Lent<Box<Vec<Persisted>>>>,
    /// Per slot, epoch and changes the buffers last refreshed whole hold:
    /// while the scripts do not run they are current and need no refresh.
    live_seen: [(u64, u64); RACK_SLOTS],
    snapshot_seen: [(u64, u64); RACK_SLOTS],
    routers: [Router; RACK_SLOTS],
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
}
struct KeySlots([u32; 128]);
impl Default for KeySlots {
    fn default() -> Self {
        Self([0; 128])
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
        if shared.generation[slot].load(Ordering::Acquire) != *generation {
            *entry = None;
            continue;
        }
        if let Some(heads) = r.poll(shared.underruns[slot].load(Ordering::Relaxed))
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
    /// [`routing::apply`] with what the audio thread last saw the parts'
    /// instruments route past their outputs, named from the instruments.
    pub(crate) fn reroute(&self, selection: &mut Selection) {
        let mics: routing::Mics = self.mics.each_ref().map(|m| m.each_ref().map(|c| c.load(Ordering::Relaxed)));
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
        for (slot, part) in selection.parts.iter().enumerate().take(RACK_SLOTS) {
            if part.path.is_empty() || part.timing.measured(&part.path, part.program) {
                continue;
            }
            let instrument = {
                let view = shared.view.lock().unwrap();
                let v = &view.parts[slot];
                let ready = v.attempted == Some((part.path.clone(), part.program)) && !v.loading && v.bytes > 0;
                v.instrument.clone().filter(|i| ready && i.path == Path::new(&part.path) && v.program == part.program)
            };
            let Some(instrument) = instrument else { continue };
            if shared.measure.busy[slot].swap(true, Ordering::AcqRel) {
                continue;
            }
            let measure = shared.measure.clone();
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
                measure.busy[slot].store(false, Ordering::Release);
            });
            if spawned.is_err() {
                shared.measure.busy[slot].store(false, Ordering::Release);
            }
        }
    }
    let plan = plan(&selection);
    let mut published = shared.published.lock().unwrap();
    let (sent, waiting) = &mut *published;
    if *sent == Some(plan) {
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
        *sent = Some(plan);
        *waiting = None;
        let _ = shared.plan.force_push(plan);
        shared.reported.store(told(&plan).to_bits(), Ordering::Relaxed);
    }
}
/// What the audio thread aligns by, from the persisted rack.
pub(crate) fn plan(selection: &Selection) -> Plan {
    let parts = || selection.parts.iter().take(RACK_SLOTS).filter(|p| !p.path.is_empty());
    let latency_ms = timing::reported_ms(parts().map(|p| p.timing.latest()));
    Plan {
        on: selection.auto_align,
        transport_only: selection.align_transport_only,
        latency_ms,
        parts: std::array::from_fn(|n| {
            selection.parts.get(n).filter(|p| !p.path.is_empty()).map_or_else(Holds::default, |p| {
                let a = &p.articulate;
                let names: Vec<&str> =
                    if a.source == p.path { a.articulations.iter().map(|a| a.name.as_str()).collect() } else { Vec::new() };
                Holds::of(&p.timing, &names, latency_ms)
            })
        }),
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
            " · {} zones skipped (missing or unreadable)",
            bank.skipped_zones
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
        s.until_poll = 0;
        s.audition_left.fill(0);
        s.align.clear();
        // The voices are gone, and the host's releases for them may be too.
        for lit in &p.shared.heard {
            lit.store(0, Ordering::Relaxed);
        }
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
            for (engine, mics) in s.rack.parts.iter().zip(&p.shared.mics) {
                for (m, out) in mics.iter().zip(outs_of(engine)) {
                    m.store(out, Ordering::Relaxed);
                }
            }
            s.until_poll = (rate * 0.1) as usize;
        } else {
            s.until_poll -= frames;
        }
        if let Some(controls) = p.shared.controls.pop() {
            s.rack.set_controls(controls);
        }
        if let Some(plan) = p.shared.plan.pop() {
            s.align.plan = plan;
        }
        // Held back only while aligning; once not, what was held plays at once.
        let holding = s.align.holding(cx.transport.playing);
        if let Some(routes) = p.shared.routes.pop() {
            for (r, route) in s.routers.iter_mut().zip(routes) {
                r.set_route(route);
            }
        }
        // Retired banks, effects and scripts go back to the loader thread to be
        // freed; stop while it cannot take more.
        while !p.shared.discard.is_full() {
            let Some((slot, generation, handoff)) = p.shared.ready.pop() else {
                break;
            };
            let engine = &mut s.rack.parts[slot];
            let current = generation == p.shared.generation[slot].load(Ordering::Acquire);
            let retired = match handoff {
                Handoff::Part {
                    bank,
                    fx,
                    script,
                    epoch,
                } if current => {
                    engine.reset(rate);
                    s.script_epoch[slot] = epoch;
                    Retired {
                        script: engine.set_script(script),
                        bank: engine.set_bank(bank),
                        fx: Some(engine.set_fx(fx)),
                        heads: None,
                    }
                }
                Handoff::Fx(fx) if current => Retired {
                    fx: Some(engine.set_fx(fx)),
                    ..Retired::default()
                },
                Handoff::Bank(bank) if current => Retired {
                    bank: engine.upgrade_bank(bank),
                    ..Retired::default()
                },
                Handoff::Script { script, epoch } if current => {
                    engine.reset(rate);
                    s.script_epoch[slot] = epoch;
                    Retired {
                        script: engine.set_script(script),
                        ..Retired::default()
                    }
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
                Handoff::Script { script, .. } => Retired {
                    script,
                    ..Retired::default()
                },
            };
            let _ = p.shared.discard.push(retired);
        }
        let scripted = s.rack.parts.iter().filter(|e| e.script().is_some()).count();
        for engine in &mut s.rack.parts {
            engine.begin_audio_block(frames, scripted, offline);
        }
        if !holding && s.align.next_due().is_some() {
            s.align.flush(&mut s.rack, &mut s.routers);
        }
        while let Some((slot, o)) = p.shared.overrides.pop() {
            if let Some(engine) = s.rack.parts.get_mut(slot) {
                engine.set_override(o);
            }
        }
        while let Some(e) = p.shared.edits.pop() {
            if e.epoch != 0 && e.epoch == s.script_epoch[e.part] {
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
                .map(|(slot, live)| (slot, version(s, slot), live, Refresh::default()));
        }
        if let Some((slot, seen, live, at)) = &mut s.live {
            let done = seen.0 != s.script_epoch[*slot]
                || (s.rack.parts[*slot].script()).is_none_or(|rt| {
                    rt.refresh_diagnostics(live);
                    (*seen == s.live_seen[*slot] && live.refresh_interface && live.interface_current)
                        || rt.refresh_live_within(live, at, LIVE_BUDGET)
                });
            if done && let Some((slot, seen, live, _)) = s.live.take() {
                s.live_seen[slot] = seen;
                let _ = p.shared.lives.push((slot, seen.0, live));
            }
        }
        if s.snapshot.is_none() && !p.shared.snapshots.is_full() {
            s.snapshot = (p.shared.snapshot_requests.pop())
                .map(|(slot, saved)| (slot, version(s, slot), saved, Refresh::default()));
        }
        if let Some((slot, seen, saved, at)) = &mut s.snapshot {
            let done = seen.0 != s.script_epoch[*slot]
                || *seen == s.snapshot_seen[*slot]
                || (s.rack.parts[*slot].script())
                    .is_none_or(|rt| rt.refresh_persistence_within(saved, at, REFRESH_BUDGET));
            if done && let Some((slot, seen, saved, at)) = s.snapshot.take() {
                s.snapshot_seen[slot] = seen;
                let _ = p.shared.snapshots.push((slot, seen.0, saved, at.changed));
            }
        }
        for (e, r) in s.rack.parts.iter_mut().zip(&s.routers) {
            e.attack = p.attack.value();
            e.release = p.release.value();
            e.cutoff = p.cutoff.value() * r.cutoff_scale();
        }
        if p.shared.panic.swap(false, Ordering::AcqRel) {
            while p.shared.keyboard.pop().is_some() {}
            for owner in &p.shared.key_owners {
                owner.store(128, Ordering::Release);
            }
            for lit in p.shared.played.iter().chain(&p.shared.heard) {
                lit.store(0, Ordering::Relaxed);
            }
            for channel in 0..16 {
                s.rack.cc(channel, 120, 0);
                s.rack.cc(channel, 121, 0);
            }
            s.audition_left.fill(0);
        }
        while let Some((slot, play)) = p.shared.keyboard.pop() {
            if slot == EVERY_PART {
                // As host MIDI on port A, channel 1 plays it.
                let (rack, routers) = (&mut s.rack, &mut s.routers);
                match play {
                    Play::Note(note, 0) => {
                        let slots = std::mem::take(&mut s.key_slots.0[note as usize & 127]);
                        articulate::dispatch_to(rack, routers, slots, In::NoteOff(0, note));
                    }
                    Play::Note(note, velocity) => {
                        s.key_slots.0[note as usize & 127] = articulate::dispatch(rack, routers, 0, In::NoteOn(0, note, velocity));
                    }
                    Play::Bend(value) => drop(articulate::dispatch(rack, routers, 0, In::Bend(0, value))),
                    Play::Mod(value) => drop(articulate::dispatch(rack, routers, 0, In::Cc(0, 1, value))),
                }
                continue;
            }
            let channel = preview_channel(&s.rack.parts[slot.min(RACK_SLOTS - 1)]);
            let ev = match play {
                Play::Note(note, 0) => In::NoteOff(s.key_channels.0[note as usize & 127], note),
                Play::Note(note, velocity) => {
                    s.key_channels.0[note as usize & 127] = channel;
                    In::NoteOn(channel, note, velocity)
                }
                Play::Bend(value) => In::Bend(channel, value),
                Play::Mod(value) => In::Cc(channel, 1, value),
            };
            articulate::play(&mut s.rack, &mut s.routers, slot, ev);
        }
        // With no part selected there is none to audition.
        let selected = p.shared.selected.load(Ordering::Relaxed) as usize;
        if p.shared.audition.swap(false, Ordering::AcqRel) && selected < RACK_SLOTS {
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
            e.note_on(channel, note, velocity);
            s.audition_left[slot] = (rate * 1.5) as usize;
        }

        let channels = b.num_output_channels();
        let thru = p.shared.midi_thru.load(Ordering::Relaxed);
        let mut peak = [0f32; 2];
        let mut gains = [0f32; MAX_BLOCK];
        let scope = p.shared.scope.source.load(Ordering::Relaxed);
        let (mut at, mut next) = (0, 0);
        loop {
            // Apply events due now; once the buffer is rendered, apply any stragglers.
            while let Some(e) = events
                .get(next)
                .filter(|e| at >= frames || e.sample_offset as usize <= at)
            {
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
                if let Some(ev) = In::from_event(&e.body) {
                    // The on-screen keys and wheels follow what the host plays.
                    let lit = |note: u8, velocity| {
                        if let Some(lit) = p.shared.heard.get(note as usize) {
                            lit.store(velocity, Ordering::Relaxed);
                        }
                    };
                    match ev {
                        In::NoteOn(_, note, velocity) => lit(note, velocity),
                        In::NoteOff(_, note) => lit(note, 0),
                        // All sound or all notes off, as a host sends on stop.
                        In::Cc(_, 120 | 123, _) => (0..128).for_each(|note| lit(note, 0)),
                        In::Bend(_, value) => p.shared.bend.store(u32::from(value), Ordering::Relaxed),
                        In::Cc(_, 1, value) => {
                            p.shared.modulation.store(u32::from(value), Ordering::Relaxed)
                        }
                        _ => {}
                    }
                    if holding {
                        let arrived = s.align.clock + e.sample_offset as u64;
                        s.align.arrive(&mut s.rack, &mut s.routers, e.port, ev, arrived, rate);
                    } else {
                        articulate::dispatch(&mut s.rack, &mut s.routers, e.port, ev);
                    }
                }
                next += 1;
            }
            if at >= frames {
                break;
            }
            let now = s.align.clock + at as u64;
            let mut due = events
                .get(next)
                .map_or(frames, |e| (e.sample_offset as usize).min(frames));
            if holding {
                s.align.release(now, &mut s.rack, &mut s.routers);
                if let Some(held) = s.align.next_due() {
                    due = due.min(at + (held - now).min(frames as u64) as usize);
                }
            }
            let len = (due - at).min(MAX_BLOCK);
            for (e, left) in s.rack.parts.iter_mut().zip(&mut s.audition_left) {
                if *left > 0 {
                    *left = left.saturating_sub(len);
                    if *left == 0 {
                        for channel in 0..16 {
                            e.cc(channel, 123, 0);
                        }
                    }
                }
            }
            for gain in &mut gains[..len] {
                *gain = db_to_linear(p.volume.read());
            }
            let ports = s.rack.bus_controls.map(|c| usize::from(c.port));
            s.rack.tap = scope.checked_sub(1).filter(|&slot| slot < RACK_SLOTS);
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
        p.shared.blocks.fetch_add(1, Ordering::Relaxed);
        s.align.clock += frames as u64;
        cx.set_meter(P::Level, peak[0].max(peak[1]).min(1.0));
        if frames > 0 && rate > 0. {
            let fall = 0.1f32.powf(frames as f32 / rate as f32);
            let m = &p.shared.meters;
            let peaks = std::mem::take(&mut s.rack.peaks);
            for ((meter, peak), clip) in m.parts.iter().zip(peaks.parts).zip(&m.clips.parts) {
                Meters::publish(meter, peak, fall, clip);
            }
            for ((meter, peak), clip) in m.buses.iter().zip(peaks.buses).zip(&m.clips.buses) {
                Meters::publish(meter, peak, fall, clip);
            }
            Meters::publish(&m.master, peak, fall, &m.clips.master);
        }
        let watch = p.shared.probe.watch.load(Ordering::Relaxed);
        if let Some((slot, group)) = Probe::watched(watch).filter(|(slot, _)| *slot < RACK_SLOTS) {
            let published = if s.rack.parts[slot].publish(group, &p.shared.probe) { watch } else { 0 };
            p.shared.probe.published.store(published, Ordering::Relaxed);
        }
        let voices: usize = s.rack.parts.iter().map(Engine::active_voices).sum();
        p.shared.voices.store(voices as u64, Ordering::Relaxed);
        let audible: usize = s.rack.parts.iter().map(Engine::audible_voices).sum();
        p.shared.audible.store(audible as u64, Ordering::Relaxed);
        let dropouts: u64 = (s.rack.parts.iter())
            .map(|e| e.underruns() + e.dropped_commands())
            .sum();
        p.shared.dropouts.store(dropouts, Ordering::Relaxed);
        for (e, late) in s.rack.parts.iter().zip(&p.shared.underruns) {
            late.store(e.underruns(), Ordering::Relaxed);
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
        ProcessStatus::Normal
    }
    fn editor(params: Arc<SamplerParams>) -> Box<dyn Editor> {
        crate::ui::editor(params)
    }
    /// Auto-align's latency: the latest part's attack (see `timing.rs`).
    fn latency(s: &Dsp) -> u32 {
        s.align.plan.latency(s.rack.parts[0].rate())
    }
}
/// What the UI shows of initialized scripts: the performance view (the last slot with one),
/// its script slot, their issues, and the keyboard the scripts set.
pub(crate) fn script_interface(rt: Option<&Runtime>) -> ScriptView {
    let Some(rt) = rt else {
        return ScriptView::default();
    };
    let live = rt.live();
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

moose::plugin! { logic:Sampler, params:SamplerParams, tasks:[Load] }

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
    // own MIDI channel) releases its last chord and starts 3 or 4 notes at once.
    let chords = paths.iter().any(|p| p == "--chords");
    let fifo = paths.iter().any(|p| p == "--fifo");
    #[cfg(not(target_os = "linux"))]
    anyhow::ensure!(!fifo, "--fifo is only supported on Linux");
    let paths: Vec<_> = paths.iter().filter(|p| !p.starts_with("--")).cloned().collect();
    let paths = &paths[..];
    anyhow::ensure!(
        (1..=RACK_SLOTS).contains(&paths.len()),
        "Benchmark requires 1..16 instruments"
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
            channel: if chords { i as i16 } else { -1 },
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
                    let channel = part as u8;
                    for key in chord_held[part].drain(..) {
                        events.push(Event::new(at, EventBody::NoteOff { group: 0, channel, note: key, velocity: 0 }));
                    }
                    for n in 0..3 + (started + part) % 2 {
                        let key = keys[(started * 5 + n * 4) % keys.len()];
                        events.push(Event::new(at, EventBody::NoteOn { group: 0, channel, note: key, velocity: 90 }));
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
mod tests {
    use super::*;

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
    fn allocations(f: impl FnOnce()) -> usize {
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
                Some(Box::new(rt.persistence())),
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
            shown.controls[0].properties["$CONTROL_PAR_VALUE"],
            crate::ksp::Value::Int(1),
            "the view shows an edit at once"
        );
        let live = p.shared.view.lock().unwrap().parts[0].live.take().unwrap();
        p.shared.live_requests.push((0, live)).ok().unwrap();

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
        p.shared.live_requests.push((slot, live)).ok().unwrap();
        let saved = Box::new(dsp.rack.parts[0].script().unwrap().persistence());
        p.shared.snapshot_requests.push((0, saved)).ok().unwrap();
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
                loop_range: Some(Loop { start: 0, end: 100, until_release: false, crossfade: 0 }),
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
        let looped = Some(Loop { start: 0, end: 100, until_release: false, crossfade: 0 });
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
                        timing: Timing { loaded: attack(ms), ..Default::default() },
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
}
