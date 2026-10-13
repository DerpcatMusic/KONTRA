use std::fmt::Debug;
use std::io::{Cursor, SeekFrom};

use crate::{Error, NIFileError, read_bytes::ReadBytesExt};

use super::chunk::Chunk;

#[doc = include_str!("../../doc/presets/Kontakt/StructuredObject.md")]
pub struct StructuredObject {
    pub version: u16,
    pub public_data: Vec<u8>,
    pub private_data: Vec<u8>,
    pub children: Vec<Chunk>,
}

impl StructuredObject {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let is_data_structured = reader.read_bool()?;
        let version = reader.read_u16_le()?;

        if !is_data_structured {
            return Ok(Self {
                public_data: reader.read_all()?,
                version,
                private_data: Vec::new(),
                children: Vec::new(),
            });
        }

        let private_data_length = reader.read_u32_le()?;
        let private_data = reader
            .read_bytes(private_data_length as usize)
            .map_err(|e| Error::context(format!("StructuredObject private data, declared length {private_data_length}/version 0x{version:04x}"), e))?;

        let public_data_length = reader.read_u32_le()?;
        let public_data = reader
            .read_bytes(public_data_length as usize)
            .map_err(|e| Error::context(format!("StructuredObject public data, declared length {public_data_length}/version 0x{version:04x}"), e))?;

        let children_data_length = reader.read_u32_le()?;
        // Three independently counted areas; child chunks cannot extend into
        // the next enclosing record. See DSP_FORMAT_SPECIFICATION.md, Kontakt
        // chunks and structured objects. v1 0cb7a8a0 used a second whole-child
        // buffer here; read final owned payloads directly instead.
        let children_start = reader.stream_position()?;
        let outer_end = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(children_start))?;
        if u64::from(children_data_length) > outer_end.saturating_sub(children_start) {
            return Err(Error::Generic(format!(
                "StructuredObject children data, declared length {children_data_length}/version 0x{version:04x}: truncated section"
            )));
        }
        let mut remaining = children_data_length;
        let mut children = Vec::new();
        while remaining != 0 {
            if remaining < 6 {
                return Err(Error::Static("Truncated StructuredObject child header"));
            }
            let id = reader.read_u16_le()?;
            let length = reader.read_u32_le()?;
            remaining -= 6;
            if length > remaining {
                return Err(Error::Generic(format!(
                    "StructuredObject child 0x{id:04x}, declared {length} bytes, available {remaining} within children section"
                )));
            }
            children.push(Chunk {
                id,
                data: reader.read_bytes(length as usize)?,
            });
            remaining -= length;
        }

        Ok(Self {
            private_data,
            version,
            public_data,
            children,
        })
    }

    pub fn find_first(&self, id: u16) -> Option<&Chunk> {
        self.children.iter().find(|c| c.id == id)
    }
}

impl Debug for StructuredObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StructuredObject")
            .field("version", &format_args!("0x{:X}", self.version))
            .field("public_data_bytes", &self.public_data.len())
            .field("private_data_bytes", &self.private_data.len())
            .field("child_count", &self.children.len())
            .finish()
    }
}

impl std::convert::TryFrom<&Chunk> for StructuredObject {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        let cursor = Cursor::new(&chunk.data);
        StructuredObject::read(cursor).map_err(|e| {
            NIFileError::context(
                format!(
                    "Kontakt structured chunk 0x{:04x}, body length {}",
                    chunk.id,
                    chunk.data.len()
                ),
                e,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_structured_object_0x28() -> Result<(), Error> {
        let mut file = File::open("tests/patchdata/KontaktV42/StructuredObject/0x28")?;
        let obj = StructuredObject::read(&mut file)?;

        assert_eq!(obj.version, 0x80);
        assert_eq!(obj.public_data.len(), 140);
        assert_eq!(obj.private_data.len(), 211);
        assert_eq!(obj.children.len(), 5);

        Ok(())
    }
}
