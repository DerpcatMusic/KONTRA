//! Source-validated, bounded product directory metadata. Never stores resource
//! payloads or access keys. Any cache failure falls back to the source parser.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    sync::atomic::{AtomicU64, Ordering},
    time::UNIX_EPOCH,
};

const MAGIC: &[u8] = b"KONTRA UFS directory metadata 1\n";
const PARSER: &[u8] = b"ufs2-v3-linked-directory-20261014-1";
const ENTRY_LIMIT: u64 = 64 << 20;
const CACHE_LIMIT: u64 = 256 << 20;
const MEMORY_LIMIT: usize = 128 << 20;
const FINGERPRINT_BYTES: usize = 4096;

pub(super) fn root(scan_active: bool) -> Option<PathBuf> {
    if scan_active {
        return None;
    }
    dirs::cache_dir().map(|root| root.join("kontra/v2-ufs-directories"))
}

/// Captured identity plus bounded source fingerprints. This detects ordinary
/// edits/replacements, not adversarial middle edits preserving every stamp.
pub(super) fn stamp(ufs: &Ufs, namespace: &[u8]) -> Option<[u8; 32]> {
    let mut file = ufs.open_snapshot().ok()?;
    let meta = file.metadata().ok()?;
    #[cfg(not(unix))]
    {
        // Platforms without stable std file IDs require a captured creation
        // generation. If unavailable, persistence misses safely.
        if meta.created().ok()? != ufs.snapshot.created().ok()? {
            return None;
        }
    }
    let mut hash = Sha256::new();
    hash.update(MAGIC);
    hash.update(PARSER);
    static PARSER_DIGEST: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
    hash.update(PARSER_DIGEST.get_or_init(|| {
        let mut hash = Sha256::new();
        hash.update(include_bytes!("ufs.rs"));
        hash.update(include_bytes!("ufs_cache.rs"));
        hash.update(include_bytes!("crypto.rs"));
        hash.finalize().into()
    }));
    hash.update(ufs.header.version.to_le_bytes());
    hash.update(Sha256::digest(namespace));
    let path = ufs.path.as_os_str().as_encoded_bytes();
    hash.update((path.len() as u64).to_le_bytes());
    hash.update(path);
    hash.update(meta.len().to_le_bytes());
    hash.update(
        meta.modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_nanos()
            .to_le_bytes(),
    );
    #[cfg(not(unix))]
    hash.update(
        meta.created()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_nanos()
            .to_le_bytes(),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (meta.ctime(), meta.ctime_nsec()) != (ufs.snapshot.ctime(), ufs.snapshot.ctime_nsec()) {
            return None;
        }
        hash.update(meta.dev().to_le_bytes());
        hash.update(meta.ino().to_le_bytes());
        hash.update(meta.ctime().to_le_bytes());
        hash.update(meta.ctime_nsec().to_le_bytes());
    }
    let size = meta.len().min(FINGERPRINT_BYTES as u64) as usize;
    hash.update(read_at(&mut file, 0, size).ok()?);
    hash.update(read_at(&mut file, meta.len() - size as u64, size).ok()?);
    let after = file.metadata().ok()?;
    if !same_file(&meta, &after) {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (meta.ctime(), meta.ctime_nsec()) != (after.ctime(), after.ctime_nsec()) {
            return None;
        }
    }
    Some(hash.finalize().into())
}

fn entry(root: &Path, stamp: [u8; 32]) -> PathBuf {
    let name: String = stamp.iter().map(|byte| format!("{byte:02x}")).collect();
    root.join(format!("{name}.directory"))
}

struct Decoder<'a> {
    bytes: &'a [u8],
    memory: usize,
}
impl<'a> Decoder<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, tail) = self.bytes.split_at_checked(n)?;
        self.bytes = tail;
        Some(head)
    }
    fn byte(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn reserve(&mut self, n: usize) -> Option<()> {
        self.memory = self.memory.checked_add(n)?;
        (self.memory <= MEMORY_LIMIT).then_some(())
    }
    fn count<T>(&mut self, min_bytes: usize) -> Option<usize> {
        let n = self.u32()? as usize;
        if n > MAX_RECORDS || n > self.bytes.len() / min_bytes {
            return None;
        }
        self.reserve(n.checked_mul(std::mem::size_of::<T>())?)?;
        Some(n)
    }
    fn blob(&mut self, limit: usize) -> Option<Vec<u8>> {
        let n = self.u32()? as usize;
        if n > limit {
            return None;
        }
        self.reserve(n)?;
        Some(self.take(n)?.to_vec())
    }
    fn text(&mut self, limit: usize) -> Option<String> {
        String::from_utf8(self.blob(limit)?).ok()
    }
    fn optional<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<Option<T>> {
        match self.byte()? {
            0 => Some(None),
            1 => Some(Some(f(self)?)),
            _ => None,
        }
    }
}
struct Encoder {
    bytes: Vec<u8>,
}
impl Encoder {
    fn put(&mut self, bytes: &[u8]) -> Option<()> {
        if self.bytes.len().checked_add(bytes.len())? > ENTRY_LIMIT as usize {
            return None;
        }
        self.bytes.extend_from_slice(bytes);
        Some(())
    }
    fn byte(&mut self, v: u8) -> Option<()> {
        self.put(&[v])
    }
    fn u32(&mut self, v: u32) -> Option<()> {
        self.put(&v.to_le_bytes())
    }
    fn u64(&mut self, v: u64) -> Option<()> {
        self.put(&v.to_le_bytes())
    }
    fn count(&mut self, n: usize) -> Option<()> {
        if n > MAX_RECORDS {
            return None;
        }
        self.u32(n.try_into().ok()?)
    }
    fn blob(&mut self, b: &[u8], limit: usize) -> Option<()> {
        if b.len() > limit {
            return None;
        }
        self.u32(b.len().try_into().ok()?)?;
        self.put(b)
    }
    fn optional<T>(
        &mut self,
        v: Option<T>,
        f: impl FnOnce(&mut Self, T) -> Option<()>,
    ) -> Option<()> {
        self.byte(u8::from(v.is_some()))?;
        if let Some(v) = v {
            f(self, v)?;
        }
        Some(())
    }
}
fn encode(d: &Directory) -> Option<Vec<u8>> {
    let mut e = Encoder { bytes: Vec::new() };
    e.count(d.records.len())?;
    for r in &d.records {
        for v in [r.offset, r.payload_offset, r.length, r.available] {
            e.u64(v)?;
        }
        e.optional(r.tag, |e, v| e.u32(v))?;
    }
    e.count(d.files.len())?;
    for m in &d.files {
        e.u64(m.record_offset)?;
        e.blob(m.name.as_bytes(), 255)?;
        e.optional(m.path.as_deref(), |e, p| e.blob(p.as_bytes(), 4096))?;
        e.optional(m.parent, |e, p| e.u64(p))?;
        e.u64(m.offset)?;
        e.u64(m.size)?;
        e.byte(match m.mode {
            Protection::Clear => 0,
            Protection::Metadata => 1,
            Protection::Content => 2,
            Protection::Unknown(n) => n,
        })?;
        e.blob(&m.footer, 12)?;
    }
    e.count(d.directories.len())?;
    for f in &d.directories {
        e.u64(f.record_offset)?;
        e.blob(f.name.as_bytes(), 255)?;
        e.optional(f.path.as_deref(), |e, p| e.blob(p.as_bytes(), 4096))?;
        e.optional(f.parent, |e, p| e.u64(p))?;
        e.u64(f.child_descriptor)?;
        for v in [f.child_index, f.last_child_table, f.child_table] {
            e.optional(v, |e, v| e.u64(v))?;
        }
        e.optional(f.child_count, |e, n| e.u32(n))?;
        e.count(f.children.len())?;
        for &child in &f.children {
            e.u64(child)?;
        }
        e.blob(&f.footer, 4)?;
    }
    e.count(d.warnings.len())?;
    for w in &d.warnings {
        e.blob(w.as_bytes(), 4096)?;
    }
    Some(e.bytes)
}
fn decode(bytes: &[u8], key: u64) -> Option<Directory> {
    let mut d = Decoder { bytes, memory: 0 };
    let n = d.count::<Record>(33)?;
    let mut records = Vec::with_capacity(n);
    for _ in 0..n {
        records.push(Record {
            offset: d.u64()?,
            payload_offset: d.u64()?,
            length: d.u64()?,
            available: d.u64()?,
            tag: d.optional(|d| d.u32())?,
        });
    }
    let n = d.count::<Member>(36)?;
    let mut files = Vec::with_capacity(n);
    for _ in 0..n {
        files.push(Member {
            record_offset: d.u64()?,
            name: d.text(255)?,
            path: d.optional(|d| d.text(4096))?,
            parent: d.optional(|d| d.u64())?,
            offset: d.u64()?,
            size: d.u64()?,
            mode: d.byte()?.into(),
            footer: d.blob(12)?,
        });
    }
    let n = d.count::<Folder>(32)?;
    let mut directories = Vec::with_capacity(n);
    for _ in 0..n {
        let record_offset = d.u64()?;
        let name = d.text(255)?;
        let path = d.optional(|d| d.text(4096))?;
        let parent = d.optional(|d| d.u64())?;
        let child_descriptor = d.u64()?;
        let child_index = d.optional(|d| d.u64())?;
        let last_child_table = d.optional(|d| d.u64())?;
        let child_table = d.optional(|d| d.u64())?;
        let child_count = d.optional(|d| d.u32())?;
        let n = d.count::<u64>(8)?;
        let mut children = Vec::with_capacity(n);
        for _ in 0..n {
            children.push(d.u64()?);
        }
        directories.push(Folder {
            record_offset,
            name,
            path,
            parent,
            child_descriptor,
            child_index,
            last_child_table,
            child_table,
            child_count,
            children,
            footer: d.blob(4)?,
        });
    }
    let n = d.count::<String>(4)?;
    let mut warnings = Vec::with_capacity(n);
    for _ in 0..n {
        warnings.push(d.text(4096)?);
    }
    d.bytes.is_empty().then_some(Directory {
        records,
        files,
        directories,
        warnings,
        metadata_key: key,
    })
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !matches!(name, "." | "..")
        && !name.contains(['/', '\\', '\0'])
}
fn safe_path(path: &str, root: bool) -> bool {
    (root && path.is_empty())
        || (!path.is_empty()
            && path.len() <= 4096
            && !path.contains(['\\', '\0'])
            && path.split('/').all(safe_name))
}

fn validate(ufs: &Ufs, d: &Directory) -> Option<()> {
    if d.records.len() > MAX_RECORDS || d.files.len() + d.directories.len() > d.records.len() {
        return None;
    }
    let mut next = HEADER_SIZE;
    for (index, r) in d.records.iter().enumerate() {
        if r.offset != next
            || ufs.header.physical_size.checked_sub(r.offset)? < 8
            || r.payload_offset != r.offset.checked_add(8)?
            || r.available != r.length.min(ufs.header.physical_size - r.payload_offset)
        {
            return None;
        }
        if r.tag.is_some() != (r.available >= 4 && ufs.header.physical_size - r.offset >= 12) {
            return None;
        }
        if r.available < r.length {
            if index + 1 != d.records.len() {
                return None;
            }
            next = ufs.header.physical_size;
        } else {
            next = r.payload_offset.checked_add(r.length)?;
        }
    }
    if next != ufs.header.physical_size {
        return None;
    }
    let records: HashMap<_, _> = d.records.iter().map(|r| (r.offset, r)).collect();
    let payloads: HashMap<_, _> = d.records.iter().map(|r| (r.payload_offset, r)).collect();
    let folders: HashMap<_, _> = d.directories.iter().map(|f| (f.record_offset, f)).collect();
    let mut names = HashMap::new();
    let mut parents = HashMap::new();
    for m in &d.files {
        let r = records.get(&m.record_offset)?;
        if r.tag != Some(FILE_TAG)
            || r.length != 289
            || r.available < 277
            || m.footer.len() != r.available as usize - 277
            || !safe_name(&m.name)
            || m.size > MAX_MEMBER_SIZE
            || m.offset.checked_add(m.size)? > ufs.header.physical_size
            || matches!(m.mode, Protection::Unknown(_))
            || m.path.as_deref().is_some_and(|p| !safe_path(p, false))
        {
            return None;
        }
        if names.insert(m.record_offset, m.name.as_str()).is_some() {
            return None;
        }
        if let Some(p) = m.parent {
            folders.get(&p)?;
            parents.insert(m.record_offset, p);
        }
    }
    for f in &d.directories {
        let r = records.get(&f.record_offset)?;
        if r.tag != Some(DIRECTORY_TAG)
            || r.length != 272
            || r.available < 268
            || f.footer.len() != r.available as usize - 268
            || !safe_name(&f.name)
            || f.path.as_deref().is_some_and(|p| !safe_path(p, true))
            || names.insert(f.record_offset, f.name.as_str()).is_some()
        {
            return None;
        }
        if let Some(p) = f.parent {
            folders.get(&p)?;
            parents.insert(f.record_offset, p);
        }
        let descriptor = payloads.get(&f.child_descriptor)?;
        if descriptor.tag != Some(DESCRIPTOR_TAG) || descriptor.available < 28 {
            return None;
        }
        if f.child_count != Some(f.children.len().try_into().ok()?) || f.child_index.is_none() {
            return None;
        }
        for pointer in [f.child_table?, f.last_child_table?] {
            if pointer != u64::MAX {
                let r = payloads.get(&pointer)?;
                if r.tag != Some(TABLE_TAG) || r.available < 24 {
                    return None;
                }
            }
        }
    }
    if d.records
        .iter()
        .filter(|r| matches!(r.tag, Some(FILE_TAG | DIRECTORY_TAG)))
        .count()
        != names.len()
    {
        return None;
    }
    let mut linked = HashSet::new();
    for f in &d.directories {
        for child in &f.children {
            names.get(child)?;
            if parents.get(child) != Some(&f.record_offset) || !linked.insert(*child) {
                return None;
            }
        }
    }
    if linked.len() != parents.len() {
        return None;
    }
    let root = d
        .directories
        .iter()
        .find(|f| f.record_offset + 8 == ufs.header.root_entry_offset)
        .map(|f| f.record_offset);
    for m in &d.files {
        if member_path(m.record_offset, &names, &parents, root).ok()? != m.path {
            return None;
        }
    }
    for f in &d.directories {
        if member_path(f.record_offset, &names, &parents, root).ok()? != f.path {
            return None;
        }
    }
    Some(())
}

pub(super) fn load(
    root: &Path,
    ufs: &Ufs,
    namespace: &[u8],
    expected: [u8; 32],
) -> Option<Directory> {
    let start = std::env::var_os("KONTRA_AUDIT_UFS_DIRECTORY").map(|_| std::time::Instant::now());
    let result = (|| {
        let mut file = open_regular(&entry(root, expected)).ok()?;
        let size = file.metadata().ok()?.len();
        if size > ENTRY_LIMIT + MAGIC.len() as u64 + 64 {
            return None;
        }
        let mut prefix = vec![0; MAGIC.len() + 64];
        file.read_exact(&mut prefix).ok()?;
        if &prefix[..MAGIC.len()] != MAGIC || prefix[MAGIC.len()..MAGIC.len() + 32] != expected {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(ENTRY_LIMIT + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > ENTRY_LIMIT
            || Sha256::digest(&bytes).as_slice() != &prefix[MAGIC.len() + 32..]
        {
            return None;
        }
        let directory = decode(
            &bytes,
            crypto::metadata_key(namespace, &ufs.header.bank_name),
        )?;
        validate(ufs, &directory)?;
        (stamp(ufs, namespace)? == expected).then_some(directory)
    })();
    if let Some(start) = start {
        eprintln!(
            "UFS_DIRECTORY_CACHE {}",
            serde_json::json!({"schema":1,"hit":result.is_some(),"lookup_decode_validate_ms":start.elapsed().as_secs_f64()*1000.,"payloads_cached":0})
        );
    }
    result
}

pub(super) fn store(
    root: &Path,
    ufs: &Ufs,
    namespace: &[u8],
    expected: [u8; 32],
    directory: &Directory,
) {
    let start = std::env::var_os("KONTRA_AUDIT_UFS_DIRECTORY").map(|_| std::time::Instant::now());
    let result =
        (|| -> Option<u64> {
            if directory.metadata_key != crypto::metadata_key(namespace, &ufs.header.bank_name) {
                return None;
            }
            validate(ufs, directory)?;
            let bytes = encode(directory)?;
            if stamp(ufs, namespace)? != expected {
                return None;
            }
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(root).ok()?;
            let filename = entry(root, expected);
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let tmp = filename.with_extension(format!(
                "tmp{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut options = OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let written = (|| -> Option<()> {
                let mut file = options.open(&tmp).ok()?;
                file.write_all(MAGIC).ok()?;
                file.write_all(&expected).ok()?;
                file.write_all(&Sha256::digest(&bytes)).ok()?;
                file.write_all(&bytes).ok()?;
                file.flush().ok()?;
                if stamp(ufs, namespace)? != expected {
                    return None;
                }
                drop(file);
                std::fs::rename(&tmp, &filename).ok()?;
                Some(())
            })();
            if written.is_none() {
                let _ = std::fs::remove_file(&tmp);
                return None;
            }
            let mut files: Vec<_> =
                std::fs::read_dir(root)
                    .ok()?
                    .flatten()
                    .filter_map(|e| {
                        let path = e.path();
                        let m = e.metadata().ok()?;
                        (m.is_file() && path.extension().is_some_and(|e| e == "directory"))
                            .then_some((m.modified().ok()?, m.len(), path))
                    })
                    .collect();
            files.sort_by_key(|f| f.0);
            let mut held: u64 = files.iter().map(|f| f.1).sum();
            for (_, size, path) in files {
                if held <= CACHE_LIMIT {
                    break;
                }
                if std::fs::remove_file(path).is_ok() {
                    held = held.saturating_sub(size);
                }
            }
            Some(bytes.len() as u64)
        })();
    if let Some(start) = start {
        eprintln!(
            "UFS_DIRECTORY_CACHE_STORE {}",
            serde_json::json!({"schema":1,"stored":result.is_some(),"encoded_bytes":result,"validate_encode_write_ms":start.elapsed().as_secs_f64()*1000.,"payloads_cached":0})
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "uvi-directory-cache-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn fixture_bank(path: &Path) -> Vec<u8> {
        let mut bytes = vec![0; HEADER_SIZE as usize];
        bytes[..4].copy_from_slice(b"UFS2");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        bytes[48..57].copy_from_slice(b"Synthetic");
        let payload = b"DO_NOT_CACHE_SAMPLE_BODY";
        bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        bytes.extend_from_slice(payload);
        let mut member = vec![0; 289];
        member[..4].copy_from_slice(&FILE_TAG.to_le_bytes());
        member[4..14].copy_from_slice(b"sample.wav");
        member[260..268].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        member[268..276].copy_from_slice(&(HEADER_SIZE + 8).to_le_bytes());
        bytes.extend_from_slice(&289u64.to_le_bytes());
        bytes.extend_from_slice(&member);
        let size = bytes.len() as u64;
        bytes[32..40].copy_from_slice(&size.to_le_bytes());
        std::fs::write(path, &bytes).unwrap();
        bytes
    }
    fn overwrite(root: &Path, key: [u8; 32], body: &[u8]) {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&key);
        bytes.extend_from_slice(&Sha256::digest(body));
        bytes.extend_from_slice(body);
        std::fs::write(entry(root, key), bytes).unwrap();
    }
    #[test]
    fn scanner_gate_never_discovers_or_uses_product_cache() {
        assert!(root(true).is_none());
    }
    #[test]
    fn explicit_read_only_cache_miss_parses_without_creating_cache_files() {
        let f = Fixture::new();
        let path = f.0.join("bank.ufs");
        fixture_bank(&path);
        let root = f.0.join("read-only");
        let ufs = Ufs::open(&path).unwrap();
        let namespace = b"cache namespace";
        let direct = ufs.decode_directory_window(namespace, 0).unwrap();
        let observed = ufs
            .decode_directory_cached_policy(namespace, &root, false)
            .unwrap();
        assert!(direct == observed);
        assert!(!root.exists());
        let key = stamp(&ufs, namespace).unwrap();
        store(&root, &ufs, namespace, key, &direct);
        let file = entry(&root, key);
        let before = std::fs::metadata(&file).unwrap().modified().unwrap();
        assert!(
            ufs.decode_directory_cached_policy(namespace, &root, false)
                .unwrap()
                == direct
        );
        assert_eq!(
            std::fs::metadata(&file).unwrap().modified().unwrap(),
            before
        );
    }
    #[test]
    fn bounded_roundtrip_excludes_payloads_keys_and_rejects_invalid_cache_data() {
        let f = Fixture::new();
        let path = f.0.join("bank.ufs");
        fixture_bank(&path);
        let root = f.0.join("cache");
        let namespace = b"cache namespace A";
        let ufs = Ufs::open(&path).unwrap();
        let directory = ufs.decode_directory_window(namespace, 0).unwrap();
        let key = stamp(&ufs, namespace).unwrap();
        store(&root, &ufs, namespace, key, &directory);
        let loaded = load(&root, &ufs, namespace, key).unwrap();
        assert!(loaded == directory, "typed directory roundtrip differs");
        let serialized = std::fs::read(entry(&root, key)).unwrap();
        assert!(
            !serialized
                .windows(24)
                .any(|w| w == b"DO_NOT_CACHE_SAMPLE_BODY")
        );
        assert!(
            !serialized
                .windows(8)
                .any(|w| w == directory.metadata_key.to_le_bytes()),
            "access material must not persist"
        );
        let body = encode(&directory).unwrap();
        for end in 0..body.len() {
            assert!(decode(&body[..end], directory.metadata_key).is_none());
        }
        let mut too_many = body.clone();
        too_many[..4].copy_from_slice(&(MAX_RECORDS as u32 + 1).to_le_bytes());
        assert!(decode(&too_many, directory.metadata_key).is_none());
        let mut damaged = serialized.clone();
        *damaged.last_mut().unwrap() ^= 1;
        std::fs::write(entry(&root, key), damaged).unwrap();
        assert!(load(&root, &ufs, namespace, key).is_none());
        for mutant in 0..4 {
            let mut invalid = directory.clone();
            match mutant {
                0 => invalid.files[0].offset = u64::MAX,
                1 => invalid.files[0].path = Some("../escape.wav".into()),
                2 => invalid.records[0].available += 1,
                _ => invalid.files[0].parent = Some(99999),
            };
            overwrite(&root, key, &encode(&invalid).unwrap());
            assert!(
                load(&root, &ufs, namespace, key).is_none(),
                "valid checksum must not accept invalid descriptor {mutant}"
            );
            let fresh = ufs.decode_directory_cached(namespace, &root).unwrap();
            assert!(
                fresh == directory,
                "corrupt cache must fall back to exact source parser"
            );
        }
        let orphan = entry(&root, key).with_extension("tmp-unpublished");
        std::fs::write(&orphan, b"partial uncommitted cache").unwrap();
        assert!(ufs.decode_directory_cached(namespace, &root).unwrap() == directory);
        store(&root, &ufs, namespace, key, &directory);
        let wrong = stamp(&ufs, b"cache namespace B").unwrap();
        assert_ne!(key, wrong);
        assert!(load(&root, &ufs, b"cache namespace B", wrong).is_none());
        let file = OpenOptions::new()
            .write(true)
            .open(entry(&root, key))
            .unwrap();
        file.set_len(ENTRY_LIMIT + MAGIC.len() as u64 + 65).unwrap();
        assert!(load(&root, &ufs, namespace, key).is_none());
    }
    #[test]
    fn rewrite_rename_tail_and_namespace_changes_miss_without_stale_directory() {
        let f = Fixture::new();
        let path = f.0.join("bank.ufs");
        let mut bytes = fixture_bank(&path);
        let root = f.0.join("cache");
        let namespace = b"cache namespace";
        let ufs = Ufs::open(&path).unwrap();
        let directory = ufs.decode_directory_window(namespace, 0).unwrap();
        let key = stamp(&ufs, namespace).unwrap();
        store(&root, &ufs, namespace, key, &directory);
        let replacement = f.0.join("replacement.ufs");
        std::fs::write(&replacement, &bytes).unwrap();
        let file = OpenOptions::new().write(true).open(&replacement).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(ufs.snapshot.modified().unwrap()))
            .unwrap();
        drop(file);
        std::fs::rename(&replacement, &path).unwrap();
        let replaced = Ufs::open(&path).unwrap();
        let renamed_key = stamp(&replaced, namespace).unwrap();
        assert!(load(&root, &replaced, namespace, renamed_key).is_none());
        bytes[HEADER_SIZE as usize + 8] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        let changed = Ufs::open(&path).unwrap();
        let changed_key = stamp(&changed, namespace).unwrap();
        assert_ne!(changed_key, key);
        assert!(load(&root, &changed, namespace, changed_key).is_none());
        // A same-size edit with restored mtime still changes bounded fingerprints
        // and (on Unix) ctime; original owner cannot publish a stale stamp.
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(ufs.snapshot.modified().unwrap()))
            .unwrap();
        drop(file);
        let changed = Ufs::open(&path).unwrap();
        assert_ne!(stamp(&changed, namespace).unwrap(), key);
        assert!(stamp(&ufs, namespace).is_none());
    }
    #[test]
    #[ignore = "requires owned bank and dev-drive test-cache directory; typed metadata only"]
    fn owned_bank_persisted_directory_parity_profile() {
        let path = std::env::var_os("KONTRA_PROFILE_BANK").expect("owned bank path");
        let root = PathBuf::from(
            std::env::var_os("KONTRA_PROFILE_CACHE_DIR")
                .expect("explicit dev-drive test-cache directory"),
        );
        let namespace = super::super::super::access::Namespaces::native().metadata;
        let ufs = Ufs::open(Path::new(&path)).unwrap();
        let key = stamp(&ufs, &namespace).unwrap();
        if std::env::var_os("KONTRA_PROFILE_CACHE_LOAD_ONLY").is_some() {
            let start = std::time::Instant::now();
            let directory =
                load(&root, &ufs, &namespace, key).expect("existing verified cache hit");
            println!(
                "UFS_PERSISTED_DIRECTORY_PROFILE {}",
                serde_json::json!({"schema":1,"load_only":true,"cache_load_ms":start.elapsed().as_secs_f64()*1000.,"members":directory.files.len(),"records":directory.records.len(),"cached_bytes":std::fs::metadata(entry(&root,key)).unwrap().len(),"source_scan":false,"payloads_cached":0,"validation":"source stamp, checksum, typed structure; exact parser parity established by population run"})
            );
            return;
        }
        let start = std::time::Instant::now();
        let direct = ufs.decode_directory_window(&namespace, 0).unwrap();
        let source_ms = start.elapsed().as_secs_f64() * 1000.;
        let start = std::time::Instant::now();
        store(&root, &ufs, &namespace, key, &direct);
        let store_ms = start.elapsed().as_secs_f64() * 1000.;
        let mut load_ms = Vec::new();
        for _ in 0..5 {
            let fresh = Ufs::open(Path::new(&path)).unwrap();
            let start = std::time::Instant::now();
            let cached = load(&root, &fresh, &namespace, key).expect("persisted hit");
            load_ms.push(start.elapsed().as_secs_f64() * 1000.);
            assert!(cached == direct, "owned directory parity differs");
        }
        println!(
            "UFS_PERSISTED_DIRECTORY_PROFILE {}",
            serde_json::json!({"schema":1,"load_only":false,"source_ms":source_ms,"store_ms":store_ms,"load_ms":load_ms,"members":direct.files.len(),"records":direct.records.len(),"cached_bytes":std::fs::metadata(entry(&root,key)).unwrap().len(),"exact_parity":true,"payloads_cached":0,"scope":"fresh file owners; ordered process/OS-warm metadata comparison; no cache flush; first-ever scan still required"})
        );
    }
}
