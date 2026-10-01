use std::io::{Cursor, Write};

use crate::{read_bytes::ReadBytesExt, Error, NIFileError};

use super::{ItemData, ItemHeader, ItemType};

/// NISound documents are made up of nested [`Item`]s.
#[derive(Clone, Debug)]
pub struct ItemContainer {
    pub header: ItemHeader,
    pub data: ItemData,
    pub children: Vec<ItemContainer>,
    /// Raw sibling-index, domain and item-id records, one per child.
    pub child_headers: Vec<[u8; 12]>,
    /// Uninterpreted bytes after the child table.
    pub trailing_data: Vec<u8>,
}

impl ItemContainer {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let header = ItemHeader::read(&mut reader)?;
        let length = header
            .length
            .checked_sub(40)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or(Error::Static("Invalid NIS item length"))?;
        let mut chunk_data = Cursor::new(reader.read_bytes(length)?);

        let data = ItemData::read(&mut chunk_data)?;
        let (children, child_headers) = Self::read_children(&mut chunk_data)?;
        let trailing_data = chunk_data.read_all()?;
        Ok(Self {
            header,
            data,
            children,
            child_headers,
            trailing_data,
        })
    }

    /// Serialized size, recomputed from the current data rather than cached headers.
    pub fn encoded_len(&self) -> Result<u64, Error> {
        if self.header.magic != b"hsin" || self.header.uuid.len() != 16 {
            return Err(Error::Static("Invalid NIS item header"));
        }
        if self.child_headers.len() != self.children.len()
            || u32::try_from(self.children.len()).is_err()
        {
            return Err(Error::Static("Invalid NIS child table"));
        }
        let mut length = 48u64
            .checked_add(self.data.encoded_len()?)
            .and_then(|n| n.checked_add(self.trailing_data.len() as u64))
            .ok_or(Error::Static("NIS item size overflow"))?;
        for child in &self.children {
            let child_length = child.encoded_len()?;
            length = length
                .checked_add(12)
                .and_then(|n| n.checked_add(child_length))
                .ok_or(Error::Static("NIS item size overflow"))?;
        }
        Ok(length)
    }

    /// Write a lossless NIS container representation, including opaque encrypted data.
    /// This does not regenerate preset checksums after editing internal preset bytes.
    pub fn write<W: Write + ?Sized>(&self, writer: &mut W) -> Result<(), Error> {
        writer.write_all(&self.encoded_len()?.to_le_bytes())?;
        writer.write_all(&1u32.to_le_bytes())?;
        writer.write_all(&self.header.magic)?;
        writer.write_all(&self.header.header_flags.to_le_bytes())?;
        writer.write_all(&self.header.reserved.to_le_bytes())?;
        writer.write_all(&self.header.uuid)?;
        self.data.write(writer)?;
        writer.write_all(&1u32.to_le_bytes())?;
        writer.write_all(&(self.children.len() as u32).to_le_bytes())?;
        for (header, child) in self.child_headers.iter().zip(&self.children) {
            writer.write_all(header)?;
            child.write(writer)?;
        }
        writer.write_all(&self.trailing_data)?;
        Ok(())
    }

    pub fn first_child(&self) -> Option<&ItemContainer> {
        self.children.get(0)
    }

    pub fn id(&self) -> ItemType {
        self.data.header.item_type()
    }

    /// Returns the first instance of Item by ItemID within child Items.
    pub fn find(&self, kind: &ItemType) -> Option<&ItemContainer> {
        // Check this Item first
        if &self.data.header.item_type() == kind {
            return Some(&self);
        }
        // Recursively search the children
        for item in &self.children {
            if let Some(frame) = item.find(kind) {
                return Some(frame);
            }
        }
        None
    }

    /// Returns the first instance of Item by ItemID within child Items.
    pub fn find_data(&self, kind: &ItemType) -> Option<&ItemData> {
        // Check this Item first
        if &self.data.header.item_type() == kind {
            return Some(&self.data);
        }
        // Recursively search the children
        for item in &self.children {
            if let Some(frame) = item.find_data(kind) {
                return Some(frame);
            }
        }
        None
    }

    /// Find the first Item of type ItemID in the document and return it
    pub fn find_item<'a, I>(&'a self, kind: &'a ItemType) -> Option<Result<I, Error>>
    where
        I: TryFrom<&'a ItemData, Error = NIFileError>,
    {
        self.find_data(&kind).map(I::try_from)
    }

    fn read_children<R: ReadBytesExt>(
        mut buf: R,
    ) -> Result<(Vec<ItemContainer>, Vec<[u8; 12]>), Error> {
        let version = buf.read_u32_le()?;
        if version != 1 {
            return Err(Error::VersionMismatch {
                expected: 1,
                got: version,
            });
        }
        let num_children = buf.read_u32_le()?;
        let mut children = Vec::new();
        let mut headers = Vec::new();
        for _ in 0..num_children {
            let mut header = [0; 12];
            buf.read_exact(&mut header)?;
            children.push(ItemContainer::read(&mut buf)?);
            headers.push(header);
        }
        Ok((children, headers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn test_item_read() -> Result<(), Error> {
        let data = File::open("test-data/NIS/Item/BNISoundPreset/BNISoundPreset-000")?;
        let item = ItemContainer::read(data)?;
        assert_eq!(item.children.len(), 0);
        Ok(())
    }

    #[test]
    fn test_item_with_children_read() -> Result<(), Error> {
        let data = File::open("tests/filetype/NISD/kontakt/7.1.3.0/000-default.nki")?;
        let item = ItemContainer::read(data)?;
        assert_eq!(item.children.len(), 1);
        Ok(())
    }
}
