//! The seam between the product shell (plugin wrappers, UI, browser, presets)
//! and the sound core (translators, runtime, DSP).
//!
//! The shell talks to the core only through [`Core`] on the audio thread and
//! [`CoreLoader`] on worker threads, exchanging plain data: [`event::Event`]
//! in, rendered output pairs and [`mix::Peaks`] out, the per-part output
//! [`tree::MixTree`] and [`report::LoadReport`] from each load. [`v2`] is the
//! one implementation, over `sampler-core`.
//!
//! | Need | Audio thread ([`Core`]) | Worker ([`CoreLoader`]) |
//! | --- | --- | --- |
//! | Load / progress | [`Core::install`] swaps a prepared part in, returns the old one | [`CoreLoader::prepare`] with [`Progress`] and cancellation |
//! | Notes / MIDI | [`Core::event`], [`Core::play`] with UMP-native [`event::Event`]s | — |
//! | Render | [`Core::render`] into [`BUSES`] DAW stereo pairs | — |
//! | Mixer | [`Core::set_mix`]: parts, output pairs and every tree node | [`Loaded::tree`] |
//! | Report | [`Core::problems`] runtime counters | [`Loaded::report`] |
//! | Browser info | — | [`CoreLoader::describe`] |
//!
//! Rules: everything on [`Core`] runs on the audio thread and must not allocate,
//! lock or touch files. Preparation and dropping happen on workers: [`Core::install`]
//! hands the replaced part back as [`Core::Retired`] instead of dropping it.

pub mod event;
pub mod mics;
pub mod mix;
pub mod report;
pub mod tree;
pub mod v2;

use std::fmt;

/// Largest block rendered in one pass; longer requests are split.
pub const MAX_BLOCK: usize = 128;
/// Stereo output buses (Kontakt's st.1…st.16).
pub const BUSES: usize = 16;
/// Initial rack storage for existing sessions; this is not a part-count limit.
pub const RACK_SLOTS: usize = 16;
pub use crate::plugin::automation_ids::HOST_AUTOMATION_SLOTS;
/// How far a part tunes, in semitones either way.
pub const TUNE_RANGE: f32 = 36.0;
/// One stereo block: `[left, right]`.
pub type Block = [[f32; MAX_BLOCK]; 2];

/// Why a core could not do something. `Display` is user-facing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreError {
    /// This core does not implement the feature yet; names it.
    Unsupported(&'static str),
    /// A preallocated capacity was exhausted.
    Capacity(&'static str),
    /// The request or its source data is invalid.
    Invalid(String),
    /// Reading or decoding the source failed.
    Load(LoadFailure),
    /// The caller canceled the preparation.
    Canceled,
}

/// A failed load: the message and, when the source tagged it, where in loading
/// it happened (typed, for reports and the corpus harness).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadFailure {
    pub message: String,
    pub stage: Option<sampler_kontakt::Stage>,
    pub kind: Option<sampler_kontakt::Kind>,
    /// File and line in the loader that tagged the error.
    pub at: Option<(&'static str, u32)>,
}

impl LoadFailure {
    /// A failure with only a message (a source that does not stage its errors).
    pub fn message(message: impl fmt::Display) -> Self {
        Self { message: message.to_string(), stage: None, kind: None, at: None }
    }
}

impl From<&sampler_kontakt::LoadError> for LoadFailure {
    fn from(e: &sampler_kontakt::LoadError) -> Self {
        Self {
            message: e.to_string(),
            stage: e.stage(),
            kind: Some(e.kind()),
            at: e.location().map(|l| (l.file(), l.line())),
        }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "{what} is not supported by this sound core"),
            Self::Capacity(what) => write!(f, "{what} capacity exhausted"),
            Self::Invalid(why) => write!(f, "invalid input: {why}"),
            Self::Load(why) => write!(f, "load failed: {}", why.message),
            Self::Canceled => f.write_str("load canceled"),
        }
    }
}

impl std::error::Error for CoreError {}

/// Host transport, as of the start of a block.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Transport {
    pub playing: bool,
    pub tempo: f64,
    pub beats: f64,
    pub signature: (u8, u8),
}

/// What a block is, given to [`Core::begin_block`] before any event.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockInfo {
    pub frames: usize,
    /// Offline renders wait for the disk instead of shedding load.
    pub offline: bool,
    pub transport: Transport,
}

/// Rendered output of one [`Core::render`] call: every bus, and which hold signal.
pub struct Rendered<'a> {
    pub buses: &'a [Block; BUSES],
    pub live: [bool; BUSES],
}

/// Voice counts for meters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Voices {
    pub active: usize,
    pub audible: usize,
    /// Stream underruns plus dropped commands, cumulative.
    pub dropouts: u64,
}

/// Load progress in thousandths, monotonic per request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Progress(pub u16);

impl Progress {
    pub const DONE: Self = Self(1000);
}

/// What to prepare for one part.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadRequest {
    pub uvi_state: Option<sampler_uvi::script::UiState>,
    /// Instrument or sample file.
    pub path: std::path::PathBuf,
    /// Program index inside a bank/multi file.
    pub program: u32,
    pub sample_rate: f64,
    /// Per-note pressure and timbre reach every zone (louder, brighter), for
    /// MPE controllers; otherwise only routes the instrument authored do.
    pub mpe: bool,
    /// Where the dynamics controllers start before the host moves them
    /// (`None`: Kontakt's power-on state).
    pub dynamics_start: Option<u8>,
    /// Voice-rendering threads (`None`: one, the audio thread alone).
    pub threads: Option<ThreadChoice>,
    /// Host-saved Kontakt UI values (menus carry item values, not positions).
    pub control_values: Vec<(sampler_ui_ir::ControlId, f64)>,
}

/// A request for voice-rendering threads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadChoice {
    Auto,
    Fixed(usize),
}

/// A prepared part with what the shell shows of it.
pub struct Loaded<P> {
    pub part: P,
    /// The part's output tree, node 0 being the instrument.
    pub tree: tree::MixTree,
    pub report: report::LoadReport,
    /// The source's script interfaces, in script order; their image assets
    /// carry the library's own metadata.
    pub interfaces: Vec<sampler_ui_ir::Interface>,
    /// The part's host-visible controls and their defaults, in id order;
    /// what [`Core::set_control`] accepts and [`Core::control_value`] reads.
    pub controls: Vec<(sampler_ui_ir::ControlId, f64)>,
    /// The translated instrument the part plays, for the views that show
    /// its articulations, mapping and sound; `None` for plain audio files.
    pub instrument: Option<std::sync::Arc<sampler_ir::Instrument>>,
    /// The script interfaces' models, for runtime UI changes.
    pub scripts: ScriptUi,
    /// The streamed samples, when the part reads them from disk as it plays.
    pub stream: Option<std::sync::Arc<Stream>>,
}

/// A part's streamed samples: their decode threads and resident start data.
pub struct Stream {
    pub streamer: sampler_kontakt::Streamer,
    pub assets: Vec<sampler_core::Pcm>,
    pub report: sampler_kontakt::StreamReport,
}

impl Stream {
    /// Bytes held in memory: start data plus the page pool.
    pub fn resident_bytes(&self) -> u64 {
        let heads: usize = self.assets.iter().map(sampler_core::Pcm::resident_bytes).sum();
        (heads + self.report.pool_bytes) as u64
    }

    /// Drop the start data of samples not played since `before` (the part's
    /// [`Core::clock`]), least recently played first, until the rest fit in
    /// `budget` bytes; they read again from disk when next played.
    pub fn trim(&self, budget: u64, before: u64) -> usize {
        let pool = self.report.pool_bytes as u64;
        self.streamer.trim(&self.assets, budget.saturating_sub(pool).try_into().unwrap_or(usize::MAX), before)
    }
}

/// One key as the scripts show it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyLook {
    /// A `$KEY_COLOR_*` index.
    pub color: Option<u8>,
    pub name: Option<String>,
    /// A keyswitch or other control key rather than a playing key.
    pub control: bool,
}

/// A part's script interfaces as its scripts change them at run time.
#[derive(Default)]
pub struct ScriptUi {
    pub uvi: Option<std::sync::Arc<sampler_uvi::scripted::UiBridge>>,
    pub uvi_revision: u64,
    pub uvi_source: Option<(String, u32)>,
    /// By script instance ([`sampler_core::ScriptInstanceId`]).
    pub views: Vec<sampler_ksp::ScriptView>,
    pub resources: Option<sampler_kontakt::Resources>,
}

impl ScriptUi {
    /// Apply an effect script `instance` emitted; true when it changed a view.
    pub fn apply(&mut self, instance: usize, effect: &sampler_core::Effect) -> bool {
        self.views.get_mut(instance).is_some_and(|v| v.apply_ui_effect(effect))
    }

    /// The keyboard as the scripts colour and name it, 128 keys; a later
    /// script's settings win.
    pub fn keys(&self) -> std::sync::Arc<[KeyLook]> {
        let mut keys = vec![KeyLook::default(); 128];
        for view in &self.views {
            for (look, key) in keys.iter_mut().zip(&view.model().interface.keys) {
                if let Some(color) = key.color.and_then(|c| u8::try_from(c).ok()) {
                    look.color = Some(color);
                }
                if let Some(name) = key.name.as_ref().filter(|n| !n.is_empty()) {
                    look.name = Some(name.clone());
                }
                if let Some(kind) = key.kind {
                    look.control = kind == 1; // $NI_KEY_TYPE_CONTROL
                }
            }
        }
        keys.into()
    }

    /// Regenerate only a script changed by this batch.
    pub fn interface(&mut self, instance: usize) -> Option<sampler_ui_ir::Interface> {
        let resources = std::cell::RefCell::new(&mut self.resources);
        let picture = |path: &str| resources.borrow_mut().as_mut()?.picture(path);
        self.views.get(instance)?.ui(&picture).ok()
    }

    /// The interfaces as they stand, like [`Loaded::interfaces`].
    pub fn interfaces(&mut self) -> Vec<sampler_ui_ir::Interface> {
        if let Some(uvi) = &self.uvi {
            return vec![(*uvi.interface()).clone()];
        }
        let resources = std::cell::RefCell::new(&mut self.resources);
        let picture = |path: &str| resources.borrow_mut().as_mut()?.picture(path);
        self.views.iter().filter_map(|v| v.ui(&picture).ok()).collect()
    }
}

/// Browser-facing facts about a source, read without preparing it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Description {
    pub name: String,
    pub zones: usize,
    pub scripts: usize,
    /// What the core could not translate.
    pub missing: Vec<report::Missing>,
}

/// The sound core on the audio thread: every method is real-time safe.
///
/// A core holds every rack part. `part` arguments index into it; an index at
/// or past [`parts`](Self::parts) is ignored. The shell splits rendering at
/// event offsets, so events apply at the next rendered frame.
pub trait Core: Send {
    /// A part prepared off the audio thread by the matching [`CoreLoader`].
    type Prepared: Send;
    /// What [`install`](Self::install) replaced; drop it on a worker.
    type Retired: Send + Default;

    fn parts(&self) -> usize;
    fn sample_rate(&self) -> f64;
    /// Silence and forget everything at a new sample rate.
    fn reset(&mut self, sample_rate: f64);
    /// Stop every note, keeping loaded parts.
    fn panic(&mut self);
    /// Swap a prepared part in; the replaced one comes back for a worker to drop.
    /// The part's tree nodes play at unity until the next [`set_mix`](Self::set_mix).
    fn install(&mut self, part: usize, prepared: Self::Prepared) -> Self::Retired;

    /// Start a block; called once, before events and rendering.
    fn begin_block(&mut self, block: &BlockInfo);
    /// Host input from MIDI/note port `port`, to every part listening there.
    fn event(&mut self, port: u8, event: event::Event);
    /// Input straight to one part, bypassing port and channel routing.
    fn play(&mut self, part: usize, event: event::Event);
    /// Whether a host note on `channel` and `key` still sounds (keyboard display).
    fn key_held(&self, channel: u8, key: u8) -> bool;
    /// Render `frames` (≤ [`MAX_BLOCK`]) onto the output pairs.
    fn render(&mut self, frames: usize) -> Rendered<'_>;
    /// Whether anything still owns exact host note `note`. A note-on that
    /// created no owner ends at once.
    fn owns(&self, note: event::HostNote) -> bool;
    /// Finish a block of `frames`: offer every ended host note to `end`, which
    /// returns false once the host refuses more. Returns refused notes.
    fn end_block(&mut self, frames: usize, end: &mut dyn FnMut(event::HostNote) -> bool) -> u64;

    /// The mixer: parts, output pairs and each part's tree nodes.
    fn set_mix(&mut self, mix: &mix::Mix);
    /// Host output port of each pair, as last set by [`set_mix`](Self::set_mix).
    fn bus_ports(&self) -> [u8; BUSES];
    /// Copy `part`'s post-fader mono signal during [`render`](Self::render).
    fn set_tap(&mut self, part: Option<usize>);
    /// The tapped signal of the last render, if a tap is set.
    fn tapped(&self, frames: usize) -> Option<&[f32]>;
    /// Peaks accumulated since the caller last cleared them.
    fn peaks_mut(&mut self) -> &mut mix::Peaks;
    /// `part`'s tree nodes below the root, each with its peak since the last
    /// call (after the node's own fader); taking them resets them.
    fn take_node_peaks(&mut self, part: usize, each: &mut dyn FnMut(usize, [f32; 2]));

    /// Edit one of `part`'s controls as its widget would: the value is
    /// clamped to the control's range (integers rounded, toggles at 0.5) and
    /// the script's `on ui_control` callback runs. False when the part has
    /// no such control or its callback could not be admitted.
    fn set_control(&mut self, part: usize, control: sampler_ui_ir::ControlId, value: f64) -> bool;
    /// The control's current value, which scripts may also change.
    fn control_value(&self, part: usize, control: sampler_ui_ir::ControlId) -> Option<f64>;
    /// Hand `part`'s queued script effects to `each` with their script
    /// instance, in order, until it returns false; the rest stay queued.
    fn take_effects(&mut self, part: usize, each: &mut dyn FnMut(usize, &sampler_core::Effect) -> bool);

    fn voices(&self) -> Voices;
    /// `part`'s runtime problems since it was installed.
    fn problems(&self, part: usize) -> report::RuntimeProblems;
    /// The articulation `part` plays, by index in its instrument, when the
    /// runtime rather than a script holds it.
    fn articulation(&self, part: usize) -> Option<usize>;
    /// `part`'s clock in frames, as [`Stream::trim`] counts it.
    fn clock(&self, part: usize) -> u64;
    /// Output latency in frames that the core reports to the host.
    fn latency(&self) -> u32;
}

/// Prepares parts for a [`Core`] on worker threads.
pub trait CoreLoader {
    type Core: Core;
    /// Read, translate and prepare `request` for installation.
    /// `progress` is called with increasing values; `canceled` is polled.
    fn prepare(
        &self,
        request: &LoadRequest,
        progress: &mut dyn FnMut(Progress),
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Loaded<<Self::Core as Core>::Prepared>, CoreError>;
    /// Browser facts about a source, cheaper than [`prepare`](Self::prepare).
    fn describe(&self, path: &std::path::Path, program: u32) -> Result<Description, CoreError>;
}
