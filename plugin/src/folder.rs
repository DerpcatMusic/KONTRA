//! The editor's "Add folder": the system's folder picker, off the editor's
//! thread, so no frame waits on it. [`crate::Tick`] takes what was picked
//! and scans it.
//!
//! On Linux the picker is the desktop portal (v2's `mui-native-dialog`):
//! dropping [`Dialog`] at plugin teardown cancels an open picker and joins
//! its worker, so no thread is left running in an unloaded binary.
//!
//! Elsewhere it is rfd's, which has no cancel, so it is modal to the host
//! instead: on macOS rfd runs it on the host's main thread; on Windows it is
//! owned by the host's window the editor opened in, which stays disabled
//! while it is open. Either way the host cannot unload the plugin under it.
//!
// ponytail: a Windows host that unloads the plugin on its own (not from its
// window) while the picker is open is the ceiling: the worker is left to
// finish. Post WM_CLOSE to the picker's thread at teardown if one shows.

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

/// The "Add folder" picker while it is open.
#[derive(Default)]
pub struct Dialog {
    #[cfg(target_os = "linux")]
    service: mui_native_dialog::DialogService,
    #[cfg(target_os = "linux")]
    job: Mutex<Option<mui_native_dialog::DialogJob>>,
    #[cfg(not(target_os = "linux"))]
    job: Mutex<Option<std::thread::JoinHandle<Option<PathBuf>>>>,
    /// An immediate error starting the picker, reported on the next [`crate::Tick`].
    error: Mutex<Option<String>>,
    /// The host's Win32 window the editor opened in; 0 when none.
    owner: AtomicIsize,
    /// The editor's X11 window on Linux; 0 when none or running native Wayland.
    #[cfg(target_os = "linux")]
    x11_parent: AtomicU32,
    /// What the open picker's folder is for.
    wanted: Mutex<Wanted>,
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
        let mut job = lock(&self.job);
        if job.is_some() {
            return;
        }
        *lock(&self.wanted) = wanted;
        let from = from.or_else(dirs::home_dir);
        match self.open(wanted.title(), from) {
            Ok(started) => {
                *lock(&self.error) = None;
                *job = Some(started);
            }
            Err(err) => {
                *lock(&self.error) = Some(err);
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn open(
        &self,
        title: &str,
        from: Option<PathBuf>,
    ) -> Result<mui_native_dialog::DialogJob, String> {
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
    fn open(
        &self,
        title: &'static str,
        from: Option<PathBuf>,
    ) -> Result<std::thread::JoinHandle<Option<PathBuf>>, String> {
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
        if lock(&self.error).is_some() {
            return true;
        }
        let job = lock(&self.job);
        let Some(job) = job.as_ref() else {
            return false;
        };
        #[cfg(target_os = "linux")]
        {
            job.is_finished() || job.try_result().is_some()
        }
        #[cfg(not(target_os = "linux"))]
        {
            job.is_finished()
        }
    }

    /// The folder picked (or a portal/spawn error) and what for, once the
    /// picker closed. `None` while still open or if the user cancelled.
    pub fn picked(&self) -> Option<Result<(Wanted, PathBuf), String>> {
        if let Some(err) = lock(&self.error).take() {
            return Some(Err(err));
        }
        let result = self.closed()?;
        Some(result.map(|path| (*lock(&self.wanted), path)))
    }

    /// The folder picked (or error), once the picker closed.
    #[cfg(target_os = "linux")]
    fn closed(&self) -> Option<Result<PathBuf, String>> {
        let mut job = lock(&self.job);
        // Finished first: the worker sends its answer before it finishes.
        let finished = job.as_ref()?.is_finished();
        let result = job.as_ref()?.try_result();
        if result.is_none() && !finished {
            return None;
        }
        job.take();
        match result? {
            Ok(Some(paths)) => paths.into_iter().next().map(Ok),
            Ok(None) => None,
            Err(err) => Some(Err(err)),
        }
    }

    /// The folder picked, once the picker closed.
    #[cfg(not(target_os = "linux"))]
    fn closed(&self) -> Option<Result<PathBuf, String>> {
        let mut job = lock(&self.job);
        if !job.as_ref()?.is_finished() {
            return None;
        }
        job.take()?.join().ok().flatten().map(Ok)
    }

    #[cfg(test)]
    pub(crate) fn record_error_for_test(&self, err: impl Into<String>) {
        *lock(&self.error) = Some(err.into());
    }
}
