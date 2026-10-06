//! The seam between the product shell (plugin wrappers, UI, browser, presets)
//! and the sound core (translators, runtime, DSP).
//!
//! The shell talks to a core only through [`Core`] on the audio thread and
//! [`CoreLoader`] on worker threads, exchanging the plain data in [`event`]
//! and [`view`]. [`v1`] adapts the existing engine; the `core-v2` feature adds
//! [`v2`] over `sampler-core`.
//!
//! What the shell needs, grouped:
//!
//! | Need | Audio thread ([`Core`]) | Worker ([`CoreLoader`]) |
//! | --- | --- | --- |
//! | Load / progress | [`Core::install`] swaps a prepared part in, returns the old one | [`CoreLoader::prepare`] with [`Progress`] and cancellation |
//! | Notes / MIDI | [`Core::event`] with [`event::In`]; [`Core::release_due`] for held-back input | — |
//! | Render | [`Core::render`] into [`BUSES`] stereo [`Block`]s | — |
//! | Parameters | [`Core::ui_control`], [`Core::set_transport`], [`BlockInfo`] | — |
//! | State save / restore | [`Core::refresh_persistence`] (budgeted, coherent) | restored [`view::Persisted`] in [`LoadRequest`] |
//! | Script UI | [`Core::refresh_live`] into the core's [`Core::Live`] buffer | initial [`view::Interface`] in the prepared part |
//! | Metering | [`Core::voices`] | — |
//! | Browser info | — | [`CoreLoader::describe`] |
//!
//! Rules: everything on [`Core`] runs on the audio thread and must not allocate,
//! lock or touch files. Preparation and dropping happen on workers: [`Core::install`]
//! hands the replaced part back as [`Core::Retired`] instead of dropping it.

pub mod event;
pub mod mix;
pub mod v1;
#[cfg(feature = "core-v2")]
pub mod v2;
pub mod view;

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
    /// Input is being held back for alignment ([`Core::holding`]).
    pub holding: bool,
    pub transport: Transport,
}

/// Rendered output of one [`Core::render`] call: every bus, and which hold signal.
pub struct Rendered<'a> {
    pub buses: &'a [Block; BUSES],
    pub live: [bool; BUSES],
}

/// Shell-wide macro controls applied to every part.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Macros {
    pub attack: f32,
    pub release: f32,
    pub cutoff: f32,
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
    /// Restored persistent script values, per script slot.
    pub persisted: Vec<view::Persisted>,
}

/// Browser-facing facts about a source, read without preparing it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Description {
    pub name: String,
    pub zones: usize,
    pub scripts: usize,
    /// What the core could not translate, one human-readable line each.
    pub unsupported: Vec<String>,
}

/// The sound core on the audio thread: every method is real-time safe.
///
/// A core holds every rack part. `part` arguments index into it; an index at
/// or past [`parts`](Self::parts) is ignored.
pub trait Core: Send {
    /// A part prepared off the audio thread by the matching [`CoreLoader`].
    type Prepared: Send;
    /// What [`install`](Self::install) replaced; drop it on a worker.
    type Retired: Send + Default;
    /// The core's reusable script-view buffer, refreshed in place.
    type Live: Send;

    fn parts(&self) -> usize;
    fn sample_rate(&self) -> f64;
    /// Silence and forget everything at a new sample rate.
    fn reset(&mut self, sample_rate: f64);
    /// Stop every note and forget held input, keeping loaded parts.
    fn panic(&mut self);
    /// Swap a prepared part in; the replaced one comes back for a worker to drop.
    fn install(&mut self, part: usize, prepared: Self::Prepared) -> Self::Retired;

    /// Whether input is held back for alignment while `playing`.
    fn holding(&self, playing: bool) -> bool;
    fn set_transport(&mut self, transport: Transport);
    /// Start a block; called once, before events and rendering.
    fn begin_block(&mut self, block: &BlockInfo);
    /// Host input `offset` frames into the block, from host port `port`.
    fn event(&mut self, port: u8, event: event::In, offset: u32, holding: bool);
    /// Whether `key` still sounds on `channel` anywhere (keyboard display).
    fn key_held(&self, channel: u8, key: u8, holding: bool) -> bool;
    /// Play held-back input due by `at` frames into the block; returns the
    /// next frame (≤ `limit`) something held is due.
    fn release_due(&mut self, at: usize, limit: usize) -> usize;
    /// Render `frames` (≤ [`MAX_BLOCK`]) onto the buses.
    fn render(&mut self, frames: usize) -> Rendered<'_>;
    /// Whether anything still owns exact host note `note`. A note-on that
    /// created no owner (an unmatched route, a consumed key switch) ends at once.
    fn owns(&self, note: event::HostNote) -> bool;
    /// Finish a block of `frames`: offer every ended host note to `end`, which
    /// returns false once the host refuses more. Returns refused notes.
    fn end_block(&mut self, frames: usize, end: &mut dyn FnMut(event::HostNote) -> bool) -> u64;

    /// On-screen keyboard input that reaches parts like host port `port` and
    /// records each part reached in `reached`, for [`event_to`](Self::event_to).
    fn event_recorded(&mut self, port: u8, event: event::In, reached: &mut [bool]);
    /// Input to the parts `parts` marks, even if routing changed since; clears the marks.
    fn event_to(&mut self, parts: &mut [bool], event: event::In);
    /// Input straight to one part, bypassing port and channel routing.
    fn play(&mut self, part: usize, event: event::In);
    /// The MIDI channel that reaches `part`'s first zone (keyboard preview).
    fn preview_channel(&self, part: usize) -> u8;
    /// Sound one preview note on `part`: `note`, else its first zone's root.
    fn audition(&mut self, part: usize, note: Option<u8>);
    /// Silence what [`audition`](Self::audition) started.
    fn audition_stop(&mut self, part: usize);

    /// Shell-wide envelope and filter macros, each `0..=1`.
    fn set_macros(&mut self, macros: Macros);
    /// The callback's smoothed CPU load, for load shedding.
    fn set_load(&mut self, load: f32);
    /// The mixer's part and bus settings.
    fn set_mix(&mut self, mix: &mix::Mix);
    /// Host output port of each bus, as last set by [`set_mix`](Self::set_mix).
    fn bus_ports(&self) -> [u8; BUSES];
    /// Copy `part`'s post-fader mono signal during [`render`](Self::render).
    fn set_tap(&mut self, part: Option<usize>);
    /// The tapped signal of the last render, if a tap is set.
    fn tapped(&self, frames: usize) -> Option<&[f32]>;
    /// Peaks accumulated since the caller last cleared them.
    fn peaks_mut(&mut self) -> &mut mix::Peaks;

    /// A user edit of script control `control` in script slot `slot`.
    fn ui_control(&mut self, part: usize, slot: usize, control: usize, value: i32);
    /// A file chosen for a script file-selector control; the path is already validated.
    fn ui_file_selection(&mut self, part: usize, slot: usize, control: usize, path: &str);
    /// Changes whenever a part's script memory may have changed; 0 without scripts.
    fn script_revision(&self, part: usize) -> u64;
    /// Copy persistent script values into `saved`, about `budget` at a time.
    /// True once `saved` holds one coherent callback boundary.
    fn refresh_persistence(&self, part: usize, saved: &mut [view::Persisted], at: &mut view::Refresh, budget: usize) -> bool;
    /// Refresh the script view in place, about `budget` at a time; true once whole.
    /// `unchanged`: the shell saw no [`script_revision`](Self::script_revision)
    /// change since `live` was last whole, so only diagnostics need copying.
    fn refresh_live(&self, part: usize, live: &mut Self::Live, at: &mut view::Refresh, budget: usize, unchanged: bool) -> bool;

    fn voices(&self) -> Voices;
    /// `part`'s cumulative stream underruns.
    fn underruns(&self, part: usize) -> u64;
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
    ) -> Result<<Self::Core as Core>::Prepared, CoreError>;
    /// Browser facts about a source, cheaper than [`prepare`](Self::prepare).
    fn describe(&self, path: &std::path::Path, program: u32) -> Result<Description, CoreError>;
}
