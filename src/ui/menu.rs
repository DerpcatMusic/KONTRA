//! Context menus: one open at a time, floated over everything where it was
//! asked for, closed by a pick, a click elsewhere or Escape.

use super::{Cx, theme::*};
use moose::mui::mui::prelude::*;
use std::path::Path;

/// What a menu is about.
#[derive(Clone, PartialEq, Debug)]
pub enum Target {
    /// A preset in the browser, by path.
    Preset(String),
    /// A rack slot.
    Part(usize),
    /// A key on the keyboard.
    Key(u8),
    /// The editor's own menu in the top bar.
    App,
    /// A script menu in a part's performance controls.
    Script { part: usize, control: usize },
}

#[derive(Clone, Debug)]
pub struct Menu {
    pub target: Target,
    /// Top-left corner, in window coordinates.
    pub at: Point,
    /// The button that opened it, which toggles it rather than dismissing it.
    pub anchor: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Open(String),
    OpenNew(String),
    Reveal(String),
    CopyPath(String),
    Favorite(String),
    Duplicate(usize),
    Remove(usize),
    Rename(usize),
    Mute(usize),
    Solo(usize),
    Move(usize, i32),
    Audition(u8),
    Folders,
    Rescan,
    Browser,
    Keyboard,
    Panic,
    /// Set a script control to a value.
    Script {
        part: usize,
        control: usize,
        value: i32,
    },
}

enum Item {
    Act {
        label: String,
        hint: &'static str,
        command: Command,
        on: bool,
    },
    Info(String),
    Rule,
}

fn act(label: impl Into<String>, hint: &'static str, command: Command) -> Item {
    Item::Act {
        label: label.into(),
        hint,
        command,
        on: false,
    }
}

fn check(label: impl Into<String>, on: bool, command: Command) -> Item {
    Item::Act {
        label: label.into(),
        hint: "",
        command,
        on,
    }
}

/// Wide enough for the longest built-in item beside its shortcut.
pub const WIDTH: f64 = TEXT * 19.;
const ROW: f64 = CONTROL + 2.;
const ID: &str = "context-menu";

/// Open a menu for `target` at the pointer.
pub fn open(ui: &Ui, cx: &mut Cx, target: Target) {
    let at = ui.local("editor-root").unwrap_or_default();
    cx.state.menu = Some(Menu {
        target,
        at,
        anchor: None,
    });
}

/// Open a menu for `target` under the element `anchor`.
pub fn open_under(ui: &Ui, cx: &mut Cx, target: Target, anchor: &str) {
    let at = ui
        .scene()
        .and_then(|s| s.surface(anchor))
        .map(|s| Point::new(s.frame.x, s.frame.y + s.frame.size.height + 1.))
        .unwrap_or_default();
    cx.state.menu = Some(Menu {
        target,
        at,
        anchor: Some(anchor.to_owned()),
    });
}

fn items(cx: &Cx, target: &Target) -> Vec<Item> {
    match target {
        Target::Preset(path) => {
            let multi = crate::import::is_multi(Path::new(path));
            let mut items = vec![act(
                if multi { "Load multi" } else { "Load" },
                "Enter",
                Command::Open(path.clone()),
            )];
            if !multi {
                items.push(act("Load into new slot", "", Command::OpenNew(path.clone())));
            }
            let favorite = cx.selection.favorites.contains(path);
            items.extend([
                Item::Rule,
                act(
                    if favorite { "Remove from favorites" } else { "Add to favorites" },
                    "",
                    Command::Favorite(path.clone()),
                ),
                act("Reveal in folder", "", Command::Reveal(path.clone())),
                act("Copy path", "", Command::CopyPath(path.clone())),
            ]);
            items
        }
        Target::Part(slot) => {
            let slot = *slot;
            let Some(part) = cx.selection.parts.get(slot).filter(|p| !p.path.is_empty()) else {
                return Vec::new();
            };
            let position = cx.selection.order.iter().position(|n| *n as usize == slot);
            let last = cx.selection.order.len().saturating_sub(1);
            let mut items = vec![
                act("Rename…", "", Command::Rename(slot)),
                act("Duplicate", "Ctrl+D", Command::Duplicate(slot)),
                Item::Rule,
                check("Mute", part.mute, Command::Mute(slot)),
                check("Solo", part.solo, Command::Solo(slot)),
                Item::Rule,
            ];
            if position.is_some_and(|p| p > 0) {
                items.push(act("Move up", "", Command::Move(slot, -1)));
            }
            if position.is_some_and(|p| p < last) {
                items.push(act("Move down", "", Command::Move(slot, 1)));
            }
            items.extend([
                act("Reveal in folder", "", Command::Reveal(part.path.clone())),
                act("Copy path", "", Command::CopyPath(part.path.clone())),
                Item::Rule,
                act("Remove", "Del", Command::Remove(slot)),
            ]);
            items
        }
        Target::Key(note) => {
            let v = cx.part_view();
            let mut items = vec![Item::Info(key_info(cx, *note))];
            if let Some(what) = v.keys.get(note).map(|k| k.name.clone()).filter(|n| !n.is_empty()) {
                items.push(Item::Info(what));
            }
            items.push(Item::Rule);
            items.push(act(format!("Audition {}", note_name(*note)), "", Command::Audition(*note)));
            items
        }
        Target::Script { part, control } => {
            let (part, control) = (*part, *control);
            let Some(c) = cx
                .view
                .parts
                .get(part)
                .and_then(|v| v.interface.as_ref())
                .and_then(|i| i.controls.get(control))
            else {
                return Vec::new();
            };
            let value = match c.properties.get("$CONTROL_PAR_VALUE") {
                Some(crate::ksp::Value::Int(n)) => *n,
                _ => 0,
            };
            c.menu
                .iter()
                .map(|(text, v)| {
                    let label = super::panel::clean(text);
                    check(
                        label,
                        *v == value,
                        Command::Script {
                            part,
                            control,
                            value: *v,
                        },
                    )
                })
                .collect()
        }
        Target::App => vec![
            check("Browser", cx.state.browser, Command::Browser),
            check("Keyboard", cx.state.keyboard, Command::Keyboard),
            Item::Rule,
            act("Library folder…", "", Command::Folders),
            act("Rescan libraries", "", Command::Rescan),
            Item::Rule,
            act("All notes off", "", Command::Panic),
        ],
    }
}

/// "C3 · plays samples", "F#0 · no samples".
fn key_info(cx: &Cx, note: u8) -> String {
    let mapped = super::instrument::current(cx).is_some_and(|i| {
        i.zones
            .iter()
            .any(|z| z.available && (z.low_key..=z.high_key).contains(&note))
    });
    format!(
        "{} · MIDI {note} · {}",
        note_name(note),
        if mapped { "plays samples" } else { "no samples" }
    )
}

/// The open menu, if any, positioned inside `window`; runs what was picked.
pub fn view(ui: &mut Ui, cx: &mut Cx, window: Size) -> Option<El> {
    let menu = cx.state.menu.clone()?;
    let anchor = menu.anchor.as_deref().unwrap_or(ID);
    if ui.dismissed(&[ID, anchor]) {
        cx.state.menu = None;
        return None;
    }
    let items = items(cx, &menu.target);
    if items.is_empty() {
        cx.state.menu = None;
        return None;
    }
    let mut rows = Vec::new();
    let mut height = 2. * TIGHT + 2.;
    let mut picked = None;
    for (n, item) in items.into_iter().enumerate() {
        match item {
            Item::Rule => {
                height += 2. * TIGHT + 1.;
                rows.push(col![rule()].pad((0, TIGHT)).shrink(0));
            }
            Item::Info(text) => {
                height += ROW;
                rows.push(
                    row![caption(text).fill(Role::Dim).lines(1).min_w(0)]
                        .align(Align::Center)
                        .pad((SPACE, 0))
                        .h(ROW)
                        .shrink(0),
                );
            }
            Item::Act {
                label,
                hint,
                command,
                on,
            } => {
                height += ROW;
                let id = format!("menu-item-{n}");
                if ui.get(id.as_str()).activated() {
                    picked = Some(command);
                }
                let mark = block(TIGHT * 1.5, TIGHT * 1.5).fill(if on {
                    Role::Ink.alpha(1.)
                } else {
                    Role::Ink.alpha(0.)
                });
                let el = row![
                    mark,
                    body(label.clone())
                        .text_size(TEXT)
                        .lines(1)
                        .flex(1)
                        .min_w(0),
                    caption(hint).fill(Role::Dim)
                ]
                .gap(SPACE)
                .align(Align::Center)
                .pad((SPACE, 0))
                .h(ROW)
                .focusable()
                .a11y(A11y::Button)
                .named(label)
                .id(id)
                .shrink(0);
                rows.push(interactive(el, false));
            }
        }
    }
    if let Some(command) = picked {
        cx.state.menu = None;
        run(ui, cx, command);
        return None;
    }
    let x = menu.at.x.min(window.width - WIDTH - TIGHT).max(TIGHT);
    let y = if menu.at.y + height > window.height - TIGHT {
        (menu.at.y - height).max(TIGHT)
    } else {
        menu.at.y
    };
    Some(
        col(rows)
            .gap(0)
            .align(Align::Stretch)
            .pad(TIGHT)
            .w(WIDTH)
            .max_size(Size::new(WIDTH, window.height - 2. * TIGHT))
            .scroll()
            .fill(Role::Level(3))
            .stroke(Role::Ink.alpha(0.14))
            .stroke_width(1)
            .at(x, y)
            .appear(Appear::Slide(0., -4.))
            .a11y(A11y::Group)
            .named("Context menu")
            .id(ID),
    )
}

/// Carry out a menu pick or a shortcut.
pub fn run(ui: &mut Ui, cx: &mut Cx, command: Command) {
    let shared = &cx.p.shared;
    match command {
        Command::Open(path) => cx.open(Path::new(&path)),
        Command::OpenNew(path) => cx.add(path),
        Command::Reveal(path) => reveal(Path::new(&path)),
        Command::CopyPath(path) => ui.set_clipboard(path),
        Command::Favorite(path) => cx.toggle_favorite(&path),
        Command::Duplicate(slot) => cx.duplicate(slot),
        Command::Remove(slot) => cx.remove(slot),
        Command::Rename(slot) => {
            cx.show(slot);
            cx.state.renaming = Some((slot, super::rack::name(cx, slot)));
        }
        Command::Mute(slot) => cx.selection.parts[slot].mute ^= true,
        Command::Solo(slot) => cx.selection.parts[slot].solo ^= true,
        Command::Move(slot, by) => cx.move_by(slot, by),
        Command::Audition(note) => shared.audition(Some(note)),
        Command::Folders => cx.state.settings = !cx.state.settings,
        Command::Rescan => {
            cx.selection.root = cx.state.root.clone();
            shared
                .view
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .root
                .clear();
        }
        Command::Browser => cx.state.browser ^= true,
        Command::Keyboard => cx.state.keyboard ^= true,
        Command::Panic => shared.panic.store(true, std::sync::atomic::Ordering::Release),
        Command::Script {
            part,
            control,
            value,
        } => shared.edit_control(part, control, value),
    }
}

/// Show `path` in the system's file manager.
fn reveal(path: &Path) {
    let folder = path.parent().unwrap_or(path);
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let spawned = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let spawned = std::process::Command::new("xdg-open").arg(folder).spawn();
    let _ = folder;
    // Reap it off this thread so it leaves no zombie behind.
    if let Ok(mut child) = spawned {
        std::thread::spawn(move || child.wait());
    }
}
