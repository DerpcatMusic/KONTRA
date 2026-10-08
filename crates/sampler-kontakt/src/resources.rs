//! Bounded library-relative resources shared by KSP pictures, fonts and NativeUI.
//! Loose files override NKR members, which override NICNT members. Never search siblings.
use std::{collections::BTreeMap, io::Read, path::{Path, PathBuf}};
use crate::{LoadError, ResourceContainer};
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
    let mut files: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
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
            if count > 65536 { return files; }
            let Ok(meta) = path.symlink_metadata() else { continue };
            if meta.is_file() { files.push(path); }
            else if meta.is_dir() && depth < 32 { pending.push((path, depth + 1)); }
        }
    }
    files
}
fn named(p: &Path, name: &str) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
}
fn normalize(name: &str) -> Option<String> {
    let name = name.replace(['\\', '|'], "/");
    if name.starts_with('/') || name.contains(':') || name.split('/').any(|s| s.is_empty() || s == "." || s == "..") { return None; }
    Some(name.to_lowercase())
}
impl Resources {
    pub fn of(instrument: &Path) -> Self {
        let path = std::fs::canonicalize(instrument).unwrap_or_else(|_| std::path::absolute(instrument).unwrap_or_else(|_| instrument.into()));
        let ancestors: Vec<_> = path.ancestors().skip(1).take_while(|p| !named(p, "Kontakt") && !named(p, "Libraries")).take(12).collect();
        let root = ancestors.iter().copied().find(|p| entries(p).iter().any(|c| named(c, "Samples") || c.extension().is_some_and(|e| e.eq_ignore_ascii_case("nicnt"))))
            .or_else(|| ancestors.iter().copied().find(|p| entries(p).iter().any(|c| named(c, "Resources"))))
            .unwrap_or_else(|| path.parent().unwrap_or(Path::new("."))).to_path_buf();
        let mut files = BTreeMap::<String, Vec<PathBuf>>::new();
        let mut containers = Vec::new();
        for folder in ancestors.into_iter().take_while(|p| p.starts_with(&root)) {
            let here = entries(folder);
            for res in here.iter().filter(|p| named(p, "Resources")) {
                for entry in loose_files(res) {
                    if let Ok(relative) = entry.strip_prefix(res) {
                        let key = format!("resources/{}", relative.to_string_lossy().replace('\\', "/").to_lowercase());
                        let paths = files.entry(key).or_default();
                        // Nearest resource tree wins; same-tree case aliases remain ambiguous.
                        if paths.is_empty() || paths[0].starts_with(res) { paths.push(entry); }
                    }
                }
            }
            let is_container = |p: &&PathBuf| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkr") || e.eq_ignore_ascii_case("nicnt"));
            containers.extend(here.iter().filter(is_container).cloned());
            for sub in here.iter().filter(|p| p.is_dir() && !named(p, "Instruments")) {
                containers.extend(entries(sub).iter().filter(is_container).cloned());
            }
        }
        containers.sort_by_key(|p| (p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nicnt")), p.clone()));
        containers.dedup();
        Self { files, containers, open: Vec::new(), failures: Vec::new(), indexed: false, root }
    }
    pub fn root(&self) -> &Path { &self.root }
    fn index(&mut self) {
        if self.indexed { return; }
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
        let normalized = name.replace('\\', "/");
        let bytes = self
            .read(&normalized)
            .or_else(|| self.read(&format!("Resources/scripts/{normalized}")))?;
        Some(ni_file::kontakt::objects::BParScript::decode_source(bytes))
    }
    /// Normalized namespace members. No bytes are extracted or persisted.
    pub fn names(&mut self, prefix: &str) -> Vec<String> {
        self.index();
        let prefix = prefix.replace(['\\', '|'], "/").to_lowercase();
        let mut names: std::collections::BTreeSet<_> = self.files.keys().filter(|n| n.starts_with(&prefix)).cloned().collect();
        for container in &self.open {
            names.extend(container.names().into_iter().filter_map(normalize).filter(|n| n.starts_with(&prefix)));
        }
        names.into_iter().collect()
    }
    pub fn locations(&self) -> Vec<PathBuf> {
        self.files.values().flatten().cloned().chain(self.containers.iter().cloned()).collect()
    }
    /// None means absent. Invalid, ambiguous, corrupt and inaccessible resources remain errors.
    pub fn read_result(&mut self, path: &str) -> Result<Option<Vec<u8>>, LoadError> {
        let root = self.root.clone();
        let invalid = |reason: &str| LoadError::Invalid { path: root.clone(), reason: reason.into() };
        let path = normalize(path).ok_or_else(|| invalid("Invalid library-relative resource path"))?;
        if let Some(paths) = self.files.get(&path) {
            if paths.len() != 1 { return Err(invalid("Ambiguous loose resource name")); }
            let file = std::fs::File::open(&paths[0]).map_err(|e| LoadError::io(&paths[0], e))?;
            if file.metadata().map_err(|e| LoadError::io(&paths[0], e))?.len() > LIMIT { return Err(invalid("Resource exceeds 32 MiB")); }
            let mut bytes = Vec::new();
            file.take(LIMIT + 1).read_to_end(&mut bytes).map_err(|e| LoadError::io(&paths[0], e))?;
            if bytes.len() as u64 > LIMIT { return Err(invalid("Resource exceeds 32 MiB")); }
            return Ok(Some(bytes));
        }
        self.index();
        for container in &mut self.open {
            if let Some(bytes) = container.read(&path)? { return Ok(Some(bytes)); }
        }
        if !self.failures.is_empty() { return Err(invalid("Resource container could not be indexed")); }
        Ok(None)
    }
    /// Compatibility metadata probe; painting uses read_result for diagnostics.
    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> { self.read_result(path).ok().flatten() }
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
#[cfg(test)]
mod tests {
    #[test]
    fn nested_names_and_library_boundary() {
        let dir = std::env::temp_dir().join(format!("kontakt-resource-boundary-{}", std::process::id()));
        let own = dir.join("own");
        std::fs::create_dir_all(own.join("Instruments/Deep")).unwrap();
        std::fs::create_dir_all(own.join("Resources/native_ui/nested")).unwrap();
        std::fs::create_dir_all(own.join("Samples")).unwrap();
        std::fs::create_dir_all(dir.join("sibling/Resources/pictures")).unwrap();
        std::fs::write(own.join("Resources/native_ui/nested/Main.nui"), b"own").unwrap();
        std::fs::write(dir.join("sibling/Resources/pictures/missing.png"), b"foreign").unwrap();
        let mut r = super::Resources::of(&own.join("Instruments/Deep/Piano.nki"));
        assert_eq!(r.read("resources\\NATIVE_UI\\nested\\MAIN.nui"), Some(b"own".to_vec()));
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
            (31, Some((40, 10)))
        );
        assert!(r.picture("Resources/pictures/missing.png").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
