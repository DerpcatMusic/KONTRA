//! Raw readers for the modulator payloads that had none: step modulator (0x09),
//! DBD envelope (0x41), glide (0x0B), envelope follower (0x0A) and group
//! dynamics (0x4A).
//! Field order follows `docs/re/kontakt-catalog/reader_schemas.tsv` and the
//! native readers ([ref],
//! [ref]). Framing is the unstructured-chunk convention of the
//! neighbouring AHDSR/Flex readers: a zero byte, the u16 version, the payload.

use std::io::{Cursor, Write};

use crate::{
    kontakt::{Chunk, KontaktError},
    read_bytes::ReadBytesExt,
    Error,
};

use super::modulation::ensure_consumed;

fn open<'a>(chunk: &'a Chunk, id: u16, versions: &[u16]) -> Result<(Cursor<&'a [u8]>, u16), Error> {
    if chunk.id != id {
        return Err(KontaktError::IncorrectID {
            expected: id,
            got: chunk.id,
        }
        .into());
    }
    let mut reader = Cursor::new(chunk.data.as_slice());
    if reader.read_u8()? != 0 {
        return Err(Error::Static(
            "Structured modulator payload is not supported",
        ));
    }
    let version = reader.read_u16_le()?;
    if !versions.contains(&version) {
        return Err(Error::VersionMismatch {
            expected: versions[0].into(),
            got: version.into(),
        });
    }
    Ok((reader, version))
}

fn floats<const N: usize>(reader: &mut Cursor<&[u8]>) -> Result<[f32; N], Error> {
    let mut out = [0.0; N];
    for v in &mut out {
        *v = reader.read_f32_le()?;
    }
    Ok(out)
}

/// Packed 13-byte `(f32, f32, f32, u8)` record, as in the AHDSR/Flex readers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PackedRecord {
    pub values: [f32; 3],
    pub flag: u8,
}

fn record(reader: &mut Cursor<&[u8]>) -> Result<PackedRecord, Error> {
    Ok(PackedRecord {
        values: floats::<3>(reader)?,
        flag: reader.read_u8()?,
    })
}

/// # EnvelopeDbd (`BParEnv_DBD`, 0x41)
///
/// Versions 0x10 `FFFFF`, 0x11 `FFFFFB`, 0x12 `FFF` + two packed records + `B`.
/// The five values are unnamed by the native reader.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvelopeDbd {
    pub version: u16,
    /// The three leading floats (all versions).
    pub values: [f32; 3],
    /// v0x10/0x11: two more floats.
    pub extra: Option<[f32; 2]>,
    /// v0x12: two packed records in place of `extra`.
    pub records: Option<[PackedRecord; 2]>,
    /// v0x11/0x12 trailing byte.
    pub flag: Option<u8>,
}

impl TryFrom<&Chunk> for EnvelopeDbd {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        let (mut r, version) = open(chunk, 0x41, &[0x12, 0x11, 0x10])?;
        let values = floats::<3>(&mut r)?;
        let (extra, records) = if version == 0x12 {
            (None, Some([record(&mut r)?, record(&mut r)?]))
        } else {
            (Some(floats::<2>(&mut r)?), None)
        };
        let flag = if version == 0x10 {
            None
        } else {
            Some(r.read_u8()?)
        };
        Ok(Self {
            version,
            values,
            extra,
            records,
            flag,
        })
    }
}

/// # Glide (`BParGlide`, 0x0B)
///
/// v0x50 `IFF`; v0x51 `IFFFFB`. The leading enum is stored 1 or 2 (object 0/1).
#[derive(Debug, Clone, PartialEq)]
pub struct Glide {
    pub version: u16,
    /// Stored enum, 1 or 2.
    pub mode: u32,
    pub values: [f32; 2],
    /// v0x51: two more floats and a byte.
    pub extra: Option<([f32; 2], u8)>,
}

impl TryFrom<&Chunk> for Glide {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        let (mut r, version) = open(chunk, 0x0B, &[0x51, 0x50])?;
        let mode = r.read_u32_le()?;
        if !matches!(mode, 1 | 2) {
            return Err(Error::Static("Glide mode out of range"));
        }
        let values = floats::<2>(&mut r)?;
        let extra = if version == 0x51 {
            Some((floats::<2>(&mut r)?, r.read_u8()?))
        } else {
            None
        };
        Ok(Self {
            version,
            mode,
            values,
            extra,
        })
    }
}

/// # EnvelopeFollower (`BParEnvF`, 0x0A), v0x60 `FFFF`.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvelopeFollower {
    pub values: [f32; 4],
}

impl TryFrom<&Chunk> for EnvelopeFollower {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        let (mut r, _) = open(chunk, 0x0A, &[0x60])?;
        Ok(Self {
            values: floats::<4>(&mut r)?,
        })
    }
}

/// # GroupDynamics (`BParGroupDynamics`, 0x4A), v0x10: 64 floats.
///
/// Native reader: [ref]. Every measured
/// corpus preset stores 64 zero floats; [`Self::has_nonzero`] lets translators
/// diagnose non-zero payloads instead of silently dropping them.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupDynamics {
    pub values: [f32; 64],
}

impl GroupDynamics {
    /// Returns `true` if any stored float is non-zero or NaN.
    pub fn has_nonzero(&self) -> bool {
        self.values.iter().any(|&v| v != 0.0)
    }
}

impl TryFrom<&Chunk> for GroupDynamics {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        let (mut r, _) = open(chunk, 0x4A, &[0x10])?;
        let values = floats::<64>(&mut r)?;
        ensure_consumed(&r)?;
        Ok(Self { values })
    }
}

/// Kontakt's step modulator holds at most 32 step floats ([ref] bounds
/// `param_1 + 0x10..0x90`).
const MAX_STEP_MOD_STEPS: usize = 32;

/// # StepMod (`BParArp`, 0x09), versions 0x80 and 0x81.
///
/// Field order and XML names verified against Kontakt 8.13.1 [ref]
///, [ref], and
/// [ref]:
/// - `u32 mode` (must be `0`), `u8 snap_to_12` (`+0xd1`, `"snapTo12"`),
///   `u32 num_steps` (`+0xc`, `"numSteps"`), `u8 single_trigger` (`+0xd0`,
///   `"singleTrigger"`), `u32 len` + `f32[len]` steps (`<= 32`, `+0x10..+0x90`),
///   `f32 frequency` (`+0x90`, `"frequency"`).
/// - v0x80 ends with `f32 legacy_note_value` (`+0xb0`, `"noteValue_frequency"`).
/// - v0x81 ends with a 13-byte [ref] sync record (`+0x98`).
#[derive(Debug, Clone, PartialEq)]
pub struct StepMod {
    pub version: u16,
    pub snap_to_12: bool,
    pub num_steps: u32,
    pub single_trigger: bool,
    pub steps: Vec<f32>,
    pub frequency: f32,
    /// v0x80 trailing `noteValue_frequency` float.
    pub legacy_note_value: Option<f32>,
    /// v0x81 trailing 13-byte frequency sync record.
    pub frequency_sync: Option<PackedRecord>,
}

fn bool_byte(reader: &mut Cursor<&[u8]>, what: &'static str) -> Result<bool, Error> {
    match reader.read_u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::Static(what)),
    }
}

impl StepMod {
    fn validate(&self) -> Result<(), Error> {
        if !matches!(self.version, 0x80 | 0x81) {
            return Err(Error::Static("Unsupported BParArp version"));
        }
        if self.steps.len() > MAX_STEP_MOD_STEPS {
            return Err(Error::Static("Step modulator has more than 32 steps"));
        }
        let shape_ok = match self.version {
            0x80 => self.legacy_note_value.is_some() && self.frequency_sync.is_none(),
            0x81 => self.legacy_note_value.is_none() && self.frequency_sync.is_some(),
            _ => false,
        };
        if !shape_ok {
            return Err(Error::Static("StepMod fields do not match version"));
        }
        Ok(())
    }

    /// Encode back to a raw 0x09 chunk, preserving exact IEEE-754 float bits.
    pub fn to_chunk(&self) -> Result<Chunk, Error> {
        self.validate()?;
        let tail = if self.version == 0x80 { 4 } else { 13 };
        let len = 3 + 18 + self.steps.len() * 4 + tail;
        let mut data = Vec::with_capacity(len);
        data.push(0);
        data.extend_from_slice(&self.version.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.push(u8::from(self.snap_to_12));
        data.extend_from_slice(&self.num_steps.to_le_bytes());
        data.push(u8::from(self.single_trigger));
        data.extend_from_slice(&(self.steps.len() as u32).to_le_bytes());
        for &step in &self.steps {
            data.extend_from_slice(&step.to_le_bytes());
        }
        data.extend_from_slice(&self.frequency.to_le_bytes());
        if let Some(note_value) = self.legacy_note_value {
            data.extend_from_slice(&note_value.to_le_bytes());
        }
        if let Some(sync) = self.frequency_sync {
            for value in sync.values {
                data.extend_from_slice(&value.to_le_bytes());
            }
            data.push(sync.flag);
        }
        Ok(Chunk { id: 0x09, data })
    }

    pub fn write(&self, writer: impl Write) -> Result<(), Error> {
        self.to_chunk()?.write(writer)
    }
}

impl TryFrom<&Chunk> for StepMod {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        let (mut r, version) = open(chunk, 0x09, &[0x81, 0x80])?;
        if r.read_u32_le()? != 0 {
            return Err(Error::Static("Unsupported BParArp mode"));
        }
        let snap_to_12 = bool_byte(&mut r, "Invalid BParArp snapTo12 flag")?;
        let num_steps = r.read_u32_le()?;
        let single_trigger = bool_byte(&mut r, "Invalid BParArp singleTrigger flag")?;
        let count = r.read_u32_le()? as usize;
        if count > MAX_STEP_MOD_STEPS {
            return Err(Error::Static("Step modulator step count exceeds 32"));
        }
        let mut steps = Vec::with_capacity(count);
        for _ in 0..count {
            steps.push(r.read_f32_le()?);
        }
        let frequency = r.read_f32_le()?;
        let (legacy_note_value, frequency_sync) = if version == 0x80 {
            (Some(r.read_f32_le()?), None)
        } else {
            (None, Some(record(&mut r)?))
        };
        ensure_consumed(&r)?;
        Ok(Self {
            version,
            snap_to_12,
            num_steps,
            single_trigger,
            steps,
            frequency,
            legacy_note_value,
            frequency_sync,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: u16, version: u16, body: &[u8]) -> Chunk {
        let mut data = vec![0];
        data.extend_from_slice(&version.to_le_bytes());
        data.extend_from_slice(body);
        Chunk { id, data }
    }
    fn f(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn dbd_versions_follow_the_reader_schemas() {
        let v10 = EnvelopeDbd::try_from(&chunk(0x41, 0x10, &f(&[1., 2., 3., 4., 5.]))).unwrap();
        assert_eq!(
            (v10.values, v10.extra, v10.flag),
            ([1., 2., 3.], Some([4., 5.]), None)
        );
        let mut body = f(&[1., 2., 3., 4., 5.]);
        body.push(7);
        let v11 = EnvelopeDbd::try_from(&chunk(0x41, 0x11, &body)).unwrap();
        assert_eq!(v11.flag, Some(7));
        let mut body = f(&[1., 2., 3.]);
        for n in 0..2u8 {
            body.extend(f(&[10. + f32::from(n), 11., 12.]));
            body.push(n);
        }
        body.push(9);
        let v12 = EnvelopeDbd::try_from(&chunk(0x41, 0x12, &body)).unwrap();
        assert_eq!(
            v12.records.unwrap()[1],
            PackedRecord {
                values: [11., 11., 12.],
                flag: 1
            }
        );
        assert_eq!(v12.flag, Some(9));
        assert!(EnvelopeDbd::try_from(&chunk(0x41, 0x13, &body)).is_err());
        assert!(EnvelopeDbd::try_from(&chunk(0x41, 0x12, &body[..body.len() - 1])).is_err());
    }

    #[test]
    fn glide_follower_and_dynamics_read_their_stored_floats() {
        let mut body = 2u32.to_le_bytes().to_vec();
        body.extend(f(&[0.5, 0.25]));
        let g = Glide::try_from(&chunk(0x0B, 0x50, &body)).unwrap();
        assert_eq!((g.mode, g.values, g.extra), (2, [0.5, 0.25], None));
        body.extend(f(&[1., 2.]));
        body.push(1);
        assert_eq!(
            Glide::try_from(&chunk(0x0B, 0x51, &body)).unwrap().extra,
            Some(([1., 2.], 1))
        );
        body[..4].copy_from_slice(&3u32.to_le_bytes());
        assert!(Glide::try_from(&chunk(0x0B, 0x51, &body)).is_err());
        assert_eq!(
            EnvelopeFollower::try_from(&chunk(0x0A, 0x60, &f(&[1., 2., 3., 4.])))
                .unwrap()
                .values,
            [1., 2., 3., 4.]
        );
        let all: Vec<f32> = (0..64).map(|n| n as f32).collect();
        let dyn_nz = GroupDynamics::try_from(&chunk(0x4A, 0x10, &f(&all))).unwrap();
        assert_eq!(dyn_nz.values[63], 63.);
        assert!(dyn_nz.has_nonzero());
        let dyn_zero = GroupDynamics::try_from(&chunk(0x4A, 0x10, &f(&[0.0; 64]))).unwrap();
        assert!(!dyn_zero.has_nonzero());
        let mut dyn_nan_vals = [0.0f32; 64];
        dyn_nan_vals[12] = f32::NAN;
        assert!(
            GroupDynamics::try_from(&chunk(0x4A, 0x10, &f(&dyn_nan_vals)))
                .unwrap()
                .has_nonzero()
        );
        assert!(GroupDynamics::try_from(&chunk(0x4A, 0x10, &f(&all[..63]))).is_err());
        let mut dyn_extra = f(&all);
        dyn_extra.push(0);
        assert!(GroupDynamics::try_from(&chunk(0x4A, 0x10, &dyn_extra)).is_err());
    }

    #[test]
    fn step_mod_versions_follow_native_reader_and_roundtrip() {
        for version in [0x80u16, 0x81] {
            let steps = vec![
                f32::from_bits(0x8000_0000),
                0.5,
                -1.0,
                f32::from_bits(0x7fc0_1234),
            ];
            let step_mod = StepMod {
                version,
                snap_to_12: true,
                num_steps: 16,
                single_trigger: false,
                steps: steps.clone(),
                frequency: 4.0,
                legacy_note_value: (version == 0x80).then_some(-1.0),
                frequency_sync: (version == 0x81).then_some(PackedRecord {
                    values: [2.0, 0.25, 0.5],
                    flag: 1,
                }),
            };
            let encoded = step_mod.to_chunk().unwrap();
            let decoded = StepMod::try_from(&encoded).unwrap();
            assert_eq!(decoded.version, version);
            assert_eq!(
                (
                    decoded.snap_to_12,
                    decoded.num_steps,
                    decoded.single_trigger
                ),
                (true, 16, false)
            );
            assert_eq!(
                decoded
                    .steps
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                steps.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            );
            assert_eq!(decoded.frequency, 4.0);
            assert_eq!(decoded.legacy_note_value, step_mod.legacy_note_value);
            assert_eq!(decoded.frequency_sync, step_mod.frequency_sync);
            let mut before = Vec::new();
            encoded.write(&mut before).unwrap();
            let mut after = Vec::new();
            decoded.write(&mut after).unwrap();
            assert_eq!(after, before);

            // Truncated payloads fail.
            for end in 0..encoded.data.len() {
                assert!(StepMod::try_from(&Chunk {
                    id: 0x09,
                    data: encoded.data[..end].to_vec(),
                })
                .is_err());
            }
            // Trailing byte fails.
            let mut extra = encoded.data.clone();
            extra.push(0);
            assert!(StepMod::try_from(&Chunk {
                id: 0x09,
                data: extra
            })
            .is_err());
            // Non-zero mode fails ([ref] throws).
            let mut bad_mode = encoded.data.clone();
            bad_mode[3] = 1;
            assert!(StepMod::try_from(&Chunk {
                id: 0x09,
                data: bad_mode
            })
            .is_err());
            // Invalid bool byte fails.
            let mut bad_bool = encoded.data.clone();
            bad_bool[7] = 2;
            assert!(StepMod::try_from(&Chunk {
                id: 0x09,
                data: bad_bool
            })
            .is_err());
            // Step count > 32 fails.
            let mut too_many = encoded.data.clone();
            too_many[13..17].copy_from_slice(&33u32.to_le_bytes());
            assert!(StepMod::try_from(&Chunk {
                id: 0x09,
                data: too_many
            })
            .is_err());
        }
    }
}
