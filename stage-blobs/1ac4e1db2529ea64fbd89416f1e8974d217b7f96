use std::io::{Cursor, Write};

use crate::{
    kontakt::{Chunk, KontaktError, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

const CHUNK_ID: u16 = 0x0F;

/// Type:           Chunk
/// SerType:        0x0F
/// Version:        0x70
/// Kontakt 7:      BParStartCriteria
/// KontaktIO:      K4PL_StartCriteria
#[derive(Debug)]
pub struct StartCriteria(pub StructuredObject);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StartCriteriaParams {
    /// Mode: Always, Start On Key, Start On Controller, Cycle Round Robin, Cycle Random, Slice Trigger
    pub mode: i32,
    pub next_criteria: i32,
    pub key_min: i16,
    pub key_max: i16,
    pub controller: i16,
    pub cc_min: i16,
    pub cc_max: i16,
    pub cycle_class: i32,
    pub slice_zone_idx: i32,
    pub slice_zone_slice_idx: i32,
    pub sequencer_only: bool,
}

impl StartCriteriaParams {
    /// Write the fixed record without interpreting native mode/operator IDs.
    pub fn write(&self, mut writer: impl Write) -> Result<(), Error> {
        for value in [self.mode, self.next_criteria] {
            writer.write_all(&value.to_le_bytes())?;
        }
        for value in [self.key_min, self.key_max, self.controller, self.cc_min, self.cc_max] {
            writer.write_all(&value.to_le_bytes())?;
        }
        for value in [self.cycle_class, self.slice_zone_idx, self.slice_zone_slice_idx] {
            writer.write_all(&value.to_le_bytes())?;
        }
        writer.write_all(&[u8::from(self.sequencer_only)])?;
        Ok(())
    }

    // 31 bytes (the list's object flag and version add another 3 bytes).
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        Ok(StartCriteriaParams {
            mode: reader.read_i32_le()?,
            next_criteria: reader.read_i32_le()?,
            key_min: reader.read_i16_le()?,
            key_max: reader.read_i16_le()?,
            controller: reader.read_i16_le()?,
            cc_min: reader.read_i16_le()?,
            cc_max: reader.read_i16_le()?,
            cycle_class: reader.read_i32_le()?,
            slice_zone_idx: reader.read_i32_le()?,
            slice_zone_slice_idx: reader.read_i32_le()?,
            sequencer_only: match reader.read_u8()? {
                0 => false,
                1 => true,
                _ => return Err(Error::Static("Invalid start criteria sequencer flag")),
            },
        })
    }
}

impl StartCriteria {
    pub fn params(&self) -> Result<StartCriteriaParams, Error> {
        StartCriteriaParams::read(&mut Cursor::new(&self.0.public_data))
    }
}

impl std::convert::TryFrom<&Chunk> for StartCriteria {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != CHUNK_ID {
            return Err(KontaktError::IncorrectID {
                expected: CHUNK_ID,
                got: chunk.id,
            }
            .into());
        }
        Ok(Self(chunk.try_into()?))
    }
}
