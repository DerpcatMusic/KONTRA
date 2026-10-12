use std::io::{Cursor, Write};

use crate::{
    kontakt::{Chunk, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

/// One packed flag followed by three opaque numeric fields.
#[derive(Debug, Clone, PartialEq)]
pub struct LfoRecord {
    pub flag: bool,
    pub values: [f32; 3],
}

/// BParLFO (0x08), versions 0x71..=0x73.
/// The first public integer selects the waveform, not the payload layout:
/// sine 0, rectangle 1, triangle 2, sawtooth 3, random 4, Multi 5.
/// Type 6 has the same packed fields as Multi; its name remains unverified.
/// Unknown numeric bits and the remaining unnamed flags are retained.
#[derive(Debug, Clone, PartialEq)]
pub struct Lfo {
    /// The enclosing StructuredObject serialization flag, not an audio mode.
    pub structured: bool,
    pub version: u16,
    pub waveform: u32,
    /// Delay, frequency (Hz when unsynchronized, count when synchronized),
    /// pulse width, and start phase in cycles, respectively.
    pub initial_values: [f32; 4],
    /// The first flag is normalizeMultiLFO; first values[0] is the frequency
    /// note value. The second flag belongs to that frequency sync record;
    /// second values[0] is the delay note value. This grouping preserves the
    /// packed byte order, which crosses the native sync-record boundaries.
    pub records: [LfoRecord; 2],
    /// The final flag of the delay sync record; its meaning remains unnamed.
    pub trailing_flag: bool,
    /// Multi mix levels: sine, rectangle, triangle, sawtooth, random.
    /// Simple waveform types do not serialize these fields.
    pub trailing_values: Option<[f32; 5]>,
    /// Additional flag introduced in version 0x73.
    pub additional_flag: Option<bool>,
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
        if !matches!(object.version, 0x71..=0x73)
            || !object.private_data.is_empty()
            || !object.children.is_empty()
        {
            return Ok(None);
        }
        let mut reader = Cursor::new(object.public_data.as_slice());
        let waveform = reader.read_u32_le()?;
        if waveform > 6 {
            return Ok(None);
        }
        let multi = waveform >= 5;
        let additional = object.version == 0x73;
        let public_len = 47 + usize::from(multi) * 20 + usize::from(additional);
        if object.public_data.len() != public_len {
            return Err(Error::Static("Invalid LFO waveform payload length"));
        }
        Ok(Some(Self {
            structured: chunk.data[0] == 1,
            version: object.version,
            waveform,
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
            trailing_values: if multi {
                Some(values(&mut reader)?)
            } else {
                None
            },
            additional_flag: if additional {
                Some(flag(&mut reader)?)
            } else {
                None
            },
        }))
    }

    /// Encode the known layout without interpreting or normalizing its values.
    pub fn to_chunk(&self) -> Result<Chunk, Error> {
        if !matches!(self.version, 0x71..=0x73) || self.waveform > 6 {
            return Err(Error::Static("Unsupported LFO version/waveform"));
        }
        if self.trailing_values.is_some() != (self.waveform >= 5)
            || self.additional_flag.is_some() != (self.version == 0x73)
        {
            return Err(Error::Static("LFO fields do not match version/waveform"));
        }
        let public_len = 47
            + usize::from(self.trailing_values.is_some()) * 20
            + usize::from(self.additional_flag.is_some());
        let mut data = Vec::new();
        data.try_reserve_exact(public_len + if self.structured { 15 } else { 3 })
            .map_err(|_| Error::Static("LFO allocation failed"))?;
        data.push(u8::from(self.structured));
        data.extend_from_slice(&self.version.to_le_bytes());
        if self.structured {
            data.extend_from_slice(&0u32.to_le_bytes());
            data.extend_from_slice(&(public_len as u32).to_le_bytes());
        }
        data.extend_from_slice(&self.waveform.to_le_bytes());
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
        if let Some(levels) = self.trailing_values {
            for value in levels {
                data.extend_from_slice(&value.to_le_bytes());
            }
        }
        if let Some(value) = self.additional_flag {
            data.push(u8::from(value));
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
        Self::read_supported(chunk)?.ok_or(Error::Static("Unsupported LFO version/waveform"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_lfo_roundtrip_preserves_unknown_bits_and_rejects_bad_bounds() {
        // Independently authored packed bytes, including signed zero and NaN.
        for version in 0x71u16..=0x73 {
            for waveform in 0u32..=6 {
                let public_len =
                    47 + usize::from(waveform >= 5) * 20 + usize::from(version == 0x73);
                let mut public = vec![0; public_len];
                public[..4].copy_from_slice(&waveform.to_le_bytes());
                for (i, offset) in [4, 8, 12, 16, 21, 25, 29, 34, 38, 42, 47, 51, 55, 59, 63]
                    .into_iter()
                    .enumerate()
                {
                    if offset + 4 > 47 && waveform < 5 {
                        continue;
                    }
                    public[offset..offset + 4].copy_from_slice(
                        &[0x80000000u32, 0x7fc01234, 0x3f800000][i % 3].to_le_bytes(),
                    );
                }
                public[20] = 1;
                public[33] = 0;
                public[46] = 1;
                if version == 0x73 {
                    public[public_len - 1] = 1;
                }
                for structured in [false, true] {
                    let mut data = vec![u8::from(structured)];
                    data.extend_from_slice(&version.to_le_bytes());
                    if structured {
                        data.extend_from_slice(&0u32.to_le_bytes());
                        data.extend_from_slice(&(public_len as u32).to_le_bytes());
                    }
                    let public_offset = data.len();
                    data.extend_from_slice(&public);
                    if structured {
                        data.extend_from_slice(&[0; 4]);
                    }
                    let original = Chunk { id: 8, data };
                    let lfo = Lfo::try_from(&original).unwrap();
                    assert_eq!(lfo.version, version);
                    assert_eq!(lfo.waveform, waveform);
                    assert_eq!(lfo.trailing_values.is_some(), waveform >= 5);
                    assert_eq!(lfo.additional_flag, (version == 0x73).then_some(true));
                    assert_eq!(lfo.initial_values[0].to_bits(), 0x80000000);
                    assert_eq!(lfo.initial_values[1].to_bits(), 0x7fc01234);
                    assert!(lfo.records[0].flag && !lfo.records[1].flag && lfo.trailing_flag);
                    let mut expected = Vec::new();
                    original.write(&mut expected).unwrap();
                    let mut rewritten = Vec::new();
                    lfo.write(&mut rewritten).unwrap();
                    assert_eq!(rewritten, expected);
                    for end in 0..original.data.len() {
                        assert!(Lfo::try_from(&Chunk {
                            id: 8,
                            data: original.data[..end].to_vec()
                        })
                        .is_err());
                    }
                    for offset in [20, 33, 46] {
                        let mut data = original.data.clone();
                        data[public_offset + offset] = 2;
                        assert!(Lfo::try_from(&Chunk { id: 8, data }).is_err());
                    }
                    if version == 0x73 {
                        let mut data = original.data.clone();
                        data[public_offset + public_len - 1] = 2;
                        assert!(Lfo::try_from(&Chunk { id: 8, data }).is_err());
                    }
                    // A simple waveform with a Multi-sized public payload is invalid,
                    // rather than silently ignoring twenty bytes of another schema.
                    let mut data = original.data.clone();
                    data[public_offset] = if waveform < 5 { 5 } else { 0 };
                    assert!(Lfo::try_from(&Chunk { id: 8, data }).is_err());
                    let mut data = original.data.clone();
                    data.push(0);
                    assert!(Lfo::try_from(&Chunk { id: 8, data }).is_err());
                    let mut data = original.data.clone();
                    data[1] = 0x70;
                    assert!(Lfo::read_supported(&Chunk { id: 8, data })
                        .unwrap()
                        .is_none());
                    let mut data = original.data;
                    data[public_offset] = 7;
                    assert!(Lfo::read_supported(&Chunk { id: 8, data })
                        .unwrap()
                        .is_none());
                }
            }
        }
    }
}
