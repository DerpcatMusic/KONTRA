use std::io::Cursor;

use super::BParamArrayBParFX8;
use crate::{
    kontakt::{Chunk, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

/// Kontakt snapshot v1/v3: saved state for an existing instrument, without zones
/// or sample mappings. The compact group records remain opaque.
#[derive(Debug)]
pub struct Snapshot {
    /// Absent for v3 script-only state; the base native groups stay in use.
    pub groups: Option<Chunk>,
    pub version: u16,
    pub group_count: u32,
    pub effect_children: Vec<Chunk>,
    /// All five Kontakt script slots, including empty/bypassed slots.
    pub persistent: Vec<Vec<String>>,
}

impl Snapshot {
    /// Opt-in compact-v1..v4 decoding; IDs retain Kontakt's original array order.
    pub fn group_snapshots(&self) -> Result<Vec<(u32, super::GroupSnapshot)>, Error> {
        let Some(group_chunk) = &self.groups else {
            return Ok(Vec::new());
        };
        let mut reader = Cursor::new(group_chunk.data.as_slice());
        let count = reader.read_u32_le()?;
        // Smallest v1 record: 115 bytes with a v0x100 source and empty fixed racks.
        if count != self.group_count
            || count as usize > group_chunk.data.len().saturating_sub(4) / 115
        {
            return Err(Error::Static("Invalid compact group snapshot count"));
        }
        let mut groups = Vec::new();
        groups
            .try_reserve(count as usize)
            .map_err(|_| Error::Static("Group snapshot allocation failed"))?;
        for id in 0..count {
            let at = reader.position();
            let group = super::GroupSnapshot::read(&mut reader).map_err(|error| {
                Error::context(format!("Compact snapshot group {id} at offset {at}"), error)
            })?;
            groups.push((id, group));
        }
        if reader.position() as usize != group_chunk.data.len() {
            return Err(Error::Static("Trailing compact group snapshot data"));
        }
        Ok(groups)
    }
}

impl TryFrom<&Chunk> for Snapshot {
    type Error = Error;
    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        if chunk.id != 0x4f {
            return Err(Error::Static("Expected Kontakt snapshot"));
        }
        let object = StructuredObject::try_from(chunk)?;
        if !matches!(object.version, 1 | 3)
            || !object.private_data.is_empty()
            || !object.children.is_empty()
        {
            return Err(Error::Static(
                "Unsupported Kontakt snapshot structure/version",
            ));
        }
        let mut reader = Cursor::new(object.public_data.as_slice());
        // The verified v3 layouts use flags 0 for native+script state or 3
        // for script-only state. Other flags remain unsupported.
        let script_only = if object.version == 3 {
            let flags = reader.read_u32_le()?;
            if !matches!(flags, 0 | 3) {
                return Err(Error::Generic(format!(
                    "Unsupported Kontakt snapshot v3 native state flags {flags}; compact group/source layout is not decoded"
                )));
            }
            flags == 3
        } else {
            false
        };
        let (groups, group_count, effect_children) = if script_only {
            (None, 0, Vec::new())
        } else {
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
                if bus.version != if object.version == 3 { 0x12 } else { 0x11 } {
                    return Err(Error::Static("Unsupported snapshot bus version"));
                }
                let end = reader.position() as usize;
                effect_children.push(Chunk {
                    id: 0x45,
                    data: object.public_data[start..end].to_vec(),
                });
            }
            if object.version == 3 {
                let start = reader.position() as usize;
                BParamArrayBParFX8::read(&mut reader, 8)?;
                let end = reader.position() as usize;
                effect_children.push(Chunk {
                    id: 0x3a,
                    data: object.public_data[start..end].to_vec(),
                });
            }
            (Some(groups), group_count, effect_children)
        };
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
            version: object.version,
            groups,
            group_count,
            effect_children,
            persistent,
        })
    }
}

/// Snapshot metadata v1's first stored name; it is not a path.
pub fn snapshot_instrument_name(chunk: &Chunk) -> Result<String, Error> {
    Ok(snapshot_metadata_names(chunk)?.0)
}

/// Both stored metadata names, without substituting a path or inferring aliases.
/// Some factory snapshots retain the template name `Kontakt` in the first
/// field and their named base in the second.
pub fn snapshot_metadata_names(chunk: &Chunk) -> Result<(String, String), Error> {
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
    let content_name = reader.read_widestring_utf16()?;
    if reader.position() as usize != object.public_data.len() || name.is_empty() {
        return Err(Error::Static("Invalid snapshot metadata"));
    }
    Ok((name, content_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_script_only_v3_without_inventing_native_state() {
        let mut data = vec![0, 3, 0];
        data.extend(3u32.to_le_bytes());
        for slot in 0..5 {
            let entries: &[&str] = if slot == 2 {
                &["$level 17", "@label saved"]
            } else {
                &[]
            };
            data.extend((entries.len() as u32).to_le_bytes());
            for entry in entries {
                data.extend((entry.len() as u32).to_le_bytes());
                data.extend(entry.as_bytes());
            }
        }
        let chunk = Chunk { id: 0x4f, data };
        let saved = Snapshot::try_from(&chunk).unwrap();
        assert!(saved.groups.is_none());
        assert_eq!(saved.group_count, 0);
        assert!(saved.group_snapshots().unwrap().is_empty());
        assert!(saved.effect_children.is_empty());
        assert_eq!(saved.persistent[2], ["$level 17", "@label saved"]);
        for end in 0..chunk.data.len() {
            assert!(Snapshot::try_from(&Chunk {
                id: chunk.id,
                data: chunk.data[..end].to_vec()
            })
            .is_err());
        }
        for flags in [0u32, 1, 2, 4, u32::MAX] {
            let mut data = chunk.data.clone();
            data[3..7].copy_from_slice(&flags.to_le_bytes());
            assert!(Snapshot::try_from(&Chunk { id: chunk.id, data }).is_err());
        }
        let mut data = chunk.data;
        data.push(0);
        assert!(Snapshot::try_from(&Chunk { id: chunk.id, data }).is_err());
    }

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
