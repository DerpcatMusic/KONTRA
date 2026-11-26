//! End-to-end mouse-wheel scrolling audit across every scrollable surface in
//! KONTRA: browser lists, multi-rack, zoomed instrument faces (vertical +
//! Shift/trackpad horizontal), settings, about changelog, report, mixer,
//! mapping groups, file-pick sheet, dropdown/KSP menus, UVI viewports,
//! file selectors, and Driver/idle wake.

use std::time::{Duration, Instant};

use kontra_ui::Pictures;
use kontra_ui::app::{
    self, App, Build, Face, FilePick, Format, Library, LibraryFolder, Part, PartTab, Sheet, Shelf,
    Tab, Zone,
};
use kontra_ui_model::{
    self as model, Interface, Kind, MenuItem, Page, PageRef, Range, Rect, Size as ModelSize, Widget,
};
use mui::Clipboard;
use mui::host::{Driver, Shared, View, Wheel};
use mui::prelude::{Buttons, El, Input, Mods, Point, PointerInput, Size, Ui, Vec2};

const WIN: Size = Size::new(1180., 780.);

fn frame(ui: &mut Ui, app: &mut App, input: Input) -> bool {
    let r = app::view(ui, app);
    let res = ui.frame(r.el, Some(WIN), input, 1. / 60.);
    assert!(res.is_ok(), "layout failed: {:?}", res.err());
    res.is_ok_and(|f| f.animating)
}

fn settle(ui: &mut Ui, app: &mut App) {
    for n in 0..240 {
        if !frame(ui, app, Input::default()) && n >= 2 {
            break;
        }
    }
}

fn center(ui: &Ui, id: &str) -> Point {
    let surface = ui.scene().and_then(|s| s.surface(id));
    assert!(surface.is_some(), "surface {id:?} missing");
    surface.map_or(Point::ZERO, |s| {
        Point::new(
            s.frame.x + s.frame.size.width / 2.,
            s.frame.y + s.frame.size.height / 2.,
        )
    })
}

fn click(ui: &mut Ui, app: &mut App, id: &str) {
    let at = center(ui, id);
    for buttons in [Buttons::PRIMARY, Buttons::default()] {
        frame(
            ui,
            app,
            Input {
                pointer: PointerInput {
                    pos: Some(at),
                    buttons,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
    }
    settle(ui, app);
}

fn wheel_at(ui: &mut Ui, app: &mut App, pos: Point, wheel: Vec2, mods: Mods) {
    // Move pointer first so hover / overlays settle just like a real user.
    frame(
        ui,
        app,
        Input {
            pointer: PointerInput {
                pos: Some(pos),
                mods,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    frame(
        ui,
        app,
        Input {
            pointer: PointerInput {
                pos: Some(pos),
                mods,
                ..Default::default()
            },
            wheel,
            ..Default::default()
        },
    );
    // Step springs to verify drawn geometry actually moves too.
    for _ in 0..30 {
        if !frame(
            ui,
            app,
            Input {
                pointer: PointerInput {
                    pos: Some(pos),
                    mods,
                    ..Default::default()
                },
                ..Default::default()
            },
        ) {
            break;
        }
    }
}

fn surface_info(ui: &Ui, id: &str) -> String {
    match ui.scene().and_then(|s| s.surface(id)) {
        Some(s) => format!(
            "frame=({:.1},{:.1} {:.1}x{:.1}) content=({:.1}x{:.1}) scroll={:?}",
            s.frame.x,
            s.frame.y,
            s.frame.size.width,
            s.frame.size.height,
            s.content.width,
            s.content.height,
            ui.scroll(id),
        ),
        None => format!("{id}: MISSING"),
    }
}

fn big_face(w: f64, h: f64) -> Face {
    let mut tree = Interface::default();
    tree.pages.push(Page {
        size: ModelSize {
            width: w,
            height: h,
        },
        ..Page::default()
    });
    let mut knob = Widget::new(
        "$cutoff",
        PageRef(0),
        Rect::new(20, 20, 60, 60),
        Kind::Knob {
            range: Range {
                min: 0.,
                max: 100.,
                default: 50.,
                step: Some(1.),
            },
            display: model::Display::default(),
        },
    );
    knob.value = Some(model::Value::Integer(50));
    tree.widgets.push(knob);
    let items: Vec<MenuItem> = (0..60)
        .map(|i| MenuItem {
            text: format!("Preset Item {i:02}"),
            value: i,
            visible: true,
        })
        .collect();
    tree.widgets.push(Widget::new(
        "$menu",
        PageRef(0),
        Rect::new(100, 20, 120, 24),
        Kind::Menu { items },
    ));
    let mut panel = Widget::new(
        "bg",
        PageRef(0),
        Rect::new(0, 0, w as u32, h as u32),
        Kind::Panel,
    );
    panel.z = -1;
    tree.widgets.push(panel);
    let len = tree.widgets.len();
    kontra_ui::layout::resolve(&mut tree, 0..len);
    Face {
        tree,
        pictures: Pictures::new(Box::new(|_| None)),
        gesture: Default::default(),
        meters: Default::default(),
    }
}

#[test]
fn browser_library_preset_recent_and_empty_lists_scroll_with_wheel() {
    // 1. Library grid (with default split == 0 AND while hovering a tile!)
    let mut ui = kontra_ui::ui();
    let kinds = ["Bass", "Lead", "Pad", "Keys", "Pluck"];
    let presets: Vec<String> = (0..50)
        .map(|i| format!("{} {:02}", kinds[i % kinds.len()], i))
        .collect();
    let mut app = App {
        libraries: (0..24)
            .map(|n| Library {
                name: format!("Library {n:02}"),
                format: Format::Kontakt,
                presets: presets.clone(),
                ..Library::default()
            })
            .collect(),
        recent: (0..30).map(|i| (0, i)).collect(),
        ..App::default()
    };
    settle(&mut ui, &mut app);
    // Select library 0 first so preset-list is also mounted below.
    click(&mut ui, &mut app, "library-0");
    let tile = center(&ui, "library-0");
    wheel_at(&mut ui, &mut app, tile, Vec2::new(0., 80.), Mods::default());
    assert!(
        ui.scroll("library-list")[1] > 0.,
        "library-list must scroll under hovered tile: {}",
        surface_info(&ui, "library-list")
    );

    // 2. Preset list
    let preset_pos = center(&ui, "preset-list");
    wheel_at(
        &mut ui,
        &mut app,
        preset_pos,
        Vec2::new(0., 80.),
        Mods::default(),
    );
    assert!(
        ui.scroll("preset-list")[1] > 0.,
        "preset-list must scroll: {}",
        surface_info(&ui, "preset-list")
    );

    // 3. Recent list
    app.view.shelf = Shelf::Recent;
    settle(&mut ui, &mut app);
    let recent_pos = center(&ui, "recent-list");
    wheel_at(
        &mut ui,
        &mut app,
        recent_pos,
        Vec2::new(0., 80.),
        Mods::default(),
    );
    assert!(
        ui.scroll("recent-list")[1] > 0.,
        "recent-list must scroll: {}",
        surface_info(&ui, "recent-list")
    );

    // 4. Empty library folder list
    let mut ui_empty = kontra_ui::ui();
    let mut app_empty = App {
        folders: (0..25)
            .map(|i| LibraryFolder {
                path: format!("/mnt/libraries/Collection {i:02}/Long Path Name Here"),
                status: "0 libraries found".into(),
                problem: true,
                ..LibraryFolder::default()
            })
            .collect(),
        ..App::default()
    };
    settle(&mut ui_empty, &mut app_empty);
    let empty_pos = center(&ui_empty, "library-empty");
    wheel_at(
        &mut ui_empty,
        &mut app_empty,
        empty_pos,
        Vec2::new(0., 80.),
        Mods::default(),
    );
    assert!(
        ui_empty.scroll("library-empty")[1] > 0.,
        "library-empty must scroll: {}",
        surface_info(&ui_empty, "library-empty")
    );
}

#[test]
fn rack_and_instrument_face_scroll_vertically_and_horizontally() {
    let mut ui = kontra_ui::ui();
    let mut app = App {
        parts: (0..6)
            .map(|i| Part {
                name: format!("Part {i}"),
                open: i == 0,
                face: Some(big_face(1200., 900.)),
                ..Part::default()
            })
            .collect(),
        ..App::default()
    };
    // Fixed 100% scale so 1200x900 overflows both axes inside the rack.
    app.view.face_scale = 1;
    settle(&mut ui, &mut app);

    // Vertical wheel over empty face background scrolls part-0-face-y.
    let face_y_surface = ui.scene().and_then(|s| s.surface("part-0-face-y"));
    assert!(face_y_surface.is_some(), "part-0-face-y missing");
    let empty_bg = face_y_surface.map_or(Point::ZERO, |s| {
        Point::new(
            s.frame.x + s.frame.size.width - 40.,
            s.frame.y + s.frame.size.height - 40.,
        )
    });
    wheel_at(
        &mut ui,
        &mut app,
        empty_bg,
        Vec2::new(0., 80.),
        Mods::default(),
    );
    assert!(
        ui.scroll("part-0-face-y")[1] > 0.,
        "part-0-face-y must scroll vertically: {}",
        surface_info(&ui, "part-0-face-y")
    );

    // Shift + vertical wheel scrolls part-0-face-x horizontally!
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    wheel_at(&mut ui, &mut app, empty_bg, Vec2::new(0., 80.), shift);
    assert!(
        ui.scroll("part-0-face-x")[0] > 0.,
        "Shift+Wheel must scroll part-0-face-x horizontally: {}",
        surface_info(&ui, "part-0-face-x")
    );

    // Rack scrolls when wheeling over a part header (never trapped by header knobs/controls).
    let header_pos = center(&ui, "part-0-band");
    wheel_at(
        &mut ui,
        &mut app,
        header_pos,
        Vec2::new(0., 120.),
        Mods::default(),
    );
    assert!(
        ui.scroll("rack")[1] > 0.,
        "rack must scroll over part header: {}",
        surface_info(&ui, "rack")
    );
}

#[test]
fn settings_about_report_mixer_mapping_pick_and_menus_scroll_with_wheel() {
    // Settings sheet (captures_wheel on outer sheet, settings-scroll inside)
    let mut ui = kontra_ui::ui();
    let mut app = App {
        folders: (0..18)
            .map(|i| LibraryFolder {
                path: format!("/mnt/audio/Folder {i:02}/Subfolder"),
                status: "12 libraries found".into(),
                ..LibraryFolder::default()
            })
            .collect(),
        ..App::default()
    };
    app.view.sheet = Some(Sheet::Settings);
    settle(&mut ui, &mut app);
    let pos = center(&ui, "settings-scroll");
    wheel_at(&mut ui, &mut app, pos, Vec2::new(0., 80.), Mods::default());
    assert!(
        ui.scroll("settings-scroll")[1] > 0.,
        "settings-scroll must scroll inside sheet: {}",
        surface_info(&ui, "settings-scroll")
    );

    // About changelog sheet (memoized log inside captures_wheel sheet)
    let mut changelog = String::from("# Changelog\n\n");
    for i in 0..40 {
        changelog.push_str(&format!(
            "## 0.6.{i}\n- Added feature {i} with enough detail to wrap lines.\n\n"
        ));
    }
    let changelog: &'static str = Box::leak(changelog.into_boxed_str());
    app.build = Build {
        version: "0.7.0",
        revision: "test",
        built: "2026-11-26",
        target: "linux",
        formats: "CLAP",
        changelog,
    };
    app.view.sheet = Some(Sheet::About);
    settle(&mut ui, &mut app);
    let pos = center(&ui, "about-changelog");
    wheel_at(&mut ui, &mut app, pos, Vec2::new(0., 80.), Mods::default());
    assert!(
        ui.scroll("about-changelog")[1] > 0.,
        "about-changelog must scroll inside sheet: {}",
        surface_info(&ui, "about-changelog")
    );
    app.view.sheet = None;

    // Report list
    app.report = (0..40)
        .map(|i| format!("Part 1: diagnostic line {i:02}"))
        .collect();
    app.view.tab = Tab::Report;
    settle(&mut ui, &mut app);
    let pos = center(&ui, "report-list");
    wheel_at(&mut ui, &mut app, pos, Vec2::new(0., 80.), Mods::default());
    assert!(
        ui.scroll("report-list")[1] > 0.,
        "report-list must scroll: {}",
        surface_info(&ui, "report-list")
    );

    // Mixer horizontal strip scroll (both Shift+Wheel AND plain mouse wheel!)
    app.parts = (0..18)
        .map(|i| Part {
            name: format!("Inst {i:02}"),
            groups: (0..30).map(|g| format!("Group {g:02}")).collect(),
            zones: vec![Zone {
                group: 0,
                sample: "s.wav".into(),
                keys: (36, 72),
                velocities: (1, 127),
                root: 60,
                ..Zone::default()
            }],
            ..Part::default()
        })
        .collect();
    app.view.tab = Tab::Mixer;
    settle(&mut ui, &mut app);
    let pos = center(&ui, "mixer-strip-scroll");
    wheel_at(&mut ui, &mut app, pos, Vec2::new(0., 80.), Mods::default());
    assert!(
        ui.scroll("mixer-strip-scroll")[0] > 0.,
        "mixer-strip-scroll must scroll horizontally with plain wheel: {}",
        surface_info(&ui, "mixer-strip-scroll")
    );

    // Mapping groups list
    app.view.tab = Tab::Rack;
    app.parts[0].open = true;
    app.parts[0].tab = PartTab::Mapping;
    settle(&mut ui, &mut app);
    let pos = center(&ui, "part-0-groups");
    wheel_at(&mut ui, &mut app, pos, Vec2::new(0., 80.), Mods::default());
    assert!(
        ui.scroll("part-0-groups")[1] > 0.,
        "part-0-groups must scroll: {}",
        surface_info(&ui, "part-0-groups")
    );

    // File pick sheet
    app.pick = Some(FilePick {
        part: 0,
        save: false,
        names: (0..30).map(|i| format!("Saved {i:02}")).collect(),
    });
    settle(&mut ui, &mut app);
    let pos = center(&ui, "pick-list");
    wheel_at(&mut ui, &mut app, pos, Vec2::new(0., 80.), Mods::default());
    assert!(
        ui.scroll("pick-list")[1] > 0.,
        "pick-list must scroll inside sheet: {}",
        surface_info(&ui, "pick-list")
    );
    app.pick = None;
    settle(&mut ui, &mut app);

    // Long KSP ui_menu popup scrolls with wheel
    app.parts[0].face = Some(big_face(600., 400.));
    app.parts[0].tab = PartTab::Interface;
    app.view.face_scale = 0;
    settle(&mut ui, &mut app);
    let menu_id = kontra_ui::input::surface(0, 1);
    click(&mut ui, &mut app, &menu_id);
    let popup_rows = format!("{menu_id}-popup/rows");
    let pos = center(&ui, &popup_rows);
    wheel_at(&mut ui, &mut app, pos, Vec2::new(0., 80.), Mods::default());
    assert!(
        ui.scroll(&popup_rows)[1] > 0.,
        "KSP ui_menu popup must scroll: {}",
        surface_info(&ui, &popup_rows)
    );

    // Tall part-needs popup scrolls with wheel and traps wheel from the rack.
    app.view.menu = None;
    if let Some(face) = app.parts[0].face.as_mut() {
        face.gesture.cancel();
    }
    app.parts[0].needs = (0..30)
        .map(|i| format!("Missing sample bank {i:02} required for full playback"))
        .collect();
    settle(&mut ui, &mut app);
    let rack_before_needs = ui.scroll("rack")[1];
    click(&mut ui, &mut app, "part-0-needs");
    let needs_list = center(&ui, "part-0-needs-list");
    wheel_at(
        &mut ui,
        &mut app,
        needs_list,
        Vec2::new(0., 80.),
        Mods::default(),
    );
    assert!(
        ui.scroll("part-0-needs-list")[1] > 0.,
        "part-0-needs-list popup must scroll: {}",
        surface_info(&ui, "part-0-needs-list")
    );
    assert_eq!(
        ui.scroll("rack")[1],
        rack_before_needs,
        "scrolling part-0-needs popup must not scroll rack underneath"
    );

    // Background scrollers behind a modal sheet are disabled and must NOT scroll.
    app.view.menu = None;
    app.libraries = (0..24)
        .map(|n| Library {
            name: format!("Library {n:02}"),
            format: Format::Kontakt,
            presets: vec!["Alpha".into()],
            ..Library::default()
        })
        .collect();
    app.view.sheet = Some(Sheet::Settings);
    settle(&mut ui, &mut app);
    let bg_lib_before = ui.scroll("library-list")[1];
    let bg_tile = center(&ui, "library-0");
    wheel_at(
        &mut ui,
        &mut app,
        bg_tile,
        Vec2::new(0., 120.),
        Mods::default(),
    );
    assert_eq!(
        ui.scroll("library-list")[1],
        bg_lib_before,
        "disabled background library-list behind Settings modal must not scroll"
    );
}

#[test]
fn uvi_viewport_knob_and_file_selector_wheel_interactions() {
    // Build a face with:
    //  0: KSP knob (claims wheel; Shift+Wheel adjusts finely without scrolling face)
    //  1: UVI vertical+horizontal Viewport panel with overflowing children
    //  2: child inside viewport (far corner at 400x400)
    //  3: KSP ui_file_selector with many files in a temp directory
    let dir = std::env::temp_dir().join(format!(
        "kontra-wheel-fs-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::fs::create_dir_all(&dir).is_ok());
    for i in 0..30 {
        assert!(std::fs::write(dir.join(format!("item_{i:02}.wav")), b"").is_ok());
    }

    let mut tree = Interface::default();
    tree.pages.push(Page {
        size: ModelSize {
            width: 700.,
            height: 400.,
        },
        ..Page::default()
    });
    let mut knob = Widget::new(
        "$cutoff",
        PageRef(0),
        Rect::new(20, 20, 48, 48),
        Kind::Knob {
            range: Range {
                min: 0.,
                max: 100.,
                default: 50.,
                step: Some(1.),
            },
            display: model::Display::default(),
        },
    );
    knob.value = Some(model::Value::Integer(50));
    tree.widgets.push(knob);

    let mut vp = Widget::new("vp", PageRef(0), Rect::new(100, 20, 180, 140), Kind::Panel);
    vp.viewport = Some([0, 0]);
    tree.widgets.push(vp);
    let mut far = Widget::new("far", PageRef(0), Rect::new(340, 340, 60, 40), Kind::Label);
    far.parent = Some(model::WidgetRef(1));
    far.text = "End".into();
    tree.widgets.push(far);

    let fs = Widget::new(
        "$fs",
        PageRef(0),
        Rect::new(320, 20, 220, 140),
        Kind::FileSelector {
            base_path: Some(dir.to_string_lossy().into_owned()),
            files: model::Files::Audio,
            column_width: Some(120),
            tops: Vec::new(),
        },
    );
    tree.widgets.push(fs);
    let len = tree.widgets.len();
    kontra_ui::layout::resolve(&mut tree, 0..len);

    let mut ui = kontra_ui::ui();
    let mut app = App {
        parts: (0..4)
            .map(|i| Part {
                name: format!("Part {i}"),
                open: i == 0,
                face: (i == 0).then(|| Face {
                    tree: tree.clone(),
                    pictures: Pictures::new(Box::new(|_| None)),
                    gesture: Default::default(),
                    meters: Default::default(),
                }),
                ..Part::default()
            })
            .collect(),
        ..App::default()
    };
    app.view.face_scale = 1;
    settle(&mut ui, &mut app);

    // 1. Wheel over UVI Viewport scrolls vertically AND Shift+Wheel scrolls horizontally,
    //    without scrolling the outer rack.
    let vp_id = "kontra-v-0-1";
    let vp_pos = center(&ui, vp_id);
    let rack_before = ui.scroll("rack")[1];
    wheel_at(
        &mut ui,
        &mut app,
        vp_pos,
        Vec2::new(0., 60.),
        Mods::default(),
    );
    assert!(
        ui.scroll(vp_id)[1] > 0.,
        "UVI viewport must scroll vertically: {}",
        surface_info(&ui, vp_id)
    );
    assert_eq!(
        ui.scroll("rack")[1],
        rack_before,
        "overflowing UVI viewport must trap wheel from rack"
    );
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    wheel_at(&mut ui, &mut app, vp_pos, Vec2::new(0., 60.), shift);
    assert!(
        ui.scroll(vp_id)[0] > 0.,
        "Shift+Wheel must scroll UVI viewport horizontally: {}",
        surface_info(&ui, vp_id)
    );

    // 2. Wheel over KSP knob adjusts knob value without scrolling rack (including Shift fine wheel).
    let knob_id = kontra_ui::input::surface(0, 0);
    let knob_pos = center(&ui, &knob_id);
    let knob_before = app.parts[0]
        .face
        .as_ref()
        .and_then(|f| f.tree.widgets.first())
        .map_or(0., kontra_ui_model::Widget::number);
    wheel_at(
        &mut ui,
        &mut app,
        knob_pos,
        Vec2::new(0., -48.),
        Mods::default(),
    );
    let knob_after = app.parts[0]
        .face
        .as_ref()
        .and_then(|f| f.tree.widgets.first())
        .map_or(0., kontra_ui_model::Widget::number);
    assert!(
        knob_after > knob_before,
        "wheel over KSP knob must adjust knob: before={knob_before}, after={knob_after}"
    );
    assert_eq!(
        ui.scroll("rack")[1],
        rack_before,
        "wheel over KSP knob must never scroll the rack"
    );

    // 3. Wheel over KSP ui_file_selector scrolls its file column without scrolling rack.
    let fs_id = kontra_ui::input::surface(0, 3);
    let fs_pos = center(&ui, &fs_id);
    wheel_at(
        &mut ui,
        &mut app,
        fs_pos,
        Vec2::new(0., 60.),
        Mods::default(),
    );
    let fs_tops = app.parts[0]
        .face
        .as_ref()
        .and_then(|f| f.tree.widgets.get(3))
        .and_then(|w| match &w.kind {
            Kind::FileSelector { tops, .. } => Some(tops.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert!(
        fs_tops.first().copied().unwrap_or(0) > 0,
        "wheel over KSP ui_file_selector must scroll column: tops={fs_tops:?}"
    );
    assert_eq!(
        ui.scroll("rack")[1],
        rack_before,
        "wheel over KSP ui_file_selector must not scroll the rack"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn idle_driver_wakes_on_wheel_and_animates_scroll_spring_to_settlement() {
    struct NoClipboard;
    impl Clipboard for NoClipboard {
        fn get(&mut self) -> Option<String> {
            None
        }
        fn set(&mut self, _: &str) {}
    }
    struct Editor(App);
    impl View for Editor {
        fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
            app::view(ui, &mut self.0).el
        }
        fn changed(&mut self) -> bool {
            false
        }
        fn request_resize(&mut self, _: u32, _: u32) -> bool {
            false
        }
    }

    let app = App {
        libraries: (0..24)
            .map(|n| Library {
                name: format!("Library {n:02}"),
                format: Format::Kontakt,
                presets: vec!["A".into()],
                ..Library::default()
            })
            .collect(),
        ..App::default()
    };
    let mut driver = Driver::new((1180, 780), 1.0, Box::new(NoClipboard));
    driver.display_paced = true;
    let mut shared = Shared {
        ui: kontra_ui::ui(),
        view: Editor(app),
    };
    let mut now = Instant::now();
    // Settle completely to idle (next_wake() == None).
    for _ in 0..120 {
        now += Duration::from_millis(16);
        driver.advance(&mut shared, now);
        if driver.next_wake().is_none() {
            break;
        }
    }
    assert_eq!(driver.next_wake(), None, "editor must settle to idle");

    // Move pointer over library-list, settle again to idle.
    let tile = center(&shared.ui, "library-0");
    driver.pointer_moved(tile, Mods::default());
    for _ in 0..120 {
        now += Duration::from_millis(16);
        driver.advance(&mut shared, now);
        if driver.next_wake().is_none() {
            break;
        }
    }
    assert_eq!(driver.next_wake(), None, "hovered tile must settle to idle");

    let tile_y = |ui: &Ui| {
        ui.scene()
            .and_then(|s| s.surface("library-0"))
            .map(|s| s.frame.y)
    };
    let before_y = tile_y(&shared.ui);
    assert!(before_y.is_some(), "library-0 surface missing before wheel");

    // Now simulate an idle window receiving a wheel notch (y = -3 lines = scroll down).
    now += Duration::from_secs(2);
    driver.wheel(Wheel::Lines(0., -3.0), Mods::default());
    assert!(
        driver.next_wake().is_some(),
        "wheel event on idle driver must immediately schedule a wake"
    );

    let mut frames = 0;
    while driver.next_wake().is_some() && frames < 120 {
        now += Duration::from_millis(16);
        if driver.advance(&mut shared, now) {
            frames += 1;
        }
    }
    assert!(
        frames >= 2,
        "scroll spring should animate frames ({frames})"
    );
    assert!(
        shared.ui.scroll("library-list")[1] > 0.,
        "library-list scroll target moved"
    );
    let after_y = tile_y(&shared.ui);
    assert!(
        before_y.zip(after_y).is_some_and(|(b, a)| a < b - 10.),
        "drawn tile must actually move upward on screen: before={before_y:?}, after={after_y:?}"
    );
}
