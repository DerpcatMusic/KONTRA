//! Event translation checks and an opt-in native presentation regression.
use super::*;
use keyboard_types::Code;
use mui::Ui;
use mui::prelude::{El, Input, knob};

#[test]
fn close_cycles_release_gui_thread_render_trees_before_model_drop() {
    struct Memo(std::sync::Weak<Vec<u8>>);
    impl View for Memo {
        fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
            ui.memo("cycle-canvas", 0, |_| {
                let art = Arc::new(vec![0u8; 8 << 20]);
                self.0 = Arc::downgrade(&art);
                mui::prelude::canvas(move |_| {
                    std::hint::black_box(&art);
                    Vec::new()
                }).w(64.).h(64.)
            })
        }
        fn changed(&mut self) -> bool { false }
        fn request_resize(&mut self, _: u32, _: u32) -> bool { false }
    }
    let mut owners = Vec::new();
    let mut retained = Vec::new();
    for _ in 0..4 {
        let shared = Arc::new(Mutex::new(Shared { ui: Ui::default(), view: Memo(Default::default()) }));
        let mut h = Handler::new(shared.clone(), Arc::default(), (64, 64), 1.);
        h.step();
        let owner = lock(&shared).view.0.clone();
        assert!(owner.upgrade().is_some(), "opened canvas must own its art");
        h.on_event_inner(&Event::Window(WindowEvent::WillClose));
        drop(h);
        assert!(owner.upgrade().is_none(), "close releases the tree while the model survives");
        let mut reopened = Handler::new(shared.clone(), Arc::default(), (64, 64), 1.);
        reopened.step();
        let reopened_owner = lock(&shared).view.0.clone();
        assert!(reopened_owner.upgrade().is_some(), "reopen must rebuild its canvas");
        reopened.on_event_inner(&Event::Window(WindowEvent::WillClose));
        drop(reopened);
        assert!(reopened_owner.upgrade().is_none(), "reopened canvas releases on close");
        std::thread::spawn(move || drop(shared)).join().unwrap();
        owners.push(owner);
        owners.push(reopened_owner);
        retained.push(owners.iter().filter_map(|owner| owner.upgrade()).map(|art| art.len()).sum::<usize>());
    }
    assert_eq!(retained, [0; 4], "closed render bytes must remain flat at zero");
}

#[test]
fn native_accessibility_stays_on_the_window_thread() {
    static_assertions::assert_not_impl_any!(NativeAccessibility: Send, Sync);
    static_assertions::assert_not_impl_any!(A11y: Send, Sync);
    static_assertions::assert_impl_all!(AccessibilityUi: Send);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires an isolated X11 display; run under Xvfb"]
fn native_accessibility_pre_show_close_and_reopen_release_the_model() {
    let shared = Arc::clone(&handler((240, 200), 1.0).shared);
    let requests = Arc::new(Requests::default());
    let model = Arc::downgrade(&shared);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&seen);
    requests.on_x11_window(Arc::new(Mutex::new(move |parent: Option<u32>| {
        let model = model.upgrade().expect("test model is still alive");
        assert!(model.try_lock().is_ok(), "native parent callbacks hold no model lock");
        observed.lock().unwrap().push(parent.is_some());
    })));
    for _ in 0..2 {
        let make = build(Arc::clone(&shared), Arc::clone(&requests), false);
        let window = Window::create(settings("KONTRA native bridge lifecycle", (240, 200)), move |cx| {
            let adapter = make(cx)?;
            assert!(adapter.handler.borrow().a11y.is_some(), "attach the shared native provider before show");
            Ok(adapter)
        }).expect("native window creation");
        assert!(requests.x11_window().is_some());
        // Close before explicit show, then reuse the portable model/requests.
        // Native callbacks may warm the renderer before mapping the window.
        window.close();
        assert_eq!(requests.x11_window(), None);
        assert_eq!(Arc::strong_count(&shared), 1, "closed native endpoints retain no model");
    }
    assert_eq!(*seen.lock().unwrap(), [true, false, true, false]);
}

#[cfg(target_os = "linux")]
#[test]
fn dialog_parent_closes_outside_model_lock_and_preserves_reopen() {
    let mut old = handler((240, 200), 1.0);
    let model = Arc::clone(&old.shared);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&seen);
    old.requests.on_x11_window(Arc::new(Mutex::new(move |parent| {
        assert!(model.try_lock().is_ok(), "native parent callbacks never hold the model");
        observed.lock().unwrap().push(parent);
    })));
    assert_eq!(old.requests.x11_window(), None);
    old.x11_window = 42;
    old.requests.x11_window.store(42, Ordering::Release);
    old.requests.notify_x11_window();
    old.on_event_inner(&Event::Window(WindowEvent::WillClose));
    assert_eq!(old.requests.x11_window(), None);
    let requests = Arc::clone(&old.requests);
    requests.x11_window.store(43, Ordering::Release);
    drop(old);
    assert_eq!(requests.x11_window(), Some(43));
    let mut current = handler((240, 200), 1.0);
    current.requests = Arc::clone(&requests);
    current.x11_window = 43;
    drop(current);
    assert_eq!(requests.x11_window(), None);
    assert_eq!(*seen.lock().unwrap(), [Some(42), None, None]);
}

#[cfg(target_os = "linux")]
#[test]
fn native_parent_hook_panic_cannot_unwind_creation_or_teardown() {
    let mut h = handler((240, 200), 1.0);
    let requests = Arc::clone(&h.requests);
    let lines = Arc::new(Mutex::new(Vec::new()));
    let logged = Arc::clone(&lines);
    requests.on_log(Arc::new(Mutex::new(move |line: &str| logged.lock().unwrap().push(line.to_owned()))));
    requests.on_x11_window(Arc::new(Mutex::new(|_| panic!("parent hook probe"))));
    h.x11_window = 42;
    requests.x11_window.store(42, Ordering::Release);
    requests.notify_x11_window();
    drop(h);
    assert_eq!(requests.x11_window(), None);
    assert_eq!(lines.lock().unwrap().len(), 2);
    assert!(lines.lock().unwrap().iter().all(|line| line.contains("parent hook probe")));
}

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

#[test]
fn window_panic_recovery_preserves_state_and_stops_the_poison_storm() {
    let mut h = handler((240, 200), 1.0);
    assert!(guard(&mut h, |h| {
        let mut state = lock(&h.shared);
        state.view.value = 0.75;
        panic!("synthetic original frame fault");
    }).is_none());
    assert!(!h.shared.is_poisoned(), "guard logging recovers the outer model lock");
    for _ in 0..64 {
        assert!(guard(&mut h, |h| h.step()).is_some());
    }
    assert_eq!(lock(&h.shared).view.value, 0.75);
    assert!(h.last_panic.is_none());
    assert!(lock(&h.shared).ui.scene().is_some(), "retry produces a scene without reopening the editor");
}

#[test]
fn repeated_window_faults_log_once_until_a_successful_callback() {
    struct Logged(Arc<Mutex<Vec<String>>>);
    impl View for Logged {
        fn log(&mut self, line: &str) { self.0.lock().unwrap().push(line.into()); }
        fn build(&mut self, _: &mut Ui, _: &Input) -> El { mui::prelude::block(100., 100.).into() }
        fn changed(&mut self) -> bool { false }
        fn request_resize(&mut self, _: u32, _: u32) -> bool { false }
    }
    let lines = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::new(Mutex::new(Shared { ui: Ui::default(), view: Logged(Arc::clone(&lines)) }));
    let mut h = Handler::new(shared, Arc::default(), (240, 200), 1.0);
    for _ in 0..64 {
        assert!(guard(&mut h, |_| panic!("synthetic persistent frame fault")).is_none());
        assert_eq!(h.last_panic.as_deref(), Some("synthetic persistent frame fault"));
    }
    assert_eq!(lines.lock().unwrap().len(), 1);
    assert!(guard(&mut h, |_| ()).is_some());
    assert!(h.last_panic.is_none());
    assert!(guard(&mut h, |_| panic!("a new frame fault")).is_none());
    assert_eq!(h.last_panic.as_deref(), Some("a new frame fault"));
    assert_eq!(lines.lock().unwrap().len(), 2);
}

/// WGPU_BACKEND=metal on Linux exercises automatic fallback. Otherwise this
/// forces CPU on the handler, without changing process environment in tests.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires an X11 display"]
fn native_cpu_fallback_presents_and_reopens() {
    cpu_presentation_fixture(false);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a live X11 display and graphics driver"]
fn native_resize_failure_falls_back_and_presents() {
    cpu_presentation_fixture(true);
}

#[cfg(target_os = "linux")]
fn cpu_presentation_fixture(resize_failure: bool) {
    use mui::prelude::*;
    use std::sync::mpsc::{Sender, channel};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};

    struct Green;
    impl View for Green {
        fn build(&mut self, _: &mut Ui, _: &Input) -> El {
            block(240., 200.).fill(Color::srgb(0., 1., 0.))
        }
        fn changed(&mut self) -> bool {
            false
        }
        fn request_resize(&mut self, _: u32, _: u32) -> bool {
            false
        }
    }
    struct Probe {
        handler: RefCell<Handler<Green>>,
        cx: WindowContext,
        phase: std::cell::Cell<u8>,
        exposed: std::cell::Cell<bool>,
        resize_failure: bool,
        result: RefCell<Option<Sender<Result<(), String>>>>,
    }
    impl WindowHandler for Probe {
        fn on_frame(&self) -> Result<(), HandlerError> {
            if self.phase.get() == 0 {
                let result = (|| {
                    let mut handler = self.handler.borrow_mut();
                    handler.tick(&self.cx, &mut None);
                    if self.resize_failure {
                        if handler.unpainted || handler.software.is_some() {
                            return Err("fixture did not establish a GPU frame".into());
                        }
                        handler.requests.resize(200, 160);
                        handler.tick(&self.cx, &mut None);
                        let gpu = handler.gpu.as_ref().ok_or("missing initial GPU")?;
                        // Lose the device before X11 acknowledges the requested resize.
                        gpu.device().0.destroy();
                        if !gpu.device_lost() {
                            return Err("destroyed GPU device loss was not observed".into());
                        }
                    }
                    Ok(())
                })();
                if result.is_err() {
                    if let Some(send) = self.result.borrow_mut().take() {
                        let _ = send.send(result);
                    }
                    return Ok(());
                }
                // Baseview flushes its X11 connection after this callback.
                self.phase.set(if self.resize_failure { 4 } else { 1 });
                return Ok(());
            }
            if self.phase.get() == 4 {
                let mut handler = self.handler.borrow_mut();
                if handler.driver.size() != (200, 160) {
                    return Ok(());
                }
                handler.tick(&self.cx, &mut None);
                if (handler.gpu.is_some()
                    || handler.software.is_some()
                    || !handler.software_only
                    || !handler.unpainted)
                    && let Some(send) = self.result.borrow_mut().take()
                {
                    let _ = send.send(Err(format!(
                        "resize failure did not detach GPU: gpu={}, cpu={}, software_only={}, unpainted={}",
                        handler.gpu.is_some(),
                        handler.software.is_some(),
                        handler.software_only,
                        handler.unpainted,
                    )));
                }
                self.phase.set(1);
                return Ok(());
            }
            if self.phase.get() == 2 {
                if self.exposed.get() {
                    self.handler.borrow_mut().tick(&self.cx, &mut None);
                    self.phase.set(3);
                }
                return Ok(());
            }
            let Some(send) = self.result.borrow_mut().take() else {
                return Ok(());
            };
            let result = (|| {
                let mut handler = self.handler.borrow_mut();
                handler.tick(&self.cx, &mut None);
                if handler.gpu.is_some() || handler.software.is_none() || handler.unpainted {
                    return Err("expected a successfully presented CPU fallback".into());
                }
                let expected = if self.resize_failure {
                    (200, 160)
                } else {
                    (240, 200)
                };
                if !handler.software_only || !handler.cpu_presented {
                    return Err("CPU submission was not observed".into());
                }
                let window = handler.requests.x11_window().ok_or("no X11 window")?;
                let (connection, _) = x11rb::rust_connection::RustConnection::connect(None)
                    .map_err(|e| e.to_string())?;
                let visual_id = connection
                    .get_window_attributes(window)
                    .map_err(|e| e.to_string())?
                    .reply()
                    .map_err(|e| e.to_string())?
                    .visual;
                let visual = connection
                    .setup()
                    .roots
                    .iter()
                    .flat_map(|s| &s.allowed_depths)
                    .flat_map(|d| &d.visuals)
                    .find(|v| v.visual_id == visual_id)
                    .ok_or("missing visual")?;
                let geometry = connection
                    .get_geometry(window)
                    .map_err(|e| e.to_string())?
                    .reply()
                    .map_err(|e| e.to_string())?;
                if (u32::from(geometry.width), u32::from(geometry.height)) != expected {
                    return Err("native window did not use the recovered extent".into());
                }
                let depth = geometry.depth;
                let deadline = Instant::now() + Duration::from_secs(1);
                loop {
                    let pixels = connection
                        .get_image(
                            ImageFormat::Z_PIXMAP,
                            window,
                            20,
                            i16::try_from(expected.1 - 20).unwrap(),
                            1,
                            1,
                            u32::MAX,
                        )
                        .map_err(|e| e.to_string())?
                        .reply()
                        .map_err(|e| e.to_string())?;
                    let bytes: [u8; 4] = pixels
                        .data
                        .get(..4)
                        .ok_or("expected 32-bit native pixel")?
                        .try_into()
                        .unwrap();
                    let pixel = if connection.setup().image_byte_order
                        == x11rb::protocol::xproto::ImageOrder::LSB_FIRST
                    {
                        u32::from_le_bytes(bytes)
                    } else {
                        u32::from_be_bytes(bytes)
                    };
                    if pixel & visual.green_mask == visual.green_mask
                        && pixel & (visual.red_mask | visual.blue_mask) == 0
                    {
                        if depth == 32 && pixel >> 24 != 255 {
                            return Err(
                                "CPU window pixels are transparent to the compositor".into()
                            );
                        }
                        break;
                    }
                    if Instant::now() >= deadline {
                        return Err(format!("CPU pixel was {:?}", pixels.data));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                // A settled tick must not ask for another UI resolve/presentation.
                let frame = handler.driver.last_frame();
                handler.tick(&self.cx, &mut None);
                if handler.driver.last_frame() != frame || handler.unpainted {
                    return Err("idle CPU editor did work".into());
                }
                if self.phase.get() == 1 {
                    // Erase the native pixels and request an actual X11 Expose.
                    connection
                        .clear_area(true, window, 0, 0, 0, 0)
                        .map_err(|e| e.to_string())?
                        .check()
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            })();
            if result.is_ok() && self.phase.get() == 1 {
                self.phase.set(2);
                *self.result.borrow_mut() = Some(send);
                return Ok(());
            }
            let _ = send.send(result);
            Ok(())
        }
        fn resized(&self, size: WindowSize) -> Result<(), HandlerError> {
            if let Ok(mut h) = self.handler.try_borrow_mut() {
                h.resized(size);
            }
            Ok(())
        }
        fn on_event(&self, event: Event) -> EventStatus {
            if self.phase.get() == 2 && matches!(event, Event::Window(WindowEvent::RedrawRequested))
            {
                self.exposed.set(true);
            }
            self.handler.borrow_mut().on_event_inner(&event)
        }
    }
    for _ in 0..2 {
        let (send, recv) = channel();
        let shared = Arc::new(Mutex::new(Shared {
            ui: Ui::default(),
            view: Green,
        }));
        let window = Window::create(
            settings("MUI CPU regression", (240, 200))
                .with_resizable(false)
                .with_scale_factor_override(Some(1.0)),
            move |cx| {
                let requests = Arc::new(Requests::default());
                let mut handler = Handler::new(shared, requests, (240, 200), 1.0);
                handler.software_only = !resize_failure
                    && (handler.software_only
                        || std::env::var("WGPU_BACKEND").as_deref() != Ok("metal"));
                handler.x11_window = match cx.window_handle()?.as_raw() {
                    raw_window_handle::RawWindowHandle::Xlib(h) => h.window as u32,
                    raw_window_handle::RawWindowHandle::Xcb(h) => h.window.get(),
                    _ => unreachable!(),
                };
                handler
                    .requests
                    .x11_window
                    .store(handler.x11_window, Ordering::Release);
                Ok(Probe {
                    handler: RefCell::new(handler),
                    cx,
                    phase: std::cell::Cell::new(0),
                    exposed: std::cell::Cell::new(false),
                    resize_failure,
                    result: RefCell::new(Some(send)),
                })
            },
        )
        .expect("CPU window");
        window.show().unwrap();
        let result = recv.recv_timeout(Duration::from_secs(30));
        window.close();
        result
            .expect("CPU frame callback")
            .expect("CPU presentation");
    }
}

/// Run with an X11 display and compute-capable EGL driver:
/// `WGPU_BACKEND=gl cargo test -p kontra-native-host native_surface_presents_and_reopens -- --ignored`
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

#[test]
fn native_drag_leave_delivers_an_empty_file_gesture_to_the_owner() {
    struct DropSpy(Vec<(usize, bool)>);
    impl View for DropSpy {
        fn build(&mut self, _: &mut Ui, _: &Input) -> El { mui::prelude::block(100.,100.) }
        fn changed(&mut self) -> bool { false }
        fn request_resize(&mut self, _: u32, _: u32) -> bool { false }
        fn drop_files(&mut self, _: &Ui, _: mui::prelude::Point, paths: &[std::path::PathBuf], dropped: bool) -> bool {
            self.0.push((paths.len(),dropped));true
        }
    }
    let shared=Arc::new(Mutex::new(Shared {ui:Ui::default(),view:DropSpy(vec![])}));
    let mut h=Handler::new(shared.clone(),Arc::default(),(400,300),1.);
    let enter=Event::Mouse(MouseEvent::DragEntered {position:PhysicalPosition::new(50.,50.),modifiers:Modifiers::default(),data:DropData::Files(vec!["/tmp/owned.wav".into()])});
    h.on_event_inner(&enter);
    h.on_event_inner(&Event::Mouse(MouseEvent::DragLeft));
    assert_eq!(lock(&shared).view.0,[(1,false),(0,false)],"the owner must observe leave and clear native mouse-over");
    assert!(h.driver.pointer().pos.is_none());
}

/// Hosts may delete the drawable before asking the editor to close. The native
/// thread must terminate and release its handler without another frame/resize.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires an isolated X11 display"]
fn native_destroyed_parent_and_drawable_stop_callbacks() {
    use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle, XcbWindowHandle};
    use std::num::NonZeroU32;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::{Sender, channel};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, CreateWindowAux, WindowClass};

    struct Parent(NonZeroU32);
    impl HasWindowHandle for Parent {
        #[expect(unsafe_code, reason = "the fixture owns the native parent during child creation")]
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            // SAFETY: the fixture owns this parent throughout child creation.
            Ok(unsafe { WindowHandle::borrow_raw(XcbWindowHandle::new(self.0).into()) })
        }
    }
    struct Probe {
        callbacks: Arc<AtomicUsize>,
        closes: Arc<AtomicUsize>,
        first_frame: RefCell<Option<Sender<()>>>,
        dropped: Sender<()>,
    }
    impl WindowHandler for Probe {
        fn on_frame(&self) -> Result<(), HandlerError> {
            self.callbacks.fetch_add(1, Ordering::SeqCst);
            if let Some(send) = self.first_frame.borrow_mut().take() {
                let _ = send.send(());
            }
            Ok(())
        }
        fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
            self.callbacks.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn on_event(&self, event: Event) -> EventStatus {
            if matches!(event, Event::Window(WindowEvent::WillClose)) {
                self.closes.fetch_add(1, Ordering::SeqCst);
            }
            EventStatus::Ignored
        }
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            let _ = self.dropped.send(());
        }
    }

    let (conn, screen) = x11rb::rust_connection::RustConnection::connect(None).unwrap();
    for parented in [true, false] {
        let parent = conn.generate_id().unwrap();
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            parent,
            conn.setup().roots[screen].root,
            0,
            0,
            64,
            64,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )
        .unwrap()
        .check()
        .unwrap();
        conn.map_window(parent).unwrap().check().unwrap();
        let parent_handle = Parent(NonZeroU32::new(parent).unwrap());
        let settings = settings("MUI destroyed drawable regression", (64, 64));
        let settings = if parented {
            settings.with_parent(&parent_handle)
        } else {
            settings
        };
        let callbacks = Arc::new(AtomicUsize::new(0));
        let closes = Arc::new(AtomicUsize::new(0));
        let (first_send, first_recv) = channel();
        let (drop_send, drop_recv) = channel();
        let (id_send, id_recv) = channel();
        let counter = Arc::clone(&callbacks);
        let close_counter = Arc::clone(&closes);
        let child = Window::create(settings, move |cx| {
            let id = match cx.window_handle()?.as_raw() {
                raw_window_handle::RawWindowHandle::Xlib(h) => h.window as u32,
                raw_window_handle::RawWindowHandle::Xcb(h) => h.window.get(),
                _ => unreachable!(),
            };
            id_send.send(id).unwrap();
            Ok(Probe {
                callbacks: counter,
                closes: close_counter,
                first_frame: RefCell::new(Some(first_send)),
                dropped: drop_send,
            })
        })
        .unwrap();
        let child_id = id_recv.recv_timeout(Duration::from_secs(3)).unwrap();
        child.show().unwrap();
        first_recv
            .recv_timeout(Duration::from_secs(3))
            .expect("live native frame");
        conn.destroy_window(if parented { parent } else { child_id })
            .unwrap()
            .check()
            .unwrap();
        assert!(
            conn.get_window_attributes(child_id)
                .unwrap()
                .reply()
                .is_err(),
            "server must delete the drawable"
        );
        // No explicit child.close(): destruction itself must stop and drop it.
        drop_recv
            .recv_timeout(Duration::from_secs(3))
            .expect("destroyed drawable must release its native handler");
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        let finished = callbacks.load(Ordering::SeqCst);
        child.close();
        assert_eq!(callbacks.load(Ordering::SeqCst), finished);
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        if !parented {
            conn.destroy_window(parent).unwrap().check().unwrap();
        }
    }
}

#[test]
fn a_same_size_resize_requires_native_presentation() {
    let mut h = handler((640, 400), 1.0);
    h.step();
    h.unpainted = false;
    h.resized(WindowSize::from_logical(LogicalSize::new(640., 400.), 1.));
    assert!(h.unpainted);
}
