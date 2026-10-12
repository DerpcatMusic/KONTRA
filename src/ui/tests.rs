use super::*;
use crate::artwork;
use crate::plugin::Play;

/// Where the owner's libraries are, for the ignored tests that need them.
const LIBRARY_ROOT: &str = "/path/to/Kontakt-Libraries";

/// The presets a scan of `root` finds.
fn presets(root: &Path) -> Vec<PathBuf> {
    shelved(root).1
}

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

pub(super) struct Harness {
    pub(super) ui: Ui,
    build: Build,
    bridge: Bridge<SamplerParams>,
    size: Size,
    computer: Arc<computer::Computer>,
    art: Arc<art::Art>,
}

impl Harness {
    pub(super) fn new(p: &Arc<SamplerParams>, width: f64, height: f64) -> Self {
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

    pub(super) fn idle(&mut self, frames: usize) {
        for _ in 0..frames {
            self.tick(Input::default());
        }
    }

    /// Focus `id` and press Enter, then let the result settle.
    pub(super) fn press(&mut self, id: &str) {
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
    assert_eq!(p.shared.keyboard.pop(), Some((crate::plugin::EVERY_PART, Play::Note(60, 100))), "nothing focused, every part plays");
    assert!(send(&mut h, key(1, "a", true)), "a repeat is swallowed");
    assert!(p.shared.keyboard.pop().is_none(), "and plays nothing");
    assert!(send(&mut h, key(2, "x", true)), "X steps the octave");
    assert!(send(&mut h, key(3, "V", true)), "V the velocity");
    assert!(send(&mut h, key(4, "k", true)));
    assert_eq!(p.shared.keyboard.pop(), Some((crate::plugin::EVERY_PART, Play::Note(84, 120))));
    assert!(send(&mut h, key(1, "a", false)));
    assert_eq!(p.shared.keyboard.pop(), Some((crate::plugin::EVERY_PART, Play::Note(60, 0))), "the note its key started stops");
    assert!(!send(&mut h, key(2, "x", false)), "ups of other keys pass");
    h.ui.focus("search");
    h.idle(2);
    assert!(!send(&mut h, key(5, "s", true)), "a text field keeps its typing");
    h.press("qwerty");
    assert_eq!(p.shared.keyboard.pop(), Some((crate::plugin::EVERY_PART, Play::Note(84, 0))), "switching off lets go");
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
    assert_eq!(p.shared.keyboard.pop(), Some((crate::plugin::EVERY_PART, Play::Note(60, 0))), "closing lets the note go");
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
    for width in [900., 1180., 1920.] {
        let p = Arc::new(SamplerParams::new());
        p.selection.write().unwrap().parts.push(Part { path: "/virtual/Library/Solo.nki".into(), ..Default::default() });
        {
            let mut selection = write(&p.selection);
            let part = &mut selection.parts[0];
            part.name = "Analog Strings · Extended performance instrument".into();
            part.channel = 15;
            part.port = 3;
            part.output = 3;
            part.output_manual = true;
        }
        {
            let mut view = lock(&p.shared.view);
            view.files = Arc::new(vec![
                "/virtual/Library/Alpha.nki".into(), "/virtual/Library/Solo.nki".into(), "/virtual/Library/Zeta.nki".into(),
            ]);
        }
        let mut h = Harness::new(&p, width, 600.);
        h.idle(2);
        let scene = h.ui.scene().unwrap();
        let frame = |id: &str| scene.surface(id).unwrap_or_else(|| panic!("missing header control {id} at window width {width}")).frame;
        let (header, name, port) = (frame("header-0"), frame("name-0"), frame("midi-0"));
        assert!((header.size.height - (rack::SLIM - 1.)).abs() < 0.5, "at {width}: {header:?}");
        assert!(name.size.width >= 120. - 0.5,"the title remains meaningful at {width}: {name:?}, {header:?}");
        assert!(port.y < name.y + name.size.height && name.y < port.y + port.size.height, "at {width}");
        for id in ["name-0","collapse-0","preset-prev-0","preset-next-0","midi-0","output-0","pan-0","volume-0","tune-0","solo-0","mute-0","more-0"] {
            let f = frame(id);
            assert!(f.x >= header.x - 0.5 && f.x + f.size.width <= header.x + header.size.width + 0.5,
                "{id} remains inside the header at {width}: {f:?}, {header:?}");
        }
        if let Some(remove) = scene.surface("remove-0") {
            assert!(remove.frame.x + remove.frame.size.width <= header.x + header.size.width + 0.5,"at {width}");
        }
        h.press("midi-0");
        h.press("menu-item-1");
        assert_eq!(read(&p.selection).parts[0].channel,0,"compact MIDI retains its routing menu");
        h.press("output-0");
        h.press("menu-item-3");
        assert_eq!(read(&p.selection).parts[0].output,1,"compact output retains its routing menu");
        h.press("mute-0");
        assert!(read(&p.selection).parts[0].mute,"mix controls remain interactive");
        h.press("preset-next-0");
        assert_eq!(read(&p.selection).parts[0].path,"/virtual/Library/Zeta.nki","preset navigation remains interactive");
        h.press("more-0");
        let last = (0..64).filter(|n| h.ui.scene().unwrap().surface(&format!("menu-item-{n}")).is_some()).next_back().unwrap();
        h.press(&format!("menu-item-{last}"));
        assert!(read(&p.selection).parts[0].path.is_empty(),"Remove remains reachable from the Part menu at {width}");
    }
}

/// A hundred libraries of a hundred presets each, in folders. The filter
/// (Ctrl+F) narrows the libraries and opens one from the keys; a library
/// shows its folders, which the arrows fold and walk; Enter loads into the
/// selected part; libraries reorder by dragging; and a search over all of
/// them builds only the rows in view, yet brings the cursor into it.
#[test]
fn the_browser_finds_by_library_and_folder() {
    let mut files: Vec<PathBuf> = (0..100)
        .flat_map(|lib| (0..100).map(move |n| format!("/virtual/Lib {lib:03}/Instruments/{} Part/Patch {n:03}.nki", n / 25).into()))
        .collect();
    files.sort();
    let p = Arc::new(SamplerParams::new());
    {
        let mut v = p.shared.view.lock().unwrap();
        v.shelf = Arc::new(crate::library::Shelf::under("/virtual", &files));
        v.files = Arc::new(files);
    }
    let mut h = Harness::new(&p, 1180., 760.);
    let key = |key: Key, mods: Mods| Input { keys: vec![KeyPress { key, mods }], ..Default::default() };
    let tap = |h: &mut Harness, k: Key| {
        h.tick(key(k, Mods::default()));
        h.idle(2);
    };
    let shown = |h: &Harness, id: &str| h.ui.scene().unwrap().surface(id).is_some();

    // Shut, the browser opens on Ctrl+F.
    h.press("toggle-browser");
    h.idle(30);
    assert!(!shown(&h, "library-filter"), "the browser shuts");
    h.tick(key(Key::Char('f'), Mods { ctrl: true, ..Mods::default() }));
    h.idle(30);
    assert_eq!(h.ui.focus_key(), Some("library-filter"), "Ctrl+F opens the browser on the library filter");
    h.tick(Input { text: "lib 042".into(), ..Default::default() });
    h.idle(2);
    assert!(shown(&h, "library-42") && !shown(&h, "library-41"), "it narrows the libraries");
    tap(&mut h, Key::Enter);
    assert_eq!(h.ui.focus_key(), Some("folder-0"), "Enter opens the match");
    // Instruments, all the library holds, starts open over its four folders.
    assert!(shown(&h, "folder-4") && !shown(&h, "instrument-5"));
    tap(&mut h, Key::Down);
    tap(&mut h, Key::Right);
    assert!(shown(&h, "instrument-2"), "Right opens a folder");
    assert_eq!(p.shared.libraries.settings().folders.get("/virtual/Lib 042/Instruments/0 Part"), Some(&true));
    tap(&mut h, Key::Right);
    assert_eq!(h.ui.focus_key(), Some("instrument-2"), "and steps into it");
    tap(&mut h, Key::Left);
    assert_eq!(h.ui.focus_key(), Some("folder-1"), "Left steps back to its folder");
    tap(&mut h, Key::Left);
    assert!(!shown(&h, "instrument-2"), "and shuts it");
    tap(&mut h, Key::Right);
    tap(&mut h, Key::Right);
    tap(&mut h, Key::Enter);
    let path = |p: &SamplerParams| p.selection.read().unwrap().parts.iter().map(|p| p.path.clone()).collect::<Vec<_>>();
    assert_eq!(path(&p), ["/virtual/Lib 042/Instruments/0 Part/Patch 000.nki"]);
    tap(&mut h, Key::Down);
    tap(&mut h, Key::Enter);
    assert_eq!(path(&p), ["/virtual/Lib 042/Instruments/0 Part/Patch 001.nki"], "Enter loads into the selected part");
    let settings = p.shared.libraries.settings();
    assert_eq!((settings.last_library.as_str(), settings.used.len()), ("/virtual/Lib 042", 1), "the place is kept");

    // Esc clears the filter; a library dragged onto another goes before it.
    h.ui.focus("library-filter");
    tap(&mut h, Key::Escape);
    // The list glides back to the chosen library: let it land first.
    h.idle(30);
    h.drag("library-42", "library-41");
    let settings = p.shared.libraries.settings();
    assert_eq!(settings.sort, crate::library::Sort::Custom);
    assert_eq!(settings.order[40..43], ["/virtual/Lib 040", "/virtual/Lib 042", "/virtual/Lib 041"]);

    // Every library searched (a second click on the chosen one lets it go):
    // ten thousand matches, a screenful built.
    let at = center(&h.ui, "library-41");
    for down in [true, false] {
        h.tick(pointer(at, down));
    }
    h.idle(2);
    assert!(!shown(&h, "folder-0"), "no library chosen");
    h.ui.focus("search");
    let start = Instant::now();
    h.tick(Input { text: "patch".into(), ..Default::default() });
    eprintln!("search over 10k presets: {:?}", start.elapsed());
    h.idle(2);
    let built = |h: &Harness| (0..10_000).filter(|n| shown(h, &format!("instrument-{n}"))).collect::<Vec<_>>();
    assert!((3..60).contains(&built(&h).len()), "{} rows built", built(&h).len());
    for _ in 0..40 {
        h.tick(key(Key::PageDown, Mods::default()));
    }
    h.idle(2);
    let cursor = built(&h);
    assert!(cursor.first().is_some_and(|&n| n > 200), "the cursor's row is brought into view: {cursor:?}");
}

#[test]
fn empty_rack_canvas_appends_and_scrolls_beyond_the_add_button() {
    let p = Arc::new(SamplerParams::new());
    {
        let mut view = p.shared.view.lock().unwrap();
        view.files = Arc::new(vec!["/virtual/Library/Piano.nki".into(), "/virtual/Library/Strings.nki".into()]);
        view.shelf = Arc::new(crate::library::Shelf::under("/virtual", &view.files));
    }
    let mut h = Harness::new(&p, 1180., 760.);
    h.press("library-0");
    h.drag("instrument-0", "rack-welcome");
    assert_eq!(p.selection.read().unwrap().order, [0], "the empty welcome canvas accepts a browser drop");
    p.selection.write().unwrap().parts[0].collapsed = true;
    h.idle(30);
    h.drag("instrument-1", "rack-empty");
    {
        let selection = p.selection.read().unwrap();
        assert_eq!(selection.order, [0, 1], "a drop beyond Add appends a new part");
        assert!(selection.parts[0].path.ends_with("Piano.nki"));
        assert!(selection.parts[1].path.ends_with("Strings.nki"));
    }
    p.selection.write().unwrap().parts[1].collapsed = true;
    h.idle(30);
    let at = center(&h.ui, "rack-empty");
    for down in [true, false] { h.tick(pointer(at, down)); }
    h.idle(2);
    assert_eq!(p.selection.read().unwrap().order.len(), 2, "clicking empty canvas never adds an instrument");
    let before = h.ui.scene().unwrap().surface("rack-drop").unwrap().frame;
    h.tick(Input { wheel: Vec2::new(0., 10_000.), ..pointer(at, false) });
    h.idle(60);
    let scene = h.ui.scene().unwrap();
    let footer = scene.surface("rack-drop").unwrap().frame;
    let viewport = scene.surface("rack-view").unwrap().frame;
    let empty = scene.surface("rack-empty").unwrap().frame;
    assert!(before.y - footer.y > before.size.height, "the rack scrolls further than the Add footer: {before:?} → {footer:?}");
    assert!(footer.y + footer.size.height <= viewport.y + 1., "Add can scroll completely above the empty viewport: {footer:?}, {viewport:?}");
    assert!(empty.size.height >= viewport.size.height - 1., "a full viewport of empty canvas remains: {empty:?}");
    h.tick(Input { wheel: Vec2::new(0., -10_000.), ..pointer(center(&h.ui, "rack-view"), false) });
    h.idle(60);
    h.drag("instrument-0", "header-1");
    {
        let selection = p.selection.read().unwrap();
        assert_eq!(selection.order.len(), 2, "an explicit header drop still replaces");
        assert!(selection.parts[1].path.ends_with("Piano.nki"));
    }
    h.drag("name-1", "header-0");
    assert_eq!(p.selection.read().unwrap().order, [1, 0], "explicit header reordering remains intact");
}

#[test]
fn rack_interactions() {
    let p = Arc::new(SamplerParams::new());
    {
        let mut v = p.shared.view.lock().unwrap();
        v.files = Arc::new(vec![
            "/virtual/Library/Piano.nki".into(),
            "/virtual/Library/Strings.nki".into(),
            "/virtual/Library/Ensemble.kontra-multi".into(),
        ]);
        v.shelf = Arc::new(crate::library::Shelf::under("/virtual", &v.files));
    }
    let mut h = Harness::new(&p, 1180., 760.);
    let parts = |p: &SamplerParams| p.selection.read().unwrap().parts.clone();

    h.press("library-0");
    h.drag("instrument-0", "rack-drop");
    assert_eq!(parts(&p).len(), 1, "a preset dropped on the rack is added");
    h.ui.focus("instrument-1");
    h.tick(Input {
        keys: vec![KeyPress { key: Key::Enter, mods: Mods { shift: true, ..Mods::default() } }],
        ..Default::default()
    });
    h.idle(3);
    assert_eq!(parts(&p).len(), 2, "Shift+Enter on a preset adds it");
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
            .ends_with("Ensemble.kontra-multi")
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
    // The routing menus: output st.4 (after Automatic and a rule), channel 2,
    // then port B (after a rule and a heading).
    for (menu, item) in [("output-0", 5), ("midi-0", 2), ("midi-0", 20)] {
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
    p.shared.press_key(1, 61, 1);
    assert_eq!(p.shared.keyboard.pop(), Some((1, Play::Note(61, 1))));
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
    assert!(native_files(&p, &Default::default(), ui, at, &files, false));
    assert!(parts(&p)[0].path.is_empty(), "hovering does not load");
    assert!(native_files(&p, &Default::default(), ui, at, &files, true));
    assert!(
        parts(&p)[0].path.ends_with("Native.nki"),
        "a dropped file takes the free slot"
    );
    assert!(!native_files(&p, &Default::default(), ui, at, &[PathBuf::from("notes.txt")], true));
    let before_append = parts(&p).len();
    assert!(native_files(
        &p,
        &Default::default(),
        ui,
        at,
        &vec![PathBuf::from("full.nki"); 17],
        true
    ));
    assert_eq!(parts(&p).len(), before_append + 17, "a native drop grows the rack rather than rejecting files");
    let at = center(ui, "header-1");
    assert!(native_files(&p, &Default::default(), ui, at, &files, true));
    assert!(
        parts(&p)[1].path.ends_with("Native.nki"),
        "a file dropped on a header replaces its part"
    );
    let multi = [PathBuf::from("/external/Multi.kontra-multi")];
    let before = p.selection.read().unwrap().clone();
    assert!(native_files(&p, &Default::default(), ui, at, &multi, false));
    assert!(p.shared.multi_request.lock().unwrap().is_none());
    assert!(native_files(&p, &Default::default(), ui, at, &multi, true));
    assert_eq!(
        p.shared.multi_request.lock().unwrap().take().unwrap(),
        "/external/Multi.kontra-multi"
    );
    assert!(*p.selection.read().unwrap() == before);
}

#[test]
fn favorites_star_from_the_row_or_the_menu_and_lead_the_browser() {
    let p = Arc::new(SamplerParams::new());
    {
        let mut v = p.shared.view.lock().unwrap();
        v.files = Arc::new(vec![
            "/virtual/Library/Piano.nki".into(),
            "/virtual/Library/Strings.nki".into(),
        ]);
        v.shelf = Arc::new(crate::library::Shelf::under("/virtual", &v.files));
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
        v.files = Arc::new(vec![
            "/virtual/Keys/Grand Piano.nki".into(),
            "/virtual/Keys/Organ.nki".into(),
            "/virtual/Toys/Toy Piano.nki".into(),
        ]);
        v.shelf = Arc::new(crate::library::Shelf::under("/virtual", &v.files));
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
        if id == "browser-split" {
            assert!((center(&h.ui, id).y - at.y - dy).abs() < 0.5,
                "the divider follows the pointer within its unconstrained range");
        }
    }
    let s = read(&p.selection).clone();
    assert!(s.browser_split as f64 > browser::SPLIT, "{}", s.browser_split);
    assert!(s.browser_width as f64 > SIDEBAR, "{}", s.browser_width);
}

#[test]
fn split_browser_keeps_both_panes_usable_across_resize_and_scale() {
    for split in [browser::SPLIT_MAX, browser::SPLIT_MIN, browser::SPLIT] {
        for scale in [1., 1.5, 2.] {
            let p = Arc::new(SamplerParams::new());
            p.selection.write().unwrap().browser_split = split as f32;
            {
                let mut view = p.shared.view.lock().unwrap();
                view.files = Arc::new((0..100).map(|n|
                    PathBuf::from(format!("/virtual/Keys/Patch {n:03}.nki"))
                ).collect());
                view.shelf = Arc::new(crate::library::Shelf::under("/virtual", &view.files));
            }
            let mut h = Harness::new(&p, 900., 600.);
            h.ui.set_scale(Some(scale));
            h.press("library-0");
            for size in [Size::new(900., 600.), Size::new(1180., 760.), Size::new(1600., 900.), Size::new(900., 600.)] {
                h.size = size;
                h.idle(3);
                let scene = h.ui.scene().unwrap();
                let frame = |id| scene.surface(id).unwrap_or_else(|| panic!("missing {id}")).frame;
                let (browser, sources, presets, filter, search, divider) = (
                    frame("browser"), frame("browser-sources-false"), frame("browser-list"),
                    frame("library-filter"), frame("search"), frame("browser-split"),
                );
                assert!(presets.size.height >= TEXT * 6. + 8. - 0.5,
                    "two tall preset rows must fit: split={split} scale={scale} size={size:?} presets={presets:?}");
                assert!(sources.size.height >= browser::THUMB.1 + 2. * TIGHT - 0.5,
                    "one library row must fit: split={split} scale={scale} sources={sources:?}");
                assert!(filter.y + filter.size.height <= sources.y + 0.5);
                assert!(sources.y + sources.size.height <= divider.y + 0.5);
                assert!(divider.y + divider.size.height <= search.y + 0.5);
                assert!(search.y + search.size.height <= presets.y + 0.5);
                assert!(presets.y + presets.size.height <= browser.y + browser.size.height + 0.5,
                    "the preset pane stays inside the browser: split={split} scale={scale} size={size:?} browser={browser:?} sources={sources:?} presets={presets:?} search={search:?}");
                assert!((presets.y + presets.size.height - browser.y - browser.size.height).abs() < 0.5,
                    "constrained pane fractions consume the available height");
                assert_eq!(read(&p.selection).browser_split, split as f32, "resize preserves the saved split");
                let start = if scene.surface("instrument-99").is_some() { "instrument-99" } else { "instrument-0" };
                h.ui.focus(start);
                h.tick(Input { keys: vec![KeyPress { key: Key::Home, mods: Mods::default() }], ..Default::default() });
                h.idle(30);
                assert_eq!(h.ui.focus_key(), Some("instrument-0"), "Home: split={split} scale={scale} size={size:?}");
                h.tick(Input { keys: vec![KeyPress { key: Key::End, mods: Mods::default() }], ..Default::default() });
                h.idle(30);
                assert_eq!(h.ui.focus_key(), Some("instrument-99"), "End: split={split} scale={scale} size={size:?}");
                let scene = h.ui.scene().unwrap();
                let viewport = scene.surface("browser-list").unwrap().frame;
                let last = scene.surface("instrument-99").unwrap().frame;
                assert!(last.y >= viewport.y - 0.5 && last.y + last.size.height <= viewport.y + viewport.size.height + 0.5,
                    "End reveals the focused last row after resize");
            }
        }
    }
}

#[test]
fn save_multi_names_the_rack_and_writes_it_under_multis() {
    let root = std::env::temp_dir().join(format!("kontakto-save-{}", std::process::id()));
    let p = Arc::new(SamplerParams::new());
    p.shared.libraries.add_root(&root, false);
    {
        let mut s = p.selection.write().unwrap();
        s.parts = vec![crate::plugin::Part {
            path: "/virtual/Keys/Piano.nki".into(),
            tune: -2.,
            ..Default::default()
        }];
        s.order = vec![0];
    }
    let mut h = Harness::new(&p, 1180., 760.);
    h.press("app-menu");
    h.press("menu-item-8");
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
        v.files = Arc::new(vec![
            "/virtual/Keys/Grand Piano.nki".into(),
            "/virtual/Keys/Organ.nki".into(),
            "/virtual/Toys/Toy Piano.nki".into(),
        ]);
        v.shelf = Arc::new(crate::library::Shelf::under("/virtual", &v.files));
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

/// Large libraries retain a bounded widget count and keyboard reachability.
#[test]
fn a_large_library_browser_keeps_scrolling_and_searching() {
    let p = Arc::new(SamplerParams::new());
    {
        let mut v = p.shared.view.lock().unwrap();
        v.files = Arc::new(
            (0..1000)
                .map(|n| PathBuf::from(format!("/virtual/Large/Patch {n:04}.nki")))
                .collect(),
        );
        v.shelf = Arc::new(crate::library::Shelf::under("/virtual", &v.files));
    }
    let mut h = Harness::new(&p, 1180., 760.);
    h.press("library-0");
    h.idle(2);
    let shown = (0..1000)
        .filter(|n| {
            h.ui.scene()
                .unwrap()
                .surface(&format!("instrument-{n}"))
                .is_some()
        })
        .count();
    println!("mounted preset rows: {shown}");
    assert!(shown < 100, "the browser must not mount all 1000 rows");
    let before = h.ui.scene().unwrap().surface("instrument-5").unwrap().frame.y;
    let at = center(&h.ui, "browser-list");
    h.tick(Input {
        wheel: Vec2::new(0., 120.),
        ..pointer(at, false)
    });
    h.idle(30);
    let after = h.ui.scene().unwrap().surface("instrument-5").unwrap().frame.y;
    let viewport =
        h.ui.scene()
            .unwrap()
            .surface("browser-list")
            .unwrap()
            .frame;
    assert!(
        before - after >= 110.,
        "normalized wheel input moves the list"
    );
    h.ui.focus("instrument-5");
    h.tick(Input {
        keys: vec![KeyPress {
            key: Key::End,
            mods: Mods::default(),
        }],
        ..Default::default()
    });
    h.idle(30);
    assert_eq!(h.ui.focus_key(), Some("instrument-999"));
    let last =
        h.ui.scene()
            .unwrap()
            .surface("instrument-999")
            .unwrap()
            .frame;
    assert!(
        last.y >= viewport.y && last.y + last.size.height <= viewport.y + viewport.size.height + 1.,
        "End reveals the last row"
    );
    h.ui.focus("search");
    h.tick(Input {
        text: "0999".into(),
        ..Default::default()
    });
    h.idle(2);
    h.press("instrument-0");
    assert!(read(&p.selection).parts[0].path.ends_with("Patch 0999.nki"));
}


fn library_instruments(files: &[PathBuf], names: &str) -> Vec<PathBuf> {
    names
        .split(',')
        .filter_map(|name| files.iter().find(|p| p.file_stem().is_some_and(|n| n == name)))
        .cloned()
        .collect()
}


/// The libraries in the folder of libraries `root`, found as a scan finds
/// them, with the hues of their own pictures.
fn shelved(root: &Path) -> (crate::library::Shelf, Vec<PathBuf>) {
    let roots = [crate::library::Root { path: root.to_string_lossy().into(), single: false }];
    let (mut shelf, files) = crate::library::scan(&roots, &Default::default()).unwrap();
    for library in &mut shelf.libraries {
        library.hue = artwork::own_hue(&library.dir);
    }
    (shelf, files)
}

/// Libraries with no artwork or library file, as empty presets in a
/// temporary folder: they show generated covers.
fn cover_libraries() -> PathBuf {
    let root = std::env::temp_dir().join(format!("kontra-covers-{}", std::process::id()));
    for (folder, presets) in [
        ("Pacific Ensemble Strings/Instruments", 12),
        ("Hollow Sun/Kinder Piano 1.1/Instruments", 3),
        ("Cinematic_Brass_Ensembles_v2.0.3/Instruments", 8),
        ("Glass Harmonica [Soundiron]/Instruments", 4),
        ("Tape Choir (KONTAKT)", 6),
        ("Spitfire Audio/Olafur Arnalds Chamber Evolutions/Instruments", 20),
    ] {
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        for n in 0..presets {
            std::fs::write(dir.join(format!("Patch {n}.nki")), []).unwrap();
        }
    }
    root
}

/// A plugin whose rack holds `instruments` (when `loaded`), loaded as the
/// loader loads them. `state` stages the
/// screenshots' special cases.
fn racked(
    files: &[PathBuf],
    instruments: &[PathBuf],
    loaded: bool,
    state: &str,
) -> Arc<SamplerParams> {
    let p = Arc::new(SamplerParams::new());
    {
        let mut view = p.shared.view.lock().unwrap();
        let (shelf, files) = match state {
            "no-libraries" => (crate::library::Shelf::default(), Vec::new()),
            "covers" => shelved(&cover_libraries()),
            _ => (shelved(Path::new(LIBRARY_ROOT)).0, files.to_vec()),
        };
        view.artwork = artwork::scan(&shelf.libraries);
        view.shelf = Arc::new(shelf);
        view.files = Arc::new(files);
        view.scanned = p.shared.libraries.wanted();
        if loaded {
            p.selection.write().unwrap().parts =
                instruments.iter().map(|i| Part { path: i.to_string_lossy().into(), ..Default::default() }).collect();
        }
    }
    {
        use moose::prelude::BackgroundTask;
        crate::plugin::Load.run(&p);
        let mut view = p.shared.view.lock().unwrap();
        if state == "error" {
            view.parts[0].status = "Load failed: missing sample data in archive".into();
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
            let done = (f64::from(crate::sound::Progress::DONE.0) * done) as u32;
            p.shared.part(slot).unwrap().load_progress.store(done, Ordering::Relaxed);
        }
        view.parts[2].report = None;
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
    let files = presets(Path::new(LIBRARY_ROOT));
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

/// The lag audit: per scenario, a frame's build+layout p50/p99 and a CPU
/// raster's p50, in microseconds of this thread's CPU time, with the
/// owner's libraries. Run with
/// `--ignored --nocapture`; `KONTAKTO_SHOT` picks the 16-part rack.
#[test]
#[ignore]
#[cfg(target_os = "linux")]
fn lag() {
    use moose::mui::mui::vello::{
        self,
        vello_cpu::{Pixmap, RenderContext, Resources},
    };
    // This thread's time on a CPU, so a loaded machine does not count.
    let cpu_us = || {
        let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: a valid out-pointer for one call.
        unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
        t.tv_sec as f64 * 1e6 + t.tv_nsec as f64 / 1e3
    };
    let files = presets(Path::new(LIBRARY_ROOT));
    let chosen = std::env::var("KONTAKTO_SHOT").unwrap_or_else(|_| {
        "03 Areia - 6 Celli - Core Techniques,Vista - 3 Cellos,Una Corda Pure,ANALOG STRINGS".into()
    });
    let library = library_instruments(&files, &chosen);
    let racks: Vec<(&str, Vec<PathBuf>)> = vec![
        ("16 parts", library.iter().cycle().take(16).cloned().collect()),
        ("ANALOG", library_instruments(&files, "ANALOG STRINGS")),
        ("Areia", library_instruments(&files, "03 Areia - 6 Celli - Core Techniques")),
    ];
    let (w, hgt) = (1600u16, 1000u16);
    let mut raster = RenderContext::new(w, hgt);
    let mut resources = Resources::default();
    let mut cache = vello::Cache::default();
    let mut pix = Pixmap::new(w, hgt);
    for (name, instruments) in racks {
        let p = racked(&files, &instruments, true, "perform");
        let mut h = Harness::new(&p, f64::from(w), f64::from(hgt));
        h.settle_art();
        let mut measure = |h: &mut Harness, label: &str, input: &mut dyn FnMut(usize, &Ui) -> Input| {
            let n = 200;
            let (mut frame, mut paint) = (Vec::new(), Vec::new());
            for i in 0..n {
                let input = input(i, &h.ui);
                let t = cpu_us();
                h.tick(input);
                frame.push(cpu_us() - t);
                if i % 4 == 0 {
                    let t = cpu_us();
                    raster.reset();
                    vello::paint(
                        &mut vello::Cpu { ctx: &mut raster, resources: &mut resources, cache: &mut cache },
                        h.ui.scene().unwrap(),
                        vello::kurbo::Affine::IDENTITY,
                    )
                    .unwrap();
                    raster.flush();
                    raster.render(&mut pix, &mut resources);
                    paint.push(cpu_us() - t);
                }
            }
            let q = |v: &mut Vec<f64>, at: f64| {
                v.sort_by(f64::total_cmp);
                v[((v.len() - 1) as f64 * at) as usize]
            };
            println!(
                "{name:>8} {label:>12}: build+layout p50 {:>6.0} p99 {:>6.0} us   cpu paint p50 {:>6.0} us",
                q(&mut frame, 0.5),
                q(&mut frame, 0.99),
                q(&mut paint, 0.5)
            );
        };
        measure(&mut h, "idle", &mut |_, _| Input::default());
        let list = center(&h.ui, "browser-list");
        measure(&mut h, "browser", &mut |i, _| Input {
            wheel: Vec2::new(0., if i % 100 < 50 { 60. } else { -60. }),
            ..pointer(list, false)
        });
        h.idle(30);
        let knob = center(&h.ui, "volume-0");
        measure(&mut h, "knob", &mut |i, _| pointer(Point::new(knob.x, knob.y - (i % 40) as f64), i % 60 != 59));
        h.idle(30);
        let edge = center(&h.ui, "resize-0");
        measure(&mut h, "resize", &mut |i, _| {
            pointer(Point::new(edge.x, edge.y - (i % 80) as f64), i % 100 != 99)
        });
        h.idle(30);
        let rack = center(&h.ui, "rack-view");
        measure(&mut h, "rack scroll", &mut |i, _| Input {
            wheel: Vec2::new(0., if i % 100 < 50 { 60. } else { -60. }),
            ..pointer(rack, false)
        });
        h.idle(30);
        let tabs = ["tab-mixer", "tab-rack", "tab-mapping", "tab-sound", "tab-info", "tab-rack"];
        measure(&mut h, "tabs", &mut |i, ui| {
            let at = center(ui, tabs[(i / 10) % tabs.len()]);
            pointer(at, i % 10 == 0)
        });
        h.press("tab-mixer");
        h.idle(30);
        let fader = center(&h.ui, "mix-fader-0");
        measure(&mut h, "mixer fader", &mut |i, _| pointer(Point::new(fader.x, fader.y - (i % 40) as f64), i % 60 != 59));
        h.press("tab-rack");
        h.idle(30);
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
    let files = presets(Path::new(LIBRARY_ROOT));
    // KONTAKTO_SHOT="Name A,Name B" renders other instruments in the rack slots.
    let chosen = std::env::var("KONTAKTO_SHOT")
        .unwrap_or_else(|_| "Vista - Harp,Vista - 3 Cellos,Vista - 5 Violins".into());
    let instruments = library_instruments(&files, &chosen);
    std::fs::create_dir_all(".impeccable/review").unwrap();
    let states: &[(&str, bool, &[&str])] = &[
        ("empty", false, &[]),
        // Libraries with no artwork: generated covers; one chosen.
        ("covers", false, &["library-2"]),
        // No library folders yet: how to add them.
        ("no-libraries", false, &[]),
        ("perform", true, &[]),
        ("library-ui", true, &[]),
        // Esc: no part selected, the keys show what each part plays.
        ("unselected", true, &[]),
        ("mapping", true, &["tab-mapping"]),
        ("rack", true, &["tab-rack"]),
        ("info", true, &["tab-info"]),
        ("library", false, &["library-1"]),
        ("multis", false, &["picker-multis"]),
        // A library's folders, one opened by the keys, a part loaded.
        ("browser-tree", true, &["library-2"]),
        // The search inside a library: flat, each with its folder under it.
        ("browser-search", false, &["library-2"]),
        // The library filter typed into, and the sort menu.
        ("browser-filter", false, &[]),
        ("browser-sort", false, &["library-sort"]),
        ("settings", true, &["app-menu", "menu-item-5"]),
        ("menu", true, &["app-menu"]),
        ("save", true, &["app-menu", "menu-item-8"]),
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
        ("mixer-tree", true, &["tab-mixer"]),
        // Auto-align on: the settings and a part's timing.
        ("timing", true, &["app-menu"]),
        ("timing-part", true, &["more-0"]),
        // The articulation list in each of its modes (instruments with one).
        ("arts-channel", true, &["art-mode-0-Channel"]),
        ("arts-velocity", true, &["art-mode-0-Velocity"]),
        // A keyswitch clicked: typed in place, or learned from a key.
        ("arts-editing", true, &["art-key-0-1"]),
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
    for &(state, loaded, presses) in states
        .iter()
        // Without the owner's library (CI), only the states with no instrument.
        .filter(|(s, loaded, _)| wanted(s) && (*s != "sticky" || sticky_ok) && (!loaded || !instruments.is_empty()))
    {
        for &(width, height) in &sizes {
            let p = racked(&files, &instruments, loaded, state);
            // A chord with a little noise under it, for the spectrum.
            let heard = state.starts_with("mixer") || state.starts_with("sound");
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
            if heard {
                p.shared.scope.push(&signal);
            }
            if state.starts_with("mixer") {
                p.shared.part(1).unwrap().clip.store(true, Ordering::Relaxed);
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
                p.shared.with_parts(|parts| {
                    for (part, [l, r]) in parts.iter().zip([[0.5, 0.42], [0.9, 1.05], [0.05, 0.03]]) {
                        part.meter[0].store(f32::to_bits(l), Ordering::Relaxed);
                        part.meter[1].store(f32::to_bits(r), Ordering::Relaxed);
                    }
                });
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
            let typed = |h: &mut Harness, id: &str, text: &str| {
                h.ui.focus(id);
                h.tick(Input { text: text.into(), ..Default::default() });
                h.idle(2);
            };
            let tap = |h: &mut Harness, key: Key| {
                h.tick(Input { keys: vec![KeyPress { key, mods: Mods::default() }], ..Default::default() });
                h.idle(2);
            };
            match state {
                "browser-tree" => {
                    for key in [Key::Down, Key::Right, Key::Right, Key::Down] {
                        tap(&mut h, key);
                    }
                }
                "browser-search" => typed(&mut h, "search", "a"),
                "browser-filter" => typed(&mut h, "library-filter", "a"),
                _ => {}
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
            // The spectrum eases in over a few of its looks, the signal still coming.
            if heard {
                for _ in 0..10 {
                    std::thread::sleep(Duration::from_millis(35));
                    p.shared.scope.push(&signal);
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


#[test]
fn failed_load_diagnostics_remain_visible_without_an_instrument() {
    use moose::prelude::BackgroundTask;
    let p = Arc::new(SamplerParams::new());
    p.selection.write().unwrap().parts.push(Part { path: "/missing-kontra-test/instrument.nki".into(), ..Default::default() });
    {
        let mut view = p.shared.view.lock().unwrap();
        view.parts[0].loading = true;
        view.parts[0].status = "Loading import…".into();
    }
    let mut h = Harness::new(&p, 1180., 760.);
    h.idle(2);
    crate::plugin::Load.run(&p);
    h.idle(2);
    let status = p.shared.view.lock().unwrap().parts[0].status.clone();
    assert!(status.starts_with("Load failed: "), "{status}");
    assert!(h.ui.scene().unwrap().surface("stage-0").is_some(), "a failed part keeps its place in the rack");
    h.press("tab-logs");
    assert!(h.ui.scene().unwrap().surface("logs-export-preview").is_some(), "global Logs is available after a failed load");
    h.press("logs-export-preview");
    assert!(h.ui.scene().unwrap().surface("logs-export-path").is_some(), "export preview does not require a loaded instrument");
    h.press("logs-export-preview");
}

#[test]
fn idle_editor_rebuilds_only_when_something_moves() {
    let p = Arc::new(SamplerParams::new());
    // The library is scanned: nothing is pending for the loader.
    p.shared.view.lock().unwrap().scanned = p.shared.libraries.wanted();
    let meters = Meters::default();
    let mut watch = Watch::default();
    let computer = computer::Computer::default();
    let mut changed = || watch.changed(&p, &meters, &computer);
    assert!(changed(), "the first tick builds");
    std::thread::sleep(Duration::from_millis(110));
    assert!(!changed(), "nothing moved: no rebuild, however long");
    let log = || crate::diagnostics::event(
        crate::diagnostics::LogLevel::Info,
        "ui_test",
        "idle_visibility",
        serde_json::json!({"reason": "synthetic journal visibility check"}),
    );
    log();
    assert!(!changed(), "a hidden journal does not rebuild this editor");
    meters.logs_visible.store(true, Ordering::Relaxed);
    assert!(changed(), "opening Logs watches the journal");
    log();
    assert!(changed(), "a visible journal refreshes when an event arrives");
    meters.logs_visible.store(false, Ordering::Relaxed);
    assert!(changed(), "closing Logs stops watching the journal");
    log();
    assert!(!changed(), "later journal events leave the hidden pane idle");
    p.shared.voices.store(3, Ordering::Relaxed);
    assert!(!changed(), "readouts wait for their next look");
    std::thread::sleep(Duration::from_millis(READOUT_MS + 10));
    assert!(changed(), "a voice count change shows on the next look");
    assert!(!changed());
    p.shared.dropouts.store(1, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(READOUT_MS + 10));
    assert!(changed(), "so does a dropout");
    p.shared.view.lock().unwrap().parts[0].loading = true;
    assert!(changed(), "loading animates");
    assert!(!changed(), "at most every {ANIMATION_MS} ms");
}

#[test]
fn a_preset_is_in_the_nearest_library_above_it() {
    use crate::library::{Library, Shelf};
    let library = |dir: &str, name: &str| Library { dir: dir.into(), name: name.into(), ..Library::default() };
    let shelf = Shelf::new(vec![library("/libs/Solo", "Solo"), library("/libs/Vendor/Areia", "Areia"), library("/libs/Solo/Extra", "Extra")]);
    let at = |path: &str| library_of(&shelf, Path::new(path));
    assert_eq!(at("/libs/Solo/Instruments/a.nki"), "Solo");
    assert_eq!(at("/libs/Vendor/Areia/Instruments/x/a.nki"), "Areia");
    assert_eq!(at("/libs/Solo/Extra/a.nki"), "Extra", "the nearer of two");
    assert_eq!(at("/libs/SoloX/a.nki"), "");
    assert_eq!(at("/other/a.nki"), "");
    #[cfg(windows)]
    assert_eq!(at(r"\libs\Solo/Instruments\a.nki"), "Solo", "mixed Windows separators");
}

/// The window as pixels, RGBA, painted from the last frame's scene.
pub(super) fn pixels(ui: &Ui, width: u16, height: u16) -> Vec<u8> {
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

/// Routing from the tree mixer: a part's strip picks its host pair, solo
/// and mute write back to the part, and Outputs opens its menu.
#[test]
fn mixer_routing_edits() {
    let p = two_parts();
    let part = |p: &SamplerParams, n: usize| p.selection.read().unwrap().parts[n].clone();
    let mut h = Harness::new(&p, 1180., 760.);
    h.press("tab-mixer");
    let shows = |h: &Harness, id: &str| h.ui.scene().unwrap().surface(id).is_some();
    let (a, b) = (1u64 << 16, 2u64 << 16);
    assert!(shows(&h, &format!("mt-strip-{a}")) && shows(&h, &format!("mt-strip-{b}")));

    // Host 1/2, 3/4, 5/6 …, then Automatic.
    h.press(&format!("mt-out-{a}"));
    h.press(&format!("mt-pick-{a}-2"));
    assert_eq!(part(&p, 0).output, 2, "the pick routes");
    assert!(part(&p, 0).output_manual, "and the route sticks");
    h.press(&format!("mt-out-{a}"));
    h.press(&format!("mt-pick-{a}-{}", crate::sound::BUSES));
    assert!(!part(&p, 0).output_manual, "Automatic hands it back");

    h.press(&format!("solo-mt-{b}"));
    h.press(&format!("mute-mt-{a}"));
    assert!(part(&p, 1).solo && part(&p, 0).mute);

    h.press("mix-outputs");
    assert!(shows(&h, "menu-item-1"), "Outputs opens its menu");
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
    for m in &p.shared.part(0).unwrap().meter {
        m.store(0.8f32.to_bits(), Ordering::Relaxed);
    }
    h.ui.frame(root, Some(h.size), Input::default(), 0.).unwrap();
    let loud = pixels(&h.ui, width, height);
    let r = h.ui.scene().unwrap().surface(&format!("mt-strip-{}", 1u64 << 16)).unwrap().frame;
    let lit = (r.y as usize..(r.y + r.size.height) as usize)
        .filter(|&y| {
            let at = |x: usize| (y * usize::from(width) + x) * 4;
            (r.x as usize..(r.x + r.size.width) as usize).any(|x| quiet[at(x)..at(x) + 4] != loud[at(x)..at(x) + 4])
        })
        .count();
    assert!(lit as f64 > r.size.height / 4., "the meter shows the level: {lit} rows");

    let meters = Meters::default();
    let mut watch = Watch::default();
    let computer = computer::Computer::default();
    let mut changed = || watch.changed(&p, &meters, &computer);
    let settle = Duration::from_millis(ANIMATION_MS + 5);
    let level = |v: f32| {
        for m in &p.shared.part(0).unwrap().meter {
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
    let root = Path::new(LIBRARY_ROOT);
    let paths = [
        "Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki",
        "Afflatus Chapter II Brass/Instruments/4. Experimental/Mega Brass.nki",
        "Audio Imperia CHORUS/Instruments/01 Multi Patches/01 Chorus - Women - Traditional Articulations.nki",
    ];
    let p = Arc::new(SamplerParams::new());
    p.shared.view.lock().unwrap().scanned = p.shared.libraries.wanted();
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
            let now = p.shared.part(slot).unwrap().load_progress.load(Ordering::Relaxed);
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

/// Whether a key top is lit: the neutral LED darkens a light key well
/// away from `rest`, a white key's unlit top.
fn glows(c: [u8; 3], rest: [u8; 3]) -> bool {
    c.iter().zip(rest).map(|(&a, b)| i32::from(a.abs_diff(b))).sum::<i32>() > 60
}

/// A drag across the keys moves the sound and the light together, one note
/// at a time; letting go anywhere stops it, and a note nothing holds any
/// more is let go on the next frame.
#[test]
fn a_glide_lights_what_sounds_and_lets_go_anywhere() {
    let p = Arc::new(SamplerParams::new());
    let mut h = Harness::new(&p, 1180., 760.);
    let rest = key_tops(&h.ui, 60..61)[0];
    let rests = key_tops(&h.ui, 48..84);
    for note in [60, 64] {
        let at = key_spot(&h.ui, note);
        for _ in 0..40 {
            h.tick(pointer(at, true));
        }
    }
    assert_eq!(keys_down(&p), [64], "one note sounds: the one under the pointer");
    let tops = key_tops(&h.ui, 60..65);
    assert!(glows(tops[4], rest), "and it is the one lit");
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
    assert!(!key_tops(&h.ui, 48..84).into_iter().zip(rests).any(|(c, r)| glows(c, r)), "and nothing stays lit");
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

/// Part and bus hues skip orange, start where they always did, and stay apart.
#[test]
fn hue_walks_skip_orange() {
    use super::theme::{golden_hue, orange};
    for from in [250., 190.] {
        assert!((golden_hue(from, 0) - from).abs() < 0.01);
        let hues: Vec<f32> = (0..16).map(|n| golden_hue(from, n)).collect();
        assert!(hues.iter().all(|&h| !orange(h)), "{hues:?}");
        assert!(hues.windows(2).all(|w| (w[0] - w[1]).rem_euclid(360.).min((w[1] - w[0]).rem_euclid(360.)) > 60.));
    }
}

#[test]
fn a_wheel_notch_scrolls_five_rows_and_a_fling_speeds_up() {
    use moose::mui::window::notch_lines;
    use std::time::Duration;
    let mut run = 0;
    // One notch, from rest: 120 points, five list rows.
    let first = notch_lines(1., None, &mut run) * TEXT;
    assert_eq!(first, 120.);
    // Fast notches the same way: each further, to a cap.
    let fast = Some(Duration::from_millis(20));
    let steps: Vec<f64> = (0..12).map(|_| notch_lines(1., fast, &mut run) * TEXT).collect();
    assert!(steps.windows(2).all(|w| w[1] >= w[0]) && steps[0] > first, "{steps:?}");
    assert_eq!(steps[11], first * 3.);
    // A turn or a pause starts over.
    assert_eq!(notch_lines(-1., fast, &mut run) * TEXT, -first);
    assert_eq!(notch_lines(-1., fast, &mut run) * TEXT, -first * 1.25);
    assert_eq!(notch_lines(-1., Some(Duration::from_millis(300)), &mut run) * TEXT, -first);
}

/// What the pointer shows over each kind of thing: resize arrows over the
/// edges, a few points either side of their line too, the hand over
/// buttons, the I-beam over text, and the knob's up-down.
#[test]
fn the_cursor_follows_what_is_under_the_pointer() {
    let p = two_parts();
    let mut h = Harness::new(&p, 1180., 760.);
    let cursor_at = |h: &mut Harness, at: Point| {
        let root = (h.build)(&mut h.ui, &mut h.bridge);
        h.ui.frame(root, Some(h.size), pointer(at, false), 1. / 60.).unwrap().cursor
    };
    let frame = |h: &Harness, id: &str| h.ui.scene().unwrap().surface(id).unwrap_or_else(|| panic!("no {id}")).frame;
    let split = frame(&h, "splitter");
    for dx in [1., EDGE_GRAB] {
        assert_eq!(cursor_at(&mut h, Point::new(split.x + dx, split.y + 200.)), Cursor::ResizeH, "browser edge +{dx}");
    }
    let divider = frame(&h, "browser-split");
    for dy in [-EDGE_GRAB / 2., EDGE_GRAB / 2.] {
        let at = Point::new(divider.x + 40., divider.y + divider.size.height / 2. + dy);
        assert_eq!(cursor_at(&mut h, at), Cursor::ResizeV, "browser divider {dy:+}");
    }
    let edge = frame(&h, "resize-0");
    for dy in [1., EDGE_GRAB] {
        let at = Point::new(edge.x + edge.size.width / 2., edge.y + edge.size.height - dy);
        assert_eq!(cursor_at(&mut h, at), Cursor::ResizeV, "part edge -{dy}");
    }
    for (id, want) in [("tab-mixer", Cursor::Hand), ("search", Cursor::Text), ("volume-0", Cursor::ResizeV)] {
        let at = center(&h.ui, id);
        assert_eq!(cursor_at(&mut h, at), want, "{id}");
    }
    let corner = frame(&h, "window-corner");
    cursor_at(&mut h, Point::new(corner.x + corner.size.width - 2., corner.y + corner.size.height - 2.));
    assert!(h.ui.get("window-corner").hovered, "the window's resize corner");
}


#[test]
fn rack_growth_preserves_restored_slots_and_same_frame_duplicate() {
    let p = Arc::new(SamplerParams::new());
    {
        let mut selection = write(&p.selection);
        selection.parts = (0..129).map(|n| Part {
            path: format!("/virtual/Restored/Part {n}.nki"),
            collapsed: true,
            ..Default::default()
        }).collect();
        selection.order = vec![128, 0, 128, 400];
    }
    // Slot128 was formerly also the no-focus sentinel. It remains a real
    // requested part, independently of the current registry capacity.
    p.shared.focus_request.store(128, Ordering::Relaxed);
    let mut h = Harness::new(&p, 1180., 760.);
    h.idle(2);
    assert_eq!(read(&p.selection).parts.len(), 129, "restoration never truncates the rack");
    assert_eq!(read(&p.selection).order.len(), 129, "order covers every occupied slot once");
    assert_eq!(selected_slot(&p), 128);
    assert_eq!(p.shared.focus_request.load(Ordering::Relaxed), u64::MAX);
    assert!(lock(&p.shared.view).parts.len() >= 129);
    // This append runs inside the editor frame, after its view was captured.
    // Rendering the new part must use a prepared row, not the prior snapshot's
    // length, and selection/routing still commit through the ordinary path.
    h.tick(Input {
        keys: vec![KeyPress { key: Key::Char('d'), mods: Mods { ctrl: true, ..Default::default() } }],
        ..Default::default()
    });
    h.idle(1); // The harness delivers keys to the following editor build.
    assert_eq!(read(&p.selection).parts.len(), 130);
    assert_eq!(read(&p.selection).parts[129].path, read(&p.selection).parts[128].path);
    assert_eq!(selected_slot(&p), 129);
    assert!(p.shared.part(129).is_some(), "meter/loader atomics are prepared before rendering");
    assert!(lock(&p.shared.view).parts.len() >= 130);
    h.idle(3);
    assert!(h.ui.scene().unwrap().surface("header-129").is_some(), "the duplicated part is revealed");
    let bounded = |h: &Harness, count: usize| {
        let scene = h.ui.scene().unwrap();
        let viewport = scene.surface("rack-view").unwrap().frame;
        let headers = (0..count).filter(|n| scene.surface(&format!("header-{n}")).is_some()).count();
        assert!(headers <= (viewport.size.height / rack::SLIM).ceil() as usize + 4,
            "header subtrees follow the viewport, not {count} stored parts: {headers}, {viewport:?}");
        assert!(scene.surface("rack-content").unwrap().frame.size.height >= count as f64 * rack::SLIM,
            "offscreen rows retain their scroll extent");
    };
    bounded(&h, 130);
    // Put the selected source at the far end, then exercise the real keyboard
    // append/reveal path there. A virtual row has no old surface to scroll to.
    write(&p.selection).order.rotate_left(2);
    h.idle(2);
    h.tick(Input {
        keys: vec![KeyPress { key: Key::Char('d'), mods: Mods { ctrl: true, ..Default::default() } }],
        ..Default::default()
    });
    h.idle(1); // Dispatch the queued shortcut through the normal build path.
    assert_eq!(read(&p.selection).parts.len(), 131);
    h.idle(60); // Allow the ordinary rack scroll spring to finish.
    let scene = h.ui.scene().unwrap();
    let viewport = scene.surface("rack-view").unwrap().frame;
    let header = scene.surface("header-130").expect("the appended virtual row is revealed").frame;
    assert!(header.y < viewport.y + viewport.size.height && header.y + header.size.height > viewport.y,
        "revealed header intersects the viewport: {header:?}, {viewport:?}");
    bounded(&h, 131);
}

#[test]
fn library_rename_edits_only_the_display_name_and_filter_follows_it() {
    let p = Arc::new(SamplerParams::new());
    let dir = "/virtual/rename-fixture/Tubular Bell";
    p.shared.libraries.edit(|s| { s.rename_library(dir, ""); s.sort = crate::library::Sort::Name; });
    {
        let mut view = p.shared.view.lock().unwrap();
        view.files = Arc::new(vec![format!("{dir}/Bell.nki").into()]);
        view.shelf = Arc::new(crate::library::Shelf::new(vec![crate::library::Library {
            dir: dir.into(), name: String::new(), instruments: 1, ..Default::default()
        }]));
    }
    let mut h = Harness::new(&p, 1180., 760.);
    let at = center(&h.ui, "library-0"); // Empty metadata names still have a visible folder fallback.
    for buttons in [Buttons::default().set(Button::Secondary, true), Buttons::default()] {
        h.tick(Input { pointer: PointerInput { pos: Some(at), buttons, ..Default::default() }, ..Default::default() });
    }
    h.idle(2);
    h.press("menu-item-0");
    h.idle(2);
    assert_eq!(h.ui.focus_key(), Some("library-name"));
    h.tick(Input { keys: vec![KeyPress { key: Key::Char('a'), mods: Mods { ctrl: true, ..Default::default() } }], ..Default::default() });
    h.tick(Input { text: "Evening Bells Library".into(), ..Default::default() });
    h.tick(enter()); h.idle(3);
    let settings = p.shared.libraries.settings();
    assert_eq!(settings.names.get(dir).map(String::as_str), Some("Evening Bells Library"));
    let view = p.shared.view.lock().unwrap();
    assert_eq!(view.shelf.libraries[0].name, "", "resource and source identity is unchanged");
    assert_eq!(view.files[0], PathBuf::from(format!("{dir}/Bell.nki")));
    assert_eq!(settings.library_name(&view.shelf.libraries[0]), "Evening Bells Library", "typed names retain their suffix");
    drop(view);
    h.ui.focus("library-filter");
    h.tick(Input { text: "evening".into(), ..Default::default() }); h.idle(2);
    assert!(h.ui.scene().unwrap().surface("library-0").is_some(), "the new name is searchable");
    h.press("library-0");
    assert!(h.ui.scene().unwrap().surface("instrument-0").is_some(), "selection retains its canonical source");
    p.shared.libraries.edit(|s| s.rename_library(dir, ""));
}

include!("viewmodel_tests.rs");
