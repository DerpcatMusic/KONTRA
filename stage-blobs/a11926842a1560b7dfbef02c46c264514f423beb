//! Bounded seekable WAV boundary: PCM16 / IEEE float32, mono/stereo, RIFF only.
//! Original implementation from format fields, not copied codec code. See NATIVE_ENTRY.md.
use sampler_core::{Frame, PAGE_FRAMES, Pcm};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(b[i..i + 2].try_into().unwrap())
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

pub fn read(path: &Path) -> io::Result<Pcm> {
    let file = std::fs::File::open(path)?;
    let source = Source::open(file)?;
    if source.file_bytes > MAX_FILE_BYTES {
        return Err(invalid("resident WAV limit is 256 MiB"));
    }
    resident(source)
}

fn resident<R: Read + Seek>(mut source: Source<R>) -> io::Result<Pcm> {
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(source.asset().frame_count())
        .map_err(io::Error::other)?;
    frames.resize(source.asset().frame_count(), [0.; 2]);
    source.read_frames(0, &mut frames)?;
    Pcm::new(source.asset().sample_rate(), frames.into_boxed_slice())
        .map_err(|_| invalid("invalid PCM sample data"))
}

/// Worker/control-owned seekable source. Opening parses bounded metadata only;
/// reading a page never loads unrelated audio or allocates a second decode buffer.
/// The source bytes must remain immutable for the lifetime of this asset revision.
pub struct Source<R> {
    input: R,
    asset: Pcm,
    file_bytes: u64,
    data_offset: u64,
    channels: usize,
    width: usize,
}
impl<R: Read + Seek> Source<R> {
    pub fn open(mut input: R) -> io::Result<Self> {
        let file_bytes = input.seek(SeekFrom::End(0))?;
        input.seek(SeekFrom::Start(0))?;
        let mut header = [0; 12];
        input.read_exact(&mut header)?;
        if &header[..4] != b"RIFF"
            || &header[8..] != b"WAVE"
            || u64::from(u32le(&header, 4)) + 8 != file_bytes
        {
            return Err(invalid("expected a complete RIFF WAVE file"));
        }
        let mut fmt = None;
        let mut data = None;
        let mut at = 12;
        while at < file_bytes {
            if file_bytes - at < 8 {
                return Err(invalid("truncated chunk header"));
            }
            input.seek(SeekFrom::Start(at))?;
            let mut header = [0; 8];
            input.read_exact(&mut header)?;
            let begin = at + 8;
            let length = u64::from(u32le(&header, 4));
            let end = begin + length;
            at = end + (length & 1);
            if at > file_bytes {
                return Err(invalid("truncated chunk or missing padding"));
            }
            match &header[..4] {
                b"fmt " => {
                    if fmt.is_some() {
                        return Err(invalid("duplicate format chunk unsupported"));
                    }
                    if length != 16 && length != 18 {
                        return Err(invalid("extended WAV formats unsupported"));
                    }
                    let mut bytes = [0; 18];
                    input.read_exact(&mut bytes[..length as usize])?;
                    if length == 18 && u16le(&bytes, 16) != 0 {
                        return Err(invalid("extended WAV formats unsupported"));
                    }
                    fmt = Some(bytes);
                }
                b"data" if data.replace((begin, length)).is_some() => {
                    return Err(invalid("duplicate data chunk unsupported"));
                }
                _ => {}
            }
        }
        let fmt = fmt.ok_or_else(|| invalid("missing format chunk"))?;
        let (encoding, channels, rate, byte_rate, align, bits) = (
            u16le(&fmt, 0),
            u16le(&fmt, 2),
            u32le(&fmt, 4),
            u32le(&fmt, 8),
            u16le(&fmt, 12),
            u16le(&fmt, 14),
        );
        if !matches!((encoding, bits), (1, 16) | (3, 32))
            || !(1..=2).contains(&channels)
            || rate == 0
            || align != channels * (bits / 8)
            || rate.checked_mul(u32::from(align)) != Some(byte_rate)
        {
            return Err(invalid(
                "only valid mono/stereo PCM16 and float32 WAV are supported",
            ));
        }
        let (data_offset, data_bytes) = data.ok_or_else(|| invalid("missing data chunk"))?;
        if data_bytes == 0 || data_bytes % u64::from(align) != 0 {
            return Err(invalid("empty or partial audio frame"));
        }
        let frames = usize::try_from(data_bytes / u64::from(align))
            .map_err(|_| invalid("frame count exceeds platform capacity"))?;
        let asset = Pcm::streamed(rate, frames).map_err(|_| invalid("invalid PCM metadata"))?;
        Ok(Self {
            input,
            asset,
            file_bytes,
            data_offset,
            channels: usize::from(channels),
            width: usize::from(bits / 8),
        })
    }

    pub fn asset(&self) -> &Pcm {
        &self.asset
    }

    pub fn read_frames(&mut self, start: usize, output: &mut [Frame]) -> io::Result<()> {
        if start
            .checked_add(output.len())
            .is_none_or(|end| end > self.asset.frame_count())
        {
            return Err(invalid("decode range outside asset"));
        }
        if output.is_empty() {
            return Ok(());
        }
        let align = self.channels * self.width;
        self.input.seek(SeekFrom::Start(
            self.data_offset + start as u64 * align as u64,
        ))?;
        let mut scratch = [0; PAGE_FRAMES * 8];
        for output in output.chunks_mut(PAGE_FRAMES) {
            let bytes = &mut scratch[..output.len() * align];
            self.input.read_exact(bytes)?;
            for (frame, bytes) in output.iter_mut().zip(bytes.chunks_exact(align)) {
                let sample = |channel: usize| {
                    let offset = channel * self.width;
                    if self.width == 2 {
                        f32::from(i16::from_le_bytes(
                            bytes[offset..offset + 2].try_into().unwrap(),
                        )) / 32768.
                    } else {
                        f32::from_bits(u32le(bytes, offset))
                    }
                };
                let left = sample(0);
                let right = if self.channels == 1 { left } else { sample(1) };
                if !left.is_finite() || !right.is_finite() {
                    return Err(invalid("nonfinite PCM sample"));
                }
                *frame = [left, right];
            }
        }
        Ok(())
    }
}

#[cfg(test)]
fn decode(bytes: &[u8]) -> io::Result<Pcm> {
    resident(Source::open(io::Cursor::new(bytes))?)
}

/// Float32 stereo WAVEFORMATEX, with fact chunk. All IO stays outside rendering.
pub fn header(out: &mut impl Write, rate: u32, frames: usize) -> io::Result<()> {
    let frames = u32::try_from(frames).map_err(|_| invalid("too many frames for RIFF"))?;
    let bytes = frames
        .checked_mul(8)
        .ok_or_else(|| invalid("RIFF size overflow"))?;
    let riff = bytes
        .checked_add(50)
        .ok_or_else(|| invalid("RIFF size overflow"))?;
    let byte_rate = rate
        .checked_mul(8)
        .filter(|_| rate != 0)
        .ok_or_else(|| invalid("invalid output rate"))?;
    out.write_all(b"RIFF")?;
    out.write_all(&riff.to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&18u32.to_le_bytes())?;
    out.write_all(&3u16.to_le_bytes())?;
    out.write_all(&2u16.to_le_bytes())?;
    out.write_all(&rate.to_le_bytes())?;
    out.write_all(&byte_rate.to_le_bytes())?;
    out.write_all(&8u16.to_le_bytes())?;
    out.write_all(&32u16.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(b"fact")?;
    out.write_all(&4u32.to_le_bytes())?;
    out.write_all(&frames.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&bytes.to_le_bytes())
}

pub fn frames(out: &mut impl Write, frames: &[Frame]) -> io::Result<()> {
    for frame in frames {
        for sample in frame {
            out.write_all(&sample.to_le_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wave_roundtrip_and_malformed_boundaries() {
        let pcm = [[-0.5, 0.5], [1.0, -1.0], [0.0, f32::MIN_POSITIVE]];
        let mut bytes = Vec::new();
        header(&mut bytes, 48000, pcm.len()).unwrap();
        frames(&mut bytes, &pcm).unwrap();
        let result = decode(&bytes).unwrap();
        assert_eq!(result.sample_rate(), 48000);
        assert_eq!(result.resident_frames().unwrap(), &pcm);
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_err());
        }
        let mut bad = bytes.clone();
        bad[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bad).is_err());
        let mut bad = bytes.clone();
        bad[58..62].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode(&bad).is_err());
        let mut bad = bytes.clone();
        bad[24..28].copy_from_slice(&0u32.to_le_bytes());
        assert!(decode(&bad).is_err());
        // Authored canonical PCM16 mono, independent of the float writer.
        let mut mono = b"RIFF".to_vec();
        mono.extend(40u32.to_le_bytes());
        mono.extend(b"WAVEfmt ");
        mono.extend(16u32.to_le_bytes());
        mono.extend(1u16.to_le_bytes());
        mono.extend(1u16.to_le_bytes());
        mono.extend(44100u32.to_le_bytes());
        mono.extend(88200u32.to_le_bytes());
        mono.extend(2u16.to_le_bytes());
        mono.extend(16u16.to_le_bytes());
        mono.extend(b"data");
        mono.extend(4u32.to_le_bytes());
        mono.extend(i16::MIN.to_le_bytes());
        mono.extend(i16::MAX.to_le_bytes());
        assert_eq!(
            decode(&mono).unwrap().resident_frames().unwrap(),
            &[[-1.0; 2], [32767.0 / 32768.0; 2]]
        );
    }

    #[test]
    fn seekable_source_reads_only_requested_frames_and_rejects_partial_or_nonfinite_data() {
        struct Counted {
            input: io::Cursor<Vec<u8>>,
            read: usize,
        }
        impl Read for Counted {
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                let count = self.input.read(output)?;
                self.read += count;
                Ok(count)
            }
        }
        impl Seek for Counted {
            fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
                self.input.seek(position)
            }
        }
        let pcm: Vec<_> = (0..PAGE_FRAMES * 3 + 3)
            .map(|n| [n as f32, -(n as f32)])
            .collect();
        let mut bytes = Vec::new();
        header(&mut bytes, 48000, pcm.len()).unwrap();
        frames(&mut bytes, &pcm).unwrap();
        let mut source = Source::open(Counted {
            input: io::Cursor::new(bytes),
            read: 0,
        })
        .unwrap();
        assert_eq!(source.input.read, 54, "opening must not decode sample data");
        assert!(source.asset().resident_frames().is_none());
        let mut out = [[0.; 2]; 3];
        for at in [PAGE_FRAMES - 1, pcm.len() - 3, 0] {
            let before = source.input.read;
            source.read_frames(at, &mut out).unwrap();
            assert_eq!(out, pcm[at..at + 3]);
            assert_eq!(source.input.read - before, out.len() * 8);
        }
        assert!(source.read_frames(pcm.len() - 2, &mut out).is_err());
        assert!(source.read_frames(usize::MAX, &mut out).is_err());
        source.read_frames(pcm.len(), &mut []).unwrap();
        source.input.input.get_mut()[source.data_offset as usize..source.data_offset as usize + 4]
            .copy_from_slice(&f32::INFINITY.to_le_bytes());
        assert!(source.read_frames(0, &mut out).is_err());
        source
            .input
            .input
            .get_mut()
            .truncate(source.data_offset as usize);
        assert!(source.read_frames(PAGE_FRAMES, &mut out).is_err());
    }

    #[test]
    fn file_worker_decodes_owned_pages_and_native_render_matches_the_file() {
        use sampler_core::*;
        let data: Vec<_> = (0..PAGE_FRAMES * 2 + 13)
            .map(|n| [(n as f32 * 0.05).sin(), (n as f32 * 0.017).cos()])
            .collect();
        let path = std::env::temp_dir().join(format!(
            "sampler-wave-{}-{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        header(&mut file, 48000, data.len()).unwrap();
        frames(&mut file, &data).unwrap();
        drop(file);
        let mut source = Source::open(std::fs::File::open(&path).unwrap()).unwrap();
        let asset = source.asset().clone();
        let (mut cache, mut worker) = StreamCache::new(3).unwrap();
        for index in (0..3).rev() {
            cache.request(&asset, index, index as u64).unwrap();
        }
        std::thread::spawn(move || {
            for _ in 0..3 {
                let mut job = worker.next_job().unwrap();
                assert_eq!(job.key().asset, source.asset().asset_id());
                let start = job.range().start;
                let result = source
                    .read_frames(start, job.frames_mut())
                    .map_err(|_| DecodeFailure::Unavailable);
                worker.complete(job, result).unwrap();
            }
        })
        .join()
        .unwrap();
        for _ in 0..3 {
            assert!(matches!(cache.poll(), Some(PageUpdate::Loaded(_))));
        }
        let plan = super::super::prepare_sample(asset, false, false).unwrap();
        let mut rt = Runtime::new(
            plan,
            Limits {
                notes: 1,
                channels: 0,
                performances: 1,
                families: 1,
                voices: 1,
                expressions: 1,
                decisions: 0,
                commands: 1,
                behaviors: 0,
                behavior_fuel: 0,
                behavior_cells: 0,
                note_cells: 0,
            },
        )
        .unwrap()
        .with_stream_cache(cache);
        rt.trigger(
            Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(1),
            },
            60,
            1.,
        )
        .unwrap();
        let mut actual = vec![[0.; 2]; data.len() + 1];
        for chunk in actual.chunks_mut(257) {
            rt.render(chunk).unwrap();
        }
        assert_eq!(&actual[..data.len()], &data);
        assert_eq!(actual[data.len()], [0.; 2]);
        assert_eq!(rt.stream_underruns(), 0);
        assert_eq!(rt.voice_count(), 0);
        std::fs::remove_file(path).unwrap();
    }
}
