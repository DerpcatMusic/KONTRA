use super::*;
use crate::platform::Result;

/// Windows removes the handler before destroying its HWND. A close requested
/// from a callback waits until that callback releases its borrow, as on Cocoa.
#[cfg(any(target_os = "windows", test))]
pub(crate) struct ClosingHandler<T> {
    inner: std::cell::RefCell<Option<T>>,
    closing: std::cell::Cell<bool>,
    cleaning: std::cell::Cell<bool>,
}

#[cfg(any(target_os = "windows", test))]
impl<T> ClosingHandler<T> {
    pub fn new() -> Self {
        Self { inner: std::cell::RefCell::new(None), closing: false.into(), cleaning: false.into() }
    }
    pub fn set(&self, handler: T) {
        *self.inner.borrow_mut() = Some(handler);
    }
    pub fn with<R>(&self, callback: impl FnOnce(&T) -> R) -> Option<R> {
        if self.closing.get() {
            return None;
        }
        let inner = self.inner.borrow();
        Some(callback(inner.as_ref()?))
    }
    pub fn request_close(&self) {
        self.closing.set(true);
    }
    pub fn is_closing(&self) -> bool {
        self.closing.get()
    }
    pub fn ready_to_close(&self) -> bool {
        self.closing.get() && !self.cleaning.get() && self.inner.try_borrow_mut().is_ok()
    }
    /// Returns true only once callbacks have returned and the handler is gone.
    /// The notification and destructor run without holding the container borrow.
    pub fn close(&self, notify: impl FnOnce(&T)) -> bool {
        self.closing.set(true);
        if self.cleaning.get() {
            return false;
        }
        let Ok(mut inner) = self.inner.try_borrow_mut() else { return false };
        self.cleaning.set(true);
        let handler = inner.take();
        drop(inner);
        if let Some(handler) = handler {
            notify(&handler);
            drop(handler);
        }
        self.cleaning.set(false);
        true
    }
}

#[cfg(test)]
mod close_tests {
    use super::ClosingHandler;
    use std::{
        cell::RefCell,
        rc::{Rc, Weak},
    };

    #[test]
    fn callback_close_defers_cleanup_until_before_native_destroy() {
        struct Handler {
            events: Rc<RefCell<Vec<&'static str>>>,
            owner: Weak<ClosingHandler<Handler>>,
        }
        impl Drop for Handler {
            fn drop(&mut self) {
                self.events.borrow_mut().push("drop renderer");
                assert!(!self
                    .owner
                    .upgrade()
                    .unwrap()
                    .close(|_| panic!("recursive destructor close")));
            }
        }
        let events = Rc::new(RefCell::new(Vec::new()));
        let handler = Rc::new(ClosingHandler::new());
        handler.set(Handler { events: Rc::clone(&events), owner: Rc::downgrade(&handler) });
        handler.with(|_| {
            events.borrow_mut().push("callback");
            assert_eq!(handler.with(|_| "nested event"), Some("nested event"));
            handler.request_close();
            assert!(!handler.ready_to_close());
            assert!(!handler.close(|_| panic!("cleanup while callback active")));
            assert!(handler.with(|_| ()).is_none());
            events.borrow_mut().push("callback returns");
        });
        assert!(handler.is_closing());
        assert!(handler.ready_to_close());
        assert!(handler.close(|_| {
            events.borrow_mut().push("WillClose");
            assert!(!handler.close(|_| panic!("recursive cleanup")));
        }));
        events.borrow_mut().push("destroy HWND");
        assert!(handler.close(|_| panic!("duplicate WillClose")));
        assert_eq!(
            *events.borrow(),
            ["callback", "callback returns", "WillClose", "drop renderer", "destroy HWND"]
        );
    }
}

pub trait WindowHandler: 'static {
    /// Requests the handler to draw a new frame.
    ///
    /// If this returns an error, the window will be considered unable to render its contents, and
    /// will be subsequently closed.
    fn on_frame(&self) -> core::result::Result<(), HandlerError>;
    /// Informs the handler that the window has been resized.
    ///
    /// # Errors
    ///
    /// This operation can fail, in which case an [`HandlerError`] can be returned.
    /// This can happen if e.g. an underlying buffer could not be resized, or some kind of driver error.
    ///
    /// In case this `resized` operation fails, `baseview` will assume that it did not meaningfully
    /// change anything, and that the window is still able to render and operate at the previous size.
    ///
    /// It will also attempt to resize the underlying platform window and parent window back to the
    /// previous size, but this is only a best-effort attempt since those operations can also fail.
    fn resized(&self, new_size: WindowSize) -> core::result::Result<(), HandlerError>;
    fn on_event(&self, event: Event) -> EventStatus;
}

type DynBuilderResult = core::result::Result<Box<dyn WindowHandler>, HandlerError>;

pub struct WindowHandlerBuilder {
    inner: Box<dyn FnOnce(WindowContext) -> DynBuilderResult + Send + 'static>,
}

impl WindowHandlerBuilder {
    pub fn new<H: WindowHandler>(
        f: impl FnOnce(WindowContext) -> core::result::Result<H, HandlerError> + Send + 'static,
    ) -> WindowHandlerBuilder {
        Self { inner: Box::new(|c| Ok(Box::new(f(c)?))) }
    }

    pub fn build(self, ctx: WindowContext) -> Result<Box<dyn WindowHandler>> {
        match (self.inner)(ctx) {
            Ok(handle) => Ok(handle),
            Err(e) => Err(platform::PlatformError::Handler(e)),
        }
    }
}
