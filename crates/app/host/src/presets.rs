//! The player's own presets: what a project saves of a part's scripts, kept
//! as `<folder>/<library>/<name>.kontra` under KONTRA's own presets folder,
//! never in a library. Loading one is loading its base instrument with
//! these values (a recall when the part already plays that base).
//!
//! Writes go to a temporary file renamed into place; rename and delete
//! touch only `.kontra` files inside the folder; no path read from a preset
//! file is ever written to.

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

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, what)
}

/// Writes `preset` as `name` of `library` (replacing one of that name: the
/// caller asked the player first); its path.
///
/// # Errors
/// When a name is empty once sanitised, or the file cannot be written.
pub fn save(folder: &Path, library: &str, name: &str, preset: &UserPreset) -> io::Result<PathBuf> {
    let dir = folder.join(sanitized(library).ok_or_else(|| invalid("library name"))?);
    let name = sanitized(name).ok_or_else(|| invalid("preset name"))?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.{EXTENSION}"));
    let bytes = serde_json::to_vec_pretty(preset).map_err(io::Error::other)?;
    let temp = dir.join(format!(".{name}.{EXTENSION}.tmp"));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, &path)?;
    Ok(path)
}

/// The user preset at `path`; None for any other file.
#[must_use]
pub fn read(path: &Path) -> Option<UserPreset> {
    let ours = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION));
    let meta = std::fs::metadata(path)
        .ok()
        .filter(|m| ours && m.is_file())?;
    if meta.len() > LIMIT {
        return None;
    }
    let preset: UserPreset = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    // Its base is an instrument, never another preset.
    let base_ours = preset
        .base
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION));
    (preset.version == VERSION && !base_ours).then_some(preset)
}

/// `path` itself when it is a preset file inside `folder`.
fn inside(folder: &Path, path: &Path) -> io::Result<PathBuf> {
    let (folder, path) = (folder.canonicalize()?, path.canonicalize()?);
    let ours = path.extension().is_some_and(|e| e == EXTENSION);
    match path.starts_with(&folder) && ours {
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
    let path = inside(folder, path)?;
    let name = sanitized(name).ok_or_else(|| invalid("preset name"))?;
    let to = path.with_file_name(format!("{name}.{EXTENSION}"));
    if to.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, name));
    }
    std::fs::rename(&path, &to)?;
    Ok(to)
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
    let Some(dir) = sanitized(library).map(|l| folder.join(l)) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = (std::fs::read_dir(dir).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == EXTENSION))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preset_saves_renames_and_deletes_only_inside_its_folder() {
        let root = std::env::temp_dir().join(format!("kontra-presets-{}", std::process::id()));
        let folder = root.join("presets");
        let base = Path::new("/lib/Piano.nki");
        let scripts = vec![vec!["1".to_owned()]];
        let preset = UserPreset {
            version: VERSION,
            base: base.to_owned(),
            scripts: scripts.clone(),
            slot_types: Vec::new(),
            slot_responses: Vec::new(),
        };
        let path = save(&folder, "Una/Corda", "../My: Soft*", &preset).unwrap();
        assert_eq!(path, folder.join("UnaCorda").join("My Soft.kontra"));
        assert_eq!(read(&path).map(|p| p.scripts), Some(scripts.clone()));
        assert_eq!(list(&folder, "Una/Corda"), std::slice::from_ref(&path));
        assert!(sanitized(" .. ").is_none());
        // The editor's Replace check names collisions by this file name.
        assert_eq!(sanitized("Soft?"), sanitized("Soft"));
        // The name is taken: no rename over it.
        let other = save(&folder, "UnaCorda", "Bright", &preset).unwrap();
        assert!(rename(&folder, &path, "Bright").is_err());
        let path = rename(&folder, &path, "Soft").unwrap();
        // Nothing outside the folder, nor any other file, is removed.
        let outside = root.join("x.kontra");
        std::fs::write(&outside, b"{}").unwrap();
        assert!(delete(&folder, &outside).is_err());
        assert!(delete(&folder, &path).is_ok() && delete(&folder, &other).is_ok());
        assert!(list(&folder, "UnaCorda").is_empty() && outside.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
