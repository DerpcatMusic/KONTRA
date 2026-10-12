//! baseview's window as a wgpu surface. Both speak raw-window-handle 0.6;
//! moose-baseview fills the Win32 HINSTANCE Vulkan needs.

/// # Safety
/// The window must outlive the returned surface.
#[expect(
    unsafe_code,
    reason = "wgpu takes raw native handles only through an unsafe constructor"
)]
pub unsafe fn create(
    instance: &wgpu::Instance,
    window: &(impl raw_window_handle::HasDisplayHandle + raw_window_handle::HasWindowHandle),
) -> Result<wgpu::Surface<'static>, String> {
    // SAFETY: both handles are read from the live `window`, and this
    // function's own contract makes the caller keep that window alive for as
    // long as the returned surface.
    unsafe {
        // KONTAKTO patch: keep the native failure reason in renderer diagnostics.
        let target = wgpu::SurfaceTargetUnsafe::from_display_and_window(window, window)
            .map_err(|e| format!("native window handles: {e}"))?;
        instance.create_surface_unsafe(target).map_err(|e| format!("native GPU surface: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle};

    struct UnavailableWindow;
    impl HasDisplayHandle for UnavailableWindow {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> { Err(HandleError::Unavailable) }
    }
    impl HasWindowHandle for UnavailableWindow {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> { Err(HandleError::Unavailable) }
    }

    #[test]
    #[expect(unsafe_code, reason = "unavailable handles cannot create a surface")]
    fn surface_failure_preserves_the_native_handle_reason_without_a_graphics_driver() {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::empty();
        let instance = wgpu::Instance::new(descriptor);
        // SAFETY: this window has no handles and no surface can escape.
        let error = unsafe { create(&instance, &UnavailableWindow) }.err().unwrap();
        assert_eq!(error, format!("native window handles: {}", HandleError::Unavailable));
    }
}
