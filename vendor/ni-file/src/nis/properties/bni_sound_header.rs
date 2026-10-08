use std::io::Cursor;

use crate::{
    kontakt::objects::BPatchHeaderV42,
    nis::{ItemData, ItemType},
    read_bytes::ReadBytesExt,
    Error, NIFileError,
};

/// Kontakt header
#[derive(Debug)]
pub struct BNISoundHeader(pub BPatchHeaderV42);

impl BNISoundHeader {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let magic = reader.read_u32_le()?;
        if magic != 0x7fa89012 {
            return Err(Error::Generic(format!(
                "Invalid BNISoundHeader magic: 0x{magic:08x}"
            )));
        }
        let _zlib_length = reader.read_u32_le()?;
        let header_version = reader.read_u16_le()?;
        if header_version != 0x0110 {
            return Err(Error::context(
                "BNISoundHeader".into(),
                Error::VersionMismatch {
                    expected: 0x0110,
                    got: u32::from(header_version),
                },
            ));
        }
        Ok(Self(BPatchHeaderV42::read_le(&mut reader)?))
    }
}

impl std::convert::TryFrom<&ItemData> for BNISoundHeader {
    type Error = NIFileError;

    fn try_from(frame: &ItemData) -> Result<Self, NIFileError> {
        let got = frame.header.item_type();
        if got != ItemType::BNISoundHeader {
            return Err(NIFileError::ItemWrapError {
                expected: ItemType::BNISoundHeader,
                got,
            });
        }
        Self::read(Cursor::new(&frame.data))
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_bni_sound_header_read() -> Result<(), Error> {
        let file =
            File::open("tests/data/Containers/NIS/objects/BNISoundHeader/BNISoundHeader-000")?;
        println!("{:?}", BNISoundHeader::read(file)?);
        Ok(())
    }
}
