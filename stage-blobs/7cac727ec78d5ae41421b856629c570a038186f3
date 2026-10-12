use std::io::{Cursor, Read, Write};

use super::{BParSrcMode, BParamArrayBParFX8};
use crate::read_bytes::ReadBytesExt;
use crate::{kontakt::Chunk, Error};

/// Compact group snapshot v1..=v4. Group IDs are the enclosing array indices.
/// Unknown public/source values and the trailing flag are retained verbatim.
#[derive(Debug)]
pub struct GroupSnapshot {
    pub version: u16,
    pub public_data: [u8; 24],
    pub source_data: Vec<u8>,
    pub fx: BParamArrayBParFX8,
    pub internal: BParamArrayBParFX8,
    pub external: BParamArrayBParFX8,
    pub trailing_flag: u8,
    /// v4 appends eight opaque bytes after the selection flag.
    pub trailing_data: Vec<u8>,
}

fn bytes<const N: usize>(reader: &mut impl Read) -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn array(reader: &mut Cursor<&[u8]>, count: u32, id: u16) -> Result<BParamArrayBParFX8, Error> {
    if reader.get_ref().get(reader.position() as usize) != Some(&0) {
        return Err(Error::Static("Unsupported structured group snapshot array"));
    }
    let array = BParamArrayBParFX8::read(reader, count)?;
    if array.items.iter().flatten().any(|c| c.id != id) {
        return Err(Error::Static("Incorrect group snapshot slot object"));
    }
    Ok(array)
}

fn write_array(array: &BParamArrayBParFX8, writer: &mut impl Write) -> Result<(), Error> {
    writer.write_all(&[0])?;
    writer.write_all(&array.version.to_le_bytes())?;
    if array.version == 0x13 {
        writer.write_all(&(array.items.len() as u32).to_le_bytes())?;
    }
    for slot in &array.items {
        writer.write_all(&[u8::from(slot.is_some())])?;
        if let Some(chunk) = slot {
            chunk.write(&mut *writer)?;
        }
    }
    Ok(())
}

// Native compact v4/mode9 ends at 99 bytes; other records retain two extra bytes.
fn source_data(reader: &mut Cursor<&[u8]>, version: u16) -> Result<Vec<u8>, Error> {
    let start = reader.position() as usize;
    let source = BParSrcMode::read(&mut *reader)?;
    if !(version == 4 && source.version == 0x106 && source.mode == 9) {
        bytes::<2>(reader)?;
    }
    Ok(reader.get_ref()[start..reader.position() as usize].to_vec())
}

impl GroupSnapshot {
    pub(crate) fn read(reader: &mut Cursor<&[u8]>) -> Result<Self, Error> {
        let header = bytes::<3>(reader)?;
        if header[0] != 0 || header[2] != 0 || !(1..=4).contains(&header[1]) {
            return Err(Error::Static("Unsupported compact group snapshot version"));
        }
        let version = u16::from_le_bytes([header[1], header[2]]);
        let public_data = bytes(reader)?;
        let fx = array(reader, 8, 0x25)?;
        let at = reader.position();
        let source_data = source_data(reader, version).map_err(|error| {
            Error::context(format!("Compact group source at offset {at}"), error)
        })?;
        let internal = array(reader, 16, 0x0d)?;
        let mut header = reader.clone();
        header.read_u8()?;
        let external_count = if header.read_u16_le()? == 0x13 {
            header.read_u32_le()?
        } else {
            32
        };
        if !matches!(external_count, 32 | 64) {
            return Err(Error::Static(
                "Unsupported group snapshot external slot count",
            ));
        }
        let external = array(reader, external_count, 0x0c)?;
        let trailing_flag = if version >= 2 {
            bytes::<1>(reader)?[0]
        } else {
            0
        };
        if trailing_flag > 1 {
            return Err(Error::Static("Invalid group snapshot trailing flag"));
        }
        let trailing_data = if version == 4 {
            bytes::<8>(reader)?.to_vec()
        } else {
            Vec::new()
        };
        Ok(Self {
            version,
            public_data,
            source_data,
            fx,
            internal,
            external,
            trailing_flag,
            trailing_data,
        })
    }

    /// Write the decoded record without modifying unknown state or slot data.
    pub fn write(&self, mut writer: impl Write) -> Result<(), Error> {
        if !matches!(self.version, 1..=4)
            || !matches!(self.external.items.len(), 32 | 64)
            || self.external.version != 0x13 && self.external.items.len() != 32
        {
            return Err(Error::Static("Invalid group snapshot version/capacity"));
        }
        for (array, count, id) in [
            (&self.fx, 8, 0x25),
            (&self.internal, 16, 0x0d),
            (&self.external, self.external.items.len(), 0x0c),
        ] {
            if array.items.len() != count
                || !matches!(array.version, 0x10..=0x13)
                || array.items.iter().flatten().any(|c| c.id != id)
            {
                return Err(Error::Static("Invalid group snapshot slot shape"));
            }
        }
        let mut source = Cursor::new(self.source_data.as_slice());
        source_data(&mut source, self.version)?;
        if source.position() as usize != self.source_data.len()
            || self.trailing_data.len() != if self.version == 4 { 8 } else { 0 }
            || self.trailing_flag > 1
            || self.version == 1 && self.trailing_flag != 0
        {
            return Err(Error::Static("Invalid group snapshot opaque state"));
        }
        writer.write_all(&[0])?;
        writer.write_all(&self.version.to_le_bytes())?;
        writer.write_all(&self.public_data)?;
        write_array(&self.fx, &mut writer)?;
        writer.write_all(&self.source_data)?;
        write_array(&self.internal, &mut writer)?;
        write_array(&self.external, &mut writer)?;
        if self.version >= 2 {
            writer.write_all(&[self.trailing_flag])?;
        }
        writer.write_all(&self.trailing_data)?;
        Ok(())
    }

    /// Standard modulator array chunks accepted by the existing group reader.
    pub fn modulation_chunks(&self) -> Result<[Chunk; 2], Error> {
        let mut internal = Vec::new();
        write_array(&self.internal, &mut internal)?;
        let mut external = Vec::new();
        write_array(&self.external, &mut external)?;
        Ok([
            Chunk {
                id: 0x3b,
                data: internal,
            },
            Chunk {
                id: 0x3c,
                data: external,
            },
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_v4_preserves_mode_dependent_sources_and_counted_slots() {
        let empty = |count| BParamArrayBParFX8 {
            version: 0x13,
            items: (0..count).map(|_| None).collect(),
        };
        for (mode, length) in [(3u32, 32), (9, 99)] {
            let mut source_data = vec![0x42; length];
            source_data[..3].copy_from_slice(&[0, 6, 1]);
            source_data[3..7].copy_from_slice(&mode.to_le_bytes());
            for at in [11, 16, 29] {
                source_data[at] = 0;
            }
            if mode == 9 {
                source_data[54] = 0;
            }
            let mut record = GroupSnapshot {
                version: 4,
                public_data: [0x39; 24],
                source_data,
                fx: empty(8),
                internal: empty(16),
                external: empty(64),
                trailing_flag: 1,
                trailing_data: vec![0x57; 8],
            };
            record.external.items[63] = Some(Chunk {
                id: 0x0c,
                data: b"authored opaque high assignment".to_vec(),
            });
            let mut original = Vec::new();
            record.write(&mut original).unwrap();
            let decoded = GroupSnapshot::read(&mut Cursor::new(original.as_slice())).unwrap();
            assert_eq!(decoded.source_data.len(), length);
            assert_eq!(decoded.external.items.len(), 64);
            assert!(decoded.external.items[63].is_some());
            let mut rewritten = Vec::new();
            decoded.write(&mut rewritten).unwrap();
            assert_eq!(rewritten, original);
            for end in 0..original.len() {
                assert!(GroupSnapshot::read(&mut Cursor::new(&original[..end])).is_err());
            }
            record.source_data[3..7].copy_from_slice(&10u32.to_le_bytes());
            assert!(record.write(Vec::new()).is_err());
            record.source_data[3..7].copy_from_slice(&mode.to_le_bytes());
            record.source_data.pop();
            assert!(record.write(Vec::new()).is_err());
        }
    }

    #[test]
    fn compact_group_roundtrip_preserves_opaque_fields_and_rejects_bad_bounds() {
        let empty = |count| BParamArrayBParFX8 {
            version: 0x12,
            items: (0..count).map(|_| None).collect(),
        };
        let mut record = GroupSnapshot {
            version: 2,
            public_data: [0x39; 24],
            source_data: vec![0x42; 32],
            fx: empty(8),
            internal: empty(16),
            external: empty(32),
            trailing_flag: 1,
            trailing_data: Vec::new(),
        };
        record.source_data[..3].copy_from_slice(&[0, 2, 1]);
        record.source_data[3..7].copy_from_slice(&3u32.to_le_bytes());
        for at in [11, 16, 29] {
            record.source_data[at] = 0;
        }
        record.internal.items[3] = Some(Chunk {
            id: 0x0d,
            data: b"authored opaque modulator".to_vec(),
        });
        // Native groups keep a source record after either value of this flag.
        let mut private = Vec::new();
        for _ in 0..136 {
            private.extend_from_slice(&8u32.to_le_bytes());
            private.extend_from_slice(&[0; 8]);
        }
        private.extend_from_slice(&[0; 24]);
        write_array(&record.fx, &mut private).unwrap();
        let flag_offset = private.len();
        private.push(0);
        private.extend_from_slice(&record.source_data);
        let mut native = super::super::Group(crate::kontakt::StructuredObject {
            version: 1,
            public_data: Vec::new(),
            private_data: private,
            children: Vec::new(),
        });
        for flag in [0, 1] {
            native.0.private_data[flag_offset] = flag;
            assert_eq!(
                native.source_state().unwrap().as_slice(),
                record.source_data
            );
        }
        native.0.private_data[flag_offset] = 2;
        assert!(native.source_state().is_err());
        native.0.private_data[flag_offset] = 0;
        native.0.private_data.pop();
        assert!(native.source_state().is_err());

        let mut original = Vec::new();
        record.write(&mut original).unwrap();
        let decoded = GroupSnapshot::read(&mut Cursor::new(original.as_slice())).unwrap();
        let mut rewritten = Vec::new();
        decoded.write(&mut rewritten).unwrap();
        assert_eq!(rewritten, original);
        for end in 0..original.len() {
            assert!(GroupSnapshot::read(&mut Cursor::new(&original[..end])).is_err());
        }
        let mut invalid = original.clone();
        invalid[27] = 2;
        assert!(GroupSnapshot::read(&mut Cursor::new(invalid.as_slice())).is_err());
        let mut invalid = original;
        *invalid.last_mut().unwrap() = 2;
        assert!(GroupSnapshot::read(&mut Cursor::new(invalid.as_slice())).is_err());
    }
}
