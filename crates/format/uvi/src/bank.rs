//! A UFS bank opened for reading: its programs, its members and the samples
//! programs name, plus a disk-backed library adapter for standalone `.uvip`
//! files. Audio header probing delegates to [`kontra_format::header`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use kontra_format::io::{ReadAt, TransformedReader};
pub(crate) use kontra_format::paths::normalize_virtual_path as normalize;
use kontra_format::paths::{
    DirCache, normalize_separators, normalize_virtual_path, resolve_case_insensitive_bounded,
    suffix_match_components,
};
use kontra_format::{ProtectionStatus, ProtectionSystem};
use kontra_ir as ir;
use uvi_file::access::Namespaces;
use uvi_file::ufs::{Directory, Member, NeedsMetadataNamespace, Protection, Ufs};

use crate::Error;
use crate::program::Sampled;

/// UVI keys member bytes in 512-byte blocks (uvi-file's `transform_blocks`).
const BLOCK: u32 = 512;

/// Audio extensions tried when a program references a `.wav` that is stored as
/// `.flac`/`.aiff`/`.ogg` inside the bank or on disk.
const AUDIO_EXTS: &[&str] = &["wav", "flac", "aif", "aiff", "ogg"];

fn is_audio(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, ext)| {
        AUDIO_EXTS
            .iter()
            .any(|audio| ext.eq_ignore_ascii_case(audio))
    })
}

struct UfsMemberReader<'a> {
    ufs: &'a Ufs,
    offset: u64,
    size: u64,
}

impl ReadAt for UfsMemberReader<'_> {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.size || buf.is_empty() {
            return Ok(0);
        }
        let avail = self.size.saturating_sub(offset);
        let take = usize::try_from(avail).map_or(buf.len(), |n| n.min(buf.len()));
        let abs = self
            .offset
            .checked_add(offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "UFS offset overflow"))?;
        let Some(dst) = buf.get_mut(..take) else {
            return Ok(0);
        };
        self.ufs
            .read_exact_at(abs, dst)
            .map_err(|e| io::Error::other(format!("{e:#}")))?;
        Ok(take)
    }
}

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
    pub(crate) fn open(path: &Path) -> std::result::Result<Self, Error> {
        let ufs = Ufs::open(path).map_err(|e| Error::new(path, "bank", format!("{e:#}")))?;
        let directory = ufs
            .decode_directory(&Namespaces::native().metadata)
            .map_err(|e| classify_directory_error(path, e))?;
        if let Some(Protection::Unknown(mode)) = directory
            .files
            .iter()
            .map(|m| m.mode)
            .find(|m| matches!(m, Protection::Unknown(_)))
        {
            return Err(Error::protected(
                path,
                "bank",
                ProtectionStatus::protected(
                    ProtectionSystem::PaceIlok,
                    format!("UFS bank uses unsupported protection mode {mode}"),
                ),
            ));
        }
        let content_key = if directory
            .files
            .iter()
            .any(|m| m.mode == Protection::Content)
        {
            let id = crate::keys::file_id(path)
                .map_err(|e| Error::new(path, "bank cache metadata", format!("{e:#}")))?;
            match crate::keys::content_key(path, &ufs, &directory, id) {
                Ok(key) => Some(key),
                Err(e) if e.is::<io::Error>() => {
                    return Err(Error::new(path, "bank", format!("{e:#}")));
                }
                Err(e) => {
                    return Err(Error::protected(
                        path,
                        "bank",
                        ProtectionStatus::protected(
                            ProtectionSystem::PaceIlok,
                            format!("UFS bank content is protected ({e:#})"),
                        ),
                    ));
                }
            }
        } else {
            None
        };
        let (mut by_path, mut by_name) = (HashMap::new(), HashMap::<String, Vec<usize>>::new());
        for (index, member) in directory.files.iter().enumerate() {
            if let Some(p) = &member.path {
                by_path.entry(p.to_ascii_lowercase()).or_insert(index);
            }
            by_name
                .entry(member.name.to_ascii_lowercase())
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

    /// Fast, metadata-only protection inspection of a `.ufs` bank. Never runs
    /// key recovery or bypasses DRM; reports locked/unsupported banks honestly.
    pub(crate) fn inspect_protection(path: &Path) -> std::result::Result<ProtectionStatus, Error> {
        let ufs = Ufs::open(path).map_err(|e| Error::new(path, "bank", format!("{e:#}")))?;
        let directory = match ufs.decode_directory(&Namespaces::native().metadata) {
            Ok(dir) => dir,
            Err(err) => {
                if err.chain().any(|c| c.is::<NeedsMetadataNamespace>()) {
                    return Ok(ProtectionStatus::protected(
                        ProtectionSystem::PaceIlok,
                        "UFS directory metadata is encrypted",
                    ));
                }
                return Err(Error::new(path, "bank", format!("{err:#}")));
            }
        };
        Ok(directory_protection(path, &directory))
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
    pub(crate) fn program_paths(path: &Path) -> std::result::Result<Vec<String>, Error> {
        let ufs = Ufs::open(path).map_err(|e| Error::new(path, "bank", format!("{e:#}")))?;
        let directory = ufs
            .decode_directory(&Namespaces::native().metadata)
            .map_err(|e| classify_directory_error(path, e))?;
        Ok(programs(&directory)
            .into_iter()
            .filter_map(|i| directory.files.get(i)?.path.clone())
            .collect())
    }

    fn programs(&self) -> Vec<usize> {
        programs(&self.directory)
    }

    fn lookup(&self, path: &str) -> Option<usize> {
        self.by_path
            .get(&normalize_virtual_path(path).to_ascii_lowercase())
            .copied()
    }

    fn read(&self, member: &Member) -> Result<Vec<u8>> {
        self.ufs
            .read_member(member, self.directory.metadata_key, self.content_key)
    }

    /// The sample a program names, relative to the program's folder: exact
    /// member path first, then alternate audio extensions across all candidate
    /// roots, then unique or trailing-component-disambiguated basename lookup
    /// (banks frequently keep `.wav` names over `.flac` members).
    fn resolve(&self, program_dir: &str, reference: &str) -> Option<usize> {
        let normalized = normalize_separators(reference);
        let (bank_rooted, cleaned) = match normalized
            .strip_prefix("$/")
            .or_else(|| normalized.strip_prefix('$'))
        {
            Some(rest) => (true, rest.trim_start_matches('/')),
            None => (
                normalized.starts_with('/'),
                normalized.trim_start_matches('/'),
            ),
        };
        if cleaned.is_empty() {
            return None;
        }
        let roots = if bank_rooted || program_dir.is_empty() {
            [
                cleaned.to_owned(),
                format!("Samples/{cleaned}"),
                if program_dir.is_empty() {
                    cleaned.to_owned()
                } else {
                    format!("{program_dir}/{cleaned}")
                },
            ]
        } else {
            [
                format!("{program_dir}/{cleaned}"),
                cleaned.to_owned(),
                format!("Samples/{cleaned}"),
            ]
        };

        // 1. Exact path match across candidate roots.
        if let Some(index) = roots.iter().find_map(|p| self.lookup(p)) {
            return Some(index);
        }

        // 2. Alternate audio extension across all candidate roots.
        if let Some(index) = roots.iter().find_map(|candidate| {
            let norm = normalize_virtual_path(candidate);
            let (stem, _) = norm.rsplit_once('.')?;
            AUDIO_EXTS
                .iter()
                .find_map(|ext| self.lookup(&format!("{stem}.{ext}")))
        }) {
            return Some(index);
        }

        // 3. Basename fallback with component-boundary trailing-path disambiguation.
        let name = normalize_virtual_path(cleaned);
        let base = name
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if let Some(index) = self.resolve_by_name(&base, &name) {
            return Some(index);
        }

        // 4. Basename fallback with alternate audio extensions.
        let (stem, _) = base.rsplit_once('.')?;
        for ext in AUDIO_EXTS {
            let alt_base = format!("{stem}.{ext}");
            if alt_base == base {
                continue;
            }
            if let Some(index) = self.resolve_by_name(&alt_base, &name) {
                return Some(index);
            }
        }
        None
    }

    fn resolve_by_name(&self, base_lower: &str, full_ref: &str) -> Option<usize> {
        let matches = self.by_name.get(base_lower)?;
        if let [only] = matches.as_slice() {
            return Some(*only);
        }
        let ref_parts: Vec<&str> = full_ref
            .split('/')
            .filter(|s| !s.is_empty() && *s != "." && *s != "..")
            .collect();
        if ref_parts.len() < 2 {
            return None;
        }
        let mut suffix_owned: Vec<String> = ref_parts
            .get(..ref_parts.len() - 1)
            .unwrap_or_default()
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        suffix_owned.push(base_lower.to_owned());
        let suffix_refs: Vec<&str> = suffix_owned.iter().map(String::as_str).collect();
        for n in (2..=suffix_refs.len()).rev() {
            let Some(tail) = suffix_refs.get(suffix_refs.len() - n..) else {
                continue;
            };
            let mut hits = matches.iter().copied().filter(|&idx| {
                self.directory
                    .files
                    .get(idx)
                    .and_then(|m| m.path.as_deref())
                    .is_some_and(|p| suffix_match_components(Path::new(p), tail))
            });
            if let (Some(only), None) = (hits.next(), hits.next()) {
                return Some(only);
            }
        }
        None
    }
}

fn classify_directory_error(path: &Path, err: anyhow::Error) -> Error {
    if err.chain().any(|c| c.is::<NeedsMetadataNamespace>()) {
        Error::protected(
            path,
            "bank",
            ProtectionStatus::protected(
                ProtectionSystem::PaceIlok,
                "UFS directory metadata is encrypted",
            ),
        )
    } else {
        Error::new(path, "bank", format!("{err:#}"))
    }
}

fn directory_protection(path: &Path, directory: &Directory) -> ProtectionStatus {
    if let Some(Protection::Unknown(mode)) = directory
        .files
        .iter()
        .map(|m| m.mode)
        .find(|m| matches!(m, Protection::Unknown(_)))
    {
        return ProtectionStatus::protected(
            ProtectionSystem::PaceIlok,
            format!("UFS bank uses unsupported protection mode {mode}"),
        );
    }
    let has_content = directory
        .files
        .iter()
        .any(|m| m.mode == Protection::Content);
    if !has_content {
        return ProtectionStatus::Open;
    }
    if crate::keys::has_cached_key(path) {
        return ProtectionStatus::Open;
    }
    let has_verifiable_png = directory.files.iter().any(|m| {
        m.mode == Protection::Content
            && (45..=16 << 20).contains(&m.size)
            && m.name.to_ascii_lowercase().ends_with(".png")
    });
    if has_verifiable_png {
        ProtectionStatus::Open
    } else {
        ProtectionStatus::protected(
            ProtectionSystem::PaceIlok,
            "PACE/iLok-protected UFS bank (encrypted content)",
        )
    }
}

impl Bank {
    /// The member a script's picture path names: `./` and `../` lead up
    /// from the script's folder, so the normalized path first, then the
    /// member name if it is unique.
    pub(crate) fn resource(&self, path: &str) -> Option<Vec<u8>> {
        let normalized = normalize_virtual_path(path);
        let found = self.lookup(&normalized).or_else(|| {
            let name = normalized.rsplit('/').next()?.to_ascii_lowercase();
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

    fn sample(&self, program_dir: &str, reference: &str) -> std::result::Result<Sampled, String> {
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

    fn path_of(&self, program_dir: &str, reference: &str) -> Option<String> {
        self.directory
            .files
            .get(self.resolve(program_dir, reference)?)?
            .path
            .clone()
    }

    fn audio_under(&self, folder: &str) -> Vec<String> {
        let norm = normalize_virtual_path(folder).to_ascii_lowercase();
        let prefix = if norm.is_empty() {
            String::new()
        } else {
            format!("{norm}/")
        };
        self.directory
            .files
            .iter()
            .filter_map(|m| m.path.clone())
            .filter(|p| {
                is_audio(p) && (prefix.is_empty() || p.to_ascii_lowercase().starts_with(&prefix))
            })
            .collect()
    }
}

impl Bank {
    fn open_sample(&self, member: &Member) -> Result<Sampled> {
        let (offset, size, key) =
            self.ufs
                .locate(member, self.directory.metadata_key, self.content_key)?;
        let keystream = key.map(|key| block_keystream(key, offset, BLOCK));
        let reader = TransformedReader::new(
            UfsMemberReader {
                ufs: &self.ufs,
                offset,
                size,
            },
            offset,
            keystream.as_ref(),
        );
        let hdr = kontra_format::header::probe_read_at(&reader, size)
            .context("sample format: unsupported or malformed audio header")?;
        let channels = u8::try_from(hdr.channels).context("sample channel count exceeds 255")?;
        ensure!(hdr.frames > 0, "sample has no frames");
        let loops = hdr.loops.into_iter().map(|l| l.to_ir_loop(false)).collect();
        let sample = ir::Sample {
            name: member.path.clone().unwrap_or_else(|| member.name.clone()),
            file: self.path.clone(),
            offset,
            len: Some(size),
            channels,
            frames: hdr.frames,
            rate: hdr.sample_rate,
            keystream,
            raw: None,
        };
        Ok(Sampled { sample, loops })
    }
}

/// Disk-backed [`crate::program::Library`] for standalone unpacked `.uvip`
/// files living outside a `.ufs` bank.
pub(crate) struct DiskLibrary {
    root: PathBuf,
    program_dir: PathBuf,
    dir_cache: RefCell<DirCache>,
}

impl DiskLibrary {
    pub(crate) fn new(uvip_path: &Path) -> Self {
        let abs = std::path::absolute(uvip_path).unwrap_or_else(|_| uvip_path.to_path_buf());
        let program_dir = abs
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let root = find_disk_root(&program_dir);
        Self {
            root,
            program_dir,
            dir_cache: RefCell::new(DirCache::new()),
        }
    }

    fn resolve_file(&self, reference: &str) -> Option<PathBuf> {
        let norm = normalize_separators(reference);
        let cleaned = norm
            .strip_prefix("$/")
            .or_else(|| norm.strip_prefix('$'))
            .unwrap_or(&norm)
            .trim_start_matches('/');
        if cleaned.is_empty() {
            return None;
        }
        let mut cache = self.dir_cache.borrow_mut();
        let root = self.root.canonicalize().ok()?;
        let rooted = norm.starts_with(['$', '/']);
        let candidates = if rooted {
            [
                (&self.root, cleaned.to_owned()),
                (&self.root, format!("Samples/{cleaned}")),
                (&self.program_dir, cleaned.to_owned()),
            ]
        } else {
            [
                (&self.program_dir, cleaned.to_owned()),
                (&self.root, cleaned.to_owned()),
                (&self.root, format!("Samples/{cleaned}")),
            ]
        };
        // Ordinary references may follow links within the library. Only an
        // explicit moved-folder mapping may name a destination outside it.
        let contained_file = |path: PathBuf| {
            let target = path.canonicalize().ok()?;
            (target.starts_with(&root) && target.is_file()).then_some(target)
        };
        let mut resolve = |base: &Path, rel: &str| {
            let source = kontra_format::paths::lexical_clean(&base.join(rel));
            if source.starts_with(&self.root)
                && let Some(moved) = kontra_format::moved_with_case(&source, Some(&mut cache))
                && moved != source
                && let Ok(target) = moved.canonicalize()
                && target.is_file()
            {
                return Some(target);
            }
            resolve_case_insensitive_bounded(base, &self.root, rel, Some(&mut cache))
                .and_then(contained_file)
        };
        for (base, rel) in &candidates {
            if let Some(found) = resolve(base, rel) {
                return Some(found);
            }
        }
        for (base, rel) in &candidates {
            let (stem, _) = rel.rsplit_once('.')?;
            for ext in AUDIO_EXTS {
                if let Some(found) = resolve(base, &format!("{stem}.{ext}")) {
                    return Some(found);
                }
            }
        }
        None
    }
}

fn find_disk_root(start: &Path) -> PathBuf {
    start
        .ancestors()
        .take(5)
        .find(|dir| {
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case("samples")
                })
        })
        .map_or_else(|| start.to_path_buf(), Path::to_path_buf)
}

impl crate::program::Library for DiskLibrary {
    fn modules(&self) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        walk_disk(&self.root, &self.root, &mut |rel, path| {
            if rel.to_ascii_lowercase().ends_with(".lua")
                && let Ok(bytes) = std::fs::read(path)
            {
                out.push((rel, bytes));
            }
        });
        out
    }

    fn sample(&self, _program_dir: &str, reference: &str) -> std::result::Result<Sampled, String> {
        let file = self
            .resolve_file(reference)
            .ok_or_else(|| format!("{reference}: not found on disk"))?;
        let hdr =
            kontra_format::header::probe_header(&file).map_err(|e| format!("{reference}: {e}"))?;
        let channels = u8::try_from(hdr.channels)
            .map_err(|_| format!("{reference}: channel count exceeds 255"))?;
        if hdr.frames == 0 {
            return Err(format!("{reference}: sample has no frames"));
        }
        let loops = hdr.loops.into_iter().map(|l| l.to_ir_loop(false)).collect();
        Ok(Sampled {
            sample: ir::Sample {
                name: reference.to_owned(),
                file,
                offset: 0,
                len: None,
                channels,
                frames: hdr.frames,
                rate: hdr.sample_rate,
                keystream: None,
                raw: None,
            },
            loops,
        })
    }

    fn path_of(&self, _program_dir: &str, reference: &str) -> Option<String> {
        let file = self.resolve_file(reference)?;
        let root = self.root.canonicalize().ok()?;
        let rel = file.strip_prefix(&root).unwrap_or(&file);
        Some(rel.to_string_lossy().replace('\\', "/"))
    }

    fn audio_under(&self, folder: &str) -> Vec<String> {
        // path_of supplies root-relative paths with their on-disk case.
        let Ok(root) = self.root.canonicalize() else {
            return Vec::new();
        };
        let dir = root.join(normalize_virtual_path(folder));
        // The starting directory must also be inside the root and unaliased.
        if !dir.starts_with(&root) || dir.canonicalize().ok().as_deref() != Some(dir.as_path()) {
            return Vec::new();
        }
        let mut out = Vec::new();
        walk_disk(&root, &dir, &mut |rel, _| {
            if is_audio(&rel) {
                out.push(rel);
            }
        });
        out
    }
}

fn walk_disk(root: &Path, dir: &Path, visit: &mut impl FnMut(String, &Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_cached_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // Do not follow file or directory symlinks during discovery.
        let path = entry.path();
        if kind.is_dir() {
            walk_disk(root, &path, visit);
        } else if kind.is_file()
            && let Ok(rel) = path.strip_prefix(root)
        {
            visit(rel.to_string_lossy().replace('\\', "/"), &path);
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::{DiskLibrary, STILL, picture_rank};
    use crate::program::Library;
    use kontra_format::paths::normalize_virtual_path;

    #[test]
    fn disk_discovery_keeps_nested_audio_and_regular_lua_modules() {
        let root =
            std::env::temp_dir().join(format!("kontra-uvi-discovery-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Responses/Nested")).unwrap();
        std::fs::write(root.join("Responses/Nested/Hall.WAV"), []).unwrap();
        std::fs::write(root.join("Responses/Nested/other.txt"), []).unwrap();
        std::fs::write(root.join("Responses/load.lua"), b"return {}").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("Responses", root.join("Alias")).unwrap();

        let disk = DiskLibrary::new(&root.join("Patch.uvip"));
        assert_eq!(disk.audio_under("Responses"), ["Responses/Nested/Hall.WAV"]);
        assert_eq!(
            disk.modules(),
            [("Responses/load.lua".into(), b"return {}".to_vec())]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn disk_resolution_tries_moved_alternate_audio_after_exact_names() {
        let scratch = std::env::temp_dir().join(format!("kontra-uvi-moved-{}", std::process::id()));
        let root = scratch.join("Library");
        let moved = scratch.join("Relocated");
        std::fs::create_dir_all(root.join("Samples")).unwrap();
        std::fs::create_dir_all(&moved).unwrap();
        let alternate = moved.join("Hit.FLAC");
        let exact = moved.join("Hit.wav");
        std::fs::write(&alternate, []).unwrap();
        let disk = DiskLibrary::new(&root.join("Patch.uvip"));
        kontra_format::set_moved(&[(root.join("Samples"), moved)]);
        let fallback = disk.resolve_file("Samples/Hit.wav");
        std::fs::write(&exact, []).unwrap();
        let preferred = disk.resolve_file("Samples/Hit.wav");
        kontra_format::set_moved(&[]);
        assert_eq!(fallback.unwrap(), alternate.canonicalize().unwrap());
        assert_eq!(preferred.unwrap(), exact.canonicalize().unwrap());
        std::fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn a_cover_comes_before_the_interface_background_and_strips_are_unnamed() {
        assert_eq!(picture_rank("Artwork/ao_cover@2x.png"), Some(0));
        assert_eq!(picture_rank("Artwork/ao_cover.png"), Some(1));
        assert_eq!(picture_rank("artwork/back@2x.png"), Some(2));
        assert_eq!(picture_rank("artwork/back.png"), Some(3));
        assert_eq!(picture_rank("artwork/knob_50x50x129@2x.png"), Some(STILL));
        assert_eq!(picture_rank("Programs/a.lua"), None);
    }

    #[test]
    fn bank_paths_collapse_without_climbing_above_the_root() {
        assert_eq!(
            normalize_virtual_path("Presets/./../Samples/x.flac"),
            "Samples/x.flac"
        );
        assert_eq!(normalize_virtual_path("../../a/b"), "a/b");
    }
}

/// UVI block-restarted word-stream XOR cipher. Each `block` bytes from `origin`
/// are XORed with a 32-bit little-endian word stream seeded by `key` and the
/// block's file offset, so random reads decrypt independently. Deliberately not `Debug`.
struct UfsBlockXor {
    key: u64,
    origin: u64,
    block: u32,
}

impl ir::ByteTransform for UfsBlockXor {
    fn apply(&self, offset: u64, bytes: &mut [u8]) {
        blocks(self.key, self.origin, u64::from(self.block), offset, bytes);
    }
}

fn block_keystream(key: u64, origin: u64, block: u32) -> ir::Keystream {
    ir::Keystream::new(Arc::new(UfsBlockXor { key, origin, block }))
}

/// The block-restarted word stream: block `n` of `origin` starts at
/// `origin + n·block`; the stream is the one a whole-block pass would XOR.
fn blocks(key: u64, origin: u64, block: u64, at: u64, bytes: &mut [u8]) {
    if block == 0 {
        return;
    }
    let Some(mut into) = at.checked_sub(origin) else {
        return;
    };
    let mut rest = bytes;
    while !rest.is_empty() {
        let start = into / block * block;
        let skip = (into - start) as usize;
        let n = (block as usize - skip).min(rest.len());
        let (head, tail) = std::mem::take(&mut rest).split_at_mut(n);
        let mask = word_stream(key, origin + start, skip + n);
        head.iter_mut()
            .zip(mask.get(skip..).unwrap_or_default())
            .for_each(|(b, m)| *b ^= m);
        rest = tail;
        into += n as u64;
    }
}

/// The first `len` bytes of the word stream seeded by `key` and `nonce`, a
/// multiplicative state stepped per word (uvi-file's `transform`).
fn word_stream(key: u64, nonce: u64, len: usize) -> Vec<u8> {
    const MIX: u64 = 0xc6a4_a793_5bd1_e995;
    const STEP: u64 = 0x5851_f42d_4c95_7f2d;
    let mix = |value: u64| {
        let value = value.wrapping_mul(MIX);
        (value ^ (value >> 47)).wrapping_mul(MIX)
    };
    let mut state = (mix(nonce) ^ key).wrapping_mul(MIX);
    let mut out = Vec::with_capacity(len + 4);
    while out.len() < len {
        let word = (((state >> 22) ^ state) >> (22 + (state >> 61))) as u32;
        out.extend(word.to_le_bytes());
        state = state.wrapping_mul(STEP);
    }
    out.truncate(len);
    out
}

#[cfg(test)]
mod keystream_tests {
    use super::block_keystream;

    #[test]
    fn block_keystream_reads_in_any_piece_and_undoes_itself() {
        let plain: Vec<u8> = (0..1500u32).map(|i| (i * 7 + 3) as u8).collect();
        let key = block_keystream(0x1234_5678_9abc_def0, 4096, 512);
        let mut whole = plain.clone();
        key.apply(4096, &mut whole);
        assert_ne!(whole, plain);
        let mut piece = whole[300..1100].to_vec();
        key.apply(4096 + 300, &mut piece);
        assert_eq!(piece, plain[300..1100]);
        key.apply(4096, &mut whole);
        assert_eq!(whole, plain);
    }

    #[test]
    fn engine_keystream_undoes_the_vendor_block_transform() {
        let plain: Vec<u8> = (0..1300u32).map(|i| (i * 31 + 5) as u8).collect();
        let (offset, key) = (4096u64, 0xfeed_beef_1234_5678u64);
        let mut cipher = plain.clone();
        uvi_file::crypto::transform_blocks(&mut cipher, key, offset);
        assert_ne!(cipher, plain);
        let stream = block_keystream(key, offset, 512);
        for (from, to) in [(0, 1300), (100, 700), (511, 513), (1000, 1300)] {
            let mut piece = cipher[from..to].to_vec();
            stream.apply(offset + from as u64, &mut piece);
            assert_eq!(piece, plain[from..to], "piece {from}..{to}");
        }
    }
}

#[cfg(test)]
#[test]
fn standalone_samples_honor_root_marker_for_exact_and_alternate_extensions() {
    let root = std::env::temp_dir().join(format!("kontra-uvi-root-marker-{}", std::process::id()));
    let presets = root.join("Presets");
    std::fs::create_dir_all(&presets).unwrap();
    std::fs::create_dir_all(root.join("Samples")).unwrap();
    for base in [&root, &presets] {
        std::fs::write(base.join("Hit.wav"), b"").unwrap();
    }
    let disk = DiskLibrary::new(&presets.join("Program.uvip"));
    for ext in ["wav", "aiff"] {
        assert_eq!(
            disk.resolve_file(&format!("$/Hit.{ext}")),
            Some(root.join("Hit.wav").canonicalize().unwrap())
        );
        assert_eq!(
            disk.resolve_file(&format!("Hit.{ext}")),
            Some(presets.join("Hit.wav").canonicalize().unwrap())
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
