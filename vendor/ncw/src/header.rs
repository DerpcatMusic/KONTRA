use std::io::Read;

use crate::read_bytes::ReadBytesExt;

type Error = crate::NcwError;

pub const HEADER_SIZE: usize = 120;
const FILE_MAGICS: [u64; 2] = [0x01A89ED631010000, 0x01A89ED630010000];

/// The parsed fields of the 120-byte file header. Template writing preserves
/// the full header bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NcwHeader {
    pub channels: u16,
    pub bits_per_sample: u16,
    pub sample_rate: u32,
    /// Number of sample frames (samples per channel).
    pub num_samples: u32,
    /// Absolute offset of the block offset table.
    pub blocks_offset: u32,
    /// Absolute offset of the first block.
    pub data_offset: u32,
    pub data_size: u32,
    /// Signature version bytes 4..6 (0x131 seen, 0x130 accepted).
    pub version: u16,
    /// Sample-format word at offset 32. In version 0x131 bit 0 selects the
    /// 32-bit output type; version 0x130 ignores it (FORMAT.md field probes).
    pub format_word: u32,
}

impl NcwHeader {
    /// The header's 32-bit output type, where the format defines one: only
    /// 32-bit version 0x131 files. Other files take their type from block flags.
    pub fn declared_float(&self) -> Option<bool> {
        (self.bits_per_sample == 32 && self.version == 0x131).then_some(self.format_word & 1 != 0)
    }

    pub fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let buf = reader.read_bytes(HEADER_SIZE)?;
        let mut reader = buf.as_slice();

        let magic = reader.read_u64_be()?;
        if !FILE_MAGICS.contains(&magic) {
            return Err(Error::InvalidFileSignature);
        }
        let version = u16::from_le_bytes([(magic >> 24) as u8, (magic >> 16) as u8]);

        Ok(Self {
            channels: reader.read_u16_le()?,
            bits_per_sample: reader.read_u16_le()?,
            sample_rate: reader.read_u32_le()?,
            num_samples: reader.read_u32_le()?,
            blocks_offset: reader.read_u32_le()?,
            data_offset: reader.read_u32_le()?,
            data_size: reader.read_u32_le()?,
            version,
            format_word: reader.read_u32_le()?,
        })
    }

    /// Reject headers the decoder cannot act on before any data is read.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.channels == 0 {
            return Err(Error::InvalidHeader("channel count is zero"));
        }
        if !matches!(self.bits_per_sample, 8 | 16 | 24 | 32) {
            return Err(Error::InvalidHeader("unsupported bits per sample"));
        }
        if self.sample_rate == 0 {
            return Err(Error::InvalidHeader("sample rate is zero"));
        }
        if self.blocks_offset < HEADER_SIZE as u32 {
            return Err(Error::InvalidHeader("block table overlaps file header"));
        }
        if self.data_offset < self.blocks_offset {
            return Err(Error::InvalidHeader(
                "data offset precedes block offset table",
            ));
        }
        let table_bytes = self.data_offset - self.blocks_offset;
        let expected_entries = self.num_samples.div_ceil(512) + 1;
        if table_bytes != expected_entries * 4 {
            return Err(Error::InvalidHeader(
                "block table length does not match frame count",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_magic_is_an_error() {
        let bytes = [0u8; HEADER_SIZE];
        assert!(matches!(
            NcwHeader::read(&mut &bytes[..]),
            Err(Error::InvalidFileSignature)
        ));
    }

    #[test]
    fn short_header_is_an_io_error() {
        let bytes = [0u8; HEADER_SIZE - 1];
        assert!(matches!(
            NcwHeader::read(&mut &bytes[..]),
            Err(Error::IoError(_))
        ));
    }
}
