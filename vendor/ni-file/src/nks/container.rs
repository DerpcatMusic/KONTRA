use std::io::{Cursor, Read, SeekFrom};

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
        let size_field = if version == "v1" {
            "zlib offset"
        } else {
            "declared length"
        };

        let compressed_data = (|| -> Result<Vec<u8>, NKSError> { Ok(match header {
            BPatchHeader::BPatchHeaderV1(_) => {
                // V1 stores the compressed stream's absolute start, not size.
                if compressed_length < at as usize {
                    return Err(NKSError::Decompression("Invalid V1 zlib offset".into()));
                }
                reader.seek(SeekFrom::Start(compressed_length as u64))?;
                reader.read_all()?
            },
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
            BPatchHeader::BPatchHeaderV42(_) => Some(
                BPatchMetaInfoHeader::read(&mut Cursor::new(&footer_raw)).map_err(|e| {
                    NKSError::context(
                        format!(
                            "NKS {version} footer at offset {}, available {} bytes",
                            at + compressed_data.len() as u64,
                            footer_raw.len()
                        ),
                        e,
                    )
                })?,
            ),
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
        self.decompressed_preset_bounded(128 << 20)
    }

    /// Decode on a worker with an explicit expansion budget, including XML.
    pub fn decompressed_preset_bounded(&self, limit: usize) -> Result<Vec<u8>, Error> {
        if self.compressed_data.is_empty() {
            return Err(Error::Static("No compressed preset data"));
        }
        let reader = Cursor::new(&self.compressed_data);

        Ok(match &self.header {
            BPatchHeader::BPatchHeaderV1(_) | BPatchHeader::BPatchHeaderV2(_) => {
                // zlib compression
                let decoder = ZlibDecoder::new(reader);
                let mut decompressed_data = Vec::new();
                decoder
                    .take((limit as u64).saturating_add(1))
                    .read_to_end(&mut decompressed_data)?;
                if decompressed_data.len() > limit {
                    return Err(Error::Static("Expanded NKS preset exceeds decode limit"));
                }

                decompressed_data
            }
            BPatchHeader::BPatchHeaderV42(ref h) => {
                // fastlz decompression
                // let decompressed_data = lz77::decompress(reader).expect("lz77");

                let decompressed_size = h.decompressed_length as usize;
                // The FastLZ binding passes both lengths to its C API as signed ints.
                if decompressed_size > limit {
                    return Err(Error::Static("Expanded NKS preset exceeds decode limit"));
                }
                if decompressed_size == 0
                    || i32::try_from(decompressed_size).is_err()
                    || i32::try_from(self.compressed_data.len()).is_err()
                {
                    return Err(Error::Static("Invalid expanded NKS preset size"));
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
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
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
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_nksfile_read_v42() -> Result<(), NKSError> {
        let file = File::open("tests/data/Containers/NKS/KontaktV42/4.2.4.5316-000.nki")?;
        let nks = NKSContainer::read(file)?;

        dbg!(nks.meta_info);
        // let _preset = nks.preset().unwrap();
        Ok(())
    }
}
