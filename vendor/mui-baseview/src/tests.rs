//! Headless: the handler without a window or a GPU. The queue, schedule
//! and routing are tested in `mui::host`; these check the translation.
use super::*;
use keyboard_types::Code;
use mui::Ui;
use mui::prelude::{El, Input, knob};

#[cfg(target_os = "linux")]
#[test]
fn native_window_attempt_is_logged_before_parent_conversion_panics() {
    use raw_window_handle::{HandleError, WindowHandle, XcbWindowHandle};
    struct Parent(Cell<bool>);
    impl HasWindowHandle for Parent {
        #[expect(unsafe_code, reason = "synthetic numeric handle is never passed to native code")]
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            if self.0.replace(true) { panic!("parent conversion probe"); }
            let raw = XcbWindowHandle::new(std::num::NonZeroU32::new(1).unwrap());
            // SAFETY: only the handle kind is inspected; the next extraction
            // panics before baseview or graphics can use the numeric handle.
            Ok(unsafe { WindowHandle::borrow_raw(raw.into()) })
        }
    }
    struct Logged(Arc<Mutex<Vec<String>>>);
    impl View for Logged {
        fn log(&mut self, line: &str) { self.0.lock().unwrap().push(line.into()); }
        fn build(&mut self, ui: &mut Ui, input: &Input) -> El { Knob { value: 0.5 }.build(ui, input) }
        fn changed(&mut self) -> bool { false }
        fn request_resize(&mut self, _: u32, _: u32) -> bool { false }
    }
    let lines = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::new(Mutex::new(Shared { ui: Ui::default(), view: Logged(Arc::clone(&lines)) }));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        open(&Parent(Cell::new(false)), "probe", (400, 300), None, shared, Arc::default())
    }));
    assert!(result.is_err());
    let lines = lines.lock().unwrap();
    let first = &lines[0];
    assert!(first.starts_with("mui-baseview: native window init entering native code "));
    for field in ["os=linux", "api=X11/Xcb", "thread=ThreadId(", "main_thread=None", "backend=", "logical_size=(400, 300)"] {
        assert!(first.contains(field), "missing {field}: {first}");
    }
    assert!(!first.contains("0x"));
    assert!(lines.iter().all(|line| !line.starts_with("mui-baseview: GPU init ")));
}

#[test]
fn uncaptured_gpu_diagnostics_reach_the_sink_without_the_model_lock() {
    let h = handler((400, 300), 1.0);
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&seen);
    let hook: LogHook = Arc::new(Mutex::new(move |line: &str| sink.lock().unwrap().push(line.into())));
    h.requests.on_log(hook);
    let hook = h.requests.log.lock().unwrap().clone().unwrap();
    // wgpu may call synchronously or from another thread. Holding the
    // model must not prevent the independent diagnostic callback.
    let _model = lock(&h.shared);
    std::thread::spawn(move || report_gpu_error(&hook, std::io::Error::other("surface validation probe"))).join().unwrap();
    assert_eq!(*seen.lock().unwrap(), ["mui-baseview: GPU failed (uncaptured error: surface validation probe)"]);
}

#[test]
fn gpu_diagnostics_do_not_reenter_a_busy_sink_and_recover_poison() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&seen);
    let hook: LogHook = Arc::new(Mutex::new(move |line: &str| sink.lock().unwrap().push(line.into())));
    let busy = hook.lock().unwrap();
    report_gpu_error(&hook, "nested GPU probe");
    assert!(seen.lock().unwrap().is_empty());
    drop(busy);
    let poison = Arc::clone(&hook);
    assert!(std::thread::spawn(move || {
        let _sink = poison.lock().unwrap();
        panic!("sink poison probe");
    }).join().is_err());
    report_gpu_error(&hook, "recovered GPU probe");
    assert_eq!(*seen.lock().unwrap(), ["mui-baseview: GPU failed (uncaptured error: recovered GPU probe)"]);
    let panicking: LogHook = Arc::new(Mutex::new(|_: &str| panic!("sink callback probe")));
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| report_gpu_error(&panicking, "GPU cause survives sink panic"))).is_ok());
}

#[test]
fn a_caught_window_panic_keeps_the_handler_usable() {
    let mut h = handler((400, 300), 1.0);
    let caught = guard(&mut h, |h| {
        let _model = lock(&h.shared);
        panic!("first-frame layout probe");
    });
    assert!(caught.is_none());
    assert_eq!(guard(&mut h, |_| 7), Some(7));
    assert!(h.step());
}

#[test]
fn gpu_startup_panic_keeps_the_backend_cause() {
    assert_eq!(gpu_panic_reason(&"Vulkan loader unavailable"),
        "panic while creating GPU resources: Vulkan loader unavailable");
    assert_eq!(gpu_panic_reason(&String::from("shader validation failed")),
        "panic while creating GPU resources: shader validation failed");
    assert_eq!(gpu_panic_reason(&42_u32),
        "panic while creating GPU resources: non-string panic payload");
}

#[cfg(target_os = "linux")]
#[test]
fn linux_embedding_identifies_x11_and_rejects_wayland_without_a_raw_handle_dump() {
    use raw_window_handle::{RawWindowHandle, WaylandWindowHandle, XcbWindowHandle, XlibWindowHandle};
    assert_eq!(linux_parent_api(XlibWindowHandle::new(1).into()), Ok("X11/Xlib"));
    assert_eq!(linux_parent_api(XcbWindowHandle::new(std::num::NonZeroU32::new(1).unwrap()).into()), Ok("X11/Xcb"));
    let handle = WaylandWindowHandle::new(std::ptr::NonNull::dangling());
    let reason = linux_parent_api(RawWindowHandle::Wayland(handle)).unwrap_err();
    assert!(reason.contains("XWayland"));
    assert!(!reason.contains("0x"));
}

#[test]
fn windows_gpu_defaults_to_dx12_and_explicit_backend_choices_stay_authoritative() {
    use wgpu::Backends as B;
    assert_eq!(gpu_backends(true, None), B::DX12);
    assert!(!gpu_backends(true, None).contains(B::VULKAN));
    for requested in [B::DX12, B::VULKAN, B::GL, B::DX12 | B::VULKAN, B::empty()] {
        assert_eq!(gpu_backends(true, Some(requested)), requested);
        assert_eq!(gpu_backends(false, Some(requested)), requested);
    }
    assert_eq!(gpu_backends(false, None), B::all());
}

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

#[test]
fn leaving_before_the_release_frame_cancels_pointer_restoration() {
    let at = PhysicalPosition::new(20., 40.);
    let moved = Event::Mouse(MouseEvent::CursorMoved { position: at, modifiers: Modifiers::default() });
    let button = |down| Event::Mouse(if down {
        MouseEvent::ButtonPressed { button: MouseButton::Left, modifiers: Modifiers::default() }
    } else {
        MouseEvent::ButtonReleased { button: MouseButton::Left, modifiers: Modifiers::default() }
    });
    for left in [Event::Mouse(MouseEvent::CursorLeft), Event::Mouse(MouseEvent::DragLeft), Event::Window(WindowEvent::Unfocused)] {
        for release_first in [false, true] {
            let mut h = handler((640, 400), 1.);
            h.step();
            h.on_event_inner(&moved);
            h.on_event_inner(&button(true));
            h.step();
            // The native hide hook records this during a dragged frame.
            h.hidden_at = Some(at);
            if release_first { h.on_event_inner(&button(false)); }
            h.on_event_inner(&left);
            if !release_first { h.on_event_inner(&button(false)); }
            assert!(h.pointer_at.is_none() && h.hidden_at.is_none(), "leave/focus loss cancels the pending warp before painting release");
            assert!(h.driver.pointer().pos.is_none(), "the next frame sees the pointer leave");
            h.step();
            assert!(h.hidden_at.is_none(), "a delayed frame cannot restore the old drag origin");
        }
    }
    let mut h = handler((640, 400), 1.);
    h.on_event_inner(&moved);
    h.on_event_inner(&button(true));
    h.hidden_at = Some(at);
    h.on_event_inner(&button(false));
    assert_eq!(h.hidden_at, Some(at), "a release inside still restores the intentional drag origin");
    assert_eq!(h.pointer_at, Some(at));
}
