//! Sample resolution and decoding for real libraries: loose WAV/NCW files and
//! members of NKX/NKR monoliths, decrypted with the library's own access data.
//! Decoded audio stays in memory; nothing is extracted or written.

use crate::LoadError;
use ni_file::{nis::LibraryKey, nkr::Archive};
use std::{
    collections::HashMap,
    fs::File,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Decoded audio: stereo frames (mono is duplicated) at the sample's own rate.
pub struct Decoded {
    pub rate: u32,
    pub frames: Vec<[f32; 2]>,
}

/// A sample's bytes: a whole file, or an archive member and the keystream it
/// is encrypted under.
#[derive(Clone)]
pub struct Source {
    pub(crate) path: PathBuf,
    pub(crate) offset: u64,
    pub(crate) size: u64,
    pub(crate) key: Option<Arc<dyn LibraryKey>>,
}

/// Resolves and decodes the samples of one library, opening each archive's
/// directory and each library key once.
pub struct Samples {
    root: PathBuf,
    archives: HashMap<PathBuf, Archive>,
    keys: HashMap<PathBuf, Arc<dyn LibraryKey>>,
    /// Lower-case basename to loose files under `root`, built on first miss.
    loose: Option<HashMap<String, Vec<PathBuf>>>,
}

impl Samples {
    /// `root` bounds every lookup: a sample never resolves outside its library.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.into()),
            archives: HashMap::new(),
            keys: HashMap::new(),
            loose: None,
        }
    }

    /// Where the sample `name`, as the instrument at `parent` authored it, lives:
    /// a file, or `<archive>/<member>` for an archive member. `None` when absent.
    pub fn resolve(&mut self, parent: &Path, name: &str) -> Result<Option<PathBuf>, LoadError> {
        let name = name.replace('\\', "/");
        // Kontakt resolves relative names from the instrument folder, and
        // monolith paths also from the library folders above it.
        let mut bases = vec![parent.to_path_buf()];
        bases.extend(
            parent
                .ancestors()
                .skip(1)
                .take_while(|at| at.starts_with(&self.root))
                .map(Path::to_path_buf),
        );
        for base in &bases {
            let candidate = base.join(&name);
            if let Some((archive, member)) = archive_member(&candidate) {
                let archive = archive
                    .canonicalize()
                    .map_err(|e| LoadError::io(&archive, e))?;
                if archive.starts_with(&self.root)
                    && self.archive(&archive)?.find(&member).is_some()
                {
                    return Ok(Some(archive.join(member)));
                }
            } else if candidate.is_file() {
                let file = candidate
                    .canonicalize()
                    .map_err(|e| LoadError::io(&candidate, e))?;
                if file.starts_with(&self.root) {
                    return Ok(Some(file));
                }
            }
        }
        // Moved loose samples: match by name, preferring the longest unique
        // directory suffix; never silently pick one of several duplicates.
        let basename = name.rsplit('/').next().unwrap_or(&name).to_lowercase();
        let root = self.root.clone();
        let loose = self.loose.get_or_insert_with(|| {
            let mut index = HashMap::<String, Vec<PathBuf>>::new();
            walk(&root, &mut |file| {
                let key = file.file_name().map(|n| n.to_string_lossy().to_lowercase());
                index.entry(key.unwrap_or_default()).or_default().push(file);
            });
            index
        });
        let Some(matches) = loose.get(&basename) else {
            return Ok(None);
        };
        if let [only] = matches.as_slice() {
            return Ok(Some(only.clone()));
        }
        let parts: Vec<_> = name
            .split('/')
            .filter(|s| !s.is_empty() && *s != "..")
            .collect();
        for n in (2..=parts.len()).rev() {
            let suffix = parts[parts.len() - n..].join("/").to_lowercase();
            let found: Vec<_> = matches
                .iter()
                .filter(|p| p.to_string_lossy().to_lowercase().ends_with(&suffix))
                .collect();
            if let [only] = found.as_slice() {
                return Ok(Some((*only).clone()));
            }
        }
        Err(LoadError::Invalid {
            path: parent.join(name),
            reason: format!("{} files share this sample's name", matches.len()),
        })
    }

    /// Decode a sample [`Samples::resolve`] returned.
    pub fn decode(&mut self, location: &Path) -> Result<Decoded, LoadError> {
        let bytes = match archive_member(location) {
            Some((archive, member)) => {
                let key = match self.keys.get(&archive) {
                    Some(key) => Some(key.clone()),
                    None => self.encrypted(&archive, &member)?,
                };
                let file = File::open(&archive).map_err(|e| LoadError::io(&archive, e))?;
                self.archive(&archive)?
                    .read_entry_with_key(file, &member, key.as_deref())
                    .map_err(|e| LoadError::decode(location, "archive member", e))?
            }
            None => std::fs::read(location).map_err(|e| LoadError::io(location, e))?,
        };
        decode(&bytes).map_err(|reason| LoadError::Invalid {
            path: location.into(),
            reason,
        })
    }

    /// Where a resolved sample's bytes live, for random-access streaming.
    pub fn source(&mut self, location: &Path) -> Result<Source, LoadError> {
        let Some((archive, member)) = archive_member(location) else {
            let size = std::fs::metadata(location)
                .map_err(|e| LoadError::io(location, e))?
                .len();
            return Ok(Source {
                path: location.into(),
                offset: 0,
                size,
                key: None,
            });
        };
        let key = match self.keys.get(&archive) {
            Some(key) => Some(key.clone()),
            None => self.encrypted(&archive, &member)?,
        };
        let file = File::open(&archive).map_err(|e| LoadError::io(&archive, e))?;
        let entry = self
            .archive(&archive)?
            .member(file, &member)
            .map_err(|e| LoadError::decode(&archive, "archive member header", e))?
            .filter(|e| e.valid)
            .ok_or_else(|| LoadError::Invalid {
                path: location.into(),
                reason: "invalid archive member".into(),
            })?;
        let key = key.filter(|_| entry.encoded && entry.key_index != 0xff);
        if entry.encoded && entry.key_index != 0xff && entry.key_index != 0x100 {
            return Err(LoadError::Invalid {
                path: location.into(),
                reason: "unsupported legacy NKX cipher".into(),
            });
        }
        Ok(Source {
            path: archive,
            offset: entry.offset,
            size: entry.size,
            key,
        })
    }

    /// Frame count of a sample, reading only its header: the first 64 KiB
    /// of a WAV or the 120-byte NCW header.
    pub fn frames(&mut self, location: &Path) -> Result<u64, LoadError> {
        use std::io::{Read, Seek, SeekFrom};
        let mut head = Vec::new();
        match archive_member(location) {
            Some((archive, member)) => {
                let key = match self.keys.get(&archive) {
                    Some(key) => Some(key.clone()),
                    None => self.encrypted(&archive, &member)?,
                };
                let mut file = File::open(&archive).map_err(|e| LoadError::io(&archive, e))?;
                let entry = self
                    .archive(&archive)?
                    .member(&mut file, &member)
                    .map_err(|e| LoadError::decode(&archive, "archive member header", e))?;
                let entry = entry
                    .filter(|e| e.valid)
                    .ok_or_else(|| LoadError::Invalid {
                        path: location.into(),
                        reason: "invalid archive member".into(),
                    })?;
                file.seek(SeekFrom::Start(entry.offset))
                    .and_then(|_| file.take(entry.size.min(1 << 16)).read_to_end(&mut head))
                    .map_err(|e| LoadError::io(&archive, e))?;
                if let Some(key) = key {
                    key.apply(&mut head);
                }
            }
            None => {
                let file = File::open(location).map_err(|e| LoadError::io(location, e))?;
                file.take(1 << 16)
                    .read_to_end(&mut head)
                    .map_err(|e| LoadError::io(location, e))?;
            }
        }
        frames(&head).ok_or_else(|| LoadError::Invalid {
            path: location.into(),
            reason: "unreadable sample header".into(),
        })
    }

    /// The archive key `member` needs, if it is encrypted.
    fn encrypted(
        &mut self,
        archive: &Path,
        member: &str,
    ) -> Result<Option<Arc<dyn LibraryKey>>, LoadError> {
        let file = File::open(archive).map_err(|e| LoadError::io(archive, e))?;
        let entry = self
            .archive(archive)?
            .member(file, member)
            .map_err(|e| LoadError::decode(archive, "archive member header", e))?;
        if !entry.is_some_and(|e| e.encoded && e.key_index != 0xff) {
            return Ok(None);
        }
        let key = crate::library_key(archive).map_err(|reason| LoadError::Access {
            path: archive.into(),
            reason,
        })?;
        self.keys.insert(archive.into(), key.clone());
        Ok(Some(key))
    }

    fn archive(&mut self, path: &Path) -> Result<&Archive, LoadError> {
        if !self.archives.contains_key(path) {
            let file = File::open(path).map_err(|e| LoadError::io(path, e))?;
            let index = Archive::read_index(file)
                .map_err(|e| LoadError::decode(path, "archive directory", e))?;
            self.archives.insert(path.into(), index);
        }
        Ok(&self.archives[path])
    }
}

/// `(archive, member)` when an ancestor of `path` is an NKX/NKR file.
fn archive_member(path: &Path) -> Option<(PathBuf, String)> {
    path.ancestors().skip(1).find_map(|parent| {
        let archive = parent
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nkx") || e.eq_ignore_ascii_case("nkr"));
        let member = path
            .strip_prefix(parent)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");
        (archive && parent.is_file()).then(|| (parent.into(), member))
    })
}

fn walk(dir: &Path, found: &mut impl FnMut(PathBuf)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(&entry.path(), found),
            Ok(kind) if kind.is_file() => found(entry.path()),
            _ => {}
        }
    }
}

/// Frame count from the start of a WAV or NCW file.
fn frames(head: &[u8]) -> Option<u64> {
    if !head.starts_with(b"RIFF") {
        return ncw::NcwHeader::read(&mut &head[..])
            .ok()
            .map(|h| u64::from(h.num_samples));
    }
    let u32le = |at: usize| {
        head.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let (mut align, mut at) = (None, 12);
    while let (Some(id), Some(len)) = (head.get(at..at + 4), u32le(at + 4)) {
        match id {
            b"fmt " => {
                align = head
                    .get(at + 20..at + 22)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
            }
            b"data" => return Some(u64::from(len) / u64::from(align.filter(|a| *a > 0)?)),
            _ => {}
        }
        at += 8 + len as usize + (len as usize & 1);
    }
    None
}

/// WAV (integer 8/16/24/32-bit or float 32-bit) or NCW bytes to stereo frames.
pub fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    if bytes.starts_with(b"RIFF") {
        return wav(bytes);
    }
    let mut reader =
        ncw::NcwReader::read(Cursor::new(bytes)).map_err(|e| format!("not WAV or NCW: {e}"))?;
    let (channels, bits, rate) = (
        reader.header.channels as usize,
        reader.header.bits_per_sample,
        reader.header.sample_rate,
    );
    let convert = ncw_sample(reader.sample_format, bits);
    let samples = reader.decode_samples().map_err(|e| format!("NCW: {e}"))?;
    Ok(Decoded {
        rate,
        frames: samples
            .chunks_exact(channels)
            .map(|f| [convert(f[0]), convert(f[channels.min(2) - 1])])
            .collect(),
    })
}

/// NCW integer or float-bit samples to f32.
pub(crate) fn ncw_sample(format: ncw::SampleFormat, bits: u16) -> impl Fn(i32) -> f32 {
    let float = format == ncw::SampleFormat::Float;
    let scale = 2f32.powi(i32::from(bits) - 1);
    move |s: i32| {
        if float {
            Some(f32::from_bits(s as u32))
                .filter(|x| x.is_finite())
                .unwrap_or(0.0)
        } else {
            s as f32 / scale
        }
    }
}

/// A WAV's sample layout: `data` is the declared data chunk, which may extend
/// past the bytes parsed.
pub(crate) struct Wav {
    pub rate: u32,
    pub channels: usize,
    pub width: usize,
    tag: u16,
    pub data: std::ops::Range<usize>,
}

impl Wav {
    /// One interleaved frame's bytes to a stereo frame (mono duplicated).
    pub fn frame(&self, frame: &[u8]) -> [f32; 2] {
        let width = self.width;
        let sample = |s: &[u8]| match (self.tag, width) {
            (3, _) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
            (_, 1) => (f32::from(s[0]) - 128.0) / 128.0,
            _ => {
                let mut word = [0u8; 4];
                word[4 - width..].copy_from_slice(s);
                i32::from_le_bytes(word) as f32 / 2_147_483_648.0
            }
        };
        [
            sample(&frame[..width]),
            sample(&frame[(self.channels.min(2) - 1) * width..][..width]),
        ]
    }
}

fn wav(bytes: &[u8]) -> Result<Decoded, String> {
    let layout = wav_layout(bytes)?;
    let data = layout.data.start.min(bytes.len())..layout.data.end.min(bytes.len());
    let frames = bytes[data]
        .chunks_exact(layout.width * layout.channels)
        .map(|frame| layout.frame(frame))
        .collect();
    Ok(Decoded {
        rate: layout.rate,
        frames,
    })
}

pub(crate) fn wav_layout(bytes: &[u8]) -> Result<Wav, String> {
    let u16le = |at: usize| {
        bytes
            .get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32le = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if bytes.get(8..12) != Some(b"WAVE") {
        return Err("RIFF without WAVE".into());
    }
    let (mut format, mut data, mut at) = (None, None, 12);
    while let (Some(id), Some(len)) = (bytes.get(at..at + 4), u32le(at + 4)) {
        let body = at + 8..at + 8 + len as usize;
        match id {
            b"fmt " => format = Some(body.start),
            b"data" => {
                data = Some(body.clone());
                break; // Samples follow; a streamed parse has only the head.
            }
            _ => {}
        }
        at = body.end + (len as usize & 1);
    }
    let fmt = format.ok_or("WAV without fmt")?;
    let data = data.ok_or("WAV without data")?;
    let field = |offset| u16le(fmt + offset).ok_or("truncated WAV fmt");
    let mut tag = field(0)?;
    let channels = usize::from(field(2)?);
    let rate = u32le(fmt + 4).ok_or("truncated WAV fmt")?;
    let bits = usize::from(field(14)?);
    if tag == 0xfffe {
        tag = field(24)?; // Extensible: the subformat GUID starts with the tag.
    }
    let width = bits.div_ceil(8);
    if channels == 0 || !matches!((tag, width), (1, 1..=4) | (3, 4)) {
        return Err(format!(
            "unsupported WAV format {tag} with {bits}-bit samples"
        ));
    }
    Ok(Wav {
        rate,
        channels,
        width,
        tag,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_bytes(tag: u16, channels: u16, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut out = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0".to_vec();
        out.extend(tag.to_le_bytes());
        out.extend(channels.to_le_bytes());
        out.extend(44100u32.to_le_bytes());
        let align = channels * bits.div_ceil(8);
        out.extend((44100 * u32::from(align)).to_le_bytes());
        out.extend(align.to_le_bytes());
        out.extend(bits.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    #[test]
    fn wav_integer_widths_and_mono_decode_to_stereo() {
        let decoded = wav(&wav_bytes(1, 1, 24, &[0, 0, 0x40, 0, 0, 0xc0])).unwrap();
        assert_eq!(decoded.rate, 44100);
        assert_eq!(decoded.frames, [[0.5, 0.5], [-0.5, -0.5]]);
        let decoded = wav(&wav_bytes(1, 2, 16, &[0, 0x40, 0, 0xc0])).unwrap();
        assert_eq!(decoded.frames, [[0.5, -0.5]]);
        let decoded = wav(&wav_bytes(3, 1, 32, &0.25f32.to_le_bytes())).unwrap();
        assert_eq!(decoded.frames, [[0.25, 0.25]]);
        assert!(wav(&wav_bytes(2, 1, 4, &[0])).is_err(), "ADPCM is refused");
    }

    #[test]
    fn ncw_round_trips_through_the_vendored_codec() {
        let pcm: Vec<i32> = (0..1000).map(|i| (i * 37 % 2000) - 1000).collect();
        let spec = ncw::PcmSpec {
            channels: 2,
            bits_per_sample: 16,
            sample_rate: 48000,
        };
        let bytes = ncw::encode_pcm(&pcm, spec, ncw::StereoMode::Direct).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.rate, 48000);
        assert_eq!(decoded.frames.len(), 500);
        assert_eq!(frames(&bytes[..120]), Some(500));
        assert_eq!(frames(&wav_bytes(1, 2, 24, &[0; 12])), Some(2));
        assert_eq!(
            decoded.frames[1],
            [pcm[2] as f32 / 32768.0, pcm[3] as f32 / 32768.0]
        );
    }
}
