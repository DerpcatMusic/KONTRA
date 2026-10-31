//! Where a page's pictures are read from, and why one could not be: the one
//! place a tree asset path becomes bytes for a face.
//!
//! The library is the one at the part's own file, else the one at the
//! instrument it was read from (a preset may sit outside its library, or the
//! library may have moved). Every failure is handed to the caller's sink with
//! its reason, so a page drawn without pictures can never be silent.

use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;

use super::Library;

/// Why a picture could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    /// The part has no instrument file: one built in memory, or a state that
    /// lost it.
    NoPath,
    /// Nothing at the instrument's library root holds resources: the file is
    /// not inside its library, the library moved, or it is not installed.
    NoLibrary,
    /// The library was found and holds no file at that path.
    NoFile,
}

impl fmt::Display for Missing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoPath => "the part has no instrument path",
            Self::NoLibrary => "no picture library at the instrument's library root",
            Self::NoFile => "the library has no file at that path",
        })
    }
}

/// Whether `library` found anything to read a picture from at all: a folder
/// or container at or under the instrument's library root. An instrument
/// whose file sits outside its library has none, and then every picture is
/// missing.
#[must_use]
pub fn found(library: &Library) -> bool {
    match library {
        Library::Kontakt(resources) => resources.found(),
        Library::Uvi(resources) => resources.found(),
    }
}

/// The library of the first of `paths` that has resources, tried in order: a
/// part's own file, then the instrument it was loaded from.
#[must_use]
pub fn open_any(paths: impl IntoIterator<Item = Option<PathBuf>>) -> Option<Library> {
    paths
        .into_iter()
        .flatten()
        .map(|path| Library::open(&path))
        .find(found)
}

enum Opened {
    Unopened,
    Absent(Missing),
    Ready(Library),
}

/// A face's pictures, read by the worker that asks: the library of the first
/// of its `paths` that has resources, opened once. Every failure goes to
/// `told` with the reason and the asset path. The reader runs off the UI
/// thread; `told` must be safe there.
pub struct Loader {
    paths: Vec<PathBuf>,
    opened: Mutex<Opened>,
    told: Box<dyn Fn(Missing, &str) + Send + Sync>,
}

impl Loader {
    /// A reader of `paths` (a part's file, then the instrument it came from),
    /// telling `told` of every picture it could not read.
    #[must_use]
    pub fn new(
        paths: impl IntoIterator<Item = Option<PathBuf>>,
        told: Box<dyn Fn(Missing, &str) + Send + Sync>,
    ) -> Self {
        Self {
            paths: paths.into_iter().flatten().collect(),
            opened: Mutex::new(Opened::Unopened),
            told,
        }
    }

    /// The bytes of the picture at the tree asset path, or `None` (told to
    /// `told` why).
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let missing = {
            let mut opened = self.opened.lock().unwrap_or_else(|e| e.into_inner());
            if let Opened::Unopened = *opened {
                *opened = match open_any(self.paths.iter().cloned().map(Some)) {
                    Some(library) => Opened::Ready(library),
                    None => Opened::Absent(if self.paths.is_empty() {
                        Missing::NoPath
                    } else {
                        Missing::NoLibrary
                    }),
                };
            }
            match &mut *opened {
                Opened::Ready(library) => match library.read(path) {
                    Some(bytes) => return Some(bytes),
                    None => Missing::NoFile,
                },
                Opened::Absent(missing) => *missing,
                Opened::Unopened => Missing::NoLibrary,
            }
        };
        (self.told)(missing, path);
        None
    }
}
