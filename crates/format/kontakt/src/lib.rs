//! Kontakt instruments (.nki, NKS/NIS, monoliths) read into the instrument
//! model (06-FORMATS.md). Sample files are located here and decoded by the
//! store; everything this cannot carry over is an [`ir::Diagnostic`].

mod access;
mod choral;
mod clocked;
mod container;
mod delays;
mod drive;
mod dynamics;
mod effects;
mod engine_pars;
mod external;
mod fastlz;
mod flair;
mod impulse;
mod laws;
mod legacy_reverb;
mod links;
mod phasis;
mod provenance;
mod readouts;
mod replika;
mod resources;
mod reverb;
mod samples;
mod script_knobs;
mod skreamer;
mod snapshot;
mod tape;
pub use snapshot::{Preset, preset};
mod solid_knobs;
mod timed;
mod translate;
mod versatile;
mod wavetable;

pub use container::chunks;
pub use engine_pars::{engine_pars, lfo_units, owner};
/// No resource a script loads, or array file it names, is larger (bytes).
pub use resources::LIMIT as RESOURCE_LIMIT;
pub use resources::Resources;
pub use reverb::{reverb, reverb_laws, reverb_networks};

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

/// Reads the instrument at `path`; a snapshot (.nksn) is its base
/// instrument with the snapshot's saved script values.
///
/// # Errors
/// When the file is not a Kontakt instrument this can read; a sample that
/// is missing or unreadable is a diagnostic, not an error.
pub fn load(path: &Path) -> Result<ir::Instrument, Error> {
    load_known(path, &[])
}

/// [`load`], a snapshot's base found among `known` (the instruments the
/// player has) when none is near it ([`preset`]).
///
/// # Errors
/// As [`load`].
pub fn load_known(path: &Path, known: &[std::path::PathBuf]) -> Result<ir::Instrument, Error> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nksn"))
    {
        return snapshot::load(path, known);
    }
    load_timed(path).map(|(instrument, _, _)| instrument)
}

/// Reads an instrument and returns the time spent reading and translating it.
///
/// # Errors
/// As [`load`]. Snapshot timing is not separated by this function.
pub fn load_timed(
    path: &Path,
) -> Result<(ir::Instrument, std::time::Duration, std::time::Duration), Error> {
    let start = std::time::Instant::now();
    let (chunks, monolith, encrypted) = container::opened(path)?;
    let read = start.elapsed();
    let start = std::time::Instant::now();
    let instrument = translate::instrument(path, &chunks, monolith, &[], encrypted, None)?;
    Ok((instrument, read, start.elapsed()))
}
