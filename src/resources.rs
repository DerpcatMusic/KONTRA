//! Shared loose/NKR resource loading for scripts, artwork and impulse responses.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// without case: libraries made on Windows or macOS spell them freely.
fn resource_dirs(dir: &Path, name: &str) -> Vec<PathBuf> {
    let child = |dir: &Path, want: &str| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(want))
            .map(|e| e.path())
            .collect()
    };
    let mut out: Vec<PathBuf> = child(dir, "Resources")
        .iter()
        .flat_map(|r| child(r, name))
        .collect();
    out.extend(child(dir, name));
    out
}

/// The text of a script slot linked to `name` (as Kontakt stores it: a bare
/// file name or any Windows or POSIX path ending in one) from the library's
/// `Resources/scripts`, loose or in its resource container.
pub fn linked_script(instrument: &Path, name: &str) -> Result<Option<Vec<u8>>, String> {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    if file.is_empty() || file == "." || file == ".." {
        return Ok(None);
    }
    Resources::of(instrument, "scripts").read(file)
}

/// The impulse response `load_ir_sample` names: an absolute path, or a file
/// in the library's `Resources/ir_samples`, loose or in its resource
/// container. Names match without case; a name without an extension
/// matches any audio file; files in `ir_samples` subfolders match by name.
pub fn ir_sample(instrument: &Path, name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        return path.is_file().then(|| path.into());
    }
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let names: Vec<String> = if path.extension().is_some() {
        vec![file.into()]
    } else {
        ["wav", "aif", "aiff", "ncw"].iter().map(|e| format!("{file}.{e}")).collect()
    };
    let mut source = Resources::of(instrument, "ir_samples");
    names.iter().find_map(|n| source.path(n))
}

/// Where a preset's resources come from: `Resources` folders near
/// it, else a resource container (`.nkr`) in or one folder below them.
pub(crate) struct Resources {
    /// Loose files by lowercase name, nearest first.
    files: HashMap<String, PathBuf>,
    /// Containers to try in order, opened on first use.
    containers: Vec<PathBuf>,
    failed_archives: Vec<String>,
    open: Vec<(File, ni_file::nkr::Archive, PathBuf)>,
    key: Option<Option<std::sync::Arc<dyn ni_file::nis::LibraryKey>>>,
    instrument: PathBuf,
    /// The `Resources` subfolder: `pictures`, `scripts` or `ir_samples`.
    folder: &'static str,
}

impl Resources {
    pub(crate) fn of(instrument: &Path, folder_name: &'static str) -> Self {
        let mut files = HashMap::new();
        let mut containers = Vec::new();
        let nkrs = |dir: &Path| {
            let mut found: Vec<_> = std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkr")))
                .collect();
            found.sort();
            found
        };
        // Four folders up, and further for an instrument filed deeper than
        // that, as far as the first folder with any resources.
        for (depth, folder) in instrument.ancestors().skip(1).take(8).enumerate() {
            if depth >= 4 && !(files.is_empty() && containers.is_empty()) {
                break;
            }
            for dir in resource_dirs(folder, folder_name) {
                for e in std::fs::read_dir(dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    files.entry(name).or_insert_with(|| e.path());
                }
            }
            containers.extend(nkrs(folder));
            // Some libraries keep their container with the samples.
            let mut subfolders: Vec<_> = std::fs::read_dir(folder)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            subfolders.sort();
            for sub in subfolders {
                containers.extend(nkrs(&sub));
            }
        }
        containers.dedup();
        Self {
            files,
            containers,
            open: Vec::new(),
            failed_archives: Vec::new(),
            key: None,
            instrument: instrument.into(),
            folder: folder_name,
        }
    }

    /// The bytes of `<folder>/<file>`, if the library has it.
    pub(crate) fn read(&mut self, file: &str) -> Result<Option<Vec<u8>>, String> {
        if let Some(path) = self.files.get(&file.to_lowercase()) {
            return read_bounded(path).map(Some);
        }
        let member = format!("Resources/{}/{file}", self.folder);
        for n in 0.. {
            if !self.opened(n) {
                return if self.failed_archives.is_empty() { Ok(None) } else {
                    Err(format!("Resource {file:?} was not found; archives could not be read: {}", self.failed_archives.join("; ")))
                };
            }
            let (f, archive, _) = &mut self.open[n];
            let Some(entry) = archive.find(&member) else {
                continue;
            };
            if entry.size > 32 * 1024 * 1024 {
                return Err(format!("{file} exceeds 32 MiB"));
            }
            let key = match &self.key {
                _ if !entry.encoded || entry.key_index == 0xff => &None,
                Some(key) => key,
                None => self.key.insert(
                    crate::access::library_key(&self.instrument).map_err(|e| e.to_string())?,
                ),
            };
            return archive
                .read_entry_with_key(f, &member, key.as_deref())
                .map(Some)
                .map_err(|e| e.to_string());
        }
        Ok(None)
    }

    /// Whether container `n` is open, opening the next ones as needed.
    fn opened(&mut self, n: usize) -> bool {
        while n == self.open.len() {
            if self.containers.is_empty() {
                return false;
            }
            let path = self.containers.remove(0);
            let mut f = match File::open(&path) {
                Ok(f) => f,
                Err(e) => { self.failed_archives.push(format!("{}: {e}", path.display())); continue; }
            };
            match ni_file::nkr::Archive::read(&mut f) {
                Ok(archive) => self.open.push((f, archive, path)),
                Err(e) => self.failed_archives.push(format!("{}: {e}", path.display())),
            }
        }
        true
    }

    /// The path of `<folder>/<file>`, or of `file` in any folder below it
    /// in a container, for [`crate::audio::decode`].
    fn path(&mut self, file: &str) -> Option<PathBuf> {
        let file = file.to_lowercase();
        if let Some(path) = self.files.get(&file) {
            return Some(path.clone());
        }
        let folder = format!("resources/{}/", self.folder);
        let nested = format!("/{file}");
        let member = |k: &&String| {
            k.strip_prefix(&folder).is_some_and(|rest| rest == file || rest.ends_with(&nested))
        };
        let mut n = 0;
        while self.opened(n) {
            let (_, archive, path) = &self.open[n];
            if let Some(k) = archive.entries.keys().filter(member).min_by_key(|k| k.len()) {
                return Some(path.join(&archive.entries[k].name));
            }
            n += 1;
        }
        None
    }
}

pub(crate) fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Resource exceeds 32 MiB".into());
    }
    Ok(bytes)
}
