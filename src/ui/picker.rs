//! The system's file dialogs, for the library folder and for saving a multi.
//! On Linux that is the XDG desktop portal (zenity when no portal answers).
//! A dialog blocks whoever opens it, so it runs on a thread of its own; the
//! editor picks the answer up on a later frame. Without a desktop to ask,
//! the inline strips do the job instead.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// What the editor asks for.
pub enum Ask {
    /// A library folder (`single`), or a folder of libraries, from `from`.
    Folder { from: PathBuf, single: bool },
    /// Where to save a multi, starting in `from` with `name` filled in.
    Multi { from: PathBuf, name: String },
    /// A picture to show as the cover of the library in `library`.
    Artwork { library: PathBuf },
    /// A folder of samples to make a library of, in `out`.
    Samples { out: PathBuf },
}

/// What came back.
pub enum Picked {
    Folder(PathBuf, bool),
    Multi(PathBuf),
    Artwork { library: PathBuf, picture: PathBuf },
    /// A library made from samples: where, or why not.
    Created(Result<PathBuf, String>),
}

#[derive(Default)]
pub struct Picker {
    /// A dialog is open: a second one waits for it.
    open: AtomicBool,
    picked: Mutex<Option<Picked>>,
    /// The library folder of each row of the browser's upper pane, as last
    /// drawn: a picture dropped on a row becomes its cover.
    pub rows: Mutex<Vec<Option<PathBuf>>>,
}

impl Picker {
    /// Whether a dialog can be shown here at all.
    pub fn available() -> bool {
        if cfg!(test) {
            return false;
        }
        if !cfg!(target_os = "linux") {
            return true;
        }
        let bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
            || std::env::var_os("XDG_RUNTIME_DIR")
                .is_some_and(|dir| PathBuf::from(dir).join("bus").exists());
        let zenity = std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|dir| dir.join("zenity").is_file())
        });
        bus || zenity
    }

    /// Open the dialog for `ask` on its own thread. False when none can be
    /// shown or one is open already: the caller falls back to its strip.
    pub fn ask(self: &Arc<Self>, ask: Ask) -> bool {
        if !Self::available() {
            return false;
        }
        if self.open.swap(true, Ordering::AcqRel) {
            // One is up already; it answers first.
            return true;
        }
        let picker = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("kontakto-file-dialog".into())
            .spawn(move || {
                let picked = show(ask);
                *super::lock(&picker.picked) = picked;
                picker.open.store(false, Ordering::Release);
            });
        if spawned.is_err() {
            self.open.store(false, Ordering::Release);
        }
        spawned.is_ok()
    }

    /// The answer, once, when the dialog has closed with one.
    pub fn take(&self) -> Option<Picked> {
        super::lock(&self.picked).take()
    }

    /// An answer is waiting: the editor should build a frame to take it.
    pub fn ready(&self) -> bool {
        super::lock(&self.picked).is_some()
    }
}

fn show(ask: Ask) -> Option<Picked> {
    match ask {
        Ask::Folder { from, single } => rfd::FileDialog::new()
            .set_title(if single { "A Kontakt library folder" } else { "A folder of Kontakt libraries" })
            .set_directory(from)
            .pick_folder()
            .map(|path| Picked::Folder(path, single)),
        Ask::Artwork { library } => rfd::FileDialog::new()
            .set_title("A picture for the library's cover")
            .set_directory(&library)
            .add_filter("Pictures", &["png", "jpg", "jpeg"])
            .pick_file()
            .map(|picture| Picked::Artwork { library, picture }),
        Ask::Samples { out } => {
            let source = rfd::FileDialog::new().set_title("A folder of WAV or AIFF samples").pick_folder()?;
            // Made here, on the dialog's thread: it reads every sample, which takes a while.
            let options = crate::creator::Options { source, name: String::new(), vendor: String::new(), out, kontra: true, kontakt: true };
            Some(Picked::Created(match crate::creator::create(&options, &|_| {}) {
                Ok(created) => Ok(created.kontra.unwrap_or_default()),
                Err(e) => Err(format!("{e:#}")),
            }))
        }
        Ask::Multi { from, name } => {
            let _ = std::fs::create_dir_all(&from);
            rfd::FileDialog::new()
                .set_title("Save the rack as a multi")
                .set_directory(from)
                .set_file_name(format!("{name}.{}", crate::import::SAVED_MULTI))
                .add_filter("KONTRA multi", &[crate::import::SAVED_MULTI])
                .save_file()
                .map(Picked::Multi)
        }
    }
}
