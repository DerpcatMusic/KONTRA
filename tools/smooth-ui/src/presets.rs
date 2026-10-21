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

/// The reference's fourteen preset categories, in its own order: the menu's
/// category column lists them and a preset's file names one of them.
pub const CATEGORIES: [&str; 14] = [
    "Misc", "Bass", "Cleanup", "Drums", "Guitar", "Imaging", "Keys", "Master bus", "Orchestral",
    "Vocals", "Mastering", "Mixbus", "Sidechain", "Sound Design",
];

/// Which presets the list lets through: the reference's filter choices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Factory,
    User,
    Favourite,
}

impl Filter {
    /// The next choice, as the filter chip steps through them.
    pub fn next(self) -> Self {
        match self {
            Filter::All => Filter::Factory,
            Filter::Factory => Filter::User,
            Filter::User => Filter::Favourite,
            Filter::Favourite => Filter::All,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Factory => "factory",
            Filter::User => "user",
            Filter::Favourite => "favourite",
        }
    }

    fn admits(self, row: &Row) -> bool {
        match self {
            Filter::All => true,
            Filter::Factory => row.factory,
            Filter::User => !row.factory,
            Filter::Favourite => row.favourite,
        }
    }
}

/// One line of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub category: String,
    pub favourite: bool,
    /// Shipped with the plug-in: it cannot be deleted or saved over.
    pub factory: bool,
    /// Pinned as the default that new instances open with.
    pub default: bool,
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
    /// Which presets the list lets through.
    pub filter: Filter,
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
    /// Pin or unpin a preset as the default.
    Default(String, bool),
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

    /// The rows the search and the filter let through, as indices into `rows`.
    pub fn shown(&self) -> Vec<usize> {
        let needle = self.search.trim().to_lowercase();
        (0..self.rows.len())
            .filter(|&i| {
                let row = &self.rows[i];
                self.filter.admits(row)
                    && (needle.is_empty()
                        || row.name.to_lowercase().contains(&needle)
                        || row.category.to_lowercase().contains(&needle))
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

    if ui.get("preset-filter").clicked {
        p.filter = p.filter.next();
        p.top = 0;
    }
    let mut search = p.search.clone();
    let field = text_input(ui, "preset-search", &mut search);
    if field.changed {
        p.search = search;
        p.top = 0;
    }
    let search_el = field.el.fill(c(t.ink)).radius(radius::FIELD);

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
    if ui.get("preset-default").clicked
        && let Some(row) = p.current_row()
    {
        asks.push(Ask::Default(row.name.clone(), !row.default));
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
            text_button(p.filter.name()).tipped("preset-filter"),
            glyph(Icon::Down, icon::BASE, c(t.ink)).id("preset-down"),
            glyph(Icon::Left, icon::BASE, c(t.ink))
                .rotation(std::f64::consts::FRAC_PI_2)
                .id("preset-up"),
        ]
        .gap(space::M)
        .align(Align::Center),
        col(lines).gap(space::TIGHT),
        row![
            name_field.el.fill(c(t.ink)).radius(radius::FIELD).grow(1.0),
            action_pill("save").tipped("preset-save"),
            action_pill("default").tipped("preset-default"),
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
