//! Screen coordinates for the editor's X11 child, in native physical pixels.
//! The temporary XCB wrapper borrows baseview's connection; it never owns it,
//! changes the event queue owner, or consumes events from the window loop.
use mui_access::accesskit::Rect;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};
use x11rb::{
    protocol::xproto::{AtomEnum, ConnectionExt},
    xcb_ffi::XCBConnection,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct WindowBounds {
    pub outer: Rect,
    pub inner: Rect,
}

/// Baseview reports resizes but not host/frame movement or reparenting. Poll
/// at most ten times/second while AT-SPI is active, publishing only changes.
#[derive(Default)]
pub(super) struct BoundsPoll {
    next: Option<Instant>,
    last: Option<WindowBounds>,
}
impl BoundsPoll {
    pub fn pause(&mut self) {
        self.next = None;
    }
    pub fn update(
        &mut self,
        now: Instant,
        read: impl FnOnce() -> Option<WindowBounds>,
    ) -> Option<WindowBounds> {
        if self.next.is_some_and(|next| now < next) {
            return None;
        }
        self.next = Some(now + Duration::from_millis(100));
        let bounds = read()?;
        if self.last == Some(bounds) {
            return None;
        }
        self.last = Some(bounds);
        Some(bounds)
    }
}

/// Query only the supplied editor XID. Host ancestors affect translation,
/// but their bounds and frame decorations are never exported as the editor.
///
/// # Safety
/// `display` must borrow a live X11 connection for the duration of this call.
#[expect(
    unsafe_code,
    reason = "borrow a live raw X11 connection without taking ownership"
)]
pub(super) unsafe fn query(
    display: RawDisplayHandle,
    window: RawWindowHandle,
) -> Option<WindowBounds> {
    let window = match window {
        RawWindowHandle::Xlib(window) => u32::try_from(window.window).ok()?,
        RawWindowHandle::Xcb(window) => window.window.get(),
        _ => return None,
    };
    let connection = match display {
        RawDisplayHandle::Xcb(display) => display.connection?.as_ptr(),
        RawDisplayHandle::Xlib(display) => {
            static XLIB_XCB: OnceLock<Option<x11_dl::xlib_xcb::Xlib_xcb>> = OnceLock::new();
            let library = XLIB_XCB
                .get_or_init(|| x11_dl::xlib_xcb::Xlib_xcb::open().ok())
                .as_ref()?;
            // SAFETY: caller retains the original live Display; this extracts
            // its existing connection and does not open another one.
            unsafe { (library.XGetXCBConnection)(display.display?.as_ptr().cast()) }
        }
        _ => return None,
    };
    if connection.is_null() || window == 0 {
        return None;
    }
    // SAFETY: caller keeps the native owner alive throughout this query.
    // `false` makes drop non-owning, so it never disconnects baseview.
    let connection = unsafe { XCBConnection::from_raw_xcb_connection(connection, false) }.ok()?;
    query_connection(&connection, window)
}

fn query_connection(connection: &XCBConnection, window: u32) -> Option<WindowBounds> {
    // Checked XCB replies fail normally on close races (BadWindow), without
    // installing a process-global Xlib error handler in a plugin host.
    let geometry = connection.get_geometry(window).ok()?.reply().ok()?;
    let position = connection
        .translate_coordinates(window, geometry.root, 0, 0)
        .ok()?
        .reply()
        .ok()?;
    if !position.same_screen {
        return None;
    }
    let (x, y, width, height) = (
        f64::from(position.dst_x),
        f64::from(position.dst_y),
        f64::from(geometry.width),
        f64::from(geometry.height),
    );
    let inner = Rect::new(x, y, x + width, y + height);
    let border = f64::from(geometry.border_width);
    let mut insets = [border; 4];
    // A standalone WM may reparent the client into a decoration frame. Its
    // extents property belongs to this client. Embedded children normally
    // have no property, and use only their own border, never the host's.
    if let Ok(atom) = connection.intern_atom(false, b"_NET_FRAME_EXTENTS")
        && let Ok(atom) = atom.reply()
        && let Ok(property) =
            connection.get_property(false, window, atom.atom, AtomEnum::CARDINAL, 0, 4)
        && let Ok(property) = property.reply()
        && let Some(values) = property.value32()
    {
        let values: Vec<_> = values.collect();
        if let [left, right, top, bottom] = values.as_slice() {
            insets = [*left, *right, *top, *bottom].map(f64::from);
        }
    }
    let [left, right, top, bottom] = insets;
    Some(WindowBounds {
        inner,
        outer: Rect::new(x - left, y - top, x + width + right, y + height + bottom),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use raw_window_handle::{XcbDisplayHandle, XcbWindowHandle};
    use std::ptr::NonNull;
    use x11rb::{
        connection::Connection,
        protocol::xproto::{ConfigureWindowAux, CreateWindowAux, PropMode, WindowClass},
        wrapper::ConnectionExt as _,
    };

    #[test]
    fn inactive_and_unchanged_bounds_need_no_native_publication() {
        let now = Instant::now();
        let a = WindowBounds {
            outer: Rect::new(10., 20., 110., 70.),
            inner: Rect::new(10., 20., 110., 70.),
        };
        let b = WindowBounds {
            inner: Rect::new(15., 25., 115., 75.),
            ..a
        };
        let mut poll = BoundsPoll::default();
        assert_eq!(poll.update(now, || Some(a)), Some(a));
        assert!(
            poll.update(now + Duration::from_millis(50), || panic!(
                "poll exceeded its rate"
            ))
            .is_none()
        );
        assert!(
            poll.update(now + Duration::from_millis(100), || Some(a))
                .is_none()
        );
        poll.pause();
        assert_eq!(
            poll.update(now + Duration::from_millis(120), || Some(b)),
            Some(b)
        );
    }

    #[test]
    #[ignore = "requires X11; run under xvfb-run with --ignored"]
    #[expect(unsafe_code, reason = "test borrows its own live XCB connection")]
    fn borrowed_connection_tracks_child_move_reparent_resize_and_close() {
        let (connection, screen) = XCBConnection::connect(None).unwrap();
        let root = &connection.setup().roots[screen];
        let parent = connection.generate_id().unwrap();
        let second = connection.generate_id().unwrap();
        let child = connection.generate_id().unwrap();
        let create = |window, parent, x, y, width, height, border| {
            connection
                .create_window(
                    x11rb::COPY_DEPTH_FROM_PARENT,
                    window,
                    parent,
                    x,
                    y,
                    width,
                    height,
                    border,
                    WindowClass::INPUT_OUTPUT,
                    x11rb::COPY_FROM_PARENT,
                    &CreateWindowAux::new(),
                )
                .unwrap()
                .check()
                .unwrap();
        };
        create(parent, root.root, 100, 80, 320, 200, 3);
        create(second, root.root, 400, 300, 320, 200, 0);
        create(child, parent, 10, 20, 80, 40, 2);
        let display = RawDisplayHandle::Xcb(XcbDisplayHandle::new(
            NonNull::new(connection.get_raw_xcb_connection()),
            screen as i32,
        ));
        let window = RawWindowHandle::Xcb(XcbWindowHandle::new(
            std::num::NonZeroU32::new(child).unwrap(),
        ));
        // SAFETY: the connection and all test windows outlive every query.
        let read = || unsafe { query(display, window) };
        assert_eq!(
            read(),
            Some(WindowBounds {
                inner: Rect::new(115., 105., 195., 145.),
                outer: Rect::new(113., 103., 197., 147.)
            })
        );
        connection
            .configure_window(parent, &ConfigureWindowAux::new().x(200).y(160))
            .unwrap()
            .check()
            .unwrap();
        assert_eq!(read().unwrap().inner, Rect::new(215., 185., 295., 225.));
        connection
            .reparent_window(child, second, 7, 9)
            .unwrap()
            .check()
            .unwrap();
        connection
            .configure_window(child, &ConfigureWindowAux::new().width(120).height(60))
            .unwrap()
            .check()
            .unwrap();
        assert_eq!(read().unwrap().inner, Rect::new(409., 311., 529., 371.));
        let atom = connection
            .intern_atom(false, b"_NET_FRAME_EXTENTS")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        connection
            .change_property32(
                PropMode::REPLACE,
                child,
                atom,
                AtomEnum::CARDINAL,
                &[5, 7, 11, 13],
            )
            .unwrap()
            .check()
            .unwrap();
        assert_eq!(read().unwrap().outer, Rect::new(404., 300., 536., 384.));
        connection.destroy_window(child).unwrap().check().unwrap();
        assert!(read().is_none(), "checked query handles a destroyed child");
        // The temporary wrappers must not disconnect the native owner.
        assert!(connection.get_geometry(parent).unwrap().reply().is_ok());
        connection.destroy_window(parent).unwrap().check().unwrap();
        connection.destroy_window(second).unwrap().check().unwrap();
    }
    #[test]
    #[ignore = "requires X11; run under xvfb-run with --ignored"]
    #[expect(
        unsafe_code,
        reason = "test owns an Xlib display and borrows its underlying XCB connection"
    )]
    fn xlib_display_uses_its_original_connection() {
        struct Display<'a>(&'a x11_dl::xlib::Xlib, *mut x11_dl::xlib::Display);
        impl Drop for Display<'_> {
            fn drop(&mut self) {
                // SAFETY: this guard uniquely owns the display, and its borrowed
                // XCB wrapper is declared later so it is dropped first.
                unsafe {
                    (self.0.XCloseDisplay)(self.1);
                }
            }
        }
        let library = x11_dl::xlib::Xlib::open().unwrap();
        // SAFETY: null selects DISPLAY; the test guard owns the returned display.
        let pointer = unsafe { (library.XOpenDisplay)(std::ptr::null()) };
        assert!(!pointer.is_null());
        let display = Display(&library, pointer);
        let xlib_xcb = x11_dl::xlib_xcb::Xlib_xcb::open().unwrap();
        // SAFETY: display is owned by the live guard above.
        let raw_connection = unsafe { (xlib_xcb.XGetXCBConnection)(display.1) };
        // SAFETY: the wrapper is non-owning and outlived by the display guard.
        let connection =
            unsafe { XCBConnection::from_raw_xcb_connection(raw_connection, false) }.unwrap();
        let window = connection.generate_id().unwrap();
        let root = connection.setup().roots[0].root;
        connection
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                root,
                150,
                90,
                80,
                40,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new(),
            )
            .unwrap()
            .check()
            .unwrap();
        let handle = RawWindowHandle::Xlib(raw_window_handle::XlibWindowHandle::new(window.into()));
        let display_handle = RawDisplayHandle::Xlib(raw_window_handle::XlibDisplayHandle::new(
            NonNull::new(display.1.cast()),
            0,
        ));
        // SAFETY: the original Xlib display remains live throughout the query.
        let bounds = unsafe { query(display_handle, handle) }.unwrap();
        assert_eq!(bounds.inner, Rect::new(150., 90., 230., 130.));
        connection.destroy_window(window).unwrap().check().unwrap();
        assert!(connection.get_geometry(root).unwrap().reply().is_ok());
    }
}
