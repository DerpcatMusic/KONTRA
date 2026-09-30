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
    window: &baseview::WindowContext,
) -> Option<wgpu::Surface<'static>> {
    // SAFETY: both handles are read from the live `window`, and this
    // function's own contract makes the caller keep that window alive for as
    // long as the returned surface.
    unsafe {
        let target = wgpu::SurfaceTargetUnsafe::from_display_and_window(window, window).ok()?;
        instance.create_surface_unsafe(target).ok()
    }
}
