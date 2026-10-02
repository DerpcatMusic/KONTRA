use std::io::{Cursor, Read, Write};

use super::BParamArrayBParFX8;
use crate::{Error, kontakt::Chunk};

/// Compact group snapshot v2. Group IDs are the enclosing array indices.
/// Unknown public/source values and the trailing flag are retained verbatim.
#[derive(Debug)]
pub struct GroupSnapshot {
    pub public_data: [u8; 24],
    pub source_data: [u8; 32],
    pub fx: BParamArrayBParFX8,
    pub internal: BParamArrayBParFX8,
    pub external: BParamArrayBParFX8,
    pub trailing_flag: u8,
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
    for slot in &array.items {
        writer.write_all(&[u8::from(slot.is_some())])?;
        if let Some(chunk) = slot {
            chunk.write(&mut *writer)?;
        }
    }
    Ok(())
}

impl GroupSnapshot {
    pub(crate) fn read(reader: &mut Cursor<&[u8]>) -> Result<Self, Error> {
        if bytes::<3>(reader)? != [0, 2, 0] {
            return Err(Error::Static("Unsupported compact group snapshot version"));
        }
        let public_data = bytes(reader)?;
        let fx = array(reader, 8, 0x25)?;
        let source_data = bytes(reader)?;
        if source_data[..3] != [0, 2, 1] {
            return Err(Error::Static("Unsupported group snapshot source version"));
        }
        let internal = array(reader, 16, 0x0d)?;
        let external = array(reader, 32, 0x0c)?;
        let trailing_flag = bytes::<1>(reader)?[0];
        if trailing_flag > 1 {
            return Err(Error::Static("Invalid group snapshot trailing flag"));
        }
        Ok(Self {
            public_data,
            source_data,
            fx,
            internal,
            external,
            trailing_flag,
        })
    }

    /// Write the decoded record without modifying unknown state or slot data.
    pub fn write(&self, mut writer: impl Write) -> Result<(), Error> {
        for (array, count, id) in [
            (&self.fx, 8, 0x25),
            (&self.internal, 16, 0x0d),
            (&self.external, 32, 0x0c),
        ] {
            if array.items.len() != count
                || !matches!(array.version, 0x10 | 0x12)
                || array.items.iter().flatten().any(|c| c.id != id)
            {
                return Err(Error::Static("Invalid group snapshot slot shape"));
            }
        }
        if self.source_data[..3] != [0, 2, 1] || self.trailing_flag > 1 {
            return Err(Error::Static("Invalid group snapshot opaque state"));
        }
        writer.write_all(&[0, 2, 0])?;
        writer.write_all(&self.public_data)?;
        write_array(&self.fx, &mut writer)?;
        writer.write_all(&self.source_data)?;
        write_array(&self.internal, &mut writer)?;
        write_array(&self.external, &mut writer)?;
        writer.write_all(&[self.trailing_flag])?;
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
    fn compact_group_roundtrip_preserves_opaque_fields_and_rejects_bad_bounds() {
        let empty = |count| BParamArrayBParFX8 {
            version: 0x12,
            items: (0..count).map(|_| None).collect(),
        };
        let mut record = GroupSnapshot {
            public_data: [0x39; 24],
            source_data: [0x42; 32],
            fx: empty(8),
            internal: empty(16),
            external: empty(32),
            trailing_flag: 1,
        };
        record.source_data[..3].copy_from_slice(&[0, 2, 1]);
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
            assert_eq!(native.source_state().unwrap(), record.source_data);
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
