use crate::{
    kontakt::{error::KontaktError, structured_object::StructuredObject, Chunk},
    read_bytes::ReadBytesExt,
    Error,
};

use super::{modulation::read_param_slots, ExternalMod};

const CHUNK_ID: u16 = 0x3C;
const SLOTS: usize = 32;

/// BParameterArraySerBParExternalMod32
///
/// Legacy 32-slot or explicitly counted v0x13 64-slot external assignments.
///
/// Type:           Chunk<StructuredObject>
/// SerType:        0x3C
/// Versions:       0x10, 0x11, 0x12, 0x13
/// Kontakt 7:      BParameterArraySerBParExternalMod32
#[derive(Debug)]
pub struct ExternalModArray32(pub StructuredObject);

impl ExternalModArray32 {
    /// Physical native capacity, independent of occupied-slot count.
    pub fn slot_count(&self) -> Result<usize, Error> {
        if self.0.version != 0x13 {
            return Ok(SLOTS);
        }
        let count = std::io::Cursor::new(&self.0.public_data).read_u32_le()? as usize;
        if !matches!(count, 32 | 64) {
            return Err(Error::Generic(format!(
                "Unsupported external modulation slot count {count}"
            )));
        }
        Ok(count)
    }

    /// Occupied slots as `(slot index, assignment)`.
    pub fn slots(&self) -> Result<Vec<(usize, ExternalMod)>, Error> {
        read_param_slots(&self.0, self.slot_count()?)?
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn counted_external_slots_keep_high_identity_and_unknown_bytes() {
        for version in [0x12u16, 0x13] {
            let mut data = vec![0];
            data.extend(version.to_le_bytes());
            if version == 0x13 {
                data.extend(32u32.to_le_bytes());
            }
            data.extend([0; 32]);
            let array = ExternalModArray32::try_from(&Chunk { id: CHUNK_ID, data }).unwrap();
            assert_eq!(array.slot_count().unwrap(), 32);
            assert!(array.slots().unwrap().is_empty());
        }
        let assignment = Chunk {
            id: 0x0c,
            data: vec![0, 0x81, 0, 0xff, 0x39],
        };
        let mut data = vec![0, 0x13, 0];
        data.extend(64u32.to_le_bytes());
        data.extend([0; 63]);
        data.push(1);
        assignment.write(&mut data).unwrap();
        let chunk = Chunk { id: CHUNK_ID, data };
        let array = ExternalModArray32::try_from(&chunk).unwrap();
        assert_eq!(array.slot_count().unwrap(), 64);
        let slots = array.slots().unwrap();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].0, 63);
        assert_eq!(slots[0].1 .0.public_data, [0xff, 0x39]);
        // Raw chunks remain the lossless writer for unknown assignment fields.
        let mut bytes = Vec::new();
        chunk.write(&mut bytes).unwrap();
        let decoded = Chunk::read(Cursor::new(&bytes)).unwrap();
        let mut written = Vec::new();
        decoded.write(&mut written).unwrap();
        assert_eq!(bytes, written);
        for end in 0..chunk.data.len() {
            let incomplete = Chunk {
                id: CHUNK_ID,
                data: chunk.data[..end].to_vec(),
            };
            assert!(ExternalModArray32::try_from(&incomplete)
                .and_then(|a| a.slots())
                .is_err());
        }
        for count in [0u32, 33, 65, u32::MAX] {
            let mut data = chunk.data.clone();
            data[3..7].copy_from_slice(&count.to_le_bytes());
            assert!(ExternalModArray32::try_from(&Chunk { id: CHUNK_ID, data })
                .unwrap()
                .slots()
                .is_err());
        }
        let mut data = chunk.data;
        data.push(0);
        assert!(ExternalModArray32::try_from(&Chunk { id: CHUNK_ID, data })
            .unwrap()
            .slots()
            .is_err());
    }
}
