use std::io::Cursor;

use crate::{
    kontakt::{error::KontaktError, structured_object::StructuredObject, Chunk},
    read_bytes::ReadBytesExt,
    Error,
};

const CHUNK_ID: u16 = 0x06;

/// BParScript
///
/// A single kontakt script object.
///
/// Type:           Chunk<StructuredObject>
/// SerType:        0x06
/// Versions:       0x50, 0x60
/// Kontakt 7:      BParScript
/// KontaktIO:
#[derive(Debug)]
pub struct BParScript(pub StructuredObject);

#[derive(Debug)]
pub struct BParScriptParams {
    pub text: Option<String>,
    pub source_editor_open: bool,
    pub touched_but_not_applied: bool,
    pub bypass: bool,
    pub password_hash: Vec<u8>,
    pub description: Option<String>,
    pub textfile_name: Option<String>,
    /// Saved values of persistent variables, one `"<name> <value...>"` entry each.
    pub persistent: Vec<String>,
}

impl BParScript {
    pub fn params(&self) -> Result<BParScriptParams, Error> {
        let mut reader = Cursor::new(&self.0.public_data);

        let mut params = BParScriptParams {
            text: {
                let length = reader.read_u32_le()?;
                if length == u32::MAX {
                    None
                } else {
                    let bytes = reader.read_bytes(length as usize)?;
                    Some(match String::from_utf8(bytes) {
                        Ok(text) => text,
                        Err(error) => encoding_rs::WINDOWS_1252
                            .decode_without_bom_handling(error.as_bytes())
                            .0
                            .into_owned(),
                    })
                }
            },
            source_editor_open: reader.read_bool()?,
            touched_but_not_applied: reader.read_bool()?,
            bypass: reader.read_bool()?,
            password_hash: {
                let length = reader.read_u32_le()?;
                reader.read_bytes(length as usize)?
            },
            description: reader.read_optional_sized_utf8()?,
            textfile_name: reader.read_optional_sized_utf8()?,
            persistent: Vec::new(),
        };
        // Older scripts end here. A present but malformed table is a parse fault.
        let has_table = (reader.position() as usize) < reader.get_ref().len();
        let mut entries = || -> Result<Vec<String>, Error> {
            let count = reader.read_u32_le()? as usize;
            let mut out = Vec::with_capacity(count.min(65536));
            for _ in 0..count {
                let length = reader.read_u32_le()? as usize;
                let bytes = reader.read_bytes(length)?;
                out.push(String::from_utf8_lossy(&bytes).into_owned());
            }
            Ok(out)
        };
        if has_table {
            params.persistent = entries()?;
        }
        Ok(params)
    }
}

impl std::convert::TryFrom<&Chunk> for BParScript {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != CHUNK_ID {
            return Err(KontaktError::IncorrectID {
                expected: CHUNK_ID,
                got: chunk.id,
            }
            .into());
        }
        Ok(Self(chunk.try_into()?))
    }
}
