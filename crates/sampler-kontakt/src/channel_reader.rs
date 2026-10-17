//! Codec-neutral packet/window owner used by normal encoded sample readers.
//!
//! This is the existing UVI Symphonia conversion and seek path, shared without
//! a second decoder per output pack. Original file channel ordinals are retained;
//! no matrix, speaker-layout interpretation or stereo fold belongs here.

use sampler_core::{ChannelCount, Frame, Pcm, SourceStorage, SourceStorageFamily};
use std::io;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{Decoder, DecoderOptions},
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
    probe::Hint,
};

/// Pack already-decoded original channel ordinals into one immutable logical
/// resident source. No matrix/speaker-layout conversion is performed. The
/// limit covers output PCM plus the caller's retained interleaved f32 input;
/// codec workspace/encoded storage are separate caller admission obligations.
pub fn resident_storage_from_interleaved(rate: u32, channels: usize, samples: &[f32],
    byte_limit: usize) -> io::Result<SourceStorage> {
    let count = ChannelCount::new(channels).map_err(resident_error)?;
    if rate == 0 || samples.is_empty() || samples.len() % channels != 0 {
        return Err(invalid_input("incomplete resident channel geometry"));
    }
    let frames = samples.len() / channels;
    let output_bytes = frames.checked_mul(size_of::<Frame>()).and_then(|n|n.checked_mul(count.pack_count()))
        .ok_or_else(||invalid_input("resident channel output overflow"))?;
    let input_bytes = samples.len().checked_mul(size_of::<f32>()).ok_or_else(||invalid_input("resident input overflow"))?;
    if output_bytes > isize::MAX as usize || input_bytes.checked_add(output_bytes).is_none_or(|n|n>byte_limit) {
        return Err(io::Error::new(io::ErrorKind::OutOfMemory,"resident channel staging limit exceeded"));
    }
    let allocate=|e:std::collections::TryReserveError|io::Error::new(io::ErrorKind::OutOfMemory,e);
    let mut packs=Vec::new();packs.try_reserve_exact(count.pack_count()).map_err(allocate)?;
    for pack in 0..count.pack_count() {
        let mut pcm=Vec::new();pcm.try_reserve_exact(frames).map_err(allocate)?;
        for frame in samples.chunks_exact(channels) {
            let left=frame[pack*2];
            let right=if channels==1 {left} else {frame.get(pack*2+1).copied().unwrap_or(0.)};
            pcm.push([left,right]);
        }
        packs.push(Pcm::new(rate,pcm.into_boxed_slice()).map_err(resident_error)?);
    }
    if channels<=2 {Ok(SourceStorage::Stereo(packs.pop().ok_or_else(||invalid_input("resident pack absent"))?))}
    else {SourceStorageFamily::new_resident(count,packs).map(SourceStorage::Channels).map_err(resident_error)}
}

/// Install normal immutable-origin reload recipes on an unpublished resident
/// source. Every physical target retains its real AssetId. Each invocation
/// decodes a complete logical source, validates its geometry, and stages only
/// that target's original ordinal. No one-Pcm revision can relabel a family.
///
/// Off audio, initial construction only. Discard the unpublished source if any
/// installation fails. A worker may repeat the full decode once per pack;
/// retained packs + largest full decode + one extracted pack need admission.
pub fn install_resident_source_reload(source:&SourceStorage,
    reload:impl Fn()->Result<SourceStorage,sampler_core::Error>+Send+Sync+'static)->Result<(),sampler_core::Error> {
    let view=source.as_source();let channels=view.channels().get();let frames=view.frame_count();let rate=view.sample_rate();
    if view.resident_view().is_none() {return Err(sampler_core::Error::NotReady);}
    let mut packs=Vec::new();view.physical_dependencies(|pcm|packs.push(pcm.clone()));
    if packs.len()!=channels.div_ceil(2) {return Err(sampler_core::Error::InvalidInput);}
    let reload=std::sync::Arc::new(reload);
    for (ordinal,pcm) in packs.into_iter().enumerate() {
        let reload=reload.clone();
        pcm.set_residency_loader(move || {
            let decoded=reload()?;let logical=decoded.as_source();
            if logical.channels().get()!=channels || logical.sample_rate()!=rate || logical.frame_count()!=frames {
                return Err(sampler_core::Error::InvalidInput);
            }
            let view=logical.resident_view().ok_or(sampler_core::Error::NotReady)?;
            let mut out=Vec::new();out.try_reserve_exact(frames).map_err(|_|sampler_core::Error::Capacity)?;
            for frame in 0..frames {
                let left=view.sample(ordinal*2,frame).ok_or(sampler_core::Error::InvalidInput)?;
                let right=if ordinal*2+1<channels {view.sample(ordinal*2+1,frame).ok_or(sampler_core::Error::InvalidInput)?} else {0.};
                out.push([left,right]);
            }
            Ok(vec![(0,out.into_boxed_slice())])
        })?;
    }
    Ok(())
}

const SKIP_FRAMES: u64 = 16384;

fn invalid(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn resident_error(error: sampler_core::Error) -> io::Error {
    let kind = if error == sampler_core::Error::Capacity {
        io::ErrorKind::OutOfMemory
    } else {
        io::ErrorKind::InvalidData
    };
    io::Error::new(kind, error)
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Borrowed, complete destinations for one original-channel read.
///
/// Pack `p` contains original file ordinals `2*p` and `2*p+1`. The unused
/// right lane of an odd final pack is positive zero. Every pack has the exact
/// requested frame count; this wrapper allocates no storage and owns no PCM.
/// A later decoder error may leave partial output, so publication belongs to
/// the caller and must occur only after a successful complete read.
pub struct ChannelPackDestinations<'destinations, 'samples> {
    channels: usize,
    frames: usize,
    packs: &'destinations mut [&'samples mut [Frame]],
}

impl<'destinations, 'samples> ChannelPackDestinations<'destinations, 'samples> {
    /// Validate complete geometry and disjoint destination ranges before decode.
    pub fn new(
        channels: usize,
        frames: usize,
        packs: &'destinations mut [&'samples mut [Frame]],
    ) -> io::Result<Self> {
        let count = channels
            .checked_add(1)
            .filter(|_| channels != 0)
            .map(|n| n / 2)
            .ok_or_else(|| invalid_input("invalid original channel count"))?;
        channels
            .checked_mul(frames)
            .and_then(|n| n.checked_mul(size_of::<f32>()))
            .ok_or_else(|| invalid_input("channel read size overflow"))?;
        if packs.len() != count || packs.iter().any(|p| p.len() != frames) {
            return Err(invalid_input("incomplete channel pack destinations"));
        }
        let bytes = frames
            .checked_mul(size_of::<Frame>())
            .ok_or_else(|| invalid_input("channel pack byte size overflow"))?;
        for (index, pack) in packs.iter().enumerate() {
            let start = pack.as_ptr() as usize;
            let end = start
                .checked_add(bytes)
                .ok_or_else(|| invalid_input("channel pack address overflow"))?;
            for prior in &packs[..index] {
                let other = prior.as_ptr() as usize;
                let other_end = other
                    .checked_add(bytes)
                    .ok_or_else(|| invalid_input("channel pack address overflow"))?;
                if bytes != 0 && start < other_end && other < end {
                    return Err(invalid_input("overlapping channel pack destinations"));
                }
            }
        }
        Ok(Self {
            channels,
            frames,
            packs,
        })
    }

    fn write(&mut self, offset: usize, samples: &[f32]) {
        for (frame, original) in samples.chunks_exact(self.channels).enumerate() {
            for (pack, output) in self.packs.iter_mut().enumerate() {
                let ordinal = pack * 2;
                output[offset + frame] = [
                    original[ordinal],
                    original.get(ordinal + 1).copied().unwrap_or(0.0),
                ];
            }
        }
    }
}

/// One encoded source, one format reader, one decoder and one converted window.
///
/// The origin/decryption/identity guard stays in the supplied normal MediaSource.
/// There is no implicit reopen or encoded-source copy. The optional control-side
/// resident producer returns validated owned storage; it does not install cache
/// pages, source revisions, or a wider executable instrument into a Runtime.
pub struct EncodedChannelReader {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track: u32,
    channels: usize,
    rate: u32,
    frames: u64,
    buffer: Option<SampleBuffer<f32>>,
    // Valid frame extent of buffer.samples(); no second packet copy.
    window_frames: usize,
    start: u64,
    #[cfg(test)]
    decoded_packets: usize,
}

impl EncodedChannelReader {
    /// Open the existing, uncertified legacy decoder path.
    ///
    /// MetadataOptions are passed unchanged to the normal probe. They are not a
    /// proof of decoder/metadata allocation bounds. No decoder certificate or
    /// aggregate resource lease is manufactured by this constructor; callers
    /// must not use it to claim a new bounded FLAC or wide-stream admission.
    pub fn open_legacy(
        source: Box<dyn MediaSource>,
        metadata: &MetadataOptions,
    ) -> io::Result<Self> {
        let stream = MediaSourceStream::new(source, Default::default());
        let mut format = symphonia::default::get_probe()
            .format(&Hint::new(), stream, &FormatOptions::default(), metadata)
            .map_err(|e| invalid(format!("not WAV, AIFF or FLAC: {e}")))?
            .format;
        let track = format
            .default_track()
            .ok_or_else(|| invalid("no audio track"))?;
        let (id, params) = (track.id, track.codec_params.clone());
        let rate = params
            .sample_rate
            .ok_or_else(|| invalid("undeclared sample rate"))?;
        let channels = params
            .channels
            .ok_or_else(|| invalid("undeclared channel count"))?
            .count();
        let frames = params.n_frames.ok_or_else(|| {
            invalid("streamed source has no declared frame count; full scan unsupported")
        })?;
        if rate == 0 || channels == 0 || frames == 0 {
            return Err(invalid("invalid audio dimensions"));
        }
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(invalid)?;
        Ok(Self {
            format,
            decoder,
            track: id,
            channels,
            rate,
            frames,
            buffer: None,
            window_frames: 0,
            start: 0,
            #[cfg(test)]
            decoded_packets: 0,
        })
    }

    /// Number of original file channels, without stereo truncation.
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Original source clock rate.
    pub fn rate(&self) -> u32 {
        self.rate
    }
    /// Declared original frame count.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    fn window_end(&self) -> u64 {
        self.start + self.window_frames as u64
    }

    fn advance(&mut self) -> io::Result<()> {
        loop {
            let packet = self.format.next_packet().map_err(invalid)?;
            if packet.track_id() != self.track {
                continue;
            }
            let decoded = self.decoder.decode(&packet).map_err(invalid)?;
            if decoded.spec().channels.count() != self.channels || decoded.spec().rate != self.rate
            {
                return Err(invalid("decoded signal geometry changed"));
            }
            let new_start = packet.ts();
            let new_window_frames = decoded.frames();
            new_start
                .checked_add(new_window_frames as u64)
                .ok_or_else(|| invalid("decoded packet timestamp overflow"))?;
            let samples = decoded
                .frames()
                .checked_mul(self.channels)
                .ok_or_else(|| invalid("decoded sample count overflow"))?;
            let capacity = decoded
                .capacity()
                .checked_mul(self.channels)
                .and_then(|n| n.checked_mul(size_of::<f32>()))
                .ok_or_else(|| invalid("decoded capacity overflow"))?;
            if capacity > isize::MAX as usize {
                return Err(invalid("decoded capacity exceeds address space"));
            }
            let buffer = self.buffer.get_or_insert_with(|| {
                SampleBuffer::new(decoded.capacity() as u64, *decoded.spec())
            });
            if samples > buffer.capacity() {
                return Err(invalid(
                    "decoded packet exceeds initial conversion capacity",
                ));
            }
            buffer.copy_interleaved_ref(decoded);
            // All fallible checks preceded conversion. Publish the matching
            // valid extent and timestamp together with no intervening error.
            self.window_frames = new_window_frames;
            self.start = new_start;
            #[cfg(test)]
            {
                self.decoded_packets += 1;
            }
            return Ok(());
        }
    }

    fn checked_end(&self, start: u64, frames: usize) -> io::Result<u64> {
        let frames = u64::try_from(frames)
            .map_err(|_| invalid_input("channel read frame count overflow"))?;
        let end = start
            .checked_add(frames)
            .ok_or_else(|| invalid_input("channel read range overflow"))?;
        if end > self.frames {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(end)
    }

    /// The existing packet/seek traversal, used once for every requested channel.
    fn read_segments(
        &mut self,
        start: u64,
        end: u64,
        mut consume: impl FnMut(usize, &[f32]),
    ) -> io::Result<()> {
        let channels = self.channels;
        let mut at = start;
        while at < end {
            let window_end = self.window_end();
            if at >= self.start && at < window_end {
                let n = end.min(window_end) - at;
                let from = ((at - self.start) as usize) * channels;
                let to = (at - start) as usize;
                let len = n as usize * channels;
                // A valid window only exists after a successful conversion.
                let buffer = self
                    .buffer
                    .as_ref()
                    .ok_or_else(|| invalid("decoded window has no conversion buffer"))?;
                consume(to, &buffer.samples()[from..from + len]);
                at += n;
                continue;
            }
            if !(at >= window_end && at - window_end < SKIP_FRAMES) {
                let sought = self
                    .format
                    .seek(
                        SeekMode::Accurate,
                        SeekTo::TimeStamp {
                            ts: at,
                            track_id: self.track,
                        },
                    )
                    .map_err(invalid)?;
                self.decoder.reset();
                self.window_frames = 0;
                self.start = sought.actual_ts;
            }
            self.advance()?;
        }
        Ok(())
    }

    /// Fill whole original interleaved frames using the legacy conversion law.
    pub fn read_interleaved(&mut self, start: u64, out: &mut [f32]) -> io::Result<()> {
        if !out.len().is_multiple_of(self.channels) {
            return Err(invalid_input(
                "interleaved destination contains a partial frame",
            ));
        }
        let end = self.checked_end(start, out.len() / self.channels)?;
        let channels = self.channels;
        self.read_segments(start, end, |offset, samples| {
            let offset = offset * channels;
            out[offset..offset + samples.len()].copy_from_slice(samples);
        })
    }

    /// Decode once and scatter all original channels into complete ordered packs.
    /// No per-pack seek, decoder, packet conversion, source cursor or heap scratch.
    pub fn read_packs(
        &mut self,
        start: u64,
        frames: usize,
        out: &mut ChannelPackDestinations<'_, '_>,
    ) -> io::Result<()> {
        if out.channels != self.channels || out.frames != frames {
            return Err(invalid_input(
                "channel pack geometry differs from encoded source",
            ));
        }
        let end = self.checked_end(start, frames)?;
        self.read_segments(start, end, |offset, samples| out.write(offset, samples))
    }

    /// Decode this complete original source directly into resident final packs.
    ///
    /// `pcm_byte_limit` admits only final resident PCM payload bytes (including
    /// the unused odd lane and legacy mono duplication). It does not account for
    /// decoder/container state, encoded bytes, allocator slack, Vec/Pcm/Arc
    /// metadata, retained revisions or RSS, and is not a global resource lease.
    /// The reader was opened through the explicitly uncertified legacy path.
    ///
    /// Mono/stereo retain the existing Stereo storage contract; mono duplicates
    /// the identical converted value into both lanes. Wider sources retain every
    /// original ordinal in an internally identified immutable resident family.
    /// No SourceStorage is returned until all decoding and pack validation pass.
    /// This control-side producer does not install storage into Prepared/Runtime
    /// or activate a new importer, codec domain, streaming or residency policy.
    pub fn read_resident_storage(&mut self, pcm_byte_limit: usize) -> io::Result<SourceStorage> {
        let frames = usize::try_from(self.frames)
            .map_err(|_| invalid_input("resident source frame count exceeds address space"))?;
        let channels = ChannelCount::new(self.channels).map_err(resident_error)?;
        self.channels
            .checked_mul(frames)
            .and_then(|samples| samples.checked_mul(size_of::<f32>()))
            .ok_or_else(|| invalid_input("resident source sample size overflow"))?;
        let pack_count = channels.pack_count();
        let pack_bytes = frames
            .checked_mul(size_of::<Frame>())
            .ok_or_else(|| invalid_input("resident pack byte size overflow"))?;
        let pcm_bytes = pack_bytes
            .checked_mul(pack_count)
            .ok_or_else(|| invalid_input("resident PCM byte size overflow"))?;
        if pack_bytes > isize::MAX as usize || pcm_bytes > pcm_byte_limit {
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "resident PCM output limit exceeded",
            ));
        }
        // Check all temporary vector element counts before allocating any final
        // PCM or advancing the reader. These are metadata, not PCM-limit claims.
        for element_size in [
            size_of::<Vec<Frame>>(),
            size_of::<Pcm>(),
            size_of::<&mut [Frame]>(),
        ] {
            pack_count
                .checked_mul(element_size)
                .filter(|bytes| *bytes <= isize::MAX as usize)
                .ok_or_else(|| invalid_input("resident pack metadata size overflow"))?;
        }
        let allocation = |error: std::collections::TryReserveError| {
            io::Error::new(io::ErrorKind::OutOfMemory, error)
        };
        let mut packs = Vec::new();
        packs.try_reserve_exact(pack_count).map_err(allocation)?;
        let mut pcm = Vec::new();
        pcm.try_reserve_exact(pack_count).map_err(allocation)?;
        for _ in 0..pack_count {
            let mut pack = Vec::new();
            pack.try_reserve_exact(frames).map_err(allocation)?;
            pack.resize(frames, [0.0; 2]);
            packs.push(pack);
        }
        {
            let mut slices = Vec::new();
            slices.try_reserve_exact(pack_count).map_err(allocation)?;
            slices.extend(packs.iter_mut().map(Vec::as_mut_slice));
            let mut out = ChannelPackDestinations::new(self.channels, frames, &mut slices)?;
            self.read_packs(0, frames, &mut out)?;
        }
        if self.channels == 1 {
            // Exact legacy mono duplication, with no interleaved file scratch.
            for frame in &mut packs[0] {
                frame[1] = frame[0];
            }
        }
        for pack in packs {
            pcm.push(Pcm::new(self.rate, pack.into_boxed_slice()).map_err(resident_error)?);
        }
        if self.channels <= 2 {
            let only = pcm
                .pop()
                .ok_or_else(|| invalid("resident stereo pack absent"))?;
            Ok(SourceStorage::Stereo(only))
        } else {
            SourceStorageFamily::new_resident(channels, pcm)
                .map(SourceStorage::Channels)
                .map_err(resident_error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const FRAMES: usize = 6000;
    fn sample(frame: usize, channel: usize) -> i16 {
        ((frame * 29 + channel * 997) % 8191) as i16 - 4095
    }
    fn expected(frame: usize, channel: usize) -> f32 {
        f32::from(sample(frame, channel)) / 32768.0
    }
    fn wav(channels: usize) -> Vec<u8> {
        let data = FRAMES * channels * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16_u32.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&(channels as u16).to_le_bytes());
        out.extend_from_slice(&48000_u32.to_le_bytes());
        out.extend_from_slice(&(48000 * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels as u16 * 2).to_le_bytes());
        out.extend_from_slice(&16_u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data as u32).to_le_bytes());
        for frame in 0..FRAMES {
            for channel in 0..channels {
                out.extend_from_slice(&sample(frame, channel).to_le_bytes());
            }
        }
        out
    }
    fn reader(channels: usize) -> EncodedChannelReader {
        EncodedChannelReader::open_legacy(
            Box::new(Cursor::new(wav(channels))),
            &MetadataOptions::default(),
        )
        .unwrap()
    }

    #[test]
    fn encoded_channel_reader_legacy_mono_stereo_conversion_and_seeks() {
        for channels in [1, 2] {
            let mut source = reader(channels);
            assert_eq!(
                (source.channels(), source.rate(), source.frames()),
                (channels, 48000, FRAMES as u64)
            );
            for (start, frames) in [(0, 1400), (3500, 500), (25, 128), (5500, 500)] {
                let mut out = vec![f32::NAN; frames * channels];
                source.read_interleaved(start as u64, &mut out).unwrap();
                for frame in 0..frames {
                    for channel in 0..channels {
                        assert_eq!(
                            out[frame * channels + channel].to_bits(),
                            expected(start + frame, channel).to_bits()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn encoded_channel_reader_scatter_preserves_all_original_ordinals() {
        for channels in [1_usize, 2, 6] {
            let mut source = reader(channels);
            let (start, frames) = (1000, 2300);
            let mut packs = vec![vec![[f32::NAN; 2]; frames]; channels.div_ceil(2)];
            {
                let mut slices = packs.iter_mut().map(Vec::as_mut_slice).collect::<Vec<_>>();
                let mut out = ChannelPackDestinations::new(channels, frames, &mut slices).unwrap();
                source.read_packs(start as u64, frames, &mut out).unwrap();
            }
            for (pack, data) in packs.iter().enumerate() {
                for (frame, value) in data.iter().enumerate() {
                    for (lane, actual) in value.iter().enumerate() {
                        let ordinal = pack * 2 + lane;
                        let wanted = if ordinal < channels {
                            expected(start + frame, ordinal)
                        } else {
                            0.0
                        };
                        assert_eq!(
                            actual.to_bits(),
                            wanted.to_bits(),
                            "channel {ordinal} frame {frame}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn encoded_channel_reader_pack_geometry_rejects_before_decode_or_write() {
        let mut source = reader(6);
        let mut one = vec![[0.75, -0.5]; 16];
        {
            let mut slices = [one.as_mut_slice()];
            assert!(ChannelPackDestinations::new(6, 16, &mut slices).is_err());
            let mut out = ChannelPackDestinations::new(2, 16, &mut slices).unwrap();
            assert_eq!(
                source.read_packs(0, 16, &mut out).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert_eq!(source.decoded_packets, 0);
        assert!(one.iter().all(|f| *f == [0.75, -0.5]));
        let mut slices = [one.as_mut_slice()];
        assert!(ChannelPackDestinations::new(2, 15, &mut slices).is_err());
        assert!(ChannelPackDestinations::new(0, 16, &mut slices).is_err());
        assert!(ChannelPackDestinations::new(usize::MAX, 16, &mut slices).is_err());
    }

    #[test]
    fn encoded_channel_reader_bad_ranges_preserve_outputs_and_normal_retry() {
        let mut source = reader(2);
        let mut partial = [0.75; 3];
        assert_eq!(
            source.read_interleaved(0, &mut partial).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        let mut frame = [0.75; 2];
        assert_eq!(
            source
                .read_interleaved(u64::MAX, &mut frame)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            source
                .read_interleaved(FRAMES as u64, &mut frame)
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert_eq!(source.decoded_packets, 0);
        assert_eq!(partial, [0.75; 3]);
        assert_eq!(frame, [0.75; 2]);
        source.read_interleaved(12, &mut frame).unwrap();
        assert_eq!(frame, [expected(12, 0), expected(12, 1)]);
    }

    #[test]
    fn encoded_channel_reader_one_packet_is_shared_by_all_packs_and_reread() {
        let mut source = reader(6);
        let mut packs = vec![vec![[0.0; 2]; 32]; 3];
        let mut slices = packs.iter_mut().map(Vec::as_mut_slice).collect::<Vec<_>>();
        let mut out = ChannelPackDestinations::new(6, 32, &mut slices).unwrap();
        source.read_packs(0, 32, &mut out).unwrap();
        assert_eq!(source.decoded_packets, 1);
        source.read_packs(0, 32, &mut out).unwrap();
        assert_eq!(source.decoded_packets, 1);
    }
}
