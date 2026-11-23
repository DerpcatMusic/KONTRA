//! Unified sampler format contract, protection diagnostics, and format registry
//! (`06-FORMATS.md`).
//!
//! Every format translator implements [`Format`] and registers with [`Registry`]
//! so hosts, scanners, and CLI tools can sniff, inspect protection status, scan
//! folders/containers, and load [`ir::Instrument`] values uniformly.

use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use kontra_ir as ir;
use serde::{Deserialize, Serialize};

use crate::Notes;

/// Bytes read from the start of a file when sniffing by header magic.
const SNIFF_HEAD_BYTES: usize = 64;

/// Stable identifier for a sampler instrument format.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FormatId {
    Kontakt,
    Uvi,
    Sfz,
    Sf2,
    DecentSampler,
    Native,
}

impl FormatId {
    /// Short lowercase identifier suitable for logs, CLI flags, and caches.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Kontakt => "kontakt",
            Self::Uvi => "uvi",
            Self::Sfz => "sfz",
            Self::Sf2 => "sf2",
            Self::DecentSampler => "decentsampler",
            Self::Native => "native",
        }
    }

    /// Human-readable display label for browser tabs and diagnostics.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Kontakt => "Kontakt",
            Self::Uvi => "UVI",
            Self::Sfz => "SFZ",
            Self::Sf2 => "SoundFont 2",
            Self::DecentSampler => "Decent Sampler",
            Self::Native => "KONTRA Native",
        }
    }
}

impl fmt::Display for FormatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// How strongly a [`Format`] recognizes a given path and header prefix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    /// This format does not recognize the input.
    #[default]
    None,
    /// Recognized by file extension alone.
    Extension,
    /// Recognized by container magic or header bytes.
    Header,
    /// Unambiguous match (matching header + extension or container member path).
    Exact,
}

impl Confidence {
    /// Whether this confidence represents any positive match.
    #[must_use]
    pub const fn is_match(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Proprietary protection or licensing system guarding a container or preset.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtectionSystem {
    /// PACE / iLok protected container (e.g., locked commercial UVI `.ufs` banks).
    PaceIlok,
    /// Native Instruments encrypted preset or `.nkx`/`.nkr` archive requiring `.nicnt` access data.
    NativeAccess,
    /// Another proprietary or unrecognized container protection scheme.
    Unknown(String),
}

impl fmt::Display for ProtectionSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PaceIlok => f.write_str("PACE / iLok"),
            Self::NativeAccess => f.write_str("Native Access"),
            Self::Unknown(name) => f.write_str(name),
        }
    }
}

/// Whether an instrument, bank, or container can be read openly or is blocked
/// by DRM/licensing protection.
///
/// Diagnostics carry only sanitized descriptions — never keys or secrets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtectionStatus {
    /// Open, unencrypted, or legitimately self-contained.
    #[default]
    Open,
    /// Guarded by a DRM or licensing layer that KONTRA does not circumvent.
    Protected {
        system: ProtectionSystem,
        detail: String,
    },
}

impl ProtectionStatus {
    /// Constructs a [`ProtectionStatus::Protected`] value.
    #[must_use]
    pub fn protected(system: ProtectionSystem, detail: impl Into<String>) -> Self {
        Self::Protected {
            system,
            detail: detail.into(),
        }
    }

    /// Returns `true` when the container/entry is readable without external DRM.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        matches!(self, Self::Open)
    }

    /// Returns `true` when the container/entry is DRM-protected.
    #[must_use]
    pub const fn is_protected(&self) -> bool {
        matches!(self, Self::Protected { .. })
    }

    /// Returns the protection system if protected.
    #[must_use]
    pub const fn system(&self) -> Option<&ProtectionSystem> {
        match self {
            Self::Open => None,
            Self::Protected { system, .. } => Some(system),
        }
    }

    /// Returns the human-readable detail if protected.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Open => None,
            Self::Protected { detail, .. } => Some(detail.as_str()),
        }
    }
}

impl fmt::Display for ProtectionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => f.write_str("open"),
            Self::Protected { system, detail } if detail.is_empty() => {
                write!(f, "protected ({system})")
            }
            Self::Protected { system, detail } => {
                write!(f, "protected ({system}): {detail}")
            }
        }
    }
}

/// Structured error returned by [`Format`] operations so callers can reliably
/// distinguish DRM protection from I/O, missing files, unsupported variants,
/// and malformed payloads.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    /// The file or container is DRM-protected and cannot be read.
    #[error("{}: {status}", path.display())]
    Protected {
        path: PathBuf,
        status: ProtectionStatus,
    },
    /// Filesystem I/O failure while reading the path or container.
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// Target file, container member, or referenced base instrument does not exist.
    #[error("{}: not found: {detail}", path.display())]
    NotFound { path: PathBuf, detail: String },
    /// Valid container/format variant that this translator does not support.
    #[error("{}: unsupported: {detail}", path.display())]
    Unsupported { path: PathBuf, detail: String },
    /// Corrupt, truncated, or structurally invalid container/preset payload.
    #[error("{}: {what}: {detail}", path.display())]
    Malformed {
        path: PathBuf,
        what: &'static str,
        detail: String,
    },
}

impl FormatError {
    /// Constructs a [`FormatError::Protected`] error.
    #[must_use]
    pub fn protected(path: impl Into<PathBuf>, status: ProtectionStatus) -> Self {
        Self::Protected {
            path: path.into(),
            status,
        }
    }

    /// Constructs a [`FormatError::Io`] error.
    #[must_use]
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Constructs a [`FormatError::NotFound`] error.
    #[must_use]
    pub fn not_found(path: impl Into<PathBuf>, detail: impl Into<String>) -> Self {
        Self::NotFound {
            path: path.into(),
            detail: detail.into(),
        }
    }

    /// Constructs a [`FormatError::Unsupported`] error.
    #[must_use]
    pub fn unsupported(path: impl Into<PathBuf>, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            path: path.into(),
            detail: detail.into(),
        }
    }

    /// Constructs a [`FormatError::Malformed`] error.
    #[must_use]
    pub fn malformed(
        path: impl Into<PathBuf>,
        what: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self::Malformed {
            path: path.into(),
            what,
            detail: detail.into(),
        }
    }

    /// Returns the structured [`ProtectionStatus`] if this failure is caused by DRM protection.
    #[must_use]
    pub const fn protection(&self) -> Option<&ProtectionStatus> {
        match self {
            Self::Protected { status, .. } => Some(status),
            _ => None,
        }
    }

    /// Returns the path associated with this error.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Protected { path, .. }
            | Self::Io { path, .. }
            | Self::NotFound { path, .. }
            | Self::Unsupported { path, .. }
            | Self::Malformed { path, .. } => path.as_path(),
        }
    }
}

/// Addresses a loadable instrument file or a member inside a bank/container.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntryRef {
    /// Path to the file, or virtual container-member path (`bank.ufs/Folder/Patch.uvip`).
    pub path: PathBuf,
    /// Optional explicit member path inside `path` when `path` is a bank/container.
    pub member: Option<String>,
}

impl EntryRef {
    /// Creates an [`EntryRef`] for a direct file or virtual member path.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            member: None,
        }
    }

    /// Creates an [`EntryRef`] for an explicit member inside a container file.
    #[must_use]
    pub fn with_member(path: impl Into<PathBuf>, member: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            member: Some(member.into()),
        }
    }

    /// Resolves `self` to a combined path (`path/member` when `member` is set).
    #[must_use]
    pub fn full_path(&self) -> PathBuf {
        match &self.member {
            Some(member) if !member.is_empty() => self.path.join(member),
            _ => self.path.clone(),
        }
    }
}

impl From<PathBuf> for EntryRef {
    fn from(path: PathBuf) -> Self {
        Self::new(path)
    }
}

impl From<&Path> for EntryRef {
    fn from(path: &Path) -> Self {
        Self::new(path)
    }
}

/// A single instrument or bank entry discovered during [`Format::scan`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanEntry {
    pub format: FormatId,
    pub path: PathBuf,
    pub name: String,
    pub protection: ProtectionStatus,
}

/// Sink receiving entries and scan issues produced by [`Format::scan`].
pub trait ScanSink {
    /// Records one discovered preset/instrument entry.
    fn entry(&mut self, entry: ScanEntry);

    /// Records a file or container that could not be scanned (or is protected).
    fn issue(&mut self, path: &Path, reason: String, protection: ProtectionStatus);
}

/// In-memory [`ScanSink`] collecting all discovered entries and issues.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanCollector {
    pub entries: Vec<ScanEntry>,
    pub issues: Vec<(PathBuf, String, ProtectionStatus)>,
}

impl ScanSink for ScanCollector {
    fn entry(&mut self, entry: ScanEntry) {
        self.entries.push(entry);
    }

    fn issue(&mut self, path: &Path, reason: String, protection: ProtectionStatus) {
        self.issues.push((path.to_path_buf(), reason, protection));
    }
}

/// Options supplied by callers when loading an instrument via [`Format::load`].
#[derive(Clone, Debug, Default)]
pub struct LoadOptions {
    /// Known catalog instruments, used by snapshot formats to bind detached presets.
    pub known_instruments: Vec<PathBuf>,
}

/// Shared load context passed into [`Format::load`].
pub struct LoadCx<'a> {
    pub diag: &'a mut Notes,
    pub options: &'a LoadOptions,
}

impl<'a> LoadCx<'a> {
    /// Creates a load context from a diagnostics accumulator and load options.
    #[must_use]
    pub fn new(diag: &'a mut Notes, options: &'a LoadOptions) -> Self {
        Self { diag, options }
    }
}

/// Uniform contract implemented by each sampler format translator crate.
pub trait Format: Send + Sync + 'static {
    /// Stable identifier for this format.
    fn id(&self) -> FormatId;

    /// Rates how confidently this format recognizes `path` and its leading bytes `head`.
    fn sniff(&self, path: &Path, head: &[u8]) -> Confidence;

    /// Inspects whether `path` is open or DRM-protected without performing a full load.
    ///
    /// # Errors
    /// Returns [`FormatError`] if `path` cannot be opened or has a malformed container header.
    fn protection(&self, path: &Path) -> Result<ProtectionStatus, FormatError> {
        let _ = path;
        Ok(ProtectionStatus::Open)
    }

    /// Scans a directory or container `root`, emitting discovered entries and issues to `out`.
    ///
    /// # Errors
    /// Returns [`FormatError`] if `root` itself cannot be traversed.
    fn scan(&self, root: &Path, out: &mut dyn ScanSink) -> Result<(), FormatError>;

    /// Loads `entry` into an [`ir::Instrument`], recording non-fatal gaps in `cx.diag`.
    ///
    /// # Errors
    /// Returns [`FormatError`] if the entry cannot be opened, is DRM-protected, or is malformed.
    fn load(&self, entry: &EntryRef, cx: &mut LoadCx<'_>) -> Result<ir::Instrument, FormatError>;
}

/// Runtime registry of [`Format`] implementations for sniffing, scanning, and loading.
#[derive(Default)]
pub struct Registry {
    formats: Vec<Box<dyn Format>>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ids: Vec<FormatId> = self.formats.iter().map(|fmt| fmt.id()).collect();
        f.debug_struct("Registry").field("formats", &ids).finish()
    }
}

impl Registry {
    /// Creates an empty format registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            formats: Vec::new(),
        }
    }

    /// Registers a format implementation.
    pub fn register<F: Format>(&mut self, format: F) -> &mut Self {
        self.formats.push(Box::new(format));
        self
    }

    /// Returns all registered formats in registration order.
    #[must_use]
    pub fn formats(&self) -> &[Box<dyn Format>] {
        &self.formats
    }

    /// Returns the registered format with identifier `id`, if present.
    #[must_use]
    pub fn by_id(&self, id: FormatId) -> Option<&dyn Format> {
        self.formats
            .iter()
            .find(|fmt| fmt.id() == id)
            .map(Box::as_ref)
    }

    /// Finds the highest-confidence format matching `path` and leading bytes `head`.
    #[must_use]
    pub fn sniff(&self, path: &Path, head: &[u8]) -> Option<(&dyn Format, Confidence)> {
        let mut best: Option<(&dyn Format, Confidence)> = None;
        for fmt in &self.formats {
            let confidence = fmt.sniff(path, head);
            if confidence.is_match() && best.is_none_or(|(_, cur)| confidence > cur) {
                best = Some((fmt.as_ref(), confidence));
            }
        }
        best
    }

    /// Reads up to 64 leading bytes from `path` (or its backing container if `path`
    /// is a virtual member path) and sniffs the best matching format.
    ///
    /// # Errors
    /// Returns an [`io::Error`] if an existing regular file cannot be opened or read.
    pub fn sniff_file(&self, path: &Path) -> io::Result<Option<(&dyn Format, Confidence)>> {
        let mut buf = [0u8; SNIFF_HEAD_BYTES];
        let head = if path.is_file() {
            read_head(path, &mut buf)?
        } else if let Some(container) = path.ancestors().skip(1).find(|p| p.is_file()) {
            read_head(container, &mut buf)?
        } else {
            &[]
        };
        Ok(self.sniff(path, head))
    }

    /// Inspects the [`ProtectionStatus`] of `path` using the best matching format.
    ///
    /// # Errors
    /// Returns [`FormatError::Unsupported`] if no registered format recognizes `path`,
    /// or forwards the matched format's error.
    pub fn probe_protection(&self, path: &Path) -> Result<ProtectionStatus, FormatError> {
        let matched = self
            .sniff_file(path)
            .map_err(|e| FormatError::io(path, e))?
            .ok_or_else(|| {
                FormatError::unsupported(path, "no registered format recognizes this file")
            })?;
        matched.0.protection(path)
    }

    /// Scans `root` across all registered formats.
    ///
    /// # Errors
    /// Returns the first fatal scan error reported by a format.
    pub fn scan(&self, root: &Path, out: &mut dyn ScanSink) -> Result<(), FormatError> {
        for fmt in &self.formats {
            fmt.scan(root, out)?;
        }
        Ok(())
    }

    /// Sniffs `path` and loads it via the highest-confidence matching format.
    ///
    /// # Errors
    /// Returns [`FormatError`] if sniffing fails, no format matches, or loading fails.
    pub fn load(&self, path: &Path, cx: &mut LoadCx<'_>) -> Result<ir::Instrument, FormatError> {
        let matched = self
            .sniff_file(path)
            .map_err(|e| FormatError::io(path, e))?
            .ok_or_else(|| {
                FormatError::unsupported(path, "no registered format recognizes this file")
            })?;
        matched.0.load(&EntryRef::new(path), cx)
    }
}

fn read_head<'a>(path: &Path, buf: &'a mut [u8]) -> io::Result<&'a [u8]> {
    let mut file = File::open(path)?;
    let mut filled = 0;
    while filled < buf.len() {
        let Some(dst) = buf.get_mut(filled..) else {
            break;
        };
        match file.read(dst) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(buf.get(..filled).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubFormat {
        id: FormatId,
        ext: &'static str,
        magic: &'static [u8],
        protected: bool,
    }

    impl Format for StubFormat {
        fn id(&self) -> FormatId {
            self.id
        }

        fn sniff(&self, path: &Path, head: &[u8]) -> Confidence {
            let ext_match = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(self.ext));
            let magic_match = !self.magic.is_empty() && head.starts_with(self.magic);
            match (ext_match, magic_match) {
                (true, true) => Confidence::Exact,
                (false, true) => Confidence::Header,
                (true, false) => Confidence::Extension,
                (false, false) => Confidence::None,
            }
        }

        fn protection(&self, _path: &Path) -> Result<ProtectionStatus, FormatError> {
            if self.protected {
                Ok(ProtectionStatus::protected(
                    ProtectionSystem::PaceIlok,
                    "requires PACE/iLok license",
                ))
            } else {
                Ok(ProtectionStatus::Open)
            }
        }

        fn scan(&self, root: &Path, out: &mut dyn ScanSink) -> Result<(), FormatError> {
            let status = self.protection(root)?;
            if status.is_protected() {
                out.issue(root, status.to_string(), status);
            } else {
                out.entry(ScanEntry {
                    format: self.id,
                    path: root.join(format!("patch.{}", self.ext)),
                    name: "patch".into(),
                    protection: status,
                });
            }
            Ok(())
        }

        fn load(
            &self,
            entry: &EntryRef,
            _cx: &mut LoadCx<'_>,
        ) -> Result<ir::Instrument, FormatError> {
            if self.protected {
                return Err(FormatError::protected(
                    entry.full_path(),
                    ProtectionStatus::protected(
                        ProtectionSystem::PaceIlok,
                        "requires PACE/iLok license",
                    ),
                ));
            }
            Ok(ir::Instrument {
                name: format!("loaded-{}", self.id.as_str()),
                source_path: Some(entry.full_path()),
                ..ir::Instrument::DEFAULT
            })
        }
    }

    #[test]
    fn registry_prefers_highest_confidence_match() {
        let mut reg = Registry::new();
        reg.register(StubFormat {
            id: FormatId::Kontakt,
            ext: "nki",
            magic: b"KONT",
            protected: false,
        });
        reg.register(StubFormat {
            id: FormatId::Uvi,
            ext: "ufs",
            magic: b"UFS2",
            protected: true,
        });

        let (fmt, conf) = reg.sniff(Path::new("misnamed.nki"), b"UFS2rest").unwrap();
        assert_eq!(fmt.id(), FormatId::Uvi);
        assert_eq!(conf, Confidence::Header);

        let (fmt, conf) = reg.sniff(Path::new("bank.ufs"), b"UFS2rest").unwrap();
        assert_eq!(fmt.id(), FormatId::Uvi);
        assert_eq!(conf, Confidence::Exact);

        assert!(reg.sniff(Path::new("readme.txt"), b"hello").is_none());
    }

    #[test]
    fn protected_format_reports_structured_error_and_scan_issue() {
        let mut reg = Registry::new();
        reg.register(StubFormat {
            id: FormatId::Uvi,
            ext: "ufs",
            magic: b"UFS2",
            protected: true,
        });

        let mut collector = ScanCollector::default();
        reg.scan(Path::new("/libs/locked.ufs"), &mut collector)
            .unwrap();
        assert!(collector.entries.is_empty());
        assert_eq!(collector.issues.len(), 1);
        assert!(collector.issues[0].2.is_protected());
        assert_eq!(
            collector.issues[0].2.system(),
            Some(&ProtectionSystem::PaceIlok)
        );

        let mut notes = Notes::default();
        let opts = LoadOptions::default();
        let mut cx = LoadCx::new(&mut notes, &opts);
        let err = reg
            .load(Path::new("/libs/locked.ufs"), &mut cx)
            .unwrap_err();
        assert!(err.protection().is_some_and(ProtectionStatus::is_protected));
        assert!(err.to_string().contains("PACE / iLok"));
    }

    #[test]
    fn protection_status_accessors_and_display() {
        let open = ProtectionStatus::Open;
        let locked = ProtectionStatus::protected(
            ProtectionSystem::NativeAccess,
            "missing .nicnt access keys",
        );
        assert!(open.is_open());
        assert!(!open.is_protected());
        assert!(locked.is_protected());
        assert_eq!(locked.system(), Some(&ProtectionSystem::NativeAccess));
        assert_eq!(locked.detail(), Some("missing .nicnt access keys"));
        assert!(locked.to_string().contains("Native Access"));
    }
}
