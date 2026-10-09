//! The editor: a library browser, a Kontakt-style rack of parts with their
//! performance controls rebuilt natively, and a playable keyboard.
//!
//! ```text
//! ┌ top bar: wordmark · activity · meters · master · global actions ───────┐
//! ├ browser ┆ tabs: rack · mixer · logs ───────────────────────────────────┤
//! │         ┆ part header: fold · name ‹ › · routing · S M · vol/pan · ✕  │
//! │ (resize)┆   performance controls (sections, strips, knobs, faders)    │
//! │         ┆ part header (folded) ─────────────────────────────────────── │
//! │         ┆ add an instrument                                           │
//! ├ keyboard dock (collapsible) ───────────────────────────────────────────┤
//! ```
//!
//! Views read a per-frame copy of the loader's [`View`] and edit a copy of the
//! host-persisted [`Selection`]; the copy is written back once per frame. Notes
//! and auditions reach the audio thread only through [`Shared`]'s lock-free
//! queues and atomics, and loading happens on the [`Load`] task, never here.
//!
//! [`Shared`]: crate::plugin::Shared

mod art;
mod browser;
mod computer;
mod cover;
mod header;
mod keyboard;
mod logs;
mod menu;
// The v2 views take plain data the core does not produce yet (submix/bus
// nodes, effect and modulation reports).
#[allow(dead_code)]
mod mix_tree;
mod ir_view;
mod generated;
#[cfg(feature = "shots")]
pub(crate) mod scan;
#[allow(dead_code)]
mod load_report;
mod bridge;
mod pictures;
mod picture_decode;
mod picture_worker;
mod native_runtime;
mod native_ui;
mod render_art;
mod inside;
mod mapping;
mod editor;
mod editor_model;
mod viz;
mod chain;
mod part;
pub(crate) mod picker;
mod rack;
mod spectrum;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod v2_tests;
#[cfg(all(test, feature = "shots"))]
mod widget_gate;
mod theme;

use crate::library;
use crate::plugin::{Load, Part, PartView, SamplerParams, Selection, View, mix};
use moose::mui::{Bridge, MuiEditor, mui::prelude::*, mui::prelude::Color};
use moose::prelude::*;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};
use theme::*;

pub(crate) fn editor(params: Arc<SamplerParams>) -> Box<dyn Editor> {
    let mut config = mui::diagnostics::Config::new("kontra", env!("CARGO_PKG_VERSION"));
    config.build = option_env!("APP_GIT_REVISION").unwrap_or("unknown").into();
    config.mui_revision = "dcf0796082feec053af1418e3a38a302ee61da0a".into();
    let reporter = mui::diagnostics::Reporter::start(config)
        .inspect_err(|error| eprintln!("KONTRA MUI reporting: {error}"))
        .ok();
    let meters = Arc::new(Meters::default());
    let computer = Arc::new(computer::Computer::default());
    #[cfg(target_os = "linux")]
    let picker = Arc::new(picker::Picker::with_runtime(Arc::clone(&params.shared.dialog_runtime)));
    #[cfg(not(target_os = "linux"))]
    let picker = Arc::new(picker::Picker::default());
    let art = Arc::new(art::Art::default());
    let build = build(&params, meters.clone(), computer.clone(), picker.clone(), art.clone());
    let (drop_params, drop_picker) = (params.clone(), picker.clone());
    let file_drag = Arc::new(std::sync::Mutex::new(None));
    let cancel_drag = file_drag.clone();
    let cancel_picker = picker.clone();
    let (cancel_params, cancel_computer) = (params.clone(), computer.clone());
    let (key_params, key_computer) = (params.clone(), computer.clone());
    let watch_params = params.clone();
    let mut watch = Watch::default();
    let size = params.shared.libraries.settings().editor_size();
    let zoom_params = params.clone();
    let close_params = params.clone();
    let close_computer = computer.clone();
    let close_picker = Arc::clone(&picker);
    #[cfg(target_os = "linux")]
    let parent_picker = Arc::clone(&picker);
    let last_size = AtomicU64::new(0);
    let editor = MuiEditor::new(params, theme::ui(), size, build)
        .on_log(move |line| {
            let _reporter = &reporter;
            let failed = line.contains("unavailable") || line.contains("failed") || line.contains("panic");
            crate::diagnostics::event(if failed { crate::diagnostics::LogLevel::Warning } else { crate::diagnostics::LogLevel::Info },
                "renderer", "native_window", serde_json::json!({"stage":"renderer", "reason":line}));
            // Persist the attempt before entering native graphics code: an
            // access violation does not unwind or wait for queued log writes.
            if line.starts_with("mui-baseview: GPU init ")
                || line.starts_with("mui-baseview: native window init entering native code ") {
                let _ = crate::diagnostics::flush(std::time::Duration::from_millis(100));
            }
        })
        .on_files(move |ui, at, paths, dropped| native_files(&drop_params, &drop_picker, &file_drag, ui, at, paths, dropped))
        .on_cancel(move |ui| {
            let_go(&cancel_params, &cancel_computer);
            native_files(&cancel_params, &cancel_picker, &cancel_drag, ui, Point::new(-1., -1.), &[], false);
        })
        .on_key(move |ui, event| key_computer.key(ui, &key_params, event))
        .hide_pointer(theme::pointer_hidden)
        .native_timing(crate::diagnostics::native_timing_hook())
        .changed(move || watch.changed(&watch_params, &meters, &computer) || picker.ready() || art.ready())
        .fixed_zoom()
        .user_zoom(move |window| {
            let size = (window.width.round() as u32, window.height.round() as u32);
            let packed = u64::from(size.0) << 32 | u64::from(size.1);
            if last_size.swap(packed, Ordering::Relaxed) != packed {
                zoom_params.shared.libraries.remember_window(size);
            }
            zoom_params.shared.libraries.settings().editor_scale()
        })
        .on_close(move || {
            let_go(&close_params, &close_computer);
            close_params.shared.editor_watch.store(usize::MAX, Ordering::Relaxed);
            close_picker.close();
            close_params.shared.libraries.flush_settings();
        })
        .resizable((900, 600));
    #[cfg(target_os = "linux")]
    let editor = editor.on_x11_window(move |parent| parent_picker.native_window(parent));
    editor.into_editor()
}

/// Focus left or the window closes: every key the editor holds comes up.
fn let_go(p: &SamplerParams, computer: &computer::Computer) {
    computer.release(p);
    p.shared.release_keyboard();
    // The next frame names the spectrum's strip again, if one still shows.
    p.shared.scope.source.store(0, Ordering::Relaxed);
}

/// The lock's data even if a panicking thread held it: the editor shows what
/// is there rather than taking the host down with it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn read<T>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(PoisonError::into_inner)
}

fn write<T>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(PoisonError::into_inner)
}

/// The top bar's eased readouts, as [`Watch`] last sampled them: `f32` bits.
#[derive(Default)]
struct Meters {
    /// Audio thread load, 0..1.
    cpu: AtomicU32,
    disk: AtomicU32,
    /// A view moves on its own (a spectrum or a peak hold falling): the
    /// last frame built says so, and frames keep coming until one does not.
    animating: AtomicBool,
    /// Journal changes affect this editor only while its Logs pane is shown.
    logs_visible: AtomicBool,
}

/// Decides, every display tick, whether anything the editor shows moved
/// outside its own input: a readout, a held key, the loader's view. An idle
/// editor then draws nothing at all.
#[derive(Default)]
struct Watch {
    signature: u64,
    /// The readouts' hash as last sampled: they move with every note, so
    /// they are looked at ten times a second, not every tick.
    readouts: u64,
    cpu: f32,
    disk: f32,
    disk_read: u64,
    disk_counter: Option<&'static AtomicU64>,
    /// The audio thread's render and rendered time when last sampled.
    busy: (u64, u64),
    /// When the readouts were last sampled.
    cpu_at: Option<Instant>,
    poll_at: Option<Instant>,
    frame_at: Option<Instant>,
}

/// Readouts and the loading line refresh this often at most.
const READOUT_MS: u64 = 100;
const ANIMATION_MS: u64 = 33;

impl Watch {
    fn changed(&mut self, p: &SamplerParams, meters: &Meters, computer: &computer::Computer) -> bool {
        let now = Instant::now();
        let due = |at: Option<Instant>, every: u64| {
            at.is_none_or(|t| now - t >= Duration::from_millis(every))
        };
        if due(self.cpu_at, READOUT_MS) {
            let since = self.cpu_at.map_or(0., |t| (now - t).as_secs_f32());
            self.cpu_at = Some(now);
            // Mean load since the last look, as Kontakt shows it. The peak
            // block's wall time read 20-80% at idle: one preempted block in
            // a tenth of a second is scheduling, not work.
            let busy = (p.shared.busy_ns.load(Ordering::Relaxed), p.shared.span_ns.load(Ordering::Relaxed));
            let (work, span) = (busy.0.saturating_sub(self.busy.0), busy.1.saturating_sub(self.busy.1));
            self.busy = busy;
            let load = if span > 0 { work as f32 / span as f32 } else { 0. };
            self.cpu = load * 0.5 + self.cpu * 0.5;
            if self.cpu < 0.005 {
                self.cpu = 0.;
            }
            meters.cpu.store(self.cpu.to_bits(), Ordering::Relaxed);
            // Disk throughput since the last look, eased; idle settles on 0.
            let counter = self.disk_counter.unwrap_or(&sampler_kontakt::DISK_READ);
            let read = counter.load(Ordering::Relaxed);
            let rate = if since > 0. {
                read.saturating_sub(self.disk_read) as f32 / 1_048_576. / since
            } else {
                0.
            };
            self.disk_read = read;
            self.disk = self.disk * 0.5 + rate * 0.5;
            if self.disk < 0.05 {
                self.disk = 0.;
            }
            meters.disk.store(self.disk.to_bits(), Ordering::Relaxed);
            let mut h = DefaultHasher::new();
            ((self.cpu * 100.).round() as u32).hash(&mut h);
            ((self.disk * 10.).round() as u32).hash(&mut h);
            p.shared.memory_snapshot().hash(&mut h);
            p.shared.voices.load(Ordering::Relaxed).hash(&mut h);
            p.shared.audible.load(Ordering::Relaxed).hash(&mut h);
            p.shared.dropouts.load(Ordering::Relaxed).hash(&mut h);
            p.shared.with_parts(|parts| { for part in parts { part.scalar_revision.load(Ordering::Acquire).hash(&mut h); part.native_revision.load(Ordering::Acquire).hash(&mut h); } });
            self.readouts = h.finish();
        }
        let mut h = DefaultHasher::new();
        self.readouts.hash(&mut h);
        pictures::revision().hash(&mut h);
        if meters.logs_visible.load(Ordering::Relaxed) {
            crate::diagnostics::revision().hash(&mut h);
            logs::wake().hash(&mut h);
        }
        p.shared.focus_request.load(Ordering::Relaxed).hash(&mut h);
        // The wheels follow incoming MIDI as it moves them.
        p.shared.bend.load(Ordering::Relaxed).hash(&mut h);
        p.shared.modulation.load(Ordering::Relaxed).hash(&mut h);
        p.shared.learned_note.load(Ordering::Relaxed).hash(&mut h);
        // Lit keys, and what the computer keyboard plays.
        for lit in p.shared.played.iter().chain(&p.shared.heard) {
            lit.load(Ordering::Relaxed).hash(&mut h);
        }
        computer.octave.load(Ordering::Relaxed).hash(&mut h);
        computer.velocity.load(Ordering::Relaxed).hash(&mut h);
        // A scan's progress, a size measured, a setting changed.
        p.shared.libraries.stamp().hash(&mut h);
        let (loading, pending) = {
            let view = lock(&p.shared.view);
            fingerprint(&view, &mut h);
            let selection = read(&p.selection);
            let pending = view.scanned != p.shared.libraries.wanted()
                || lock(&p.shared.multi_request).is_some()
                || (0..view.parts.len().max(selection.parts.len())).any(|n| {
                    let (path, program) = selection.parts.get(n).map_or(("", 0), |p| (p.path.as_str(), p.program));
                    match view.parts.get(n).and_then(|v| v.attempted.as_ref()) {
                        Some((a, b, ..)) => (a.as_str(), *b) != (path, program),
                        None => !path.is_empty(),
                    }
                });
            (view.parts.iter().any(|v| v.loading), pending)
        };
        // Meters read their atomics as they are laid out: while any shows a
        // level, frames run on the animation clock; the fall to silence
        // changes the signature, so the last one draws them empty.
        let m = &p.shared.meters;
        let sounding = p.shared.with_parts(|parts| parts.iter().any(|part| crate::plugin::Meters::read(&part.meter) != [0.; 2]))
            || m.buses.iter().chain([&m.master]).any(|meter| crate::plugin::Meters::read(meter) != [0.; 2]);
        sounding.hash(&mut h);
        // Progress and the sweep redraw on the animation's own clock.
        let moving = meters.animating.load(Ordering::Relaxed);
        let animate = (loading || sounding || moving) && due(self.frame_at, ANIMATION_MS);
        if animate {
            self.frame_at = Some(now);
        }
        let signature = h.finish();
        let moved = signature != self.signature;
        self.signature = signature;
        // A stopped host runs no audio thread to start the loader: frames poll it.
        let poll = pending && due(self.poll_at, READOUT_MS);
        if poll {
            self.poll_at = Some(now);
        }
        // The loading line sweeps until the first samples arrive.
        moved || poll || animate
    }
}

/// A copy of the loader's view for a frame to read.
fn shown(view: &Mutex<View>) -> View {
    lock(view).clone()
}

/// What of the loader's view is on screen, cheaply: pointers of what it
/// replaces wholesale, and the small fields it edits in place.
fn fingerprint(view: &View, h: &mut DefaultHasher) {
    fn at<T>(a: &Option<Arc<T>>) -> usize {
        a.as_ref().map_or(0, |a| Arc::as_ptr(a) as *const () as usize)
    }
    (&view.status, &view.multi_status, view.scanned).hash(h);
    (Arc::as_ptr(&view.files) as usize, view.artwork.len()).hash(h);
    for v in &view.parts {
        (v.loading, &v.status, v.program, v.ui_revision, v.generation).hash(h);
        (at(&v.tree), at(&v.report), at(&v.trace), Arc::as_ptr(&v.interfaces) as *const () as usize).hash(h);
    }
}

/// What plays behind a part's controls, from [`Selection::appearance`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Appearance {
    Plain = 0,
    /// A deep, dark shade of its library's color.
    Color = 1,
    /// Its library's artwork, blurred, drained and darkened.
    Artwork = 2,
}

impl Appearance {
    fn of(saved: u8) -> Self {
        match saved {
            1 => Self::Color,
            2 => Self::Artwork,
            _ => Self::Plain,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Rack,
    Mixer,
    Report,
    Logs,
}

/// Editor-only state that outlives a frame but not the window.
struct EditorState {
    search: String,
    /// What the browser's lower pane lists; `None` searches every library.
    source: Option<browser::Source>,
    /// The browser pane that last held the focus, and the upper pane's
    /// share of the height.
    pane: Option<browser::Pane>,
    split: f64,
    uvi: bool,
    tab: Tab,
    settings: bool,
    /// The name a "Save multi…" is typing, and why the last try failed.
    saving: Option<String>,
    save_error: String,
    /// The browser is shown, and how wide.
    browser: bool,
    sidebar: f64,
    /// The keyboard dock is open.
    keyboard: bool,
    /// First octave on the keyboard.
    octave: i16,
    /// The part the keyboard was last centered on.
    keyboard_for: Option<(String, u32)>,
    /// The part the Mapping, Sound and Info views show, and the rack marks
    /// and the keyboard plays unless `unselected`: Esc or a click on the
    /// empty rack lets go, and the keyboard then plays every part MIDI would.
    selected: usize,
    unselected: bool,
    notice: String,
    root: String,
    last_poll: Instant,
    /// The top bar's readouts, as [`Watch`] last eased them.
    meters: Arc<Meters>,
    /// The context menu showing.
    menu: Option<menu::Menu>,
    /// The browser's keyboard cursor: a preset path, or a folder's.
    cursor: Option<String>,
    /// The browser's library filter, scroll and rows.
    browse: browser::Browse,
    logs: logs::State,
    /// A part's name while it is being edited.
    renaming: Option<(usize, String)>,
    /// The master spectrum shows under the mixer.
    /// Each library's color, thumbnail, banner and backdrop, made from its
    /// artwork off the frame.
    art: Arc<art::Art>,
    /// The browser's files by library: of which scan, shelf and kind.
    libraries: (std::sync::Weak<Vec<PathBuf>>, usize, bool, Arc<Libraries>),
    /// How far the rack is scrolled (where it glides to), a part to scroll
    /// to once it is laid out, and a part's height while its edge is dragged.
    rack_y: f64,
    rack_scrolls: HashMap<String, [f64; 2]>,
    /// Where the rack was scrolled to when last drawn.
    rack_drawn: f64,
    reveal: Option<usize>,
    resizing: Option<(usize, f64)>,
    /// Each part's notices and controls at their full height, as last laid out.
    bodies: HashMap<usize, f64>,
    /// Each preset's neighbors in its library folder, of which scan and shelf.
    neighbors: (std::sync::Weak<Vec<PathBuf>>, usize, HashMap<String, [Option<String>; 2]>),
    started: Instant,
    /// The computer keyboard's octave, velocity and held keys.
    computer: Arc<computer::Computer>,
    /// A mouse glissando: the key it began on (which holds the pointer)
    /// and the key now sounding.
    gliss: Option<(u8, u8)>,
    /// The mod wheel's unrounded value while it is dragged.
    modulation: Option<f64>,
    /// The system file dialog, answering on a later frame.
    picker: Arc<picker::Picker>,
    /// The mixer's strip width and meter holds.
    /// The mixer shows the output tree, else the flat console.
    mix_tree: mix_tree::State,
    report: load_report::State,
    /// Each part's library interface as drawn, by slot.
    faces: HashMap<usize, part::Face>,
    /// Each part's views beside its interface.
    inside: HashMap<usize, inside::State>,
    editor: editor::State,
    /// The spectrum on screen, and the strip it shows this frame
    /// ([`crate::plugin::Scope::source`]; 0 for none).
    analyser: spectrum::Analyser,
    scope: usize,
    /// The window's size when its resize corner was grabbed.
    corner: Option<Size>,
}

impl EditorState {
    /// Select `slot`: the rack marks it and the keyboard plays it.
    fn select(&mut self, slot: usize) {
        self.selected = slot;
        self.unselected = false;
    }

    /// Let go of the selection: no part is marked, the keyboard plays them all.
    fn selected_none(&mut self) {
        self.unselected = true;
    }

    /// The selected part, if one is.
    fn chosen(&self) -> Option<usize> {
        (!self.unselected).then_some(self.selected)
    }

    /// The slot the keyboard, computer keys and wheels play.
    fn played(&self) -> usize {
        self.chosen().unwrap_or(crate::plugin::EVERY_PART)
    }
}

/// The browser's files by library name, as indices into the scan.
type Libraries = std::collections::BTreeMap<String, Vec<usize>>;

/// One frame's inputs: the loader's view, the rack being edited, the editor state.
struct Cx<'a> {
    p: &'a Arc<SamplerParams>,
    view: View,
    /// The app's settings as this frame began: library folders and covers.
    settings: Arc<crate::library::Settings>,
    selection: Selection,
    state: &'a mut EditorState,
}

#[derive(Clone)]
enum RackDrag {
    Instrument(String),
    Part(usize),
}

impl Cx<'_> {
    /// `library`'s artwork made into what the editor shows, once it is.
    /// Its own artwork, unless the player chose a picture or the generated
    /// cover for it; the generated cover when it has none.
    fn looks(&self, library: &str) -> Option<Arc<art::Looks>> {
        let found = self.view.shelf.named(library)?;
        let artwork = self.view.artwork.get(library).cloned();
        let cover = || cover::Spec::new(&found.name, &found.vendor, found.hue);
        let dir = found.dir.to_string_lossy();
        let source = match (self.settings.covers.get(dir.as_ref()), artwork) {
            (Some(crate::library::Cover::Custom { file, stamp }), _) => art::Source::File(file.into(), *stamp),
            (Some(crate::library::Cover::Generated), artwork) => art::Source::Cover(cover(), artwork),
            (None, None) => art::Source::Cover(cover(), None),
            (None, Some(image)) => art::Source::Image(image),
        };
        self.state.art.get(library, source)
    }

    /// `library`'s color: its artwork's dominant hue at a fixed, quiet
    /// lightness and chroma, so every library reads alike. None when that
    /// hue is orange: no color is drawn orange.
    fn tint(&self, library: &str) -> Option<Color> {
        let hue = self.looks(library)?.tint.filter(|&h| !theme::orange(h))?;
        Some(Color::oklch(0.66, 0.11, hue))
    }

    /// Whether artwork shows blurred.
    fn blurred(&self) -> bool {
        !self.selection.sharp_artwork
    }

    /// The library a preset lives in, by name.
    fn library_of(&self, path: &Path) -> String {
        library_of(&self.view.shelf, path)
    }

    /// The name of the instrument loaded in `slot`, once one is.
    fn instrument_name(&self, slot: usize) -> Option<String> {
        self.view.parts.get(slot).map(|v| v.active.clone()).filter(|n| !n.is_empty())
    }

    /// The selected part when it holds an instrument.
    fn part(&self) -> Option<&Part> {
        self.selection
            .parts
            .get(self.state.selected)
            .filter(|p| !p.path.is_empty())
    }

    /// Select `slot`, unfold it and scroll the rack to it.
    fn show(&mut self, slot: usize) {
        self.state.select(slot);
        self.state.reveal = Some(slot);
        self.state.notice.clear();
        self.state.renaming = None;
        if let Some(part) = self.selection.parts.get_mut(slot) {
            part.collapsed = false;
        }
    }

    fn remember(&mut self, path: &str) {
        // Its library was used now: the browser can list by that.
        if let Some(library) = self.view.shelf.of(Path::new(path)) {
            let dir = library.dir.to_string_lossy().into_owned();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            if self.settings.used.get(&dir) != Some(&now) {
                self.p.shared.libraries.edit(|s| {
                    s.used.insert(dir, now);
                });
            }
        }
        let recent = &mut self.selection.recent;
        recent.retain(|p| p != path);
        recent.insert(0, path.to_owned());
        recent.truncate(6);
    }

    /// Star `path`, or unstar it.
    fn toggle_favorite(&mut self, path: &str) {
        let favorites = &mut self.selection.favorites;
        match favorites.iter().position(|p| p == path) {
            Some(at) => {
                favorites.remove(at);
            }
            None => favorites.push(path.to_owned()),
        }
    }

    /// Open a preset from the browser: a multi replaces the rack, an instrument
    /// already in the rack is shown, anything else takes a free slot.
    fn open(&mut self, path: &Path) {
        let text = path.to_string_lossy().into_owned();
        self.remember(&text);
        if library::is_multi(path) {
            self.p.shared.queue_multi(text);
            self.state.notice.clear();
        } else if let Some(slot) = self
            .selection
            .parts
            .iter()
            .position(|p| p.path == text && p.program == 0)
        {
            self.show(slot);
        } else {
            self.add(text);
        }
    }

    // The frame snapshot predates these edits: prepare its new rows as well as
    // the shared atomics before any header or control indexes the appended slot.
    fn ensure_parts(&mut self) {
        self.p.shared.ensure_parts(self.selection.parts.len());
        self.view.parts.resize_with(self.view.parts.len().max(self.selection.parts.len()), PartView::default);
    }

    /// Add an instrument to the first free slot and show it.
    fn add(&mut self, path: String) {
        self.remember(&path);
        let mut slot = 0;
        for program in 0..program_count(&path) {
            let part = Part { program, ..new_part(&self.selection, &self.settings, path.clone()) };
            slot = add_part(&mut self.selection, part);
        }
        self.ensure_parts();
        self.show(slot);
    }

    /// Apply to an explicit base, leaving the part intact until validation.
    fn snapshot(&mut self, slot: usize, path: String) {
        let accepted = self.selection.parts.get(slot)
            .is_some_and(|part| self.p.shared.queue_snapshot(slot, part, path));
        if accepted { self.show(slot); }
        else { self.state.notice = "Select a base NKI instrument before loading a snapshot.".into(); }
    }

    /// Put another instrument (or a multi) in `slot`.
    fn replace(&mut self, slot: usize, path: String) {
        self.remember(&path);
        if library::is_multi(Path::new(&path)) {
            self.p.shared.queue_multi(path);
            return;
        }
        replace_part(&mut self.selection.parts[slot], path);
        self.show(slot);
    }

    /// A copy of `slot` in the next free slot, shown.
    fn duplicate(&mut self, slot: usize) {
        let Some(part) = self.selection.parts.get(slot).cloned() else {
            return;
        };
        let copy = add_part(&mut self.selection, part);
        self.ensure_parts();
        move_part(&mut self.selection, copy, slot);
        move_part(&mut self.selection, slot, copy);
        self.show(copy);
    }

    /// Empty `slot` and show whichever part now comes first.
    fn remove(&mut self, slot: usize) {
        if slot >= self.selection.parts.len() {
            return;
        }
        self.selection.parts[slot] = Part::default();
        self.selection.order.retain(|n| *n as usize != slot);
        let next = self.selection.order.first().map_or(0, |n| *n as usize);
        self.state.select(next);
        self.state.renaming = None;
    }

    /// Move `slot` `by` places along the rack.
    fn move_by(&mut self, slot: usize, by: i32) {
        let order = self.selection.order.clone();
        let Some(at) = order.iter().position(|n| *n as usize == slot) else {
            return;
        };
        match by {
            -1 if at > 0 => move_part(&mut self.selection, slot, order[at - 1] as usize),
            1 if at + 1 < order.len() => {
                let next = order[at + 1] as usize;
                move_part(&mut self.selection, next, slot);
            }
            _ => {}
        }
    }
}

/// The name of the library `path` is in; empty outside every library.
fn library_of(shelf: &crate::library::Shelf, path: &Path) -> String {
    shelf.of(path).map(|l| l.name.clone()).unwrap_or_default()
}

/// Instrument state belongs to its preset; rack routing and player settings stay.
fn replace_part(part: &mut Part, path: String) {
    part.path = path;
    part.program = 0;
    part.snapshot.clear();
    part.view = 0;
    part.name.clear();
    // Another instrument has another output tree, switching and dynamics.
    part.nodes.clear();
    part.switching = 0;
    part.articulation_overlay = Default::default();
    part.dynamics = -1;
}

/// How many rack parts `path` opens as: one per program of a Kontakt multi.
fn program_count(path: &str) -> u32 {
    let path = Path::new(path);
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkm")) {
        return 1;
    }
    sampler_kontakt::read_multi(path).map_or(1, |m| m.programs.len().max(1) as u32)
}

/// A part for `path` on the input and output the settings give new parts.
fn new_part(selection: &Selection, settings: &crate::library::Settings, path: String) -> Part {
    let (port, channel) = settings.new_input.unwrap_or_else(|| selection.next_input());
    Part {
        path,
        port,
        channel,
        output: settings.new_output.unwrap_or(0),
        output_manual: settings.new_output.is_some(),
        ..Default::default()
    }
}

/// Put `part` in the first empty slot, or append a new slot.
fn add_part(selection: &mut Selection, part: Part) -> usize {
    let slot = selection
        .parts
        .iter()
        .position(|p| p.path.is_empty())
        .unwrap_or(selection.parts.len());
    if slot == selection.parts.len() {
        selection.parts.push(part);
    } else {
        selection.parts[slot] = part;
    }
    selection.order.retain(|n| *n != slot as u32);
    selection.order.push(slot as u32);
    slot
}

/// Move `from` to just before `before` in the rack order.
fn move_part(selection: &mut Selection, from: usize, before: usize) {
    if from == before {
        return;
    }
    selection.order.retain(|n| *n != from as u32);
    let to = selection
        .order
        .iter()
        .position(|n| *n == before as u32)
        .unwrap_or(selection.order.len());
    selection.order.insert(to, from as u32);
}

/// Clamp what the host restored and keep `order` a permutation of loaded slots.
fn sanitize(selection: &mut Selection) {
    for part in &mut selection.parts {
        part.channel = part.channel.clamp(-1, 15);
        part.port = part.port.min(3);
        part.output = part.output.min(crate::sound::BUSES as u8 - 1);
        part.gain = if part.gain.is_finite() {
            part.gain.clamp(-60., 6.)
        } else {
            0.
        };
        part.pan = if part.pan.is_finite() {
            part.pan.clamp(-1., 1.)
        } else {
            0.
        };
        let tune = crate::sound::TUNE_RANGE;
        part.tune = if part.tune.is_finite() {
            part.tune.clamp(-tune, tune)
        } else {
            0.
        };
    }
    let mut seen = vec![false; selection.parts.len()];
    let parts = &selection.parts;
    selection.order.retain(|n| {
        let n = *n as usize;
        let keep = n < parts.len() && !parts[n].path.is_empty() && !seen[n];
        if keep {
            seen[n] = true;
        }
        keep
    });
    for (n, part) in selection.parts.iter().enumerate() {
        if !part.path.is_empty() && !seen[n] {
            selection.order.push(n as u32);
        }
    }
}

/// The bounded current OS gesture; source epoch still guards admission.
struct FileDrag {
    slot: usize, epoch: u64, source_slot: u8, widget: sampler_ui_ir::Widget,
    paths: Vec<(u32, sampler_ui_ir::Value)>,
}

fn widget_files(p: &SamplerParams, drag: &std::sync::Mutex<Option<FileDrag>>, ui: &Ui, at: Point, paths: &[PathBuf], dropped: bool) -> Option<bool> {
    let view = shown(&p.shared.view);
    let mut target = None;
    let mut rejected = false;
    for (slot, published) in view.parts.iter().enumerate() {
        for (shown, base) in published.interfaces.iter().enumerate() {
            let mut face = base.clone();
            if let Some(patch) = published.updates.get(shown) { patch.apply(base, &Default::default(), &mut face); }
            let namespace = format!("part-{slot}-epoch-{}-script-{shown}", published.generation);
            if ir_view::file_drop_target(ui, &namespace, &face, at).is_none() { continue; }
            let Some((n, edits)) = ir_view::file_drop(ui, &namespace, &face, at, paths, dropped) else { rejected = true; break };
            let interaction = part::interaction(&edits[0]);
            let source_slot = match face.source { sampler_ui_ir::Source::Ksp { slot } => slot, _ => 0 };
            let paths = edits.into_iter().map(|e| (e.index, e.value)).collect();
            target = Some((FileDrag { slot, epoch: published.generation, source_slot, widget: face.widgets[n.0].clone(), paths }, interaction));
            break;
        }
        if target.is_some() || rejected { break; }
    }
    let mut last = lock(drag);
    if let Some(previous) = last.take() {
        let same = target.as_ref().is_some_and(|(next, _)| next.slot == previous.slot && next.epoch == previous.epoch
            && next.source_slot == previous.source_slot && next.widget.source_id == previous.widget.source_id);
        if !same {
            p.shared.set_widget_batch_at(previous.slot, previous.epoch, previous.source_slot, &previous.widget, previous.paths,
                sampler_core::WidgetInteraction { event: 4, mouse_over: false, ..Default::default() });
        }
    }
    let Some((target, interaction)) = target else { return rejected.then_some(false) };
    let accepted = p.shared.set_widget_batch_at(target.slot, target.epoch, target.source_slot, &target.widget, target.paths.clone(), interaction);
    if accepted && !dropped { *last = Some(target); }
    Some(accepted)
}

/// Desktop files first target authored MouseAreas, then library/rack actions.
fn native_files(p: &SamplerParams, picker: &picker::Picker, drag: &std::sync::Mutex<Option<FileDrag>>, ui: &Ui, at: Point, paths: &[PathBuf], dropped: bool) -> bool {
    if let Some(accepted) = widget_files(p, drag, ui, at, paths, dropped) { return accepted; }

    let inside = |id: &str| {
        ui.scene().and_then(|s| s.surface(id)).is_some_and(|s| {
            let r = s.frame;
            at.x >= r.x && at.x < r.x + r.size.width && at.y >= r.y && at.y < r.y + r.size.height
        })
    };
    // A picture on a library in the browser becomes its cover.
    let picture = |p: &PathBuf| {
        p.extension().is_some_and(|e| ["png", "jpg", "jpeg"].iter().any(|x| e.eq_ignore_ascii_case(x)))
    };
    if paths.len() == 1 && picture(&paths[0]) {
        let rows = lock(&picker.rows).clone();
        let Some(library) = (rows.into_iter().enumerate())
            .find_map(|(n, dir)| dir.filter(|_| inside(&format!("library-{n}"))))
        else {
            return false;
        };
        if dropped {
            let _ = p.shared.libraries.set_artwork(&library, &paths[0]);
        }
        return true;
    }
    // A folder dropped on the browser is a folder of libraries to add.
    if paths.len() == 1 && paths[0].is_dir() && inside("browser") {
        if dropped {
            p.shared.libraries.add_root(&paths[0], false);
        }
        return true;
    }
    if paths.len() == 1 && library::is_multi(&paths[0]) {
        if dropped {
            p.shared.queue_multi(paths[0].to_string_lossy().into());
        }
        return true;
    }
    if paths.len() == 1 && paths[0].extension().is_some_and(|e| e.eq_ignore_ascii_case("nksn")) {
        let selection = read(&p.selection);
        let target = (0..selection.parts.len()).find(|n| inside(&format!("header-{n}")));
        let Some(slot) = target.filter(|&n| selection.parts[n].snapshot_base()) else { return false; };
        if dropped { p.shared.queue_snapshot(slot, &selection.parts[slot], paths[0].to_string_lossy().into_owned()); }
        return true;
    }
    if paths.is_empty() || !paths.iter().all(|p| library::is_instrument(p)) {
        return false;
    }
    let mut selection = write(&p.selection);
    // A part's header takes the file in place of the part.
    let target = (0..selection.parts.len()).find(|n| inside(&format!("header-{n}")));
    if dropped {
        for (n, path) in paths.iter().enumerate() {
            let path = path.to_string_lossy().into_owned();
            let slot = match target.filter(|_| n == 0) {
                Some(slot) => {
                    replace_part(&mut selection.parts[slot], path);
                    slot
                }
                None => {
                    let settings = p.shared.libraries.settings();
                    let mut slot = 0;
                    for program in 0..program_count(&path) {
                        let part = Part { program, ..new_part(&selection, &settings, path.clone()) };
                        slot = add_part(&mut selection, part);
                    }
                    slot
                }
            };
            p.shared.focus_request.store(slot as u64, Ordering::Relaxed);
        }
    }
    true
}

fn build(
    params: &Arc<SamplerParams>,
    meters: Arc<Meters>,
    computer: Arc<computer::Computer>,
    picker: Arc<picker::Picker>,
    art: Arc<art::Art>,
) -> impl FnMut(&mut Ui, &mut Bridge<SamplerParams>) -> El + Send + 'static + use<> {
    let mut state = EditorState {
        search: String::new(),
        source: None,
        pane: None,
        split: match read(&params.selection).browser_split {
            s if s > 0. => f64::from(s).clamp(browser::SPLIT_MIN, browser::SPLIT_MAX),
            _ => browser::SPLIT,
        },
        uvi: false,
        tab: Tab::Rack,
        settings: false,
        saving: None,
        save_error: String::new(),
        browser: true,
        sidebar: match read(&params.selection).browser_width {
            w if w > 0. => f64::from(w).clamp(SIDEBAR_MIN, SIDEBAR_MAX),
            _ => SIDEBAR,
        },
        keyboard: true,
        octave: 2,
        keyboard_for: None,
        selected: 0,
        // Every part shows until one is clicked.
        unselected: true,
        notice: String::new(),
        root: String::new(),
        last_poll: Instant::now() - Duration::from_secs(1),
        meters,
        menu: None,
        cursor: None,
        browse: Default::default(),
        logs: Default::default(),
        renaming: None,
        art,
        libraries: Default::default(),
        rack_y: 0.,
        rack_scrolls: Default::default(),
        rack_drawn: 0.,
        reveal: None,
        resizing: None,
        bodies: HashMap::new(),
        neighbors: Default::default(),
        started: Instant::now(),
        computer,
        gliss: None,
        modulation: None,
        picker,
        mix_tree: Default::default(),
        report: Default::default(),
        faces: Default::default(),
        inside: Default::default(),
        editor: Default::default(),
        analyser: Default::default(),
        scope: 0,
        corner: None,
    };
    move |ui, bridge| {
        // The loader also runs from the audio thread; poll here so a stopped host still loads.
        if state.last_poll.elapsed() > Duration::from_millis(100) {
            if let Some(tasks) = bridge.context().and_then(|c| c.tasks::<Load>()) {
                tasks.spawn_coalescing(Load);
            }
            state.last_poll = Instant::now();
        }
        let p = bridge.params().clone();
        p.shared.editor_watch.store(usize::MAX,Ordering::Relaxed);
        let mut selection = read(&p.selection).clone();
        p.shared.ensure_parts(selection.parts.len());
        let mut view = shown(&p.shared.view);
        view.parts.resize_with(view.parts.len().max(selection.parts.len()), PartView::default);
        let before = selection.clone();
        sanitize(&mut selection);
        let focus = p.shared.focus_request.swap(u64::MAX, Ordering::Relaxed);
        if focus < selection.parts.len() as u64 {
            state.select(focus as usize);
            state.notice.clear();
        }
        state.selected = state.selected.min(selection.parts.len().saturating_sub(1));
        p.shared.selected.store(state.played() as u64, Ordering::Relaxed);
        let window = ui
            .scene()
            .and_then(|s| s.surface("editor-root"))
            .map_or(Size::new(1180., 760.), |s| s.frame.size);

        let mut cx = Cx {
            p: &p,
            view,
            settings: p.shared.libraries.settings(),
            selection,
            state: &mut state,
        };
        mapping::release(ui, &mut cx);
        shortcuts(ui, &mut cx);
        picked(&mut cx);
        let top = header::top_bar(ui, &mut cx, bridge);
        let settings = cx.state.settings.then(|| header::settings(ui, &mut cx));
        let saving = cx.state.saving.is_some().then(|| header::save_multi(ui, &mut cx));
        let browser_w = cx
            .state
            .sidebar
            .clamp(SIDEBAR_MIN, SIDEBAR_MAX.min(window.width * 0.42));
        // The drawer slides out from under the left edge and back; drawn
        // only while any of it shows.
        let open = ui.tween_with("sidebar-open", if cx.state.browser { 1. } else { 0. }, quick());
        let sidebar = (open > 0.005).then(|| {
            let drawer = browser::sidebar(ui, &mut cx).w(browser_w).h(Len::Pct(100.)).shrink(0);
            stack![drawer.anchor(Align::End, Align::Start)]
                .w((browser_w * open).round())
                .h(Len::Pct(100.))
                .shrink(0)
                .clip()
        });
        let splitter = cx.state.browser.then(|| splitter(ui, &mut cx, browser_w));
        let main = main_view(ui, &mut cx);
        let keys = keyboard::dock(ui, &mut cx);
        let menu = menu::view(ui, &mut cx, window);
        let ghost = ghost(ui, &cx);
        cx.state.meters.logs_visible.store(cx.state.tab == Tab::Logs, Ordering::Relaxed);

        let ui_zoom = cx.settings.editor_scale();
        let Cx { mut selection, view, .. } = cx;
        if selection != before {
            // Parts added, removed or rerouted are routed at once.
            p.shared.reroute(&mut selection);
            let mut current = write(&p.selection);
            if *current == before {
                *current = selection;
                let _ = p.shared.controls.force_push(mix(&current));
                p.shared
                    .midi_thru
                    .store(current.midi_thru, Ordering::Release);
            }
        }
        p.shared.selected.store(state.played() as u64, Ordering::Relaxed);

        let mut shell = vec![top, header::loading_bar(&view, &p, state.started)];
        shell.extend(settings);
        shell.extend(saving);
        let mut middle: Vec<El> = sidebar.into_iter().collect();
        middle.extend(splitter);
        middle.push(main);
        shell.push(row(middle).gap(0).flex(1).min_h(0));
        shell.push(rule());
        shell.push(keys);
        let mut layers = vec![col(shell).gap(0).full(), resize_corner(ui, &mut state.corner, window, ui_zoom, bridge)];
        layers.extend(menu);
        layers.extend(ghost);
        stack(layers)
            .full()
            .fill(Role::Background)
            .radius(0)
            .clip()
            .id("editor-root")
    }
}

/// Take what a file dialog answered: a library folder to scan, or where
/// to save the rack as a multi.
fn picked(cx: &mut Cx) {
    match cx.state.picker.take() {
        Some(picker::Picked::DialogError(error)) => cx.state.notice = format!("The file picker could not open: {error}"),
        Some(picker::Picked::Revealed(result)) => {
            if let Err(error) = result { cx.state.notice = error; }
        }
        Some(picker::Picked::Created(Ok(path))) => {
            cx.p.shared.libraries.add_root(&path, true);
            cx.state.notice = format!("Library created in {}", path.display());
        }
        Some(picker::Picked::Created(Err(e))) => cx.state.notice = format!("No library was created: {e}"),
        Some(picker::Picked::Folder(path, single)) => cx.p.shared.libraries.add_root(&path, single),
        Some(picker::Picked::Artwork { library, picture }) => {
            if let Err(e) = cx.p.shared.libraries.set_artwork(&library, &picture) {
                cx.state.notice = format!("The picture was not used: {e}");
            }
        }
        Some(picker::Picked::Snapshot { slot, source, path }) => {
            if cx.selection.parts.get(slot).is_some_and(|p| p.source() == source) {
                cx.snapshot(slot, path.to_string_lossy().into_owned());
            } else { cx.state.notice = "Snapshot ignored: the base instrument changed while its dialog was open.".into(); }
        }
        Some(picker::Picked::Multi(mut path)) => {
            if !library::is_multi(&path) {
                path.as_mut_os_string().push(format!(".{}", library::MULTI));
            }
            if let Err(e) = header::save_multi_as(cx, &path) {
                cx.state.notice = format!("The multi was not saved: {e:#}");
            }
        }
        None => {}
    }
}

/// Global keys: Delete removes the shown part, Ctrl+D duplicates it, Space
/// auditions it. The browser reads the arrows and Enter itself.
fn shortcuts(ui: &mut Ui, cx: &mut Cx) {
    // A focused button takes Space for itself.
    let free = ui
        .focus_key()
        .is_none_or(|k| k.starts_with("key-") || k.starts_with("header-") || k.starts_with("name-"));
    let keys = ui.shortcuts().to_vec();
    let slot = cx.state.selected;
    let loaded = cx.part().is_some() && cx.state.chosen().is_some();
    for k in keys {
        let ctrl = k.mods.ctrl || k.mods.cmd;
        match k.key {
            Key::Delete if loaded && cx.state.renaming.is_none() => cx.remove(slot),
            Key::Char('d' | 'D') if ctrl && loaded => cx.duplicate(slot),
            // The browser shut: Ctrl+F opens it on its filter.
            Key::Char('f' | 'F') if ctrl && !cx.state.browser => (cx.state.browser, cx.state.browse.find) = (true, true),
            Key::Char(' ') if free && loaded && !k.mods.shift => cx.p.shared.audition(None),
            Key::Escape if cx.state.menu.is_none() && cx.state.renaming.is_none() && !cx.state.inside.values().any(inside::State::editing) && !cx.state.browse.typing() => cx.state.selected_none(),
            _ => {}
        }
    }
}

/// The handle between the browser and the rest: drag to resize, double-click
/// to restore the width.
fn splitter(ui: &mut Ui, cx: &mut Cx, width: f64) -> El {
    let r = ui.get("splitter");
    let state = &mut *cx.state;
    if r.dragged {
        state.sidebar = (width + r.drag_delta.x).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
    }
    if r.double_clicked {
        state.sidebar = SIDEBAR;
    }
    // Saved once let go, not on every step of the drag.
    if r.released || r.double_clicked {
        cx.selection.browser_width = state.sidebar as f32;
    }
    let lift = theme::edge_lift(ui, "splitter");
    // A hairline at rest, the accent under the hand; grabbed a little wide.
    canvas(move |s| {
        let mut d = vec![Draw::fill(rect(0., 0., 1., s.height), Role::Ink.alpha(0.08))];
        d.extend(theme::edge_mark(s, 1., true, lift));
        d
    })
    .w(theme::EDGE_GRAB + 2.)
    .h(Len::Pct(100.))
    .shrink(0)
    .cursor(Cursor::ResizeH)
    .tip("Drag to resize the browser, double-click to reset")
    .named("Resize browser")
    .id("splitter")
}

/// The window's resize corner, bottom right: drag it to size the window
/// (the host decides), with the diagonal cursor and a grip that warms.
fn resize_corner(ui: &mut Ui, from: &mut Option<Size>, window: Size, zoom: f64, bridge: &mut Bridge<SamplerParams>) -> El {
    let id = "window-corner";
    let r = ui.get(id);
    if r.pressed {
        *from = Some(window);
    }
    if let (true, Some(from)) = (r.dragged, *from) {
        let (w, h) = (((from.width + r.drag_total.x) * zoom).max(900.), ((from.height + r.drag_total.y) * zoom).max(600.));
        if (w.round(), h.round()) != ((window.width * zoom).round(), (window.height * zoom).round())
            && let Some(c) = bridge.context()
        {
            // A host that sizes only from its own frame says no; nothing to undo.
            let _ = c.request_resize(w.round() as u32, h.round() as u32);
        }
    }
    if r.released {
        *from = None;
    }
    moose::mui::window::resize_corner(r.hovered || r.held);
    let lift = theme::edge_lift(ui, id);
    canvas(move |s| {
        let ink = if lift > 0.01 { Fill::from(accent().with_alpha(0.35 + 0.55 * lift)) } else { Role::Ink.alpha(0.2) };
        // Two short diagonals in the corner, on pixel centres.
        [4., 8.]
            .into_iter()
            .map(|d| {
                let path = moose::mui::mui::geometry::Path::polyline(
                    [Point::new(s.width - d - 1.5, s.height - 1.5), Point::new(s.width - 1.5, s.height - d - 1.5)],
                    false,
                );
                Draw::stroke(path, ink.clone(), 1.)
            })
            .collect()
    })
    .square(theme::EDGE_GRAB * 2. + 2.)
    .anchor(Align::End, Align::End)
    .tip("Drag to resize the window")
    .named("Resize the window")
    .id(id)
}

/// What a drag carries, following the pointer.
fn ghost(ui: &Ui, cx: &Cx) -> Option<El> {
    let label = match ui.dragging::<RackDrag>()? {
        RackDrag::Instrument(path) => header::stem(path),
        RackDrag::Part(slot) => rack::name(cx, *slot),
    };
    let at = ui.local("editor-root")?;
    Some(
        row![body(label).text_size(TEXT).lines(1).min_w(0)]
            .align(Align::Center)
            .pad((SPACE, TIGHT))
            .max_size(Size::new(SIDEBAR, CONTROL * 2.))
            .fill(Role::Level(3))
            .stroke(accent())
            .stroke_width(1)
            .at(at.x + INSET, at.y + SPACE),
    )
}

/// View tabs over the rack, the mixer or the logs.
fn main_view(ui: &mut Ui, cx: &mut Cx) -> El {
    let mut tabs = Vec::new();
    for (tab, label, id) in [
        (Tab::Rack, "Rack", "tab-rack"),
        (Tab::Mixer, "Mixer", "tab-mixer"),
        (Tab::Report, "Report", "tab-report"),
        (Tab::Logs, "Logs", "tab-logs"),
    ] {
        let (hit, el) = theme::tab(ui, id, label, cx.state.tab == tab);
        if hit {
            cx.state.tab = tab;
        }
        tabs.push(el);
    }
    let mut content = vec![
        row(tabs)
            .gap(INSET + TIGHT)
            .align(Align::Center)
            .pad((INSET, 0))
            .shrink(0)
            .fill(Role::Surface),
        rule(),
    ];
    if !cx.state.notice.is_empty() {
        content.push(banner(Role::Warning, cx.state.notice.clone()));
    }
    // A spectrum shown below names its strip; none shown, none is copied.
    cx.state.scope = 0;
    cx.state.meters.animating.store(false, Ordering::Relaxed);
    content.push(match cx.state.tab {
        Tab::Rack => rack::view(ui, cx),
        Tab::Mixer => mixer_view(ui, cx),
        Tab::Report => report_view(ui, cx),
        Tab::Logs => logs::view(ui, cx),
    });
    cx.p.shared.scope.source.store(cx.state.scope, Ordering::Relaxed);
    if cx.state.analyser.busy() && cx.state.scope != 0 {
        cx.state.meters.animating.store(true, Ordering::Relaxed);
    }
    col(content)
        .gap(0)
        .flex(1)
        .min_w(0)
        .min_h(0)
        .fill(Role::Background)
        .id("center")
}

/// The mixer tab: each instrument's outputs as a tree.
fn mixer_view(ui: &mut Ui, cx: &mut Cx) -> El {
    let mode = crate::routing::Outputs::of(cx.selection.outputs).label();
    let (outputs_hit, outputs) = dropdown(ui, "mix-outputs", mode, "Outputs: routing to the host");
    if outputs_hit {
        menu::open_under(ui, cx, menu::Target::Routing, "mix-outputs");
    }
    let m = &mut cx.state.mix_tree;
    let (narrow_hit, narrow) = latch(ui, "mix-narrow", "Narrow", "Narrow strips: level and routing", !m.wide);
    let (wide_hit, wide) = latch(ui, "mix-wide", "Wide", "Wide strips: with the instrument's inserts", m.wide);
    if narrow_hit || wide_hit {
        m.wide = wide_hit;
    }
    let (off_hit, off) = latch(ui, "mix-spectrum-off", "Off", "No spectrum", m.spectrum == mix_tree::Spectrum::Off);
    let (part_hit, part) = latch(ui, "mix-spectrum-part", "Part", "The selected part's output", m.spectrum == mix_tree::Spectrum::Part);
    let (master_hit, master) = latch(ui, "mix-spectrum-master", "Master", "Everything sent to the host", m.spectrum == mix_tree::Spectrum::Master);
    for (hit, to) in [(off_hit, mix_tree::Spectrum::Off), (part_hit, mix_tree::Spectrum::Part), (master_hit, mix_tree::Spectrum::Master)] {
        if hit {
            m.spectrum = to;
        }
    }
    let bar = strip(vec![section("Strips"), segmented(vec![narrow, wide]), spacer(),
        section("Spectrum"), segmented(vec![off, part, master]), section("Outputs"), outputs])
        .pad((INSET, TIGHT)).fill(Role::Surface);
    let mut tree = bridge::tree(cx);
    let levels = bridge::levels(cx.p, &tree);
    let height = ui.scene().and_then(|s| s.surface("mix-tree")).map_or(TEXT * 36., |s| s.frame.size.height - 2. * SPACE);
    let body = mix_tree::view(ui, &mut tree, &mut cx.state.mix_tree, crate::sound::BUSES as u8, height, levels);
    bridge::apply(cx, &tree);
    for node in &tree.nodes {
        let id = node.id;
        if id >> 16 != 0 && id & 0xffff == 0 {
            let anchor = format!("mt-aux-{id}");
            if ui.get(anchor.as_str()).activated() { menu::open_under(ui, cx, menu::Target::Aux((id >> 16) as usize - 1), &anchor); }
        }
        if (id >> 16 == 0 || id & 0xffff == 0) && [format!("mt-strip-{id}"), format!("mt-name-{id}"), format!("mt-fader-{id}"), format!("mt-pan-{id}")].iter().any(|s| ui.get(s.as_str()).clicked_with(Button::Secondary)) {
            menu::open(ui, cx, menu::Target::Mixer(id));
        }
    }
    cx.state.meters.animating.store(true, Ordering::Relaxed);
    let mut rows = vec![bar, rule(), body];
    if cx.state.mix_tree.spectrum != mix_tree::Spectrum::Off {
        let source = match cx.state.mix_tree.spectrum {
            mix_tree::Spectrum::Part => cx.state.chosen().map_or(crate::plugin::SCOPE_MASTER, |slot| slot + 1),
            _ => crate::plugin::SCOPE_MASTER,
        };
        let shape = cx.spectrum(source);
        rows.push(spectrum::panel(shape, "mix-spectrum-graph").h(TEXT * 10.).flex(0).pad(INSET).shrink(0));
    }
    col(rows).gap(0).flex(1).min_h(0).min_w(0)
}

/// The selected part's load report.
fn report_view(ui: &mut Ui, cx: &mut Cx) -> El {
    if cx.part().is_none() {
        return caption("Select a loaded instrument to see its report").fill(secondary());
    }
    let slot = cx.state.selected;
    let report = bridge::report(cx, slot);
    load_report::view(ui, &mut cx.state.report, &report)
}

impl Cx<'_> {
    /// The spectrum of `source` ([`crate::plugin::Scope::source`]), which
    /// the audio thread copies from while this frame shows it.
    fn spectrum(&mut self, source: usize) -> Arc<spectrum::Shape> {
        self.state.scope = source;
        let rate = f64::from_bits(self.p.shared.rate.load(Ordering::Relaxed)) as f32;
        self.state.analyser.update(&self.p.shared.scope, source, rate)
    }
}

#[cfg(feature = "shots")]
pub use ir_view::uvi_ui_health;
#[cfg(test)]
mod loop_audit;
#[cfg(test)]
mod browser_tests;
#[cfg(test)]
mod chrome_tests;
#[cfg(test)]
mod keyboard_tests;
#[cfg(test)]
mod popup_tests;
#[cfg(test)]
mod distill_tests;
#[cfg(test)]
 pub(crate) fn audit_frames(p: &Arc<SamplerParams>) -> serde_json::Value { tests::audit_frames(p) }
#[cfg(all(test, feature = "library-access"))]
mod uvi_audit;
