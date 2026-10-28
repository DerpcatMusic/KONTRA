//! The browser: one search, the format, a star filter, and one list that
//! drills from libraries into a library's presets. Its one action is to
//! pick an instrument.

use mui::prelude::*;

use super::{Action, App, Format};
use crate::kit::{Icon, field, icon_button, list_row, rule, tab};
use crate::theme::{SPACE, TIGHT, secondary};

pub(super) fn view(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let v = &mut app.view;
    let hint = if v.library.is_some() {
        "Search presets"
    } else {
        "Search libraries"
    };
    let search = field(ui, "search", hint, &mut v.search);
    let mut formats = Vec::new();
    for (format, label) in [(Format::Kontakt, "Kontakt"), (Format::Uvi, "UVI")] {
        let t = tab(ui, format!("format-{label}"), label, v.format == format);
        if t.changed && v.format != format {
            (v.format, v.library) = (format, None);
            v.search.clear();
        }
        formats.push(t.el);
    }
    let star = icon_button(ui, "starred", Icon::Star, "Starred only", v.starred_only);
    if star.changed {
        v.starred_only = !v.starred_only;
    }
    formats.extend([spacer(), star.el]);
    let list = match v.library {
        Some(lib) => presets(ui, app, lib, out),
        None => libraries(ui, app),
    };
    col![
        col![search.el, row(formats).align(Align::Center).gap(0)]
            .gap(TIGHT)
            .pad(SPACE),
        rule(),
        list.grow(1).min_h(0),
    ]
    .h(Len::Pct(100.))
}

fn matches(name: &str, search: &str) -> bool {
    search.is_empty() || name.to_lowercase().contains(&search.to_lowercase())
}

fn libraries(ui: &mut Ui, app: &mut App) -> El {
    let v = &app.view;
    let shown: Vec<(usize, &super::Library)> = app
        .libraries
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            l.format == v.format && (l.starred || !v.starred_only) && matches(&l.name, &v.search)
        })
        .collect();
    if shown.is_empty() {
        let why = if app.libraries.is_empty() {
            "No libraries yet. Add a folder in Settings."
        } else {
            "Nothing matches."
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
                false,
            );
            if r.changed {
                picked = Some(n);
            }
            r.el
        })
        .collect();
    if let Some(n) = picked {
        app.view.library = Some(n);
        app.view.search.clear();
    }
    col(rows).scroll().min_h(0)
}

fn presets(ui: &mut Ui, app: &mut App, lib: usize, out: &mut Vec<Action>) -> El {
    let Some(l) = app.libraries.get(lib) else {
        app.view.library = None;
        return spacer();
    };
    let back = icon_button(ui, "browser-back", Icon::Left, "All libraries", false);
    let title_row = row![
        back.el,
        body(l.name.clone())
            .text_weight(Weight::SEMIBOLD)
            .lines(1)
            .min_w(0)
    ]
    .gap(TIGHT)
    .align(Align::Center)
    .pad((0., SPACE));
    let loaded: Vec<&str> = app.parts.iter().map(|p| p.name.as_str()).collect();
    let rows: Vec<El> = l
        .presets
        .iter()
        .enumerate()
        .filter(|(_, p)| matches(p, &app.view.search))
        .map(|(n, p)| {
            let r = list_row(
                ui,
                format!("preset-{n}"),
                (p, ""),
                None,
                loaded.contains(&p.as_str()),
            );
            if r.changed {
                out.push(Action::Load {
                    library: lib,
                    preset: n,
                });
            }
            r.el
        })
        .collect();
    if back.changed {
        app.view.library = None;
        app.view.search.clear();
    }
    col![title_row, col(rows).scroll().min_h(0).grow(1)]
}
