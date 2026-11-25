//! What a script's file selector lists (`ui_file_selector`), as Kontakt 8
//! orders it: a folder's folders, then its files, each by name, case aside.
//! The editor draws it; `fs_navigate` steps through it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// One folder or file a column lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub folder: bool,
}

/// `dir`'s folders then its files with extensions `ext`, each by name, case
/// aside; read once per change of the folder.
pub fn list(dir: &Path, ext: &[&str]) -> Arc<[Entry]> {
    type Cache = HashMap<(PathBuf, String), (Option<SystemTime>, Arc<[Entry]>)>;
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    let stamp = std::fs::metadata(dir).and_then(|m| m.modified()).ok();
    let key = (dir.to_path_buf(), ext.join(","));
    if let Ok(cache) = CACHE.lock()
        && let Some((at, entries)) = cache.as_ref().and_then(|c| c.get(&key))
        && *at == stamp
    {
        return entries.clone();
    }
    // A listing an I/O error cut short is shown but not cached, so the next
    // call reads again; a name that is not UTF-8 is left out (a script's
    // path string cannot name it).
    let mut complete = true;
    let read = std::fs::read_dir(dir);
    complete &= read.is_ok();
    let mut entries: Vec<Entry> = read
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let Some((kind, e)) = e.ok().and_then(|e| Some((e.file_type().ok()?, e))) else {
                complete = false;
                return None;
            };
            let name = e.file_name().into_string().ok()?;
            let folder = kind.is_dir();
            let wanted = folder
                || Path::new(&name)
                    .extension()
                    .is_some_and(|x| ext.iter().any(|w| x.eq_ignore_ascii_case(w)));
            (wanted && !name.starts_with('.')).then_some(Entry { name, folder })
        })
        .collect();
    entries.sort_by_cached_key(|e| (!e.folder, e.name.to_lowercase()));
    let entries: Arc<[Entry]> = entries.into();
    if complete && let Ok(mut cache) = CACHE.lock() {
        // ponytail: never evicts; a selector browsing thousands of folders
        // keeps their listings until the plugin closes.
        cache
            .get_or_insert_with(HashMap::new)
            .insert(key, (stamp, entries.clone()));
    }
    entries
}

/// The file listed before (`next` false) or after the one at `path` in its
/// folder, folders skipped; None at either end or when `path` lists nothing.
#[must_use]
pub fn step(path: &str, ext: &[&str], next: bool) -> Option<PathBuf> {
    let path = Path::new(path);
    let (dir, name) = (path.parent()?, path.file_name()?.to_str()?);
    let listed = list(dir, ext);
    let files: Vec<&Entry> = listed.iter().filter(|e| !e.folder).collect();
    let at = files.iter().position(|e| e.name == name)?;
    let to = if next {
        at.checked_add(1)?
    } else {
        at.checked_sub(1)?
    };
    Some(dir.join(&files.get(to)?.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_through_a_folders_files_in_listed_order() {
        let dir = std::env::temp_dir().join(format!("kontra-selector-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Sub")).unwrap();
        for f in ["b.wav", "A.wav", "c.txt", "d.WAV"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let ext = &["wav"];
        let at = |f: &str| dir.join(f).to_string_lossy().into_owned();
        assert_eq!(step(&at("A.wav"), ext, true), Some(dir.join("b.wav")));
        assert_eq!(step(&at("b.wav"), ext, true), Some(dir.join("d.WAV")));
        assert_eq!(step(&at("b.wav"), ext, false), Some(dir.join("A.wav")));
        assert_eq!(step(&at("A.wav"), ext, false), None);
        assert_eq!(step(&at("d.WAV"), ext, true), None);
        assert_eq!(step("", ext, true), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
