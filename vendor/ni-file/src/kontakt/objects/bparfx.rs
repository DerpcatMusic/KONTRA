use std::io::Cursor;

use crate::{
    kontakt::{error::KontaktError, structured_object::StructuredObject, Chunk},
    read_bytes::ReadBytesExt,
    Error, NIFileError,
};

const CHUNK_ID: u16 = 0x25;

/// # BParFX
///
/// An effect slot. The effect object is the first child; its chunk ID is the
/// effect's serialization type (e.g. 0x16 BParFXIRC, 0x59 BParFXGaloisReverb).
/// The slot state shared by every effect lives in the private data, the slot
/// has no public data.
///
/// - Type:           Chunk<StructuredObject>
/// - SerType:        0x25
/// - Versions:       0x50
/// - Kontakt 7:      BParFX
///
#[derive(Debug)]
pub struct BParFX(pub StructuredObject);

/// Slot state from the BParFX private data (22 bytes, version 0x50).
///
/// Layout verified against 1,700 local slots:
/// `u32 effect_type, u32 0, u8 0, u8 bypass, f32 output_gain, f32 dry_level, i32 -1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BParFXParams {
    /// Kontakt's internal effect enum (not the serialization ID): 2 compressor,
    /// 11 send levels, 19 convolution, 20 gainer, 39 reverb, ...
    pub effect_type: u32,
    pub bypass: bool,
    /// Linear gain applied to the processed ("wet") signal.
    pub output_gain: f32,
    /// Linear level of the unprocessed signal mixed back in.
    pub dry_level: f32,
}

impl BParFX {
    pub fn version(&self) -> u16 {
        self.0.version
    }

    pub fn effect(&self) -> Option<&Chunk> {
        self.0.children.first()
    }

    pub fn params(&self) -> Result<BParFXParams, Error> {
        let data = &self.0.private_data;
        if data.len() < 22 {
            return Err(NIFileError::Generic(format!(
                "BParFX private data is {} bytes, expected 22",
                data.len()
            )));
        }
        let mut r = Cursor::new(data);
        let effect_type = r.read_u32_le()?;
        let _reserved = r.read_u32_le()?;
        let _flag = r.read_u8()?;
        let bypass = r.read_u8()? != 0;
        Ok(BParFXParams {
            effect_type,
            bypass,
            output_gain: r.read_f32_le()?,
            dry_level: r.read_f32_le()?,
        })
    }
}

impl std::convert::TryFrom<&Chunk> for BParFX {
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

    #[test]
    fn slot_state_is_read_from_private_data() {
        let mut private = 19u32.to_le_bytes().to_vec();
        private.extend([0, 0, 0, 0, 0, 1]);
        private.extend(0.5f32.to_le_bytes());
        private.extend(1f32.to_le_bytes());
        private.extend((-1i32).to_le_bytes());
        let fx = BParFX(StructuredObject {
            version: 0x50,
            public_data: Vec::new(),
            private_data: private,
            children: Vec::new(),
        });
        let p = fx.params().unwrap();
        assert_eq!(p.effect_type, 19);
        assert!(p.bypass);
        assert_eq!((p.output_gain, p.dry_level), (0.5, 1.0));
    }
}
