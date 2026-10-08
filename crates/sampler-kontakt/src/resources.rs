//! An instrument's resource files (script pictures and their `.txt`
//! layouts, performance views): loose `Resources/<kind>` folders near the
//! instrument, else its resource container (`.nkr`), opened with the
//! library's own key.

use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct Resources {
    /// Loose files by lowercase library-relative path (`resources/pictures/x.png`).
    files: HashMap<String, PathBuf>,
    /// Containers not opened yet, nearest first.
    containers: Vec<PathBuf>,
    open: Vec<(File, ni_file::nkr::Archive)>,
    key: Option<Option<Arc<dyn ni_file::nis::LibraryKey>>>,
    instrument: PathBuf,
}

impl Resources {
    /// Kontakt stores either a basename or an authored host path. Resolve its
    /// basename inside this instrument's resources, never the saved host path.
    pub(crate) fn linked_script(&mut self, name: &str) -> Option<String> {
        let file = name.trim().rsplit(['/', '\\']).next()?;
        if file.is_empty() || matches!(file, "." | "..") {
            return None;
        }
        let bytes = self.read(&format!("Resources/scripts/{file}"))?;
        let text = script_text(&bytes);
        (!text.trim().is_empty()).then_some(text)
    }

    /// What lies near `instrument`; nothing is read until asked for.
    pub fn of(instrument: &Path) -> Self {
        let entries = |dir: &Path| -> Vec<PathBuf> {
            let mut v: Vec<_> = std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .collect();
            v.sort();
            v
        };
        let named = |p: &Path, name: &str| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
        };
        let nkr = |p: &PathBuf| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkr"));
        let (mut files, mut containers) = (HashMap::new(), Vec::new());
        // Up to four folders up, further only while nothing was found.
        for (depth, folder) in instrument.ancestors().skip(1).take(8).enumerate() {
            if depth >= 4 && !(files.is_empty() && containers.is_empty()) {
                break;
            }
            for res in entries(folder)
                .into_iter()
                .filter(|p| named(p, "Resources"))
            {
                for sub in entries(&res).into_iter().filter(|p| p.is_dir()) {
                    let kind = sub.file_name().unwrap().to_string_lossy().to_lowercase();
                    for f in entries(&sub) {
                        let name = f.file_name().unwrap().to_string_lossy().to_lowercase();
                        files.entry(format!("resources/{kind}/{name}")).or_insert(f);
                    }
                }
            }
            let here = entries(folder);
            containers.extend(here.iter().filter(|p| nkr(p)).cloned());
            // Some libraries keep their container with the samples.
            for sub in here.iter().filter(|p| p.is_dir()) {
                containers.extend(entries(sub).into_iter().filter(nkr));
            }
        }
        containers.dedup();
        Self {
            files,
            containers,
            open: Vec::new(),
            key: None,
            instrument: instrument.into(),
        }
    }

    /// The bytes at library-relative `path`, if the library has them.
    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        if let Some(f) = self.files.get(&path.to_lowercase()) {
            if std::fs::metadata(f).ok()?.len() > 32 << 20 {
                return None;
            }
            return std::fs::read(f).ok();
        }
        for n in 0.. {
            while n == self.open.len() {
                if self.containers.is_empty() {
                    return None;
                }
                let at = self.containers.remove(0);
                if let Some(open) = File::open(&at)
                    .ok()
                    .and_then(|mut f| Some((ni_file::nkr::Archive::read_index(&mut f).ok()?, f)))
                {
                    self.open.push((open.1, open.0));
                }
            }
            let (f, archive) = &mut self.open[n];
            let Ok(Some(entry)) = archive.member(&mut *f, path) else {
                continue;
            };
            if !entry.valid || entry.size > 32 << 20 {
                continue;
            }
            let needs_key = entry.encoded && entry.key_index != 0xff;
            archive.entries.insert(entry.name.to_lowercase(), entry);
            let key = match &self.key {
                _ if !needs_key => None,
                Some(key) => key.clone(),
                None => self
                    .key
                    .insert(crate::library_key(&self.instrument).ok())
                    .clone(),
            };
            return archive.read_entry_with_key(f, path, key.as_deref()).ok();
        }
        None
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
        assert_eq!(
            r.linked_script(r"C:\old\LINKED.TXT").as_deref(),
            Some(source)
        );
        for missing in ["Empty.txt", "missing.txt", "..", ""] {
            assert!(r.linked_script(missing).is_none());
        }
        assert_eq!(super::script_text(b"\xef\xbb\xbfhello"), "hello");
        assert_eq!(super::script_text(b"caf\xe9"), "café");
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
