use std::io::Cursor;

use crate::{
    kontakt::{error::KontaktError, Chunk, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

const CHUNK_ID: u16 = 0x2b;

#[derive(Debug)]
pub struct VoiceGroup(pub StructuredObject);

#[derive(Debug)]
pub struct VoiceGroupParams {
    pub name: String,
    pub kill_mode: u16,
    pub prefer_released: u8,
    pub max_voices: u32,
    pub fade_ms: u32,
    pub exclusion_group: i32,
}

impl VoiceGroup {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        if reader.read_u8()? != 0 {
            return Err(Error::Static("Unsupported structured VoiceGroup format"));
        }
        Ok(Self(StructuredObject {
            version: reader.read_u16_le()?,
            public_data: reader.read_all()?,
            private_data: Vec::new(),
            children: Vec::new(),
        }))
    }

    pub fn params(&self) -> Result<VoiceGroupParams, Error> {
        if self.0.version != 0x60 {
            return Err(Error::VersionMismatch { expected: 0x60, got: self.0.version.into() });
        }
        if !self.0.private_data.is_empty() || !self.0.children.is_empty() {
            return Err(Error::Static("Unsupported structured VoiceGroup state"));
        }
        let mut reader = Cursor::new(&self.0.public_data);
        let params = VoiceGroupParams {
            name: reader.read_widestring_utf16()?,
            kill_mode: reader.read_u16_le()?,
            prefer_released: reader.read_u8()?,
            max_voices: reader.read_u32_le()?,
            fade_ms: reader.read_u32_le()?,
            exclusion_group: reader.read_i32_le()?,
        };
        if reader.position() != self.0.public_data.len() as u64 {
            return Err(Error::Static("Unsupported VoiceGroup trailing data"));
        }
        Ok(params)
    }
}

impl std::convert::TryFrom<&Chunk> for VoiceGroup {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != CHUNK_ID {
            return Err(KontaktError::IncorrectID {
                expected: CHUNK_ID,
                got: chunk.id,
            }
            .into());
        }
        let reader = Cursor::new(&chunk.data);
        Self::read(reader)
    }
}
