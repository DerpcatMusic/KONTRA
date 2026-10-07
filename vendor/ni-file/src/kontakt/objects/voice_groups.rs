use std::io::Cursor;

use crate::{
    Error,
    kontakt::{Chunk, KontaktError, objects::voice_limit::VoiceLimit},
    read_bytes::ReadBytesExt,
};

use super::VoiceGroup;

const MAX_VOICE_GROUPS: usize = 128;

/// An array of VoiceGroups.
///
/// - SerType:        0x32
/// - Known Versions: 0x60
/// - Kontakt 7:      BProgram::readVoiceGroups()
/// - KontaktIO:      VoiceGroups
///
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceGroups {
    pub voice_limit: VoiceLimit,
    pub groups: Vec<Option<VoiceGroup>>,
}

impl VoiceGroups {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        // The first record is the program's voice limit, not an array header.
        let voice_limit = VoiceGroup::read(&mut reader)?.0;
        let indexes = reader.read_bytes(MAX_VOICE_GROUPS / 8)?;
        let mut groups = Vec::with_capacity(MAX_VOICE_GROUPS);
        for i in 0..MAX_VOICE_GROUPS {
            groups.push(if indexes[i / 8] & (1 << (i % 8)) != 0 {
                Some(VoiceGroup::read(&mut reader)?)
            } else {
                None
            });
        }
        if !reader.read_all()?.is_empty() {
            return Err(Error::Static("Trailing voice group data"));
        }

        Ok(Self {
            voice_limit,
            groups,
        })
    }
}

impl std::convert::TryFrom<&Chunk> for VoiceGroups {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != 0x32 {
            return Err(KontaktError::IncorrectID {
                expected: 0x32,
                got: chunk.id,
            }
            .into());
        }
        let reader = Cursor::new(&chunk.data);
        Self::read(reader)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Read};

    use super::*;

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_voice_groups_v60() -> Result<(), Error> {
        let mut file = File::open("tests/data/Objects/Kontakt/VoiceGroups/v60/000")?;

        VoiceGroups::read(&mut file)?;

        // Ensure the read completed
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        assert_eq!(buf.len(), 0, "Excess data found: {} bytes", buf.len());

        Ok(())
    }
}
