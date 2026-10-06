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
    Load(String),
    /// The caller canceled the preparation.
    Canceled,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "{what} is not supported by this sound core"),
            Self::Capacity(what) => write!(f, "{what} capacity exhausted"),
            Self::Invalid(why) => write!(f, "invalid input: {why}"),
            Self::Load(why) => write!(f, "load failed: {why}"),
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
    /// Instrument or sample file.
    pub path: std::path::PathBuf,
    /// Program index inside a bank/multi file.
    pub program: u32,
    pub sample_rate: f64,
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

    fn voices(&self) -> Voices;
    /// `part`'s runtime problems since it was installed.
    fn problems(&self, part: usize) -> report::RuntimeProblems;
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
