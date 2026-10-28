//! Where a preset's sample names live. Lookups
//! never leave the library root: a sample outside it does not resolve.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use kontra_ir as ir;
use ni_file::nkr::Archive;

/// A sample's bytes: a whole file, or a member of a monolith or archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Location {
    pub file: PathBuf,
    pub offset: u64,
    /// `None`: to the end of `file`.
    pub size: Option<u64>,
    pub keystream: Option<ir::Keystream>,
}

pub(crate) struct Resolver {
    root: PathBuf,
    parent: PathBuf,
    /// Lower-case member path to its bytes, when the instrument is a monolith.
    monolith: HashMap<String, Location>,
    /// Each archive opened once, with its member index.
    archives: HashMap<PathBuf, Option<(File, Archive)>>,
    /// Lower-case basename to files under `root`, built on the first miss.
    loose: Option<HashMap<String, Vec<PathBuf>>>,
    /// The library's keystream, read on the first encrypted member.
    key: Option<std::sync::Arc<[u8]>>,
}

/// `path` with `.` and `..` resolved by name, without touching the disk.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

impl Resolver {
    pub(crate) fn new(instrument: &Path) -> Result<Self, String> {
        let parent = instrument.parent().unwrap_or(Path::new(".")).to_path_buf();
        // As far up as resources.rs looks: an instrument outside any library
        // must not make the home folder the root.
        let root = instrument
            .ancestors()
            .take(8)
            .find(|p| p.join("Samples").is_dir())
            .unwrap_or(&parent)
            .to_path_buf();
        let mut monolith = HashMap::new();
        let mut file = File::open(instrument).map_err(|e| e.to_string())?;
        let mut magic = [0; 16];
        if file.read_exact(&mut magic).is_ok() && &magic == b"/\\ NI FC MTD  /\\" {
            file.rewind().map_err(|e| e.to_string())?;
            let container = ni_file::file_container::NIFileContainer::read(&mut file)
                .map_err(|e| e.to_string())?;
            for item in container.items {
                let location = Location {
                    file: instrument.to_path_buf(),
                    offset: container.file_section_offset + item.file_start_offset,
                    size: Some(item.file_size),
                    keystream: None,
                };
                monolith.insert(
                    item.filename.replace(['|', '\\'], "/").to_lowercase(),
                    location,
                );
            }
        }
        Ok(Self {
            root,
            parent,
            monolith,
            archives: HashMap::new(),
            loose: None,
            key: None,
        })
    }

    /// The bytes of the sample the preset names `name`, if the library has it.
    pub(crate) fn resolve(&mut self, name: &str) -> Result<Option<Location>, String> {
        let name = name.replace(['\\', '|'], "/");
        if let Some(found) = self.member(&name)? {
            return Ok(Some(found));
        }
        // Relative to the instrument folder, then each folder above it in the library.
        let bases: Vec<PathBuf> = self
            .parent
            .ancestors()
            .take_while(|a| a.starts_with(&self.root))
            .map(Path::to_path_buf)
            .collect();
        for base in &bases {
            // `..` is normal in Kontakt's tables, but never past the root.
            let candidate = lexical(&base.join(&name));
            if !candidate.starts_with(&self.root) {
                continue;
            }
            if let Some((archive, member)) = archive_member(&candidate) {
                if let Some(found) = self.archive_entry(&archive, &member)? {
                    return Ok(Some(found));
                }
            } else if candidate.is_file() {
                return Ok(Some(whole(candidate)));
            }
        }
        // A moved archive: find the archive by name, keep the member exact.
        if let Some((prefix, member)) = archive_reference(&name)
            && let Some(archive) = self.by_name(prefix)?
        {
            return self.archive_entry(&archive, member);
        }
        match self.by_name(&name)? {
            Some(file) => Ok(Some(whole(file))),
            None => Ok(None),
        }
    }

    /// A monolith member: the exact path, else a unique match of its basename.
    fn member(&self, name: &str) -> Result<Option<Location>, String> {
        let name = name.to_lowercase();
        if let Some(found) = self.monolith.get(&name) {
            return Ok(Some(found.clone()));
        }
        let base = name.rsplit('/').next().unwrap_or(&name);
        let suffix = format!("/{base}");
        let mut hits = self
            .monolith
            .iter()
            .filter(|(path, _)| *path == base || path.ends_with(&suffix));
        match (hits.next(), hits.next()) {
            (Some((_, found)), None) => Ok(Some(found.clone())),
            (Some(_), Some(_)) => Err(format!("several monolith members are named {base}")),
            _ => Ok(None),
        }
    }

    fn archive_entry(&mut self, archive: &Path, member: &str) -> Result<Option<Location>, String> {
        let index = match self.archives.get(archive) {
            Some(index) => index,
            None => {
                let index = File::open(archive).ok().and_then(|f| {
                    let index = Archive::read_index(&f).ok()?;
                    Some((f, index))
                });
                self.archives.entry(archive.to_path_buf()).or_insert(index)
            }
        };
        let Some((file, index)) = index else {
            return Ok(None);
        };
        let Some(entry) = index.member(file, member).map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        if !entry.valid {
            return Err(format!("damaged archive member {member}"));
        }
        let keystream = match (entry.encoded, entry.key_index) {
            (false, _) | (true, 0xff) => None,
            (true, 0x100) => {
                let stream = match &self.key {
                    Some(key) => key.clone(),
                    None => self.key.insert(crate::access::keystream(archive)?).clone(),
                };
                Some(ir::Keystream {
                    stream,
                    origin: entry.offset,
                })
            }
            (true, other) => return Err(format!("archive cipher {other:#x}")),
        };
        Ok(Some(Location {
            file: archive.to_path_buf(),
            offset: entry.offset,
            size: Some(entry.size),
            keystream,
        }))
    }

    /// A file under the root with this name: the only one, else the one whose
    /// folders match the most trailing components of `name`.
    fn by_name(&mut self, name: &str) -> Result<Option<PathBuf>, String> {
        let base = name.rsplit('/').next().unwrap_or(name).to_lowercase();
        let root = &self.root;
        let loose = self.loose.get_or_insert_with(|| {
            let mut index = HashMap::<String, Vec<PathBuf>>::new();
            walk(root, &mut |file| {
                let key = file.file_name().map(|n| n.to_string_lossy().to_lowercase());
                index.entry(key.unwrap_or_default()).or_default().push(file);
            });
            index
        });
        let Some(matches) = loose.get(&base) else {
            return Ok(None);
        };
        if let [only] = matches.as_slice() {
            return Ok(Some(only.clone()));
        }
        let parts: Vec<_> = name
            .split('/')
            .filter(|s| !s.is_empty() && *s != "..")
            .collect();
        for n in (2..=parts.len()).rev() {
            let suffix = parts
                .get(parts.len() - n..)
                .unwrap_or_default()
                .join("/")
                .to_lowercase();
            let mut found = matches
                .iter()
                .filter(|p| p.to_string_lossy().to_lowercase().ends_with(&suffix));
            if let (Some(only), None) = (found.next(), found.next()) {
                return Ok(Some(only.clone()));
            }
        }
        Err(format!("{} files are named {base}", matches.len()))
    }
}

fn whole(file: PathBuf) -> Location {
    Location {
        file,
        offset: 0,
        size: None,
        keystream: None,
    }
}

/// `…/x.nkx/member` split into the archive and its member, when x.nkx is a file.
fn archive_member(path: &Path) -> Option<(PathBuf, String)> {
    path.ancestors().skip(1).find_map(|parent| {
        let is_archive = parent
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nkx") || e.eq_ignore_ascii_case("nkr"));
        (is_archive && parent.is_file()).then(|| {
            let member = path
                .strip_prefix(parent)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            Some((parent.to_path_buf(), member))
        })?
    })
}

fn archive_reference(name: &str) -> Option<(&str, &str)> {
    name.match_indices('/').find_map(|(at, _)| {
        let (prefix, rest) = (name.get(..at)?, name.get(at + 1..)?);
        Path::new(prefix)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nkx") || e.eq_ignore_ascii_case("nkr"))
            .then_some((prefix, rest))
    })
}

fn walk(dir: &Path, found: &mut impl FnMut(PathBuf)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(&entry.path(), found),
            Ok(kind) if kind.is_file() => found(entry.path()),
            _ => {}
        }
    }
}
