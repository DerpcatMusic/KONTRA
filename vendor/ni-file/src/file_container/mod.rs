use crate::read_bytes::ReadBytesExt;
use crate::string_reader::StringReader;
use crate::Error;

const FC_TOC_MARKER_END: u64 = 0xF1F1F1F1F1F1F1F1;
const FC_MTD_MARKER_START: &[u8; 16] = b"/\\ NI FC MTD  /\\";

/// Kontakt archive that bundles a preset, samples and other files.
pub struct NIFileContainer {
    pub file_section_offset: u64,
    pub items: Vec<FileContainerItem>,
}

pub struct FileContainerItem {
    pub index: u64,
    pub filename: String,
    pub file_start_offset: u64,
    pub file_size: u64,
}

impl NIFileContainer {
    /// Read a member by its stored index, bounded by the caller's allocation limit.
    /// `reader` must refer to the same container; its cursor ends after the member.
    pub fn read_member<R: ReadBytesExt>(
        &self,
        mut reader: R,
        index: u64,
        limit: u64,
    ) -> Result<Vec<u8>, Error> {
        let mut matching = self.items.iter().filter(|item| item.index == index);
        let item = matching
            .next()
            .ok_or(Error::Static("NI FileContainer member index not found"))?;
        if matching.next().is_some() {
            return Err(Error::Static("Ambiguous NI FileContainer member index"));
        }
        if item.file_size > limit {
            return Err(Error::Static("NI FileContainer member exceeds read limit"));
        }
        let offset = self
            .file_section_offset
            .checked_add(item.file_start_offset)
            .ok_or(Error::Static("NI FileContainer member offset overflow"))?;
        let size = usize::try_from(item.file_size)
            .map_err(|_| Error::Static("NI FileContainer member exceeds address space"))?;
        reader.seek(std::io::SeekFrom::Start(offset))?;
        Ok(reader.read_bytes(size)?)
    }

    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        // NI FC MTD
        // Native Instruments FileContainer MetaData
        let mtd_magic = reader.read_bytes(16)?;
        if mtd_magic != FC_MTD_MARKER_START {
            return Err(Error::Static("Invalid NI FileContainer metadata marker"));
        }

        let _header_chunk = reader.read_bytes(256)?;
        let file_count = reader.read_u64_le()?;
        let total_size = reader.read_u64_le()?;
        let position = reader.stream_position()?;
        let length = reader.seek(std::io::SeekFrom::End(0))?;
        reader.seek(std::io::SeekFrom::Start(position))?;
        if file_count > 100_000 || file_count > length.saturating_sub(position) / 640 {
            return Err(Error::Static("Invalid NI FileContainer file count"));
        }

        // NI FC TOC
        // Native Instruments FileContainer Table Of Contents
        // Table 1
        let mtd_magic = reader.read_bytes(16)?;
        if mtd_magic != b"/\\ NI FC TOC  /\\" {
            return Err(Error::Static("Invalid NI FileContainer table marker"));
        }

        let _header_chunk = reader.read_bytes(600)?;

        let mut offset: u64 = 0;
        let mut items = Vec::new();
        for _ in 0..file_count {
            let index = reader.read_u64_le()?;
            let _ = reader.read_bytes(16)?;

            let buf = reader.read_bytes(600)?;
            let filename = StringReader::read_nullterminated_utf16(&mut std::io::Cursor::new(buf))?;

            let _ = reader.read_u64_le()?;

            let file_start_offset = offset;
            let file_end_offset = reader.read_u64_le()?;
            let file_size = file_end_offset
                .checked_sub(file_start_offset)
                .filter(|_| file_end_offset <= total_size)
                .ok_or(Error::Static("Invalid NI FileContainer member range"))?;
            offset = file_end_offset;

            items.push(FileContainerItem {
                index,
                filename,
                file_start_offset,
                file_size,
            });
        }

        let end_marker = reader.read_u64_le()?;
        if end_marker != FC_TOC_MARKER_END {
            return Err(Error::Static("Invalid NI FileContainer table end"));
        }

        let _pad = reader.read_bytes(16)?;

        // NI FC TOC
        // Native Instruments FileContainer Table Of Contents
        // Table 2
        let mtd_magic = reader.read_bytes(16)?;
        if mtd_magic != b"/\\ NI FC TOC  /\\" {
            return Err(Error::Static("Invalid NI FileContainer table marker"));
        }

        let _header_chunk = reader.read_bytes(592)?;

        let file_section_offset = reader.stream_position()?;
        if offset != total_size || total_size > length.saturating_sub(file_section_offset) {
            return Err(Error::Static("Truncated NI FileContainer file section"));
        }

        Ok(Self {
            file_section_offset,
            items,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    #[test]
    fn authored_monolith_members_are_indexed_bounded_and_exact() {
        use std::io::Cursor;
        let mut bytes = FC_MTD_MARKER_START.to_vec();
        bytes.extend([0; 256]);
        bytes.extend(2u64.to_le_bytes());
        bytes.extend(7u64.to_le_bytes());
        bytes.extend(b"/\\ NI FC TOC  /\\");
        bytes.extend([0; 600]);
        for (index, name, end) in [(42u64, "OurPatch.nki", 3u64), (900, "OurSample.wav", 7)] {
            bytes.extend(index.to_le_bytes());
            bytes.extend([0; 16]);
            let mut filename = name
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            filename.resize(600, 0);
            bytes.extend(filename);
            bytes.extend(0u64.to_le_bytes());
            bytes.extend(end.to_le_bytes());
        }
        bytes.extend(FC_TOC_MARKER_END.to_le_bytes());
        bytes.extend([0; 16]);
        bytes.extend(b"/\\ NI FC TOC  /\\");
        bytes.extend([0; 592]);
        bytes.extend(b"nkiwave");
        let container = NIFileContainer::read(Cursor::new(&bytes)).unwrap();
        assert_eq!(container.items[0].filename, "OurPatch.nki");
        assert_eq!(
            container.read_member(Cursor::new(&bytes), 42, 3).unwrap(),
            b"nki"
        );
        assert_eq!(
            container.read_member(Cursor::new(&bytes), 900, 4).unwrap(),
            b"wave"
        );
        assert!(container.read_member(Cursor::new(&bytes), 42, 2).is_err());
        assert!(container.read_member(Cursor::new(&bytes), 0, 100).is_err());
        assert!(container
            .read_member(Cursor::new(&bytes[..bytes.len() - 1]), 900, 4)
            .is_err());
        for end in 0..bytes.len() {
            assert!(
                NIFileContainer::read(Cursor::new(&bytes[..end])).is_err(),
                "truncated at {end}"
            );
        }
        for at in [0, 288, 2208] {
            let mut damaged = bytes.clone();
            damaged[at] ^= 1;
            assert!(NIFileContainer::read(Cursor::new(damaged)).is_err());
        }
        let mut duplicate = NIFileContainer::read(Cursor::new(&bytes)).unwrap();
        duplicate.items[1].index = 42;
        assert!(duplicate.read_member(Cursor::new(&bytes), 42, 7).is_err());
        let mut overflow = NIFileContainer::read(Cursor::new(&bytes)).unwrap();
        overflow.file_section_offset = u64::MAX;
        assert!(overflow.read_member(Cursor::new(&bytes), 900, 7).is_err());
    }

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_filecontainer_nki() -> Result<(), Error> {
        let file = File::open("tests/data/Containers/FileContainer/files/000-default.nki")?;
        NIFileContainer::read(file)?;
        Ok(())
    }

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_filecontainer_nkm() -> Result<(), Error> {
        let file = File::open("tests/data/Containers/FileContainer/files/001-multi.nkm")?;
        NIFileContainer::read(file)?;
        Ok(())
    }
}
