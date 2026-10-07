use crate::{Error, read_bytes::ReadBytesExt};

// BVoiceLimit id: 0x2B
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceLimit {
    pub name: String,
    /// Method to decide which voices will be killed.
    /// - Options: Any, Oldest, Newest, Highest, Lowest
    /// - Default: Oldest
    pub kill_mode: i16,
    /// Prefer already released voices when stealing.
    /// - Default: true
    pub prefer_released: bool,
    /// Maximum number of voices that can be used by this voice group.
    /// - Default: 1
    pub max_num_voices: i32,
    /// Time in ms for stolen voices to fade out.
    /// - Default: 10
    pub ms_fade_time: i32,
    /// Native exclusion class; negative values disable it in the v1 importer.
    /// Playback/choke semantics require native validation.
    pub exclusion_group: i32,
}

impl VoiceLimit {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        Ok(Self {
            name: reader.read_widestring_utf16()?,
            kill_mode: reader.read_i16_le()?,
            prefer_released: match reader.read_u8()? {
                0 => false,
                1 => true,
                _ => return Err(Error::Static("Invalid voice limit prefer-released flag")),
            },
            max_num_voices: reader.read_i32_le()?,
            ms_fade_time: reader.read_i32_le()?,
            exclusion_group: reader.read_i32_le()?,
        })
    }
}
