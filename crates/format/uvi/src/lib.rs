//! UVI programs (`.uvip`, stored in `.ufs` banks or standalone on disk) read
//! into the instrument model. A bank program is addressed as
//! `bank.ufs/Folder/Name.uvip`; `bank.ufs` alone is the bank's first program.
//! Scripts (UVIScript) run on the engine; [`load_with`] also takes the samples
//! they load as the program loads.

mod bank;
mod keys;
mod program;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use kontra_format::{
    Confidence, EntryRef, Format, FormatError, FormatId, LoadCx, ProtectionStatus, ScanEntry,
    ScanSink,
};
use kontra_ir as ir;

pub use program::Swap;

/// UVI format implementation for [`kontra_format::Registry`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Uvi;

impl Uvi {
    pub const ID: FormatId = FormatId::Uvi;
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
    pub(crate) fn new(path: &Path, what: &'static str, reason: impl std::fmt::Display) -> Self {
        Self {
            path: path.to_path_buf(),
            what,
            reason: reason.to_string(),
            protection: ProtectionStatus::Open,
        }
    }

    pub(crate) fn protected(path: &Path, what: &'static str, status: ProtectionStatus) -> Self {
        let reason = status.to_string();
        Self {
            path: path.to_path_buf(),
            what,
            reason,
            protection: status,
        }
    }

    /// Returns the structured [`ProtectionStatus`] when this failure was caused
    /// by DRM protection (e.g., a PACE/iLok-locked UFS bank).
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
        if err.reason.contains("no such program") {
            return Self::not_found(err.path, err.reason);
        }
        Self::malformed(err.path, err.what, err.reason)
    }
}

/// Whether `path` addresses a program inside a UFS bank or a standalone `.uvip` file.
#[must_use]
pub fn is_uvi(path: &Path) -> bool {
    is_standalone_uvip(path) || split(path).is_some()
}

fn is_standalone_uvip(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("uvip"))
}

/// Inspects whether the UFS bank (or program path inside it) is open or
/// DRM-protected without decoding payloads or performing key recovery.
///
/// # Errors
/// When the file cannot be opened or has an invalid UFS header.
pub fn inspect_protection(path: &Path) -> Result<ProtectionStatus, Error> {
    if let Some((bank_path, _)) = split(path) {
        return bank::Bank::inspect_protection(&bank_path);
    }
    if is_standalone_uvip(path) {
        return Ok(ProtectionStatus::Open);
    }
    Err(Error::new(
        path,
        "address",
        "no .ufs bank or .uvip file in the path",
    ))
}

/// The programs in the bank at `bank`, as paths inside it, sorted.
///
/// # Errors
/// When the bank cannot be opened.
pub fn programs(bank: &Path) -> Result<Vec<String>, Error> {
    bank::Bank::program_paths(bank)
}

/// Lists program paths and inspects the protection state from the same UFS
/// directory. Does not read program payloads or recover keys.
///
/// # Errors
/// When the bank or its directory cannot be opened. An encrypted directory
/// that cannot be inspected returns an error with its protection status.
pub fn inspect_programs(bank: &Path) -> Result<(Vec<String>, ProtectionStatus), Error> {
    bank::Bank::list_programs(bank, true)
}

/// Fast read-only check for whether a verified content key is already cached
/// for the UFS bank at `bank`. Used by library scan caches to invalidate stale
/// [`ProtectionStatus::Protected`] entries once a bank is unlocked.
#[must_use]
pub fn has_cached_key(bank: &Path) -> bool {
    bank::Bank::has_cached_key(bank)
}

/// Reads the program at `path` (see the module docs for the addressing).
///
/// # Errors
/// When the bank cannot be opened or the program is not in it, or its XML is
/// not a program this can read; a sample that is missing or unreadable is a
/// diagnostic, not an error.
pub fn load(path: &Path) -> Result<ir::Instrument, Error> {
    load_with(path, &|_| Vec::new())
}

/// [`load`], with `swaps` running each script's load and returning the
/// samples it puts on oscillators.
///
/// # Errors
/// As [`load`].
pub fn load_with(
    path: &Path,
    swaps: &dyn Fn(&ir::Script) -> Vec<Swap>,
) -> Result<ir::Instrument, Error> {
    load_with_read_error(path, swaps, |path, err| Error::new(path, "program", err))
}

// Let the public UVI API retain its error type while the registry keeps the
// original filesystem error from reading a standalone program.
fn load_with_read_error<E: From<Error>>(
    path: &Path,
    swaps: &dyn Fn(&ir::Script) -> Vec<Swap>,
    read_error: impl FnOnce(&Path, std::io::Error) -> E,
) -> Result<ir::Instrument, E> {
    if let Some((bank_path, member)) = split(path) {
        let bank = bank::Bank::open(&bank_path)?;
        let (in_bank, text) = bank
            .program_text(&member)
            .map_err(|e| Error::new(path, "program", format!("{e:#}")))?;
        let folder = in_bank.rsplit_once('/').map_or("", |(folder, _)| folder);
        return Ok(translate_program(path, &text, folder, &bank, swaps)?);
    }
    if is_standalone_uvip(path) {
        let bytes = std::fs::read(path).map_err(|err| read_error(path, err))?;
        let text = uvi_file::crypto::decode_program_bytes(
            &bytes,
            &uvi_file::access::Namespaces::native().program,
        )
        .map_err(|e| Error::new(path, "program", format!("{e:#}")))?;
        let disk = bank::DiskLibrary::new(path);
        return Ok(translate_program(path, &text, "", &disk, swaps)?);
    }
    Err(Error::new(path, "address", "no .ufs bank or .uvip file in the path").into())
}

fn translate_program<L: program::Library>(
    path: &Path,
    text: &str,
    folder: &str,
    library: &L,
    swaps: &dyn Fn(&ir::Script) -> Vec<Swap>,
) -> Result<ir::Instrument, Error> {
    let doc = uvi_file::parse_program_xml(text)
        .map_err(|e| Error::new(path, "program XML", format!("{e:#}")))?;
    let root = doc.root_element();
    let root = if root.has_tag_name("Program") {
        root
    } else {
        root.children()
            .find(|n| n.has_tag_name("Program"))
            .ok_or_else(|| Error::new(path, "program XML", "no Program element"))?
    };
    let unfinished =
        program::unfinished(root, folder, library).map_err(|e| Error::new(path, "program", e))?;
    // The scripts' load pass builds a script state: the document goes first.
    drop(doc);
    let source_path = std::path::absolute(path).ok();
    Ok(ir::Instrument {
        source_path,
        ..unfinished.finish(swaps)
    })
}

/// The files of the bank a program lies in, for its script's pictures.
pub struct Resources(Option<bank::Bank>);

impl Resources {
    /// The bank of the program at `path`; reads nothing when it cannot open.
    #[must_use]
    pub fn open(path: &Path) -> Self {
        Self(split(path).and_then(|(bank, _)| bank::Bank::open(&bank).ok()))
    }

    /// The file at a script's picture path (`../artwork/knob.png`).
    #[must_use]
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.as_ref()?.resource(path)
    }
}

/// The picture that stands for the bank at `path` in a browser: its cover
/// or splash, else its interface's background, else its largest still
/// picture; none when it cannot open or holds none.
#[must_use]
pub fn cover(path: &Path) -> Option<Vec<u8>> {
    bank::Bank::open(path).ok()?.cover()
}

/// The bank a virtual path lies in, and its member (empty for the bank itself).
/// Real standalone presets and directories named `*.ufs` do not address banks.
fn split(path: &Path) -> Option<(PathBuf, String)> {
    if is_standalone_uvip(path) && path.is_file() {
        return None;
    }
    let bank = path
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ufs")) && !p.is_dir())?;
    let member = path
        .strip_prefix(bank)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    Some((bank.to_path_buf(), member))
}

impl Format for Uvi {
    fn id(&self) -> FormatId {
        Self::ID
    }

    fn sniff(&self, path: &Path, head: &[u8]) -> Confidence {
        let ufs_magic = head.starts_with(b"UFS2");
        if let Some((_, member)) = split(path) {
            if ufs_magic || !member.is_empty() {
                return Confidence::Exact;
            }
            return Confidence::Extension;
        }
        if is_standalone_uvip(path) {
            let trimmed = std::str::from_utf8(head)
                .map(|s| s.trim_start_matches('\u{feff}').trim_start())
                .unwrap_or_default();
            if trimmed.starts_with("<?xml")
                || trimmed.starts_with("<UVI4")
                || trimmed.starts_with("<Program")
            {
                return Confidence::Exact;
            }
            return Confidence::Extension;
        }
        Confidence::None
    }

    fn protection(&self, path: &Path) -> Result<ProtectionStatus, FormatError> {
        inspect_protection(path).map_err(FormatError::from)
    }

    fn scan(&self, root: &Path, out: &mut dyn ScanSink) -> Result<(), FormatError> {
        if root.is_file() {
            scan_file(root, out);
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
            let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for path in paths {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                if name.starts_with('.') {
                    continue;
                }
                let Ok(meta) = std::fs::metadata(&path) else {
                    continue;
                };
                if meta.is_dir() {
                    if !matches!(
                        name.as_str(),
                        "samples" | "documentation" | "documents" | "docs" | "resources"
                    ) {
                        stack.push(path);
                    }
                } else if meta.is_file() {
                    scan_file(&path, out);
                }
            }
        }
        Ok(())
    }

    fn load(&self, entry: &EntryRef, _cx: &mut LoadCx<'_>) -> Result<ir::Instrument, FormatError> {
        load_with_read_error(&entry.full_path(), &|_| Vec::new(), |path, err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                FormatError::not_found(path, err.to_string())
            } else {
                FormatError::io(path, err)
            }
        })
    }
}

fn scan_file(path: &Path, out: &mut dyn ScanSink) {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ufs"))
    {
        let protection = match inspect_protection(path) {
            Ok(status) => status,
            Err(err) => {
                out.issue(path, err.to_string(), err.protection);
                return;
            }
        };
        match programs(path) {
            Ok(list) => {
                for member in list {
                    let name = Path::new(&member)
                        .file_stem()
                        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                    out.entry(ScanEntry {
                        format: FormatId::Uvi,
                        path: path.join(member),
                        name,
                        protection: protection.clone(),
                    });
                }
                if protection.is_protected() {
                    out.issue(path, protection.to_string(), protection);
                }
            }
            Err(err) => {
                let status = if err.protection.is_protected() {
                    err.protection.clone()
                } else {
                    protection
                };
                out.issue(path, err.to_string(), status);
            }
        }
    } else if is_standalone_uvip(path) {
        let name = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        out.entry(ScanEntry {
            format: FormatId::Uvi,
            path: path.to_path_buf(),
            name,
            protection: ProtectionStatus::Open,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontra_format::{ProtectionSystem, Registry, ScanCollector};

    #[test]
    fn a_program_is_addressed_inside_its_bank() {
        let (bank, member) = split(Path::new("/lib/VWinds.ufs/Presets/Bb.uvip")).unwrap();
        assert_eq!(bank, Path::new("/lib/VWinds.ufs"));
        assert_eq!(member, "Presets/Bb.uvip");
        assert_eq!(split(Path::new("/lib/VWinds.ufs")).unwrap().1, "");
        assert!(is_uvi(Path::new("/lib/Patch.uvip")));
        assert!(!is_uvi(Path::new("/lib/Piano.nki")));
    }

    #[test]
    fn uvi_format_sniffs_banks_members_and_standalone_uvip() {
        let mut reg = Registry::new();
        reg.register(Uvi);

        let (fmt, conf) = reg
            .sniff(Path::new("Bank.ufs"), b"UFS2\x03\x00\x00\x00")
            .unwrap();
        assert_eq!(fmt.id(), FormatId::Uvi);
        assert_eq!(conf, Confidence::Exact);

        let (fmt, conf) = reg
            .sniff(Path::new("Bank.ufs/Presets/Lead.uvip"), b"")
            .unwrap();
        assert_eq!(fmt.id(), FormatId::Uvi);
        assert_eq!(conf, Confidence::Exact);

        let (fmt, conf) = reg
            .sniff(Path::new("Loose.uvip"), b"<?xml version=\"1.0\"?><UVI4>")
            .unwrap();
        assert_eq!(fmt.id(), FormatId::Uvi);
        assert_eq!(conf, Confidence::Exact);

        assert!(reg.sniff(Path::new("Piano.nki"), b"not-uvi").is_none());
        let extensionless = Path::new("Bank");
        assert!(reg.sniff(extensionless, b"UFS2").is_none());
        assert!(!is_uvi(extensionless));
    }

    #[test]
    fn standalone_uvip_loads_from_disk_with_case_insensitive_samples() {
        let root = std::env::temp_dir().join(format!("kontra-uvi-disk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Samples/Strings")).unwrap();
        std::fs::create_dir_all(root.join("Presets.ufs")).unwrap();

        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(48_000u32.to_le_bytes());
        wav.extend(192_000u32.to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(400u32.to_le_bytes());
        wav.resize(wav.len() + 400, 0);
        let riff_len = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&riff_len.to_le_bytes());
        std::fs::write(root.join("Samples/Strings/Violin_C4.WAV"), &wav).unwrap();

        let uvip = root.join("Presets.ufs/Solo.uvip");
        let xml = r#"<UVI4><Program Name="Solo Violin"><Layers><Layer Name="L1"><Keygroups><Keygroup Name="KG1" LowKey="48" HighKey="72"><Oscillators><SamplePlayer SamplePath="..\samples\strings\violin_c4.wav" BaseNote="60"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>"#;
        std::fs::write(&uvip, xml).unwrap();

        let inst = load(&uvip).unwrap();
        assert_eq!(inst.name, "Solo Violin");
        assert_eq!(inst.samples.len(), 1);
        assert_eq!(inst.samples[0].frames, 100);
        assert_eq!(inst.samples[0].rate, 48_000);
        assert!(
            inst.diagnostics
                .iter()
                .all(|d| d.impact != ir::Impact::Broken)
        );

        let mut collector = ScanCollector::default();
        Uvi.scan(&root, &mut collector).unwrap();
        assert_eq!(collector.entries.len(), 1);
        assert_eq!(collector.entries[0].protection, ProtectionStatus::Open);
        assert_eq!(collector.entries[0].path, uvip);

        let mut registry = Registry::new();
        registry.register(Uvi);
        assert_eq!(
            registry.probe_protection(&uvip).unwrap(),
            ProtectionStatus::Open
        );
        let mut notes = kontra_format::Notes::default();
        let options = kontra_format::LoadOptions::default();
        let mut cx = LoadCx::new(&mut notes, &options);
        assert_eq!(registry.load(&uvip, &mut cx).unwrap().name, "Solo Violin");
        std::fs::create_dir_all(root.join("Responses/Nested")).unwrap();
        std::fs::write(root.join("Responses/Room.wav"), &wav).unwrap();
        std::fs::write(root.join("Responses/Nested/Hall.WAV"), &wav).unwrap();
        std::fs::write(
            &uvip,
            r#"<Program><Scripts><ScriptProcessor Bypass="0"><script><![CDATA[-- loadImpulse]]></script></ScriptProcessor></Scripts><Auxs><AuxEffect Name="Aux0" Bypass="0" Gain="1"><Inserts><Convolver Bypass="0" Dry="0" Wet="1" SamplePath="$/Responses/Room.wav"/></Inserts></AuxEffect></Auxs></Program>"#,
        )
        .unwrap();
        let scripted = registry.load(&uvip, &mut cx).unwrap();
        assert!(scripted.responses.iter().any(|(name, _)| name == "hall"));

        std::fs::remove_file(&uvip).unwrap();
        assert!(matches!(
            registry.load(&uvip, &mut cx),
            Err(FormatError::NotFound { .. })
        ));
        std::fs::write(&uvip, "<NotAProgram/>").unwrap();
        assert!(matches!(
            registry.load(&uvip, &mut cx),
            Err(FormatError::Malformed { .. })
        ));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn locked_ufs_bank_without_verifiable_content_reports_pace_ilok_protection() {
        let root = std::env::temp_dir().join(format!("kontra-uvi-locked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bank_path = root.join("Locked.ufs");

        let mut bytes = vec![0u8; 320];
        bytes[..4].copy_from_slice(b"UFS2");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        bytes[8..24].fill(7);
        bytes[48..54].copy_from_slice(b"Locked");
        bytes[304] = 0;

        let mut payload = vec![0u8; 289];
        payload[..4].copy_from_slice(&0x6758_50e4u32.to_le_bytes());
        payload[4..14].copy_from_slice(b"patch.uvip");
        payload[260..268].copy_from_slice(&16u64.to_le_bytes());
        payload[268..276].copy_from_slice(&320u64.to_le_bytes());
        payload[276] = 9; // Protection::Unknown(9)
        bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&payload);
        let size = bytes.len() as u64;
        bytes[32..40].copy_from_slice(&size.to_le_bytes());
        std::fs::write(&bank_path, &bytes).unwrap();

        let status = inspect_protection(&bank_path).unwrap();
        assert!(status.is_protected());
        assert_eq!(status.system(), Some(&ProtectionSystem::PaceIlok));

        let err = load(&bank_path).unwrap_err();
        assert!(err.protection().is_some_and(ProtectionStatus::is_protected));
        let fmt_err = FormatError::from(err);
        assert!(
            fmt_err
                .protection()
                .is_some_and(ProtectionStatus::is_protected)
        );

        #[cfg(unix)]
        {
            // Switch to the supported content mode so opening reaches the
            // cache stamp instead of rejecting an unknown protection mode.
            let path = root.join("OldTimestamp.ufs");
            bytes[320 + 8 + 276] = 2;
            std::fs::write(&path, &bytes).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH - std::time::Duration::from_secs(1))
                .unwrap();
            let error = load(&path).unwrap_err();
            assert!(error.protection().is_none());
            assert_eq!(error.what, "bank cache metadata");
        }

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn real_uvi_bank_loads_via_consolidated_header_prober_when_installed() {
        let bank = Path::new("/data/main/Libraries/UVI/VWinds - Flutes/VWinds-C_Flute.ufs");
        if !bank.is_file() {
            return;
        }
        assert_eq!(inspect_protection(bank).unwrap(), ProtectionStatus::Open);
        let inst = load(bank).unwrap();
        assert!(!inst.samples.is_empty());
        assert!(!inst.groups.is_empty());
        assert!(
            inst.diagnostics
                .iter()
                .all(|d| d.impact != ir::Impact::Broken),
            "unexpected broken diagnostics: {:?}",
            inst.diagnostics
        );
    }
}
