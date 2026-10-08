use crate::{
    kontakt::{
        chunk_set::KontaktChunks,
        objects::{NKIAppVersion, PatchType},
    },
    read_bytes::ReadBytesExt,
    Error,
};

use super::{
    kon5::Kon5, kon6::Kon6, kon7::Kon7, multi::KontaktMulti, KontaktV1, KontaktV2, KontaktV42,
};

#[derive(Debug)]
pub enum KontaktPreset {
    KontaktV1(KontaktV1),
    KontaktV2(KontaktV2),
    KontaktV42(KontaktV42),
    Kon5(Kon5),
    Kon6(Kon6),
    Kon7(Kon7),
    NKM(KontaktMulti),
    /// Raw chunks with no supported application/patch schema; not a decoded instrument.
    Unsupported(KontaktChunks),
}

impl KontaktPreset {
    pub fn read<R: ReadBytesExt>(
        reader: R,
        id: &str,
        patch_type: &PatchType,
        _version: &NKIAppVersion,
    ) -> Result<KontaktPreset, Error> {
        let chunks = KontaktChunks::read(reader)?;

        Ok(match patch_type {
            PatchType::NKI => match id {
                "Kon4" => Self::KontaktV42(chunks.try_into()?),
                "Kon5" => Self::Kon5(chunks.try_into()?),
                "Kon6" => Self::Kon6(chunks.try_into()?),
                "Kon7" => Self::Kon7(chunks.try_into()?),
                _ => Self::Unsupported(chunks),
            },
            _ => Self::Unsupported(chunks),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kontakt::Chunk;
    use std::io::Cursor;

    fn version() -> NKIAppVersion {
        NKIAppVersion {
            major: 7,
            minor_1: 0,
            minor_2: 0,
            minor_3: 0,
        }
    }

    #[test]
    fn nis_readers_unknown_schema_preserves_raw_chunks_and_rejects_truncation() {
        let chunks = KontaktChunks(vec![
            Chunk {
                id: 0xf123,
                data: vec![0, 1, 0xff],
            },
            Chunk {
                id: 0x28,
                data: vec![0xff],
            }, // opaque, not a valid Program
            Chunk {
                id: 0xf123,
                data: vec![3, 4],
            },
        ]);
        let mut bytes = Vec::new();
        chunks.write(&mut bytes).unwrap();
        for (id, patch_type) in [
            ("Kon8", PatchType::NKI),
            ("other", PatchType::NKI),
            ("Kon7", PatchType::NKM),
            ("Kon4", PatchType::NKB),
            ("Kon5", PatchType::NKP),
            ("Kon6", PatchType::NKG),
            ("Kon7", PatchType::NKZ),
            ("Kon7", PatchType::Unknown(0xffff)),
        ] {
            let preset =
                KontaktPreset::read(Cursor::new(&bytes), id, &patch_type, &version()).unwrap();
            let KontaktPreset::Unsupported(raw) = preset else {
                panic!("unexpected semantic support for {id}/{patch_type:?}")
            };
            let mut roundtrip = Vec::new();
            raw.write(&mut roundtrip).unwrap();
            assert_eq!(roundtrip, bytes);
            assert!(KontaktPreset::read(
                Cursor::new(&bytes[..bytes.len() - 1]),
                id,
                &patch_type,
                &version()
            )
            .is_err());
        }
    }

    #[test]
    fn nis_readers_known_instrument_schemas_keep_decoding_and_return_errors() {
        let mut program = vec![1]; // structured Program with empty sections
        program.extend(0x80u16.to_le_bytes());
        program.extend([0; 12]); // private, public, child lengths
        for id in ["Kon4", "Kon5", "Kon6", "Kon7"] {
            let table = if id == "Kon4" {
                Chunk {
                    id: 0x3d,
                    data: vec![0; 12],
                }
            } else {
                let mut data = 2u16.to_le_bytes().to_vec();
                data.extend([0; 12]); // three empty filename tables
                Chunk { id: 0x4b, data }
            };
            let chunks = KontaktChunks(vec![
                Chunk {
                    id: 0x28,
                    data: program.clone(),
                },
                table,
            ]);
            let mut bytes = Vec::new();
            chunks.write(&mut bytes).unwrap();
            let preset =
                KontaktPreset::read(Cursor::new(bytes), id, &PatchType::NKI, &version()).unwrap();
            assert!(matches!(
                (id, preset),
                ("Kon4", KontaktPreset::KontaktV42(_))
                    | ("Kon5", KontaktPreset::Kon5(_))
                    | ("Kon6", KontaktPreset::Kon6(_))
                    | ("Kon7", KontaktPreset::Kon7(_))
            ));
            assert!(
                KontaktPreset::read(Cursor::new(vec![]), id, &PatchType::NKI, &version()).is_err()
            );
        }
    }
}
