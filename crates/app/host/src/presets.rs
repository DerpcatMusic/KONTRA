//! The player's own presets: what a project saves of a part's scripts, kept
//! as `<folder>/<library>/<name>.kontra` under KONTRA's own presets folder,
//! never in a library. Loading one is loading its base instrument with
//! these values (a recall when the part already plays that base).
//!
//! Writes go to a temporary file of their own, then into place: a new
//! preset never replaces a file (the filesystem refuses one already there,
//! whatever its case), a replace is one rename. Rename and delete touch
//! only `.kontra` files inside the folder; no path read from a preset file
//! is ever written to.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::state::{SlotResponse, SlotType};

/// The extension of a user preset file.
pub const EXTENSION: &str = "kontra";
pub const VERSION: u32 = 1;
/// The most a preset file may hold; anything larger is not one.
const LIMIT: u64 = 16 << 20;

/// A user preset: its base instrument and what a project saves of a part
/// playing it: by instrument script its values, and the effect types and
/// responses its scripts loaded after `on init`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserPreset {
    pub version: u32,
    pub base: PathBuf,
    pub scripts: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slot_types: Vec<SlotType>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slot_responses: Vec<SlotResponse>,
}

/// KONTRA's presets folder.
#[must_use]
pub fn folder() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("kontra/presets"))
}

pub use kontra_ui_model::preset_file_name as sanitized;

/// Whether a save may replace a preset of the same name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overwrite {
    /// A new preset: a file of that name (in any case) refuses it.
    Create,
    /// The player chose to replace the preset of that name.
    Replace,
}

/// Whether `path` names a user preset file, by its extension in any case.
#[must_use]
pub fn ours(path: &Path) -> bool {
    (path.extension()).is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// `library`'s folder under `folder`: its name with separators, reserved
/// and control characters, `%` and a leading dot percent-encoded, so two
/// libraries never share one.
fn library_dir(folder: &Path, library: &str) -> io::Result<PathBuf> {
    let mut name = String::new();
    for (at, c) in library.trim().chars().enumerate() {
        if c.is_control() || r#"/\:*?"<>|%"#.contains(c) || (at == 0 && c == '.') {
            for b in c.encode_utf8(&mut [0; 4]).bytes() {
                name.push_str(&format!("%{b:02X}"));
            }
        } else {
            name.push(c);
        }
    }
    match name.is_empty() {
        true => Err(invalid("library name")),
        false => Ok(folder.join(name)),
    }
}

/// `bytes` in a new file of `dir` no other save uses; its path. Nothing is
/// left behind when it cannot be written.
fn written(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SAVES: AtomicU64 = AtomicU64::new(0);
    let n = SAVES.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let done = file.write_all(bytes).and_then(|()| file.sync_all());
    if let Err(e) = done {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    Ok(temp)
}
fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, what)
}

/// Puts the new file `temp` (holding `bytes`) at `path` by `link`, which
/// the filesystem refuses in one step when the name is taken. Where it
/// has no links (FAT, exFAT, some network mounts) the file is created
/// there exclusively instead and written, removed again if the write
/// fails.
fn created(
    temp: &Path,
    path: &Path,
    bytes: &[u8],
    link: impl Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    use std::io::Write;
    match link(temp, path) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => {
            let mut file = std::fs::File::create_new(path)?;
            let done = file.write_all(bytes).and_then(|()| file.sync_all());
            if done.is_err() {
                drop(file);
                let _ = std::fs::remove_file(path);
            }
            done
        }
        linked => linked,
    }
}

/// Writes `preset` as `name` of `library`; its path. A preset of that
/// name stays unless `overwrite` is [`Overwrite::Replace`].
///
/// # Errors
/// `AlreadyExists` when creating over a preset of that name; when a name
/// is empty once sanitised, the preset is larger than a preset file may
/// be, or the file cannot be written.
pub fn save(
    folder: &Path,
    library: &str,
    name: &str,
    preset: &UserPreset,
    overwrite: Overwrite,
) -> io::Result<PathBuf> {
    let dir = library_dir(folder, library)?;
    let name = sanitized(name).ok_or_else(|| invalid("preset name"))?;
    let bytes = serde_json::to_vec_pretty(preset).map_err(io::Error::other)?;
    if u64::try_from(bytes.len()).map_or(true, |n| n > LIMIT) {
        return Err(invalid("preset larger than a preset file may be"));
    }
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.{EXTENSION}"));
    let temp = written(&dir, &name, &bytes)?;
    let placed = match overwrite {
        Overwrite::Create => created(&temp, &path, &bytes, |from, to| {
            std::fs::hard_link(from, to)
        }),
        Overwrite::Replace => std::fs::rename(&temp, &path),
    };
    if overwrite == Overwrite::Create || placed.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    placed.map(|()| path)
}

/// The user preset at `path`; None for any other file.
#[must_use]
pub fn read(path: &Path) -> Option<UserPreset> {
    let meta = std::fs::metadata(path)
        .ok()
        .filter(|m| ours(path) && m.is_file())?;
    if meta.len() > LIMIT {
        return None;
    }
    let preset: UserPreset = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    // Its base is an instrument, never another preset.
    (preset.version == VERSION && !ours(&preset.base)).then_some(preset)
}

/// `path` itself when it is a preset file inside `folder`.
fn inside(folder: &Path, path: &Path) -> io::Result<PathBuf> {
    let (folder, path) = (folder.canonicalize()?, path.canonicalize()?);
    match path.starts_with(&folder) && ours(&path) {
        true => Ok(path),
        false => Err(invalid("not a KONTRA user preset")),
    }
}

/// Renames the preset at `path` to `name` in its folder; its new path.
///
/// # Errors
/// When `path` is not a user preset in `folder`, the name is empty or
/// taken, or the rename fails.
pub fn rename(folder: &Path, path: &Path, name: &str) -> io::Result<PathBuf> {
    let real = inside(folder, path)?;
    let name = sanitized(name).ok_or_else(|| invalid("preset name"))?;
    let file = format!("{name}.{EXTENSION}");
    let to = real.with_file_name(&file);
    if to.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, name));
    }
    std::fs::rename(&real, &to)?;
    // As the caller spelled its folder (the listing's spelling, which may
    // pass through a link), when that is the folder the file is really in.
    let spelled = path.parent().filter(|dir| {
        dir.canonicalize().ok().as_deref() == to.parent() && path.file_name() == real.file_name()
    });
    Ok(spelled.map_or(to, |dir| dir.join(file)))
}

/// Deletes the preset at `path`.
///
/// # Errors
/// When `path` is not a user preset in `folder`, or it cannot be removed.
pub fn delete(folder: &Path, path: &Path) -> io::Result<()> {
    std::fs::remove_file(inside(folder, path)?)
}

/// `library`'s user presets, sorted by name.
#[must_use]
pub fn list(folder: &Path, library: &str) -> Vec<PathBuf> {
    let Ok(dir) = library_dir(folder, library) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = (std::fs::read_dir(dir).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| ours(p))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of this test alone, removed when dropped (also when an
    /// assertion fails).
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn preset(scripts: Vec<Vec<String>>) -> UserPreset {
        UserPreset {
            version: VERSION,
            base: PathBuf::from("/lib/Piano.nki"),
            scripts,
            slot_types: Vec::new(),
            slot_responses: Vec::new(),
        }
    }

    #[test]
    fn a_preset_saves_renames_and_deletes_only_inside_its_folder() {
        let root =
            Scratch(std::env::temp_dir().join(format!("kontra-presets-{}", std::process::id())));
        let folder = root.0.join("presets");
        let scripts = vec![vec!["1".to_owned()]];
        let one = preset(scripts.clone());
        let create = Overwrite::Create;
        let path = save(&folder, "Una/Corda", "../My: Soft*", &one, create).unwrap();
        assert_eq!(path, folder.join("Una%2FCorda").join("My Soft.kontra"));
        assert_eq!(read(&path).map(|p| p.scripts), Some(scripts.clone()));
        assert_eq!(list(&folder, "Una/Corda"), std::slice::from_ref(&path));
        // Libraries whose names differ by a reserved character list apart.
        let other = save(&folder, "UnaCorda", "Bright", &one, create).unwrap();
        assert_eq!(list(&folder, "UnaCorda"), std::slice::from_ref(&other));
        assert!(sanitized(" .. ").is_none());
        // The editor's Replace check names collisions by this file name.
        assert_eq!(sanitized("Soft?"), sanitized("Soft"));
        // Creating over a preset of that name is refused; it stays as it was.
        let two = preset(vec![vec!["2".to_owned()]]);
        let taken = save(&folder, "Una/Corda", "My Soft", &two, create);
        assert_eq!(
            taken.map_err(|e| e.kind()),
            Err(io::ErrorKind::AlreadyExists)
        );
        assert_eq!(read(&path).map(|p| p.scripts), Some(scripts.clone()));
        // A preset too large to read back is refused before the file moves.
        let huge = preset(vec![vec!["x".repeat(usize::try_from(LIMIT).unwrap())]]);
        assert!(save(&folder, "Una/Corda", "My Soft", &huge, Overwrite::Replace).is_err());
        assert_eq!(read(&path).map(|p| p.scripts), Some(scripts.clone()));
        let replaced = save(&folder, "Una/Corda", "My Soft", &two, Overwrite::Replace).unwrap();
        assert_eq!(
            read(&replaced).map(|p| p.scripts),
            Some(two.scripts.clone())
        );
        // No temporary file stays behind.
        let files = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(files, 1);
        // The name is taken: no rename over it.
        let bright = save(&folder, "Una/Corda", "Bright", &one, create).unwrap();
        assert!(rename(&folder, &path, "Bright").is_err());
        let path = rename(&folder, &path, "Soft").unwrap();
        // The new path keeps the spelling of the folder it was listed under.
        assert_eq!(path, folder.join("Una%2FCorda").join("Soft.kontra"));
        // Nothing outside the folder, nor any other file, is removed.
        let outside = root.0.join("x.kontra");
        std::fs::write(&outside, b"{}").unwrap();
        assert!(delete(&folder, &outside).is_err());
        assert!(delete(&folder, &path).is_ok() && delete(&folder, &bright).is_ok());
        assert!(list(&folder, "Una/Corda").is_empty() && outside.exists());
        assert!(ours(Path::new("a.KONTRA")) && !ours(Path::new("a.nki")));
    }

    /// Without hard links the file is created in place, and a name that is
    /// taken is still refused.
    #[test]
    fn a_new_preset_is_created_where_files_cannot_be_linked() {
        let root =
            Scratch(std::env::temp_dir().join(format!("kontra-nolink-{}", std::process::id())));
        std::fs::create_dir_all(&root.0).unwrap();
        let (temp, path) = (root.0.join("t.tmp"), root.0.join("New.kontra"));
        std::fs::write(&temp, b"new").unwrap();
        let unlinkable = |_: &Path, _: &Path| Err(io::Error::from(io::ErrorKind::Unsupported));
        created(&temp, &path, b"new", unlinkable).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let taken = created(&temp, &path, b"other", unlinkable);
        assert_eq!(
            taken.map_err(|e| e.kind()),
            Err(io::ErrorKind::AlreadyExists)
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }
}
