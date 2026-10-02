use std::io::{Cursor, Write};

use crate::{
    Error,
    kontakt::{Chunk, KontaktError},
    read_bytes::ReadBytesExt,
};

const CHUNK_ID: u16 = 0x3F;
const VERSION: u16 = 0x11;
// Corpus-inferred v0x11 minimum: four packed 13-byte records.
// Preserve additional opaque bytes rather than imposing an exact-size cap.
const AHDSR_TAIL: usize = 52;

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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// At least 52 trailing bytes: four `(f32, f32, f32, bool)` records
    /// of unknown meaning, plus any opaque extension bytes.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub unknown_tail: Vec<u8>,
}

impl EnvelopeAhdsr {
    fn validate(&self) -> Result<(), Error> {
        let times = [self.attack_ms, self.decay_ms, self.hold_ms, self.release_ms];
        if self.unknown_tail.len() < AHDSR_TAIL {
            return Err(Error::Static("Incomplete AHDSR opaque metadata"));
        }
        if !(-1.0..=1.0).contains(&self.attack_curve)
            || !(0.0..=1.0).contains(&self.sustain)
            || !times.iter().all(|t| t.is_finite() && *t >= 0.0)
        {
            return Err(Error::Static("AHDSR envelope values out of range"));
        }
        Ok(())
    }

    /// Encode editable parameters while preserving the unknown flag/tail.
    /// Requires metadata from a complete record, not a runtime-cache copy.
    pub fn to_chunk(&self) -> Result<Chunk, Error> {
        self.validate()?;
        let length = self
            .unknown_tail
            .len()
            .checked_add(28)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or(Error::Static("AHDSR chunk too large"))?;
        let mut data = Vec::new();
        data.try_reserve_exact(length as usize)
            .map_err(|_| Error::Static("AHDSR allocation failed"))?;
        data.extend_from_slice(&[0, VERSION as u8, 0]);
        for value in [
            self.attack_curve,
            self.attack_ms,
            self.decay_ms,
            self.hold_ms,
            self.release_ms,
            self.sustain,
        ] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.push(self.unknown_flag);
        data.extend_from_slice(&self.unknown_tail);
        Ok(Chunk { id: CHUNK_ID, data })
    }

    pub fn write(&self, writer: impl Write) -> Result<(), Error> {
        self.to_chunk()?.write(writer)
    }
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

        envelope.validate()?;
        Ok(envelope)
    }
}

const FLEX_CHUNK_ID: u16 = 0x40;
/// Kontakt's flex envelope holds at most 32 breakpoints.
const MAX_FLEX_POINTS: u32 = 32;
/// Unknown bytes after the points in version 0x11.
const FLEX_TAIL: usize = 13;
const FLEX_TAIL_V12: usize = FLEX_TAIL + 2;

/// One flex envelope breakpoint, reached from the previous one.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

impl EnvelopeFlex {
    fn validate(&self) -> Result<(), Error> {
        let count = self.points.len();
        if count == 0
            || count > MAX_FLEX_POINTS as usize
            || self.sustain as usize >= count
            || self.unknown_index as usize >= count
        {
            return Err(Error::Static("Flex envelope indices out of range"));
        }
        if !matches!(self.unknown_tail.len(), FLEX_TAIL | FLEX_TAIL_V12)
            || !self.points.iter().all(|p| {
                p.time_ms.is_finite()
                    && p.time_ms >= 0.0
                    && (0.0..=1.0).contains(&p.level)
                    && (0.0..=1.0).contains(&p.curve)
            })
        {
            return Err(Error::Static("Flex envelope values out of range"));
        }
        Ok(())
    }

    /// Preserve v0x11/v0x12 using their distinct 13/15-byte opaque tails.
    /// Missing opaque metadata is rejected instead of synthesizing defaults.
    pub fn to_chunk(&self) -> Result<Chunk, Error> {
        self.validate()?;
        let version = if self.unknown_tail.len() == FLEX_TAIL {
            0x11u16
        } else {
            0x12
        };
        let mut data = Vec::new();
        data.try_reserve_exact(15 + self.points.len() * 12 + self.unknown_tail.len())
            .map_err(|_| Error::Static("Flex envelope allocation failed"))?;
        data.push(0);
        data.extend_from_slice(&version.to_le_bytes());
        data.extend_from_slice(&(self.points.len() as u32 - 1).to_le_bytes());
        data.extend_from_slice(&self.unknown_index.to_le_bytes());
        data.extend_from_slice(&self.sustain.to_le_bytes());
        for point in &self.points {
            for value in [point.time_ms, point.level, point.curve] {
                data.extend_from_slice(&value.to_le_bytes());
            }
        }
        data.extend_from_slice(&self.unknown_tail);
        Ok(Chunk {
            id: FLEX_CHUNK_ID,
            data,
        })
    }

    pub fn write(&self, writer: impl Write) -> Result<(), Error> {
        self.to_chunk()?.write(writer)
    }
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
            0x12 => FLEX_TAIL_V12,
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

        if unknown_tail.len() != tail {
            return Err(Error::Static("Flex envelope values out of range"));
        }
        let envelope = Self {
            points,
            sustain,
            unknown_index,
            unknown_tail,
        };
        envelope.validate()?;
        Ok(envelope)
    }
}
