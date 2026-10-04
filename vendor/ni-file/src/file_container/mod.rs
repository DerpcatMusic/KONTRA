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
        if mtd_magic != FC_MTD_MARKER_START {
            return Err(Error::Static("Monolith header tag not found."));
        }

        let _header_chunk = reader.read_bytes(256)?;
        let file_count = reader.read_u64_le()?;
        let _total_size = reader.read_u64_le()?;

        // NI FC TOC
        // Native Instruments FileContainer Table Of Contents
        // Table 1
        let mtd_magic = reader.read_bytes(16)?;
        if mtd_magic != b"/\\ NI FC TOC  /\\" {
            return Err(Error::Static("FileContainer first table marker not found"));
        }

        let _header_chunk = reader.read_bytes(600)?;

        // Every table entry requires 8 + 16 + 600 + 8 + 8 bytes.
        let records_start = reader.stream_position()?;
        let stream_end = reader.seek(std::io::SeekFrom::End(0))?;
        reader.seek(std::io::SeekFrom::Start(records_start))?;
        if file_count > stream_end.saturating_sub(records_start) / 640 {
            return Err(Error::Static("Invalid FileContainer file count"));
        }

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
        if end_marker != FC_TOC_MARKER_END {
            return Err(Error::Static("FileContainer table end marker not found"));
        }

        let _pad = reader.read_bytes(16)?;

        // NI FC TOC
        // Native Instruments FileContainer Table Of Contents
        // Table 2
        let mtd_magic = reader.read_bytes(16)?;
        if mtd_magic != b"/\\ NI FC TOC  /\\" {
            return Err(Error::Static("FileContainer second table marker not found"));
        }

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
    fn altered_container_markers_return_errors_at_the_marker_boundary() {
        let (bytes, records_end, _) = authored_container(&[4]);
        let markers = [
            (0, 16, "Monolith header tag not found."),
            (16 + 256 + 8 + 8, 16, "FileContainer first table marker not found"),
            (records_end as usize, 8, "FileContainer table end marker not found"),
            (records_end as usize + 8 + 16, 16, "FileContainer second table marker not found"),
        ];
        for (start, len, expected) in markers {
            for byte in start..start + len {
                let mut altered = bytes.clone();
                altered[byte] ^= 1;
                let mut reader = std::io::Cursor::new(altered);
                assert!(matches!(NIFileContainer::read(&mut reader),
                    Err(Error::Static(message)) if message == expected));
                assert_eq!(reader.position(), (start + len) as u64);
            }
        }
    }

    #[test]
    fn truncated_container_markers_preserve_read_errors() {
        let (bytes, records_end, _) = authored_container(&[4]);
        for (start, len) in [
            (0, 16),
            (16 + 256 + 8 + 8, 16),
            (records_end as usize, 8),
            (records_end as usize + 8 + 16, 16),
        ] {
            let mut reader = std::io::Cursor::new(bytes[..start + len - 1].to_vec());
            let error = NIFileContainer::read(&mut reader).err().unwrap();
            if len == 8 {
                assert!(matches!(error, Error::IO(error)
                    if error.kind() == std::io::ErrorKind::UnexpectedEof));
                assert_eq!(reader.position(), (start + len - 1) as u64);
            } else {
                assert!(matches!(error,
                    Error::ReadBytesError(crate::read_bytes::ReadBytesError::Generic(message))
                    if message == format!("Read at offset {start}: declared {len} bytes, available {} bytes", len - 1)));
                assert_eq!(reader.position(), start as u64);
            }
        }
    }

    #[test]
    fn impossible_file_counts_fail_before_reading_table_entries() {
        for (ends, declared_count) in [
            (Vec::<u64>::new(), 1),
            (vec![4], 2),
            (vec![4], u64::MAX),
        ] {
            let (mut bytes, _, _) = authored_container(&ends);
            bytes[16 + 256..16 + 256 + 8].copy_from_slice(&declared_count.to_le_bytes());
            let mut reader = std::io::Cursor::new(bytes);
            assert!(matches!(NIFileContainer::read(&mut reader),
                Err(Error::Static("Invalid FileContainer file count"))));
            assert_eq!(reader.position(), 16 + 256 + 8 + 8 + 16 + 600);
        }
    }

    #[test]
    fn feasible_count_keeps_exact_record_boundary_read_error() {
        let (mut bytes, records_end, _) = authored_container(&[4]);
        bytes.truncate(records_end as usize);
        let mut reader = std::io::Cursor::new(bytes);
        assert!(matches!(NIFileContainer::read(&mut reader), Err(Error::IO(error))
            if error.kind() == std::io::ErrorKind::UnexpectedEof));
        assert_eq!(reader.position(), records_end);
    }

    #[test]
    fn empty_file_table_preserves_metadata_and_payload_boundary() {
        let (mut bytes, _, metadata_end) = authored_container(&[]);
        bytes.extend([0; 4]);
        let mut reader = std::io::Cursor::new(bytes);
        let container = NIFileContainer::read(&mut reader).unwrap();
        assert!(container.items.is_empty());
        assert_eq!(container.file_section_offset, metadata_end);
        assert_eq!(reader.position(), metadata_end);
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
