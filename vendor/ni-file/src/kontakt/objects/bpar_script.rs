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
    /// Inline and linked scripts use UTF-8, with the legacy Windows-1252 fallback.
    pub fn decode_source(bytes: Vec<u8>) -> String {
        match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => encoding_rs::WINDOWS_1252
                .decode_without_bom_handling(error.as_bytes())
                .0
                .into_owned(),
        }
    }

    pub fn params(&self) -> Result<BParScriptParams, Error> {
        // Native BParScript reader [ref] accepts only v0x50 and v0x60.
        if !matches!(self.0.version, 0x50 | 0x60) {
            return Err(Error::Generic(format!(
                "Unsupported BParScript version 0x{:X}",
                self.0.version
            )));
        }
        let mut reader = Cursor::new(&self.0.public_data);

        let mut params = BParScriptParams {
            text: {
                let length = reader.read_u32_le()?;
                // Native optional string [ref]: any negative signed length.
                if (length as i32) < 0 {
                    None
                } else {
                    let bytes = reader.read_bytes(length as usize)?;
                    Some(Self::decode_source(bytes))
                }
            },
            source_editor_open: reader.read_bool()?,
            touched_but_not_applied: reader.read_bool()?,
            bypass: reader.read_bool()?,
            password_hash: {
                let length = reader.read_u32_le()?;
                // In v0x50, password_hash is an optional string
                // ([ref]: negative i32 means absent); in v0x60 it is a
                // u32-counted byte vector.
                if self.0.version == 0x50 && (length as i32) < 0 {
                    Vec::new()
                } else {
                    reader.read_bytes(length as usize)?
                }
            },
            description: reader.read_optional_sized_utf8()?,
            textfile_name: reader.read_optional_sized_utf8()?,
            persistent: Vec::new(),
        };
        // BParScript reader [ref] reads the u32 count unconditionally for
        // v0x50 and v0x60; a truncated count or entry is a parse fault.
        let mut entries = || -> Result<Vec<String>, Error> {
            let count = reader.read_u32_le()? as usize;
            let remaining = self
                .0
                .public_data
                .len()
                .saturating_sub(reader.position() as usize);
            if count > remaining / 4 {
                return Err(Error::Static("Invalid script persistence count"));
            }
            let mut out = Vec::with_capacity(count.min(65536));
            for _ in 0..count {
                let length = reader.read_u32_le()? as usize;
                let bytes = reader.read_bytes(length)?;
                out.push(Self::decode_source(bytes));
            }
            Ok(out)
        };
        params.persistent = entries()?;
        if reader.position() as usize != self.0.public_data.len() {
            return Err(Error::Static("Unexpected trailing bytes in BParScript"));
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_persistent_count_is_required_and_every_negative_length_is_absent() {
        let mut public_data = Vec::new();
        public_data.extend(u32::MAX.to_le_bytes()); // No inline source.
        public_data.extend([0, 0, 0]);
        public_data.extend(0u32.to_le_bytes()); // No password.
        public_data.extend(u32::MAX.to_le_bytes()); // No description.
        public_data.extend(u32::MAX.to_le_bytes()); // No linked name.
        let mut script = BParScript(StructuredObject {
            version: 0x60,
            public_data,
            private_data: vec![],
            children: vec![],
        });
        assert!(script.params().is_err(), "EOF is not a legacy omission");
        script.0.public_data.extend(0u32.to_le_bytes());
        assert!(script.params().unwrap().persistent.is_empty());
        script.0.public_data[0..4].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        assert!(script.params().unwrap().text.is_none());
        script
            .0
            .public_data
            .truncate(script.0.public_data.len() - 4);
        script.0.public_data.extend(1u32.to_le_bytes());
        assert!(script.params().is_err()); // Count1 without its entry.
        assert_eq!(BParScript::decode_source(vec![b'{', 0xe9, b'}']), "{é}");

        // v0x50 password_hash uses optional string semantics (-1 = absent).
        let mut v50 = Vec::new();
        v50.extend(u32::MAX.to_le_bytes()); // text absent
        v50.extend([0, 0, 0]);
        v50.extend(u32::MAX.to_le_bytes()); // password_hash absent (-1)
        v50.extend(u32::MAX.to_le_bytes()); // description absent
        v50.extend(u32::MAX.to_le_bytes()); // textfile_name absent
        v50.extend(0u32.to_le_bytes()); // persistent count = 0
        let v50_script = BParScript(StructuredObject {
            version: 0x50,
            public_data: v50.clone(),
            private_data: vec![],
            children: vec![],
        });
        assert!(v50_script.params().unwrap().password_hash.is_empty());

        // Unsupported version is rejected.
        let bad_ver = BParScript(StructuredObject {
            version: 0x70,
            public_data: v50.clone(),
            private_data: vec![],
            children: vec![],
        });
        assert!(bad_ver.params().is_err());

        // Trailing bytes are rejected.
        let mut trailing = v50;
        trailing.push(0);
        assert!(BParScript(StructuredObject {
            version: 0x50,
            public_data: trailing,
            private_data: vec![],
            children: vec![],
        })
        .params()
        .is_err());
    }
}
