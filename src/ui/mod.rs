//! The editor: a library browser, a Kontakt-style rack of parts with their
//! performance controls rebuilt natively, and a playable keyboard.
//!
//! ```text
//! ┌ top bar: wordmark · activity · meters · master · global actions ───────┐
//! ├ browser ┆ tabs: rack · mapping · info ─────────────────────────────────┤
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

mod browser;
mod header;
mod instrument;
mod keyboard;
mod menu;
mod panel;
mod rack;
#[cfg(test)]
mod tests;
mod theme;

use crate::engine::RACK_SLOTS;
use crate::import;
use crate::plugin::{Load, Part, PartView, SamplerParams, Selection, View, rack_controls};
use moose::mui::{Bridge, MuiEditor, mui::prelude::*, mui::prelude::Color};
use moose::prelude::*;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};
use theme::*;

pub(crate) fn editor(params: Arc<SamplerParams>) -> Box<dyn Editor> {
    let cpu = Arc::new(AtomicU32::new(0));
    let build = build(&params, cpu.clone());
    let drop_params = params.clone();
    let cancel_params = params.clone();
    let watch_params = params.clone();
    let mut watch = Watch::default();
    MuiEditor::new(params, theme::ui(), (1180, 760), build)
        .on_files(move |ui, at, paths, dropped| native_files(&drop_params, ui, at, paths, dropped))
        .on_cancel(move |_| cancel_params.shared.release_keyboard())
        .changed(move || watch.changed(&watch_params, &cpu))
        .fixed_zoom()
        .resizable((900, 600))
        .into_editor()
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

/// Decides, every display tick, whether anything the editor shows moved
/// outside its own input: a readout, a held key, the loader's view. An idle
/// editor then draws nothing at all.
#[derive(Default)]
struct Watch {
    signature: u64,
    cpu: f32,
    cpu_at: Option<Instant>,
    poll_at: Option<Instant>,
}

impl Watch {
    fn changed(&mut self, p: &SamplerParams, cpu_out: &AtomicU32) -> bool {
        let now = Instant::now();
        let due = |at: Option<Instant>, every: u64| {
            at.is_none_or(|t| now - t >= Duration::from_millis(every))
        };
        // The audio thread keeps its peak load; ease it down ten times a second.
        if due(self.cpu_at, 100) {
            self.cpu_at = Some(now);
            let peak = f32::from_bits(p.shared.cpu.swap(0, Ordering::Relaxed) as u32);
            self.cpu = peak.max(self.cpu * 0.8);
            cpu_out.store(self.cpu.to_bits(), Ordering::Relaxed);
        }
        let mut h = DefaultHasher::new();
        ((self.cpu * 100.).round() as u32).hash(&mut h);
        p.shared.voices.load(Ordering::Relaxed).hash(&mut h);
        p.shared.focus_request.load(Ordering::Relaxed).hash(&mut h);
        for owner in &p.shared.key_owners {
            (owner.load(Ordering::Relaxed) < 128).hash(&mut h);
        }
        let (loading, pending) = {
            let view = lock(&p.shared.view);
            fingerprint(&view, &mut h);
            let selection = read(&p.selection);
            let root = if selection.root.is_empty() {
                import::LIBRARY_ROOT
            } else {
                &selection.root
            };
            let pending = view.root != root
                || lock(&p.shared.multi_request).is_some()
                || (0..RACK_SLOTS).any(|n| {
                    let part = selection.parts.get(n);
                    let target = part.map_or((String::new(), 0), |p| (p.path.clone(), p.program));
                    view.parts[n].attempted.as_ref() != Some(&target)
                        && !(target.0.is_empty() && view.parts[n].attempted.is_none())
                });
            (view.parts.iter().any(|v| v.loading), pending)
        };
        if loading {
            for progress in &p.shared.load_progress {
                progress.load(Ordering::Relaxed).hash(&mut h);
            }
        }
        let signature = h.finish();
        let moved = signature != self.signature;
        self.signature = signature;
        // A stopped host runs no audio thread to start the loader: frames poll it.
        let poll = pending && due(self.poll_at, 100);
        if poll {
            self.poll_at = Some(now);
        }
        // The loading line sweeps until the first samples arrive.
        moved || poll || loading
    }
}

/// What of the loader's view is on screen, cheaply: pointers of what it
/// replaces wholesale, and the small fields it edits in place.
fn fingerprint(view: &View, h: &mut DefaultHasher) {
    fn at<T>(a: &Option<Arc<T>>) -> usize {
        a.as_ref().map_or(0, |a| Arc::as_ptr(a) as *const () as usize)
    }
    (&view.status, &view.multi_status, &view.root).hash(h);
    (Arc::as_ptr(&view.files) as usize, view.artwork.len()).hash(h);
    for v in &view.parts {
        (v.loading, v.bytes, &v.status, v.program, v.interface_status.len()).hash(h);
        (at(&v.instrument), at(&v.interface), at(&v.wallpaper)).hash(h);
        (Arc::as_ptr(&v.keys) as usize, Arc::as_ptr(&v.pictures) as usize).hash(h);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Rack,
    Mapping,
    Info,
}

/// Editor-only state that outlives a frame but not the window.
struct EditorState {
    search: String,
    /// The library open in the browser; `None` lists every library.
    library: Option<String>,
    multis: bool,
    tab: Tab,
    settings: bool,
    /// The browser is shown, and how wide.
    browser: bool,
    sidebar: f64,
    /// The keyboard dock is open.
    keyboard: bool,
    /// First octave on the keyboard.
    octave: i16,
    /// The part the keyboard was last centered on.
    keyboard_for: Option<(String, u32)>,
    selected: usize,
    notice: String,
    root: String,
    last_poll: Instant,
    /// Smoothed audio thread load, 0..1, as [`Watch`] last eased it.
    cpu: Arc<AtomicU32>,
    /// Script control being dragged: part, control, unrounded value.
    held: Option<(usize, usize, f64)>,
    /// The context menu showing.
    menu: Option<menu::Menu>,
    /// The browser's keyboard cursor: a preset path.
    cursor: Option<String>,
    /// Presets opened this session, newest first.
    recent: Vec<String>,
    /// A part's name while it is being edited.
    renaming: Option<(usize, String)>,
    /// Each library's color, from its artwork, worked out once.
    tints: HashMap<String, Option<Color>>,
    started: Instant,
}

impl EditorState {
    /// `library`'s color: its artwork's dominant hue at a fixed, quiet
    /// lightness and chroma, so every library reads alike.
    fn tint(&mut self, view: &View, library: &str) -> Option<Color> {
        *self
            .tints
            .entry(library.to_owned())
            .or_insert_with(|| {
                let hue = crate::artwork::tint(view.artwork.get(library)?)?;
                Some(Color::oklch(0.66, 0.11, hue))
            })
    }
}

/// One frame's inputs: the loader's view, the rack being edited, the editor state.
struct Cx<'a> {
    p: &'a SamplerParams,
    view: &'a View,
    selection: Selection,
    state: &'a mut EditorState,
}

#[derive(Clone)]
enum RackDrag {
    Instrument(String),
    Part(usize),
}

impl Cx<'_> {
    /// The library folder a preset lives in, relative to the scanned root.
    fn library_of(&self, path: &Path) -> String {
        library_of(&self.view.root, path)
    }

    fn part_view(&self) -> &PartView {
        &self.view.parts[self.state.selected]
    }

    /// The selected part when it holds an instrument.
    fn part(&self) -> Option<&Part> {
        self.selection
            .parts
            .get(self.state.selected)
            .filter(|p| !p.path.is_empty())
    }

    /// Select `slot` and unfold it.
    fn show(&mut self, slot: usize) {
        self.state.selected = slot;
        self.state.notice.clear();
        self.state.renaming = None;
        if let Some(part) = self.selection.parts.get_mut(slot) {
            part.collapsed = false;
        }
    }

    fn remember(&mut self, path: &str) {
        self.state.recent.retain(|p| p != path);
        self.state.recent.insert(0, path.to_owned());
        self.state.recent.truncate(6);
    }

    /// Open a preset from the browser: a multi replaces the rack, an instrument
    /// already in the rack is shown, anything else takes a free slot.
    fn open(&mut self, path: &Path) {
        let text = path.to_string_lossy().into_owned();
        self.remember(&text);
        if import::is_multi(path) {
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

    /// Add an instrument to the first free slot and show it.
    fn add(&mut self, path: String) {
        self.remember(&path);
        match add_part(
            &mut self.selection,
            Part {
                path,
                ..Default::default()
            },
        ) {
            Some(slot) => self.show(slot),
            None => {
                self.state.notice =
                    "The rack is full (16 instruments). Remove one to add another.".into()
            }
        }
    }

    /// Put another instrument (or a multi) in `slot`.
    fn replace(&mut self, slot: usize, path: String) {
        self.remember(&path);
        if import::is_multi(Path::new(&path)) {
            self.p.shared.queue_multi(path);
            return;
        }
        let part = &mut self.selection.parts[slot];
        part.path = path;
        part.program = 0;
        part.group = u32::MAX;
        part.name.clear();
        self.show(slot);
    }

    /// A copy of `slot` in the next free slot, shown.
    fn duplicate(&mut self, slot: usize) {
        let Some(part) = self.selection.parts.get(slot).cloned() else {
            return;
        };
        match add_part(&mut self.selection, part) {
            Some(copy) => {
                move_part(&mut self.selection, copy, slot);
                move_part(&mut self.selection, slot, copy);
                self.show(copy);
            }
            None => self.state.notice = "The rack is full (16 instruments).".into(),
        }
    }

    /// Empty `slot` and show whichever part now comes first.
    fn remove(&mut self, slot: usize) {
        if slot >= self.selection.parts.len() {
            return;
        }
        self.selection.parts[slot] = Part::default();
        self.selection.order.retain(|n| *n as usize != slot);
        let next = self.selection.order.first().map_or(0, |n| *n as usize);
        self.state.selected = next;
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

fn library_of(root: &str, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .and_then(|r| r.components().next())
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Put `part` in the first empty slot; `None` when the rack is full.
fn add_part(selection: &mut Selection, part: Part) -> Option<usize> {
    let slot = selection
        .parts
        .iter()
        .position(|p| p.path.is_empty())
        .or_else(|| (selection.parts.len() < RACK_SLOTS).then_some(selection.parts.len()))?;
    if slot == selection.parts.len() {
        selection.parts.push(part);
    } else {
        selection.parts[slot] = part;
    }
    selection.order.retain(|n| *n != slot as u32);
    selection.order.push(slot as u32);
    Some(slot)
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
    selection.parts.truncate(RACK_SLOTS);
    for part in &mut selection.parts {
        part.channel = part.channel.clamp(-1, 15);
        part.port = part.port.min(3);
        part.output = part.output.min(7);
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
    }
    let mut seen = [false; RACK_SLOTS];
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

/// Files dragged in from the desktop: `.nki` into the slot under the pointer
/// or free slots, one `.nkm` replaces the rack. Returns whether they are accepted.
fn native_files(p: &SamplerParams, ui: &Ui, at: Point, paths: &[PathBuf], dropped: bool) -> bool {
    if paths.len() == 1 && import::is_multi(&paths[0]) {
        if dropped {
            p.shared.queue_multi(paths[0].to_string_lossy().into());
        }
        return true;
    }
    let nki = |p: &PathBuf| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki"));
    if paths.is_empty() || !paths.iter().all(nki) {
        return false;
    }
    let mut selection = write(&p.selection);
    let inside = |id: &str| {
        ui.scene().and_then(|s| s.surface(id)).is_some_and(|s| {
            let r = s.frame;
            at.x >= r.x && at.x < r.x + r.size.width && at.y >= r.y && at.y < r.y + r.size.height
        })
    };
    // A part's header takes the file in place of the part.
    let target = (0..selection.parts.len()).find(|n| inside(&format!("header-{n}")));
    let free = RACK_SLOTS.saturating_sub(
        selection
            .parts
            .iter()
            .filter(|p| !p.path.is_empty())
            .count(),
    );
    if paths.len() > free + usize::from(target.is_some()) {
        return false;
    }
    if dropped {
        for (n, path) in paths.iter().enumerate() {
            let path = path.to_string_lossy().into_owned();
            let slot = match target.filter(|_| n == 0) {
                Some(slot) => {
                    let part = &mut selection.parts[slot];
                    part.path = path;
                    part.program = 0;
                    part.group = u32::MAX;
                    part.name.clear();
                    Some(slot)
                }
                None => add_part(
                    &mut selection,
                    Part {
                        path,
                        ..Default::default()
                    },
                ),
            };
            if let Some(slot) = slot {
                p.shared.focus_request.store(slot as u64, Ordering::Relaxed);
            }
        }
    }
    true
}

fn build(
    params: &Arc<SamplerParams>,
    cpu: Arc<AtomicU32>,
) -> impl FnMut(&mut Ui, &mut Bridge<SamplerParams>) -> El + Send + 'static {
    let mut root = read(&params.selection).root.clone();
    if root.is_empty() {
        root = import::LIBRARY_ROOT.into();
    }
    let mut state = EditorState {
        search: String::new(),
        library: None,
        multis: false,
        tab: Tab::Rack,
        settings: false,
        browser: true,
        sidebar: SIDEBAR,
        keyboard: true,
        octave: 2,
        keyboard_for: None,
        selected: 0,
        notice: String::new(),
        root,
        last_poll: Instant::now() - Duration::from_secs(1),
        cpu,
        held: None,
        menu: None,
        cursor: None,
        recent: Vec::new(),
        renaming: None,
        tints: HashMap::new(),
        started: Instant::now(),
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
        let view = lock(&p.shared.view).clone();
        let mut selection = read(&p.selection).clone();
        let before = selection.clone();
        sanitize(&mut selection);
        let focus = p.shared.focus_request.swap(128, Ordering::Relaxed);
        if focus < RACK_SLOTS as u64 {
            state.selected = focus as usize;
            state.notice.clear();
        }
        state.selected = state.selected.min(selection.parts.len().saturating_sub(1));
        p.shared
            .selected
            .store(state.selected as u64, Ordering::Relaxed);
        let window = ui
            .scene()
            .and_then(|s| s.surface("editor-root"))
            .map_or(Size::new(1180., 760.), |s| s.frame.size);

        let mut cx = Cx {
            p: &p,
            view: &view,
            selection,
            state: &mut state,
        };
        shortcuts(ui, &mut cx);
        let top = header::top_bar(ui, &mut cx, bridge);
        let settings = cx.state.settings.then(|| header::settings(ui, &mut cx));
        let browser_w = cx
            .state
            .sidebar
            .clamp(SIDEBAR_MIN, SIDEBAR_MAX.min(window.width * 0.42));
        let sidebar = cx
            .state
            .browser
            .then(|| browser::sidebar(ui, &mut cx).w(browser_w));
        let splitter = cx.state.browser.then(|| splitter(ui, cx.state, browser_w));
        let main = main_view(ui, &mut cx);
        let keys = keyboard::dock(ui, &mut cx);
        let menu = menu::view(ui, &mut cx, window);
        let ghost = ghost(ui, &cx);

        let Cx { selection, .. } = cx;
        if selection != before {
            let mut current = write(&p.selection);
            if *current == before {
                *current = selection;
                let _ = p.shared.controls.force_push(rack_controls(&current));
                p.shared
                    .midi_thru
                    .store(current.midi_thru, Ordering::Release);
            }
        }
        p.shared
            .selected
            .store(state.selected as u64, Ordering::Relaxed);

        let mut shell = vec![top, header::loading_bar(&view, &p, state.started)];
        shell.extend(settings);
        let mut middle: Vec<El> = sidebar.into_iter().collect();
        middle.extend(splitter);
        middle.push(main);
        shell.push(row(middle).gap(0).flex(1).min_h(0));
        shell.push(rule());
        shell.push(keys);
        let mut layers = vec![col(shell).gap(0).full()];
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

/// Global keys: Delete removes the shown part, Ctrl+D duplicates it, Space
/// auditions it. The browser reads the arrows and Enter itself.
fn shortcuts(ui: &mut Ui, cx: &mut Cx) {
    // A focused button takes Space for itself.
    let free = ui
        .focus_key()
        .is_none_or(|k| k.starts_with("key-") || k.starts_with("header-") || k.starts_with("name-"));
    let keys = ui.shortcuts().to_vec();
    let slot = cx.state.selected;
    let loaded = cx.part().is_some();
    for k in keys {
        let ctrl = k.mods.ctrl || k.mods.cmd;
        match k.key {
            Key::Delete if loaded && cx.state.renaming.is_none() => cx.remove(slot),
            Key::Char('d' | 'D') if ctrl && loaded => cx.duplicate(slot),
            Key::Char(' ') if free && loaded && !k.mods.shift => cx.p.shared.audition(None),
            _ => {}
        }
    }
}

/// The handle between the browser and the rest: drag to resize, double-click
/// to restore the width.
fn splitter(ui: &mut Ui, state: &mut EditorState, width: f64) -> El {
    let r = ui.get("splitter");
    if r.dragged {
        state.sidebar = (width + r.drag_delta.x).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
    }
    if r.double_clicked {
        state.sidebar = SIDEBAR;
    }
    let lift = ui.state("splitter").hover.max(if r.held { 1. } else { 0. }) as f32;
    canvas(move |s| {
        vec![Draw::fill(
            rect(0., 0., if lift > 0.5 { 2. } else { 1. }, s.height),
            Role::Ink.alpha(0.08 + 0.25 * lift),
        )]
    })
    .w(4)
    .h(Len::Pct(100.))
    .shrink(0)
    .cursor(Cursor::ResizeH)
    .tip("Drag to resize the browser, double-click to reset")
    .named("Resize browser")
    .id("splitter")
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
            .pad((TIGHT, SPACE))
            .max_size(Size::new(SIDEBAR, CONTROL * 2.))
            .fill(Role::Level(3))
            .stroke(accent())
            .stroke_width(1)
            .opacity(0.94)
            .at(at.x + INSET, at.y + SPACE),
    )
}

/// View tabs over the rack, or over the selected part's mapping or details.
fn main_view(ui: &mut Ui, cx: &mut Cx) -> El {
    let mut tabs = Vec::new();
    for (tab, label, id) in [
        (Tab::Rack, "Rack", "tab-rack"),
        (Tab::Mapping, "Mapping", "tab-mapping"),
        (Tab::Info, "Info", "tab-info"),
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
            .pad((0, INSET))
            .shrink(0)
            .fill(Role::Surface),
        rule(),
    ];
    if !cx.state.notice.is_empty() {
        content.push(banner(Role::Warning, cx.state.notice.clone()));
    }
    let slot = cx.state.selected;
    if cx.state.tab == Tab::Rack {
        content.push(rack::view(ui, cx));
    } else if cx.part().is_none() {
        content.push(instrument::welcome(cx));
    } else {
        // The selected part's header stays on top of its mapping and details.
        content.push(rack::header(ui, cx, slot));
        content.push(rule());
        content.extend(instrument::notices(cx, slot));
        content.push(match cx.state.tab {
            Tab::Mapping => instrument::mapping(ui, cx),
            _ => instrument::info(cx),
        });
    }
    col(content)
        .gap(0)
        .flex(1)
        .min_w(0)
        .min_h(0)
        .fill(Role::Background)
        .id("center")
}
