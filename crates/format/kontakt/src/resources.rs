//! A library's resource files (`Resources/…`): loose folders, `.nkr` archives
//! and `.nicnt` containers in the library root and the folders just under it
//! (Shreddage keeps its archive in `Samples/`).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ni_file::file_container::NIFileContainer;
use ni_file::nis::LibraryKey;
use ni_file::nkr::Archive;

/// No resource a script loads is larger.
const LIMIT: u64 = 32 << 20;
/// The member table starts at this marker within the first 4 MiB of a `.nicnt`.
const MARKER: &[u8] = b"/\\ NI FC MTD  /\\";
/// The library root is at most this far above an instrument.
pub(crate) const DEPTH: usize = 8;

enum Source {
    Loose(PathBuf),
    Nkr(File, Archive, PathBuf),
    Nicnt(File, NIFileContainer, PathBuf),
}

pub struct Resources {
    sources: Vec<Source>,
    instrument: PathBuf,
    key: Option<Arc<dyn LibraryKey>>,
    /// The key was looked up, found or not: it is looked up once.
    keyed: bool,
}

impl Resources {
    /// Every resource source of the library the instrument at `path` belongs to.
    #[must_use]
    pub fn open(path: &Path) -> Self {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.into());
        let root = root(&path);
        let mut places = vec![root.clone()];
        places.extend(
            entries(&root)
                .into_iter()
                .filter(|p| p.is_dir() && !named(p, "Instruments") && !named(p, "Resources")),
        );
        let mut sources = Vec::new();
        for place in places {
            for p in entries(&place) {
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase);
                if named(&p, "Resources") && p.is_dir() {
                    sources.push(Source::Loose(p));
                } else if ext.as_deref() == Some("nkr") {
                    let archive = File::open(&p).ok().and_then(|mut f| {
                        let index = Archive::read_index(&mut f).ok()?;
                        Some(Source::Nkr(f, index, p.clone()))
                    });
                    sources.extend(archive);
                } else if ext.as_deref() == Some("nicnt") {
                    sources.extend(nicnt_table(&p));
                }
            }
        }
        Self {
            sources,
            instrument: path,
            key: None,
            keyed: false,
        }
    }

    /// `Resources/<name>` (`data/x.nka`), matched without case; `None` if absent.
    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        let want = format!("/{}", name.to_lowercase());
        let matches = |stored: &str| {
            format!("/{}", stored.replace(['|', '\\'], "/").to_lowercase()).ends_with(&want)
        };
        for i in 0..self.sources.len() {
            let found = match self.sources.get_mut(i)? {
                Source::Loose(dir) => {
                    let file = File::open(dir.join(name)).ok();
                    file.and_then(|f| {
                        let mut bytes = Vec::new();
                        f.take(LIMIT).read_to_end(&mut bytes).ok()?;
                        Some(bytes)
                    })
                }
                Source::Nicnt(file, table, _) => table
                    .items
                    .iter()
                    .find(|item| matches(&item.filename))
                    .and_then(|item| table.read_item(&mut *file, item, LIMIT).ok()),
                Source::Nkr(file, archive, _) => {
                    let Some(entry) = archive.members().find(|e| matches(&e.name)).cloned() else {
                        continue;
                    };
                    if entry.size > LIMIT {
                        continue;
                    }
                    if entry.encoded && !self.keyed {
                        self.keyed = true;
                        self.key = crate::access::library_key(&self.instrument).ok();
                    }
                    let found = archive
                        .read_entry_with_key(&mut *file, &entry.name, self.key.as_deref())
                        .ok();
                    // An encrypted archive need not flag its members
                    // (Afflatus, Dolce): a failed read fetches the key once.
                    if found.is_none() && !self.keyed {
                        self.keyed = true;
                        self.key = crate::access::library_key(&self.instrument).ok();
                        archive
                            .read_entry_with_key(&mut *file, &entry.name, self.key.as_deref())
                            .ok()
                    } else {
                        found
                    }
                }
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }
}

impl Resources {
    /// Where `Resources/<name>` sits in a loose folder or a `.nicnt`, for a
    /// reader that opens it itself (a convolution response).
    pub(crate) fn locate(&self, name: &str) -> Option<crate::samples::Location> {
        let want = format!("/{}", name.replace(['|', '\\'], "/").to_lowercase());
        self.sources.iter().find_map(|source| match source {
            Source::Loose(dir) => {
                let file = dir.join(name);
                // Inside the folder, `..` and symlinks resolved.
                let real = |p: &Path| p.canonicalize().ok();
                let inside = real(&file)
                    .zip(real(dir))
                    .is_some_and(|(f, d)| f.starts_with(d));
                (inside && file.is_file()).then_some(crate::samples::Location {
                    file,
                    offset: 0,
                    size: None,
                    keystream: None,
                })
            }
            Source::Nicnt(_, table, path) => {
                let item = table.items.iter().find(|item| {
                    format!(
                        "/{}",
                        item.filename.replace(['|', '\\'], "/").to_lowercase()
                    )
                    .ends_with(&want)
                })?;
                Some(crate::samples::Location {
                    file: path.clone(),
                    offset: table
                        .file_section_offset
                        .checked_add(item.file_start_offset)?,
                    size: Some(item.file_size),
                    keystream: None,
                })
            }
            Source::Nkr(..) => None,
        })
    }

    /// The library's `.nkr` archives.
    pub(crate) fn archives(&self) -> impl Iterator<Item = &Path> {
        self.sources.iter().filter_map(|s| match s {
            Source::Nkr(_, _, path) => Some(path.as_path()),
            _ => None,
        })
    }
}

/// The library folder of the instrument at absolute `path`: the nearest
/// ancestor holding `Samples`, a `.nicnt` or `Resources`; else its own folder.
pub(crate) fn root(path: &Path) -> PathBuf {
    path.ancestors()
        .skip(1)
        .take(DEPTH)
        .find(|dir| {
            entries(dir).iter().any(|p| {
                named(p, "Samples")
                    || named(p, "Resources")
                    || p.extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("nicnt"))
            })
        })
        .or_else(|| path.parent())
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<_> = std::fs::read_dir(dir)
        .map(|read| read.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    found.sort();
    found
}

fn named(path: &Path, name: &str) -> bool {
    path.file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case(name))
}

fn nicnt_table(path: &Path) -> Option<Source> {
    let mut file = File::open(path).ok()?;
    let mut head = Vec::new();
    (&mut file).take(4 << 20).read_to_end(&mut head).ok()?;
    let at = head
        .windows(MARKER.len())
        .enumerate()
        .skip(1)
        .find_map(|(at, bytes)| (bytes == MARKER).then_some(at))?;
    file.seek(SeekFrom::Start(at as u64)).ok()?;
    let (table, _) = NIFileContainer::read_table(&mut file).ok()?;
    Some(Source::Nicnt(file, table, path.to_path_buf()))
}
