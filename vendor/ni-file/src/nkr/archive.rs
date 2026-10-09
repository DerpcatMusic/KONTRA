//! Read-only NKX/NKR directory index; encrypted members need a caller-supplied keystream.
//! Format references: unnks and nkxtract; verified against local v0x110 archives.
use crate::{read_bytes::ReadBytesExt, Error};
use std::{
    collections::{HashMap, HashSet},
    io::{BufReader, Read, Seek, SeekFrom},
};

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub header_offset: u64,
    pub issue: Option<&'static str>,
    pub offset: u64,
    pub size: u64,
    pub encoded: bool,
    pub key_index: u32,
    pub valid: bool,
    /// The member header was read and `issue`/`valid`/`offset`/`size` are final.
    pub checked: bool,
}
#[derive(Debug)]
pub struct Archive {
    pub entries: HashMap<String, Entry>,
    // Only case collisions need extra storage; ordinary indexes keep their existing size.
    case_variants: HashMap<String, Entry>,
    /// Archive file length in bytes.
    pub length: u64,
    pub issues: Vec<String>,
}
impl Archive {
    /// Index every member and validate every member header.
    pub fn read<R: Read + Seek>(reader: R) -> Result<Self, Error> {
        let mut reader = BufReader::new(reader);
        let mut archive = Self::read_index(&mut reader)?;
        for e in archive
            .entries
            .values_mut()
            .chain(archive.case_variants.values_mut())
        {
            check(&mut reader, e, archive.length)?;
        }
        let invalid_headers = archive.members().filter(|entry| !entry.valid).count();
        if invalid_headers > 0 {
            archive.issues.push(format!(
                "{invalid_headers} archive members have missing/corrupt headers"
            ));
        }
        Ok(archive)
    }
    /// Index member names only: headers are validated per member by
    /// [`Archive::member`]. Reading a directory is sequential; every member
    /// header is a random read, so skipping unused members saves most I/O.
    pub fn read_index<R: Read + Seek>(reader: R) -> Result<Self, Error> {
        let mut reader = BufReader::new(reader);
        let length = reader.seek(SeekFrom::End(0))?;
        let mut entries = HashMap::new();
        let mut case_variants = HashMap::new();
        let mut member_count = 0;
        let mut visited = HashSet::new();
        let mut issues = Vec::new();
        directory(
            &mut reader,
            0,
            "",
            length,
            &mut visited,
            &mut entries,
            &mut case_variants,
            &mut member_count,
            &mut issues,
            0,
        )?;
        Ok(Self {
            entries,
            case_variants,
            length,
            issues,
        })
    }
    /// The entry for `name` with its header validated (read from `reader` if
    /// the archive was indexed lazily).
    pub fn member<R: Read + Seek>(
        &self,
        mut reader: R,
        name: &str,
    ) -> Result<Option<Entry>, Error> {
        let Some(e) = self.find(name) else {
            return Ok(None);
        };
        let mut e = e.clone();
        if !e.checked {
            check(&mut reader, &mut e, self.length)?;
        }
        Ok(Some(e))
    }
    /// Every distinct exact path, including case variants. Identical paths are last-wins.
    pub fn members(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values().chain(self.case_variants.values())
    }
    /// Exact case first; an absent exact spelling uses the last folded directory entry.
    pub fn find(&self, name: &str) -> Option<&Entry> {
        let name = name.replace('\\', "/");
        self.case_variants
            .get(&name)
            .or_else(|| self.entries.get(&name.to_lowercase()))
    }
    pub fn read_entry<R: Read + Seek>(&self, mut reader: R, name: &str) -> Result<Vec<u8>, Error> {
        self.read_entry_with_key(&mut reader, name, None)
    }
    pub fn read_entry_with_key<R: Read + Seek>(
        &self,
        mut reader: R,
        name: &str,
        key: Option<&dyn crate::nis::LibraryKey>,
    ) -> Result<Vec<u8>, Error> {
        let e = self
            .member(&mut reader, name)?
            .ok_or(Error::Static("Archive member not found"))?;
        if !e.valid {
            return Err(invalid(e.issue.unwrap_or("Invalid archive member")));
        }
        reader.seek(SeekFrom::Start(e.offset))?;
        let mut bytes = reader.read_bytes(e.size as usize)?;
        if e.encoded && e.key_index != 0xff {
            if e.key_index != 0x100 {
                return Err(invalid("Unsupported legacy NKX cipher"));
            }
            key.ok_or_else(|| invalid("Encrypted archive member needs local library access data"))?
                .apply(&mut bytes);
        }
        Ok(bytes)
    }
}
fn invalid(message: &str) -> Error {
    Error::Generic(message.into())
}
#[allow(clippy::too_many_arguments)] // The recursive directory walk shares its bounded index.
fn directory<R: ReadBytesExt>(
    r: &mut R,
    offset: u64,
    prefix: &str,
    length: u64,
    visited: &mut HashSet<u64>,
    entries: &mut HashMap<String, Entry>,
    case_variants: &mut HashMap<String, Entry>,
    member_count: &mut usize,
    issues: &mut Vec<String>,
    depth: usize,
) -> Result<(), Error> {
    if depth > 32 || !visited.insert(offset) {
        return Err(invalid(&format!("Invalid/cyclic NKX directory at {offset:#x} ({prefix}), depth {depth}, file length {length}")));
    }
    let available = length.saturating_sub(offset);
    if offset.checked_add(22).is_none_or(|n| n > length) {
        return Err(invalid(&format!("Truncated NKX directory header at {offset:#x} ({prefix}): need 22 bytes, available {available}, file length {length}")));
    }
    r.seek(SeekFrom::Start(offset))?;
    let magic = r.read_u32_le().map_err(|e| {
        Error::context(
            format!("NKX directory signature read at {offset:#x} ({prefix}), file length {length}"),
            e,
        )
    })?;
    if magic != 0x5e70ac54 {
        let issue = format!("Invalid NKX directory signature at {offset:#x} ({prefix}): got {magic:#010x} (little-endian), expected 0x5e70ac54, file length {length}, available {available}");
        if depth == 0 {
            return Err(invalid(&issue));
        }
        issues.push(issue);
        return Ok(());
    }
    let version = r.read_u16_le()?;
    if version != 0x110 && version != 0x111 {
        return Err(invalid(&format!("Unsupported NKX directory version {version:#x} at {offset:#x} ({prefix}), supported 0x110/0x111, file length {length}")));
    }
    r.read_u32_le()?;
    r.read_u32_le()?;
    let count = r.read_u32_le()?;
    r.read_u32_le()?;
    if count > 1_000_000 {
        return Err(invalid("Invalid NKX directory size"));
    }
    let mut children = Vec::with_capacity(count as usize);
    // Entries are contiguous after the 22-byte directory header: read them
    // all before any seek elsewhere, tracking the position without syscalls.
    let mut start = offset + 22;
    for entry in 0..count {
        if start.checked_add(8).is_none_or(|n| n > length) {
            return Err(invalid(&format!("Truncated NKX directory entry {entry}/{count} at {start:#x} ({prefix}): need 8 bytes, available {}, file length {length}", length.saturating_sub(start))));
        }
        let size = r.read_u16_le()? as u64;
        let reference = r.read_u32_le()?;
        let kind = r.read_u16_le()?;
        if size < 8 || !size.is_multiple_of(2) || start + size > length {
            return Err(invalid(&format!("Invalid NKX directory entry {entry}/{count} length {size} at {start:#x} ({prefix}), available {}, file length {length}", length.saturating_sub(start))));
        }
        // Bounded by the check above; `read_bytes` would seek and drop the read buffer.
        let mut bytes = vec![0; (size - 8) as usize];
        r.read_exact(&mut bytes)?;
        let words: Vec<_> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .take_while(|c| *c != 0)
            .collect();
        let name = String::from_utf16(&words).map_err(|_| invalid("Invalid NKX filename"))?;
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
            return Err(invalid("Invalid NKX filename component"));
        }
        let full = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        children.push((kind, reference, full));
        start += size;
    }
    for (kind, reference, full) in children {
        match kind {
            1 => directory(
                r,
                reference as u64,
                &full,
                length,
                visited,
                entries,
                case_variants,
                member_count,
                issues,
                depth + 1,
            )?,
            0 | 2 | 4 => {
                if *member_count >= 1_000_000 {
                    return Err(invalid("Excessive NKX member"));
                }
                *member_count += 1;
                let file_offset = if kind == 2 {
                    reference ^ 0x1f4e0c8d
                } else {
                    reference
                } as u64;
                let e = Entry {
                    name: full.clone(),
                    header_offset: file_offset,
                    offset: file_offset,
                    size: 0,
                    encoded: false,
                    key_index: 0xff,
                    valid: false,
                    checked: false,
                    issue: None,
                };
                case_variants.remove(&full);
                if let Some(previous) = entries.insert(full.to_lowercase(), e) {
                    if previous.name != full {
                        case_variants.insert(previous.name.clone(), previous);
                    }
                }
            }
            _ => return Err(invalid("Unknown NKX entry kind")),
        }
    }
    Ok(())
}

/// Read and validate a member header: one read of the longest header form.
fn check<R: Read + Seek>(r: &mut R, e: &mut Entry, length: u64) -> Result<(), Error> {
    let file_offset = e.header_offset;
    e.checked = true;
    e.issue = if file_offset + 22 > length {
        Some("Truncated NKX member header")
    } else {
        r.seek(SeekFrom::Start(file_offset))?;
        let mut header = [0; 31];
        let n = (length - file_offset).min(31) as usize;
        r.read_exact(&mut header[..n])?;
        let magic = u32::from_le_bytes(header[..4].try_into().unwrap());
        let version = u16::from_le_bytes(header[4..6].try_into().unwrap());
        let header_size = match magic {
            0x2ae905fa => 22,
            0x4916e63c => 27,
            0x16ccf80a => 31,
            _ => 0,
        };
        if header[..22].iter().all(|b| *b == 0) {
            Some("Zero-filled NKX member header")
        } else if header_size == 0 {
            Some("Invalid NKX member signature")
        } else if version != 0x110 && version != 0x111 {
            Some("Unsupported NKX member version")
        } else if file_offset + header_size > length {
            Some("Truncated NKX member header")
        } else {
            e.offset = file_offset + header_size;
            e.key_index = u32::from_le_bytes(header[10..14].try_into().unwrap());
            // A key hint can describe encrypted data even with the 22-byte
            // header (Solo's shared NKR), but plaintext resources are verified below.
            e.encoded = magic == 0x16ccf80a || e.key_index == 0x100;
            let at = if magic == 0x2ae905fa { 14 } else { 19 };
            e.size = u32::from_le_bytes(header[at..at + 4].try_into().unwrap()) as u64;
            if e.offset + e.size > length {
                Some("Truncated NKX member payload")
            } else {
                // Shared NKR resources also use the 22-byte header and a key
                // hint for plaintext pictures. Validate the whole resource before
                // overriding that hint; an encrypted member still needs its key.
                if magic == 0x2ae905fa && e.encoded && e.size <= 32 << 20 {
                    let png = &header[22..30] == b"\x89PNG\r\n\x1a\n";
                    let layout =
                        e.size <= 64 << 10 && e.name.to_ascii_lowercase().ends_with(".txt");
                    if png || layout {
                        r.seek(SeekFrom::Start(e.offset))?;
                        let bytes = r.read_bytes(e.size as usize)?;
                        if (png && clear_png(&bytes)) || (layout && clear_picture_layout(&bytes)) {
                            e.encoded = false;
                        }
                    }
                }
                None
            }
        }
    };
    e.valid = e.issue.is_none();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn repeated_records_cannot_bypass_the_global_member_limit() {
        let mut bytes = 0x5e70ac54u32.to_le_bytes().to_vec();
        bytes.extend(0x110u16.to_le_bytes());
        bytes.extend([0; 8]);
        bytes.extend(2u32.to_le_bytes());
        bytes.extend([0; 4]);
        for _ in 0..2 {
            bytes.extend(12u16.to_le_bytes());
            bytes.extend(80u32.to_le_bytes());
            bytes.extend(0u16.to_le_bytes());
            bytes.extend([b'a', 0, 0, 0]);
        }
        let mut entries = HashMap::new();
        let mut count = 999_999;
        let error = directory(
            &mut Cursor::new(bytes),
            0,
            "",
            46,
            &mut HashSet::new(),
            &mut entries,
            &mut HashMap::new(),
            &mut count,
            &mut Vec::new(),
            0,
        )
        .unwrap_err();
        assert!(error.to_string().contains("Excessive NKX member"));
        assert_eq!(count, 1_000_000);
        assert_eq!(entries.len(), 1);
    }
}

/// A complete plaintext PNG, including chunk framing and every CRC. A matching
/// signature alone must never cause encrypted bytes to bypass their cipher.
fn clear_png(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let (mut at, mut data) = (8usize, false);
    while let Some(header) = bytes.get(at..at + 8) {
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let Some(end) = at.checked_add(12).and_then(|n| n.checked_add(size)) else {
            return false;
        };
        let Some(chunk) = bytes.get(at + 4..end) else {
            return false;
        };
        let kind = &header[4..8];
        if !kind.iter().all(u8::is_ascii_alphabetic)
            || (at == 8 && (kind != b"IHDR" || size != 13))
            || (at != 8 && kind == b"IHDR")
        {
            return false;
        }
        let mut crc = flate2::Crc::new();
        crc.update(&chunk[..chunk.len() - 4]);
        if crc.sum() != u32::from_be_bytes(chunk[chunk.len() - 4..].try_into().unwrap()) {
            return false;
        }
        data |= kind == b"IDAT" && size > 0;
        if kind == b"IEND" {
            return size == 0 && data && end == bytes.len();
        }
        at = end;
    }
    false
}

/// A bounded picture-layout property list, not arbitrary UTF-8 or a KSP script.
fn clear_picture_layout(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let text = text.trim_start_matches('\u{feff}');
    if !text
        .bytes()
        .all(|b| b.is_ascii_graphic() || b" \t\r\n".contains(&b))
    {
        return false;
    }
    let (mut fields, mut frames, mut flags) = (HashSet::new(), false, false);
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Some((key, value)) = line.split_once(':') else {
            return false;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        let boolean = value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("no");
        let number = value.parse::<u32>().is_ok();
        if key.is_empty()
            || !key.bytes().all(|b| b.is_ascii_alphabetic() || b == b' ')
            || !fields.insert(key.clone())
            || fields.len() > 32
            || (!boolean && !number)
        {
            return false;
        }
        match key.as_str() {
            "number of animations" => {
                if !number {
                    return false;
                }
                frames = true;
            }
            "horizontal animation"
            | "horizontal resizable"
            | "vertical resizable"
            | "has alpha channel" => {
                if !boolean {
                    return false;
                }
                flags = true;
            }
            _ => {}
        }
    }
    frames && flags
}
