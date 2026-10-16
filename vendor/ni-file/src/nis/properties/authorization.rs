// Authorization
//
// Properties
// - LicenseInfo
// - authorizaton-level

use std::io::Cursor;

use crate::{
    nis::{ItemData, ItemType},
    read_bytes::ReadBytesExt,
    NIFileError,
};

#[derive(Debug)]
pub struct Authorization {
    level: u32,
    read_checksum: u32,
}

impl std::convert::TryFrom<&ItemData> for Authorization {
    type Error = NIFileError;

    fn try_from(frame: &ItemData) -> std::result::Result<Self, Self::Error> {
        if frame.header.item_type() != ItemType::Authorization {
            return Err(NIFileError::ItemWrapError {
                expected: ItemType::Authorization,
                got: frame.header.item_type(),
            });
        }
        Authorization::read(Cursor::new(&frame.data))
    }
}

impl Authorization {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, NIFileError> {
        // version == 1
        let version = reader.read_u32_le()?;
        if version != 1 {
            return Err(NIFileError::VersionMismatch { expected: 1, got: version });
        }

        let a = reader.read_u32_le()?; // 0x18
        if a == 1 {
            // LicenseInfo::read
            let num_snpids = reader.read_u32_le()?;
            for _ in 0..num_snpids {
                let snp_id = reader.read_widestring_utf16()?;
                if !snp_id.is_empty() {
                    return Err(NIFileError::InvalidField {
                        field: "Authorization SNP ID",
                        expected: "empty string (supported LicenseInfo representation)",
                        actual: snp_id,
                    });
                }
            }
        }

        reader.read_u32_le()?;

        // authorizationLevel
        let level = reader.read_u32_le()?; // 24, default: 1
        let read_checksum = reader.read_u32_le()?; // 28, default: 0

        Ok(Authorization {
            level,
            read_checksum,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    fn encode(version: u32, license_info: bool, snp: &str, level: u32, checksum: u32) -> Vec<u8> {
        let mut words = vec![version, if license_info { 1 } else { 0 }];
        let mut bytes = Vec::new();
        if license_info {
            words.push(1);
            words.push(snp.encode_utf16().count() as u32);
        }
        for word in words { bytes.extend(word.to_le_bytes()); }
        if license_info { for c in snp.encode_utf16() { bytes.extend(c.to_le_bytes()); } }
        for word in [0u32, level, checksum] { bytes.extend(word.to_le_bytes()); }
        bytes
    }

    #[test]
    fn authorization_supported_layouts_round_trip() {
        for license_info in [false, true] {
            let bytes = encode(1, license_info, "", 24, 0x1234);
            let mut reader = Cursor::new(&bytes);
            let parsed = Authorization::read(&mut reader).unwrap();
            assert_eq!((parsed.level, parsed.read_checksum), (24, 0x1234));
            assert_eq!(reader.position() as usize, bytes.len());
            assert_eq!(encode(1, license_info, "", parsed.level, parsed.read_checksum), bytes);
            for n in 0..bytes.len() { assert!(Authorization::read(Cursor::new(&bytes[..n])).is_err()); }
        }
    }

    #[test]
    fn authorization_rejects_unsupported_version_and_license_info() {
        assert!(matches!(Authorization::read(Cursor::new(encode(2, false, "", 1, 0))),
            Err(NIFileError::VersionMismatch { expected: 1, got: 2 })));
        assert!(matches!(Authorization::read(Cursor::new(encode(1, true, "abc", 1, 0))),
            Err(NIFileError::InvalidField { field: "Authorization SNP ID", .. })));
    }

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_authorization_read() -> Result<(), NIFileError> {
        let file = File::open(
            "tests/data/nisound/chunks/item-frame-property/kontakt-5/106-Authorization.data",
        )?;

        let _auth = Authorization::read(&file)?;

        Ok(())
    }
}
