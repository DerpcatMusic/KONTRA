use std::io::{Cursor, Write};

use crate::{
    Error,
    kontakt::{Chunk, StructuredObject},
    read_bytes::ReadBytesExt,
};

/// One packed flag followed by three opaque numeric fields.
#[derive(Debug, Clone, PartialEq)]
pub struct LfoRecord {
    pub flag: bool,
    pub values: [f32; 3],
}

/// BParLFO (0x08), version 0x71, public layout 5.
/// Field positions are decoded; frequency units, waveform order and flag
/// meanings are deliberately unspecified. Unknown numeric bits are retained.
#[derive(Debug, Clone, PartialEq)]
pub struct Lfo {
    /// The enclosing StructuredObject serialization flag, not an audio mode.
    pub structured: bool,
    pub layout_id: u32,
    pub initial_values: [f32; 4],
    pub records: [LfoRecord; 2],
    pub trailing_flag: bool,
    pub trailing_values: [f32; 5],
}

fn values<const N: usize>(reader: &mut Cursor<&[u8]>) -> Result<[f32; N], Error> {
    let mut values = [0.0; N];
    for value in &mut values {
        *value = reader.read_f32_le()?;
    }
    Ok(values)
}

fn flag(reader: &mut Cursor<&[u8]>) -> Result<bool, Error> {
    match reader.read_u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::Static("Invalid packed LFO flag")),
    }
}

impl Lfo {
    /// Unknown versions/layouts remain opaque to the internal-modulator reader.
    pub(crate) fn read_supported(chunk: &Chunk) -> Result<Option<Self>, Error> {
        if chunk.id != 0x08 {
            return Err(Error::Static("Expected LFO chunk"));
        }
        if !matches!(chunk.data.first(), Some(0 | 1)) {
            return Err(Error::Static("Invalid LFO serialization flag"));
        }
        let mut reader = Cursor::new(chunk.data.as_slice());
        let object = StructuredObject::read(&mut reader)?;
        if reader.position() as usize != chunk.data.len() {
            return Err(Error::Static("Trailing LFO object data"));
        }
        if object.version != 0x71 || !object.private_data.is_empty() || !object.children.is_empty()
        {
            return Ok(None);
        }
        let mut reader = Cursor::new(object.public_data.as_slice());
        let layout_id = reader.read_u32_le()?;
        if layout_id != 5 {
            return Ok(None);
        }
        if object.public_data.len() != 67 {
            return Err(Error::Static("Invalid LFO layout 5 payload length"));
        }
        Ok(Some(Self {
            structured: chunk.data[0] == 1,
            layout_id,
            initial_values: values(&mut reader)?,
            records: [
                LfoRecord {
                    flag: flag(&mut reader)?,
                    values: values(&mut reader)?,
                },
                LfoRecord {
                    flag: flag(&mut reader)?,
                    values: values(&mut reader)?,
                },
            ],
            trailing_flag: flag(&mut reader)?,
            trailing_values: values(&mut reader)?,
        }))
    }

    /// Encode the known layout without interpreting or normalizing its values.
    pub fn to_chunk(&self) -> Result<Chunk, Error> {
        if self.layout_id != 5 {
            return Err(Error::Static("Unsupported LFO layout"));
        }
        let mut data = Vec::new();
        data.try_reserve_exact(if self.structured { 82 } else { 70 })
            .map_err(|_| Error::Static("LFO allocation failed"))?;
        data.extend_from_slice(&[u8::from(self.structured), 0x71, 0]);
        if self.structured {
            data.extend_from_slice(&0u32.to_le_bytes());
            data.extend_from_slice(&67u32.to_le_bytes());
        }
        data.extend_from_slice(&self.layout_id.to_le_bytes());
        for value in self.initial_values {
            data.extend_from_slice(&value.to_le_bytes());
        }
        for record in &self.records {
            data.push(u8::from(record.flag));
            for value in record.values {
                data.extend_from_slice(&value.to_le_bytes());
            }
        }
        data.push(u8::from(self.trailing_flag));
        for value in self.trailing_values {
            data.extend_from_slice(&value.to_le_bytes());
        }
        if self.structured {
            data.extend_from_slice(&0u32.to_le_bytes());
        }
        Ok(Chunk { id: 0x08, data })
    }

    /// Write the complete chunk, including its existing object envelope.
    pub fn write(&self, writer: impl Write) -> Result<(), Error> {
        self.to_chunk()?.write(writer)
    }
}

impl TryFrom<&Chunk> for Lfo {
    type Error = Error;
    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        Self::read_supported(chunk)?.ok_or(Error::Static("Unsupported LFO version/layout"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_lfo_roundtrip_preserves_unknown_bits_and_rejects_bad_bounds() {
        // Independently authored packed bytes, including signed zero and NaN.
        let mut public = vec![0; 67];
        public[..4].copy_from_slice(&5u32.to_le_bytes());
        for (i, offset) in [4, 8, 12, 16, 21, 25, 29, 34, 38, 42, 47, 51, 55, 59, 63]
            .into_iter()
            .enumerate()
        {
            public[offset..offset + 4]
                .copy_from_slice(&[0x80000000u32, 0x7fc01234, 0x3f800000][i % 3].to_le_bytes());
        }
        public[20] = 1;
        public[33] = 0;
        public[46] = 1;
        for structured in [false, true] {
            let mut data = vec![u8::from(structured), 0x71, 0];
            if structured {
                data.extend_from_slice(&[0, 0, 0, 0, 67, 0, 0, 0]);
            }
            let public_offset = data.len();
            data.extend_from_slice(&public);
            if structured {
                data.extend_from_slice(&[0; 4]);
            }
            let original = Chunk { id: 8, data };
            let lfo = Lfo::try_from(&original).unwrap();
            assert_eq!(lfo.initial_values[0].to_bits(), 0x80000000);
            assert_eq!(lfo.initial_values[1].to_bits(), 0x7fc01234);
            assert!(lfo.records[0].flag && !lfo.records[1].flag && lfo.trailing_flag);
            let mut expected = Vec::new();
            original.write(&mut expected).unwrap();
            let mut rewritten = Vec::new();
            lfo.write(&mut rewritten).unwrap();
            assert_eq!(rewritten, expected);
            for end in 0..original.data.len() {
                assert!(
                    Lfo::try_from(&Chunk {
                        id: 8,
                        data: original.data[..end].to_vec()
                    })
                    .is_err()
                );
            }
            for offset in [20, 33, 46] {
                let mut data = original.data.clone();
                data[public_offset + offset] = 2;
                assert!(Lfo::try_from(&Chunk { id: 8, data }).is_err());
            }
            let mut data = original.data.clone();
            data.push(0);
            assert!(Lfo::try_from(&Chunk { id: 8, data }).is_err());
            let mut data = original.data.clone();
            data[1] = 0x70;
            assert!(
                Lfo::read_supported(&Chunk { id: 8, data })
                    .unwrap()
                    .is_none()
            );
            let mut data = original.data;
            data[public_offset] = 6;
            assert!(
                Lfo::read_supported(&Chunk { id: 8, data })
                    .unwrap()
                    .is_none()
            );
        }
    }
}
