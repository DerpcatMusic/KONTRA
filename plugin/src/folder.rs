//! The editor's "Add folder": the system's folder picker, off the editor's
//! thread, so no frame waits on it. [`crate::Tick`] takes what was picked
//! and scans it.
//!
//! On Linux the picker uses the desktop portal through `mui-native-dialog`.
//! Closing the editor cancels its request. The dialog service joins its
//! workers at plugin teardown.
//!
//! On macOS rfd runs the picker on the host's main thread. On Windows the
//! dialog disables its Win32 owner while open. A host that forcibly unloads
//! the plugin during a Windows dialog may leave the worker running.

use std::path::PathBuf;
use std::sync::Mutex;
#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicU32;
use std::sync::atomic::{AtomicIsize, Ordering};

use crate::lock;

/// What a picked folder is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wanted {
    /// One more folder of libraries to scan.
    #[default]
    Libraries,
    /// Kontakt's user snapshot folder ([`kontra_library::Prefs::kontakt_snapshots`]).
    Snapshots,
}

impl Wanted {
    fn title(self) -> &'static str {
        match self {
            Self::Libraries => "A folder of libraries",
            Self::Snapshots => "Kontakt's user snapshot folder (User Content)",
        }
    }
}

#[cfg(target_os = "linux")]
type Job = mui_native_dialog::DialogJob;
#[cfg(not(target_os = "linux"))]
type Job = std::thread::JoinHandle<Option<PathBuf>>;

enum Picker {
    Open(Wanted, Job),
    #[cfg(target_os = "linux")]
    Ready(Wanted, PathBuf),
    Failed(String),
}

/// The "Add folder" picker while it is open.
#[derive(Default)]
pub struct Dialog {
    #[cfg(target_os = "linux")]
    service: mui_native_dialog::DialogService,
    picker: Mutex<Option<Picker>>,
    /// The host's Win32 window the editor opened in; 0 when none.
    owner: AtomicIsize,
    /// The editor's X11 window on Linux; 0 when none or running native Wayland.
    #[cfg(target_os = "linux")]
    x11_parent: AtomicU32,
}

impl Dialog {
    /// The host's Win32 window the editor opened in, or `None` as it closes.
    pub fn set_owner(&self, hwnd: Option<isize>) {
        self.owner.store(hwnd.unwrap_or(0), Ordering::Release);
    }

    /// The editor's X11 window on Linux, or `None` when closed / Wayland.
    #[cfg(target_os = "linux")]
    pub fn set_x11_parent(&self, xid: Option<u32>) {
        self.x11_parent.store(xid.unwrap_or(0), Ordering::Release);
    }

    /// Opens the picker at `from` for a folder `wanted` for, unless one is
    /// open.
    pub fn ask(&self, wanted: Wanted, from: Option<PathBuf>) {
        let mut picker = lock(&self.picker);
        if matches!(*picker, Some(Picker::Open(..))) {
            return;
        }
        *picker = Some(
            match self.open(wanted.title(), from.or_else(dirs::home_dir)) {
                Ok(job) => Picker::Open(wanted, job),
                Err(err) => Picker::Failed(err),
            },
        );
    }

    #[cfg(target_os = "linux")]
    fn open(&self, title: &str, from: Option<PathBuf>) -> Result<Job, String> {
        use mui_native_dialog::{DialogKind, DialogRequest, Parent};
        let xid = self.x11_parent.load(Ordering::Acquire);
        let request = DialogRequest {
            parent: (xid != 0).then_some(Parent::X11(xid)),
            title: title.into(),
            kind: DialogKind::PickFolder { multiple: false },
            directory: from,
            filters: Vec::new(),
        };
        self.service.spawn(request)
    }

    #[cfg(not(target_os = "linux"))]
    fn open(&self, title: &'static str, from: Option<PathBuf>) -> Result<Job, String> {
        let owner = self.owner.load(Ordering::Acquire);
        std::thread::Builder::new()
            .name("kontra-folder-dialog".into())
            .spawn(move || {
                let mut d = rfd::FileDialog::new().set_title(title);
                if let Some(from) = from {
                    d = d.set_directory(from);
                }
                if cfg!(windows) && owner != 0 {
                    d = d.set_parent(&moose::mui::ParentWindow::win32(owner));
                }
                d.pick_folder()
            })
            .map_err(|err| err.to_string())
    }

    /// Whether a picker answer or error is waiting for [`Self::picked`].
    pub fn has_result(&self) -> bool {
        match lock(&self.picker).as_ref() {
            Some(Picker::Open(_, job)) => job.is_finished(),
            #[cfg(target_os = "linux")]
            Some(Picker::Ready(..)) => true,
            Some(Picker::Failed(_)) => true,
            None => false,
        }
    }

    /// The folder picked (or a portal/spawn error) and what for, once the
    /// picker closed. `None` while still open or if the user cancelled.
    pub fn picked(&self) -> Option<Result<(Wanted, PathBuf), String>> {
        let picker = {
            let mut slot = lock(&self.picker);
            if matches!(slot.as_ref(), Some(Picker::Open(_, job)) if !job.is_finished()) {
                return None;
            }
            slot.take()?
        };
        let (wanted, job) = match picker {
            Picker::Open(wanted, job) => (wanted, job),
            #[cfg(target_os = "linux")]
            Picker::Ready(wanted, path) => return Some(Ok((wanted, path))),
            Picker::Failed(err) => return Some(Err(err)),
        };
        #[cfg(target_os = "linux")]
        let result = job
            .try_result()?
            .map(|paths| paths.and_then(|paths| paths.into_iter().next()));
        #[cfg(not(target_os = "linux"))]
        let result = job
            .join()
            .map_err(|_| "Folder picker worker stopped unexpectedly".into());
        result
            .map(|path| path.map(|path| (wanted, path)))
            .transpose()
    }

    /// Cancel an in-flight portal request without waiting on the GUI thread.
    /// Any answer or error already queued by the worker is retained so the
    /// background tick still consumes it even if `is_finished()` has not yet
    /// flipped.
    #[cfg(target_os = "linux")]
    pub fn cancel(&self) {
        let mut slot = lock(&self.picker);
        if !matches!(slot.as_ref(), Some(Picker::Open(..))) {
            return;
        }
        let Some(Picker::Open(wanted, job)) = slot.take() else {
            return;
        };
        match job.try_result() {
            Some(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    *slot = Some(Picker::Ready(wanted, path));
                }
            }
            Some(Err(err)) => {
                *slot = Some(Picker::Failed(err));
            }
            // User cancelled (`Some(Ok(None))`) or still in-flight (`None`):
            // drop `job` to cancel any active portal request without blocking.
            Some(Ok(None)) | None => {}
        }
    }

    #[cfg(test)]
    pub(crate) fn record_error_for_test(&self, err: impl Into<String>) {
        *lock(&self.picker) = Some(Picker::Failed(err.into()));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use mui_native_dialog::{DialogKind, DialogRequest, Parent};
    use std::time::{Duration, Instant};

    #[test]
    fn portal_readiness_preserves_the_result() -> Result<(), String> {
        let dialog = Dialog::default();
        let job = dialog.service.spawn(DialogRequest {
            // Invalid before contacting D-Bus, so this needs no desktop session.
            parent: Some(Parent::X11(0)),
            title: "Pick a folder".into(),
            kind: DialogKind::PickFolder { multiple: false },
            directory: None,
            filters: Vec::new(),
        })?;
        *lock(&dialog.picker) = Some(Picker::Open(Wanted::Libraries, job));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dialog.has_result() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(dialog.has_result());
        assert_eq!(
            dialog.picked(),
            Some(Err("File picker parent has no X11 window".into()))
        );
        assert!(!dialog.has_result());
        Ok(())
    }

    #[test]
    fn cancel_preserves_ready_and_failed_results() {
        let dialog = Dialog::default();
        let ready_path = std::path::PathBuf::from("/tmp/kontra-lib");
        *lock(&dialog.picker) = Some(Picker::Ready(Wanted::Libraries, ready_path.clone()));
        dialog.cancel();
        assert!(dialog.has_result());
        assert_eq!(dialog.picked(), Some(Ok((Wanted::Libraries, ready_path))));

        dialog.record_error_for_test("portal error");
        dialog.cancel();
        assert!(dialog.has_result());
        assert_eq!(dialog.picked(), Some(Err("portal error".into())));
    }
}
