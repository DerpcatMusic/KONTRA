//! Context menus: one open at a time, floated over everything where it was
//! asked for, closed by a pick, a click elsewhere or Escape.

use super::{Cx, theme::*};
use crate::sound::{BUSES, Streaming};
use moose::mui::mui::prelude::*;
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
    Snapshots(usize),
    View(usize),
    /// A key on the keyboard.
    Key(u8),
    /// The editor's own menu in the top bar.
    App,
    /// A part's MIDI input: its channel and port.
    Midi(usize),
    /// A part's output pair.
    Output(usize),
    /// The mixer's routing: the Outputs mode and the one-click actions.
    Routing,
    Articulations(usize),
    Articulation(usize, String),
    ArtDriver(usize),
    Aux(usize),
    Mixer(u64),
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
    Art(usize, super::inside::ArtAction),
    Aux(usize, i16),
    RenameStrip(u64),
    ResetStrip(u64),
    Streaming(Streaming),
    PartStreaming(usize, Option<Streaming>),
    LoadSnapshot(usize),
    SelectSnapshot {
        slot: usize,
        source: (String, u32, String),
        path: String,
    },
    DefaultView(usize, crate::library::ViewMode),
    MpeZone(usize, u8),
    Open(String),
    View(usize, u8),
    OpenNew(String),
    Reveal(String),
    CopyPath(String),
    Favorite(String),
    Duplicate(usize),
    Remove(usize),
    Rename(usize),
    RenameLibrary(String),
    Mute(usize),
    Solo(usize),
    Move(usize, i32),
    Audition(u8),
    /// Show or hide the Settings workspace.
    Folders,
    /// Add a library folder (`true`) or a folder of libraries.
    AddFolder(bool),
    ImportKontakt,
    CreateLibrary,
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
    Logs,
    About,
    Keyboard,
    Panic,
    /// A part's MIDI channel (-1 omni), its port (0..4), its output bus.
    Channel(usize, i16),
    Port(usize, u8),
    Mpe(usize),
    BendRange(usize, u8),
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
    /// [`crate::plugin::Selection::memory_budget_mb`].
    MemoryBudget(u32),
    /// Auto-align timing on or off, and only while the transport plays.
    AutoAlign,
    AlignTransportOnly,
    /// Set how late a part sounds by hand, ms; `None` goes back to what was measured.
    Lateness(usize, Option<f32>),
    ExcludeTiming(usize),
    Remeasure(usize),
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
        Target::Snapshots(slot) => {
            let Some(part) = cx.selection.parts.get(*slot).filter(|p| p.snapshot_base()) else {
                return Vec::new();
            };
            let mut items = Vec::new();
            if let Some(catalog) = cx.view.shelf.snapshots.get(Path::new(&part.path)) {
                for path in &catalog.paths {
                    let label = super::header::stem(&path.to_string_lossy());
                    let category = path
                        .parent()
                        .and_then(Path::file_name)
                        .unwrap_or_default()
                        .to_string_lossy();
                    items.push(Item::Act {
                        label: format!("{label} · {category}"),
                        hint: "",
                        on: path == Path::new(&part.snapshot),
                        command: Command::SelectSnapshot {
                            slot: *slot,
                            source: part.source(),
                            path: path.to_string_lossy().into_owned(),
                        },
                    });
                }
            }
            if !items.is_empty() {
                items.push(Item::Rule);
            }
            items.push(check(
                "Original instrument",
                part.snapshot.is_empty(),
                Command::SelectSnapshot {
                    slot: *slot,
                    source: part.source(),
                    path: String::new(),
                },
            ));
            items.push(act("Load snapshot…", "", Command::LoadSnapshot(*slot)));
            items
        }
        Target::View(slot) => {
            let mode = super::part::mode(cx, *slot);
            let mut items = vec![
                check(
                    "Original",
                    mode == crate::library::ViewMode::Original,
                    Command::View(*slot, 1),
                ),
                check(
                    "Vector",
                    mode == crate::library::ViewMode::Vectorized,
                    Command::View(*slot, 3),
                ),
                check(
                    "KONTRA",
                    mode == crate::library::ViewMode::Kontra,
                    Command::View(*slot, 2),
                ),
            ];
            if mode != cx.settings.view_mode {
                items.extend([
                    Item::Rule,
                    act(
                        format!("Make {} the default", mode.label()),
                        "",
                        Command::DefaultView(*slot, mode),
                    ),
                ]);
            }
            items
        }
        Target::ArtDriver(slot) => {
            use super::inside::ArtAction;
            let part = &cx.selection.parts[*slot];
            let now = part.articulation_overlay.driver.unwrap_or_else(|| {
                if part.switching & 0x80 != 0 {
                    part.switching >> 1 & 7
                } else {
                    cx.view.parts[*slot]
                        .instrument
                        .as_ref()
                        .map_or(0, |i| i.switching.driver as u8)
                }
            });
            ["Keys", "Velocity", "Channel", "CC", "Program"]
                .iter()
                .enumerate()
                .map(|(n, label)| {
                    check(
                        *label,
                        now == n as u8,
                        Command::Art(*slot, ArtAction::Driver(n as u8)),
                    )
                })
                .collect()
        }
        Target::Articulations(slot) => {
            use super::inside::ArtAction;
            let part = &cx.selection.parts[*slot];
            let learns = part.articulation_overlay.driver.unwrap_or_else(|| {
                if part.switching & 0x80 != 0 {
                    part.switching >> 1 & 7
                } else {
                    cx.view.parts[*slot]
                        .instrument
                        .as_ref()
                        .map_or(0, |i| i.switching.driver as u8)
                }
            }) == 0;
            vec![
                act(
                    "Reset mappings",
                    "Restore source inputs and display order",
                    Command::Art(*slot, ArtAction::Reset),
                ),
                check(
                    "Keep original keys",
                    cx.selection.parts[*slot]
                        .articulation_overlay
                        .keep_originals,
                    Command::Art(*slot, ArtAction::Keep),
                ),
                if learns {
                    act(
                        "MIDI learn",
                        "Learn a key for the active row",
                        Command::Art(*slot, ArtAction::Learn(None)),
                    )
                } else {
                    Item::Info("MIDI learn available in Keys mode".into())
                },
                Item::Rule,
                act(
                    "Reassign triggers in this order",
                    "Explicitly assign existing triggers in display order",
                    Command::Art(*slot, ArtAction::Reassign),
                ),
                act(
                    "Split velocities evenly",
                    "Spread 1–127 over participating rows in display order",
                    Command::Art(*slot, ArtAction::Split),
                ),
            ]
        }
        Target::Articulation(slot, id) => {
            use super::inside::ArtAction;
            let part = &cx.selection.parts[*slot];
            let learns = part.articulation_overlay.driver.unwrap_or_else(|| {
                if part.switching & 0x80 != 0 {
                    part.switching >> 1 & 7
                } else {
                    cx.view.parts[*slot]
                        .instrument
                        .as_ref()
                        .map_or(0, |i| i.switching.driver as u8)
                }
            }) == 0;
            let mut items = vec![
                if learns {
                    act(
                        "MIDI learn",
                        "Play a new key for this articulation",
                        Command::Art(*slot, ArtAction::Learn(Some(id.clone()))),
                    )
                } else {
                    Item::Info("MIDI learn available in Keys mode".into())
                },
                act(
                    "Clear trigger",
                    "Remove this row's input in the current mode",
                    Command::Art(*slot, ArtAction::Clear(id.clone())),
                ),
                act(
                    "Reset row mappings",
                    "Restore all this row's source triggers",
                    Command::Art(*slot, ArtAction::ResetRow(id.clone())),
                ),
                Item::Rule,
                act(
                    "Move up",
                    "",
                    Command::Art(*slot, ArtAction::Move(id.clone(), -1)),
                ),
                act(
                    "Move down",
                    "",
                    Command::Art(*slot, ArtAction::Move(id.clone(), 1)),
                ),
            ];
            if let Some(inst) = cx.view.parts[*slot].instrument.as_ref() {
                let ids = crate::sound::articulation::identities(&inst.articulations);
                if let Some(n) = ids.iter().position(|i| i == id) {
                    let input = cx.selection.parts[*slot].articulation_overlay.input(
                        id,
                        &inst.articulations[n],
                        sampler_ir::Driver::Keys,
                    );
                    if let crate::sound::articulation::Input::Keys(keys) = input {
                        items.push(Item::Info(format!(
                            "Keys: {}",
                            keys.into_iter()
                                .map(note_name)
                                .collect::<Vec<_>>()
                                .join(", ")
                        )));
                    }
                }
            }
            items.push(check(
                "Use in channel/velocity modes",
                part.articulation_overlay
                    .inputs
                    .get(id)
                    .is_none_or(|a| a.enabled != Some(false)),
                Command::Art(*slot, ArtAction::Include(id.clone())),
            ));
            items
        }
        Target::Aux(slot) => {
            let Some(part) = cx.selection.parts.get(*slot) else {
                return Vec::new();
            };
            let mut items = vec![
                check("No send", part.aux < 0, Command::Aux(*slot, -1)),
                Item::Rule,
            ];
            items.extend((0..BUSES).map(|n| {
                check(
                    bus_item(cx, n),
                    part.aux == n as i16,
                    Command::Aux(*slot, n as i16),
                )
            }));
            items
        }
        Target::Mixer(id) => vec![
            act("Rename…", "", Command::RenameStrip(*id)),
            act(
                "Reset strip",
                "Level, pan, switches and send; routing and name stay",
                Command::ResetStrip(*id),
            ),
        ],
        Target::Library(name) => {
            let Some(library) = cx.view.shelf.named(name) else {
                return Vec::new();
            };
            let dir = library.dir.to_string_lossy().into_owned();
            let chosen = cx.settings.covers.get(&dir);
            let own = cx.view.artwork.contains_key(name);
            let mut items = vec![
                act(
                    "Rename…",
                    "Display name only; files stay where they are",
                    Command::RenameLibrary(dir.clone()),
                ),
                Item::Rule,
                act("Move library up", "", Command::MoveLibrary(dir.clone(), -1)),
                act(
                    "Move library down",
                    "",
                    Command::MoveLibrary(dir.clone(), 1),
                ),
                Item::Rule,
                act("Change artwork…", "", Command::ChangeArtwork(dir.clone())),
            ];
            if own && chosen != Some(&crate::library::Cover::Generated) {
                items.push(act(
                    "Use generated cover",
                    "",
                    Command::GeneratedCover(dir.clone()),
                ));
            }
            if chosen.is_some() {
                items.push(act(
                    if own {
                        "Reset to its artwork"
                    } else {
                        "Reset cover"
                    },
                    "",
                    Command::ResetCover(dir.clone()),
                ));
            }
            let pinned = cx.settings.pinned.contains(&dir);
            items.extend([
                Item::Rule,
                act(
                    if pinned { "Unpin" } else { "Pin to top" },
                    "",
                    Command::Pin(dir.clone()),
                ),
                act("Reveal in folder", "", Command::Reveal(dir.clone())),
                act("Copy path", "", Command::CopyPath(dir)),
            ]);
            items
        }
        Target::LibrarySort => crate::library::Sort::ALL
            .into_iter()
            .map(|sort| {
                check(
                    sort.label(),
                    cx.settings.sort == sort,
                    Command::SortLibraries(sort),
                )
            })
            .collect(),
        Target::Libraries => {
            let mut items = vec![
                act("Add folder of libraries…", "", Command::AddFolder(false)),
                act("Add library folder…", "", Command::AddFolder(true)),
                act("Find installed libraries", "", Command::ImportKontakt),
                act("Create library from folder…", "", Command::CreateLibrary),
                Item::Rule,
                act("Settings…", "", Command::Folders),
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
            let multi = crate::library::is_multi(Path::new(path));
            let mut items = vec![act(
                if multi { "Load multi" } else { "Load" },
                "Enter",
                Command::Open(path.clone()),
            )];
            if !multi {
                items.push(act(
                    "Load into new slot",
                    "",
                    Command::OpenNew(path.clone()),
                ));
            }
            let favorite = cx.is_favorite(path);
            items.extend([
                Item::Rule,
                act(
                    if favorite {
                        "Remove from favorites"
                    } else {
                        "Add to favorites"
                    },
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
            // Where its samples play from, the rack's way unless it has its own.
            let rack = match cx.selection.streaming {
                Streaming::Auto => "Samples as the rack (streaming)",
                Streaming::RamOnly => "Samples as the rack (all in RAM)",
            };
            items.extend([
                check(
                    rack,
                    part.streaming.is_none(),
                    Command::PartStreaming(slot, None),
                ),
                check(
                    "Stream from disk",
                    part.streaming == Some(Streaming::Auto),
                    Command::PartStreaming(slot, Some(Streaming::Auto)),
                ),
                check(
                    "Load all into RAM",
                    part.streaming == Some(Streaming::RamOnly),
                    Command::PartStreaming(slot, Some(Streaming::RamOnly)),
                ),
                Item::Rule,
            ]);
            if cx.selection.auto_align {
                timing_items(cx, slot, &mut items);
            }
            if part.snapshot_base() {
                items.insert(1, act("Load snapshot…", "", Command::LoadSnapshot(slot)));
            }
            items.extend([
                Item::Info("MPE".into()),
                check("MPE off", !part.mpe, Command::MpeZone(slot, 0)),
                check(
                    "Lower zone",
                    part.mpe && !part.mpe_upper,
                    Command::MpeZone(slot, 1),
                ),
                check(
                    "Upper zone",
                    part.mpe && part.mpe_upper,
                    Command::MpeZone(slot, 2),
                ),
                Item::Rule,
            ]);
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
            let mut items = vec![Item::Info(key_info(cx, *note))];
            if let Some(what) = cx
                .view
                .parts
                .get(cx.state.selected)
                .and_then(|v| v.keys.get(*note as usize))
                .and_then(|k| k.name.clone())
                .filter(|n| !n.is_empty())
            {
                items.push(Item::Info(what));
            }
            items.push(Item::Rule);
            items.push(act(
                format!("Audition {}", note_name(*note)),
                "",
                Command::Audition(*note),
            ));
            items
        }
        Target::Midi(slot) => {
            let Some(part) = cx.selection.parts.get(*slot) else {
                return Vec::new();
            };
            let mut items = vec![check("Omni", part.channel < 0, Command::Channel(*slot, -1))];
            items.extend((0..16).map(|c| {
                check(
                    format!("Channel {}", c + 1),
                    part.channel == c,
                    Command::Channel(*slot, c),
                )
            }));
            items.extend([Item::Rule, Item::Info("Port".into())]);
            items.extend((0..4u8).map(|n| {
                check(
                    format!("Port {}", char::from(b'A' + n)),
                    part.port == n,
                    Command::Port(*slot, n),
                )
            }));
            items.extend([
                Item::Rule,
                check("MPE", part.mpe, Command::Mpe(*slot)),
                Item::Rule,
                Item::Info("Bend range".into()),
            ]);
            items.extend(
                [
                    (0, "As the instrument".to_owned()),
                    (2, "±2".into()),
                    (12, "±12".into()),
                    (24, "±24".into()),
                    (48, "±48 (MPE)".into()),
                ]
                .map(|(n, label)| check(label, part.bend_range == n, Command::BendRange(*slot, n))),
            );
            items
        }
        Target::Output(slot) => {
            let Some(part) = cx.selection.parts.get(*slot) else {
                return Vec::new();
            };
            let mut items = vec![
                check("Automatic", !part.output_manual, Command::AutoOutput(*slot)),
                Item::Rule,
            ];
            items.extend((0..BUSES).map(|n| {
                let label = bus_item(cx, n);
                check(
                    label,
                    part.output_manual && usize::from(part.output) == n,
                    Command::Output(*slot, n as u8),
                )
            }));
            items
        }
        Target::Routing => {
            use crate::routing::Outputs;
            let now = Outputs::of(cx.selection.outputs);
            let mut items = vec![Item::Info("Outputs".into())];
            items.extend(
                Outputs::ALL.map(|o| check(o.label(), now == o, Command::Outputs(o as u8))),
            );
            items.extend([
                Item::Rule,
                act(
                    "Give every instrument its own output",
                    "",
                    Command::OwnOutputs,
                ),
                act("Name outputs after instruments", "", Command::NameOutputs),
                act("Reset routing", "", Command::ResetRouting),
                Item::Rule,
                act(
                    "Give every instrument its own MIDI channel",
                    "",
                    Command::OwnChannels,
                ),
                act("All Omni", "", Command::AllOmni),
            ]);
            items
        }
        Target::App => {
            let mut items = vec![
                check("Browser", cx.state.browser, Command::Browser),
                check("Keyboard", cx.state.keyboard, Command::Keyboard),
                act("Report…", "", Command::Logs),
                act("About KONTRA…", "", Command::About),
                Item::Rule,
                act("Settings…", "", Command::Folders),
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
                check(
                    "Disk streaming: Auto",
                    rack == Streaming::Auto,
                    Command::Streaming(Streaming::Auto),
                ),
                check(
                    "Load all into RAM",
                    rack == Streaming::RamOnly,
                    Command::Streaming(Streaming::RamOnly),
                ),
                Item::Rule,
                check(
                    "Auto-align timing",
                    cx.selection.auto_align,
                    Command::AutoAlign,
                ),
            ]);
            if cx.selection.auto_align {
                let told = f32::from_bits(
                    cx.p.shared
                        .reported
                        .load(std::sync::atomic::Ordering::Relaxed),
                );
                items.extend([
                    check(
                        "Only while the transport plays",
                        cx.selection.align_transport_only,
                        Command::AlignTransportOnly,
                    ),
                    Item::Info(format!("Experimental · {told:.0} ms latency")),
                ]);
            } else {
                items.push(Item::Info("Experimental".into()));
            }
            items.extend([Item::Rule, Item::Info("Appearance".into())]);
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
                check(
                    "Artwork blur",
                    !cx.selection.sharp_artwork,
                    Command::ArtworkBlur,
                ),
                check(
                    "Sticky headers",
                    !cx.selection.sticky_off,
                    Command::StickyHeaders,
                ),
                Item::Rule,
                Item::Info("Sample memory".into()),
            ]);
            for (mb, label) in [
                (0, "Keep all"),
                (1024, "1 GB"),
                (2048, "2 GB"),
                (4096, "4 GB"),
                (8192, "8 GB"),
            ] {
                items.push(check(
                    label,
                    cx.selection.memory_budget_mb == mb,
                    Command::MemoryBudget(mb),
                ));
            }
            items.push(act(
                "Create library from folder…",
                "",
                Command::CreateLibrary,
            ));
            items
        }
    }
}

/// A part's timing under auto-align: how late it sounds and why, per
/// articulation, and the player's say.
fn timing_items(cx: &Cx, slot: usize, items: &mut Vec<Item>) {
    let timing = crate::plugin::timing_for(&cx.selection.parts[slot]);
    let t = timing.as_ref();
    let latest = t.latest();
    items.push(Item::Info(format!(
        "Timing −{latest:.0} ms · {}",
        t.basis()
    )));
    let status = &cx.view.parts[slot].timing_status;
    if !status.is_empty() {
        items.push(Item::Info(status.clone()));
    }
    if t.override_ms.is_none() && !t.exclude {
        let ms = |d: &crate::timing::Delay, legato| {
            d.ms(legato, 100)
                .map_or("–".into(), |ms| format!("−{ms:.0}"))
        };
        for d in t.arts.iter().filter(|d| d.max().is_some()) {
            let legato = if d.mono() {
                format!(", legato {} ms", ms(d, true))
            } else {
                String::new()
            };
            items.push(Item::Info(format!(
                "{} {} ms{legato}",
                d.name,
                ms(d, false)
            )));
        }
    }
    items.extend([
        act(
            "Play 10 ms earlier",
            "",
            Command::Lateness(slot, Some((latest + 10.).min(crate::timing::MAX_MS))),
        ),
        act(
            "Play 10 ms later",
            "",
            Command::Lateness(slot, Some((latest - 10.).max(0.))),
        ),
        check(
            "As measured",
            t.override_ms.is_none(),
            Command::Lateness(slot, None),
        ),
        check(
            "Exclude from alignment",
            t.exclude,
            Command::ExcludeTiming(slot),
        ),
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

fn key_info(cx: &Cx, note: u8) -> String {
    let mapped = cx
        .view
        .parts
        .get(cx.state.selected)
        .and_then(|v| v.report.as_ref())
        .is_some_and(|r| r.decoded.maps(note));
    format!(
        "{} · MIDI {note} · {}",
        note_name(note),
        if mapped {
            "plays samples"
        } else {
            "no samples"
        }
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
                    row![caption(text).fill(secondary()).lines(1).min_w(0)]
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
                // Only shortcuts occupy the trailing column; explanations stay on hover.
                let shortcut = matches!(
                    &command,
                    Command::Open(_) | Command::Duplicate(_) | Command::Remove(_)
                );
                let explanation = if matches!(
                    &command,
                    Command::Art(_, super::inside::ArtAction::Move(_, _))
                ) {
                    Some("Changes display order only; trigger assignments stay unchanged")
                } else if !shortcut && !hint.is_empty() {
                    Some(hint)
                } else {
                    None
                };
                let tip =
                    explanation.map_or_else(|| label.clone(), |text| format!("{label}\n{text}"));
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
                    caption(if shortcut { hint } else { "" }).fill(secondary())
                ]
                .gap(SPACE)
                .align(Align::Center)
                .pad((SPACE, 0))
                .h(ROW)
                .focusable()
                .a11y(A11y::Button)
                .tip(tip)
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
    let width = WIDTH.min(window.width - 2. * TIGHT);
    let x = menu.at.x.min(window.width - width - TIGHT).max(TIGHT);
    let y = if menu.at.y + height > window.height - TIGHT {
        (menu.at.y - height).max(TIGHT)
    } else {
        menu.at.y
    };
    ui.capture_popup_wheel(ID);
    ui.capture_popup_wheel("context-menu-backdrop");
    Some(
        stack![
            // A popup owns pointer hits outside its panel too. This retires the
            // underlying hover tooltip and consumes its dismissal click.
            block(window.width, window.height).id("context-menu-backdrop"),
            col(rows)
                .gap(0)
                .align(Align::Stretch)
                .pad(TIGHT)
                .w(width)
                .max_size(Size::new(width, window.height - 2. * TIGHT))
                .scroll()
                .fill(Role::Level(3))
                .stroke(Role::Ink.alpha(0.14))
                .stroke_width(1)
                .at(x, y)
                .a11y(A11y::Group)
                .named("Context menu")
                .id(ID),
        ]
        .full(),
    )
}

/// Carry out a menu pick or a shortcut.
pub fn run(ui: &mut Ui, cx: &mut Cx, command: Command) {
    let shared = &cx.p.shared;
    match command {
        Command::View(slot, mode) => {
            if let Some(part) = cx.selection.parts.get_mut(slot) {
                part.view = mode;
            }
            let chosen = super::part::mode(cx, slot);
            let path = cx.selection.parts[slot].path.clone();
            cx.p.shared.libraries.edit(|settings| {
                settings.instrument_views.insert(path, chosen);
            });
        }
        Command::Aux(slot, n) => {
            if let Some(part) = cx.selection.parts.get_mut(slot) {
                part.aux = n;
            }
        }
        Command::RenameStrip(id) => {
            if let Some(node) = super::bridge::tree(cx)
                .nodes
                .into_iter()
                .find(|n| n.id == id)
            {
                cx.state.mix_tree.renaming = Some((id, node.name));
            }
        }
        Command::ResetStrip(id) => super::bridge::reset(cx, id),
        Command::Art(slot, action) => super::inside::action(ui, cx, slot, action),
        Command::Open(path) => cx.open(Path::new(&path)),
        Command::OpenNew(path) => cx.add(path),
        Command::Reveal(path) => {
            if !cx.state.picker.ask(super::picker::Ask::Reveal(path.into())) {
                cx.state.notice = "Could not start Reveal: another file operation is still running. Retry when it finishes.".into();
            }
        }
        Command::CopyPath(path) => ui.set_clipboard(path),
        Command::Favorite(path) => cx.toggle_favorite(&path),
        Command::Duplicate(slot) => cx.duplicate(slot),
        Command::Remove(slot) => cx.remove(slot),
        Command::RenameLibrary(dir) => {
            if let Some(library) = cx
                .view
                .shelf
                .libraries
                .iter()
                .find(|l| l.dir == Path::new(&dir))
            {
                cx.state.browse.renaming = Some((dir, cx.settings.library_name(library)));
            }
        }
        Command::Rename(slot) => {
            cx.show(slot);
            cx.state.renaming = Some((slot, super::rack::name(cx, slot)));
        }
        Command::SelectSnapshot { slot, source, path } => {
            if cx
                .selection
                .parts
                .get(slot)
                .is_some_and(|part| part.source() == source)
            {
                cx.snapshot(slot, path);
            } else {
                cx.state.notice =
                    "Snapshot ignored: the base instrument changed while its menu was open.".into();
            }
        }
        Command::LoadSnapshot(slot) => {
            if let Some(part) = cx.selection.parts.get(slot) {
                let from = Path::new(if part.snapshot.is_empty() {
                    &part.path
                } else {
                    &part.snapshot
                })
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
                let ask = super::picker::Ask::Snapshot {
                    slot,
                    source: part.source(),
                    from,
                };
                if !cx.state.picker.ask(ask) {
                    cx.state.notice =
                        "No file dialog here: drop a .nksn snapshot onto this instrument's header."
                            .into();
                }
            }
        }
        Command::Mute(slot) => cx.selection.parts[slot].mute ^= true,
        Command::Solo(slot) => cx.selection.parts[slot].solo ^= true,
        Command::Move(slot, by) => cx.move_by(slot, by),
        Command::Audition(note) => shared.audition(Some(note)),
        Command::Folders => cx.state.settings = !cx.state.settings,
        Command::AddFolder(single) => super::header::add_folder(cx, single),
        Command::ImportKontakt => shared.libraries.import_kontakt(),
        Command::CreateLibrary => {
            let out = super::header::root(cx).into();
            if !cx.state.picker.ask(super::picker::Ask::Samples { out }) {
                cx.state.notice =
                    "No file dialog here: use `kontakto create-library <folder>`".into();
            }
        }
        Command::Rescan => shared.libraries.rescan(),
        Command::SortLibraries(sort) => shared.libraries.edit(|s| s.sort = sort),
        Command::Pin(dir) => {
            shared
                .libraries
                .edit(|s| match s.pinned.iter().position(|d| *d == dir) {
                    Some(at) => {
                        s.pinned.remove(at);
                    }
                    None => s.pinned.push(dir),
                })
        }
        Command::CancelScan => shared.libraries.cancel(),
        Command::ChangeArtwork(library) => {
            if !cx.state.picker.ask(super::picker::Ask::Artwork {
                library: library.into(),
            }) {
                cx.state.notice =
                    "No file dialog here: drop a PNG or JPEG onto the library instead.".into();
            }
        }
        Command::GeneratedCover(library) => shared
            .libraries
            .set_cover(Path::new(&library), Some(crate::library::Cover::Generated)),
        Command::ResetCover(library) => shared.libraries.set_cover(Path::new(&library), None),
        Command::MoveLibrary(dir, by) => {
            let mut order: Vec<String> = cx
                .settings
                .arrange(cx.view.shelf.libraries.iter())
                .into_iter()
                .map(|l| l.dir.to_string_lossy().into_owned())
                .collect();
            if let Some(at) = order.iter().position(|p| *p == dir)
                && let Some(to) = at
                    .checked_add_signed(by as isize)
                    .filter(|&to| to < order.len())
            {
                order.swap(at, to);
                shared.libraries.edit(|s| {
                    s.order = order;
                    s.sort = crate::library::Sort::Custom;
                });
            }
        }
        Command::ResetLibraryOrder => shared.libraries.edit(|s| {
            s.order.clear();
            s.sort = crate::library::Sort::Name;
        }),
        Command::SaveMulti => {
            // A saved multi offers its own name back; anything else starts blank.
            let current = &cx.selection.multi;
            let name = if crate::library::is_multi(Path::new(current)) {
                super::header::stem(current)
            } else {
                String::new()
            };
            let from = super::header::multi_path(&super::header::root(cx), "x")
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let ask = super::picker::Ask::Multi {
                from,
                name: if name.is_empty() {
                    "Multi".into()
                } else {
                    name.clone()
                },
            };
            if !cx.state.picker.ask(ask) {
                cx.state.saving = Some(name);
                cx.state.save_error.clear();
            }
        }
        Command::Browser => cx.state.browser ^= true,
        Command::Logs => {
            cx.state.tab = super::Tab::Report;
            cx.state.settings = false;
            cx.state.logs.show_entries();
        }
        Command::About => {
            cx.state.settings = false;
            cx.state.tab = super::Tab::Report;
            cx.state.logs.about = true;
        }
        Command::Appearance(look) => cx.selection.appearance = look as u8,
        Command::ArtworkBlur => cx.selection.sharp_artwork ^= true,
        Command::StickyHeaders => cx.selection.sticky_off ^= true,
        Command::Streaming(mode) => cx.selection.streaming = mode,
        Command::PartStreaming(slot, mode) => {
            if let Some(part) = cx.selection.parts.get_mut(slot) {
                part.streaming = mode;
            }
        }
        Command::MpeZone(slot, zone) => {
            let part = &mut cx.selection.parts[slot];
            part.mpe = zone != 0;
            part.mpe_upper = zone == 2;
            if part.mpe && part.bend_range == 0 {
                part.bend_range = 48;
            }
        }
        Command::DefaultView(slot, mode) => {
            shared.libraries.edit(|s| s.view_mode = mode);
            cx.selection.parts[slot].view = 0;
            shared.libraries.edit(|s| {
                s.instrument_views.remove(&cx.selection.parts[slot].path);
            });
        }
        Command::AutoAlign => cx.selection.auto_align ^= true,
        Command::AlignTransportOnly => cx.selection.align_transport_only ^= true,
        Command::Lateness(slot, ms) => cx.selection.parts[slot].timing.override_ms = ms,
        Command::ExcludeTiming(slot) => cx.selection.parts[slot].timing.exclude ^= true,
        Command::Remeasure(slot) => cx.selection.parts[slot].timing.source.clear(),
        Command::MemoryBudget(mb) => cx.selection.memory_budget_mb = mb,
        Command::Keyboard => cx.state.keyboard ^= true,
        Command::Panic => shared
            .panic
            .store(true, std::sync::atomic::Ordering::Release),
        Command::Channel(slot, channel) => cx.selection.parts[slot].channel = channel,
        Command::Port(slot, port) => cx.selection.parts[slot].port = port,
        Command::Mpe(slot) => {
            let part = &mut cx.selection.parts[slot];
            part.mpe = !part.mpe;
            // MPE controllers bend ±48 on member channels by default.
            if part.mpe && part.bend_range == 0 {
                part.bend_range = 48;
            }
        }
        Command::BendRange(slot, n) => cx.selection.parts[slot].bend_range = n,
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
            cx.selection
                .parts
                .iter_mut()
                .for_each(|p| p.output_manual = false);
        }
        Command::OwnChannels => crate::routing::own_channels(&mut cx.selection),
        Command::AllOmni => crate::routing::all_omni(&mut cx.selection),
        Command::NameOutputs => crate::routing::name_outputs(&mut cx.selection),
        Command::ResetRouting => crate::routing::reset(&mut cx.selection),
    }
}

/// Show `path` in the system's file manager.
/// Called by the owned file-operation worker, never while painting.
pub fn reveal(path: &Path) -> Result<(), String> {
    let target = reveal_target(path);
    let result = match &target {
        Ok((target, directory)) => reveal_native(target, *directory),
        Err(error) => Err(error.clone()),
    };
    crate::diagnostics::event(
        if result.is_err() {
            crate::diagnostics::LogLevel::Warning
        } else {
            crate::diagnostics::LogLevel::Info
        },
        "browser",
        if result.is_err() {
            "reveal_failed"
        } else {
            "reveal_started"
        },
        serde_json::json!({"path":path, "resolved":target.as_ref().ok().map(|(p, _)| p), "reason":result.as_ref().err()}),
    );
    result
}

fn reveal_target(path: &Path) -> Result<(std::path::PathBuf, bool), String> {
    let target = path
        .canonicalize()
        .map_err(|error| format!("Could not reveal {}: {error}", path.display()))?;
    let metadata = target
        .metadata()
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    if !(metadata.is_file() || metadata.is_dir()) {
        return Err(format!(
            "Could not reveal {}: this is neither a file nor a folder",
            path.display()
        ));
    }
    Ok((target, metadata.is_dir()))
}

fn reveal_native(target: &Path, directory: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let spawned = {
        let mut command = std::process::Command::new("open");
        if !directory {
            command.arg("-R");
        }
        command.arg(&target).spawn()
    };
    #[cfg(target_os = "windows")]
    let spawned = {
        use std::os::windows::{
            ffi::{OsStrExt, OsStringExt},
            process::CommandExt,
        };
        let units = windows_shell_units(&target.as_os_str().encode_wide().collect::<Vec<_>>())
            .map_err(|error| format!("Could not reveal {}: {error}", target.display()))?;
        let mut command = std::process::Command::new("explorer.exe");
        if directory {
            command.arg(std::ffi::OsString::from_wide(&units));
        } else {
            // Explorer parses its own comma syntax. Quoting the whole /select
            // argument via Command::arg can send a spaced path to Documents.
            // Windows file names cannot contain a quote; canonicalization has
            // already validated this filesystem path.
            let mut selected: Vec<u16> = "/select,\"".encode_utf16().collect();
            selected.extend_from_slice(&units);
            selected.push(b'"' as u16);
            command.raw_arg(std::ffi::OsString::from_wide(&selected));
        }
        command.spawn()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let spawned = std::process::Command::new("xdg-open")
        .arg(if directory {
            target
        } else {
            target.parent().unwrap_or(target)
        })
        .spawn();
    let mut child = spawned.map_err(|error| {
        format!(
            "Could not start the file manager for {}: {error}",
            target.display()
        )
    })?;
    // Explorer may hand this request to an existing shell and its exit code
    // does not confirm which window opened. Closing its handle is sufficient
    // on Windows; Unix children are reaped by this owned worker.
    #[cfg(not(target_os = "windows"))]
    {
        let status = child
            .wait()
            .map_err(|error| format!("File manager failed for {}: {error}", target.display()))?;
        if !status.success() {
            return Err(format!(
                "File manager could not reveal {}: {status}",
                target.display()
            ));
        }
    }
    #[cfg(target_os = "windows")]
    let _ = &mut child;
    Ok(())
}

#[cfg(any(test, target_os = "windows"))]
fn windows_shell_units(path: &[u16]) -> Result<Vec<u16>, String> {
    // The filesystem's extended namespace is not Explorer's shell namespace.
    let slash = b'\\' as u16;
    if path.starts_with(&[slash, slash, b'.' as u16, slash]) {
        return Err("Reveal cannot open a Windows device path".into());
    }
    if !path.starts_with(&[slash, slash, b'?' as u16, slash]) {
        return Ok(path.to_vec());
    }
    let rest = &path[4..];
    if rest.len() >= 4
        && rest[..4]
            .iter()
            .zip(b"UNC\\")
            .all(|(&u, &b)| u == b as u16 || u == b.to_ascii_lowercase() as u16)
    {
        let mut unc = vec![slash, slash];
        unc.extend_from_slice(&rest[4..]);
        return Ok(unc);
    }
    if rest.len() >= 3
        && matches!(rest[0], 65..=90 | 97..=122)
        && rest[1] == b':' as u16
        && rest[2] == slash
    {
        return Ok(rest.to_vec());
    }
    Err("Reveal cannot open this Windows extended path namespace".into())
}

#[cfg(test)]
mod reveal_tests {
    use super::*;

    #[test]
    fn reveal_keeps_native_folders_and_rejects_missing_targets() {
        let dir = std::env::temp_dir()
            .join(format!("kontra-reveal-{}", std::process::id()))
            .join("Library With Spaces – Bells");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Preset.nki");
        std::fs::write(&file, []).unwrap();
        assert_eq!(
            reveal_target(&dir).unwrap(),
            (dir.canonicalize().unwrap(), true)
        );
        assert_eq!(
            reveal_target(&file).unwrap(),
            (file.canonicalize().unwrap(), false)
        );
        let missing = dir.join("Not Here.nki");
        assert!(
            reveal_target(&missing)
                .unwrap_err()
                .contains(&missing.display().to_string())
        );
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn explorer_paths_preserve_drive_unc_spaces_and_unicode() {
        for (from, to) in [
            (r"\\?\D:\Sound Sets\Café Bells", r"D:\Sound Sets\Café Bells"),
            (
                r"\\?\UNC\server\share\Sound Sets\Bells",
                r"\\server\share\Sound Sets\Bells",
            ),
            (r"\\server\share\Sound Sets", r"\\server\share\Sound Sets"),
            (r"C:\Sound Sets", r"C:\Sound Sets"),
        ] {
            let units: Vec<_> = from.encode_utf16().collect();
            assert_eq!(
                windows_shell_units(&units).unwrap(),
                to.encode_utf16().collect::<Vec<_>>()
            );
        }
        for path in [r"\\.\PhysicalDrive0", r"\\?\Volume{opaque}\"] {
            assert!(windows_shell_units(&path.encode_utf16().collect::<Vec<_>>()).is_err());
        }
        let mut native: Vec<_> = r"\\?\C:\Library\".encode_utf16().collect();
        native.push(0xd800); // Native Windows names need not be valid Rust UTF-8.
        assert_eq!(windows_shell_units(&native).unwrap(), native[4..]);
    }
}
