//! Last browser index. Cache hits are listings, never sample or reader authority.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Write;

const LIMIT: u64 = 64 << 20;
const ITEMS: usize = 500_000;
const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<u128>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
    header: Option<[u8; 32]>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Fingerprint(BTreeMap<PathBuf, Stamp>);

impl Fingerprint {
    pub(super) fn read(roots: &[Root], reader: Option<&Path>, progress: &Progress) -> Option<Self> {
        let mut stamps = BTreeMap::new();
        for root in roots {
            let walk = walkdir::WalkDir::new(&root.path).follow_links(false).into_iter()
                .filter_entry(|entry| entry.depth() == 0 || !entry.file_type().is_dir()
                    || !entry.file_name().to_string_lossy().starts_with('.')
                        && !entry.file_name().eq_ignore_ascii_case("samples"));
            for entry in walk {
                if progress.canceled() || stamps.len() >= ITEMS { return None; }
                let entry = entry.ok()?;
                let path = entry.path();
                if entry.file_type().is_dir() || crate::creator::is_instrument(path) || import::is_multi(path)
                    || path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("nksn") || ext.eq_ignore_ascii_case("nicnt")
                        || cfg!(feature = "uvi") && ext.eq_ignore_ascii_case("ufs")) {
                    stamps.insert(path.to_owned(), stamp(path)?);
                }
            }
        }
        if cfg!(feature = "uvi") && let Some(reader) = reader { stamps.insert(reader.to_owned(), stamp(reader)?); }
        Some(Self(stamps))
    }
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = path.metadata().ok()?;
    let header = if cfg!(feature = "uvi") && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("ufs")) {
        let mut bytes = [0u8; 320];
        File::open(path).ok()?.read_exact(&mut bytes).ok()?;
        Some(Sha256::digest(bytes).into())
    } else { None };
    #[cfg(unix)]
    let identity = { use std::os::unix::fs::MetadataExt; (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()) };
    Some(Stamp { size: meta.len(), modified: meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|t| t.as_nanos()),
        #[cfg(unix)] identity, header })
}

#[derive(Serialize, Deserialize)]
struct Bank {
    path: PathBuf,
    presets: Vec<UviPreset>,
    status: String,
    bytes: usize,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Index {
    schema: u32,
    importer: String,
    uvi: bool,
    roots: Vec<Root>,
    reader: Option<PathBuf>,
    fingerprint: Fingerprint,
    libraries: Vec<Library>,
    per_root: Vec<usize>,
    files: Vec<PathBuf>,
    snapshots: BTreeMap<PathBuf, Snapshots>,
    banks: Vec<Bank>,
}

fn importer() -> &'static str { option_env!("KONTRA_IMPORT_HASH").unwrap_or("source-direct") }

impl Index {
    pub(super) fn load(path: &Path, roots: &[Root], reader: Option<&Path>) -> Option<Self> {
        let mut bytes = Vec::new();
        File::open(path).ok()?.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > LIMIT { return None; }
        let split = bytes.iter().position(|byte| *byte == b'\n')?;
        let (digest, body) = (std::str::from_utf8(&bytes[..split]).ok()?, &bytes[split + 1..]);
        if digest != format!("{:x}", Sha256::digest(body)) { return None; }
        let index: Self = serde_json::from_slice(body).ok()?;
        if index.schema != SCHEMA || index.importer != importer() || index.uvi != cfg!(feature = "uvi")
            || index.roots != roots || index.reader.as_deref() != reader || index.files.len() > ITEMS
            || index.fingerprint.0.len() > ITEMS || index.libraries.len() > 16_384 || index.banks.len() > 256
            || !cfg!(feature = "uvi") && !index.banks.is_empty() { return None; }
        if index.banks.iter().any(|bank| bank.presets.len() > 16_384 || bank.bytes > 8 << 20
            || bank.presets.iter().any(|preset| preset.source.bank != bank.path || preset.source.member.len() > 4096
                || !preset.source.member.to_ascii_lowercase().ends_with(".uvip")
                || Path::new(&preset.source.member).components().any(|c| !matches!(c, std::path::Component::Normal(_)))))
            || index.banks.iter().map(|bank| bank.bytes).sum::<usize>() > 64 << 20 { return None; }
        Some(index)
    }

    pub(super) fn has_uvi(&self) -> bool { !self.banks.is_empty() }

    pub(super) fn current(&self, roots: &[Root], reader: Option<&Path>, progress: &Progress) -> bool {
        Fingerprint::read(roots, reader, progress).as_ref() == Some(&self.fingerprint)
    }

    pub(super) fn scanned(&self, pending: bool) -> Scanned {
        let mut shelf = Shelf::new(self.libraries.clone());
        shelf.per_root = self.per_root.clone();
        shelf.snapshots = self.snapshots.iter().map(|(path, snapshots)| (path.clone(), snapshots.clone())).collect();
        for bank in &self.banks {
            shelf.uvi.insert(bank.path.clone(), Arc::new(UviBank {
                presets: bank.presets.iter().cloned().map(Arc::new).collect(),
                status: if pending && bank.status.is_empty() { "Checking cached bank…".into() } else { bank.status.clone() },
                bytes: bank.bytes,
            }));
        }
        Scanned { shelf: Arc::new(shelf), files: Arc::new(self.files.clone()), artwork: HashMap::new(), imported: None }
    }

    pub(super) fn store(path: &Path, roots: &[Root], reader: Option<&Path>, fingerprint: Fingerprint, scanned: &Scanned) {
        if scanned.files.len() > ITEMS || scanned.shelf.libraries.len() > 16_384 { return; }
        let index = Self {
            schema: SCHEMA, importer: importer().into(), uvi: cfg!(feature = "uvi"), roots: roots.to_vec(), reader: reader.map(Path::to_owned), fingerprint,
            libraries: scanned.shelf.libraries.clone(), per_root: scanned.shelf.per_root.clone(), files: (*scanned.files).clone(),
            snapshots: scanned.shelf.snapshots.iter().map(|(path, snapshots)| (path.clone(), snapshots.clone())).collect(),
            banks: scanned.shelf.uvi.iter().map(|(path, bank)| Bank { path: path.clone(), presets: bank.presets.iter().map(|p| (**p).clone()).collect(), status: bank.status.clone(), bytes: bank.bytes }).collect(),
        };
        let Ok(body) = serde_json::to_vec(&index) else { return };
        let mut bytes = format!("{:x}\n", Sha256::digest(&body)).into_bytes();
        bytes.extend_from_slice(&body);
        if bytes.len() as u64 > LIMIT { return; }
        let Some(parent) = path.parent() else { return };
        if std::fs::create_dir_all(parent).is_err() { return; }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let tmp = path.with_extension(format!("tmp-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let mut options = std::fs::OpenOptions::new(); options.write(true).create_new(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        let written = options.open(&tmp).and_then(|mut file| file.write_all(&bytes))
            .and_then(|()| std::fs::rename(&tmp, path));
        if written.is_err() { let _ = std::fs::remove_file(tmp); }
    }
}

pub(super) fn path() -> Option<PathBuf> {
    if cfg!(test) { return None; }
    Some(dirs::cache_dir()?.join("kontra/library-index-v1.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (PathBuf, PathBuf, Vec<Root>, Scanned) {
        let dir = super::super::tests::tree(name, &[("Kontakt/Instruments/Piano.nki", "owned fixture")]);
        let path = dir.with_extension("library-index");
        let roots = vec![Root { path: dir.to_string_lossy().into_owned(), single: false }];
        let progress = Progress::default();
        let (shelf, files) = scan(&roots, &progress).unwrap();
        (dir, path, roots, Scanned { shelf: Arc::new(shelf), files: Arc::new(files), artwork: HashMap::new(), imported: None })
    }

    #[test]
    fn index_round_trip_preserves_kontakt_rows_and_rejects_corruption_roots_and_new_files() {
        let (dir, path, roots, mut scanned) = fixture("browser-index-roundtrip");
        let snapshot = dir.join("Kontakt/Instruments/Bright.nksn"); std::fs::write(&snapshot, "snapshot fixture").unwrap();
        Arc::get_mut(&mut scanned.shelf).unwrap().snapshots.insert(scanned.files[0].clone(), Snapshots { instrument: "Piano".into(), paths: vec![snapshot.clone()] });
        let fingerprint = Fingerprint::read(&roots, None, &Progress::default()).unwrap();
        Index::store(&path, &roots, None, fingerprint, &scanned);
        let index = Index::load(&path, &roots, None).unwrap();
        assert_eq!(*index.scanned(true).files, *scanned.files);
        assert_eq!(index.scanned(false).shelf.libraries, scanned.shelf.libraries);
        assert_eq!(index.scanned(false).shelf.snapshots[&scanned.files[0]].paths, [snapshot]);
        assert_eq!(index.scanned(false).shelf.snapshots[&scanned.files[0]].instrument, "Piano");
        assert_eq!(index.scanned(false).shelf.per_root, scanned.shelf.per_root);
        assert!(index.current(&roots, None, &Progress::default()));
        assert!(Index::load(&path, &[], None).is_none());
        assert!(Index::load(&path, &roots, Some(Path::new("/different-reader"))).is_none());
        std::fs::write(dir.join("Kontakt/Instruments/New.nki"), []).unwrap();
        assert!(!index.current(&roots, None, &Progress::default()), "new presets invalidate through their containing directory");
        let mut bytes = std::fs::read(&path).unwrap(); let last = bytes.len() - 2; bytes[last] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        assert!(Index::load(&path, &roots, None).is_none(), "the payload hash rejects corruption before deserialization");
        let _ = std::fs::remove_file(path); let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    #[cfg(feature = "uvi")]
    fn index_bank_identity_and_same_stat_replacement_are_guarded() {
        let (dir, path, roots, mut scanned) = fixture("browser-index-bank");
        let bank = dir.join("Authored.ufs"); super::super::tests::authored_uvi_bank(&bank, 1);
        let scanner = super::super::tests::authored_uvi_scanner(&dir);
        let (name, inventory) = { let mut catalog = lock(&scanner.uvi); catalog.reader_path = Some(dir.join("reader")); catalog.inventory(&bank).unwrap() };
        let mut shelf = Shelf::new(scanned.shelf.libraries.clone());
        shelf.libraries.push(Library { dir: bank.clone(), name, instruments: inventory.presets.len(), ..Default::default() });
        shelf.uvi.insert(bank.clone(), inventory.clone()); scanned.shelf = Arc::new(shelf);
        let reader = dir.join("reader");
        Index::store(&path, &roots, Some(&reader), Fingerprint::read(&roots, Some(&reader), &Progress::default()).unwrap(), &scanned);
        let index = Index::load(&path, &roots, Some(&reader)).unwrap();
        assert_eq!(index.scanned(true).shelf.uvi[&bank].presets[0].source, inventory.presets[0].source);
        assert_eq!(index.scanned(true).shelf.uvi[&bank].status, "Checking cached bank…");
        assert!(index.scanned(false).shelf.uvi[&bank].status.is_empty());
        let before = bank.metadata().unwrap(); super::super::tests::authored_uvi_bank(&bank, 2);
        File::options().write(true).open(&bank).unwrap().set_times(std::fs::FileTimes::new().set_modified(before.modified().unwrap())).unwrap();
        assert_eq!(bank.metadata().unwrap().len(), before.len());
        assert!(!index.current(&roots, Some(&reader), &Progress::default()), "header hash and inode/change-time protect replaced banks even when size/mtime match");
        let _ = std::fs::remove_file(path); let _ = std::fs::remove_dir_all(dir);
    }
}
