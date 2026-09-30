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
    art: Arc<art::Art>,
}

impl Harness {
    fn new(p: &Arc<SamplerParams>, width: f64, height: f64) -> Self {
        let computer = Arc::<computer::Computer>::default();
        let art = Arc::<art::Art>::default();
        let mut h = Self {
            ui: theme::ui(),
            build: Box::new(build(p, Arc::default(), computer.clone(), Arc::default(), art.clone())),
            bridge: Bridge::new(p.clone()),
            size: Size::new(width, height),
            computer,
            art,
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

    /// Frames until the library artwork asked for is made and drawn.
    fn settle_art(&mut self) {
        loop {
            self.idle(1);
            if !self.art.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        self.idle(2);
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

/// A mouse glissando with no instrument loaded: one key sounds and lights at
/// a time, across white and black keys, the key it began on looks like any
/// other once the pointer has left it, and letting go anywhere, even outside
/// the window, stops it and leaves nothing lit.
#[test]
fn a_glissando_follows_the_pointer_and_lets_go_anywhere() {
    let p = Arc::new(SamplerParams::new());
    let (width, height) = (1180u16, 760u16);
    let mut h = Harness::new(&p, width.into(), height.into());
    let frame = |h: &Harness, n: u8| h.ui.scene().unwrap().surface(&format!("key-{n}")).unwrap().frame;
    let low = |h: &Harness, n: u8| {
        let r = frame(h, n);
        Point::new(r.x + r.size.width / 2., r.y + r.size.height * 0.85)
    };
    let color = |h: &Harness, at: Point| {
        let pix = pixels(&h.ui, width, height);
        let i = (at.y as usize * usize::from(width) + at.x as usize) * 4;
        [pix[i], pix[i + 1], pix[i + 2]]
    };
    let lit = |p: &SamplerParams| (0..128u8).filter(|&n| p.shared.played[n as usize].load(Ordering::Relaxed) > 0).collect::<Vec<_>>();
    let (c, e) = (low(&h, 60), low(&h, 64));
    let (c_rest, e_rest) = (color(&h, c), color(&h, e));
    for _ in 0..20 {
        h.tick(pointer(c, false));
    }
    assert_ne!(color(&h, c), c_rest, "a key lifts under the pointer");
    let sent = |p: &SamplerParams| std::iter::from_fn(|| p.shared.keyboard.pop()).map(|(_, play)| play).collect::<Vec<_>>();
    let path = [60u8, 61, 62, 63, 64];
    let mut want = Vec::new();
    for (i, &n) in path.iter().enumerate() {
        // Black keys are pressed high, where they lie over the white ones.
        let r = frame(&h, n);
        let at = Point::new(r.x + r.size.width / 2., r.y + r.size.height * if n % 12 == 1 || n % 12 == 3 { 0.3 } else { 0.85 });
        for _ in 0..3 {
            h.tick(pointer(at, true));
        }
        assert_eq!(lit(&p), [n], "only the key under the pointer lights");
        let got = sent(&p);
        let velocity = match got.last() {
            Some(Play::Note(m, v)) if *m == n && *v > 0 => *v,
            other => panic!("{n} starts: {got:?} {other:?}"),
        };
        if i > 0 {
            want.push(Play::Note(path[i - 1], 0));
        }
        want.push(Play::Note(n, velocity));
        assert_eq!(got, want[want.len() - got.len()..], "each key goes as the next starts");
    }
    // Resting on E, the LED left behind on C has faded: C looks as it did
    // before it was played, though it holds the pointer's capture.
    for _ in 0..40 {
        h.tick(pointer(e, true));
    }
    assert_eq!(color(&h, c), c_rest, "the key the glissando began on lets go of its look");
    assert_ne!(color(&h, e), e_rest, "the sounding key is lit");
    // Out of the window, then up.
    let away = Input {
        pointer: PointerInput { pos: None, buttons: Buttons::PRIMARY, ..Default::default() },
        ..Default::default()
    };
    h.tick(away.clone());
    assert_eq!(lit(&p), [64], "leaving the keys keeps the last one sounding");
    h.tick(Input { pointer: PointerInput { pos: None, ..Default::default() }, ..Default::default() });
    h.idle(40);
    assert_eq!(sent(&p), [Play::Note(64, 0)], "letting go outside the window stops it, once");
    assert!(lit(&p).is_empty(), "nothing stays lit");
    assert_eq!(color(&h, e), e_rest, "and nothing looks lit");
    // Pressed and let go off the key, on the window's chrome.
    h.tick(pointer(c, true));
    h.tick(pointer(c, true));
    let off = center(&h.ui, "panic");
    h.tick(pointer(off, true));
    h.tick(pointer(off, false));
    h.idle(3);
    assert_eq!(sent(&p).len(), 2, "one note-on, one note-off");
    assert!(lit(&p).is_empty());
}

/// Closing the editor lets go of what its keyboard holds: the mouse's note
/// and the computer keyboard's, as losing the focus does.
#[test]
fn closing_the_editor_lets_go_of_the_keys() {
    let p = Arc::new(SamplerParams::new());
    let mut h = Harness::new(&p, 1180., 760.);
    let at = center(&h.ui, "key-60");
    h.tick(pointer(at, true));
    h.tick(pointer(at, true));
    assert_eq!(p.shared.keyboard.pop().map(|(_, play)| play).and_then(|play| match play {
        Play::Note(n, v) if v > 0 => Some(n),
        _ => None,
    }), Some(60));
    let mut editor = editor(p.clone());
    editor.close();
    assert_eq!(p.shared.keyboard.pop(), Some((0, Play::Note(60, 0))), "closing lets the note go");
    assert!(p.shared.played.iter().all(|v| v.load(Ordering::Relaxed) == 0), "and unlights it");
    // Reopened, the gesture the close cut short sends nothing more.
    h.ui.close();
    h.idle(3);
    assert!(p.shared.keyboard.pop().is_none());
}

fn selected_slot(p: &SamplerParams) -> usize {
    p.shared.selected.load(Ordering::Relaxed) as usize
}

/// Every header is the one compact line, open or folded, narrow or wide:
/// the name shrinks before anything leaves the rack.
#[test]
fn a_header_is_one_line_at_any_width() {
    for width in [900., 1180.] {
        let p = Arc::new(SamplerParams::new());
        p.selection.write().unwrap().parts.push(Part {
            path: "/virtual/Library/Una Corda Pure.nki".into(),
            ..Default::default()
        });
        let mut h = Harness::new(&p, width, 600.);
        h.idle(2);
        let scene = h.ui.scene().unwrap();
        let frame = |id: &str| scene.surface(id).unwrap().frame;
        let (header, name, port, remove) = (frame("header-0"), frame("name-0"), frame("midi-0"), frame("remove-0"));
        assert!((header.size.height - (rack::SLIM - 1.)).abs() < 0.5, "at {width}: {header:?}");
        assert!(port.y < name.y + name.size.height && name.y < port.y + port.size.height, "at {width}");
        assert!(remove.x + remove.size.width <= header.x + header.size.width, "at {width}");
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

    let tall = |h: &Harness| h.ui.scene().unwrap().surface("part-0").unwrap().frame.size.height;
    let open = tall(&h);
    h.press("collapse-0");
    assert!(parts(&p)[0].collapsed, "the chevron folds a part");
    let folding = tall(&h);
    assert!(folding < open - 1. && folding > rack::SLIM + 1., "it springs shut, not at once: {open} to {folding}");
    h.idle(30);
    assert!(h.ui.scene().unwrap().surface("stage-0").is_none());
    assert!((tall(&h) - rack::SLIM).abs() < 0.5, "to its header alone");
    h.press("collapse-0");
    let opening = tall(&h);
    assert!(opening > rack::SLIM + 1. && opening < open - 1., "and springs open: {opening}");
    h.idle(30);
    assert!(h.ui.scene().unwrap().surface("stage-0").is_some(), "and unfolds it");
    assert!((tall(&h) - open).abs() < 0.5);
    // The part's foot drags it down to its slim line, which folds it; a
    // double-click on the foot unfolds it again.
    let edge = |h: &Harness| center(&h.ui, "resize-0");
    let at = edge(&h);
    for dy in [0., -4., -400.] {
        h.tick(pointer(Point::new(at.x, at.y + dy), true));
    }
    h.tick(pointer(Point::new(at.x, at.y - 400.), false));
    h.idle(30);
    assert!(parts(&p)[0].collapsed, "dragged to its slim line, a part folds");
    let at = edge(&h);
    for down in [true, false, true, false] {
        h.tick(pointer(at, down));
    }
    h.idle(30);
    let part = &parts(&p)[0];
    assert!(!part.collapsed && part.height == 0., "a double-click unfolds it whole");
    // Dragged part way, it follows the pointer as it goes and stays there.
    h.idle(60); // not a double click
    let at = edge(&h);
    h.tick(pointer(at, true));
    let mut heights = Vec::new();
    for dy in [-4., -12., -20.] {
        h.tick(pointer(Point::new(at.x, at.y + dy), true));
        h.tick(pointer(Point::new(at.x, at.y + dy), true));
        heights.push(tall(&h));
    }
    h.tick(pointer(Point::new(at.x, at.y - 20.), false));
    h.idle(30);
    assert!(heights.windows(2).all(|w| w[1] < w[0] - 5.), "the height follows the drag: {heights:?}");
    assert!((tall(&h) - heights[2]).abs() < 1. && heights[2] < open - 10., "and stays: {} of {open}", tall(&h));
    assert!(!parts(&p)[0].collapsed && parts(&p)[0].height > 0., "{:?} {:?}", parts(&p)[0].collapsed, parts(&p)[0].height);
    let at = edge(&h);
    for down in [true, false, true, false] {
        h.tick(pointer(at, down));
    }
    h.idle(30);

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
        "artwork" | "sharp" | "sticky" => 2,
        _ => 0,
    };
    p.selection.write().unwrap().sharp_artwork = state == "sharp";
    if state == "resized" {
        // The first part sized short: its controls clip and fade out.
        p.selection.write().unwrap().parts[0].height = 220.;
    }
    if state == "loading" {
        // One part reading its samples, one not yet read at all.
        let mut view = p.shared.view.lock().unwrap();
        for (slot, done) in [(1, 0.42), (2, 0.)] {
            view.parts[slot].loading = true;
            view.parts[slot].bytes = 0;
            let done = (f64::from(crate::engine::LOAD_DONE) * done) as u32;
            p.shared.load_progress[slot].store(done, Ordering::Relaxed);
        }
        view.parts[2].instrument = None;
        view.parts[2].interface = None;
    }
    if state == "pressed" {
        // Held on every kind of key: keyswitch red, unmapped grey, mapped
        // green, white and black, soft to hard. From the host: a played key
        // nothing on screen holds is let go on the first frame.
        for (note, velocity) in [(14, 127), (15, 60), (30, 100), (32, 100), (48, 25), (61, 90), (64, 127), (66, 60)] {
            p.shared.heard[note].store(velocity, Ordering::Relaxed);
        }
    }
    if state == "playing" {
        // Keys sounding, soft to hard, from the host; a keyswitch.
        for (note, velocity) in [(15, 100), (48, 40), (52, 127), (55, 90), (58, 110), (61, 30)] {
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
        let t = std::time::Instant::now();
        let sections = panel::sections(&interface, &pictures);
        println!("{sections:#?}\nsections in {} us", t.elapsed().as_micros());
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
    let library = library_instruments(&files, &chosen);
    // KONTAKTO_PARTS="1,4,8,16": the rack filled to each count, cycling the chosen.
    let counts = std::env::var("KONTAKTO_PARTS").unwrap_or_else(|_| "1,4,8,16".into());
    for count in counts.split(',').filter_map(|n| n.trim().parse::<usize>().ok()) {
    println!("{count} parts");
    let instruments: Vec<_> = library.iter().cycle().take(count).cloned().collect();
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
        // The median, and the least: a busy machine delays some frames,
        // never speeds one up.
        let spread = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            (v[v.len() / 2], v[0])
        };
        let ((fm, fl), (pm, pl)) = (spread(&mut frame), spread(&mut paint));
        println!("{label:>10}: build+layout {fm:>6.0} us (min {fl:>5.0})   cpu paint {pm:>6.0} us (min {pl:>5.0})");
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
        shared.shared.heard[note].store(if i % 2 == 0 { 100 } else { 0 }, Ordering::Relaxed);
        Input::default()
    });
    }
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
    let states: [(&str, bool, &[&str]); 30] = [
        ("empty", false, &[]),
        ("perform", true, &[]),
        // Esc: no part selected, the keys show what each part plays.
        ("unselected", true, &[]),
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
        ("sharp", true, &[]),
        // Scrolled down: the parts above stack their headers at the top.
        ("sticky", true, &[]),
        // The first part dragged shorter: its controls clip.
        ("resized", true, &[]),
        ("loading", true, &[]),
        ("pressed", true, &[]),
        ("sound", true, &["tab-sound"]),
        // Edited: the library's curves stay drawn faintly under the edits.
        ("sound-edited", true, &["tab-sound"]),
        ("sound-compact", true, &["tab-sound", "edit-compact"]),
        ("sound-modulation", true, &["tab-sound", "edit-lower-Modulation"]),
        ("sound-effects", true, &["tab-sound", "edit-lower-Effects"]),
        // A value double-clicked into a field.
        ("sound-typing", true, &["tab-sound"]),
        ("mixer-wide", true, &["tab-mixer", "mix-wide"]),
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
    // Sticky headers need a second part to scroll past the first.
    let sticky_ok = instruments.len() > 1;
    for (state, loaded, presses) in states
        .into_iter()
        .filter(|(s, ..)| wanted(s) && (*s != "sticky" || sticky_ok))
    {
        for &(width, height) in &sizes {
            let p = racked(&files, &instruments, loaded, state);
            if state == "sound-edited" {
                use crate::engine::overrides::{Override, Param};
                let mut selection = p.selection.write().unwrap();
                for (param, offset) in [(Param::Release, -0.2), (Param::Attack, 0.15), (Param::Sustain, -0.2)] {
                    selection.parts[0].edits.set(Override { group: None, param, offset });
                }
            }
            if state.starts_with("mixer") || state.starts_with("sound") {
                // A chord with a little noise under it, for the spectrum.
                let mut seed = 1u32;
                let signal: Vec<f32> = (0..crate::plugin::SCOPE)
                    .map(|n| {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
                        let t = n as f32 / 48_000.;
                        [(110., 0.3), (220., 0.2), (330., 0.12), (440., 0.1), (880., 0.05), (1760., 0.02), (3520., 0.008)]
                            .iter()
                            .map(|(hz, a)| a * (std::f32::consts::TAU * hz * t).sin())
                            .sum::<f32>()
                            + 0.004 * noise
                    })
                    .collect();
                p.shared.scope.push(&signal);
            }
            if state.starts_with("mixer") {
                p.shared.meters.clips.parts[1].store(true, Ordering::Relaxed);
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
            if state == "unselected" {
                h.tick(Input { keys: vec![KeyPress { key: Key::Escape, mods: Mods::default() }], ..Default::default() });
            }
            if state == "sound-typing" {
                let at = center(&h.ui, "edit-envelope-value-Release");
                for down in [true, false, true, false] {
                    h.tick(pointer(at, down));
                }
            }
            // The spectrum eases in over a few of its looks.
            if state.starts_with("mixer") || state.starts_with("sound") {
                for _ in 0..10 {
                    std::thread::sleep(Duration::from_millis(35));
                    h.idle(1);
                }
            }
            h.settle_art();
            // Springs (the browser drawer) come to rest.
            h.idle(30);
            if state == "sticky" {
                let at = center(&h.ui, "rack-view");
                h.tick(Input { wheel: Vec2::new(0., 600.), ..pointer(at, false) });
                h.idle(60);
            }
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
                visible.extend([
                    "tab-info",
                    match state {
                        "mixer" => "master-strip",
                        // Wide strips scroll at 900: the toolbar stays.
                        "mixer-wide" => "mix-wide",
                        // Scrolled away, its header is stuck at the top.
                        "sticky" => "header-0",
                        _ => "header-0",
                    },
                ]);
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

/// A rack of one part running `script`, its performance view read.
fn scripted_part(script: &str) -> Arc<SamplerParams> {
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
    p
}

/// Control `n`'s value in part 0's performance view.
fn control_value(p: &SamplerParams, n: usize) -> crate::ksp::Value {
    let view = p.shared.view.lock().unwrap();
    view.parts[0].interface.as_ref().unwrap().controls[n].properties["$CONTROL_PAR_VALUE"].clone()
}

/// Wide switches stacked at one pitch, one set, read as a list: each row
/// named by its label, with its on/off as a check box and its keyswitch; a
/// row the script hid below the fold comes back.
#[test]
fn an_articulation_list_picks_one_and_turns_rows_on() {
    let p = scripted_part("on init\nmake_perfview\nset_ui_height_px(200)\ndeclare ui_switch $art0\nmove_control_px($art0, 10, 50)\nset_control_par(get_ui_id($art0), $CONTROL_PAR_WIDTH, 100)\nset_control_par(get_ui_id($art0), $CONTROL_PAR_HEIGHT, 20)\ndeclare ui_switch $art1\nmove_control_px($art1, 10, 72)\nset_control_par(get_ui_id($art1), $CONTROL_PAR_WIDTH, 100)\nset_control_par(get_ui_id($art1), $CONTROL_PAR_HEIGHT, 20)\ndeclare ui_switch $art2\nmove_control_px($art2, 10, 94)\nset_control_par(get_ui_id($art2), $CONTROL_PAR_WIDTH, 100)\nset_control_par(get_ui_id($art2), $CONTROL_PAR_HEIGHT, 20)\ndeclare ui_switch $art3\nmove_control_px($art3, 10, 116)\nset_control_par(get_ui_id($art3), $CONTROL_PAR_WIDTH, 100)\nset_control_par(get_ui_id($art3), $CONTROL_PAR_HEIGHT, 20)\ndeclare ui_label $name0(1,1)\nset_text($name0, \"Sustain\")\nmove_control_px($name0, 10, 50)\nset_control_par(get_ui_id($name0), $CONTROL_PAR_WIDTH, 110)\nset_control_par(get_ui_id($name0), $CONTROL_PAR_HEIGHT, 20)\ndeclare ui_label $name1(1,1)\nset_text($name1, \"Staccato\")\nmove_control_px($name1, 10, 72)\nset_control_par(get_ui_id($name1), $CONTROL_PAR_WIDTH, 110)\nset_control_par(get_ui_id($name1), $CONTROL_PAR_HEIGHT, 20)\ndeclare ui_label $name2(1,1)\nset_text($name2, \"Pizzicato\")\nmove_control_px($name2, 10, 94)\nset_control_par(get_ui_id($name2), $CONTROL_PAR_WIDTH, 110)\nset_control_par(get_ui_id($name2), $CONTROL_PAR_HEIGHT, 20)\ndeclare ui_label $name3(1,1)\nset_text($name3, \"Tremolo\")\nmove_control_px($name3, 10, 116)\nset_control_par(get_ui_id($name3), $CONTROL_PAR_WIDTH, 110)\nset_control_par(get_ui_id($name3), $CONTROL_PAR_HEIGHT, 20)\ndeclare ui_switch $onoff0\nmove_control_px($onoff0, 10, 50)\nset_control_par(get_ui_id($onoff0), $CONTROL_PAR_WIDTH, 18)\nset_control_par(get_ui_id($onoff0), $CONTROL_PAR_HEIGHT, 18)\ndeclare ui_switch $onoff1\nmove_control_px($onoff1, 10, 72)\nset_control_par(get_ui_id($onoff1), $CONTROL_PAR_WIDTH, 18)\nset_control_par(get_ui_id($onoff1), $CONTROL_PAR_HEIGHT, 18)\ndeclare ui_switch $onoff2\nmove_control_px($onoff2, 10, 94)\nset_control_par(get_ui_id($onoff2), $CONTROL_PAR_WIDTH, 18)\nset_control_par(get_ui_id($onoff2), $CONTROL_PAR_HEIGHT, 18)\ndeclare ui_switch $onoff3\nmove_control_px($onoff3, 10, 116)\nset_control_par(get_ui_id($onoff3), $CONTROL_PAR_WIDTH, 18)\nset_control_par(get_ui_id($onoff3), $CONTROL_PAR_HEIGHT, 18)\ndeclare ui_label $key0(1,1)\nset_text($key0, \"C#-0\")\nmove_control_px($key0, 90, 52)\ndeclare ui_label $key1(1,1)\nset_text($key1, \"C#-1\")\nmove_control_px($key1, 90, 74)\ndeclare ui_label $key2(1,1)\nset_text($key2, \"C#-2\")\nmove_control_px($key2, 90, 96)\ndeclare ui_label $key3(1,1)\nset_text($key3, \"C#-3\")\nmove_control_px($key3, 90, 118)\n$art0 := 1\nset_control_par(get_ui_id($art3), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)\nset_control_par(get_ui_id($name3), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)\nset_control_par(get_ui_id($onoff3), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)\nset_control_par(get_ui_id($key3), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)\nend on");
    let mut h = Harness::new(&p, 1180., 760.);
    let scene = h.ui.scene().unwrap();
    for n in [0, 1, 2, 3] {
        assert!(scene.surface(&format!("ksp-0-{n}")).is_some(), "row {n} is shown");
        assert!(scene.surface(&format!("ksp-0-{}", n + 8)).is_some(), "row {n}'s check box");
    }
    // The labels and keyswitches are part of the rows, not controls.
    assert!(scene.surface("ksp-0-4").is_none() && scene.surface("ksp-0-12").is_none());
    h.press("ksp-0-2");
    assert_eq!(control_value(&p, 2), crate::ksp::Value::Int(1), "a row picks its choice");
    let check = center(&h.ui, "ksp-0-9");
    h.tick(pointer(check, true));
    h.tick(pointer(check, false));
    h.idle(3);
    assert_eq!(control_value(&p, 9), crate::ksp::Value::Int(1), "a check box turns its row on");
    assert_eq!(control_value(&p, 1), crate::ksp::Value::Int(0), "and does not pick the row");
}

#[test]
fn performance_controls_edit_the_script() {
    let script = "on init\nmake_perfview\nset_ui_height_px(200)\ndeclare ui_switch $legato\nset_text($legato, \"Legato\")\nmove_control_px($legato, 10, 10)\ndeclare ui_knob $vibrato(0, 100, 1)\nmove_control_px($vibrato, 200, 10)\ndeclare ui_menu $mic\nadd_menu_item($mic, \"Close\", 0)\nadd_menu_item($mic, \"Room\", 1)\nmove_control_px($mic, 400, 10)\nend on";
    let p = scripted_part(script);
    let value = |n: usize| control_value(&p, n);
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

/// The editor opens and paints while a big multi loads: nothing it builds
/// waits on the loader. Progress only rises. Needs the libraries:
/// `cargo test --release --lib editor_opens_while_parts_load -- --ignored --nocapture`
#[test]
#[ignore]
fn editor_opens_while_parts_load() {
    use crate::plugin::Load;
    use moose::prelude::BackgroundTask;
    let root = Path::new(import::LIBRARY_ROOT);
    let paths = [
        "Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki",
        "Afflatus Chapter II Brass/Instruments/4. Experimental/Mega Brass.nki",
        "Audio Imperia CHORUS/Instruments/01 Multi Patches/01 Chorus - Women - Traditional Articulations.nki",
    ];
    let p = Arc::new(SamplerParams::new());
    p.shared.view.lock().unwrap().root = import::LIBRARY_ROOT.into();
    p.selection.write().unwrap().parts = (paths.iter())
        .map(|path| Part {
            path: root.join(path).to_string_lossy().into(),
            ..Default::default()
        })
        .collect();
    let started = Instant::now();
    let loader = {
        let p = p.clone();
        std::thread::spawn(move || Load.run(&p))
    };
    std::thread::sleep(Duration::from_millis(300));
    let opened = Instant::now();
    let mut h = Harness::new(&p, 1180., 760.);
    let first = opened.elapsed();
    let (mut worst, mut frames, mut last) = (Duration::ZERO, 0, [0u32; 3]);
    while !loader.is_finished() {
        let t = Instant::now();
        h.idle(1);
        (worst, frames) = (worst.max(t.elapsed()), frames + 1);
        for (slot, last) in last.iter_mut().enumerate() {
            let now = p.shared.load_progress[slot].load(Ordering::Relaxed);
            assert!(now >= *last, "slot {slot} progress fell from {last} to {now}");
            *last = now;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    loader.join().unwrap();
    println!(
        "opened mid-load in {:.1} ms (3 frames); {frames} frames while loading, worst {:.1} ms; loads took {:.1} s",
        first.as_secs_f64() * 1e3,
        worst.as_secs_f64() * 1e3,
        started.elapsed().as_secs_f64()
    );
    assert!(first < Duration::from_millis(500) && worst < Duration::from_millis(250));
}

/// Dragging the envelope's release handle left shortens the release as an
/// override on the part (the instrument untouched); the panel's reset
/// plays the library's again.
#[test]
fn sound_tab_drags_an_envelope_handle_into_the_override_layer() {
    use crate::engine::overrides::Param;
    let p = scripted_part("on init\nend on");
    let env = crate::modulation::Ahdsr {
        attack_curve: 0.0,
        attack_ms: 10.0,
        decay_ms: 500.0,
        hold_ms: 0.0,
        release_ms: 1000.0,
        sustain: 0.5,
        unknown_flag: 0,
        unknown_tail: Vec::new(),
    };
    {
        let mut view = p.shared.view.lock().unwrap();
        let i = view.parts[0].instrument.as_mut().unwrap();
        Arc::get_mut(i).unwrap().groups = vec![import::Group { volume_env: Some(env), ..Default::default() }];
    }
    let mut h = Harness::new(&p, 1180., 760.);
    h.press("tab-sound");
    let frame = h.ui.scene().unwrap().surface("edit-envelope").expect("the envelope graph").frame;
    // The release handle: the envelope's end, at the floor, inset by SPACE.
    let at = Point::new(frame.x + frame.size.width - SPACE, frame.y + frame.size.height - SPACE);
    for (x, down) in [(0., true), (-10., true), (-60., true), (-60., false)] {
        h.tick(pointer(Point::new(at.x + x, at.y), down));
    }
    h.idle(2);
    let offset = p.selection.read().unwrap().parts[0].edits.get(None, Param::Release);
    assert!(offset < -0.01, "release moved down: {offset}");
    let library = p.shared.view.lock().unwrap().parts[0].instrument.as_ref().unwrap().groups[0].volume_env.as_ref().unwrap().release_ms;
    assert_eq!(library, 1000.0, "the instrument is untouched");
    h.press("edit-envelope-reset");
    assert!(p.selection.read().unwrap().parts[0].edits.0.is_empty(), "reset plays the library's");

    // A value double-clicked takes one typed in.
    h.idle(60);
    let at = center(&h.ui, "edit-envelope-value-Attack");
    for down in [true, false, true, false] {
        h.tick(pointer(at, down));
    }
    h.idle(2);
    assert!(h.ui.scene().unwrap().surface("edit-envelope-value-Attack-edit").is_some(), "a field to type in");
    h.tick(Input { keys: vec![KeyPress { key: Key::Char('a'), mods: Mods { ctrl: true, ..Default::default() } }], ..Default::default() });
    h.tick(Input { text: "250 ms".into(), ..Default::default() });
    h.tick(enter());
    h.idle(3);
    let edits = p.selection.read().unwrap().parts[0].edits.clone();
    let attack = Param::Attack.apply(0.01, edits.offset(0, Param::Attack));
    assert!((attack - 0.25).abs() < 0.01, "the attack plays {attack} s");
}

fn key_spot(ui: &Ui, note: u8) -> Point {
    let r = ui.scene().unwrap().surface(&format!("key-{note}")).unwrap().frame;
    Point::new(r.x + r.size.width / 2., r.y + r.size.height * 0.9)
}

fn keys_down(p: &SamplerParams) -> Vec<u8> {
    (0..128u8).filter(|&n| p.shared.played[n as usize].load(Ordering::Relaxed) > 0).collect()
}

/// The top edge of each of `notes`, where a key's LED shines, as painted.
fn key_tops(ui: &Ui, notes: std::ops::Range<u8>) -> Vec<[u8; 3]> {
    let pix = pixels(ui, 1180, 760);
    notes
        .map(|n| {
            let r = ui.scene().unwrap().surface(&format!("key-{n}")).unwrap().frame;
            let o = (((r.y + 3.) as usize) * 1180 + (r.x + r.size.width / 2.) as usize) * 4;
            [pix[o], pix[o + 1], pix[o + 2]]
        })
        .collect()
}

fn glows(c: [u8; 3]) -> bool {
    i32::from(c[0]) - i32::from(c[2]) > 30
}

/// A drag across the keys moves the sound and the light together, one note
/// at a time; letting go anywhere stops it, and a note nothing holds any
/// more is let go on the next frame.
#[test]
fn a_glide_lights_what_sounds_and_lets_go_anywhere() {
    let p = Arc::new(SamplerParams::new());
    let mut h = Harness::new(&p, 1180., 760.);
    let rest = key_tops(&h.ui, 60..61)[0];
    for note in [60, 64] {
        let at = key_spot(&h.ui, note);
        for _ in 0..40 {
            h.tick(pointer(at, true));
        }
    }
    assert_eq!(keys_down(&p), [64], "one note sounds: the one under the pointer");
    let tops = key_tops(&h.ui, 60..65);
    assert!(glows(tops[4]), "and it is the one lit");
    assert!(
        tops[0].iter().zip(rest).all(|(a, b)| a.abs_diff(b) <= 2),
        "the key the drag began on is at rest, not pressed: {:?} vs {rest:?}",
        tops[0]
    );
    let away = Point::new(600., 300.);
    for _ in 0..3 {
        h.tick(pointer(away, true));
    }
    assert_eq!(keys_down(&p), [64], "off the keys the last note holds");
    h.tick(pointer(away, false));
    h.idle(40);
    assert!(keys_down(&p).is_empty(), "let go off the keys, it stops");
    assert!(!key_tops(&h.ui, 48..84).into_iter().any(glows), "and nothing stays lit");
    let sent: Vec<_> = std::iter::from_fn(|| p.shared.keyboard.pop()).map(|(_, play)| play).collect();
    assert!(
        matches!(sent[..], [Play::Note(60, _), Play::Note(60, 0), Play::Note(64, _), Play::Note(64, 0)]),
        "{sent:?}"
    );

    // A release lost with the editor that held it (dropped mid-drag).
    p.shared.press_key(0, 70, 100);
    let _ = p.shared.keyboard.pop();
    h.idle(1);
    assert!(keys_down(&p).is_empty(), "a note nothing holds is let go");
    assert_eq!(p.shared.keyboard.pop(), Some((0, Play::Note(70, 0))));
    // The mouse and a computer key on one note: a note-off for each note-on.
    p.shared.press_key(0, 72, 100);
    p.shared.press_key(0, 72, 90);
    p.shared.release_key(72);
    p.shared.release_key(72);
    let sent: Vec<_> = std::iter::from_fn(|| p.shared.keyboard.pop()).map(|(_, play)| play).collect();
    assert_eq!(sent, [Play::Note(72, 100), Play::Note(72, 0), Play::Note(72, 90), Play::Note(72, 0)]);
}

/// The editor's window, headless: the real event queue and frame schedule
/// over the editor's build, cancelled as `editor()` cancels it.
struct Window {
    build: Build,
    bridge: Bridge<SamplerParams>,
    p: Arc<SamplerParams>,
    computer: Arc<computer::Computer>,
    watch: Watch,
}

impl moose::mui::mui::host::View for Window {
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        (self.build)(ui, &mut self.bridge)
    }
    fn changed(&mut self) -> bool {
        self.watch.changed(&self.p, &Meters::default(), &self.computer)
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn cancel(&mut self, _: &Ui) {
        let_go(&self.p, &self.computer);
    }
}

struct NoClipboard;
impl moose::mui::mui::Clipboard for NoClipboard {
    fn get(&mut self) -> Option<String> {
        None
    }
    fn set(&mut self, _: &str) {}
}

/// However a glide ends -- the button up outside the window, focus lost,
/// the window closed -- its note stops; and whatever arrives in whatever
/// order, never two keys sound at once.
#[test]
fn the_window_lets_go_of_a_glide_however_it_ends() {
    use moose::mui::mui::host::{Driver, Shared};
    let p = Arc::new(SamplerParams::new());
    let computer = Arc::<computer::Computer>::default();
    let mut s = Shared {
        ui: theme::ui(),
        view: Window {
            build: Box::new(build(&p, Arc::default(), computer.clone(), Arc::default(), Arc::default())),
            bridge: Bridge::new(p.clone()),
            p: p.clone(),
            computer,
            watch: Watch::default(),
        },
    };
    let now = std::cell::Cell::new(Instant::now());
    let tick = |d: &mut Driver, s: &mut Shared<Window>, n: usize| {
        for _ in 0..n {
            now.set(now.get() + Duration::from_millis(16));
            d.advance(s, now.get());
        }
    };
    let mut d = Driver::new((1180, 760), 1.0, Box::new(NoClipboard));
    tick(&mut d, &mut s, 3);
    let keys: Vec<Point> = (48..84).map(|n| key_spot(&s.ui, n)).collect();
    let mods = Mods::default();
    let glide = |d: &mut Driver, s: &mut Shared<Window>| {
        d.pointer_moved(keys[12], mods);
        d.button(Button::Primary, true, mods);
        tick(d, s, 2);
        d.pointer_moved(keys[16], mods);
        tick(d, s, 2);
        assert_eq!(keys_down(&s.view.p), [64]);
    };
    glide(&mut d, &mut s);
    d.pointer_left();
    d.button(Button::Primary, false, mods);
    glide(&mut d, &mut s);
    d.pointer_left();
    tick(&mut d, &mut s, 2);
    assert_eq!(keys_down(&p), [64], "off the window the last note holds");
    d.button(Button::Primary, false, mods);
    tick(&mut d, &mut s, 2);
    assert!(keys_down(&p).is_empty(), "the button up outside the window");
    glide(&mut d, &mut s);
    d.focus(false);
    tick(&mut d, &mut s, 1);
    assert!(keys_down(&p).is_empty(), "focus lost mid-glide");
    d.focus(true);
    d.button(Button::Primary, false, mods);
    glide(&mut d, &mut s);
    d.close(&mut s);
    assert!(keys_down(&p).is_empty(), "the window closed mid-glide, with no frame after");

    let mut d = Driver::new((1180, 760), 1.0, Box::new(NoClipboard));
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut down = false;
    for step in 0..3000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        match seed % 16 {
            0..=6 => d.pointer_moved(keys[(seed >> 8) as usize % keys.len()], mods),
            7..=9 => {
                down = !down;
                d.button(Button::Primary, down, mods);
            }
            10 => d.pointer_left(),
            11 => {
                d.focus(false);
                down = false;
            }
            12 => d.focus(true),
            _ => tick(&mut d, &mut s, 1),
        }
        assert!(keys_down(&p).len() <= 1, "step {step}: {:?}", keys_down(&p));
    }
    d.button(Button::Primary, false, mods);
    tick(&mut d, &mut s, 2);
    assert!(keys_down(&p).is_empty());
}

/// A clicked header marks its part and the keys play it alone; Esc, or a
/// click on the rack off every part, lets go, and the keys then play every
/// part as MIDI channel 1 would.
#[test]
fn the_keys_play_the_selected_part_or_every_part() {
    let p = two_parts();
    let mut h = Harness::new(&p, 1180., 760.);
    h.idle(3);
    let click = |h: &mut Harness, at: Point| {
        h.tick(pointer(at, true));
        h.tick(pointer(at, false));
        h.idle(2);
    };
    let play = |h: &mut Harness| {
        let at = key_spot(&h.ui, 60);
        h.tick(pointer(at, true));
        h.tick(pointer(at, true));
        h.tick(pointer(at, false));
        h.idle(2);
        let sent: Vec<_> = std::iter::from_fn(|| p.shared.keyboard.pop()).collect();
        sent.first().map(|(slot, _)| *slot)
    };
    let header = h.ui.scene().unwrap().surface("header-1").unwrap().frame;
    click(&mut h, Point::new(header.x + header.size.width * 0.3, header.y + 4.));
    assert_eq!(selected_slot(&p), 1);
    assert_eq!(play(&mut h), Some(1), "the selected part plays");

    h.tick(Input { keys: vec![KeyPress { key: Key::Escape, mods: Mods::default() }], ..Default::default() });
    h.idle(2);
    assert_eq!(selected_slot(&p), crate::plugin::EVERY_PART, "Esc lets go");
    assert!(h.ui.scene().unwrap().surface("part-ranges").is_some(), "the keys show each part's range");
    assert_eq!(play(&mut h), Some(crate::plugin::EVERY_PART), "and every part plays");

    let name = center(&h.ui, "name-0");
    click(&mut h, name);
    assert_eq!(selected_slot(&p), 0);
    let body = center(&h.ui, "body-1");
    click(&mut h, body);
    assert_eq!(selected_slot(&p), 1, "a click on a part's controls' ground selects it");
    // Below the rack's foot there is only the rack.
    let foot = h.ui.scene().unwrap().surface("rack-drop").unwrap().frame;
    click(&mut h, Point::new(foot.x + 40., foot.y + foot.size.height + 40.));
    assert_eq!(selected_slot(&p), crate::plugin::EVERY_PART, "a click on the empty rack lets go");
}
