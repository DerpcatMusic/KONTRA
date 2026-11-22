//! SoundFont 2 (`.sf2`, versions 2.01 and 2.04) and SF3 (`.sf3`) banks read
//! into the instrument model (06-FORMATS.md). One preset becomes one
//! [`ir::Instrument`]; its samples point straight into the bank's `smpl`
//! chunk (mono 16-bit little-endian PCM, no header). What the IR cannot
//! carry is an [`ir::Diagnostic`].
//!
//! Not carried, and why:
//! - SF3 (Ogg Vorbis) and ROM samples: reported `Broken`, their zones dropped.
//! - `sm24`: the low byte of 24-bit samples; played from the 16-bit words
//!   (reported `Cosmetic`, it lies below -90 dB).
//! - Modulators stored in `pmod`/`imod` other than the SF2 defaults, and the
//!   generators listed in `gens::UNSUPPORTED`: reported `Audible`.
//! - The default CC91/CC93 (reverb/chorus send) modulators: an instrument
//!   carries no send effects of its own.

mod gens;
mod laws;
mod parser;
mod translate;

#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use kontra_ir as ir;

/// Why a bank or preset could not be read at all.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("not a SoundFont: {0}")]
    NotSoundFont(&'static str),
    /// A chunk, record or index that breaks the file's own bounds.
    #[error("malformed {chunk}: {detail}")]
    Malformed { chunk: &'static str, detail: String },
    #[error("no preset at bank {bank}, program {program}")]
    NoPreset { bank: u16, program: u16 },
    /// The selected preset exceeds the importer's cumulative expansion budget.
    #[error("preset expansion exceeds the limit of {limit} {unit}")]
    ExpansionLimit { limit: usize, unit: &'static str },
    #[error("the bank holds no presets")]
    NoPresets,
}

/// A preset of a bank: what a player lists and selects by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetInfo {
    pub bank: u16,
    pub program: u16,
    pub name: String,
}

/// The presets of the bank at `path`, by bank then program.
///
/// # Errors
/// When the file cannot be read or is not a well-formed SoundFont.
pub fn list_presets(path: &Path) -> Result<Vec<PresetInfo>, Error> {
    Ok(presets(&open(path)?))
}

/// Preset `program` of bank `bank` in the file at `path`.
///
/// # Errors
/// When the file cannot be read, is malformed, holds no such preset, or the
/// preset exceeds the importer's expansion budget. A sample that cannot play
/// is a diagnostic, not an error.
pub fn read_preset(path: &Path, bank: u16, program: u16) -> Result<ir::Instrument, Error> {
    let sf = open(path)?;
    translate::preset(&sf, find(&sf, bank, program)?, path)
}

/// The first preset (lowest bank, then program) of the file at `path`.
///
/// # Errors
/// As [`read_preset`]; [`Error::NoPresets`] for a bank without presets.
pub fn read_first(path: &Path) -> Result<ir::Instrument, Error> {
    let sf = open(path)?;
    let first = presets(&sf).into_iter().next().ok_or(Error::NoPresets)?;
    translate::preset(&sf, find(&sf, first.bank, first.program)?, path)
}

/// [`read_preset`] on a bank already in memory; samples point into
/// `file_path`, which the bytes are taken to be.
///
/// # Errors
/// As [`read_preset`].
pub fn translate_bytes(
    bytes: &[u8],
    file_path: &Path,
    bank: u16,
    program: u16,
) -> Result<ir::Instrument, Error> {
    let sf = parser::parse(&mut Cursor::new(bytes), file_path)?;
    translate::preset(&sf, find(&sf, bank, program)?, file_path)
}

/// Reads only the chunks the presets need; the sample data stays on disk.
fn open(path: &Path) -> Result<parser::SoundFont, Error> {
    let mut file = File::open(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parser::parse(&mut file, path)
}

/// The presets before the terminal record, by bank then program.
fn presets(sf: &parser::SoundFont) -> Vec<PresetInfo> {
    let mut list: Vec<PresetInfo> = (sf.presets.iter())
        .take(sf.presets.len().saturating_sub(1))
        .map(|p| PresetInfo {
            bank: p.bank,
            program: p.program,
            name: p.name.clone(),
        })
        .collect();
    list.sort_by_key(|p| (p.bank, p.program));
    list
}

/// The index of the first preset header at `(bank, program)`.
fn find(sf: &parser::SoundFont, bank: u16, program: u16) -> Result<usize, Error> {
    if sf.presets.len() < 2 {
        return Err(Error::NoPresets);
    }
    (sf.presets.iter())
        .take(sf.presets.len() - 1)
        .position(|p| p.bank == bank && p.program == program)
        .ok_or(Error::NoPreset { bank, program })
}
