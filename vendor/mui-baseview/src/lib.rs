//! A native MUI window over baseview: [`open`] parents it under a plugin
//! host's window (mui-truce's `MuiEditor` is one consumer; any other
//! framework's adapter opens the same window with its own [`View`]), and
//! [`run`] is the same window as a standalone app.
//!
//! This crate only translates baseview's events into [`mui::host::Driver`]
//! calls and presents through `mui::vello::host::Host`; the queue, the
//! frame schedule, zoom and key routing are `mui::host`'s, so another
//! window crate hosts the same [`View`] the same way.
#![deny(unsafe_code)]
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use a11y::A11y;
/// The baseview this window runs on (moose-baseview), so a consumer names
/// `Window`, `WindowSettings` and friends without depending on it itself.
pub use baseview;
use baseview::dpi::{LogicalSize, PhysicalPosition};
use baseview::{
    DropData, DropEffect, Event, EventStatus, HandlerError, MouseButton, MouseCursor, MouseEvent,
    ScrollDelta, Window, WindowContext, WindowEvent, WindowHandler, WindowSettings, WindowSize,
};
use keyboard_types::{Key as HostKey, KeyState, KeyboardEvent, Modifiers, NamedKey};
use mui::host::{Driver, Modifier, NativeKey, Wheel};
/// KONTAKTO patch: what a [`KeyHook`] is handed.
pub use mui::host::KeyEvent;
pub use mui::host::{Shared, View, lock};
use mui::prelude::{Button, Cursor, Key, Mods, Point};
use mui::vello::host::{Frame, Host, target_size};
use mui::vello::kurbo::Affine;
use raw_window_handle::HasWindowHandle;

mod timing;
pub use timing::{NativeFrameOutcome, NativeFrameSample, NativeTimingHook, NativeTimingReport, NATIVE_METRICS, NATIVE_OUTCOMES, NATIVE_TIMING_LIMIT};

const GPU_RETRY: Duration = Duration::from_millis(500);
/// KONTAKTO patch: MUI lines per wheel notch (see the wheel event below).
/// baseview does not read the system's scroll-lines setting, so this is fixed:
/// ten of the theme's 12-point lines, 120 points or five list rows.
const LINES_PER_NOTCH: f64 = 10.;
/// KONTAKTO patch: notches this close together are one fling, and each one
/// in it scrolls further than the last, up to [`MAX_GAIN`] times a notch.
const FLING: Duration = Duration::from_millis(90);
const MAX_GAIN: f64 = 3.;

/// KONTAKTO patch: lines one notch of `y` scrolls, `gap` after the last
/// notch: a fast run the same way speeds up, a pause or a turn starts over.
/// `run` is the notches in the fling so far, signed by direction.
pub fn notch_lines(y: f64, gap: Option<Duration>, run: &mut i32) -> f64 {
    let way = if y < 0. { -1 } else { 1 };
    *run = if gap.is_some_and(|g| g < FLING) && run.signum() == way { *run + way } else { way };
    let gain = (1. + 0.25 * f64::from(run.abs() - 1)).min(MAX_GAIN);
    y * LINES_PER_NOTCH * gain
}

thread_local! {
    /// KONTAKTO patch: see [`resize_corner`].
    static CORNER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// KONTAKTO patch: the pointer is over (or dragging) the window's resize
/// corner: show the diagonal resize cursor, which MUI's `Cursor` lacks.
/// Set from the view's build, on the window's thread, every frame.
pub fn resize_corner(on: bool) {
    CORNER.with(|c| c.set(on));
}

/// KONTAKTO patch: an app's look at every key event, down and up, before
/// MUI routes it. `true` takes the key: MUI and the host never see it.
pub type KeyHook = Arc<Mutex<dyn FnMut(&mui::Ui, &KeyEvent) -> bool + Send>>;

/// KONTAKTO patch: asked every tick with the frame's `Ui`: whether the
/// pointer hides now (a knob being dragged). When it stops, the pointer
/// comes back where it hid.
pub type PointerHook = Arc<Mutex<dyn FnMut(&mui::Ui) -> bool + Send>>;

/// Requests from the host's thread, applied by the window's next tick,
/// which is the only place baseview's `WindowContext` can be touched.
#[derive(Default)]
pub struct Requests {
    size: AtomicU64,
    scale: AtomicU64,
    redraw: AtomicBool,
    /// KONTAKTO patch: see [`KeyHook`].
    keys: Mutex<Option<KeyHook>>,
    /// KONTAKTO patch: see [`PointerHook`].
    pointer: Mutex<Option<PointerHook>>,
    /// KONTAKTO patch: one bounded native capture, configured before open.
    timing: Mutex<Option<NativeTimingHook>>,
}

impl Requests {
    /// KONTAKTO patch: hand every key event to `hook` first.
    pub fn on_key(&self, hook: KeyHook) {
        *self.keys.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hook);
    }

    /// KONTAKTO patch: let `hook` hide the pointer; see [`PointerHook`].
    pub fn on_pointer(&self, hook: PointerHook) {
        *self.pointer.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hook);
    }

    /// Capture the next primary drag for ten seconds; deliver one report off the frame path.
    pub fn on_native_timing(&self, hook: NativeTimingHook) {
        *self.timing.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hook);
    }

    /// Resize the child window to `width` x `height` logical points.
    pub fn resize(&self, width: u32, height: u32) {
        self.size.store(
            u64::from(width) << 32 | u64::from(height),
            Ordering::Release,
        );
    }
    /// The host's content scale changed.
    pub fn scale(&self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            self.scale.store(factor.to_bits(), Ordering::Release);
        }
    }
    /// Rebuild the tree on the next tick even if nothing it polls moved.
    pub fn redraw(&self) {
        self.redraw.store(true, Ordering::Release);
    }
}

/// Open the window under `parent`, `size` logical points. `scale` pins the
/// window's scale factor (the host's content scale); `None` follows the OS.
/// `None` back when the parent handle is unusable or the window could not be
/// made; the reason goes to [`View::log`]. Dropping the window closes it.
pub fn open<V: View + Send + 'static>(
    parent: &impl HasWindowHandle,
    title: &str,
    size: (u32, u32),
    scale: Option<f64>,
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
) -> Option<Window> {
    // baseview panics on a handle it cannot read.
    if let Err(e) = parent.window_handle() {
        log(&shared, &format!("mui-baseview: no parent window ({e})"));
        return None;
    }
    let settings = settings(title, size)
        .with_parent(parent)
        .with_scale_factor_override(scale);
    let sink = Arc::clone(&shared);
    let window =
        Window::create(settings, build(shared, requests, true)).and_then(|w| w.show().map(|()| w));
    window
        .map_err(|e| log(&sink, &format!("mui-baseview: window failed ({e})")))
        .ok()
}

/// Run a top-level window of `size` logical points at the system scale, until
/// it closes: an app's main loop. Never call it from a plugin: it tells the
/// OS this process is baseview's alone (Windows DPI awareness).
pub fn run<V: View + Send + 'static>(
    title: &str,
    size: (u32, u32),
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
) {
    // SAFETY: `run` is an app's main loop, documented as never a plugin's:
    // this process hosts no other windowing library.
    #[expect(unsafe_code, reason = "baseview's standalone-process opt-in")]
    unsafe {
        baseview::assume_standalone_in_process();
    }
    let sink = Arc::clone(&shared);
    let window = Window::create(settings(title, size), build(shared, requests, false));
    if let Err(e) = window.and_then(Window::run_until_closed) {
        log(&sink, &format!("mui-baseview: window failed ({e})"));
    }
}

fn settings(title: &str, size: (u32, u32)) -> WindowSettings {
    WindowSettings::new()
        .with_title(title)
        .with_size(LogicalSize::new(f64::from(size.0), f64::from(size.1)))
}

/// The handler constructor `open` and `run` share.
fn build<V: View + Send + 'static>(
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    parented: bool,
) -> impl FnOnce(WindowContext) -> Result<Adapter<V>, HandlerError> + Send + 'static {
    move |cx: WindowContext| {
        let size = cx.size();
        let physical = (size.physical.width, size.physical.height);
        let mut handler = Handler::new(shared, requests, physical, size.scale_factor);
        handler.a11y = Some(A11y::new());
        handler.parented = parented;
        let timed = handler.timing.is_some();
        Ok(Adapter {
            cx,
            handler: RefCell::new(handler),
            pending_resize: Cell::new(None),
            pending_events: RefCell::new(VecDeque::new()),
            timed,
            reentrant: Cell::new(0),
        })
    }
}

/// The window's event handler. Public so a framework adapter can drive it
/// headless in its own tests ([`Handler::new`], [`Handler::step`],
/// [`Handler::on_event_inner`]); a window gets one from [`open`] or [`run`].
#[doc(hidden)]
pub struct Handler<V> {
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    gpu: Option<Host>,
    gpu_retry_at: Instant,
    applied_cursor: Option<MouseCursor>,
    /// The current scene is not on screen yet: paint it.
    unpainted: bool,
    /// A screen reader's side; only a real window has one.
    a11y: Option<A11y>,
    /// A child of a host's window: keep it pinned to the parent's top.
    parented: bool,
    /// The window's scale factor: baseview's pointer is in pixels.
    scale: f64,
    /// The keyboard capture last asked of baseview.
    captured: Option<bool>,
    /// KONTAKTO patch: where the pointer last was, in physical pixels, and
    /// where it hid, while a [`PointerHook`] hides it.
    pointer_at: Option<PhysicalPosition<f64>>,
    hidden_at: Option<PhysicalPosition<f64>>,
    /// KONTAKTO patch: the last wheel notch, and the fling it is part of.
    notch: (Option<Instant>, i32),
    /// The queue and the frame schedule.
    pub driver: Driver,
    timing: Option<timing::Capture>,
}

impl<V: View> Handler<V> {
    /// A handler with no window, GPU or screen reader yet.
    pub fn new(
        shared: Arc<Mutex<Shared<V>>>,
        requests: Arc<Requests>,
        size: (u32, u32),
        scale: f64,
    ) -> Self {
        let timing = requests.timing.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone().map(|hook| timing::Capture::new(hook, size, scale));
        Self {
            shared,
            requests,
            gpu: None,
            gpu_retry_at: Instant::now(),
            applied_cursor: None,
            unpainted: true,
            a11y: None,
            parented: false,
            scale,
            captured: None,
            pointer_at: None,
            hidden_at: None,
            notch: (None, 0),
            driver: Driver::new(size, scale, Box::new(Clipboard::default())),
            timing,
        }
    }

    fn tick(&mut self, window: &WindowContext, sample: &mut Option<NativeFrameSample>) {
        let requests = &self.requests;
        let packed = requests.size.swap(0, Ordering::AcqRel);
        let bits = requests.scale.swap(0, Ordering::AcqRel);
        if packed != 0 || bits != 0 {
            // Read before the override: it changes the logical size baseview
            // reports, and a new scale keeps the window's points.
            let logical = if packed != 0 {
                LogicalSize::new((packed >> 32) as f64, (packed & u64::from(u32::MAX)) as f64)
            } else {
                window.size().logical
            };
            if bits != 0 {
                let _ = window.set_scale_factor_override(Some(f64::from_bits(bits)));
            }
            let _ = window.resize(logical);
            // Not every platform reports a resize it was asked for.
            self.resized(window.size());
        }
        // A hidden or detached editor cannot present, and on Windows this is
        // the host's GUI thread: a blocking present there freezes the host.
        let Ok(handle) = window.window_handle().map(|h| h.as_raw()) else {
            if let Some(s) = sample { s.outcome = NativeFrameOutcome::NoWindow; }
            return;
        };
        if platform::should_skip_frame(handle) {
            if let Some(s) = sample { s.outcome = NativeFrameOutcome::Hidden; }
            return;
        }
        // macOS: keep the child pinned to the parent's top as it resizes.
        // A top-level window's view is its content view: leave it be.
        if self.parented {
            platform::reanchor_to_superview_top(handle);
        }
        let now = Instant::now();
        let size = self.driver.size();
        if self.requests.redraw.swap(false, Ordering::AcqRel) {
            self.driver.redraw();
        }
        // Lost between presents: an idle editor would never find out. The
        // next present rebuilds the device.
        if self.gpu.as_ref().is_some_and(Host::device_lost) {
            self.unpainted = true;
        }
        if self.gpu.is_none() && target_size(size.0, size.1).is_some() && now >= self.gpu_retry_at {
            match open_gpu(window, size) {
                Ok(gpu) => {
                    self.gpu = Some(gpu);
                    self.unpainted = true;
                }
                Err(e) => {
                    log(
                        &self.shared,
                        &format!("mui-baseview: GPU unavailable ({e}); retrying"),
                    );
                    self.gpu_retry_at = now + GPU_RETRY;
                }
            }
        }
        // The lock covers the frame and a snapshot of its scene, not the
        // present: acquiring a surface texture can wait out a vsync, and a
        // host-thread close() or state load must not wait with it.
        // ponytail: one scene clone per painted frame; have `Ui` hand out an
        // `Arc<ResolvedScene>` if it shows in a profile.
        let mut hide = false;
        let scene = {
            let lock_at = sample.as_ref().map(|_| Instant::now());
            let mut s = lock(&self.shared);
            if let Some(sample) = sample { sample.lock_ns = timing::elapsed(lock_at); }
            let a11y = self.a11y.as_mut();
            if let Some(a11y) = &a11y
                && a11y.wants_tree()
            {
                self.driver.redraw();
            }
            if let Some(a11y) = a11y
                && a11y.apply(&mut s.ui)
            {
                self.driver.redraw();
            }
            let advance_at = sample.as_ref().map(|_| Instant::now());
            let fresh = self.driver.advance(&mut s, now);
            if let Some(sample) = sample {
                sample.advance_ns = timing::elapsed(advance_at);
                sample.new_scene = fresh;
            }
            // KONTAKTO patch: the app says whether the pointer hides.
            let hook = self.requests.pointer.lock().ok().and_then(|h| h.clone());
            if let Some(hook) = hook {
                let mut hook = hook.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                hide = hook(&s.ui);
            }
            // Keys typed into a field must not reach the host's shortcuts;
            // every other key does. Windows only; a no-op elsewhere, where an
            // ignored key already goes to the host. Only on a change: each
            // call also moves focus.
            let capture = s.ui.focus_is_text();
            if self.captured != Some(capture) {
                window.set_keyboard_capture(capture);
                self.captured = Some(capture);
            }
            if let Some(a11y) = self.a11y.as_mut()
                && (fresh || a11y.wants_tree())
            {
                a11y.publish(&s.ui);
            }
            self.unpainted |= fresh;
            if self.unpainted && self.gpu.is_some() {
                let scene_at = sample.as_ref().map(|_| Instant::now());
                let scene = s.ui.scene().cloned();
                if let Some(sample) = sample { sample.scene_ns = timing::elapsed(scene_at); }
                scene
            } else {
                None
            }
        };
        if self.gpu.is_none() {
            if let Some(sample) = sample { sample.outcome = NativeFrameOutcome::NoGpu; }
        }
        if let (Some(gpu), Some(scene)) = (self.gpu.as_mut(), scene) {
            let resize_at = sample.as_ref().map(|_| Instant::now());
            let mut resize_failed = false;
            if let Err(e) = gpu.resize(size.0, size.1) {
                resize_failed = true;
                log(&self.shared, &format!("mui-baseview: {e}"));
            }
            if let Some(sample) = sample { sample.resize_ns = timing::elapsed(resize_at); }
            let present_at = sample.as_ref().map(|_| Instant::now());
            let presented = gpu.present(&scene, Affine::scale(self.driver.ui_scale()));
            if let Some(sample) = sample {
                sample.present_ns = timing::elapsed(present_at);
                sample.outcome = if resize_failed { NativeFrameOutcome::Error } else { match &presented {
                    Ok(Frame::Presented(_)) => NativeFrameOutcome::Presented,
                    Ok(Frame::Current) => NativeFrameOutcome::Current,
                    Ok(Frame::Skipped) => NativeFrameOutcome::Skipped,
                    Ok(Frame::SurfaceLost) => NativeFrameOutcome::SurfaceLost,
                    Err(_) => NativeFrameOutcome::Error,
                }};
            }
            match presented {
                Ok(Frame::Presented(_) | Frame::Current) => self.unpainted = false,
                Ok(Frame::Skipped) => {}
                Ok(Frame::SurfaceLost) => {
                    // SAFETY: the surface comes from this window's live
                    // native handle, and baseview drops the handler that owns
                    // it before the window.
                    #[expect(unsafe_code, reason = "calls the unsafe surface constructor")]
                    let surface = unsafe { surface::create(gpu.instance(), window) };
                    if let Some(surface) = surface {
                        gpu.replace_surface(surface);
                    } else {
                        log(&self.shared, "mui-baseview: surface lost; rebuilding");
                        self.gpu = None;
                        self.gpu_retry_at = now + GPU_RETRY;
                    }
                }
                Err(e) => {
                    // Not a lost surface: painting it again would fail
                    // again. A lost device rebuilds on its own schedule.
                    log(&self.shared, &format!("mui-baseview: {e}"));
                    self.unpainted = false;
                }
            }
        }
        // KONTAKTO patch: hidden where a drag began; back there after.
        match (hide, self.hidden_at) {
            (true, None) => self.hidden_at = self.pointer_at,
            (false, Some(at)) => {
                platform::warp_pointer(window, at.x, at.y, self.scale);
                self.hidden_at = None;
            }
            _ => {}
        }
        let cursor = if self.hidden_at.is_some() {
            MouseCursor::Hidden
        } else if CORNER.with(std::cell::Cell::get) {
            MouseCursor::NwseResize
        } else {
            native_cursor(self.driver.cursor())
        };
        if self.applied_cursor != Some(cursor) {
            let _ = window.set_mouse_cursor(cursor);
            self.applied_cursor = Some(cursor);
        }
    }

    /// One display tick without a window or GPU, a frame's time after the
    /// last: what the headless tests drive.
    pub fn step(&mut self) -> bool {
        if self.requests.redraw.swap(false, Ordering::AcqRel) {
            self.driver.redraw();
        }
        let now = self.driver.last_frame() + Duration::from_millis(16);
        self.driver.advance(&mut lock(&self.shared), now)
    }

    /// The window's new size, as baseview reports it.
    pub fn resized(&mut self, size: WindowSize) {
        self.scale = size.scale_factor;
        let physical = (size.physical.width, size.physical.height);
        self.driver.resized(physical, size.scale_factor);
        if let Some(capture) = &mut self.timing { capture.geometry(physical, size.scale_factor); }
    }

    /// One native event, as baseview delivers it.
    pub fn on_event_inner(&mut self, event: &Event) -> EventStatus {
        if let Some(capture) = &mut self.timing {
            match event {
                Event::Mouse(MouseEvent::ButtonPressed { button: MouseButton::Left, .. }) => capture.primary = true,
                Event::Mouse(MouseEvent::ButtonReleased { button: MouseButton::Left, .. })
                | Event::Window(WindowEvent::Unfocused) => capture.primary = false,
                Event::Mouse(MouseEvent::CursorMoved { .. }) => capture.pointer_move(Instant::now()),
                _ => {}
            }
        }
        let scale = self.scale;
        let points = |p: PhysicalPosition<f64>| Point::new(p.x / scale, p.y / scale);
        let d = &mut self.driver;
        match event {
            Event::Keyboard(key) => {
                // KONTAKTO patch: the app's hook first.
                let event = key_event(key);
                let hook = self.requests.keys.lock().ok().and_then(|h| h.clone());
                if let Some(hook) = hook {
                    let mut hook = hook.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    if hook(&lock(&self.shared).ui, &event) {
                        return EventStatus::Captured;
                    }
                }
                let kept = d.key(&lock(&self.shared), &event);
                return if kept {
                    EventStatus::Captured
                } else {
                    EventStatus::Ignored
                };
            }
            Event::Mouse(mouse) => match *mouse {
                MouseEvent::CursorMoved {
                    position,
                    modifiers,
                } => {
                    // KONTAKTO patch: kept to put a hidden pointer back.
                    self.pointer_at = Some(position);
                    d.pointer_moved(points(position), mods(modifiers));
                }
                MouseEvent::ButtonPressed { button, modifiers }
                | MouseEvent::ButtonReleased { button, modifiers } => {
                    if let Some(b) = mouse_button(button) {
                        let down = matches!(mouse, MouseEvent::ButtonPressed { .. });
                        d.button(b, down, mods(modifiers));
                    }
                }
                MouseEvent::WheelScrolled { delta, modifiers } => {
                    let wheel = match delta {
                        // KONTAKTO patch: baseview reports one line per notch on every
                        // platform, and MUI makes a line one text height
                        // (12 points): half a list row. A notch moves
                        // five rows, more in a fast run (`notch_lines`);
                        // trackpads send pixels and stay exact. The lists
                        // glide to where the notch puts them.
                        ScrollDelta::Lines { x, y } => {
                            let now = Instant::now();
                            let gap = self.notch.0.map(|t| now - t);
                            self.notch.0 = Some(now);
                            let along = if y != 0. { y } else { x };
                            let lines = notch_lines(f64::from(along), gap, &mut self.notch.1);
                            if y != 0. { Wheel::Lines(0., lines) } else { Wheel::Lines(lines, 0.) }
                        }
                        ScrollDelta::Pixels { x, y } => Wheel::Pixels(f64::from(x), f64::from(y)),
                    };
                    d.wheel(wheel, mods(modifiers));
                }
                MouseEvent::CursorLeft | MouseEvent::DragLeft => {
                    // KONTAKTO patch: a release may still be waiting for a
                    // frame. Do not restore an already-outside pointer.
                    self.pointer_at = None;
                    self.hidden_at = None;
                    d.pointer_left();
                }
                MouseEvent::DragEntered {
                    position,
                    modifiers,
                    ref data,
                }
                | MouseEvent::DragMoved {
                    position,
                    modifiers,
                    ref data,
                }
                | MouseEvent::DragDropped {
                    position,
                    modifiers,
                    ref data,
                } => {
                    let at = points(position);
                    let DropData::Files(paths) = data else {
                        d.pointer_moved(at, mods(modifiers));
                        return EventStatus::Ignored;
                    };
                    let dropped = matches!(mouse, MouseEvent::DragDropped { .. });
                    let s = &mut *lock(&self.shared);
                    return if d.drop_files(s, at, mods(modifiers), paths, dropped) {
                        EventStatus::AcceptDrop(DropEffect::Copy)
                    } else {
                        EventStatus::Ignored
                    };
                }
                _ => {}
            },
            Event::Window(e @ (WindowEvent::Focused | WindowEvent::Unfocused)) => {
                let focused = matches!(e, WindowEvent::Focused);
                if !focused {
                    // KONTAKTO patch: never warp back into an inactive editor.
                    self.pointer_at = None;
                    self.hidden_at = None;
                }
                d.focus(focused);
                if let Some(a11y) = self.a11y.as_mut() {
                    a11y.focus(focused);
                }
            }
            Event::Window(WindowEvent::WillClose) => d.close(&mut lock(&self.shared)),
            // A scale change arrives as a resize too.
            _ => {}
        }
        EventStatus::Captured
    }
}

/// baseview calls its handler through `&self`, and a call can re-enter it
/// (on Windows, a resize from inside a frame is reported before `resize`
/// returns). A call that finds the handler busy is kept, the latest resize
/// and every event, and delivered once the outer call returns.
struct Adapter<V> {
    cx: WindowContext,
    handler: RefCell<Handler<V>>,
    pending_resize: Cell<Option<WindowSize>>,
    pending_events: RefCell<VecDeque<Event>>,
    timed: bool,
    reentrant: Cell<u64>,
}

impl<V: View> Adapter<V> {
    fn drain(&self, h: &mut Handler<V>) {
        if let Some(size) = self.pending_resize.take() {
            guard(h, |h| h.resized(size));
        }
        while let Some(event) = self.pending_events.borrow_mut().pop_front() {
            guard(h, |h| h.on_event_inner(&event));
        }
    }
}

/// baseview calls from a platform callback: a panic crossing it takes the
/// host down, not just the editor. The guard only exists under
/// `panic = "unwind"` (the `plugin` profile); under release's abort the
/// panic kills the process before it gets here.
fn guard<V: View, R>(h: &mut Handler<V>, f: impl FnOnce(&mut Handler<V>) -> R) -> Option<R> {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(h))).ok();
    if r.is_none() {
        log(
            &h.shared,
            "mui-baseview: panic in the window, swallowed at the FFI edge",
        );
    }
    r
}

impl<V: View + 'static> WindowHandler for Adapter<V> {
    fn on_frame(&self) -> Result<(), HandlerError> {
        if let Ok(mut h) = self.handler.try_borrow_mut() {
            let at = h.timing.as_ref().map(|_| Instant::now());
            self.drain(&mut h);
            let mut sample = at.and_then(|at| h.timing.as_mut().and_then(|c| c.begin(at)));
            if sample.as_ref().is_some_and(|s| s.interval_ns == 0) { self.reentrant.set(0); }
            let completed = guard(&mut h, |h| h.tick(&self.cx, &mut sample)).is_some();
            self.drain(&mut h);
            if let (Some(mut sample), Some(at)) = (sample, at) {
                sample.total_ns = timing::ns(at.elapsed());
                if !completed { sample.outcome = NativeFrameOutcome::Panicked; }
                if let Some(capture) = &mut h.timing {
                    capture.record(sample, Instant::now(), self.reentrant.replace(0));
                }
            }
        } else if self.timed {
            self.reentrant.set(self.reentrant.get().saturating_add(1));
        }
        Ok(())
    }

    fn resized(&self, size: WindowSize) -> Result<(), HandlerError> {
        match self.handler.try_borrow_mut() {
            Ok(mut h) => {
                guard(&mut h, |h| h.resized(size));
                self.drain(&mut h);
            }
            Err(_) => self.pending_resize.set(Some(size)),
        }
        Ok(())
    }

    fn on_event(&self, event: Event) -> EventStatus {
        let Ok(mut h) = self.handler.try_borrow_mut() else {
            self.pending_events.borrow_mut().push_back(event);
            return EventStatus::Ignored;
        };
        let status = guard(&mut h, |h| h.on_event_inner(&event)).unwrap_or(EventStatus::Ignored);
        self.drain(&mut h);
        status
    }
}

/// A baseview key as `mui::host` names it.
fn key_event(key: &KeyboardEvent) -> KeyEvent {
    let mut code = DefaultHasher::new();
    key.code.hash(&mut code);
    KeyEvent {
        code: code.finish(),
        key: match &key.key {
            HostKey::Character(s) => NativeKey::Text(s.clone()),
            HostKey::Named(NamedKey::Shift) => NativeKey::Modifier(Modifier::Shift),
            HostKey::Named(NamedKey::Control) => NativeKey::Modifier(Modifier::Ctrl),
            HostKey::Named(NamedKey::Alt | NamedKey::AltGraph) => {
                NativeKey::Modifier(Modifier::Alt)
            }
            HostKey::Named(NamedKey::Meta) => NativeKey::Modifier(Modifier::Cmd),
            // keyboard-types prints the W3C name `Key::from_name` reads.
            HostKey::Named(other) => {
                Key::from_fmt(format_args!("{other}")).map_or(NativeKey::Other, NativeKey::Named)
            }
        },
        down: key.state == KeyState::Down,
        mods: mods(key.modifiers),
    }
}

const fn mouse_button(b: MouseButton) -> Option<Button> {
    match b {
        MouseButton::Left => Some(Button::Primary),
        MouseButton::Right => Some(Button::Secondary),
        MouseButton::Middle => Some(Button::Middle),
        _ => None,
    }
}

const fn mods(m: Modifiers) -> Mods {
    Mods {
        shift: m.contains(Modifiers::SHIFT),
        ctrl: m.contains(Modifiers::CONTROL),
        alt: m.contains(Modifiers::ALT),
        cmd: m.contains(Modifiers::META),
    }
}

const fn native_cursor(cursor: Cursor) -> MouseCursor {
    match cursor {
        Cursor::Arrow => MouseCursor::Default,
        Cursor::Hand | Cursor::Grab => MouseCursor::Hand,
        Cursor::Grabbing => MouseCursor::HandGrabbing,
        Cursor::Text => MouseCursor::Text,
        Cursor::ResizeH => MouseCursor::EwResize,
        Cursor::ResizeV => MouseCursor::NsResize,
        Cursor::Crosshair => MouseCursor::Crosshair,
        Cursor::Forbidden => MouseCursor::NotAllowed,
    }
}

/// X11 drops a selection when its owner goes, so Linux keeps an arboard
/// owner alive. baseview writes the clipboard elsewhere but cannot read it:
/// there, a paste gets what this editor copied last.
///
/// Also a [`mui::Clipboard`], for a baseview host of your own:
/// `Ui::new(theme).clipboard(Clipboard::default())`.
#[derive(Default)]
pub struct Clipboard {
    #[cfg(target_os = "linux")]
    x11: Option<arboard::Clipboard>,
    #[cfg(not(target_os = "linux"))]
    last: Option<String>,
}

impl mui::Clipboard for Clipboard {
    fn get(&mut self) -> Option<String> {
        self.read()
    }
    fn set(&mut self, text: &str) {
        self.write(text);
    }
}

impl Clipboard {
    #[cfg(target_os = "linux")]
    fn read(&mut self) -> Option<String> {
        if self.x11.is_none() {
            self.x11 = arboard::Clipboard::new().ok();
        }
        self.x11.as_mut()?.get_text().ok()
    }
    #[cfg(target_os = "linux")]
    fn write(&mut self, text: &str) {
        if self.x11.is_none() {
            self.x11 = arboard::Clipboard::new().ok();
        }
        if let Some(x11) = self.x11.as_mut() {
            let _ = x11.set_text(text.to_owned());
        }
    }
    #[cfg(not(target_os = "linux"))]
    fn read(&mut self) -> Option<String> {
        self.last.clone()
    }
    #[cfg(not(target_os = "linux"))]
    fn write(&mut self, text: &str) {
        baseview::copy_to_clipboard(text);
        self.last = Some(text.to_owned());
    }
}

fn log<V: View>(shared: &Mutex<Shared<V>>, line: &str) {
    lock(shared).view.log(line);
}

/// A device and renderer for this window's surface. A driver panic becomes
/// an error the editor can show, when the plugin unwinds; under
/// `panic = "abort"` it is the host's crash.
fn open_gpu(window: &WindowContext, size: (u32, u32)) -> Result<Host, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        // SAFETY: the surface comes from this window's live native handle,
        // and baseview drops the handler that owns it before the window.
        #[expect(unsafe_code, reason = "calls the unsafe surface constructor")]
        let surface = unsafe { surface::create(&instance, window) }
            .ok_or("native surface creation failed")?;
        Host::new(instance, surface, size).map_err(|e| e.to_string())
    }))
    .map_err(|_| "panic while creating GPU resources".to_owned())?
}

mod a11y;
mod platform;
mod surface;
#[cfg(test)]
mod tests;
