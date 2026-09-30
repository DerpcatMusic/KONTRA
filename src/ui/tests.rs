use super::*;
use crate::artwork;
use crate::engine::load_scripts;
use crate::plugin::{Play, script_interface};

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
    computer: Arc<computer::Computer>,
}

impl Harness {
    fn new(p: &Arc<SamplerParams>, width: f64, height: f64) -> Self {
        let computer = Arc::<computer::Computer>::default();
        let mut h = Self {
            ui: theme::ui(),
            build: Box::new(build(p, Arc::default(), computer.clone(), Arc::default())),
            bridge: Bridge::new(p.clone()),
            size: Size::new(width, height),
            computer,
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

/// The computer keyboard plays from the home row once switched on, holds a
/// note until its key comes up, and leaves text fields their typing.
#[test]
fn the_computer_keyboard_plays_while_switched_on() {
    use moose::mui::mui::host::{KeyEvent, NativeKey};
    let p = Arc::new(SamplerParams::new());
    let mut h = Harness::new(&p, 1180., 760.);
    let key = |code: u64, text: &str, down: bool| KeyEvent {
        code,
        key: NativeKey::Text(text.into()),
        down,
        mods: Mods::default(),
    };
    let computer = h.computer.clone();
    let send = |h: &mut Harness, e: KeyEvent| computer.key(&h.ui, &p, &e);
    assert!(!send(&mut h, key(1, "a", true)), "off, keys pass through");
    h.press("qwerty");
    assert!(read(&p.selection).qwerty, "the top bar switches it on");
    assert!(send(&mut h, key(1, "a", true)));
    assert_eq!(p.shared.keyboard.pop(), Some((0, Play::Note(60, 100))));
    assert!(send(&mut h, key(1, "a", true)), "a repeat is swallowed");
    assert!(p.shared.keyboard.pop().is_none(), "and plays nothing");
    assert!(send(&mut h, key(2, "x", true)), "X steps the octave");
    assert!(send(&mut h, key(3, "V", true)), "V the velocity");
    assert!(send(&mut h, key(4, "k", true)));
    assert_eq!(p.shared.keyboard.pop(), Some((0, Play::Note(84, 120))));
    assert!(send(&mut h, key(1, "a", false)));
    assert_eq!(p.shared.keyboard.pop(), Some((0, Play::Note(60, 0))), "the note its key started stops");
    assert!(!send(&mut h, key(2, "x", false)), "ups of other keys pass");
    h.ui.focus("search");
    h.idle(2);
    assert!(!send(&mut h, key(5, "s", true)), "a text field keeps its typing");
    h.press("qwerty");
    assert_eq!(p.shared.keyboard.pop(), Some((0, Play::Note(84, 0))), "switching off lets go");
    assert!(!send(&mut h, key(4, "k", false)));
}

fn selected_slot(p: &SamplerParams) -> usize {
    p.shared.selected.load(Ordering::Relaxed) as usize
}

/// A narrow rack moves the header's controls under the part name, so the
/// name keeps a line of its own instead of truncating.
#[test]
fn a_narrow_header_gives_the_name_its_line() {
    for (width, below) in [(900., true), (1180., false)] {
        let p = Arc::new(SamplerParams::new());
        p.selection.write().unwrap().parts.push(Part {
            path: "/virtual/Library/Una Corda Pure.nki".into(),
            ..Default::default()
        });
        let mut h = Harness::new(&p, width, 600.);
        h.idle(2);
        let scene = h.ui.scene().unwrap();
        let frame = |id: &str| scene.surface(id).unwrap().frame;
        let (name, port) = (frame("name-0"), frame("midi-0"));
        assert_eq!(port.y > name.y + name.size.height, below, "at {width}");
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
    h.press("preset-next-0");
    assert_eq!(parts(&p).len(), 2, "clicking a loaded preset shows it");
    assert!(
        parts(&p)[0].path.ends_with("Strings.nki"),
        "next preset replaces the shown part"
    );
    h.press("preset-prev-0");
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

    h.drag("name-1", "header-0");
    assert_eq!(
        p.selection.read().unwrap().order,
        vec![1, 0],
        "parts reorder by dragging their names"
    );

    h.press("tab-rack");
    h.press("mute-0");
    assert!(parts(&p)[0].mute);
    // The routing menus: output st.4, channel 2, then port B (after a rule and a heading).
    for (menu, item) in [("output-0", 3), ("midi-0", 2), ("midi-0", 20)] {
        h.press(menu);
        h.press(&format!("menu-item-{item}"));
    }
    let part = &parts(&p)[0];
    assert_eq!((part.output, part.channel, part.port), (3, 1, 1));
    // Pan, gain and tune drag up; a double-click brings each back.
    for id in ["pan-0", "volume-0", "tune-0"] {
        let at = center(&h.ui, id);
        for dy in [0., -5., -20.] {
            h.tick(pointer(Point::new(at.x, at.y + dy), true));
        }
        h.tick(pointer(Point::new(at.x, at.y - 20.), false));
        h.idle(30);
    }
    let part = &parts(&p)[0];
    assert!(part.pan > 0. && part.gain > 0. && part.tune > 0., "{} {} {}", part.pan, part.gain, part.tune);

    h.press("collapse-0");
    assert!(parts(&p)[0].collapsed, "the chevron folds a part");
    assert!(h.ui.scene().unwrap().surface("stage-0").is_none());
    h.press("collapse-0");
    assert!(
        h.ui.scene().unwrap().surface("stage-0").is_some(),
        "and unfolds it"
    );

    // Lower on a key plays louder; dragging across the keys moves the note
    // along, and letting go stops it.
    let frame = |id: &str| h.ui.scene().unwrap().surface(id).unwrap().frame;
    let (c, d, e) = (frame("key-60"), frame("key-62"), frame("key-64"));
    let at = |r: moose::mui::mui::scene::Frame, down: f64| Point::new(r.x + r.size.width / 2., r.y + r.size.height * down);
    // The editor sees a frame's input as the next one builds.
    let mut hold = |pos: Point, down: bool| {
        h.tick(pointer(pos, down));
        h.tick(pointer(pos, down));
    };
    hold(at(c, 0.9), true);
    let first = p.shared.keyboard.pop();
    let Some((0, Play::Note(60, loud))) = first else { panic!("C plays: {first:?}") };
    assert!(p.shared.played[60].load(Ordering::Relaxed) == loud, "and lights");
    hold(at(d, 0.9), true);
    hold(at(e, 0.9), true);
    hold(at(e, 0.9), false);
    let sent: Vec<_> = std::iter::from_fn(|| p.shared.keyboard.pop()).collect();
    assert_eq!(
        sent.iter().map(|(_, play)| *play).collect::<Vec<_>>(),
        [Play::Note(60, 0), Play::Note(62, loud), Play::Note(62, 0), Play::Note(64, loud), Play::Note(64, 0)],
        "a glissando lets each key go as the next starts"
    );
    assert!(p.shared.played.iter().all(|v| v.load(Ordering::Relaxed) == 0), "nothing stays lit");
    hold(at(c, 0.1), true);
    hold(at(c, 0.1), false);
    let Some((0, Play::Note(60, soft))) = p.shared.keyboard.pop() else { panic!("C plays") };
    assert!(soft < loud, "the top of a key plays softer: {soft} vs {loud}");
    assert_eq!(p.shared.keyboard.pop(), Some((0, Play::Note(60, 0))));
    p.shared.key_owners[61].store(1, Ordering::Relaxed);
    p.shared.release_keyboard();
    assert_eq!(p.shared.keyboard.pop(), Some((1, Play::Note(61, 0))));
    p.shared.release_keyboard();
    assert!(p.shared.keyboard.pop().is_none());

    // The wheels bend and modulate the selected part the way its keys play
    // it; the pitch wheel springs back to the middle, the mod wheel stays.
    for id in ["wheel-pitch", "wheel-mod"] {
        let at = center(&h.ui, id);
        for dy in [0., -5., -20.] {
            h.tick(pointer(Point::new(at.x, at.y + dy), true));
        }
        h.tick(pointer(Point::new(at.x, at.y - 20.), false));
        h.idle(1);
    }
    let sent: Vec<(usize, Play)> = std::iter::from_fn(|| p.shared.keyboard.pop()).collect();
    let slot = selected_slot(&p);
    assert!(sent.iter().all(|(s, _)| *s == slot), "to the selected part");
    assert!(sent.iter().any(|(_, play)| matches!(play, Play::Bend(v) if *v > 8192)), "{sent:?}");
    assert!(sent.contains(&(slot, Play::Bend(8192))), "pitch springs back: {sent:?}");
    assert!(matches!(sent.last(), Some((_, Play::Mod(v))) if *v > 0), "{sent:?}");
    assert_eq!(p.shared.bend.load(Ordering::Relaxed), 8192);
    assert!(p.shared.modulation.load(Ordering::Relaxed) > 0, "mod stays where it is set");
    // A Shift drag moves the mod wheel finer than a step a pixel; the
    // pixels still add up.
    h.idle(60); // not a double click, which would reset it
    let before = p.shared.modulation.load(Ordering::Relaxed);
    let at = center(&h.ui, "wheel-mod");
    let fine = |y: f64, down: bool| {
        let mut input = pointer(Point::new(at.x, y), down);
        input.pointer.mods.shift = true;
        input
    };
    for dy in 0..30 {
        h.tick(fine(at.y - f64::from(dy), true));
    }
    h.tick(fine(at.y - 30., false));
    h.idle(1);
    let after = p.shared.modulation.load(Ordering::Relaxed);
    assert!(after > before, "fine moves add up: {before} to {after}");
    while p.shared.keyboard.pop().is_some() {}

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
    let at = center(ui, "header-1");
    assert!(native_files(&p, ui, at, &files, true));
    assert!(
        parts(&p)[1].path.ends_with("Native.nki"),
        "a file dropped on a header replaces its part"
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
fn favorites_star_from_the_row_or_the_menu_and_lead_the_browser() {
    let p = Arc::new(SamplerParams::new());
    {
        let mut v = p.shared.view.lock().unwrap();
        v.root = "/virtual".into();
        v.files = Arc::new(vec![
            "/virtual/Library/Piano.nki".into(),
            "/virtual/Library/Strings.nki".into(),
        ]);
    }
    let mut h = Harness::new(&p, 1180., 760.);
    let favorites = |p: &SamplerParams| p.selection.read().unwrap().favorites.clone();

    h.press("library-0");
    h.press("star-1");
    assert_eq!(favorites(&p), ["/virtual/Library/Strings.nki"]);
    // Right-click the row: Load, Load into new slot, a rule, then favorites.
    let at = center(&h.ui, "instrument-0");
    for buttons in [Buttons::default().set(Button::Secondary, true), Buttons::default()] {
        h.tick(Input {
            pointer: PointerInput {
                pos: Some(at),
                buttons,
                ..Default::default()
            },
            ..Default::default()
        });
    }
    h.idle(2);
    h.press("menu-item-3");
    assert_eq!(favorites(&p).len(), 2);
    h.press("star-1");
    assert_eq!(favorites(&p), ["/virtual/Library/Piano.nki"], "a second star unsets it");

    // Favorites, above the libraries, lists it; Enter there loads it.
    h.press("source-favorites");
    assert_eq!(h.ui.focus_key(), Some("instrument-0"), "Enter on an entry moves on to its presets");
    h.press("instrument-0");
    let selection = p.selection.read().unwrap().clone();
    assert!(selection.parts[0].path.ends_with("Piano.nki"));
    assert_eq!(selection.recent, ["/virtual/Library/Piano.nki"], "loading records it");
}

/// The browser's two panes: the arrows walk each, Tab crosses between
/// them, the search filters the chosen library, and a second click on it
/// widens the search to every library.
#[test]
fn the_split_browser_walks_both_panes() {
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
    let shown = |h: &Harness, id: &str| h.ui.scene().unwrap().surface(id).is_some();
    let key = |key: Key| Input {
        keys: vec![KeyPress { key, mods: Mods::default() }],
        ..Default::default()
    };
    assert!(!shown(&h, "instrument-0"), "nothing chosen, nothing listed");
    h.ui.focus("library-0");
    h.tick(key(Key::Down));
    h.idle(2);
    assert_eq!(h.ui.focus_key(), Some("library-1"), "Down walks the libraries");
    assert!(shown(&h, "instrument-0") && !shown(&h, "instrument-1"), "and lists the one it lands on");
    h.tick(key(Key::Tab));
    h.idle(2);
    assert_eq!(h.ui.focus_key(), Some("instrument-0"), "Tab crosses to the presets");
    h.tick(key(Key::Tab));
    h.idle(2);
    assert_eq!(h.ui.focus_key(), Some("library-1"), "and back to the library");
    h.tick(key(Key::Up));
    h.idle(2);
    assert!(shown(&h, "instrument-1"), "Keys lists both its presets");
    h.ui.focus("search");
    h.tick(Input { text: "organ".into(), ..Default::default() });
    h.idle(2);
    assert!(shown(&h, "instrument-0") && !shown(&h, "instrument-1"), "the search filters them");
    h.tick(key(Key::Enter));
    h.idle(3);
    assert!(read(&p.selection).parts[0].path.ends_with("Organ.nki"), "Enter loads");

    // The divider and the browser's edge drag, and are kept once let go.
    for (id, dx, dy) in [("browser-split", 0., 40.), ("splitter", 30., 0.)] {
        let at = center(&h.ui, id);
        for (step, down) in [(0., true), (0.5, true), (1., true), (1., false)] {
            h.tick(pointer(Point::new(at.x + dx * step, at.y + dy * step), down));
        }
        h.idle(2);
    }
    let s = read(&p.selection).clone();
    assert!(s.browser_split as f64 > browser::SPLIT, "{}", s.browser_split);
    assert!(s.browser_width as f64 > SIDEBAR, "{}", s.browser_width);
}

#[test]
fn save_multi_names_the_rack_and_writes_it_under_multis() {
    let root = std::env::temp_dir().join(format!("kontakto-save-{}", std::process::id()));
    let p = Arc::new(SamplerParams::new());
    {
        let mut s = p.selection.write().unwrap();
        s.root = root.to_string_lossy().into_owned();
        s.parts = vec![crate::plugin::Part {
            path: "/virtual/Keys/Piano.nki".into(),
            tune: -2.,
            ..Default::default()
        }];
        s.order = vec![0];
    }
    let mut h = Harness::new(&p, 1180., 760.);
    h.press("app-menu");
    h.press("menu-item-6");
    assert!(h.ui.scene().unwrap().surface("multi-name").is_some(), "Save multi… asks for a name");
    h.type_into("multi-name", "Duo/Night");
    let path = header::multi_path(&root.to_string_lossy(), "DuoNight");
    let saved = crate::plugin::SavedMulti::read(&path);
    std::fs::remove_dir_all(&root).ok();
    let saved = saved.expect("the multi is written under Multis");
    assert_eq!(saved.name, "DuoNight");
    assert!(saved.parts == p.selection.read().unwrap().parts);
    assert_eq!(p.selection.read().unwrap().multi, path.to_string_lossy());
    assert!(h.ui.scene().unwrap().surface("multi-name").is_none(), "the strip closes");
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

/// The owner's library instruments named in `names` (comma-separated stems).
fn library_instruments(files: &[PathBuf], names: &str) -> Vec<Arc<import::Instrument>> {
    names
        .split(',')
        .filter_map(|name| files.iter().find(|p| p.file_stem().is_some_and(|n| n == name)))
        .filter_map(|p| import::read(p).ok())
        .map(Arc::new)
        .collect()
}

/// `i`'s scripts run, and `KONTAKTO_PRESS="$var=1,$other=2"` edits made
/// (a page switch, say) as a player would.
fn scripted(i: &import::Instrument) -> crate::plugin::ScriptView {
    let Some(mut rt) = load_scripts(i, i.script_state.clone(), 48000.).0 else {
        return script_interface(None);
    };
    let mut engine = crate::ksp::LogEngine::new(Vec::new(), 48_000.0);
    // A second of audio: listeners and waits run as they would once playing.
    let mut run = |rt: &mut crate::ksp::Runtime| (0..100).for_each(|_| rt.process(&mut engine, 480));
    run(&mut rt);
    for press in std::env::var("KONTAKTO_PRESS").unwrap_or_default().split(',') {
        let Some((var, value)) = press.split_once('=') else { continue };
        let live = script_interface(Some(&rt));
        let control = live
            .interface
            .as_ref()
            .and_then(|u| u.controls.iter().position(|c| c.variable == var));
        if let (Some(control), Ok(value)) = (control, value.parse()) {
            let mut engine = crate::ksp::LogEngine::new(Vec::new(), 48_000.0);
            rt.ui_control(&mut engine, live.slot, control, value);
        }
        run(&mut rt);
    }
    script_interface(Some(&rt))
}

/// A plugin whose rack holds `instruments` (when `loaded`) as the loader
/// leaves them: scripts run, pictures read. `state` stages the
/// screenshots' special cases.
fn racked(files: &[PathBuf], instruments: &[Arc<import::Instrument>], loaded: bool, state: &str) -> Arc<SamplerParams> {
    let root = Path::new(import::LIBRARY_ROOT);
    let p = Arc::new(SamplerParams::new());
    {
        let mut view = p.shared.view.lock().unwrap();
        view.artwork = artwork::scan(root, files);
        view.files = Arc::new(files.to_vec());
        view.root = import::LIBRARY_ROOT.into();
        for (slot, i) in instruments.iter().enumerate().filter(|_| loaded) {
            p.selection.write().unwrap().parts.push(Part {
                path: i.path.to_string_lossy().into(),
                group: i.first_playable_group().unwrap_or(0) as u32,
                ..Default::default()
            });
            let script = scripted(i);
            let (interface, keys) = (script.interface, script.keys);
            // Control pictures, as the plugin loads them: they size controls.
            let names = interface.iter().flat_map(|u| &u.controls).filter_map(|c| {
                match c.properties.get("$CONTROL_PAR_PICTURE") {
                    Some(crate::ksp::Value::Text(n)) => Some(n.as_str()),
                    _ => None,
                }
            });
            let pictures = Arc::new(artwork::pictures(&i.path, names));
            view.parts[slot] = PartView {
                pictures,
                wallpaper: artwork::performance(i, interface.as_ref().map(|u| u.wallpaper.as_str()))
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
    p.selection.write().unwrap().appearance = match state {
        "color" => 1,
        "artwork" => 2,
        _ => 0,
    };
    if state == "playing" {
        // Keys sounding, soft to hard, on screen and from the host.
        for (note, velocity) in [(48, 40), (52, 127)] {
            p.shared.played[note].store(velocity, Ordering::Relaxed);
        }
        for (note, velocity) in [(55, 90), (58, 110), (61, 30)] {
            p.shared.heard[note].store(velocity, Ordering::Relaxed);
        }
    }
    p
}

/// Prints a library instrument's script controls as authored and as the
/// panel reads them. `KONTAKTO_SHOT` names it; run with `--ignored --nocapture`.
#[test]
#[ignore]
fn dump_panel() {
    let files = import::presets(Path::new(import::LIBRARY_ROOT)).unwrap_or_default();
    let names = std::env::var("KONTAKTO_SHOT").unwrap_or_default();
    for i in library_instruments(&files, &names) {
        let script = scripted(&i);
        for (note, k) in script.keys.iter() {
            println!("key {note} {:?} {:?}", k.color, k.name);
        }
        let (lo, hi) = i.zones.iter().filter(|z| z.available).fold((127, 0), |(l, h), z| (z.low_key.min(l), z.high_key.max(h)));
        println!("zones span {lo}..={hi}");
        let Some(interface) = script.interface else { continue };
        println!("== {} ({}x{})", i.name, interface.width, interface.height);
        for (n, c) in interface.controls.iter().enumerate() {
            let prop = |k: &str| c.properties.get(&format!("$CONTROL_PAR_{k}")).map(|v| format!("{v:?}")).unwrap_or_default();
            println!(
                "{n:3} {:12} {:28} x{} y{} w{} h{} hide{} text={} pic={} val={}",
                c.kind, c.variable, prop("POS_X"), prop("POS_Y"), prop("WIDTH"), prop("HEIGHT"),
                prop("HIDE"), prop("TEXT"), prop("PICTURE"), prop("VALUE")
            );
        }
        let names = interface.controls.iter().filter_map(|c| match c.properties.get("$CONTROL_PAR_PICTURE") {
            Some(crate::ksp::Value::Text(n)) => Some(n.as_str()),
            _ => None,
        });
        let pictures = artwork::pictures(&i.path, names);
        println!("{:#?}", panel::sections(&interface, &pictures));
    }
}

/// What one frame costs, in microseconds: the view's build plus layout and
/// paint-list resolve, and a CPU raster of the result (a stand-in for the
/// GPU's share). `KONTAKTO_SHOT` picks the rack; run with `--ignored`.
#[test]
#[ignore]
fn frame_cost() {
    use moose::mui::mui::vello::{
        self,
        vello_cpu::{Pixmap, RenderContext, Resources},
    };
    use std::time::Instant;
    let files = import::presets(Path::new(import::LIBRARY_ROOT)).unwrap_or_default();
    let chosen = std::env::var("KONTAKTO_SHOT").unwrap_or_else(|_| {
        "03 Areia - 6 Celli - Core Techniques,Vista - 3 Cellos,Una Corda Pure".into()
    });
    let instruments = library_instruments(&files, &chosen);
    let p = racked(&files, &instruments, true, "perform");
    let (w, hgt) = (1600u16, 1000u16);
    let mut h = Harness::new(&p, f64::from(w), f64::from(hgt));
    h.idle(10);
    let mut raster = RenderContext::new(w, hgt);
    let mut resources = Resources::default();
    let mut cache = vello::Cache::default();
    let mut pix = Pixmap::new(w, hgt);
    let mut measure = |h: &mut Harness, label: &str, input: &mut dyn FnMut(usize) -> Input| {
        let n = 120;
        let (mut frame, mut paint) = (Vec::new(), Vec::new());
        for i in 0..n {
            let t = Instant::now();
            h.tick(input(i));
            frame.push(t.elapsed().as_secs_f64() * 1e6);
            let t = Instant::now();
            raster.reset();
            vello::paint(
                &mut vello::Cpu { ctx: &mut raster, resources: &mut resources, cache: &mut cache },
                h.ui.scene().unwrap(),
                vello::kurbo::Affine::IDENTITY,
            )
            .unwrap();
            raster.flush();
            raster.render(&mut pix, &mut resources);
            paint.push(t.elapsed().as_secs_f64() * 1e6);
        }
        let median = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        println!(
            "{label:>10}: build+layout {:>7.0} us   cpu paint {:>7.0} us",
            median(&mut frame),
            median(&mut paint)
        );
    };
    measure(&mut h, "idle", &mut |_| Input::default());
    let knob = center(&h.ui, "volume-0");
    measure(&mut h, "dragging", &mut |i| {
        pointer(Point::new(knob.x, knob.y - (i % 40) as f64), i % 60 != 59)
    });
    h.idle(3);
    let shared = p.clone();
    measure(&mut h, "keys", &mut |i| {
        let note = 48 + (i % 24);
        shared.shared.played[note].store(if i % 2 == 0 { 100 } else { 0 }, Ordering::Relaxed);
        Input::default()
    });
}

/// Renders the editor in its main states to `.impeccable/review/` (git-ignored)
/// for a visual check. Uses the owner's library when present.
#[test]
fn screenshot() {
    use moose::mui::mui::vello::{
        self,
        vello_cpu::{Pixmap, RenderContext, Resources},
    };
    let files = import::presets(Path::new(import::LIBRARY_ROOT)).unwrap_or_default();
    // KONTAKTO_SHOT="Name A,Name B" renders other instruments in the rack slots.
    let chosen = std::env::var("KONTAKTO_SHOT")
        .unwrap_or_else(|_| "Vista - Harp,Vista - 3 Cellos,Vista - 5 Violins".into());
    let instruments = library_instruments(&files, &chosen);
    std::fs::create_dir_all(".impeccable/review").unwrap();
    let states: [(&str, bool, &[&str]); 17] = [
        ("empty", false, &[]),
        ("perform", true, &[]),
        ("mapping", true, &["tab-mapping"]),
        ("rack", true, &["tab-rack"]),
        ("info", true, &["tab-info"]),
        ("library", false, &["library-1"]),
        ("multis", false, &["picker-multis"]),
        ("settings", true, &["app-menu", "menu-item-3"]),
        ("menu", true, &["app-menu"]),
        ("save", true, &["app-menu", "menu-item-6"]),
        ("collapsed", true, &["toggle-browser", "keyboard-toggle"]),
        ("error", true, &[]),
        ("playing", true, &["qwerty"]),
        ("color", true, &[]),
        ("artwork", true, &[]),
        ("folded", true, &["collapse-0", "collapse-1"]),
        ("mixer", true, &["tab-mixer"]),
    ];
    // KONTAKTO_STATES="perform,rack" renders only those states.
    let only = std::env::var("KONTAKTO_STATES").unwrap_or_default();
    let wanted = |state: &str| only.is_empty() || only.split(',').any(|s| s == state);
    // KONTAKTO_WIDTHS="900,2000" renders those window widths instead.
    let widths = std::env::var("KONTAKTO_WIDTHS").unwrap_or_else(|_| "1180,900".into());
    let sizes: Vec<(u16, u16)> = widths
        .split(',')
        .filter_map(|w| w.trim().parse().ok())
        .map(|w: u16| (w, if w < 1000 { 600 } else if w > 1500 { 1000 } else { 760 }))
        .collect();
    for (state, loaded, presses) in states.into_iter().filter(|(s, ..)| wanted(s)) {
        for &(width, height) in &sizes {
            let p = racked(&files, &instruments, loaded, state);
            if state == "mixer" {
                // Mid-song: parts on two buses, one sending to a named third.
                let mut selection = p.selection.write().unwrap();
                for (slot, part) in selection.parts.iter_mut().enumerate() {
                    part.output = [0, 0, 1][slot % 3];
                    part.gain = [-3., 0.2, -10.][slot % 3];
                    part.pan = [0., -0.1, 0.34][slot % 3];
                }
                if let Some(part) = selection.parts.get_mut(1) {
                    (part.aux, part.aux_gain) = (2, -6.);
                }
                selection.bus_mut(2).name = "Hall".into();
                selection.bus_mut(1).gain = -4.5;
                drop(selection);
                let m = &p.shared.meters;
                for (meter, [l, r]) in m.parts.iter().zip([[0.5, 0.42], [0.9, 1.05], [0.05, 0.03]]) {
                    meter[0].store(f32::to_bits(l), Ordering::Relaxed);
                    meter[1].store(f32::to_bits(r), Ordering::Relaxed);
                }
                for (meter, [l, r]) in m.buses.iter().zip([[0.7, 0.6], [0.04, 0.03], [0.2, 0.25]]) {
                    meter[0].store(f32::to_bits(l), Ordering::Relaxed);
                    meter[1].store(f32::to_bits(r), Ordering::Relaxed);
                }
                m.master[0].store(0.3f32.to_bits(), Ordering::Relaxed);
                m.master[1].store(0.28f32.to_bits(), Ordering::Relaxed);
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
            let mut visible = vec!["octave-up", "panic", "app-menu", "toggle-browser"];
            if state != "collapsed" {
                visible.push("search");
            }
            if loaded {
                visible.extend(["tab-info", if state == "mixer" { "master-strip" } else { "header-0" }]);
            } else {
                visible.push("rack-drop");
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
            if loaded && state != "collapsed" {
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

    h.press("ksp-0-0");
    assert_eq!(value(0), crate::ksp::Value::Int(1), "a switch toggles on");
    h.press("ksp-0-0");
    assert_eq!(value(0), crate::ksp::Value::Int(0), "and off");

    let knob = center(&h.ui, "ksp-0-1");
    for y in [0., -10., -60.] {
        h.tick(pointer(Point::new(knob.x, knob.y + y), true));
    }
    h.tick(pointer(Point::new(knob.x, knob.y - 60.), false));
    h.idle(2);
    let crate::ksp::Value::Int(dragged) = value(1) else {
        panic!("knob value")
    };
    assert!(dragged > 10, "dragging a knob up raises it, got {dragged}");

    h.press("ksp-0-2");
    h.press("menu-item-1");
    assert_eq!(
        value(2),
        crate::ksp::Value::Int(1),
        "a menu item sets the menu"
    );
}

#[test]
fn idle_editor_rebuilds_only_when_something_moves() {
    let p = Arc::new(SamplerParams::new());
    // The library is scanned: nothing is pending for the loader.
    p.shared.view.lock().unwrap().root = import::LIBRARY_ROOT.into();
    static DISK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let meters = Meters::default();
    let mut watch = Watch {
        disk_counter: Some(&DISK),
        ..Watch::default()
    };
    let computer = computer::Computer::default();
    let mut changed = || watch.changed(&p, &meters, &computer);
    assert!(changed(), "the first tick builds");
    std::thread::sleep(Duration::from_millis(110));
    assert!(!changed(), "nothing moved: no rebuild, however long");
    p.shared.voices.store(3, Ordering::Relaxed);
    assert!(!changed(), "readouts wait for their next look");
    std::thread::sleep(Duration::from_millis(READOUT_MS + 10));
    assert!(changed(), "a voice count change shows on the next look");
    assert!(!changed());
    p.shared.dropouts.store(1, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(READOUT_MS + 10));
    assert!(changed(), "so does a dropout");
    // Samples read from disk move the disk readout, which then settles.
    DISK.fetch_add(8 << 20, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(READOUT_MS + 10));
    assert!(changed(), "disk throughput shows");
    assert!(f32::from_bits(meters.disk.load(Ordering::Relaxed)) > 1.);
    for _ in 0..12 {
        std::thread::sleep(Duration::from_millis(READOUT_MS + 10));
        changed();
    }
    std::thread::sleep(Duration::from_millis(READOUT_MS + 10));
    assert!(!changed(), "an idle disk reads 0 and rebuilds nothing");
    p.shared.view.lock().unwrap().parts[0].loading = true;
    assert!(changed(), "loading animates");
    assert!(!changed(), "at most every {ANIMATION_MS} ms");
}

#[test]
fn library_is_the_first_folder_under_the_root() {
    let at = |root: &str, path: &str| library_of(root, Path::new(path));
    assert_eq!(at("/libs", "/libs/Solo/Instruments/a.nki"), "Solo");
    assert_eq!(at("/libs/", "/libs//Solo/a.nki"), "Solo");
    assert_eq!(at("/libs", "/libsX/Solo/a.nki"), "");
    assert_eq!(at("/libs", "/other/a.nki"), "");
}

/// The window as pixels, RGBA, painted from the last frame's scene.
fn pixels(ui: &Ui, width: u16, height: u16) -> Vec<u8> {
    use moose::mui::mui::vello::{
        self,
        vello_cpu::{Pixmap, RenderContext, Resources},
    };
    let mut ctx = RenderContext::new(width, height);
    let mut resources = Resources::default();
    vello::paint(
        &mut vello::Cpu {
            ctx: &mut ctx,
            resources: &mut resources,
            cache: &mut vello::Cache::default(),
        },
        ui.scene().unwrap(),
        vello::kurbo::Affine::IDENTITY,
    )
    .unwrap();
    ctx.flush();
    let mut pix = Pixmap::new(width, height);
    ctx.render(&mut pix, &mut resources);
    pix.take_unpremultiplied()
        .iter()
        .flat_map(|p| [p.r, p.g, p.b, p.a])
        .collect()
}

fn two_parts() -> Arc<SamplerParams> {
    let p = Arc::new(SamplerParams::new());
    for name in ["Piano", "Strings"] {
        p.selection.write().unwrap().parts.push(Part {
            path: format!("/virtual/Library/{name}.nki"),
            ..Default::default()
        });
    }
    p
}

/// Routing from the mixer: menus pick a part's output, send and MIDI
/// input and a bus's host port; a strip dropped on a bus routes it there.
#[test]
fn mixer_routing_edits() {
    let p = two_parts();
    let part = |p: &SamplerParams, n: usize| p.selection.read().unwrap().parts[n].clone();
    let mut h = Harness::new(&p, 1180., 760.);
    h.press("tab-mixer");
    let shows = |h: &Harness, id: &str| h.ui.scene().unwrap().surface(id).is_some();
    assert!(shows(&h, "strip-0") && shows(&h, "strip-1") && shows(&h, "master-strip"));
    assert!(shows(&h, "bus-0") && !shows(&h, "bus-2"), "only buses in use");

    h.press("mix-out-0");
    h.press("menu-item-2");
    assert_eq!(part(&p, 0).output, 2, "the output menu routes");
    assert!(shows(&h, "bus-2"), "a bus in use gets its strip");

    // No send, a rule, then st.1…: the fourth bus.
    h.press("mix-aux-1");
    h.press("menu-item-5");
    assert_eq!(part(&p, 1).aux, 3);
    assert!(shows(&h, "bus-3"));

    // Omni, channels 1–16, a rule, a heading, ports A–D.
    h.press("mix-in-1");
    h.press("menu-item-3");
    h.press("mix-in-1");
    h.press("menu-item-20");
    assert_eq!((part(&p, 1).port, part(&p, 1).channel), (1, 2));

    h.drag("mix-name-1", "bus-2");
    assert_eq!(part(&p, 1).output, 2, "a strip dropped on a bus plays through it");

    h.press("solo-mix-0");
    h.press("mute-bus-2");
    assert!(part(&p, 0).solo);
    assert!(p.selection.read().unwrap().bus(2).mute);

    h.press("bus-port-2");
    h.press("menu-item-5");
    assert_eq!(p.selection.read().unwrap().bus(2).port, 5);

    assert!(!shows(&h, "bus-1"));
    h.press("mix-add-bus");
    assert!(shows(&h, "bus-1"), "+ shows the next bus");

    // Right-click: Rename…, Reset, a rule, Route to…
    p.selection.write().unwrap().parts[0].gain = -6.;
    h.idle(2);
    let at = center(&h.ui, "strip-0");
    for buttons in [Buttons::default().set(Button::Secondary, true), Buttons::default()] {
        h.tick(Input {
            pointer: PointerInput {
                pos: Some(at),
                buttons,
                ..Default::default()
            },
            ..Default::default()
        });
    }
    h.idle(2);
    h.press("menu-item-1");
    assert_eq!((part(&p, 0).gain, part(&p, 0).solo), (0., false), "Reset");
    assert_eq!(part(&p, 0).output, 2, "and the routing stays");
}

/// A meter is a canvas that reads the audio thread's level as the scene is
/// walked: the same tree, framed again, shows the new level. The editor
/// asks for frames only while a meter moves.
#[test]
fn mixer_meters_paint_without_a_rebuild() {
    let p = two_parts();
    let (width, height) = (1180u16, 760u16);
    let mut h = Harness::new(&p, f64::from(width), f64::from(height));
    h.press("tab-mixer");
    let root = (h.build)(&mut h.ui, &mut h.bridge);
    h.ui.frame(root.clone(), Some(h.size), Input::default(), 0.).unwrap();
    let quiet = pixels(&h.ui, width, height);
    for m in &p.shared.meters.parts[0] {
        m.store(0.8f32.to_bits(), Ordering::Relaxed);
    }
    h.ui.frame(root, Some(h.size), Input::default(), 0.).unwrap();
    let loud = pixels(&h.ui, width, height);
    let r = h.ui.scene().unwrap().surface("mix-fader-0-meter").unwrap().frame;
    let x = (r.x + 1.) as usize;
    let at = |y: usize| (y * usize::from(width) + x) * 4;
    let lit = (r.y as usize..(r.y + r.size.height) as usize)
        .filter(|&y| quiet[at(y)..at(y) + 4] != loud[at(y)..at(y) + 4])
        .count();
    assert!(lit as f64 > r.size.height / 2., "the meter shows the level: {lit} rows");

    static DISK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let meters = Meters::default();
    let mut watch = Watch {
        disk_counter: Some(&DISK),
        ..Watch::default()
    };
    let computer = computer::Computer::default();
    let mut changed = || watch.changed(&p, &meters, &computer);
    let settle = Duration::from_millis(ANIMATION_MS + 5);
    let level = |v: f32| {
        for m in &p.shared.meters.parts[0] {
            m.store(v.to_bits(), Ordering::Relaxed);
        }
    };
    level(0.);
    changed();
    std::thread::sleep(settle);
    assert!(!changed(), "silent meters ask for nothing");
    level(0.5);
    std::thread::sleep(settle);
    assert!(changed(), "a moving meter asks for a frame");
    level(0.);
    std::thread::sleep(settle);
    assert!(changed(), "and one more to draw it empty");
    std::thread::sleep(settle);
    assert!(!changed(), "silent meters ask for nothing");
}
