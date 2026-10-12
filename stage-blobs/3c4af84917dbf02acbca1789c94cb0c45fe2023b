//! Original GPU-free native regressions. Use an isolated X11 server.
#![cfg(target_os = "linux")]

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


#[test]
#[ignore = "requires an isolated X11 server; no REAPER process is touched"]
fn nested_ancestor_map_hide_reparent_and_reopen_receive_frames() {
    let host = Host::new();
    // SAFETY: both host ancestors belong to our owned X11 display.
    let middle = unsafe {
        let middle = (host.lib.XCreateSimpleWindow)(host.display, host.parent, 0, 0, 64, 64, 0, 0, 0);
        assert_ne!(middle, 0);
        (host.lib.XMapWindow)(host.display, middle);
        (host.lib.XSync)(host.display, 0);
        middle
    };
    let open = |parent| {
        let frames = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&frames);
        let (tx, rx) = mpsc::channel();
        let child = Window::create(
            WindowSettings::new()
                .with_size(moose_baseview::dpi::LogicalSize::new(64.0, 64.0))
                .with_parent(&Parent(parent)),
            move |cx| {
                let id = match cx.window_handle().unwrap().as_raw() {
                    RawWindowHandle::Xlib(h) => h.window,
                    RawWindowHandle::Xcb(h) => u64::from(h.window.get()),
                    _ => panic!("expected X11 window"),
                };
                tx.send(id).unwrap();
                Ok(Count(counter))
            },
        ).unwrap();
        let id = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        child.show().unwrap();
        (child, id, frames)
    };
    let wait_frame = |frames: &AtomicU64, previous| {
        let deadline = Instant::now() + Duration::from_secs(2);
        while frames.load(Ordering::Relaxed) <= previous && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(frames.load(Ordering::Relaxed) > previous, "viewable descendant must receive a native frame");
    };
    let (child, child_id, frames) = open(middle);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(host.map_state(middle), xlib::IsUnviewable);
    assert_eq!(host.map_state(child_id), xlib::IsUnviewable);
    assert_eq!(frames.load(Ordering::Relaxed), 0);
    // SAFETY: map the initially hidden top ancestor, not the immediate parent.
    unsafe {
        (host.lib.XMapWindow)(host.display, host.parent);
        (host.lib.XSync)(host.display, 0);
    }
    wait_frame(&frames, 0);
    assert_eq!(host.map_state(child_id), xlib::IsViewable);
    // SAFETY: keep every native handle alive while hiding the top ancestor.
    unsafe {
        (host.lib.XUnmapWindow)(host.display, host.parent);
        (host.lib.XSync)(host.display, 0);
    }
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(host.map_state(child_id), xlib::IsUnviewable);
    let hidden_frames = frames.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(frames.load(Ordering::Relaxed), hidden_frames, "hidden ancestors must suppress frames");
    // SAFETY: reparent the middle ancestor to the root while top stays hidden.
    unsafe {
        (host.lib.XReparentWindow)(host.display, middle, host.root, 0, 0);
        (host.lib.XSync)(host.display, 0);
    }
    wait_frame(&frames, hidden_frames);
    assert_eq!(host.map_state(child_id), xlib::IsViewable);
    child.close();
    // A fresh editor must not depend on the prior handler's timer or frame count.
    let (reopened, reopened_id, reopened_frames) = open(middle);
    wait_frame(&reopened_frames, 0);
    assert_eq!(host.map_state(reopened_id), xlib::IsViewable);
    reopened.close();
    // SAFETY: both plugin children are closed before their owned middle parent.
    unsafe { (host.lib.XDestroyWindow)(host.display, middle); }
}
