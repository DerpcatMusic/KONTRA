//! The editor's "Add folder": the system's folder picker (rfd) on a thread
//! of its own, so no frame waits on it. [`crate::Tick`] takes what was
//! picked and scans it; a plugin dropped with the dialog open waits for it
//! to close, so the thread never outlives the plugin's code.

use std::path::PathBuf;
use std::thread::JoinHandle;

use crate::{Shared, lock};

/// The open dialog, answering the folder picked.
pub type Dialog = Option<JoinHandle<Option<PathBuf>>>;

/// Opens the dialog, unless one is open.
pub fn ask(shared: &Shared) {
    // Read before the dialog's lock: the tick takes them the other way round.
    let from = (lock(&shared.browser).as_ref())
        .and_then(|b| b.prefs.roots.first().map(|r| r.path.clone()))
        .or_else(dirs::home_dir);
    let mut dialog = lock(&shared.folder);
    if dialog.is_some() {
        return;
    }
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

impl Drop for Shared {
    fn drop(&mut self) {
        let dialog = self.folder.get_mut().map(Option::take);
        if let Ok(Some(dialog)) = dialog {
            let _closed = dialog.join();
        }
    }
}
