//! The editor driven by the pointer, headless: a click drills into a
//! library, a click on a preset asks to load it, a menu pick lands, and
//! once motion settles the editor asks for no more frames.

use kontra_ui::app::{
    self, Action, App, Envelope, Face, Format, Library, Param, Part, PartTab, Sound,
};
use mui::prelude::{Buttons, Input, Key, KeyPress, Mods, Point, PointerInput, Size, Ui};

fn frame(ui: &mut Ui, app: &mut App, input: Input) -> (Vec<Action>, bool) {
    let r = app::view(ui, app);
    let f = ui.frame(r.el, Some(Size::new(900., 600.)), input, 1. / 60.);
    (r.changed, f.is_ok_and(|f| f.animating))
}

/// Frames until nothing moves; the actions they carried (a click lands the
/// frame after its release).
fn settle(ui: &mut Ui, app: &mut App) -> Vec<Action> {
    let mut out = Vec::new();
    for _ in 0..240 {
        let (actions, moving) = frame(ui, app, Input::default());
        out.extend(actions);
        if !moving {
            break;
        }
    }
    out
}

fn click(ui: &mut Ui, app: &mut App, id: &str) -> Vec<Action> {
    clicks(ui, app, id, 1)
}

/// `n` clicks on `id` in quick succession (2: a double-click).
fn clicks(ui: &mut Ui, app: &mut App, id: &str, n: usize) -> Vec<Action> {
    // A missing target clicks nothing, and the asserts after it say so.
    let Some(s) = ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame) else {
        return Vec::new();
    };
    let at = Point::new(s.x + s.size.width / 2., s.y + s.size.height / 2.);
    let mut out = Vec::new();
    let presses = [Buttons::PRIMARY, Buttons::default()].repeat(n);
    for buttons in presses {
        let pointer = PointerInput {
            pos: Some(at),
            buttons,
            ..Default::default()
        };
        out.extend(
            frame(
                ui,
                app,
                Input {
                    pointer,
                    ..Default::default()
                },
            )
            .0,
        );
    }
    out.extend(settle(ui, app));
    out
}

#[test]
fn clicks_drive_the_editor_and_idle_costs_no_frames() {
    let mut ui = kontra_ui::ui();
    let mut app = App {
        libraries: vec![Library {
            name: "Areia".into(),
            format: Format::Kontakt,
            presets: vec!["Violins".into()],
            ..Library::default()
        }],
        ..App::default()
    };
    settle(&mut ui, &mut app);
    assert!(click(&mut ui, &mut app, "library-0").is_empty());
    assert_eq!(app.view.library, Some(0));
    // A click selects; a double-click loads.
    assert!(click(&mut ui, &mut app, "preset-0").is_empty());
    assert_eq!(app.view.picked, Some((0, 0)));
    assert_eq!(
        clicks(&mut ui, &mut app, "preset-0", 2),
        vec![Action::Load {
            library: 0,
            preset: 0,
            into: None
        }]
    );
    app.parts.push(Part {
        name: "Violins".into(),
        channel: 1,
        ..Part::default()
    });
    settle(&mut ui, &mut app);
    click(&mut ui, &mut app, "part-0-channel");
    assert_eq!(app.view.menu.as_deref(), Some("part-0-channel"));
    assert_eq!(
        click(&mut ui, &mut app, "part-0-channel/list/0"),
        vec![Action::Channel(0, 0)],
        "the first cell is every channel"
    );
    assert_eq!(app.view.menu, None, "a pick closes the menu");
    click(&mut ui, &mut app, "part-0-output");
    assert_eq!(
        click(&mut ui, &mut app, "part-0-output/list/2"),
        vec![Action::Output(0, 2)]
    );
    click(&mut ui, &mut app, "tab-Mixer");
    assert_eq!(
        click(&mut ui, &mut app, "mix-0-sm-mute"),
        vec![Action::Mute(0, true)],
        "the mixer's strip drives the same part"
    );
    assert!(
        !frame(&mut ui, &mut app, Input::default()).1,
        "settled: the host draws nothing until something changes"
    );
}

#[test]
fn a_dragged_envelope_handle_sets_every_group() {
    let mut ui = kontra_ui::ui();
    let sound = Sound {
        envelope: Some(Envelope {
            attack: 0.1,
            decay: 0.5,
            sustain: 0.5,
            release: 0.4,
            ..Envelope::default()
        }),
        ..Sound::default()
    };
    let mut app = App {
        parts: vec![Part {
            name: "Violins".into(),
            groups: vec!["Long".into(), "Short".into()],
            sounds: vec![sound.clone(), sound],
            open: true,
            tab: PartTab::Sound,
            ..Part::default()
        }],
        ..App::default()
    };
    settle(&mut ui, &mut app);
    let f = ui
        .scene()
        .and_then(|s| s.surface("part-0-envelope"))
        .map(|s| s.frame);
    assert!(f.is_some(), "the envelope graph is on screen");
    let Some(f) = f else { return };
    // The decay handle: 0.6 s of a 1.25 s span, at the sustain; the graph
    // is inset by 8 points.
    let (w, h) = (f.size.width - 16., f.size.height - 16.);
    let at = |x: f64, y: f64| Point::new(f.x + 8. + x * w, f.y + 8. + (1. - y) * h);
    let press = |pos, buttons| Input {
        pointer: PointerInput {
            pos: Some(pos),
            buttons,
            ..Default::default()
        },
        ..Default::default()
    };
    let start = at(0.6 / 1.25, 0.5);
    let mut out = Vec::new();
    for (pos, b) in [
        (start, Buttons::PRIMARY),
        (at(0.8 / 1.25, 0.25), Buttons::PRIMARY),
        (at(0.8 / 1.25, 0.25), Buttons::PRIMARY),
        (at(0.8 / 1.25, 0.25), Buttons::default()),
    ] {
        out.extend(frame(&mut ui, &mut app, press(pos, b)).0);
    }
    out.extend(settle(&mut ui, &mut app));
    let sustain = |g: usize| {
        app.parts
            .first()
            .and_then(|p| p.sounds.get(g))
            .and_then(|s| s.envelope)
            .map(|e| e.sustain)
    };
    assert!(out.iter().any(|a| matches!(a, Action::Sound { group: None, param: Param::Decay, value, .. } if (value - 0.7).abs() < 0.02)), "{out:?}");
    assert!((sustain(0).unwrap_or(0.) - 0.25).abs() < 0.02);
    assert_eq!(sustain(0), sustain(1), "All groups: both move");
}

/// Two rack parts showing their libraries' faces in one frame: each control
/// takes its own surface, and the frame lays out (two faces once shared
/// their ids, and the Rack stopped drawing: "two nodes share the id").
#[test]
fn two_open_faces_lay_out_side_by_side() {
    use kontra_ui_model::{Interface, Kind, Page, PageRef, Range, Rect, Size as PageSize, Widget};
    let mut tree = Interface::default();
    tree.pages.push(Page {
        size: PageSize {
            width: 300.,
            height: 100.,
        },
        ..Page::default()
    });
    let range = Range {
        max: 1.,
        ..Range::default()
    };
    let knob = Kind::Knob {
        range,
        display: kontra_ui_model::Display::default(),
    };
    tree.widgets.push(Widget::new(
        "$k",
        PageRef(0),
        Rect::new(10, 10, 40, 40),
        knob,
    ));
    let part = || Part {
        name: "Strings".into(),
        face: Some(Face {
            tree: tree.clone(),
            pictures: kontra_ui::Pictures::new(Box::new(|_| None)),
            gesture: kontra_ui::input::Gesture::default(),
            meters: Default::default(),
        }),
        open: true,
        tab: PartTab::Interface,
        ..Part::default()
    };
    let mut ui = kontra_ui::ui();
    let mut app = App {
        parts: vec![part(), part()],
        ..App::default()
    };
    let r = app::view(&mut ui, &mut app);
    let laid = ui.frame(
        r.el,
        Some(Size::new(900., 900.)),
        Input::default(),
        1. / 60.,
    );
    assert!(laid.is_ok(), "{:?}", laid.err());
    for n in 0..2 {
        let id = kontra_ui::input::surface(n, 0);
        assert!(ui.scene().and_then(|s| s.surface(&id)).is_some(), "{id}");
    }
}

#[test]
fn a_small_window_keeps_the_keyboard_in_it() {
    use kontra_ui_model::{Interface, Page, Size as PageSize};
    let mut tree = Interface::default();
    tree.pages.push(Page {
        size: PageSize {
            width: 1000.,
            height: 700.,
        },
        ..Page::default()
    });
    let mut ui = kontra_ui::ui();
    let mut app = App {
        parts: vec![Part {
            name: "Strings".into(),
            face: Some(Face {
                tree,
                pictures: kontra_ui::Pictures::new(Box::new(|_| None)),
                gesture: kontra_ui::input::Gesture::default(),
                meters: Default::default(),
            }),
            open: true,
            tab: PartTab::Interface,
            ..Part::default()
        }],
        ..App::default()
    };
    let window = Size::new(900., 560.);
    for _ in 0..3 {
        let r = app::view(&mut ui, &mut app);
        let laid = ui.frame(r.el, Some(window), Input::default(), 1. / 60.);
        assert!(laid.is_ok(), "{:?}", laid.err());
    }
    assert!((app.width - window.width).abs() < 0.5, "{}", app.width);
    let bed = ui.scene().and_then(|s| s.layout.frame("keybed"));
    assert!(bed.is_some(), "no keybed");
    let Some(bed) = bed else { return };
    let (x1, y1) = (bed.x + bed.size.width, bed.y + bed.size.height);
    assert!(bed.x >= 0. && x1 <= window.width + 0.5, "{bed:?}");
    assert!(bed.y >= 0. && y1 <= window.height + 0.5, "{bed:?}");
}

#[test]
fn a_face_in_the_rack_takes_the_pointer() {
    use kontra_ui_model::{Interface, Kind, Page, PageRef, Rect, Size as PageSize, Widget};
    let mut tree = Interface::default();
    tree.pages.push(Page {
        size: PageSize {
            width: 600.,
            height: 300.,
        },
        ..Page::default()
    });
    tree.widgets.push(Widget::new(
        "$b",
        PageRef(0),
        Rect::new(100, 100, 40, 40),
        Kind::Button { momentary: false },
    ));
    let mut ui = kontra_ui::ui();
    let mut app = App {
        parts: vec![Part {
            name: "Piano".into(),
            face: Some(Face {
                tree,
                pictures: kontra_ui::Pictures::new(Box::new(|_| None)),
                gesture: kontra_ui::input::Gesture::default(),
                meters: Default::default(),
            }),
            open: true,
            tab: PartTab::Interface,
            ..Part::default()
        }],
        ..App::default()
    };
    settle(&mut ui, &mut app);
    let id = kontra_ui::input::surface(0, 0);
    let out = click(&mut ui, &mut app, &id);
    assert!(
        out.iter().any(|a| matches!(a, Action::Control { .. })),
        "{out:?}"
    );
}

/// Pointer frames pressing on `from`, moving to `to` and releasing there.
fn drag(ui: &mut Ui, app: &mut App, from: &str, to: &str) -> Vec<Action> {
    let centre = |ui: &Ui, id: &str| {
        let f = ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame)?;
        Some(Point::new(
            f.x + f.size.width / 2.,
            f.y + f.size.height / 2.,
        ))
    };
    let (Some(a), Some(b)) = (centre(ui, from), centre(ui, to)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let steps = 8;
    for k in 0..=steps + 1 {
        let t = f64::from(k.min(steps)) / f64::from(steps);
        let at = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
        let buttons = if k <= steps {
            Buttons::PRIMARY
        } else {
            Buttons::default()
        };
        let pointer = PointerInput {
            pos: Some(at),
            buttons,
            ..Default::default()
        };
        let input = Input {
            pointer,
            ..Default::default()
        };
        out.extend(frame(ui, app, input).0);
    }
    out.extend(settle(ui, app));
    out
}

#[test]
fn a_preset_dragged_onto_a_part_replaces_it_and_onto_free_space_adds_one() {
    let mut ui = kontra_ui::ui();
    let mut app = App {
        libraries: vec![Library {
            name: "Areia".into(),
            format: Format::Kontakt,
            presets: vec!["Violins".into(), "Violas".into()],
            ..Library::default()
        }],
        parts: vec![Part {
            name: "Violins".into(),
            library: "Areia".into(),
            ..Part::default()
        }],
        ..App::default()
    };
    settle(&mut ui, &mut app);
    click(&mut ui, &mut app, "library-0");
    let onto_part = drag(&mut ui, &mut app, "preset-1", "part-0-band");
    assert!(
        onto_part.contains(&Action::Replace {
            part: 0,
            library: 0,
            preset: 1
        }),
        "{onto_part:?}"
    );
    let onto_rack = drag(&mut ui, &mut app, "preset-1", "rack");
    assert!(
        onto_rack.contains(&Action::Load {
            library: 0,
            preset: 1,
            into: None
        }),
        "{onto_rack:?}"
    );
}

/// The edge between the libraries and the presets drags: down gives the
/// libraries more of the browser's height, and the view keeps the share.
#[test]
fn the_browser_split_drags_and_keeps_its_share() {
    let mut ui = kontra_ui::ui();
    let mut app = App::default();
    settle(&mut ui, &mut app);
    let at = (ui.scene())
        .and_then(|s| s.surface("browser-split"))
        .map(|s| s.frame);
    assert!(at.is_some(), "the browser has a split edge");
    let Some(f) = at else { return };
    let (x, y) = (f.x + f.size.width / 2., f.y + f.size.height / 2.);
    for k in 0..=10 {
        let pointer = PointerInput {
            pos: Some(Point::new(x, y + f64::from(k.min(9)) * 10.)),
            buttons: if k < 10 {
                Buttons::PRIMARY
            } else {
                Buttons::default()
            },
            ..Default::default()
        };
        let input = Input {
            pointer,
            ..Default::default()
        };
        frame(&mut ui, &mut app, input);
    }
    assert!(app.view.split > 0.55, "dragged down: {}", app.view.split);
    assert!(
        app.view.split <= 0.7,
        "held to v2's bound: {}",
        app.view.split
    );
}

/// With a part playing, a double-click names it as the part to play in;
/// the host plays only a preset of an instrument there (the plugin test).
#[test]
fn a_double_click_names_the_focused_part() {
    let mut ui = kontra_ui::ui();
    let mut app = App {
        libraries: vec![Library {
            name: "Strings".into(),
            format: Format::Kontakt,
            presets: vec!["Snapshots/Airy".into(), "Strings".into()],
            recalls: vec![0],
            ..Library::default()
        }],
        parts: vec![Part {
            name: "Strings".into(),
            library: "Strings".into(),
            ..Part::default()
        }],
        ..App::default()
    };
    settle(&mut ui, &mut app);
    click(&mut ui, &mut app, "library-0");
    let load = |preset, into| Action::Load {
        library: 0,
        preset,
        into,
    };
    click(&mut ui, &mut app, "preset-0");
    assert_eq!(
        clicks(&mut ui, &mut app, "preset-0", 2),
        vec![load(0, Some(0))]
    );
    click(&mut ui, &mut app, "preset-1");
    assert_eq!(
        clicks(&mut ui, &mut app, "preset-1", 2),
        vec![load(1, Some(0))]
    );
}

/// One key, then frames until the editor settles.
fn key(ui: &mut Ui, app: &mut App, key: Key) -> Vec<Action> {
    let mods = Mods::default();
    let keys = vec![KeyPress { key, mods }];
    let (mut out, _) = frame(
        ui,
        app,
        Input {
            keys,
            ..Default::default()
        },
    );
    out.extend(settle(ui, app));
    out
}

/// The browser by keys alone: Down from the search into the presets, Down
/// along them, Enter loads; typing on a row searches, Escape clears it.
#[test]
fn the_presets_walk_and_load_by_keys() {
    let mut ui = kontra_ui::ui();
    let presets = ["Strings Legato", "Strings Pizzicato", "Strings Staccato"];
    let mut app = App {
        libraries: vec![Library {
            name: "Strings".into(),
            format: Format::Kontakt,
            presets: presets.iter().map(|p| format!("Instruments/{p}")).collect(),
            ..Library::default()
        }],
        ..App::default()
    };
    settle(&mut ui, &mut app);
    click(&mut ui, &mut app, "library-0");
    ui.focus("preset-search");
    key(&mut ui, &mut app, Key::Down);
    key(&mut ui, &mut app, Key::Down);
    assert_eq!(app.view.picked, Some((0, 1)));
    let load = Action::Load {
        library: 0,
        preset: 1,
        into: None,
    };
    assert_eq!(key(&mut ui, &mut app, Key::Enter), vec![load]);
    let text = "stac".to_owned();
    let (_, _) = frame(
        &mut ui,
        &mut app,
        Input {
            text,
            ..Default::default()
        },
    );
    settle(&mut ui, &mut app);
    assert_eq!(app.view.preset_search, "stac");
    key(&mut ui, &mut app, Key::Escape);
    assert_eq!(app.view.preset_search, "");
}

/// MUI's built-in layout/accessibility/occlusion/idle-rebuild audit across
/// KONTRA's design size and key tabs/sheets.
#[test]
fn mui_audit_passes_across_sizes_and_views() {
    let sizes = [Size::new(1180., 780.), Size::new(1475., 975.)];

    // 1. Default Rack view with a loaded library and part.
    let mut ui = kontra_ui::ui();
    let mut app = App {
        libraries: vec![Library {
            name: "Areia".into(),
            format: Format::Kontakt,
            presets: vec!["Violins".into(), "Celli".into()],
            ..Library::default()
        }],
        parts: vec![Part {
            name: "Violins".into(),
            library: "Areia".into(),
            channel: 1,
            output: 1,
            open: true,
            ..Part::default()
        }],
        folders: vec!["/music/libraries".into()],
        build: "Version 3.0.0 · test".into(),
        ..App::default()
    };
    let findings = mui::audit::audit(&mut ui, sizes, |ui| app::view(ui, &mut app).el);
    assert!(
        findings.is_empty(),
        "Rack view audit findings:\n{}",
        findings
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );

    // 2. Mixer tab.
    app.view.tab = app::Tab::Mixer;
    let findings = mui::audit::audit(&mut ui, sizes, |ui| app::view(ui, &mut app).el);
    assert!(
        findings.is_empty(),
        "Mixer view audit findings:\n{}",
        findings
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );

    // 3. Report tab.
    app.view.tab = app::Tab::Report;
    app.report.push("Sample notice for audit".into());
    let findings = mui::audit::audit(&mut ui, sizes, |ui| app::view(ui, &mut app).el);
    assert!(
        findings.is_empty(),
        "Report view audit findings:\n{}",
        findings
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );

    // 4. Settings and About sheets (modal over main view).
    app.view.tab = app::Tab::Rack;
    for (name, sheet) in [
        ("Settings", app::Sheet::Settings),
        ("About", app::Sheet::About),
    ] {
        app.view.sheet = Some(sheet);
        let findings = mui::audit::audit(&mut ui, [Size::new(1180., 780.)], |ui| {
            app::view(ui, &mut app).el
        });
        assert!(
            findings.is_empty(),
            "{name} sheet audit findings:\n{}",
            findings
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    // 5. Part sub-tabs (Sound, Mapping, Articulations, Info) & collapsed browser.
    app.view.sheet = None;
    app.view.browser_hidden = true;
    for tab in [
        PartTab::Sound,
        PartTab::Mapping,
        PartTab::Articulations,
        PartTab::Info,
    ] {
        if let Some(p) = app.parts.first_mut() {
            p.tab = tab;
        }
        let findings = mui::audit::audit(&mut ui, [Size::new(1180., 780.)], |ui| {
            app::view(ui, &mut app).el
        });
        assert!(
            findings.is_empty(),
            "PartTab::{tab:?} audit findings:\n{}",
            findings
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

#[test]
fn protected_library_surfaces_lock_badge_tooltip_banner_and_passes_audit() {
    let mut ui = kontra_ui::ui();
    let mut app = App {
        libraries: vec![
            Library {
                name: "Locked Falcon Bank".into(),
                format: Format::Uvi,
                protection: app::Protection::Protected {
                    system: "PACE / iLok".into(),
                    detail: "PACE/iLok-encrypted UFS bank requires UVI Workstation/Falcon license"
                        .into(),
                },
                presets: Vec::new(),
                ..Library::default()
            },
            Library {
                name: "Open Falcon Bank".into(),
                format: Format::Uvi,
                protection: app::Protection::Open,
                presets: vec!["Pads/Warm Pad".into()],
                ..Library::default()
            },
        ],
        ..App::default()
    };
    app.view.format = Format::Uvi;
    settle(&mut ui, &mut app);

    // Card carries locked status in its accessibility name and hover tooltip.
    let card_surface = ui.scene().and_then(|s| s.surface("library-0")).cloned();
    assert!(card_surface.is_some(), "protected library card is rendered");
    let card_frame = card_surface.as_ref().map(|s| s.frame);
    if let Some(f) = card_frame {
        let hover = PointerInput {
            pos: Some(Point::new(
                f.x + f.size.width / 2.,
                f.y + f.size.height / 2.,
            )),
            ..Default::default()
        };
        let mut tip = None;
        for _ in 0..3 {
            let r = app::view(&mut ui, &mut app);
            let res = ui.frame(
                r.el,
                Some(Size::new(900., 600.)),
                Input {
                    pointer: hover,
                    ..Default::default()
                },
                0.5,
            );
            tip = res.ok().and_then(|out| out.tip.map(|(t, _)| t));
        }
        assert!(
            tip.as_deref()
                .is_some_and(|t| t.contains("Locked (PACE / iLok)") && t.contains("iLok")),
            "expected lock tooltip on protected library card, got {tip:?}"
        );
    }

    // Selecting the protected library surfaces the banner pill and empty-presets lock notice.
    click(&mut ui, &mut app, "library-0");
    assert_eq!(app.view.library, Some(0));
    assert!(
        ui.scene()
            .and_then(|s| s.surface("library-protection-badge"))
            .is_some(),
        "selected protected library banner should show lock badge"
    );
    assert!(
        ui.scene()
            .and_then(|s| s.surface("library-protection-notice"))
            .is_some(),
        "empty protected library should show protection notice instead of generic empty text"
    );

    // Both Grid and Cards browser layouts pass MUI audit cleanly with protected libraries.
    for tiles in [app::Tiles::Grid, app::Tiles::Cards] {
        app.view.tiles = tiles;
        let findings = mui::audit::audit(&mut ui, [Size::new(1180., 780.)], |ui| {
            app::view(ui, &mut app).el
        });
        assert!(
            findings.is_empty(),
            "Protected library ({tiles:?}) audit findings:\n{}",
            findings
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

/// Pointer frames pressing `id` at `from` and releasing at `to`, each a
/// share (0..1, 0..1) of its frame; the actions they carried.
fn sweep(ui: &mut Ui, app: &mut App, id: &str, from: (f64, f64), to: (f64, f64)) -> Vec<Action> {
    // A missing target sweeps nothing, and the asserts after it say so.
    let Some(f) = ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame) else {
        return Vec::new();
    };
    let at = |(x, y): (f64, f64)| Point::new(f.x + f.size.width * x, f.y + f.size.height * y);
    let mut out = Vec::new();
    let steps = 8;
    for k in 0..=steps + 1 {
        let t = f64::from(k.min(steps)) / f64::from(steps);
        let p = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        let buttons = if k <= steps {
            Buttons::PRIMARY
        } else {
            Buttons::default()
        };
        let pointer = PointerInput {
            pos: Some(at(p)),
            buttons,
            ..Default::default()
        };
        let input = Input {
            pointer,
            ..Default::default()
        };
        out.extend(frame(ui, app, input).0);
    }
    out.extend(settle(ui, app));
    out
}

fn one_part() -> App {
    App {
        parts: vec![Part {
            name: "Violins".into(),
            library: "Areia".into(),
            channel: 1,
            output: 1,
            ..Part::default()
        }],
        ..App::default()
    }
}

/// The classic controls are on screen, pass MUI's audit at both sizes, and
/// each drives what it drove: master gain, pan, pitch bend and CC 1.
#[test]
fn the_classic_controls_are_back_and_drive_their_params() {
    let sizes = [Size::new(1180., 780.), Size::new(1475., 975.)];
    let mut ui = kontra_ui::ui();
    let mut app = one_part();
    let findings = mui::audit::audit(&mut ui, sizes, |ui| app::view(ui, &mut app).el);
    assert!(findings.is_empty(), "{findings:?}");
    settle(&mut ui, &mut app);
    let scene = ui.scene().unwrap();
    for id in ["master", "pitch", "modulation", "part-0-pan"] {
        assert!(scene.surface(id).is_some(), "{id} is on screen");
    }

    // Master: the level is where the wedge is pressed, by amplitude; the
    // middle is 0 dB and the end +6 dB.
    let out = sweep(&mut ui, &mut app, "master", (0.25, 0.5), (0.5, 0.5));
    let last = out.iter().rev().find_map(|a| match a {
        Action::Master(db) => Some(*db),
        _ => None,
    });
    let db = last.expect("the wedge set the master");
    assert!(db.abs() < 0.3, "the middle is 0 dB: {db}");
    assert!(
        out.iter()
            .any(|a| matches!(a, Action::Master(d) if *d < -3.)),
        "{out:?}"
    );
    assert!((app.master - db).abs() < 1e-12);
    // The far end (just inside it: the edge itself is outside the frame).
    sweep(&mut ui, &mut app, "master", (0.9, 0.5), (0.999, 0.5));
    assert!(
        (app.master - kontra_ui::kit::WEDGE_TOP_DB).abs() < 0.05,
        "{}",
        app.master
    );

    // Pan: a drag left pans left.
    let out = sweep(&mut ui, &mut app, "part-0-pan", (0.5, 0.5), (0.1, 0.5));
    assert!(
        out.iter()
            .any(|a| matches!(a, Action::Pan(0, p) if *p < -0.5)),
        "{out:?}"
    );

    // Pitch: up bends up, and the release springs it back to 0.
    let out = sweep(&mut ui, &mut app, "pitch", (0.5, 0.5), (0.5, 0.1));
    assert!(
        out.iter()
            .any(|a| matches!(a, Action::Pitch(p) if *p > 0.5)),
        "{out:?}"
    );
    assert!(
        matches!(out.iter().rev().find(|a| matches!(a, Action::Pitch(_))), Some(Action::Pitch(p)) if *p == 0.)
    );

    // Mod wheel: up raises CC 1, and it stays.
    let out = sweep(&mut ui, &mut app, "modulation", (0.5, 0.9), (0.5, 0.1));
    let m = out.iter().rev().find_map(|a| match a {
        Action::Modulation(m) => Some(*m),
        _ => None,
    });
    assert!(m.is_some_and(|m| m > 0.6), "{out:?}");
}

/// A value set from outside (a preset, automation) is drawn at once: the
/// controls hold no glide, so nothing is left animating.
#[test]
fn the_controls_draw_a_value_from_outside_without_a_glide() {
    let mut ui = kontra_ui::ui();
    let mut app = one_part();
    settle(&mut ui, &mut app);
    app.master = -20.;
    app.parts[0].pan = 1.;
    app.parts[0].volume = -30.;
    let r = app::view(&mut ui, &mut app);
    let f = ui
        .frame(
            r.el,
            Some(Size::new(900., 600.)),
            Input::default(),
            1. / 60.,
        )
        .expect("the moved controls render");
    assert!(!f.animating, "a moved value animates nothing");
}

/// From silence an arrow key still raises the master one whole step.
#[test]
fn an_arrow_raises_the_master_from_silence() {
    let mut ui = kontra_ui::ui();
    let mut app = one_part();
    app.master = -60.;
    settle(&mut ui, &mut app);
    ui.focus("master");
    let out = key(&mut ui, &mut app, Key::Right);
    assert!(app.master > -60., "{out:?}");
    assert!(
        out.iter().any(|a| matches!(a, Action::Master(_))),
        "{out:?}"
    );
    // Just under a grid point, Left still lowers it.
    app.master = kontra_ui::kit::wedge_db(0.524);
    settle(&mut ui, &mut app);
    let before = app.master;
    key(&mut ui, &mut app, Key::Left);
    assert!(app.master < before, "{before} -> {}", app.master);
}

/// A double click puts the master back to 0 dB, and holding the second
/// press still leaves it there.
#[test]
fn a_double_click_resets_the_master_and_a_still_hold_keeps_it() {
    let mut ui = kontra_ui::ui();
    let mut app = one_part();
    settle(&mut ui, &mut app);
    let f = ui.scene().unwrap().surface("master").unwrap().frame;
    let pos = Some(Point::new(
        f.x + f.size.width * 0.2,
        f.y + f.size.height * 0.5,
    ));
    let press = |down: bool| Input {
        pointer: PointerInput {
            pos,
            buttons: if down {
                Buttons::PRIMARY
            } else {
                Buttons::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    for down in [true, false, true, true, true, true, false] {
        frame(&mut ui, &mut app, press(down));
    }
    settle(&mut ui, &mut app);
    assert_eq!(app.master, 0., "the double click's 0 dB held");
}

#[test]
fn the_wedge_reads_by_amplitude_like_vlc() {
    use kontra_ui::kit::{wedge_at, wedge_db, wedge_text};
    assert_eq!(wedge_at(-60.), 0.);
    assert!((wedge_at(0.) - 0.5).abs() < 0.002);
    assert_eq!(wedge_at(6.), 1.);
    for t in [0.1, 0.25, 0.5, 0.75, 1.] {
        assert!((wedge_at(wedge_db(t)) - t).abs() < 1e-9, "{t}");
    }
    assert_eq!(wedge_text(0.), "100% · 0.0 dB");
    assert_eq!(wedge_text(-60.), "0% · -inf dB");
    assert_eq!(wedge_text(6.), "200% · 6.0 dB");
}

#[test]
fn report_hides_latent_diagnostics_until_developer_toggle_is_enabled() {
    let mut ui = kontra_ui::ui();
    let mut app = App {
        report: vec!["[Audible] part 1 (Piano): unsupported fx".into()],
        report_latent: vec!["[Latent] part 1 (Piano): ignored group.flag".into()],
        ..App::default()
    };
    app.view.tab = app::Tab::Report;
    settle(&mut ui, &mut app);

    // Only the visible diagnostic row (report-0) exists by default.
    assert!(ui.scene().and_then(|s| s.surface("report-0")).is_some());
    assert!(
        ui.scene().and_then(|s| s.surface("report-1")).is_none(),
        "Latent diagnostic must be hidden before Developer toggle is enabled"
    );
    assert!(!app.view.report_developer);

    // Clicking the Developer toggle reveals report-1.
    click(&mut ui, &mut app, "report-developer");
    assert!(app.view.report_developer);
    assert!(ui.scene().and_then(|s| s.surface("report-0")).is_some());
    assert!(
        ui.scene().and_then(|s| s.surface("report-1")).is_some(),
        "Latent diagnostic must appear when Developer toggle is enabled"
    );

    // Passes MUI audit with Developer mode on.
    let findings = mui::audit::audit(&mut ui, [Size::new(1180., 780.)], |ui| {
        app::view(ui, &mut app).el
    });
    assert!(
        findings.is_empty(),
        "Report developer mode audit findings:\n{}",
        findings
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}
