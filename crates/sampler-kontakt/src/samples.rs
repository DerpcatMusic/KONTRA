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

/// Resolves and decodes the samples of one library, opening each archive's
/// directory and each library key once.
pub struct Samples {
    root: PathBuf,
    archives: HashMap<PathBuf, Archive>,
    keys: HashMap<PathBuf, Arc<dyn LibraryKey>>,
    /// Numeric headers only; repeated zone trims need no additional disk reads.
    frame_counts: HashMap<PathBuf, u64>,
    /// Lower-case basename to loose files under `root`, built on first miss.
    loose: Option<HashMap<String, Vec<PathBuf>>>,
    #[cfg(feature = "library-access")]
    content_roots: Vec<PathBuf>,
}

impl Samples {
    /// `root` bounds every lookup: a sample never resolves outside its library.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.into()),
            archives: HashMap::new(),
            keys: HashMap::new(),
            frame_counts: HashMap::new(),
            loose: None,
            #[cfg(feature = "library-access")]
            content_roots: content_roots(),
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
        #[cfg(feature = "library-access")]
        if let Some(relative) = name.strip_prefix("Content/")
            && let Some(file) = content_file(&self.content_roots, relative).map_err(|reason| {
                LoadError::Invalid {
                    path: parent.join(&name),
                    reason,
                }
            })?
        {
            return Ok(Some(file));
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

    /// Frame count of a sample, reading only its header: the first 64 KiB
    /// of a WAV or the 120-byte NCW header.
    pub fn frames(&mut self, location: &Path) -> Result<u64, LoadError> {
        use std::io::{Read, Seek, SeekFrom};
        if let Some(&count) = self.frame_counts.get(location) {
            return Ok(count);
        }
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
        let count = frames(&head).ok_or_else(|| LoadError::Invalid {
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
            let file = File::open(path).map_err(|e| LoadError::io(path, e))?;
            let index = Archive::read_index(file)
                .map_err(|e| LoadError::decode(path, "archive directory", e))?;
            self.archives.insert(path.into(), index);
        }
        Ok(&self.archives[path])
    }
}

#[cfg(feature = "library-access")]
fn content_roots() -> Vec<PathBuf> {
    if let Some(paths) = std::env::var_os("KONTRA_KONTAKT_CONTENT") {
        return std::env::split_paths(&paths)
            .filter_map(|p| p.canonicalize().ok())
            .collect();
    }
    static ROOTS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    ROOTS
        .get_or_init(|| {
            let mut roots = Vec::new();
            let wine = std::env::var_os("WINEPREFIX")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".wine")));
            let mut bases = vec![PathBuf::from("/mnt/Windows11/Program Files")];
            if let Some(wine) = wine {
                bases.push(wine.join("drive_c/Program Files"));
            }
            for base in bases {
                for relative in ["Native Instruments", "Common Files/VST3"] {
                    // ponytail: bounded standard install discovery; use KONTRA_KONTAKT_CONTENT for custom installs.
                    find_content(&base.join(relative), 6, false, &mut 4096, &mut roots);
                }
            }
            roots.sort();
            roots.dedup();
            roots
        })
        .clone()
}

#[cfg(feature = "library-access")]
fn find_content(
    path: &Path,
    depth: usize,
    kontakt: bool,
    budget: &mut usize,
    roots: &mut Vec<PathBuf>,
) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if kontakt && name == "content" {
        if let Ok(root) = path.canonicalize() {
            roots.push(root);
        }
        return;
    }
    if depth == 0 {
        return;
    }
    let kontakt = kontakt
        || name == "kontakt"
        || name.starts_with("kontakt ")
        || name.starts_with("kontakt.");
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                find_content(&entry.path(), depth - 1, kontakt, budget, roots);
            }
        }
    }
}

#[cfg(feature = "library-access")]
fn content_file(roots: &[PathBuf], relative: &str) -> Result<Option<PathBuf>, String> {
    use std::io::Read;
    let relative = Path::new(relative);
    if !relative
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return Err("Invalid Kontakt player Content path".into());
    }
    let mut matches: Vec<_> = roots
        .iter()
        .filter_map(|root| {
            let file = root.join(relative).canonicalize().ok()?;
            (file.starts_with(root) && file.is_file()).then_some(file)
        })
        .collect();
    matches.sort();
    matches.dedup();
    let Some(first) = matches.first() else {
        return Ok(None);
    };
    // Multiple installed players/backups may provide the same built-in file.
    // Only byte-identical copies are interchangeable; cap comparison reads.
    if matches.len() > 1 {
        let read = |path: &Path| -> Result<Vec<u8>, String> {
            let mut bytes = Vec::new();
            File::open(path)
                .and_then(|f| f.take((32 << 20) + 1).read_to_end(&mut bytes))
                .map_err(|e| e.to_string())?;
            if bytes.len() > 32 << 20 {
                return Err("Kontakt Content comparison exceeds 32 MiB".into());
            }
            Ok(bytes)
        };
        let bytes = read(first)?;
        for file in matches.iter().skip(1) {
            if read(file)? != bytes {
                return Err(
                    "Ambiguous Kontakt player Content file; set KONTRA_KONTAKT_CONTENT".into(),
                );
            }
        }
    }
    Ok(Some(first.clone()))
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
    let float = reader.sample_format == ncw::SampleFormat::Float;
    let samples = reader.decode_samples().map_err(|e| format!("NCW: {e}"))?;
    let scale = 2f32.powi(i32::from(bits) - 1);
    let convert = |s: i32| {
        if float {
            Some(f32::from_bits(s as u32))
                .filter(|x| x.is_finite())
                .unwrap_or(0.0)
        } else {
            s as f32 / scale
        }
    };
    Ok(Decoded {
        rate,
        frames: samples
            .chunks_exact(channels)
            .map(|f| [convert(f[0]), convert(f[channels.min(2) - 1])])
            .collect(),
    })
}

fn wav(bytes: &[u8]) -> Result<Decoded, String> {
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
        let body = at + 8..(at + 8 + len as usize).min(bytes.len());
        match id {
            b"fmt " => format = Some(body.start),
            b"data" => data = Some(body.clone()),
            _ => {}
        }
        at = body.start + len as usize + (len as usize & 1);
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
    let sample = |s: &[u8]| match (tag, width) {
        (3, _) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
        (_, 1) => (f32::from(s[0]) - 128.0) / 128.0,
        _ => {
            let mut word = [0u8; 4];
            word[4 - width..].copy_from_slice(s);
            i32::from_le_bytes(word) as f32 / 2_147_483_648.0
        }
    };
    let frames = bytes[data]
        .chunks_exact(width * channels)
        .map(|frame| {
            [
                sample(&frame[..width]),
                sample(&frame[(channels.min(2) - 1) * width..][..width]),
            ]
        })
        .collect();
    Ok(Decoded { rate, frames })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "library-access")]
    #[test]
    fn player_content_is_bounded_and_duplicate_installs_must_agree() {
        let root = std::env::temp_dir().join(format!("v2-player-content-{}", std::process::id()));
        let library = root.join("library");
        let roots = [
            root.join("Kontakt Authored A/Content"),
            root.join("Kontakt Authored B/Content"),
        ];
        std::fs::create_dir_all(&library).unwrap();
        let relative = "Tools/Authored/tone.wav";
        let wav = wav_bytes(1, 1, 16, &[0, 0x40]);
        for content in &roots {
            std::fs::create_dir_all(content.join("Tools/Authored")).unwrap();
            std::fs::write(content.join(relative), &wav).unwrap();
        }
        let mut samples = Samples::new(&library);
        samples.content_roots.clear();
        find_content(&root, 6, false, &mut 32, &mut samples.content_roots);
        assert_eq!(samples.content_roots.len(), 2);
        let file = samples
            .resolve(&library, &format!("Content/{relative}"))
            .unwrap()
            .unwrap();
        assert!(!file.starts_with(&library));
        assert_eq!(samples.decode(&file).unwrap().frames, [[0.5, 0.5]]);
        assert!(content_file(&samples.content_roots, "../../escape.wav").is_err());
        std::fs::write(roots[1].join(relative), b"different authored data").unwrap();
        assert!(
            samples
                .resolve(&library, &format!("Content/{relative}"))
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

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
        std::fs::remove_dir_all(root).unwrap();
    }
}
