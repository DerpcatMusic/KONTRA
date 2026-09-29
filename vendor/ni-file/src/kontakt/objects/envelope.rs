use std::io::Cursor;

use crate::{
    kontakt::{Chunk, KontaktError},
    read_bytes::ReadBytesExt,
    Error,
};

const CHUNK_ID: u16 = 0x3F;
const VERSION: u16 = 0x11;

/// # EnvelopeAhdsr
///
/// Kontakt's AHDSR envelope parameters. Field order was established from
/// value ranges across local presets; see `audits/MODULATION.md`.
///
/// Type:           Chunk (unstructured)
/// SerType:        0x3F
/// Versions:       0x11
/// Kontakt 7:      BParEnv_AHDSR
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct EnvelopeAhdsr {
    /// Attack curve shape, -1..=1 (0 is linear).
    pub attack_curve: f32,
    /// Attack time in milliseconds.
    pub attack_ms: f32,
    /// Decay time in milliseconds.
    pub decay_ms: f32,
    /// Hold time in milliseconds.
    pub hold_ms: f32,
    /// Release time in milliseconds.
    pub release_ms: f32,
    /// Sustain level as a linear gain, 0..=1.
    pub sustain: f32,
    /// Boolean flag of unknown meaning (possibly retrigger or AHD-only mode).
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_flag: u8,
    /// Trailing 52 bytes: four `(f32, f32, f32, bool)` records of unknown meaning.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_tail: Vec<u8>,
}

impl TryFrom<&Chunk> for EnvelopeAhdsr {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != CHUNK_ID {
            return Err(KontaktError::IncorrectID {
                expected: CHUNK_ID,
                got: chunk.id,
            }
            .into());
        }

        let mut reader = Cursor::new(&chunk.data);
        if reader.read_u8()? != 0 {
            return Err(Error::Static("Structured AHDSR envelope is not supported"));
        }
        let version = reader.read_u16_le()?;
        if version != VERSION {
            return Err(Error::VersionMismatch {
                expected: VERSION.into(),
                got: version.into(),
            });
        }

        let envelope = Self {
            attack_curve: reader.read_f32_le()?,
            attack_ms: reader.read_f32_le()?,
            decay_ms: reader.read_f32_le()?,
            hold_ms: reader.read_f32_le()?,
            release_ms: reader.read_f32_le()?,
            sustain: reader.read_f32_le()?,
            unknown_flag: reader.read_u8()?,
            unknown_tail: reader.read_all()?,
        };

        let times = [
            envelope.attack_ms,
            envelope.decay_ms,
            envelope.hold_ms,
            envelope.release_ms,
        ];
        let valid = (-1.0..=1.0).contains(&envelope.attack_curve)
            && (0.0..=1.0).contains(&envelope.sustain)
            && times.iter().all(|t| t.is_finite() && *t >= 0.0);
        if !valid {
            return Err(Error::Static("AHDSR envelope values out of range"));
        }

        Ok(envelope)
    }
}
