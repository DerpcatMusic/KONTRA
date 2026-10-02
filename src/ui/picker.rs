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
    ScriptFile { part: usize, epoch: u64, slot: usize, control: usize, from: PathBuf, file_type: i32 },
    Snapshot { slot: usize, source: (String, u32, String), from: PathBuf },
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
    ScriptFile { part: usize, epoch: u64, slot: usize, control: usize, result: Result<String, String> },
    Snapshot { slot: usize, source: (String, u32, String), path: PathBuf },
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
        Ask::ScriptFile { part, epoch, slot, control, from, file_type } => {
            let extensions: &[&str] = match file_type {
                0 => &["mid", "midi"],
                1 => &["wav", "aif", "aiff", "ncw"],
                2 => &["nka"],
                _ => return Some(Picked::ScriptFile { part, epoch, slot, control, result: Err("Unsupported file selector type".into()) }),
            };
            let result = match from.canonicalize() {
                Ok(base) if base.is_dir() => {
                    let path = rfd::FileDialog::new().set_title("Select an instrument file")
                        .set_directory(&base).add_filter("Instrument files", extensions).pick_file()?;
                    selected_file(&base, &path, extensions)
                }
                _ => Err("File selector base directory is unavailable".into()),
            };
            Some(Picked::ScriptFile { part, epoch, slot, control, result })
        }
        Ask::Snapshot { slot, source, from } => rfd::FileDialog::new()
            .set_title("Load a snapshot for this instrument")
            .set_directory(from)
            .add_filter("Kontakt snapshot", &["nksn"])
            .pick_file()
            .map(|path| Picked::Snapshot { slot, source, path }),
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

/// Runs only on the picker worker, so symlink and case handling never blocks audio.
fn selected_file(base: &PathBuf, path: &PathBuf, extensions: &[&str]) -> Result<String, String> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    if !path.starts_with(base) || !path.is_file() { return Err("Select a file inside the authored base directory".into()) }
    if !path.extension().is_some_and(|e| extensions.iter().any(|x| e.eq_ignore_ascii_case(x))) {
        return Err("Selected file does not match the authored file type".into());
    }
    let text = path.to_str().ok_or("File path is not valid UTF-8")?.replace('\\', "/");
    if text.len() > 1280 || text.chars().count() > 320 { return Err("File path exceeds KSP's 320-character string limit".into()) }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_file_type_and_base_directory_are_enforced() {
        let dir = std::env::temp_dir().join(format!("kontra-picker-{}",std::process::id()));
        std::fs::create_dir_all(dir.join("base")).unwrap();
        let base = dir.join("base").canonicalize().unwrap();
        let inside = base.join("Valid.NKA");
        std::fs::write(&inside,b"%preset\n42\n").unwrap();
        let outside = dir.join("outside.nka");
        std::fs::write(&outside,b"%preset\n0\n").unwrap();
        assert!(selected_file(&base,&inside,&["nka"]).is_ok());
        assert!(selected_file(&base,&inside,&["mid"]).is_err());
        assert!(selected_file(&base,&outside,&["nka"]).is_err());
        #[cfg(unix)] {
            let linked = base.join("linked.nka");
            std::os::unix::fs::symlink(&outside,&linked).unwrap();
            assert!(selected_file(&base,&linked,&["nka"]).is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
