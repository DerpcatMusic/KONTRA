//! Raw readers for the modulator payloads that had none: DBD envelope (0x41),
//! glide (0x0B), envelope follower (0x0A) and group dynamics (0x4A).
//! Field order follows `docs/re/kontakt-catalog/reader_schemas.tsv` and the
//! native readers (0x140cffd40, 0x140d034d0, 0x140cffac0, 0x140d035f0);
//! values are raw stored floats. Their physical meaning, and how they become
//! runtime state, are not established (see `modulators.md`), so nothing here
//! converts or names units. Framing is the unstructured-chunk convention of the
//! neighbouring AHDSR/Flex readers: a zero byte, the u16 version, the payload.
//! Validated against the schema table and synthetic bytes only; no corpus file
//! carrying these objects was available.

use std::io::Cursor;

use crate::{
    kontakt::{Chunk, KontaktError},
    read_bytes::ReadBytesExt,
    Error,
};

fn open<'a>(chunk: &'a Chunk, id: u16, versions: &[u16]) -> Result<(Cursor<&'a Vec<u8>>, u16), Error> {
    if chunk.id != id {
        return Err(KontaktError::IncorrectID { expected: id, got: chunk.id }.into());
    }
    let mut reader = Cursor::new(&chunk.data);
    if reader.read_u8()? != 0 {
        return Err(Error::Static("Structured modulator payload is not supported"));
    }
    let version = reader.read_u16_le()?;
    if !versions.contains(&version) {
        return Err(Error::VersionMismatch { expected: versions[0].into(), got: version.into() });
    }
    Ok((reader, version))
}

fn floats<const N: usize>(reader: &mut Cursor<&Vec<u8>>) -> Result<[f32; N], Error> {
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

fn record(reader: &mut Cursor<&Vec<u8>>) -> Result<PackedRecord, Error> {
    Ok(PackedRecord { values: floats::<3>(reader)?, flag: reader.read_u8()? })
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
        let flag = if version == 0x10 { None } else { Some(r.read_u8()?) };
        Ok(Self { version, values, extra, records, flag })
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
        let extra = if version == 0x51 { Some((floats::<2>(&mut r)?, r.read_u8()?)) } else { None };
        Ok(Self { version, mode, values, extra })
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
        Ok(Self { values: floats::<4>(&mut r)? })
    }
}

/// # GroupDynamics (`BParGroupDynamics`, 0x4A), v0x10: 64 floats.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupDynamics {
    pub values: [f32; 64],
}

impl TryFrom<&Chunk> for GroupDynamics {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Error> {
        let (mut r, _) = open(chunk, 0x4A, &[0x10])?;
        Ok(Self { values: floats::<64>(&mut r)? })
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
        assert_eq!((v10.values, v10.extra, v10.flag), ([1., 2., 3.], Some([4., 5.]), None));
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
        assert_eq!(v12.records.unwrap()[1], PackedRecord { values: [11., 11., 12.], flag: 1 });
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
        assert_eq!(Glide::try_from(&chunk(0x0B, 0x51, &body)).unwrap().extra, Some(([1., 2.], 1)));
        body[..4].copy_from_slice(&3u32.to_le_bytes());
        assert!(Glide::try_from(&chunk(0x0B, 0x51, &body)).is_err());
        assert_eq!(
            EnvelopeFollower::try_from(&chunk(0x0A, 0x60, &f(&[1., 2., 3., 4.]))).unwrap().values,
            [1., 2., 3., 4.]
        );
        let all: Vec<f32> = (0..64).map(|n| n as f32).collect();
        assert_eq!(GroupDynamics::try_from(&chunk(0x4A, 0x10, &f(&all))).unwrap().values[63], 63.);
        assert!(GroupDynamics::try_from(&chunk(0x4A, 0x10, &f(&all[..63]))).is_err());
    }
}
