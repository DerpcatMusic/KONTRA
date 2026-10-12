use windows_core::{ComObject, HSTRING};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, RECT, WPARAM},
    UI::{Controls::WM_MOUSELEAVE, WindowsAndMessaging::*},
};

use crate::dpi::{PhysicalPosition, PhysicalSize, Size};
use crate::platform::frame_rate::frame_interval;
use crate::{warn, EventStatus, HandlerError, WindowHandler};
use std::cell::Cell;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Dwm::DwmFlush;

pub(crate) const BV_WINDOW_MUST_CLOSE: u32 = WM_USER + 1;
/// MOOSE: posted by `set_keyboard_capture` to move focus outside of handler callbacks.
pub(crate) const BV_KEYBOARD_CAPTURE_FOCUS: u32 = WM_USER + 2;
/// MOOSE: posted by the [`FramePacer`] thread once per compositor frame.
const BV_FRAME: u32 = WM_USER + 3;
pub(crate) const BV_IME_CONFIGURE: u32 = WM_USER + 4;

use super::drop_target::DropTarget;
use super::*;
use crate::handler::{ClosingHandler, WindowHandlerBuilder};
use crate::host::Host;
use crate::platform::win::window_state::{WindowSharedState, WindowState};
use crate::platform::PlatformError;
use crate::window::WindowInitializer;
use crate::wrappers::win32::cursor::SystemCursor;
use crate::wrappers::win32::window::*;
use crate::wrappers::win32::{
    ole_initialize, ole_uninitialize, run_thread_message_loop_until, Dpi, DpiAwarenessGuard,
    LibraryModule, Rect, WindowStyle,
};
use crate::{Event, MouseButton, MouseEvent, ScrollDelta, WindowEvent, WindowSize};

fn hi_word(wparam: WPARAM) -> u16 {
    ((wparam >> 16) & 0xffff) as u16
}

fn lo_word(lparam: LPARAM) -> u16 {
    (lparam & 0xffff) as u16
}

/// MOOSE: drives `on_frame` at the display's refresh rate. A helper thread waits on `DwmFlush`
/// (the next desktop composition) and posts [`BV_FRAME`]; `pending` keeps at most one frame
/// message queued, and is only cleared once `on_frame` returns so input is never starved.
/// A minimised window is paced at 20 Hz. Nothing here touches the process timer resolution.
struct FramePacer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FramePacer {
    fn start(hwnd: HWND, pending: Arc<AtomicBool>) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let hwnd = hwnd as usize;
        let thread =
            std::thread::Builder::new().name("baseview-frame-pacer".into()).spawn(move || {
                let hwnd = hwnd as HWND;
                let fallback = frame_interval(None);
                let mut last_frame = Instant::now();
                while !thread_stop.load(Ordering::Acquire) {
                    // SAFETY: plain queries; a stale handle just fails.
                    if unsafe { IsIconic(GetAncestor(hwnd, GA_ROOT)) } != 0 {
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }
                    // DwmFlush fails without composition and can return at once when the
                    // desktop is idle; a sleep keeps either case from spinning.
                    // SAFETY: no arguments.
                    let flushed = unsafe { DwmFlush() } >= 0;
                    if !flushed || last_frame.elapsed() < Duration::from_millis(1) {
                        std::thread::sleep(fallback);
                    }
                    last_frame = Instant::now();
                    // SAFETY: posting to a destroyed window fails harmlessly.
                    if !pending.swap(true, Ordering::AcqRel)
                        && unsafe { PostMessageW(hwnd, BV_FRAME, 0, 0) } == 0
                    {
                        pending.store(false, Ordering::Release);
                    }
                }
            })?;
        Ok(Self { stop, thread: Some(thread) })
    }
}

impl Drop for FramePacer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct WindowHandle {
    init: Cell<Option<WindowInitializer>>,
    hwnd: Cell<Option<HWnd>>,
    state: Rc<WindowSharedState>,
}

impl WindowHandle {
    pub fn run_until_closed(self) -> Result<()> {
        self.show()?;

        run_thread_message_loop_until(|| !self.is_open())?;
        Ok(())
    }

    pub fn is_open(&self) -> bool {
        self.state.is_alive.get()
    }

    pub fn is_resizable(&self) -> bool {
        self.state.sizing_strategy.is_resizable()
    }

    pub fn min_size(&self) -> Option<Size> {
        self.state.sizing_strategy.min_size()
    }

    pub fn max_size(&self) -> Option<Size> {
        self.state.sizing_strategy.max_size()
    }

    pub fn size(&self) -> WindowSize {
        self.state.size()
    }

    pub fn resize(&self, new_size: Size) -> Result<()> {
        let new_size = new_size.to_physical(self.state.scale_factor());
        let hwnd = match self.hwnd.get() {
            Some(hwnd) => hwnd,
            None => {
                self.state.current_size.set(new_size);
                return Ok(());
            }
        };

        let _guard = self.state.originate_host_resize();
        let dpi_ctx =
            DpiAwarenessGuard::new(&self.state.user32, self.state.dpi_scaling_strategy.get())?;
        hwnd.resize_and_activate(new_size, self.state.current_dpi.get(), &dpi_ctx)?;

        if self.state.current_size.get() == new_size {
            Ok(())
        } else {
            Err(PlatformError::ResizeFailed)
        }
    }

    pub fn suggest_scale_factor(&self, scale_factor: f64) -> Result<()> {
        let current_scale_factor = self.state.scale_factor();
        self.state.fallback_scale_factor.set(Some(scale_factor));

        if self.state.current_dpi.get().is_some() {
            return Ok(());
        }

        let Some(hwnd) = self.hwnd.get() else { return Ok(()) };

        let current_size = self.state.current_size.get();
        let new_size = self
            .state
            .current_size
            .get()
            .to_logical::<f64>(current_scale_factor)
            .to_physical(self.state.scale_factor());

        // This call doesn't meaningfully change the scaling factor, ignore the result
        if current_size == new_size {
            return Ok(());
        }

        let _guard = self.state.originate_host_resize();
        let dpi_ctx =
            DpiAwarenessGuard::new(&self.state.user32, self.state.dpi_scaling_strategy.get())?;

        hwnd.resize_and_activate(new_size, None, &dpi_ctx)?;

        if self.state.current_size.get() == new_size {
            Ok(())
        } else {
            Err(PlatformError::ResizeFailed)
        }
    }

    pub fn set_scale_factor_override(&self, scale_factor: Option<f64>) -> Result<()> {
        self.state.scale_factor_override.set(scale_factor);
        Ok(())
    }

    pub fn set_keyboard_capture(&self, capture: bool) {
        if let Some(hwnd) = self.hwnd.get() {
            super::window_state::set_keyboard_capture(hwnd, capture);
        }
    }

    pub fn set_parent(&self, new_parent: ParentWindowHandle) -> Result<()> {
        let hwnd = match self.hwnd.get() {
            Some(hwnd) => hwnd,
            None => {
                let Some(mut init) = self.init.take() else { return Ok(()) };
                init.settings.parent = Some(new_parent.into());

                let window = BaseviewWindow::create(Rc::clone(&self.state), init)?;
                self.hwnd.set(Some(window));

                return Ok(());
            }
        };

        if !self.state.parented.get() {
            panic!("Called set_parent on a floating window")
        }

        hwnd.set_parent(&new_parent.handle)?;

        Ok(())
    }

    #[inline]
    pub fn handle_main_thread_callback(&self) {
        // No-op
    }

    pub fn show(&self) -> Result<()> {
        let hwnd = match self.hwnd.get() {
            Some(hwnd) => hwnd,
            None => {
                let Some(init) = self.init.take() else { return Ok(()) };

                let window = BaseviewWindow::create(Rc::clone(&self.state), init)?;
                self.hwnd.set(Some(window));

                return Ok(());
            }
        };

        hwnd.show_and_activate();

        Ok(())
    }

    pub fn hide(&self) -> Result<()> {
        let Some(hwnd) = self.hwnd.get() else { return Ok(()) };
        hwnd.hide();

        Ok(())
    }
}

impl Drop for WindowHandle {
    fn drop(&mut self) {
        if !self.state.is_alive.get() {
            return;
        }

        if let Some(hwnd) = self.hwnd.take() {
            // Dispatch synchronously so normal host close releases graphics now.
            // A callback in progress defers destruction until its outer message returns.
            unsafe {
                SendMessageW(hwnd.as_raw(), BV_WINDOW_MUST_CLOSE, 1, 0);
            }
        }
    }
}

pub struct BaseviewWindow {
    window_state: Rc<WindowState>,
    shared_state: Rc<WindowSharedState>,
    initial_size: Size,

    handler_builder: Cell<Option<WindowHandlerBuilder>>,
    handler: ClosingHandler<Box<dyn WindowHandler>>,
    native_destroying: Cell<bool>,
    destroy_started: Cell<bool>,
    close_posted: Cell<bool>,
    host: Host,

    // Things not directly used, but kept so their Drop impl runs when the window is destroyed
    _keyboard_hook: Cell<Option<hook::KeyboardHookHandle>>,
    _drop_target: Cell<Option<ComObject<DropTarget>>>,
    ole_initialized: Cell<bool>,
    frame_pacer: Cell<Option<FramePacer>>,
    frame_pending: Arc<AtomicBool>,

    #[cfg(feature = "opengl")]
    pub gl_config: Option<crate::gl::GlConfig>,
}

impl BaseviewWindow {
    pub fn create(shared_state: Rc<WindowSharedState>, init: WindowInitializer) -> Result<HWnd> {
        shared_state.init(&init);

        let style = WindowStyle::from_settings(&init.settings);
        let parent = init.settings.parent.map(|p| p.inner.handle);

        let dpi_ctx =
            DpiAwarenessGuard::new(&shared_state.user32, shared_state.dpi_scaling_strategy.get())?;

        let window_size = shared_state.current_size.get();

        let initializer = {
            let shared_state = Rc::clone(&shared_state);

            move |hwnd: HWnd| {
                let window_state = Rc::new(WindowState::new(
                    hwnd,
                    shared_state.user32.clone(),
                    Rc::clone(&shared_state),
                ));

                BaseviewWindow {
                    window_state,
                    initial_size: init.settings.size,
                    handler_builder: Cell::new(Some(init.builder)),
                    handler: ClosingHandler::new(),
                    native_destroying: false.into(),
                    destroy_started: false.into(),
                    close_posted: false.into(),
                    shared_state,
                    host: init.host,

                    _drop_target: None.into(),
                    ole_initialized: false.into(),
                    frame_pacer: None.into(),
                    frame_pending: Arc::new(AtomicBool::new(false)),
                    _keyboard_hook: None.into(),

                    #[cfg(feature = "opengl")]
                    gl_config: init.settings.gl_config,
                }
            }
        };

        let rect = dpi_ctx.client_area_to_nc_area(window_size.into(), style, None)?;
        let title = HSTRING::from(init.settings.title);
        let window = create_window(&title, style, rect.size(), parent, &dpi_ctx, initializer)?;

        Ok(window)
    }

    fn finish_close(&self, window: HWnd) {
        if !self.handler.is_closing() || self.destroy_started.replace(true) {
            return;
        }
        if !self.handler.close(|handler| {
            handler.on_event(Event::Window(WindowEvent::WillClose));
        }) {
            self.destroy_started.set(false);
            return;
        }
        if !self.native_destroying.get() {
            if let Err(e) = window.destroy() {
                warn!("Failed to destroy window: {}", e);
            }
        }
    }

    fn notify_destroyed_to_host(&self) {
        if self.shared_state.destroy_host_originated.get() {
            return;
        };

        self.host.notify_destroyed()
    }

    fn request_resize_from_host(
        &self, new_size: WindowSize,
    ) -> core::result::Result<(), HandlerError> {
        if self.shared_state.resize_host_originated.get() {
            return Ok(());
        };

        self.host.request_resize(new_size)
    }

    pub(crate) fn handle_on_frame(&self) {
        if let Some(Err(e)) = self.handler.with(|handler| handler.on_frame()) {
            warn!("Error while rendering frame: {}", e);
            self.window_state.request_close();
        }
    }

    pub(crate) fn handle_event(&self, event: Event) -> EventStatus {
        let status =
            self.handler.with(|handler| handler.on_event(event)).unwrap_or(EventStatus::Ignored);
        // COM drag/drop also invokes this outside a window message. Schedule its
        // deferred close after the last callback returns, without pumping a
        // repost loop while a nested callback is still active.
        if self.handler.ready_to_close()
            && !self.native_destroying.get()
            && !self.destroy_started.get()
            && !self.close_posted.replace(true)
        {
            self.window_state.request_close();
        }
        status
    }
}

impl Drop for BaseviewWindow {
    fn drop(&mut self) {
        self.shared_state.is_alive.set(false);
        self.notify_destroyed_to_host();
    }
}

impl WindowImpl for BaseviewWindow {
    fn non_client_create(&self, window: HWnd) -> std::result::Result<(), PlatformError> {
        if self.shared_state.dpi_scaling_strategy.get().assume_96_dpi {
            window.enable_non_client_dpi_scaling(&self.shared_state.user32);
        }

        Ok(())
    }

    fn after_create(&self, window: HWnd) -> core::result::Result<(), PlatformError> {
        let window_state = &self.window_state;

        self._keyboard_hook.set(Some(hook::init_keyboard_hook(window.as_raw())));

        // Now we can get the actual dpi of the window.
        let dpi = window_state
            .shared
            .dpi_scaling_strategy
            .get()
            .get_dpi_for_window(window, &self.shared_state.user32);

        if dpi.is_some() {
            window_state.shared.current_dpi.set(dpi);
        }

        // We cannot create a window in "logical" pixels, and we can't DPI-scale to physical pixels because we
        // have no way to know where the window will end up.
        // So, at window creation, we assume a DPI=96, and if it ends up wrong, we resize the window
        // to the actual logical size the user desired.
        // MOOSE: the effective scale also honours `scale_factor_override`.
        let new_size = self.initial_size.to_physical(window_state.shared.scale_factor());
        if new_size != window_state.shared.current_size.get() {
            // Preemptively update so a synchronous WM_SIZE from SetWindowPos below
            // doesn't also emit Resized.
            window_state.shared.current_size.set(new_size);
            let guard = DpiAwarenessGuard::new(
                &window_state.shared.user32,
                self.shared_state.dpi_scaling_strategy.get(),
            )?;
            window.resize_and_activate(new_size, window_state.shared.current_dpi.get(), &guard)?;
        }

        // MOOSE: every successful OleInitialize (S_OK or S_FALSE) is balanced by an
        // OleUninitialize in `before_destroy`. A host GUI thread that is already in the
        // multithreaded apartment returns RPC_E_CHANGED_MODE: that only disables drag and
        // drop, it must not fail the whole editor window.
        match ole_initialize() {
            Ok(()) => {
                self.ole_initialized.set(true);
                let drop_target =
                    ComObject::new(DropTarget::new(Rc::downgrade(window_state), window));
                match window.register_drag_drop(drop_target.as_interface()) {
                    Ok(()) => self._drop_target.set(Some(drop_target)),
                    Err(e) => warn!("RegisterDragDrop failed, drag and drop disabled: {}", e),
                }
            }
            Err(e) => warn!("OleInitialize failed, drag and drop disabled: {}", e),
        }

        #[cfg(feature = "opengl")]
        if let Some(gl_config) = self.gl_config.clone() {
            let gl_context = gl::GlContextInner::create(window, gl_config)?;

            let Ok(()) = self.window_state.gl_context.set(Rc::new(gl_context)) else {
                unreachable!();
            };
        };

        let handler = {
            let context = crate::WindowContext::new(Rc::clone(&self.window_state));
            let Some(handler_builder) = self.handler_builder.take() else {
                unreachable!();
            };

            handler_builder.build(context)?
        };
        self.handler.set(handler);

        let pacer = FramePacer::start(window.as_raw(), Arc::clone(&self.frame_pending))
            .map_err(windows_core::Error::from)?;
        self.frame_pacer.set(Some(pacer));

        Ok(())
    }

    unsafe fn handle_message(
        &self, window: HWnd, msg: u32, wparam: WPARAM, lparam: LPARAM,
    ) -> Option<LRESULT> {
        let result = unsafe { wnd_proc_inner(window, msg, wparam, lparam, self) };
        self.finish_close(window);
        result
    }

    fn before_destroy(&self, window: HWnd) {
        // External parent/OS destruction cannot be deferred here. Normally no
        // callback is active, so release native resources while HWND is valid.
        // Forced external destruction during a reentrant callback remains the
        // host's responsibility; cleanup follows as soon as that callback returns.
        self.native_destroying.set(true);
        self.handler.request_close();
        self.finish_close(window);
        drop(self.frame_pacer.take());
        if let Some(drop_target) = self._drop_target.take() {
            let _ = window.revoke_drag_drop();
            drop(drop_target);
        }
        if self.ole_initialized.replace(false) {
            ole_uninitialize();
        }
    }
}

/// Our custom `wnd_proc` handler. If the result contains a value, then this is returned after
/// handling any deferred tasks. otherwise the default window procedure is invoked.
#[allow(clippy::unwrap_used, reason = "Refactor this in a later PR")] // TODO
unsafe fn wnd_proc_inner(
    window: HWnd, msg: u32, wparam: WPARAM, lparam: LPARAM, window_bv: &BaseviewWindow,
) -> Option<LRESULT> {
    let window_state = &window_bv.window_state;
    match msg {
        WM_MOUSEMOVE => {
            if window_state.mouse_was_outside_window.get() {
                // this makes Windows track whether the mouse leaves the window.
                // When the mouse leaves it results in a `WM_MOUSELEAVE` event.
                // Couldn't find a good way to track whether the mouse enters,
                // but if `WM_MOUSEMOVE` happens, the mouse must have entered.
                let _ = window.start_cursor_leave_tracking();
                window_state.mouse_was_outside_window.set(false);

                let enter_event = Event::Mouse(MouseEvent::CursorEntered);
                window_bv.handle_event(enter_event);
            }

            let x = (lparam & 0xFFFF) as i16 as i32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;

            let move_event = Event::Mouse(MouseEvent::CursorMoved {
                position: PhysicalPosition { x, y }.cast(),
                modifiers: window_state
                    .keyboard_state
                    .borrow()
                    .get_modifiers_from_mouse_wparam(wparam),
            });

            window_bv.handle_event(move_event);
            Some(0)
        }

        WM_MOUSELEAVE => {
            window_bv.handle_event(Event::Mouse(MouseEvent::CursorLeft));

            window_state.mouse_was_outside_window.set(true);
            Some(0)
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let value = (wparam >> 16) as i16;
            let value = value as i32;
            let value = value as f32 / WHEEL_DELTA as f32;

            let event = Event::Mouse(MouseEvent::WheelScrolled {
                delta: if msg == WM_MOUSEWHEEL {
                    ScrollDelta::Lines { x: 0.0, y: value }
                } else {
                    ScrollDelta::Lines { x: value, y: 0.0 }
                },
                modifiers: window_state
                    .keyboard_state
                    .borrow()
                    .get_modifiers_from_mouse_wparam(wparam),
            });

            window_bv.handle_event(event);
            Some(0)
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONUP | WM_RBUTTONDOWN
        | WM_RBUTTONUP | WM_XBUTTONDOWN | WM_XBUTTONUP => {
            let mut mouse_button_counter = window_state.mouse_button_counter.get();

            fn get_xbutton_wparam(wparam: WPARAM) -> u16 {
                hi_word(wparam)
            }

            const XBUTTON1: u16 = 0x1;
            const XBUTTON2: u16 = 0x2;

            let button = match msg {
                WM_LBUTTONDOWN | WM_LBUTTONUP => Some(MouseButton::Left),
                WM_MBUTTONDOWN | WM_MBUTTONUP => Some(MouseButton::Middle),
                WM_RBUTTONDOWN | WM_RBUTTONUP => Some(MouseButton::Right),
                WM_XBUTTONDOWN | WM_XBUTTONUP => match get_xbutton_wparam(wparam) {
                    XBUTTON1 => Some(MouseButton::Back),
                    XBUTTON2 => Some(MouseButton::Forward),
                    _ => None,
                },
                _ => None,
            };

            if let Some(button) = button {
                let event = match msg {
                    WM_LBUTTONDOWN | WM_MBUTTONDOWN | WM_RBUTTONDOWN | WM_XBUTTONDOWN => {
                        // Capture the mouse cursor on button down
                        mouse_button_counter = mouse_button_counter.saturating_add(1);
                        window.set_capture();
                        MouseEvent::ButtonPressed {
                            button,
                            modifiers: window_state
                                .keyboard_state
                                .borrow()
                                .get_modifiers_from_mouse_wparam(wparam),
                        }
                    }
                    WM_LBUTTONUP | WM_MBUTTONUP | WM_RBUTTONUP | WM_XBUTTONUP => {
                        // Release the mouse cursor capture when all buttons are released
                        mouse_button_counter = mouse_button_counter.saturating_sub(1);
                        if mouse_button_counter == 0 {
                            HWnd::release_capture();
                        }

                        MouseEvent::ButtonReleased {
                            button,
                            modifiers: window_state
                                .keyboard_state
                                .borrow()
                                .get_modifiers_from_mouse_wparam(wparam),
                        }
                    }
                    _ => {
                        unreachable!()
                    }
                };

                window_state.mouse_button_counter.set(mouse_button_counter);
                window_bv.handle_event(Event::Mouse(event));
            }

            None
        }
        BV_FRAME => {
            window_bv.handle_on_frame();
            window_bv.frame_pending.store(false, Ordering::Release);
            Some(0)
        }
        WM_CLOSE => {
            window_bv.handler.request_close();
            Some(0)
        }
        BV_IME_CONFIGURE => {
            if let Some(event) = window_state.ime.apply(window.as_raw()) {
                window_bv.handle_event(Event::Ime(event));
            }
            Some(0)
        }
        WM_IME_SETCONTEXT => {
            // Draw preedit in MUI, keep the OS candidate window.
            Some(unsafe {
                DefWindowProcW(window.as_raw(), msg, wparam, lparam & !(0x80000000u32 as isize))
            })
        }
        WM_IME_STARTCOMPOSITION => {
            if window_state.ime.accepts(window.as_raw()) {
                window_state.ime.composing.set(true);
                window_state.ime.position(window.as_raw());
            }
            Some(0)
        }
        WM_IME_COMPOSITION => {
            let events = window_state.ime.composition(window.as_raw(), lparam as u32);
            for event in events {
                window_bv.handle_event(Event::Ime(event));
            }
            Some(0)
        }
        WM_IME_ENDCOMPOSITION => {
            window_state.ime.composing.set(false);
            if window_state.ime.accepts(window.as_raw()) {
                window_bv.handle_event(Event::Ime(crate::Ime::Preedit {
                    text: String::new(),
                    cursor: None,
                }));
            }
            Some(0)
        }
        WM_IME_REQUEST if wparam as u32 == windows_sys::Win32::UI::Input::Ime::IMR_DOCUMENTFEED => {
            window_state.ime.reconversion(window.as_raw(), lparam)
        }
        WM_IME_CHAR => Some(0),
        WM_CHAR | WM_SYSCHAR | WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP
        | WM_INPUTLANGCHANGE => {
            let opt_event = window_state.keyboard_state.borrow_mut().process_message(
                window.as_raw(),
                msg,
                wparam,
                lparam,
            );

            if let Some(mut event) = opt_event {
                if window_state.ime.composing.get()
                    && matches!(event.key, keyboard_types::Key::Character(_))
                {
                    event.key = keyboard_types::Key::Named(keyboard_types::NamedKey::Process);
                }
                window_bv.handle_event(Event::Keyboard(event));
            }

            if msg != WM_SYSKEYDOWN {
                Some(0)
            } else {
                None
            }
        }
        WM_SETFOCUS => {
            window_bv.handle_event(Event::Window(WindowEvent::Focused));

            None
        }
        WM_KILLFOCUS => {
            window_bv.handle_event(Event::Window(WindowEvent::Unfocused));

            None
        }
        WM_SIZE => {
            let width = (lparam & 0xFFFF) as u16 as u32;
            let height = ((lparam >> 16) & 0xFFFF) as u16 as u32;

            let new_size = PhysicalSize { width, height };
            let current_size = window_state.shared.current_size.get();

            // Only send the event if anything changed
            if current_size == new_size {
                return None;
            }

            let previous = window_state.shared.current_size.replace(new_size);
            let new_size = WindowSize::from_physical(new_size, window_state.shared.scale_factor());

            if let Err(e) = window_bv.handler.with(|handler| handler.resized(new_size))? {
                warn!("Window Handler failed to resize: {}", e);
                window_state.shared.current_size.set(previous);

                if let Err(e) = window_state.resize(previous.into()) {
                    warn!("Failed to resize back to previous window size: {}", e);
                }

                return Some(-1);
            }

            if let Err(e) = window_bv.request_resize_from_host(new_size) {
                warn!("Resize request from Host failed: {}. Reverting to previous size.", e);

                if let Some(Err(e)) = window_bv.handler.with(|handler| handler.resized(new_size)) {
                    warn!("Window Handler failed to resize to previous window size: {}", e);
                }

                window_state.shared.current_size.set(previous);
                if let Err(e) = window_state.resize(previous.into()) {
                    warn!("Failed to resize back to previous window size: {}", e);
                }

                return Some(-1);
            }

            None
        }
        WM_DPICHANGED => {
            let suggested_nc_rect = Rect((lparam as *const RECT).read());
            let Some(dpi) = NonZeroU32::new((wparam & 0xFFFF) as u16 as u32) else {
                return Some(-1);
            };
            let dpi = Dpi(dpi);

            let dpi_ctx = DpiAwarenessGuard::new(
                &window_state.user32,
                window_state.shared.dpi_scaling_strategy.get(),
            )
            .unwrap();
            let style = window.get_style().unwrap();
            let suggested_rect =
                dpi_ctx.nc_area_to_client_area(suggested_nc_rect, style, Some(dpi)).unwrap();

            let new_size = suggested_rect.size();

            let dpi_changed = window_state.shared.current_dpi.get() != Some(dpi);
            let changed = window_state.shared.current_size.get() != new_size || dpi_changed;

            window_state.shared.current_dpi.set(Some(dpi));
            if dpi_changed {
                window_bv.handle_event(Event::Window(WindowEvent::ScaleFactorChanged(
                    dpi.scale_factor(),
                )));
            }
            let previous_size = window_state.shared.current_size.replace(new_size);

            // Windows makes us resize the window manually. This however will not send a WM_SIZE event,
            // hence why we are notifying the window handler manually below.
            let _ = window.set_nc_rect(suggested_nc_rect);

            if changed {
                let new_size =
                    WindowSize::from_physical(new_size, window_state.shared.scale_factor());

                if let Some(Err(e)) = window_bv.handler.with(|handler| handler.resized(new_size)) {
                    warn!("Window Handler failed to resize: {}", e);
                    window_state.shared.current_size.set(previous_size);

                    if let Err(e) = window_state.resize(previous_size.into()) {
                        warn!("Failed to resize back to previous window size: {}", e);
                    }
                }

                if let Err(e) = window_bv.request_resize_from_host(new_size) {
                    warn!("Resize request from Host failed: {}. Reverting to previous size.", e);

                    if let Some(Err(e)) =
                        window_bv.handler.with(|handler| handler.resized(new_size))
                    {
                        warn!("Window Handler failed to resize to previous window size: {}", e);
                    }

                    window_state.shared.current_size.set(previous_size);
                    if let Err(e) = window_state.resize(previous_size.into()) {
                        warn!("Failed to resize back to previous window size: {}", e);
                    }

                    return Some(-1);
                }
            }

            None
        }
        // MOOSE: child windows never get WM_DPICHANGED. Per-monitor-v2 children get
        // WM_DPICHANGED_AFTERPARENT instead once the top-level window moved to a monitor with
        // another DPI. Re-read the DPI, report it, and keep the logical size unless an
        // override pins the scale (then the handler decides what to do).
        WM_DPICHANGED_AFTERPARENT => {
            let shared = &window_state.shared;
            let Some(dpi) =
                shared.dpi_scaling_strategy.get().get_dpi_for_window(window, &window_state.user32)
            else {
                return Some(0);
            };

            let previous_scale = shared.scale_factor();
            if shared.current_dpi.replace(Some(dpi)) == Some(dpi) {
                return Some(0);
            }

            window_bv
                .handle_event(Event::Window(WindowEvent::ScaleFactorChanged(dpi.scale_factor())));

            if shared.scale_factor_override.get().is_none() {
                let new_size = shared
                    .current_size
                    .get()
                    .to_logical::<f64>(previous_scale)
                    .to_physical::<u32>(shared.scale_factor());
                if let Err(e) = window_state.resize(new_size.into()) {
                    warn!("Failed to resize after a DPI change: {}", e);
                }
            }

            Some(0)
        }
        // If WM_SETCURSOR returns `None`, WM_SETCURSOR continues to get handled by the outer window(s),
        // If it returns `Some(1)`, the current window decides what the cursor is
        WM_SETCURSOR => {
            let low_word = lo_word(lparam) as u32;
            let mouse_in_window = low_word == HTCLIENT;
            if mouse_in_window {
                // Here we need to set the cursor back to what the state says, since it can have changed when outside the window
                if let Ok(cursor) = SystemCursor::load(window_state.cursor_icon.get()) {
                    cursor.set()
                }
                Some(1)
            } else {
                // Cursor is being changed by some other window, e.g. when having mouse on the borders to resize it
                None
            }
        }
        WM_GETMINMAXINFO => {
            let sizing = window_state.shared.sizing_strategy;

            // Only implement this message if we actually need to specify a min/max size
            if let (None, None) = (sizing.min_size(), sizing.max_size()) {
                return None;
            }

            let info = lparam as *mut MINMAXINFO;

            let ctx = DpiAwarenessGuard::new(
                &window_state.user32,
                window_state.shared.dpi_scaling_strategy.get(),
            )
            .unwrap();
            let style = window.get_style().unwrap();
            let dpi = window_state.shared.current_dpi.get();

            if let Some(size) = sizing.min_size() {
                let size = size.to_physical(window_state.shared.scale_factor());
                let size =
                    ctx.client_area_to_nc_area(size.into(), style, dpi).unwrap().size().cast();
                let pt = POINT { x: size.width, y: size.height };
                (&raw mut (*info).ptMinTrackSize).write(pt);
            }

            if let Some(size) = sizing.max_size() {
                let size = size.to_physical(window_state.shared.scale_factor());
                let size =
                    ctx.client_area_to_nc_area(size.into(), style, dpi).unwrap().size().cast();
                let pt = POINT { x: size.width, y: size.height };
                (&raw mut (*info).ptMaxTrackSize).write(pt);
            }

            Some(0)
        }
        // WM_DESTROY is handled by the outer procedure, which invokes before_destroy
        // and releases the window-owned state reference.
        BV_KEYBOARD_CAPTURE_FOCUS => {
            let focused = HWnd::get_focused_window() == window.as_raw();
            if wparam != 0 {
                if !focused {
                    let _ = window.set_focus();
                }
            } else if focused {
                let parent = GetParent(window.as_raw());
                if !parent.is_null() {
                    windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(parent);
                }
            }
            Some(0)
        }
        BV_WINDOW_MUST_CLOSE => {
            window_bv.close_posted.set(false);
            if wparam != 0 {
                window_bv.shared_state.mark_host_destroy();
            }
            window_bv.handler.request_close();
            Some(0)
        }
        _ => None,
    }
}

impl WindowHandle {
    pub fn create_window(init: WindowInitializer) -> Result<WindowHandle> {
        let extended_user_32 = LibraryModule::load()?;

        let shared_state = WindowSharedState::new(extended_user_32, &init.settings);

        if init.settings.wait_for_parent && init.settings.parent.is_none() {
            return Ok(WindowHandle {
                hwnd: None.into(),
                state: shared_state,
                init: Some(init).into(),
            });
        }

        let window = BaseviewWindow::create(Rc::clone(&shared_state), init)?;

        Ok(WindowHandle { hwnd: Some(window).into(), state: shared_state, init: None.into() })
    }
}

pub fn copy_to_clipboard(_data: &str) {
    unimplemented!()
}
