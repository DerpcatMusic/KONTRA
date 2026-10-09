//! Port from v1 0cb7a8a0:src/audio.rs PCM reader: accurate random access.
use anyhow::{Context, Result, bail, ensure};
use std::io;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{self, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
    probe::Hint,
};
type Frame = [f32; 2];
const SKIP_AHEAD: u64 = 16384;
pub(crate) struct Reader {
    pub rate: u32,
    pub frames: u64,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn codecs::Decoder>,
    track: u32,
    buffer: Vec<Frame>,
    start: u64,
}
impl Reader {
    pub fn open(mut source: Box<dyn MediaSource>) -> Result<Self> {
        // v1 creator::probe COMM prefix. Symphonia 0.5 counts SSND's eight
        // offset/block bytes as PCM frames; COMM is the authoritative count.
        use std::io::{Read, Seek, SeekFrom};
        let mut head = [0; 12];
        source.read_exact(&mut head)?;
        ensure!(
            &head[..4] == b"FORM" && matches!(&head[8..], b"AIFF" | b"AIFC"),
            "Not AIFF"
        );
        let end = (u64::from(u32::from_be_bytes(head[4..8].try_into().unwrap())) + 8)
            .min(source.byte_len().unwrap_or(u64::MAX));
        let mut at = 12u64;
        let mut declared = None;
        while at + 8 <= end {
            source.seek(SeekFrom::Start(at))?;
            let mut chunk = [0; 8];
            source.read_exact(&mut chunk)?;
            let size = u64::from(u32::from_be_bytes(chunk[4..].try_into().unwrap()));
            ensure!(at + 8 + size <= end, "Truncated AIFF chunk");
            if &chunk[..4] == b"COMM" && size >= 18 {
                let mut common = [0; 6];
                source.read_exact(&mut common)?;
                declared = Some(u64::from(u32::from_be_bytes(
                    common[2..].try_into().unwrap(),
                )));
                break;
            }
            at += 8 + size + (size & 1);
        }
        let frames = declared.context("AIFF has no COMM chunk")?;
        source.seek(SeekFrom::Start(0))?;
        let stream = MediaSourceStream::new(source, Default::default());
        let format = symphonia::default::get_probe()
            .format(
                &Hint::new().with_extension("aiff"),
                stream,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )?
            .format;
        let track = format.default_track().context("No audio track")?;
        let params = &track.codec_params;
        let rate = params.sample_rate.context("Sample rate is not declared")?;
        if let Some(channels) = params.channels {
            ensure!(
                (1..=2).contains(&channels.count()),
                "Only mono/stereo samples are supported"
            );
        }
        let decoder = symphonia::default::get_codecs().make(params, &DecoderOptions::default())?;
        let track = track.id;
        ensure!(rate > 0 && frames > 0, "Empty sample or invalid rate");
        Ok(Self {
            rate,
            frames,
            format,
            decoder,
            track,
            buffer: Vec::new(),
            start: 0,
        })
    }
}
impl Reader {
    fn end(&self) -> u64 {
        self.start + self.buffer.len() as u64
    }

    pub fn read(&mut self, mut start: u64, mut out: &mut [Frame]) -> Result<()> {
        if start < self.start || start > self.end() + SKIP_AHEAD {
            self.seek(start)?;
        }
        while !out.is_empty() {
            if start < self.end() && start >= self.start {
                let offset = (start - self.start) as usize;
                let n = (self.buffer.len() - offset).min(out.len());
                out[..n].copy_from_slice(&self.buffer[offset..offset + n]);
                out = &mut out[n..];
                start += n as u64;
            } else if start < self.start {
                bail!("Sample seek landed past frame {start}");
            } else if !self.decode_next()? {
                bail!("Sample ended before its declared length");
            }
        }
        Ok(())
    }

    fn seek(&mut self, frame: u64) -> Result<()> {
        let to = SeekTo::TimeStamp {
            ts: frame,
            track_id: self.track,
        };
        let seeked = self.format.seek(SeekMode::Accurate, to)?;
        self.decoder.reset();
        self.buffer.clear();
        self.start = seeked.actual_ts;
        Ok(())
    }

    /// Decode the next packet into the buffer; false at end of stream.
    fn decode_next(&mut self) -> Result<bool> {
        let packet = loop {
            match self.format.next_packet() {
                Ok(packet) if packet.track_id() == self.track => break packet,
                Ok(_) => continue,
                Err(SymphoniaError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                Err(e) => return Err(e.into()),
            }
        };
        let decoded = self.decoder.decode(&packet)?;
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        ensure!(
            (1..=2).contains(&channels),
            "Only mono/stereo samples are supported"
        );
        let mut pcm = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        pcm.copy_interleaved_ref(decoded);
        self.start = packet.ts();
        self.buffer.clear();
        self.buffer.extend(
            pcm.samples()
                .chunks_exact(channels)
                .map(|s| [s[0], s[channels - 1]]),
        );
        Ok(true)
    }
}
