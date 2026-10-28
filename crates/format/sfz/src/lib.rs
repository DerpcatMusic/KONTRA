//! SFZ instruments (sfzformat.com) read into the instrument model
//! (06-FORMATS.md). Sample files are opened for their header only; what SFZ
//! says that the IR cannot carry is an [`ir::Diagnostic`].

mod laws;
mod parse;
mod translate;

use std::fmt::Display;
use std::path::{Path, PathBuf};

use kontra_ir as ir;

/// Why an instrument could not be read at all.
#[derive(Debug, thiserror::Error)]
#[error("{}: {what}: {reason}", path.display())]
pub struct Error {
    pub path: PathBuf,
    pub what: &'static str,
    pub reason: String,
}

impl Error {
    fn new(path: &Path, what: &'static str, reason: impl Display) -> Self {
        Self {
            path: path.to_path_buf(),
            what,
            reason: reason.to_string(),
        }
    }
}

/// Reads the SFZ file at `path`; the instrument is named after the file.
///
/// # Errors
/// When the file cannot be read. A sample that is missing or unreadable is a
/// diagnostic, not an error.
pub fn load(path: &Path) -> Result<ir::Instrument, Error> {
    let text = parse::read(path).map_err(|e| Error::new(path, "read", e))?;
    let base = path.parent().unwrap_or(Path::new("."));
    let mut instrument = parse(&text, base);
    instrument.name = path
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    Ok(instrument)
}

/// Reads SFZ `text`; sample paths resolve from `base_dir`.
pub fn parse(text: &str, base_dir: &Path) -> ir::Instrument {
    translate::instrument(&parse::document(text, base_dir), base_dir)
}
