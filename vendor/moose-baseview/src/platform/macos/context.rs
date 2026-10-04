use crate::dpi::Size;
use crate::platform::macos::view::BaseviewView;
use crate::platform::Result;
use crate::platform::{PlatformHandle, WindowSharedState};
use crate::wrappers::appkit::{View, ViewRef};
use crate::*;
use dispatch2::MainThreadBound;
use objc2::rc::Weak;
use objc2::runtime::NSObjectProtocol;
use objc2::{MainThreadMarker, Message};
use raw_window_handle::DisplayHandle;
use std::rc::Rc;

#[derive(Clone)]
pub struct WindowContext {
    mtm: MainThreadMarker,
    view: Weak<View<BaseviewView>>,
    state: Rc<WindowSharedState>,
}

impl WindowContext {
    pub(crate) fn new(view: ViewRef<'_, BaseviewView>) -> Self {
        Self {
            view: Weak::from_retained(&view.view.retain()),
            state: Rc::clone(&view.state),
            mtm: view.mtm,
        }
    }

    pub fn request_close(&self) {
        let Some(view) = self.view.load() else { return };
        let Some(view) = view.inner_ref() else { return };
        BaseviewView::close(view, false);
    }

    pub fn has_focus(&self) -> bool {
        let Some(view) = self.view.load() else { return false };
        let Some(window) = view.window() else {
            return false;
        };

        if !window.isKeyWindow() {
            return false;
        }

        let Some(first_responder) = window.firstResponder() else {
            return false;
        };

        view.isEqual(Some(&*first_responder))
    }

    pub fn focus(&self) -> Result<()> {
        let Some(view) = self.view.load() else { return Ok(()) };
        if let Some(window) = view.window() {
            window.makeFirstResponder(Some(&view));
        }

        Ok(())
    }

    pub fn resize(&self, size: Size) -> Result<()> {
        let Some(view) = self.view.load() else { return Ok(()) };
        let Some(view) = view.inner_ref() else { return Ok(()) };
        if view.inner.state.closed.get() {
            return Ok(());
        }

        BaseviewView::resize(view, size, true, false);

        Ok(())
    }

    pub fn set_ime_configuration(&self, config: Option<crate::ImeConfiguration>) {
        let Some(view) = self.view.load() else {
            return;
        };
        let Some(inner) = view.inner_ref() else {
            return;
        };
        inner.ime.focused.set(self.has_focus());
        let config = config.filter(crate::ImeConfiguration::valid);
        if *inner.ime.configuration.borrow() == config {
            return;
        }
        let previous = inner.ime.configuration.borrow().clone();
        let switched = previous.as_ref().map(|c| &c.id) != config.as_ref().map(|c| &c.id);
        let cancelled = crate::ime::composition_cancelled(
            &previous,
            &config,
            !inner.ime.marked.borrow().is_empty(),
        );
        let enabled = config.is_some();
        let changed = inner.ime.enabled() != enabled;
        inner.ime.configure(&config);
        if changed || switched || cancelled {
            if !enabled || switched || cancelled {
                inner.ime.marked.borrow_mut().clear();
                inner.ime.discarding.set(true);
                unsafe {
                    let context: Option<objc2::rc::Retained<objc2::runtime::AnyObject>> =
                        objc2::msg_send![&*view, inputContext];
                    if let Some(context) = context {
                        let _: () = objc2::msg_send![&*context, discardMarkedText];
                    }
                }
                inner.ime.discarding.set(false);
            }
            BaseviewView::trigger_event(
                inner,
                crate::Event::Ime(if cancelled {
                    crate::Ime::Preedit { text: String::new(), cursor: None }
                } else if enabled {
                    crate::Ime::Enabled
                } else {
                    crate::Ime::Disabled
                }),
            );
        }
        unsafe {
            let context: Option<objc2::rc::Retained<objc2::runtime::AnyObject>> =
                objc2::msg_send![&*view, inputContext];
            if let Some(context) = context {
                let _: () = objc2::msg_send![&*context, invalidateCharacterCoordinates];
            }
        }
    }

    pub fn set_keyboard_capture(&self, _capture: bool) {
        // No-op: ignored key events already propagate to the host on this platform.
    }

    pub fn set_scale_factor_override(&self, _scale_factor: Option<f64>) -> Result<()> {
        // No-op on macOS: coordinates are logical and the backing scale is authoritative.
        Ok(())
    }

    pub fn set_mouse_cursor(&self, cursor: MouseCursor) -> Result<()> {
        let Some(view) = self.view.load() else { return Ok(()) };
        let Some(view) = view.inner_ref() else { return Ok(()) };

        view.inner.cursor_manager.set_cursor(cursor);

        Ok(())
    }

    pub fn size(&self) -> WindowSize {
        WindowSize::from_logical(self.state.size.get(), self.state.scale_factor.get())
    }

    pub fn scale_factor(&self) -> f64 {
        self.state.scale_factor.get()
    }

    #[cfg(feature = "opengl")]
    pub fn gl_context(&self) -> Option<crate::gl::GlContext> {
        Some(crate::gl::GlContext::new(self.view.load()?.inner()?.gl_context.get()?.clone()))
    }

    pub fn window_handle(&self) -> Option<raw_window_handle::WindowHandle<'_>> {
        View::window_handle_from_weak(&self.view)
    }

    pub fn display_handle(&self) -> DisplayHandle<'_> {
        DisplayHandle::appkit()
    }

    pub fn platform_handle(&self) -> PlatformHandle {
        PlatformHandle { inner: MainThreadBound::new(self.view.clone(), self.mtm) }
    }
}
