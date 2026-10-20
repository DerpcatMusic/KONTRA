//! The preset menu: the list, the search, and what a press on it asks for.
//!
//! The view holds what the menu shows and says what was asked; the plug-in
//! owns the files. A row is a name, a category and a heart. A press on a row
//! loads it, a double click loads it and closes the menu, and the heart marks
//! it a favourite. The foot of the menu saves what is on the dials under the
//! name typed there, or deletes the preset that is selected.

use crate::skin::*;
use crate::theme;
use crate::tokens::{icon, metric, radius, space, ty};
use mui::prelude::*;

/// One line of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub category: String,
    pub favourite: bool,
    /// Shipped with the plug-in: it cannot be deleted or saved over.
    pub factory: bool,
}

/// What the menu and the bar above it show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Presets {
    pub rows: Vec<Row>,
    /// The loaded preset, as an index into `rows`.
    pub current: Option<usize>,
    pub open: bool,
    pub search: String,
    /// The name the foot saves under.
    pub name: String,
    /// The first row the list shows.
    pub top: usize,
}

/// How many rows the list shows at once.
pub const VISIBLE: usize = 7;

/// What the menu asks of the plug-in.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// Load the preset before (-1) or after (1) the current one.
    Step(i32),
    Load(String),
    Favourite(String, bool),
    Save(String),
    Delete(String),
}

impl Presets {
    /// The loaded preset, if there is one.
    pub fn current_row(&self) -> Option<&Row> {
        self.current.and_then(|i| self.rows.get(i))
    }

    /// The title the bar shows.
    pub fn title(&self) -> &str {
        self.current_row().map_or("init", |r| r.name.as_str())
    }

    /// The rows the search lets through, as indices into `rows`.
    pub fn shown(&self) -> Vec<usize> {
        let needle = self.search.trim().to_lowercase();
        (0..self.rows.len())
            .filter(|&i| {
                needle.is_empty()
                    || self.rows[i].name.to_lowercase().contains(&needle)
                    || self.rows[i].category.to_lowercase().contains(&needle)
            })
            .collect()
    }
}

/// The bar's own presses: the pager, the heart, and the plate that opens the
/// menu.
pub fn bar_asks(ui: &Ui, p: &mut Presets) -> Vec<Ask> {
    let mut asks = Vec::new();
    if ui.get("preset-prev").clicked {
        asks.push(Ask::Step(-1));
    } else if ui.get("preset-next").clicked {
        asks.push(Ask::Step(1));
    } else if ui.get("preset-heart").clicked
        && let Some(row) = p.current_row()
    {
        asks.push(Ask::Favourite(row.name.clone(), !row.favourite));
    } else if ui.get("preset-title").clicked || ui.get("preset-bar").clicked {
        p.open = !p.open;
        if p.open {
            p.name = p.current_row().map(|r| r.name.clone()).unwrap_or_default();
        }
    }
    asks
}

/// The menu, over the plot under the toolbar.
pub fn menu(ui: &mut Ui, p: &mut Presets) -> (El, Vec<Ask>) {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let mut asks = Vec::new();
    if !p.open {
        return (block(space::NONE, space::NONE).shrink(0), asks);
    }
    let shown = p.shown();
    p.top = p.top.min(shown.len().saturating_sub(VISIBLE));
    if ui.get("preset-up").clicked {
        p.top = p.top.saturating_sub(1);
    }
    if ui.get("preset-down").clicked {
        p.top = (p.top + 1).min(shown.len().saturating_sub(VISIBLE));
    }

    let mut search = p.search.clone();
    let field = text_input(ui, "preset-search", &mut search);
    if field.changed {
        p.search = search;
        p.top = 0;
    }
    let search_el = field.el;

    let mut lines: Vec<El> = Vec::new();
    for &index in shown.iter().skip(p.top).take(VISIBLE) {
        let row = p.rows[index].clone();
        let id = format!("preset-row-{index}");
        let heart = format!("preset-row-heart-{index}");
        if ui.get(heart.as_str()).clicked {
            asks.push(Ask::Favourite(row.name.clone(), !row.favourite));
        } else if ui.get(id.as_str()).double_clicked {
            asks.push(Ask::Load(row.name.clone()));
            p.open = false;
        } else if ui.get(id.as_str()).clicked {
            asks.push(Ask::Load(row.name.clone()));
            p.name = row.name.clone();
        }
        let chosen = p.current == Some(index);
        lines.push(
            row![
                label(&row.name, ty::BODY, c(t.ink)).grow(1.0),
                label(&row.category, ty::FINE, c(t.ink_quiet)),
                glyph(
                    Icon::Heart,
                    icon::SMALL,
                    if row.favourite { c(t.heart) } else { ca(t.ink, t.veil_soft) },
                )
                .id(heart),
            ]
            .gap(space::M)
            .pad((space::M, space::NONE))
            .h(metric::FIELD_H)
            .align(Align::Center)
            .radius(radius::FIELD)
            .fill(if chosen { c(t.chip) } else { ca(t.ground, 0.0) })
            .id(id),
        );
    }

    // The foot: a name to save under, and the two things to do with it.
    let mut name = p.name.clone();
    let name_field = text_input(ui, "preset-name", &mut name);
    if name_field.changed {
        p.name = name;
    }
    let can_delete = p.current_row().is_some_and(|r| !r.factory);
    if ui.get("preset-save").clicked && !p.name.trim().is_empty() {
        asks.push(Ask::Save(p.name.trim().to_string()));
    }
    if ui.get("preset-delete").clicked
        && can_delete
        && let Some(row) = p.current_row()
    {
        asks.push(Ask::Delete(row.name.clone()));
    }

    let el = col![
        row![
            search_el.grow(1.0),
            glyph(Icon::Down, icon::BASE, c(t.ink)).id("preset-down"),
            glyph(Icon::Left, icon::BASE, c(t.ink))
                .rotation(std::f64::consts::FRAC_PI_2)
                .id("preset-up"),
        ]
        .gap(space::M)
        .align(Align::Center),
        col(lines).gap(space::TIGHT),
        row![
            name_field.el.grow(1.0),
            action_pill("save").tipped("preset-save"),
            if can_delete {
                action_pill("delete").tipped("preset-delete")
            } else {
                block(space::NONE, space::NONE).shrink(0)
            },
        ]
        .gap(space::XS)
        .align(Align::Center),
    ]
    .gap(space::S)
    .pad(space::L)
    .w(metric::PRESET_W)
    .radius(radius::BOX)
    .fill(ca(t.ground, 0.97))
    .shrink(0)
    .id("preset-menu");
    (el, asks)
}
