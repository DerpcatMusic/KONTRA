use std::io::Cursor;

use crate::{kontakt::structured_object::StructuredObject, read_bytes::ReadBytesExt, Error};

#[derive(Debug)]
pub struct Zone(pub StructuredObject);

/// Type:           StructuredObject
/// Kontakt 7:      BZone, BProgram::readZones()
/// KontaktIO:      K4PL_Zone<K4PO::K4PL_ZoneDataV95>
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ZoneParams {
    pub sample_start: i32,
    pub sample_end: i32,
    pub sample_start_mod_range: i32,
    pub low_velocity: i16,
    pub high_velocity: i16,
    pub low_key: i16,
    pub high_key: i16,
    pub fade_low_velocity: i16,
    pub fade_high_velocity: i16,
    pub fade_low_key: i16,
    pub fade_high_key: i16,
    pub root_key: i16,
    pub zone_volume: f32,
    pub zone_pan: f32,
    pub zone_tune: f32,
    /// Whether the native record contains sample metadata. Absent metadata
    /// below uses zero placeholders and filename_id -1; it must never resolve PCM.
    pub sample_present: bool,
    /// The index of the file in the filetable.
    pub filename_id: i32,
    pub filename_prefix: Option<[u8; 6]>,
    pub sample_data_type: i32,
    pub sample_rate: i32,
    pub num_channels: u8,
    pub num_frames: i32,
    pub reserved1: i32,
    pub reserved2: Option<i32>,
    pub root_note: i32,
    pub tuning: f32,
    pub reserved3: u8,
    pub reserved4: i32,
    pub unknown_tail: Vec<u8>,
    // LoopArray 0x39
    // QuickBrowseData 0x4e
    // PrivateRawObject 0x35
}

impl Zone {
    /// Sample reference from the common zone prefix, independent of later
    /// version-specific public parameters.
    pub fn filename_id(&self) -> Result<i32, Error> {
        if !self.has_sample()? {
            return Ok(-1);
        }
        let at = if self.0.version >= 0x9a { 48 } else { 42 };
        let bytes = self
            .0
            .public_data
            .get(at..at + 4)
            .ok_or(Error::Static("Truncated zone sample reference"))?;
        Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
    }

    /// The v0x9a..9c common prefix ends with two flags and an opaque u32.
    /// The second flag gates the entire sample suffix in the native serializer.
    pub fn has_sample(&self) -> Result<bool, Error> {
        if !matches!(self.0.version, 0x92..=0x95 | 0x97..=0x9c) {
            return Err(Error::Static("Unsupported zone version"));
        }
        if self.0.version < 0x9a {
            return Ok(true);
        }
        let prefix = self
            .0
            .public_data
            .get(42..48)
            .ok_or(Error::Static("Truncated zone sample-presence prefix"))?;
        match prefix.get(1) {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            Some(_) => Err(Error::Static("Invalid zone sample-presence flag")),
            None => Err(Error::Static("Truncated zone sample-presence flag")),
        }
    }

    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        Ok(Self(StructuredObject::read(&mut reader)?))
    }

    pub fn params(&self) -> Result<ZoneParams, Error> {
        let mut reader = Cursor::new(&self.0.public_data);

        let sample_present = self.has_sample()?;
        let mut params = ZoneParams {
            sample_start: reader.read_i32_le()?,
            sample_end: reader.read_i32_le()?,
            sample_start_mod_range: reader.read_i32_le()?,
            low_velocity: reader.read_i16_le()?,
            high_velocity: reader.read_i16_le()?,
            low_key: reader.read_i16_le()?,
            high_key: reader.read_i16_le()?,
            fade_low_velocity: reader.read_i16_le()?,
            fade_high_velocity: reader.read_i16_le()?,
            fade_low_key: reader.read_i16_le()?,
            fade_high_key: reader.read_i16_le()?,
            root_key: reader.read_i16_le()?,
            zone_volume: reader.read_f32_le()?,
            zone_pan: reader.read_f32_le()?,
            zone_tune: reader.read_f32_le()?,
            filename_prefix: if self.0.version >= 0x9a {
                let mut prefix = [0; 6];
                std::io::Read::read_exact(&mut reader, &mut prefix)?;
                Some(prefix)
            } else {
                None
            },
            sample_present,
            filename_id: -1,
            ..ZoneParams::default()
        };
        if sample_present {
            params.filename_id = reader.read_i32_le()?;
            params.sample_data_type = reader.read_i32_le()?;
            params.sample_rate = reader.read_i32_le()?;
            params.num_channels = reader.read_u8()?;
            params.num_frames = reader.read_i32_le()?;
            params.reserved1 = reader.read_i32_le()?;
            params.reserved2 = if self.0.version < 0x96 {
                Some(reader.read_i32_le()?)
            } else {
                None
            };
            params.root_note = reader.read_i32_le()?;
            params.tuning = reader.read_f32_le()?;
            params.reserved3 = reader.read_u8()?;
            params.reserved4 = reader.read_i32_le()?;
        }
        params.unknown_tail = reader.read_all()?;
        Ok(params)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;
    use crate::Error;

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_zone_data_v9a_000() -> Result<(), Error> {
        let file =
            File::open("tests/data/Objects/Kontakt/ZoneData/ZoneDataV9A/ZoneDataV9A-000.kon")?;
        let zone = Zone::read(file)?;
        assert_eq!(zone.0.version, 0x9A);
        zone.params()?;
        Ok(())
    }
}
