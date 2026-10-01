use std::io::Cursor;

use crate::{
    nis::{ItemData, ItemType},
    read_bytes::ReadBytesExt,
    NIFileError,
};

use super::subtree_item::SubtreeItem;

/// A container for compressed presets.
#[derive(Debug)]
pub struct EncryptionItem {
    pub subtree: SubtreeItem,
    pub is_encrypted: bool,
}

impl std::convert::TryFrom<&ItemData> for EncryptionItem {
    type Error = NIFileError;

    fn try_from(frame: &ItemData) -> Result<Self, NIFileError> {
        debug_assert_eq!(frame.header.item_type(), ItemType::EncryptionItem);

        Self::read_with_key(frame, None)
    }
}

impl EncryptionItem {
    pub fn read_with_key(
        frame: &ItemData,
        key: Option<&dyn super::subtree_item::LibraryKey>,
    ) -> Result<Self, NIFileError> {
        let subtree_frame = frame
            .child()
            .ok_or(NIFileError::Static("Missing preset subtree"))?;
        let mut reader = Cursor::new(&frame.data);
        if reader.read_u32_le()? != 1 {
            return Err(NIFileError::Static("Unsupported encryption item version"));
        }
        let is_encrypted = reader.read_bool()?;
        if is_encrypted && key.is_none() {
            return Err(NIFileError::Static(
                "Encrypted preset needs local library access data",
            ));
        }
        Ok(Self {
            subtree: SubtreeItem::read_with_key(
                Cursor::new(&subtree_frame.data),
                if is_encrypted { key } else { None },
            )?,
            is_encrypted,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Read};

    use super::*;

    #[test]
    fn test_authorization_read() -> Result<(), NIFileError> {
        let mut file =
            File::open("tests/data/Containers/NIS/objects/EncryptionItem/000-EncryptionItem")?;

        let item = ItemData::read(&file)?;
        let _enc = EncryptionItem::try_from(&item)?;

        // ensure the read completed
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        assert_eq!(buf.len(), 0, "Excess data found");

        Ok(())
    }
}
