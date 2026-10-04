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
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        // NI FC MTD
        // Native Instruments FileContainer MetaData
        let mtd_magic = reader.read_bytes(16)?;
        debug_assert_eq!(
            mtd_magic, FC_MTD_MARKER_START,
            "Monolith header tag not found."
        );

        let _header_chunk = reader.read_bytes(256)?;
        let file_count = reader.read_u64_le()?;
        let total_size = reader.read_u64_le()?;
        dbg!(total_size);

        // NI FC TOC
        // Native Instruments FileContainer Table Of Contents
        // Table 1
        let mtd_magic = reader.read_bytes(16)?;
        debug_assert_eq!(mtd_magic, b"/\\ NI FC TOC  /\\");

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
                .ok_or(Error::Static("FileContainer end offset precedes previous file"))?;
            offset = file_end_offset;

            items.push(FileContainerItem {
                index,
                filename,
                file_start_offset,
                file_size,
            });
        }

        let end_marker = reader.read_u64_le()?;
        assert_eq!(end_marker, FC_TOC_MARKER_END);

        let _pad = reader.read_bytes(16)?;

        // NI FC TOC
        // Native Instruments FileContainer Table Of Contents
        // Table 2
        let mtd_magic = reader.read_bytes(16)?;
        debug_assert_eq!(mtd_magic, b"/\\ NI FC TOC  /\\");

        let _header_chunk = reader.read_bytes(592)?;

        let file_section_offset = reader.stream_position()?;

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

    // Authored FileContainer metadata; no native library content or access data.
    fn authored_container(ends: &[u64]) -> (Vec<u8>, u64, u64) {
        let mut bytes = FC_MTD_MARKER_START.to_vec();
        bytes.extend([0; 256]);
        bytes.extend((ends.len() as u64).to_le_bytes());
        bytes.extend(ends.last().copied().unwrap_or(0).to_le_bytes());
        bytes.extend(b"/\\ NI FC TOC  /\\");
        bytes.extend([0; 600]);
        for (index, end) in ends.iter().enumerate() {
            bytes.extend((index as u64).to_le_bytes());
            bytes.extend([0; 16]);
            let mut filename = [0; 600];
            for (dest, unit) in filename.chunks_exact_mut(2).zip("authored.ncw".encode_utf16()) {
                dest.copy_from_slice(&unit.to_le_bytes());
            }
            bytes.extend(filename);
            bytes.extend(0u64.to_le_bytes());
            bytes.extend(end.to_le_bytes());
        }
        let records_end = bytes.len() as u64;
        bytes.extend(FC_TOC_MARKER_END.to_le_bytes());
        bytes.extend([0; 16]);
        bytes.extend(b"/\\ NI FC TOC  /\\");
        bytes.extend([0; 592]);
        let metadata_end = bytes.len() as u64;
        (bytes, records_end, metadata_end)
    }

    #[test]
    fn descending_file_offsets_return_error_after_the_declared_record() {
        for ends in [[10, 5], [u64::MAX, 0]] {
            let (bytes, records_end, _) = authored_container(&ends);
            let mut reader = std::io::Cursor::new(bytes);
            assert!(matches!(NIFileContainer::read(&mut reader),
                Err(Error::Static("FileContainer end offset precedes previous file"))));
            assert_eq!(reader.position(), records_end);
        }
    }

    #[test]
    fn monotonic_and_zero_length_file_ranges_preserve_reader_ownership() {
        let ends = [0, 0, 8, 8, 12];
        let (mut bytes, _, metadata_end) = authored_container(&ends);
        bytes.extend([0; 12]);
        let mut reader = std::io::Cursor::new(bytes);
        let container = NIFileContainer::read(&mut reader).unwrap();
        assert_eq!(container.file_section_offset, metadata_end);
        assert_eq!(reader.position(), metadata_end, "parser consumed file payload bytes");
        assert_eq!(container.items.len(), ends.len());
        assert_eq!(container.items.iter().map(|item| item.file_start_offset).collect::<Vec<_>>(),
            [0, 0, 0, 8, 8]);
        assert_eq!(container.items.iter().map(|item| item.file_size).collect::<Vec<_>>(),
            [0, 0, 8, 0, 4]);
        assert!(container.items.iter().all(|item| item.filename == "authored.ncw"));
    }

    #[test]
    fn test_filecontainer_nki() -> Result<(), Error> {
        let file = File::open("tests/data/Containers/FileContainer/files/000-default.nki")?;
        NIFileContainer::read(file)?;
        Ok(())
    }

    #[test]
    fn test_filecontainer_nkm() -> Result<(), Error> {
        let file = File::open("tests/data/Containers/FileContainer/files/001-multi.nkm")?;
        NIFileContainer::read(file)?;
        Ok(())
    }
}
