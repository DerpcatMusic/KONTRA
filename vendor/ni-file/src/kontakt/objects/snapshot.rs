use std::io::Cursor;

use super::BParamArrayBParFX8;
use crate::{
    kontakt::{Chunk, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

/// Kontakt snapshot v1: saved state for an existing instrument, without zones
/// or sample mappings. The compact group records remain opaque.
#[derive(Debug)]
pub struct Snapshot {
    pub groups: Chunk,
    pub group_count: u32,
    pub effect_children: Vec<Chunk>,
    /// All five Kontakt script slots, including empty/bypassed slots.
    pub persistent: Vec<Vec<String>>,
}

impl TryFrom<&Chunk> for Snapshot {
    type Error = Error;
    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        if chunk.id != 0x4f {
            return Err(Error::Static("Expected Kontakt snapshot"));
        }
        let object = StructuredObject::try_from(chunk)?;
        if object.version != 1 || !object.private_data.is_empty() || !object.children.is_empty() {
            return Err(Error::Static(
                "Unsupported Kontakt snapshot structure/version",
            ));
        }
        let mut reader = Cursor::new(object.public_data.as_slice());
        let groups = Chunk::read(&mut reader)?;
        if groups.id != 0x33 {
            return Err(Error::Static("Snapshot has no group list"));
        }
        let group_count = Cursor::new(&groups.data).read_u32_le()?;
        if group_count as usize > groups.data.len().saturating_sub(4) / 3 {
            return Err(Error::Static("Invalid snapshot group count"));
        }
        let mut effect_children = Vec::new();
        // Snapshot v1 serializes instrument inserts, sends, then 16 buses.
        for _ in 0..2 {
            let start = reader.position() as usize;
            BParamArrayBParFX8::read(&mut reader, 8)?;
            let end = reader.position() as usize;
            effect_children.push(Chunk {
                id: 0x3a,
                data: object.public_data[start..end].to_vec(),
            });
        }
        for _ in 0..16 {
            let start = reader.position() as usize;
            let bus = StructuredObject::read(&mut reader)?;
            if bus.version != 0x11 {
                return Err(Error::Static("Unsupported snapshot bus version"));
            }
            let end = reader.position() as usize;
            effect_children.push(Chunk {
                id: 0x45,
                data: object.public_data[start..end].to_vec(),
            });
        }
        let mut persistent = Vec::new();
        for _ in 0..5 {
            let count = reader.read_u32_le()? as usize;
            let remaining = object.public_data.len() - reader.position() as usize;
            if count > remaining / 4 {
                return Err(Error::Static("Invalid snapshot persistence count"));
            }
            let mut entries = Vec::new();
            entries
                .try_reserve(count)
                .map_err(|_| Error::Static("Snapshot persistence allocation failed"))?;
            for _ in 0..count {
                let length = reader.read_u32_le()? as usize;
                let bytes = reader.read_bytes(length)?;
                entries.push(
                    String::from_utf8(bytes)
                        .map_err(|_| Error::Static("Invalid snapshot persistence UTF-8"))?,
                );
            }
            persistent.push(entries);
        }
        if reader.position() as usize != object.public_data.len() {
            return Err(Error::Static("Unsupported trailing Kontakt snapshot state"));
        }
        Ok(Self {
            groups,
            group_count,
            effect_children,
            persistent,
        })
    }
}

/// Snapshot metadata v1 names the required base instrument; it is not a path.
pub fn snapshot_instrument_name(chunk: &Chunk) -> Result<String, Error> {
    if chunk.id != 0x51 {
        return Err(Error::Static("Expected Kontakt snapshot metadata"));
    }
    let object = StructuredObject::try_from(chunk)?;
    if object.version != 1 {
        return Err(Error::Static("Unsupported snapshot metadata version"));
    }
    let mut reader = Cursor::new(&object.public_data);
    if reader.read_u32_le()? != 0 {
        return Err(Error::Static("Unsupported snapshot metadata flags"));
    }
    let name = reader.read_widestring_utf16()?;
    let _library_name = reader.read_widestring_utf16()?;
    if reader.position() as usize != object.public_data.len() || name.is_empty() {
        return Err(Error::Static("Invalid snapshot metadata"));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_snapshot_boundaries_and_rejects_damaged_state() {
        // Authored zero-group snapshot: empty racks, buses and four scripts;
        // slot 2 contains numeric and text persistence, including a newline.
        let mut body = Vec::new();
        Chunk {
            id: 0x33,
            data: 0u32.to_le_bytes().to_vec(),
        }
        .write(&mut body)
        .unwrap();
        for _ in 0..2 {
            body.extend([0, 0x12, 0]);
            body.extend([0; 8]);
        }
        for _ in 0..16 {
            body.extend([1, 0x11, 0]);
            body.extend([0; 12]);
        }
        for slot in 0..5 {
            let entries: &[&str] = if slot == 2 {
                &["$level 17", "@label two\nlines"]
            } else {
                &[]
            };
            body.extend((entries.len() as u32).to_le_bytes());
            for entry in entries {
                body.extend((entry.len() as u32).to_le_bytes());
                body.extend(entry.as_bytes());
            }
        }
        let mut data = vec![0, 1, 0];
        data.extend(&body);
        let mut chunk = Chunk { id: 0x4f, data };
        let parsed = Snapshot::try_from(&chunk).unwrap();
        assert_eq!(parsed.group_count, 0);
        assert_eq!(parsed.effect_children.len(), 18);
        assert_eq!(parsed.persistent[2], ["$level 17", "@label two\nlines"]);
        for length in 0..chunk.data.len() {
            assert!(
                Snapshot::try_from(&Chunk {
                    id: chunk.id,
                    data: chunk.data[..length].to_vec()
                })
                .is_err(),
                "truncation {length}"
            );
        }
        chunk.data.push(0);
        assert!(Snapshot::try_from(&chunk).is_err());
        chunk.data.pop();
        let count_offset = 3 + 10 + 22 + 16 * 15 + 8;
        chunk.data[count_offset..count_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Snapshot::try_from(&chunk).is_err());
    }
}
