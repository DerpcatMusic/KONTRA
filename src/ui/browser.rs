//! The browser, split like Bitwig's: the libraries above, with Favorites and
//! Recent over them, and the chosen one's presets below, grouped by folder.
//! The search filters the presets below (every library when none is
//! chosen). Click selects, double-click or Enter loads, drag drops onto the
//! rack, right-click offers the rest; the arrows walk each pane and Tab
//! crosses between them. The divider and the browser's edge both drag.

use super::{Cx, RackDrag, menu, theme::*};
use crate::import;
use moose::mui::mui::prelude::*;
use moose::mui::mui::scene::Fit;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What the lower pane lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Favorites,
    Recent,
    Library(String),
}

/// The upper pane's share of the browser's height.
pub const SPLIT: f64 = 0.36;
pub const SPLIT_MIN: f64 = 0.14;
pub const SPLIT_MAX: f64 = 0.7;

pub fn ordered_libraries<'a>(
    shelf: &'a crate::library::Shelf,
    settings: &crate::library::Settings,
) -> Vec<&'a crate::library::Library> {
    let ranks: BTreeMap<_, _> = settings
        .order
        .iter()
        .enumerate()
        .map(|(n, p)| (Path::new(p), n))
        .collect();
    let mut libs: Vec<_> = shelf.libraries.iter().collect();
    libs.sort_by_cached_key(|l| {
        (
            ranks
                .get(l.dir.as_path())
                .copied()
                .unwrap_or(usize::MAX),
            l.name.to_lowercase(),
        )
    });
    libs
}

const PRESET_ROW: f64 = CONTROL + SPACE;

/// The rack's scrolling model, with more wheel travel for the browser.
fn scroll_position(ui: &mut Ui, id: &str, y: &mut f64, content: f64) -> (f64, f64) {
    let height = ui
        .scene()
        .and_then(|s| s.surface(id))
        .map_or(400., |s| s.frame.size.height);
    if let Some(w) = ui.wheel(id) {
        *y += w.y * 3.;
    }
    let bar_id = format!("{id}-bar");
    let bar = ui.get(bar_id.as_str());
    if bar.dragged && height > 0. {
        let thumb = (height * height / content.max(1.)).max(CONTROL).min(height);
        *y += bar.drag_delta.y * (content - height).max(0.) / (height - thumb).max(1.);
    }
    if let Some(w) = ui.wheel(bar_id.as_str()) {
        *y += w.y * 3.;
    }
    *y = y.clamp(0., (content - height).max(0.));
    let drawn = if bar.held {
        *y
    } else {
        ui.tween_with(format!("{id}-y"), *y, quick())
    };
    (height, drawn)
}

fn scroll_view(ui: &mut Ui, id: &str, items: Vec<El>, content: f64, height: f64, y: f64) -> El {
    let viewport = col![
        col(items)
            .gap(0)
            .align(Align::Stretch)
            .w(Len::Pct(100.))
            .shrink(0)
            .id(format!("{id}-content"))
    ]
    .gap(0)
    .align(Align::Stretch)
    .flex(1)
    .min_w(0)
    .min_h(0)
    .h(Len::Pct(100.))
    .scroll()
    .no_scrollbar()
    .scrolled(0., y.round())
    .id(id.to_owned());
    let mut cols = vec![viewport];
    if content > height + 0.5 {
        cols.push(scrollbar(
            ui,
            &format!("{id}-bar"),
            "Scroll presets or libraries",
            y,
            height,
            content,
        ));
    }
    row(cols).gap(0).align(Align::Stretch).flex(1).min_h(0)
}

pub fn sidebar(ui: &mut Ui, cx: &mut Cx) -> El {
    let view = cx.view;
    let multis = cx.state.multis;
    // Which library each file is in, worked out once per scan.
    let (files, shelf, kind, grouped) = &mut cx.state.libraries;
    let scanned = Arc::as_ptr(&view.shelf) as usize;
    if !files.upgrade().is_some_and(|f| Arc::ptr_eq(&f, &view.files)) || *shelf != scanned || *kind != multis {
        let mut by: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (n, file) in view.files.iter().enumerate().filter(|(_, f)| import::is_multi(f) == multis) {
            let library = super::library_of(&view.shelf, file);
            if !library.is_empty() {
                by.entry(library).or_default().push(n);
            }
        }
        (*files, *shelf, *kind, *grouped) = (Arc::downgrade(&view.files), scanned, multis, Arc::new(by));
    }
    let libraries: BTreeMap<String, Vec<&PathBuf>> = grouped
        .iter()
        .map(|(name, files)| (name.clone(), files.iter().map(|&n| &view.files[n]).collect()))
        .collect();
    // Favorites and recents of the kind picked.
    let kind = |paths: &[String]| -> Vec<PathBuf> {
        paths
            .iter()
            .map(PathBuf::from)
            .filter(|p| import::is_multi(p) == multis)
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
        (false, "Instruments", "picker-instruments"),
        (true, "Multis", "picker-multis"),
    ] {
        let (hit, el) = tab(ui, id, label, multis == multi);
        if hit {
            cx.state.multis = multi;
        }
        kinds.push(el);
    }

    let library_search = search_field(ui, cx, multis, true);
    let library_needle = cx.state.library_search.to_lowercase();
    let ordered: Vec<_> = ordered_libraries(&view.shelf, &cx.settings)
        .into_iter()
        .filter(|l| libraries.contains_key(&l.name))
        .filter(|l| {
            library_needle.is_empty()
                || format!("{} {}", l.name, l.vendor)
                    .to_lowercase()
                    .contains(&library_needle)
        })
        .collect();
    // The upper pane: the two pseudo-entries, then the matching libraries.
    let mut sources = vec![
        ("source-favorites".to_owned(), Source::Favorites),
        ("source-recent".to_owned(), Source::Recent),
    ];
    for (idx, library) in ordered.iter().enumerate() {
        sources.push((
            format!("library-{idx}"),
            Source::Library(library.name.clone()),
        ));
    }
    // Which folder each row shows, for a picture dropped on one.
    *super::lock(&cx.state.picker.rows) =
        ordered.iter().map(|l| Some(l.dir.clone())).collect();
    if cx
        .state
        .source
        .as_ref()
        .is_some_and(|s| matches!(s, Source::Library(n) if !libraries.contains_key(n)))
    {
        cx.state.source = None;
    }
    // Enter on an entry opens it and moves on to its presets.
    let mut enter = false;
    let mut rows = Vec::new();
    for (n, (id, source)) in sources.iter().enumerate() {
        let r = ui.get(id.as_str());
        if r.clicked_with(Button::Primary) {
            // A second click lets the library go: the search spans them all.
            cx.state.source = (cx.state.source.as_ref() != Some(source)).then(|| source.clone());
            cx.state.cursor = None;
        }
        if r.key_activated {
            cx.state.source = Some(source.clone());
            cx.state.cursor = None;
            enter = true;
        }
        if let (true, Source::Library(name)) = (r.clicked_with(Button::Secondary), source) {
            menu::open(ui, cx, menu::Target::Library(name.clone()));
        }
        if ui.focused(id.as_str()) {
            let step = ui.shortcuts().iter().fold(0i32, |at, k| match k.key {
                Key::Down => at + 1,
                Key::Up => at - 1,
                _ => at,
            });
            if step != 0 {
                let next = (n as i32 + step).clamp(0, sources.len() as i32 - 1) as usize;
                cx.state.source = Some(sources[next].1.clone());
                cx.state.cursor = None;
                ui.focus(sources[next].0.clone());
            }
        }
        let (label, count, thumb) = match source {
            Source::Favorites => ("Favorites".to_owned(), favorites.len(), symbol(Icon::Star)),
            Source::Recent => ("Recent".to_owned(), recent.len(), symbol(Icon::Recent)),
            Source::Library(name) => (
                library_label(name),
                libraries[name].len(),
                match cx.looks(name).and_then(|l| l.thumb.clone()) {
                    Some(image) => block(THUMB.0, THUMB.1)
                        .fill(Fill::Image(image, Fit::Cover))
                        .shrink(0),
                    None => symbol(Icon::Sidebar),
                },
            ),
        };
        let chosen = cx.state.source.as_ref() == Some(source);
        let progress = match source {
            Source::Library(name) => loading.get(name).map(|(sum, n)| sum / *n as f64),
            _ => None,
        };
        let about = match source {
            Source::Library(name) => view.shelf.named(name).map(about),
            _ => None,
        };
        rows.push(source_row(id, label, count, thumb, chosen, progress, about));
        if n == 1 {
            rows.push(rule().pad((TIGHT, INSET)));
        }
    }
    let scanning = cx.p.shared.libraries.scanning();
    if libraries.is_empty() && scanning.is_none() {
        if view.files.is_empty() {
            // First, so its buttons are in view above favorites and recent.
            rows.insert(0, empty_state(ui, cx));
        } else {
            rows.push(hint(if multis { "No multis in these libraries." } else { "No instruments in these libraries." }));
        }
    }

    // The lower pane: the chosen source's presets, the search filtering them.
    let search = search_field(ui, cx, multis, false);
    let needle = cx.state.search.to_lowercase();
    let matches = |p: &Path| needle.is_empty() || stem(p).to_lowercase().contains(&needle);
    let mut folders = BTreeSet::new();
    let mut folder_root = PathBuf::new();
    let mut folder_at = PathBuf::new();
    let mut groups: Vec<(String, Vec<PathBuf>)> = Vec::new();
    let mut push = |group: String, path: PathBuf| match groups.last_mut() {
        Some((g, paths)) if *g == group => paths.push(path),
        _ => groups.push((group, vec![path])),
    };
    let empty = match &cx.state.source {
        Some(Source::Favorites) => {
            favorites.into_iter().filter(|p| matches(p)).for_each(|p| push(String::new(), p));
            "Star a preset to keep it here."
        }
        Some(Source::Recent) => {
            recent.into_iter().filter(|p| matches(p)).for_each(|p| push(String::new(), p));
            "Presets you open show up here."
        }
        Some(Source::Library(name)) => {
            let dir = view.shelf.named(name).map(|l| l.dir.clone()).unwrap_or_default();
            folder_root = dir.clone();
            let container = dir.join(if multis { "Multis" } else { "Instruments" });
            if libraries[name].iter().all(|p| p.starts_with(&container)) { folder_root = container; }
            if !cx.state.browser_folder.as_ref().is_some_and(|(n, k, p)| n == name && *k == multis && p.starts_with(&folder_root)) {
                cx.state.browser_folder = Some((name.clone(), multis, folder_root.clone()));
            }
            folder_at = cx.state.browser_folder.as_ref().unwrap().2.clone();
            for path in &libraries[name] {
                if !needle.is_empty() {
                    if matches(path) { push(subfolder(&dir, path), (*path).clone()); }
                } else if path.parent() == Some(folder_at.as_path()) {
                    push(String::new(), (*path).clone());
                } else if let Ok(relative) = path.strip_prefix(&folder_at)
                    && relative.components().count() > 1
                    && let Some(first) = relative.components().next()
                { folders.insert(folder_at.join(first.as_os_str())); }
            }
            "Nothing here matches that search."
        }
        None if !needle.is_empty() => {
            for (name, files) in &libraries {
                for path in files.iter().filter(|p| matches(p)) {
                    push(library_label(name), (*path).clone());
                }
            }
            "Nothing matches that search."
        }
        None => "Choose a library above, or search them all.",
    };
    let mut heading = Vec::new();
    if let Some(Source::Library(name)) = &cx.state.source
        && let Some(library) = view.shelf.named(name)
    {
        let size = cx.p.shared.libraries.size(&library.dir);
        heading.push(library_heading(library, size));
        if needle.is_empty() {
            let (up, up_el) = icon_button(ui, "folder-up", Icon::Left, "Parent folder", false);
            if up && folder_at != folder_root && let Some(parent) = folder_at.parent() {
                cx.state.browser_folder = Some((name.clone(), multis, parent.to_owned()));
                cx.state.cursor = None;
            }
            let relative = folder_at.strip_prefix(&folder_root).unwrap_or(&folder_at);
            heading.push(row![up_el.when(folder_at == folder_root, |e| e.disabled()), caption(if relative.as_os_str().is_empty() {
                "All folders".into()
            } else { relative.to_string_lossy().into_owned() }).fill(Role::Dim).lines(1).min_w(0)]
                .gap(SPACE).align(Align::Center).pad((TIGHT, INSET)).shrink(0));
        }
    }
    enum Row {
        Folder(PathBuf),
        Heading(String, usize),
        Preset(usize),
    }
    let mut entries: Vec<Row> = folders.into_iter().map(Row::Folder).collect();
    let mut listed = Vec::new();
    for (group, paths) in groups {
        if !group.is_empty() { entries.push(Row::Heading(group, paths.len())); }
        for path in paths { entries.push(Row::Preset(listed.len())); listed.push(path); }
    }
    let key = format!(
        "{:?}-{multis}-{needle}-{}",
        cx.state.source,
        folder_at.display()
    );
    if cx.state.browser_list != key {
        cx.state.browser_scroll[1] = 0.;
        cx.state.browser_list = key;
    }
    let before = cx.state.cursor.clone();
    walk(ui, cx, &listed);
    if cx.state.cursor != before
        && let Some(n) = cx.state.cursor.as_ref().and_then(|p| {
            listed
                .iter()
                .position(|l| l.to_string_lossy() == p.as_str())
        })
        && let Some(row) = entries
            .iter()
            .position(|r| matches!(r, Row::Preset(i) if *i == n))
    {
        let height = ui
            .scene()
            .and_then(|s| s.surface("browser-presets"))
            .map_or(400., |s| s.frame.size.height);
        let top = row as f64 * PRESET_ROW;
        let y = &mut cx.state.browser_scroll[1];
        if top < *y {
            *y = top;
        } else if top + PRESET_ROW > *y + height {
            *y = top + PRESET_ROW - height;
        }
    }
    let content = entries.len() as f64 * PRESET_ROW;
    let (height, y) = scroll_position(
        ui,
        "browser-presets",
        &mut cx.state.browser_scroll[1],
        content,
    );
    // Only near-view rows become widgets. Sparse retained focus/capture rows
    // preserve keyboard navigation and a drag that crosses the viewport.
    let mut items = Vec::new();
    let mut gap = 0.;
    let focus = ui.focus_key().map(str::to_owned);
    for (row, entry) in entries.into_iter().enumerate() {
        let top = row as f64 * PRESET_ROW;
        let retained = match &entry {
            Row::Preset(n) => {
                focus.as_deref() == Some(format!("instrument-{n}").as_str())
                    || ui.get(format!("instrument-{n}").as_str()).held
            }
            _ => false,
        };
        if !retained && (top + PRESET_ROW < y - height || top > y + 2. * height) {
            gap += PRESET_ROW;
            continue;
        }
        if gap > 0. {
            items.push(block(0, gap).shrink(0));
            gap = 0.;
        }
        let el = match entry {
            Row::Preset(n) => preset(ui, cx, n, &listed[n]),
            Row::Heading(name, count) => group_heading(&name, count),
            Row::Folder(path) => {
                let id = format!("folder-{row}");
                let (hit, el) = action(
                    ui,
                    id,
                    &format!(
                        "{}  ›",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    false,
                );
                if hit && let Some(Source::Library(name)) = &cx.state.source {
                    cx.state.browser_folder = Some((name.clone(), multis, path.clone()));
                    cx.state.cursor = None;
                }
                el.named(format!("Open folder {}", path.display()))
                    .pad((0., INSET))
            }
        };
        items.push(el.h(PRESET_ROW).min_h(PRESET_ROW).shrink(0));
    }
    if gap > 0. {
        items.push(block(0, gap).shrink(0));
    }
    if items.is_empty() && !libraries.is_empty() {
        items.push(hint(empty));
    }
    let preset_view = scroll_view(ui, "browser-presets", items, content, height, y);
    let into_presets = || {
        if listed.is_empty() {
            "search".to_owned()
        } else {
            "instrument-0".to_owned()
        }
    };
    if enter {
        ui.focus(into_presets());
    }
    // Tab crosses the panes: into the presets at the cursor, back to the
    // chosen entry. The Ui has moved the focus on by now; this overrides it.
    let tabbed = ui
        .focus_key()
        .is_some_and(|k| ui.keys(k.to_owned()).iter().any(|k| k.key == Key::Tab));
    if tabbed {
        match cx.state.pane {
            Some(Pane::Sources) => {
                let at = cx.state.cursor.as_ref().and_then(|c| {
                    listed.iter().position(|p| p.to_string_lossy() == c.as_str())
                });
                ui.focus(at.map_or_else(into_presets, |n| format!("instrument-{n}")));
            }
            Some(Pane::Presets) => {
                let chosen = sources.iter().find(|(_, s)| Some(s) == cx.state.source.as_ref());
                if let Some((id, _)) = chosen.or(sources.first()) {
                    ui.focus(id.clone());
                }
            }
            None => {}
        }
    }
    cx.state.pane = ui.focus_key().and_then(pane_of);

    let scan_line = scan_line(ui, cx, scanning);
    let split = split_divider(ui, cx);
    let source_content = ui
        .scene()
        .and_then(|s| s.surface("browser-sources-content"))
        .map_or(0., |s| s.frame.size.height);
    let (source_height, source_y) = scroll_position(
        ui,
        "browser-sources",
        &mut cx.state.browser_scroll[0],
        source_content,
    );
    let source_view = scroll_view(
        ui,
        "browser-sources",
        rows,
        source_content,
        source_height,
        source_y,
    );
    col![
        section_bar(
            "Browser",
            vec![caption(listed.len().to_string()).text_size(SMALL).fill(Role::Dim), add_el, hide_el]
        ),
        row(kinds).gap(INSET + TIGHT).pad(edges(0., INSET, 0., INSET)).shrink(0),
        scan_line,
        rule(),
        col![col![library_search].pad((SPACE, INSET)).shrink(0), source_view]
            .gap(0).align(Align::Stretch).h(Len::Pct(cx.state.split * 100.)).shrink(0),
        split,
        col![search].pad(edges(SPACE, INSET, SPACE, INSET)).shrink(0),
        col(heading).gap(0).align(Align::Stretch).shrink(0),
        preset_view,
    ]
    .gap(0)
    .h(Len::Pct(100.))
    .shrink(0)
    .fill(Role::Surface)
    .clip()
    .id("browser")
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
    } else if id.starts_with("instrument-") || id.starts_with("star-") || id == "search" {
        Some(Pane::Presets)
    } else {
        None
    }
}

/// A library thumbnail's size in the upper pane.
pub const THUMB: (f64, f64) = (TEXT * 3., TEXT * 1.75);

/// A pseudo-entry's mark where a library shows its artwork.
fn symbol(icon: Icon) -> El {
    stack![glyph(icon, TEXT, Role::Ink.alpha(0.6)).centered()]
        .w(THUMB.0)
        .h(THUMB.1)
        .fill(Role::Field)
        .shrink(0)
}

/// One entry of the upper pane: an accent edge when chosen, the thumbnail,
/// the name, how many presets; while one of its instruments loads, how far
/// it is, and a thin bar under the name filling with it.
fn source_row(
    id: &str,
    label: String,
    count: usize,
    thumb: El,
    chosen: bool,
    loading: Option<f64>,
    about: Option<String>,
) -> El {
    let name = body(label.clone())
        .text_size(TEXT)
        .fill(if chosen || loading.is_some() { Role::Ink } else { Role::Dim })
        .lines(1)
        .min_w(0);
    let named = format!("{label}, {count} presets");
    let (name, count) = match loading {
        Some(done) => (
            col![name, progress_bar(done)].gap(3).align(Align::Start).flex(1).min_w(0),
            super::rack::load_chip(done, false),
        ),
        None => (name.flex(1), caption(count.to_string()).text_size(SMALL).fill(Role::Dim)),
    };
    let el = row![
        block(2, THUMB.1).fill(if chosen { Fill::from(accent()) } else { Role::Ink.alpha(0.) }),
        thumb,
        name,
        count,
    ]
    .gap(SPACE)
    .align(Align::Center)
    .pad(edges(TIGHT, INSET, TIGHT, 0.))
    .when(chosen, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(named)
    .tip({
        let verb = if chosen { "Click again to search every library" } else { "Show its presets below" };
        match about {
            Some(about) => format!("{about}\n{verb}, right-click for its cover"),
            None => verb.to_owned(),
        }
    })
    .id(id.to_owned())
    .shrink(0);
    interactive(el, chosen)
}

/// A library's tooltip: its vendor, and whether it has a library file or
/// was recognized by its folders.
fn about(library: &crate::library::Library) -> String {
    let mut out = library_label(&library.name);
    if !library.vendor.is_empty() {
        out += &format!(" by {}", library.vendor);
    }
    out += if library.registered { "\nHas a library file" } else { "\nFound by its folders, no library file" };
    out
}

/// The chosen library over its presets: its name, vendor, how many
/// instruments and multis, and its size on disk once measured.
fn library_heading(library: &crate::library::Library, size: Option<u64>) -> El {
    let plural = |n: usize, one: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") };
    let mut facts = vec![plural(library.instruments, "instrument")];
    if library.multis > 0 {
        facts.push(plural(library.multis, "multi"));
    }
    facts.push(match size {
        Some(bytes) if bytes >= 1 << 30 => format!("{:.1} GB", bytes as f64 / f64::from(1 << 30)),
        Some(bytes) => format!("{:.0} MB", bytes as f64 / f64::from(1 << 20)),
        None => "measuring size".into(),
    });
    let mut lines = vec![body(library_label(&library.name)).text_size(TEXT).lines(1).min_w(0)];
    if !library.vendor.is_empty() {
        lines.push(caption(library.vendor.clone()).fill(Role::Dim).lines(1).min_w(0));
    }
    lines.push(caption(facts.join(" · ")).fill(Role::Dim).lines(1).min_w(0));
    col(lines).gap(2).align(Align::Start).pad(edges(TIGHT, INSET, SPACE, INSET)).shrink(0)
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
        caption(format!("Scanning · {folders} folders · {found}")).fill(Role::Dim).lines(1).flex(1).min_w(0),
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
        caption(
            "Add the folder that holds your Kontakt libraries: each library in it is found, \
             with or without a library file. Or add one library's own folder."
        )
        .fill(Role::Dim)
        .lines(5),
        many_el,
        one_el,
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

/// The divider between the panes: drag it to share out the height, double-
/// click to reset. The saved state takes it when let go.
fn split_divider(ui: &mut Ui, cx: &mut Cx) -> El {
    let id = "browser-split";
    let r = ui.get(id);
    let height = ui
        .scene()
        .and_then(|s| s.surface("browser"))
        .map_or(600., |s| s.frame.size.height);
    if r.dragged {
        cx.state.split = (cx.state.split + r.drag_delta.y / height.max(1.)).clamp(SPLIT_MIN, SPLIT_MAX);
    }
    if r.double_clicked {
        cx.state.split = SPLIT;
    }
    if r.released || r.double_clicked {
        cx.selection.browser_split = cx.state.split as f32;
    }
    let lift = ui.state(id).hover.max(if r.held { 1. } else { 0. }) as f32;
    canvas(move |s| {
        let t = if lift > 0.5 { 2. } else { 1. };
        vec![Draw::fill(
            rect(0., ((s.height - t) / 2.).round(), s.width, t),
            Role::Ink.alpha(0.08 + 0.25 * lift),
        )]
    })
    .w(Len::Pct(100.))
    .h(5)
    .shrink(0)
    .cursor(Cursor::ResizeV)
    .tip("Drag to share out the height, double-click to reset")
    .named("Resize the library list")
    .id(id)
}

/// The search field with its glyph, placeholder and clear button. The arrows
/// and Enter still work from inside it.
fn search_field(ui: &mut Ui, cx: &mut Cx, multis: bool, libraries: bool) -> El {
    let id = if libraries {
        "library-search"
    } else {
        "search"
    };
    let query = if libraries {
        &mut cx.state.library_search
    } else {
        &mut cx.state.search
    };
    if ui.keys(id).iter().any(|k| k.key == Key::Escape) {
        query.clear();
    }
    let field = text_input(ui, id, query);
    let placeholder = query.is_empty() && !ui.focused(id);
    let (clear, clear_el) = icon_button(
        ui,
        format!("{id}-clear"),
        Icon::Close,
        "Clear the search",
        false,
    );
    if clear {
        query.clear();
    }
    let mut layers = vec![
        field
            .el
            .w(Len::Pct(100.))
            .h(CONTROL + TIGHT)
            .pad(edges(0., CONTROL + TIGHT, 0., CONTROL))
            .named(if libraries { "Search libraries" } else { "Search presets" }),
        glyph(Icon::Search, TEXT + 2., Role::Ink.alpha(0.45))
            .anchor(Align::Start, Align::Center)
            .offset(SPACE, 0.),
        row![
            caption(if libraries {
                "Search libraries"
            } else if multis {
                "Search multis"
            } else {
                "Search instruments"
            })
            .fill(Role::Dim)
        ]
        .align(Align::Center)
        .pad(edges(0., 0., 0., CONTROL + TIGHT))
        .h(CONTROL + TIGHT)
        .when(!placeholder, |e| e.opacity(0.))
        .disabled(),
    ];
    if !query.is_empty() {
        layers.push(clear_el.anchor(Align::End, Align::Center));
    }
    stack(layers).w(Len::Pct(100.)).h(CONTROL + TIGHT).shrink(0)
}

/// Up and Down move the cursor along `listed`, Enter loads it.
fn walk(ui: &mut Ui, cx: &mut Cx, listed: &[PathBuf]) {
    let from_search = ui.focused("search");
    let free = ui.focus_key().is_none_or(|k| k.starts_with("instrument-"));
    let keys: Vec<KeyPress> = if from_search {
        ui.keys("search").to_vec()
    } else if free {
        ui.shortcuts().to_vec()
    } else {
        Vec::new()
    };
    if listed.is_empty() {
        return;
    }
    let at = cx
        .state
        .cursor
        .as_ref()
        .and_then(|c| listed.iter().position(|p| p.to_string_lossy() == *c));
    for k in keys {
        let next = match (k.key, at) {
            (Key::Home, _) => Some(0),
            (Key::End, _) => Some(listed.len() - 1),
            (Key::Down, None) => Some(0),
            (Key::Down, Some(n)) => Some((n + 1).min(listed.len() - 1)),
            (Key::Up, Some(n)) => Some(n.saturating_sub(1)),
            (Key::Up, None) => Some(listed.len() - 1),
            (Key::Enter, Some(n)) if from_search || ui.focus_key().is_none() => {
                cx.open(&listed[n].clone());
                None
            }
            (Key::Enter, None) if from_search => {
                cx.open(&listed[0].clone());
                None
            }
            _ => None,
        };
        if let Some(n) = next {
            cx.state.cursor = Some(listed[n].to_string_lossy().into_owned());
            if !from_search {
                ui.focus(format!("instrument-{n}"));
            }
        }
    }
}

/// A list section's heading: it sticks to the top while its section scrolls.
fn group_heading(label: &str, count: usize) -> El {
    row![
        section(label).flex(1),
        caption(count.to_string()).text_size(SMALL).fill(Role::Dim)
    ]
    .gap(SPACE)
    .align(Align::Center)
    .pad((INSET, TIGHT + 1.))
    .fill(Role::Surface)
    .sticky()
    .shrink(0)
}

/// One preset row: click selects, double-click or Enter loads, drag drops it
/// onto the rack, right-click opens its menu. A star at its end, shown on
/// hover and kept once set, makes it a favorite.
fn preset(ui: &mut Ui, cx: &mut Cx, n: usize, path: &Path) -> El {
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
        Role::Ink.alpha(if favorite { 0.85 } else { 0.4 + 0.5 * star_hover }),
    )
    .centered()]
    .square(TEXT + 2.)
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
    if r.double_clicked || r.key_activated {
        cx.state.cursor = Some(text.clone());
        cx.open(path);
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
        "Double-click to load this multi into the rack"
    } else {
        "Double-click to load, drag onto the rack"
    };
    let el = row![
        block(1, TEXT).fill(if loaded {
            Fill::from(accent())
        } else {
            Role::Ink.alpha(0.)
        }),
        body(without_library(&stem(path), &cx.library_of(path)).to_owned())
            .text_size(TEXT)
            .fill(if loaded || cursor { Role::Ink } else { Role::Dim })
            .lines(1)
            .flex(1)
            .min_w(0),
        match loading {
            Some(done) => super::rack::load_chip(done, false),
            None => star,
        }
    ]
    .gap(INSET - 1.)
    .align(Align::Center)
    .pad((SPACE, TIGHT + 1.))
    .when(cursor, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(stem(path))
    .tip(format!("{}\n{verb}", path.display()))
    .id(id)
    .shrink(0);
    interactive(el, cursor)
}

/// The folders between a library and a preset, without "Instruments"/"Multis".
fn subfolder(library: &Path, path: &Path) -> String {
    let folder = path.parent().and_then(|p| p.strip_prefix(library).ok());
    folder
        .into_iter()
        .flat_map(|f| f.components())
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|c| !matches!(c.to_lowercase().as_str(), "instruments" | "multis"))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn hint(text: &str) -> El {
    col![body(text).fill(Role::Dim).text_size(TEXT).lines(4)]
        .pad(edges(SPACE, INSET, SPACE, INSET))
        .shrink(0)
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
