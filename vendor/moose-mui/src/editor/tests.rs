//! The moose side, headless: a recording host behind `PluginContext`, the
//! window's handler driven by synthetic events, and the host calls that
//! come out.
use super::*;
use crate::window::Handler;
use crate::window::baseview::{self, Event, MouseEvent};
use keyboard_types::{Key as HostKey, KeyState, KeyboardEvent, Modifiers, NamedKey};
use moose::prelude::*;
use moose_core::editor::ClosureBridge;
use mui::prelude::{Point, knob, toggle};
use mui::scene::prelude::row;
use std::sync::Mutex;

#[derive(Params)]
struct Synth {
    #[param(id = 10, name = "Gain", range = "linear(0, 1)", default = 0.5)]
    gain: FloatParam,
    #[param(id = 20, name = "Voices", range = "discrete(1, 8)", default = 1)]
    voices: IntParam,
    #[param(id = 30, name = "Bypass", default = false)]
    bypass: BoolParam,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Call {
    Begin(u32),
    Set(u32, f64),
    End(u32),
}

type Log = Arc<Mutex<Vec<Call>>>;

fn context(params: &Arc<Synth>) -> (PluginContext, Log) {
    let log = Log::default();
    let (begins, sets, ends) = (log.clone(), log.clone(), log.clone());
    let (writer, reader) = (params.clone(), params.clone());
    let bridge = ClosureBridge {
        begin_edit: Box::new(move |id| begins.lock().unwrap().push(Call::Begin(id))),
        end_edit: Box::new(move |id| ends.lock().unwrap().push(Call::End(id))),
        set_param: Box::new(move |id, v| {
            // A format wrapper writes the store as well as the host.
            writer.set_normalized(id, v);
            sets.lock().unwrap().push(Call::Set(id, v));
        }),
        get_param: Box::new(move |id| reader.get_normalized(id).unwrap_or(0.0)),
        get_param_plain: Box::new(|_| 0.0),
        format_param: Box::new(|_| String::new()),
        request_resize: Box::new(|_, _| false),
        get_meter: Box::new(|_| 0.0),
        get_state: Box::new(Vec::new),
        set_state: Box::new(|_| {}),
        transport: Box::new(|| None),
    };
    (
        PluginContext::from_closures(bridge, params.clone() as Arc<dyn Params>),
        log,
    )
}

/// A gain knob, a voices knob and a bypass toggle, each bound.
fn editor(params: &Arc<Synth>) -> MuiEditor<Synth> {
    MuiEditor::new(params.clone(), Ui::default(), (400, 300), |ui, bridge| {
        let gain = bridge.bind(ui, 10u32, |ui, id, v| knob(ui, id, "Gain", v, 0.0..=1.0));
        let voices = bridge.bind(ui, 20u32, |ui, id, v| knob(ui, id, "Voices", v, 0.0..=1.0));
        let bypass = bridge.bind_bool(ui, 30u32, |ui, id, on| toggle(ui, id, "Bypass", on));
        row([gain, voices, bypass])
    })
}

/// The editor opened on a recording host, and the window handler over it.
fn open(params: &Arc<Synth>) -> (MuiEditor<Synth>, Handler<Session<Synth>>, Log) {
    open_with(params, editor(params))
}

fn open_with(
    params: &Arc<Synth>,
    editor: MuiEditor<Synth>,
) -> (MuiEditor<Synth>, Handler<Session<Synth>>, Log) {
    let (context, log) = context(params);
    lock(&editor.shared)
        .view
        .bridge
        .attach(context.with_params(params.clone()));
    let mut h = Handler::new(
        editor.shared.clone(),
        editor.requests.clone(),
        (400, 300),
        1.0,
    );
    h.step();
    (editor, h, log)
}

fn centre(editor: &MuiEditor<Synth>, param: u32) -> Point {
    let s = lock(&editor.shared);
    let f =
        s.ui.scene()
            .unwrap()
            .surface(&crate::widget_id(param))
            .unwrap()
            .frame;
    Point::new(f.x + f.size.width / 2.0, f.y + f.size.height / 2.0)
}

fn mouse(h: &mut Handler<Session<Synth>>, event: MouseEvent) {
    h.on_event_inner(&Event::Mouse(event));
}

fn at(h: &mut Handler<Session<Synth>>, p: Point) {
    mouse(
        h,
        MouseEvent::CursorMoved {
            position: baseview::dpi::PhysicalPosition::new(p.x, p.y),
            modifiers: Modifiers::default(),
        },
    );
}

fn press(h: &mut Handler<Session<Synth>>, down: bool) {
    let (button, modifiers) = (baseview::MouseButton::Left, Modifiers::default());
    mouse(
        h,
        if down {
            MouseEvent::ButtonPressed { button, modifiers }
        } else {
            MouseEvent::ButtonReleased { button, modifiers }
        },
    );
}

fn calls(log: &Log) -> Vec<Call> {
    std::mem::take(&mut *log.lock().unwrap())
}

/// Every Set sits between one Begin and one End on its own id.
fn bracketed(calls: &[Call], id: u32) -> bool {
    let mut open = false;
    for call in calls {
        match *call {
            Call::Begin(i) if i == id => {
                if open {
                    return false;
                }
                open = true;
            }
            Call::End(i) if i == id => {
                if !open {
                    return false;
                }
                open = false;
            }
            Call::Set(i, _) if i == id && !open => return false,
            _ => {}
        }
    }
    !open
}

#[test]
fn a_drag_is_one_host_gesture_and_the_value_lands_inside_it() {
    let params = Arc::new(Synth::default());
    let (editor, mut h, log) = open(&params);
    let p = centre(&editor, 10);
    at(&mut h, p);
    press(&mut h, true);
    h.step();
    for dy in 1..=6 {
        at(&mut h, Point::new(p.x, p.y - f64::from(dy) * 5.0));
        h.step();
    }
    press(&mut h, false);
    h.step();
    h.step();
    let c = calls(&log);
    assert_eq!(c.first(), Some(&Call::Begin(10)), "{c:?}");
    assert_eq!(c.last(), Some(&Call::End(10)), "{c:?}");
    assert!(c.iter().filter(|c| matches!(c, Call::Set(10, _))).count() >= 5);
    assert!(bracketed(&c, 10), "{c:?}");
    assert!(params.gain.value() > 0.5, "dragging up raised the gain");
}

#[test]
fn a_key_step_is_one_bracket_and_the_late_pair_is_not_a_second_one() {
    let params = Arc::new(Synth::default());
    let (editor, mut h, log) = open(&params);
    lock(&editor.shared).ui.focus(crate::widget_id(10u32));
    h.step();
    for state in [KeyState::Down, KeyState::Up] {
        h.on_event_inner(&Event::Keyboard(KeyboardEvent {
            state,
            key: HostKey::Named(NamedKey::ArrowUp),
            modifiers: Modifiers::default(),
            ..KeyboardEvent::default()
        }));
    }
    for _ in 0..4 {
        h.step();
    }
    let c = calls(&log);
    assert!(
        matches!(c.as_slice(), [Call::Begin(10), Call::Set(10, v), Call::End(10)] if *v > 0.5),
        "{c:?}"
    );
}

#[test]
fn a_toggle_click_sets_the_bool_inside_its_press_release_bracket() {
    let params = Arc::new(Synth::default());
    let (editor, mut h, log) = open(&params);
    at(&mut h, centre(&editor, 30));
    press(&mut h, true);
    press(&mut h, false);
    for _ in 0..3 {
        h.step();
    }
    let c = calls(&log);
    assert_eq!(c, [Call::Begin(30), Call::Set(30, 1.0), Call::End(30)]);
    assert!(params.bypass.value());
}

#[test]
fn a_discrete_drag_sends_only_whole_steps_and_accumulates_between_them() {
    let params = Arc::new(Synth::default());
    let (editor, mut h, log) = open(&params);
    let p = centre(&editor, 20);
    at(&mut h, p);
    press(&mut h, true);
    h.step();
    // 1 px at a time: each move alone is under one of the seven steps.
    for dy in 1..=40 {
        at(&mut h, Point::new(p.x, p.y - f64::from(dy)));
        h.step();
    }
    press(&mut h, false);
    h.step();
    h.step();
    let c = calls(&log);
    let sets: Vec<f64> = c
        .iter()
        .filter_map(|c| match c {
            Call::Set(20, v) => Some(*v),
            _ => None,
        })
        .collect();
    assert!(
        !sets.is_empty(),
        "small moves accumulated into a step: {c:?}"
    );
    for v in sets {
        let steps = v * 7.0;
        assert!((steps - steps.round()).abs() < 1e-9, "{v} is a whole step");
    }
    assert!(bracketed(&c, 20), "{c:?}");
}

#[test]
fn closing_mid_drag_ends_the_gesture_without_another_frame() {
    let params = Arc::new(Synth::default());
    let (mut editor, mut h, log) = open(&params);
    at(&mut h, centre(&editor, 10));
    press(&mut h, true);
    // The press frame delivers the edge; the next tree dispatches it.
    h.step();
    h.step();
    assert_eq!(calls(&log), [Call::Begin(10)]);
    editor.close();
    assert_eq!(calls(&log), [Call::End(10)]);
    editor.close();
    assert!(calls(&log).is_empty(), "a second close owes nothing");
}

#[test]
fn a_state_load_ends_the_gesture_in_flight() {
    let params = Arc::new(Synth::default());
    let (mut editor, mut h, log) = open(&params);
    let p = centre(&editor, 10);
    at(&mut h, p);
    press(&mut h, true);
    h.step();
    h.step();
    editor.state_changed();
    assert_eq!(calls(&log), [Call::Begin(10), Call::End(10)]);
    // A button still held re-captures as a fresh press: whatever follows is
    // its own bracket, never a value outside one.
    at(&mut h, Point::new(p.x, p.y - 30.0));
    h.step();
    at(&mut h, Point::new(p.x, p.y - 60.0));
    h.step();
    press(&mut h, false);
    h.step();
    h.step();
    let c = calls(&log);
    assert!(bracketed(&c, 10), "{c:?}");
}

#[test]
fn a_control_that_leaves_the_tree_mid_drag_ends_its_gesture() {
    let params = Arc::new(Synth::default());
    let hidden = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hide = hidden.clone();
    let editor = MuiEditor::new(
        params.clone(),
        Ui::default(),
        (400, 300),
        move |ui, bridge| {
            if hide.load(std::sync::atomic::Ordering::Relaxed) {
                return row([]);
            }
            bridge.bind(ui, 10u32, |ui, id, v| knob(ui, id, "Gain", v, 0.0..=1.0))
        },
    );
    let (editor, mut h, log) = open_with(&params, editor);
    let p = centre(&editor, 10);
    at(&mut h, p);
    press(&mut h, true);
    h.step();
    at(&mut h, Point::new(p.x, p.y - 30.0));
    h.step();
    h.step();
    let c = calls(&log);
    assert!(
        matches!(c.as_slice(), [Call::Begin(10), .., Call::Set(10, _)]),
        "{c:?}"
    );
    hidden.store(true, std::sync::atomic::Ordering::Relaxed);
    editor.requests.redraw();
    h.step();
    assert_eq!(calls(&log), [Call::End(10)]);
    let s = lock(&editor.shared);
    assert_eq!(
        s.view.bridge.value(10u32).to_bits(),
        params.get_normalized(10).unwrap().to_bits(),
        "the store's value, not the drag's"
    );
}

#[test]
fn one_parameter_bound_twice_still_paints_and_brackets_by_parameter() {
    let params = Arc::new(Synth::default());
    let editor = MuiEditor::new(params.clone(), Ui::default(), (400, 300), |ui, bridge| {
        let a = bridge.bind(ui, 10u32, |ui, id, v| knob(ui, id, "Gain", v, 0.0..=1.0));
        let field = crate::widget_id(10u32).field("fine");
        let b = bridge.bind_as(ui, 10u32, field, |ui, id, v| {
            knob(ui, id, "Fine", v, 0.0..=1.0)
        });
        row([a, b])
    });
    let (editor, mut h, log) = open_with(&params, editor);
    let fine = {
        let s = lock(&editor.shared);
        let scene = s.ui.scene().expect("the tree resolved");
        let f = scene.surface("param/10/fine").expect("both controls").frame;
        Point::new(f.x + f.size.width / 2.0, f.y + f.size.height / 2.0)
    };
    at(&mut h, fine);
    press(&mut h, true);
    h.step();
    at(&mut h, Point::new(fine.x, fine.y - 30.0));
    h.step();
    press(&mut h, false);
    h.step();
    h.step();
    let c = calls(&log);
    assert!(
        matches!(
            c.as_slice(),
            [Call::Begin(10), .., Call::Set(10, _), Call::End(10)]
        ),
        "{c:?}"
    );
}

#[test]
fn host_automation_wakes_an_idle_editor_and_is_what_the_tree_reads() {
    let params = Arc::new(Synth::default());
    let (_editor, mut h, log) = open(&params);
    for _ in 0..600 {
        h.step();
    }
    assert!(!h.step(), "settled");
    params.set_normalized(10, 0.9);
    assert!(h.step(), "automation is a frame");
    assert!(
        calls(&log).is_empty(),
        "reading automation sends nothing back"
    );
}

#[test]
fn a_read_only_parameter_never_reaches_the_host() {
    #[derive(Params)]
    struct Meter {
        #[param(id = 1, name = "Level", default = 0.0, flags = "readonly")]
        level: FloatParam,
    }
    let params = Arc::new(Meter::default());
    let (context, log) = {
        let log = Log::default();
        let l = log.clone();
        let bridge = ClosureBridge {
            begin_edit: Box::new(move |id| l.lock().unwrap().push(Call::Begin(id))),
            end_edit: Box::new(|_| {}),
            set_param: Box::new(|_, _| {}),
            get_param: Box::new(|_| 0.0),
            get_param_plain: Box::new(|_| 0.0),
            format_param: Box::new(|_| String::new()),
            request_resize: Box::new(|_, _| false),
            get_meter: Box::new(|_| 0.0),
            get_state: Box::new(Vec::new),
            set_state: Box::new(|_| {}),
            transport: Box::new(|| None),
        };
        (
            PluginContext::from_closures(bridge, params.clone() as Arc<dyn Params>),
            log,
        )
    };
    let mut bridge = Bridge::new(params.clone());
    bridge.attach(context.with_params(params));
    let mut ui = Ui::default();
    let el = bridge.bind(&mut ui, 1u32, |ui, id, v| {
        *v = 0.7;
        knob(ui, id, "Level", v, 0.0..=1.0)
    });
    ui.frame(el, None, mui::prelude::Input::default(), 0.0)
        .unwrap();
    assert!(log.lock().unwrap().is_empty());
}

#[test]
fn set_size_holds_the_floor_and_a_fixed_editor_refuses() {
    let params = Arc::new(Synth::default());
    let mut fixed = editor(&params);
    assert!(!fixed.can_resize());
    assert!(!fixed.set_size(500, 400));
    let mut sized = editor(&params).resizable((200, 100));
    assert_eq!(sized.min_size(), (200, 100));
    assert!(!sized.set_size(199, 400));
    assert!(sized.set_size(200, 100));
    assert_eq!(sized.size(), (200, 100));
}

/// moose's CLAP wrapper builds a fresh editor per `gui.create` and a host
/// may set the scale only once: the next editor still opens at it.
#[test]
fn a_reopened_editor_keeps_the_host_scale() {
    let params = Arc::new(Synth::new());
    editor(&params).set_scale_factor(1.5);
    let reopened = editor(&params);
    assert_eq!(reopened.scale.get(), Some(1.5));
}

/// The host scale pins the window, never times the OS scale; macOS follows
/// its own backing scale.
#[test]
fn the_host_scale_is_the_window_scale_off_macos() {
    let mut scale = crate::HostScale::default();
    scale.set(1.25);
    let expected = (!cfg!(target_os = "macos")).then_some(1.25);
    assert_eq!(scale.policy(), expected);
}

#[test]
fn a_changed_callback_wakes_an_idle_editor() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let params = Arc::new(Synth::default());
    let dirty = Arc::new(AtomicBool::new(false));
    let d = dirty.clone();
    let editor = editor(&params).changed(move || d.swap(false, Ordering::Relaxed));
    let (_editor, mut h, _) = open_with(&params, editor);
    for _ in 0..600 {
        h.step();
    }
    assert!(!h.step(), "settled");
    dirty.store(true, Ordering::Relaxed);
    assert!(h.step(), "a meter outside the params is a frame");
    assert!(!h.step(), "and only one");
}

/// The zoom a handler over `editor` settles at, `physical` pixels at `scale`.
fn zoom_at(editor: &MuiEditor<Synth>, physical: (u32, u32), scale: f64) -> f64 {
    let mut h = Handler::new(
        editor.shared.clone(),
        editor.requests.clone(),
        physical,
        scale,
    );
    h.step();
    h.driver.ui_scale() / scale
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "the zooms are exact ratios of whole sizes"
)]
fn a_resized_window_fits_the_design_size_unless_the_zoom_is_fixed() {
    let params = Arc::new(Synth::default());
    let fit = editor(&params).resizable((200, 150));
    assert_eq!(zoom_at(&fit, (400, 300), 1.0), 1.0, "design size");
    assert_eq!(
        zoom_at(&fit, (600, 450), 1.5),
        1.0,
        "design size, host scale"
    );
    assert_eq!(zoom_at(&fit, (800, 600), 1.0), 2.0);
    assert_eq!(zoom_at(&fit, (1600, 900), 2.0), 1.5, "the tighter axis");
    let fixed = editor(&params).resizable((200, 150)).fixed_zoom();
    assert_eq!(zoom_at(&fixed, (800, 600), 1.0), 1.0);
}

#[test]
fn native_file_drop_reaches_editor_callback() {
    let params = Arc::new(Synth::new());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let record = calls.clone();
    let editor = editor(&params).on_files(move |_, at, paths, dropped| {
        record.lock().unwrap().push((at, paths.to_vec(), dropped));
        true
    });
    let shared = &mut *lock(&editor.shared);
    let paths = vec![std::path::PathBuf::from("instrument.nki")];
    assert!(shared.view.drop_files(&shared.ui, Point::new(10.,20.), &paths, false));
    assert!(shared.view.drop_files(&shared.ui, Point::new(10.,20.), &paths, true));
    assert_eq!(calls.lock().unwrap().len(), 2);
    assert!(calls.lock().unwrap()[1].2);
}

#[test]
fn cancellation_reaches_editor_callback() {
    let params=Arc::new(Synth::new());let called=Arc::new(std::sync::atomic::AtomicBool::new(false));let record=called.clone();
    let editor=editor(&params).on_cancel(move |_|record.store(true,std::sync::atomic::Ordering::Relaxed));
    let shared=&mut *lock(&editor.shared);shared.view.cancel(&shared.ui);assert!(called.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn closing_the_editor_cancels_the_apps_gestures() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let params = Arc::new(Synth::new());
    let called = Arc::new(AtomicBool::new(false));
    let record = called.clone();
    let editor = editor(&params).on_cancel(move |_| record.store(true, Ordering::Relaxed));
    let (mut editor, _h, _log) = open_with(&params, editor);
    editor.close();
    assert!(called.load(Ordering::Relaxed), "a held note is let go with the window");
}
