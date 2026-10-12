use std::io::{Cursor, Write};

use crate::{
    Error,
    kontakt::{Chunk, KontaktError},
    read_bytes::ReadBytesExt,
};

/// Saved BParGlide child0x0B. Unknown modes and timing bytes are retained;
/// playback admission is separate from lossless decoding.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Glide {
    pub version: u16,
    /// Native saved1=Speed, saved2=Time.
    pub mode: u32,
    /// Milliseconds when synchronization is disabled.
    pub coefficient: f32,
    /// Native synchronization is enabled only above float1e-9.
    pub sync_note_value: f32,
    /// v51 begins with two opaque f32 values and one flag; v50 has no suffix.
    pub unknown_tail: Vec<u8>,
}

impl Glide {
    fn validate(&self) -> Result<(), Error> {
        if !matches!(self.version, 0x50 | 0x51) {
            return Err(Error::Static("Unsupported BParGlide version"));
        }
        if self.version == 0x51 && self.unknown_tail.len() < 9 {
            return Err(Error::Static("Incomplete BParGlide timing metadata"));
        }
        Ok(())
    }

    pub fn to_chunk(&self) -> Result<Chunk, Error> {
        self.validate()?;
        let mut data = vec![0];
        data.extend(self.version.to_le_bytes());
        data.extend(self.mode.to_le_bytes());
        data.extend(self.coefficient.to_le_bytes());
        data.extend(self.sync_note_value.to_le_bytes());
        data.extend_from_slice(&self.unknown_tail);
        Ok(Chunk { id: 0x0B, data })
    }

    pub fn write(&self, writer: impl Write) -> Result<(), Error> {
        self.to_chunk()?.write(writer)
    }
}

impl TryFrom<&Chunk> for Glide {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != 0x0B {
            return Err(KontaktError::IncorrectID {
                expected: 0x0B,
                got: chunk.id,
            }
            .into());
        }
        let mut reader = Cursor::new(chunk.data.as_slice());
        if reader.read_u8()? != 0 {
            return Err(Error::Static("Structured BParGlide is not supported"));
        }
        let version = reader.read_u16_le()?;
        if !matches!(version, 0x50 | 0x51) {
            return Err(Error::Static("Unsupported BParGlide version"));
        }
        let glide = Self {
            version,
            mode: reader.read_u32_le()?,
            coefficient: reader.read_f32_le()?,
            sync_note_value: reader.read_f32_le()?,
            unknown_tail: reader.read_all()?,
        };
        glide.validate()?;
        Ok(glide)
    }
}
