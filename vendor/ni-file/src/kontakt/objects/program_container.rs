use std::io::Cursor;

use crate::{
    kontakt::{Chunk, KontaktError, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

use super::{ProgramList, VoiceGroup};

const CHUNK_ID: u16 = 0x29;

/// # ProgramContainer
///
/// - Type:           Chunk<StructuredObject>
/// - SerType:        0x29
/// - Versions:       0x50, 0x51
/// - Kontakt 7:      BProgramContainer
/// - KontaktIO:      K4PL\_ProgramContainer
///
#[derive(Debug)]
pub struct ProgramContainer(pub StructuredObject);

#[derive(Debug)]
pub struct ProgramContainerParams {
    pub name: String,
    pub volume: f32,
    pub pan: f32,
}

/// The verified v0x51 bank-container subset with an empty parameter vector.
/// The full private bytes remain available on `ProgramContainer::0`.
#[derive(Debug)]
pub struct ProgramContainerPrivateParams {
    pub mode: u32,
    pub flags: [u8; 4],
    pub selected: u16,
    /// High word: output channel count; low word: same-count record ordinal.
    pub output_geometry: u32,
}

impl ProgramContainer {
    /// Read the common v0x50/v0x51 selector prefix without interpreting
    /// the remaining private controls or admitting their playback policy.
    pub fn mode(&self) -> Result<u32, Error> {
        if !matches!(self.0.version, 0x50 | 0x51) {
            return Err(Error::Static("Unsupported ProgramContainer mode version"));
        }
        let mut reader = Cursor::new(&self.0.private_data);
        let mode = reader.read_u32_le()?;
        for _ in 0..4 { reader.read_u8()?; }
        reader.read_u16_le()?;
        Ok(mode)
    }

    pub fn params(&self) -> Result<ProgramContainerParams, Error> {
        let mut reader = Cursor::new(&self.0.public_data);

        let params = ProgramContainerParams {
            name: reader.read_widestring_utf16()?,
            volume: reader.read_f32_le()?,
            pan: reader.read_f32_le()?,
        };
        if reader.position() != self.0.public_data.len() as u64 {
            return Err(Error::Static("Unsupported ProgramContainer public trailing data"));
        }
        Ok(params)
    }

    pub fn private_params(&self) -> Result<ProgramContainerPrivateParams, Error> {
        if self.0.version != 0x51 {
            return Err(Error::VersionMismatch { expected: 0x51, got: self.0.version.into() });
        }
        let mut reader = Cursor::new(&self.0.private_data);
        let mode = reader.read_u32_le()?;
        let flags = [reader.read_u8()?, reader.read_u8()?, reader.read_u8()?, reader.read_u8()?];
        let selected = reader.read_u16_le()?;
        if reader.read_u32_le()? != 0 {
            return Err(Error::Static("Nonempty ProgramContainer parameter vector is unsupported"));
        }
        let output_geometry = reader.read_u32_le()?;
        if reader.position() != self.0.private_data.len() as u64 {
            return Err(Error::Static("Unsupported ProgramContainer private trailing data"));
        }
        Ok(ProgramContainerPrivateParams { mode, flags, selected, output_geometry })
    }

    pub fn voice_group(&self) -> Result<VoiceGroup, Error> {
        self.0.find_first(0x2b).ok_or(KontaktError::MissingChunk(0x2b))?.try_into()
    }

    // pub fn save_settings(&self) -> Result<SaveSettings, Error> {
    //     (&self.0.children[1]).try_into()
    // }

    pub fn program_list(&self) -> Result<ProgramList, Error> {
        self.0.find_first(0x36).ok_or(KontaktError::MissingChunk(0x36))?.try_into()
    }

    // pub fn programs(&self) -> Result<Vec<VoiceGroup>, Error> {
    //     self.0.children.iter().map(VoiceGroup::try_from).collect()
    // }
}

impl std::convert::TryFrom<&Chunk> for ProgramContainer {
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
    use crate::Error;
    use std::fs::File;

    #[test]
    fn test_bank() -> Result<(), Error> {
        let chunk = Chunk::read(File::open(
            "tests/data/Objects/Kontakt/0x29-ProgramContainer/ProgramContainerV51-000.kon",
        )?)?;
        let pc = ProgramContainer::try_from(&chunk)?;

        assert_eq!(pc.0.version, 0x51);
        assert_eq!(pc.0.children.len(), 3);
        assert_eq!(pc.0.children[0].id, 0x2B); // VoiceGroup
        assert_eq!(pc.0.children[1].id, 0x47); // SaveSettings
        assert_eq!(pc.0.children[2].id, 0x36); // ProgramList

        pc.params()?;
        pc.voice_group()?;
        pc.program_list()?;
        // println!("{:X}", pc.0.children[2].id);
        Ok(())
    }
}
