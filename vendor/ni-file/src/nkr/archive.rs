//! Read-only NKX/NKR directory index with optional library-provided resource keys.
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
    /// Archive file length in bytes.
    pub length: u64,
    pub issues: Vec<String>,
}
impl Archive {
    /// Index every member and validate every member header.
    pub fn read<R: Read + Seek>(reader: R) -> Result<Self, Error> {
        let mut reader = BufReader::new(reader);
        let mut archive = Self::read_index(&mut reader)?;
        for e in archive.entries.values_mut() {
            check(&mut reader, e, archive.length)?;
        }
        let invalid_headers = archive.entries.values().filter(|entry| !entry.valid).count();
        if invalid_headers > 0 {
            archive.issues.push(format!("{invalid_headers} archive members have missing/corrupt headers"));
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
        let mut visited = HashSet::new();
        let mut issues = Vec::new();
        directory(&mut reader, 0, "", length, &mut visited, &mut entries, &mut issues, 0)?;
        Ok(Self { entries, length, issues })
    }
    /// The entry for `name` with its header validated (read from `reader` if
    /// the archive was indexed lazily).
    pub fn member<R: Read + Seek>(&self, mut reader: R, name: &str) -> Result<Option<Entry>, Error> {
        let Some(e) = self.find(name) else { return Ok(None) };
        let mut e = e.clone();
        if !e.checked {
            check(&mut reader, &mut e, self.length)?;
        }
        Ok(Some(e))
    }
    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.get(&name.replace('\\', "/").to_lowercase())
    }
    pub fn read_entry<R: Read + Seek>(&self, mut reader: R, name: &str) -> Result<Vec<u8>, Error> {
        self.read_entry_with_key(&mut reader, name, None)
    }
    pub fn read_entry_with_key<R: Read + Seek>(
        &self,
        mut reader: R,
        name: &str,
        key: Option<&crate::nis::LibraryKey>,
    ) -> Result<Vec<u8>, Error> {
        let e = self
            .find(name)
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
fn directory<R: ReadBytesExt>(
    r: &mut R,
    offset: u64,
    prefix: &str,
    length: u64,
    visited: &mut HashSet<u64>,
    entries: &mut HashMap<String, Entry>,
    issues: &mut Vec<String>,
    depth: usize,
) -> Result<(), Error> {
    if depth > 32 || !visited.insert(offset) || offset.checked_add(22).is_none_or(|n| n > length) {
        return Err(invalid("Invalid/cyclic NKX directory"));
    }
    r.seek(SeekFrom::Start(offset))?;
    if r.read_u32_le()? != 0x5e70ac54 {
        let issue = format!("Invalid NKX directory signature at {offset:#x} ({prefix})");
        if depth == 0 {
            return Err(invalid(&issue));
        }
        issues.push(issue);
        return Ok(());
    }
    let version = r.read_u16_le()?;
    if version != 0x110 && version != 0x111 {
        return Err(invalid("Unsupported NKX directory version"));
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
    for _ in 0..count {
        let size = r.read_u16_le()? as u64;
        let reference = r.read_u32_le()?;
        let kind = r.read_u16_le()?;
        if size < 8 || size % 2 != 0 || start + size > length {
            return Err(invalid("Invalid NKX entry length"));
        }
        // Bounded by the check above; `read_bytes` would seek and drop the read buffer.
        let mut bytes = vec![0; (size - 8) as usize];
        r.read_exact(&mut bytes)?;
        let words: Vec<_> = bytes
            .chunks_exact(2)
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
                issues,
                depth + 1,
            )?,
            0 | 2 | 4 => {
                if entries.len() >= 1_000_000 || entries.contains_key(&full.to_lowercase()) {
                    return Err(invalid("Duplicate or excessive NKX member"));
                }
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
                entries.insert(full.to_lowercase(), e);
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
            // Members naming the library key (0x100) are encrypted whatever their
            // header magic: Solo's shared NKR uses the 22-byte header.
            e.encoded = magic == 0x16ccf80a || e.key_index == 0x100;
            let at = if magic == 0x2ae905fa { 14 } else { 19 };
            e.size = u32::from_le_bytes(header[at..at + 4].try_into().unwrap()) as u64;
            if e.offset + e.size > length {
                Some("Truncated NKX member payload")
            } else {
                None
            }
        }
    };
    e.valid = e.issue.is_none();
    Ok(())
}
