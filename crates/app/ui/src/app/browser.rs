//! The browser, as v2 laid it out: the libraries above (search, order,
//! filter, format, favorites, recent), the chosen library's instruments
//! below as a folder tree. Its one action is to pick an instrument.

use mui::prelude::*;

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kontra_ui_model::{AssetRef, ImageMeta};

use super::find::{self, found};
use super::{Action, App, Format, Shelf, tidy};
use crate::Pictures;
use crate::kit::{self, Icon, field, glyph, icon_button, interactive, list_row, rule, tab};
use crate::theme::{CONTROL, SIDEBAR, SMALL, SPACE, TEXT, TIGHT, accent, secondary};

pub(super) fn view(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    escape(ui, app);
    let top = libraries(ui, app, out);
    let bottom = presets(ui, app, out);
    let edge = split(ui, app);
    // The libraries' share of the column as it was laid out last frame: a
    // share of free space would leave them none, the presets' rows being
    // longer than the column.
    let top = match column_height(ui) {
        Some(h) => top.h(h * shared(app.view.split)).shrink(0),
        None => top.grow(1).min_h(0),
    };
    col![top, edge, bottom.grow(1).min_h(0)]
        .h(Len::Pct(100.))
        .id("browser-column")
}

/// The library list's share of the browser's height, as v2 split it; a
/// split never set (0) is v2's default.
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

/// The edge between the libraries and the presets (v2's `src/ui/browser.rs`
/// `split_divider`): a drag shares out the height, a double click resets
/// it. A hairline at rest, lit under the hand.
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

/// `text` with the words of `query` it holds marked, so a search's hits
/// stand out of the list.
fn marked(text: &str, query: &str, size: f64, ink: Fill) -> El {
    let piece = |s: &str, hit: bool| {
        let t = body(s.to_owned()).text_size(size).lines(1).shrink(0);
        match hit {
            true => t.fill(Role::Primary).text_weight(Weight::SEMIBOLD),
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

fn libraries(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let v = &mut app.view;
    let search = field(ui, "search", "Search libraries and presets", &mut v.search);
    let order = kit::dropdown(
        ui,
        &mut v.menu,
        "sort",
        (None, if v.descending { "Z–A" } else { "A–Z" }, "Order"),
        &["A–Z", "Z–A"],
        Some(usize::from(v.descending)),
    );
    if let Some(o) = order.changed {
        v.descending = o == 1;
    }
    let shelves = [Shelf::All, Shelf::Favorites, Shelf::Recent];
    let at = shelves.iter().position(|&s| s == v.shelf);
    let filter = kit::dropdown(
        ui,
        &mut v.menu,
        "filter",
        (Some(Icon::Filter), "", "Show"),
        &["All libraries", "Favorites", "Recent"],
        at,
    );
    if let Some(&s) = filter.changed.and_then(|s| shelves.get(s)) {
        v.shelf = s;
    }
    let add = icon_button(ui, "browser-add", Icon::Plus, "Add a library folder", false);
    if add.changed {
        out.push(Action::AddFolder);
    }
    let hide = icon_button(ui, "browser-hide", Icon::Left, "Hide the browser", false);
    if hide.changed {
        v.browser_hidden = true;
    }
    let mut formats = Vec::new();
    for (format, label) in [(Format::Kontakt, "Kontakt"), (Format::Uvi, "Falcon / UVI")] {
        let t = tab(ui, format!("format-{label}"), label, v.format == format);
        if t.changed && v.format != format {
            (v.format, v.library) = (format, None);
            v.search.clear();
        }
        formats.push(t.el);
    }
    let format = v.format;
    let count = app.libraries.iter().filter(|l| l.format == format).count();
    formats.extend([spacer(), caption(count.to_string()).fill(secondary())]);
    let starred = app.libraries.iter().filter(|l| l.starred).count();
    let mut shelf_rows = Vec::new();
    for (shelf, icon, label, n) in [
        (Shelf::Favorites, Icon::Star, "Favorites", starred),
        (Shelf::Recent, Icon::Clock, "Recent", app.recent.len()),
    ] {
        let id = format!("shelf-{label}");
        let on = app.view.shelf == shelf;
        if ui.get(id.clone()).activated() {
            app.view.shelf = if on { Shelf::All } else { shelf };
        }
        shelf_rows.push(
            interactive(
                row![
                    stack![glyph(icon, TEXT, secondary()).centered()]
                        .square(CONTROL)
                        .fill(Role::Field),
                    body(label).text_size(TEXT).grow(1),
                    caption(n.to_string()).fill(secondary()),
                ]
                .gap(SPACE)
                .align(Align::Center)
                .h(CONTROL + TIGHT)
                .pad((0., SPACE))
                .when(on, |e| e.fill(Role::Raised)),
            )
            .focusable()
            .a11y(A11y::Button)
            .named(label)
            .id(id),
        );
    }
    let list = if app.view.shelf == Shelf::Recent {
        recent(ui, app, out)
    } else {
        library_list(ui, app)
    };
    col![
        row![
            search.el.grow(1).min_w(0),
            order.el,
            filter.el,
            add.el,
            hide.el
        ]
        .gap(TIGHT)
        .align(Align::Center)
        .pad(kit::edges(SPACE, SPACE, 0., SPACE)),
        row(formats)
            .align(Align::Center)
            .pad(kit::edges(0., SPACE, 0., 0.)),
        rule(),
        col(shelf_rows).pad((TIGHT, 0.)),
        rule(),
        list.grow(1).min_h(0),
    ]
}

/// A library card's width over its height: the picture reads as one, three
/// or four to a list.
const CARD: f64 = 4.;
/// The chosen library's banner over its presets, a little taller.
const BANNER: f64 = 3.;

fn library_list(ui: &mut Ui, app: &mut App) -> El {
    let v = &app.view;
    let query = v.search.trim().to_owned();
    // A library its name finds, or one with presets the search finds (and
    // how many).
    let mut shown: Vec<(usize, &super::Library, Option<usize>)> = (app.libraries.iter())
        .enumerate()
        .filter(|(_, l)| l.format == v.format && (l.starred || v.shelf != Shelf::Favorites))
        .filter_map(|(n, l)| {
            if found(&[l.name.as_str()], &query) {
                return Some((n, l, None));
            }
            let hits = (l.presets.iter())
                .filter(|p| found(&[p.as_str()], &query))
                .count();
            (hits > 0).then_some((n, l, Some(hits)))
        })
        .collect();
    shown.sort_by_cached_key(|(_, l, _)| tidy::library(&l.name).to_lowercase());
    if v.descending {
        shown.reverse();
    }
    if shown.is_empty() {
        let why = if app.libraries.is_empty() {
            "No libraries yet"
        } else {
            "Nothing matches"
        };
        return caption(why).fill(secondary()).pad(SPACE * 2.);
    }
    let ids: Vec<String> = shown.iter().map(|s| format!("library-{}", s.0)).collect();
    let step = SIDEBAR / CARD + 1.;
    let mut picked = None;
    let mut focus = (ui.keys("search").iter())
        .any(|k| k.key == Key::Down)
        .then_some(0);
    let mut typed = String::new();
    let mut covers = app.covers.as_mut();
    let mut rows = Vec::new();
    for (k, (&(n, l, hits), id)) in shown.iter().zip(&ids).enumerate() {
        if ui.get(id.as_str()).activated() {
            picked = Some(n);
        }
        for key in ui.keys(id.as_str()) {
            match key.key {
                Key::Down => focus = Some(k + 1),
                Key::Up => focus = k.checked_sub(1),
                _ => {}
            }
        }
        typed.push_str(ui.text(id.as_str()));
        let image = covers.as_deref_mut().and_then(|c| cover(c, n, &l.art));
        let count = hits.map_or_else(|| l.presets.len().to_string(), |h| format!("{h} found"));
        let name = tidy::library(&l.name);
        rows.push(card(id, name, &count, image, v.library == Some(n), &query));
    }
    let v = &mut app.view;
    type_into(ui, "search", &mut v.search, &typed);
    if let Some(id) = focus.and_then(|k| ids.get(k)) {
        ui.focus(id.as_str());
        follow(ui, "library-list", focus.unwrap_or(0) as f64 * step, step);
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
    col(rows).gap(1).scroll().id("library-list").min_h(0)
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

fn picture(image: Option<Arc<Image>>, height: f64) -> El {
    match image {
        Some(image) => block(Len::Pct(100.), height).fill(Fill::Image(image, Fit::Cover)),
        None => block(Len::Pct(100.), height).fill(Role::Raised),
    }
}

/// A library as Kontakt's browser shows one: its picture edge to edge,
/// cropped to fill, its name and preset count in white at its foot, an
/// accent edge when chosen.
fn card(
    id: &str,
    name: &str,
    count: &str,
    image: Option<Arc<Image>>,
    chosen: bool,
    query: &str,
) -> El {
    let height = SIDEBAR / CARD;
    let mark = block(3, Len::Pct(100.)).fill(if chosen {
        Fill::from(accent())
    } else {
        Role::Ink.alpha(0.)
    });
    let white = Fill::from(Color::srgb(1., 1., 1.));
    let foot = row![
        marked(name, query, TEXT * 1.15, white).grow(1).min_w(0),
        caption(count.to_owned())
            .text_size(SMALL)
            .fill(Color::srgb(0.85, 0.85, 0.85))
            .shrink(0),
    ]
    .gap(TIGHT)
    .align(Align::Center);
    let label = row![
        mark,
        col![spacer(), foot]
            .pad(kit::edges(0., SPACE, TIGHT * 1.5, SPACE))
            .grow(1)
            .min_w(0)
            .h(Len::Pct(100.)),
    ]
    .w(Len::Pct(100.))
    .h(Len::Pct(100.));
    let el = stack![picture(image, height), scrim(0.85), label]
        .w(Len::Pct(100.))
        .h(height)
        .clip()
        .focusable()
        .a11y(A11y::Button)
        .named(format!("{name}, {count} presets"))
        .tip(format!("{name}\nShow its presets below"))
        .id(id.to_owned())
        .shrink(0);
    kit::interactive(el)
}

fn recent(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let into = focused(&app.parts);
    let v = &mut app.view;
    let rows: Vec<El> = app
        .recent
        .iter()
        .enumerate()
        .filter_map(|(n, &(library, preset))| {
            let l = app.libraries.get(library)?;
            let name = l.presets.get(preset)?;
            let id = format!("recent-{n}");
            let at = (library, preset);
            let r = list_row(ui, id.clone(), (name, &l.name), None, v.picked == Some(at));
            pick(ui, &id, (&mut v.picked, &mut v.clicked), at, into, out);
            Some(r.el)
        })
        .collect();
    if rows.is_empty() {
        return caption("Nothing loaded yet")
            .fill(secondary())
            .pad(SPACE * 2.);
    }
    col(rows).gap(1).pad(TIGHT).scroll().min_h(0)
}

/// The chosen library under its banner, its presets foldered by what is
/// left of their paths once the noise every row repeats is gone
/// ([`tidy`]).
fn presets(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let loading = app.loading.is_some();
    let load_label = format!("{} %", app.load_progress.percent);
    let into = focused(&app.parts);
    let v = &mut app.view;
    let search = field(
        ui,
        "preset-search",
        "Search presets and folders",
        &mut v.preset_search,
    );
    let search = search.el.pad(kit::edges(TIGHT, TIGHT, TIGHT, TIGHT));
    let Some((lib, l)) = v
        .library
        .and_then(|n| app.libraries.get_mut(n).map(|l| (n, l)))
    else {
        return col![search];
    };
    let cover = (app.covers.as_mut()).and_then(|c| cover(c, lib, &l.art));
    let star = icon_button(ui, "star", Icon::Star, "Favorite", l.starred);
    if star.changed {
        l.starred = !l.starred;
        out.push(Action::Star(lib, l.starred));
    }
    // The libraries' search reaches in when it found presets here, not the
    // library itself.
    let query = match v.preset_search.trim() {
        "" if !found(&[l.name.as_str()], &v.search) => v.search.trim().to_owned(),
        own => own.to_owned(),
    };
    // ponytail: tidied every frame (one library, a few hundred paths); keep
    // it with the catalog if a huge library shows in the frame time.
    let shown = tidy::presets(&l.name, &l.presets);
    let loaded: Vec<&str> = app.parts.iter().map(|p| p.name.as_str()).collect();
    let mut folders: Vec<Folder<'_>> = Vec::new();
    for (n, s) in shown.iter().enumerate() {
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
    let matched: usize = folders.iter().map(|f| f.1.len()).sum();
    let mut rows = Vec::new();
    // The preset rows in list order, with their row in `rows`.
    let mut order: Vec<(usize, usize)> = Vec::new();
    let mut fold = None;
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
            rows.push(preset_row(&id, (&s.name, path, detail), &query, on, indent));
            pick(ui, &id, (&mut v.picked, &mut v.clicked), at, into, out);
        }
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
    // A library opened again shows its last pick, once its rows are laid
    // out.
    if v.reveal {
        let want = v.picked.filter(|p| p.0 == lib).map(|p| p.1);
        let at = order.iter().find(|o| Some(o.0) == want).or(order.first());
        match at {
            Some(&(n, row)) => {
                let id = format!("preset-{n}");
                if (ui.scene()).is_some_and(|s| s.surface(id.as_str()).is_some()) {
                    v.reveal = false;
                    ui.focus(id);
                    let y = (row as f64 - 3.).max(0.) * CONTROL;
                    ui.set_scroll("preset-list", [0., y]);
                }
            }
            None => v.reveal = false,
        }
    }
    if let Some(p) = v.picked.filter(|p| p.0 == lib) {
        match v.last.iter_mut().find(|l| l.0 == lib) {
            Some(l) => *l = p,
            None => v.last.push(p),
        }
    }
    let count = match query.is_empty() {
        true => format!("{} presets", l.presets.len()),
        false => format!("{matched} of {} presets", l.presets.len()),
    };
    let white = Color::srgb(1., 1., 1.);
    let title = col![
        body(tidy::library(&l.name).to_owned())
            .text_size(TEXT * 1.3)
            .text_weight(Weight::SEMIBOLD)
            .fill(white)
            .lines(1)
            .min_w(0)
            .tip(l.name.clone()),
        caption(count).fill(Color::srgb(0.85, 0.85, 0.85)),
    ]
    .gap(1)
    .align(Align::Start)
    .grow(1)
    .min_w(0);
    let height = SIDEBAR / BANNER;
    let foot = row![title, star.el].gap(TIGHT).align(Align::End);
    let banner = stack![
        picture(cover, height),
        scrim(0.9),
        col![spacer(), foot]
            .pad(kit::edges(0., TIGHT, SPACE, SPACE))
            .w(Len::Pct(100.))
            .h(Len::Pct(100.)),
    ]
    .w(Len::Pct(100.))
    .h(height)
    .clip()
    .shrink(0);
    let list = match rows.is_empty() {
        true => caption("Nothing matches").fill(secondary()).pad(SPACE * 2.),
        false => col(rows).scroll().id("preset-list").min_h(0).grow(1),
    };
    col![banner, search, list]
}

/// A folder's path and its presets, as (index, shown).
type Folder<'a> = (&'a [String], Vec<(usize, &'a tidy::Shown)>);

/// A folder's header: its path as breadcrumbs, the folder itself brightest.
fn folder_row(id: String, crumbs: &[String], count: usize, shut: bool) -> El {
    let chevron = if shut { Icon::Right } else { Icon::Down };
    let mut parts = vec![glyph(chevron, TEXT * 0.8, secondary())];
    if let Some((last, parents)) = crumbs.split_last() {
        for p in parents {
            parts.push(caption(p.clone()).fill(secondary()).lines(1).min_w(0));
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

/// A preset row's pointer, as v2 read it: a click selects it, a
/// double-click or Enter loads it (a preset into the focused part `into`),
/// a drag carries it to the rack (onto a part to replace it, onto free
/// space to add one).
fn pick(
    ui: &mut Ui,
    id: &str,
    (picked, clicked): (&mut Option<(usize, usize)>, &mut Option<Instant>),
    (library, preset): (usize, usize),
    into: Option<usize>,
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
        out.push(Action::Load {
            library,
            preset,
            into,
        });
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
