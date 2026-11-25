//! The sheets: Settings (library folders, interface, appearance, MIDI) and
//! About (the build and the changelog).

use mui::prelude::*;

use super::{Action, App, Build, Presentation, SCALES, Sheet};
use crate::kit::{self, Icon, button, icon_button, interactive, latch, rule, section};
use crate::theme::{self, CONTROL, HAIR, Look, SPACE, TEXT, TIGHT, TOKENS, secondary};

/// A labelled row: the label takes the room left, the control sits at the end.
fn line(label: &str, control: El) -> El {
    row![body(label.to_owned()).lines(1).min_w(0).grow(1), control]
        .gap(SPACE)
        .align(Align::Center)
        .min_h(CONTROL)
}

/// A titled block of rows, left aligned, rows stretched to its width.
fn group(title: &str, rows: Vec<El>) -> El {
    // Wrap the section title in a row so `Align::Stretch` does not centre it.
    let mut all = vec![row![section(title)].align(Align::Center)];
    all.extend(rows);
    col(all).gap(TIGHT).align(Align::Stretch)
}

/// The settings sheet's body: two balanced columns (Library, MIDI & Output,
/// About on the left; Interface and Appearance on the right) so every section
/// is separated clearly and fits on screen without hiding controls off-fold.
pub(super) fn view(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let left = col![
        library(ui, app, out),
        rule(),
        midi(ui, app, out),
        rule(),
        about(ui, app),
    ]
    .gap(SPACE)
    .align(Align::Stretch)
    .flex(1)
    .min_w(0);
    // Equal halves: long folder paths ellipsize instead of squeezing this one.
    let right = col![interface(ui, app, out), rule(), appearance(ui, app, out)]
        .gap(SPACE)
        .align(Align::Stretch)
        .flex(1)
        .min_w(0);
    // A column scrolls down; a scrolling row would scroll sideways and keep
    // its full height, spilling a long folder list out of the sheet.
    col![
        row![left, kit::vrule(), right]
            .gap(SPACE * 2.)
            .align(Align::Stretch)
    ]
    .align(Align::Stretch)
    .scroll()
    .id("settings-scroll")
    .min_h(0)
    .grow(1)
}

fn library(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let add = button(ui, "add-folder", "Add folder…", false);
    out.extend(add.changed.then_some(Action::AddFolder));
    let rescan = button(ui, "rescan", "Rescan", false);
    out.extend(rescan.changed.then_some(Action::Refresh));
    let choose = button(ui, "snapshot-folder", "Choose…", false);
    out.extend(choose.changed.then_some(Action::ChooseSnapshotFolder));
    let mut rows = Vec::new();
    for (n, f) in app.folders.iter().enumerate() {
        let remove = icon_button(
            ui,
            format!("folder-remove-{n}"),
            Icon::Close,
            if f.auto {
                "Hide this found folder"
            } else {
                "Remove this folder"
            },
            false,
        );
        out.extend(remove.changed.then_some(Action::RemoveFolder(n)));
        rows.push(super::browser::scan_folder(f, Some(remove.el)).id(format!("folder-{n}")));
    }
    if app.folders.is_empty() {
        rows.push(caption("No folders yet").fill(secondary()));
    }
    rows.push(row![rescan.el, add.el].gap(SPACE).right());
    // One short status; the path, or what to do, on hover.
    let (path, found) = &app.snapshot_folder;
    let (said, tip) = match found {
        Some(true) => (
            path.rsplit(['/', '\\'])
                .find(|s| !s.is_empty())
                .unwrap_or(path)
                .to_owned(),
            path.clone(),
        ),
        Some(false) => ("Not found".to_owned(), format!("Not found: {path}")),
        None => (
            "Not set".to_owned(),
            "Choose the User Content folder Kontakt saves snapshots in".to_owned(),
        ),
    };
    let status = caption(said).fill(secondary()).tip(tip).lines(1).min_w(0);
    rows.push(line(
        "Kontakt snapshots",
        row![status, choose.el].gap(SPACE).align(Align::Center),
    ));
    group("Library", rows)
}

fn interface(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let labels: Vec<String> = SCALES.iter().map(|s| format!("{s}%")).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let percent = if app.view.scale == 0 {
        100
    } else {
        app.view.scale
    };
    let at = SCALES.iter().position(|&s| s == percent);
    let current = at.and_then(|n| refs.get(n)).copied().unwrap_or("100%");
    let size = kit::dropdown(
        ui,
        &mut app.view.menu,
        "scale",
        (None, current, "Interface scale"),
        &refs,
        at,
    );
    if let Some(&s) = size.changed.and_then(|n| SCALES.get(n)) {
        app.view.scale = s;
        out.push(Action::Scale(s));
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
    let at = Presentation::ALL
        .iter()
        .position(|l| *l == app.view.face_look);
    let look = kit::dropdown(
        ui,
        &mut app.view.menu,
        "face-look",
        (
            None,
            at.and_then(|a| Presentation::LABELS.get(a))
                .copied()
                .unwrap_or("Original"),
            "How a library's view is drawn, unless its part chose",
        ),
        &Presentation::LABELS,
        at,
    );
    if let Some(&l) = look.changed.and_then(|a| Presentation::ALL.get(a)) {
        app.view.face_look = l;
    }
    group(
        "Interface",
        vec![
            line("Interface scale", size.el),
            line("Library view size", face.el),
            line("Library look", look.el),
        ],
    )
}

/// Presets, a tint and each colour token; every edit applies at once and
/// goes to the host to save.
fn appearance(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let before = theme::look();
    let mut look = before;
    let names: Vec<&str> = Look::PRESETS.iter().map(|(n, _)| *n).collect();
    let at = Look::PRESETS.iter().position(|(_, l)| *l == look);
    let preset = kit::dropdown(
        ui,
        &mut app.view.menu,
        "theme-preset",
        (
            None,
            at.and_then(|a| names.get(a)).copied().unwrap_or("Custom"),
            "A ready-made set of colours",
        ),
        &names,
        at,
    );
    if let Some(&(_, l)) = preset.changed.and_then(|n| Look::PRESETS.get(n)) {
        look = l;
    }
    let mut tint = f64::from(look.tint());
    let slider = kit::slider(
        ui,
        "theme-tint",
        "How far the greys lean toward the accent",
        &mut tint,
        0.0..=1.0,
        0.,
        (TEXT * 10., CONTROL * 0.8),
    );
    if slider.changed {
        look = look.with_tint(tint as f32);
    }
    let reset = button(ui, "theme-reset", "Reset", false);
    if reset.changed {
        look = Look::GRAPHITE;
    }
    let mut rows = vec![
        line(
            "Theme",
            row![preset.el, reset.el].gap(SPACE).align(Align::Center),
        ),
        line("Tint", slider.el),
    ];
    for (key, label) in TOKENS {
        let id = format!("theme-{key}");
        let Some(mut color) = look.get(key) else {
            continue;
        };
        let hit = ui.get(id.as_str()).activated();
        let open = kit::toggle(ui, &mut app.view.menu, &id, hit);
        let swatch = interactive(
            row![
                block(TEXT, TEXT)
                    .fill(color)
                    .stroke(Role::Ink.alpha(0.3))
                    .stroke_width(HAIR),
                caption(theme::hex(color)).fill(Role::Ink).lines(1),
            ]
            .gap(TIGHT)
            .align(Align::Center)
            .pad((TIGHT * 1.5, 0))
            .h(CONTROL * 0.9)
            .focusable()
            .a11y(A11y::Button)
            .named(format!("{label} colour"))
            .id(id.clone()),
        );
        let mut control = vec![swatch];
        if open {
            let picker = color_picker(ui, format!("{id}-picker"), &mut color, ColorOpts::default());
            if picker.changed {
                look.set(key, color);
            }
            control.push(kit::popup(&id, picker.el));
        }
        rows.push(line(label, stack(control)));
    }
    if look != before {
        theme::set_look(look);
        theme::sync(ui);
        out.push(Action::Theme(look.tokens()));
    }
    group("Appearance", rows)
}

fn midi(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let on = |b: bool| if b { "On" } else { "Off" };
    let thru = latch(
        ui,
        "settings-thru",
        on(app.midi_thru),
        "Pass MIDI through to the host",
        app.midi_thru,
    );
    if thru.changed {
        app.midi_thru = !app.midi_thru;
        out.push(Action::MidiThru(app.midi_thru));
    }
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
    group(
        "MIDI & Output",
        vec![
            line("MIDI thru", thru.el),
            line("New parts: next free channel", next.el),
            line("New parts: own output", own.el),
        ],
    )
}

fn about(ui: &mut Ui, app: &mut App) -> El {
    let open = button(ui, "settings-about", "About KONTRA…", false);
    if open.changed {
        app.view.sheet = Some(Sheet::About);
    }
    group(
        "About & diagnostics",
        vec![line(
            &format!("KONTRA {} · {}", app.build.version, app.build.revision),
            open.el,
        )],
    )
}

/// One release in the changelog: its heading and its lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub heading: String,
    pub lines: Vec<String>,
}

/// CHANGELOG.md's releases in file order (newest first): each `## ` heading
/// and the `- ` lines under it, Markdown links and emphasis reduced to
/// their text. The preamble before the first release is left out.
#[must_use]
pub fn changelog(text: &str) -> Vec<Release> {
    let mut releases: Vec<Release> = Vec::new();
    for raw in text.lines() {
        if let Some(h) = raw.strip_prefix("## ") {
            releases.push(Release {
                heading: plain(h),
                lines: Vec::new(),
            });
            continue;
        }
        let Some(r) = releases.last_mut() else {
            continue;
        };
        if let Some(item) = raw.strip_prefix("- ").or_else(|| raw.strip_prefix("* ")) {
            r.lines.push(plain(item));
        } else if raw.starts_with("  ") && !raw.trim().is_empty() {
            // A wrapped item continues the one before it.
            if let Some(last) = r.lines.last_mut() {
                last.push(' ');
                last.push_str(&plain(raw.trim()));
            }
        }
    }
    releases
}

/// `s` without Markdown: `[text](url)` → text; `**`, `` ` `` dropped.
fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        let (before, after) = rest.split_at(open);
        out.push_str(before);
        let close = after
            .find("](")
            .and_then(|c| Some((c, c + after[c..].find(')')?)));
        match close {
            Some((c, end)) => {
                out.push_str(after.get(1..c).unwrap_or_default());
                rest = after.get(end + 1..).unwrap_or_default();
            }
            None => {
                out.push('[');
                rest = after.get(1..).unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    // Noto Sans has no arrows: `→` would draw an empty box.
    out.replace("**", "")
        .replace('`', "")
        .replace('→', "->")
        .trim()
        .to_owned()
}

/// About: the build on the left, the changelog scrolling on the right.
fn about_view(ui: &mut Ui, build: &Build) -> El {
    let fact = |k: &str, v: &str, id: &str| {
        row![
            caption(k.to_owned())
                .fill(secondary())
                .w(TEXT * 5.)
                .shrink(0),
            caption(v.to_owned())
                .fill(Role::Ink)
                .lines(2)
                .min_w(0)
                .grow(1)
                .id(id.to_owned()),
        ]
        .gap(SPACE)
        .align(Align::Start)
    };
    let left = col![
        row![title("KONTRA")].align(Align::Center),
        caption("One engine that plays Kontakt, UVI and SFZ libraries as they were written.")
            .fill(secondary()),
        rule(),
        fact("Version", build.version, "about-version"),
        fact("Commit", build.revision, "about-revision"),
        fact("Built", build.built, "about-built"),
        fact("Target", build.target, "about-target"),
        fact("Formats", build.formats, "about-formats"),
    ]
    .gap(SPACE)
    .align(Align::Stretch)
    .w(TEXT * 20.)
    .shrink(0);
    // Hundreds of wrapped lines: parsed, shaped and laid out once, then kept
    // while the list scrolls under them (`changelog` is a `&'static str`,
    // so where it lives is what it is).
    let text = build.changelog;
    let log = ui.memo("about-changelog-log", (text.as_ptr(), text.len()), |_| {
        changelog_list(text)
    });
    let right = col![
        row![section("Changelog")].align(Align::Center),
        col![log].scroll().id("about-changelog").min_h(0).grow(1),
    ]
    .gap(SPACE)
    .align(Align::Stretch)
    .grow(1)
    .min_w(0)
    .min_h(0);
    row![left, kit::vrule(), right]
        .gap(SPACE * 2.)
        .align(Align::Stretch)
        .min_h(0)
        .grow(1)
}

/// The changelog's releases, a heading over its bulleted lines each.
fn changelog_list(text: &str) -> El {
    let mut log = Vec::new();
    for (idx, r) in changelog(text).into_iter().enumerate() {
        log.push(
            row![
                body(r.heading)
                    .text_weight(Weight::SEMIBOLD)
                    .lines(1)
                    .min_w(0)
            ]
            .pad(kit::edges(if idx == 0 { 0. } else { SPACE }, 0., 0., 0.)),
        );
        // One text node for all its lines: a scroll moves every node under
        // the scroller each frame, so the cost follows the count of nodes.
        let lines: Vec<String> = r.lines.iter().map(|l| format!("•  {l}")).collect();
        if !lines.is_empty() {
            log.push(caption(lines.join("\n")).fill(Role::Ink).min_w(0));
        }
    }
    col(log)
        .gap(TIGHT)
        .align(Align::Stretch)
        .pad(kit::edges(0., SPACE, 0., 0.))
}

/// The open sheet: Settings, a preset dialog or About.
pub(super) fn sheet(ui: &mut Ui, app: &mut App, which: Sheet, out: &mut Vec<Action>) -> El {
    let (title, content) = match which {
        Sheet::Settings => ("Settings", view(ui, app, out)),
        Sheet::SavePreset(n) | Sheet::RenamePreset(n) | Sheet::DeletePreset(n) => {
            super::preset::sheet(ui, app, n, which, out)
        }
        Sheet::About => ("About", about_view(ui, &app.build)),
    };
    let r = kit::sheet(ui, "sheet", title, content);
    if r.changed {
        app.view.sheet = None;
    }
    match which {
        Sheet::About => {
            r.el.w(Len::Pct(100.))
                .h(Len::Pct(100.))
                .max_size((TEXT * 64., TEXT * 46.))
                .shrink(1)
                .min_w(0)
                .min_h(0)
        }
        Sheet::Settings => {
            r.el.w(Len::Pct(100.))
                .max_size((TEXT * 60., TEXT * 42.))
                .shrink(1)
                .min_w(0)
                .min_h(0)
        }
        _ => r.el,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changelog_reads_releases_newest_first() {
        let text = "# Changelog\n\nIntro\n- not a release line\n\n## 0.6.24 (unreleased)\n\n- About shows [the build](https://x/1)\n- **Bold** `code`\n  wrapped on\n\n## 0.6.23 — 2026-11-24\n\n- kontra: fix (#144)\n";
        let r = changelog(text);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].heading, "0.6.24 (unreleased)");
        assert_eq!(
            r[0].lines,
            ["About shows the build", "Bold code wrapped on"]
        );
        assert_eq!(r[1].heading, "0.6.23 — 2026-11-24");
        assert_eq!(r[1].lines, ["kontra: fix (#144)"]);
        assert_eq!(plain("a [b (c)"), "a [b (c)");
    }
}
