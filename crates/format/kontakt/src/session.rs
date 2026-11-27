//! Decode Kontakt host session chunks (VST3 `Comp` / VST2 `.fxb` `hsin`
//! containers) into structured multi/slot state for automated migration.
//!
//! Encrypted commercial-library session chunks carry their library's SNPID in
//! the unencrypted NIS `Authorization` header (`0x6a`). [`Keyring`] maps
//! installed `.nicnt` files by SNPID so session chunks extracted from DAW
//! projects decrypt cleanly without residing inside the library folder.

use std::collections::{BTreeSet, HashMap};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ni_file::NIFile;
use ni_file::kontakt::objects::{BParScript, Bank, FNTableImpl, Program};
use ni_file::kontakt::{Chunk, KontaktChunks, StructuredObject};
use ni_file::nis::schema::{NISObject, PresetChunkItem, Repository};
use ni_file::nis::{AppSpecificProperties, ItemContainer, ItemType};

use crate::access::{self, NicntInfo};

const PROGRAM: u16 = 0x28;
const PROGRAM_LIST: u16 = 0x36;
const FILE_TABLE: u16 = 0x4b;
const OLD_FILE_TABLE: u16 = 0x3d;
const SLOT_TABLE: u16 = 0x48;
const SCRIPT: u16 = 0x06;

/// Metadata for a registered Kontakt library discovered from its `.nicnt` file.
#[derive(Clone, Debug)]
pub struct LibraryRecord {
    pub snpid: String,
    pub name: Option<String>,
    pub dir: PathBuf,
    keystream: Option<Arc<[u8]>>,
}

/// SNPID-indexed keyring built from installed `.nicnt` files.
#[derive(Clone, Debug, Default)]
pub struct Keyring {
    by_snpid: HashMap<String, LibraryRecord>,
}

impl Keyring {
    /// Scans `roots` (up to depth 3) for `.nicnt` files and builds a keyring.
    #[must_use]
    pub fn scan(roots: &[PathBuf]) -> Self {
        let mut keyring = Self::default();
        let mut visited = BTreeSet::new();
        for root in roots {
            keyring.walk(root, 0, &mut visited);
        }
        keyring
    }

    fn walk(&mut self, dir: &Path, depth: usize, visited: &mut BTreeSet<PathBuf>) {
        if depth > 3 {
            return;
        }
        let key = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        if !visited.insert(key) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_file()
                && path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("nicnt"))
            {
                if let Some(NicntInfo {
                    snpid,
                    name,
                    keystream,
                }) = access::read_nicnt(&path)
                {
                    let Some(parent) = path.parent() else {
                        continue;
                    };
                    let norm = snpid.to_ascii_lowercase();
                    let replace = self
                        .by_snpid
                        .get(&norm)
                        .is_none_or(|old| old.keystream.is_none() && keystream.is_some());
                    if replace {
                        self.by_snpid.insert(
                            norm,
                            LibraryRecord {
                                snpid,
                                name,
                                dir: parent.to_path_buf(),
                                keystream,
                            },
                        );
                    }
                }
            } else if path.is_dir() {
                let skip = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    n.starts_with('.')
                        || n.eq_ignore_ascii_case("samples")
                        || n.eq_ignore_ascii_case("documentation")
                        || n.eq_ignore_ascii_case("resources")
                });
                if !skip {
                    self.walk(&path, depth + 1, visited);
                }
            }
        }
    }

    /// Looks up a library record by SNPID (case-insensitive).
    #[must_use]
    pub fn get(&self, snpid: &str) -> Option<&LibraryRecord> {
        self.by_snpid.get(&snpid.to_ascii_lowercase())
    }

    /// Number of indexed `.nicnt` libraries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_snpid.len()
    }

    /// Whether no `.nicnt` libraries were indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_snpid.is_empty()
    }
}

/// Outcome of decoding a Kontakt host state chunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionStatus {
    /// Decoded successfully; `slots` holds every loaded rack instrument.
    Decoded,
    /// Decoded successfully, and the Kontakt multi had 0 loaded instruments.
    Empty,
    /// Chunk is encrypted and its library SNPID has no installed `.nicnt` key.
    MissingLibraryKey,
}

/// Library matched from the chunk's SNPID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchedLibrary {
    pub snpid: String,
    pub name: Option<String>,
    pub dir: PathBuf,
}

/// One instrument slot extracted from a Kontakt host state chunk.
#[derive(Clone, Debug)]
pub struct SessionSlot {
    pub index: u16,
    pub name: String,
    /// Kontakt port+channel: 0 = omni, 1..=64 = A1..D16.
    pub channel: u16,
    /// Stereo output index from 0.
    pub output: u16,
    /// Program header linear gain (`laws::gain(volume)`).
    pub volume_gain: f32,
    /// Program header pan (`laws::pan(pan)` in -1..=1).
    pub pan: f32,
    /// Non-bypassed, non-empty KSP script slots' persistent variable lines,
    /// ordered identically to `translate::instrument`.
    pub scripts: Vec<Vec<String>>,
    /// Distinct directory/path hints from the multi's file table.
    pub sample_hints: Vec<String>,
    /// Resource container path hint (`.nkr`), if any.
    pub resource_container: Option<String>,
}

/// Decoded Kontakt host state chunk.
#[derive(Clone, Debug)]
pub struct KontaktSession {
    pub status: SessionStatus,
    pub encrypted: bool,
    pub snpids: Vec<String>,
    pub matched_library: Option<MatchedLibrary>,
    pub multi_script_count: usize,
    pub slots: Vec<SessionSlot>,
    pub slot_errors: Vec<String>,
}

/// Decodes a raw Kontakt VST3 `Comp` (or `hsin`) state chunk using `keyring`.
///
/// # Errors
/// When the bytes are not a valid Kontakt NIS/NKS container or its chunk stream
/// cannot be parsed.
pub fn decode_state(bytes: &[u8], keyring: &Keyring) -> Result<KontaktSession, String> {
    let mut cursor = Cursor::new(bytes);
    let container = NIFile::read(&mut cursor).map_err(|e| format!("NI container: {e}"))?;
    let NIFile::NISoundContainer(nis) = container else {
        return Err("not a Kontakt NISound container".to_owned());
    };
    let mut snpids = Vec::new();
    collect_snpids(&nis, &mut snpids);
    let inner = unwrap_app_specific(nis, 0)?;
    let NISObject::BNISoundPreset(preset) = Repository::from(inner).infer_schema() else {
        return Err("NIS container is not a sound preset".to_owned());
    };
    let encrypted = preset
        .is_encrypted()
        .map_err(|e| format!("NIS encryption flag: {e}"))?;

    let matched = snpids.iter().find_map(|id| keyring.get(id)).cloned();
    let matched_library = matched.as_ref().map(|lib| MatchedLibrary {
        snpid: lib.snpid.clone(),
        name: lib.name.clone(),
        dir: lib.dir.clone(),
    });

    let key = if encrypted {
        let Some(stream) = matched.as_ref().and_then(|lib| lib.keystream.clone()) else {
            return Ok(KontaktSession {
                status: SessionStatus::MissingLibraryKey,
                encrypted,
                snpids,
                matched_library,
                multi_script_count: 0,
                slots: Vec::new(),
                slot_errors: Vec::new(),
            });
        };
        Some(access::key_from_stream(stream))
    } else {
        None
    };

    let item = preset
        .encryption_item_with_key(key.as_deref())
        .map_err(|e| format!("decrypting NIS preset: {e}"))?;
    let subtree = item
        .subtree
        .item()
        .map_err(|e| format!("reading NIS subtree: {e}"))?;
    let chunk_bytes = PresetChunkItem::from(subtree)
        .properties()
        .map_err(|e| format!("reading NIS preset chunk: {e}"))?
        .0;

    let chunks = KontaktChunks::read(Cursor::new(chunk_bytes))
        .map_err(|e| format!("parsing Kontakt chunks: {e}"))?;
    let bank_chunk = chunks
        .find_first(ni_file::kontakt::objects::KONTAKT_BANK_ID)
        .ok_or_else(|| "Kontakt session has no bank chunk".to_owned())?;
    let bank = Bank::try_from(bank_chunk).map_err(|e| format!("bank chunk: {e}"))?;

    let multi_script_count = bank
        .0
        .children
        .iter()
        .filter(|c| c.id == SCRIPT)
        .filter_map(|c| BParScript::try_from(c).and_then(|s| s.params()).ok())
        .filter(|s| !s.bypass && s.text.as_deref().is_some_and(|t| !t.trim().is_empty()))
        .count();

    let list = bank.slot_list().map_err(|e| format!("slot list: {e}"))?;
    let table_chunk = chunks
        .find_first(FILE_TABLE)
        .or_else(|| chunks.find_first(OLD_FILE_TABLE));
    let fntable = table_chunk.and_then(|t| FNTableImpl::try_from(t).ok());
    let sample_hints = collect_file_hints(fntable.as_ref());

    let slot_table = bank.0.find_first(SLOT_TABLE);
    let stride = slot_table.map_or(0, |t| t.data.len() / 64);
    let channel_of = |index: u16| -> u16 {
        let at = usize::from(index).saturating_mul(stride);
        slot_table
            .and_then(|t| t.data.get(at..at.saturating_add(2)))
            .and_then(|b| b.try_into().ok())
            .map_or(0, u16::from_le_bytes)
    };

    let mut indices: Vec<u16> = list.slots.keys().copied().collect();
    indices.sort_unstable();

    let mut slots = Vec::new();
    let mut slot_errors = Vec::new();
    for index in indices {
        let Some(container) = list.slots.get(&index) else {
            continue;
        };
        let Some(prog_chunk) = container
            .0
            .find_first(PROGRAM_LIST)
            .and_then(|l| first_program(&l.data))
        else {
            slot_errors.push(format!("slot {}: no program", index + 1));
            continue;
        };
        let program = match Program::try_from(&prog_chunk) {
            Ok(p) => p,
            Err(e) => {
                slot_errors.push(format!("slot {}: program chunk: {e}", index + 1));
                continue;
            }
        };
        let params = match program.public_record() {
            Ok(p) => p,
            Err(e) => {
                slot_errors.push(format!("slot {}: program record: {e}", index + 1));
                continue;
            }
        };
        let resource_container = params
            .resource_container_filename_ref
            .or(params.prefix.resource_container_filename)
            .and_then(|r| u32::try_from(r).ok())
            .and_then(|idx| {
                fntable
                    .as_ref()
                    .and_then(|ft| ft.special_filetable.get(&idx).cloned())
            });

        let mut scripts = Vec::new();
        for chunk in program.0.children.iter().filter(|c| c.id == SCRIPT) {
            let Ok(script) = BParScript::try_from(chunk).and_then(|s| s.params()) else {
                continue;
            };
            if script.bypass {
                continue;
            }
            let has_inline = script.text.as_deref().is_some_and(|t| !t.trim().is_empty());
            let has_linked = script
                .textfile_name
                .as_deref()
                .is_some_and(|n| !n.is_empty());
            if has_inline || has_linked {
                scripts.push(script.persistent);
            }
        }

        let output = slot_output(&container.0.private_data);
        slots.push(SessionSlot {
            index,
            name: params.prefix.name,
            channel: channel_of(index),
            output,
            volume_gain: params.prefix.volume,
            pan: params.prefix.pan.clamp(-1.0, 1.0),
            scripts,
            sample_hints: sample_hints.clone(),
            resource_container,
        });
    }

    let status = if slots.is_empty() {
        SessionStatus::Empty
    } else {
        SessionStatus::Decoded
    };

    Ok(KontaktSession {
        status,
        encrypted,
        snpids,
        matched_library,
        multi_script_count,
        slots,
        slot_errors,
    })
}

/// Reads an `.nki` file's base program header gain and pan so migration can
/// preserve user-adjusted instrument volume/pan deltas without double-applying
/// the `.nki`'s built-in defaults.
pub fn base_program_mix(path: &Path) -> Option<(f32, f32)> {
    let chunks = crate::container::chunks(path).ok()?;
    let prog_chunk = chunks.find_first(PROGRAM)?;
    let program = Program::try_from(prog_chunk).ok()?;
    let params = program.public_record().ok()?;
    Some((params.prefix.volume, params.prefix.pan.clamp(-1.0, 1.0)))
}

fn unwrap_app_specific(container: ItemContainer, depth: usize) -> Result<ItemContainer, String> {
    if depth > 3 {
        return Err("NIS container nested too deep".to_owned());
    }
    if let Some(data) = container.find_data(&ItemType::AppSpecific) {
        let app =
            AppSpecificProperties::try_from(data).map_err(|e| format!("NIS app specific: {e}"))?;
        let inner = app
            .subtree_item
            .item()
            .map_err(|e| format!("NIS app subtree: {e}"))?;
        return unwrap_app_specific(inner, depth + 1);
    }
    Ok(container)
}

/// Collects every non-empty, non-`"000"` SNPID carried in any unencrypted
/// `ItemType::Authorization` item inside `container` (walking `ItemData.inner`,
/// `AppSpecific` subtrees, and children).
fn collect_snpids(container: &ItemContainer, out: &mut Vec<String>) {
    let mut layer = Some(&container.data);
    while let Some(data) = layer {
        if data.header.item_type() == ItemType::Authorization {
            for id in parse_authorization_snpids(&data.data) {
                if !id.is_empty() && id != "000" && !out.contains(&id) {
                    out.push(id);
                }
            }
        }
        if data.header.item_type() == ItemType::AppSpecific
            && let Ok(app) = AppSpecificProperties::try_from(data)
            && let Ok(inner) = app.subtree_item.item()
        {
            collect_snpids(&inner, out);
        }
        layer = data.child();
    }
    for child in &container.children {
        collect_snpids(child, out);
    }
}

fn parse_authorization_snpids(data: &[u8]) -> Vec<String> {
    let read_u32 = |at: usize| -> Option<u32> {
        let slice = data.get(at..at.checked_add(4)?)?;
        Some(u32::from_le_bytes(slice.try_into().ok()?))
    };
    if read_u32(0) != Some(1) || read_u32(4) != Some(1) {
        return Vec::new();
    }
    // Kontakt 6/7/8 LicenseInfo carries an inner version word (1) before
    // `num_snpids` (at byte 12); older layouts place `num_snpids` at byte 8.
    for count_offset in [12usize, 8usize] {
        if let Some(ids) = read_utf16_list(data, count_offset)
            && !ids.is_empty()
        {
            return ids;
        }
    }
    Vec::new()
}

fn read_utf16_list(data: &[u8], count_offset: usize) -> Option<Vec<String>> {
    let mut pos = count_offset;
    let read_u32 = |pos: &mut usize| -> Option<u32> {
        let slice = data.get(*pos..pos.checked_add(4)?)?;
        *pos += 4;
        Some(u32::from_le_bytes(slice.try_into().ok()?))
    };
    let count = read_u32(&mut pos)?;
    if count == 0 || count > 32 {
        return None;
    }
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let units = read_u32(&mut pos)?;
        if units == 0 || units > 32 {
            return None;
        }
        let byte_len = (units as usize).checked_mul(2)?;
        let end = pos.checked_add(byte_len)?;
        let raw = data.get(pos..end)?;
        pos = end;
        let (pairs, _) = raw.as_chunks::<2>();
        let utf16: Vec<u16> = pairs.iter().map(|&c| u16::from_le_bytes(c)).collect();
        let text = String::from_utf16(&utf16).ok()?;
        if !text.bytes().all(|b| b.is_ascii_graphic()) {
            return None;
        }
        out.push(text);
    }
    Some(out)
}

fn collect_file_hints(fntable: Option<&FNTableImpl>) -> Vec<String> {
    let Some(ft) = fntable else {
        return Vec::new();
    };
    let mut hints = Vec::new();
    let iter = ft
        .special_filetable
        .values()
        .chain(ft.sample_filetable.values());
    for entry in iter {
        if hints.len() >= 16 {
            break;
        }
        let cleaned = entry.replace('\\', "/");
        if !cleaned.is_empty() && !hints.contains(&cleaned) {
            hints.push(cleaned);
        }
    }
    hints
}

fn slot_output(private: &[u8]) -> u16 {
    let n = private.len();
    private
        .get(n.saturating_sub(4)..n.saturating_sub(2))
        .and_then(|b| b.try_into().ok())
        .map_or(0, u16::from_le_bytes)
}

fn first_program(list: &[u8]) -> Option<Chunk> {
    let start = 4;
    let mut reader = Cursor::new(list.get(start..)?);
    let count = i16::from_le_bytes(list.get(..2)?.try_into().ok()?);
    (count > 0).then_some(())?;
    StructuredObject::read(&mut reader).ok()?;
    let end = start + usize::try_from(reader.position()).ok()?;
    Some(Chunk {
        id: PROGRAM,
        data: list.get(start..end)?.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_snpid_parser_extracts_utf16_ids() {
        let mut buf = Vec::new();
        for w in [1u32, 1, 2, 3] {
            buf.extend_from_slice(&w.to_le_bytes());
        }
        for c in "000".encode_utf16() {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        buf.extend_from_slice(&3u32.to_le_bytes());
        for c in "W91".encode_utf16() {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        assert_eq!(parse_authorization_snpids(&buf), vec!["000", "W91"]);
    }

    #[test]
    fn keyring_scans_nicnt_snpid_and_keystream() {
        let dir = std::env::temp_dir().join(format!("kontra-keyring-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = dir.join("TestLib");
        std::fs::create_dir_all(&lib).unwrap();
        let hex32 = "ab".repeat(32);
        let hex16 = "cd".repeat(16);
        std::fs::write(
            lib.join("Test.nicnt"),
            format!(
                "<ProductHints><Name>Test Library</Name><SNPID>W91</SNPID><JDX>{hex32}</JDX><HU>{hex16}</HU></ProductHints>"
            ),
        )
        .unwrap();
        let keyring = Keyring::scan(std::slice::from_ref(&dir));
        let rec = keyring
            .get("w91")
            .expect("SNPID lookup is case-insensitive");
        assert_eq!(rec.name.as_deref(), Some("Test Library"));
        assert_eq!(rec.dir, lib);
        assert!(rec.keystream.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
