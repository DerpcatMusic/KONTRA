//! The editor driven by the pointer, headless: a click drills into a
//! library, a click on a preset asks to load it, a menu pick lands, and
//! once motion settles the editor asks for no more frames.

use kontra_ui::app::{
    self, Action, App, Envelope, Face, Format, Library, Param, Part, PartTab, Sound,
};
use mui::prelude::{Buttons, Input, Point, PointerInput, Size, Ui};

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
    // A missing target clicks nothing, and the asserts after it say so.
    let Some(s) = ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame) else {
        return Vec::new();
    };
    let at = Point::new(s.x + s.size.width / 2., s.y + s.size.height / 2.);
    let mut out = Vec::new();
    for buttons in [Buttons::PRIMARY, Buttons::default()] {
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
            starred: false,
            art: Vec::new(),
        }],
        ..App::default()
    };
    settle(&mut ui, &mut app);
    assert!(click(&mut ui, &mut app, "library-0").is_empty());
    assert_eq!(app.view.library, Some(0));
    assert_eq!(
        click(&mut ui, &mut app, "preset-0"),
        vec![Action::Load {
            library: 0,
            preset: 0
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
        click(&mut ui, &mut app, "part-0-channel-0"),
        vec![Action::Channel(0, 0)],
        "the first cell is every channel"
    );
    assert_eq!(app.view.menu, None, "a pick closes the menu");
    click(&mut ui, &mut app, "part-0-output");
    assert_eq!(
        click(&mut ui, &mut app, "part-0-output-2"),
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
