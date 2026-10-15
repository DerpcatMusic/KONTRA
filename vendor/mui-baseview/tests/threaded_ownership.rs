//! Type-level ownership contract; native execution is covered by x11_visibility.
#![cfg(target_os = "linux")]

use kontra_native_host::baseview::{ThreadedWindow, Window, host::HostCallbacks};

static_assertions::assert_impl_all!(ThreadedWindow: Send);
static_assertions::assert_not_impl_any!(ThreadedWindow: Sync);
static_assertions::assert_not_impl_any!(Window: Send, Sync);
static_assertions::assert_not_impl_any!(Box<dyn HostCallbacks>: Send, Sync);

#[test]
fn no_host_controller_is_send_but_callback_windows_are_not() {
    fn assert_send<T: Send>() {}
    assert_send::<ThreadedWindow>();
}
