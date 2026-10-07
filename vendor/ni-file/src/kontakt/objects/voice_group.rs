use std::io::Cursor;

use crate::{
    Error,
    kontakt::{Chunk, error::KontaktError},
    read_bytes::ReadBytesExt,
};

const CHUNK_ID: u16 = 0x2b;

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceGroup(pub super::VoiceLimit);

impl VoiceGroup {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        if reader.read_u8()? != 0 {
            return Err(Error::Static("Unsupported structured voice limit"));
        }
        let version = reader.read_u16_le()?;
        if version != 0x60 {
            return Err(Error::Generic(format!(
                "Unsupported voice limit version 0x{version:x}"
            )));
        }
        Ok(Self(super::VoiceLimit::read(reader)?))
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
