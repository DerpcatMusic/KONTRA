//! The sheets, and Settings: the library folders, Kontakt's snapshot
//! folder, the interface's size, MIDI.

use mui::prelude::*;

use super::{Action, App, FaceLook, SCALES, Sheet, SnapshotFolder};
use crate::kit::{self, Icon, button, icon_button, latch, rule, section};
use crate::theme::{SPACE, TIGHT, secondary};

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
    let at = FaceLook::ALL.iter().position(|l| *l == app.view.face_look);
    let look = kit::dropdown(
        ui,
        &mut app.view.menu,
        "face-look",
        (
            None,
            at.and_then(|a| FaceLook::LABELS.get(a))
                .copied()
                .unwrap_or("Original"),
            "How a library's view is drawn, unless its part chose",
        ),
        &FaceLook::LABELS,
        at,
    );
    if let Some(&l) = look.changed.and_then(|a| FaceLook::ALL.get(a)) {
        app.view.face_look = l;
    }
    let add = button(ui, "add-folder", "Add folder…", false);
    if add.changed {
        out.push(Action::AddFolder);
    }
    let choose = button(ui, "snapshot-folder", "Choose…", false);
    if choose.changed {
        out.push(Action::ChooseSnapshotFolder);
    }
    let snapshots = match &app.snapshot_folder {
        SnapshotFolder::Found(f) => caption(f.clone()).tip(f.clone()),
        SnapshotFolder::Missing(f) => caption(format!("Not found: {f}"))
            .fill(secondary())
            .tip(f.clone()),
        SnapshotFolder::Unset => {
            caption("Not set: choose the User Content folder Kontakt saves snapshots in")
                .fill(secondary())
        }
    };
    let snapshots = snapshots
        .lines(1)
        .min_w(0)
        .grow(1)
        .id("snapshot-folder-path");
    let rescan = button(ui, "rescan", "Rescan", false);
    if rescan.changed {
        out.push(Action::Refresh);
    }
    let on = |b: bool| if b { "On" } else { "Off" };
    let next = latch(
        ui,
        "settings-next-channel",
        on(app.next_channel),
        "A new part listens on the lowest MIDI channel no other part has",
        app.next_channel,
    );
    let own = latch(
        ui,
        "settings-own-output",
        on(app.own_output),
        "A new part plays into its own output pair",
        app.own_output,
    );
    if next.changed || own.changed {
        app.next_channel ^= next.changed;
        app.own_output ^= own.changed;
        out.push(Action::NewParts {
            next_channel: app.next_channel,
            own_output: app.own_output,
        });
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
        row![section("Kontakt snapshots"), spacer(), choose.el].align(Align::Center),
        snapshots,
        rule(),
        section("Interface"),
        line("Size", size.el),
        line("Library view", face.el),
        line("Library look", look.el),
        rule(),
        section("MIDI / Output"),
        line("MIDI thru", thru.el),
        line("New parts: next free channel", next.el),
        line("New parts: own output", own.el),
    ]
    .gap(SPACE)
}

/// The open sheet: Settings, a preset dialog or About.
pub(super) fn sheet(ui: &mut Ui, app: &mut App, which: Sheet, out: &mut Vec<Action>) -> El {
    let (title, content) = match which {
        Sheet::Settings => ("Settings", view(ui, app, out)),
        Sheet::SavePreset(n) | Sheet::RenamePreset(n) | Sheet::DeletePreset(n) => {
            super::preset::sheet(ui, app, n, which, out)
        }
        Sheet::About => (
            "About",
            col![
                title("KONTRA"),
                body(app.build.clone()).id("about-build"),
                caption(
                    "One engine that plays Kontakt, UVI and SFZ libraries as they were written."
                )
                .fill(secondary()),
            ]
            .gap(SPACE),
        ),
    };
    let r = kit::sheet(ui, "sheet", title, content);
    if r.changed {
        app.view.sheet = None;
    }
    r.el
}
