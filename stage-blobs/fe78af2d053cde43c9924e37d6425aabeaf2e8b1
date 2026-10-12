//! The system's file dialogs, for the library folder and for saving a multi.
//! Linux polls the desktop portal without blocking the GUI and cancels jobs
//! before their native parent closes. Windows/macOS retain their native rfd
//! worker. Filesystem work stays off the GUI; plugin teardown joins workers.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::Runtime;

/// What the editor asks for.
pub enum Ask {
    /// Open a validated native folder/file path without a blocking UI call.
    Reveal(PathBuf),
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
    DialogError(String),
    Revealed(Result<(), String>),
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
    answer: Arc<Answer>,
    #[cfg(not(target_os = "linux"))]
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    #[cfg(target_os = "linux")]
    linux: linux::State,
    /// The library folder of each row of the browser's upper pane, as last
    /// drawn: a picture dropped on a row becomes its cover.
    pub rows: Mutex<Vec<Option<PathBuf>>>,
}

#[derive(Default)]
struct Answer {
    open: AtomicBool,
    picked: Mutex<Option<Picked>>,
    #[cfg(target_os = "linux")]
    current: Mutex<Option<Arc<linux::Operation>>>,
}

impl Drop for Picker {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        self.close();
        #[cfg(not(target_os = "linux"))]
        if let Some(worker) = super::lock(&self.worker).take() { let _ = worker.join(); }
    }
}

impl Picker {
    #[cfg(target_os = "linux")]
    pub(crate) fn with_runtime(runtime: Arc<Runtime>) -> Self {
        Self { answer: Arc::default(), linux: linux::State::new(runtime), rows: Mutex::default() }
    }

    #[cfg(target_os = "linux")]
    pub fn native_window(&self, parent: Option<u32>) {
        self.linux.parent(parent, &self.answer);
    }

    /// GUI close cancels; final plugin teardown owns worker joining.
    pub fn close(&self) {
        #[cfg(target_os = "linux")]
        self.linux.close(&self.answer);
    }
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
        bus
    }

    /// Run a file operation on its owned worker. Unavailable dialogs return
    /// false for their inline fallback; Reveal returns false when busy.
    pub fn ask(self: &Arc<Self>, ask: Ask) -> bool {
        if !matches!(ask, Ask::Reveal(_)) && !Self::available() {
            return false;
        }
        if self.answer.open.swap(true, Ordering::AcqRel) {
            // Reveal has no inline fallback: do not report a dropped request as started.
            return !matches!(ask, Ask::Reveal(_));
        }
        #[cfg(target_os = "linux")]
        return self.linux.start(ask, Arc::clone(&self.answer));
        #[cfg(not(target_os = "linux"))]
        {
        if let Some(worker) = super::lock(&self.worker).take() { let _ = worker.join(); }
        let answer = self.answer.clone();
        let spawned = std::thread::Builder::new()
            .name("kontakto-file-dialog".into())
            .spawn(move || {
                let picked = show(ask);
                *super::lock(&answer.picked) = picked;
                answer.open.store(false, Ordering::Release);
                super::logs::wake_worker();
            });
        match spawned {
            Ok(worker) => { *super::lock(&self.worker) = Some(worker); true }
            Err(_) => { self.answer.open.store(false, Ordering::Release); false }
        }
        }
    }

    /// The answer, once, when the dialog has closed with one.
    pub fn take(&self) -> Option<Picked> {
        #[cfg(target_os = "linux")]
        self.linux.poll(&self.answer);
        super::lock(&self.answer.picked).take()
    }

    /// An answer is waiting: the editor should build a frame to take it.
    pub fn ready(&self) -> bool {
        #[cfg(target_os = "linux")]
        self.linux.poll(&self.answer);
        super::lock(&self.answer.picked).is_some()
    }
}

#[cfg(not(target_os = "linux"))]
fn show(ask: Ask) -> Option<Picked> {
    match ask {
        Ask::Reveal(path) => Some(Picked::Revealed(super::menu::reveal(&path))),
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
    fn reveal_completion_is_owned_and_a_busy_request_is_not_silently_dropped() {
        let _lease = crate::diagnostics::acquire();
        let picker = Arc::new(Picker::default());
        picker.answer.open.store(true, Ordering::Release);
        assert!(!picker.ask(Ask::Reveal("/not-started".into())));
        picker.answer.open.store(false, Ordering::Release);
        let missing = std::env::temp_dir().join(format!("kontra-reveal-missing-{}", std::process::id()));
        assert!(picker.ask(Ask::Reveal(missing.clone())), "Reveal does not need a file-dialog backend");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !picker.ready() && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(!picker.answer.open.load(Ordering::Acquire));
        let Some(Picked::Revealed(Err(error))) = picker.take() else { panic!("missing-path result was not delivered") };
        assert!(error.contains(&missing.display().to_string()));
        drop(picker);
    }

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
