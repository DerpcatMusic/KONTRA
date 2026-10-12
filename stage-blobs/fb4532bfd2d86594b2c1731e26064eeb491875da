//! Bounded UFS2/v3 records, decrypted names and explicitly keyed member reads.
//! Leaf-table links recover exact member paths; opaque record footers are retained.
//! Some faithfully parsed record fields are retained but unused by this crate.
#![allow(dead_code)]
use super::crypto;
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::{HashMap, HashSet},
    fs::{File, Metadata, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const HEADER_SIZE: u64 = 320;
const DIRECTORY_TAG: u32 = 0x2fba_3632;
const FILE_TAG: u32 = 0x6758_50e4;
const DESCRIPTOR_TAG: u32 = 0x1847_b398;
const TABLE_TAG: u32 = 0x3ca8_6aaf;
const MAX_RECORDS: usize = 1_000_000;
pub const MAX_MEMBER_SIZE: u64 = 512 << 20;

#[derive(Debug)]
pub struct Header {
    pub version: u32,
    pub uuid: [u8; 16],
    /// Historical API name for the opaque word at +32; it is not a verified file size.
    pub expected_size: u64,
    pub physical_size: u64,
    pub bank_name: String,
    pub root_entry_offset: u64,
    pub encoded_names: bool,
}

#[derive(Debug)]
pub(crate) struct NeedsMetadataNamespace;
impl std::fmt::Display for NeedsMetadataNamespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Encoded UFS names or metadata-protected members require a reader metadata namespace")
    }
}
impl std::error::Error for NeedsMetadataNamespace {}

#[derive(Debug, Clone)]
pub struct Record {
    /// Offset of the eight-byte payload-length prefix.
    pub offset: u64,
    pub payload_offset: u64,
    pub length: u64,
    pub available: u64,
    pub tag: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Protection {
    Clear,
    Metadata,
    Content,
    Unknown(u8),
}
impl From<u8> for Protection {
    fn from(mode: u8) -> Self {
        match mode {
            0 => Self::Clear,
            1 => Self::Metadata,
            2 => Self::Content,
            n => Self::Unknown(n),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Member {
    pub record_offset: u64,
    pub name: String,
    pub parent: Option<u64>,
    pub path: Option<String>,
    pub size: u64,
    pub offset: u64,
    pub mode: Protection,
    pub footer: Vec<u8>,
}

#[derive(Debug)]
pub struct Folder {
    pub record_offset: u64,
    pub name: String,
    pub parent: Option<u64>,
    pub path: Option<String>,
    pub children: Vec<u64>,
    pub child_descriptor: u64,
    pub child_index: Option<u64>,
    pub last_child_table: Option<u64>,
    pub child_table: Option<u64>,
    pub child_count: Option<u32>,
    pub footer: Vec<u8>,
}

pub struct Directory {
    pub files: Vec<Member>,
    pub directories: Vec<Folder>,
    pub records: Vec<Record>,
    pub warnings: Vec<String>,
    /// Deliberately kept out of Debug: it is bank access material.
    pub metadata_key: u64,
}

impl std::fmt::Debug for Directory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Directory")
            .field("files", &self.files)
            .field("directories", &self.directories)
            .field("records", &self.records)
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct Ufs {
    path: PathBuf,
    pub header: Header,
    snapshot: Metadata,
    header_bytes: Vec<u8>,
    // Pin the original inode; operations always use independently opened cursors.
    _snapshot_file: File,
}

fn u64_le(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes[..8].try_into().unwrap())
}
fn u32_le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().unwrap())
}

fn text(bytes: &[u8]) -> Result<String> {
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .context("UFS name lacks NUL terminator")?;
    std::str::from_utf8(&bytes[..end])
        .map(str::to_owned)
        .context("UFS name is not UTF-8; check metadata namespace")
}

fn open_regular(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    ensure!(
        file.metadata()?.file_type().is_file(),
        "UFS input is not a regular file"
    );
    Ok(file)
}

fn same_file(before: &Metadata, after: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev() || before.ino() != after.ino() {
            return false;
        }
    }
    // Timestamps detect ordinary in-place writes, not adversarial content changes.
    before.len() == after.len() && before.modified().ok() == after.modified().ok()
}

fn read_at(file: &mut File, offset: u64, size: usize) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; size];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn member_path(
    mut offset: u64,
    names: &HashMap<u64, &str>,
    parents: &HashMap<u64, u64>,
    root: Option<u64>,
) -> Result<Option<String>> {
    let mut parts = Vec::new();
    for _ in 0..256 {
        if Some(offset) == root && !parents.contains_key(&offset) {
            parts.reverse();
            let path = parts.join("/");
            ensure!(path.len() <= 4096, "UFS member path exceeds resource limit");
            return Ok(Some(path));
        }
        let name = names.get(&offset).context("Missing UFS path metadata")?;
        ensure!(
            !name.is_empty() && !matches!(*name, "." | "..") && !name.contains(['/', '\\']),
            "Unsafe UFS path component at record {offset}"
        );
        parts.push(*name);
        let Some(parent) = parents.get(&offset) else {
            return Ok(None);
        };
        offset = *parent;
    }
    bail!("UFS member path exceeds depth limit or contains a directory cycle")
}

impl Ufs {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file =
            open_regular(path).with_context(|| format!("Opening UFS {}", path.display()))?;
        let snapshot = file.metadata()?;
        let physical_size = snapshot.len();
        ensure!(physical_size >= HEADER_SIZE, "UFS header is truncated");
        let bytes = read_at(&mut file, 0, HEADER_SIZE as usize)?;
        ensure!(&bytes[..4] == b"UFS2", "Expected UFS2 container");
        ensure!(
            same_file(&snapshot, &file.metadata()?),
            "UFS bank changed while reading header"
        );
        let version = u32_le(&bytes[4..8]);
        ensure!(version == 3, "Unsupported UFS version {version}");
        Ok(Self {
            path: path.to_owned(),
            header: Header {
                version,
                uuid: bytes[8..24].try_into().unwrap(),
                expected_size: u64_le(&bytes[32..40]),
                physical_size,
                bank_name: text(&bytes[48..304])?,
                root_entry_offset: u64_le(&bytes[40..48]),
                encoded_names: bytes[304] != 0,
            },
            snapshot,
            header_bytes: bytes,
            _snapshot_file: file,
        })
    }

    pub(crate) fn check_snapshot(&self, file: &File) -> Result<()> {
        ensure!(
            same_file(&self.snapshot, &file.metadata()?),
            "UFS bank changed since header snapshot; reload bank"
        );
        Ok(())
    }

    // The returned private cursor is positioned after the validated header.
    pub(crate) fn open_snapshot(&self) -> Result<File> {
        let mut file = open_regular(&self.path)?;
        self.check_snapshot(&file)?;
        ensure!(
            read_at(&mut file, 0, HEADER_SIZE as usize)? == self.header_bytes,
            "UFS bank header changed since snapshot; reload bank"
        );
        self.check_snapshot(&file)?;
        Ok(file)
    }

    /// Walk the length-prefixed record chain without scanning opaque resource bytes.
    pub fn records(&self) -> Result<Vec<Record>> {
        let mut file = self.open_snapshot()?;
        let mut records = Vec::new();
        let mut offset = HEADER_SIZE;
        while offset < self.header.physical_size {
            ensure!(
                records.len() < MAX_RECORDS,
                "UFS exceeds record resource limit"
            );
            ensure!(
                self.header.physical_size - offset >= 8,
                "Truncated UFS record length at {offset}"
            );
            let length = u64_le(&read_at(&mut file, offset, 8)?);
            let payload_offset = offset + 8;
            let available = length.min(self.header.physical_size - payload_offset);
            let tag = if available >= 4 {
                Some(u32_le(&read_at(&mut file, payload_offset, 4)?))
            } else {
                None
            };
            records.push(Record {
                offset,
                payload_offset,
                length,
                available,
                tag,
            });
            if available < length {
                break;
            }
            offset = payload_offset
                .checked_add(length)
                .context("UFS record offset overflow")?;
        }
        self.check_snapshot(&file)?;
        Ok(records)
    }

    pub fn decode_directory(&self, namespace: &[u8]) -> Result<Directory> {
        if self.header.encoded_names && namespace.is_empty() {
            return Err(NeedsMetadataNamespace.into());
        }
        let records = self.records()?;
        let key = crypto::metadata_key(namespace, &self.header.bank_name);
        let mut directory = Directory {
            files: Vec::new(),
            directories: Vec::new(),
            records,
            warnings: Vec::new(),
            metadata_key: key,
        };
        let mut file = self.open_snapshot()?;
        for record in &directory.records {
            if record.available < record.length {
                directory.warnings.push(format!(
                    "Record at {} has {} of {} payload bytes; unavailable tail preserved",
                    record.offset, record.available, record.length
                ));
            }
            if let Some(DIRECTORY_TAG | FILE_TAG) = record.tag {
                let is_file = record.tag == Some(FILE_TAG);
                let required = if is_file { 277 } else { 268 };
                ensure!(
                    record.available >= required,
                    "Truncated UFS metadata record at {}",
                    record.offset
                );
                ensure!(
                    record.length == if is_file { 289 } else { 272 },
                    "Unexpected UFS metadata record length at {}",
                    record.offset
                );
                let mut bytes =
                    read_at(&mut file, record.payload_offset, record.available as usize)?;
                if self.header.encoded_names {
                    crypto::transform(&mut bytes[4..260], key, record.payload_offset + 4);
                }
                let name = text(&bytes[4..260])
                    .with_context(|| format!("UFS metadata record {}", record.offset))?;
                if is_file {
                    let member = Member {
                        record_offset: record.offset,
                        name,
                        parent: None,
                        path: None,
                        size: u64_le(&bytes[260..268]),
                        offset: u64_le(&bytes[268..276]),
                        mode: bytes[276].into(),
                        footer: bytes[277..].to_vec(),
                    };
                    if member
                        .offset
                        .checked_add(member.size)
                        .is_none_or(|end| end > self.header.physical_size)
                    {
                        directory.warnings.push(format!(
                            "Member record {} references bytes outside the physical container",
                            record.offset
                        ));
                    }
                    if let Protection::Unknown(mode) = member.mode {
                        directory.warnings.push(format!(
                            "Member record {} uses unsupported mode {}",
                            record.offset, mode
                        ));
                    }
                    directory.files.push(member);
                } else {
                    directory.directories.push(Folder {
                        record_offset: record.offset,
                        name,
                        parent: None,
                        path: None,
                        children: Vec::new(),
                        child_index: None,
                        last_child_table: None,
                        child_descriptor: u64_le(&bytes[260..268]),
                        child_table: None,
                        child_count: None,
                        footer: bytes[268..].to_vec(),
                    });
                }
            }
        }
        let by_payload: HashMap<u64, &Record> = directory
            .records
            .iter()
            .map(|r| (r.payload_offset, r))
            .collect();
        let names: HashMap<u64, &str> = directory
            .files
            .iter()
            .map(|m| (m.record_offset, m.name.as_str()))
            .chain(
                directory
                    .directories
                    .iter()
                    .map(|d| (d.record_offset, d.name.as_str())),
            )
            .collect();
        let mut parents = HashMap::new();
        let mut folder_links = HashMap::new();
        for folder in &directory.directories {
            let Some(descriptor) = by_payload.get(&folder.child_descriptor) else {
                directory.warnings.push(format!(
                    "Directory record {} has unavailable child descriptor",
                    folder.record_offset
                ));
                continue;
            };
            if descriptor.tag != Some(DESCRIPTOR_TAG) || descriptor.available < 28 {
                directory.warnings.push(format!(
                    "Directory record {} has an unrecognized child descriptor",
                    folder.record_offset
                ));
                continue;
            }
            let bytes = read_at(&mut file, descriptor.payload_offset, 28)?;
            let index = u64_le(&bytes[4..12]);
            let first = u64_le(&bytes[12..20]);
            let last = u64_le(&bytes[20..28]);
            let mut pointer = first;
            let mut previous = u64::MAX;
            let mut visited = HashSet::new();
            let mut children = Vec::new();
            while pointer != u64::MAX {
                ensure!(
                    visited.insert(pointer),
                    "UFS child table chain contains a cycle at {pointer}"
                );
                let Some(table) = by_payload
                    .get(&pointer)
                    .filter(|r| r.tag == Some(TABLE_TAG) && r.available >= 8)
                else {
                    directory.warnings.push(format!(
                        "Directory record {} has unavailable child table at {pointer}",
                        folder.record_offset
                    ));
                    break;
                };
                let count = u32_le(&read_at(&mut file, pointer + 4, 4)?) as u64;
                let used = 24 + count * 264;
                ensure!(
                    used <= table.available,
                    "UFS child table at {pointer} lacks active entries or links"
                );
                ensure!(
                    children.len() as u64 + count <= MAX_RECORDS as u64,
                    "UFS directory exceeds child resource limit"
                );
                let mut bytes = read_at(&mut file, pointer, used as usize)?;
                for i in 0..count as usize {
                    let at = 8 + i * 264;
                    if self.header.encoded_names {
                        crypto::transform(&mut bytes[at..at + 256], key, pointer + at as u64);
                    }
                    let name = text(&bytes[at..at + 256])?;
                    let child_pointer = u64_le(&bytes[at + 256..at + 264]);
                    let Some(child) = by_payload.get(&child_pointer) else {
                        directory.warnings.push(format!("Child table at {pointer} references unavailable metadata at {child_pointer}"));
                        continue;
                    };
                    ensure!(
                        names.get(&child.offset).is_some_and(|n| *n == name),
                        "UFS child name disagrees with metadata at {child_pointer}"
                    );
                    ensure!(
                        parents.insert(child.offset, folder.record_offset).is_none(),
                        "UFS metadata at {child_pointer} has multiple directory links"
                    );
                    children.push(child.offset);
                }
                let at = 8 + count as usize * 264;
                ensure!(
                    u64_le(&bytes[at..at + 8]) == previous,
                    "UFS child table at {pointer} has inconsistent previous link"
                );
                previous = pointer;
                pointer = u64_le(&bytes[at + 8..at + 16]);
            }
            if pointer == u64::MAX {
                ensure!(
                    previous == last,
                    "UFS directory record {} has inconsistent last table",
                    folder.record_offset
                );
            }
            folder_links.insert(folder.record_offset, (index, first, last, children));
        }
        let root = directory
            .directories
            .iter()
            .find(|d| d.record_offset + 8 == self.header.root_entry_offset)
            .map(|d| d.record_offset);
        let paths: HashMap<u64, Option<String>> = names
            .keys()
            .map(|&offset| member_path(offset, &names, &parents, root).map(|path| (offset, path)))
            .collect::<Result<_>>()?;
        for member in &mut directory.files {
            member.parent = parents.get(&member.record_offset).copied();
            member.path = paths.get(&member.record_offset).cloned().flatten();
        }
        for folder in &mut directory.directories {
            folder.parent = parents.get(&folder.record_offset).copied();
            folder.path = paths.get(&folder.record_offset).cloned().flatten();
            if let Some((index, first, last, children)) = folder_links.remove(&folder.record_offset)
            {
                folder.child_index = Some(index);
                folder.child_table = Some(first);
                folder.last_child_table = Some(last);
                folder.child_count = Some(children.len() as u32);
                folder.children = children;
            }
        }
        let unresolved = directory.files.iter().filter(|m| m.path.is_none()).count();
        if unresolved > 0 {
            directory.warnings.push(format!(
                "{unresolved} member paths could not be linked to the container root"
            ));
        }
        if namespace.is_empty() && directory.files.iter().any(|m| m.mode == Protection::Metadata) {
            return Err(NeedsMetadataNamespace.into());
        }
        self.check_snapshot(&file)?;
        Ok(directory)
    }

    /// Where a member's bytes are and the key they are stored under, for
    /// reading it in pieces. Same checks and access rules as [`Self::read_member`].
    pub(crate) fn locate(
        &self,
        member: &Member,
        metadata_key: u64,
        content_key: Option<u64>,
    ) -> Result<(u64, u64, Option<u64>)> {
        ensure!(
            member
                .offset
                .checked_add(member.size)
                .is_some_and(|end| end <= self.header.physical_size),
            "UFS member range exceeds physical container"
        );
        let key = match member.mode {
            Protection::Clear => None,
            Protection::Metadata => Some(metadata_key),
            Protection::Content => Some(
                content_key.context("UFS mode 2 member requires a caller-supplied content key")?,
            ),
            Protection::Unknown(mode) => bail!("Unsupported UFS member encryption mode {mode}"),
        };
        Ok((member.offset, member.size, key))
    }

    /// Never derives or guesses content access data; encrypted mode 2 needs a supplied key.
    pub fn read_member(
        &self,
        member: &Member,
        metadata_key: u64,
        content_key: Option<u64>,
    ) -> Result<Vec<u8>> {
        ensure!(
            member.size <= MAX_MEMBER_SIZE,
            "UFS member exceeds 512 MiB resource limit"
        );
        ensure!(
            member
                .offset
                .checked_add(member.size)
                .is_some_and(|end| end <= self.header.physical_size),
            "UFS member range exceeds physical container"
        );
        let key = match member.mode {
            Protection::Clear => None,
            Protection::Metadata => Some(metadata_key),
            Protection::Content => Some(
                content_key.context("UFS mode 2 member requires a caller-supplied content key")?,
            ),
            Protection::Unknown(mode) => bail!("Unsupported UFS member encryption mode {mode}"),
        };
        let mut file = self.open_snapshot()?;
        let size = usize::try_from(member.size).context("UFS member does not fit address space")?;
        let mut bytes = read_at(&mut file, member.offset, size)?;
        self.check_snapshot(&file)?;
        if let Some(key) = key {
            crypto::transform_blocks(&mut bytes, key, member.offset);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the user's local UFS corpus and pinned reader image"]
    fn reference_directories_match_independent_numeric_index() {
        let corpus = std::env::var_os("KONTRA_UFS_CORPUS").expect("KONTRA_UFS_CORPUS");
        let reader = std::env::var_os("KONTRA_UVI_REFERENCE_READER").expect("KONTRA_UVI_REFERENCE_READER");
        let namespaces = super::super::access::ReaderNamespaces::open(Path::new(&reader)).unwrap();
        let corpus: serde_json::Value = serde_json::from_slice(&std::fs::read(corpus).unwrap()).unwrap();
        let mut files = 0;
        let mut folders = 0;
        for bank in corpus["corpus"].as_array().unwrap() {
            let ufs = Ufs::open(Path::new(bank["path"].as_str().unwrap())).unwrap();
            let directory = ufs.decode_directory(&namespaces.metadata).unwrap();
            assert_eq!(directory.files.len() as u64, bank["counts"]["file"].as_u64().unwrap());
            assert_eq!(directory.directories.len() as u64, bank["counts"]["folder"].as_u64().unwrap());
            assert!(directory.files.iter().all(|member| member.path.is_some()));
            assert!(directory.directories.iter().all(|folder| folder.path.is_some()));
            files += directory.files.len();
            folders += directory.directories.len();
        }
        assert_eq!((files, folders), (127828, 1409));
    }
    use std::io::Write;

    #[cfg(unix)]
    #[test]
    fn same_header_size_and_mtime_replacement_cannot_supply_stale_member_bytes() {
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let fixture = Fixture(std::env::temp_dir().join(format!(
                "uvi-reopened-bank-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(&fixture.0).unwrap();
        let path = fixture.0.join("bank.ufs");
        let replacement = fixture.0.join("replacement.ufs");
        let mut bytes = vec![0; HEADER_SIZE as usize];
        bytes[..4].copy_from_slice(b"UFS2");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        bytes[48..56].copy_from_slice(b"Authored");
        bytes.extend_from_slice(&4u64.to_le_bytes());
        bytes.extend_from_slice(b"AAAA");
        let size = bytes.len() as u64;
        bytes[32..40].copy_from_slice(&size.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        let ufs = Ufs::open(&path).unwrap();
        let member = Member {
            record_offset: HEADER_SIZE,
            name: "authored.bin".into(),
            path: Some("authored.bin".into()),
            parent: None,
            offset: HEADER_SIZE + 8,
            size: 4,
            mode: Protection::Clear,
            footer: Vec::new(),
        };
        assert_eq!(ufs.read_member(&member, 0, None).unwrap(), b"AAAA");
        assert_eq!(ufs.records().unwrap().len(), 1);
        assert!(ufs.decode_directory(b"authored namespace").is_ok());
        bytes[HEADER_SIZE as usize + 8..].copy_from_slice(b"BBBB");
        std::fs::write(&replacement, &bytes).unwrap();
        let file = OpenOptions::new().write(true).open(&replacement).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(ufs.snapshot.modified().unwrap()))
            .unwrap();
        drop(file);
        std::fs::rename(&replacement, &path).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), ufs.snapshot.len());
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            ufs.snapshot.modified().unwrap()
        );
        for error in [
            ufs.records().unwrap_err(),
            ufs.decode_directory(b"authored namespace").unwrap_err(),
            ufs.read_member(&member, 0, None).unwrap_err(),
        ] {
            assert!(error.to_string().contains("changed since header snapshot"));
        }
        // The retained owner pins the original inode, without shared cursor reads.
        use std::os::unix::fs::FileExt;
        let mut original = [0; 4];
        ufs._snapshot_file
            .read_exact_at(&mut original, HEADER_SIZE + 8)
            .unwrap();
        assert_eq!(&original, b"AAAA");
        // A fresh owner is allowed to read the replacement bank.
        assert_eq!(
            Ufs::open(&path)
                .unwrap()
                .read_member(&member, 0, None)
                .unwrap(),
            b"BBBB"
        );
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn authored_records_member_and_missing_footer() {
        let path =
            std::env::temp_dir().join(format!("kontra-synthetic-ufs-{}.ufs", std::process::id()));
        let namespace = b"authored metadata namespace";
        let key = crypto::metadata_key(namespace, "Synthetic");
        let mut bytes = vec![0; 320];
        bytes[..4].copy_from_slice(b"UFS2");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        bytes[48..57].copy_from_slice(b"Synthetic");
        bytes[304] = 1;
        let plain: Vec<u8> = (0..1031).map(|i| i as u8).collect();
        let mut encrypted = plain.clone();
        crypto::transform_blocks(&mut encrypted, key, 328);
        bytes.extend_from_slice(&(encrypted.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&encrypted);
        let record_offset = bytes.len() as u64;
        let mut metadata = vec![0; 277];
        metadata[..4].copy_from_slice(&FILE_TAG.to_le_bytes());
        metadata[4..12].copy_from_slice(b"test.wav");
        crypto::transform(&mut metadata[4..260], key, record_offset + 12);
        metadata[260..268].copy_from_slice(&(plain.len() as u64).to_le_bytes());
        metadata[268..276].copy_from_slice(&328u64.to_le_bytes());
        metadata[276] = 1;
        bytes.extend_from_slice(&289u64.to_le_bytes());
        bytes.extend_from_slice(&metadata);
        let expected = bytes.len() as u64 + 12;
        bytes[32..40].copy_from_slice(&expected.to_le_bytes());
        File::create(&path).unwrap().write_all(&bytes).unwrap();
        let ufs = Ufs::open(&path).unwrap();
        let directory = ufs.decode_directory(namespace).unwrap();
        assert_eq!(directory.files.len(), 1);
        assert_eq!(directory.files[0].name, "test.wav");
        assert!(directory.files[0].footer.is_empty());
        assert_eq!(directory.records.last().unwrap().available, 277);
        assert_eq!(
            ufs.read_member(&directory.files[0], key, None).unwrap(),
            plain
        );
        let mut member = directory.files[0].clone();
        member.offset = u64::MAX;
        assert!(ufs.read_member(&member, key, None).is_err());
        member.offset = 328;
        member.mode = Protection::Content;
        assert!(ufs.read_member(&member, key, None).is_err());
        assert!(ufs.decode_directory(b"wrong namespace").is_err());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn authored_linked_leaf_paths_and_missing_padding() {
        fn append(bytes: &mut Vec<u8>, payload: &[u8], declared: u64) -> u64 {
            let pointer = bytes.len() as u64 + 8;
            bytes.extend_from_slice(&declared.to_le_bytes());
            bytes.extend_from_slice(payload);
            pointer
        }
        fn named(bytes: &mut Vec<u8>, tag: u32, name: &str, key: u64) -> u64 {
            let mut payload = vec![0; if tag == FILE_TAG { 289 } else { 272 }];
            payload[..4].copy_from_slice(&tag.to_le_bytes());
            payload[4..4 + name.len()].copy_from_slice(name.as_bytes());
            crypto::transform(&mut payload[4..260], key, bytes.len() as u64 + 12);
            let length = payload.len() as u64;
            append(bytes, &payload, length)
        }
        fn descriptor(bytes: &mut Vec<u8>, folder: u64) -> u64 {
            let mut payload = vec![0; 34];
            payload[..4].copy_from_slice(&DESCRIPTOR_TAG.to_le_bytes());
            let pointer = append(bytes, &payload, 34);
            bytes[folder as usize + 260..folder as usize + 268]
                .copy_from_slice(&pointer.to_le_bytes());
            pointer
        }
        fn table(bytes: &mut Vec<u8>, entries: &[(&str, u64)], key: u64, short: bool) -> u64 {
            let pointer = bytes.len() as u64 + 8;
            let active = 24 + entries.len() * 264;
            let mut payload = vec![0; if short { active } else { 16932 }];
            payload[..4].copy_from_slice(&TABLE_TAG.to_le_bytes());
            payload[4..8].copy_from_slice(&(entries.len() as u32).to_le_bytes());
            for (i, &(name, child)) in entries.iter().enumerate() {
                let at = 8 + i * 264;
                payload[at..at + name.len()].copy_from_slice(name.as_bytes());
                crypto::transform(&mut payload[at..at + 256], key, pointer + at as u64);
                payload[at + 256..at + 264].copy_from_slice(&child.to_le_bytes());
            }
            payload[active - 16..active].fill(255);
            append(bytes, &payload, 16932)
        }
        fn point(bytes: &mut [u8], descriptor: u64, first: u64, last: u64) {
            for (field, value) in [(4, first), (12, first), (20, last)] {
                let at = descriptor as usize + field;
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        let namespace = b"authored metadata namespace";
        let key = crypto::metadata_key(namespace, "Synthetic");
        let mut bytes = vec![0; 320];
        bytes[..4].copy_from_slice(b"UFS2");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        bytes[48..57].copy_from_slice(b"Synthetic");
        bytes[304] = 1;
        append(&mut bytes, b"uninterpreted preceding record", 30);
        let root = named(&mut bytes, DIRECTORY_TAG, "Root", key);
        bytes[40..48].copy_from_slice(&root.to_le_bytes());
        let a = named(&mut bytes, DIRECTORY_TAG, "A", key);
        let b = named(&mut bytes, DIRECTORY_TAG, "B", key);
        let a_file = named(&mut bytes, FILE_TAG, "same.wav", key);
        let a_other = named(&mut bytes, FILE_TAG, "other.wav", key);
        let b_file = named(&mut bytes, FILE_TAG, "same.wav", key);
        let root_descriptor = descriptor(&mut bytes, root);
        let a_descriptor = descriptor(&mut bytes, a);
        let b_descriptor = descriptor(&mut bytes, b);
        let root_table = table(&mut bytes, &[("A", a), ("B", b)], key, false);
        let first = table(&mut bytes, &[("same.wav", a_file)], key, false);
        let last = table(&mut bytes, &[("other.wav", a_other)], key, false);
        let short = table(&mut bytes, &[("same.wav", b_file)], key, true);
        point(&mut bytes, root_descriptor, root_table, root_table);
        point(&mut bytes, a_descriptor, first, last);
        point(&mut bytes, b_descriptor, short, short);
        bytes[first as usize + 280..first as usize + 288].copy_from_slice(&last.to_le_bytes());
        bytes[last as usize + 272..last as usize + 280].copy_from_slice(&first.to_le_bytes());
        let expected = bytes.len() as u64 + 16644;
        bytes[32..40].copy_from_slice(&expected.to_le_bytes());
        let path = std::env::temp_dir().join(format!(
            "kontra-synthetic-ufs-tree-{}.ufs",
            std::process::id()
        ));
        File::create(&path).unwrap().write_all(&bytes).unwrap();
        let ufs = Ufs::open(&path).unwrap();
        let directory = ufs.decode_directory(namespace).unwrap();
        assert_eq!(
            directory
                .files
                .iter()
                .map(|m| m.path.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["A/same.wav", "A/other.wav", "B/same.wav"]
        );
        assert_eq!(directory.directories[1].child_count, Some(2));
        assert_eq!(directory.records.last().unwrap().available, 288);
        assert_eq!(directory.warnings.len(), 1);
        // Clear directory names use the same framing and need no reader namespace.
        let mut clear = bytes.clone();
        clear[304] = 0;
        for pointer in [root, a, b, a_file, a_other, b_file] {
            let at = pointer as usize + 4;
            crypto::transform(&mut clear[at..at + 256], key, pointer + 4);
        }
        for pointer in [root_table, first, last, short] {
            let count = u32_le(&clear[pointer as usize + 4..pointer as usize + 8]);
            for i in 0..count as usize {
                let at = pointer as usize + 8 + i * 264;
                crypto::transform(&mut clear[at..at + 256], key, at as u64);
            }
        }
        File::create(&path).unwrap().write_all(&clear).unwrap();
        let clear_bank = Ufs::open(&path).unwrap();
        assert!(!clear_bank.header.encoded_names);
        let clear_directory = clear_bank.decode_directory(&[]).unwrap();
        assert_eq!(clear_directory.files.iter().map(|m| m.path.as_deref().unwrap()).collect::<Vec<_>>(),
            ["A/same.wav", "A/other.wav", "B/same.wav"]);
        bytes[first as usize + 280..first as usize + 288].copy_from_slice(&first.to_le_bytes());
        File::create(&path).unwrap().write_all(&bytes).unwrap();
        assert!(
            Ufs::open(&path)
                .unwrap()
                .decode_directory(namespace)
                .is_err()
        );
        std::fs::remove_file(path).unwrap();
    }
}
