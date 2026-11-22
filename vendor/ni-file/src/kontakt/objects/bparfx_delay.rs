use std::io::Cursor;

use crate::{
    kontakt::{Chunk, KontaktError, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

const CHUNK_ID: u16 = 0x10;

/// # FXDelay
///
/// Delay effect.
///
/// - Type:           Chunk<StructuredObject>
/// - SerType:        0x10
/// - Versions:       0x50, 0x51
/// - Kontakt 7:      BParFXDelay
/// - KontaktIO:      ?
///
#[derive(Debug)]
pub struct FXDelay(pub StructuredObject);

/// Decoded `BParFXDelay` (`0x10`) parameters (`Kontakt 8.exe` `0x140d00b60`):
/// v0x50 stores 5 floats (`time_ms`, `damping`, `pan`, `feedback`, `sync_beats`);
/// v0x51 appends 2 sync floats (`sync_f1`, `sync_f2`) and a `sync_flag` bool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FXDelayParams {
    pub time_ms: f32,
    pub damping: f32,
    pub pan: f32,
    pub feedback: f32,
    pub sync_beats: f32,
    pub sync_f1: Option<f32>,
    pub sync_f2: Option<f32>,
    pub sync_flag: bool,
}

impl FXDelay {
    pub fn params(&self) -> Result<FXDelayParams, Error> {
        if !matches!(self.0.version, 0x50 | 0x51) {
            return Err(Error::Generic(format!(
                "Unsupported BParFXDelay version 0x{:X}",
                self.0.version
            )));
        }
        let mut reader = Cursor::new(self.0.public_data.as_slice());
        let time_ms = reader.read_f32_le()?;
        let damping = reader.read_f32_le()?;
        let pan = reader.read_f32_le()?;
        let feedback = reader.read_f32_le()?;
        let sync_beats = reader.read_f32_le()?;
        let (sync_f1, sync_f2, sync_flag) = if self.0.version == 0x51 {
            (
                Some(reader.read_f32_le()?),
                Some(reader.read_f32_le()?),
                reader.read_bool()?,
            )
        } else {
            // Native 0x140d00b60 sets +0x19 = 1 in v0x50.
            (None, None, true)
        };
        if reader.position() as usize != self.0.public_data.len() {
            return Err(Error::Static("Unexpected trailing bytes in BParFXDelay"));
        }
        Ok(FXDelayParams {
            time_ms,
            damping,
            pan,
            feedback,
            sync_beats,
            sync_f1,
            sync_f2,
            sync_flag,
        })
    }
}

impl std::convert::TryFrom<&Chunk> for FXDelay {
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
    fn reads_v50_and_v51_delay_records() {
        let mut v50 = Vec::new();
        for v in [250.0f32, 0.4, 0.5, 0.3, -1.0] {
            v50.extend(v.to_le_bytes());
        }
        let d50 = FXDelay(StructuredObject {
            version: 0x50,
            public_data: v50.clone(),
            private_data: vec![],
            children: vec![],
        });
        let p50 = d50.params().unwrap();
        assert_eq!(
            (
                p50.time_ms,
                p50.damping,
                p50.pan,
                p50.feedback,
                p50.sync_beats
            ),
            (250.0, 0.4, 0.5, 0.3, -1.0)
        );
        assert!(p50.sync_flag);
        assert_eq!((p50.sync_f1, p50.sync_f2), (None, None));

        let mut v51 = v50;
        v51.extend(0.125f32.to_le_bytes());
        v51.extend(0.0f32.to_le_bytes());
        v51.push(0);
        let d51 = FXDelay(StructuredObject {
            version: 0x51,
            public_data: v51,
            private_data: vec![],
            children: vec![],
        });
        let p51 = d51.params().unwrap();
        assert!(!p51.sync_flag);
        assert_eq!((p51.sync_f1, p51.sync_f2), (Some(0.125), Some(0.0)));
    }
}
