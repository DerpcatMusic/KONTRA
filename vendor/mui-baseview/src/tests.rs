//! Headless: the handler without a window or a GPU. The queue, schedule
//! and routing are tested in `mui::host`; these check the translation.
use super::*;
use keyboard_types::Code;
use mui::Ui;
use mui::prelude::{El, Input, knob};

/// A knob that claims Escape.
struct Knob {
    value: f64,
}

impl View for Knob {
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        knob(ui, "k", "K", &mut self.value, 0.0..=1.0).into()
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn claims_key(&self, key: &Key, _: Mods) -> bool {
        *key == Key::Escape
    }
}

fn handler(size: (u32, u32), scale: f64) -> Handler<Knob> {
    let shared = Arc::new(Mutex::new(Shared {
        ui: Ui::default(),
        view: Knob { value: 0.5 },
    }));
    Handler::new(shared, Arc::default(), size, scale)
}

fn key(key: HostKey, code: Code, state: KeyState, modifiers: Modifiers) -> Event {
    Event::Keyboard(KeyboardEvent {
        state,
        key,
        code,
        modifiers,
        ..KeyboardEvent::default()
    })
}

#[test]
fn baseview_events_reach_the_driver_in_its_terms() {
    let mut h = handler((640, 400), 1.0);
    h.resized(WindowSize::from_logical(
        LogicalSize::new(320.0, 200.0),
        2.0,
    ));
    assert_eq!((h.driver.size(), h.driver.ui_scale()), ((640, 400), 2.0));
    h.on_event_inner(&Event::Mouse(MouseEvent::CursorMoved {
        // baseview's pointer is in pixels.
        position: PhysicalPosition::new(20.0, 40.0),
        modifiers: Modifiers::default(),
    }));
    assert_eq!(h.driver.pointer().pos, Some(Point::new(10.0, 20.0)));
    // X11 samples a press's state before the modifier is set...
    let alt = |state, m| key(HostKey::Named(NamedKey::Alt), Code::AltLeft, state, m);
    h.on_event_inner(&alt(KeyState::Down, Modifiers::default()));
    assert!(h.driver.pointer().mods.alt);
    // ...and a release's while it is still held.
    h.on_event_inner(&alt(KeyState::Up, Modifiers::ALT));
    assert!(!h.driver.pointer().mods.alt);

    h.step();
    let space = key(
        HostKey::Character(" ".into()),
        Code::Space,
        KeyState::Down,
        Modifiers::default(),
    );
    assert_eq!(h.on_event_inner(&space), EventStatus::Ignored);
    let escape = key(
        HostKey::Named(NamedKey::Escape),
        Code::Escape,
        KeyState::Down,
        Modifiers::default(),
    );
    assert_eq!(h.on_event_inner(&escape), EventStatus::Captured);
    lock(&h.shared).ui.focus("k");
    let up = key(
        HostKey::Named(NamedKey::ArrowUp),
        Code::ArrowUp,
        KeyState::Down,
        Modifiers::default(),
    );
    assert_eq!(h.on_event_inner(&up), EventStatus::Captured);
}

#[test]
fn a_redraw_request_from_the_host_thread_is_a_frame() {
    let mut h = handler((400, 300), 1.0);
    assert!(h.step(), "the first tick paints");
    for _ in 0..600 {
        h.step();
    }
    assert!(!h.step(), "a settled window paints nothing");
    h.requests.redraw();
    assert!(h.step());
}
