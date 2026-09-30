//! The browser: every library as a card, one library's presets under a
//! sticky header, or search results across all libraries, grouped by library.
//! Click selects, double-click or Enter loads, drag drops onto the rack,
//! right-click offers the rest; the arrows walk the list.

use super::{Cx, RackDrag, menu, theme::*};
use crate::import;
use moose::mui::mui::prelude::*;
use moose::mui::mui::scene::Fit;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn sidebar(ui: &mut Ui, cx: &mut Cx) -> El {
    let view = cx.view;
    let multis = cx.state.multis;
    let mut libraries = BTreeMap::<String, Vec<&PathBuf>>::new();
    for file in view.files.iter().filter(|f| import::is_multi(f) == multis) {
        libraries.entry(cx.library_of(file)).or_default().push(file);
    }

    let (hide, hide_el) = icon_button(ui, "browser-hide", Icon::Left, "Hide the browser", false);
    if hide {
        cx.state.browser = false;
    }
    let search = search_field(ui, cx, multis);

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

    let needle = cx.state.search.to_lowercase();
    let mut items = Vec::new();
    // Presets in the order they are listed, for the arrow keys.
    let mut listed: Vec<PathBuf> = Vec::new();
    let mut heading = None;
    let mut count = view.files.len();
    let open = cx
        .state
        .library
        .clone()
        .filter(|l| libraries.contains_key(l));
    if !needle.is_empty() {
        count = 0;
        for (name, files) in &libraries {
            let hits: Vec<_> = files
                .iter()
                .filter(|f| stem(f).to_lowercase().contains(&needle))
                .collect();
            if hits.is_empty() {
                continue;
            }
            count += hits.len();
            let mut group = vec![group_heading(&library_label(name), hits.len())];
            for path in hits {
                group.push(preset(ui, cx, listed.len(), path));
                listed.push((*path).clone());
            }
            items.push(col(group).gap(0).shrink(0));
        }
        if count == 0 {
            items.push(hint("Nothing matches that search."));
        }
    } else if let Some(open) = open {
        let (back, back_el) = icon_button(ui, "library-back", Icon::Left, "All libraries", false);
        if back {
            cx.state.library = None;
        }
        count = libraries[&open].len();
        // The library's header stays put while its presets scroll.
        let mut top = vec![
            row![
                back_el,
                body(library_label(&open))
                    .text_size(TEXT + 1.)
                    .text_weight(Weight::SEMIBOLD)
                    .lines(1)
                    .flex(1)
                    .min_w(0),
            ]
            .gap(TIGHT)
            .align(Align::Center)
            .pad((TIGHT, TIGHT))
            .shrink(0),
        ];
        if let Some(image) = view.artwork.get(&open) {
            top.push(artwork(image, CONTROL * 3.));
        }
        top.push(rule());
        heading = Some(col(top).gap(0).shrink(0));
        // Subfolders become sections whose headings stick while they scroll.
        let mut folders: Vec<(String, Vec<&PathBuf>)> = Vec::new();
        for path in &libraries[&open] {
            let here = subfolder(&view.root, &open, path);
            match folders.last_mut() {
                Some((folder, paths)) if *folder == here => paths.push(path),
                _ => folders.push((here, vec![path])),
            }
        }
        for (folder, paths) in folders {
            let mut group = Vec::new();
            if !folder.is_empty() {
                group.push(group_heading(&folder, paths.len()));
            }
            for path in paths {
                group.push(preset(ui, cx, listed.len(), path));
                listed.push(path.clone());
            }
            items.push(col(group).gap(0).shrink(0));
        }
    } else {
        if !cx.state.recent.is_empty() {
            let recent: Vec<PathBuf> = cx.state.recent.iter().map(PathBuf::from).collect();
            let mut group = vec![group_heading("Recent", recent.len())];
            for path in &recent {
                group.push(preset(ui, cx, listed.len(), path));
                listed.push(path.clone());
            }
            items.push(col(group).gap(0).shrink(0));
            items.push(group_heading("Libraries", libraries.len()));
        }
        let mut cards = Vec::new();
        for (idx, (name, files)) in libraries.iter().enumerate() {
            let id = format!("library-{idx}");
            if ui.get(id.as_str()).activated() {
                cx.state.library = Some(name.clone());
            }
            cards.push(card(view.artwork.get(name), name, files.len(), id));
        }
        if !cards.is_empty() {
            items.push(grid(2, cards).gap(SPACE).min_col(SIDEBAR_MIN).pad(SPACE).shrink(0));
        }
        if libraries.is_empty() {
            items.push(hint(if view.files.is_empty() {
                "No libraries found. Choose the folder that holds your Kontakt libraries from the menu (top right)."
            } else if multis {
                "No multis in these libraries."
            } else {
                "No instruments in these libraries."
            }));
        }
    }
    walk(ui, cx, &listed);

    let list_id = format!(
        "browser-{}-{multis}-{needle}",
        cx.state.library.as_deref().unwrap_or("")
    );
    col![
        section_bar(
            "Browser",
            vec![caption(count.to_string()).text_size(SMALL).fill(Role::Dim), hide_el]
        ),
        col![search, row(kinds).gap(INSET + TIGHT).shrink(0)]
            .gap(TIGHT)
            .pad(edges(0., INSET, 0., INSET))
            .shrink(0),
        rule(),
        heading.unwrap_or_else(|| block(0, 0)),
        col(items)
            .gap(0)
            .align(Align::Stretch)
            .pad(edges(0., 0., SPACE, 0.))
            .flex(1)
            .min_h(0)
            .scroll()
            .id(list_id),
    ]
    .gap(0)
    .h(Len::Pct(100.))
    .shrink(0)
    .fill(Role::Surface)
    .clip()
}

/// The search field with its glyph, placeholder and clear button. The arrows
/// and Enter still work from inside it.
fn search_field(ui: &mut Ui, cx: &mut Cx, multis: bool) -> El {
    if ui.keys("search").iter().any(|k| k.key == Key::Escape) {
        cx.state.search.clear();
    }
    let field = text_input(ui, "search", &mut cx.state.search);
    let placeholder = cx.state.search.is_empty() && !ui.focused("search");
    let (clear, clear_el) = icon_button(ui, "search-clear", Icon::Close, "Clear the search", false);
    if clear {
        cx.state.search.clear();
    }
    let mut layers = vec![
        field
            .el
            .w(Len::Pct(100.))
            .h(CONTROL + TIGHT)
            .pad(edges(0., CONTROL + TIGHT, 0., CONTROL))
            .named("Search presets"),
        glyph(Icon::Search, TEXT + 2., Role::Ink.alpha(0.45))
            .anchor(Align::Start, Align::Center)
            .offset(SPACE, 0.),
        row![
            caption(if multis {
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
    if !cx.state.search.is_empty() {
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

/// A library as a card: its artwork over its name and preset count.
fn card(image: Option<&std::sync::Arc<Image>>, name: &str, presets: usize, id: String) -> El {
    let mut parts = Vec::new();
    match image {
        Some(image) => parts.push(artwork(image, CONTROL * 2.5)),
        None => parts.push(
            row![section(&library_label(name))]
                .align(Align::Center)
                .justify(Justify::Center)
                .pad((0, SPACE))
                .h(CONTROL * 2.5)
                .fill(Role::Field)
                .shrink(0),
        ),
    }
    parts.push(
        row![
            body(library_label(name))
                .text_size(TEXT)
                .lines(1)
                .flex(1)
                .min_w(0),
            caption(presets.to_string()).text_size(SMALL).fill(Role::Dim)
        ]
        .align(Align::Center)
        .gap(SPACE)
        .pad((TIGHT, SPACE)),
    );
    let el = col(parts)
        .gap(0)
        .fill(Role::Raised)
        .clip()
        .focusable()
        .a11y(A11y::Button)
        .named(format!("{name}, {presets} presets"))
        .tip(name.to_owned())
        .id(id)
        .shrink(0);
    el.on(State::Hover, |s| s.fill(Role::Level(3)))
        .on(State::FocusVisible, |s| s.stroke(Role::Primary.alpha(0.9)).stroke_width(1))
        .animate_with(quick())
}

/// A list section's heading: it sticks to the top while its section scrolls.
fn group_heading(label: &str, count: usize) -> El {
    row![
        section(label).flex(1),
        caption(count.to_string()).text_size(SMALL).fill(Role::Dim)
    ]
    .gap(SPACE)
    .align(Align::Center)
    .pad((TIGHT + 1., INSET))
    .fill(Role::Surface)
    .sticky()
    .shrink(0)
}

/// One preset row: click selects, double-click or Enter loads, drag drops it
/// onto the rack, right-click opens its menu.
fn preset(ui: &mut Ui, cx: &mut Cx, n: usize, path: &Path) -> El {
    let id = format!("instrument-{n}");
    let text = path.to_string_lossy().into_owned();
    let loaded = cx.selection.parts.iter().any(|p| p.path == text);
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
        body(stem(path))
            .text_size(TEXT)
            .fill(if loaded || cursor { Role::Ink } else { Role::Dim })
            .lines(1)
            .min_w(0)
    ]
    .gap(INSET - 1.)
    .align(Align::Center)
    .pad((TIGHT + 1., SPACE))
    .when(cursor, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(stem(path))
    .tip(format!("{}\n{verb}", path.display()))
    .id(id)
    .shrink(0);
    interactive(el, cursor)
}

fn artwork(image: &std::sync::Arc<Image>, height: f64) -> El {
    block(Len::Pct(100.), height)
        .fill(Fill::Image(image.clone(), Fit::Cover))
        .shrink(0)
}

/// The folders between a library and a preset, without "Instruments"/"Multis".
fn subfolder(root: &str, library: &str, path: &Path) -> String {
    let folder = path
        .parent()
        .and_then(|p| p.strip_prefix(Path::new(root).join(library)).ok());
    folder
        .into_iter()
        .flat_map(|f| f.components())
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|c| !matches!(c.to_lowercase().as_str(), "instruments" | "multis"))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn hint(text: &str) -> El {
    body(text)
        .fill(Role::Dim)
        .text_size(TEXT)
        .lines(4)
        .pad((SPACE, INSET))
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
