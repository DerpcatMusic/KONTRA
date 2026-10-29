//! The editor's "Add folder": the system's folder picker (rfd) on a thread
//! of its own, so no frame waits on it. [`crate::Tick`] takes what was
//! picked and scans it.
//!
// ponytail: rfd has no cancel, and on macOS it runs the dialog on the main
// thread, so joining it at teardown can deadlock the host. The thread is
// left to finish on its own instead (it holds no plugin state); a host that
// unloads the plugin's binary while the dialog is open is the ceiling. Use
// a cancellable dialog (v2's mui-native-dialog) if that shows up.

use std::path::PathBuf;
use std::thread::JoinHandle;

use crate::{Shared, lock};

/// The open dialog, answering the folder picked.
pub type Dialog = Option<JoinHandle<Option<PathBuf>>>;

/// Opens the dialog at `from`, unless one is open.
pub fn ask(shared: &Shared, from: Option<PathBuf>) {
    let mut dialog = lock(&shared.folder);
    if dialog.is_some() {
        return;
    }
    let from = from.or_else(dirs::home_dir);
    *dialog = std::thread::Builder::new()
        .name("kontra-folder-dialog".into())
        .spawn(move || {
            let mut d = rfd::FileDialog::new().set_title("A folder of libraries");
            if let Some(from) = from {
                d = d.set_directory(from);
            }
            d.pick_folder()
        })
        .ok();
}

/// The folder picked, once the dialog closed.
pub fn picked(shared: &Shared) -> Option<PathBuf> {
    let mut dialog = lock(&shared.folder);
    if !dialog.as_ref()?.is_finished() {
        return None;
    }
    dialog.take()?.join().ok().flatten()
}
