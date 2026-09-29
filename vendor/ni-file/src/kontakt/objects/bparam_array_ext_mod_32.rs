use crate::{
    kontakt::{error::KontaktError, structured_object::StructuredObject, Chunk},
    Error,
};

use super::{modulation::read_param_slots, ExternalMod};

const CHUNK_ID: u16 = 0x3C;
const SLOTS: usize = 32;

/// BParameterArraySerBParExternalMod32
///
/// Thirty-two optional external modulation assignment slots of a group.
///
/// Type:           Chunk<StructuredObject>
/// SerType:        0x3C
/// Versions:       0x10, 0x12
/// Kontakt 7:      BParameterArraySerBParExternalMod32
#[derive(Debug)]
pub struct ExternalModArray32(pub StructuredObject);

impl ExternalModArray32 {
    /// Occupied slots as `(slot index, assignment)`.
    pub fn slots(&self) -> Result<Vec<(usize, ExternalMod)>, Error> {
        read_param_slots(&self.0, SLOTS)?
            .into_iter()
            .map(|(slot, chunk)| Ok((slot, ExternalMod::try_from(&chunk)?)))
            .collect()
    }
}

impl std::convert::TryFrom<&Chunk> for ExternalModArray32 {
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
