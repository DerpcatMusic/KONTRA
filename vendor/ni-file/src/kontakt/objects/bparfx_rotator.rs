use crate::{
    kontakt::{error::KontaktError, Chunk},
    read_bytes::ReadBytesExt,
    Error,
};

const CHUNK_ID: u16 = 0x22;

/// Type:           Chunk<Data>
/// SerType:        0x22
/// Versions:       0x50
/// Kontakt 7:      BParFXRotator
/// KontaktIO:
#[derive(Debug)]
/// Native fields in read order (speed, balance, accel_hi, accel_lo, distance, mix).
pub struct BParFXRotator(pub super::EffectParameters);

impl BParFXRotator {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let data = reader.read_all()?;
        super::EffectParameters::read(CHUNK_ID, 0x50, &data)?
            .map(Self)
            .ok_or(Error::Static("BParFXRotator has no native reader"))
    }
}

impl std::convert::TryFrom<&Chunk> for BParFXRotator {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != CHUNK_ID {
            return Err(KontaktError::IncorrectID {
                expected: CHUNK_ID,
                got: chunk.id,
            }
            .into());
        }
        let object = crate::kontakt::StructuredObject::try_from(chunk)?;
        super::EffectParameters::read(CHUNK_ID, object.version, &object.public_data)?
            .map(Self)
            .ok_or(Error::Static("BParFXRotator has no native reader"))
    }
}
