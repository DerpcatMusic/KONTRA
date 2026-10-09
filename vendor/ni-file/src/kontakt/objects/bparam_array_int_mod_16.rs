use crate::{
    kontakt::{error::KontaktError, structured_object::StructuredObject, Chunk},
    Error,
};

use super::{modulation::read_param_slots, InternalMod};

const CHUNK_ID: u16 = 0x3B;
const SLOTS: usize = 16;

/// BParameterArraySerBParInternalMod16
///
/// Sixteen optional internal modulator slots of a group.
///
/// Type:           Chunk<StructuredObject>
/// SerType:        0x3B
/// Versions:       0x10, 0x11, 0x12, 0x13
/// Kontakt 7:      BParameterArraySerBParInternalMod16
/// KontaktIO:      BParamArray<16>
#[derive(Debug)]
pub struct InternalModArray16(pub StructuredObject);

impl InternalModArray16 {
    /// Occupied slots as `(slot index, modulator)`.
    pub fn slots(&self) -> Result<Vec<(usize, InternalMod)>, Error> {
        read_param_slots(&self.0, SLOTS)?
            .into_iter()
            .map(|(slot, chunk)| Ok((slot, InternalMod::try_from(&chunk)?)))
            .collect()
    }
}

impl std::convert::TryFrom<&Chunk> for InternalModArray16 {
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
