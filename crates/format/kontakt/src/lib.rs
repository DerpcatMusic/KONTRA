//! Kontakt instruments (.nki, NKS/NIS, monoliths) read into the instrument
//! model (06-FORMATS.md). Sample files are located here and decoded by the
//! store; everything this cannot carry over is an [`ir::Diagnostic`].

use kontra_kontakt_effects::{
    choral, delays, dynamics, flair, legacy_reverb, phasis, skreamer, tape,
};

mod access;
mod clocked;
mod container;
mod drive;
mod effects;
mod engine_pars;
mod external;
mod ignored;
mod impulse;
mod laws;
mod links;
mod multi;
pub use multi::{Slot, is_multi, scripts as multi_scripts, slots as multi};
mod provenance;
mod raum;
mod readouts;
mod replika;
mod resources;
mod reverb;
mod samples;
mod script_knobs;
mod snapshot;
pub use snapshot::{Preset, preset};
mod solid_knobs;
mod timed;
mod translate;
mod versatile;
mod wavetable;
mod wowflutter;

pub use container::chunks;
pub use engine_pars::{engine_pars, lfo_units, owner};
/// No resource a script loads, or array file it names, is larger (bytes).
pub use resources::LIMIT as RESOURCE_LIMIT;
pub use resources::Resources;
pub use reverb::{reverb, reverb_laws, reverb_networks};
pub use wavetable::{mod_kinds, warp_modes};

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

/// A preset file's chunk stream, decompressed and decrypted, and whether it
/// was encrypted: what [`load_chunks`] takes back, so a test can damage the
/// stream inside its compression.
///
/// # Errors
/// As [`load`].
pub fn decode(path: &Path) -> Result<(Vec<u8>, Option<bool>), Error> {
    container::decoded(path).map(|(bytes, _, encrypted)| (bytes, encrypted))
}

/// The instrument of a chunk stream [`decode`] gave (damaged or not), its
/// samples resolving beside `path`.
///
/// # Errors
/// As [`load`].
pub fn load_chunks(
    path: &Path,
    bytes: Vec<u8>,
    encrypted: Option<bool>,
) -> Result<ir::Instrument, Error> {
    container::from_bytes(path, bytes, encrypted)
}

/// As [`load`], resolving a snapshot's base against `known` when needed.
pub fn load_known(path: &Path, known: &[std::path::PathBuf]) -> Result<ir::Instrument, Error> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nksn"))
    {
        return snapshot::load(path, known);
    }
    let (chunks, monolith, encrypted) =
        kontra_format::timing::measure(kontra_format::timing::Stage::Read, || {
            container::opened(path)
        })?;
    kontra_format::timing::measure(kontra_format::timing::Stage::Translate, || {
        translate::instrument(path, &chunks, monolith, &[], encrypted, None)
    })
}
