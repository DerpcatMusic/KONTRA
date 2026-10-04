//! Original GPU-free native regressions. Use an isolated X11 server.
#![cfg(target_os = "linux")]

use kontra_native_host::baseview as moose_baseview;
use moose_baseview::{
    Event, EventStatus, HandlerError, Window, WindowHandler, WindowSettings, WindowSize,
};
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, WindowHandle, XlibWindowHandle,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant};
use x11_dl::xlib;

struct Parent(u64);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: exercise() owns the parent until the child is closed.
        Ok(unsafe { WindowHandle::borrow_raw(XlibWindowHandle::new(self.0).into()) })
    }
}

struct Count(Arc<AtomicU64>);

impl WindowHandler for Count {
    fn on_frame(&self) -> Result<(), HandlerError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
        Ok(())
    }
    fn on_event(&self, _: Event) -> EventStatus {
        EventStatus::Ignored
    }
}

struct Host {
    lib: xlib::Xlib,
    display: *mut xlib::Display,
    root: xlib::Window,
    parent: xlib::Window,
}

impl Host {
    fn new() -> Self {
        let lib = xlib::Xlib::open().unwrap();
        // SAFETY: initialize Xlib threading before opening our owned display.
        unsafe {
            assert_ne!((lib.XInitThreads)(), 0);
            let display = (lib.XOpenDisplay)(std::ptr::null());
            assert!(!display.is_null(), "requires an isolated X11 server");
            let root = (lib.XDefaultRootWindow)(display);
            let parent = (lib.XCreateSimpleWindow)(display, root, 0, 0, 64, 64, 0, 0, 0);
            assert_ne!(parent, 0);
            (lib.XSync)(display, 0);
            Self { lib, display, root, parent }
        }
    }
    fn map_state(&self, window: xlib::Window) -> i32 {
        // SAFETY: a live window on our owned display and a writable attributes struct.
        unsafe {
            let mut attrs: xlib::XWindowAttributes = std::mem::zeroed();
            assert_ne!((self.lib.XGetWindowAttributes)(self.display, window, &mut attrs), 0);
            attrs.map_state
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        // SAFETY: the baseview child is closed before its host/display are dropped.
        unsafe {
            (self.lib.XDestroyWindow)(self.display, self.parent);
            (self.lib.XCloseDisplay)(self.display);
        }
    }
}

fn exercise(reparent: bool) {
    let host = Host::new();
    let frames = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&frames);
    let (tx, rx) = mpsc::channel();
    let child = Window::create(
        WindowSettings::new()
            .with_size(moose_baseview::dpi::LogicalSize::new(64.0, 64.0))
            .with_parent(&Parent(host.parent)),
        move |cx| {
            let id = match cx.window_handle().unwrap().as_raw() {
                RawWindowHandle::Xlib(h) => h.window,
                RawWindowHandle::Xcb(h) => u64::from(h.window.get()),
                _ => panic!("expected X11 window"),
            };
            tx.send(id).unwrap();
            Ok(Count(counter))
        },
    )
    .unwrap();
    let child_id = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    child.show().unwrap();
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(host.map_state(child_id), xlib::IsUnviewable);
    assert_eq!(frames.load(Ordering::Relaxed), 0);
    // SAFETY: operate on live windows; synchronize before checking their state.
    unsafe {
        if reparent {
            (host.lib.XReparentWindow)(host.display, child_id, host.root, 0, 0);
        } else {
            (host.lib.XMapWindow)(host.display, host.parent);
        }
        (host.lib.XSync)(host.display, 0);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while frames.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let map_state = host.map_state(child_id);
    let callbacks = frames.load(Ordering::Relaxed);
    child.close();
    assert_eq!(map_state, xlib::IsViewable);
    assert!(callbacks > 0, "server-viewable child must receive a native frame");
}

#[test]
#[ignore = "requires an isolated X11 server; see PATCHES.md"]
fn late_parent_map_receives_frames() {
    exercise(false);
}

#[test]
#[ignore = "requires an isolated X11 server; see PATCHES.md"]
fn reparent_to_root_receives_frames() {
    exercise(true);
}
