//! A UFS bank opened for reading: its programs, its members and the samples
//! programs name. Keys come from the bank's own directory and the content
//! key recovered from its PNG members.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use kontra_ir as ir;
use uvi_file::access::Namespaces;
use uvi_file::ufs::{Directory, Member, Protection, Ufs};

use crate::header;

/// Member bytes read to find a sample's header. WAV headers can put the data
/// chunk well past the first kilobytes; this covers everything in the banks.
const HEADER_BYTES: u64 = 64 << 10;

/// UVI keys member bytes in 512-byte blocks (uvi-file's `transform_blocks`).
const BLOCK: u32 = 512;

pub(crate) struct Bank {
    path: PathBuf,
    ufs: Ufs,
    directory: Directory,
    content_key: Option<u64>,
    /// Lower-case member path to its index in the directory.
    by_path: HashMap<String, usize>,
    /// Lower-case member name to its indices in the directory.
    by_name: HashMap<String, Vec<usize>>,
}

impl Bank {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let ufs = Ufs::open(path)?;
        let directory = ufs.decode_directory(&Namespaces::native().metadata)?;
        let content_key = directory
            .files
            .iter()
            .any(|m| m.mode == Protection::Content)
            .then(|| crate::keys::content_key(path, &ufs, &directory))
            .transpose()?;
        let (mut by_path, mut by_name) = (HashMap::new(), HashMap::<String, Vec<usize>>::new());
        for (index, member) in directory.files.iter().enumerate() {
            if let Some(p) = &member.path {
                by_path.entry(p.to_lowercase()).or_insert(index);
            }
            by_name
                .entry(member.name.to_lowercase())
                .or_default()
                .push(index);
        }
        Ok(Self {
            path: path.to_path_buf(),
            ufs,
            directory,
            content_key,
            by_path,
            by_name,
        })
    }

    /// The program at `member` (a path inside the bank; empty: the first
    /// program), as the path it has in the bank and its clear XML.
    pub(crate) fn program_text(&self, member: &str) -> Result<(String, String)> {
        let found = if member.is_empty() {
            self.programs().into_iter().next()
        } else {
            self.lookup(member)
        };
        let member = self
            .directory
            .files
            .get(found.context("no such program in this bank")?)
            .context("member index")?;
        let bytes = self.read(member)?;
        let text = uvi_file::crypto::decode_program_bytes(&bytes, &Namespaces::native().program)?;
        Ok((member.path.clone().unwrap_or_default(), text))
    }

    /// The program paths of the bank at `path`, sorted, from its directory
    /// alone: no content key is recovered, so listing a protected bank costs
    /// what listing a clear one does.
    pub(crate) fn program_paths(path: &Path) -> Result<Vec<String>> {
        let directory = Ufs::open(path)?.decode_directory(&Namespaces::native().metadata)?;
        Ok(programs(&directory)
            .into_iter()
            .filter_map(|i| directory.files.get(i)?.path.clone())
            .collect())
    }

    fn programs(&self) -> Vec<usize> {
        programs(&self.directory)
    }

    fn lookup(&self, path: &str) -> Option<usize> {
        self.by_path.get(&normalize(path).to_lowercase()).copied()
    }

    fn read(&self, member: &Member) -> Result<Vec<u8>> {
        self.ufs
            .read_member(member, self.directory.metadata_key, self.content_key)
    }

    /// The sample a program names, relative to the program's folder: exact
    /// member path first, then the member name if it is unique, then the
    /// same stem in another audio format (banks keep WAV names over FLAC).
    fn resolve(&self, program_dir: &str, reference: &str) -> Option<usize> {
        let reference = reference.replace('\\', "/");
        let roots = [
            format!("{program_dir}/{reference}"),
            reference.clone(),
            format!("Samples/{reference}"),
        ];
        if let Some(index) = roots.iter().find_map(|p| self.lookup(p)) {
            return Some(index);
        }
        let name = normalize(&reference);
        let base = name.rsplit('/').next().unwrap_or_default().to_lowercase();
        if let Some([only]) = self.by_name.get(&base).map(Vec::as_slice) {
            return Some(*only);
        }
        let (stem, _) = name.rsplit_once('.')?;
        ["wav", "aif", "aiff", "flac"]
            .iter()
            .find_map(|format| self.lookup(&format!("{stem}.{format}")))
    }
}

impl Bank {
    /// The member a script's picture path names: `./` and `../` lead up
    /// from the script's folder, so the path below them first, then the
    /// member name if it is unique.
    pub(crate) fn resource(&self, path: &str) -> Option<Vec<u8>> {
        let path = path.replace('\\', "/");
        let below = path.trim_start_matches(['.', '/']);
        let found = self.lookup(below).or_else(|| {
            let name = below.rsplit('/').next()?.to_lowercase();
            match self.by_name.get(&name).map(Vec::as_slice) {
                Some([only]) => Some(*only),
                _ => None,
            }
        })?;
        self.read(self.directory.files.get(found)?).ok()
    }
}

impl Bank {
    /// The bank's cover: its own cover or splash picture, else its
    /// interface's background, else its largest still picture (not a
    /// control strip); the larger variant (`@2x`) first.
    pub(crate) fn cover(&self) -> Option<Vec<u8>> {
        let mut pictures: Vec<(usize, &Member)> = (self.directory.files.iter())
            .filter_map(|m| Some((picture_rank(m.path.as_deref()?)?, m)))
            .collect();
        pictures.sort_by_key(|(rank, m)| (*rank, std::cmp::Reverse(m.size)));
        let (named, rest): (Vec<_>, Vec<_>) = pictures.into_iter().partition(|(r, _)| *r < STILL);
        let named = named.into_iter().find_map(|(_, m)| self.read(m).ok());
        // ponytail: reads the largest few whole to see their size; a header
        // read if a bank's pictures are huge.
        named.or_else(|| {
            (rest.into_iter().take(8))
                .filter_map(|(_, m)| self.read(m).ok())
                .find(|b| png_size(b).is_some_and(|(w, h)| w >= 300 && w * 3 >= h && h * 4 >= w))
        })
    }
}

/// Where a picture member stands as a bank's cover, best first; `STILL`
/// and after are unnamed pictures, tried by their shape.
const STILL: usize = 4;

fn picture_rank(path: &str) -> Option<usize> {
    let file = path.rsplit('/').next()?.to_ascii_lowercase();
    let (stem, ext) = file.rsplit_once('.')?;
    if !matches!(ext, "png" | "jpg" | "jpeg") {
        return None;
    }
    let (stem, big) = match stem.strip_suffix("@2x") {
        Some(stem) => (stem, 0),
        None => (stem, 1),
    };
    let named = if stem.contains("cover") || stem.contains("splash") {
        0
    } else if stem.starts_with("back") || stem.contains("background") || stem == "bg" {
        2
    } else {
        STILL
    };
    Some(named + big * usize::from(named < STILL))
}

/// A PNG's width and height from its header.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let be = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    (bytes.get(..8) == Some(b"\x89PNG\r\n\x1a\n")).then_some(())?;
    Some((be(16)?, be(20)?))
}

impl crate::program::Library for Bank {
    fn modules(&self) -> Vec<(String, Vec<u8>)> {
        self.directory
            .files
            .iter()
            .filter_map(|m| Some((m.path.clone()?, m)))
            .filter(|(path, _)| path.to_ascii_lowercase().ends_with(".lua"))
            .filter_map(|(path, m)| Some((path, self.read(m).ok()?)))
            .collect()
    }

    /// Small data members, each checked by its size against what is left
    /// of `TOTAL_LIMIT` before it is read; one the budget leaves out is
    /// listed with no bytes. A member over `FILE_LIMIT` is no script's data.
    fn data(&self) -> Vec<(String, Option<Vec<u8>>)> {
        use crate::program::files::{FILE_LIMIT, TOTAL_LIMIT, may_be_data};
        let mut out = Vec::new();
        let mut left = TOTAL_LIMIT;
        for m in &self.directory.files {
            let size = usize::try_from(m.size).unwrap_or(usize::MAX);
            let Some(path) = m
                .path
                .as_ref()
                .filter(|p| size <= FILE_LIMIT && may_be_data(p))
            else {
                continue;
            };
            let bytes = (size <= left).then(|| self.read(m).ok()).flatten();
            let bytes = bytes.filter(|b| b.len() <= FILE_LIMIT.min(left));
            left -= bytes.as_ref().map_or(0, Vec::len);
            out.push((path.clone(), bytes));
        }
        out
    }

    fn sample(&self, program_dir: &str, reference: &str) -> Result<ir::Sample, String> {
        let index = self
            .resolve(program_dir, reference)
            .ok_or_else(|| format!("{reference}: not in this bank"))?;
        let member = self
            .directory
            .files
            .get(index)
            .ok_or_else(|| format!("{reference}: member index"))?;
        self.open_sample(member)
            .map_err(|e| format!("{reference}: {e:#}"))
    }
}

impl Bank {
    fn open_sample(&self, member: &Member) -> Result<ir::Sample> {
        let (offset, size, key) =
            self.ufs
                .locate(member, self.directory.metadata_key, self.content_key)?;
        let keystream = key.map(|key| ir::Keystream::Blocks {
            key,
            origin: offset,
            block: BLOCK,
        });
        let mut head = vec![0; size.min(HEADER_BYTES) as usize];
        self.ufs.read_exact_at(offset, &mut head)?;
        if let Some(keystream) = &keystream {
            keystream.apply(offset, &mut head);
        }
        let format = header::probe(&head, size)
            .context("sample format: WAV (PCM) and FLAC are read; AIFF and CAF are not yet")?;
        ensure!(format.frames > 0, "sample has no frames");
        Ok(ir::Sample {
            name: member.path.clone().unwrap_or_else(|| member.name.clone()),
            file: self.path.clone(),
            offset,
            len: Some(size),
            channels: format.channels,
            frames: format.frames,
            rate: format.rate,
            keystream,
        })
    }
}

/// Collapses `.`, `..` and empty parts of a bank path; `..` above the root is
/// dropped (a bank has no parent to climb to).
/// Member indices of a bank's programs, by path.
fn programs(directory: &Directory) -> Vec<usize> {
    let mut found: Vec<(&str, usize)> = directory
        .files
        .iter()
        .enumerate()
        .filter_map(|(i, m)| Some((m.path.as_deref()?, i)))
        .filter(|(p, _)| p.to_ascii_lowercase().ends_with(".uvip"))
        .collect();
    found.sort();
    found.into_iter().map(|(_, i)| i).collect()
}

pub(crate) fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn a_cover_comes_before_the_interface_background_and_strips_are_unnamed() {
        assert_eq!(picture_rank("Artwork/ao_cover.png"), Some(0));
        assert_eq!(picture_rank("artwork/back@2x.png"), Some(2));
        assert_eq!(picture_rank("artwork/back.png"), Some(3));
        assert_eq!(picture_rank("artwork/knob_50x50x129@2x.png"), Some(STILL));
        assert_eq!(picture_rank("Programs/a.lua"), None);
    }

    #[test]
    fn bank_paths_collapse_without_climbing_above_the_root() {
        assert_eq!(normalize("Presets/./../Samples/x.flac"), "Samples/x.flac");
        assert_eq!(normalize("../../a/b"), "a/b");
    }
}

#[cfg(test)]
mod keystream_tests {
    use kontra_ir as ir;

    /// Bytes the vendor's block transform encrypts decrypt through the
    /// engine's keystream at any piece of the member.
    #[test]
    fn engine_keystream_undoes_the_vendor_block_transform() {
        let plain: Vec<u8> = (0..1300u32).map(|i| (i * 31 + 5) as u8).collect();
        let (offset, key) = (4096u64, 0xfeed_beef_1234_5678u64);
        let mut cipher = plain.clone();
        uvi_file::crypto::transform_blocks(&mut cipher, key, offset);
        assert_ne!(cipher, plain);
        let stream = ir::Keystream::Blocks {
            key,
            origin: offset,
            block: 512,
        };
        for (from, to) in [(0, 1300), (100, 700), (511, 513), (1000, 1300)] {
            let mut piece = cipher[from..to].to_vec();
            stream.apply(offset + from as u64, &mut piece);
            assert_eq!(piece, plain[from..to], "piece {from}..{to}");
        }
    }
}
