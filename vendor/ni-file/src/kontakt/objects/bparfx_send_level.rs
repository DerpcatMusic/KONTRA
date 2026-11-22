use std::io::Cursor;

use crate::{
    kontakt::{error::KontaktError, Chunk, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

const CHUNK_ID: u16 = 0x17;
const MAX_SEND_LEVELS: usize = 8;
const MAX_ROUTING_LEVELS: usize = 17;

/// Type:           Chunk<StructuredObject>
/// SerType:        0x17
/// Versions:       0x50, 0x51
/// Kontakt 8:      BParFXSendLevels
#[derive(Debug)]
pub struct BParFXSendLevels(pub StructuredObject);

#[derive(Debug, Clone, PartialEq)]
pub struct BParFXSendLevelsParams {
    /// Send levels (up to 8 floats; [ref] into `+0x08..+0x28`).
    pub send_levels: Vec<f32>,
    /// Output routing levels present in v0x51 (up to 17 floats into `+0x28..+0x6c`).
    pub routing_levels: Vec<f32>,
}

fn read_f32_list(reader: &mut Cursor<&[u8]>, max: usize) -> Result<Vec<f32>, Error> {
    let count = reader.read_u32_le()? as usize;
    let take = count.min(max);
    let mut values = Vec::with_capacity(take);
    for _ in 0..take {
        let v = reader.read_f32_le()?;
        if !v.is_finite() {
            return Err(Error::Static("Non-finite BParFXSendLevels value"));
        }
        values.push(v);
    }
    Ok(values)
}

impl BParFXSendLevels {
    pub fn params(&self) -> Result<BParFXSendLevelsParams, Error> {
        if !matches!(self.0.version, 0x50 | 0x51) {
            return Err(Error::Generic(format!(
                "Unsupported BParFXSendLevels version 0x{:X}",
                self.0.version
            )));
        }
        let mut reader = Cursor::new(self.0.public_data.as_slice());
        let send_levels = read_f32_list(&mut reader, MAX_SEND_LEVELS)?;
        let routing_levels = if self.0.version == 0x51 {
            read_f32_list(&mut reader, MAX_ROUTING_LEVELS)?
        } else {
            Vec::new()
        };
        if reader.position() as usize != self.0.public_data.len() {
            return Err(Error::Static(
                "Unexpected trailing bytes in BParFXSendLevels",
            ));
        }
        Ok(BParFXSendLevelsParams {
            send_levels,
            routing_levels,
        })
    }
}

impl std::convert::TryFrom<&Chunk> for BParFXSendLevels {
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
    fn reads_v50_and_v51_send_levels() {
        let mut v50 = 2u32.to_le_bytes().to_vec();
        v50.extend(0.5f32.to_le_bytes());
        v50.extend(0.25f32.to_le_bytes());
        let s50 = BParFXSendLevels(StructuredObject {
            version: 0x50,
            public_data: v50.clone(),
            private_data: vec![],
            children: vec![],
        });
        let p50 = s50.params().unwrap();
        assert_eq!(p50.send_levels, [0.5, 0.25]);
        assert!(p50.routing_levels.is_empty());

        let mut v51 = v50;
        v51.extend(1u32.to_le_bytes());
        v51.extend(1.0f32.to_le_bytes());
        let s51 = BParFXSendLevels(StructuredObject {
            version: 0x51,
            public_data: v51,
            private_data: vec![],
            children: vec![],
        });
        let p51 = s51.params().unwrap();
        assert_eq!(p51.send_levels, [0.5, 0.25]);
        assert_eq!(p51.routing_levels, [1.0]);
    }
}
