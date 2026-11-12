//! The player's own presets in a part's header: Save, Save as, Rename and
//! Delete after the preset list, each a click the player makes; naming and
//! deleting ask in a sheet first.

use mui::prelude::*;

use super::{Action, App, Sheet};
use crate::kit::{self, button};
use crate::theme::{SPACE, secondary};

/// The folder the player's own presets are listed in, by each library's.
pub const USER: &str = "User/";

/// What to do with the player's own preset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresetEdit {
    /// Save what the part plays under this name; a user preset of that
    /// name is replaced (the player clicked Save or Replace).
    Save(String),
    /// Rename the user preset the part plays.
    Rename(String),
    /// Delete the user preset the part plays.
    Delete,
}

/// What the header's preset menu offers after the presets, for the preset
/// a part plays: Save only for a user preset (it replaces that file),
/// Rename and Delete as well.
#[must_use]
pub(super) fn edits(playing: Option<&str>) -> &'static [&'static str] {
    if playing.is_some_and(|p| p.starts_with(USER)) {
        &["Save", "Save as…", "Rename…", "Delete…"]
    } else {
        &["Save as…"]
    }
}

/// Edit `pick` of [`edits`] for part `n` playing `name`: Save at once, the
/// rest open their sheet.
pub(super) fn picked(app: &mut App, n: usize, name: &str, pick: &str, out: &mut Vec<Action>) {
    let sheet = match pick {
        "Save" => {
            let edit = PresetEdit::Save(name.to_owned());
            out.push(Action::Preset { part: n, edit });
            return;
        }
        "Rename…" => Sheet::RenamePreset(n),
        "Delete…" => Sheet::DeletePreset(n),
        _ => Sheet::SavePreset(n),
    };
    app.view.sheet = Some(sheet);
    app.view.preset_name = name.to_owned();
}

/// The sheet `which` asks in: a name to save or rename to, or a delete to
/// confirm. A name a user preset of the library has already reads Replace.
pub(super) fn sheet(
    ui: &mut Ui,
    app: &mut App,
    part: usize,
    which: Sheet,
    out: &mut Vec<Action>,
) -> (&'static str, El) {
    let (title, verb) = match which {
        Sheet::RenamePreset(_) => ("Rename preset", "Rename"),
        Sheet::DeletePreset(_) => ("Delete preset", "Delete"),
        _ => ("Save preset", "Save"),
    };
    let library =
        (app.parts.get(part)).and_then(|p| app.libraries.iter().find(|l| l.name == p.library));
    let taken = library.is_some_and(|l| {
        let name = app.view.preset_name.trim();
        l.presets.iter().any(|p| p.strip_prefix(USER) == Some(name))
    });
    let mut items = Vec::new();
    if matches!(which, Sheet::DeletePreset(_)) {
        let text = format!("Delete \"{}\"? The file is removed.", app.view.preset_name);
        items.push(caption(text).fill(secondary()));
    } else {
        items.push(kit::field(ui, "preset-name", "Preset name", &mut app.view.preset_name).el);
    }
    let verb = if taken && verb == "Save" {
        "Replace"
    } else {
        verb
    };
    let go = button(ui, "preset-go", verb, false);
    let name = app.view.preset_name.trim().to_owned();
    if go.changed && (verb == "Delete" || !name.is_empty()) {
        let edit = match which {
            Sheet::RenamePreset(_) => PresetEdit::Rename(name),
            Sheet::DeletePreset(_) => PresetEdit::Delete,
            _ => PresetEdit::Save(name),
        };
        out.push(Action::Preset { part, edit });
        app.view.sheet = None;
    }
    items.push(go.el);
    (title, col(items).gap(SPACE))
}
