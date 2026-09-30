use super::*;
use crate::artwork;
use crate::engine::load_scripts;
use crate::plugin::script_interface;

fn enter() -> Input {
    Input {
        keys: vec![KeyPress {
            key: Key::Enter,
            mods: Mods::default(),
        }],
        ..Default::default()
    }
}

fn pointer(pos: Point, down: bool) -> Input {
    Input {
        pointer: PointerInput {
            pos: Some(pos),
            buttons: if down {
                Buttons::PRIMARY
            } else {
                Buttons::default()
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

fn center(ui: &Ui, id: &str) -> Point {
    let r = ui
        .scene()
        .unwrap()
        .surface(id)
        .unwrap_or_else(|| panic!("no {id}"))
        .frame;
    Point::new(r.x + r.size.width / 2., r.y + r.size.height / 2.)
}

type Build = Box<dyn FnMut(&mut Ui, &mut Bridge<SamplerParams>) -> El>;

struct Harness {
    ui: Ui,
    build: Build,
    bridge: Bridge<SamplerParams>,
    size: Size,
}

impl Harness {
    fn new(p: &Arc<SamplerParams>, width: f64, height: f64) -> Self {
        let mut h = Self {
            ui: theme::ui(),
            build: Box::new(build(p)),
            bridge: Bridge::new(p.clone()),
            size: Size::new(width, height),
        };
        h.idle(3);
        h
    }

    fn tick(&mut self, input: Input) {
        let root = (self.build)(&mut self.ui, &mut self.bridge);
        self.ui
            .frame(root, Some(self.size), input, 1. / 60.)
            .unwrap();
    }

    fn idle(&mut self, frames: usize) {
        for _ in 0..frames {
            self.tick(Input::default());
        }
    }

    /// Focus `id` and press Enter, then let the result settle.
    fn press(&mut self, id: &str) {
        self.ui.focus(id);
        self.tick(enter());
        self.idle(3);
    }

    fn drag(&mut self, from: &str, to: &str) {
        let (from, to) = (center(&self.ui, from), center(&self.ui, to));
        for (pos, down) in [
            (from, true),
            (Point::new(from.x - 15., from.y), true),
            (to, true),
            (to, false),
        ] {
            self.tick(pointer(pos, down));
        }
        self.idle(2);
    }

    fn type_into(&mut self, id: &str, text: &str) {
        self.ui.focus(id);
        self.tick(enter());
        self.idle(2);
        self.tick(Input {
            keys: vec![KeyPress {
                key: Key::Char('a'),
                mods: Mods {
                    ctrl: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        });
        self.tick(Input {
            text: text.into(),
            ..Default::default()
        });
        self.tick(enter());
        self.idle(3);
    }
}

#[test]
fn rack_interactions() {
    let p = Arc::new(SamplerParams::new());
    {
        let mut v = p.shared.view.lock().unwrap();
        v.root = "/virtual".into();
        v.files = Arc::new(vec![
            "/virtual/Library/Piano.nki".into(),
            "/virtual/Library/Strings.nki".into(),
            "/virtual/Library/Ensemble.nkm".into(),
        ]);
    }
    let mut h = Harness::new(&p, 1180., 760.);
    let parts = |p: &SamplerParams| p.selection.read().unwrap().parts.clone();

    h.press("library-0");
    h.drag("instrument-0", "rack-drop");
    assert_eq!(parts(&p).len(), 1, "a preset dropped on the rack is added");
    h.press("instrument-1");
    assert_eq!(parts(&p).len(), 2, "clicking a preset adds it");
    h.press("instrument-0");
    h.press("preset-next");
    assert_eq!(parts(&p).len(), 2, "clicking a loaded preset shows it");
    assert!(
        parts(&p)[0].path.ends_with("Strings.nki"),
        "next preset replaces the shown part"
    );
    h.press("preset-prev");
    assert!(parts(&p)[0].path.ends_with("Piano.nki"));

    h.press("picker-multis");
    h.press("instrument-0");
    assert!(
        p.shared
            .multi_request
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .ends_with("Ensemble.nkm")
    );
    assert_eq!(parts(&p).len(), 2, "the rack stays until the multi loads");
    h.press("picker-instruments");

    h.drag("part-1", "part-0");
    assert_eq!(
        p.selection.read().unwrap().order,
        vec![1, 0],
        "chips reorder by dragging"
    );

    h.press("tab-rack");
    h.press("mute-0");
    assert!(parts(&p)[0].mute);
    for (id, text) in [("output-0", "4"), ("channel-0", "2"), ("port-0", "2")] {
        h.type_into(id, text);
    }
    let part = &parts(&p)[0];
    assert_eq!((part.output, part.channel, part.port), (3, 1, 1));

    h.press("part-0");
    assert!(
        h.ui.scene().unwrap().surface("instrument-stage").is_some(),
        "a chip opens the instrument view"
    );

    let key = center(&h.ui, "key-60");
    h.tick(pointer(key, true));
    h.tick(pointer(key, false));
    h.idle(1);
    assert_eq!(p.shared.keyboard.pop(), Some((0, 60, true)));
    assert_eq!(p.shared.keyboard.pop(), Some((0, 60, false)));
    p.shared.key_owners[61].store(1, Ordering::Relaxed);
    p.shared.release_keyboard();
    assert_eq!(p.shared.keyboard.pop(), Some((1, 61, false)));
    p.shared.release_keyboard();
    assert!(p.shared.keyboard.pop().is_none());

    h.press("tab-rack");
    h.press("remove-0");
    let s = p.selection.read().unwrap().clone();
    assert!(s.parts[0].path.is_empty());
    assert!(s.parts[1].path.ends_with("Strings.nki"));

    let ui = &h.ui;
    let files = vec![PathBuf::from("/external/Native.nki")];
    let at = Point::new(10., 10.);
    assert!(native_files(&p, ui, at, &files, false));
    assert!(parts(&p)[0].path.is_empty(), "hovering does not load");
    assert!(native_files(&p, ui, at, &files, true));
    assert!(
        parts(&p)[0].path.ends_with("Native.nki"),
        "a dropped file takes the free slot"
    );
    assert!(!native_files(&p, ui, at, &[PathBuf::from("bad.wav")], true));
    assert!(!native_files(
        &p,
        ui,
        at,
        &vec![PathBuf::from("full.nki"); RACK_SLOTS],
        true
    ));
    let at = center(ui, "part-1");
    assert!(native_files(&p, ui, at, &files, true));
    assert!(
        parts(&p)[1].path.ends_with("Native.nki"),
        "a file dropped on a chip replaces it"
    );
    let multi = [PathBuf::from("/external/Multi.nkm")];
    let before = p.selection.read().unwrap().clone();
    assert!(native_files(&p, ui, at, &multi, false));
    assert!(p.shared.multi_request.lock().unwrap().is_none());
    assert!(native_files(&p, ui, at, &multi, true));
    assert_eq!(
        p.shared.multi_request.lock().unwrap().take().unwrap(),
        "/external/Multi.nkm"
    );
    assert!(*p.selection.read().unwrap() == before);
}

#[test]
fn search_groups_results_by_library() {
    let p = Arc::new(SamplerParams::new());
    {
        let mut v = p.shared.view.lock().unwrap();
        v.root = "/virtual".into();
        v.files = Arc::new(vec![
            "/virtual/Keys/Grand Piano.nki".into(),
            "/virtual/Keys/Organ.nki".into(),
            "/virtual/Toys/Toy Piano.nki".into(),
        ]);
    }
    let mut h = Harness::new(&p, 1180., 760.);
    h.ui.focus("search");
    h.tick(Input {
        text: "piano".into(),
        ..Default::default()
    });
    h.idle(3);
    let scene = h.ui.scene().unwrap();
    assert!(scene.surface("instrument-1").is_some() && scene.surface("instrument-2").is_none());
    h.press("instrument-1");
    assert!(
        p.selection.read().unwrap().parts[0]
            .path
            .ends_with("Toy Piano.nki")
    );
}

/// Renders the editor in its main states to `.impeccable/review/` (git-ignored)
/// for a visual check. Uses the owner's library when present.
#[test]
fn screenshot() {
    use moose::mui::mui::vello::{
        self,
        vello_cpu::{Pixmap, RenderContext, Resources},
    };
    let root = Path::new(import::LIBRARY_ROOT);
    let files = import::presets(root).unwrap_or_default();
    // KONTAKTO_SHOT="Name A,Name B" renders other instruments in the rack slots.
    let chosen = std::env::var("KONTAKTO_SHOT")
        .unwrap_or_else(|_| "Vista - Harp,Vista - 3 Cellos,Vista - 5 Violins".into());
    let instruments: Vec<_> = chosen
        .split(',')
        .filter_map(|name| {
            files
                .iter()
                .find(|p| p.file_stem().is_some_and(|n| n == name))
        })
        .filter_map(|p| import::read(p).ok())
        .map(Arc::new)
        .collect();
    std::fs::create_dir_all(".impeccable/review").unwrap();
    let states: [(&str, bool, &[&str]); 9] = [
        ("empty", false, &[]),
        ("perform", true, &[]),
        ("mapping", true, &["tab-mapping"]),
        ("rack", true, &["tab-rack"]),
        ("info", true, &["tab-info"]),
        ("library", false, &["library-0"]),
        ("multis", false, &["picker-multis"]),
        ("settings", true, &["settings"]),
        ("error", true, &[]),
    ];
    for (state, loaded, presses) in states {
        for (width, height) in [(1180u16, 760u16), (900, 600)] {
            let p = Arc::new(SamplerParams::new());
            {
                let mut view = p.shared.view.lock().unwrap();
                view.artwork = artwork::scan(root, &files);
                view.files = Arc::new(files.clone());
                view.root = import::LIBRARY_ROOT.into();
                for (slot, i) in instruments.iter().enumerate().filter(|_| loaded) {
                    p.selection.write().unwrap().parts.push(Part {
                        path: i.path.to_string_lossy().into(),
                        group: i.first_playable_group().unwrap_or(0) as u32,
                        ..Default::default()
                    });
                    let script = script_interface(
                        load_scripts(i, i.script_state.clone(), 48000.).0.as_deref(),
                    );
                    let (interface, keys) = (script.interface, script.keys);
                    view.parts[slot] = PartView {
                        wallpaper: artwork::performance(
                            i,
                            interface.as_ref().map(|u| u.wallpaper.as_str()),
                        )
                        .unwrap_or(None),
                        interface,
                        keys,
                        instrument: Some(i.clone()),
                        active: i.name.clone(),
                        bytes: 180 << 20,
                        loading: state == "error" && slot == 2,
                        status: if state == "error" && slot == 0 {
                            "Load failed: missing sample data in archive".into()
                        } else {
                            format!("{} groups · {} zones", i.groups.len(), i.zones.len())
                        },
                        ..Default::default()
                    };
                }
            }
            let mut h = Harness::new(&p, f64::from(width), f64::from(height));
            for id in presses {
                h.press(id);
            }
            h.idle(2);
            let scene = h.ui.scene().unwrap();
            let mut ctx = RenderContext::new(width, height);
            let mut resources = Resources::default();
            vello::paint(
                &mut vello::Cpu {
                    ctx: &mut ctx,
                    resources: &mut resources,
                    cache: &mut vello::Cache::default(),
                },
                scene,
                vello::kurbo::Affine::IDENTITY,
            )
            .unwrap();
            ctx.flush();
            let mut pix = Pixmap::new(width, height);
            ctx.render(&mut pix, &mut resources);
            let rgba: Vec<u8> = pix
                .take_unpremultiplied()
                .iter()
                .flat_map(|p| [p.r, p.g, p.b, p.a])
                .collect();
            moose::core::screenshot::save_png(
                Path::new(&format!(".impeccable/review/{state}-{width}.png")),
                &rgba,
                u32::from(width),
                u32::from(height),
            );
            let mut visible = vec!["search", "octave-up", "rack-drop", "panic"];
            if loaded {
                visible.extend(["tab-info", "performance-play", "part-0"]);
            }
            for id in visible {
                let r = scene
                    .surface(id)
                    .unwrap_or_else(|| panic!("{state}: no {id}"))
                    .frame;
                assert!(
                    r.x >= 0.
                        && r.y >= 0.
                        && r.x + r.size.width <= f64::from(width) + 1.
                        && r.y + r.size.height <= f64::from(height) + 1.,
                    "{state} {width}: {id} outside the window: {r:?}"
                );
            }
            if state == "info" {
                assert!(scene.surface("details-scroll").is_some());
            }
            if state == "mapping" {
                assert!(scene.surface("groups-scroll").is_some());
            }
            if state == "settings" {
                assert!(scene.surface("root").is_some());
            }
            if loaded {
                // The keyboard centers on the instrument, so pick a key it shows.
                let key = (0..128)
                    .find(|n| h.ui.scene().unwrap().surface(&format!("key-{n}")).is_some())
                    .unwrap();
                h.press(&format!("key-{key}"));
                assert!(p.shared.audition.load(Ordering::Acquire));
                assert_eq!(p.shared.audition_note.load(Ordering::Relaxed), key);
            }
        }
    }
}

#[test]
fn performance_controls_edit_the_script() {
    let script = "on init\nmake_perfview\nset_ui_height_px(200)\ndeclare ui_switch $legato\nset_text($legato, \"Legato\")\nmove_control_px($legato, 10, 10)\ndeclare ui_knob $vibrato(0, 100, 1)\nmove_control_px($vibrato, 200, 10)\ndeclare ui_menu $mic\nadd_menu_item($mic, \"Close\", 0)\nadd_menu_item($mic, \"Room\", 1)\nmove_control_px($mic, 400, 10)\nend on";
    let mut engine = crate::ksp::LogEngine::new(Vec::new(), 48_000.0);
    let (rt, errors) = crate::ksp::Runtime::with_scripts(&[script], &mut engine, 8, Vec::new());
    assert!(errors.iter().all(Option::is_none), "{errors:?}");
    let path = "/virtual/Library/Solo.nki";
    let p = Arc::new(SamplerParams::new());
    p.selection.write().unwrap().parts.push(Part {
        path: path.into(),
        ..Default::default()
    });
    {
        let mut view = p.shared.view.lock().unwrap();
        view.files = Arc::new(vec![path.into()]);
        view.parts[0].interface = script_interface(Some(&rt)).interface;
        view.parts[0].instrument = Some(Arc::new(import::Instrument {
            path: path.into(),
            name: "Solo".into(),
            groups: Vec::new(),
            zones: Vec::new(),
            warnings: Vec::new(),
            missing_samples: Vec::new(),
            scripts: vec![script.into()],
            fx: Default::default(),
            voice_limit: None,
            voice_groups: Vec::new(),
            script_state: Vec::new(),
            kontakt_sample_bytes: 0.0,
            kontakt_preload: 0,
        }));
    }
    let value = |n: usize| {
        let view = p.shared.view.lock().unwrap();
        view.parts[0].interface.as_ref().unwrap().controls[n].properties["$CONTROL_PAR_VALUE"]
            .clone()
    };
    let mut h = Harness::new(&p, 1180., 760.);

    h.press("ksp-control-0");
    assert_eq!(value(0), crate::ksp::Value::Int(1), "a switch toggles on");
    h.press("ksp-control-0");
    assert_eq!(value(0), crate::ksp::Value::Int(0), "and off");

    let knob = center(&h.ui, "ksp-control-1");
    for y in [0., -10., -60.] {
        h.tick(pointer(Point::new(knob.x, knob.y + y), true));
    }
    h.tick(pointer(Point::new(knob.x, knob.y - 60.), false));
    h.idle(2);
    let crate::ksp::Value::Int(dragged) = value(1) else {
        panic!("knob value")
    };
    assert!(dragged > 10, "dragging a knob up raises it, got {dragged}");

    h.press("ksp-control-2");
    h.press("ksp-menu-2-1");
    assert_eq!(
        value(2),
        crate::ksp::Value::Int(1),
        "a menu item sets the menu"
    );
}
