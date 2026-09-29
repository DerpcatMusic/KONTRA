use std::io::Cursor;

use crate::{
    Error,
    kontakt::{Chunk, KontaktError},
    read_bytes::ReadBytesExt,
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

const FLEX_CHUNK_ID: u16 = 0x40;
/// Kontakt's flex envelope holds at most 32 breakpoints.
const MAX_FLEX_POINTS: u32 = 32;
/// Unknown bytes after the points in version 0x11.
const FLEX_TAIL: usize = 13;

/// One flex envelope breakpoint, reached from the previous one.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct FlexPoint {
    /// Time from the previous point (the start at level 0 for the first) in milliseconds.
    pub time_ms: f32,
    /// Level, 0..=1.
    pub level: f32,
    /// Segment curve, 0..=1; 0.5 is linear.
    pub curve: f32,
}

/// # EnvelopeFlex
///
/// Kontakt's flex (breakpoint) envelope. Layout and meaning were established
/// from local presets; see `audits/MODULATION.md`.
///
/// Type:           Chunk (unstructured)
/// SerType:        0x40
/// Versions:       0x11, 0x12 (two more trailing bytes)
/// Kontakt 7:      BParEnv_Flex
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct EnvelopeFlex {
    pub points: Vec<FlexPoint>,
    /// Index into `points` held while the key is down.
    pub sustain: u32,
    /// Index into `points`, `sustain - 1` in every local preset (loop start?).
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_index: u32,
    /// Trailing bytes: a `u16` (v0x12 only), one `(f32, f32, f32)` record and a
    /// byte, all of unknown meaning.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_tail: Vec<u8>,
}

impl TryFrom<&Chunk> for EnvelopeFlex {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != FLEX_CHUNK_ID {
            return Err(KontaktError::IncorrectID {
                expected: FLEX_CHUNK_ID,
                got: chunk.id,
            }
            .into());
        }

        let mut reader = Cursor::new(&chunk.data);
        if reader.read_u8()? != 0 {
            return Err(Error::Static("Structured flex envelope is not supported"));
        }
        let version = reader.read_u16_le()?;
        let tail = match version {
            0x11 => FLEX_TAIL,
            0x12 => FLEX_TAIL + 2,
            _ => {
                return Err(Error::Generic(format!(
                    "Unsupported flex envelope version 0x{version:X}"
                )));
            }
        };

        let last = reader.read_u32_le()?;
        let unknown_index = reader.read_u32_le()?;
        let sustain = reader.read_u32_le()?;
        if last >= MAX_FLEX_POINTS || sustain > last || unknown_index > last {
            return Err(Error::Static("Flex envelope indices out of range"));
        }
        let points = (0..=last)
            .map(|_| {
                Ok(FlexPoint {
                    time_ms: reader.read_f32_le()?,
                    level: reader.read_f32_le()?,
                    curve: reader.read_f32_le()?,
                })
            })
            .collect::<Result<Vec<_>, std::io::Error>>()?;
        let unknown_tail = reader.read_all()?;

        let valid = unknown_tail.len() == tail
            && points.iter().all(|p| {
                p.time_ms.is_finite()
                    && p.time_ms >= 0.0
                    && (0.0..=1.0).contains(&p.level)
                    && (0.0..=1.0).contains(&p.curve)
            });
        if !valid {
            return Err(Error::Static("Flex envelope values out of range"));
        }

        Ok(Self {
            points,
            sustain,
            unknown_index,
            unknown_tail,
        })
    }
}
