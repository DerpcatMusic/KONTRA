use crate::{read_bytes::ReadBytesExt, Error};

pub struct BFileName;
pub struct BFileNameSegment;

/// Native filename segments, retaining their kind and UTF-16 code units.
/// Unlike a joined path, this can be written without losing location anchors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BFileNameRecord {
    pub segments: Vec<BFileNameSegmentRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BFileNameSegmentRecord {
    pub kind: u8,
    /// Present only for kinds 1, 2, 4, 5, 8 and 9. Code units are not normalized.
    pub text: Option<Vec<u16>>,
}

impl BFileNameRecord {
    pub(crate) fn read(reader: &mut std::io::Cursor<&[u8]>) -> Result<Self, Error> {
        let count = reader.read_i32_le()?;
        if count < 0
            || count as usize
                > reader
                    .get_ref()
                    .len()
                    .saturating_sub(reader.position() as usize)
        {
            return Err(Error::Static("Invalid filename segment count"));
        }
        let mut segments = Vec::new();
        segments
            .try_reserve_exact(count as usize)
            .map_err(|e| Error::Generic(e.to_string()))?;
        for _ in 0..count {
            let kind = reader.read_u8()?;
            let text = match kind {
                1 | 2 | 4 | 5 | 8 | 9 => {
                    let length = reader.read_u32_le()? as usize;
                    if length
                        > reader
                            .get_ref()
                            .len()
                            .saturating_sub(reader.position() as usize)
                            / 2
                    {
                        return Err(Error::Static("Invalid filename UTF-16 length"));
                    }
                    let mut units = Vec::new();
                    units
                        .try_reserve_exact(length)
                        .map_err(|e| Error::Generic(e.to_string()))?;
                    for _ in 0..length {
                        units.push(reader.read_u16_le()?);
                    }
                    Some(units)
                }
                3 | 6 | 11 => None,
                _ => {
                    return Err(Error::Generic(format!(
                        "Unsupported filename segment {kind}"
                    )))
                }
            };
            segments.push(BFileNameSegmentRecord { kind, text });
        }
        Ok(Self { segments })
    }

    pub(crate) fn encoded_len(&self) -> Result<usize, Error> {
        i32::try_from(self.segments.len())
            .map_err(|_| Error::Static("Too many filename segments"))?;
        let mut length = 4usize;
        for segment in &self.segments {
            let extra = match (segment.kind, &segment.text) {
                (1 | 2 | 4 | 5 | 8 | 9, Some(units)) => {
                    u32::try_from(units.len())
                        .map_err(|_| Error::Static("Filename text too long"))?;
                    units.len().checked_mul(2).and_then(|n| n.checked_add(5))
                }
                (3 | 6 | 11, None) => Some(1),
                _ => return Err(Error::Static("Invalid filename segment kind or payload")),
            };
            length = extra
                .and_then(|n| length.checked_add(n))
                .ok_or(Error::Static("Filename too large"))?;
        }
        Ok(length)
    }

    pub(crate) fn append_to(&self, data: &mut Vec<u8>) {
        data.extend((self.segments.len() as i32).to_le_bytes());
        for segment in &self.segments {
            data.push(segment.kind);
            if let Some(units) = &segment.text {
                data.extend((units.len() as u32).to_le_bytes());
                for unit in units {
                    data.extend(unit.to_le_bytes());
                }
            }
        }
    }
}

impl BFileName {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Vec<String>, Error> {
        let segments = reader.read_i32_le()?;
        if segments < 0 {
            return Err(Error::Static("Invalid filename segment count"));
        }
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
