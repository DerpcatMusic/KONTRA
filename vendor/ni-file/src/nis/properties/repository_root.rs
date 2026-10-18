use std::{fmt::Display, io::Cursor};

use crate::{
    nis::{ItemData, ItemType},
    read_bytes::ReadBytesExt,
    NIFileError,
};

/// Usually the top-level [`Item`][crate::nisound::Item] of a repository. Contains NISound version information.
#[derive(Debug)]
pub struct RepositoryRoot {
    pub nisound_version: RepositoryVersion,
    /// Found values: 0
    pub repository_magic: u32,
    /// Found values: 1, 2
    pub repository_type: u32,
    pub segments: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub struct RepositoryVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl From<u32> for RepositoryVersion {
    fn from(i: u32) -> Self {
        RepositoryVersion {
            major: (i >> 0x14) & 0xff,
            minor: (i >> 0xc) & 0xff,
            patch: i & 0xfff,
        }
    }
}

impl Display for RepositoryVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}.{}.{}", self.major, self.minor, self.patch))
    }
}

impl std::convert::TryFrom<&ItemData> for RepositoryRoot {
    type Error = NIFileError;

    fn try_from(item: &ItemData) -> std::result::Result<Self, Self::Error> {
        if item.header.item_type() != ItemType::RepositoryRoot {
            return Err(NIFileError::ItemWrapError {
                expected: ItemType::RepositoryRoot,
                got: item.header.item_type(),
            });
        }
        RepositoryRoot::read(Cursor::new(item.data.clone()))
    }
}

impl RepositoryRoot {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, NIFileError> {
        // itemVersion == 1
        let version = reader.read_u32_le()?;
        if version != 1 {
            return Err(NIFileError::VersionMismatch { expected: 1, got: version });
        }

        let nisound_version = RepositoryVersion::from(reader.read_u32_le()?);
        let repository_magic = reader.read_u32_le()?;
        let repository_type = reader.read_u32_le()?;

        let segments_version = reader.read_u32_le()?;
        if segments_version != 1 {
            return Err(NIFileError::InvalidField {
                field: "RepositoryRoot segments version",
                expected: "1",
                actual: segments_version.to_string(),
            });
        }

        let num_segments = reader.read_u32_le()?;
        let mut segments = Vec::new();
        for _ in 0..num_segments {
            segments.push(reader.read_widestring_utf16()?);
        }

        // // SNPID::read
        // let a = reader.read_widestring_utf16()?;
        // // assert_eq!(a, "");
        //
        // // UUID::read
        // let a = reader.read_widestring_utf16()?;
        // // assert_eq!(a, "");
        //
        // let a = reader.read_u64_le()?;
        //
        // let mut buf = Vec::new();
        // reader.read_to_end(&mut buf)?;
        //
        // dbg!(&buf, buf.len());

        // repository-type
        // referenced-item-uuid

        Ok(Self {
            nisound_version,
            repository_magic,
            repository_type,
            segments,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    fn encode(item: &RepositoryRoot) -> Vec<u8> {
        let version = &item.nisound_version;
        let mut bytes = Vec::new();
        for word in [1, (version.major << 20) | (version.minor << 12) | version.patch,
                     item.repository_magic, item.repository_type, 1, item.segments.len() as u32] {
            bytes.extend(word.to_le_bytes());
        }
        for segment in &item.segments {
            bytes.extend((segment.encode_utf16().count() as u32).to_le_bytes());
            for c in segment.encode_utf16() { bytes.extend(c.to_le_bytes()); }
        }
        bytes
    }

    #[test]
    fn repository_segments_round_trip_and_truncations_fail() {
        let source = RepositoryRoot {
            nisound_version: RepositoryVersion { major: 1, minor: 7, patch: 14 },
            repository_magic: 0, repository_type: 2,
            segments: vec!["".into(), "Libraries".into(), "音楽🎹".into()],
        };
        let bytes = encode(&source);
        let mut reader = Cursor::new(&bytes);
        let item = RepositoryRoot::read(&mut reader).unwrap();
        assert_eq!(item.segments, source.segments);
        assert_eq!(item.nisound_version, source.nisound_version);
        assert_eq!(encode(&item), bytes);
        assert_eq!(reader.position() as usize, bytes.len());
        for n in 0..bytes.len() { assert!(RepositoryRoot::read(Cursor::new(&bytes[..n])).is_err()); }
        for (offset, segment_version) in [(0, false), (16, true)] {
            let mut invalid = bytes.clone();
            invalid[offset..offset+4].copy_from_slice(&2u32.to_le_bytes());
            let err = RepositoryRoot::read(Cursor::new(invalid)).unwrap_err();
            if segment_version { assert!(matches!(err, NIFileError::InvalidField { field: "RepositoryRoot segments version", .. })); }
            else { assert!(matches!(err, NIFileError::VersionMismatch { expected: 1, got: 2 })); }
        }
    }

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_repository_root_read_000() -> Result<(), NIFileError> {
        let file =
            File::open("tests/data/Containers/NIS/objects/RepositoryRoot/RepositoryRoot-000")?;
        let item = RepositoryRoot::read(file)?;

        assert_eq!(
            item.nisound_version,
            RepositoryVersion {
                major: 1,
                minor: 7,
                patch: 14,
            }
        );
        assert_eq!(0, item.repository_magic);
        assert_eq!(1, item.repository_type);

        Ok(())
    }

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_repository_root_read_001() -> Result<(), NIFileError> {
        let file =
            File::open("tests/data/Containers/NIS/objects/RepositoryRoot/RepositoryRoot-001")?;
        let item = RepositoryRoot::read(file)?;

        assert_eq!(
            item.nisound_version,
            RepositoryVersion {
                major: 1,
                minor: 4,
                patch: 0,
            }
        );
        assert_eq!(0, item.repository_magic);
        assert_eq!(2, item.repository_type);

        Ok(())
    }
}
