// Groups allow you to apply settings like effects, volume, panning, etc. to multiple samples at once rather than having to adjust each one individually.

use crate::{
    kontakt::{Chunk, KontaktError, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

use super::Group;

pub const KONTAKT_GROUPLIST_ID: u16 = 0x33;

/// Type:           Chunk
/// SerType:        0x33
/// Kontakt 7:      BProgram::readGroups()
/// KontaktIO:      GroupList
#[derive(Debug)]
pub struct GroupList {
    pub groups: Vec<Group>,
}

impl GroupList {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let count_at = reader.stream_position()?;
        let num_groups = reader.read_u32_le()?;
        let position = reader.stream_position()?;
        let end = reader.seek(std::io::SeekFrom::End(0))?;
        reader.seek(std::io::SeekFrom::Start(position))?;
        let available = end.saturating_sub(position);
        // Every existing envelope needs at least its flag and u16 version.
        // This is a framing lower bound, not a new group-version admission rule.
        let minimum = u64::from(num_groups) * 3;
        if minimum > available {
            return Err(Error::Generic(format!(
                "GroupList count {num_groups} at offset {count_at}: at least {minimum} bytes required, {available} available"
            )));
        }
        let mut groups = Vec::new();

        for index in 0..num_groups {
            let at = reader.stream_position()?;
            let group = StructuredObject::read(&mut reader).map_err(|e| {
                Error::context(
                    format!("GroupList group {index} of {num_groups} at offset {at}"),
                    e,
                )
            })?;
            groups.push(Group(group));
        }

        Ok(Self { groups })
    }
}

impl std::convert::TryFrom<&Chunk> for GroupList {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != KONTAKT_GROUPLIST_ID {
            return Err(KontaktError::IncorrectID {
                expected: KONTAKT_GROUPLIST_ID,
                got: chunk.id,
            }
            .into());
        }
        let mut reader = std::io::Cursor::new(&chunk.data);
        let groups = Self::read(&mut reader)?;
        let at = reader.position();
        if at != chunk.data.len() as u64 {
            return Err(Error::Generic(format!(
                "Kontakt GroupList chunk 0x0033: {} trailing bytes at body offset {at}",
                chunk.data.len() as u64 - at
            )));
        }
        Ok(groups)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Read};

    use super::*;

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_group_list_000() -> Result<(), Error> {
        let mut file = File::open("tests/data/Objects/Kontakt/GroupList/GroupList-000")?;

        GroupList::try_from(&Chunk::read(&mut file)?)?;

        // Ensure the read completed
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        assert_eq!(buf.len(), 0, "Excess data found: {} bytes", buf.len());

        Ok(())
    }

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_group_list_001() -> Result<(), Error> {
        let mut file = File::open("tests/data/Objects/Kontakt/GroupList/GroupList-001")?;

        GroupList::try_from(&Chunk::read(&mut file)?)?;

        // Ensure the read completed
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        assert_eq!(buf.len(), 0, "Excess data found: {} bytes", buf.len());

        Ok(())
    }
}
