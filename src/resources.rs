//! Shared loose/NKR resource loading for scripts, artwork and impulse responses.

use std::collections::HashMap;
use std::fs::File;
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

/// Array data from the library's loose or archived Resources/data folder.
pub fn data_file(instrument: &Path, name: &str) -> Result<Option<Vec<u8>>, String> {
    Resources::of(instrument, "data").read(name)
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
    roots: Vec<PathBuf>,
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
        let mut roots = Vec::new();
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
                roots.push(dir.clone());
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
            roots,
            open: Vec::new(),
            failed_archives: Vec::new(),
            key: None,
            instrument: instrument.into(),
            folder: folder_name,
        }
    }

    /// Relative package members, preserving namespaces and archive spelling.
    pub(crate) fn names(&mut self) -> Vec<String> {
        let mut names = std::collections::BTreeMap::new();
        for root in &self.roots {
            for e in walkdir::WalkDir::new(root)
                .into_iter()
                .flatten()
                .filter(|e| e.file_type().is_file())
            {
                if let Ok(relative) = e.path().strip_prefix(root) {
                    let name = relative.to_string_lossy().replace('\\', "/");
                    names.entry(name.to_lowercase()).or_insert(name);
                }
            }
        }
        let prefix = format!("Resources/{}/", self.folder).to_lowercase();
        for n in 0.. {
            if !self.opened(n) {
                break;
            }
        }
        for (_, archive, _) in &self.open {
            for e in archive.entries.values() {
                if e.name.to_lowercase().starts_with(&prefix) {
                    let name = &e.name[prefix.len()..];
                    names
                        .entry(name.to_lowercase())
                        .or_insert_with(|| name.to_owned());
                }
            }
        }
        names.into_values().collect()
    }

    /// The bytes of `<folder>/<file>`, if the library has it.
    pub(crate) fn read(&mut self, file: &str) -> Result<Option<Vec<u8>>, String> {
        let normalized = file.replace('\\', "/");
        let path = Path::new(&normalized);
        if path.is_absolute()
            || path
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!("Invalid resource path {file:?}"));
        }
        let file = normalized.as_str();
        // Loose resources override archive members, including nested module assets.
        for root in &self.roots {
            let mut candidate = root.clone();
            for component in path.components() {
                let wanted = component.as_os_str().to_string_lossy();
                let exact = candidate.join(component);
                candidate = if exact.exists() {
                    exact
                } else {
                    let Some(found) = std::fs::read_dir(&candidate).ok().and_then(|entries| {
                        entries.flatten().find(|e| {
                            e.file_name()
                                .to_string_lossy()
                                .eq_ignore_ascii_case(&wanted)
                        })
                    }) else {
                        candidate.clear();
                        break;
                    };
                    found.path()
                };
            }
            if candidate.is_file() {
                return read_file(&candidate).map(Some);
            }
        }
        if let Some(path) = self.files.get(&file.to_lowercase()) {
            return read_file(path).map(Some);
        }
        let member = format!("Resources/{}/{file}", self.folder);
        for n in 0.. {
            if !self.opened(n) {
                return if self.failed_archives.is_empty() { Ok(None) } else {
                    Err(format!("Resource {file:?} was not found; archives could not be read: {}", self.failed_archives.join("; ")))
                };
            }
            let (f, archive, _) = &mut self.open[n];
            let Some(entry) = archive.member(&mut *f, &member).map_err(|e| e.to_string())? else {
                continue;
            };
            let needs_key = entry.encoded && entry.key_index != 0xff;
            archive.entries.insert(entry.name.to_lowercase(), entry);
            let key = match &self.key {
                _ if !needs_key => &None,
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
            match ni_file::nkr::Archive::read_index(&mut f) {
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

pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_resources_validate_only_requested_headers_and_keep_keyed_reads() {
        fn directory(out: &mut Vec<u8>, offset: usize, entries: &[(&str, u32, u16)]) {
            out.resize(offset, 0);
            out.extend(0x5e70ac54u32.to_le_bytes());
            out.extend(0x111u16.to_le_bytes());
            out.extend([0; 8]);
            out.extend((entries.len() as u32).to_le_bytes());
            out.extend([0; 4]);
            for &(name, reference, kind) in entries {
                let name: Vec<_> = name.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
                out.extend(((name.len() + 8) as u16).to_le_bytes());
                out.extend(reference.to_le_bytes());
                out.extend(kind.to_le_bytes());
                out.extend(name);
            }
        }
        fn member(out: &mut Vec<u8>, offset: usize, key: u32, payload: &[u8]) {
            out.resize(offset, 0);
            out.extend(0x2ae905fau32.to_le_bytes());
            out.extend(0x111u16.to_le_bytes());
            out.extend([0; 4]);
            out.extend(key.to_le_bytes());
            out.extend((payload.len() as u32).to_le_bytes());
            out.extend([0; 4]);
            out.extend(payload);
        }
        struct Key;
        impl ni_file::nis::LibraryKey for Key {
            fn apply_at(&self, _: u64, bytes: &mut [u8]) {
                for byte in bytes { *byte ^= 7; }
            }
        }
        let mut bytes = Vec::new();
        directory(&mut bytes, 0, &[("Resources", 128, 1)]);
        directory(&mut bytes, 128, &[("pictures", 256, 1)]);
        directory(&mut bytes, 256, &[("clear.bin", 512, 4), ("keyed.bin", 768, 4), ("broken.bin", 1024, 4)]);
        member(&mut bytes, 512, 0xff, b"clear");
        member(&mut bytes, 768, 0x100, &b"secret".iter().map(|b| b ^ 7).collect::<Vec<_>>());
        bytes.resize(1046, 0);
        let dir = std::env::temp_dir().join(format!("kontra-lazy-resources-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("resources.nkr"), bytes).unwrap();
        let mut resources = Resources::of(&dir.join("instrument.nki"), "pictures");
        assert!(resources.opened(0));
        assert!(resources.open[0].1.entries.values().all(|e| !e.checked));
        assert_eq!(resources.read("CLEAR.BIN").unwrap().unwrap(), b"clear");
        assert!(resources.key.is_none());
        assert_eq!(resources.open[0].1.entries.values().filter(|e| e.checked).count(), 1);
        resources.key = Some(Some(std::sync::Arc::new(Key)));
        assert_eq!(resources.read("keyed.bin").unwrap().unwrap(), b"secret");
        assert_eq!(resources.read("KEYED.BIN").unwrap().unwrap(), b"secret");
        assert_eq!(resources.open[0].1.entries.values().filter(|e| e.checked).count(), 2);
        assert!(!resources.open[0].1.find("Resources/pictures/broken.bin").unwrap().checked);
        assert!(resources.read("broken.bin").is_err());
        drop(resources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
