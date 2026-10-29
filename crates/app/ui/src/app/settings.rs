//! Settings: the library folders, the interface's size, MIDI.

use mui::prelude::*;

use super::{Action, App, SCALES};
use crate::kit::{self, Icon, button, icon_button, latch, rule, section};
use crate::theme::{SPACE, TIGHT};

/// The settings sheet's body.
pub(super) fn view(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let thru = latch(
        ui,
        "settings-thru",
        if app.midi_thru { "On" } else { "Off" },
        "Pass MIDI through to the host",
        app.midi_thru,
    );
    if thru.changed {
        app.midi_thru = !app.midi_thru;
        out.push(Action::MidiThru(app.midi_thru));
    }
    let labels: Vec<String> = SCALES.iter().map(|s| format!("{:.0}%", s * 100.)).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let current = refs.get(app.view.scale).copied().unwrap_or("100%");
    let size = kit::dropdown(
        ui,
        &mut app.view.menu,
        "scale",
        (None, current, "Editor size"),
        &refs,
        Some(app.view.scale),
    );
    if let Some(n) = size.changed {
        app.view.scale = n;
        out.extend(SCALES.get(n).map(|&s| Action::Scale(s)));
    }
    let faces = ["Fit", "100%", "150%", "200%"];
    let face = kit::dropdown(
        ui,
        &mut app.view.menu,
        "face-scale",
        (
            None,
            faces.get(app.view.face_scale).copied().unwrap_or("Fit"),
            "How large a library's own view is drawn",
        ),
        &faces,
        Some(app.view.face_scale),
    );
    if let Some(n) = face.changed {
        app.view.face_scale = n;
    }
    let add = button(ui, "add-folder", "Add folder…", false);
    if add.changed {
        out.push(Action::AddFolder);
    }
    let rescan = button(ui, "rescan", "Rescan", false);
    if rescan.changed {
        out.push(Action::Refresh);
    }
    let mut folders = Vec::new();
    for (n, f) in app.folders.iter().enumerate() {
        let remove = icon_button(
            ui,
            format!("folder-remove-{n}"),
            Icon::Close,
            "Remove this folder",
            false,
        );
        if remove.changed {
            out.push(Action::RemoveFolder(n));
        }
        let name = caption(f.clone()).lines(1).min_w(0).grow(1).tip(f.clone());
        folders.push(row![name, remove.el].align(Align::Center));
    }
    let folders = col(folders)
        .gap(TIGHT)
        .align(Align::Stretch)
        .w(Len::Pct(100.));
    let line = |label: &str, control: El| {
        row![body(label.to_owned()), spacer(), control].align(Align::Center)
    };
    col![
        row![section("Library folders"), spacer(), rescan.el, add.el]
            .gap(SPACE)
            .align(Align::Center),
        folders,
        rule(),
        section("Interface"),
        line("Size", size.el),
        line("Library view", face.el),
        rule(),
        section("MIDI / Output"),
        line("MIDI thru", thru.el),
    ]
    .gap(SPACE)
}
