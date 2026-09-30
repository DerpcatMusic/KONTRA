//! The editor: a library browser, a rack of parts, the selected instrument's
//! performance view and a playable keyboard.
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
mod rack;
#[cfg(test)]
mod tests;
mod theme;

use crate::engine::RACK_SLOTS;
use crate::import;
use crate::plugin::{Load, Part, PartView, SamplerParams, Selection, View, rack_controls};
use moose::mui::{Bridge, MuiEditor, mui::prelude::*};
use moose::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use theme::*;

pub(crate) fn editor(params: Arc<SamplerParams>) -> Box<dyn Editor> {
    let build = build(&params);
    let drop_params = params.clone();
    let cancel_params = params.clone();
    let changed = outside_changes(params.clone());
    MuiEditor::new(params, theme::ui(), (1180, 760), build)
        .on_files(move |ui, at, paths, dropped| native_files(&drop_params, ui, at, paths, dropped))
        .on_cancel(move |_| cancel_params.shared.release_keyboard())
        .changed(changed)
        .fixed_zoom()
        .resizable((900, 600))
        .into_editor()
}

/// When to rebuild for state outside the editor's own input: every tick
/// while loading animates or the voice count moves, else at the loader's
/// 100 ms poll, the fastest the view it publishes can change. Rebuilding
/// every tick kept the UI thread at 15-30% of a core while idle.
fn outside_changes(p: Arc<SamplerParams>) -> impl FnMut() -> bool + Send + 'static {
    let (mut last, mut voices) = (Instant::now(), u64::MAX);
    move || {
        let now_voices = p.shared.voices.load(Ordering::Relaxed);
        let busy = {
            let view = p.shared.view.lock().unwrap();
            view.parts.iter().any(|v| v.loading) || view.multi_status.starts_with("Loading")
        };
        if busy || now_voices != voices || last.elapsed() >= Duration::from_millis(100) {
            (last, voices) = (Instant::now(), now_voices);
            return true;
        }
        false
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Perform,
    Mapping,
    Rack,
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
    /// First octave on the keyboard.
    octave: i16,
    /// The part the keyboard was last centered on.
    keyboard_for: Option<(String, u32)>,
    selected: usize,
    notice: String,
    root: String,
    last_poll: Instant,
    /// Smoothed audio thread load, 0..1.
    cpu: f32,
    /// Script control being dragged, with its unrounded value.
    held: Option<(usize, f64)>,
    /// Script menu showing its items.
    menu: Option<usize>,
    started: Instant,
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

    /// Show `slot` in the instrument view.
    fn show(&mut self, slot: usize) {
        self.state.selected = slot;
        self.state.tab = Tab::Perform;
        self.state.notice.clear();
    }

    /// Open a preset from the browser: a multi replaces the rack, an instrument
    /// already in the rack is shown, anything else takes a free slot.
    fn open(&mut self, path: &Path) {
        let text = path.to_string_lossy().into_owned();
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
        if import::is_multi(Path::new(&path)) {
            self.p.shared.queue_multi(path);
            return;
        }
        let part = &mut self.selection.parts[slot];
        part.path = path;
        part.program = 0;
        part.group = u32::MAX;
        self.show(slot);
    }
}

/// String slicing, not `Path::strip_prefix`: the browser asks for every
/// preset on each rebuild, and component parsing was most of an idle frame.
fn library_of(root: &str, path: &Path) -> String {
    path.to_str()
        .and_then(|p| p.strip_prefix(root.trim_end_matches('/')))
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(|rest| rest.split('/').find(|c| !c.is_empty()))
        .unwrap_or_default()
        .to_owned()
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
    let mut selection = p.selection.write().unwrap();
    let target = (0..selection.parts.len()).find(|n| {
        ui.scene()
            .and_then(|s| s.surface(&format!("part-{n}")))
            .is_some_and(|s| {
                let r = s.frame;
                at.x >= r.x
                    && at.x < r.x + r.size.width
                    && at.y >= r.y
                    && at.y < r.y + r.size.height
            })
    });
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
) -> impl FnMut(&mut Ui, &mut Bridge<SamplerParams>) -> El + Send + 'static {
    let mut root = params.selection.read().unwrap().root.clone();
    if root.is_empty() {
        root = import::LIBRARY_ROOT.into();
    }
    let mut state = EditorState {
        search: String::new(),
        library: None,
        multis: false,
        tab: Tab::Perform,
        settings: false,
        octave: 2,
        keyboard_for: None,
        selected: 0,
        notice: String::new(),
        root,
        last_poll: Instant::now() - Duration::from_secs(1),
        cpu: 0.,
        held: None,
        menu: None,
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
        let view = p.shared.view.lock().unwrap().clone();
        let mut selection = p.selection.read().unwrap().clone();
        let before = selection.clone();
        sanitize(&mut selection);
        let focus = p.shared.focus_request.swap(128, Ordering::Relaxed);
        if focus < RACK_SLOTS as u64 {
            state.selected = focus as usize;
            state.tab = Tab::Perform;
            state.notice.clear();
        }
        state.selected = state.selected.min(selection.parts.len().saturating_sub(1));
        p.shared
            .selected
            .store(state.selected as u64, Ordering::Relaxed);

        let mut cx = Cx {
            p: &p,
            view: &view,
            selection,
            state: &mut state,
        };
        let top = header::top_bar(ui, &mut cx, bridge);
        let settings = cx.state.settings.then(|| header::settings(ui, &mut cx));
        let sidebar = browser::sidebar(ui, &mut cx);
        let main = main_view(ui, &mut cx);
        let keys = keyboard::strip(ui, &mut cx);

        let Cx { selection, .. } = cx;
        if selection != before {
            let mut current = p.selection.write().unwrap();
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
        shell.push(row![sidebar, vrule(), main].flex(1).min_h(0));
        shell.push(rule());
        shell.push(keys);
        col(shell)
            .gap(0)
            .full()
            .fill(Role::Background)
            .radius(0)
            .clip()
    }
}

/// Rack strip, instrument header, view tabs, notices and the current view.
fn main_view(ui: &mut Ui, cx: &mut Cx) -> El {
    let strip = rack::strip(ui, cx);
    let empty = cx.selection.parts.iter().all(|p| p.path.is_empty());
    let mut content = vec![strip, rule()];
    if empty && cx.state.tab != Tab::Rack {
        content.push(instrument::welcome(cx));
    } else {
        content.push(instrument::header(ui, cx));
        let mut tabs = Vec::new();
        for (tab, label, id) in [
            (Tab::Perform, "Perform", "tab-perform"),
            (Tab::Mapping, "Mapping", "tab-mapping"),
            (Tab::Rack, "Rack", "tab-rack"),
            (Tab::Info, "Info", "tab-info"),
        ] {
            let (hit, el) = theme::tab(ui, id, label, cx.state.tab == tab);
            if hit {
                cx.state.tab = tab;
            }
            tabs.push(el);
        }
        content.push(row(tabs).gap(HALF).pad((WIDE - GAP, 0)).shrink(0));
        content.push(rule());
        content.extend(instrument::notices(cx));
        content.push(match cx.state.tab {
            Tab::Perform => instrument::perform(ui, cx),
            Tab::Mapping => instrument::mapping(ui, cx),
            Tab::Rack => rack::mixer(ui, cx),
            Tab::Info => instrument::info(cx),
        });
    }
    col(content)
        .gap(0)
        .flex(1)
        .min_w(0)
        .min_h(0)
        .fill(Role::Background)
}
