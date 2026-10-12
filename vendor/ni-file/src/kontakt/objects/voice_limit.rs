use crate::{read_bytes::ReadBytesExt, Error};

/// BVoiceLimit (SerType 0x2b), retaining names and signed scalar values.
#[derive(Debug)]
pub struct VoiceLimit {
    pub name: String,
    /// Method to decide which voices will be killed.
    /// - Options: Any, Oldest, Newest, Highest, Lowest
    /// - Default: Oldest
    pub kill_mode: i16,
    /// Prefer to keep already released voices.
    /// The wire byte is normalized to false for 0 and true for any nonzero value.
    /// Retain the original Chunk for byte-exact serialization.
    /// - Default: true
    pub prefer_released: bool,
    /// Maximum number of voices that can be used by this voice group.
    /// - Default: 1
    pub max_num_voices: i32,
    /// Time in ms for stolen voices to fade out.
    /// - Default: 10
    pub ms_fade_time: i32,
    /// Kills playing samples in other exclusion groups.
    /// Native omitted-group default is -1; preserve the signed wire value.
    pub exclusion_group: i32,
}

impl VoiceLimit {
    /// Read an inline flag/version/body record, without a chunk ID or length.
    pub(crate) fn read_inline<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        if reader.read_u8()? != 0 {
            return Err(Error::Static("VoiceLimit structured flag must be 0"));
        }
        let version = reader.read_u16_le()?;
        if version != 0x60 {
            return Err(Error::context(
                "VoiceLimit".into(),
                Error::VersionMismatch {
                    expected: 0x60,
                    got: u32::from(version),
                },
            ));
        }
        Self::read(reader)
    }

    /// Read the v0x60 field body, after its inline flag/version header.
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        Ok(Self {
            name: reader.read_widestring_utf16()?,
            kill_mode: reader.read_i16_le()?,
            prefer_released: reader.read_u8()? != 0,
            max_num_voices: reader.read_i32_le()?,
            ms_fade_time: reader.read_i32_le()?,
            exclusion_group: reader.read_i32_le()?,
        })
    }
}
