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
use kontra_format::{
    Confidence, EntryRef, Format, FormatError, FormatId, LoadCx, ProtectionStatus, ScanEntry,
    ScanSink, scan_dir,
};
use kontra_ir as ir;

/// DecentSampler format implementation for [`kontra_format::Registry`].
#[derive(Clone, Copy, Debug, Default)]
pub struct DecentSampler;

impl DecentSampler {
    pub const ID: FormatId = FormatId::DecentSampler;
}

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

fn is_dspreset_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dspreset"))
}

fn to_format_error(path: &Path, err: Error) -> FormatError {
    match err {
        Error::Io { path, source } => {
            if source.kind() == io::ErrorKind::NotFound {
                FormatError::not_found(path, source.to_string())
            } else {
                FormatError::io(path, source)
            }
        }
        Error::Xml(e) => FormatError::malformed(path, "xml", e.to_string()),
        Error::MissingRoot => {
            FormatError::malformed(path, "xml", "missing <DecentSampler> root element")
        }
    }
}

impl Format for DecentSampler {
    fn id(&self) -> FormatId {
        Self::ID
    }

    fn sniff(&self, path: &Path, head: &[u8]) -> Confidence {
        let ext = is_dspreset_extension(path);
        let text = kontra_format::head_text(head).trim_start();
        let has_tag = text.contains("<DecentSampler");
        let is_xml = text.starts_with("<?xml");
        match (ext, has_tag, is_xml) {
            (true, true, _) | (true, _, true) => Confidence::Exact,
            (false, true, _) => Confidence::Header,
            (true, false, false) => Confidence::Extension,
            (false, false, _) => Confidence::None,
        }
    }

    fn scan(&self, root: &Path, out: &mut dyn ScanSink) -> Result<(), FormatError> {
        scan_dir(root, out, |path, sink| {
            if is_dspreset_extension(path) {
                let name = path
                    .file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                sink.entry(ScanEntry {
                    format: Self::ID,
                    path: path.to_path_buf(),
                    name,
                    protection: ProtectionStatus::Open,
                });
            }
        })
    }

    fn load(&self, entry: &EntryRef, cx: &mut LoadCx<'_>) -> Result<ir::Instrument, FormatError> {
        let path = entry.full_path();
        let inst = read_file(&path).map_err(|err| to_format_error(&path, err))?;
        cx.diag.record_diagnostics(&inst.diagnostics);
        Ok(inst)
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;
    use kontra_format::{LoadOptions, Notes, Registry, ScanCollector};

    #[test]
    fn decentsampler_format_sniffs_scans_and_loads_via_registry() {
        let mut reg = Registry::new();
        reg.register(DecentSampler);

        let (fmt, conf) = reg
            .sniff(Path::new("Felt.dspreset"), b"<DecentSampler><groups/>")
            .unwrap();
        assert_eq!(
            (fmt.id(), conf),
            (FormatId::DecentSampler, Confidence::Exact)
        );
        let (fmt, conf) = reg
            .sniff(Path::new("Felt.xml"), b"<DecentSampler><groups/>")
            .unwrap();
        assert_eq!(
            (fmt.id(), conf),
            (FormatId::DecentSampler, Confidence::Header)
        );
        let (fmt, conf) = reg.sniff(Path::new("Felt.dspreset"), b"").unwrap();
        assert_eq!(
            (fmt.id(), conf),
            (FormatId::DecentSampler, Confidence::Extension)
        );
        assert!(reg.sniff(Path::new("Other.xml"), b"<Other/>").is_none());
        // The 64-byte sniff read can end inside a character: keep the tag.
        let mut cut = b"<DecentSampler>".to_vec();
        cut.resize(63, b' ');
        cut.push("\u{e9}".as_bytes()[0]);
        let (fmt, conf) = reg.sniff(Path::new("Felt.xml"), &cut).unwrap();
        assert_eq!(
            (fmt.id(), conf),
            (FormatId::DecentSampler, Confidence::Header)
        );

        let root = std::env::temp_dir().join(format!("kontra-ds-fmt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let mut collector = ScanCollector::default();
        let err = reg.scan(&root.join("missing"), &mut collector).unwrap_err();
        assert!(matches!(err, FormatError::NotFound { .. }));

        std::fs::create_dir_all(&root).unwrap();
        let preset = root.join("Felt.dspreset");
        std::fs::write(
            &preset,
            r#"<DecentSampler><groups><group><sample path="missing.wav" rootNote="60"/></group></groups></DecentSampler>"#,
        )
        .unwrap();

        reg.scan(&root, &mut collector).unwrap();
        assert_eq!(collector.entries.len(), 1);
        assert_eq!(collector.entries[0].path, preset);
        assert_eq!(collector.entries[0].format, FormatId::DecentSampler);

        let mut notes = Notes::default();
        let opts = LoadOptions::default();
        let mut cx = LoadCx::new(&mut notes, &opts);
        let inst = reg.load(&preset, &mut cx).unwrap();
        assert_eq!(inst.name, "Felt");
        let diags = notes.finish();
        assert!(
            diags.iter().any(|d| d.feature == "samples not found"),
            "missing sample diagnostic should be forwarded to cx.diag: {diags:?}"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }
}
