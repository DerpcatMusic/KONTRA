//! The browser, as v2 laid it out: the libraries above (search, order,
//! filter, format, favorites, recent), the chosen library's instruments
//! below as a folder tree. Its one action is to pick an instrument.

use mui::prelude::*;

use std::sync::Arc;
use std::time::{Duration, Instant};

use kontra_ui_model::{AssetRef, ImageMeta};

use super::{Action, App, Format, Shelf};
use crate::Pictures;
use crate::kit::{self, Icon, field, glyph, icon_button, interactive, list_row, rule, tab};
use crate::theme::{CONTROL, SPACE, TEXT, TIGHT, secondary};

pub(super) fn view(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let top = libraries(ui, app, out);
    let bottom = presets(ui, app, out);
    col![top.grow(1).min_h(0), rule(), bottom.grow(1).min_h(0)].h(Len::Pct(100.))
}

fn matches(name: &str, search: &str) -> bool {
    search.is_empty() || name.to_lowercase().contains(&search.to_lowercase())
}

fn libraries(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let v = &mut app.view;
    let search = field(ui, "search", "Search", &mut v.search);
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

fn library_list(ui: &mut Ui, app: &mut App) -> El {
    let v = &app.view;
    let mut shown: Vec<(usize, &super::Library)> = app
        .libraries
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            l.format == v.format
                && (l.starred || v.shelf != Shelf::Favorites)
                && matches(&l.name, &v.search)
        })
        .collect();
    shown.sort_by_cached_key(|(_, l)| l.name.to_lowercase());
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
    let mut picked = None;
    let rows: Vec<El> = shown
        .iter()
        .map(|&(n, l)| {
            let r = list_row(
                ui,
                format!("library-{n}"),
                (&l.name, &l.presets.len().to_string()),
                None,
                v.library == Some(n),
            );
            if r.changed {
                picked = Some(n);
            }
            r.el
        })
        .collect();
    if let Some(n) = picked {
        app.view.library = Some(n);
        app.view.preset_search.clear();
    }
    col(rows).gap(1).pad(TIGHT).scroll().min_h(0)
}

fn recent(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let view = &mut app.view;
    let rows: Vec<El> = app
        .recent
        .iter()
        .enumerate()
        .filter_map(|(n, &(library, preset))| {
            let l = app.libraries.get(library)?;
            let name = l.presets.get(preset)?;
            let id = format!("recent-{n}");
            let at = (library, preset);
            let r = list_row(
                ui,
                id.clone(),
                (name, &l.name),
                None,
                view.picked == Some(at),
            );
            pick(ui, &id, (&mut view.picked, &mut view.clicked), at, out);
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

/// The chosen library's instruments, foldered by the path inside it.
fn presets(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let loading = app.loading.is_some();
    let v = &mut app.view;
    let search = field(
        ui,
        "preset-search",
        "Search presets and folders",
        &mut v.preset_search,
    );
    let Some((lib, l)) = v
        .library
        .and_then(|n| app.libraries.get_mut(n).map(|l| (n, l)))
    else {
        return col![search.el];
    };
    let cover = (app.covers.as_mut()).and_then(|c| cover(c, lib, &l.art));
    let star = icon_button(ui, "star", Icon::Star, "Favorite", l.starred);
    if star.changed {
        l.starred = !l.starred;
        out.push(Action::Star(lib, l.starred));
    }
    let title = col![
        body(l.name.clone())
            .text_weight(Weight::SEMIBOLD)
            .lines(1)
            .min_w(0)
            .tip(l.name.clone()),
        caption(format!("{} instruments", l.presets.len())).fill(secondary()),
    ]
    .gap(1)
    .align(Align::Start)
    .grow(1)
    .min_w(0);
    let loaded: Vec<&str> = app.parts.iter().map(|p| p.name.as_str()).collect();
    let mut folders: Vec<(&str, Vec<(usize, &str)>)> = Vec::new();
    for (n, path) in l.presets.iter().enumerate() {
        if !matches(path, &v.preset_search) {
            continue;
        }
        let (folder, name) = path.rsplit_once('/').unwrap_or(("Instruments", path));
        match folders.iter_mut().find(|f| f.0 == folder) {
            Some(f) => f.1.push((n, name)),
            None => folders.push((folder, vec![(n, name)])),
        }
    }
    let mut rows = Vec::new();
    let mut fold = None;
    for (folder, items) in &folders {
        let id = format!("folder-{folder}");
        let shut = v.folded.iter().any(|f| f == folder);
        if ui.get(id.clone()).activated() {
            fold = Some((*folder).to_owned());
        }
        rows.push(
            interactive(
                row![
                    glyph(
                        if shut { Icon::Right } else { Icon::Down },
                        TEXT * 0.8,
                        secondary()
                    ),
                    body((*folder).to_owned())
                        .text_size(TEXT)
                        .lines(1)
                        .min_w(0)
                        .grow(1),
                    caption(items.len().to_string()).fill(secondary()),
                ]
                .gap(TIGHT)
                .align(Align::Center)
                .h(CONTROL)
                .pad((0., SPACE)),
            )
            .focusable()
            .a11y(A11y::Button)
            .named((*folder).to_owned())
            .id(id),
        );
        if shut {
            continue;
        }
        for &(n, name) in items {
            let id = format!("preset-{n}");
            let at = (lib, n);
            let shown = loaded.contains(&name) || v.picked == Some(at);
            let busy = loading && v.picked == Some(at);
            let r = list_row(
                ui,
                id.clone(),
                (name, if busy { "Loading…" } else { "" }),
                None,
                shown,
            );
            pick(ui, &id, (&mut v.picked, &mut v.clicked), at, out);
            rows.push(r.el.pad(kit::edges(0., 0., 0., SPACE)));
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
    let banner = cover.map(|img| {
        block(Len::Pct(100.), Len::Auto)
            .aspect(BANNER)
            .fill(Fill::Image(img, Fit::Cover))
            .pad((0., SPACE))
    });
    col![
        search.el,
        col(banner),
        row![title, star.el]
            .gap(TIGHT)
            .align(Align::Center)
            .pad((TIGHT, SPACE)),
        col(rows)
            .pad(kit::edges(0., SPACE, 0., 0.))
            .scroll()
            .min_h(0)
            .grow(1),
    ]
}

/// A preset on its way from the browser to the rack.
pub(super) struct Carried {
    pub library: usize,
    pub preset: usize,
}

/// mui's double-click window.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// A preset row's pointer, as v2 read it: a click selects it, a
/// double-click or Enter loads it into a new part, a drag carries it to
/// the rack (onto a part to replace it, onto free space to add one).
fn pick(
    ui: &mut Ui,
    id: &str,
    (picked, clicked): (&mut Option<(usize, usize)>, &mut Option<Instant>),
    (library, preset): (usize, usize),
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
        out.push(Action::Load { library, preset });
    } else if r.clicked {
        *picked = Some((library, preset));
        *clicked = Some(Instant::now());
    }
    if r.dragged && ui.dragging::<Carried>().is_none() {
        *picked = Some((library, preset));
        ui.start_drag(id, Carried { library, preset });
    }
}

/// A library-browser banner's width over its height (Kontakt's 905 × 99).
const BANNER: f64 = 905. / 99.;
/// The most cover pictures of one library tried.
const COVERS: usize = 4;

/// The first of a library's cover pictures that has decoded; asks for the
/// rest meanwhile.
pub(super) fn cover(covers: &mut Pictures, lib: usize, art: &[String]) -> Option<Arc<Image>> {
    let meta = ImageMeta::default();
    (art.iter().take(COVERS).enumerate())
        .find_map(|(k, path)| covers.frame(AssetRef(lib * COVERS + k), path, meta, 0, 1.))
}
