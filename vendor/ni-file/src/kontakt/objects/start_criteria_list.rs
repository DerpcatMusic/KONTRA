use std::io::{Cursor, SeekFrom, Write};

use crate::{
    kontakt::{objects::StartCriteriaParams, Chunk, KontaktError},
    read_bytes::ReadBytesExt,
    Error,
};

const CHUNK_ID: u16 = 0x38;

/// StartCriteriaList
///
/// Group Start Options - determines conditions for which a group
/// is triggered. Maximum of 4 conditions per group.
///
/// Type:           Chunk<Raw>
/// SerType:        ?
/// Kontakt 7:      ?
/// KontaktIO:      StartCritList
///
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StartCriteriaList {
    /// Original occupied-row mask. Retain holes when writing the list.
    pub mask: u8,
    pub items: Vec<StartCriteriaParams>,
    /// Uninterpreted bytes following the known records in the bounded list.
    #[cfg_attr(feature = "serde", serde(default))]
    pub unknown_tail: Vec<u8>,
}

impl StartCriteriaList {
    pub fn write(&self, mut writer: impl Write) -> Result<(), Error> {
        if self.mask > 15 || self.mask.count_ones() as usize != self.items.len() {
            return Err(Error::Static("Start criteria mask does not match records"));
        }
        writer.write_all(&[self.mask])?;
        for item in &self.items {
            writer.write_all(&[0, 0x70, 0])?;
            item.write(&mut writer)?;
        }
        writer.write_all(&self.unknown_tail)?;
        Ok(())
    }

    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let num_items = reader.read_i8()?;
        let mut items = Vec::new();

        if !(0..=15).contains(&num_items) {
            return Err(Error::Static("Invalid start criteria mask"));
        }

        for i in 0..4 {
            if num_items & (1 << (i & 0x1F)) != 0 {
                // ensure raw data
                match reader.read_u8()? {
                    0 => {}
                    1 => return Err(Error::Static("Unexpected structured start criteria")),
                    _ => return Err(Error::Static("Invalid start criteria object flag")),
                }

                // ensure startcriteria v70
                let version = reader.read_u16_le()?;
                if version != 0x70 {
                    return Err(Error::Static("Unsupported start criteria version"));
                }

                let item = StartCriteriaParams::read(&mut reader)?;
                items.push(item);
            }
        }

        let position = reader.stream_position()?;
        let end = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(position))?;
        let remaining = usize::try_from(
            end.checked_sub(position)
                .ok_or(Error::Static("Invalid start criteria cursor"))?,
        )
        .map_err(|_| Error::Static("Start criteria tail exceeds address space"))?;
        let unknown_tail = reader.read_bytes(remaining)?;
        Ok(Self {
            mask: num_items as u8,
            items,
            unknown_tail,
        })
    }
}

impl std::convert::TryFrom<&Chunk> for StartCriteriaList {
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
