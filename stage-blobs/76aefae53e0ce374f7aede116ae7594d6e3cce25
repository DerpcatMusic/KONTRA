use std::io::Cursor;

use crate::{
    Error,
    kontakt::{Chunk, KontaktError, StructuredObject, objects::Loop},
    read_bytes::ReadBytesExt,
};

/// Type:           Chunk
/// SerType:        0x39
/// Kontakt 7:      array<BLoop>
/// KontaktIO:      LoopArray
#[derive(Debug, Clone, PartialEq)]
pub struct LoopArray {
    pub mask: u8,
    pub items: Vec<Loop>,
    /// Serialized slot of each occupied entry; holes must not renumber loops.
    pub slots: Vec<u8>,
}

impl LoopArray {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let mask = reader.read_u8()?;
        let mut items = Vec::new();
        let mut slots = Vec::new();

        for slot in 0..8 {
            if mask & (1 << slot) == 0 {
                continue;
            }
            slots.push(slot);
            let start = reader.stream_position()?;
            let structured = match reader.read_u8()? {
                0 => false,
                1 => true,
                _ => return Err(Error::Static("Invalid loop object flag")),
            };
            let version = reader.read_u16_le()?;
            if version != 0x60 {
                return Err(Error::Generic(format!(
                    "Unsupported loop version {version:x}"
                )));
            }
            if structured {
                reader.seek(std::io::SeekFrom::Start(start))?;
                let so = StructuredObject::read(&mut reader)?;
                items.push(Loop::read(Cursor::new(&so.public_data))?);
            } else {
                items.push(Loop::read(&mut reader)?);
            }
        }

        Ok(Self { mask, items, slots })
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
