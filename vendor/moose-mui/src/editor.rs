//! moose's `Editor` over the window in [`crate::window`].
use std::sync::{Arc, Mutex};

use moose_core::editor::{Editor, PluginContext, RawWindowHandle};
use moose_params::Params;
use mui::Ui;
use mui::input::Input;
use mui::scene::{El, Size};

use crate::Bridge;
use crate::platform::{HostScale, ParentWindow};
use crate::window::{self, Requests, Shared, View, lock};

type Build<P> = Box<dyn FnMut(&mut Ui, &mut Bridge<P>) -> El + Send>;
type FileDrop = Box<dyn FnMut(&Ui, mui::prelude::Point, &[std::path::PathBuf], bool) -> bool + Send>;
type Changed = Box<dyn FnMut() -> bool + Send>;

/// The model half: the app's build closure and the parameters it binds.
pub(crate) struct Session<P: Params> {
    pub(crate) bridge: Bridge<P>,
    build: Build<P>,
    /// [`MuiEditor::changed`]: model state outside the bridge.
    changed: Option<Changed>,
    file_drop: Option<FileDrop>,
    cancel: Option<Box<dyn FnMut(&Ui) + Send>>,
    /// The size `build` was designed at, fitted to the window; `None` is
    /// [`MuiEditor::fixed_zoom`].
    design: Option<Size>,
}

impl<P: Params> View for Session<P> {
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        let root = (self.build)(ui, &mut self.bridge);
        self.bridge.end_unbound();
        root
    }
    fn drop_files(&mut self, ui: &Ui, at: mui::prelude::Point, paths: &[std::path::PathBuf], dropped: bool) -> bool {
        self.file_drop.as_mut().is_some_and(|f| f(ui, at, paths, dropped))
    }
    fn cancel(&mut self, ui: &Ui) {if let Some(f)=&mut self.cancel {f(ui);}}
    fn changed(&mut self) -> bool {
        // Both run every tick: the bridge snapshots values as it compares.
        self.bridge.changed() | self.changed.as_mut().is_some_and(|f| f())
    }
    fn zoom(&self, window: Size) -> f64 {
        self.design.map_or(1.0, |d| {
            (window.width / d.width).min(window.height / d.height)
        })
    }
    fn request_resize(&mut self, width: u32, height: u32) -> bool {
        self.bridge
            .context()
            .is_some_and(|c| c.request_resize(width, height))
    }
}

/// A MUI editor for any moose plugin: `build` makes the tree every frame
/// from the retained `Ui` and a [`Bridge`] to the plugin's parameters.
/// State the editor keeps between frames lives in the closure's captures;
/// it and the `Ui` survive a close and reopen.
///
/// The editor redraws when the bridge sees a moose parameter or meter move.
/// Model state outside those -- meters, status, anything in the plugin's own
/// atomics -- must report through [`MuiEditor::changed`], or it only repaints
/// when the mouse moves; `.changed(|| true)` rebuilds every tick.
///
/// A resized window scales the whole tree to fit the `size` it was designed
/// at (`min` of the two axes' ratios, so 1.0 at that size); the spare axis
/// gets the extra room. [`MuiEditor::fixed_zoom`] keeps it at 1.0 instead,
/// so the tree lays out into the new size unscaled.
///
/// ```ignore
/// fn editor(params: Arc<GainParams>) -> Box<dyn Editor> {
///     let ui = Ui::default().font(font);
///     MuiEditor::new(params, ui, (320, 200), |ui, bridge| {
///         bridge.bind(ui, P::Gain, |ui, id, v| knob(ui, id, "Gain", v, 0.0..=1.0))
///     })
///     .resizable((240, 160))
///     // The audio thread sets `dirty` when a meter moves.
///     .changed(move || dirty.swap(false, Ordering::Relaxed))
///     .into_editor()
/// }
/// ```
///
/// # Panics and build profile
///
/// The editor's window callbacks catch panics so a UI bug stays in the UI.
/// That only works when the plugin cdylib unwinds: build it with the
/// workspace's `plugin` profile (`cargo build --profile plugin`), never plain
/// `--release`, whose `panic = "abort"` makes every UI panic abort the DAW.
pub struct MuiEditor<P: Params> {
    shared: Arc<Mutex<Shared<Session<P>>>>,
    requests: Arc<Requests>,
    params: Arc<P>,
    size: (u32, u32),
    min: Option<(u32, u32)>,
    pub(crate) scale: HostScale,
    window: Option<Handle>,
    /// [`MuiEditor::on_key`], handed to each window it opens.
    keys: Option<window::KeyHook>,
    /// [`MuiEditor::hide_pointer`], handed to each window it opens.
    pointer: Option<window::PointerHook>,
    timing: Option<window::NativeTimingHook>,
}

/// Whole logical points, as the host's window API takes them.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a window is a few thousand points, never negative"
)]
fn points(s: Size) -> (u32, u32) {
    (
        s.width.round().max(0.0) as u32,
        s.height.round().max(0.0) as u32,
    )
}

/// The one field of [`MuiEditor`] that is not auto-`Send`.
struct Handle(window::baseview::Window);

// SAFETY: `baseview::Window` wraps a native window pointer and is not
// auto-`Send`. moose calls `open`, `set_size`, `window_scale` and `close`
// from one GUI thread, never concurrently, so the window never leaves the
// thread that made it; `Send` is only what `Box<dyn Editor>` asks for.
// moose-gui's own `GpuEditor` makes the same argument.
#[expect(unsafe_code, reason = "vouches Send for the baseview window alone")]
unsafe impl Send for Handle {}

impl<P: Params> MuiEditor<P> {
    /// A fixed-size editor, `size` logical points.
    pub fn new(
        params: Arc<P>,
        ui: Ui,
        size: impl Into<Size>,
        build: impl FnMut(&mut Ui, &mut Bridge<P>) -> El + Send + 'static,
    ) -> Self {
        let size = size.into();
        let session = Session {
            bridge: Bridge::new(Arc::clone(&params)),
            build: Box::new(build),
            changed: None,
            file_drop: None,
            cancel: None,
            design: Some(size),
        };
        Self {
            shared: Arc::new(Mutex::new(Shared { ui, view: session })),
            requests: Arc::default(),
            params,
            size: points(size),
            min: None,
            scale: HostScale::default(),
            window: None,
            keys: None,
            pointer: None,
            timing: None,
        }
    }

    /// Let the host resize the window, down to `min` logical points.
    #[must_use]
    pub fn resizable(mut self, min: impl Into<Size>) -> Self {
        self.min = Some(points(min.into()));
        self
    }

    /// Poll `f` every display tick alongside the parameters: `true` means
    /// the model moved (a meter, a status) and the tree rebuilds.
    #[must_use]
    pub fn changed(self, f: impl FnMut() -> bool + Send + 'static) -> Self {
        lock(&self.shared).view.changed = Some(Box::new(f));
        self
    }

    /// Handle native file drags. Return true to accept; mutate only when `dropped`.
    #[must_use]
    pub fn on_files(self, f: impl FnMut(&Ui, mui::prelude::Point, &[std::path::PathBuf], bool) -> bool + Send + 'static) -> Self {
        lock(&self.shared).view.file_drop = Some(Box::new(f));
        self
    }

    /// Release app-owned gestures when focus leaves or the window closes.
    #[must_use]
    pub fn on_cancel(self, f: impl FnMut(&Ui) + Send + 'static) -> Self {
        lock(&self.shared).view.cancel = Some(Box::new(f));
        self
    }

    /// See every key event, down and up, before MUI routes it; `true` takes
    /// the key from MUI and the host. Runs on the window's thread with the
    /// last frame's `Ui`.
    #[must_use]
    pub fn on_key(mut self, f: impl FnMut(&Ui, &window::KeyEvent) -> bool + Send + 'static) -> Self {
        self.keys = Some(Arc::new(Mutex::new(f)));
        self
    }

    /// Hide the pointer while `f` says so (a knob being dragged), and put it
    /// back where it hid once `f` stops. Asked every display tick with the
    /// frame's `Ui`, on the window's thread.
    #[must_use]
    pub fn hide_pointer(mut self, f: impl FnMut(&Ui) -> bool + Send + 'static) -> Self {
        self.pointer = Some(Arc::new(Mutex::new(f)));
        self
    }

    /// Opt-in native callback capture: one ten-second primary drag report per window.
    #[must_use]
    pub fn native_timing(mut self, hook: Option<window::NativeTimingHook>) -> Self {
        self.timing = hook;
        self
    }

    /// Keep the zoom at 1.0 when the window resizes: the tree lays out into
    /// the new size instead of scaling to fit its design size.
    #[must_use]
    pub fn fixed_zoom(self) -> Self {
        lock(&self.shared).view.design = None;
        self
    }

    fn close_window(&mut self) {
        if let Some(Handle(window)) = self.window.take() {
            #[cfg(target_os = "linux")]
            {
                window.close_bounded(std::time::Duration::from_millis(250));
            }
            #[cfg(not(target_os = "linux"))]
            window.close();
        }
    }
}

impl<P: Params> Editor for MuiEditor<P> {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn can_resize(&self) -> bool {
        self.min.is_some()
    }

    fn min_size(&self) -> (u32, u32) {
        self.min.unwrap_or(self.size)
    }

    fn open(&mut self, parent: RawWindowHandle, context: PluginContext) {
        if self.window.is_some() {
            self.close();
        }
        lock(&self.shared)
            .view
            .bridge
            .attach(context.with_params(Arc::clone(&self.params)));
        // A request made while closed was for the last window.
        self.requests = Arc::default();
        if let Some(keys) = &self.keys {
            self.requests.on_key(Arc::clone(keys));
        }
        if let Some(pointer) = &self.pointer {
            self.requests.on_pointer(Arc::clone(pointer));
        }
        if let Some(timing) = &self.timing {
            self.requests.on_native_timing(Arc::clone(timing));
        }
        self.window = window::open(
            &ParentWindow(parent),
            "MUI",
            self.size,
            self.scale.policy(),
            Arc::clone(&self.shared),
            Arc::clone(&self.requests),
        )
        .map(Handle);
    }

    fn close(&mut self) {
        {
            // Released before the window closes: on macOS and Windows
            // baseview tears the handler down on this thread, inside
            // `close`, and its last event would wait on this lock.
            let mut s = lock(&self.shared);
            let s = &mut *s;
            s.view.bridge.close();
            // The bridge ended the host's gestures; these are the same edges.
            s.ui.close();
            // And the app's own (a held note): the window's last event may
            // never come, and a host may build a fresh editor next time.
            s.view.cancel(&s.ui);
        }
        self.close_window();
    }

    fn set_size(&mut self, width: u32, height: u32) -> bool {
        match self.min {
            Some((w, h)) if width >= w && height >= h => {
                self.size = (width, height);
                self.requests.resize(width, height);
                true
            }
            _ => false,
        }
    }

    fn set_scale_factor(&mut self, factor: f64) {
        self.scale.set(factor);
        self.requests.scale(factor);
    }

    fn window_scale(&self) -> Option<f64> {
        let open = self.window.as_ref().filter(|w| w.0.is_open())?;
        (!cfg!(target_os = "macos")).then(|| open.0.size().scale_factor)
    }

    fn state_changed(&mut self) {
        // A preset replaced what a gesture in flight was editing: end it,
        // and stop the drag so it cannot keep writing the old value.
        let mut s = lock(&self.shared);
        s.ui.cancel();
        s.view.bridge.end_all();
        self.requests.redraw();
    }
}

impl<P: Params> Drop for MuiEditor<P> {
    fn drop(&mut self) {
        // The host may have torn its side down already: no callbacks here.
        lock(&self.shared).view.bridge.detach();
        self.close_window();
    }
}

#[cfg(test)]
mod tests;
