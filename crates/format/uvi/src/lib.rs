//! UVI programs (`.uvip`, stored in `.ufs` banks) read into the instrument
//! model. A program is addressed as `bank.ufs/Folder/Name.uvip`; `bank.ufs`
//! alone is the bank's first program. Scripts (UVIScript) run on the engine;
//! [`load_with`] also takes the samples they load as the program loads.

mod bank;
mod header;
mod keys;
mod program;

use std::path::{Path, PathBuf};

use kontra_ir as ir;

pub use program::Swap;

/// Why an instrument could not be read at all.
#[derive(Debug, thiserror::Error)]
#[error("{}: {what}: {reason}", path.display())]
pub struct Error {
    pub path: PathBuf,
    pub what: &'static str,
    pub reason: String,
}

impl Error {
    fn new(path: &Path, what: &'static str, reason: impl std::fmt::Display) -> Self {
        Self {
            path: path.to_path_buf(),
            what,
            reason: reason.to_string(),
        }
    }
}

/// Whether `path` addresses a program inside a UFS bank.
#[must_use]
pub fn is_uvi(path: &Path) -> bool {
    split(path).is_some()
}

/// The programs in the bank at `bank`, as paths inside it, sorted.
///
/// # Errors
/// When the bank cannot be opened.
pub fn programs(bank: &Path) -> Result<Vec<String>, Error> {
    bank::Bank::program_paths(bank).map_err(|e| Error::new(bank, "bank", format!("{e:#}")))
}

/// Reads the program at `path` (see the module docs for the addressing).
///
/// # Errors
/// When the bank cannot be opened or the program is not in it, or its XML is
/// not a program this can read; a sample that is missing or unreadable is a
/// diagnostic, not an error.
pub fn load(path: &Path) -> Result<ir::Instrument, Error> {
    load_with(path, &|_| Vec::new())
}

/// [`load`], with `swaps` running each script's load and returning the
/// samples it puts on oscillators.
///
/// # Errors
/// As [`load`].
pub fn load_with(
    path: &Path,
    swaps: &dyn Fn(&ir::Script) -> Vec<Swap>,
) -> Result<ir::Instrument, Error> {
    let (bank_path, member) =
        split(path).ok_or_else(|| Error::new(path, "address", "no .ufs bank in the path"))?;
    let bank =
        bank::Bank::open(&bank_path).map_err(|e| Error::new(path, "bank", format!("{e:#}")))?;
    let (in_bank, text) = bank
        .program_text(&member)
        .map_err(|e| Error::new(path, "program", format!("{e:#}")))?;
    let doc = uvi_file::parse_program_xml(&text)
        .map_err(|e| Error::new(path, "program XML", format!("{e:#}")))?;
    let root = doc.root_element();
    let root = if root.has_tag_name("Program") {
        root
    } else {
        root.children()
            .find(|n| n.has_tag_name("Program"))
            .ok_or_else(|| Error::new(path, "program XML", "no Program element"))?
    };
    let folder = in_bank.rsplit_once('/').map_or("", |(folder, _)| folder);
    program::translate(root, folder, &bank, swaps).map_err(|e| Error::new(path, "program", e))
}

/// The bank a path lies in, and the member inside it (empty: the bank itself).
fn split(path: &Path) -> Option<(PathBuf, String)> {
    let bank = path
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ufs")))?;
    let member = path
        .strip_prefix(bank)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    Some((bank.to_path_buf(), member))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_is_addressed_inside_its_bank() {
        let (bank, member) = split(Path::new("/lib/VWinds.ufs/Presets/Bb.uvip")).unwrap();
        assert_eq!(bank, Path::new("/lib/VWinds.ufs"));
        assert_eq!(member, "Presets/Bb.uvip");
        assert_eq!(split(Path::new("/lib/VWinds.ufs")).unwrap().1, "");
        assert!(!is_uvi(Path::new("/lib/Piano.nki")));
    }
}
