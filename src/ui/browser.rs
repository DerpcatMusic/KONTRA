//! The library sidebar: every library as a card, one library's presets, or
//! search results across all libraries, grouped by library.

use super::{Cx, RackDrag, theme::*};
use crate::import;
use moose::mui::mui::prelude::*;
use moose::mui::mui::scene::Fit;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub fn sidebar(ui: &mut Ui, cx: &mut Cx) -> El {
    let view = cx.view;
    let multis = cx.state.multis;
    let mut libraries = BTreeMap::<String, Vec<&PathBuf>>::new();
    for file in view.files.iter().filter(|f| import::is_multi(f) == multis) {
        libraries.entry(cx.library_of(file)).or_default().push(file);
    }

    let field = text_input(ui, "search", &mut cx.state.search);
    let placeholder = cx.state.search.is_empty() && !ui.focused("search");
    let search = stack![
        field
            .el
            .w(Len::Pct(100.))
            .h(32)
            .radius(8)
            .named("Search presets"),
        row![
            caption(if multis {
                "Search multis"
            } else {
                "Search instruments"
            })
            .fill(Role::Dim)
        ]
        .align(Align::Center)
        .pad((GAP + HALF, 0))
        .w(Len::Pct(100.))
        .h(32)
        .when(!placeholder, |e| e.opacity(0.))
        .disabled()
    ]
    .w(Len::Pct(100.))
    .shrink(0);

    let mut kinds = Vec::new();
    for (multi, label, id) in [
        (false, "Instruments", "picker-instruments"),
        (true, "Multis", "picker-multis"),
    ] {
        let (hit, el) = action(ui, id, label, multis == multi);
        if hit {
            cx.state.multis = multi;
        }
        kinds.push(el.flex(1));
    }

    let needle = cx.state.search.to_lowercase();
    let mut items = Vec::new();
    let mut n = 0;
    if !needle.is_empty() {
        for (name, files) in &libraries {
            let hits: Vec<_> = files
                .iter()
                .filter(|f| stem(f).to_lowercase().contains(&needle))
                .collect();
            if hits.is_empty() {
                continue;
            }
            items.push(section(&library_label(name)).pad(edges(GAP, HALF, 0., HALF)));
            for path in hits {
                items.push(preset(ui, cx, n, path));
                n += 1;
            }
        }
        if n == 0 {
            items.push(hint("Nothing matches that search."));
        }
    } else if let Some(open) = cx
        .state
        .library
        .clone()
        .filter(|l| libraries.contains_key(l))
    {
        let (back, back_el) = action(ui, "library-back", "‹  All libraries", false);
        if back {
            cx.state.library = None;
        }
        items.push(row![back_el].shrink(0));
        if let Some(image) = view.artwork.get(&open) {
            items.push(artwork(image, 96.).radius(8).clip());
        }
        items.push(
            row![
                body(library_label(&open))
                    .text_weight(Weight::SEMIBOLD)
                    .lines(2)
                    .flex(1)
                    .min_w(0),
                caption(libraries[&open].len().to_string()).fill(Role::Dim)
            ]
            .align(Align::Center)
            .pad((HALF, GAP))
            .shrink(0),
        );
        for path in &libraries[&open] {
            items.push(preset(ui, cx, n, path));
            n += 1;
        }
    } else {
        for (idx, (name, files)) in libraries.iter().enumerate() {
            let id = format!("library-{idx}");
            if ui.get(id.as_str()).activated() {
                cx.state.library = Some(name.clone());
            }
            let mut card = Vec::new();
            if let Some(image) = view.artwork.get(name) {
                card.push(artwork(image, 64.));
            }
            card.push(
                row![
                    body(library_label(name))
                        .text_size(12)
                        .lines(2)
                        .flex(1)
                        .min_w(0),
                    caption(files.len().to_string()).fill(Role::Dim)
                ]
                .align(Align::Center)
                .gap(GAP)
                .pad((GAP + HALF, GAP)),
            );
            items.push(
                col(card)
                    .gap(0)
                    .fill(Role::Surface)
                    .radius(8)
                    .clip()
                    .focusable()
                    .a11y(A11y::Button)
                    .named(format!("{name}, {} presets", files.len()))
                    .tip(name.clone())
                    .id(id)
                    .on(State::Hover, |s| s.fill(Role::Raised))
                    .shrink(0),
            );
        }
        if libraries.is_empty() {
            items.push(hint(if view.files.is_empty() {
                "No libraries found. Choose the folder that holds your Kontakt libraries under Folders."
            } else if multis {
                "No multis in these libraries."
            } else {
                "No instruments in these libraries."
            }));
        }
    }

    let list_id = format!(
        "browser-{}-{multis}-{needle}",
        cx.state.library.as_deref().unwrap_or("")
    );
    col![
        row![
            section("Library"),
            spacer(),
            caption(view.files.len().to_string()).fill(Role::Dim)
        ]
        .align(Align::Center)
        .pad(edges(WIDE, GAP + HALF, GAP, GAP + HALF))
        .shrink(0),
        col![search, row(kinds).gap(HALF)]
            .gap(GAP)
            .pad(edges(0., GAP, GAP, GAP))
            .shrink(0),
        rule(),
        col(items)
            .gap(GAP)
            .pad(GAP)
            .flex(1)
            .min_h(0)
            .scroll()
            .id(list_id),
    ]
    .gap(0)
    .w(SIDEBAR)
    .shrink(0)
    .fill(Role::Surface)
    .clip()
}

/// One preset row: click to open, drag onto the rack.
fn preset(ui: &mut Ui, cx: &mut Cx, n: usize, path: &PathBuf) -> El {
    let id = format!("instrument-{n}");
    let text = path.to_string_lossy();
    let loaded = cx.selection.parts.iter().any(|p| p.path == text);
    let (hit, el) = action(ui, id.as_str(), &stem(path), loaded);
    if ui.get(id.as_str()).dragged {
        ui.start_drag(id.as_str(), RackDrag::Instrument(text.clone().into_owned()));
    }
    if hit {
        cx.open(path);
    }
    let verb = if import::is_multi(path) {
        "Load multi into the rack"
    } else {
        "Open instrument"
    };
    el.min_w(0)
        .w(Len::Pct(100.))
        .lines(2)
        .min_h(32)
        .shrink(0)
        .tip(format!("{verb}: {}", path.display()))
}

fn artwork(image: &std::sync::Arc<Image>, height: f64) -> El {
    block(Len::Pct(100.), height)
        .fill(Fill::Image(image.clone(), Fit::Cover))
        .shrink(0)
}

fn hint(text: &str) -> El {
    body(text).fill(Role::Dim).text_size(12).lines(4).pad(GAP)
}

fn stem(path: &std::path::Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
