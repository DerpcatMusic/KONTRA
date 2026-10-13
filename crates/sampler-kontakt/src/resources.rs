//! Bounded library-relative resources shared by KSP pictures, fonts and NativeUI.
//! Loose files override NKR members, which override NICNT members. Never search siblings.
use crate::{LoadError, ResourceContainer};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};
const LIMIT: u64 = 32 << 20;

pub struct Resources {
    files: BTreeMap<String, Vec<PathBuf>>,
    containers: Vec<PathBuf>,
    open: Vec<ResourceContainer>,
    failures: Vec<String>,
    indexed: bool,
    root: PathBuf,
}
fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    files
}
fn loose_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![(root.to_path_buf(), 0)];
    let mut files = Vec::new();
    let mut count = 0;
    while let Some((dir, depth)) = pending.pop() {
        for path in entries(&dir) {
            count += 1;
            if count > 65536 {
                return files;
            }
            let Ok(meta) = path.symlink_metadata() else {
                continue;
            };
            if meta.is_file() {
                files.push(path);
            } else if meta.is_dir() && depth < 32 {
                pending.push((path, depth + 1));
            }
        }
    }
    files
}
fn named(p: &Path, name: &str) -> bool {
    p.file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
}
fn normalize(name: &str) -> Option<String> {
    let name = name.replace(['\\', '|'], "/");
    if name.starts_with('/')
        || name.contains(':')
        || name
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return None;
    }
    Some(name.to_lowercase())
}
impl Resources {
    /// Resource ownership of one preset program. A multi may contain a factory
    /// MIDI tool and a library instrument whose NativeUI entries are both
    /// named `main`; the saved BProgram/F0 reference disambiguates them.
    pub fn for_program(instrument: &Path, program: u32) -> Result<Self, LoadError> {
        use ni_file::kontakt::objects::{Bank, FileNameListPreK51, Program};
        // Synthetic UI fixtures have only a loose Resources tree.
        if !instrument.is_file() {
            return Ok(Self::of(instrument));
        }
        let chunks = crate::read_chunks(instrument)?;
        let decode = |what, error| LoadError::decode(instrument, what, error);
        let selected = if let Some(chunk) = chunks.find_first(3) {
            let bank = Bank::try_from(chunk).map_err(|e| decode("resource bank", e))?;
            let mut slots: Vec<_> = bank
                .slot_list()
                .map_err(|e| decode("resource slots", e))?
                .slots
                .into_iter()
                .collect();
            slots.sort_by_key(|(slot, _)| *slot);
            let mut programs = Vec::new();
            for (_, slot) in slots {
                programs.extend(
                    slot.program_list()
                        .map_err(|e| decode("resource programs", e))?
                        .programs,
                );
            }
            programs.into_iter().nth(program as usize)
        } else if program == 0 {
            chunks
                .find_first(0x28)
                .map(Program::try_from)
                .transpose()
                .map_err(|e| decode("resource program", e))?
        } else {
            None
        };
        let selected = selected.ok_or_else(|| LoadError::Invalid {
            path: instrument.into(),
            reason: "resource program is absent".into(),
        })?;
        let specials = match chunks
            .filename_tables()
            .map_err(|e| decode("resource filenames", e))?
        {
            Some(table) => table.special_filetable,
            None => chunks
                .find_first(0x3d)
                .map(FileNameListPreK51::try_from)
                .transpose()
                .map_err(|e| decode("resource legacy filenames", e))?
                .map(|t| t.special_filetable)
                .unwrap_or_default(),
        };
        Self::program_resources(instrument, &selected, &specials)
    }

    fn program_resources(
        instrument: &Path,
        program: &ni_file::kontakt::objects::Program,
        specials: &std::collections::HashMap<u32, String>,
    ) -> Result<Self, LoadError> {
        let invalid = |reason: &str| LoadError::Invalid {
            path: instrument.into(),
            reason: reason.into(),
        };
        let record = program
            .public_record()
            .map_err(|e| LoadError::decode(instrument, "resource owner", e))?;
        let Some(reference) = record.resource_container_filename_ref.filter(|r| *r != -1) else {
            return Ok(Self::of(instrument));
        };
        let name = specials
            .get(&(reference as u32))
            .ok_or_else(|| invalid("authored resource container reference is absent"))?;
        if name.is_empty() {
            return Ok(Self::of(instrument));
        }
        if !Path::new(&name.replace('\\', "/"))
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nkr") || e.eq_ignore_ascii_case("nicnt"))
        {
            return Err(invalid(
                "authored resource owner is not a resource container",
            ));
        }
        let base = Self::of(instrument);
        let mut resolver = crate::Samples::new(base.root());
        let container = resolver
            .resolve(instrument.parent().unwrap_or(Path::new(".")), name)?
            .filter(|p| p.is_file())
            .ok_or_else(|| invalid("authored resource container is unavailable"))?;
        // Rebase loose resources as well as the archive. Never let another
        // program's same-named module satisfy an explicit resource owner.
        let mut owned = Self::of(&container);
        owned.containers = vec![container];
        Ok(owned)
    }

    pub fn of(instrument: &Path) -> Self {
        let path = std::fs::canonicalize(instrument).unwrap_or_else(|_| {
            std::path::absolute(instrument).unwrap_or_else(|_| instrument.into())
        });
        let ancestors: Vec<_> = path
            .ancestors()
            .skip(1)
            .take_while(|p| !named(p, "Kontakt") && !named(p, "Libraries"))
            .take(12)
            .collect();
        let root = ancestors
            .iter()
            .copied()
            .find(|p| {
                entries(p).iter().any(|c| {
                    named(c, "Samples")
                        || c.extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("nicnt"))
                })
            })
            .or_else(|| {
                ancestors
                    .iter()
                    .copied()
                    .find(|p| entries(p).iter().any(|c| named(c, "Resources")))
            })
            .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")))
            .to_path_buf();
        let mut files = BTreeMap::<String, Vec<PathBuf>>::new();
        let mut containers = Vec::new();
        for folder in ancestors.into_iter().take_while(|p| p.starts_with(&root)) {
            let here = entries(folder);
            for res in here.iter().filter(|p| named(p, "Resources")) {
                for entry in loose_files(res) {
                    if let Ok(relative) = entry.strip_prefix(res) {
                        let key = format!(
                            "resources/{}",
                            relative.to_string_lossy().replace('\\', "/").to_lowercase()
                        );
                        let paths = files.entry(key).or_default();
                        // Nearest resource tree wins; same-tree case aliases remain ambiguous.
                        if paths.is_empty() || paths[0].starts_with(res) {
                            paths.push(entry);
                        }
                    }
                }
            }
            let is_container = |p: &&PathBuf| {
                p.extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("nkr") || e.eq_ignore_ascii_case("nicnt")
                })
            };
            containers.extend(here.iter().filter(is_container).cloned());
            for sub in here
                .iter()
                .filter(|p| p.is_dir() && !named(p, "Instruments"))
            {
                containers.extend(entries(sub).iter().filter(is_container).cloned());
            }
        }
        containers.sort_by_key(|p| {
            (
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("nicnt")),
                p.clone(),
            )
        });
        containers.dedup();
        Self {
            files,
            containers,
            open: Vec::new(),
            failures: Vec::new(),
            indexed: false,
            root,
        }
    }

    /// Files and containers that participate in lookup, for read-only diagnostics
    /// and survey cache identity. Resource contents and library keys stay private.
    pub fn locations(&self) -> Vec<PathBuf> {
        let mut paths: Vec<_> = self
            .files
            .values()
            .flatten()
            .cloned()
            .chain(self.containers.iter().cloned())
            .collect();
        paths.sort();
        paths.dedup();
        paths
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    fn index(&mut self) {
        if self.indexed {
            return;
        }
        self.indexed = true;
        for path in &self.containers {
            match ResourceContainer::open(path) {
                Ok(container) => self.open.push(container),
                Err(e) => self.failures.push(e.to_string()),
            }
        }
    }

    /// Resolve linked script source before the inline fallback. Resource bytes
    /// remain in memory and are never written to a plaintext cache.
    pub fn script(&mut self, name: &str) -> Option<String> {
        let name = name.trim();
        let file = name.rsplit(['/', '\\']).next()?;
        if file.is_empty() || matches!(file, "." | "..") {
            return None;
        }
        let normalized = name.replace('\\', "/");
        let bytes = self
            .read(&normalized)
            .or_else(|| self.read(&format!("Resources/scripts/{file}")))?;
        let text = script_text(&bytes);
        (!text.trim().is_empty()).then_some(text)
    }
    /// Normalized namespace members. No bytes are extracted or persisted.
    pub fn names(&mut self, prefix: &str) -> Vec<String> {
        self.index();
        let prefix = prefix.replace(['\\', '|'], "/").to_lowercase();
        let mut names: std::collections::BTreeSet<_> = self
            .files
            .keys()
            .filter(|n| n.starts_with(&prefix))
            .cloned()
            .collect();
        for container in &self.open {
            names.extend(
                container
                    .names()
                    .into_iter()
                    .filter_map(normalize)
                    .filter(|n| n.starts_with(&prefix)),
            );
        }
        names.into_iter().collect()
    }
    /// None means absent. Invalid, ambiguous, corrupt and inaccessible resources remain errors.
    pub fn read_result(&mut self, path: &str) -> Result<Option<Vec<u8>>, LoadError> {
        let root = self.root.clone();
        let invalid = |reason: &str| LoadError::Invalid {
            path: root.clone(),
            reason: reason.into(),
        };
        let path =
            normalize(path).ok_or_else(|| invalid("Invalid library-relative resource path"))?;
        if let Some(paths) = self.files.get(&path) {
            if paths.len() != 1 {
                return Err(invalid("Ambiguous loose resource name"));
            }
            let file = std::fs::File::open(&paths[0]).map_err(|e| LoadError::io(&paths[0], e))?;
            if file
                .metadata()
                .map_err(|e| LoadError::io(&paths[0], e))?
                .len()
                > LIMIT
            {
                return Err(invalid("Resource exceeds 32 MiB"));
            }
            let mut bytes = Vec::new();
            file.take(LIMIT + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| LoadError::io(&paths[0], e))?;
            if bytes.len() as u64 > LIMIT {
                return Err(invalid("Resource exceeds 32 MiB"));
            }
            return Ok(Some(bytes));
        }
        self.index();
        for container in &mut self.open {
            if let Some(bytes) = container.read(&path)? {
                return Ok(Some(bytes));
            }
        }
        if !self.failures.is_empty() {
            return Err(invalid("Resource container could not be indexed"));
        }
        Ok(None)
    }
    /// Compatibility metadata probe; painting uses read_result for diagnostics.
    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        self.read_result(path).ok().flatten()
    }
    /// The layout of the picture at `path` (`.png`): its `.txt` and its
    /// frame size from the image header, whichever the library has.
    pub fn picture(&mut self, path: &str) -> Option<sampler_ui_ir::ImageMeta> {
        let png = self.read(path);
        let txt = path
            .rsplit_once('.')
            .map_or(path.to_owned(), |(stem, _)| format!("{stem}.txt"));
        let txt = self
            .read(&txt)
            .map(|t| String::from_utf8_lossy(&t).into_owned());
        if png.is_none() && txt.is_none() {
            return None;
        }
        Some(sampler_ksp::ui::picture_meta_png(
            txt.as_deref(),
            &png.unwrap_or_default(),
        ))
    }
}
fn script_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    if bytes.starts_with(b"\xff\xfe")
        || (bytes.len() >= 4 && bytes[0] != 0 && bytes[1] == 0 && bytes[3] == 0)
    {
        let units: Vec<_> = bytes
            .strip_prefix(b"\xff\xfe")
            .unwrap_or(bytes)
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    // ponytail: legacy byte strings use v1's Latin-1 fallback; use a Windows-1252
    // decoder if a library needs the 0x80..0x9f punctuation mapping.
    String::from_utf8(bytes.to_vec())
        .unwrap_or_else(|_| bytes.iter().map(|&b| char::from(b)).collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn program_container_reference_disambiguates_same_named_native_modules() {
        use ni_file::kontakt::{StructuredObject, objects::Program};
        let root = std::env::temp_dir().join(format!(
            "kontakt-program-resource-owner-{}",
            std::process::id()
        ));
        let library = root.join("Library");
        let factory = library.join("Factory/Tool");
        for (folder, text) in [
            (&library, b"library main".as_slice()),
            (&factory, b"controller main".as_slice()),
        ] {
            std::fs::create_dir_all(folder.join("Resources/native_ui")).unwrap();
            std::fs::create_dir_all(folder.join("Samples")).unwrap();
            std::fs::write(folder.join("Resources/native_ui/main.nui"), text).unwrap();
            // Empty valid NKR index; loose resources keep their documented precedence.
            let mut archive = 0x5e70ac54u32.to_le_bytes().to_vec();
            archive.extend(0x110u16.to_le_bytes());
            archive.extend([0; 16]);
            std::fs::write(folder.join("Samples/owner.nkr"), archive).unwrap();
        }
        std::fs::create_dir_all(library.join("Instruments")).unwrap();
        let path = library.join("Instruments/Multi.nkm");
        let specials = [
            (0, "Samples/owner.nkr".into()),
            (1, "Factory/Tool/Samples/owner.nkr".into()),
        ]
        .into();
        let program = |reference: i32| {
            // Verified v0x91 BProgram prefix and F0; no later optional suffix.
            let mut public_data = vec![0; 70];
            public_data.extend(reference.to_le_bytes());
            Program(StructuredObject {
                version: 0x91,
                public_data,
                private_data: vec![],
                children: vec![],
            })
        };
        assert_eq!(
            super::Resources::of(&path)
                .read("Resources/native_ui/main.nui")
                .unwrap(),
            b"library main"
        );
        for (reference, expected) in [
            (0, b"library main".as_slice()),
            (1, b"controller main".as_slice()),
        ] {
            let mut owned =
                super::Resources::program_resources(&path, &program(reference), &specials).unwrap();
            assert_eq!(
                owned.read("Resources/native_ui/main.nui").unwrap(),
                expected
            );
            assert_eq!(
                owned
                    .locations()
                    .iter()
                    .filter(|p| p.extension().is_some_and(|e| e == "nkr"))
                    .count(),
                1,
                "only the authored container can satisfy this program's module names"
            );
        }
        assert!(
            super::Resources::program_resources(&path, &program(7), &specials).is_err(),
            "an unresolved owner must not silently borrow the library's main"
        );
        assert!(super::Resources::program_resources(&path, &program(-7), &specials).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn linked_scripts_reload_from_resources_and_decode_saved_encodings() {
        let dir =
            std::env::temp_dir().join(format!("kontakt-linked-script-{}", std::process::id()));
        let scripts = dir.join("Resources/Scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::create_dir_all(dir.join("Instruments")).unwrap();
        let source = "on init\nmessage(\"linked\")\nend on";
        let mut bytes = vec![0xff, 0xfe];
        for unit in source.encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        std::fs::write(scripts.join("Linked.txt"), bytes).unwrap();
        std::fs::write(scripts.join("Empty.txt"), b" \n").unwrap();
        let mut r = super::Resources::of(&dir.join("Instruments/Piano.nki"));
        assert_eq!(r.script(r"C:\old\LINKED.TXT").as_deref(), Some(source));
        for missing in ["Empty.txt", "missing.txt", "..", ""] {
            assert!(r.script(missing).is_none());
        }
        assert_eq!(super::script_text(b"\xef\xbb\xbfhello"), "hello");
        assert_eq!(super::script_text(b"caf\xe9"), "café");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nested_names_and_library_boundary() {
        let dir =
            std::env::temp_dir().join(format!("kontakt-resource-boundary-{}", std::process::id()));
        let own = dir.join("own");
        std::fs::create_dir_all(own.join("Instruments/Deep")).unwrap();
        std::fs::create_dir_all(own.join("Resources/native_ui/nested")).unwrap();
        std::fs::create_dir_all(own.join("Samples")).unwrap();
        std::fs::create_dir_all(dir.join("sibling/Resources/pictures")).unwrap();
        std::fs::write(own.join("Resources/native_ui/nested/Main.nui"), b"own").unwrap();
        std::fs::write(
            dir.join("sibling/Resources/pictures/missing.png"),
            b"foreign",
        )
        .unwrap();
        let mut r = super::Resources::of(&own.join("Instruments/Deep/Piano.nki"));
        assert_eq!(
            r.read("resources\\NATIVE_UI\\nested\\MAIN.nui"),
            Some(b"own".to_vec())
        );
        assert!(r.read("Resources/pictures/missing.png").is_none());
        assert!(r.read("Resources/native_ui/../../missing.png").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn loose_pictures_are_found_above_the_instrument() {
        let dir = std::env::temp_dir().join(format!("kontakt-resources-{}", std::process::id()));
        let pictures = dir.join("Resources/pictures");
        std::fs::create_dir_all(&pictures).unwrap();
        std::fs::create_dir_all(dir.join("Instruments")).unwrap();
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(40u32.to_be_bytes());
        png.extend(310u32.to_be_bytes());
        std::fs::write(pictures.join("Knob.png"), &png).unwrap();
        std::fs::write(pictures.join("Knob.txt"), "Number of Animations: 31\n").unwrap();
        let mut r = super::Resources::of(&dir.join("Instruments/Piano.nki"));
        let meta = r.picture("Resources/pictures/Knob.png").unwrap();
        assert_eq!(
            (meta.frames, meta.size.map(|s| (s.width, s.height))),
            (31, Some((40., 10.)))
        );
        assert!(r.picture("Resources/pictures/missing.png").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
