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

/// Identify an owner before considering containers in its child folders. A
/// sibling NKR is not evidence that an instrument belongs to that library.
fn resource_root(instrument: &Path) -> Option<PathBuf> {
    let mut nearest = None;
    for folder in instrument.ancestors().skip(1).take(8) {
        let mut resources = false;
        let mut registered = false;
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let p = entry.path();
            resources |= (p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("resources")) && p.is_dir())
                || (p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkr")) && p.is_file());
            registered |= p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nicnt")) && p.is_file();
        }
        if registered {
            return Some(folder.into());
        }
        if folder.file_name().is_some_and(|n| n.eq_ignore_ascii_case("instruments")) {
            return folder.parent().map(Path::to_owned);
        }
        if resources && nearest.is_none() { nearest = Some(folder.into()); }
    }
    nearest
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

/// The two native UI source languages use separate resource roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UiDialect {
    LegacyLua,
    KompleteScript,
}

#[derive(Debug)]
pub(crate) struct UiEntry {
    pub(crate) dialect: UiDialect,
    /// Exact relative file under the dialect's resource folder.
    pub(crate) file: String,
    /// Documented feature-set selector; absent is not an inferred version.
    pub(crate) target_version: Option<String>,
    pub(crate) source: Vec<u8>,
}

/// Resolve a caller-observed entry, never infer a selected module from a default
/// filename or feed modern typed `.kscript` modules to the legacy Lua frontend.
/// `max_bytes` is the caller's resource budget, checked before payload allocation.
pub(crate) fn resolve_ui_entry(
    instrument: &Path,
    requested: &str,
    max_bytes: usize,
) -> Result<Option<UiEntry>, String> {
    let normalized = requested.replace('\\', "/");
    let mut name = normalized.as_str();
    let mut root = None;
    let resources_prefix = name
        .get(..10)
        .is_some_and(|p| p.eq_ignore_ascii_case("Resources/"));
    if let Some(rest) = name
        .get(..10)
        .filter(|p| p.eq_ignore_ascii_case("Resources/"))
    {
        name = &name[rest.len()..];
    }
    for (folder, dialect) in [
        ("native_ui/", UiDialect::LegacyLua),
        ("komplete_scripts/", UiDialect::KompleteScript),
    ] {
        if name
            .get(..folder.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(folder))
        {
            name = &name[folder.len()..];
            root = Some(dialect);
            break;
        }
    }
    if resources_prefix && root.is_none() {
        return Err(
            "Native UI entries must be under Resources/native_ui or Resources/komplete_scripts"
                .into(),
        );
    }
    let extension = if name
        .get(name.len().saturating_sub(4)..)
        .is_some_and(|s| s.eq_ignore_ascii_case(".nui"))
    {
        name = &name[..name.len() - 4];
        Some(UiDialect::LegacyLua)
    } else if name
        .get(name.len().saturating_sub(8)..)
        .is_some_and(|s| s.eq_ignore_ascii_case(".kscript"))
    {
        name = &name[..name.len() - 8];
        Some(UiDialect::KompleteScript)
    } else {
        None
    };
    if root.zip(extension).is_some_and(|(a, b)| a != b) {
        return Err("Native UI entry extension contradicts its resource folder".into());
    }
    let selected = extension.or(root);
    if name.is_empty()
        || !name.split(['/', '.']).all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
    {
        return Err(format!("Invalid native UI entry path {requested:?}"));
    }
    let path = name.replace('.', "/");
    // Entry and optional metadata share one ownership decision, including if
    // files change while the source is being read.
    let root = resource_root(instrument);
    let mut found = None;
    for dialect in [UiDialect::LegacyLua, UiDialect::KompleteScript] {
        if selected.is_some_and(|s| s != dialect) {
            continue;
        }
        if dialect == UiDialect::KompleteScript
            && !path.split('/').all(|p| {
                p.as_bytes()[0].is_ascii_lowercase()
                    && p.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            })
        {
            if selected.is_some() {
                return Err(format!("Invalid Komplete UI module name {requested:?}"));
            }
            continue;
        }
        let (folder, ext) = match dialect {
            UiDialect::LegacyLua => ("native_ui", "nui"),
            UiDialect::KompleteScript => ("komplete_scripts", "kscript"),
        };
        let file = format!("{path}.{ext}");
        let mut resources = Resources::within(instrument, folder, root.clone());
        if let Some(source) = resources.read_limited(&file, max_bytes)? {
            if found.is_some() {
                return Err(format!(
                    "Ambiguous native UI entry {requested:?}: specify .nui or .kscript"
                ));
            }
            found = Some(UiEntry {
                dialect,
                file,
                target_version: None,
                source,
            });
        }
    }
    if let Some(entry) = &mut found {
        if let Some(bytes) =
            Resources::within(instrument, "info", root).read_limited("library.json", max_bytes)?
        {
            let metadata: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|e| format!("Resources/info/library.json: {e}"))?;
            if let Some(version) = metadata
                .get("general")
                .and_then(|g| g.get("kontaktTargetVersion"))
            {
                entry.target_version = Some(version.as_str().ok_or("Resources/info/library.json general.kontaktTargetVersion must be a string")?.to_owned());
            }
        }
    }
    Ok(found)
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

/// Loose and archived resources within the instrument's identified library.
pub(crate) struct Resources {
    /// Loose files by lowercase name, nearest first.
    files: HashMap<String, PathBuf>,
    loose_dirs: Vec<PathBuf>,
    /// Containers to try in order, opened on first use.
    containers: Vec<PathBuf>,
    failed_archives: Vec<String>,
    open: Vec<(File, ni_file::nkr::Archive, PathBuf)>,
    key: Option<Option<std::sync::Arc<dyn ni_file::nis::LibraryKey>>>,
    instrument: PathBuf,
    root: Option<PathBuf>,
    /// The `Resources` subfolder: `pictures`, `scripts` or `ir_samples`.
    folder: &'static str,
}

impl Resources {
    pub(crate) fn of(instrument: &Path, folder_name: &'static str) -> Self {
        Self::within(instrument, folder_name, resource_root(instrument))
    }

    fn within(instrument: &Path, folder_name: &'static str, root: Option<PathBuf>) -> Self {
        let mut files = HashMap::new();
        let mut loose_dirs = Vec::new();
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
        // Preserve nearby overrides and Samples/*.nkr discovery, but never
        // ascend above the owner into adjacent libraries or shared loose files.
        for folder in instrument.ancestors().skip(1).take_while(|folder|
            root.as_deref().is_some_and(|root| folder.starts_with(root))) {
            for dir in resource_dirs(folder, folder_name) {
                loose_dirs.push(dir.clone());
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
            loose_dirs,
            containers,
            open: Vec::new(),
            failed_archives: Vec::new(),
            key: None,
            instrument: instrument.into(),
            root,
            folder: folder_name,
        }
    }

    /// The bytes of `<folder>/<file>`, if the library has it.
    pub(crate) fn read(&mut self, file: &str) -> Result<Option<Vec<u8>>, String> {
        self.read_limited(file, usize::MAX)
    }

    /// Bound payload allocation before opening Lua modules or other untrusted resources.
    /// Requested nested loose paths are resolved component by component, without scanning
    /// the whole resource tree or accepting parent/absolute paths.
    pub(crate) fn read_limited(&mut self, file: &str, limit: usize) -> Result<Option<Vec<u8>>, String> {
        if self.root.is_none() {
            return Err(format!("Library resource ownership could not be determined for {}", self.instrument.display()));
        }
        let bounded = |path: &Path| -> Result<Option<Vec<u8>>, String> {
            let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            if file.metadata().map_err(|e| e.to_string())?.len() > limit as u64 {
                return Err(format!("Resource {} exceeds {limit} bytes", path.display()));
            }
            let mut bytes = Vec::new();
            file.take((limit as u64).saturating_add(1)).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            if bytes.len() > limit { return Err(format!("Resource {} exceeds {limit} bytes", path.display())); }
            Ok(Some(bytes))
        };
        if let Some(path) = self.files.get(&file.to_lowercase()).filter(|p| p.is_file()) {
            return bounded(path);
        }
        let components: Vec<_> = Path::new(file).components().collect();
        if components.len() <= 64 && components.iter().all(|p| matches!(p, std::path::Component::Normal(_))) {
            for root in &self.loose_dirs {
                let mut path = root.clone();
                let found = components.iter().all(|part| {
                    let std::path::Component::Normal(part) = part else { return false };
                    let next = std::fs::read_dir(&path).into_iter().flatten().flatten().find(|entry|
                        entry.file_name().to_string_lossy().eq_ignore_ascii_case(&part.to_string_lossy()));
                    if let Some(next) = next { path = next.path(); true } else { false }
                });
                if found && path.is_file() { return bounded(&path); }
            }
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
            if entry.size > limit as u64 {
                return Err(format!("Resource {file:?} exceeds {limit} bytes"));
            }
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
        if self.root.is_none() {
            crate::diagnostics::resource(&self.instrument, file, "Library resource ownership could not be determined");
            return None;
        }
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

    #[test]
    fn resources_keep_library_ownership_for_archives_loose_files_and_ui_metadata() {
        fn archive(path: &Path, folder: &str, name: &str, payload: &[u8]) {
            let mut bytes = Vec::new();
            directory(&mut bytes, 0, &[("Resources", 128, 1)]);
            directory(&mut bytes, 128, &[(folder, 256, 1)]);
            directory(&mut bytes, 256, &[(name, 512, 4)]);
            member(&mut bytes, 512, 0xff, payload);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let owner = tempfile::tempdir().unwrap();
        let root = owner.path();
        let a = root.join("LibraryA");
        let b = root.join("LibraryB");
        let instrument = a.join("iNsTrUmEnTs/Deep/instrument.nki");
        std::fs::create_dir_all(instrument.parent().unwrap()).unwrap();
        std::fs::create_dir_all(a.join("rEsOuRcEs/native_ui/components")).unwrap();
        std::fs::create_dir_all(root.join("Resources/native_ui")).unwrap();
        std::fs::create_dir_all(b.join("Instruments")).unwrap();
        std::fs::write(b.join("Library.NICNT"), []).unwrap();
        std::fs::write(a.join("rEsOuRcEs/native_ui/main.nui"), b"return {owner='A'}").unwrap();
        std::fs::write(a.join("rEsOuRcEs/native_ui/components/card.nui"), b"return {card='A'}").unwrap();
        std::fs::write(root.join("Resources/native_ui/common.nui"), b"foreign loose override").unwrap();
        let own_nkr = a.join("Samples/owned.NKR");
        archive(&own_nkr, "native_ui", "common.nui", b"A archive");
        let root_nkr = a.join("library.nkr");
        archive(&root_nkr, "native_ui", "root.nui", b"A root archive");
        let foreign_nkr = b.join("foreign.nkr");
        archive(&foreign_nkr, "native_ui", "common.nui", b"B archive");
        assert_eq!(resource_root(&instrument), Some(a.clone()));
        let mut source = Resources::of(&instrument, "native_ui");
        assert_eq!(source.containers, [root_nkr.clone(), own_nkr.clone()]);
        assert_eq!(source.read("COMMON.NUI").unwrap().unwrap(), b"A archive");
        assert_eq!(Resources::of(&b.join("Instruments/other.nki"), "native_ui").read("common.nui").unwrap().unwrap(), b"B archive");
        drop(source);

        // Optional metadata stays with A even when B has the same member,
        // then an unreadable archive, then disappears after discovery.
        archive(&foreign_nkr, "info", "library.json", br#"{"general":{"kontaktTargetVersion":"foreign"}}"#);
        let entry = resolve_ui_entry(&instrument, "Resources/native_ui/main.NUI", 1024).unwrap().unwrap();
        assert_eq!(entry.source, b"return {owner='A'}");
        assert_eq!(entry.target_version, None);
        assert_eq!(resolve_ui_entry(&instrument, "components.card.nui", 1024).unwrap().unwrap().source, b"return {card='A'}");
        std::fs::write(&foreign_nkr, b"bad foreign index").unwrap();
        assert!(resolve_ui_entry(&instrument, "main.nui", 1024).unwrap().is_some());
        let mut metadata = Resources::of(&instrument, "info");
        std::fs::remove_file(&foreign_nkr).unwrap();
        assert!(metadata.read("library.json").unwrap().is_none());
        drop(metadata);
        assert_eq!(resource_root(&b.join("Patches/other.nki")), Some(b.clone()), "NICNT anchors a library without an Instruments path");
        assert_eq!(resource_root(&a.join("standalone.nki")), Some(a.clone()), "the nearest own Resources folder also anchors loose libraries");

        // A closer archive overrides a library archive without hiding other
        // resources within the explicit Instruments library boundary.
        let near_nkr = instrument.parent().unwrap().join("near.nkr");
        archive(&near_nkr, "native_ui", "common.nui", b"nearest archive");
        assert_eq!(Resources::of(&instrument, "native_ui").read("common.nui").unwrap().unwrap(), b"nearest archive");
        std::fs::create_dir_all(a.join("rEsOuRcEs/info")).unwrap();
        std::fs::write(a.join("rEsOuRcEs/info/library.json"), br#"{"general":{"kontaktTargetVersion":"owned"}}"#).unwrap();
        assert_eq!(resolve_ui_entry(&instrument, "common.nui", 1024).unwrap().unwrap().target_version.as_deref(), Some("owned"));
        let local_resources = instrument.parent().unwrap().join("Resources");
        std::fs::create_dir_all(local_resources.join("native_ui")).unwrap();
        std::fs::write(local_resources.join("native_ui/main.nui"), b"return {local=true}").unwrap();
        let local_entry = resolve_ui_entry(&instrument, "main.nui", 1024).unwrap().unwrap();
        assert_eq!(local_entry.source, b"return {local=true}");
        assert_eq!(local_entry.target_version.as_deref(), Some("owned"));
        std::fs::remove_file(&near_nkr).unwrap();
        let mut inherited = Resources::of(&instrument, "native_ui");
        assert_eq!(inherited.read("common.nui").unwrap().unwrap(), b"A archive");
        assert_eq!(inherited.read("root.nui").unwrap().unwrap(), b"A root archive");
        drop(inherited);
        std::fs::remove_dir_all(local_resources).unwrap();
        std::fs::remove_dir_all(a.join("rEsOuRcEs/info")).unwrap();
        // Owned invalid/missing archives and present invalid metadata retain
        // their errors; ownership never turns these into optional absence.
        std::fs::write(&own_nkr, b"bad owned index").unwrap();
        assert!(Resources::of(&instrument, "info").read("library.json").unwrap_err().contains("owned.NKR"));
        archive(&own_nkr, "info", "library.json", b"not JSON");
        assert!(resolve_ui_entry(&instrument, "main.nui", 1024).unwrap_err().contains("library.json"));
        let mut missing = Resources::of(&instrument, "info");
        std::fs::remove_file(&own_nkr).unwrap();
        assert!(missing.read("library.json").unwrap_err().contains("owned.NKR"));
        drop(missing);

        let unknown = tempfile::tempdir().unwrap();
        let deep = unknown.path().join("a/b/c/d/e/f/g/h");
        std::fs::create_dir_all(&deep).unwrap();
        let mut unknown_source = Resources::of(&deep.join("instrument.nki"), "info");
        assert!(unknown_source.read("library.json").unwrap_err().contains("ownership"));
    }

    #[test]
    fn native_ui_entries_keep_dialects_versions_and_resource_bounds_distinct() {
        let dir = std::env::temp_dir().join(format!("kontra-ui-entry-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Resources/native_ui/components")).unwrap();
        std::fs::create_dir_all(dir.join("Resources/komplete_scripts")).unwrap();
        std::fs::create_dir_all(dir.join("Resources/info")).unwrap();
        let instrument = dir.join("authored.nki");
        let legacy = b"return {text='authored Lua'}";
        let modern = b"import { Text } from ui\nexport var main = Text(\"authored\")";
        std::fs::write(dir.join("Resources/native_ui/main.nui"), legacy).unwrap();
        std::fs::write(dir.join("Resources/native_ui/components/card.nui"), legacy).unwrap();
        let first = resolve_ui_entry(&instrument, "main", 1024)
            .unwrap()
            .unwrap();
        assert_eq!(first.dialect, UiDialect::LegacyLua);
        assert_eq!(first.source, legacy);
        assert_eq!(first.target_version, None);
        assert_eq!(
            resolve_ui_entry(&instrument, "components.card.NUI", 1024)
                .unwrap()
                .unwrap()
                .file,
            "components/card.nui"
        );
        std::fs::write(dir.join("Resources/komplete_scripts/main.kscript"), modern).unwrap();
        std::fs::write(
            dir.join("Resources/info/library.json"),
            br#"{"general":{"kontaktTargetVersion":"8.4.0","other":"retained externally"}}"#,
        )
        .unwrap();
        assert!(
            resolve_ui_entry(&instrument, "main", 1024)
                .unwrap_err()
                .contains("Ambiguous")
        );
        let entry = resolve_ui_entry(
            &instrument,
            "Resources\\komplete_scripts\\main.KSCRIPT",
            1024,
        )
        .unwrap()
        .unwrap();
        assert_eq!(entry.dialect, UiDialect::KompleteScript);
        assert_eq!(entry.file, "main.kscript");
        assert_eq!(entry.source, modern);
        assert_eq!(entry.target_version.as_deref(), Some("8.4.0"));
        assert_eq!(
            resolve_ui_entry(&instrument, "main.nui", 1024)
                .unwrap()
                .unwrap()
                .dialect,
            UiDialect::LegacyLua
        );
        for invalid in [
            "../main",
            "/main.nui",
            "C:\\main.nui",
            "Resources/pictures/main.nui",
            "native_ui/main.kscript",
            "Main.kscript",
            "controls..main",
            "main\0.nui",
        ] {
            assert!(
                resolve_ui_entry(&instrument, invalid, 1024).is_err(),
                "{invalid:?}"
            );
        }
        assert!(resolve_ui_entry(&instrument, "main.kscript", modern.len() - 1).is_err());
        assert!(
            resolve_ui_entry(&instrument, "absent.kscript", 1024)
                .unwrap()
                .is_none()
        );
        std::fs::write(
            dir.join("Resources/info/library.json"),
            br#"{"general":{"kontaktTargetVersion":8}}"#,
        )
        .unwrap();
        assert!(resolve_ui_entry(&instrument, "main.kscript", 1024).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lazy_resources_validate_only_requested_headers_and_keep_keyed_reads() {
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
        // Ancestor resource discovery scans one child level. Keep this archive
        // below its own temporary library root, outside other tests' sibling scans.
        let owner = tempfile::tempdir().unwrap();
        let dir = owner.path().join("library");
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
