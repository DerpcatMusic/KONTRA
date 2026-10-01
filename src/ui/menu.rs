//! Context menus: one open at a time, floated over everything where it was
//! asked for, closed by a pick, a click elsewhere or Escape.

use super::{Cx, mixer::{self, Strip}, theme::*};
use crate::engine::{BUSES, Streaming};
use moose::mui::mui::prelude::*;
use crate::articulate::Zone;
use std::path::Path;

/// What a menu is about.
#[derive(Clone, PartialEq, Debug)]
pub enum Target {
    /// A preset in the browser, by path.
    Preset(String),
    /// A library in the browser, by name.
    Library(String),
    /// The browser's add button: library folders.
    Libraries,
    /// How the browser lists the libraries.
    LibrarySort,
    /// A rack slot.
    Part(usize),
    /// How a rack slot shows its library's performance view.
    View(usize),
    /// A key on the keyboard.
    Key(u8),
    /// The editor's own menu in the top bar.
    App,
    /// A script menu in a part's performance controls.
    Script { part: usize, control: usize },
    /// A part's MIDI input: its channel and port.
    Midi(usize),
    /// A part's output bus, its aux send bus; a bus's host port.
    Output(usize),
    Aux(usize),
    BusPort(usize),
    /// The mixer's routing: the Outputs mode and the one-click actions.
    Routing,
    /// A mixer strip.
    Strip(Strip),
    /// An articulation's keyswitch, channel or velocity range.
    Articulation { part: usize, row: usize, field: ArtField },
}

/// Which of an articulation's settings a menu is about.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ArtField {
    Key,
    Channel,
    Velocity,
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
    /// Show a part's envelope, filter and effects in the Sound tab.
    EditSound(usize),
    Mute(usize),
    Solo(usize),
    /// How a part shows its performance view ([`crate::plugin::Part::view`]).
    View(usize, u8),
    /// The mode every part follows, which a part then follows too.
    DefaultView(usize, crate::library::ViewMode),
    Move(usize, i32),
    Audition(u8),
    /// Show or hide the library folders strip.
    Folders,
    /// Add a library folder (`true`) or a folder of libraries.
    AddFolder(bool),
    ImportKontakt,
    Rescan,
    CancelScan,
    /// A library's cover, by its folder: a picture chosen for it, the
    /// generated one, or back to its own artwork.
    ChangeArtwork(String),
    GeneratedCover(String),
    ResetCover(String),
    MoveLibrary(String, i32),
    ResetLibraryOrder,
    /// How the browser lists the libraries; a library pinned above the rest, by folder.
    SortLibraries(crate::library::Sort),
    Pin(String),
    SaveMulti,
    Browser,
    Keyboard,
    Panic,
    /// Where the rack's samples play from.
    Streaming(Streaming),
    /// Where a part's samples play from; `None` follows the rack.
    PartStreaming(usize, Option<Streaming>),
    /// A part's MIDI channel (-1 omni), its port (0..4), its output bus.
    Channel(usize, i16),
    Port(usize, u8),
    Output(usize, u8),
    /// Route a part automatically again.
    AutoOutput(usize),
    /// The mixer's Outputs mode ([`crate::routing::Outputs`]).
    Outputs(u8),
    OwnOutputs,
    OwnChannels,
    AllOmni,
    NameOutputs,
    ResetRouting,
    Appearance(super::Appearance),
    StickyHeaders,
    ArtworkBlur,
    /// A part's send bus, -1 for none.
    Aux(usize, i16),
    /// A bus's host port, -1 for its own.
    BusPort(usize, i16),
    StripRename(Strip),
    StripReset(Strip),
    StripRoute(Strip),
    /// Play an articulation's keyswitch from another key; `None` restores
    /// it, [`crate::articulate::CLEARED`] clears it.
    Remap(usize, usize, Option<u8>),
    /// Take an articulation's keyswitch from the next key played.
    Learn(usize, usize),
    /// An articulation's channel back to the library order's.
    ResetChannel(usize, usize),
    /// An articulation takes part in channel and velocity mode, or not.
    TakePart(usize, usize),
    /// Spread 1–127 evenly over the articulations taking part.
    SplitVelocities(usize),
    KeepOriginal(usize),
    Mpe(usize, Zone),
    BendRange(usize, u8),
    /// Auto-align timing on or off, and only while the transport plays.
    AutoAlign,
    AlignTransportOnly,
    /// Set how late a part sounds by hand, ms; `None` goes back to what was measured.
    Lateness(usize, Option<f32>),
    ExcludeTiming(usize),
    Remeasure(usize),
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
        Target::Library(name) => {
            let Some(library) = cx.view.shelf.named(name) else { return Vec::new() };
            let dir = library.dir.to_string_lossy().into_owned();
            let chosen = cx.settings.covers.get(&dir);
            let own = cx.view.artwork.contains_key(name);
            let mut items = vec![
                act("Move library up", "", Command::MoveLibrary(dir.clone(), -1)),
                act("Move library down", "", Command::MoveLibrary(dir.clone(), 1)),
                Item::Rule,
                act("Change artwork…", "", Command::ChangeArtwork(dir.clone())),
            ];
            if own && chosen != Some(&crate::library::Cover::Generated) {
                items.push(act("Use generated cover", "", Command::GeneratedCover(dir.clone())));
            }
            if chosen.is_some() {
                items.push(act(if own { "Reset to its artwork" } else { "Reset cover" }, "", Command::ResetCover(dir.clone())));
            }
            let pinned = cx.settings.pinned.contains(&dir);
            items.extend([
                Item::Rule,
                act(if pinned { "Unpin" } else { "Pin to top" }, "", Command::Pin(dir.clone())),
                act("Reveal in folder", "", Command::Reveal(dir.clone())),
                act("Copy path", "", Command::CopyPath(dir)),
            ]);
            items
        }
        Target::LibrarySort => crate::library::Sort::ALL
            .into_iter()
            .map(|sort| check(sort.label(), cx.settings.sort == sort, Command::SortLibraries(sort)))
            .collect(),
        Target::Libraries => {
            let mut items = vec![
                act("Add folder of libraries…", "", Command::AddFolder(false)),
                act("Add library folder…", "", Command::AddFolder(true)),
                act("Import from Kontakt", "", Command::ImportKontakt),
                Item::Rule,
                act("Library folders…", "", Command::Folders),
                act("Reset library order to A–Z", "", Command::ResetLibraryOrder),
            ];
            if cx.p.shared.libraries.scanning().is_some() {
                items.push(act("Stop scanning", "", Command::CancelScan));
            } else {
                items.push(act("Rescan libraries", "", Command::Rescan));
            }
            items
        }
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
        Target::View(slot) => {
            let slot = *slot;
            let now = super::perf_view::shows(cx, slot);
            let mut items: Vec<Item> = (crate::library::ViewMode::ALL.iter())
                .map(|&m| check(m.label(), now == m, Command::View(slot, super::perf_view::code(Some(m)))))
                .collect();
            if now != cx.settings.view_mode {
                items.extend([Item::Rule, act(format!("Make {} the default", now.label()), "", Command::DefaultView(slot, now))]);
            }
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
                act("Edit sound", "", Command::EditSound(slot)),
                act("Rename…", "", Command::Rename(slot)),
                act("Duplicate", "Ctrl+D", Command::Duplicate(slot)),
                Item::Rule,
                check("Mute", part.mute, Command::Mute(slot)),
                check("Solo", part.solo, Command::Solo(slot)),
                Item::Rule,
                Item::Info("MPE".into()),
            ];
            for (zone, label) in [(Zone::Off, "MPE off"), (Zone::Lower, "Lower zone"), (Zone::Upper, "Upper zone")] {
                items.push(check(label, part.mpe.zone == zone, Command::Mpe(slot, zone)));
            }
            if part.mpe.zone != Zone::Off {
                for range in [2, 12, 24, 48] {
                    let label = format!("Bend range ±{range}");
                    items.push(check(label, part.mpe.bend_range == range, Command::BendRange(slot, range)));
                }
            }
            items.push(Item::Rule);
            // Where its samples play from, the rack's way unless it has its own.
            let rack = match cx.selection.streaming {
                Streaming::Auto => "Samples as the rack (streaming)",
                Streaming::RamOnly => "Samples as the rack (all in RAM)",
            };
            items.extend([
                check(rack, part.streaming.is_none(), Command::PartStreaming(slot, None)),
                check("Stream from disk", part.streaming == Some(Streaming::Auto), Command::PartStreaming(slot, Some(Streaming::Auto))),
                check("Load all into RAM", part.streaming == Some(Streaming::RamOnly), Command::PartStreaming(slot, Some(Streaming::RamOnly))),
                Item::Rule,
            ]);
            if cx.selection.auto_align {
                timing_items(cx, slot, &mut items);
            }
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
        Target::Midi(slot) => {
            let Some(part) = cx.selection.parts.get(*slot) else {
                return Vec::new();
            };
            let mut items = vec![check("Omni", part.channel < 0, Command::Channel(*slot, -1))];
            items.extend((0..16).map(|c| check(format!("Channel {}", c + 1), part.channel == c, Command::Channel(*slot, c))));
            items.extend([Item::Rule, Item::Info("Port".into())]);
            items.extend((0..4u8).map(|n| {
                check(format!("Port {}", char::from(b'A' + n)), part.port == n, Command::Port(*slot, n))
            }));
            items
        }
        &Target::Articulation { part, row, field } => {
            use crate::articulate::CLEARED;
            let Some(a) = cx.selection.parts.get(part).map(|p| &p.articulate) else {
                return Vec::new();
            };
            let Some(r) = a.articulations.get(row) else {
                return Vec::new();
            };
            let take_part = check("Plays by channel and velocity", r.enabled, Command::TakePart(part, row));
            match field {
                ArtField::Key => {
                    let Some(key) = r.key else {
                        return Vec::new();
                    };
                    vec![
                        Item::Info(format!("{} · keyswitch {}", r.name, note_name(key))),
                        act("Learn", "", Command::Learn(part, row)),
                        check(format!("Reset to library default ({})", note_name(key)), r.remap.is_none(), Command::Remap(part, row, None)),
                        check("Clear", r.remap == Some(CLEARED), Command::Remap(part, row, Some(CLEARED))),
                        Item::Rule,
                        check("Original keys still switch", a.keep_original, Command::KeepOriginal(part)),
                    ]
                }
                ArtField::Channel => vec![
                    Item::Info(format!("{} · channel {}", r.name, r.channel + 1)),
                    check(format!("Reset to library default (Ch {})", row % 16 + 1), usize::from(r.channel) == row % 16, Command::ResetChannel(part, row)),
                    take_part,
                ],
                ArtField::Velocity => vec![
                    Item::Info(format!("{} · velocity {}–{}", r.name, r.low, r.high)),
                    act("Reset: split evenly", "", Command::SplitVelocities(part)),
                    take_part,
                ],
            }
        }
        Target::Strip(strip) => {
            let strip = *strip;
            if let Strip::Part(slot) = strip
                && cx.selection.parts.get(slot).is_none_or(|p| p.path.is_empty())
            {
                return Vec::new();
            }
            let mut items = vec![
                act("Rename…", "", Command::StripRename(strip)),
                act("Reset", "", Command::StripReset(strip)),
                Item::Rule,
                act("Route to…", "", Command::StripRoute(strip)),
            ];
            if let Strip::Part(slot) = strip {
                items.extend([Item::Rule, act("Edit sound", "", Command::EditSound(slot))]);
            }
            items
        }
        Target::Output(slot) => {
            let Some(part) = cx.selection.parts.get(*slot) else {
                return Vec::new();
            };
            let mut items = vec![check("Automatic", !part.output_manual, Command::AutoOutput(*slot)), Item::Rule];
            items.extend((0..BUSES).map(|n| {
                let label = bus_item(cx, n);
                check(label, part.output_manual && usize::from(part.output) == n, Command::Output(*slot, n as u8))
            }));
            items
        }
        Target::Routing => {
            use crate::routing::Outputs;
            let now = Outputs::of(cx.selection.outputs);
            let mut items = vec![Item::Info("Outputs".into())];
            items.extend(Outputs::ALL.map(|o| check(o.label(), now == o, Command::Outputs(o as u8))));
            items.extend([
                Item::Rule,
                act("Give every instrument its own output", "", Command::OwnOutputs),
                act("Name outputs after instruments", "", Command::NameOutputs),
                act("Reset routing", "", Command::ResetRouting),
                Item::Rule,
                act("Give every instrument its own MIDI channel", "", Command::OwnChannels),
                act("All Omni", "", Command::AllOmni),
            ]);
            items
        }
        Target::Aux(slot) => {
            let Some(part) = cx.selection.parts.get(*slot) else {
                return Vec::new();
            };
            let mut items = vec![check("No send", part.aux < 0, Command::Aux(*slot, -1)), Item::Rule];
            items.extend((0..BUSES).map(|n| {
                check(bus_item(cx, n), part.aux == n as i16, Command::Aux(*slot, n as i16))
            }));
            items
        }
        Target::BusPort(n) => {
            let bus = cx.selection.bus(*n);
            let port = if bus.port < 0 { *n } else { bus.port as usize };
            (0..BUSES)
                .map(|k| {
                    let label = format!("Host out {}", mixer::port_text(k));
                    let to = if k == *n { -1 } else { k as i16 };
                    check(label, port == k, Command::BusPort(*n, to))
                })
                .collect()
        }
        Target::App => {
            let mut items = vec![
                check("Browser", cx.state.browser, Command::Browser),
                check("Keyboard", cx.state.keyboard, Command::Keyboard),
                Item::Rule,
                act("Library folders…", "", Command::Folders),
                act("Rescan libraries", "", Command::Rescan),
                Item::Rule,
            ];
            if !cx.selection.order.is_empty() {
                items.extend([act("Save multi…", "", Command::SaveMulti), Item::Rule]);
            }
            items.push(act("All notes off", "", Command::Panic));
            let rack = cx.selection.streaming;
            items.extend([
                Item::Rule,
                Item::Info("Performance".into()),
                check("Disk streaming: Auto", rack == Streaming::Auto, Command::Streaming(Streaming::Auto)),
                check("Load all into RAM", rack == Streaming::RamOnly, Command::Streaming(Streaming::RamOnly)),
                Item::Rule,
                check("Auto-align timing", cx.selection.auto_align, Command::AutoAlign),
            ]);
            if cx.selection.auto_align {
                let told = f32::from_bits(cx.p.shared.reported.load(std::sync::atomic::Ordering::Relaxed));
                items.extend([
                    check("Only while the transport plays", cx.selection.align_transport_only, Command::AlignTransportOnly),
                    Item::Info(format!("Experimental · {told:.0} ms latency")),
                ]);
            } else {
                items.push(Item::Info("Experimental".into()));
            }
            items.extend([
                Item::Rule, Item::Info("Appearance".into()),
            ]);
            let now = super::Appearance::of(cx.selection.appearance);
            for (look, label) in [
                (super::Appearance::Plain, "Plain"),
                (super::Appearance::Color, "Library color"),
                (super::Appearance::Artwork, "Library artwork"),
            ] {
                items.push(check(label, now == look, Command::Appearance(look)));
            }
            items.extend([
                Item::Rule,
                check("Artwork blur", !cx.selection.sharp_artwork, Command::ArtworkBlur),
                check("Sticky headers", !cx.selection.sticky_off, Command::StickyHeaders),
            ]);
            items
        }
    }
}

/// A part's timing under auto-align: how late it sounds and why, per
/// articulation, and the player's say.
fn timing_items(cx: &Cx, slot: usize, items: &mut Vec<Item>) {
    let t = &cx.selection.parts[slot].timing;
    let latest = t.latest();
    items.push(Item::Info(format!("Timing −{latest:.0} ms · {}", t.basis())));
    let status = &cx.view.parts[slot].timing_status;
    if !status.is_empty() {
        items.push(Item::Info(status.clone()));
    }
    if t.override_ms.is_none() && !t.exclude {
        let ms = |d: &crate::timing::Delay, legato| d.ms(legato, 100).map_or("–".into(), |ms| format!("−{ms:.0}"));
        for d in t.arts.iter().filter(|d| d.max().is_some()) {
            let legato = if d.mono() { format!(", legato {} ms", ms(d, true)) } else { String::new() };
            items.push(Item::Info(format!("{} {} ms{legato}", d.name, ms(d, false))));
        }
    }
    items.extend([
        act("Play 10 ms earlier", "", Command::Lateness(slot, Some((latest + 10.).min(crate::timing::MAX_MS)))),
        act("Play 10 ms later", "", Command::Lateness(slot, Some((latest - 10.).max(0.)))),
        check("As measured", t.override_ms.is_none(), Command::Lateness(slot, None)),
        check("Exclude from alignment", t.exclude, Command::ExcludeTiming(slot)),
        act("Measure again", "", Command::Remeasure(slot)),
        Item::Rule,
    ]);
}

/// "st.3", or "st.3 · Drums" once named.
fn bus_item(cx: &Cx, n: usize) -> String {
    let own = crate::plugin::Bus::default().label(n);
    match crate::routing::label(&cx.selection, n) {
        name if name == own => name,
        name => format!("{own} · {name}"),
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
        Command::EditSound(slot) => {
            cx.show(slot);
            cx.state.tab = super::Tab::Sound;
        }
        Command::Mute(slot) => cx.selection.parts[slot].mute ^= true,
        Command::View(slot, code) => cx.selection.parts[slot].view = code,
        Command::DefaultView(slot, mode) => {
            shared.libraries.edit(|s| s.view_mode = mode);
            cx.selection.parts[slot].view = 0;
        }
        Command::Solo(slot) => cx.selection.parts[slot].solo ^= true,
        Command::Move(slot, by) => cx.move_by(slot, by),
        Command::Audition(note) => shared.audition(Some(note)),
        Command::Folders => cx.state.settings = !cx.state.settings,
        Command::AddFolder(single) => super::header::add_folder(cx, single),
        Command::ImportKontakt => shared.libraries.import_kontakt(),
        Command::Rescan => shared.libraries.rescan(),
        Command::SortLibraries(sort) => shared.libraries.edit(|s| s.sort = sort),
        Command::Pin(dir) => shared.libraries.edit(|s| match s.pinned.iter().position(|d| *d == dir) {
            Some(at) => {
                s.pinned.remove(at);
            }
            None => s.pinned.push(dir),
        }),
        Command::CancelScan => shared.libraries.cancel(),
        Command::ChangeArtwork(library) => {
            if !cx.state.picker.ask(super::picker::Ask::Artwork { library: library.into() }) {
                cx.state.notice = "No file dialog here: drop a PNG or JPEG onto the library instead.".into();
            }
        }
        Command::GeneratedCover(library) => {
            shared.libraries.set_cover(Path::new(&library), Some(crate::library::Cover::Generated))
        }
        Command::ResetCover(library) => shared.libraries.set_cover(Path::new(&library), None),
        Command::MoveLibrary(dir, by) => {
            let mut order: Vec<String> = cx.settings.arrange(cx.view.shelf.libraries.iter())
                .into_iter().map(|l| l.dir.to_string_lossy().into_owned()).collect();
            if let Some(at) = order.iter().position(|p| *p == dir)
                && let Some(to) = at.checked_add_signed(by as isize).filter(|&to| to < order.len())
            {
                order.swap(at, to);
                shared.libraries.edit(|s| { s.order = order; s.sort = crate::library::Sort::Custom; });
            }
        }
        Command::ResetLibraryOrder => shared.libraries.edit(|s| { s.order.clear(); s.sort = crate::library::Sort::Name; }),
        Command::SaveMulti => {
            // A saved multi offers its own name back; anything else starts blank.
            let current = &cx.selection.multi;
            let name = if crate::import::is_saved_multi(Path::new(current)) {
                super::header::stem(current)
            } else {
                String::new()
            };
            let from = super::header::multi_path(&super::header::root(cx), "x")
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let ask = super::picker::Ask::Multi { from, name: if name.is_empty() { "Multi".into() } else { name.clone() } };
            if !cx.state.picker.ask(ask) {
                cx.state.saving = Some(name);
                cx.state.save_error.clear();
            }
        }
        Command::Browser => cx.state.browser ^= true,
        Command::Appearance(look) => cx.selection.appearance = look as u8,
        Command::ArtworkBlur => cx.selection.sharp_artwork ^= true,
        Command::StickyHeaders => cx.selection.sticky_off ^= true,
        Command::Keyboard => cx.state.keyboard ^= true,
        Command::Streaming(mode) => cx.selection.streaming = mode,
        Command::PartStreaming(slot, mode) => {
            if let Some(part) = cx.selection.parts.get_mut(slot) {
                part.streaming = mode;
            }
        }
        Command::AutoAlign => cx.selection.auto_align ^= true,
        Command::AlignTransportOnly => cx.selection.align_transport_only ^= true,
        Command::Lateness(slot, ms) => cx.selection.parts[slot].timing.override_ms = ms,
        Command::ExcludeTiming(slot) => cx.selection.parts[slot].timing.exclude ^= true,
        Command::Remeasure(slot) => cx.selection.parts[slot].timing.source.clear(),
        Command::Panic => shared.panic.store(true, std::sync::atomic::Ordering::Release),
        Command::Remap(part, row, to) => {
            if let Some(r) = cx.selection.parts.get_mut(part).and_then(|p| p.articulate.articulations.get_mut(row)) {
                r.remap = to.filter(|&to| Some(to) != r.key);
            }
        }
        Command::Learn(part, row) => super::panel::begin(ui, cx, &format!("art-key-{part}-{row}")),
        Command::ResetChannel(part, row) => {
            if let Some(r) = cx.selection.parts.get_mut(part).and_then(|p| p.articulate.articulations.get_mut(row)) {
                r.channel = (row % 16) as u8;
            }
        }
        Command::TakePart(part, row) => {
            if let Some(r) = cx.selection.parts.get_mut(part).and_then(|p| p.articulate.articulations.get_mut(row)) {
                r.enabled ^= true;
            }
        }
        Command::SplitVelocities(part) => {
            if let Some(p) = cx.selection.parts.get_mut(part) {
                p.articulate.split_velocities();
            }
        }
        Command::KeepOriginal(part) => {
            if let Some(p) = cx.selection.parts.get_mut(part) {
                p.articulate.keep_original ^= true;
            }
        }
        Command::Mpe(slot, zone) => cx.selection.parts[slot].mpe.zone = zone,
        Command::BendRange(slot, range) => cx.selection.parts[slot].mpe.bend_range = range,
        Command::Channel(slot, channel) => cx.selection.parts[slot].channel = channel,
        Command::Port(slot, port) => cx.selection.parts[slot].port = port,
        Command::Output(slot, output) => {
            let part = &mut cx.selection.parts[slot];
            (part.output, part.output_manual) = (output, true);
        }
        Command::AutoOutput(slot) => cx.selection.parts[slot].output_manual = false,
        Command::Outputs(0) => crate::routing::to_stereo(&mut cx.selection),
        Command::Outputs(mode) => cx.selection.outputs = mode,
        Command::OwnOutputs => {
            use crate::routing::Outputs;
            if Outputs::of(cx.selection.outputs) == Outputs::Stereo {
                cx.selection.outputs = Outputs::Instrument as u8;
            }
            cx.selection.parts.iter_mut().for_each(|p| p.output_manual = false);
        }
        Command::OwnChannels => crate::routing::own_channels(&mut cx.selection),
        Command::AllOmni => crate::routing::all_omni(&mut cx.selection),
        Command::NameOutputs => crate::routing::name_outputs(&mut cx.selection),
        Command::ResetRouting => crate::routing::reset(&mut cx.selection),
        Command::Script {
            part,
            control,
            value,
        } => shared.edit_control(part, control, value),
        Command::Aux(slot, n) => {
            if let Some(part) = cx.selection.parts.get_mut(slot) {
                part.aux = n;
            }
        }
        Command::BusPort(n, port) => cx.selection.bus_mut(n).port = port,
        Command::StripRename(strip) => mixer::start_rename(cx, strip),
        Command::StripReset(strip) => mixer::reset(cx, strip),
        Command::StripRoute(strip) => open(
            ui,
            cx,
            match strip {
                Strip::Part(slot) => Target::Output(slot),
                Strip::Bus(n) => Target::BusPort(n),
            },
        ),
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
