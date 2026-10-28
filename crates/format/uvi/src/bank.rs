//! A UFS bank opened for reading: its programs, its members and the samples
//! programs name. Keys come from the bank's own directory and the content
//! key recovered from its PNG members.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use kontra_ir as ir;
use uvi_file::access::{Namespaces, recover_content_key};
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
            .then(|| recover_content_key(path, &ufs, &directory))
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

    /// Member indices of the bank's programs, by path.
    fn programs(&self) -> Vec<usize> {
        let mut found: Vec<(&str, usize)> = self
            .directory
            .files
            .iter()
            .enumerate()
            .filter_map(|(i, m)| Some((m.path.as_deref()?, i)))
            .filter(|(p, _)| p.to_ascii_lowercase().ends_with(".uvip"))
            .collect();
        found.sort();
        found.into_iter().map(|(_, i)| i).collect()
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
        let mut file = self.ufs.open_snapshot()?;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut head)?;
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
fn normalize(path: &str) -> String {
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
