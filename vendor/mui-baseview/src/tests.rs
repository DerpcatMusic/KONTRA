//! Event translation checks and an opt-in native presentation regression.
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

/// Run with an X11 display and compute-capable EGL driver:
/// `WGPU_BACKEND=gl cargo test -p mui-baseview native_surface_presents_and_reopens -- --ignored`
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a live X11 display and graphics driver"]
fn native_surface_presents_and_reopens() {
    use std::sync::mpsc::{Sender, channel};

    struct Probe {
        // Drop graphics before the native context, as the production handler does.
        gpu: RefCell<Option<Host>>,
        cx: WindowContext,
        result: RefCell<Option<Sender<Result<(), String>>>>,
    }
    impl WindowHandler for Probe {
        fn on_frame(&self) -> Result<(), HandlerError> {
            if let Some(send) = self.result.borrow_mut().take() {
                let result = (|| {
                    let mut gpu = open_gpu(&self.cx, (240, 200), |_| {})?;
                    let mut h = handler((240, 200), 1.0);
                    h.step();
                    let scene = lock(&h.shared).ui.scene_snapshot().ok_or("no scene")?;
                    for replacement in 0..3 {
                        if replacement != 0 {
                            // SAFETY: cx outlives gpu and the replacement surface.
                            #[expect(unsafe_code, reason = "exercises native surface recovery")]
                            let surface = unsafe { surface::create(gpu.instance(), &self.cx) }
                                .map_err(|e| format!("surface recreation failed: {e}"))?;
                            gpu.try_replace_surface(surface)
                                .map_err(|e| format!("replacement {replacement}: {e}"))?;
                            assert_eq!(gpu.generation(), replacement);
                        }
                        if !matches!(
                            gpu.present(&scene, Affine::IDENTITY)
                                .map_err(|e| e.to_string())?,
                            Frame::Presented(_)
                        ) {
                            return Err(format!(
                                "frame after replacement {replacement} was not presented"
                            ));
                        }
                    }
                    *self.gpu.borrow_mut() = Some(gpu);
                    Ok(())
                })();
                let _ = send.send(result);
            }
            Ok(())
        }
        fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
            Ok(())
        }
        fn on_event(&self, _: Event) -> EventStatus {
            EventStatus::Ignored
        }
    }

    for (parented, map_first) in [(false, false), (true, true), (true, false)] {
        for _ in 0..2 {
            let parent = parented.then(|| {
                let (send, recv) = channel();
                let window =
                    Window::create(settings("MUI regression parent", (240, 200)), move |cx| {
                        send.send(cx.platform_handle()).expect("parent handle");
                        Ok(Probe {
                            gpu: RefCell::new(None),
                            cx,
                            result: RefCell::new(None),
                        })
                    })
                    .expect("parent creation");
                let handle = recv
                    .recv_timeout(Duration::from_secs(30))
                    .expect("parent handle");
                if map_first {
                    window.show().expect("parent mapping before attachment");
                }
                (window, handle)
            });
            let mut options = settings("MUI presentation regression", (240, 200));
            if let Some((_, handle)) = &parent {
                options = options.with_parent(handle);
            }
            let (send, recv) = channel();
            let window = Window::create(options, |cx| {
                Ok(Probe {
                    gpu: RefCell::new(None),
                    cx,
                    result: RefCell::new(Some(send)),
                })
            })
            .expect("native window creation");
            window.show().expect("native window mapping");
            if let Some((window, _)) = &parent
                && !map_first
            {
                window.show().expect("parent mapping after attachment");
            }
            let result = recv.recv_timeout(Duration::from_secs(30));
            window.close();
            result
                .expect("first frame callback")
                .expect("native presentation");
        }
    }
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

#[test]
fn a_key_hook_hears_downs_and_ups_first_and_can_take_them() {
    let mut h = handler((400, 300), 1.0);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&heard);
    h.requests
        .on_key(Arc::new(Mutex::new(move |_: &Ui, e: &KeyEvent| {
            log.lock().unwrap().push((e.key.clone(), e.down));
            // Takes "a" only.
            e.key == NativeKey::Text("a".into())
        })));
    h.step();
    let a = |state| {
        key(
            HostKey::Character("a".into()),
            Code::KeyA,
            state,
            Modifiers::default(),
        )
    };
    assert_eq!(h.on_event_inner(&a(KeyState::Down)), EventStatus::Captured);
    assert_eq!(h.on_event_inner(&a(KeyState::Up)), EventStatus::Captured);
    // Not taken: MUI routes it as before, and the knob claims Escape.
    let escape = key(
        HostKey::Named(NamedKey::Escape),
        Code::Escape,
        KeyState::Down,
        Modifiers::default(),
    );
    assert_eq!(h.on_event_inner(&escape), EventStatus::Captured);
    let space = key(
        HostKey::Character(" ".into()),
        Code::Space,
        KeyState::Down,
        Modifiers::default(),
    );
    assert_eq!(h.on_event_inner(&space), EventStatus::Ignored);
    assert_eq!(
        *heard.lock().unwrap(),
        [
            (NativeKey::Text("a".into()), true),
            (NativeKey::Text("a".into()), false),
            (NativeKey::Named(Key::Escape), true),
            (NativeKey::Text(" ".into()), true),
        ]
    );
}

#[test]
fn ime_configuration_converts_geometry_and_keeps_native_text_ranges() {
    let source = mui::host::ImeConfiguration {
        id: "field".into(),
        area: (Point::new(10.0, 20.0), mui::scene::Size::new(1.0, 16.0)),
        text: "a😀é".into(),
        selection: 1..5,
        marked: Some(1..5),
    };
    let a = native_ime(source.clone(), 1.5);
    assert_eq!(a.position, PhysicalPosition::new(15.0, 30.0));
    assert_eq!(a.size, baseview::dpi::PhysicalSize::new(1.5, 24.0));
    assert_eq!((a.selection.clone(), a.marked.clone()), (1..5, Some(1..5)));
    assert_eq!(a, native_ime(source.clone(), 1.5));
    assert_ne!(
        a,
        native_ime(source, 2.0),
        "DPI changes update candidate placement"
    );
}

#[test]
fn native_composition_reaches_the_driver_once() {
    struct InputSpy {
        seen: Vec<mui::prelude::Ime>,
        text: String,
    }
    impl View for InputSpy {
        fn build(&mut self, _: &mut Ui, input: &Input) -> El {
            self.seen.extend(input.ime.clone());
            self.text.push_str(&input.text);
            mui::prelude::block(20.0, 20.0)
        }
        fn changed(&mut self) -> bool {
            false
        }
        fn request_resize(&mut self, _: u32, _: u32) -> bool {
            false
        }
    }
    let shared = Arc::new(Mutex::new(Shared {
        ui: Ui::default(),
        view: InputSpy {
            seen: Vec::new(),
            text: String::new(),
        },
    }));
    let mut h = Handler::new(Arc::clone(&shared), Arc::default(), (200, 100), 1.0);
    h.step();
    for event in [
        baseview::Ime::Enabled,
        baseview::Ime::Selection(1..5),
        baseview::Ime::Preedit {
            text: "日本".into(),
            cursor: Some((6, 6)),
        },
        baseview::Ime::Commit("日本".into()),
        baseview::Ime::Disabled,
    ] {
        assert_eq!(h.on_event_inner(&Event::Ime(event)), EventStatus::Captured);
    }
    h.step();
    let s = lock(&shared);
    assert_eq!(s.view.seen.len(), 5);
    assert!(matches!(&s.view.seen[3], mui::prelude::Ime::Commit(s) if s == "日本"));
    assert!(
        s.view.text.is_empty(),
        "composition has its own channel, never duplicated as typed text"
    );
}

#[test]
fn scene_snapshot_does_not_hold_the_model_lock() {
    let mut h = handler((640, 400), 1.0);
    h.step();
    let snapshot = lock(&h.shared).ui.scene_snapshot().unwrap();
    let mut model = h.shared.try_lock().expect("native callback can reenter");
    assert!(Arc::ptr_eq(&snapshot, &model.ui.scene_snapshot().unwrap()));
    model.ui.blur();
    assert!(snapshot.surface("k").is_some());
}

#[test]
fn queued_native_callbacks_can_reenter_and_preserve_event_order() {
    let queue = RefCell::new(VecDeque::from([1, 2]));
    let mut delivered = Vec::new();
    drain_events(&queue, |event| {
        delivered.push(event);
        if event == 1 {
            queue.borrow_mut().push_back(3);
        }
    });
    assert_eq!(delivered, [1, 2, 3]);
    assert!(queue.borrow().is_empty());
}

#[test]
fn app_gpu_error_observation_preserves_other_observers_and_resets_on_rebuild() {
    let mut generation = None;
    let mut app_cursor = 0;
    let mut core_cursor = 0;
    let observe = |cursor: &mut u64| {
        if *cursor == 1 { return None; }
        *cursor = 1;
        Some("uncaptured validation error".to_string())
    };
    assert!(observe_generation_error(&mut generation, &mut app_cursor, 4, observe).is_some());
    assert!(observe_generation_error(&mut generation, &mut app_cursor, 4, observe).is_none());
    assert!(observe(&mut core_cursor).is_some(), "the app must not consume Host's error record");
    assert!(observe_generation_error(&mut generation, &mut app_cursor, 5, observe).is_some(), "a new device may reuse the same error sequence");
}
