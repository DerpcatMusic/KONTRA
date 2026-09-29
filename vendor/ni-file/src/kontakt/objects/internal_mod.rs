use std::io::Cursor;

use crate::{
    Error,
    kontakt::{Chunk, error::KontaktError, structured_object::StructuredObject},
    read_bytes::ReadBytesExt,
};

use super::{
    EnvelopeAhdsr, EnvelopeFlex, ModTarget,
    modulation::{ensure_consumed, read_name, read_targets},
};

const CHUNK_ID: u16 = 0x0D;
const ENVELOPE_ID: u16 = 0x07;
/// Category stored after the name; external assignments store 1 or 2 there too.
const INTERNAL_CATEGORY: u32 = 2;

/// # InternalMod
///
/// An internal modulator (envelope, LFO...) and the parameters it drives.
///
/// Type:           Chunk<StructuredObject>, fields in private data
/// SerType:        0x0D
/// Versions:       0x80, 0x81
/// Kontakt 7:      BParInternalMod
#[derive(Debug)]
pub struct InternalMod(pub StructuredObject);

/// Decoded internal modulator.
#[derive(Debug, Clone, PartialEq)]
pub struct InternalModParams {
    /// KSP modulator name (`find_mod`), e.g. `ENV_AHDSR`.
    pub name: String,
    pub targets: Vec<ModTarget>,
    pub modulator: Modulator,
    /// Four flag bytes; the first may be bypass, unverified.
    pub unknown_flags: [u8; 4],
    /// Numeric id; meaning unknown.
    pub unknown_id: u32,
}

/// The modulation source object nested in an internal modulator.
#[derive(Debug, Clone, PartialEq)]
pub enum Modulator {
    Ahdsr(EnvelopeAhdsr),
    Flex(EnvelopeFlex),
    /// Undecoded modulator, identified by its chunk id.
    Other {
        chunk_id: u16,
    },
}

impl InternalMod {
    pub fn params(&self) -> Result<InternalModParams, Error> {
        if !matches!(self.0.version, 0x80 | 0x81) {
            return Err(Error::Generic(format!(
                "Unsupported BParInternalMod version 0x{:X}",
                self.0.version
            )));
        }

        let mut reader = Cursor::new(self.0.private_data.as_slice());
        let targets = read_targets(&mut reader)?;
        let mut unknown_flags = [0; 4];
        std::io::Read::read_exact(&mut reader, &mut unknown_flags)?;
        let unknown_id = reader.read_u32_le()?;
        let name = read_name(&mut reader)?;
        let category = reader.read_u32_le()?;
        if category != INTERNAL_CATEGORY {
            return Err(Error::Generic(format!(
                "Unexpected internal modulator category {category}"
            )));
        }
        ensure_consumed(&reader)?;

        Ok(InternalModParams {
            name,
            targets,
            modulator: self.modulator()?,
            unknown_flags,
            unknown_id,
        })
    }

    /// The envelope wrapper (0x07) holds the concrete modulator chunk.
    fn modulator(&self) -> Result<Modulator, Error> {
        let wrapper = self
            .0
            .find_first(ENVELOPE_ID)
            .ok_or(KontaktError::MissingChunk(ENVELOPE_ID))?;
        let wrapper = StructuredObject::try_from(wrapper)?;
        let inner = wrapper
            .children
            .first()
            .ok_or(Error::Static("Modulator wrapper has no modulator"))?;
        Ok(match inner.id {
            0x3F => Modulator::Ahdsr(EnvelopeAhdsr::try_from(inner)?),
            0x40 => Modulator::Flex(EnvelopeFlex::try_from(inner)?),
            chunk_id => Modulator::Other { chunk_id },
        })
    }
}

impl std::convert::TryFrom<&Chunk> for InternalMod {
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
