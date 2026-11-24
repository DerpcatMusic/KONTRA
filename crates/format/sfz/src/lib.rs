//! SFZ instruments (sfzformat.com) read into the instrument model
//! (06-FORMATS.md). Sample files are opened for their header only; what SFZ
//! says that the IR cannot carry is an [`ir::Diagnostic`].

mod laws;
mod parse;
mod translate;

use std::fmt::Display;
use std::path::{Path, PathBuf};

use kontra_format::{
    Confidence, EntryRef, Format, FormatError, FormatId, LoadCx, ProtectionStatus, ScanEntry,
    ScanSink, scan_dir,
};
use kontra_ir as ir;

/// SFZ format implementation for [`kontra_format::Registry`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Sfz;

impl Sfz {
    pub const ID: FormatId = FormatId::Sfz;
}

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
    load_with_error(path, |err| Error::new(path, "read", err))
}

fn load_with_error<E>(
    path: &Path,
    map_io: impl FnOnce(std::io::Error) -> E,
) -> Result<ir::Instrument, E> {
    let text = parse::read(path).map_err(map_io)?;
    let base = path.parent().unwrap_or(Path::new("."));
    let mut instrument = parse(&text, base);
    instrument.name = path
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    instrument.source_path = Some(path.to_path_buf());
    Ok(instrument)
}

/// Reads SFZ `text`; sample paths resolve from `base_dir`.
pub fn parse(text: &str, base_dir: &Path) -> ir::Instrument {
    translate::instrument(&parse::document(text, base_dir), base_dir)
}

fn is_sfz_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("sfz"))
}

fn has_sfz_header(head: &[u8], with_ext: bool) -> bool {
    let lower = kontra_format::head_text(head).to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    // Without the extension, the first token past `//` comments must be SFZ
    // structure, so a tag quoted in another format's text is not a header.
    let mut start = lower.trim_start();
    while let Some(comment) = start.strip_prefix("//") {
        start = comment
            .split_once('\n')
            .map_or("", |(_, rest)| rest)
            .trim_start();
    }
    let structural = with_ext
        || ["<", "#include", "#define"]
            .iter()
            .any(|t| start.starts_with(t));
    let strong = ["<region>", "<control>", "<global>", "<master>"]
        .iter()
        .any(|tag| lower.contains(tag));
    if structural && strong {
        return true;
    }
    with_ext
        && ["<group>", "sample=", "#include", "#define", "//"]
            .iter()
            .any(|tok| lower.contains(tok))
}

impl Format for Sfz {
    fn id(&self) -> FormatId {
        Self::ID
    }

    fn sniff(&self, path: &Path, head: &[u8]) -> Confidence {
        let ext = is_sfz_extension(path);
        let header = has_sfz_header(head, ext);
        match (ext, header) {
            (true, true) => Confidence::Exact,
            (false, true) => Confidence::Header,
            (true, false) => Confidence::Extension,
            (false, false) => Confidence::None,
        }
    }

    fn scan(&self, root: &Path, out: &mut dyn ScanSink) -> Result<(), FormatError> {
        scan_dir(root, out, |path, sink| {
            if is_sfz_extension(path) {
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
        let inst = load_with_error(&path, |err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                FormatError::not_found(&path, err.to_string())
            } else {
                FormatError::io(&path, err)
            }
        })?;
        cx.diag.record_diagnostics(&inst.diagnostics);
        Ok(inst)
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;
    use kontra_format::{LoadOptions, Notes, Registry, ScanCollector};

    #[test]
    fn sfz_format_sniffs_scans_and_loads_via_registry() {
        let mut reg = Registry::new();
        reg.register(Sfz);

        let (fmt, conf) = reg
            .sniff(Path::new("Patch.sfz"), b"<region> sample=a.wav")
            .unwrap();
        assert_eq!((fmt.id(), conf), (FormatId::Sfz, Confidence::Exact));
        let (fmt, conf) = reg
            .sniff(Path::new("Patch.txt"), b"<region> sample=a.wav")
            .unwrap();
        assert_eq!((fmt.id(), conf), (FormatId::Sfz, Confidence::Header));
        let (fmt, conf) = reg.sniff(Path::new("Patch.sfz"), b"").unwrap();
        assert_eq!((fmt.id(), conf), (FormatId::Sfz, Confidence::Extension));
        assert!(reg.sniff(Path::new("Other.txt"), b"hello").is_none());
        // A quoted tag in another format's text is not a header.
        assert!(
            reg.sniff(Path::new("p.kontra"), b"name = \"<region>\"\nversion = 1")
                .is_none()
        );
        let (_, conf) = reg
            .sniff(
                Path::new("Patch.txt"),
                b"// piano\n<control>\n<region> sample=a.wav",
            )
            .unwrap();
        assert_eq!(conf, Confidence::Header);
        // A read that cuts a character short keeps the text before it.
        let mut cut = b"<region> sample=a.wav".to_vec();
        cut.extend_from_slice(&"\u{e9}".as_bytes()[..1]);
        let (_, conf) = reg.sniff(Path::new("Patch.txt"), &cut).unwrap();
        assert_eq!(conf, Confidence::Header);

        let root = std::env::temp_dir().join(format!("kontra-sfz-fmt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let mut collector = ScanCollector::default();
        let err = reg
            .scan(&root.join("nonexistent"), &mut collector)
            .unwrap_err();
        assert!(matches!(err, FormatError::NotFound { .. }));

        std::fs::create_dir_all(root.join("Samples")).unwrap();
        let patch = root.join("Lead.sfz");
        std::fs::write(&patch, "<region> sample=missing.wav key=60\n").unwrap();
        std::fs::write(root.join("Samples/ignored.sfz"), "<region> sample=a.wav\n").unwrap();

        reg.scan(&root, &mut collector).unwrap();
        assert_eq!(collector.entries.len(), 1);
        assert_eq!(collector.entries[0].path, patch);
        assert_eq!(collector.entries[0].format, FormatId::Sfz);

        let mut notes = Notes::default();
        let opts = LoadOptions::default();
        let mut cx = LoadCx::new(&mut notes, &opts);
        let inst = reg.load(&patch, &mut cx).unwrap();
        assert_eq!(inst.name, "Lead");
        let diags = notes.finish();
        assert!(
            diags.iter().any(|d| d.feature == "samples not found"),
            "missing sample diagnostic should be forwarded to cx.diag: {diags:?}"
        );

        let mut notes = Notes::default();
        let mut cx = LoadCx::new(&mut notes, &opts);
        let missing_err = reg.load(&root.join("missing.sfz"), &mut cx).unwrap_err();
        assert!(matches!(missing_err, FormatError::NotFound { .. }));

        std::fs::remove_dir_all(&root).unwrap();
    }
}
