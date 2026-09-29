//! The two pieces of `moose_gui::platform` the editor uses, ported so the
//! plugin does not link moose-gui, whose wgpu 29 would sit in the graph next
//! to MUI's wgpu 30.
//!
//! Ported from truce-gui 6.3.0 `src/platform.rs`
//! (<https://github.com/truce-audio/truce>), licensed
//! `LicenseRef-TruceLicense-1.0`.
use std::sync::atomic::{AtomicU64, Ordering};

use moose_core::editor::RawWindowHandle;
use raw_window_handle as rwh;

/// Moose's parent handle as raw-window-handle 0.6: what
/// [`mui_baseview::open`] takes.
pub struct ParentWindow(pub RawWindowHandle);

impl rwh::HasWindowHandle for ParentWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let null = || rwh::HandleError::Unavailable;
        let raw = match self.0 {
            RawWindowHandle::AppKit(ptr) => rwh::RawWindowHandle::AppKit(
                rwh::AppKitWindowHandle::new(std::ptr::NonNull::new(ptr).ok_or_else(null)?),
            ),
            RawWindowHandle::UiKit(ptr) => rwh::RawWindowHandle::UiKit(
                rwh::UiKitWindowHandle::new(std::ptr::NonNull::new(ptr).ok_or_else(null)?),
            ),
            RawWindowHandle::Win32(ptr) => {
                rwh::RawWindowHandle::Win32(rwh::Win32WindowHandle::new(
                    std::num::NonZeroIsize::new(ptr as isize).ok_or_else(null)?,
                ))
            }
            RawWindowHandle::X11(id) => {
                let id = u32::try_from(id)
                    .ok()
                    .and_then(std::num::NonZeroU32::new)
                    .ok_or_else(null)?;
                rwh::RawWindowHandle::Xcb(rwh::XcbWindowHandle::new(id))
            }
        };
        // SAFETY: the host keeps the parent alive while the editor is open;
        // this only re-types its handle.
        #[expect(unsafe_code, reason = "borrow_raw vouches for the host's handle")]
        let handle = unsafe { rwh::WindowHandle::borrow_raw(raw) };
        Ok(handle)
    }
}

/// The last scale any host passed to [`HostScale::set`], f64 bits, 0 = never.
///
/// moose's CLAP wrapper builds a new editor on every `gui.create` and tells
/// it the host scale only when the host calls `gui.set_scale`, which hosts
/// that set it once per instance do not repeat, so a reopened editor came
/// back at 1.0 inside a 1.5x frame. (moose's VST3 wrapper replays it.)
// ponytail: one scale per process; per-instance if a host ever mixes scales.
static HOST_SCALE: AtomicU64 = AtomicU64::new(0);

/// What moose's editor tells it about scale, as the scale an embedded
/// window opens with. Feed it `Editor::set_scale_factor`; it remembers the
/// last host scale across editors.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostScale {
    host: Option<f64>,
}

impl HostScale {
    /// The host's content scale; ignored unless finite and positive.
    pub fn set(&mut self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            self.host = Some(factor);
            HOST_SCALE.store(factor.to_bits(), Ordering::Relaxed);
        }
    }
    /// This editor's host scale, else the last one any editor was given.
    pub fn get(&self) -> Option<f64> {
        self.host.or(match HOST_SCALE.load(Ordering::Relaxed) {
            0 => None,
            bits => Some(f64::from_bits(bits)),
        })
    }
    /// The scale to pin the window to: the host's when it gave one, else
    /// `None` for the OS scale, never the two multiplied. `None` on macOS,
    /// whose `AppKit` coordinates are logical and backing scale is the OS's.
    #[must_use]
    pub fn policy(&self) -> Option<f64> {
        if cfg!(target_os = "macos") {
            None
        } else {
            self.get()
        }
    }
}
