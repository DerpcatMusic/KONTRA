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
            let file_size = file_end_offset.checked_sub(file_start_offset)
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
