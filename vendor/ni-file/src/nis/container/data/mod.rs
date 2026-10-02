pub mod item_data_header;
pub mod item_type;

pub use item_data_header::*;
pub use item_type::*;

use crate::{read_bytes::ReadBytesExt, Error};
use std::io::{Cursor, Write};

#[derive(Clone, Debug)]
pub struct ItemData {
    pub header: ItemDataHeader,
    pub inner: Option<Box<ItemData>>,
    pub data: Vec<u8>,
}

impl ItemData {
    pub fn encoded_len(&self) -> Result<u64, Error> {
        if self.header.version != 1
            || (self.header.item_type() == ItemType::Item) != self.inner.is_none()
        {
            return Err(Error::Static("Invalid NIS data layer"));
        }
        let inner_length = self
            .inner
            .as_ref()
            .map_or(Ok(0), |inner| inner.encoded_len())?;
        20u64
            .checked_add(self.data.len() as u64)
            .and_then(|n| n.checked_add(inner_length))
            .ok_or(Error::Static("NIS data layer size overflow"))
    }

    /// Preserve layer identifiers and opaque properties; recompute all layer lengths.
    pub fn write<W: Write + ?Sized>(&self, writer: &mut W) -> Result<(), Error> {
        writer.write_all(&self.encoded_len()?.to_le_bytes())?;
        let mut domain = self.header.domain_id;
        domain.reverse();
        writer.write_all(&domain)?;
        writer.write_all(&self.header.item_id.to_le_bytes())?;
        writer.write_all(&self.header.version.to_le_bytes())?;
        if let Some(inner) = &self.inner {
            inner.write(writer)?;
        }
        writer.write_all(&self.data)?;
        Ok(())
    }

    pub fn child(&self) -> Option<&ItemData> {
        self.inner.as_ref().map(Box::as_ref)
    }

    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let at = reader.stream_position()?;
        let header = ItemDataHeader::read(&mut reader).map_err(|e| Error::context(format!("NIS data header at offset {at}, expected 20 bytes/version 1"), e))?;
        let length = header
            .length
            .checked_sub(20)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or(Error::Static("Invalid NIS item data length"))?;

        let body = reader.read_bytes(length).map_err(|e| Error::context(format!("NIS {:?} data body at offset {}, declared length {length}/version {}", header.item_type(), at + 20, header.version), e))?;
        if header.item_type() == ItemType::Item {
            return Ok(Self {
                header,
                inner: None,
                data: body,
            });
        }
        let (domain, item) = (header.domain_id, header.item_id);
        let version = header.version;
        Self::read_body(header, &body).map_err(|e| Error::context(format!("NIS domain {} item 0x{item:08x} data body at offset {}, declared length {length}/version {version}", String::from_utf8_lossy(&domain), at + 20), e))
    }

    pub(crate) fn read_cursor(reader: &mut Cursor<&[u8]>) -> Result<Self, Error> {
        let at = reader.position();
        let header = ItemDataHeader::read(&mut *reader).map_err(|e| Error::context(format!("NIS data header at body-relative offset {at}, expected 20 bytes/version 1"), e))?;
        let length = header
            .length
            .checked_sub(20)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or(Error::Static("Invalid NIS item data length"))?;
        let (domain, item) = (header.domain_id, header.item_id);
        let version = header.version;
        let body = super::read_slice(reader, length).map_err(|e| Error::context(format!("NIS domain {} item 0x{item:08x} data body at body-relative offset {}, declared length {length}/version {version}", String::from_utf8_lossy(&domain), at + 20), e))?;
        Self::read_body(header, body).map_err(|e| Error::context(format!("NIS domain {} item 0x{item:08x} data body at body-relative offset {}, declared length {length}/version {version}", String::from_utf8_lossy(&domain), at + 20), e))
    }

    fn read_body(header: ItemDataHeader, body: &[u8]) -> Result<Self, Error> {
        let mut reader = Cursor::new(body);
        let inner = if header.item_type() == ItemType::Item {
            None
        } else {
            Some(Box::new(Self::read_cursor(&mut reader)?))
        };
        let remaining = body.len() - reader.position() as usize;
        let data = reader.read_bytes(remaining)?;
        Ok(Self {
            header,
            inner,
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    #[test]
    fn test_item_frame_read_000() -> Result<(), Error> {
        let file = File::open("tests/patchdata/NISD/ItemFrame/RepositoryRoot-000")?;
        let item = ItemData::read(file)?;

        assert_eq!(item.data.len(), 58);
        assert_eq!(item.header.item_type(), ItemType::RepositoryRoot);
        assert_eq!(
            item.inner.unwrap().header.item_type(),
            ItemType::Authorization
        );

        Ok(())
    }

    #[test]
    fn test_item_frame_read_001() -> Result<(), Error> {
        let file = File::open("tests/patchdata/NISD/ItemFrame/RepositoryRoot-001")?;
        let item = ItemData::read(file)?;

        assert_eq!(item.data.len(), 58);
        assert_eq!(item.header.item_type(), ItemType::RepositoryRoot);
        assert_eq!(
            item.inner.unwrap().header.item_type(),
            ItemType::Authorization
        );

        Ok(())
    }
}
