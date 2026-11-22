//! DecentSampler (`.dspreset`) XML instruments translated into KONTRA's
//! shared [`kontra_ir::Instrument`] model using the `kontra-format` SDK.
//!
//! Sample files are probed for headers only; any unsupported element, effect,
//! or binding is reported via [`kontra_format::Notes`] with zero silent drops.

mod effects;
mod modulators;
mod translate;

use std::io;
use std::path::{Path, PathBuf};

use kontra_format::header::{AudioHeader, probe_header};
use kontra_ir as ir;

/// Why a DecentSampler preset could not be read or parsed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("parse xml: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("missing <DecentSampler> root element")]
    MissingRoot,
}

/// Reads and translates a `.dspreset` file from disk.
///
/// Missing or unreadable sample files are reported as [`ir::Diagnostic`]s
/// rather than failing the preset load.
///
/// # Errors
/// Returns [`Error::Io`] if the file cannot be read, [`Error::Xml`] if the XML
/// is malformed, or [`Error::MissingRoot`] if `<DecentSampler>` is absent.
pub fn read_file(path: &Path) -> Result<ir::Instrument, Error> {
    let bytes = std::fs::read(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let stripped = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    let xml = String::from_utf8_lossy(stripped);
    let base_dir = path.parent().unwrap_or(Path::new("."));
    let mut instrument = translate_xml(&xml, base_dir, &probe_header)?;
    instrument.name = path
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    instrument.source_path = Some(path.to_path_buf());
    Ok(instrument)
}

/// Translates `.dspreset` XML text into an [`ir::Instrument`], resolving sample
/// paths relative to `base_dir` and probing headers with `probe`.
///
/// # Errors
/// Returns [`Error::Xml`] on malformed XML or [`Error::MissingRoot`] if the
/// document root is not `<DecentSampler>`.
pub fn translate_xml(
    xml: &str,
    base_dir: &Path,
    probe: &dyn Fn(&Path) -> io::Result<AudioHeader>,
) -> Result<ir::Instrument, Error> {
    let doc = roxmltree::Document::parse(xml)?;
    let root = doc.root_element();
    if !root.has_tag_name("DecentSampler") {
        return Err(Error::MissingRoot);
    }
    Ok(translate::translate(root, base_dir, probe))
}
