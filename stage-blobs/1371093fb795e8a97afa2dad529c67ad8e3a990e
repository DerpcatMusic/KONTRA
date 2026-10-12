// SubtreeItem
//
// Properties
// - num-hidden-items

/*
    SubtreeItem (0x73, 115)
    appears on compressed segments

    u32  1
    bool is_compressed
    u32  decompressed_size
    u32  compressed_size
    &[compressed_size;u8] compressed_data

    SubtreeItem.readItem(&stream) {
        let header_item = Item::readItem(&stream)?;

        if stream.read_u32 != 0 {
            return Err(VERSION_MISMATCH);
        }

        let is_compressed = stream.read_bool();
        header_item[6] = is_compressed;

        if !is_compressed {
            eax_12 = Item::read();
            return;
        }

        let decompressed_size = stream.read_u32();
        let compressed_size = stream.read_u32();

        let mut buffer;
        let size = stream.read_raw(&buffer, compressed_size);

        if size != compressed_size {
            return Err(INTERNAL_ERROR);
        }

        if is_compressed {
            return SubtreeItem::decompressInputStream(&stream);
        }
    }
*/

use std::io::{Cursor, Write};

use crate::nis::{ItemContainer, ItemData, ItemType};
use crate::read_bytes::ReadBytesExt;
use crate::Error;

#[derive(Debug)]
pub struct SubtreeItem {
    pub inner_data: Vec<u8>,
}

impl std::convert::TryFrom<&ItemData> for SubtreeItem {
    type Error = Error;

    fn try_from(frame: &ItemData) -> Result<Self, Error> {
        debug_assert_eq!(frame.header.item_type(), ItemType::SubtreeItem);
        Self::read(Cursor::new(&frame.data))
    }
}

impl SubtreeItem {
    /// Decompress and return compressed internal Item.
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        Self::read_with_key(&mut reader, None)
    }

    pub fn read_with_key<R: ReadBytesExt>(
        mut reader: R,
        key: Option<&dyn LibraryKey>,
    ) -> Result<Self, Error> {
        if reader.read_u32_le()? != 1 {
            return Err(Error::Static("Unsupported subtree version"));
        }
        let inner_data = if reader.read_bool()? {
            let expanded = reader.read_u32_le()? as usize;
            if expanded == 0 || i32::try_from(expanded).is_err() {
                return Err(Error::Static("Invalid expanded subtree size"));
            }
            let compressed_size = reader.read_u32_le()? as usize;
            if i32::try_from(compressed_size).is_err() {
                return Err(Error::Static(
                    "Compressed subtree exceeds FastLZ length range",
                ));
            }
            let mut compressed = reader.read_bytes(compressed_size)?;
            if let Some(key) = key {
                key.apply(&mut compressed);
            }
            if compressed.is_empty() {
                return Err(Error::Static("Empty compressed subtree"));
            }
            let mut output = Vec::new();
            output
                .try_reserve_exact(expanded)
                .map_err(|_| Error::Static("Unable to allocate expanded subtree"))?;
            output.resize(expanded, 0);
            let result = fastlz::decompress(&compressed, &mut output)
                .map_err(|_| Error::Static("Invalid compressed subtree"))?;
            if result.len() != expanded {
                return Err(Error::Static(
                    "Subtree decompression length mismatch (invalid data or library key)",
                ));
            }
            output
        } else {
            if key.is_some() {
                return Err(Error::Static("Unsupported uncompressed encrypted subtree"));
            }
            let length = reader.read_u64_le()?;
            reader.seek(std::io::SeekFrom::Current(-8))?;
            reader.read_bytes(
                usize::try_from(length).map_err(|_| Error::Static("Subtree too large"))?,
            )?
        };

        Ok(SubtreeItem { inner_data })
    }

    /// Encode the subtree, optionally compressing and applying a caller-provided
    /// symmetric keystream. Encryption markers in the containing item must match.
    pub fn write_with_key<W: Write>(
        &self,
        mut writer: W,
        compressed: bool,
        key: Option<&dyn LibraryKey>,
    ) -> Result<(), Error> {
        if !compressed && key.is_some() {
            return Err(Error::Static("Unsupported uncompressed encrypted subtree"));
        }
        // Validate that the body is one complete NIS item before writing any bytes.
        let mut reader = Cursor::new(self.inner_data.as_slice());
        ItemContainer::read_cursor(&mut reader)?;
        if reader.position() != self.inner_data.len() as u64 {
            return Err(Error::Static("Trailing data after subtree item"));
        }
        let packed = if compressed {
            if self.inner_data.is_empty() || i32::try_from(self.inner_data.len()).is_err() {
                return Err(Error::Static("Invalid expanded subtree size"));
            }
            // FastLZ needs >5% scratch space and returns its output size as a signed int.
            let capacity = self
                .inner_data
                .len()
                .checked_add(self.inner_data.len() / 16)
                .and_then(|n| n.checked_add(128))
                .filter(|&n| i32::try_from(n).is_ok())
                .ok_or(Error::Static(
                    "Compressed subtree exceeds FastLZ length range",
                ))?;
            let mut output = Vec::new();
            output
                .try_reserve_exact(capacity)
                .map_err(|_| Error::Static("Unable to allocate compressed subtree"))?;
            output.resize(capacity, 0);
            let length = fastlz::compress(&self.inner_data, &mut output)
                .map_err(|_| Error::Static("Subtree compression failed"))?
                .len();
            output.truncate(length);
            if let Some(key) = key {
                key.apply(&mut output);
            }
            Some(output)
        } else {
            None
        };
        writer.write_all(&1u32.to_le_bytes())?;
        writer.write_all(&[compressed as u8])?;
        if let Some(packed) = packed {
            writer.write_all(&(self.inner_data.len() as u32).to_le_bytes())?;
            writer.write_all(&(packed.len() as u32).to_le_bytes())?;
            writer.write_all(&packed)?;
        } else {
            writer.write_all(&self.inner_data)?;
        }
        Ok(())
    }

    pub fn item(&self) -> Result<ItemContainer, Error> {
        let container = ItemContainer::read_cursor(&mut Cursor::new(self.inner_data.as_slice()))?;
        Ok(container)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Read};

    use super::*;

    #[test]
    fn test_read_subtree() -> Result<(), Error> {
        let mut data = File::open("tests/data/Containers/NIS/objects/SubtreeItem/SubtreeItem-000")?;
        let subtree = SubtreeItem::read(&mut data)?;

        assert_eq!(subtree.inner_data.len(), 4524);
        let item = subtree.item()?;

        assert_eq!(item.id(), ItemType::Item);

        // Ensure the read completed
        let mut buf = Vec::new();
        data.read_to_end(&mut buf)?;
        assert_eq!(buf.len(), 0, "Excess data found");

        Ok(())
    }
}

/// A caller-supplied keystream for encrypted presets and archive members.
/// ni-file derives no keys; implementations live outside this crate.
pub trait LibraryKey: Send + Sync {
    /// XOR the symmetric keystream into bytes starting `offset` bytes into the resource.
    /// Applying the same stream twice restores the original bytes.
    fn apply_at(&self, offset: u64, bytes: &mut [u8]);
    fn apply(&self, bytes: &mut [u8]) {
        self.apply_at(0, bytes);
    }
}
