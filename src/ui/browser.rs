//! The browser, split like Bitwig's: the libraries above, with Favorites and
//! Recent over them, and the chosen one's presets below, in its folders.
//!
//! The libraries list in the order the player picks (dragged by hand, by
//! name, by last use or by vendor; pinned ones first), and a filter over
//! them (Ctrl+F or /) narrows them by name or vendor: the arrows walk the
//! matches, Enter opens one, Esc clears. A library shows its folders as on
//! disk, each folding open and shut and remembered; the search lists flat
//! matches instead, each with its folder under it. Click selects, Enter
//! loads into the selected part, Shift+Enter into a new one, double-click
//! loads, drag drops onto the rack, right-click offers the rest; the arrows
//! walk each pane (Left and Right fold), and Tab crosses between them. The
//! divider and the browser's edge both drag. The library and row last
//! shown come back with the editor.

use super::{Cx, RackDrag, menu, theme::*};
use crate::library as import;
use crate::library::{Folder, Library};
use moose::mui::mui::prelude::*;
use moose::mui::mui::scene::Fit;
use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[path = "browser_geometry.rs"]
mod geometry;

/// What the lower pane lists.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Source {
    Favorites,
    Recent,
    Library(String),
}

/// The upper pane's share of the browser's height.
pub const SPLIT: f64 = 0.55;
pub const SPLIT_MIN: f64 = 0.14;
pub const SPLIT_MAX: f64 = 0.7;

/// A row of the lower pane, and one with its folder under it. Every row is
/// one of these heights: the list builds only the rows in view.
const ROW: f64 = TEXT * 2. + 2.;
const ROW2: f64 = TEXT * 3. + 4.;
/// A library's row in the upper pane, and the rule under Recent.
const SOURCE_ROW: f64 = THUMB.1 + 2. * TIGHT;
const SOURCE_RULE: f64 = 2. * INSET + 1.;
/// How far a folder level steps in.
const INDENT: f64 = TEXT;

/// The browser's own state between frames.
#[derive(Default)]
pub struct Browse {
    positions: [Position; 2],
    /// The quick filter over the libraries, and as it was last drawn.
    pub filter: String,
    filtered: String,
    /// How far each pane is slid.
    sources_y: f64,
    list_y: f64,
    /// What the lower pane lists, hashed: it starts at the top when this changes.
    listing: u64,
    /// Bring the chosen library, and the cursor's row, into view.
    reveal_source: u8,
    reveal_row: bool,
    /// The library and row last shown were put back.
    restored: bool,
    /// The lower pane's rows, and what they were made from, hashed.
    rows: (Option<u64>, Arc<Listed>),
    /// The browser's field holding the focus as the last frame ended: Esc
    /// takes the focus before the frame, so the field is told it this way.
    typing: Option<&'static str>,
    /// Ctrl+F opened the browser: the filter takes the focus once it is drawn.
    pub find: bool,
    /// A library display name being typed, keyed by its stable folder.
    pub renaming: Option<(String, String)>,
}

#[derive(Default)]
struct Position {
    source: Option<Source>,
    cursor: Option<String>,
    search: String,
    filter: String,
    sources_y: f64,
    list_y: f64,
    listing: u64,
}

fn is_uvi(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("uvip") || e.eq_ignore_ascii_case("ufs"))
        || path.components().any(|c| Path::new(c.as_os_str()).extension().is_some_and(|e| e.eq_ignore_ascii_case("ufs")))
}

impl Browse {
    /// Esc this frame was meant for one of the browser's fields.
    pub fn typing(&self) -> bool {
        self.typing.is_some() || self.renaming.is_some()
    }
}

/// A row of the lower pane: a folder of the chosen library, or a preset.
#[derive(Clone, Debug)]
enum Row {
    Folder { path: String, name: String, depth: usize, count: usize, open: bool },
    /// `under` is its folder, shown under its name in a flat list.
    Preset { path: PathBuf, depth: usize, under: String, index: usize },
}

impl Row {
    /// What the cursor holds when on this row.
    fn key(&self) -> String {
        match self {
            Row::Folder { path, .. } => path.clone(),
            Row::Preset { path, .. } => path.to_string_lossy().into_owned(),
        }
    }

    fn depth(&self) -> usize {
        match self {
            Row::Folder { depth, .. } | Row::Preset { depth, .. } => *depth,
        }
    }

    fn height(&self) -> f64 {
        match self {
            Row::Preset { under, .. } if !under.is_empty() => ROW2,
            _ => ROW,
        }
    }

    fn id(&self, n: usize) -> String {
        match self {
            Row::Folder { .. } => format!("folder-{n}"),
            Row::Preset { index, .. } => format!("instrument-{index}"),
        }
    }
}

/// Rows and their scroll/cursor metadata, rebuilt together when the list changes.
#[derive(Default)]
struct Listed {
    rows: Vec<Row>,
    tops: Vec<f64>,
    indices: HashMap<String, usize>,
    presets: usize,
}

impl Listed {
    fn new(mut rows: Vec<Row>) -> Self {
        let mut tops = Vec::with_capacity(rows.len() + 1);
        let mut indices = HashMap::with_capacity(rows.len());
        let (mut y, mut presets) = (0., 0);
        for (n, row) in rows.iter_mut().enumerate() {
            tops.push(y);
            y += row.height();
            indices.entry(row.key()).or_insert(n);
            if let Row::Preset { index, .. } = row { *index = presets; presets += 1; }
        }
        tops.push(y);
        Self { rows, tops, indices, presets }
    }
}

/// A library being dragged to a new place in the list, by folder.
#[derive(Clone)]
struct LibraryDrag(String);

pub fn sidebar(ui: &mut Ui, cx: &mut Cx) -> El {
    let catalog = cx.view.shelf.clone();
    let presets = cx.view.files.clone();
    let settings = cx.settings.clone();
    if !cx.state.browse.restored && let Some(library) = catalog.libraries.iter().find(|l| l.dir == Path::new(&settings.last_library)) {
        cx.state.uvi = presets.iter().find(|p| p.starts_with(&library.dir)).is_some_and(|p| is_uvi(p));
    }
    let uvi = cx.state.uvi;
    // Which library each file is in, worked out once per scan.
    let (files, shelf, kind, grouped) = &mut cx.state.libraries;
    let scanned = Arc::as_ptr(&catalog) as usize;
    if !files.upgrade().is_some_and(|f| Arc::ptr_eq(&f, &presets)) || *shelf != scanned || *kind != uvi {
        let mut by: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (n, file) in presets.iter().enumerate().filter(|(_, f)| import::is_multi(f) || is_uvi(f) == uvi) {
            if let Some(library) = catalog.of(file) {
                by.entry(library.name.clone()).or_default().push(n);
            }
        }
        (*files, *shelf, *kind, *grouped) = (Arc::downgrade(&presets), scanned, uvi, Arc::new(by));
    }
    let grouped = grouped.clone();
    // The libraries as listed: pinned ones, then by the sort chosen.
    let arranged: Vec<&Library> = settings.arrange(grouped.keys().filter_map(|name| catalog.named(name)));
    let dirs: Vec<String> = arranged.iter().map(|l| l.dir.to_string_lossy().into_owned()).collect();
    // Favorites and recents of the kind picked.
    let kind = |paths: &[String]| -> Vec<PathBuf> {
        paths
            .iter()
            .map(PathBuf::from)
            .filter(|p| import::is_multi(p) || is_uvi(p) == uvi)
            .collect()
    };
    let (favorites, recent) = (kind(&cx.selection.favorites), kind(&cx.selection.recent));
    // How far each library's loading parts are, averaged.
    let mut loading: BTreeMap<String, (f64, usize)> = BTreeMap::new();
    for (slot, part) in cx.selection.parts.iter().enumerate() {
        if let Some(done) = super::rack::loading(cx, slot) {
            let at = loading.entry(cx.library_of(Path::new(&part.path))).or_default();
            (at.0, at.1) = (at.0 + done, at.1 + 1);
        }
    }
    // Back where the player left off, once the libraries are in.
    if !cx.state.browse.restored && !arranged.is_empty() {
        cx.state.browse.restored = true;
        let at = dirs.iter().position(|d| *d == settings.last_library);
        if let (None, Some(at)) = (&cx.state.source, at) {
            cx.state.source = Some(Source::Library(arranged[at].name.clone()));
            cx.state.cursor = (!settings.last_row.is_empty()).then(|| settings.last_row.clone());
            (cx.state.browse.reveal_source, cx.state.browse.reveal_row) = (2, true);
        }
    }

    let (hide, hide_el) = icon_button(ui, "browser-hide", Icon::Left, "Hide the browser", false);
    if hide {
        cx.state.browser = false;
    }
    let (add, add_el) = icon_button(ui, "libraries-add", Icon::Plus, "Add libraries, manage their folders", false);
    if add {
        menu::open_under(ui, cx, menu::Target::Libraries, "libraries-add");
    }
    let mut kinds = Vec::new();
    for (multi, label, id) in [
        (false, "Kontakt", "bank-kontakt"),
        (true, "Falcon / UVI", "bank-uvi"),
    ] {
        let (hit, el) = tab(ui, id, label, uvi == multi);
        if hit {
            if cx.state.uvi != multi {
                let browse = &mut cx.state.browse;
                browse.positions[uvi as usize] = Position {
                    source: cx.state.source.take(), cursor: cx.state.cursor.take(), search: std::mem::take(&mut cx.state.search),
                    filter: std::mem::take(&mut browse.filter),
                    sources_y: browse.sources_y, list_y: browse.list_y, listing: browse.listing,
                };
                let position = std::mem::take(&mut browse.positions[multi as usize]);
                cx.state.source = position.source;
                cx.state.cursor = position.cursor;
                cx.state.search = position.search;
                (browse.sources_y, browse.list_y, browse.listing) = (position.sources_y, position.list_y, position.listing);
                browse.filter = position.filter;
                browse.filtered = browse.filter.clone();
                browse.restored = true;
                browse.reveal_source = 0;
                cx.state.uvi = multi;
                return sidebar(ui, cx);
            }
        }
        kinds.push(el);
    }

    // Where the focus goes, moved once every row has read this frame's keys
    // with the focus where it was: a key acts once.
    let mut focus_to: Option<String> = None;
    // Ctrl+F or / goes to the library filter; Ctrl+F there goes on to the
    // preset search, and back.
    let ctrl_f = |k: &KeyPress| matches!(k.key, Key::Char('f' | 'F')) && (k.mods.ctrl || k.mods.cmd);
    let slash = |k: &KeyPress| k.key == Key::Char('/') && !(k.mods.ctrl || k.mods.cmd);
    if std::mem::take(&mut cx.state.browse.find)
        || ui.shortcuts().iter().any(|k| ctrl_f(k) || slash(k))
        || ui.keys("search").iter().any(ctrl_f)
    {
        focus_to = Some("library-filter".into());
    } else if ui.keys("library-filter").iter().any(ctrl_f) {
        focus_to = Some("search".into());
    }
    // Esc in a field clears it and keeps it focused; in an empty one it lets go.
    let escaped = (cx.state.browse.typing.take()).filter(|_| ui.shortcuts().iter().any(|k| k.key == Key::Escape));
    for (id, text) in [("library-filter", &mut cx.state.browse.filter), ("search", &mut cx.state.search)] {
        if escaped == Some(id) && !text.is_empty() {
            text.clear();
            focus_to = Some(id.into());
        }
    }
    let filter_keys = ui.keys("library-filter").to_vec();
    let filter = search_field(ui, "library-filter", &mut cx.state.browse.filter, "Filter libraries", "Filter libraries by name or vendor");
    let (sort, sort_el) = dropdown(ui, "library-sort", settings.sort.label(), "Sort the libraries");
    if sort {
        menu::open_under(ui, cx, menu::Target::LibrarySort, "library-sort");
    }

    // The upper pane: the two pseudo-entries, then every library the filter
    // lets through.
    let words: Vec<String> = cx.state.browse.filter.to_lowercase().split_whitespace().map(str::to_owned).collect();
    let mut sources = Vec::new();
    if words.is_empty() {
        sources.push(("source-favorites".to_owned(), Source::Favorites));
        sources.push(("source-recent".to_owned(), Source::Recent));
    }
    for (n, library) in arranged.iter().enumerate() {
        let hay = format!("{} {} {}", settings.library_name(library), library.name, library.vendor).to_lowercase();
        if words.iter().all(|w| hay.contains(w.as_str())) {
            sources.push((format!("library-{n}"), Source::Library(library.name.clone())));
        }
    }
    // Which folder each row shows, for a picture dropped on one.
    *super::lock(&cx.state.picker.rows) = arranged.iter().map(|l| Some(l.dir.clone())).collect();
    // A new filter picks its first match, unless the chosen one still matches.
    let chosen_listed = |cx: &Cx| sources.iter().any(|(_, s)| Some(s) == cx.state.source.as_ref());
    if cx.state.browse.filter != cx.state.browse.filtered {
        cx.state.browse.filtered = cx.state.browse.filter.clone();
        if !words.is_empty() && !chosen_listed(cx) && let Some((_, first)) = sources.first() {
            cx.state.source = Some(first.clone());
            cx.state.cursor = None;
        }
        cx.state.browse.reveal_source = 2;
    }
    if cx.state.source.as_ref().is_some_and(|s| matches!(s, Source::Library(name) if !grouped.contains_key(name))) {
        cx.state.source = None;
    }
    // Enter on an entry opens it and moves on to its presets.
    let mut enter = false;
    let step = |at: Option<usize>, key: Key, len: usize| match (key, at) {
        (Key::Down, None) => Some(0),
        (Key::Down, Some(n)) => Some((n + 1).min(len - 1)),
        (Key::Up, Some(n)) => Some(n.saturating_sub(1)),
        (Key::Up, None) => Some(len - 1),
        _ => None,
    };
    for k in &filter_keys {
        let at = sources.iter().position(|(_, s)| Some(s) == cx.state.source.as_ref());
        if let Some(next) = step(at, k.key, sources.len()).filter(|_| !sources.is_empty()) {
            cx.state.source = Some(sources[next].1.clone());
            cx.state.cursor = None;
            cx.state.browse.reveal_source = 2;
        }
        if k.key == Key::Enter && at.is_some() {
            enter = true;
            cx.state.browse.reveal_source = 2;
            keep_place(cx);
        }
    }
    let mut rows = Vec::new();
    // Where the chosen entry sits in the pane, to bring it into view.
    let (mut top, mut chosen_at) = (TIGHT, None);
    let pinned = |dir: &str| settings.pinned.iter().any(|p| p == dir);
    let dragging = ui.dragging::<LibraryDrag>().map(|d| d.0.clone());
    for (n, (id, source)) in sources.iter().enumerate() {
        let r = ui.get(id.as_str());
        let editing = matches!(source, Source::Library(name) if catalog.named(name).is_some_and(|l| cx.state.browse.renaming.as_ref().is_some_and(|(dir, _)| Path::new(dir) == l.dir)));
        if !editing && r.clicked_with(Button::Primary) {
            // A second click lets the library go: the search spans them all.
            cx.state.source = (cx.state.source.as_ref() != Some(source)).then(|| source.clone());
            cx.state.cursor = None;
            cx.state.browse.reveal_source = 2;
            keep_place(cx);
        }
        if !editing && r.key_activated {
            cx.state.source = Some(source.clone());
            cx.state.cursor = None;
            enter = true;
            cx.state.browse.reveal_source = 2;
            keep_place(cx);
        }
        if let (true, Source::Library(name)) = (r.clicked_with(Button::Secondary), source) {
            menu::open(ui, cx, menu::Target::Library(name.clone()));
        }
        if ui.focused(id.as_str()) {
            let walked = ui.shortcuts().iter().fold(None, |at, k| step(at.or(Some(n)), k.key, sources.len()).or(at));
            if let Some(next) = walked.filter(|&next| next != n) {
                cx.state.source = Some(sources[next].1.clone());
                cx.state.cursor = None;
                cx.state.browse.reveal_source = 2;
                focus_to = Some(sources[next].0.clone());
            }
        }
        // A library dragged onto another goes before it.
        let dir = match source {
            Source::Library(name) => catalog.named(name).map(|l| l.dir.to_string_lossy().into_owned()),
            _ => None,
        };
        let mut over = false;
        if let Some(dir) = &dir {
            if !editing && r.dragged && r.button == Some(Button::Primary) {
                ui.start_drag(id.as_str(), LibraryDrag(dir.clone()));
            }
            if let Some(LibraryDrag(from)) = ui.dropped_on::<LibraryDrag>(id.as_str()) {
                cx.p.shared.libraries.edit(|s| s.reorder(&dirs, &from, Some(dir)));
            }
            over = r.drop_target && dragging.as_ref().is_some_and(|from| from != dir);
        }
        let (label, count, thumb, height, card) = match source {
            Source::Favorites => ("Favorites".to_owned(), favorites.len(), symbol(Icon::Star), SOURCE_ROW, false),
            Source::Recent => ("Recent".to_owned(), recent.len(), symbol(Icon::Recent), SOURCE_ROW, false),
            Source::Library(name) => {
                // Own panels are already decoded by the import worker.
                let image = catalog.named(name).filter(|l| !settings.covers.contains_key(l.dir.to_string_lossy().as_ref()))
                    .and_then(|_| cx.view.artwork.get(name)).cloned()
                    .or_else(|| cx.looks(name).and_then(|l| l.thumb.clone()));
                let width = (cx.state.sidebar - 2. * TIGHT).max(1.);
                let height = image.as_ref().map_or(width / 4., |i| width * f64::from(i.height) / f64::from(i.width));
                let art = match image {
                    Some(image) => block(Len::Pct(100.), height).fill(Fill::Image(image, Fit::Contain)),
                    None => stack![glyph(Icon::Sidebar, TEXT, secondary()).centered()].w(Len::Pct(100.)).h(height).fill(Role::Raised),
                }.id(format!("{id}-art")).disabled().shrink(0);
                (catalog.named(name).map_or_else(|| library_label(name), |l| settings.library_name(l)),
                    grouped[name].len(), art, height + SOURCE_ROW + 2. * TIGHT, true)
            },
        };
        let chosen = cx.state.source.as_ref() == Some(source);
        if chosen {
            chosen_at = Some((top, top + height));
        }
        let progress = match source {
            Source::Library(name) => loading.get(name).map(|(sum, n)| sum / *n as f64),
            _ => None,
        };
        let about = match source {
            Source::Library(name) => catalog.named(name).map(|l| about(l, &label, dir.as_deref().is_some_and(pinned))),
            _ => None,
        };
        let edit = dir.as_deref().and_then(|dir| library_name(ui, cx, dir));
        let el = source_row(id, label, count, thumb, height, card, chosen, progress, about, edit);
        rows.push(if over {
            stack![el, block(Len::Pct(100.), 2).fill(accent()).anchor(Align::Start, Align::Start)].shrink(0)
        } else {
            el
        });
        top += height;
        // A rule under Recent, and under the pinned libraries.
        let last_pinned = dir.as_deref().is_some_and(pinned)
            && sources.get(n + 1).is_some_and(|(_, s)| match s {
                Source::Library(name) => !catalog.named(name).is_some_and(|l| pinned(&l.dir.to_string_lossy())),
                _ => false,
            });
        if source == &Source::Recent || last_pinned {
            rows.push(col![rule()].pad((TIGHT, INSET)).shrink(0));
            top += SOURCE_RULE;
        }
    }
    // Dropped past the last library, it goes to the end.
    if dragging.is_some() && !arranged.is_empty() {
        if let Some(LibraryDrag(from)) = ui.dropped_on::<LibraryDrag>("libraries-end") {
            cx.p.shared.libraries.edit(|s| s.reorder(&dirs, &from, None));
        }
        let over = ui.get("libraries-end").drop_target;
        rows.push(
            col![block(Len::Pct(100.), 2).fill(if over { Fill::from(accent()) } else { Role::Ink.alpha(0.) })]
                .h(CONTROL)
                .shrink(0)
                .id("libraries-end"),
        );
        top += CONTROL;
    }
    let scanning = cx.p.shared.libraries.scanning();
    let bank_problem = uvi && !catalog.bank_issues.is_empty();
    if bank_problem {
        let count: usize = catalog.bank_issues.iter().map(|i| i.locations.len()).sum();
        let unsupported: usize = catalog.bank_issues.iter().filter(|i| i.unsupported).map(|i| i.locations.len()).sum();
        let mut message = if unsupported > 0 { format!("Protected library: not supported ({unsupported} banks). ") } else { String::new() };
        if count > unsupported { message.push_str(&format!("{} UVI banks could not be read. ", count - unsupported)); }
        message.push_str("See Logs for the affected locations.");
        rows.insert(0, hint(&message).id("uvi-bank-problem"));
    }
    if !bank_problem && arranged.is_empty() && scanning.is_none() {
        if presets.is_empty() {
            // First, so its buttons are in view above favorites and recent.
            rows.insert(0, empty_state(ui, cx));
        } else {
            rows.push(hint(if uvi { "No Falcon / UVI libraries." } else { "No Kontakt libraries." }));
        }
    } else if !bank_problem && sources.is_empty() {
        rows.push(hint("No library matches that filter."));
    }
    for (n, root) in settings.roots.iter().enumerate().rev() {
        if let Some(problem) = catalog.root_problem(Path::new(&root.path)) {
            let message = format!("{}\n{problem}", root.path);
            rows.insert(usize::from(bank_problem), hint(&message).tip(message.clone()).id(format!("uvi-bank-root-{n}")));
        }
    }
    let reveal = chosen_at.filter(|_| cx.state.browse.reveal_source > 0);
    let sources_id = format!("browser-sources-{uvi}");
    // The rows are summed above; the empty state and hints are measured.
    let measured = ui.scene().and_then(|s| s.surface("browser-sources-content")).map_or(0., |s| s.frame.size.height);
    let (sources_y, sources_bar, revealed) =
        slide(ui, &sources_id, &mut cx.state.browse.sources_y, measured.max(top + TIGHT), reveal, false);
    // Selection changes the lower header: reveal again against its settled viewport.
    if chosen_at.is_none() { cx.state.browse.reveal_source = 0; }
    else if revealed { cx.state.browse.reveal_source = cx.state.browse.reveal_source.saturating_sub(1); }

    // The lower pane: the chosen source's presets, the search filtering them.
    let search = search_field(
        ui,
        "search",
        &mut cx.state.search,
        "Search presets",
        "Search presets",
    );
    let needle = cx.state.search.to_lowercase();
    let made_of = {
        let mut h = DefaultHasher::new();
        (Arc::as_ptr(&presets) as usize, presets.len(), Arc::as_ptr(&catalog) as usize, uvi).hash(&mut h);
        (&cx.state.source, &needle, &favorites, &recent, &dirs, &settings.folders, &settings.names).hash(&mut h);
        h.finish()
    };
    if cx.state.browse.rows.0 != Some(made_of) {
        let rows = list(cx, &arranged, &grouped, &favorites, &recent, &needle);
        cx.state.browse.rows = (Some(made_of), Arc::new(Listed::new(rows)));
    }
    let listed = cx.state.browse.rows.1.clone();
    let listed_rows = &listed.rows;
    let listing = {
        let mut h = DefaultHasher::new();
        (&cx.state.source, &needle, uvi).hash(&mut h);
        h.finish()
    };
    let fresh = cx.state.browse.listing != listing;
    if fresh {
        cx.state.browse.listing = listing;
        cx.state.browse.list_y = 0.;
    }
    let empty = match &cx.state.source {
        Some(Source::Favorites) => "Star a preset to keep it here.",
        Some(Source::Recent) => "Presets you open show up here.",
        Some(Source::Library(_)) => "Nothing here matches that search.",
        None if !needle.is_empty() => "Nothing matches that search.",
        None => "Choose a library above, or search them all.",
    };
    let list_id = if uvi { "browser-list-uvi" } else { "browser-list" };
    let view_h = ui.scene().and_then(|s| s.surface(list_id)).map_or(0., |s| s.frame.size.height);
    walk(ui, cx, &listed, ((view_h / ROW) as usize).max(1), &mut focus_to);
    // Into the presets, at the cursor (the first row when none), brought into view.
    let into_presets = |cx: &mut Cx| {
        let n = cursor_at(cx, &listed).unwrap_or(0);
        let Some(row) = listed_rows.get(n) else { return "search".to_owned() };
        cx.state.cursor = Some(row.key());
        cx.state.browse.reveal_row = true;
        row.id(n)
    };
    if enter {
        focus_to = Some(into_presets(cx));
    }
    // Tab crosses the panes: into the presets at the cursor, back to the
    // chosen entry. The Ui has moved the focus on by now; this overrides it.
    let tabbed = ui
        .focus_key()
        .is_some_and(|k| ui.keys(k.to_owned()).iter().any(|k| k.key == Key::Tab));
    if tabbed {
        match cx.state.pane {
            Some(Pane::Sources) => focus_to = Some(into_presets(cx)),
            Some(Pane::Presets) => {
                let chosen = sources.iter().find(|(_, s)| Some(s) == cx.state.source.as_ref());
                if let Some((id, _)) = chosen.or(sources.first()) {
                    focus_to = Some(id.clone());
                }
            }
            None => {}
        }
    }
    let mut above = Vec::new();
    if let Some(Source::Library(name)) = &cx.state.source
        && let Some(library) = catalog.named(name)
    {
        let size = cx.p.shared.libraries.size(&library.dir);
        if size.is_none() && ui.get("library-size").activated() { cx.p.shared.libraries.measure_size(&library.dir); }
        above.push(library_heading(library, settings.library_name(library), size));
        if needle.is_empty() {
            above.extend(crumbs(ui, cx, library, &listed));
        }
    }
    if listed_rows.is_empty() && !arranged.is_empty() {
        above.push(hint(empty));
    }
    // The row offsets were measured when these rows were built.
    let tops = &listed.tops;
    let y = tops[listed_rows.len()];
    let at = cursor_at(cx, &listed);
    let reveal = at.filter(|_| cx.state.browse.reveal_row).map(|n| (tops[n], tops[n + 1]));
    let (list_y, list_bar, revealed) = slide(ui, list_id, &mut cx.state.browse.list_y, y, reveal, fresh);
    if revealed || at.is_none() {
        cx.state.browse.reveal_row = false;
    }
    // Only the rows in view are built; spacers stand in for the rest.
    let shown = if view_h > 0. { view_h } else { 2000. };
    let first = tops.partition_point(|&t| t <= list_y).saturating_sub(1);
    let last = tops[..listed_rows.len()].partition_point(|&t| t < list_y + shown);
    let mut items = vec![block(1, tops[first]).shrink(0)];
    for n in first..last.max(first) {
        items.push(match &listed_rows[n] {
            Row::Folder { .. } => folder(ui, cx, n, &listed_rows[n]),
            Row::Preset { path, depth, under, index } => preset(ui, cx, *index, path, *depth, under),
        });
    }
    items.push(block(1, y - tops[last.max(first)]).shrink(0));

    if let Some(id) = focus_to {
        ui.focus(id);
    }
    cx.state.browse.typing = ["library-filter", "search"].into_iter().find(|id| ui.focused(*id));
    cx.state.pane = ui.focus_key().and_then(pane_of);
    // How many presets are listed, folded away or not; all of them before
    // a library is chosen.
    let count = match &cx.state.source {
        Some(Source::Library(name)) if needle.is_empty() => grouped.get(name).map_or(0, Vec::len),
        None if needle.is_empty() => grouped.values().map(Vec::len).sum(),
        _ => listed.presets,
    };
    let counted = caption(count.to_string())
        .text_size(SMALL)
        .fill(secondary())
        .tip(format!("{count} presets"))
        .id("browser-count");
    let scan_line = scan_line(ui, cx, scanning);
    let split = split_divider(ui, cx);
    let list = col(items)
        .gap(0)
        .align(Align::Stretch)
        .w(Len::Pct(100.))
        .h(Len::Pct(100.))
        .scroll()
        .no_scrollbar()
        .scrolled(0., list_y)
        .id(list_id);
    let sources_list = col![col(rows).gap(0).align(Align::Stretch).pad((0, TIGHT)).shrink(0).id("browser-sources-content")]
        .gap(0)
        .align(Align::Stretch)
        .w(Len::Pct(100.))
        .h(Len::Pct(100.))
        .scroll()
        .no_scrollbar()
        .scrolled(0., sources_y)
        .id(sources_id);
    let pane = |el: El, bar: Option<El>| {
        let mut layers = vec![el];
        layers.extend(bar.map(|b| b.anchor(Align::End, Align::Start)));
        stack(layers).w(Len::Pct(100.))
    };
    // Fixed chrome takes its own rows; the lists share the remaining height
    // without squeezing either viewport below useful rows.
    grid![1;
        section_bar(
            "Browser",
            vec![counted, add_el, hide_el]
        ),
        row(kinds).gap(INSET + TIGHT).pad(edges(0., INSET, 0., INSET)).shrink(0),
        scan_line,
        row![filter.flex(1).min_w(0), sort_el.shrink(0)]
            .gap(TIGHT)
            .align(Align::Center)
            .pad(edges(SPACE, INSET, TIGHT, INSET))
            .shrink(0),
        rule(),
        pane(sources_list, sources_bar).h(Len::Pct(100.)).min_h(0),
        split,
        col![search.w(Len::Pct(100.))].pad(edges(SPACE, INSET, SPACE, INSET)).shrink(0),
        col(above).gap(0).align(Align::Stretch).shrink(0),
        pane(list, list_bar).h(Len::Pct(100.)).min_h(0),
    ]
    .grid_tracks([GridTrack::MinFr { min: 0., fr: 1. }])
    .grid_rows([
        GridTrack::MaxContent, GridTrack::MaxContent, GridTrack::MaxContent,
        GridTrack::MaxContent, GridTrack::MaxContent,
        GridTrack::MinFr { min: SOURCE_ROW, fr: cx.state.split * 100. },
        GridTrack::MaxContent, GridTrack::MaxContent, GridTrack::MaxContent,
        GridTrack::MinFr { min: ROW2 * 2., fr: (1. - cx.state.split) * 100. },
    ])
    .align(Align::Stretch)
    .gap(0)
    .h(Len::Pct(100.))
    .shrink(0)
    .fill(Role::Surface)
    .clip()
    .id("browser")
}

/// The lower pane's rows: the chosen library's folders as on disk, or flat
/// matches of the search with their folders under them.
fn list(
    cx: &Cx,
    arranged: &[&Library],
    grouped: &super::Libraries,
    favorites: &[PathBuf],
    recent: &[PathBuf],
    needle: &str,
) -> Vec<Row> {
    let view = &cx.view;
    let words: Vec<&str> = needle.split_whitespace().collect();
    // A preset's library and its folders inside it.
    let place = |path: &Path| -> (String, String) {
        match view.shelf.of(path) {
            Some(l) => (cx.settings.library_name(l), folders(&l.dir, path)),
            None => (String::new(), path.parent().map(stem).unwrap_or_default()),
        }
    };
    // Its name or its folders hold every word.
    let hit = |path: &Path, folders: &str| {
        words.is_empty() || {
            let hay = format!("{} {folders}", stem(path)).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        }
    };
    let flat = |path: &Path, with_library: bool| -> Option<Row> {
        let (library, folders) = place(path);
        hit(path, &folders).then(|| {
            let under = match (with_library, library.is_empty(), folders.is_empty()) {
                (true, false, false) => format!("{library} / {folders}"),
                (true, false, true) => library,
                _ => folders,
            };
            Row::Preset { path: path.to_path_buf(), depth: 0, under, index: 0 }
        })
    };
    let files = |name: &str| -> Vec<&Path> {
        let mut paths: Vec<_> = grouped.get(name).into_iter().flatten().map(|&n| view.files[n].as_path()).collect();
        if let Some(library) = view.shelf.named(name) {
            paths.extend(view.shelf.snapshots.iter().filter(|(base, _)| view.shelf.of(base).is_some_and(|l| l.dir == library.dir))
                .flat_map(|(_, snapshots)| snapshots.paths.iter().map(PathBuf::as_path)));
        }
        paths.sort_by_cached_key(|p| import::natural(&p.to_string_lossy()));
        paths.dedup();
        paths
    };
    match &cx.state.source {
        Some(Source::Favorites) => favorites.iter().filter_map(|p| flat(p, true)).collect(),
        Some(Source::Recent) => recent.iter().filter_map(|p| flat(p, true)).collect(),
        Some(Source::Library(name)) if !words.is_empty() => files(name).into_iter().filter_map(|p| flat(p, false)).collect(),
        Some(Source::Library(name)) => {
            let Some(library) = view.shelf.named(name) else { return Vec::new() };
            let paths = files(name);
            categories(library, &paths, &cx.settings.folders)
        }
        None if !words.is_empty() => arranged
            .iter()
            .flat_map(|l| files(&l.name))
            .filter_map(|p| flat(p, true))
            .collect(),
        None => Vec::new(),
    }
}

fn categories(library: &Library, paths: &[&Path], open: &BTreeMap<String, bool>) -> Vec<Row> {
    let mut out = Vec::new();
    for name in ["Instruments", "Multis", "Presets", "Snapshots"] {
        let mut selected: Vec<&Path> = paths.iter().copied().filter(|p| {
            let category = if import::is_multi(p) || p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkm")) { "Multis" }
                else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nksn")) { "Snapshots" }
                else if is_uvi(p) { "Presets" } else { "Instruments" };
            category == name
        }).collect();
        if selected.is_empty() { continue; }
        selected.sort_by_cached_key(|p| import::natural(&p.to_string_lossy()));
        let mut root = library.dir.clone();
        // Bank members keep their source identity while hiding the container wrapper.
        if name == "Presets" && let Some(first) = selected[0].strip_prefix(&root).ok().and_then(|p| p.components().next()) {
            let bank = root.join(first.as_os_str());
            if bank.extension().is_some_and(|e| e.eq_ignore_ascii_case("ufs")) && selected.iter().all(|p| p.starts_with(&bank)) { root = bank; }
        }
        let category = root.join(name);
        if selected.iter().all(|p| p.starts_with(&category)) { root = category; }
        let key = library.dir.join(name).to_string_lossy().into_owned();
        let opened = open.get(&key).copied().unwrap_or(name == "Instruments" || name == "Presets");
        out.push(Row::Folder { path: key, name: name.into(), depth: 0, count: selected.len(), open: opened });
        if opened { flatten(&Folder::tree(&root, selected.iter().copied()), 1, open, &selected, &mut out); }
    }
    out
}

/// `folder`'s rows: its folders, each followed by its own rows when open,
/// then its presets. A folder that is all its parent holds starts open.
fn flatten(folder: &Folder, depth: usize, open: &BTreeMap<String, bool>, paths: &[&Path], out: &mut Vec<Row>) {
    let only = folder.folders.len() == 1 && folder.presets.is_empty();
    for sub in &folder.folders {
        if folder.name.to_ascii_lowercase().ends_with(".ufs") && sub.name.eq_ignore_ascii_case("Presets") {
            flatten(sub, depth, open, paths, out);
            continue;
        }
        let is_open = open.get(&sub.path).copied().unwrap_or(only);
        out.push(Row::Folder {
            path: sub.path.clone(),
            name: sub.name.strip_suffix(".ufs").unwrap_or(&sub.name).to_owned(),
            depth,
            count: sub.count,
            open: is_open,
        });
        if is_open {
            flatten(sub, depth + 1, open, paths, out);
        }
    }
    for &n in &folder.presets {
        out.push(Row::Preset { path: paths[n].to_path_buf(), depth, under: String::new(), index: 0 });
    }
}

/// The folders between a library and a preset: "Instruments / Legato".
fn folders(library: &Path, path: &Path) -> String {
    let folder = path.parent().and_then(|p| p.strip_prefix(library).ok());
    folder
        .into_iter()
        .flat_map(|f| f.components())
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" / ")
}

fn cursor_at(cx: &Cx, listed: &Listed) -> Option<usize> {
    listed.indices.get(cx.state.cursor.as_deref()?).copied()
}

/// Open or shut the folder at `path`, for good.
fn set_open(cx: &Cx, path: &str, open: bool) {
    cx.p.shared.libraries.edit(|s| {
        s.folders.insert(path.to_owned(), open);
    });
}

/// Keep the library shown and the row chosen, to come back to.
fn keep_place(cx: &Cx) {
    let library = match &cx.state.source {
        Some(Source::Library(name)) => cx.view.shelf.named(name).map(|l| l.dir.to_string_lossy().into_owned()),
        _ => None,
    }
    .unwrap_or_default();
    let row = cx.state.cursor.clone().unwrap_or_default();
    if cx.settings.last_library != library || cx.settings.last_row != row {
        cx.p.shared.libraries.edit(|s| (s.last_library, s.last_row) = (library, row));
    }
}

/// Load `path` into the selected part, or into a new one when `new` (or
/// when no part with an instrument is selected). A multi replaces the rack.
fn load(cx: &mut Cx, path: &Path, new: bool) {
    let text = path.to_string_lossy().into_owned();
    cx.state.cursor = Some(text.clone());
    keep_place(cx);
    let selected = (cx.state.chosen()).filter(|&s| cx.selection.parts.get(s).is_some_and(|p| !p.path.is_empty()));
    match selected {
        _ if import::is_multi(path) => cx.open(path),
        _ if new => cx.add(text),
        // Already in the rack: shown, not loaded twice.
        _ if cx.selection.parts.iter().any(|p| p.path == text && p.program == 0) => cx.open(path),
        Some(slot) => cx.replace(slot, text),
        _ => cx.open(path),
    }
}

/// The pane a focused id sits in, for Tab to cross from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Sources,
    Presets,
}

fn pane_of(id: &str) -> Option<Pane> {
    if id.starts_with("library-") || id.starts_with("source-") {
        Some(Pane::Sources)
    } else if id.starts_with("instrument-") || id.starts_with("folder-") || id.starts_with("star-") || id == "search" {
        Some(Pane::Presets)
    } else {
        None
    }
}

/// A library thumbnail's size in the upper pane.
pub const THUMB: (f64, f64) = (TEXT * 3., TEXT * 1.75);

/// A pseudo-entry's mark where a library shows its artwork.
fn symbol(icon: Icon) -> El {
    stack![glyph(icon, TEXT, secondary()).centered()]
        .w(THUMB.0)
        .h(THUMB.1)
        .fill(Role::Field)
        .shrink(0)
}

/// The active inline display-name field, committed through global preferences.
fn library_name(ui: &mut Ui, cx: &mut Cx, dir: &str) -> Option<El> {
    let (_, text) = cx.state.browse.renaming.as_mut().filter(|(at, _)| at == dir)?;
    let id = "library-name";
    let existed = ui.scene().is_some_and(|s| s.surface(id).is_some());
    if !existed { ui.focus(id); }
    let field = text_edit(ui, id, text, TextOpts::default());
    let cancel = ui.keys(id).iter().any(|k| k.key == Key::Escape);
    let done = field.changed.submitted || (existed && !ui.focused(id));
    let el = field.el.h(STRIP).flex(1).min_w(0).named("Library display name");
    if cancel { cx.state.browse.renaming = None; }
    else if done {
        let (dir, name) = cx.state.browse.renaming.take().unwrap();
        cx.p.shared.libraries.edit(|settings| settings.rename_library(&dir, &name));
    }
    Some(el)
}

/// One entry of the upper pane: an accent edge when chosen, the thumbnail,
/// the name, how many presets; while one of its instruments loads, how far
/// it is, and a thin bar under the name filling with it.
fn source_row(
    id: &str,
    label: String,
    count: usize,
    thumb: El,
    height: f64,
    card: bool,
    chosen: bool,
    loading: Option<f64>,
    about: Option<String>,
    edit: Option<El>,
) -> El {
    let name = edit.unwrap_or_else(|| body(label.clone())
        .text_size(TEXT)
        .fill(if chosen || loading.is_some() { Fill::from(Role::Ink) } else { secondary() })
        .lines(1)
        .min_w(0));
    let named = format!("{label}, {count} presets");
    let (name, count) = match loading {
        Some(done) => (
            col![name, progress_bar(done)].gap(3).align(Align::Start).flex(1).min_w(0),
            super::rack::load_chip(done, false),
        ),
        None => (name.flex(1), caption(count.to_string()).text_size(SMALL).fill(secondary())),
    };
    let mark = block(2, THUMB.1).fill(if chosen { Fill::from(accent()) } else { Role::Ink.alpha(0.) });
    let el = if card {
        col![thumb, row![mark, name, count].gap(SPACE).align(Align::Center)
            .pad(edges(0., INSET, 0., 0.)).h(SOURCE_ROW)]
            .gap(0).align(Align::Stretch).pad((TIGHT, TIGHT)).h(height)
    } else {
        row![mark, thumb, name, count].gap(SPACE).align(Align::Center)
            .pad(edges(0., INSET, 0., 0.)).h(height)
    }
    .when(chosen, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(named)
    .tip({
        let verb = if chosen { "Click again to search every library" } else { "Show its presets below" };
        match about {
            Some(about) => format!("{about}\n{verb}, drag to reorder, right-click for more"),
            None => verb.to_owned(),
        }
    })
    .id(id.to_owned())
    .shrink(0);
    interactive(el, chosen)
}

/// A library's tooltip: its vendor, and whether it has a library file or
/// was recognized by its folders.
fn about(library: &Library, name: &str, pinned: bool) -> String {
    let mut out = name.to_owned();
    if !library.vendor.is_empty() {
        out += &format!(" by {}", library.vendor);
    }
    out += if library.registered { "\nHas a library file" } else { "\nFound by its folders, no library file" };
    if pinned {
        out += "\nPinned to the top";
    }
    out
}

/// The chosen library over its presets: its name, vendor, how many
/// instruments and multis, and its size on disk once measured.
fn library_heading(library: &Library, name: String, size: Option<u64>) -> El {
    let plural = |n: usize, one: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") };
    let mut facts = vec![plural(library.instruments, "instrument")];
    if library.multis > 0 {
        facts.push(plural(library.multis, "multi"));
    }
    facts.push(match size {
        Some(bytes) if bytes >= 1 << 30 => format!("{:.1} GB", bytes as f64 / f64::from(1 << 30)),
        Some(bytes) => format!("{:.0} MB", bytes as f64 / f64::from(1 << 20)),
        None => "Calculate size".into(),
    });
    let mut lines = vec![body(name.clone()).text_size(TEXT).lines(1).min_w(0).tip(name)];
    if !library.vendor.is_empty() {
        lines.push(caption(library.vendor.clone()).fill(secondary()).lines(1).min_w(0));
    }
    let facts = caption(facts.join(" · ")).fill(secondary()).lines(1).min_w(0);
    lines.push(if size.is_none() {
        interactive(facts.focusable().a11y(A11y::Button).named("Calculate library size")
            .tip("Count file sizes in the background").id("library-size"), false)
    } else { facts });
    col(lines).gap(2).align(Align::Start).pad(edges(TIGHT, INSET, SPACE, INSET)).shrink(0)
}

/// Where the cursor is in the library: its folders, each a step back to.
/// None at the library's top.
fn crumbs(ui: &mut Ui, cx: &mut Cx, library: &Library, listed: &Listed) -> Option<El> {
    let at = cursor_at(cx, listed)?;
    let rows = &listed.rows;
    let folder = match &rows[at] {
        Row::Folder { path, .. } => PathBuf::from(path),
        Row::Preset { path, .. } => path.parent()?.to_path_buf(),
    };
    let inside = folder.strip_prefix(&library.dir).ok()?;
    // One folder deep, its row says as much.
    if inside.components().count() < 2 {
        return None;
    }
    let mut items = Vec::new();
    let mut path = library.dir.clone();
    for (n, part) in inside.components().enumerate() {
        path.push(part);
        let key = path.to_string_lossy().into_owned();
        let id = format!("crumb-{n}");
        if ui.get(id.as_str()).activated() {
            cx.state.cursor = Some(key.clone());
            cx.state.browse.reveal_row = true;
        }
        if n > 0 {
            items.push(caption("/").fill(secondary()).shrink(0));
        }
        let here = cx.state.cursor.as_deref() == Some(key.as_str());
        let name = part.as_os_str().to_string_lossy().into_owned();
        items.push(interactive(
            row![caption(name.clone()).fill(if here { Fill::from(Role::Ink) } else { secondary() }).lines(1).min_w(0)]
                .pad((TIGHT, 0))
                .min_w(0)
                .focusable()
                .a11y(A11y::Button)
                .named(format!("Back to {name}"))
                .tip(format!("Back to {name}"))
                .id(id),
            here,
        ));
    }
    (!items.is_empty()).then(|| {
        row(items)
            .gap(0)
            .align(Align::Center)
            .pad(edges(0., INSET, TIGHT, INSET - TIGHT))
            .clip()
            .shrink(0)
            .named("Folders")
    })
}

/// Under the tabs while libraries are looked for: how far, and a stop.
fn scan_line(ui: &mut Ui, cx: &mut Cx, scanning: Option<(usize, usize)>) -> El {
    let Some((folders, found)) = scanning else {
        return block(0, 0);
    };
    let (stop, stop_el) = icon_button(ui, "scan-stop", Icon::Close, "Stop scanning", false);
    if stop {
        cx.p.shared.libraries.cancel();
    }
    let found = if found == 1 { "1 library".to_owned() } else { format!("{found} libraries") };
    row![
        caption(format!("Scanning · {folders} folders · {found}")).fill(secondary()).lines(1).flex(1).min_w(0),
        stop_el
    ]
    .gap(SPACE)
    .align(Align::Center)
    .pad(edges(TIGHT, SPACE, 0., INSET))
    .shrink(0)
    .named("Library scan progress")
}

/// No libraries yet: how to add them, and the two ways to.
fn empty_state(ui: &mut Ui, cx: &mut Cx) -> El {
    let (many, many_el) = action(ui, "empty-add-many", "Add folder of libraries…", false);
    let (one, one_el) = action(ui, "empty-add-one", "Add library folder…", false);
    if many || one {
        super::header::add_folder(cx, one);
    }
    col![
        body("No libraries yet").text_size(TEXT).lines(1),
        many_el.tip("Add the folder that holds your libraries; each library is found, with or without a library file."),
        one_el.tip("Add one library's own folder."),
    ]
    .gap(SPACE)
    .align(Align::Start)
    .pad(edges(SPACE, INSET, SPACE, INSET))
    .shrink(0)
}

/// A thin track filling with the accent to `done` (0..1).
fn progress_bar(done: f64) -> El {
    canvas(move |s| {
        vec![
            Draw::fill(rect(0., 0., s.width, s.height), Role::Ink.alpha(0.12)),
            Draw::fill(rect(0., 0., (s.width * done).max(s.height), s.height), accent()),
        ]
    })
    .w(Len::Pct(100.))
    .h(2)
    .shrink(0)
    .named("Load progress")
}

/// A pane slid by the browser itself, so a row can be brought into view:
/// the wheel and the bar at its edge move it to `y`, and `reveal`, a row's
/// top and bottom, brings that row in. Returns where it is drawn, its bar
/// when it overflows, and whether `reveal` was done. A `fresh` list is
/// drawn where it is put, not glided to.
fn slide(ui: &mut Ui, id: &str, y: &mut f64, content_h: f64, reveal: Option<(f64, f64)>, fresh: bool) -> (f64, Option<El>, bool) {
    let from = *y;
    let view_h = ui.scene().and_then(|s| s.surface(id)).map_or(0., |s| s.frame.size.height);
    if let Some(w) = ui.wheel(id) {
        *y += w.y;
    }
    let bar_id = format!("{id}-bar");
    bar_drag(ui, &bar_id, y, view_h, content_h);
    let revealed = view_h > 0. && reveal.is_some();
    if let Some((top, bottom)) = reveal.filter(|_| view_h > 0.) {
        *y = geometry::reveal_offset(*y, view_h, top, bottom);
    }
    *y = y.clamp(0., (content_h - view_h).max(0.));
    // The rows are built for where it is drawn, gliding to `y`.
    let snap = fresh || leaps(from, *y) || ui.get(bar_id.as_str()).held;
    let drawn = glide(ui, id, *y, snap);
    let bar = (view_h > 0. && content_h > view_h + 0.5).then(|| scrollbar(ui, &bar_id, "Scroll the list", drawn, view_h, content_h));
    (drawn, bar, revealed)
}

/// The divider between the panes: drag it to share out the height, double-
/// click to reset. The saved state takes it when let go.
fn split_divider(ui: &mut Ui, cx: &mut Cx) -> El {
    let id = "browser-split";
    let r = ui.get(id);
    let height = ui
        .scene()
        .and_then(|s| {
            Some(s.surface(&format!("browser-sources-{}", cx.state.uvi))?.frame.size.height
                + s.surface(if cx.state.uvi { "browser-list-uvi" } else { "browser-list" })?.frame.size.height)
        })
        .unwrap_or(600.);
    if r.dragged {
        cx.state.split = (cx.state.split + r.drag_delta.y / height.max(1.)).clamp(SPLIT_MIN, SPLIT_MAX);
    }
    if r.double_clicked {
        cx.state.split = SPLIT;
    }
    if r.released || r.double_clicked {
        cx.selection.browser_split = cx.state.split as f32;
    }
    let lift = edge_lift(ui, id);
    // A hairline at rest, the accent under the hand; grabbed a little wide.
    canvas(move |s| {
        let mid = (s.height / 2.).floor();
        let mut d = vec![Draw::fill(rect(0., mid, s.width, 1.), Role::Ink.alpha(0.08))];
        d.extend(edge_mark(s, mid + 0.5, false, lift));
        d
    })
    .w(Len::Pct(100.))
    .h(EDGE_GRAB + 3.)
    .shrink(0)
    .cursor(Cursor::ResizeV)
    .tip("Drag to share out the height, double-click to reset")
    .named("Resize the library list")
    .id(id)
}

/// A search field with its glyph, placeholder and clear button. The arrows
/// and Enter still work from inside it.
fn search_field(ui: &mut Ui, id: &str, text: &mut String, placeholder: &str, name: &str) -> El {
    let field = text_input(ui, id, text);
    let empty = text.is_empty() && !ui.focused(id);
    let (clear, clear_el) = icon_button(ui, format!("{id}-clear"), Icon::Close, "Clear", false);
    if clear {
        text.clear();
    }
    let mut layers = vec![
        field
            .el
            .w(Len::Pct(100.))
            .h(CONTROL + TIGHT)
            .pad(edges(0., CONTROL + TIGHT, 0., CONTROL))
            .named(name.to_owned()),
        glyph(Icon::Search, TEXT + 2., secondary())
            .anchor(Align::Start, Align::Center)
            .offset(SPACE, 0.),
        row![caption(placeholder.to_owned()).fill(secondary()).lines(1).min_w(0)]
            .align(Align::Center)
            .pad(edges(0., 0., 0., CONTROL + TIGHT))
            .h(CONTROL + TIGHT)
            .min_w(0)
            .when(!empty, |e| e.opacity(0.))
            .disabled(),
    ];
    if !text.is_empty() {
        layers.push(clear_el.anchor(Align::End, Align::Center));
    }
    stack(layers).h(CONTROL + TIGHT).shrink(0)
}

/// The keys on the list: Up and Down (and the pages, Home and End) move the
/// cursor, Right opens a folder or steps into it, Left shuts it or steps
/// back to the folder above; Enter opens a folder or loads a preset
/// (Shift+Enter into a new part).
fn walk(ui: &mut Ui, cx: &mut Cx, listed: &Listed, page: usize, focus_to: &mut Option<String>) {
    let rows = &listed.rows;
    let from_search = ui.focused("search");
    let free = ui.focus_key().is_none_or(|k| k.starts_with("instrument-") || k.starts_with("folder-"));
    let keys: Vec<KeyPress> = if from_search {
        ui.keys("search").to_vec()
    } else if free {
        ui.shortcuts().to_vec()
    } else {
        Vec::new()
    };
    if rows.is_empty() {
        return;
    }
    let last = rows.len() - 1;
    let unfocused = ui.focus_key().is_none();
    for k in keys {
        let at = cursor_at(cx, listed);
        let parent = |n: usize| {
            let depth = rows[n].depth().checked_sub(1)?;
            (0..n).rev().find(|&i| matches!(&rows[i], Row::Folder { depth: d, .. } if *d == depth))
        };
        let next = match (k.key, at) {
            (Key::Down, None) => Some(0),
            (Key::Down, Some(n)) => Some((n + 1).min(last)),
            (Key::Up, Some(n)) => Some(n.saturating_sub(1)),
            (Key::Up, None) => Some(last),
            (Key::PageDown, n) => Some(n.map_or(0, |n| (n + page).min(last))),
            (Key::PageUp, n) => Some(n.map_or(0, |n| n.saturating_sub(page))),
            (Key::Home, _) if !from_search => Some(0),
            (Key::End, _) if !from_search => Some(last),
            (Key::Right, Some(n)) if !from_search => match &rows[n] {
                Row::Folder { open: false, path, .. } => {
                    set_open(cx, path, true);
                    None
                }
                Row::Folder { open: true, .. } => Some((n + 1).min(last)),
                _ => None,
            },
            (Key::Left, Some(n)) if !from_search => match &rows[n] {
                Row::Folder { open: true, path, .. } => {
                    set_open(cx, path, false);
                    None
                }
                _ => parent(n),
            },
            (Key::Enter, Some(n)) if from_search || unfocused => {
                match &rows[n] {
                    Row::Folder { path, open, .. } => set_open(cx, path, !open),
                    Row::Preset { path, .. } => load(cx, &path.clone(), k.mods.shift),
                }
                None
            }
            (Key::Enter, None) if from_search => {
                if let Some(Row::Preset { path, .. }) = rows.iter().find(|r| matches!(r, Row::Preset { .. })) {
                    load(cx, &path.clone(), k.mods.shift);
                }
                None
            }
            _ => None,
        };
        if let Some(n) = next {
            cx.state.cursor = Some(rows[n].key());
            cx.state.browse.reveal_row = true;
            if !from_search {
                *focus_to = Some(rows[n].id(n));
            }
        }
    }
}

/// A folder row: click or Enter opens or shuts it, the arrow shows which,
/// and how many presets it holds in all.
fn folder(ui: &mut Ui, cx: &mut Cx, n: usize, row: &Row) -> El {
    let Row::Folder { path, name, depth, count, open } = row else { return block(0, ROW) };
    let id = row.id(n);
    let r = ui.get(id.as_str());
    // A double-click's second click leaves it as the first left it.
    if (r.clicked_with(Button::Primary) && !r.double_clicked) || r.key_activated {
        cx.state.cursor = Some(path.clone());
        set_open(cx, path, !open);
        keep_place(cx);
    }
    let cursor = cx.state.cursor.as_deref() == Some(path.as_str());
    let el = row![
        glyph(if *open { Icon::Down } else { Icon::Right }, TEXT, secondary()).shrink(0),
        body(name.clone())
            .text_size(TEXT)
            .fill(if cursor { Fill::from(Role::Ink) } else { secondary() })
            .lines(1)
            .flex(1)
            .min_w(0),
        caption(count.to_string()).text_size(SMALL).fill(secondary()).shrink(0),
    ]
    .gap(TIGHT)
    .align(Align::Center)
    .pad(edges(0., SPACE, 0., SPACE + *depth as f64 * INDENT))
    .h(ROW)
    .when(cursor, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(format!("{name}, {count} presets, {}", if *open { "open" } else { "shut" }))
    .tip(format!("{name}\nClick to {}, Left and Right fold", if *open { "shut" } else { "open" }))
    .id(id)
    .shrink(0);
    interactive(el, cursor)
}

/// One preset row: click selects, Enter loads into the selected part
/// (Shift+Enter into a new one), double-click loads, drag drops it onto the
/// rack, right-click opens its menu. A star at its end, shown on hover and
/// kept once set, makes it a favorite. `under`, when set, is its folder.
fn preset(ui: &mut Ui, cx: &mut Cx, n: usize, path: &Path, depth: usize, under: &str) -> El {
    let id = format!("instrument-{n}");
    let star_id = format!("star-{n}");
    let text = path.to_string_lossy().into_owned();
    let loaded = cx.selection.parts.iter().any(|p| p.path == text);
    let loading = (cx.selection.parts.iter().enumerate())
        .filter(|(_, p)| p.path == text)
        .find_map(|(slot, _)| super::rack::loading(cx, slot));
    if ui.get(star_id.as_str()).activated() {
        cx.toggle_favorite(&text);
    }
    let favorite = cx.selection.favorites.contains(&text);
    let hover = ui.state(id.as_str()).hover.max(ui.state(star_id.as_str()).hover);
    let star_hover = ui.state(star_id.as_str()).hover as f32;
    let star = stack![glyph(
        if favorite { Icon::StarFilled } else { Icon::Star },
        TEXT,
        if favorite || star_hover > 0.5 { Fill::from(Role::Ink) } else { secondary() },
    )
    .centered()]
    .square(TEXT + 2.)
    .cursor(Cursor::Hand)
    .focusable()
    .a11y(A11y::Toggle { on: favorite })
    .named(if favorite { "Remove from favorites" } else { "Add to favorites" })
    .tip(if favorite { "Remove from favorites" } else { "Add to favorites" })
    .id(star_id)
    .when(!favorite && hover < 0.5, |e| e.opacity(0.));
    let r = ui.get(id.as_str());
    if r.clicked_with(Button::Primary) {
        cx.state.cursor = Some(text.clone());
    }
    if r.double_clicked {
        cx.state.cursor = Some(text.clone());
        keep_place(cx);
        cx.open(path);
    } else if r.key_activated {
        let shift = ui.keys(id.as_str()).iter().any(|k| k.mods.shift);
        load(cx, path, shift);
    }
    if r.clicked_with(Button::Secondary) {
        cx.state.cursor = Some(text.clone());
        menu::open(ui, cx, menu::Target::Preset(text.clone()));
    }
    if r.dragged && r.button == Some(Button::Primary) {
        ui.start_drag(id.as_str(), RackDrag::Instrument(text.clone()));
    }
    let cursor = cx.state.cursor.as_deref() == Some(text.as_str());
    let verb = if import::is_multi(path) {
        "Double-click or Enter to load this multi into the rack"
    } else {
        "Enter loads into the selected part, Shift+Enter or double-click into a new one; drag onto the rack"
    };
    let name = body(without_library(&stem(path), &cx.library_of(path)).to_owned())
        .text_size(TEXT)
        .fill(if loaded || cursor { Fill::from(Role::Ink) } else { secondary() })
        .lines(1)
        .min_w(0);
    let name = if under.is_empty() {
        name.flex(1)
    } else {
        col![name, caption(under.to_owned()).fill(secondary()).lines(1).min_w(0)]
            .gap(1)
            .align(Align::Start)
            .flex(1)
            .min_w(0)
    };
    // Presets in a folder line up with its name, past its arrow.
    let indent = if depth > 0 { depth as f64 * INDENT - SPACE } else { 0. };
    let el = row![
        block(1, TEXT).fill(if loaded { Fill::from(accent()) } else { Role::Ink.alpha(0.) }),
        name,
        match loading {
            Some(done) => super::rack::load_chip(done, false),
            None => star,
        },
    ]
    .gap(INSET - 1.)
    .align(Align::Center)
    .pad(edges(0., SPACE, 0., SPACE + indent))
    .h(if under.is_empty() { ROW } else { ROW2 })
    .when(cursor, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(stem(path))
    .tip(format!("{}\n{verb}", path.display()))
    .id(id)
    .shrink(0);
    interactive(el, cursor)
}

fn hint(text: &str) -> El {
    col![body(text).fill(secondary()).text_size(TEXT).lines(4)]
        .pad(edges(SPACE, INSET, SPACE, INSET))
        .shrink(0)
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_separate_native_multis_snapshots_and_natural_order() {
        let library = Library { dir: "/lib".into(), ..Default::default() };
        let paths = ["/lib/Patch 10.nki", "/lib/Patch 2.nki", "/lib/Rack.nkm", "/lib/Scene.kontra-multi", "/lib/Snap.nksn"];
        let paths: Vec<_> = paths.iter().map(Path::new).collect();
        let rows = categories(&library, &paths, &BTreeMap::new());
        let names: Vec<_> = rows.iter().filter_map(|r| match r { Row::Folder { name, .. } => Some(name.as_str()), _ => None }).collect();
        assert_eq!(names, ["Instruments", "Multis", "Snapshots"]);
        let files: Vec<_> = rows.iter().filter_map(|r| match r { Row::Preset { path, .. } => Some(path.as_path()), _ => None }).collect();
        assert_eq!(files, [Path::new("/lib/Patch 2.nki"), Path::new("/lib/Patch 10.nki")]);
        let rows = categories(&library, &paths, &BTreeMap::from([("/lib/Multis".into(), true)]));
        assert!(rows.iter().any(|r| matches!(r, Row::Preset { path, .. } if path.ends_with("Rack.nkm"))));
    }

    #[test]
    fn cached_rows_preserve_scroll_offsets_counts_and_cursor_positions() {
        let mut rows = vec![Row::Folder {
            path: "/virtual/Large/Instruments".into(),
            name: "Instruments".into(),
            depth: 0,
            count: 10_000,
            open: true,
        }];
        rows.extend((0..10_000).map(|n| Row::Preset {
            path: format!("/virtual/Large/Instruments/Patch {n:05}.nki").into(),
            depth: 1,
            index: 0,
            under: if n % 2 == 0 { "Instruments".into() } else { String::new() },
        }));
        // Duplicate favorites historically resolve to the first matching row.
        rows.push(rows[1].clone());
        let listed = Listed::new(rows);
        let mut y = 0.;
        for (n, row) in listed.rows.iter().enumerate() {
            assert_eq!(listed.tops[n], y);
            y += row.height();
        }
        assert_eq!(listed.tops.last(), Some(&y));
        assert_eq!(listed.presets, 10_001);
        for n in [0, 1, 5000, 10_000] {
            let key = listed.rows[n].key();
            assert_eq!(listed.indices.get(&key), Some(&n));
        }
        assert!(!listed.indices.contains_key("missing"));
        let replacement = Listed::new(vec![listed.rows[10_000].clone()]);
        assert_eq!(replacement.indices.get(&listed.rows[10_000].key()), Some(&0));
        assert!(!replacement.indices.contains_key(&listed.rows[1].key()));
        let empty = Listed::new(Vec::new());
        assert_eq!(empty.tops, [0.]);
        assert_eq!(empty.presets, 0);
        assert!(empty.indices.is_empty());
    }
}
