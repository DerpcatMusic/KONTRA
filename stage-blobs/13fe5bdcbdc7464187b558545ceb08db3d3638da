use std::io::{Read, Seek, SeekFrom};

use crate::block::{BlockHeader, ChannelEncoding, SAMPLES_PER_BLOCK, SampleFormat};
use crate::block::{decode_mid_side, read_block};
use crate::header::NcwHeader;
use crate::read_bytes::ReadBytesExt;

type Error = crate::NcwError;

/// Reader for an NCW file.
///
/// Samples are returned as `i32`. For PCM files (see [`NcwReader::sample_format`])
/// they are sign-extended integer samples at the header's bit depth. For float
/// files they are the raw IEEE-754 bit patterns; convert with
/// `f32::from_bits(sample as u32)`.
#[derive(Debug)]
pub struct NcwReader<R> {
    reader: R,
    pub header: NcwHeader,
    /// Byte offset of each block, relative to `header.data_offset`.
    pub block_offsets: Vec<u32>,
    /// Sample format declared by the first block of the file.
    pub sample_format: SampleFormat,
    /// Reader position after the last decoded block, if nothing moved it since.
    position: Option<u64>,
    /// Per-channel samples of the last decoded block, reused across blocks.
    channels: Vec<Vec<i32>>,
}

impl<R: Read + Seek> NcwReader<R> {
    /// Parse the file header and block offset table.
    pub fn read(mut reader: R) -> Result<Self, Error> {
        let header = NcwHeader::read(&mut reader)?;
        header.validate()?;

        let file_len = reader.seek(SeekFrom::End(0))?;
        if u64::from(header.data_offset) + u64::from(header.data_size) > file_len {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
        }

        // The table holds one entry per block plus a trailing end-of-data
        // sentinel, which is not a block.
        let table_entries = (header.data_offset - header.blocks_offset) / 4;
        let num_blocks = table_entries.saturating_sub(1) as usize;

        reader.seek(SeekFrom::Start(header.blocks_offset as u64))?;
        // Not pre-sized: `num_blocks` comes from an untrusted header, and a
        // short file will fail the reads long before the vector grows large.
        let mut block_offsets = Vec::new();
        for _ in 0..num_blocks {
            block_offsets.push(reader.read_u32_le()?);
        }

        let sentinel = reader.read_u32_le()?;
        if sentinel != header.data_size {
            return Err(Error::InvalidHeader(
                "terminal offset differs from data size",
            ));
        }
        validate_offsets(&header, &block_offsets)?;

        let sample_format = match block_offsets.first() {
            Some(&offset) => {
                reader.seek(SeekFrom::Start(header.data_offset as u64 + offset as u64))?;
                BlockHeader::read(&mut reader)?.sample_format()
            }
            None => SampleFormat::Pcm,
        };

        Ok(Self {
            reader,
            header,
            block_offsets,
            sample_format,
            position: None,
            channels: Vec::new(),
        })
    }

    /// Borrow the underlying reader.
    pub fn get_ref(&self) -> &R {
        &self.reader
    }

    /// Mutably borrow the underlying reader. Moving its position is harmless:
    /// the next block read seeks explicitly.
    pub fn get_mut(&mut self) -> &mut R {
        self.position = None;
        &mut self.reader
    }

    /// Consume the decoder and return the underlying reader.
    pub fn into_inner(self) -> R {
        self.reader
    }

    /// Decode every block into interleaved 32-bit samples.
    ///
    /// The result has exactly `num_samples * channels` entries. Mid/side
    /// encoded blocks (see [`BlockHeader::channel_encoding`]) are converted to
    /// left/right, so the output is always plain channel order.
    pub fn decode_samples(&mut self) -> Result<Vec<i32>, Error> {
        // These fields are public, so validate again in case the caller edited them.
        self.header.validate()?;
        validate_offsets(&self.header, &self.block_offsets)?;
        let file_len = self.reader.seek(SeekFrom::End(0))?;
        if u64::from(self.header.data_offset) + u64::from(self.header.data_size) > file_len {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
        }
        let mut interleaved = Vec::new();
        for index in 0..self.block_offsets.len() {
            self.decode_block_into(index, &mut interleaved)?;
        }
        Ok(interleaved)
    }

    /// Number of frames in each block except possibly the last.
    pub const FRAMES_PER_BLOCK: usize = SAMPLES_PER_BLOCK;

    /// Decode one block and append its interleaved frames to `out`, returning
    /// the frame count. Blocks are independently addressable, so a streamer can
    /// seek to any frame by decoding `frame / FRAMES_PER_BLOCK`.
    pub fn decode_block_into(&mut self, index: usize, out: &mut Vec<i32>) -> Result<usize, Error> {
        let channels = self.decode_block(index)?;
        let frames = channels.first().map_or(0, Vec::len);
        let additional = frames
            .checked_mul(channels.len())
            .ok_or(Error::InvalidHeader("decoded sample count overflow"))?;
        out.try_reserve(additional)
            .map_err(|_| Error::InvalidHeader("decoded samples exceed available memory"))?;
        for i in 0..frames {
            for channel in channels {
                out.push(channel[i]);
            }
        }
        Ok(frames)
    }

    /// Decode one block into per-channel sample buffers, each holding the
    /// block's frames. The buffers are reused, so decoding allocates nothing
    /// once warm.
    pub fn decode_block(&mut self, index: usize) -> Result<&[Vec<i32>], Error> {
        let num_samples = self.header.num_samples as usize;
        let num_channels = self.header.channels as usize;
        let offset = *self
            .block_offsets
            .get(index)
            .ok_or(Error::InvalidHeader("block index out of range"))?;
        let end = self
            .block_offsets
            .get(index + 1)
            .copied()
            .unwrap_or(self.header.data_size);
        if end < offset || end > self.header.data_size {
            return Err(Error::InvalidHeader(
                "invalid block offsets or channel framing",
            ));
        }
        // Consecutive blocks need no seek, which would discard a read buffer.
        let start = u64::from(self.header.data_offset) + u64::from(offset);
        if self.position.take() != Some(start) {
            self.reader.seek(SeekFrom::Start(start))?;
        }
        // A malformed payload must never consume bytes from the next group.
        let mut group = self.reader.by_ref().take(u64::from(end - offset));
        let channels = &mut self.channels;
        channels.resize_with(num_channels, Vec::new);
        let mut mid_side = false;
        for (channel_index, channel) in channels.iter_mut().enumerate() {
            let block_header = BlockHeader::read(&mut group).map_err(|error| match error {
                Error::InvalidBlockSignature => Error::InvalidBlockSignatureAt {
                    block: index,
                    channel: channel_index,
                },
                other => other,
            })?;
            if block_header.sample_format() != self.sample_format {
                return Err(Error::InvalidHeader("sample format changes between blocks"));
            }
            if channel_index == 0 {
                mid_side = block_header.channel_encoding() == ChannelEncoding::MidSide;
                if mid_side && num_channels != 2 {
                    return Err(Error::InvalidHeader(
                        "mid/side encoding requires exactly two channels",
                    ));
                }
            }
            channel.clear();
            read_block(&mut group, &self.header, &block_header, channel)?;
        }
        if group.limit() != 0 {
            return Err(Error::InvalidHeader("unconsumed block bytes"));
        }
        self.position = Some(u64::from(self.header.data_offset) + u64::from(end));
        if mid_side {
            let (mid, side) = channels.split_at_mut(1);
            decode_mid_side(&mut mid[0], &mut side[0], self.sample_format);
        }
        let frames = (num_samples.saturating_sub(index * SAMPLES_PER_BLOCK)).min(SAMPLES_PER_BLOCK);
        for channel in channels.iter_mut() {
            channel.truncate(frames);
        }
        Ok(channels)
    }
}

// Check framing before allocating any decoded sample buffers. Width one is the
// smallest representation accepted by the legacy reader: 16 header + 64 payload bytes.
fn validate_offsets(header: &NcwHeader, offsets: &[u32]) -> Result<(), Error> {
    if offsets.len() != header.num_samples.div_ceil(SAMPLES_PER_BLOCK as u32) as usize {
        return Err(Error::InvalidHeader(
            "block count does not match frame count",
        ));
    }
    if offsets.is_empty() {
        if header.data_size != 0 {
            return Err(Error::InvalidHeader("empty audio has nonempty block data"));
        }
        return Ok(());
    }
    if offsets[0] != 0 {
        return Err(Error::InvalidHeader("first block offset is not zero"));
    }
    let minimum = u32::from(header.channels) * 80;
    for (index, &start) in offsets.iter().enumerate() {
        let end = offsets.get(index + 1).copied().unwrap_or(header.data_size);
        if end > header.data_size || end.checked_sub(start).is_none_or(|size| size < minimum) {
            return Err(Error::InvalidHeader(
                "invalid block offsets or channel framing",
            ));
        }
    }
    Ok(())
}
