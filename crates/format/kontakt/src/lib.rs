//! Kontakt instruments (.nki, NKS/NIS, monoliths) read into the instrument
//! model (06-FORMATS.md). Sample files are located here and decoded by the
//! store; everything this cannot carry over is an [`ir::Diagnostic`].

use kontra_kontakt_effects::amps::{ac_box, bass_pro, hot_solo, jump, twang, van51};
use kontra_kontakt_effects::{
    amps, bite, cabinet, cat, choral, core_cell, cry_wah, delays, dstortion, dynamics, dyx, flair,
    freak, kolor, legacy_reverb, mangling, phasis, plate_reverb, psyche, ringmod, rotator,
    skreamer, skreamer_deluxe, stereo_tune, supergt, tape, twin_delay, vibrato,
};

mod access;
#[cfg(test)]
mod audit;
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
pub use multi::{
    Multi, Slot, is_multi, load as multi_load, scripts as multi_scripts, slots as multi,
};
#[cfg(test)]
mod fit;
mod held;
mod provenance;
mod raum;
mod readouts;
mod replika;
mod resources;
mod reverb;
mod samples;
mod script_knobs;
pub mod session;
mod snapshot;
pub use snapshot::{
    Base as SnapshotBase, Preset, load_onto as load_snapshot_onto, open_base as open_snapshot_base,
    preset, save as save_snapshot,
};
mod solid_knobs;
mod source_pars;
mod timed;
mod translate;
mod versatile;
mod wavetable;
mod wowflutter;

pub use container::chunks;
pub use engine_pars::{accepts, engine_pars, held, lfo_units, owner};
/// No resource a script loads, or array file it names, is larger (bytes).
pub use resources::LIMIT as RESOURCE_LIMIT;
pub use resources::Resources;
pub use reverb::{reverb, reverb_laws, reverb_networks};
pub use script_knobs::slot_types;
pub use wavetable::{mod_kinds, warp_modes};

use std::collections::BTreeSet;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use kontra_format::{
    Confidence, EntryRef, Format, FormatError, FormatId, LoadCx, ProtectionStatus, ScanEntry,
    ScanSink,
};
use kontra_ir as ir;

/// Kontakt format implementation for [`kontra_format::Registry`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Kontakt;

impl Kontakt {
    pub const ID: FormatId = FormatId::Kontakt;
}

/// Why an instrument could not be read at all.
#[derive(Debug, thiserror::Error)]
#[error("{}: {what}: {reason}", path.display())]
pub struct Error {
    pub path: PathBuf,
    pub what: &'static str,
    pub reason: String,
    pub protection: ProtectionStatus,
}

impl Error {
    fn new(path: &Path, what: &'static str, reason: impl Display) -> Self {
        Self {
            path: path.to_path_buf(),
            what,
            reason: reason.to_string(),
            protection: ProtectionStatus::Open,
        }
    }

    fn protected(path: &Path, what: &'static str, status: ProtectionStatus) -> Self {
        let reason = status.to_string();
        Self {
            path: path.to_path_buf(),
            what,
            reason,
            protection: status,
        }
    }

    /// Returns the structured [`ProtectionStatus`] when this failure was caused
    /// by DRM / missing Native Access library keys.
    #[must_use]
    pub const fn protection(&self) -> Option<&ProtectionStatus> {
        if self.protection.is_protected() {
            Some(&self.protection)
        } else {
            None
        }
    }
}

impl From<Error> for FormatError {
    fn from(err: Error) -> Self {
        if err.protection.is_protected() {
            return Self::protected(err.path, err.protection);
        }
        match err.what {
            "open" => Self::io(err.path, std::io::Error::other(err.reason)),
            "base instrument" => Self::not_found(err.path, err.reason),
            _ if err.reason.contains("legacy XML") => Self::unsupported(err.path, err.reason),
            _ => Self::malformed(err.path, err.what, err.reason),
        }
    }
}

/// Inspects whether the Kontakt preset at `path` is open or requires unavailable
/// Native Access encryption keys, without running a full translation.
///
/// # Errors
/// When `path` cannot be opened or is not a recognized Kontakt container.
pub fn inspect_protection(path: &Path) -> Result<ProtectionStatus, Error> {
    container::inspect_protection(path)
}

/// Whether the preset at `path` is encrypted, from its container alone: no
/// access data is looked up and nothing is decrypted.
///
/// # Errors
/// When `path` cannot be opened or is not a recognized Kontakt container.
pub fn encrypted(path: &Path) -> Result<bool, Error> {
    container::encrypted(path)
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

fn is_kontakt_extension(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        ["nki", "nkm", "nksn", "nkb"]
            .iter()
            .any(|ext| e.eq_ignore_ascii_case(ext))
    })
}

fn has_kontakt_magic(head: &[u8]) -> bool {
    if head.get(12..16) == Some(b"hsin") {
        return true;
    }
    let Some(magic_bytes) = head.get(..4).and_then(|s| <[u8; 4]>::try_from(s).ok()) else {
        return false;
    };
    let magic = u32::from_le_bytes(magic_bytes);
    matches!(
        magic,
        0x5EE5_6EB3
            | 0xB36E_E55E
            | 0x1290_A87F
            | 0x7FA8_9012
            | 0x5343_8710
            | 0x1087_4353
            | 0x5AE5_D6A4
            | 0xA4D6_E55A
            | 0x2F5C_204E
            | 0x4E20_5C2F
    )
}

impl Format for Kontakt {
    fn id(&self) -> FormatId {
        Self::ID
    }

    fn sniff(&self, path: &Path, head: &[u8]) -> Confidence {
        let ext = is_kontakt_extension(path);
        let magic = has_kontakt_magic(head);
        match (ext, magic) {
            (true, true) => Confidence::Exact,
            (false, true) => Confidence::Header,
            (true, false) => Confidence::Extension,
            (false, false) => Confidence::None,
        }
    }

    fn protection(&self, path: &Path) -> Result<ProtectionStatus, FormatError> {
        inspect_protection(path).map_err(FormatError::from)
    }

    fn scan(&self, root: &Path, out: &mut dyn ScanSink) -> Result<(), FormatError> {
        if root.is_file() {
            if is_kontakt_extension(root) {
                emit_scan_file(root, out);
            }
            return Ok(());
        }
        let mut visited = BTreeSet::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let key = dir.canonicalize().unwrap_or_else(|_| dir.clone());
            if !visited.insert(key) {
                continue;
            }
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) => {
                    out.issue(&dir, e.to_string(), ProtectionStatus::Open);
                    continue;
                }
            };
            let mut paths = Vec::new();
            for entry in entries {
                match entry {
                    Ok(entry) => paths.push(entry.path()),
                    Err(e) => out.issue(&dir, e.to_string(), ProtectionStatus::Open),
                }
            }
            paths.sort();
            for path in paths {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                if name.starts_with('.') {
                    continue;
                }
                let meta = match std::fs::metadata(&path) {
                    Ok(meta) => meta,
                    Err(e) => {
                        out.issue(&path, e.to_string(), ProtectionStatus::Open);
                        continue;
                    }
                };
                if meta.is_dir() {
                    if !matches!(
                        name.as_str(),
                        "samples" | "documentation" | "documents" | "docs" | "resources"
                    ) {
                        stack.push(path);
                    }
                } else if meta.is_file() && is_kontakt_extension(&path) {
                    emit_scan_file(&path, out);
                }
            }
        }
        Ok(())
    }

    fn load(&self, entry: &EntryRef, cx: &mut LoadCx<'_>) -> Result<ir::Instrument, FormatError> {
        let path = entry.full_path();
        load_known(&path, &cx.options.known_instruments).map_err(FormatError::from)
    }
}

fn emit_scan_file(path: &Path, out: &mut dyn ScanSink) {
    let name = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    // Follow file symlinks so `.nicnt` lookup walks the real library's ancestors.
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    match inspect_protection(&resolved) {
        Ok(protection) => out.entry(ScanEntry {
            format: FormatId::Kontakt,
            path: path.to_path_buf(),
            name,
            protection,
        }),
        Err(err) => out.issue(path, err.to_string(), err.protection),
    }
}
