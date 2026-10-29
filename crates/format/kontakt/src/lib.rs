//! Kontakt instruments (.nki, NKS/NIS, monoliths) read into the instrument
//! model (06-FORMATS.md). Sample files are located here and decoded by the
//! store; everything this cannot carry over is an [`ir::Diagnostic`].

mod access;
mod clocked;
mod container;
mod delays;
mod drive;
mod dynamics;
mod effects;
mod external;
mod fastlz;
mod impulse;
mod laws;
mod legacy_reverb;
mod resources;
mod reverb;
mod samples;
mod script_knobs;
mod skreamer;
mod translate;
mod versatile;
mod wavetable;

pub use container::chunks;
pub use resources::Resources;
pub use reverb::{reverb, reverb_laws, reverb_networks};
pub use script_knobs::script_knobs;

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

/// Reads the instrument at `path`.
///
/// # Errors
/// When the file is not a Kontakt instrument this can read; a sample that
/// is missing or unreadable is a diagnostic, not an error.
pub fn load(path: &Path) -> Result<ir::Instrument, Error> {
    let chunks = container::chunks(path)?;
    translate::instrument(path, &chunks)
}
