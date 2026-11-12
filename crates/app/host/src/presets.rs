//! The player's presets as the host saves them: what was typed, cleaned
//! into a file name the editor's Replace check shares, then
//! [`kontra_library::presets`].

use std::io;
use std::path::{Path, PathBuf};

pub use kontra_library::presets::{
    EXTENSION, Overwrite, SavedRack, SlotResponse, SlotType, UserPreset, VERSION, delete, folder,
    list, ours, read,
};
pub use kontra_ui_model::preset_file_name as sanitized;

fn cleaned(name: &str) -> io::Result<String> {
    sanitized(name).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "preset name"))
}

/// [`kontra_library::presets::save`] under the cleaned `name`.
///
/// # Errors
/// As it does, and when nothing of `name` is left once cleaned.
pub fn save(
    folder: &Path,
    library: &str,
    name: &str,
    preset: &UserPreset,
    overwrite: Overwrite,
) -> io::Result<PathBuf> {
    kontra_library::presets::save(folder, library, &cleaned(name)?, preset, overwrite)
}

/// [`kontra_library::presets::rename`] to the cleaned `name`.
///
/// # Errors
/// As it does, and when nothing of `name` is left once cleaned.
pub fn rename(folder: &Path, path: &Path, name: &str) -> io::Result<PathBuf> {
    kontra_library::presets::rename(folder, path, &cleaned(name)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_name_is_cleaned_into_the_file_name() {
        assert!(cleaned(" .. ").is_err());
        // The editor's Replace check names collisions by this file name.
        assert_eq!(sanitized("Soft?"), sanitized("Soft"));
        assert_eq!(cleaned("../My: Soft*").ok().as_deref(), Some("My Soft"));
    }
}
