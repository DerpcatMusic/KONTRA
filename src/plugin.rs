use crate::articulate::{self, Articulate, In, Mpe, Route, Router};
use crate::artwork;
use crate::{
    engine::{
        BUSES, Bank, BusControls, Engine, MAX_BLOCK, Mix, NO_AUX, PartControls, RACK_SLOTS, Rack,
        Streaming, TUNE_RANGE, load_scripts,
    },
    fx::FxProcessor,
    import::{self, Instrument},
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
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
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
    rate: AtomicU64,
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
    generation: [AtomicU64; RACK_SLOTS],
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
    pub(crate) view: Mutex<View>,
    /// Voices sounding across the rack, reported by the audio thread.
    pub(crate) voices: AtomicU64,
    /// Of those, the ones not muted by their scripts: the ones rendered.
    pub(crate) audible: AtomicU64,
    /// Audio thread load (`f32` bits): render time over block time, peak-held.
    pub(crate) cpu: AtomicU64,
    /// Voice blocks whose streamed samples were not read in time (played
    /// silent) plus script engine calls dropped by a full queue, summed over
    /// the parts' engines since each was created.
    pub(crate) dropouts: AtomicU64,
}
#[derive(Default, Clone)]
pub(crate) struct PartView {
    pub(crate) program: u32,
    pub(crate) interface: Option<Arc<crate::ksp::Interface>>,
    pub(crate) interface_status: String,
    pub(crate) wallpaper: Option<Arc<Image>>,
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
    /// Rate of the effects handed to the audio thread; 0 when none were.
    pub(crate) fx_rate: f64,
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
}
#[derive(Clone)]
pub(crate) struct View {
    /// Last script epoch handed out; tags runtimes so stale persistence snapshots are ignored.
    pub(crate) script_epoch: u64,
    pub(crate) multi_status: String,
    pub(crate) artwork: HashMap<String, Arc<Image>>,
    pub(crate) root: String,
    pub(crate) files: Arc<Vec<PathBuf>>,
    pub(crate) parts: [PartView; RACK_SLOTS],
    pub(crate) status: String,
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
            generation: std::array::from_fn(|_| AtomicU64::new(0)),
            load_progress: std::array::from_fn(|_| AtomicU32::new(0)),
            audition: AtomicBool::new(false),
            audition_note: AtomicU64::new(128),
            selected: AtomicU64::new(0),
            focus_request: AtomicU64::new(128),
            panic: AtomicBool::new(false),
            midi_thru: AtomicBool::new(false),
            multi_request: Mutex::new(None),
            voices: AtomicU64::new(0),
            audible: AtomicU64::new(0),
            cpu: AtomicU64::new(0),
            dropouts: AtomicU64::new(0),
            view: Mutex::new(View {
                script_epoch: 0,
                multi_status: String::new(),
                artwork: Default::default(),
                root: String::new(),
                files: Arc::default(),
                parts: std::array::from_fn(|_| PartView::default()),
                status: "Choose a library and select a preset".into(),
            }),
        }
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
}
impl Meters {
    pub fn read(meter: &[AtomicU32; 2]) -> [f32; 2] {
        meter
            .each_ref()
            .map(|m| f32::from_bits(m.load(Ordering::Relaxed)))
    }

    /// Hold `peak` or let the shown level fall by `fall`.
    fn publish(meter: &[AtomicU32; 2], peak: [f32; 2], fall: f32) {
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
            })
            .unwrap_or_default()
    })
}
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
}
/// What the audio thread replaced, freed on the loader thread.
#[expect(dead_code, reason = "held only to be dropped off the audio thread")]
#[derive(Default)]
struct Retired {
    bank: Option<Box<Bank>>,
    fx: Option<FxProcessor>,
    script: Option<Box<Runtime>>,
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
    script: Option<&Runtime>,
) -> u64 {
    view.script_epoch += 1;
    let v = &mut view.parts[slot];
    v.script_epoch = view.script_epoch;
    v.snapshot = snapshot;
    v.snapshot_lent = None;
    v.live = script.map(|rt| Box::new(rt.live()));
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

    /// Ask the loader to replace the rack with the multi at `path`.
    pub(crate) fn queue_multi(&self, path: String) {
        self.view.lock().unwrap().multi_status = "Loading multi…".into();
        *self.multi_request.lock().unwrap() = Some(path);
    }

    /// Start `note` on `slot` at `velocity` (1..=127) from the on-screen keyboard.
    pub(crate) fn press_key(&self, slot: usize, note: u8, velocity: u8) {
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
        if owner < RACK_SLOTS as u64
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
/// A rack KONTAKTO saved: a `.kontakto-multi` file, JSON. An NKM would have
/// to embed every instrument's program, which nothing here writes, so this
/// names the instruments instead:
///
/// ```json
/// { "format": "kontakto-multi", "version": 1, "name": "Evening",
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
    const FORMAT: &str = "kontakto-multi";

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
        anyhow::ensure!(multi.format == Self::FORMAT, "Not a KONTAKTO multi");
        anyhow::ensure!(multi.version <= 1, "Saved by a newer KONTAKTO");
        Ok(multi)
    }
}

pub struct Load;
impl BackgroundTask for Load {
    type Params = SamplerParams;
    const SERIALIZED: bool = true;
    fn run(self, params: &SamplerParams) {
        while params.shared.discard.pop().is_some() {}
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
                    let mut current = params.selection.write().unwrap();
                    if *current == before {
                        current.order = (0..parts.len() as u32).collect();
                        current.parts = parts;
                        current.multi = path;
                        params.shared.focus_request.store(0, Ordering::Release);
                        params.shared.view.lock().unwrap().multi_status = status;
                    } else {
                        params.shared.view.lock().unwrap().multi_status =
                            "Multi load canceled because the rack changed".into();
                    }
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
        let root = if selection.root.is_empty() {
            import::LIBRARY_ROOT
        } else {
            &selection.root
        };
        if params.shared.view.lock().unwrap().root != root {
            let result = import::presets(Path::new(root));
            let artwork = result
                .as_ref()
                .ok()
                .map(|f| artwork::scan(Path::new(root), f))
                .unwrap_or_default();
            let mut view = params.shared.view.lock().unwrap();
            view.root = root.into();
            view.artwork = artwork;
            match result {
                Ok(files) => {
                    view.files = Arc::new(files);
                    view.status = format!("{} presets", view.files.len());
                }
                Err(e) => {
                    view.files = Arc::default();
                    view.status = format!("Scan failed: {e:#}");
                }
            }
        }
        {
            let current = params.selection.read().unwrap();
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
                let (script, snapshot, _) =
                    scripts(&instrument, &part.script_state, params.shared.rate());
                let mut view = params.shared.view.lock().unwrap();
                let epoch = next_epoch(&mut view, slot, snapshot, script.as_deref());
                view.parts[slot].script_state = part.script_state.clone();
                let _ = params.shared.ready.force_push((
                    slot,
                    params.shared.generation[slot].load(Ordering::Acquire),
                    Handoff::Script { script, epoch },
                ));
                continue;
            }
            let cached = {
                let mut view = params.shared.view.lock().unwrap();
                let v = &mut view.parts[slot];
                if v.attempted.as_ref() == Some(&target) && v.streaming == streaming {
                    continue;
                }
                (v.attempted, v.streaming) = (Some(target.clone()), streaming);
                v.status = "Loading samples…".into();
                params.shared.load_progress[slot].store(0, Ordering::Relaxed);
                v.loading = true;
                v.script_epoch = 0;
                v.snapshot = None;
                v.live = None;
                v.instrument
                    .as_ref()
                    .filter(|i| i.path == Path::new(&part.path) && v.program == part.program)
                    .cloned()
            };
            let generation = params.shared.generation[slot].fetch_add(1, Ordering::AcqRel) + 1;
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
            let result = (|| -> anyhow::Result<_> {
                let instrument = if let Some(i) = cached {
                    i
                } else {
                    Arc::new(import::read_program(Path::new(&part.path), part.program)?)
                };
                // Progress by phase: parsed 5%, scripts 10%, the bank the rest.
                let progress = &params.shared.load_progress[slot];
                progress.fetch_max(crate::engine::LOAD_DONE / 20, Ordering::Relaxed);
                let (script, snapshot, _) =
                    scripts(&instrument, &part.script_state, params.shared.rate());
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
                            interface.map(|u| u.wallpaper.as_str()),
                        );
                        let names =
                            interface
                                .into_iter()
                                .flat_map(|u| &u.controls)
                                .filter_map(|c| match c.properties.get("$CONTROL_PAR_PICTURE") {
                                    Some(crate::ksp::Value::Text(name)) => Some(name.as_str()),
                                    _ => None,
                                });
                        (wallpaper, artwork::pictures(&instrument.path, names))
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
                if instrument.zones.is_empty() {
                    return Ok((instrument, None, script, snapshot));
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
                let resident: usize = params
                    .shared
                    .view
                    .lock()
                    .unwrap()
                    .parts
                    .iter()
                    .enumerate()
                    .filter(|(n, _)| *n != slot)
                    .map(|(_, p)| p.bytes)
                    .sum();
                // The part gets what the 2 GiB rack has left; a tight budget
                // streams more instead of failing.
                let budget = crate::engine::MEMORY_LIMIT
                    .min((2 * crate::engine::MEMORY_LIMIT).saturating_sub(resident));
                let bank = Box::new(Bank::load_counting(
                    &instrument,
                    budget,
                    streaming,
                    script.as_deref().map_or(&[], |rt| &rt.init_controllers),
                    &params.shared.load_progress[slot],
                )?);
                Ok((instrument, Some(bank), script, snapshot))
            })();
            let current = params.selection.read().unwrap();
            if current.parts.get(slot).map(|p| (&p.path, p.program)) != Some((&target.0, target.1))
            {
                params.shared.view.lock().unwrap().parts[slot].loading = false;
                continue;
            }
            drop(current);
            let mut view = params.shared.view.lock().unwrap();
            view.parts[slot].loading = false;
            match result {
                Ok((instrument, bank, script, snapshot)) => {
                    let epoch = if script.is_some() {
                        next_epoch(&mut view, slot, snapshot, script.as_deref())
                    } else {
                        0
                    };
                    let v = &mut view.parts[slot];
                    v.script_state = part.script_state.clone();
                    v.active = instrument.name.clone();
                    v.bytes = bank.as_ref().map(|b| b.bytes).unwrap_or(0);
                    v.status = bank.as_deref().map(bank_status).unwrap_or_else(|| {
                        "Controller instrument · KSP playback unavailable".into()
                    });
                    let rate = params.shared.rate();
                    v.fx_rate = rate;
                    let fx = instrument.fx.processor(rate as f32, MAX_BLOCK);
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
            };
            let Some(instrument) = stale else { continue };
            let fx = instrument.fx.processor(rate as f32, MAX_BLOCK);
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
            if v.interface.as_deref() != live.interface.as_ref() {
                v.interface = live.interface.clone().map(Arc::new);
            }
            if *v.keys != live.keys {
                v.keys = Arc::new(live.keys.clone());
            }
            v.script_slot = live.slot;
            v.live = Some(live);
        }
        for slot in 0..RACK_SLOTS {
            let mut view = params.shared.view.lock().unwrap();
            if let Some(live) = view.parts[slot].live.take()
                && let Err((_, live)) = params.shared.live_requests.push((slot, live))
            {
                view.parts[slot].live = Some(live);
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
    rack: Rack,
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
}
/// A lent buffer being refreshed: slot, epoch and changes at the start,
/// the buffer, progress.
type Lent<T> = (usize, (u64, u64), T, Refresh);
/// Script values copied into a lent buffer per block: large tables take
/// several blocks rather than one long one.
const REFRESH_BUDGET: usize = 16384;
const SNAPSHOT_EVERY: std::time::Duration = std::time::Duration::from_secs(1);
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
        if s.until_poll <= frames {
            if let Some(tasks) = cx.tasks::<Load>() {
                tasks.spawn_coalescing(Load);
            }
            s.until_poll = (rate * 0.1) as usize;
        } else {
            s.until_poll -= frames;
        }
        if let Some(controls) = p.shared.controls.pop() {
            s.rack.set_controls(controls);
        }
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
                    }
                }
                Handoff::Fx(fx) if current => Retired {
                    fx: Some(engine.set_fx(fx)),
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
                Handoff::Part {
                    bank, fx, script, ..
                } => Retired {
                    bank,
                    fx: Some(fx),
                    script,
                },
                Handoff::Fx(fx) => Retired {
                    fx: Some(fx),
                    ..Retired::default()
                },
                Handoff::Script { script, .. } => Retired {
                    script,
                    ..Retired::default()
                },
            };
            let _ = p.shared.discard.push(retired);
        }
        while let Some(e) = p.shared.edits.pop() {
            if e.epoch != 0 && e.epoch == s.script_epoch[e.part] {
                s.rack.parts[e.part].ui_control(e.slot, e.control, e.value);
                s.routers[e.part].forget();
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
                || *seen == s.live_seen[*slot]
                || (s.rack.parts[*slot].script())
                    .is_none_or(|rt| rt.refresh_live_within(live, at, LIVE_BUDGET));
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
            let channel = preview_channel(&s.rack.parts[slot.min(RACK_SLOTS - 1)]);
            let ev = match play {
                Play::Note(note, 0) => In::NoteOff(channel, note),
                Play::Note(note, velocity) => In::NoteOn(channel, note, velocity),
                Play::Bend(value) => In::Bend(channel, value),
                Play::Mod(value) => In::Cc(channel, 1, value),
            };
            articulate::play(&mut s.rack, &mut s.routers, slot, ev);
        }
        if p.shared.audition.swap(false, Ordering::AcqRel) {
            let slot = (p.shared.selected.load(Ordering::Relaxed) as usize).min(RACK_SLOTS - 1);
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
                        In::Bend(_, value) => p.shared.bend.store(u32::from(value), Ordering::Relaxed),
                        In::Cc(_, 1, value) => {
                            p.shared.modulation.store(u32::from(value), Ordering::Relaxed)
                        }
                        _ => {}
                    }
                    articulate::dispatch(&mut s.rack, &mut s.routers, e.port, ev);
                }
                next += 1;
            }
            if at >= frames {
                break;
            }
            let due = events
                .get(next)
                .map_or(frames, |e| (e.sample_offset as usize).min(frames));
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
            let (buses, live) = s.rack.render_live(len);
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
            at += len;
        }
        cx.set_meter(P::Level, peak[0].max(peak[1]).min(1.0));
        if frames > 0 && rate > 0. {
            let fall = 0.1f32.powf(frames as f32 / rate as f32);
            let m = &p.shared.meters;
            let peaks = std::mem::take(&mut s.rack.peaks);
            for (meter, peak) in m.parts.iter().zip(peaks.parts) {
                Meters::publish(meter, peak, fall);
            }
            for (meter, peak) in m.buses.iter().zip(peaks.buses) {
                Meters::publish(meter, peak, fall);
            }
            Meters::publish(&m.master, peak, fall);
        }
        let voices: usize = s.rack.parts.iter().map(Engine::active_voices).sum();
        p.shared.voices.store(voices as u64, Ordering::Relaxed);
        let audible: usize = s.rack.parts.iter().map(Engine::audible_voices).sum();
        p.shared.audible.store(audible as u64, Ordering::Relaxed);
        let dropouts: u64 = (s.rack.parts.iter())
            .map(|e| e.underruns() + e.dropped_commands())
            .sum();
        p.shared.dropouts.store(dropouts, Ordering::Relaxed);
        if frames > 0 && rate > 0. {
            // Positive `f32` bits order like the values: the UI swaps out the peak since it last looked.
            let load = (started.elapsed().as_secs_f64() * rate / frames as f64) as f32;
            p.shared
                .cpu
                .fetch_max(u64::from(load.to_bits()), Ordering::Relaxed);
        }
        ProcessStatus::Normal
    }
    fn editor(params: Arc<SamplerParams>) -> Box<dyn Editor> {
        crate::ui::editor(params)
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
fn rss_mib() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            let line = s.lines().find(|l| l.starts_with("VmRSS:"))?;
            line.split_whitespace().nth(1)?.parse::<f64>().ok()
        })
        .map_or(0., |kib| kib / 1024.)
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
pub fn bench_host(paths: &[String], seconds: f64, notes: usize) -> anyhow::Result<()> {
    use moose::core::bus_routing::{BusActivation, BusRouting};
    use std::time::Duration;
    const FRAMES: usize = 512;
    const RATE: f64 = 48000.;
    let p = Arc::new(SamplerParams::new());
    let ram_only = paths.iter().any(|p| p == "--ram-only");
    let paths: Vec<_> = paths.iter().filter(|p| *p != "--ram-only").cloned().collect();
    let paths = &paths[..];
    let mut selection = p.selection.write().unwrap();
    if ram_only {
        selection.streaming = Streaming::RamOnly;
    }
    selection.parts = paths
        .iter()
        .map(|path| Part {
            path: path.clone(),
            ..Default::default()
        })
        .collect();
    drop(selection);
    let mut dsp = Dsp::default();
    let transport = TransportInfo::default();
    let mut data = vec![vec![0f32; FRAMES]; 2 * BUSES];
    let mut outgoing = EventList::with_capacity(64);
    let (instructions, cycles) = (Counter::open(1), Counter::open(0));
    let mut process = |dsp: &mut Dsp, events: &EventList| {
        let mut channels: Vec<_> = data.iter_mut().map(|v| v.as_mut_slice()).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, FRAMES);
        let mut routing = BusRouting::new();
        for _ in 0..BUSES {
            routing.push_output(2, BusActivation::Active);
        }
        outgoing.clear();
        let mut cx = ProcessContext::new(&transport, RATE, FRAMES, &mut outgoing)
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
        &AudioConfig::new(RATE, FRAMES),
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
    let keys: Vec<u8> = {
        let b = dsp.rack.parts[0].bank().unwrap();
        let low = b.zones().iter().map(|z| z.low_key).min().unwrap_or(48);
        let high = b.zones().iter().map(|z| z.high_key).max().unwrap_or(72);
        // Keyswitches sit low: play the upper part of the range.
        (low.max(36)..=high.min(96)).collect()
    };
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
    let block = Duration::from_secs_f64(FRAMES as f64 / RATE);
    let blocks = (seconds * RATE) as usize / FRAMES;
    let every = (RATE / notes.max(1) as f64) as usize;
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
        let (mut voices, mut cpu, mut voice_blocks, mut audible_blocks) = (0, 0f32, 0usize, 0usize);
        let start = Instant::now();
        for b in 0..blocks {
            events.clear();
            let frame = b * FRAMES;
            while playing && started * every < frame + FRAMES {
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
            let (wall, cpu_ms, millions, spent) = process(&mut dsp, &events);
            cycle_counts.push(spent);
            times.push(wall);
            cpus.push(cpu_ms);
            counts.push(millions);
            let now = p.shared.voices.load(Ordering::Relaxed);
            (voices, voice_blocks) = (voices.max(now), voice_blocks + now as usize);
            audible_blocks += dsp.rack.parts.iter().map(|e| e.audible_voices()).sum::<usize>();
            cpu = cpu.max(f32::from_bits(p.shared.cpu.swap(0, Ordering::Relaxed) as u32));
            if let Some(wait) = (start + block * (b + 1) as u32).checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
        }
        if playing {
            let mut off = EventList::with_capacity(64);
            for key in held.drain(..) {
                off.push(Event::new(0, EventBody::NoteOff { group: 0, channel: 0, note: key, velocity: 0 }));
            }
            process(&mut dsp, &off);
        }
        let deadline = FRAMES as f64 / RATE * 1e3;
        let mean_voices = voice_blocks as f64 / times.len() as f64;
        let mean_audible = audible_blocks as f64 / times.len() as f64;
        let whole = (cpu_clock(2) - process_cpu) / start.elapsed().as_secs_f64();
        println!(
            "{phase}: {} blocks · mean {mean_voices:.0} ({mean_audible:.0} audible), peak {voices} voices · peak reported CPU {:.1}% · whole process {:.1}% of a core",
            times.len(),
            cpu * 100.,
            whole * 100.
        );
        if counts.iter().any(|&c| c > 0.) {
            let mut sorted = counts.clone();
            sorted.sort_by(f64::total_cmp);
            let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
            let mean = counts.iter().sum::<f64>() / counts.len() as f64;
            let mean_cycles = cycle_counts.iter().sum::<f64>() / cycle_counts.len() as f64;
            let per_voice = if playing && mean_voices >= 1. {
                let voice_frames = mean_voices * FRAMES as f64;
                let audible_frames = mean_audible.max(1.) * FRAMES as f64;
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
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
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
                    ..Default::default()
                },
            ],
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
        let path = root.join("Multis").join("Evening.kontakto-multi");
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
    fn control_edits_run_the_script_and_report_back() {
        let script = "on init\nmake_perfview\ndeclare ui_switch $legato\nmake_persistent($legato)\ndeclare ui_label $l(1,1)\nend on\non ui_control($legato)\nset_text($l, \"Legato\")\nset_key_color(36, $KEY_COLOR_BLUE)\nend on";
        let mut engine = crate::ksp::LogEngine::new(Vec::new(), 48_000.0);
        let (rt, errors) = Runtime::with_scripts(&[script], &mut engine, 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        let p = SamplerParams::new();
        let mut dsp = Dsp::default();
        {
            let mut view = p.shared.view.lock().unwrap();
            let epoch = next_epoch(&mut view, 0, Some(Box::new(rt.persistence())), Some(&rt));
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

        let (slot, epoch, live) = p.shared.lives.pop().expect("the live view comes back");
        assert_eq!((slot, epoch), (0, dsp.script_epoch[0]));
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
        p.shared.view.lock().unwrap().root = import::LIBRARY_ROOT.into();
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
        p.shared.view.lock().unwrap().root = import::LIBRARY_ROOT.into();
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
        p.shared.view.lock().unwrap().root = import::LIBRARY_ROOT.into();
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
}
