use std::io::{Cursor, Read};

use flate2::read::ZlibDecoder;

use crate::{
    kontakt::{
        objects::{BPatchHeader, BPatchHeaderV42, BPatchMetaInfoHeader},
        schemas::{KontaktPreset, KontaktV1, KontaktV2},
        KontaktPatch,
    },
    read_bytes::ReadBytesExt,
    Error,
};

use super::error::NKSError;

#[derive(Debug)]
pub struct NKSContainer {
    pub header: BPatchHeader,
    pub compressed_data: Vec<u8>,
    pub meta_info: Option<BPatchMetaInfoHeader>,
}

impl NKSContainer {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, NKSError> {
        let magic = reader.read_u32_le()?;

        // NOTE: 0xab85ef01 is also valid
        match magic {
            0xB36EE55E | 0x7FA89012 | 0xA4D6E55A | 0x10874353 => {}
            _ => return Err(NKSError::InvalidMagicNumber(magic)),
        };

        // For BPatchHeaderV1, this field is zlib_start
        let compressed_length = reader.read_u32_le()? as usize;
        let header = BPatchHeader::read_le(&mut reader)?;
        let version = match &header {
            BPatchHeader::BPatchHeaderV1(_) => "v1",
            BPatchHeader::BPatchHeaderV2(_) => "v2",
            BPatchHeader::BPatchHeaderV42(_) => "v42",
        };
        let at = reader.stream_position()?;
        let size_field = if version == "v1" { "zlib offset" } else { "declared length" };

        let compressed_data = (|| -> Result<Vec<u8>, NKSError> { Ok(match header {
            BPatchHeader::BPatchHeaderV1(_) => reader.read_all()?,
            BPatchHeader::BPatchHeaderV2(ref h) => match h.is_monolith {
                true => {
                    return Err(NKSError::Decompression(
                        "Legacy NKS monoliths are unsupported".into(),
                    ))
                }
                false => {
                    if compressed_length == 0 {
                        let mut buf = Vec::new();
                        reader.read_to_end(&mut buf)?;
                        buf
                    } else {
                        reader.read_bytes(compressed_length)?
                    }
                }
            },
            BPatchHeader::BPatchHeaderV42(ref h) => match h.is_monolith {
                true => {
                    return Err(NKSError::Decompression(
                        "Legacy NKS monoliths are unsupported".into(),
                    ))
                }
                false => reader.read_bytes(compressed_length)?,
            },
        }) })().map_err(|e| NKSError::context(format!("NKS {version} compressed body at offset {at}, {size_field} {compressed_length}"), e))?;

        // std::fs::write("compressed", &compressed_data)?;

        let footer_raw = reader.read_all()?;
        let meta_info = match header {
            BPatchHeader::BPatchHeaderV1(_) => None,
            BPatchHeader::BPatchHeaderV2(_) => None,
            BPatchHeader::BPatchHeaderV42(_) => {
                Some(BPatchMetaInfoHeader::read(&mut Cursor::new(&footer_raw)).map_err(|e| NKSError::context(format!("NKS {version} footer at offset {}, available {} bytes", at + compressed_data.len() as u64, footer_raw.len()), e))?)
            }
        };

        // let meta_info = None;

        // std::fs::write("compressed", &reader.read_all()?)?;

        Ok(Self {
            header,
            compressed_data,
            meta_info,
        })
    }

    /// Decompress raw internal preset data
    pub fn decompressed_preset(&self) -> Result<Vec<u8>, Error> {
        self.decompressed_preset_bounded(usize::MAX)
    }

    /// Decompress with a caller-selected maximum expanded byte length.
    pub fn decompressed_preset_bounded(&self, max_expanded: usize) -> Result<Vec<u8>, Error> {
        if self.compressed_data.is_empty() {
            return Err(Error::Static("No compressed preset data"));
        }
        let reader = Cursor::new(&self.compressed_data);

        Ok(match &self.header {
            BPatchHeader::BPatchHeaderV1(_) | BPatchHeader::BPatchHeaderV2(_) => {
                // zlib compression
                let mut decoder = ZlibDecoder::new(reader);
                let mut decompressed_data = Vec::new();
                (&mut decoder).take(max_expanded as u64).read_to_end(&mut decompressed_data)?;
                if decoder.read(&mut [0])? != 0 {
                    return Err(Error::Static("Expanded NKS preset exceeds decode limit"));
                }

                decompressed_data
            }
            BPatchHeader::BPatchHeaderV42(ref h) => {
                // fastlz decompression
                // let decompressed_data = lz77::decompress(reader).expect("lz77");

                let decompressed_size = h.decompressed_length as usize;
                // The FastLZ binding passes both lengths to its C API as signed ints.
                if decompressed_size == 0
                    || i32::try_from(decompressed_size).is_err()
                    || i32::try_from(self.compressed_data.len()).is_err()
                {
                    return Err(Error::Static("Invalid expanded NKS preset size"));
                }
                if decompressed_size > max_expanded {
                    return Err(Error::Static("Expanded NKS preset exceeds decode limit"));
                }
                let mut decompressed_data = Vec::new();
                decompressed_data
                    .try_reserve_exact(decompressed_size)
                    .map_err(|_| Error::Static("Unable to allocate expanded NKS preset"))?;
                decompressed_data.resize(decompressed_size, 0);
                let result = fastlz::decompress(&self.compressed_data, &mut decompressed_data)
                    .map_err(|_| Error::Static("Invalid compressed NKS preset"))?;
                if result.len() != decompressed_size {
                    return Err(Error::Static("NKS decompression length mismatch"));
                }
                decompressed_data
            }
        })
    }

    /// Decompress internal preset data and return a KontaktPreset
    pub fn preset(&self) -> Result<KontaktPreset, Error> {
        let data = self.decompressed_preset()?;

        Ok(match &self.header {
            BPatchHeader::BPatchHeaderV1(_) => {
                let mut raw_preset = Cursor::new(data);

                KontaktPreset::KontaktV1(KontaktV1::read(&mut raw_preset)?)
            }
            BPatchHeader::BPatchHeaderV2(_) => {
                let raw_preset = Cursor::new(data);

                KontaktPreset::KontaktV2(KontaktV2::read(raw_preset)?)
            }
            BPatchHeader::BPatchHeaderV42(header) => {
                // fastlz compression
                let header: BPatchHeaderV42 = header.clone(); // an ugly clone

                KontaktPatch { header, data }.preset()?
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    #[test]
    fn bounded_zlib_checks_exact_output_length() -> Result<(), Error> {
        use std::io::Write;
        use crate::kontakt::objects::BPatchHeaderV1;
        let bytes = b"authored expanded preset";
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes)?;
        let container = NKSContainer {
            header: BPatchHeader::BPatchHeaderV1(BPatchHeaderV1 {
                u_version: 2, u_a: 0, u_b: 0, u_c: 0, u_d: 0,
                created_at: time::Date::from_calendar_date(2000, time::Month::January, 1).unwrap(),
                samples_size: 0,
            }),
            compressed_data: encoder.finish()?,
            meta_info: None,
        };
        assert_eq!(container.decompressed_preset_bounded(bytes.len())?, bytes);
        assert!(matches!(container.decompressed_preset_bounded(bytes.len() - 1),
            Err(Error::Static("Expanded NKS preset exceeds decode limit"))));
        Ok(())
    }

    #[test]
    fn bounded_fastlz_rejects_corrupt_expanded_header() -> Result<(), Error> {
        // Authored v42 header with a declared 2 GiB expansion and tiny compressed body.
        let mut bytes = vec![0; 212];
        bytes[..4].copy_from_slice(&0xEA37631Au32.to_le_bytes());
        let mut header = BPatchHeaderV42::read_le(Cursor::new(bytes))?;
        header.decompressed_length = i32::MAX as u32;
        let container = NKSContainer {
            header: BPatchHeader::BPatchHeaderV42(header),
            compressed_data: vec![3, b't', b'e', b's', b't'],
            meta_info: None,
        };
        assert!(matches!(container.decompressed_preset_bounded(256 * 1024 * 1024),
            Err(Error::Static("Expanded NKS preset exceeds decode limit"))));
        Ok(())
    }

    #[test]
    fn test_nksv1_nki_0x5ee56eb3() -> Result<(), NKSError> {
        let file = File::open("tests/data/Containers/NKS/KontaktV1/000-kontaktv1-nki.nki")?;
        let nks = NKSContainer::read(file)?;

        assert!(matches!(nks.header, BPatchHeader::BPatchHeaderV1(_)));
        Ok(())
    }

    #[test]
    #[ignore]
    fn test_nksfile_read_phv2_monolith_kon2_nki() -> Result<(), NKSError> {
        let file =
            File::open("tests/data/Containers/NKS/KontaktV2/000-phv2_monolith_kon2_nki.nki")?;
        let _nks = NKSContainer::read(file)?;
        Ok(())
    }

    #[test]
    fn test_nksfile_read_v42() -> Result<(), NKSError> {
        let file = File::open("tests/data/Containers/NKS/KontaktV42/4.2.4.5316-000.nki")?;
        let nks = NKSContainer::read(file)?;

        dbg!(nks.meta_info);
        // let _preset = nks.preset().unwrap();
        Ok(())
    }
}
