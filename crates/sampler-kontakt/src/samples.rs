//! Sample resolution and decoding for real libraries: loose WAV/NCW files and
//! members of NKX/NKR monoliths, decrypted with the library's own access data.
//! Decoded audio stays in memory; nothing is extracted or written.

use crate::LoadError;
use ni_file::{nis::LibraryKey, nkr::Archive};
use std::{
    collections::HashMap,
    ffi::OsString,
    fs::File,
    io::{self, Cursor, Read, Seek, SeekFrom},
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
    pub(crate) handle: Option<Arc<File>>,
    pub(crate) header: Option<(u32, usize)>,
}

/// Resolves and decodes the samples of one library, opening each archive's
/// directory and each library key once.
pub struct Samples {
    root: PathBuf,
    // Port from v1 0cb7a8a0:src/import.rs: one path walk per archive per load.
    canonical: HashMap<OsString, PathBuf>,
    is_file: HashMap<OsString, bool>,
    archives: HashMap<PathBuf, Arc<Archive>>,
    keys: HashMap<PathBuf, Arc<dyn LibraryKey>>,
    handles: HashMap<PathBuf, Arc<File>>,
    /// Numeric headers only; repeated zone trims need no additional disk reads.
    frame_counts: HashMap<PathBuf, u64>,
    /// Lower-case basename to loose files under `root`, built on first miss.
    loose: Option<HashMap<String, Vec<PathBuf>>>,
}

impl Samples {
    /// `root` bounds every lookup: a sample never resolves outside its library.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.into()),
            canonical: HashMap::new(),
            is_file: HashMap::new(),
            archives: HashMap::new(),
            keys: HashMap::new(),
            handles: HashMap::new(),
            frame_counts: HashMap::new(),
            loose: None,
        }
    }

    fn archive_member(&mut self, path: &Path) -> Option<(PathBuf, String)> {
        archive_member_where(path, |parent| {
            self.archives.contains_key(parent)
                || match self.is_file.get(parent.as_os_str()) {
                    Some(&is_file) => is_file,
                    None => *self
                        .is_file
                        .entry(parent.into())
                        .or_insert(parent.is_file()),
                }
        })
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
            if let Some((archive, member)) = self.archive_member(&candidate) {
                if !self.canonical.contains_key(archive.as_os_str()) {
                    self.canonical.insert(
                        archive.clone().into(),
                        archive
                            .canonicalize()
                            .map_err(|e| LoadError::io(&archive, e))?,
                    );
                }
                let archive = self.canonical[archive.as_os_str()].clone();
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
        let bytes = match self.archive_member(location) {
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
        let Some((archive, member)) = self.archive_member(location) else {
            let size = std::fs::metadata(location)
                .map_err(|e| LoadError::io(location, e))?
                .len();
            return Ok(Source {
                path: location.into(),
                offset: 0,
                size,
                key: None,
                handle: None,
                header: None,
            });
        };
        self.archive(&archive)?;
        let handle = self.handles[&archive].clone();
        let entry = self.archives[&archive]
            .member(
                FileAt {
                    file: &handle,
                    pos: 0,
                },
                &member,
            )
            .map_err(|e| LoadError::decode(&archive, "archive member header", e))?
            .filter(|e| e.valid)
            .ok_or_else(|| LoadError::Invalid {
                path: location.into(),
                reason: "invalid archive member".into(),
            })?;
        if entry.encoded && entry.key_index != 0xff && entry.key_index != 0x100 {
            return Err(LoadError::Invalid {
                path: location.into(),
                reason: "unsupported legacy NKX cipher".into(),
            });
        }
        let key = if entry.encoded && entry.key_index == 0x100 {
            if !self.keys.contains_key(&archive) {
                let key = crate::library_key(&archive).map_err(|reason| LoadError::Access {
                    path: archive.clone(),
                    reason,
                })?;
                self.keys.insert(archive.clone(), key);
            }
            self.keys.get(&archive).cloned()
        } else {
            None
        };
        Ok(Source {
            path: archive,
            offset: entry.offset,
            size: entry.size,
            key,
            handle: Some(handle),
            header: None,
        })
    }

    pub(crate) fn cached_source(
        &mut self,
        location: &Path,
        file: &Path,
        offset: u64,
        size: u64,
        keyed: bool,
        header: (u32, usize),
    ) -> Option<Source> {
        let holding = self
            .archive_member(location)
            .map_or_else(|| location.to_path_buf(), |(archive, _)| archive);
        if holding != file || !file.starts_with(&self.root) {
            return None;
        }
        let archive = holding != location;
        let handle = if archive {
            if !self.handles.contains_key(file) {
                self.handles
                    .insert(file.into(), Arc::new(File::open(file).ok()?));
            }
            Some(self.handles[file].clone())
        } else {
            None
        };
        let key = if keyed {
            if !archive {
                return None;
            }
            if !self.keys.contains_key(file) {
                self.keys
                    .insert(file.into(), crate::library_key(file).ok()?);
            }
            Some(self.keys[file].clone())
        } else {
            None
        };
        Some(Source {
            path: file.into(),
            offset,
            size,
            key,
            handle,
            header: Some(header),
        })
    }

    /// Resolve byte ranges concurrently using the already indexed archives.
    /// Results and errors retain the caller's asset order.
    pub(crate) fn sources(
        &self,
        locations: &[&Path],
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Vec<Source>, LoadError> {
        if locations.is_empty() {
            return Ok(Vec::new());
        }
        let workers = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(8);
        std::thread::scope(|scope| {
            let jobs: Vec<_> = locations
                .chunks(locations.len().div_ceil(workers))
                .map(|chunk| {
                    let mut samples = Self {
                        root: self.root.clone(),
                        canonical: HashMap::new(),
                        is_file: self.is_file.clone(),
                        archives: self.archives.clone(),
                        keys: self.keys.clone(),
                        handles: self.handles.clone(),
                        frame_counts: HashMap::new(),
                        loose: None,
                    };
                    scope.spawn(move || {
                        chunk
                            .iter()
                            .map(|location| {
                                if canceled() {
                                    return Err(LoadError::Canceled);
                                }
                                samples.source(location)
                            })
                            .collect::<Result<Vec<_>, _>>()
                    })
                })
                .collect();
            let mut sources = Vec::with_capacity(locations.len());
            for job in jobs {
                sources.extend(job.join().expect("sample source worker")?);
            }
            Ok(sources)
        })
    }

    /// Frame count of a sample, reading only its header: the first 64 KiB
    /// of a WAV, an AIFF metadata probe or the 120-byte NCW header.
    pub fn frames(&mut self, location: &Path) -> Result<u64, LoadError> {
        use std::io::{Read, Seek, SeekFrom};
        if let Some(&count) = self.frame_counts.get(location) {
            return Ok(count);
        }
        let mut head = Vec::new();
        match self.archive_member(location) {
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
                if entry.encoded
                    && entry.key_index != 0xff
                    && let Some(key) = key
                {
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
        let count = if head.starts_with(b"FORM") {
            let source = self.source(location)?;
            crate::SampleReader::open(&source).ok().map(|r| r.frames() as u64)
        } else { frames(&head) }.ok_or_else(|| LoadError::Invalid {
            path: location.into(),
            reason: "unreadable sample header".into(),
        })?;
        self.frame_counts.insert(location.into(), count);
        Ok(count)
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
            let mut file = File::open(path).map_err(|e| LoadError::io(path, e))?;
            let index = Archive::read_index(&mut file)
                .map_err(|e| LoadError::decode(path, "archive directory", e))?;
            self.handles.insert(path.into(), Arc::new(file));
            self.archives.insert(path.into(), Arc::new(index));
        }
        Ok(&self.archives[path])
    }
}

/// Private cursor over a shared archive handle. Positional reads keep workers
/// independent without reopening the archive or racing its seek position.
pub(crate) struct FileAt<'a> {
    pub(crate) file: &'a File,
    pub(crate) pos: u64,
}
impl Read for FileAt<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::read_at(self.file, buf, self.pos)?;
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(self.file, buf, self.pos)?;
        self.pos += n as u64;
        Ok(n)
    }
}
impl Seek for FileAt<'_> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.pos = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(n) => self.pos.checked_add_signed(n),
            SeekFrom::End(n) => self.file.metadata()?.len().checked_add_signed(n),
        }
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(self.pos)
    }
}

// Port from v1 0cb7a8a0:src/import.rs: format only an actual archive member.
fn archive_member_where(path: &Path, mut is_file: impl FnMut(&Path) -> bool) -> Option<(PathBuf, String)> {
    for parent in path.ancestors().skip(1) {
        if parent.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkx") || e.eq_ignore_ascii_case("nkr")) && is_file(parent) {
            return Some((parent.to_path_buf(), path.strip_prefix(parent).ok()?.to_string_lossy().replace('\\', "/")));
        }
    }
    None
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

/// WAV, AIFF or NCW bytes to stereo frames.
pub fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    if bytes.starts_with(b"FORM") {
        let mut reader = crate::pcm::Reader::open(Box::new(Cursor::new(bytes.to_vec()))).map_err(|e| format!("AIFF: {e:#}"))?;
        let len = usize::try_from(reader.frames).map_err(|_| "AIFF too long")?;
        let mut frames = vec![[0.; 2]; len];
        reader.read(0, &mut frames).map_err(|e| format!("AIFF: {e:#}"))?;
        return Ok(Decoded { rate: reader.rate, frames });
    }
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

    #[test]
    fn a_cached_archive_key_does_not_decrypt_clear_member_headers() {
        struct TestKey;
        impl LibraryKey for TestKey {
            fn apply_at(&self, _: u64, bytes: &mut [u8]) {
                for byte in bytes {
                    *byte ^= 0x55;
                }
            }
        }
        let names = ["clear.wav", "protected.wav"];
        let wav = wav_bytes(1, 1, 16, &[0, 0x40]);
        let mut bytes = 0x5e70ac54u32.to_le_bytes().to_vec();
        bytes.extend(0x110u16.to_le_bytes());
        bytes.extend([0; 8]);
        bytes.extend(2u32.to_le_bytes());
        bytes.extend([0; 4]);
        let mut offset = 22 + names.iter().map(|n| 8 + (n.len() + 1) * 2).sum::<usize>();
        for (index, name) in names.iter().enumerate() {
            bytes.extend(((8 + (name.len() + 1) * 2) as u16).to_le_bytes());
            bytes.extend((offset as u32).to_le_bytes());
            bytes.extend(0u16.to_le_bytes());
            for c in name.encode_utf16().chain([0]) {
                bytes.extend(c.to_le_bytes());
            }
            offset += if index == 0 { 22 } else { 31 } + wav.len();
        }
        for (magic, size, key) in [(0x2ae905fau32, 22, 0xffu32), (0x16ccf80a, 31, 0x100)] {
            let mut header = vec![0; size];
            header[..4].copy_from_slice(&magic.to_le_bytes());
            header[4..6].copy_from_slice(&0x110u16.to_le_bytes());
            header[10..14].copy_from_slice(&key.to_le_bytes());
            let at = if size == 22 { 14 } else { 19 };
            header[at..at + 4].copy_from_slice(&(wav.len() as u32).to_le_bytes());
            bytes.extend(header);
            let mut payload = wav.clone();
            if key == 0x100 {
                TestKey.apply(&mut payload);
            }
            bytes.extend(payload);
        }
        let root = std::env::temp_dir().join(format!("v2-mixed-nkx-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("authored.nkx");
        std::fs::write(&archive, bytes).unwrap();
        let mut samples = Samples::new(&root);
        samples.keys.insert(archive.clone(), Arc::new(TestKey));
        for name in names {
            assert_eq!(samples.frames(&archive.join(name)).unwrap(), 1);
            assert_eq!(
                samples.decode(&archive.join(name)).unwrap().frames,
                [[0.5, 0.5]]
            );
        }
        let locations: Vec<_> = names
            .into_iter()
            .rev()
            .map(|name| archive.join(name))
            .collect();
        let listed: Vec<_> = locations.iter().map(PathBuf::as_path).collect();
        let sources = samples.sources(&listed, &|| false).unwrap();
        assert!(Arc::ptr_eq(
            sources[0].handle.as_ref().unwrap(),
            sources[1].handle.as_ref().unwrap()
        ));
        std::thread::scope(|scope| {
            for source in sources {
                scope.spawn(move || {
                    for _ in 0..100 {
                        let mut reader = crate::SampleReader::open(&source).unwrap();
                        let mut frames = [[0.; 2]];
                        reader.read(0, &mut frames).unwrap();
                        assert_eq!(frames, [[0.5, 0.5]]);
                    }
                });
            }
        });
        assert!(matches!(
            samples.sources(&listed, &|| true),
            Err(LoadError::Canceled)
        ));
        let missing = root.join("absent.ncw");
        assert!(samples.sources(&[missing.as_path()], &|| false).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
