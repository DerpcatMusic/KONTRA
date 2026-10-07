//! Random-access reads of UVI samples, for streaming: the encoded audio file
//! (WAV, AIFF or FLAC; loose, or a bank member decrypted in memory as it is
//! read, never written out) is decoded a window at a time through the same
//! decoder a full load uses, so a streamed frame equals the loaded frame.

use sampler_kontakt::{AssetSource, SampleReader};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    sync::Arc,
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{Decoder, DecoderOptions},
    errors::Error as AudioError,
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::{MediaSource, MediaSourceStream},
    meta::{Limit, MetadataOptions},
    probe::Hint,
};

/// Bytes per file read; a multiple of the cipher's 512-byte blocks.
const CHUNK: usize = 32 << 10;
/// A read this far past the decoded window decodes forward rather than seeks.
const SKIP_FRAMES: u64 = 16384;

/// Where one encoded audio file's bytes live.
#[derive(Clone)]
pub(crate) enum Origin {
    /// A whole loose file.
    File(PathBuf),
    /// A bank member and the cipher key it is stored under.
    #[cfg(feature = "library-access")]
    Member {
        ufs: Arc<crate::ufs::Ufs>,
        offset: u64,
        size: u64,
        key: Option<u64>,
    },
}

impl Origin {
    fn open(&self) -> io::Result<Bytes> {
        let (file, base, size, key, guard) = match self {
            Self::File(path) => {
                let file = File::open(path)?;
                let meta = file.metadata()?;
                (file, 0, meta.len(), None, Guard::Loose(meta.len(), meta.modified().ok()))
            }
            #[cfg(feature = "library-access")]
            Self::Member { ufs, offset, size, key } => (
                ufs.open_snapshot().map_err(|e| io::Error::other(e.to_string()))?,
                *offset,
                *size,
                *key,
                Guard::Bank(ufs.clone()),
            ),
        };
        Ok(Bytes { file, base, size, key, guard, pos: 0, buf: Vec::new(), at: 0, riff: false })
    }
}

/// What a read re-checks, so a library changed under a stream fails cleanly.
enum Guard {
    /// Length and modification time of a loose file when it was opened.
    Loose(u64, Option<std::time::SystemTime>),
    #[cfg(feature = "library-access")]
    Bank(Arc<crate::ufs::Ufs>),
}

impl Guard {
    fn check(&self, file: &File) -> io::Result<()> {
        match self {
            Self::Loose(len, modified) => {
                let meta = file.metadata()?;
                if meta.len() != *len || meta.modified().ok() != *modified {
                    return Err(invalid("sample file changed while streaming"));
                }
                Ok(())
            }
            #[cfg(feature = "library-access")]
            Self::Bank(ufs) => ufs.check_snapshot(file).map_err(|e| invalid(e)),
        }
    }
}

/// A seekable, on-the-fly decrypted view of one member.
struct Bytes {
    guard: Guard,
    file: File,
    /// Physical offset of the member, also the cipher nonce base.
    base: u64,
    size: u64,
    key: Option<u64>,
    pos: u64,
    /// Decrypted bytes from member offset `at`.
    buf: Vec<u8>,
    at: u64,
    riff: bool,
}

impl Read for Bytes {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.size {
            return Ok(0);
        }
        let end = self.at + self.buf.len() as u64;
        if self.buf.is_empty() || self.pos < self.at || self.pos >= end {
            let start = self.pos - self.pos % 512;
            let len = CHUNK.min(usize::try_from(self.size - start).unwrap_or(CHUNK));
            self.buf.resize(len, 0);
            self.file.seek(SeekFrom::Start(self.base + start))?;
            let mut filled = 0;
            while filled < len {
                match self.file.read(&mut self.buf[filled..])? {
                    0 => break,
                    n => filled += n,
                }
            }
            if filled < len {
                self.buf.clear();
                return Err(invalid("sample data truncated"));
            }
            self.guard.check(&self.file)?;
            if let Some(key) = self.key {
                crate::crypto::transform_blocks(&mut self.buf, key, self.base + start);
            }
            self.at = start;
            if start == 0 {
                self.riff = self.buf.starts_with(b"RIFF");
            }
            // Symphonia needs a consistent RIFF extent; banks do not always keep one.
            if self.riff && start == 0 && self.buf.len() >= 8 {
                self.buf[4..8].copy_from_slice(&((self.size - 8) as u32).to_le_bytes());
            }
        }
        let offset = (self.pos - self.at) as usize;
        let n = out.len().min(self.buf.len().saturating_sub(offset));
        out[..n].copy_from_slice(&self.buf[offset..offset + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Bytes {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.pos = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(delta) => self.size.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
        }
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(self.pos)
    }
}

impl MediaSource for Bytes {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.size)
    }
}

fn invalid(e: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

/// One encoded file, read by frame position.
struct Stream {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track: u32,
    channels: usize,
    rate: u32,
    frames: u64,
    buffer: Option<SampleBuffer<f32>>,
    /// Interleaved samples of the last decoded packet, from frame `start`.
    window: Vec<f32>,
    start: u64,
}

impl Stream {
    fn open(origin: &Origin) -> io::Result<Self> {
        let stream = MediaSourceStream::new(Box::new(origin.open()?), Default::default());
        let metadata = MetadataOptions {
            limit_metadata_bytes: Limit::Maximum(2 << 20),
            limit_visual_bytes: Limit::Maximum(2 << 20),
        };
        let mut format = symphonia::default::get_probe()
            .format(&Hint::new(), stream, &FormatOptions::default(), &metadata)
            .map_err(|e| invalid(format!("not WAV, AIFF or FLAC: {e}")))?
            .format;
        let track = format.default_track().ok_or_else(|| invalid("no audio track"))?;
        let (id, params) = (track.id, track.codec_params.clone());
        let rate = params.sample_rate.ok_or_else(|| invalid("undeclared sample rate"))?;
        let channels = params.channels.ok_or_else(|| invalid("undeclared channel count"))?.count();
        let frames = match params.n_frames {
            Some(n) => n,
            None => {
                let mut total = 0;
                loop {
                    match format.next_packet() {
                        Ok(p) if p.track_id() == id => total += p.dur,
                        Ok(_) => {}
                        Err(AudioError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                        Err(e) => return Err(invalid(e)),
                    }
                }
                format
                    .seek(SeekMode::Accurate, SeekTo::TimeStamp { ts: 0, track_id: id })
                    .map_err(invalid)?;
                total
            }
        };
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
            window: Vec::new(),
            // Nothing decoded yet: the first packet is frame 0.
            start: 0,
        })
    }

    fn window_end(&self) -> u64 {
        self.start + (self.window.len() / self.channels) as u64
    }

    /// Decode the next packet into the window.
    fn advance(&mut self) -> io::Result<()> {
        loop {
            let packet = self.format.next_packet().map_err(invalid)?;
            if packet.track_id() != self.track {
                continue;
            }
            let decoded = self.decoder.decode(&packet).map_err(invalid)?;
            let buffer = self
                .buffer
                .get_or_insert_with(|| SampleBuffer::new(decoded.capacity() as u64, *decoded.spec()));
            buffer.copy_interleaved_ref(decoded);
            self.window.clear();
            self.window.extend_from_slice(buffer.samples());
            self.start = packet.ts();
            return Ok(());
        }
    }

    /// Fill `out` (whole interleaved frames) from frame `start`.
    fn read(&mut self, start: u64, out: &mut [f32]) -> io::Result<()> {
        let channels = self.channels;
        let end = start + (out.len() / channels) as u64;
        if end > self.frames {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let mut at = start;
        while at < end {
            let window_end = self.window_end();
            if at >= self.start && at < window_end {
                let n = end.min(window_end) - at;
                let from = ((at - self.start) as usize) * channels;
                let to = ((at - start) as usize) * channels;
                let len = n as usize * channels;
                out[to..to + len].copy_from_slice(&self.window[from..from + len]);
                at += n;
                continue;
            }
            // Decode forward when the target is at or just past the window,
            // else seek (to a frame at or before it).
            if !(at >= window_end && at - window_end < SKIP_FRAMES) {
                let to = SeekTo::TimeStamp { ts: at, track_id: self.track };
                let sought = self.format.seek(SeekMode::Accurate, to).map_err(invalid)?;
                self.decoder.reset();
                self.window.clear();
                self.start = sought.actual_ts;
            }
            self.advance()?;
        }
        Ok(())
    }
}

/// An audio resource: one file, or a bundle of synchronized mono files.
pub(crate) struct Sample {
    pub parts: Vec<Origin>,
}

impl AssetSource for Sample {
    fn open(&self) -> io::Result<SampleReader> {
        let mut streams = self.parts.iter().map(Stream::open).collect::<io::Result<Vec<_>>>()?;
        if streams.len() > 1
            && streams.iter().any(|s| s.channels != 1 || s.rate != streams[0].rate)
        {
            return Err(invalid("channel bundle members must be mono at one rate"));
        }
        let frames = streams.iter().map(|s| s.frames).min().expect("a sample has a part");
        let rate = streams[0].rate;
        let mut scratch = Vec::<f32>::new();
        Ok(SampleReader::custom(rate, frames as usize, move |start, out| {
            let start = start as u64;
            match streams.as_mut_slice() {
                [one] => {
                    let channels = one.channels;
                    scratch.resize(out.len() * channels, 0.0);
                    one.read(start, &mut scratch)?;
                    for (frame, s) in out.iter_mut().zip(scratch.chunks_exact(channels)) {
                        *frame = [s[0], s[channels.min(2) - 1]];
                    }
                }
                many => {
                    for (k, stream) in many.iter_mut().take(2).enumerate() {
                        scratch.resize(out.len(), 0.0);
                        stream.read(start, &mut scratch)?;
                        for (frame, &s) in out.iter_mut().zip(&scratch) {
                            frame[k] = s;
                        }
                    }
                    if many.len() == 1 {
                        out.iter_mut().for_each(|f| f[1] = f[0]);
                    }
                }
            }
            Ok(())
        }))
    }
}

/// The source of a loose file or bundle, checked by opening it once.
pub(crate) fn source(parts: Vec<Origin>) -> Result<Arc<dyn AssetSource>, String> {
    let sample = Sample { parts };
    sample.open().map_err(|e| e.to_string())?;
    Ok(Arc::new(sample))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(frames: usize, channels: u16, bad_extent: bool) -> Vec<u8> {
        let mut data = Vec::new();
        for i in 0..frames {
            for c in 0..channels {
                data.extend((((i * 37 + c as usize * 11) % 4000) as i16 - 2000).to_le_bytes());
            }
        }
        let mut out = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0".to_vec();
        out.extend(channels.to_le_bytes());
        out.extend(44100u32.to_le_bytes());
        out.extend((44100 * 2 * u32::from(channels)).to_le_bytes());
        out.extend((2 * channels).to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        if !bad_extent {
            let n = (out.len() - 8) as u32;
            out[4..8].copy_from_slice(&n.to_le_bytes());
        }
        out
    }

    #[test]
    fn streamed_reads_equal_a_full_decode_for_files_and_bundles() {
        let dir = std::env::temp_dir().join(format!("uvi-stream-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, bytes: &[u8]| {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            Origin::File(path)
        };
        let stereo = wav(50000, 2, true);
        let mono = [wav(40000, 1, false), wav(45000, 1, true)];
        let cases = [
            (vec![write("a.wav", &stereo)], vec![stereo.clone()]),
            (
                vec![write("l.wav", &mono[0]), write("r.wav", &mono[1])],
                mono.to_vec(),
            ),
            (vec![write("m.wav", &mono[0])], vec![mono[0].clone()]),
        ];
        for (parts, bytes) in cases {
            let (full, _) = crate::audio::decode(&bytes).unwrap();
            let mut reader = Sample { parts }.open().unwrap();
            assert_eq!((reader.frames(), reader.rate()), (full.frames.len(), 44100));
            // Forward, backward, within a window, across the skip threshold, and the end.
            let end = full.frames.len();
            for range in [0..3000, 3000..7000, 100..200, end - 1..end, 20000..30000, 0..end] {
                let mut out = vec![[0.0; 2]; range.len()];
                reader.read(range.start, &mut out).unwrap();
                assert_eq!(out, full.frames[range]);
            }
            assert!(reader.read(full.frames.len() - 1, &mut [[0.0; 2]; 2]).is_err());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_changed_or_truncated_under_a_stream_fails_cleanly() {
        let dir = std::env::temp_dir().join(format!("uvi-stream-change-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bytes = wav(50000, 2, false);
        for truncate in [false, true] {
            let path = dir.join(format!("{truncate}.wav"));
            std::fs::write(&path, &bytes).unwrap();
            let mut reader = Sample { parts: vec![Origin::File(path.clone())] }.open().unwrap();
            let mut out = vec![[0.0; 2]; 100];
            reader.read(0, &mut out).unwrap();
            let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            if truncate {
                file.set_len(bytes.len() as u64 / 2).unwrap();
            } else {
                // Same length, new modification time.
                file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5)).unwrap();
            }
            // Far enough ahead to need a fresh chunk of the file.
            assert!(reader.read(40000, &mut out).is_err(), "truncate {truncate}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
