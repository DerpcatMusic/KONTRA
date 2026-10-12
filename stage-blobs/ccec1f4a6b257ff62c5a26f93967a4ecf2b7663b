//! Headless: the driver with no window and no GPU.
use super::*;
use crate::prelude::{block, knob};

struct NoClipboard;
impl Clipboard for NoClipboard {
    fn get(&mut self) -> Option<String> {
        None
    }
    fn set(&mut self, _: &str) {}
}

/// A knob, a count of the trees built and the trail each one read.
struct Knob {
    value: f64,
    builds: usize,
    trails: Vec<usize>,
    changed: bool,
    zoom: f64,
}

impl Default for Knob {
    fn default() -> Self {
        Self {
            value: 0.5,
            builds: 0,
            trails: Vec::new(),
            changed: false,
            zoom: 1.0,
        }
    }
}

impl View for Knob {
    fn build(&mut self, ui: &mut Ui, input: &Input) -> El {
        self.builds += 1;
        self.trails.push(input.trail.len());
        knob(ui, "k", "K", &mut self.value, 0.0..=1.0).into()
    }
    fn changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn zoom(&self, window: Size) -> f64 {
        // Negative: fit a 200-point-wide design to the window.
        if self.zoom < 0.0 {
            window.width / 200.0
        } else {
            self.zoom
        }
    }
    fn claims_key(&self, key: &Key, _: Mods) -> bool {
        *key == Key::Escape
    }
}

/// A plain surface: no springs, so nothing but its gesture edges asks for
/// another frame.
struct Pad;

impl View for Pad {
    fn build(&mut self, _: &mut Ui, _: &Input) -> El {
        block(100.0, 100.0).id("pad")
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}

/// Counts the Enter presses its trees read; refuses layout while `refuse`.
struct Keys {
    enters: usize,
    refuse: bool,
}

impl View for Keys {
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        self.enters += ui
            .shortcuts()
            .iter()
            .filter(|k| k.key == Key::Enter)
            .count();
        let side = if self.refuse { f64::NAN } else { 100.0 };
        block(side, side).id("pad")
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}

/// A driver over `view`, and a clock a frame apart per `step`.
struct Rig<V> {
    d: Driver,
    s: Shared<V>,
}

impl<V: View> Rig<V> {
    fn new(view: V, size: (u32, u32), scale: f64) -> Self {
        Self {
            d: Driver::new(size, scale, Box::new(NoClipboard)),
            s: Shared {
                ui: Ui::default(),
                view,
            },
        }
    }
    fn step(&mut self) -> bool {
        let now = self.d.last_frame() + Duration::from_millis(16);
        self.d.advance(&mut self.s, now)
    }
    fn key(&mut self, key: &KeyEvent) -> bool {
        self.d.key(&self.s, key)
    }
}

fn none() -> Mods {
    Mods::default()
}

fn at(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn press(code: u64, key: NativeKey, mods: Mods) -> KeyEvent {
    KeyEvent {
        code,
        key,
        down: true,
        mods,
    }
}

fn release(code: u64, key: NativeKey, mods: Mods) -> KeyEvent {
    KeyEvent {
        code,
        key,
        down: false,
        mods,
    }
}

fn inputs(d: &Driver) -> Vec<&Input> {
    d.pending
        .iter()
        .map(|p| match p {
            Pending::Input(i) | Pending::Move(i) | Pending::Drag(i) => i,
            Pending::Cancel(_) => panic!("unexpected cancel"),
        })
        .collect()
}

#[test]
fn layout_is_logical_when_the_surface_is_physical() {
    assert_eq!(logical_size((2560, 1600), 2.0), Size::new(1280.0, 800.0));
    let mut r = Rig::new(Knob::default(), (640, 400), 1.0);
    r.step();
    r.d.resized((640, 400), 2.0);
    assert_eq!((r.d.size(), r.d.ui_scale()), ((640, 400), 2.0));
    assert!(r.d.dirty);
}

#[test]
fn zoom_scales_the_paint_and_divides_the_pointer_and_pixel_wheel() {
    let mut r = Rig::new(
        Knob {
            zoom: 2.0,
            ..Knob::default()
        },
        (800, 600),
        1.5,
    );
    r.step();
    assert_eq!(r.d.ui_scale(), 3.0);
    r.d.pointer_moved(at(100.0, 60.0), none());
    assert_eq!(r.d.pointer().pos, Some(at(50.0, 30.0)));
    r.d.wheel(Wheel::Pixels(0.0, 10.0), none());
    assert_eq!(inputs(&r.d).last().unwrap().wheel, Vec2::new(0.0, -5.0));
}

/// A view that fits its design to the window is handed the logical size.
#[test]
fn zoom_reads_the_logical_window_size() {
    let fit = || Knob {
        zoom: -1.0,
        ..Knob::default()
    };
    let mut r = Rig::new(fit(), (800, 600), 2.0);
    r.step();
    assert_eq!(r.d.ui_scale(), 4.0, "400 logical wide is a zoom of 2");
}

#[test]
fn keyboard_modifiers_track_their_own_edges() {
    let mut r = Rig::new(Knob::default(), (400, 400), 1.0);
    let alt = Mods {
        alt: true,
        ..none()
    };
    // X11 samples a press's state before the modifier is set...
    r.key(&press(1, NativeKey::Modifier(Modifier::Alt), none()));
    assert!(r.d.pointer().mods.alt);
    // ...and a release's while it is still held.
    r.key(&release(1, NativeKey::Modifier(Modifier::Alt), alt));
    assert!(!r.d.pointer().mods.alt);
}

#[test]
fn space_is_a_key_and_text_but_a_shortcut_is_only_a_key() {
    let ctrl = Mods {
        ctrl: true,
        ..none()
    };
    let mut input = Input::default();
    typed(&mut input, &NativeKey::Text(" ".into()), none());
    assert_eq!((input.text.as_str(), input.keys[0].key), (" ", Key::Space));
    let mut input = Input::default();
    typed(&mut input, &NativeKey::Text("c".into()), ctrl);
    assert_eq!(
        (input.text.as_str(), input.keys[0].key),
        ("", Key::Char('c'))
    );
    assert!(input.keys[0].mods.ctrl);
    let mut input = Input::default();
    typed(&mut input, &NativeKey::Text("c".into()), none());
    assert_eq!((input.text.as_str(), input.keys.len()), ("c", 0));
    assert!(is_paste(&NativeKey::Text("V".into()), ctrl));
}

#[test]
fn keys_route_to_the_host_unless_focus_or_a_shortcut_claims_them() {
    let mut r = Rig::new(Knob::default(), (400, 400), 1.0);
    r.step();
    let space = || NativeKey::Text(" ".into());
    let up = || NativeKey::Named(Key::Up);
    // Nothing focused: the host's, but the frame still carries the mods.
    assert!(!r.key(&press(1, space(), none())));
    assert!(!r.key(&release(1, space(), none())));
    assert!(!r.key(&press(2, up(), none())));
    assert!(!r.key(&release(2, up(), none())));
    assert_eq!(r.d.pending.len(), 4, "still read, for the modifiers");
    assert!(
        inputs(&r.d)
            .iter()
            .all(|i| i.keys.is_empty() && i.text.is_empty())
    );
    // The view's shortcut, focused or not.
    assert!(r.key(&press(3, NativeKey::Named(Key::Escape), none())));
    // A focused control: its navigation keys, never Space or characters.
    r.s.ui.focus("k");
    assert!(!r.key(&press(1, space(), none())));
    assert!(!r.key(&press(4, NativeKey::Text("a".into()), none())));
    assert!(r.key(&press(2, up(), none())));
    // The release follows its press, even after focus moved.
    r.s.ui.blur();
    assert!(r.key(&release(2, up(), none())));
    assert!(!r.key(&release(2, up(), none())));
}

#[test]
fn queued_pointer_edges_keep_their_order() {
    let mut r = Rig::new(Knob::default(), (200, 200), 1.0);
    r.d.pointer_moved(at(10.0, 20.0), none());
    r.d.button(Button::Primary, true, none());
    r.d.button(Button::Primary, false, none());
    let i = inputs(&r.d);
    assert_eq!(i.len(), 3);
    assert_eq!(i[0].pointer.pos, Some(at(10.0, 20.0)));
    assert!(i[1].pointer.buttons.contains(Button::Primary));
    assert!(!i[2].pointer.buttons.contains(Button::Primary));
}

#[test]
fn hover_bursts_coalesce_but_drag_samples_and_modifier_changes_survive() {
    let mut r = Rig::new(Knob::default(), (1280, 800), 1.0);
    for x in 0..1000 {
        r.d.pointer_moved(at(f64::from(x), 20.0), none());
    }
    assert_eq!(r.d.pending.len(), 1);
    assert_eq!(inputs(&r.d)[0].pointer.pos, Some(at(999.0, 20.0)));
    let shift = Mods {
        shift: true,
        ..none()
    };
    r.d.pointer_moved(at(1000.0, 20.0), shift);
    assert_eq!(r.d.pending.len(), 2);
    r.d.button(Button::Primary, true, shift);
    for x in 0..10 {
        r.d.pointer_moved(at(f64::from(x), 30.0), shift);
    }
    r.d.button(Button::Primary, false, shift);
    assert_eq!(r.d.pending.len(), 14);
}

#[test]
fn drag_samples_between_two_edges_are_one_frame_with_a_trail() {
    let mut r = Rig::new(Knob::default(), (400, 400), 1.0);
    r.step();
    r.d.pointer_moved(at(10.0, 10.0), none());
    r.d.button(Button::Primary, true, none());
    for x in [12.0, 14.0, 16.0] {
        r.d.pointer_moved(at(x, 10.0), none());
    }
    r.d.button(Button::Primary, false, none());
    r.s.view.trails.clear();
    assert!(r.step());
    // hover, press, one drag frame (two samples skipped), release.
    assert_eq!(r.s.view.trails, [0, 0, 2, 0]);
}

#[test]
fn focus_loss_queues_a_cancel_after_the_pressed_snapshot() {
    let mut r = Rig::new(Knob::default(), (200, 200), 1.0);
    r.d.button(Button::Primary, true, none());
    r.d.focus(false);
    assert!(r.d.pointer().buttons.is_empty());
    assert!(
        matches!(r.d.pending.front(), Some(Pending::Input(i)) if i.pointer.buttons.contains(Button::Primary))
    );
    assert!(matches!(r.d.pending.get(1), Some(Pending::Cancel(p)) if p.buttons.is_empty()));
}

#[test]
fn an_idle_tick_builds_nothing_until_something_changes() {
    let mut r = Rig::new(Knob::default(), (400, 300), 1.0);
    assert!(r.step(), "the first tick paints");
    let settled = r.s.view.builds;
    // Knob springs may still be settling from the first frame.
    for _ in 0..600 {
        r.step();
    }
    let idle = r.s.view.builds;
    assert!(!r.step(), "a settled editor paints nothing");
    assert_eq!(r.s.view.builds, idle);
    assert!(idle >= settled);

    r.s.view.changed = true;
    assert!(r.step(), "a model change is a frame");
    r.d.redraw();
    assert!(r.step(), "so is a redraw request");
    r.d.button(Button::Primary, true, none());
    assert!(r.step(), "and an edge");
}

#[test]
fn a_press_and_release_between_two_ticks_are_two_frames() {
    let mut r = Rig::new(Knob::default(), (400, 300), 1.0);
    r.step();
    let before = r.s.view.builds;
    r.d.pointer_moved(at(5.0, 5.0), none());
    r.d.button(Button::Primary, true, none());
    r.d.button(Button::Primary, false, none());
    assert!(r.step());
    assert_eq!(r.s.view.builds - before, 3, "one frame per queued event");
    assert!(r.d.pending.is_empty());
}

#[test]
fn a_minimised_window_holds_its_input_until_it_has_a_size() {
    let mut r = Rig::new(Knob::default(), (400, 300), 1.0);
    r.step();
    r.d.resized((0, 0), 1.0);
    r.d.button(Button::Primary, true, none());
    assert!(!r.step());
    assert_eq!(r.d.pending.len(), 1, "the press is kept, not dropped");
    r.d.resized((400, 300), 1.0);
    assert!(r.step());
    assert!(r.d.pending.is_empty());
}

#[test]
fn a_delivered_edge_gets_the_tree_that_dispatches_it_and_an_inert_hover_none() {
    let mut r = Rig::new(Pad, (200, 200), 1.0);
    r.step();
    r.d.pointer_moved(at(50.0, 50.0), none());
    r.step();
    assert!(r.step(), "the new hover is owed one tree");
    assert!(!r.step(), "then hovering a plain surface settles");
    r.d.pointer_moved(at(51.0, 50.0), none());
    assert!(!r.step(), "a hover that lands on the same target is inert");
    r.d.button(Button::Primary, true, none());
    assert!(r.step());
    assert_eq!(
        r.s.ui.edit("pad"),
        Some(crate::Edit::Begin),
        "the press delivered a Begin"
    );
    assert!(r.step(), "and the next tree, which dispatches it, comes");
}

#[test]
fn a_refused_layout_does_not_replay_the_keys_it_took() {
    let mut r = Rig::new(
        Keys {
            enters: 0,
            refuse: false,
        },
        (200, 200),
        1.0,
    );
    r.step();
    r.s.view.refuse = true;
    r.d.push_key(&press(1, NativeKey::Named(Key::Enter), none()), true);
    assert!(!r.step(), "the layout is refused");
    r.s.view.refuse = false;
    for x in 0..4 {
        r.d.pointer_moved(at(f64::from(x), 1.0), none());
        r.step();
    }
    assert_eq!(r.s.view.enters, 1, "one press, read once");
}

/// A plain surface that draws the pointer: every hover over it is a frame.
struct Tracker;

impl View for Tracker {
    fn build(&mut self, ui: &mut Ui, input: &Input) -> El {
        Pad.build(ui, input)
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn still(&self, _: &Ui, from: Option<Point>, to: Option<Point>) -> bool {
        from == to
    }
}

#[test]
fn a_view_that_draws_the_pointer_rebuilds_on_an_inert_hover() {
    let mut r = Rig::new(Tracker, (200, 200), 1.0);
    r.step();
    r.d.pointer_moved(at(50.0, 50.0), none());
    r.step();
    while r.step() {}
    r.d.pointer_moved(at(51.0, 50.0), none());
    assert!(r.step(), "Ui::inert says skip, the view says it moved");
    assert_eq!(r.d.framed, Some(at(51.0, 50.0)));
}

/// A plugin view keeps its canvas caches and goes to the window thread
/// behind `Arc<Mutex<Shared<V>>>`, so all of it must be `Send`. The tree
/// (`El`) is not, by design: it is built and painted on one thread.
#[test]
fn shared_state_with_canvas_caches_is_send() {
    fn send<T: Send>() {}
    fn shared<T: Send + Sync>() {}
    type Cache = crate::prelude::CanvasCache<u64>;
    send::<Ui>();
    send::<Cache>();
    // A view that is nothing but a cache stands in for one that keeps some.
    shared::<std::sync::Arc<Mutex<Shared<Cache>>>>();
}

#[test]
fn profiling_tracks_invalidation_skips_coalescing_and_clock_regressions() {
    let mut r = Rig::new(Pad, (400, 300), 1.0);
    assert!(r.d.profiler().is_none());
    r.d.enable_profiling(ProfileConfig { capacity: 4 });
    assert!(r.step());
    for _ in 0..8 {
        assert!(!r.step());
    }
    assert!(r.d.next_wake().is_none());
    // Outside the offered root: changing its hover target would legitimately build.
    r.d.pointer_moved(at(1000., 1000.), none());
    r.d.pointer_moved(at(1010., 1000.), none());
    assert!(!r.step());
    r.d.redraw();
    assert!(r.d.next_wake().is_some());
    assert!(r.step());
    let now = r.d.last_frame();
    assert!(!r.d.advance(&mut r.s, now - Duration::from_secs(1)));
    assert_eq!(r.d.last_frame(), now);
    let profile = r.d.profiler().unwrap();
    assert_eq!(profile.counter(Counter::Invalidations), 1);
    assert_eq!(profile.counter(Counter::InputsEnqueued), 2);
    assert_eq!(profile.counter(Counter::HoverCoalesced), 1);
    assert_eq!(profile.counter(Counter::InertHoverSkips), 1);
    assert_eq!(profile.counter(Counter::IdleSkips), 8);
    assert_eq!(profile.counter(Counter::ClockRegressions), 1);
    assert_eq!(profile.counter(Counter::Frames), 2);
    assert_eq!(profile.percentiles(Phase::Advance).retained, 4);
    assert!(profile.percentiles(Phase::Resolve).total >= 2);
    assert_eq!(profile.percentiles(Phase::PresentCall).total, 0);
    r.d.disable_profiling();
    assert!(r.d.profiler().is_none());
}

#[test]
fn queued_ime_edges_survive_throttling_minimize_and_clock_regression() {
    let mut r = Rig::new(Pad, (400, 300), 1.0);
    assert!(r.step());
    r.d.enable_profiling(ProfileConfig::default());
    r.d.min_interval = Some(Duration::from_millis(100));
    r.d.ime(Ime::Preedit {
        text: "あ".into(),
        cursor: Some((0, 3)),
    });
    r.d.ime(Ime::Commit("あ".into()));
    assert_eq!(inputs(&r.d).len(), 2);
    assert!(!r.step());
    assert_eq!(
        r.d.next_wake(),
        Some(r.d.last_frame() + Duration::from_millis(100))
    );
    r.d.resized((0, 0), 1.0);
    assert!(!r.step());
    assert_eq!(r.d.next_wake(), None);
    assert_eq!(inputs(&r.d).len(), 2);
    r.d.resized((400, 300), 1.0);
    let now = r.d.last_frame() + Duration::from_millis(100);
    assert!(r.d.advance(&mut r.s, now));
    assert!(r.d.pending.is_empty());
    let profile = r.d.profiler().unwrap();
    assert_eq!(profile.counter(Counter::InputsDispatched), 2);
    assert_eq!(profile.counter(Counter::MinimizedSkips), 1);
    assert_eq!(profile.counter(Counter::ThrottledSkips), 1);
    assert_eq!(profile.percentiles(Phase::InputQueueWait).total, 1);
}

#[test]
fn ime_configuration_uses_utf8_boundaries_and_disables_after_blur() {
    struct Text(String);
    impl View for Text {
        fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
            crate::widgets::text_input(ui, "edit", &mut self.0).el
        }
        fn changed(&mut self) -> bool {
            false
        }
        fn request_resize(&mut self, _: u32, _: u32) -> bool {
            false
        }
    }
    let mut r = Rig::new(Text("aé🙂".into()), (400, 300), 1.);
    r.s.ui =
        Ui::default().font(crate::prelude::Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    r.step();
    r.s.ui.focus("edit");
    r.d.ime_selection(1..7);
    r.step();
    assert!(r.d.next_wake().is_some());
    r.step();
    let config = r.d.ime_configuration(&r.s.ui).unwrap();
    assert_eq!(config.id, "edit");
    assert_eq!(config.text, "aé🙂");
    assert_eq!(config.selection, 1..7);
    assert_eq!(r.d.ime_area(), Some(config.area));
    r.d.ime(Ime::Preedit {
        text: "日本".into(),
        cursor: Some((3, 6)),
    });
    r.step();
    r.step();
    let config = r.d.ime_configuration(&r.s.ui).unwrap();
    assert_eq!(config.text, "a日本");
    assert_eq!(config.marked, Some(1..7));
    assert_eq!(r.s.view.0, "aé🙂");
    // Ordered replacements between two ticks target the appropriate text.
    r.d.ime_selection(1..7);
    r.d.ime(Ime::Commit("X".into()));
    r.d.ime_selection(0..1);
    r.d.ime(Ime::Commit("Y".into()));
    r.step();
    r.step();
    assert_eq!(r.s.view.0, "YX");
    assert!(r.d.ime_configuration(&r.s.ui).unwrap().marked.is_none());
    r.d.focus(false);
    assert!(r.d.ime_configuration(&r.s.ui).is_none());
    assert!(r.d.ime_area().is_none());
    r.d.ime_selection(0..2);
    r.d.ime(Ime::Preedit {
        text: "late".into(),
        cursor: None,
    });
    r.d.ime(Ime::Commit("late".into()));
    for _ in 0..4 {
        r.step();
    }
    assert_eq!(
        r.s.view.0, "YX",
        "late callbacks cannot edit an unfocused window"
    );
    assert!(r.s.ui.text_input_state().unwrap().marked.is_none());
    r.d.focus(true);
    r.d.ime_selection(0..2);
    r.d.ime(Ime::Commit("resumed".into()));
    r.step();
    r.step();
    assert_eq!(r.s.view.0, "resumed", "native input resumes with focus");
    r.s.ui.blur();
    r.d.redraw();
    r.step();
    assert!(r.d.ime_configuration(&r.s.ui).is_none());
    r.d.close(&mut r.s);
    assert!(r.d.ime_area().is_none());
}
