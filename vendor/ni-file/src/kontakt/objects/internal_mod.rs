use std::io::Cursor;

use crate::{
    kontakt::{error::KontaktError, structured_object::StructuredObject, Chunk},
    read_bytes::ReadBytesExt,
    Error,
};

use super::{
    modulation::{ensure_consumed, read_assignment_name, read_targets},
    EnvelopeAhdsr, EnvelopeDbd, EnvelopeFlex, EnvelopeFollower, Glide, Lfo, ModTarget,
};

const CHUNK_ID: u16 = 0x0D;
const ENVELOPE_ID: u16 = 0x07;
/// Category stored after the name (`0140970000.c:4980-5086`):
/// 1 = LFO (0x08), 2 = Envelope wrapper (0x07 -> AHDSR/Flex/DBD),
/// 3 = StepMod (0x09), 4 = EnvelopeFollower (0x0A), 5 = Glide (0x0B).
const CATEGORIES: [u32; 5] = [1, 2, 3, 4, 5];

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
    /// Native v0x80/81: [router-open UI state, bypass, retrigger, unknown].
    /// Boolean field order is corroborated by the native typed reader and XML
    /// importer stores; unknown fields remain byte-preserved.
    pub unknown_flags: [u8; 4],
    /// Numeric id; meaning unknown.
    pub unknown_id: u32,
}

/// The modulation source object nested in an internal modulator.
#[derive(Debug, Clone, PartialEq)]
pub enum Modulator {
    Ahdsr(EnvelopeAhdsr),
    Flex(EnvelopeFlex),
    Lfo(Lfo),
    /// Raw stored values only; the native runtime mapping is unproved.
    Dbd(EnvelopeDbd),
    Glide(Glide),
    Follower(EnvelopeFollower),
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
        let legacy = self.0.version == 0x80;
        let targets = read_targets(&mut reader, legacy)?;
        let mut unknown_flags = [0; 4];
        std::io::Read::read_exact(&mut reader, &mut unknown_flags)?;
        let unknown_id = reader.read_u32_le()?;
        let name = read_assignment_name(&mut reader, legacy)?;
        let category = reader.read_u32_le()?;
        if !CATEGORIES.contains(&category) {
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
        let wrapper;
        let inner = if let Some(chunk) = self.0.find_first(ENVELOPE_ID) {
            wrapper = StructuredObject::try_from(chunk)?;
            wrapper
                .children
                .first()
                .ok_or(Error::Static("Modulator wrapper has no modulator"))?
        } else {
            self.0
                .children
                .first()
                .ok_or(KontaktError::MissingChunk(ENVELOPE_ID))?
        };
        Ok(match inner.id {
            0x3F => Modulator::Ahdsr(EnvelopeAhdsr::try_from(inner)?),
            0x40 => Modulator::Flex(EnvelopeFlex::try_from(inner)?),
            0x41 => Modulator::Dbd(EnvelopeDbd::try_from(inner)?),
            0x0B => Modulator::Glide(Glide::try_from(inner)?),
            0x0A => Modulator::Follower(EnvelopeFollower::try_from(inner)?),
            0x08 => match Lfo::read_supported(inner)? {
                Some(lfo) => Modulator::Lfo(lfo),
                None => Modulator::Other { chunk_id: inner.id },
            },
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

#[cfg(test)]
mod tests {
    use super::*;

    fn internal_mod_with(category: u32, child: Chunk) -> InternalMod {
        let mut private = Vec::new();
        private.extend(1u32.to_le_bytes());
        private.extend(5u32.to_le_bytes());
        private.extend(b"pitch");
        private.extend(0.5f32.to_le_bytes());
        private.extend((-1i16).to_le_bytes());
        private.push(0);
        private.extend(0i16.to_le_bytes());
        private.extend(0u32.to_le_bytes());
        private.extend([0, 0]);
        private.extend([0u8; 4]);
        private.extend(0u32.to_le_bytes());
        private.extend(4u32.to_le_bytes());
        private.extend(b"MOD0");
        private.extend(category.to_le_bytes());
        InternalMod(StructuredObject {
            version: 0x81,
            private_data: private,
            public_data: Vec::new(),
            children: vec![child],
        })
    }

    #[test]
    fn categories_3_and_4_parse_step_mod_and_envelope_follower() {
        let step_chunk = Chunk {
            id: 0x09,
            data: vec![0, 0x10, 0x00],
        };
        let step = internal_mod_with(3, step_chunk).params().unwrap();
        assert_eq!(step.modulator, Modulator::Other { chunk_id: 0x09 });

        let mut envf_data = vec![0];
        envf_data.extend(0x60u16.to_le_bytes());
        for v in [1.0f32, 2.0, 3.0, 4.0] {
            envf_data.extend(v.to_le_bytes());
        }
        let envf = internal_mod_with(
            4,
            Chunk {
                id: 0x0A,
                data: envf_data,
            },
        )
        .params()
        .unwrap();
        assert_eq!(
            envf.modulator,
            Modulator::Follower(EnvelopeFollower {
                values: [1.0, 2.0, 3.0, 4.0],
            })
        );
    }
}
