use std::io::Cursor;

use crate::{
    kontakt::{chunk::Chunk, error::KontaktError, structured_object::StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

use super::zone_list::ZoneList;

const CHUNK_ID: u16 = 0x28;

/// SerType:        0x28
/// Known Versions: 0x80 .. 0xAF
/// Kontakt 7:      BProgram
/// KontaktIO:      K4PL_Program
#[derive(Debug)]
pub struct Program(pub StructuredObject);

#[derive(Debug, Default)]
pub struct ProgramPublicParams {
    pub name: String,
    pub num_bytes_samples_total: f64,
    pub transpose: i8,
    pub volume: f32,
    pub pan: f32,
    pub tune: f32,
    pub low_velocity: u8,
    pub high_velocity: u8,
    pub low_key: u8,
    pub high_key: u8,
    pub default_key_switch: i16,
    pub dfd_channel_preload_size: i32,
    pub library_id: i32,
    pub fingerprint: u32,
    pub loading_flags: u32,
    pub group_solo: bool,
    pub cat_icon_idx: i32,
    pub instrument_credits: String,
    pub instrument_author: String,
    pub instrument_url: String,
    pub instrument_cat1: i16,
    pub instrument_cat2: i16,
    pub instrument_cat3: i16,
    pub resource_container_filename: Option<i32>,
    pub wallpaper_filename: Option<i32>,
}

impl ProgramPublicParams {
    pub fn read<R: ReadBytesExt>(mut reader: R, version: u16) -> Result<Self, Error> {
        Ok(Self {
            name: reader.read_widestring_utf16()?,
            num_bytes_samples_total: reader.read_f64_le()?,
            transpose: reader.read_i8()?,
            volume: reader.read_f32_le()?,
            pan: reader.read_f32_le()?,
            tune: reader.read_f32_le()?,
            low_velocity: reader.read_u8()?,
            high_velocity: reader.read_u8()?,
            low_key: reader.read_u8()?,
            high_key: reader.read_u8()?,
            default_key_switch: reader.read_i16_le()?,
            dfd_channel_preload_size: reader.read_i32_le()?,
            library_id: reader.read_i32_le()?,
            fingerprint: reader.read_u32_le()?,
            loading_flags: reader.read_u32_le()?,
            group_solo: reader.read_bool()?,
            cat_icon_idx: reader.read_i32_le()?,
            instrument_credits: reader.read_widestring_utf16()?,
            instrument_author: reader.read_widestring_utf16()?,
            instrument_url: reader.read_widestring_utf16()?,
            instrument_cat1: reader.read_i16_le()?,
            instrument_cat2: reader.read_i16_le()?,
            instrument_cat3: reader.read_i16_le()?,
            resource_container_filename: {
                match version {
                    _ => None,
                }
            },
            wallpaper_filename: {
                match version {
                    _ => None,
                }
            },
        })
    }
}

impl Program {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        Ok(Self(StructuredObject::read(&mut reader)?))
    }

    pub fn version(&self) -> u16 {
        self.0.version
    }

    pub fn params(&self) -> Result<ProgramPublicParams, Error> {
        let reader = Cursor::new(&self.0.public_data);

        ProgramPublicParams::read(reader, self.0.version)

        // match self.0.version {
        //     0x80 | 0x82 | 0x90 => Ok(ProgramDataV80::read(reader)?),
        //     0xa6 => todo!(),
        //     0xa7 => todo!(),
        //     0xa8 | 0xa9 | 0xaa | 0xab | 0xac | 0xad | 0xae => todo!(),
        //     0xaf => todo!(),
        //     _ => todo!(),
        // }
    }

    pub fn private_params(&self) -> ProgramDataPrivateParams {
        ProgramDataPrivateParams { version: self.0.version, data: self.0.private_data.clone() }
    }

    pub fn zone_list(&self) -> Option<Result<ZoneList, Error>> {
        self.0
            .find_first(0x34)
            .map(|chunk| ZoneList::try_from(chunk))
    }

    pub fn children(&self) -> &Vec<Chunk> {
        &self.0.children
    }

    // 0x32 VoiceGroups
    // 0x33 GroupList
}

impl std::convert::TryFrom<&Chunk> for Program {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != CHUNK_ID {
            return Err(KontaktError::IncorrectID {
                expected: CHUNK_ID,
                got: chunk.id,
            }
            .into());
        }
        let reader = Cursor::new(&chunk.data);
        Self::read(reader)
    }
}

/// Verified BProgram private-prefix settings; the rest of the record stays opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SustainPedalMode {
    SustainPlusController,
    SustainWithoutController,
    ControllerOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProgramControllerOptions {
    pub sustain_mode: SustainPedalMode,
    pub standard_volume_pan: bool,
    pub standard_note_modes: bool,
    pub cc7_max_db: i32,
}

impl Default for ProgramControllerOptions {
    fn default() -> Self {
        Self { sustain_mode: SustainPedalMode::SustainPlusController,
            standard_volume_pan: true, standard_note_modes: true, cc7_max_db: 0 }
    }
}

/// Original bounded private record, including unsupported versions and fields.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProgramDataPrivateParams {
    pub version: u16,
    pub data: Vec<u8>,
}

impl ProgramDataPrivateParams {
    pub fn read<R: ReadBytesExt>(mut reader: R, version: u16) -> Result<Self, Error> {
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        Ok(Self { version, data })
    }

    pub fn controller_options(&self) -> Result<Option<ProgramControllerOptions>, Error> {
        // Kontakt 8.13.1 BProgram::doReadPrivPars, 0x140d0a2c0. Each listed
        // archive branch shares this prefix; unknown versions remain opaque.
        if !matches!(self.version, 0x80 | 0x82 | 0x90..=0x92 | 0xa0..=0xb5) {
            return Ok(None);
        }
        let mut reader = Cursor::new(&self.data);
        let sustain_mode = match reader.read_u32_le()? {
            0 => SustainPedalMode::SustainPlusController,
            1 => SustainPedalMode::SustainWithoutController,
            2 => SustainPedalMode::ControllerOnly,
            _ => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,
                "Invalid BProgram sustain-pedal mode").into()),
        };
        Ok(Some(ProgramControllerOptions { sustain_mode,
            standard_volume_pan: reader.read_u8()? != 0,
            standard_note_modes: reader.read_u8()? != 0, cc7_max_db: reader.read_i32_le()? }))
    }

    pub fn write<W: std::io::Write>(&self, mut writer: W) -> Result<(), Error> {
        writer.write_all(&self.data)?;
        Ok(())
    }
}

#[cfg(test)]
mod controller_option_tests {
    use super::*;

    #[test]
    fn private_prefix_modes_flags_and_unknown_tail_roundtrip() {
        for (raw, mode) in [(0u32, SustainPedalMode::SustainPlusController),
            (1, SustainPedalMode::SustainWithoutController), (2, SustainPedalMode::ControllerOnly)] {
            let mut bytes = raw.to_le_bytes().to_vec();
            bytes.extend([0, 0xff]); // Native action flags use nonzero, not == 1.
            bytes.extend((-6i32).to_le_bytes());
            bytes.extend([0xde, 0xad, 0, 0xff]);
            for version in [0x80, 0x82, 0x90, 0x92, 0xa0, 0xae, 0xb5] {
                let record = ProgramDataPrivateParams::read(Cursor::new(&bytes), version).unwrap();
                assert_eq!(record.controller_options().unwrap(), Some(ProgramControllerOptions {
                    sustain_mode: mode, standard_volume_pan: false,
                    standard_note_modes: true, cc7_max_db: -6 }));
                let mut written = Vec::new();
                record.write(&mut written).unwrap();
                assert_eq!(written, bytes);
                assert_eq!(record.version, version);
            }
        }
    }

    #[test]
    fn unknown_versions_and_malformed_prefixes_stay_opaque_without_panics() {
        let record = ProgramDataPrivateParams::read(Cursor::new([0xff, 0]), 0x81).unwrap();
        assert_eq!(record.controller_options().unwrap(), None);
        let mut written = Vec::new(); record.write(&mut written).unwrap();
        assert_eq!(written, [0xff, 0]);
        let mut valid = 0u32.to_le_bytes().to_vec(); valid.extend([1, 1]);
        valid.extend(0i32.to_le_bytes());
        for len in 0..valid.len() {
            let record = ProgramDataPrivateParams::read(Cursor::new(&valid[..len]), 0x80).unwrap();
            assert!(record.controller_options().is_err());
            let mut written = Vec::new(); record.write(&mut written).unwrap();
            assert_eq!(written, valid[..len]);
        }
        valid[..4].copy_from_slice(&3u32.to_le_bytes());
        let record = ProgramDataPrivateParams::read(Cursor::new(valid), 0x80).unwrap();
        assert!(record.controller_options().is_err());
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    #[test]
    fn test_private_params_v80() -> Result<(), Error> {
        let mut file = File::open("tests/data/Objects/Kontakt/Program/v80/private_params/000")?;
        let _params = ProgramDataPrivateParams::read(&mut file, 0x80)?;
        Ok(())
    }
}
