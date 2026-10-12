use std::io::Cursor;

use crate::{
    kontakt::{error::KontaktError, Chunk},
    read_bytes::ReadBytesExt,
    Error,
};

const CHUNK_ID: u16 = 0x2b;

/// An inline v0x60 voice-limit override, or the body of a 0x2b chunk.
#[derive(Debug)]
pub struct VoiceGroup {
    pub voice_limit: super::VoiceLimit,
}

impl VoiceGroup {
    pub fn read<R: ReadBytesExt>(reader: R) -> Result<Self, Error> {
        Ok(Self {
            voice_limit: super::VoiceLimit::read_inline(reader)?,
        })
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
        let mut reader = Cursor::new(&chunk.data);
        let group = Self::read(&mut reader)?;
        if reader.position() != chunk.data.len() as u64 {
            return Err(Error::Static("Trailing VoiceGroup chunk data"));
        }
        Ok(group)
    }
}
