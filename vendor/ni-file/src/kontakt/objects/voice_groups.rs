use std::io::Cursor;

use crate::{
    Error,
    kontakt::{Chunk, KontaktError, objects::voice_limit::VoiceLimit},
    read_bytes::ReadBytesExt,
};

use super::VoiceGroup;

/// An instrument voice limit and 128 indexed optional voice-group overrides.
/// For v0x60 the body is one inline limit, a 16-byte LSB-first mask, then
/// one inline limit per set bit in ascending order, without tags or lengths.
/// Raw bodies remain available in the parent `Chunk` on decoding errors.
///
/// - SerType:        0x32
/// - Known Versions: 0x60
/// - Kontakt 7:      BProgram::readVoiceGroups()
/// - KontaktIO:      VoiceGroups
///
#[derive(Debug)]
pub struct VoiceGroups {
    pub voice_limit: VoiceLimit,
    /// Exactly 128 slots; clear mask bits remain None, without inferred defaults.
    pub groups: Vec<Option<VoiceGroup>>,
}

impl VoiceGroups {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let voice_limit = VoiceLimit::read_inline(&mut reader)
            .map_err(|e| Error::context("VoiceGroups instrument limit".into(), e))?;
        let mask = reader.read_bytes(16)?;
        let mut groups = Vec::with_capacity(128);
        for index in 0..128 {
            groups.push(if mask[index / 8] & (1 << (index % 8)) != 0 {
                Some(
                    VoiceGroup::read(&mut reader)
                        .map_err(|e| Error::context(format!("VoiceGroups override {index}"), e))?,
                )
            } else {
                None
            });
        }
        Ok(Self {
            voice_limit,
            groups,
        })
    }
}

impl std::convert::TryFrom<&Chunk> for VoiceGroups {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != 0x32 {
            return Err(KontaktError::IncorrectID {
                expected: 0x32,
                got: chunk.id,
            }
            .into());
        }
        let mut reader = Cursor::new(&chunk.data);
        let groups = Self::read(&mut reader)?;
        if reader.position() != chunk.data.len() as u64 {
            return Err(Error::Static("Trailing VoiceGroups chunk data"));
        }
        Ok(groups)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic wire records only; this is not a semantic serialization API.
    fn inline_limit(name: &str, seed: i32) -> Vec<u8> {
        let mut data = vec![0, 0x60, 0];
        data.extend((name.encode_utf16().count() as u32).to_le_bytes());
        data.extend(name.encode_utf16().flat_map(u16::to_le_bytes));
        data.extend((-(seed as i16)).to_le_bytes());
        data.push(u8::from(seed % 2 == 0));
        data.extend((-seed).to_le_bytes());
        data.extend((seed * 3).to_le_bytes());
        data.extend((-seed - 1).to_le_bytes());
        data
    }

    fn check_limit(limit: &VoiceLimit, name: &str, seed: i32) {
        assert_eq!(limit.name, name);
        assert_eq!(limit.kill_mode, -(seed as i16));
        assert_eq!(limit.prefer_released, seed % 2 == 0);
        assert_eq!(limit.max_num_voices, -seed);
        assert_eq!(limit.ms_fade_time, seed * 3);
        assert_eq!(limit.exclusion_group, -seed - 1);
    }

    #[test]
    fn reader_regression_voice_groups_v60_indexed_limits() {
        let indices = [0, 7, 8, 63, 64, 127];
        let mut data = inline_limit("Instrument 🎹", 5);
        let mut mask = [0u8; 16];
        for index in indices {
            mask[index / 8] |= 1 << (index % 8);
        }
        assert_eq!(mask[0], 0x81);
        assert_eq!(mask[1], 0x01);
        assert_eq!(mask[7], 0x80);
        assert_eq!(mask[8], 0x01);
        assert_eq!(mask[15], 0x80);
        data.extend(mask);
        for index in indices {
            data.extend(inline_limit(&format!("Group {index} Ω"), index as i32));
        }
        let chunk = Chunk { id: 0x32, data };
        let decoded = VoiceGroups::try_from(&chunk).unwrap();
        check_limit(&decoded.voice_limit, "Instrument 🎹", 5);
        assert_eq!(decoded.groups.len(), 128);
        for (index, group) in decoded.groups.iter().enumerate() {
            if indices.contains(&index) {
                check_limit(
                    &group.as_ref().unwrap().voice_limit,
                    &format!("Group {index} Ω"),
                    index as i32,
                );
            } else {
                assert!(group.is_none(), "absent override {index} must stay None");
            }
        }
        assert!(chunk.into_object().is_ok());
        // Every truncation must fail, including incomplete mask and the last override.
        for end in 0..chunk.data.len() {
            assert!(
                VoiceGroups::try_from(&Chunk {
                    id: 0x32,
                    data: chunk.data[..end].to_vec()
                })
                .is_err(),
                "accepted truncation at {end}"
            );
        }
        let mut encoded = Vec::new();
        chunk.write(&mut encoded).unwrap();
        let mut preserved = Vec::new();
        Chunk::read(Cursor::new(&encoded))
            .unwrap()
            .write(&mut preserved)
            .unwrap();
        assert_eq!(preserved, encoded);

        let mut empty = inline_limit("", 0);
        empty.extend([0; 16]);
        assert_eq!(empty.len(), 38);
        assert!(
            VoiceGroups::try_from(&Chunk {
                id: 0x32,
                data: empty
            })
            .unwrap()
            .groups
            .iter()
            .all(Option::is_none)
        );
        let mut full = inline_limit("", 0);
        full.extend([0xff; 16]);
        for index in 0..128 {
            full.extend(inline_limit("", index));
        }
        let full = VoiceGroups::try_from(&Chunk {
            id: 0x32,
            data: full,
        })
        .unwrap();
        assert_eq!(full.groups.len(), 128);
        assert!(full.groups.iter().all(Option::is_some));
        check_limit(&full.groups[127].as_ref().unwrap().voice_limit, "", 127);
    }

    #[test]
    fn reader_regression_voice_groups_reject_bad_inline_records() {
        let decode = |data| VoiceGroups::try_from(&Chunk { id: 0x32, data });
        let limit = inline_limit("Ω", 1);
        let mut data = limit.clone();
        data.extend([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let group_offset = data.len();
        data.extend(&limit);
        for offset in [0, group_offset] {
            for flag in [1, 2, 255] {
                let mut malformed = data.clone();
                malformed[offset] = flag;
                assert!(decode(malformed).is_err());
            }
            for version in [0x5fu16, 0x61, 0xffff] {
                let mut malformed = data.clone();
                malformed[offset + 1..offset + 3].copy_from_slice(&version.to_le_bytes());
                let chunk = Chunk {
                    id: 0x32,
                    data: malformed,
                };
                assert!(chunk.into_object().is_err());
                assert_eq!(&chunk.data[offset + 1..offset + 3], &version.to_le_bytes());
            }
            let mut oversized = data.clone();
            oversized[offset + 3..offset + 7].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(decode(oversized).is_err());
            let mut invalid_utf16 = data.clone();
            invalid_utf16[offset + 7..offset + 9].copy_from_slice(&0xd800u16.to_le_bytes());
            assert!(decode(invalid_utf16).is_err());
            // Native reads/stores a byte and tests nonzero, rather than requiring 1.
            for preference in [0, 1, 2, 255] {
                let mut changed = data.clone();
                changed[offset + 11] = preference; // one UTF-16 unit, then i16 kill mode
                let chunk = Chunk {
                    id: 0x32,
                    data: changed.clone(),
                };
                let decoded = VoiceGroups::try_from(&chunk).unwrap();
                let value = if offset == 0 {
                    &decoded.voice_limit
                } else {
                    &decoded.groups[0].as_ref().unwrap().voice_limit
                };
                assert_eq!(value.prefer_released, preference != 0);
                assert_eq!(
                    chunk.data, changed,
                    "bool normalization must not alter raw bytes"
                );
                let mut encoded = Vec::new();
                chunk.write(&mut encoded).unwrap();
                assert_eq!(Chunk::read(Cursor::new(encoded)).unwrap().data, changed);
            }
        }
        let mut trailing = data.clone();
        trailing.push(0);
        assert!(decode(trailing).is_err());
        // A generic stream reader consumes one record, leaving outer framing to its caller.
        let mut stream = Cursor::new([data.as_slice(), &[0xaa]].concat());
        VoiceGroups::read(&mut stream).unwrap();
        assert_eq!(stream.position(), data.len() as u64);
        assert!(
            VoiceGroups::try_from(&Chunk {
                id: 0x33,
                data: vec![]
            })
            .is_err()
        );

        let standalone = Chunk {
            id: 0x2b,
            data: limit,
        };
        check_limit(
            &VoiceGroup::try_from(&standalone).unwrap().voice_limit,
            "Ω",
            1,
        );
        assert!(standalone.into_object().is_ok());
        for end in 0..standalone.data.len() {
            assert!(
                VoiceGroup::try_from(&Chunk {
                    id: 0x2b,
                    data: standalone.data[..end].to_vec()
                })
                .is_err()
            );
        }
        let mut bad = Chunk {
            id: 0x2b,
            data: standalone.data.clone(),
        };
        bad.data.push(0);
        assert!(VoiceGroup::try_from(&bad).is_err());
        bad.id = 0x32;
        assert!(VoiceGroup::try_from(&bad).is_err());
        assert_eq!(
            standalone.data,
            inline_limit("Ω", 1),
            "raw chunk remains intact"
        );
    }
}
