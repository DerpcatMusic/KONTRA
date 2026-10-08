use std::io::Cursor;

use crate::{
    kontakt::{objects::Loop, Chunk, KontaktError, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

/// Type:           Chunk
/// SerType:        0x39
/// Kontakt 7:      array<BLoop>
/// KontaktIO:      LoopArray
#[derive(Debug)]
pub struct LoopArray {
    pub items: Vec<Loop>,
}

impl LoopArray {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let mask = reader.read_u8()?;
        let mut items = Vec::new();

        for slot in 0..8 {
            if mask & (1 << slot) == 0 { continue; }
            let start = reader.stream_position()?;
            let structured = reader.read_bool()?;
            let version = reader.read_u16_le()?;
            if version != 0x60 { return Err(Error::Generic(format!("Unsupported loop version {version:x}"))); }
            if structured {
                reader.seek(std::io::SeekFrom::Start(start))?;
                let so = StructuredObject::read(&mut reader)?;
                items.push(Loop::read(Cursor::new(&so.public_data))?);
            } else {
                items.push(Loop::read(&mut reader)?);
            }
        }

        Ok(Self { items })
    }
}

impl std::convert::TryFrom<&Chunk> for LoopArray {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != 0x39 {
            return Err(KontaktError::IncorrectID {
                expected: 0x39,
                got: chunk.id,
            }
            .into());
        }
        let reader = Cursor::new(&chunk.data);
        Self::read(reader)
    }
}
