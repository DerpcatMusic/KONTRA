//! The browser, as v2 laid it out: the libraries above (search, order,
//! filter, format, favorites, recent), the chosen library's instruments
//! below as a folder tree. Its one action is to pick an instrument.

use mui::prelude::*;

use super::{Action, App, Format, Shelf};
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

fn recent(ui: &mut Ui, app: &App, out: &mut Vec<Action>) -> El {
    let rows: Vec<El> = app
        .recent
        .iter()
        .enumerate()
        .filter_map(|(n, &(library, preset))| {
            let l = app.libraries.get(library)?;
            let name = l.presets.get(preset)?;
            let r = list_row(ui, format!("recent-{n}"), (name, &l.name), None, false);
            if r.changed {
                out.push(Action::Load { library, preset });
            }
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
            let r = list_row(
                ui,
                format!("preset-{n}"),
                (name, ""),
                None,
                loaded.contains(&name),
            );
            if r.changed {
                out.push(Action::Load {
                    library: lib,
                    preset: n,
                });
            }
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
    col![
        search.el,
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
