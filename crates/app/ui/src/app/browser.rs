//! The preset browser: libraries above (search, shelves, sort, filter,
//! unified/separated format view, grid/cards layout) and the selected
//! library's presets below as a collapsible folder tree.

use mui::prelude::*;

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kontra_ui_model::{AssetRef, ImageMeta};

use super::find::{self, found};
use super::{Action, App, Format, LibraryFilter, LibrarySort, LibraryView, Shelf, Tiles, tidy};
use crate::Pictures;
use crate::kit::{self, Icon, button, field, glyph, icon_button, interactive, rule};
use crate::theme::{CONTROL, SIDEBAR, SMALL, SPACE, TEXT, TIGHT, accent, secondary};

pub(super) fn view(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    escape(ui, app);
    let top = libraries(ui, app, out);
    let bottom = presets(ui, app, out);
    let edge = split(ui, app);
    // Percentage height avoids both a 1-frame jump on open/resize and a
    // flex-share collapse when the presets list is taller than the column.
    let top = top.h(Len::Pct(shared(app.view.split) * 100.)).shrink(0);
    col![top, edge, bottom.grow(1).min_h(0)]
        .h(Len::Pct(100.))
        .id("browser-column")
}

/// The library list's default share of the browser's height; 0 uses this.
const SPLIT: f64 = 0.55;
const SPLIT_MIN: f64 = 0.14;
const SPLIT_MAX: f64 = 0.7;

fn column_height(ui: &Ui) -> Option<f64> {
    (ui.scene())
        .and_then(|s| s.surface("browser-column"))
        .map(|s| s.frame.size.height)
        .filter(|&h| h > 0.)
}

fn shared(split: f64) -> f64 {
    if split > 0. {
        split.clamp(SPLIT_MIN, SPLIT_MAX)
    } else {
        SPLIT
    }
}

/// The edge between the libraries and the presets: a drag shares out the
/// height, a double click resets it. A hairline at rest, lit under the hand.
fn split(ui: &mut Ui, app: &mut App) -> El {
    let id = "browser-split";
    let r = ui.get(id);
    let height = column_height(ui).unwrap_or(600.);
    if r.dragged {
        let now = shared(app.view.split) + r.drag_delta.y / height.max(1.);
        app.view.split = now.clamp(SPLIT_MIN, SPLIT_MAX);
    }
    if r.double_clicked {
        app.view.split = 0.;
    }
    let lit = r.hovered || r.held;
    col![block(Len::Pct(100.), 1).fill(if lit {
        Fill::from(accent())
    } else {
        Role::Ink.alpha(0.08)
    })]
    .justify(Justify::Center)
    .w(Len::Pct(100.))
    .h(2. * TIGHT + 1.)
    .shrink(0)
    .cursor(Cursor::ResizeV)
    .tip("Drag to share out the height, double-click to reset")
    .named("Resize the library list")
    .id(id)
}

/// A search hit's letters: the one blue in the browser, on graphite.
const HIT: Color = Color::oklch(0.8, 0.13, 235.);

/// `text` with the words of `query` it holds marked, so a search's hits
/// stand out of the list.
fn marked(text: &str, query: &str, size: f64, ink: Fill) -> El {
    let piece = |s: &str, hit: bool| {
        let t = body(s.to_owned()).text_size(size).lines(1).shrink(0);
        match hit {
            true => t.fill(HIT).text_weight(Weight::SEMIBOLD),
            false => t.fill(ink.clone()),
        }
    };
    let mut pieces = Vec::new();
    let mut at = 0;
    for Range {
        start: from,
        end: to,
    } in find::hits(text, query)
    {
        if let Some(s) = text.get(at..from).filter(|s| !s.is_empty()) {
            pieces.push(piece(s, false));
        }
        if let Some(s) = text.get(from..to) {
            pieces.push(piece(s, true));
        }
        at = to;
    }
    if pieces.is_empty() {
        return body(text.to_owned())
            .text_size(size)
            .fill(ink)
            .lines(1)
            .min_w(0);
    }
    if let Some(s) = text.get(at..).filter(|s| !s.is_empty()) {
        pieces.push(piece(s, false).shrink(1).min_w(0));
    }
    row(pieces).align(Align::Center).min_w(0).clip()
}

/// Escape with no menu or sheet open clears the presets' search, else the
/// libraries'.
fn escape(ui: &Ui, app: &mut App) {
    let v = &mut app.view;
    let pressed = ui.shortcuts().iter().any(|k| k.key == Key::Escape);
    if !pressed || v.menu.is_some() || v.sheet.is_some() {
        return;
    }
    if v.preset_search.is_empty() {
        v.search.clear();
    } else {
        v.preset_search.clear();
    }
}

/// Keeps the row at `top`, `h` tall, in view of scroll area `list`.
fn follow(ui: &mut Ui, list: &str, top: f64, h: f64) {
    let shown = (ui.scene())
        .and_then(|s| s.surface(list))
        .map(|s| s.frame.size.height);
    let Some(shown) = shown else { return };
    let at = ui.scroll(list)[1];
    let y = if top < at {
        top
    } else if top + h > at + shown {
        top + h - shown
    } else {
        return;
    };
    ui.set_scroll(list, [0., y.max(0.)]);
}

/// Text typed on a row goes to its list's search field, the caret after it.
fn type_into(ui: &mut Ui, field: &str, search: &mut String, typed: &str) {
    if typed.is_empty() {
        return;
    }
    search.push_str(typed);
    ui.focus(field);
    let end = search.chars().count();
    ui.request_action(SemanticAction::set_selection(field, end, end));
}

/// Whether `library` belongs to the current view's format scope.
fn in_scope(library: &super::Library, view: LibraryView, format: Format) -> bool {
    match view {
        LibraryView::Unified => true,
        LibraryView::Separated => library.format == format,
    }
}

/// Whether `library` passes the active attribute filter.
fn passes_filter(library: &super::Library, filter: LibraryFilter) -> bool {
    match filter {
        LibraryFilter::All => true,
        LibraryFilter::Playable => {
            !library.protection.is_protected() && !library.presets.is_empty()
        }
        LibraryFilter::HasSnapshots => !library.recalls.is_empty(),
        LibraryFilter::Locked => library.protection.is_protected(),
    }
}

/// Short badge label for a library format in Unified view.
fn format_tag(format: Format) -> &'static str {
    match format {
        Format::Kontakt => "NK",
        Format::Uvi => "UVI",
    }
}

/// Compact shelf selector button in the browser bar.
fn shelf_chip(
    ui: &mut Ui,
    id: &str,
    icon: Option<Icon>,
    label: &str,
    count: Option<usize>,
    active: bool,
    tip: String,
) -> (El, bool) {
    let changed = ui.get(id).activated();
    let tint = if active {
        Fill::from(Role::Ink)
    } else {
        secondary()
    };
    let mut parts = Vec::new();
    if let Some(ic) = icon {
        parts.push(glyph(ic, SMALL, tint.clone()));
    }
    if !label.is_empty() {
        parts.push(caption(label.to_owned()).fill(tint).lines(1));
    }
    if let Some(n) = count {
        parts.push(
            caption(n.to_string())
                .text_size(SMALL * 0.9)
                .fill(secondary())
                .lines(1),
        );
    }
    let line = if active {
        Fill::from(accent())
    } else {
        Role::Ink.alpha(0.)
    };
    let el = col![
        spacer(),
        row(parts).gap(TIGHT * 0.75).align(Align::Center),
        spacer(),
        block(Len::Pct(100.), 1).fill(line),
    ]
    .align(Align::Center)
    .pad((TIGHT * 1.5, 0.))
    .h(CONTROL)
    .shrink(0)
    .when(active, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(tip.clone())
    .tip(tip)
    .id(id.to_owned());
    (interactive(el), changed)
}

/// A format filter: a small pill with its count, lit when current.
fn format_chip(ui: &mut Ui, id: &str, label: &str, count: usize, current: bool) -> (El, bool) {
    let hit = ui.get(id).activated();
    let ink = if current {
        Fill::from(Role::Ink)
    } else {
        secondary()
    };
    let el = row![
        caption(label.to_owned())
            .text_size(SMALL)
            .fill(ink)
            .lines(1),
        caption(count.to_string())
            .text_size(SMALL * 0.9)
            .fill(secondary())
            .lines(1),
    ]
    .gap(TIGHT * 0.75)
    .align(Align::Center)
    .pad((TIGHT * 1.5, TIGHT * 0.5))
    .pill()
    .shrink(0)
    .when(current, |e| {
        e.fill(Role::Raised).stroke(accent()).stroke_width(1)
    })
    .focusable()
    .a11y(A11y::Button)
    .named(format!("{label} ({count})"))
    .id(id.to_owned());
    (interactive(el), hit)
}

fn libraries(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let total_in_scope = app
        .libraries
        .iter()
        .filter(|l| in_scope(l, app.view.library_view, app.view.format))
        .count();
    let starred_count = app
        .libraries
        .iter()
        .filter(|l| in_scope(l, app.view.library_view, app.view.format))
        .map(|l| usize::from(l.starred) + l.favorites.len())
        .sum::<usize>();
    let recent_count = app
        .recent
        .iter()
        .filter(|&&(lib, _)| {
            app.libraries
                .get(lib)
                .is_some_and(|l| in_scope(l, app.view.library_view, app.view.format))
        })
        .count();

    let v = &mut app.view;
    let search = field(ui, "search", "Search libraries and presets", &mut v.search);
    let add = icon_button(ui, "browser-add", Icon::Plus, "Add a library folder", false);
    if add.changed {
        out.push(Action::AddFolder);
    }
    let scanning = app.scan.clone();
    let machine = icon_button(
        ui,
        "browser-scan-machine",
        Icon::Search,
        "Scan my machine: find every library on this computer's drives",
        scanning.as_ref().is_some_and(|s| s.machine),
    );
    if machine.changed {
        out.push(if scanning.is_some() {
            Action::CancelScan
        } else {
            Action::ScanMachine
        });
    }
    let hide = icon_button(ui, "browser-hide", Icon::Left, "Hide the browser", false);
    if hide.changed {
        v.browser_hidden = true;
    }

    // Compact shelf tabs: All, Favorites, Recent.
    let (all_chip, all_hit) = shelf_chip(
        ui,
        "shelf-All",
        None,
        "All",
        (total_in_scope > 0).then_some(total_in_scope),
        v.shelf == Shelf::All,
        format!("All libraries ({total_in_scope})"),
    );
    if all_hit {
        v.shelf = Shelf::All;
    }
    let (fav_chip, fav_hit) = shelf_chip(
        ui,
        "shelf-Favorites",
        Some(Icon::Star),
        "",
        (starred_count > 0).then_some(starred_count),
        v.shelf == Shelf::Favorites,
        format!("Favorites ({starred_count})"),
    );
    if fav_hit {
        v.shelf = if v.shelf == Shelf::Favorites {
            Shelf::All
        } else {
            Shelf::Favorites
        };
    }
    let (recent_chip, recent_hit) = shelf_chip(
        ui,
        "shelf-Recent",
        Some(Icon::Clock),
        "",
        (recent_count > 0).then_some(recent_count),
        v.shelf == Shelf::Recent,
        format!("Recent ({recent_count})"),
    );
    if recent_hit {
        v.shelf = if v.shelf == Shelf::Recent {
            Shelf::All
        } else {
            Shelf::Recent
        };
    }

    // One funnel menu holds both "Sort by" and "Show" (filter) sections.
    let sorts = [
        LibrarySort::NameAsc,
        LibrarySort::NameDesc,
        LibrarySort::MostPresets,
        LibrarySort::FavoritesFirst,
    ];
    let filters = [
        LibraryFilter::All,
        LibraryFilter::Playable,
        LibraryFilter::HasSnapshots,
        LibraryFilter::Locked,
    ];
    let sort_idx = sorts.iter().position(|&s| s == v.sort).unwrap_or(0);
    let filter_idx = filters
        .iter()
        .position(|&f| f == v.library_filter)
        .unwrap_or(0);
    let options = kit::sectioned_menu(
        ui,
        &mut v.menu,
        "filter",
        (Icon::Filter, "Sort and filter libraries"),
        &[
            (
                "Sort by",
                &["Name A–Z", "Name Z–A", "Most presets", "Favorites first"],
                sort_idx,
            ),
            (
                "Show",
                &[
                    "All libraries",
                    "Playable only",
                    "Has snapshots",
                    "Locked only",
                ],
                filter_idx,
            ),
        ],
        v.sort != LibrarySort::NameAsc || v.library_filter != LibraryFilter::All,
    );
    match options.changed {
        Some((0, i)) => v.sort = sorts.get(i).copied().unwrap_or_default(),
        Some((_, i)) => v.library_filter = filters.get(i).copied().unwrap_or_default(),
        None => {}
    }

    // 2-column grid vs full-width cards toggle.
    let (tile_icon, tile_lit, tile_tip) = match v.tiles {
        Tiles::Grid => (Icon::Grid, true, "2-column grid · click for large cards"),
        Tiles::Cards => (Icon::Cards, false, "Large cards · click for 2-column grid"),
    };
    let tiles = icon_button(ui, "browser-tiles", tile_icon, tile_tip, tile_lit);
    if tiles.changed {
        v.tiles = match v.tiles {
            Tiles::Grid => Tiles::Cards,
            Tiles::Cards => Tiles::Grid,
        };
    }

    // Unified vs separated: "All formats" is the unified list, a format tab
    // separates the libraries down to that format.
    let format_bar = {
        let count = |f: Format| app.libraries.iter().filter(|l| l.format == f).count();
        let unified = v.library_view == LibraryView::Unified;
        let mut chips = Vec::new();
        let (all, hit) = format_chip(
            ui,
            "format-All",
            "All formats",
            app.libraries.len(),
            unified,
        );
        if hit {
            v.library_view = LibraryView::Unified;
        }
        chips.push(all);
        for (format, label) in [(Format::Kontakt, "Kontakt"), (Format::Uvi, "Falcon / UVI")] {
            let current = !unified && v.format == format;
            let (chip, hit) = format_chip(
                ui,
                &format!("format-{label}"),
                label,
                count(format),
                current,
            );
            if hit && !current {
                (v.library_view, v.format, v.library) = (LibraryView::Separated, format, None);
            }
            chips.push(chip);
        }
        // Secondary to the shelves above: small pills that wrap onto a
        // second line in a narrow sidebar instead of running off its edge.
        row(chips)
            .wrap()
            .gap(TIGHT * 0.5)
            .line_gap(TIGHT * 0.5)
            .align(Align::Center)
    };

    let list = match app.view.shelf {
        Shelf::Recent => recent(ui, app, out),
        Shelf::Favorites => favorites_view(ui, app, out),
        Shelf::All if app.libraries.is_empty() && app.scan.is_none() => empty(ui, app, out),
        Shelf::All => libraries_grid(ui, app),
    };
    let progress = scanning.map(|s| {
        let cancel = button(ui, "scan-cancel", "Cancel", false);
        if cancel.changed {
            out.push(Action::CancelScan);
        }
        // Where on one line, how far on the next: a long folder name
        // clips its own line, never the counts.
        let (place, counts) = scan_line(&s);
        row![
            col![
                caption(place).fill(secondary()).lines(1).min_w(0),
                caption(counts).fill(secondary()).lines(1).min_w(0),
            ]
            .align(Align::Start)
            .min_w(0)
            .grow(1)
            .id("scan-progress"),
            cancel.el,
        ]
        .gap(SPACE)
        .align(Align::Center)
        .pad(kit::edges(0., SPACE, 0., SPACE))
    });
    // Search, then the shelves (the primary scope) with their view
    // options, then the format filter: one gap between the three.
    let head = col![
        row![search.el.grow(1).min_w(0), machine.el, add.el, hide.el]
            .gap(TIGHT)
            .align(Align::Center),
        row![
            all_chip,
            fav_chip,
            recent_chip,
            spacer(),
            options.el,
            tiles.el
        ]
        .gap(TIGHT)
        .align(Align::Center),
        format_bar,
    ]
    .gap(TIGHT)
    .pad(kit::edges(SPACE, SPACE, SPACE, SPACE));
    col![
        head,
        rule(),
        col(progress.into_iter().collect::<Vec<_>>()),
        list.grow(1).min_h(0),
    ]
}

/// Where a scan is, "Scanning Kontakt…", and what it has counted so far,
/// "1,204 files · 6 libraries".
fn scan_line(s: &super::ScanProgress) -> (String, String) {
    let grouped = |n: usize| {
        let d = n.to_string();
        let mut out = String::new();
        for (i, c) in d.chars().enumerate() {
            if i > 0 && (d.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
        out
    };
    let mut place = vec![
        "Scanning",
        if s.machine { "this machine" } else { "" },
        &s.folder,
    ];
    place.retain(|p| !p.is_empty());
    (
        format!("{}\u{2026}", place.join(" ")),
        format!(
            "{} files \u{b7} {} libraries",
            grouped(s.files),
            grouped(s.found)
        ),
    )
}

/// One scanned folder: its path, an "auto" tag when discovery found it, and
/// how its scan went (in the warning colour when it found nothing or could
/// not read), with the hint on a line of its own.
pub(super) fn scan_folder(f: &super::LibraryFolder, control: Option<El>) -> El {
    let mut top = vec![
        caption(f.path.clone())
            .lines(1)
            .min_w(0)
            .grow(1)
            .tip(f.path.clone()),
    ];
    if f.auto {
        top.push(
            caption("auto")
                .fill(secondary())
                .tip("Found on its own; remove to hide it"),
        );
    }
    top.extend(control);
    let mut lines = vec![row(top).gap(TIGHT).align(Align::Center)];
    if !f.status.is_empty() {
        let status = caption(f.status.clone()).lines(1).min_w(0);
        lines.push(if f.problem {
            row![glyph(Icon::Warning, TEXT, secondary()), status]
                .gap(TIGHT)
                .align(Align::Center)
        } else {
            status.fill(secondary())
        });
    }
    if let Some(hint) = &f.hint {
        lines.push(
            caption(hint.clone())
                .fill(secondary())
                .min_w(0)
                .w(Len::Pct(100.)),
        );
    }
    col(lines).gap(1).w(Len::Pct(100.))
}

/// The browser with no library: why, folder by folder, and what to do.
fn empty(ui: &mut Ui, app: &App, out: &mut Vec<Action>) -> El {
    let scan = button(ui, "empty-scan-machine", "Scan my machine", false);
    if scan.changed {
        out.push(Action::ScanMachine);
    }
    let add = button(ui, "empty-add-folder", "Add folder\u{2026}", false);
    if add.changed {
        out.push(Action::AddFolder);
    }
    let said = if app.folders.is_empty() {
        "No libraries yet. Scan this machine, or add the folder your libraries are in."
    } else {
        "No libraries found in these folders:"
    };
    let mut rows = vec![caption(said).fill(secondary())];
    rows.extend(app.folders.iter().map(|f| scan_folder(f, None)));
    rows.push(row![scan.el, add.el].gap(SPACE));
    col(rows)
        .gap(SPACE)
        .pad(SPACE * 2.)
        .scroll()
        .id("library-empty")
        .min_h(0)
}

/// A library card's width over its height, one to a row: the picture
/// reads large.
const CARD: f64 = 4.;
/// A grid tile's, two to a row: about ten libraries show at once.
const TILE: f64 = 2.75;
/// Outer breathing room around the library grid so edge cards can pop out
/// slightly without clipping.
const GRID_PAD: f64 = 2.;
/// Gutter between library tiles.
const GRID_GAP: f64 = 1.;
/// Elevation pop-out in logical points for selected / hovered cards.
const POP_SELECTED: f64 = 3.5;
const POP_HOVERED: f64 = 3.0;
const POP_SELECTED_HOVERED: f64 = 4.0;
const POP_SELECTED_IDLE_UNDER_HOVER: f64 = 1.8;
const POP_EPSILON: f64 = 0.05;

/// Empty state for the library list, with a quick reset button when search or
/// filters hid everything.
fn empty_libraries(ui: &mut Ui, app: &mut App, label: &str) -> El {
    let filtered = !app.view.search.trim().is_empty()
        || app.view.library_filter != LibraryFilter::All
        || app.view.shelf != Shelf::All;
    let mut items = vec![caption(label.to_owned()).fill(secondary())];
    if filtered {
        let reset = button(ui, "browser-clear-filters", "Clear filters", false);
        if reset.changed {
            app.view.search.clear();
            app.view.library_filter = LibraryFilter::All;
            app.view.shelf = Shelf::All;
        }
        items.push(reset.el);
    }
    col(items).gap(SPACE).align(Align::Start).pad(SPACE * 2.)
}

/// Whether the catalog contains libraries from more than one format, so
/// Unified view can show subtle format tags when helpful.
fn has_multiple_formats(libraries: &[super::Library]) -> bool {
    let Some(first) = libraries.first().map(|l| l.format) else {
        return false;
    };
    libraries.iter().any(|l| l.format != first)
}

/// A library entry ready for display in the grid.
struct ShownLibrary<'a> {
    index: usize,
    library: &'a super::Library,
    name_score: Option<usize>,
    preset_hits: Option<usize>,
}

/// Builds the interactive library grid (with pop-out elevation, drop shadows
/// cast over dimmed neighbors, and hover focus dimming).
fn libraries_grid(ui: &mut Ui, app: &mut App) -> El {
    let Some(content) = library_grid_content(ui, app, false) else {
        let msg = if app.libraries.is_empty() {
            "No libraries yet"
        } else {
            "Nothing matches"
        };
        return empty_libraries(ui, app, msg);
    };
    col![content]
        .scroll()
        .id("library-list")
        .min_h(0)
        .w(Len::Pct(100.))
}

/// Favorites shelf: starred libraries (using the same elevated grid) followed
/// by starred presets filtered by the active search query.
fn favorites_view(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let libs = library_grid_content(ui, app, true);
    let fav_presets = favorites(ui, app, out);
    if libs.is_none() && fav_presets.is_empty() {
        let msg = if app.view.search.trim().is_empty() {
            "Nothing starred yet"
        } else {
            "No matching favorites"
        };
        return empty_libraries(ui, app, msg);
    }
    let mut parts = Vec::new();
    if let Some(grid) = libs {
        parts.push(grid);
    }
    if !fav_presets.is_empty() {
        if !parts.is_empty() {
            parts.push(rule());
        }
        parts.extend(fav_presets);
    }
    col(parts)
        .gap(1)
        .scroll()
        .id("library-list")
        .min_h(0)
        .w(Len::Pct(100.))
}

/// Builds the scrolling content stack of library tiles (normal grid cells plus
/// elevated `.overlay()` cards for selected/hovered tiles).
fn library_grid_content(ui: &mut Ui, app: &mut App, starred_only: bool) -> Option<El> {
    let show_format_badge =
        app.view.library_view == LibraryView::Unified && has_multiple_formats(&app.libraries);
    let query = app.view.search.trim().to_owned();
    let library_view = app.view.library_view;
    let format = app.view.format;
    let filter = app.view.library_filter;
    let sort = app.view.sort;
    let tiles_mode = app.view.tiles;
    let chosen_lib = app.view.library;

    let mut shown: Vec<ShownLibrary<'_>> = (app.libraries.iter())
        .enumerate()
        .filter(|(_, l)| in_scope(l, library_view, format))
        .filter(|(_, l)| !starred_only || l.starred)
        .filter(|(_, l)| passes_filter(l, filter))
        .filter_map(|(index, library)| {
            if query.is_empty() {
                return Some(ShownLibrary {
                    index,
                    library,
                    name_score: None,
                    preset_hits: None,
                });
            }
            let tidy_name = tidy::library(&library.name);
            let name_score = find::score(&[tidy_name, library.name.as_str()], &query);
            if name_score.is_some() {
                return Some(ShownLibrary {
                    index,
                    library,
                    name_score,
                    preset_hits: None,
                });
            }
            let hits = (library.presets.iter())
                .filter(|p| found(&[p.as_str()], &query))
                .count();
            (hits > 0).then_some(ShownLibrary {
                index,
                library,
                name_score: None,
                preset_hits: Some(hits),
            })
        })
        .collect();

    if shown.is_empty() {
        return None;
    }

    // Apply user sort order first, then stably promote better search matches
    // when a query is active.
    shown.sort_by(|a, b| {
        let cmp_name = || {
            tidy::library(&a.library.name)
                .to_lowercase()
                .cmp(&tidy::library(&b.library.name).to_lowercase())
        };
        let base = match sort {
            LibrarySort::NameAsc => cmp_name(),
            LibrarySort::NameDesc => cmp_name().reverse(),
            LibrarySort::MostPresets => b
                .library
                .presets
                .len()
                .cmp(&a.library.presets.len())
                .then_with(cmp_name),
            LibrarySort::FavoritesFirst => b
                .library
                .starred
                .cmp(&a.library.starred)
                .then_with(cmp_name),
        };
        if query.is_empty() {
            return base;
        }
        match (a.name_score, b.name_score) {
            (Some(sa), Some(sb)) => sb.cmp(&sa).then(base),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => b
                .preset_hits
                .unwrap_or(0)
                .cmp(&a.preset_hits.unwrap_or(0))
                .then(base),
        }
    });

    let (per_row, ratio): (usize, f64) = match tiles_mode {
        Tiles::Grid => (2, TILE),
        Tiles::Cards => (1, CARD),
    };
    let list_w = ui
        .scene()
        .and_then(|s| s.surface("library-list"))
        .map(|s| s.frame.size.width)
        .filter(|&w| w > 1.)
        .unwrap_or(SIDEBAR);
    let inner_w = (list_w - 2. * GRID_PAD).max(1.);
    let cell_w = (inner_w - GRID_GAP * (per_row.saturating_sub(1)) as f64) / per_row as f64;
    let height = SIDEBAR / per_row as f64 / ratio;

    let ids: Vec<String> = shown
        .iter()
        .map(|s| format!("library-{}", s.index))
        .collect();

    // Detect hover across all visible library cards up front so every non-
    // hovered sibling can smoothly dim in the same frame.
    let hovered_lib = shown
        .iter()
        .zip(&ids)
        .find_map(|(s, id)| ui.get(id.as_str()).hovered.then_some(s.index));
    let any_hovered = hovered_lib.is_some();
    let any_selected = chosen_lib.is_some_and(|c| shown.iter().any(|s| s.index == c));

    let mut picked = None;
    let mut focus = (ui.keys("search").iter())
        .any(|k| k.key == Key::Down)
        .then_some(0);
    let mut typed = String::new();
    let mut into_presets = false;

    let mut covers = app.covers.as_mut();
    let mut grid_cells = Vec::with_capacity(shown.len());
    // (z_rank, overlay_element) so hovered always paints above chosen, and
    // both paint above every dimmed sibling in the grid.
    let mut elevated_overlays: Vec<(u8, El)> = Vec::new();

    for (k, (entry, id)) in shown.iter().zip(&ids).enumerate() {
        let n = entry.index;
        let l = entry.library;
        let r = ui.get(id.as_str());
        if r.activated() {
            picked = Some(n);
        }
        if r.key_activated {
            into_presets = true;
        }
        for key in ui.keys(id.as_str()) {
            match (key.key, tiles_mode) {
                (Key::Right, Tiles::Cards) => (picked, into_presets) = (Some(n), true),
                (Key::Right, Tiles::Grid) => focus = Some(k + 1),
                (Key::Left, Tiles::Grid) => focus = k.checked_sub(1),
                (Key::Down, _) => focus = Some(k + per_row),
                (Key::Up, _) => focus = k.checked_sub(per_row),
                _ => {}
            }
        }
        typed.push_str(ui.text(id.as_str()));

        let chosen = chosen_lib == Some(n);
        let hovered = hovered_lib == Some(n);
        let target_pop = match (chosen, hovered, any_hovered) {
            (true, true, _) => POP_SELECTED_HOVERED,
            (true, false, false) => POP_SELECTED,
            (true, false, true) => POP_SELECTED_IDLE_UNDER_HOVER,
            (false, true, _) => POP_HOVERED,
            (false, false, _) => 0.,
        };
        let target_dim = if any_hovered {
            if hovered {
                0.
            } else if chosen {
                0.18
            } else {
                0.42
            }
        } else if any_selected {
            if chosen { 0. } else { 0.32 }
        } else {
            0.
        };
        let pop = ui.tween(format!("lib-pop-{n}"), target_pop).max(0.);
        let dim = ui.tween(format!("lib-dim-{n}"), target_dim).clamp(0., 0.75);

        let image = covers.as_deref_mut().and_then(|c| cover(c, n, &l.art));
        let count = entry
            .preset_hits
            .map_or_else(|| l.presets.len().to_string(), |h| format!("{h} found"));
        let name = tidy::library(&l.name);
        let format_badge = show_format_badge.then(|| format_tag(l.format));

        if pop > POP_EPSILON {
            // Keep an unnamed spacer in the grid slot so layout never shifts,
            // and render the actual interactive card in the local overlay layer
            // slightly enlarged with drop shadows cast over dimmed siblings.
            grid_cells.push(block(cell_w, height).shrink(0));
            let popped_w = cell_w + 2. * pop;
            let popped_h = height + 2. * pop;
            let col_idx = k % per_row;
            let row_idx = k / per_row;
            let cell_x = GRID_PAD + col_idx as f64 * (cell_w + GRID_GAP);
            let cell_y = GRID_PAD + row_idx as f64 * (height + GRID_GAP);
            let x = (cell_x - pop).clamp(0.5, (list_w - popped_w - 0.5).max(0.5));
            let y = (cell_y - pop).max(0.5);
            let el = card(
                id,
                (name, &count),
                &l.protection,
                format_badge,
                image,
                CardState {
                    chosen,
                    hovered,
                    height: popped_h,
                    pop,
                    dim,
                },
                &query,
            )
            .w(popped_w)
            .h(popped_h)
            .at(x, y)
            .overlay();
            let z_rank = if hovered {
                2
            } else if chosen {
                1
            } else {
                0
            };
            elevated_overlays.push((z_rank, el));
        } else {
            grid_cells.push(
                card(
                    id,
                    (name, &count),
                    &l.protection,
                    format_badge,
                    image,
                    CardState {
                        chosen,
                        hovered,
                        height,
                        pop: 0.,
                        dim,
                    },
                    &query,
                )
                .w(cell_w)
                .shrink(0),
            );
        }
    }

    let v = &mut app.view;
    type_into(ui, "search", &mut v.search, &typed);
    if let Some(k) = focus.filter(|&k| k < ids.len()) {
        if let Some(id) = ids.get(k) {
            ui.focus(id.as_str());
        }
        let step = height + GRID_GAP;
        follow(
            ui,
            "library-list",
            GRID_PAD + (k / per_row) as f64 * step,
            step,
        );
    }
    if let Some(n) = picked {
        if v.library != Some(n) {
            v.preset_search.clear();
        }
        v.library = Some(n);
        let last = v.last.iter().find(|l| l.0 == n).copied();
        v.picked = last.or(v.picked.filter(|p| p.0 == n));
        v.reveal = true;
    }
    if into_presets {
        ui.focus("preset-search");
    }

    let mut rows = Vec::new();
    let mut cells = grid_cells.into_iter().peekable();
    while cells.peek().is_some() {
        let pair: Vec<El> = cells.by_ref().take(per_row).collect();
        rows.push(row(pair).gap(GRID_GAP).w(Len::Pct(100.)).shrink(0));
    }

    elevated_overlays.sort_by_key(|(z, _)| *z);
    let mut layers = vec![
        col(rows)
            .gap(GRID_GAP)
            .pad(GRID_PAD)
            .w(Len::Pct(100.))
            .shrink(0),
    ];
    layers.extend(elevated_overlays.into_iter().map(|(_, el)| el));
    Some(stack(layers).w(Len::Pct(100.)).shrink(0))
}

/// The presets the player starred, across the current view's libraries and
/// filtered by the active search query.
fn favorites(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> Vec<El> {
    let into = focused(&app.parts);
    let loading = app.loading.is_some();
    let query = app.view.search.trim().to_owned();
    let library_view = app.view.library_view;
    let format = app.view.format;
    let v = &mut app.view;
    let mut rows = Vec::new();
    for (library, l) in app.libraries.iter().enumerate() {
        if !in_scope(l, library_view, format) {
            continue;
        }
        let lib = tidy::library(&l.name);
        for &preset in &l.favorites {
            let Some(path) = l.presets.get(preset) else {
                continue;
            };
            let name = tidy::name(&l.name, path);
            if !query.is_empty() && !found(&[name, lib, path], &query) {
                continue;
            }
            let id = format!("favorite-{library}-{preset}");
            let at = (library, preset);
            let el = searchable_row(&id, (name, lib), &query, v.picked == Some(at));
            pick(
                ui,
                &id,
                (&mut v.picked, &mut v.clicked),
                at,
                into,
                loading,
                out,
            );
            rows.push(el);
        }
    }
    rows
}

/// A list row that highlights search hits in both its label and library detail.
fn searchable_row(id: &str, (label, detail): (&str, &str), query: &str, selected: bool) -> El {
    let mark = block(2, Len::Pct(100.)).fill(if selected {
        Fill::from(accent())
    } else {
        Role::Ink.alpha(0.)
    });
    let ink = if selected {
        Fill::from(Role::Ink)
    } else {
        secondary()
    };
    let el = row![
        mark,
        marked(label, query, TEXT, ink).grow(1).min_w(0),
        marked(detail, query, SMALL, secondary()).shrink(0),
    ]
    .gap(SPACE)
    .align(Align::Center)
    .pad(kit::edges(0., SPACE, 0., 0.))
    .h(CONTROL)
    .w(Len::Pct(100.))
    .when(selected, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(label.to_owned())
    .tip(format!("{label} · {detail}"))
    .id(id.to_owned());
    interactive(el)
}

/// A dark ramp up from a picture's foot, for white text over it.
fn scrim(foot: f32) -> El {
    let black = |a: f32| Color::srgb(0., 0., 0.).with_alpha(a);
    block(Len::Pct(100.), Len::Pct(100.)).fill(Gradient::linear(
        180.,
        [
            (0., black(0.)),
            (0.45, black(foot * 0.35)),
            (1., black(foot)),
        ],
    ))
}

/// A library's cover edge to edge; until it decodes (or when it has none) a
/// dark wash tinted by its name, so tiles stay apart instead of one grey.
fn picture(image: Option<Arc<Image>>, height: f64, name: &str) -> El {
    let tile = block(Len::Pct(100.), height);
    if let Some(image) = image {
        return tile.fill(Fill::Image(image, Fit::Cover));
    }
    let hue = name
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b.into()));
    let [r, g, b] = [0, 8, 16].map(|s| 0.14 + 0.16 * ((hue >> s) & 0xff) as f32 / 255.);
    tile.fill(Gradient::linear(
        135.,
        [
            (0., Color::srgb(r, g, b)),
            (1., Color::srgb(r * 0.45, g * 0.45, b * 0.45)),
        ],
    ))
}

/// Per-frame visual state of a library card.
struct CardState {
    chosen: bool,
    hovered: bool,
    height: f64,
    pop: f64,
    dim: f64,
}

/// A library tile: its cover picture edge to edge, its name and preset count in
/// white at its foot, optional lock/format badges, dim veil when another card
/// holds focus, and elevated drop shadows + highlight rim when selected or
/// hovered.
fn card(
    id: &str,
    (name, count): (&str, &str),
    protection: &super::Protection,
    format_badge: Option<&str>,
    image: Option<Arc<Image>>,
    state: CardState,
    query: &str,
) -> El {
    let CardState {
        chosen,
        hovered,
        height,
        pop,
        dim,
    } = state;
    let small = height < SIDEBAR / CARD;
    let (size, pad) = if small {
        (SMALL, TIGHT * 1.5)
    } else {
        (TEXT * 1.15, SPACE)
    };
    let mark = block(3, Len::Pct(100.)).fill(if chosen {
        Fill::from(accent())
    } else {
        Role::Ink.alpha(0.)
    });
    let white = Fill::from(Color::srgb(1., 1., 1.));
    let foot = row![
        marked(name, query, size, white).grow(1).min_w(0),
        caption(count.to_owned())
            .text_size(SMALL * 0.9)
            .fill(Color::srgb(0.88, 0.88, 0.88))
            .shrink(0),
    ]
    .gap(TIGHT)
    .align(Align::Center);

    let mut top_badges = Vec::new();
    if let Some(tag) = format_badge {
        top_badges.push(
            row![
                caption(tag.to_owned())
                    .text_size(SMALL * 0.78)
                    .fill(Color::srgb(0.84, 0.84, 0.88))
            ]
            .align(Align::Center)
            .pad((1., 4.))
            .radius(2.)
            .fill(Color::srgb(0.06, 0.06, 0.08).with_alpha(0.72))
            .shrink(0),
        );
    }
    if protection.is_protected() {
        let amber = Color::srgb(0.95, 0.72, 0.28);
        let badge_bg = Color::srgb(0.08, 0.08, 0.1).with_alpha(0.8);
        let icon_size = if small { SMALL * 0.95 } else { TEXT * 0.95 };
        top_badges.push(
            row![glyph(Icon::Lock, icon_size, Fill::from(amber))]
                .align(Align::Center)
                .pad((2., 4.))
                .radius(3.)
                .fill(badge_bg)
                .shrink(0),
        );
    }

    let mut inner = Vec::new();
    let top_pad = if top_badges.is_empty() {
        0.
    } else {
        let mut top_row = vec![spacer()];
        top_row.extend(top_badges);
        inner.push(row(top_row).gap(TIGHT * 0.75).align(Align::Center));
        TIGHT
    };
    inner.push(spacer());
    inner.push(foot);

    let label = row![
        mark,
        col(inner)
            .pad(kit::edges(top_pad, pad, TIGHT * 1.5, pad))
            .grow(1)
            .min_w(0)
            .h(Len::Pct(100.)),
    ]
    .w(Len::Pct(100.))
    .h(Len::Pct(100.));

    // No hover tip: the tile shows its name and count already, and a tip
    // over a grid the pointer sweeps covers the next row. The lock badge
    // says it is locked; why is in its accessible name.
    let a11y_name = match protection {
        super::Protection::Open => format!("{name}, {count} presets"),
        super::Protection::Protected { system, detail } => {
            format!("{name}, {count} presets, locked ({system}): {detail}")
        }
    };

    let mut layers = vec![picture(image, height, name), scrim(0.84), label];
    if dim > 0.005 {
        layers.push(
            block(Len::Pct(100.), Len::Pct(100.))
                .fill(Color::srgb(0.04, 0.04, 0.06).with_alpha(dim as f32)),
        );
    }

    // Clip the image & scrim inside the inner stack so drop shadows on the
    // outer card box cast freely over dimmed siblings.
    let content = stack(layers).w(Len::Pct(100.)).h(Len::Pct(100.)).clip();

    let lift_t = (pop / POP_SELECTED).clamp(0., 1.);
    let mut card_box = stack![content]
        .w(Len::Pct(100.))
        .h(height)
        .cursor(Cursor::Hand)
        .on(State::FocusVisible, |s| {
            s.stroke(Role::Primary.alpha(0.9)).stroke_width(1)
        })
        .focusable()
        .a11y(A11y::Button)
        .named(a11y_name)
        .id(id.to_owned())
        .shrink(0);

    if lift_t > 0.01 {
        let rim_alpha = if chosen {
            0.36 * lift_t
        } else if hovered {
            0.26 * lift_t
        } else {
            0.16 * lift_t
        };
        let contact = Shadow {
            blur: 4. * lift_t,
            dx: 0.,
            dy: 2. * lift_t,
            spread: 0.5 * lift_t,
            fill: Fill::Color(Color::oklcha(0., 0., 0., (0.62 * lift_t) as f32)),
            ..Shadow::soft(4. * lift_t)
        };
        let ambient = Shadow {
            blur: 16. * lift_t,
            dx: 0.,
            dy: 6. * lift_t,
            spread: 1.5 * lift_t,
            fill: Fill::Color(Color::oklcha(0., 0., 0., (0.58 * lift_t) as f32)),
            ..Shadow::soft(16. * lift_t)
        };
        card_box = card_box
            .shadows([ambient, contact])
            .stroke(Color::srgb(1., 1., 1.).with_alpha(rim_alpha as f32))
            .stroke_width(1);
    }
    card_box
}

fn recent(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let into = focused(&app.parts);
    let loading = app.loading.is_some();
    let query = app.view.search.trim().to_owned();
    let library_view = app.view.library_view;
    let format = app.view.format;
    let v = &mut app.view;
    let rows: Vec<El> = app
        .recent
        .iter()
        .enumerate()
        .filter_map(|(n, &(library, preset))| {
            let l = app.libraries.get(library)?;
            if !in_scope(l, library_view, format) {
                return None;
            }
            let path = l.presets.get(preset)?;
            let name = tidy::name(&l.name, path);
            let lib = tidy::library(&l.name);
            if !query.is_empty() && !found(&[name, lib, path], &query) {
                return None;
            }
            let id = format!("recent-{n}");
            let at = (library, preset);
            let el = searchable_row(&id, (name, lib), &query, v.picked == Some(at));
            pick(
                ui,
                &id,
                (&mut v.picked, &mut v.clicked),
                at,
                into,
                loading,
                out,
            );
            Some(el)
        })
        .collect();
    if rows.is_empty() {
        let msg = if query.is_empty() {
            "Nothing loaded yet"
        } else {
            "No matching recent presets"
        };
        return empty_libraries(ui, app, msg);
    }
    col(rows)
        .gap(1)
        .pad(TIGHT)
        .scroll()
        .id("recent-list")
        .min_h(0)
}

/// The chosen library's presets under a compact single-row header (no
/// duplicate cover banner), grouped into collapsible folders.
fn presets(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let loading = app.loading.is_some();
    let load_label = format!("{} %", app.load_progress.percent);
    let into = focused(&app.parts);
    let v = &mut app.view;
    let Some((lib, l)) = v
        .library
        .and_then(|n| app.libraries.get_mut(n).map(|l| (n, l)))
    else {
        return col![
            caption("Select a library above to browse its presets")
                .fill(secondary())
                .pad(SPACE * 2.)
        ];
    };

    let search = field(
        ui,
        "preset-search",
        "Search presets and folders",
        &mut v.preset_search,
    );
    let search_el = search.el.pad(kit::edges(TIGHT, SPACE, TIGHT, SPACE));

    let star = icon_button(
        ui,
        "star",
        Icon::Star,
        if l.starred {
            "Unstar library"
        } else {
            "Star library"
        },
        l.starred,
    );
    if star.changed {
        l.starred = !l.starred;
        out.push(Action::Star(lib, l.starred));
    }
    let locate =
        (l.offline.is_some()).then(|| button(ui, "library-locate", "Locate\u{2026}", false));
    if locate.as_ref().is_some_and(|b| b.changed) {
        out.push(Action::Locate(lib));
    }
    // The libraries' search reaches in when it found presets here, not the
    // library itself.
    let query = match v.preset_search.trim() {
        "" if !found(&[tidy::library(&l.name), l.name.as_str()], &v.search) => {
            v.search.trim().to_owned()
        }
        own => own.to_owned(),
    };
    let shown = tidy::presets(&l.name, &l.presets);
    let loaded: Vec<&str> = app.parts.iter().map(|p| p.name.as_str()).collect();

    let mut folders: Vec<Folder<'_>> = Vec::new();
    let mut folder_keys: BTreeSet<String> = BTreeSet::new();
    for (n, s) in shown.iter().enumerate() {
        if !s.crumbs.is_empty() {
            folder_keys.insert(s.crumbs.join("/"));
        }
        let mut texts: Vec<&str> = s.crumbs.iter().map(String::as_str).collect();
        texts.push(&s.name);
        if !found(&texts, &query) {
            continue;
        }
        match folders.iter_mut().find(|f| f.0 == s.crumbs.as_slice()) {
            Some(f) => f.1.push((n, s)),
            None => folders.push((&s.crumbs, vec![(n, s)])),
        }
    }

    // Collapse / expand all folders button in the slim header when the library
    // has folders and no search query is forcing them open.
    let all_folded = !folder_keys.is_empty() && folder_keys.iter().all(|k| v.folded.contains(k));
    let fold_all = (!folder_keys.is_empty() && query.is_empty()).then(|| {
        let (icon, tip) = if all_folded {
            (Icon::UnfoldAll, "Expand all folders")
        } else {
            (Icon::FoldAll, "Collapse all folders")
        };
        let b = icon_button(ui, "preset-fold-all", icon, tip, all_folded);
        if b.changed {
            if all_folded {
                v.folded.retain(|k| !folder_keys.contains(k));
            } else {
                for key in &folder_keys {
                    if !v.folded.contains(key) {
                        v.folded.push(key.clone());
                    }
                }
            }
        }
        b.el
    });

    let matched: usize = folders.iter().map(|f| f.1.len()).sum();
    let mut rows = Vec::new();
    let mut order: Vec<(usize, usize)> = Vec::new();
    let mut fold = None;
    let mut starred = None;
    for (crumbs, items) in &folders {
        let key = crumbs.join("/");
        if !crumbs.is_empty() {
            let id = format!("folder-{key}");
            // A search opens every folder its hits are in.
            let shut = query.is_empty() && v.folded.contains(&key);
            if ui.get(id.as_str()).activated() {
                fold = Some(key.clone());
            }
            rows.push(folder_row(id, crumbs, items.len(), shut));
            if shut {
                continue;
            }
        }
        for &(n, s) in items {
            let id = format!("preset-{n}");
            let at = (lib, n);
            let path = l.presets.get(n).map_or("", String::as_str);
            let file = path.rsplit('/').next().unwrap_or(path);
            let on = loaded.contains(&file) || v.picked == Some(at);
            let busy = loading && v.picked == Some(at);
            let detail = if busy { load_label.as_str() } else { "" };
            order.push((n, rows.len()));
            let indent = if crumbs.is_empty() { SPACE } else { SPACE * 2. };
            let row = preset_row(&id, (&s.name, path, detail), &query, on, indent);
            pick(
                ui,
                &id,
                (&mut v.picked, &mut v.clicked),
                at,
                into,
                loading,
                out,
            );
            let fav = l.favorites.contains(&n);
            let star = (v.picked == Some(at) || fav).then(|| {
                let tip = if fav {
                    "Unstar this preset"
                } else {
                    "Star this preset"
                };
                let b = icon_button(ui, format!("fav-{n}"), Icon::Star, tip, fav);
                if b.changed {
                    starred = Some((n, !fav));
                }
                b.el
            });
            rows.push(match star {
                Some(star) => row![row.grow(1).min_w(0), star].align(Align::Center),
                None => row,
            });
        }
    }
    if let Some((n, on)) = starred {
        l.favorites.retain(|&f| f != n);
        if on {
            l.favorites.push(n);
        }
        out.push(Action::StarPreset(lib, n, on));
    }
    if let Some(f) = fold {
        match v.folded.iter().position(|x| *x == f) {
            Some(at) => {
                v.folded.remove(at);
            }
            None => v.folded.push(f),
        }
    }

    // Keys: Down from the field into the list, Up and Down along it
    // (selecting as they go), Enter loads (`pick`), typing searches.
    let mut to = (ui.keys("preset-search").iter())
        .any(|k| k.key == Key::Down)
        .then_some(0);
    let mut typed = String::new();
    for (k, &(n, _)) in order.iter().enumerate() {
        let id = format!("preset-{n}");
        for key in ui.keys(id.as_str()) {
            match key.key {
                Key::Down => to = Some(k + 1),
                Key::Up => to = k.checked_sub(1),
                _ => {}
            }
        }
        typed.push_str(ui.text(id.as_str()));
    }
    type_into(ui, "preset-search", &mut v.preset_search, &typed);
    if let Some(&(n, row)) = to.and_then(|k| order.get(k)) {
        v.picked = Some((lib, n));
        ui.focus(format!("preset-{n}"));
        follow(ui, "preset-list", row as f64 * CONTROL, CONTROL);
    }
    // A library opened again shows its last pick once its scene is ready, then
    // clears the flag so it never fights the user's subsequent scrolling.
    if v.reveal && ui.scene().is_some() {
        let want = v.picked.filter(|p| p.0 == lib).map(|p| p.1);
        let at = order.iter().find(|o| Some(o.0) == want).or(order.first());
        if let Some(&(_, row)) = at {
            let y = (row as f64 - 3.).max(0.) * CONTROL;
            ui.set_scroll("preset-list", [0., y]);
        }
        v.reveal = false;
    }
    if let Some(p) = v.picked.filter(|p| p.0 == lib) {
        match v.last.iter_mut().find(|l| l.0 == lib) {
            Some(l) => *l = p,
            None => v.last.push(p),
        }
    }

    // Slim single-row header (no duplicate cover picture taking vertical space).
    let amber = Color::srgb(0.95, 0.72, 0.28);
    let count = match query.is_empty() {
        true => l.presets.len().to_string(),
        false => format!("{matched}/{}", l.presets.len()),
    };
    let mut head_items = vec![
        body(tidy::library(&l.name).to_owned())
            .text_size(TEXT)
            .text_weight(Weight::SEMIBOLD)
            .fill(Role::Ink)
            .lines(1)
            .grow(1)
            .min_w(0)
            .tip(l.name.clone()),
        caption(count)
            .fill(secondary())
            .lines(1)
            .shrink(0)
            .tip(format!("{} presets", l.presets.len())),
    ];
    if let super::Protection::Protected { system, detail } = &l.protection {
        let pill = row![
            glyph(Icon::Lock, SMALL * 0.85, Fill::from(amber)),
            caption(system.clone())
                .text_size(SMALL * 0.85)
                .fill(amber)
                .lines(1)
                .min_w(0),
        ]
        .gap(TIGHT * 0.5)
        .align(Align::Center)
        .pad((2., 5.))
        .radius(3.)
        .fill(Color::srgb(0.08, 0.08, 0.1).with_alpha(0.85))
        .tip(detail.clone())
        .shrink(0)
        .id("library-protection-badge");
        head_items.push(pill);
    }
    if let Some(fold_btn) = fold_all {
        head_items.push(fold_btn);
    }
    if let Some(why) = &l.offline {
        head_items.push(
            caption(format!("Offline \u{b7} {why}"))
                .text_size(SMALL * 0.9)
                .fill(amber)
                .lines(1)
                .min_w(0)
                .tip("Library moved or missing: Locate\u{2026} the folder it is in now")
                .id("library-offline-badge"),
        );
    }
    head_items.extend(locate.map(|b| b.el));
    head_items.push(star.el);

    let header = row(head_items)
        .gap(TIGHT)
        .align(Align::Center)
        .pad(kit::edges(TIGHT * 0.5, SPACE, TIGHT * 0.5, SPACE))
        .h(CONTROL + TIGHT)
        .w(Len::Pct(100.))
        .shrink(0)
        .id("preset-library-header");

    let list = match (rows.is_empty(), &l.protection) {
        (true, super::Protection::Protected { system, detail }) if l.presets.is_empty() => col![
            row![
                glyph(Icon::Lock, TEXT, Fill::from(amber)),
                body(format!("Protected library ({system})"))
                    .text_size(TEXT)
                    .text_weight(Weight::SEMIBOLD)
                    .fill(amber)
                    .lines(1)
                    .min_w(0),
            ]
            .gap(TIGHT)
            .align(Align::Center),
            caption(detail.clone()).fill(secondary()),
        ]
        .gap(TIGHT)
        .pad(SPACE * 2.)
        .id("library-protection-notice"),
        (true, _) => {
            let mut empty = vec![caption("Nothing matches").fill(secondary())];
            if !v.preset_search.trim().is_empty() {
                let clear = button(ui, "preset-clear-search", "Clear search", false);
                if clear.changed {
                    v.preset_search.clear();
                }
                empty.push(clear.el);
            }
            col(empty).gap(SPACE).align(Align::Start).pad(SPACE * 2.)
        }
        (false, _) => col(rows).scroll().id("preset-list").min_h(0).grow(1),
    };
    // An offline library's presets stay listed, dimmed.
    let list = if l.offline.is_some() {
        list.opacity(0.5)
    } else {
        list
    };
    col![header, search_el, list]
}

/// A folder's path and its presets, as (index, shown).
type Folder<'a> = (&'a [String], Vec<(usize, &'a tidy::Shown)>);

/// A folder's header: its path as breadcrumbs, the folder itself brightest.
fn folder_row(id: String, crumbs: &[String], count: usize, shut: bool) -> El {
    let chevron = if shut { Icon::Right } else { Icon::Down };
    let mut parts = vec![glyph(chevron, TEXT * 0.8, secondary())];
    if let Some((last, parents)) = crumbs.split_last() {
        // Deep paths keep their two nearest folders; the tooltip has all.
        if parents.len() > 1 {
            parts.push(caption("…").fill(secondary()).shrink(0));
            parts.push(caption("›").fill(secondary()).shrink(0));
        }
        if let Some(p) = parents.last() {
            parts.push(
                caption(p.clone())
                    .fill(secondary())
                    .lines(1)
                    .min_w(0)
                    .shrink(3),
            );
            parts.push(caption("›").fill(secondary()).shrink(0));
        }
        parts.push(
            body(last.clone())
                .text_size(SMALL)
                .text_weight(Weight::SEMIBOLD)
                .lines(1)
                .min_w(0)
                .grow(1),
        );
    }
    parts.push(caption(count.to_string()).fill(secondary()).shrink(0));
    let path = crumbs.join(" › ");
    interactive(
        row(parts)
            .gap(TIGHT)
            .align(Align::Center)
            .h(CONTROL)
            .w(Len::Pct(100.))
            .pad(kit::edges(0., SPACE, 0., SPACE)),
    )
    .focusable()
    .a11y(A11y::Button)
    .named(path.clone())
    .tip(path)
    .id(id)
}

/// A preset's row: its name with the search's hits marked, the load's
/// progress at its end; `path` is its tooltip.
fn preset_row(
    id: &str,
    (name, path, detail): (&str, &str, &str),
    query: &str,
    on: bool,
    indent: f64,
) -> El {
    let mark = block(2, Len::Pct(100.)).fill(if on {
        Fill::from(accent())
    } else {
        Role::Ink.alpha(0.)
    });
    let el = row![
        mark,
        marked(name, query, TEXT, Fill::from(Role::Ink))
            .grow(1)
            .min_w(0),
        caption(detail.to_owned())
            .fill(secondary())
            .lines(1)
            .shrink(0),
    ]
    .gap(indent - 2.)
    .align(Align::Center)
    .pad(kit::edges(0., SPACE, 0., 0.))
    .h(CONTROL)
    .w(Len::Pct(100.))
    .when(on, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(name.to_owned())
    .tip(path.to_owned())
    .id(id.to_owned());
    interactive(el)
}

/// The focused part: the first one open, else the first.
pub(super) fn focused(parts: &[super::Part]) -> Option<usize> {
    (parts.iter().position(|p| p.open)).or_else(|| (!parts.is_empty()).then_some(0))
}

/// A preset on its way from the browser to the rack.
pub(super) struct Carried {
    pub library: usize,
    pub preset: usize,
}

/// mui's double-click window.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// A preset row's pointer: a click selects it, a double-click or Enter loads
/// it (a preset into the focused part `into`), a drag carries it to the rack
/// (onto a part to replace it, onto free space to add one).
fn pick(
    ui: &mut Ui,
    id: &str,
    (picked, clicked): (&mut Option<(usize, usize)>, &mut Option<Instant>),
    (library, preset): (usize, usize),
    into: Option<usize>,
    loading: bool,
    out: &mut Vec<Action>,
) {
    let r = ui.get(id);
    // The second click on the selected row is the double-click: mui's own
    // flag lives for one input intake, which a real host's release clears
    // before the next build reads it.
    let again = r.clicked
        && *picked == Some((library, preset))
        && clicked.is_some_and(|t| t.elapsed() < DOUBLE_CLICK);
    if again || r.key_activated {
        *clicked = None;
        if !loading {
            out.push(Action::Load {
                library,
                preset,
                into,
            });
        }
    } else if r.clicked {
        *picked = Some((library, preset));
        *clicked = Some(Instant::now());
    }
    if r.dragged && ui.dragging::<Carried>().is_none() {
        *picked = Some((library, preset));
        ui.start_drag(id, Carried { library, preset });
    }
}

/// The most cover pictures of one library tried.
const COVERS: usize = 4;

/// The first of a library's cover pictures that has decoded; asks for the
/// rest meanwhile.
pub(super) fn cover(covers: &mut Pictures, lib: usize, art: &[String]) -> Option<Arc<Image>> {
    let meta = ImageMeta::default();
    (art.iter().take(COVERS).enumerate())
        .find_map(|(k, path)| covers.frame(AssetRef(lib * COVERS + k), path, meta, 0, 1.))
}

#[cfg(test)]
mod tests {
    use super::{super::ScanProgress, scan_line};

    #[test]
    fn the_scan_line_says_where_and_how_far() {
        let line = |folder: &str, files, found, machine| {
            let (a, b) = scan_line(&ScanProgress {
                folder: folder.into(),
                files,
                found,
                machine,
            });
            format!("{a} | {b}")
        };
        assert_eq!(
            line("Kontakt", 1204, 6, false),
            "Scanning Kontakt… | 1,204 files · 6 libraries"
        );
        assert_eq!(line("", 0, 0, false), "Scanning… | 0 files · 0 libraries");
        assert_eq!(
            line("UVI", 1_234_567, 1000, true),
            "Scanning this machine UVI… | 1,234,567 files · 1,000 libraries"
        );
    }
}
