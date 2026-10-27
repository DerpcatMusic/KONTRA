//! Where a preset's sample names live, and what their headers say. Lookups
//! never leave the library root: a sample outside it does not resolve.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use kontra_ir as ir;
use ni_file::nkr::Archive;

/// A sample's bytes: a whole file, or a member of a monolith or archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Location {
    pub file: PathBuf,
    pub offset: u64,
    pub size: u64,
    pub keystream: Option<ir::Keystream>,
}

pub(crate) struct Resolver {
    root: PathBuf,
    parent: PathBuf,
    /// Lower-case member path to its bytes, when the instrument is a monolith.
    monolith: HashMap<String, Location>,
    archives: HashMap<PathBuf, Option<Archive>>,
    /// Lower-case basename to files under `root`, built on the first miss.
    loose: Option<HashMap<String, Vec<PathBuf>>>,
    /// The library's keystream, read on the first encrypted member.
    key: Option<std::sync::Arc<[u8]>>,
}

impl Resolver {
    pub(crate) fn new(instrument: &Path) -> Result<Self, String> {
        let parent = instrument.parent().unwrap_or(Path::new(".")).to_path_buf();
        let root = instrument
            .ancestors()
            .find(|p| p.join("Samples").is_dir())
            .unwrap_or(&parent)
            .to_path_buf();
        let mut monolith = HashMap::new();
        let mut file = File::open(instrument).map_err(|e| e.to_string())?;
        let mut magic = [0; 16];
        if file.read_exact(&mut magic).is_ok() && &magic == b"/\\ NI FC MTD  /\\" {
            file.rewind().map_err(|e| e.to_string())?;
            let container = ni_file::file_container::NIFileContainer::read(&mut file)
                .map_err(|e| e.to_string())?;
            for item in container.items {
                let location = Location {
                    file: instrument.to_path_buf(),
                    offset: container.file_section_offset + item.file_start_offset,
                    size: item.file_size,
                    keystream: None,
                };
                monolith.insert(
                    item.filename.replace(['|', '\\'], "/").to_lowercase(),
                    location,
                );
            }
        }
        Ok(Self {
            root,
            parent,
            monolith,
            archives: HashMap::new(),
            loose: None,
            key: None,
        })
    }

    /// The bytes of the sample the preset names `name`, if the library has it.
    pub(crate) fn resolve(&mut self, name: &str) -> Result<Option<Location>, String> {
        let name = name.replace(['\\', '|'], "/");
        if let Some(found) = self.member(&name)? {
            return Ok(Some(found));
        }
        // Relative to the instrument folder, then each folder above it in the library.
        let bases: Vec<PathBuf> = self
            .parent
            .ancestors()
            .take_while(|a| a.starts_with(&self.root))
            .map(Path::to_path_buf)
            .collect();
        for base in &bases {
            let candidate = base.join(&name);
            if let Some((archive, member)) = archive_member(&candidate) {
                if let Some(found) = self.archive_entry(&archive, &member)? {
                    return Ok(Some(found));
                }
            } else if candidate.is_file() {
                return Ok(Some(whole(candidate)?));
            }
        }
        // A moved archive: find the archive by name, keep the member exact.
        if let Some((prefix, member)) = archive_reference(&name)
            && let Some(archive) = self.by_name(prefix)?
        {
            return self.archive_entry(&archive, member);
        }
        match self.by_name(&name)? {
            Some(file) => Ok(Some(whole(file)?)),
            None => Ok(None),
        }
    }

    /// A monolith member: the exact path, else a unique match of its basename.
    fn member(&self, name: &str) -> Result<Option<Location>, String> {
        let name = name.to_lowercase();
        if let Some(found) = self.monolith.get(&name) {
            return Ok(Some(found.clone()));
        }
        let base = name.rsplit('/').next().unwrap_or(&name);
        let suffix = format!("/{base}");
        let mut hits = self
            .monolith
            .iter()
            .filter(|(path, _)| *path == base || path.ends_with(&suffix));
        match (hits.next(), hits.next()) {
            (Some((_, found)), None) => Ok(Some(found.clone())),
            (Some(_), Some(_)) => Err(format!("several monolith members are named {base}")),
            _ => Ok(None),
        }
    }

    fn archive_entry(&mut self, archive: &Path, member: &str) -> Result<Option<Location>, String> {
        let index = match self.archives.get(archive) {
            Some(index) => index,
            None => {
                let index = File::open(archive)
                    .ok()
                    .and_then(|f| Archive::read_index(f).ok());
                self.archives.entry(archive.to_path_buf()).or_insert(index)
            }
        };
        let Some(index) = index else {
            return Ok(None);
        };
        let file = File::open(archive).map_err(|e| e.to_string())?;
        let Some(entry) = index.member(file, member).map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        if !entry.valid {
            return Err(format!("damaged archive member {member}"));
        }
        let keystream = match (entry.encoded, entry.key_index) {
            (false, _) | (true, 0xff) => None,
            (true, 0x100) => {
                let stream = match &self.key {
                    Some(key) => key.clone(),
                    None => self.key.insert(crate::access::keystream(archive)?).clone(),
                };
                Some(ir::Keystream {
                    stream,
                    origin: entry.offset,
                })
            }
            (true, other) => return Err(format!("archive cipher {other:#x}")),
        };
        Ok(Some(Location {
            file: archive.to_path_buf(),
            offset: entry.offset,
            size: entry.size,
            keystream,
        }))
    }

    /// A file under the root with this name: the only one, else the one whose
    /// folders match the most trailing components of `name`.
    fn by_name(&mut self, name: &str) -> Result<Option<PathBuf>, String> {
        let base = name.rsplit('/').next().unwrap_or(name).to_lowercase();
        let root = &self.root;
        let loose = self.loose.get_or_insert_with(|| {
            let mut index = HashMap::<String, Vec<PathBuf>>::new();
            walk(root, &mut |file| {
                let key = file.file_name().map(|n| n.to_string_lossy().to_lowercase());
                index.entry(key.unwrap_or_default()).or_default().push(file);
            });
            index
        });
        let Some(matches) = loose.get(&base) else {
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
            let suffix = parts
                .get(parts.len() - n..)
                .unwrap_or_default()
                .join("/")
                .to_lowercase();
            let mut found = matches
                .iter()
                .filter(|p| p.to_string_lossy().to_lowercase().ends_with(&suffix));
            if let (Some(only), None) = (found.next(), found.next()) {
                return Ok(Some(only.clone()));
            }
        }
        Err(format!("{} files are named {base}", matches.len()))
    }
}

fn whole(file: PathBuf) -> Result<Location, String> {
    let size = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
    Ok(Location {
        file,
        offset: 0,
        size,
        keystream: None,
    })
}

/// `…/x.nkx/member` split into the archive and its member, when x.nkx is a file.
fn archive_member(path: &Path) -> Option<(PathBuf, String)> {
    path.ancestors().skip(1).find_map(|parent| {
        let is_archive = parent
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nkx") || e.eq_ignore_ascii_case("nkr"));
        (is_archive && parent.is_file()).then(|| {
            let member = path
                .strip_prefix(parent)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            Some((parent.to_path_buf(), member))
        })?
    })
}

fn archive_reference(name: &str) -> Option<(&str, &str)> {
    name.match_indices('/').find_map(|(at, _)| {
        let (prefix, rest) = (name.get(..at)?, name.get(at + 1..)?);
        Path::new(prefix)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nkx") || e.eq_ignore_ascii_case("nkr"))
            .then_some((prefix, rest))
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

/// Reads the header at `at` and describes the sample: codec, layout and where its data is.
pub(crate) fn describe(name: String, at: &Location) -> Result<ir::Sample, String> {
    let mut file = File::open(&at.file).map_err(|e| e.to_string())?;
    let mut read = |pos: u64, out: &mut [u8]| -> Result<(), String> {
        if pos + out.len() as u64 > at.size {
            return Err("header past the end of the sample".into());
        }
        file.seek(SeekFrom::Start(at.offset + pos))
            .map_err(|e| e.to_string())?;
        file.read_exact(out).map_err(|e| e.to_string())?;
        if let Some(key) = &at.keystream {
            key.apply(at.offset + pos, out);
        }
        Ok(())
    };
    let mut magic = [0; 12];
    read(0, &mut magic)?;
    let sample = |codec, channels: u16, frames: u64, rate: u32, offset: u64, len: u64| {
        Ok(ir::Sample {
            name: name.clone(),
            file: at.file.clone(),
            offset: at.offset + offset,
            len,
            codec,
            channels: u8::try_from(channels).map_err(|_| format!("{channels} channels"))?,
            frames,
            rate,
            keystream: at.keystream.clone(),
        })
    };
    match magic.get(..4) {
        Some(b"RIFF") => {
            let mut pos = 12;
            let mut format = None;
            while pos + 8 <= at.size {
                let mut head = [0; 8];
                read(pos, &mut head)?;
                let (id, rest) = head.split_at(4);
                let len = u64::from(u32::from_le_bytes(rest.try_into().unwrap_or_default()));
                match id {
                    b"fmt " => {
                        let mut f = [0; 40];
                        let n = len.min(40) as usize;
                        read(pos + 8, f.get_mut(..n).unwrap_or_default())?;
                        let u16le = |i: usize| u16::from_le_bytes(at_n(&f, i));
                        let mut tag = u16le(0);
                        if tag == 0xFFFE && n >= 26 {
                            tag = u16le(24);
                        }
                        let rate = u32::from_le_bytes(at_n(&f, 4));
                        format = Some((tag, u16le(2), rate, u16le(14)));
                    }
                    b"data" => {
                        let (tag, channels, rate, bits) =
                            format.ok_or("WAV data before its format")?;
                        let bits = u8::try_from(bits).map_err(|_| "bit depth")?;
                        let float = match tag {
                            1 => false,
                            3 => true,
                            _ => return Err(format!("WAV format tag {tag}")),
                        };
                        let len = len.min(at.size - pos - 8);
                        let frame = u64::from(channels) * u64::from(bits / 8);
                        let codec = ir::Codec::Pcm {
                            bits,
                            float,
                            big_endian: false,
                        };
                        return sample(codec, channels, len / frame.max(1), rate, pos + 8, len);
                    }
                    _ => {}
                }
                pos += 8 + len + (len & 1);
            }
            Err("WAV without data".into())
        }
        Some(b"FORM") => {
            let aifc = magic.get(8..12) == Some(b"AIFC");
            let mut pos = 12;
            let mut comm = None;
            while pos + 8 <= at.size {
                let mut head = [0; 8];
                read(pos, &mut head)?;
                let (id, rest) = head.split_at(4);
                let len = u64::from(u32::from_be_bytes(rest.try_into().unwrap_or_default()));
                match id {
                    b"COMM" => {
                        let mut c = [0; 22];
                        read(
                            pos + 8,
                            c.get_mut(..if aifc { 22 } else { 18 }).unwrap_or_default(),
                        )?;
                        let channels = u16::from_be_bytes(at_n(&c, 0));
                        let frames = u32::from_be_bytes(at_n(&c, 2));
                        let bits = u16::from_be_bytes(at_n(&c, 6));
                        let exponent = i32::from(u16::from_be_bytes(at_n(&c, 8)) & 0x7FFF);
                        let mantissa = u64::from_be_bytes(at_n(&c, 10));
                        let rate =
                            (mantissa as f64 * 2f64.powi(exponent - 16_383 - 63)).round() as u32;
                        let kind = c.get(18..22).unwrap_or_default();
                        let (float, big_endian) = match kind {
                            _ if !aifc => (false, true),
                            b"NONE" | b"twos" => (false, true),
                            b"sowt" => (false, false),
                            b"fl32" | b"FL32" => (true, true),
                            _ => return Err(format!("AIFC {}", String::from_utf8_lossy(kind))),
                        };
                        comm = Some((channels, frames, bits, rate, float, big_endian));
                    }
                    b"SSND" => {
                        let (channels, frames, bits, rate, float, big_endian) =
                            comm.ok_or("AIFF sound before its format")?;
                        let mut o = [0; 4];
                        read(pos + 8, &mut o)?;
                        let skip = u64::from(u32::from_be_bytes(o));
                        let bits = u8::try_from(bits).map_err(|_| "bit depth")?;
                        let start = pos + 16 + skip;
                        let len = (len.saturating_sub(8 + skip)).min(at.size.saturating_sub(start));
                        let codec = ir::Codec::Pcm {
                            bits,
                            float,
                            big_endian,
                        };
                        return sample(codec, channels, u64::from(frames), rate, start, len);
                    }
                    _ => {}
                }
                pos += 8 + len + (len & 1);
            }
            Err("AIFF without sound data".into())
        }
        _ => {
            let mut head = [0; 120];
            let n = at.size.min(120) as usize;
            read(0, head.get_mut(..n).unwrap_or_default())?;
            let h = ncw::NcwHeader::read(&mut head.get(..n).unwrap_or_default())
                .map_err(|e| format!("not WAV, AIFF or NCW: {e}"))?;
            sample(
                ir::Codec::Ncw,
                h.channels,
                u64::from(h.num_samples),
                h.sample_rate,
                0,
                at.size,
            )
        }
    }
}

/// `N` bytes of `b` from `at`, zero where `b` is short.
fn at_n<const N: usize>(b: &[u8], at: usize) -> [u8; N] {
    b.get(at..at + N)
        .and_then(|s| s.try_into().ok())
        .unwrap_or([0; N])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_header_names_its_data_after_other_chunks() {
        let mut wav = b"RIFF\0\0\0\0WAVE".to_vec();
        wav.extend(b"LIST\x03\0\0\0abc\0");
        wav.extend(b"fmt \x10\0\0\0");
        wav.extend(1u16.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(44_100u32.to_le_bytes());
        wav.extend([0; 6]);
        wav.extend(24u16.to_le_bytes());
        wav.extend(b"data\x0c\0\0\0");
        wav.extend([0; 12]);
        let dir = std::env::temp_dir().join("kontra-kontakt-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.wav");
        std::fs::write(&path, &wav).unwrap();
        let at = Location {
            file: path,
            offset: 0,
            size: wav.len() as u64,
            keystream: None,
        };
        let s = describe("a".into(), &at).unwrap();
        assert_eq!(
            s.codec,
            ir::Codec::Pcm {
                bits: 24,
                float: false,
                big_endian: false
            }
        );
        assert_eq!(
            (s.channels, s.frames, s.rate, s.offset, s.len),
            (2, 2, 44_100, 56, 12)
        );
    }
}
