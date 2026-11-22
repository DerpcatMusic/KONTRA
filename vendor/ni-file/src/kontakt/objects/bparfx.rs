use std::io::Cursor;

use crate::{
    kontakt::{error::KontaktError, structured_object::StructuredObject, Chunk},
    read_bytes::ReadBytesExt,
    Error, NIFileError,
};

const CHUNK_ID: u16 = 0x25;

/// # BParFX
///
/// An effect slot. The effect object is the first child; its chunk ID is the
/// effect's serialization type (e.g. 0x16 BParFXIRC, 0x59 BParFXGaloisReverb).
/// The slot state shared by every effect lives in the private data, the slot
/// has no public data.
///
/// - Type:           Chunk<StructuredObject>
/// - SerType:        0x25
/// - Versions:       0x50
/// - Kontakt 7:      BParFX
///
#[derive(Debug)]
pub struct BParFX(pub StructuredObject);

/// Slot state from the BParFX private data (version 0x50).
///
/// Layout verified against `Kontakt 8.exe` `0x140d09aa0`:
/// - Empty slot: 4 bytes (`u32 effect_type == 0`).
/// - Occupied slot (22 bytes, `IBIBFFI`):
///   `u32 effect_type` (`0..4`), `u8 unknown_flag` (`4`, `+0x2c`),
///   `u32 reserved` (`5..9`, `+0x0c`), `u8 bypass` (`9`, `+0x24`),
///   `f32 output_gain` (`10..14`), `f32 dry_level` (`14..18`),
///   `i32 unknown_id` (`18..22`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BParFXParams {
    /// Kontakt's internal effect enum (not the serialization ID): 0 empty,
    /// 2 compressor, 11 send levels, 19 convolution, 20 gainer, 39 reverb, ...
    pub effect_type: u32,
    pub bypass: bool,
    /// Linear gain applied to the processed ("wet") signal.
    pub output_gain: f32,
    /// Linear level of the unprocessed signal mixed back in.
    pub dry_level: f32,
    pub reserved: u32,
    pub unknown_flag: u8,
    pub unknown_id: i32,
}

impl BParFX {
    pub fn version(&self) -> u16 {
        self.0.version
    }

    pub fn effect(&self) -> Option<&Chunk> {
        self.0.children.first()
    }

    pub fn params(&self) -> Result<BParFXParams, Error> {
        let data = &self.0.private_data;
        if self.0.version != 0x50 {
            return Err(NIFileError::Generic(format!(
                "BParFX v0x{:x} unsupported; expected v0x50",
                self.0.version
            )));
        }
        if data.len() == 4 {
            let mut r = Cursor::new(data);
            let effect_type = r.read_u32_le()?;
            if effect_type == 0 {
                return Ok(BParFXParams {
                    effect_type: 0,
                    bypass: false,
                    output_gain: 1.0,
                    dry_level: 0.0,
                    reserved: 0,
                    unknown_flag: 0,
                    unknown_id: -1,
                });
            }
        }
        if data.len() != 22 {
            return Err(NIFileError::Generic(format!(
                "BParFX v0x{:x} private data is {} bytes; supported v0x50 needs 4 (empty) or 22",
                self.0.version,
                data.len()
            )));
        }
        let mut r = Cursor::new(data);
        let effect_type = r.read_u32_le()?;
        let unknown_flag = r.read_u8()?;
        let reserved = r.read_u32_le()?;
        let bypass = match r.read_u8()? {
            0 => false,
            1 => true,
            _ => return Err(Error::Static("Invalid FX bypass byte")),
        };
        let output_gain = r.read_f32_le()?;
        let dry_level = r.read_f32_le()?;
        if !output_gain.is_finite() || !dry_level.is_finite() {
            return Err(Error::Static("Non-finite FX slot gain"));
        }
        Ok(BParFXParams {
            effect_type,
            bypass,
            output_gain,
            dry_level,
            reserved,
            unknown_flag,
            unknown_id: r.read_i32_le()?,
        })
    }
}

impl std::convert::TryFrom<&Chunk> for BParFX {
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

/// The repeated-subtype Ladder records, serialized filter types 30..=41.
/// Native v0x90/91 has two type IDs followed by a leading parameter and
/// cutoff/resonance. v0x92 inserts one byte between the IDs. Its meaning is
/// unknown; it is not a trailing byte or another float. Other versions and
/// filter families have different layouts and are deliberately not decoded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BParFXFilterRecord {
    pub version: u16,
    pub filter_type: i32,
    pub unknown_flag: Option<u8>,
    pub leading_value: f32,
    pub cutoff: f32,
    pub resonance: f32,
}

impl BParFXFilterRecord {
    pub fn read(version: u16, data: &[u8]) -> Result<Self, Error> {
        let expected = match version {
            0x90 | 0x91 => 20,
            0x92 => 21,
            _ => return Err(Error::Static("Unsupported Ladder filter record version")),
        };
        if data.len() != expected {
            return Err(Error::Static("Invalid Ladder filter record length"));
        }
        let mut r = Cursor::new(data);
        let filter_type = r.read_i32_le()?;
        if !(30..=41).contains(&filter_type) {
            return Err(Error::Static(
                "Unsupported Ladder filter serialization type",
            ));
        }
        let unknown_flag = if version == 0x92 {
            Some(r.read_u8()?)
        } else {
            None
        };
        if r.read_i32_le()? != filter_type {
            return Err(Error::Static(
                "Mismatched Ladder filter serialization types",
            ));
        }
        Ok(Self {
            version,
            filter_type,
            unknown_flag,
            leading_value: r.read_f32_le()?,
            cutoff: r.read_f32_le()?,
            resonance: r.read_f32_le()?,
        })
    }

    /// Write only the public record. The enclosing raw Chunk/StructuredObject
    /// retains its original framing, private bytes, and children independently.
    pub fn write(&self, mut writer: impl std::io::Write) -> Result<(), Error> {
        if !matches!(self.version, 0x90..=0x92)
            || !(30..=41).contains(&self.filter_type)
            || (self.version == 0x92) != self.unknown_flag.is_some()
        {
            return Err(Error::Static(
                "Invalid Ladder filter record version/type/flag shape",
            ));
        }
        writer.write_all(&self.filter_type.to_le_bytes())?;
        if let Some(flag) = self.unknown_flag {
            writer.write_all(&[flag])?;
        }
        writer.write_all(&self.filter_type.to_le_bytes())?;
        for value in [self.leading_value, self.cutoff, self.resonance] {
            writer.write_all(&value.to_le_bytes())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_filter_records_preserve_versioned_flags_and_exact_boundaries() {
        for version in [0x90u16, 0x91, 0x92] {
            for filter_type in 30i32..=41 {
                let mut raw = filter_type.to_le_bytes().to_vec();
                if version == 0x92 {
                    raw.push(0xa5);
                } // Opaque byte, not guessed boolean semantics.
                raw.extend(filter_type.to_le_bytes());
                let values = [0x8000_0000u32, 0x3f40_0000, 0x7fc0_0123];
                for value in values {
                    raw.extend(value.to_le_bytes());
                }
                let parsed = BParFXFilterRecord::read(version, &raw).unwrap();
                assert_eq!(
                    (parsed.filter_type, parsed.unknown_flag),
                    (filter_type, (version == 0x92).then_some(0xa5))
                );
                assert_eq!(
                    [
                        parsed.leading_value.to_bits(),
                        parsed.cutoff.to_bits(),
                        parsed.resonance.to_bits()
                    ],
                    values
                );
                let mut rewritten = Vec::new();
                parsed.write(&mut rewritten).unwrap();
                assert_eq!(rewritten, raw);
                let mut edited = parsed;
                edited.cutoff = 0.625;
                if let Some(flag) = &mut edited.unknown_flag {
                    *flag ^= 0xff;
                }
                rewritten.clear();
                edited.write(&mut rewritten).unwrap();
                let readback = BParFXFilterRecord::read(version, &rewritten).unwrap();
                assert_eq!(
                    (readback.cutoff, readback.unknown_flag),
                    (0.625, edited.unknown_flag)
                );
                assert_eq!(readback.leading_value.to_bits(), values[0]);
                assert_eq!(readback.resonance.to_bits(), values[2]);
                for end in 0..raw.len() {
                    assert!(BParFXFilterRecord::read(version, &raw[..end]).is_err());
                }
                let mut extra = raw.clone();
                extra.push(0);
                assert!(BParFXFilterRecord::read(version, &extra).is_err());
                let mut mismatch = raw.clone();
                let at = if version == 0x92 { 5 } else { 4 };
                mismatch[at..at + 4].copy_from_slice(&(filter_type + 1).to_le_bytes());
                assert!(BParFXFilterRecord::read(version, &mismatch).is_err());
                assert!(BParFXFilterRecord::read(0x93, &raw).is_err());
                let mut invalid = parsed;
                invalid.version = 0x93;
                let mut output = Vec::new();
                assert!(invalid.write(&mut output).is_err());
                assert!(output.is_empty());
                invalid = parsed;
                invalid.unknown_flag = if version == 0x92 { None } else { Some(0) };
                assert!(invalid.write(&mut output).is_err());
                assert!(output.is_empty());
                invalid = parsed;
                invalid.filter_type = 42;
                assert!(invalid.write(&mut output).is_err());
                assert!(output.is_empty());
            }
        }
    }

    #[test]
    fn slot_state_is_read_from_private_data() {
        let mut private = 19u32.to_le_bytes().to_vec();
        private.push(0x42); // byte 4: unknown_flag (+0x2c)
        private.extend(0x1122_3344u32.to_le_bytes()); // bytes 5..9: reserved (+0x0c)
        private.push(1); // byte 9: bypass (+0x24)
        private.extend(0.5f32.to_le_bytes());
        private.extend(1f32.to_le_bytes());
        private.extend((-1i32).to_le_bytes());
        let fx = BParFX(StructuredObject {
            version: 0x50,
            public_data: Vec::new(),
            private_data: private,
            children: Vec::new(),
        });
        let p = fx.params().unwrap();
        assert_eq!(p.effect_type, 19);
        assert_eq!(p.unknown_flag, 0x42);
        assert_eq!(p.reserved, 0x1122_3344);
        assert!(p.bypass);
        assert_eq!((p.output_gain, p.dry_level, p.unknown_id), (0.5, 1.0, -1));

        // Native 0x140d09aa0 stops after 4 bytes when effect_type == 0.
        let empty = BParFX(StructuredObject {
            version: 0x50,
            public_data: Vec::new(),
            private_data: 0u32.to_le_bytes().to_vec(),
            children: Vec::new(),
        });
        let ep = empty.params().unwrap();
        assert_eq!(ep.effect_type, 0);
        assert!(!ep.bypass);

        // Non-zero effect_type cannot end at 4 bytes.
        let short_occupied = BParFX(StructuredObject {
            version: 0x50,
            public_data: Vec::new(),
            private_data: 19u32.to_le_bytes().to_vec(),
            children: Vec::new(),
        });
        assert!(short_occupied.params().is_err());
    }
}
