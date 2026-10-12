use super::drag_n_drop::DragNDropState;
use super::keyboard::{convert_key_press_event, convert_key_release_event, key_mods};
use super::*;
use std::result::Result;

use crate::dpi::{PhysicalPosition, PhysicalSize};
use crate::host::HostMainThreadCaller;
use crate::platform::frame_rate::frame_interval;
use crate::platform::x11::error::FatalError;
use crate::platform::x11::window_thread::{
    HostCallback, WindowThreadRequest, WindowThreadResponseMessage,
};
use crate::warn;
use crate::wrappers::xkbcommon::XkbcommonState;
use crate::{
    Event, EventStatus, MouseButton, MouseEvent, ScrollDelta, WindowEvent, WindowHandler,
    WindowSize,
};
use calloop::generic::Generic;
use calloop::timer::{TimeoutAction, Timer};
use calloop::{Interest, LoopHandle, LoopSignal, Mode, PostAction};
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::errors::ConnectionError;
use x11rb::protocol::randr::{ConnectionExt as _, ModeFlag, ModeInfo};
use x11rb::protocol::xproto::{ConnectionExt as _, EventMask, KeyPressEvent, KeyReleaseEvent};
use x11rb::protocol::Event as XEvent;
use x11rb::CURRENT_TIME;

pub struct MainThreadCaller {
    sender: mpsc::Sender<HostCallback>,
    caller: Box<dyn HostMainThreadCaller>,
}

impl MainThreadCaller {
    pub(crate) fn new(
        main_thread: Option<Box<dyn HostMainThreadCaller>>,
    ) -> (Option<Self>, Option<Receiver<HostCallback>>) {
        let Some(main_thread) = main_thread else {
            return (None, None);
        };

        let (sender, receiver) = mpsc::channel();
        (Some(Self { sender, caller: main_thread }), Some(receiver))
    }

    pub fn send(&mut self, msg: HostCallback) -> Result<(), FatalError> {
        self.sender.send(msg).map_err(|_| FatalError::SendMainThread)?;
        self.caller.call_main_thread();
        Ok(())
    }
}

pub(crate) struct EventLoop {
    handler: Box<dyn WindowHandler>,
    window: Rc<WindowInner>,

    new_size: Option<PhysicalSize<u16>>,
    new_parent_size: Option<PhysicalSize<u16>>,
    exposed: bool,

    /// `on_frame` pacing: the refresh period of the monitor under the window (MOOSE).
    frame_interval: Duration,
    /// Set by `ConfigureNotify`: the window may have moved to another monitor.
    refresh_rate_stale: bool,
    refresh_rate_queried_at: Instant,

    loop_signal: LoopSignal,

    drag_n_drop: DragNDropState,
    xkb_state: Option<XkbcommonState>,

    run_error: Option<PlatformError>,

    response_sender: mpsc::Sender<WindowThreadResponseMessage>,
    main_thread: Option<MainThreadCaller>,

    /// Keys currently held down in this window, by X keycode. Used to flag
    /// auto-repeat key-downs (MOOSE X01).
    pressed_keys: [bool; 256],
    /// Key-downs the handler ignored and that were forwarded to the embed
    /// parent, so the matching key-up can be synthesized on focus loss/close.
    forwarded_keys: [Option<KeyPressEvent>; 256],
}

impl EventLoop {
    pub fn new(
        window: Rc<WindowInner>, handler: Box<dyn WindowHandler>,
        request_receiver: calloop::channel::Channel<WindowThreadRequest>,
        response_sender: mpsc::Sender<WindowThreadResponseMessage>,
        main_thread: Option<MainThreadCaller>, inner: &mut calloop::EventLoop<'static, Self>,
    ) -> Result<Self, PlatformError> {
        let loop_handle = inner.handle();

        let frame_interval = frame_interval(query_refresh_hz(&window));
        Self::setup_frame_timer(&loop_handle, frame_interval)?;

        loop_handle
            .insert_source(
                Generic::new_with_error(
                    Arc::clone(&window.connection.conn),
                    Interest::READ,
                    Mode::Edge,
                ),
                |_, _, e| e.handle_connection_event_ready(),
            )
            .map_err(|e| e.error)?;

        loop_handle
            .insert_source(request_receiver, |e, _, l| l.handle_main_thread_request(e))
            .map_err(|e| e.error)?;

        Ok(Self {
            loop_signal: inner.get_signal(),
            handler,
            new_size: None,
            new_parent_size: None,
            exposed: false,
            frame_interval,
            refresh_rate_stale: false,
            refresh_rate_queried_at: Instant::now(),
            drag_n_drop: DragNDropState::NoCurrentSession,
            xkb_state: XkbcommonState::new(&window.connection),
            run_error: None,
            main_thread,
            pressed_keys: [false; 256],
            forwarded_keys: [None; 256],

            window,
            response_sender,
        })
    }

    #[inline]
    fn drain_xcb_events(&mut self) -> Result<bool, ConnectionError> {
        if self.window.main_thread_shared.callbacks_revoked() {
            return Ok(false);
        }
        let mut event_received = false;
        // Core X11 auto-repeat arrives as a KeyRelease/KeyPress pair with the
        // same keycode, timestamp and window. Swallow the synthetic release so
        // handlers see one repeated key-down instead of release + fresh press.
        let mut pending_release: Option<KeyReleaseEvent> = None;
        while let Some(event) = self.window.connection.conn.poll_for_event()? {
            if self.window.main_thread_shared.callbacks_revoked() {
                break;
            }
            event_received = true;
            match event {
                XEvent::KeyRelease(release) => {
                    if let Some(previous) = pending_release.replace(release) {
                        self.handle_xcb_event(XEvent::KeyRelease(previous))?;
                    }
                }
                XEvent::KeyPress(press) => {
                    let release = pending_release.take();
                    if let Some(release) = release.filter(|r| !is_auto_repeat_pair(r, &press)) {
                        self.handle_xcb_event(XEvent::KeyRelease(release))?;
                    }
                    self.handle_xcb_event(XEvent::KeyPress(press))?;
                }
                event => {
                    if let Some(release) = pending_release.take() {
                        self.handle_xcb_event(XEvent::KeyRelease(release))?;
                    }
                    self.handle_xcb_event(event)?;
                }
            }
        }
        if let Some(release) =
            pending_release.filter(|_| !self.window.main_thread_shared.callbacks_revoked())
        {
            self.handle_xcb_event(XEvent::KeyRelease(release))?;
        }

        Ok(event_received)
    }

    fn setup_frame_timer(
        loop_handle: &LoopHandle<'_, Self>, interval: Duration,
    ) -> Result<(), calloop::Error> {
        fn handle_frame(evloop: &mut EventLoop, previous_deadline: Instant) -> TimeoutAction {
            evloop.exposed = true;

            // A window drag sends a ConfigureNotify per step: re-query a few times a second at most.
            if evloop.refresh_rate_stale
                && evloop.refresh_rate_queried_at.elapsed() >= Duration::from_millis(250)
            {
                evloop.refresh_rate_stale = false;
                evloop.refresh_rate_queried_at = Instant::now();
                evloop.frame_interval = frame_interval(query_refresh_hz(&evloop.window));
            }

            // Keep a steady cadence. If a frame overran its slot, restart the cadence from now
            // instead of queueing catch-up frames.
            let interval = evloop.frame_interval;
            match previous_deadline.checked_add(interval) {
                Some(next) if next > Instant::now() => TimeoutAction::ToInstant(next),
                _ => TimeoutAction::ToDuration(interval),
            }
        }

        loop_handle
            .insert_source(Timer::from_duration(interval), |i, _, e| handle_frame(e, i))
            .map_err(|e| e.error)?;

        Ok(())
    }

    fn handle_redraw(&mut self) {
        if self.window.main_thread_shared.callbacks_revoked() {
            return;
        }
        if !self.exposed {
            return;
        }
        self.exposed = false;

        if !self.window.visibility_state.own_window_is_viewable() {
            return;
        }

        if let Err(e) = self.handler.on_frame() {
            self.trigger_fatal_error(e.into());
            return;
        }

        self.drain_ime();
        // Any socket error will be handled in the next poll
        let _ = self.window.connection.conn.flush();
    }

    fn handle_coalesced_resize_events(&mut self) -> Result<(), FatalError> {
        if self.window.main_thread_shared.callbacks_revoked() {
            return Ok(());
        }
        let mut comes_from_parent = false;
        if let Some(new_parent_size) = self.new_parent_size.take() {
            if new_parent_size != self.window.get_size() {
                // The parent was resized, which means we should resize ourselves too.
                if let Err(e) = self.window.xcb_window.resize(new_parent_size.cast()) {
                    crate::warn!("Failed to resize window: {}", e);
                } else {
                    // Makes the rest of this function run on the new parent size immediately (without waiting for a ConfigureNotify round-trip)
                    // Also overrides any new sizes we may have received this event loop iteration,it would probably be invalidated anyway
                    self.new_size = Some(new_parent_size);
                    comes_from_parent = true;
                }
            }
        }

        let Some(new_size) = self.new_size.take() else { return Ok(()) };
        let previous = self.window.store_size(new_size);

        if previous == new_size {
            return Ok(());
        };

        let scale_factor = self.window.scaling_factor.get();
        let new_size = WindowSize::from_physical(new_size.cast(), scale_factor);

        if let Err(e) = self.handler.resized(new_size) {
            warn!("Window Handler failed to resize: {}", e);
            self.window.store_size(previous);
            self.window.xcb_window.resize(previous.cast())?.check_warn();
            return Ok(());
        }

        // Host requests use resize_immediately, which stops the previous == new_size condition
        // So if we're here, it's guaranteed not to be from a host request

        if !comes_from_parent {
            if let Some(host) = self.main_thread.as_mut() {
                host.send(HostCallback::Resized {
                    new_size,
                    previous: WindowSize::from_physical(previous.cast(), scale_factor),
                })?;
            }
        }

        // Immediately schedule a redraw, do not wait for an "expose" event
        self.exposed = true;

        Ok(())
    }

    fn handle_main_thread_request(&mut self, event: calloop::channel::Event<WindowThreadRequest>) {
        match event {
            calloop::channel::Event::Closed => {
                // Closed channel means the sender, i.e. the Window Handle has been dropped.
                // It should already stop this event loop on drop, but we'll take the hint.
                self.stop_now();
            }
            calloop::channel::Event::Msg(req) => match self.handle_request(req) {
                Ok(()) => self.send_response(Ok(())),
                Err(e) => self.send_response(Err(e.to_string())),
            },
        }
    }

    fn send_response(&mut self, response: WindowThreadResponseMessage) {
        if let Err(e) = self.response_sender.send(response) {
            warn!("Failed to send response back to main thread: {}", &e);
            if let Err(e) = e.0 {
                crate::error!("Request failed: {}", e)
            }

            self.stop_now();
        }
    }

    fn stop_now(&self) {
        self.loop_signal.stop();
        self.loop_signal.wakeup();
    }

    fn trigger_fatal_error(&mut self, error: PlatformError) {
        if self.run_error.is_none() {
            self.run_error = Some(error);
        }
        self.stop_now();
    }

    fn handle_request(&mut self, req: WindowThreadRequest) -> Result<(), PlatformError> {
        match req {
            WindowThreadRequest::Resize(new_size) => {
                let scale_factor = self.window.scaling_factor.get();
                let new_size = new_size.to_physical(scale_factor);

                self.window.resize_immediately(new_size, &*self.handler)?;

                Ok(())
            }
            WindowThreadRequest::SuggestScaleFactor(scale) => {
                // If the scaling factor is already provided by the system, do nothing
                if !self.window.scaling_factor.suggest(scale) {
                    return Ok(());
                };

                let current_logical_size = self.window.get_size().to_logical::<f64>(1.0);
                let new_physical_size = current_logical_size.to_physical(scale);

                self.window.resize_immediately(new_physical_size, &*self.handler)?;

                Ok(())
            }
            WindowThreadRequest::SetScaleFactorOverride(scale_factor) => {
                self.window.set_scale_factor_override(scale_factor)
            }
            WindowThreadRequest::SetParent(new_parent) => {
                self.window.xcb_window.reparent(Some(new_parent.window_id))?;

                Ok(())
            }
            WindowThreadRequest::Show => {
                self.window.xcb_window.map_window()?.check()?;
                self.window.visibility_state.window_mapped(self.window.xcb_window.id());
                Ok(())
            }
            WindowThreadRequest::Hide => {
                self.window.xcb_window.unmap_window()?.check()?;
                self.window.visibility_state.window_unmapped(self.window.xcb_window.id());
                Ok(())
            }
        }
    }

    fn handle_connection_event_ready(&mut self) -> Result<PostAction, FatalError> {
        self.drain_xcb_events()?;

        Ok(PostAction::Continue)
    }

    fn handle_idle(&mut self) {
        if let Err(e) = self.try_handle_idle() {
            self.trigger_fatal_error(e.into());
        }
    }

    fn try_handle_idle(&mut self) -> Result<(), FatalError> {
        if self.window.main_thread_shared.callbacks_revoked() {
            self.stop_now();
            return Ok(());
        }
        // Check for any events in the internal buffers before going to sleep:
        self.drain_xcb_events()?;

        loop {
            self.handle_coalesced_resize_events()?;
            self.handle_redraw();

            if !self.drain_xcb_events()? {
                break;
            }
        }

        self.window.connection.conn.flush()?;

        Ok(())
    }

    pub fn run(mut self, mut inner: calloop::EventLoop<Self>) -> Result<(), PlatformError> {
        self.drain_xcb_events()?;
        inner.run(None, &mut self, Self::handle_idle)?;

        if !self.window.main_thread_shared.callbacks_revoked() {
            self.release_forwarded_keys();
            self.handle_event(Event::Window(WindowEvent::WillClose));
        }

        // If the event loop doesn't stop because the host asked it to, then we should notify it
        if !self.window.main_thread_shared.is_stop_host_requested() {
            if let Some(main_thread) = self.main_thread.as_mut() {
                if let Err(e) = main_thread.send(HostCallback::Destroyed) {
                    warn!("Could not notify host that X11 thread is stopping: {}", e)
                }
            }
        }

        if let Some(err) = self.run_error {
            return Err(err);
        };

        Ok(())
    }

    fn drain_ime(&mut self) {
        let events =
            self.window.ime.borrow_mut().as_mut().map(|ime| ime.drain()).unwrap_or_default();
        for event in events {
            // MUI's queued XIM callbacks must observe bounded-close revocation too.
            if self.window.main_thread_shared.callbacks_revoked() {
                break;
            }
            match event {
                super::ime::NativeEvent::Input(event) => {
                    self.handle_event(event);
                }
                super::ime::NativeEvent::Key(event) => {
                    let is_down =
                        event.response_type & 0x7f == x11rb::protocol::xproto::KEY_PRESS_EVENT;
                    let mut key = if is_down {
                        convert_key_press_event(&event, &mut self.xkb_state)
                    } else {
                        convert_key_release_event(&event, &mut self.xkb_state)
                    };
                    if let Some(pressed) = self.pressed_keys.get_mut(usize::from(event.detail)) {
                        key.repeat = is_down && *pressed;
                        *pressed = is_down;
                    }
                    if self.handler.on_event(Event::Keyboard(key)) == EventStatus::Ignored {
                        self.forward_key_event(event);
                    }
                }
            }
        }
    }

    fn handle_xcb_event(&mut self, event: XEvent) -> Result<(), ConnectionError> {
        let filtered = self.window.ime.borrow_mut().as_mut().is_some_and(|ime| ime.filter(&event));
        self.drain_ime();
        if self.window.main_thread_shared.callbacks_revoked() {
            return Ok(());
        }
        if filtered {
            return Ok(());
        }
        if let XEvent::KeyPress(key) | XEvent::KeyRelease(key) = &event {
            if key.event == self.window.raw_id() {
                let forwarded =
                    self.window.ime.borrow_mut().as_mut().is_some_and(|ime| ime.forward(key));
                if forwarded {
                    return Ok(());
                }
            }
        }

        // For all the keyboard and mouse events, you can fetch
        // `x`, `y`, `detail`, and `state`.
        // - `x` and `y` are the position inside the window where the cursor currently is
        //   when the event happened.
        // - `detail` will tell you which keycode was pressed/released (for keyboard events)
        //   or which mouse button was pressed/released (for mouse events).
        //   For mouse events, here's what the value means (at least on my current mouse):
        //      1 = left mouse button
        //      2 = middle mouse button (scroll wheel)
        //      3 = right mouse button
        //      4 = scroll wheel up
        //      5 = scroll wheel down
        //      8 = lower side button ("back" button)
        //      9 = upper side button ("forward" button)
        //   Note that you *will* get a "button released" event for even the scroll wheel
        //   events, which you can probably ignore.
        // - `state` will tell you the state of the main three mouse buttons and some of
        //   the keyboard modifier keys at the time of the event.
        //   http://rtbo.github.io/rust-xcb/src/xcb/ffi/xproto.rs.html#445

        match event {
            ////
            // window
            ////
            XEvent::ClientMessage(event) if event.window == self.window.raw_id() => {
                if event.format != 32 {
                    return Ok(());
                }

                if event.data.as_data32()[0] == self.window.connection.atoms.WM_DELETE_WINDOW {
                    self.window.request_close();
                    return Ok(());
                }

                ////
                // drag n drop
                ////
                if event.type_ == self.window.connection.atoms.XdndEnter {
                    self.drag_n_drop.handle_enter_event(&self.window, &*self.handler, &event)?;
                } else if event.type_ == self.window.connection.atoms.XdndPosition {
                    self.drag_n_drop.handle_position_event(&self.window, &*self.handler, &event)?;
                } else if event.type_ == self.window.connection.atoms.XdndDrop {
                    self.drag_n_drop.handle_drop_event(&self.window, &*self.handler, &event)?;
                } else if event.type_ == self.window.connection.atoms.XdndLeave {
                    self.drag_n_drop.handle_leave_event(&*self.handler, &event);
                }
            }

            XEvent::SelectionNotify(event) => {
                if event.property == self.window.connection.atoms.XdndSelection {
                    self.drag_n_drop.handle_selection_notify_event(
                        &self.window,
                        &*self.handler,
                        &event,
                    )?;
                }
            }

            XEvent::Error(e) => {
                warn!("Received leftover X11 error: {:?}", e);
            }

            XEvent::ConfigureNotify(event) => {
                // Our window, an ancestor or any top-level moved: maybe onto another monitor.
                self.refresh_rate_stale = true;
                // These are coalesced and then handled asynchronously at the end of the event loop
                if event.window == self.window.raw_id() {
                    self.new_size = Some(PhysicalSize::new(event.width, event.height));
                } else if Some(event.window)
                    == self.window.visibility_state.parent_id().map(|i| i.get())
                {
                    // Also resize the window if the parent is resized
                    // This works around some hosts that might not call set_size() right away (or at all...)
                    self.new_parent_size = Some(PhysicalSize::new(event.width, event.height));
                }
            }

            XEvent::Expose(e) if e.window == self.window.raw_id() => self.exposed = true,

            ////
            // mouse
            ////
            XEvent::MotionNotify(event) if event.event == self.window.raw_id() => {
                let physical_pos = PhysicalPosition::new(event.event_x, event.event_y);

                self.handle_event(Event::Mouse(MouseEvent::CursorMoved {
                    position: physical_pos.cast(),
                    modifiers: key_mods(event.state),
                }));
            }

            XEvent::EnterNotify(event) if event.event == self.window.raw_id() => {
                self.handle_event(Event::Mouse(MouseEvent::CursorEntered));
                // since no `MOTION_NOTIFY` event is generated when `ENTER_NOTIFY` is generated,
                // we generate a CursorMoved as well, so the mouse position from here isn't lost
                let physical_pos = PhysicalPosition::new(event.event_x, event.event_y);
                self.handle_event(Event::Mouse(MouseEvent::CursorMoved {
                    position: physical_pos.cast(),
                    modifiers: key_mods(event.state),
                }));
            }

            XEvent::LeaveNotify(event) if event.event == self.window.raw_id() => {
                self.handle_event(Event::Mouse(MouseEvent::CursorLeft));
            }

            XEvent::ButtonPress(event) if event.event == self.window.raw_id() => {
                match event.detail {
                    4..=7 => {
                        self.handle_event(Event::Mouse(MouseEvent::WheelScrolled {
                            delta: match event.detail {
                                4 => ScrollDelta::Lines { x: 0.0, y: 1.0 },
                                5 => ScrollDelta::Lines { x: 0.0, y: -1.0 },
                                6 => ScrollDelta::Lines { x: -1.0, y: 0.0 },
                                7 => ScrollDelta::Lines { x: 1.0, y: 0.0 },
                                _ => unreachable!(),
                            },
                            modifiers: key_mods(event.state),
                        }));
                    }
                    detail => {
                        self.handle_event(Event::Mouse(MouseEvent::ButtonPressed {
                            button: mouse_id(detail),
                            modifiers: key_mods(event.state),
                        }));
                    }
                }
            }

            XEvent::ButtonRelease(event)
                if event.event == self.window.raw_id() && !(4..=7).contains(&event.detail) =>
            {
                let button_id = mouse_id(event.detail);
                self.handle_event(Event::Mouse(MouseEvent::ButtonReleased {
                    button: button_id,
                    modifiers: key_mods(event.state),
                }));
            }

            ////
            // keys
            ////
            XEvent::KeyPress(event) if event.event == self.window.raw_id() => {
                let mut key = convert_key_press_event(&event, &mut self.xkb_state);
                if let Some(pressed) = self.pressed_keys.get_mut(usize::from(event.detail)) {
                    key.repeat = std::mem::replace(pressed, true);
                }
                if self.handler.on_event(Event::Keyboard(key)) == EventStatus::Ignored {
                    self.forward_key_event(event);
                }
            }

            XEvent::KeyRelease(event) if event.event == self.window.raw_id() => {
                if let Some(pressed) = self.pressed_keys.get_mut(usize::from(event.detail)) {
                    *pressed = false;
                }
                let key = convert_key_release_event(&event, &mut self.xkb_state);
                if self.handler.on_event(Event::Keyboard(key)) == EventStatus::Ignored {
                    self.forward_key_event(event);
                }
            }

            XEvent::FocusIn(event) if event.event == self.window.raw_id() => {
                self.window.is_focused.set(true);
                let config = self.window.ime_configuration.borrow().clone();
                self.window.set_ime_configuration(config);
                self.drain_ime();
                self.handle_event(Event::Window(WindowEvent::Focused));
            }

            XEvent::FocusOut(e) if e.event == self.window.raw_id() => {
                self.window.is_focused.set(false);
                let config = self.window.ime_configuration.borrow().clone();
                self.window.set_ime_configuration(config);
                self.drain_ime();
                self.pressed_keys.fill(false);
                self.release_forwarded_keys();
                self.handle_event(Event::Window(WindowEvent::Unfocused));
            }

            XEvent::MapNotify(e) => {
                if let Some(window_id) = NonZero::new(e.window) {
                    if window_id == self.window.xcb_window.id() {
                        self.window.is_mapped.set(true);
                    }

                    let became_viewable = self.window.visibility_state.window_mapped(window_id);

                    if became_viewable {
                        self.exposed = true;
                    }
                }
            }

            XEvent::UnmapNotify(e) => {
                if let Some(window_id) = NonZero::new(e.window) {
                    if window_id == self.window.xcb_window.id() {
                        self.window.is_mapped.set(false)
                    }

                    self.window.visibility_state.window_unmapped(window_id);
                }
            }

            XEvent::ReparentNotify(e) => {
                if let Some(window_id) = NonZero::new(e.window) {
                    self.window.visibility_state.window_reparented(
                        window_id,
                        NonZero::new(e.parent),
                        &self.window.connection,
                    )
                }
            }

            XEvent::DestroyNotify(e) => {
                if let Some(window_id) = NonZero::new(e.window) {
                    self.window
                        .visibility_state
                        .window_destroyed(window_id, &self.window.connection)
                }
            }

            _ => {}
        }

        Ok(())
    }

    fn handle_event(&mut self, event: Event) {
        if !self.window.main_thread_shared.callbacks_revoked() {
            self.handler.on_event(event);
        }
    }

    /// Hands a key event the handler ignored to the embed parent, so host
    /// shortcuts (transport, undo, ...) keep working while the editor has
    /// focus. Ported from KURV (K26).
    fn forward_key_event(&mut self, mut event: KeyPressEvent) {
        let Some(parent) = self.window.visibility_state.parent_id() else {
            return;
        };
        // XSendEvent sets the high bit. Never bounce a host-redispatched event
        // back; only forward original input.
        if event.response_type & 0x80 != 0 {
            return;
        }
        event.event = parent.get();
        event.child = self.window.raw_id();
        let conn = &self.window.connection.conn;
        let mask = EventMask::KEY_PRESS | EventMask::KEY_RELEASE;
        if conn.send_event(true, parent.get(), mask, event).is_ok() {
            if let Some(slot) = self.forwarded_keys.get_mut(usize::from(event.detail)) {
                *slot = (event.response_type == x11rb::protocol::xproto::KEY_PRESS_EVENT)
                    .then_some(event);
            }
        }
        let _ = conn.flush();
    }

    fn release_forwarded_keys(&mut self) {
        let held: Vec<KeyPressEvent> =
            self.forwarded_keys.iter_mut().filter_map(Option::take).collect();
        for mut event in held {
            event.response_type = x11rb::protocol::xproto::KEY_RELEASE_EVENT;
            event.time = CURRENT_TIME;
            self.forward_key_event(event);
        }
    }
}

/// Refresh rate of the RandR CRTC under the window's centre, `None` when RandR is missing or
/// the window is off every CRTC (MOOSE).
fn query_refresh_hz(window: &WindowInner) -> Option<f64> {
    let conn = window.connection.conn.xcb_connection();
    let root = window.connection.conn.default_screen().root;
    let origin = conn.translate_coordinates(window.raw_id(), root, 0, 0).ok()?.reply().ok()?;
    let size = window.get_size();
    let x = i32::from(origin.dst_x).saturating_add(i32::from(size.width >> 1));
    let y = i32::from(origin.dst_y).saturating_add(i32::from(size.height >> 1));

    let resources = conn.randr_get_screen_resources_current(root).ok()?.reply().ok()?;
    let crtcs: Vec<_> = resources
        .crtcs
        .iter()
        .filter_map(|&crtc| conn.randr_get_crtc_info(crtc, resources.config_timestamp).ok())
        .collect();
    let crtc = crtcs.into_iter().filter_map(|cookie| cookie.reply().ok()).find(|crtc| {
        let (cx, cy) = (i32::from(crtc.x), i32::from(crtc.y));
        crtc.mode != 0
            && (cx..cx.saturating_add(crtc.width.into())).contains(&x)
            && (cy..cy.saturating_add(crtc.height.into())).contains(&y)
    })?;
    mode_refresh_hz(resources.modes.iter().find(|mode| mode.id == crtc.mode)?)
}

fn mode_refresh_hz(mode: &ModeInfo) -> Option<f64> {
    let mut lines = f64::from(mode.vtotal);
    if mode.mode_flags.contains(ModeFlag::DOUBLE_SCAN) {
        lines *= 2.0;
    }
    if mode.mode_flags.contains(ModeFlag::INTERLACE) {
        lines /= 2.0;
    }
    let pixels = f64::from(mode.htotal) * lines;
    (pixels > 0.0).then(|| f64::from(mode.dot_clock) / pixels)
}

fn is_auto_repeat_pair(release: &KeyReleaseEvent, press: &KeyPressEvent) -> bool {
    release.detail == press.detail && release.time == press.time && release.event == press.event
}

fn mouse_id(id: u8) -> MouseButton {
    match id {
        1 => MouseButton::Left,
        2 => MouseButton::Middle,
        3 => MouseButton::Right,
        8 => MouseButton::Back,
        9 => MouseButton::Forward,
        id => MouseButton::Other(id),
    }
}

#[cfg(test)]
mod tests {
    use super::mode_refresh_hz;
    use x11rb::protocol::randr::{ModeFlag, ModeInfo};

    fn mode(dot_clock: u32, htotal: u16, vtotal: u16, flags: ModeFlag) -> ModeInfo {
        ModeInfo {
            id: 1,
            width: 0,
            height: 0,
            dot_clock,
            hsync_start: 0,
            hsync_end: 0,
            htotal,
            hskew: 0,
            vsync_start: 0,
            vsync_end: 0,
            vtotal,
            name_len: 0,
            mode_flags: flags,
        }
    }

    #[test]
    fn refresh_rate_from_mode_timings() {
        // CVT 1920x1080@60 reduced blanking: 138.5 MHz, 2080x1111.
        let hz = mode_refresh_hz(&mode(138_500_000, 2080, 1111, ModeFlag::from(0u32)));
        assert!((hz.unwrap_or_default() - 59.934).abs() < 0.001);
        let double = mode_refresh_hz(&mode(100, 1, 1, ModeFlag::DOUBLE_SCAN));
        assert_eq!(double, Some(50.0));
        assert_eq!(mode_refresh_hz(&mode(100, 1, 1, ModeFlag::INTERLACE)), Some(200.0));
        assert_eq!(mode_refresh_hz(&mode(100, 0, 0, ModeFlag::from(0u32))), None);
    }
}
