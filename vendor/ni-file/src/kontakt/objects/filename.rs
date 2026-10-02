use crate::{read_bytes::ReadBytesExt, Error};

pub struct BFileName;
pub struct BFileNameSegment;

impl BFileName {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Vec<String>, Error> {
        let segments = reader.read_i32_le()?;
        let mut filename = Vec::new();
        for _ in 0..segments {
            filename.push(BFileNameSegment::read(&mut reader)?);
        }
        Ok(filename)
    }

    // K4PatchLib::BFileName::Retrieve
    pub fn read_filename<R: ReadBytesExt>(mut reader: R) -> Result<BFileName, Error> {
        let i = reader.read_i32_le()?;
        if i < 0 {
            reader.read_widestring_utf16()?;
        } else if i > 0 {
        }

        Ok(BFileName)
    }
}

/// Internally, kontakt breaks paths into segments for multiplatform support.
impl BFileNameSegment {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<String, Error> {
        let segment_type = reader.read_u8()?;
        Ok(match segment_type {
            1 => {
                // root file node
                format!("{}:", reader.read_widestring_utf16()?)
            }
            3 => {
                // parent dir
                String::from("..")
            }
            2 | 4 | 5 => reader.read_widestring_utf16()?,
            6 => {
                // set special location
                String::new()
            }
            11 => {
                // Snapshot library-root location anchor, with no payload. An
                // initial empty segment preserves the anchor as a leading /
                // when the table joins its path. Resolve it against the base
                // instrument's library, not the snapshot's filesystem folder.
                String::new()
            }
            8 => {
                // Library (nkx)
                reader.read_widestring_utf16()?
            }
            9 => {
                // multi file (used like a dir)
                reader.read_widestring_utf16()?
            }
            _ => {
                return Err(Error::Generic(format!(
                    "Unsupported filename segment {segment_type}"
                )))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn snapshot_location_anchor_preserves_path_and_rejects_bad_segments() {
        let mut data = 3i32.to_le_bytes().to_vec();
        data.push(11);
        for (kind, text) in [(2, "Samples"), (4, "Authored.ncw")] {
            data.push(kind);
            data.extend((text.len() as u32).to_le_bytes());
            for unit in text.encode_utf16() {
                data.extend(unit.to_le_bytes());
            }
        }
        let mut reader = Cursor::new(&data);
        assert_eq!(
            BFileName::read(&mut reader).unwrap().join("/"),
            "/Samples/Authored.ncw"
        );
        assert_eq!(reader.position() as usize, data.len());
        for end in 0..data.len() {
            assert!(BFileName::read(Cursor::new(&data[..end])).is_err());
        }
        let mut unknown = 1i32.to_le_bytes().to_vec();
        unknown.push(255);
        assert!(BFileName::read(Cursor::new(unknown)).is_err());
    }
}
