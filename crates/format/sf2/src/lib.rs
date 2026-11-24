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

use kontra_format::{
    Confidence, EntryRef, Format, FormatError, FormatId, LoadCx, ProtectionStatus, ScanEntry,
    ScanSink, scan_dir,
};
use kontra_ir as ir;

/// SoundFont 2 format implementation for [`kontra_format::Registry`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Sf2;

impl Sf2 {
    pub const ID: FormatId = FormatId::Sf2;
}

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

/// Parses a virtual SoundFont preset path such as `Bank.sf2/000:001 Name.sf2p`
/// or `Bank.sf2/0:1` into `(bank_path, bank, program)`.
#[must_use]
pub fn parse_slot(path: &Path) -> Option<(&Path, u16, u16)> {
    let bank_path = path.parent().filter(|p| is_sf2_extension(p))?;
    let name = path.file_name()?.to_str()?;
    let head = name.split_whitespace().next().unwrap_or(name);
    let head = head.strip_suffix(".sf2p").unwrap_or(head);
    let (bank, program) = head.split_once(':')?;
    Some((bank_path, bank.parse().ok()?, program.parse().ok()?))
}

fn is_sf2_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("sf2") || e.eq_ignore_ascii_case("sf3"))
}

fn has_sf2_magic(head: &[u8]) -> bool {
    head.get(..4) == Some(b"RIFF") && head.get(8..12) == Some(b"sfbk")
}

fn to_format_error(path: &Path, err: Error) -> FormatError {
    match err {
        Error::Io { path, source } => {
            if source.kind() == std::io::ErrorKind::NotFound {
                FormatError::not_found(path, source.to_string())
            } else {
                FormatError::io(path, source)
            }
        }
        Error::NotSoundFont(reason) => FormatError::malformed(path, "SoundFont", reason),
        Error::Malformed { chunk, detail } => FormatError::malformed(path, chunk, detail),
        Error::NoPreset { bank, program } => {
            FormatError::not_found(path, format!("no preset at bank {bank}, program {program}"))
        }
        Error::ExpansionLimit { .. } => FormatError::unsupported(path, err.to_string()),
        Error::NoPresets => FormatError::malformed(path, "SoundFont", "the bank holds no presets"),
    }
}

/// Reads the first preset of `path`, recording any remaining presets as a
/// cosmetic diagnostic in one file open pass.
fn read_first_with_summary(path: &Path) -> Result<ir::Instrument, Error> {
    let sf = open(path)?;
    let list = presets(&sf);
    let first = list.first().ok_or(Error::NoPresets)?;
    let mut out = translate::preset(&sf, find(&sf, first.bank, first.program)?, path)?;
    let rest: Vec<String> = list
        .iter()
        .skip(1)
        .map(|p| format!("{}:{} {}", p.bank, p.program, p.name))
        .collect();
    if !rest.is_empty() {
        out.diagnostics.push(ir::Diagnostic {
            impact: ir::Impact::Cosmetic,
            feature: "SoundFont presets not loaded".into(),
            detail: rest.join(", "),
        });
    }
    Ok(out)
}

impl Format for Sf2 {
    fn id(&self) -> FormatId {
        Self::ID
    }

    fn sniff(&self, path: &Path, head: &[u8]) -> Confidence {
        let ext = is_sf2_extension(path) || parse_slot(path).is_some();
        let magic = has_sf2_magic(head);
        match (ext, magic) {
            (true, true) => Confidence::Exact,
            (false, true) => Confidence::Header,
            (true, false) => Confidence::Extension,
            (false, false) => Confidence::None,
        }
    }

    fn scan(&self, root: &Path, out: &mut dyn ScanSink) -> Result<(), FormatError> {
        scan_dir(root, out, |path, sink| {
            if !is_sf2_extension(path) {
                return;
            }
            match list_presets(path) {
                Ok(list) => {
                    for preset in list {
                        let trimmed = preset.name.trim();
                        let safe = if trimmed.is_empty() {
                            "Preset".to_owned()
                        } else {
                            trimmed.replace(['/', '\\'], "-")
                        };
                        let virtual_path = path.join(format!(
                            "{:03}:{:03} {safe}.sf2p",
                            preset.bank, preset.program
                        ));
                        sink.entry(ScanEntry {
                            format: Self::ID,
                            path: virtual_path,
                            name: preset.name,
                            protection: ProtectionStatus::Open,
                        });
                    }
                }
                Err(err) => sink.issue(path, err.to_string(), ProtectionStatus::Open),
            }
        })
    }

    fn load(&self, entry: &EntryRef, cx: &mut LoadCx<'_>) -> Result<ir::Instrument, FormatError> {
        let full = entry.full_path();
        let mut inst = if let Some((bank_path, bank, program)) = parse_slot(&full) {
            let mut out =
                read_preset(bank_path, bank, program).map_err(|err| to_format_error(&full, err))?;
            out.source_path = Some(full);
            out
        } else {
            read_first_with_summary(&full).map_err(|err| to_format_error(&full, err))?
        };
        if inst.source_path.is_none() {
            inst.source_path = Some(entry.full_path());
        }
        cx.diag.record_diagnostics(&inst.diagnostics);
        Ok(inst)
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;
    use kontra_format::{LoadOptions, Notes, Registry, ScanCollector};

    fn minimal_sf2_bytes() -> Vec<u8> {
        fn chunk(id: &[u8], body: &[u8]) -> Vec<u8> {
            let mut out = id.to_vec();
            out.extend((body.len() as u32).to_le_bytes());
            out.extend(body);
            out
        }
        fn list(kind: &[u8], chunks: &[Vec<u8>]) -> Vec<u8> {
            chunk(b"LIST", &[kind.to_vec(), chunks.concat()].concat())
        }
        fn name(s: &str) -> Vec<u8> {
            let mut b = s.as_bytes().to_vec();
            b.resize(20, 0);
            b
        }
        fn words(ws: &[u16]) -> Vec<u8> {
            ws.iter().flat_map(|w| w.to_le_bytes()).collect()
        }
        let mut smpl = vec![0u8; 200];
        smpl.resize(smpl.len() + 92, 0);
        let mut shdr = Vec::new();
        for (n, end, rate, kind) in [("s", 100u32, 44_100u32, 1u16), ("EOS", 0, 0, 0)] {
            shdr.extend(name(n));
            for v in [0u32, end, 0, 0, rate] {
                shdr.extend(v.to_le_bytes());
            }
            shdr.extend([60, 0, 0, 0]);
            shdr.extend(kind.to_le_bytes());
        }
        let phdr = [
            [name("Piano"), words(&[0, 0, 0]), vec![0; 12]].concat(),
            [name("Organ"), words(&[1, 0, 1]), vec![0; 12]].concat(),
            [name("EOP"), words(&[0, 0, 2]), vec![0; 12]].concat(),
        ]
        .concat();
        let pdta = list(
            b"pdta",
            &[
                chunk(b"phdr", &phdr),
                chunk(b"pbag", &words(&[0, 0, 1, 0, 2, 0])),
                chunk(b"pmod", &[0; 10]),
                chunk(b"pgen", &words(&[41, 0, 41, 0, 0, 0])),
                chunk(
                    b"inst",
                    &[name("I"), words(&[0]), name("EOI"), words(&[1])].concat(),
                ),
                chunk(b"ibag", &words(&[0, 0, 1, 0])),
                chunk(b"imod", &[0; 10]),
                chunk(b"igen", &words(&[53, 0, 0, 0])),
                chunk(b"shdr", &shdr),
            ],
        );
        let body = [
            b"sfbk".to_vec(),
            list(
                b"INFO",
                &[chunk(b"ifil", &words(&[2, 1])), chunk(b"INAM", b"Test\0\0")],
            ),
            list(b"sdta", &[chunk(b"smpl", &smpl)]),
            pdta,
        ]
        .concat();
        chunk(b"RIFF", &body)
    }

    #[test]
    fn sf2_format_sniffs_scans_and_loads_via_registry() {
        let mut reg = Registry::new();
        reg.register(Sf2);

        let bytes = minimal_sf2_bytes();
        let head = &bytes[..16];
        let (fmt, conf) = reg.sniff(Path::new("GM.sf2"), head).unwrap();
        assert_eq!((fmt.id(), conf), (FormatId::Sf2, Confidence::Exact));
        let (fmt, conf) = reg.sniff(Path::new("GM.bin"), head).unwrap();
        assert_eq!((fmt.id(), conf), (FormatId::Sf2, Confidence::Header));
        let (fmt, conf) = reg
            .sniff(Path::new("GM.sf2/000:001 Organ.sf2p"), b"")
            .unwrap();
        assert_eq!((fmt.id(), conf), (FormatId::Sf2, Confidence::Extension));
        assert!(reg.sniff(Path::new("Readme.txt"), b"plain").is_none());

        let root = std::env::temp_dir().join(format!("kontra-sf2-fmt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let mut collector = ScanCollector::default();
        let err = reg.scan(&root.join("missing"), &mut collector).unwrap_err();
        assert!(matches!(err, FormatError::NotFound { .. }));

        std::fs::create_dir_all(&root).unwrap();
        let bank = root.join("Bank.sf2");
        std::fs::write(&bank, &bytes).unwrap();

        reg.scan(&root, &mut collector).unwrap();
        assert_eq!(collector.entries.len(), 2);
        assert_eq!(collector.entries[0].name, "Piano");
        assert_eq!(collector.entries[1].name, "Organ");

        let mut notes = Notes::default();
        let opts = LoadOptions::default();
        let mut cx = LoadCx::new(&mut notes, &opts);
        let first = reg.load(&bank, &mut cx).unwrap();
        assert_eq!(first.name, "Piano");
        let diags = notes.finish();
        assert!(
            diags
                .iter()
                .any(|d| d.feature == "SoundFont presets not loaded"),
            "remaining presets diagnostic should be forwarded to cx.diag: {diags:?}"
        );

        let mut notes = Notes::default();
        let mut cx = LoadCx::new(&mut notes, &opts);
        let second = reg.load(&collector.entries[1].path, &mut cx).unwrap();
        assert_eq!(second.name, "Organ");

        std::fs::remove_dir_all(&root).unwrap();
    }
}
