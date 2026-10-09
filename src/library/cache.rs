//! Persistent catalog metadata only; no scripts, samples or decoded instruments.
use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) enum Metadata {
    Product(String, String),
    Bank(Vec<String>, Option<String>, [u8; 16]),
    Snapshot(String),
    Instrument(String),
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    stamp: (u64, u128),
    metadata: Option<Metadata>,
}

#[derive(Default, Debug)]
pub(super) struct Stats {
    pub changed: usize,
    pub reads: usize,
    pub reused: usize,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Cache {
    version: u32,
    entries: BTreeMap<PathBuf, Entry>,
    #[serde(skip)]
    seen: BTreeSet<PathBuf>,
    #[serde(skip)]
    dirty: bool,
    #[serde(skip)]
    pub stats: Stats,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            version: 3,
            entries: BTreeMap::new(),
            seen: BTreeSet::new(),
            dirty: false,
            stats: Stats::default(),
        }
    }
}

// port from v1 0cb7a8a0:src/cache.rs: metadata validation never reads payloads.
fn stamp(path: &Path) -> Option<(u64, u128)> {
    let meta = path.metadata().ok()?;
    Some((
        meta.len(),
        meta.modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos(),
    ))
}

impl Cache {
    pub fn path() -> Option<PathBuf> {
        if cfg!(test) || std::env::var_os("KONTRA_SCAN_ACTIVE").is_some() {
            return None;
        }
        Some(dirs::cache_dir()?.join("kontra/library-index.json"))
    }

    pub fn load(path: Option<&Path>) -> Self {
        path.filter(|p| p.metadata().is_ok_and(|m| m.len() <= 32 << 20))
            .and_then(|p| serde_json::from_slice::<Self>(&std::fs::read(p).ok()?).ok())
            .filter(|c| c.version == 3)
            .unwrap_or_default()
    }

    pub fn observe(&mut self, path: &Path) -> Option<()> {
        let stamp = stamp(path)?;
        self.seen.insert(path.into());
        if self.entries.get(path).is_some_and(|e| e.stamp == stamp) {
            self.stats.reused += 1;
        } else {
            self.entries.insert(
                path.into(),
                Entry {
                    stamp,
                    metadata: None,
                },
            );
            self.stats.changed += 1;
            self.dirty = true;
        }
        Some(())
    }

    pub fn memo(
        &mut self,
        path: &Path,
        read: impl FnOnce() -> Option<Metadata>,
    ) -> Option<Metadata> {
        self.observe(path)?;
        if let Some(metadata) = &self.entries.get(path)?.metadata {
            return Some(metadata.clone());
        }
        self.stats.reads += 1;
        let metadata = read()?;
        if self
            .entries
            .get(path)
            .is_some_and(|e| Some(e.stamp) == stamp(path))
        {
            self.entries.get_mut(path)?.metadata = Some(metadata.clone());
            self.dirty = true;
        }
        Some(metadata)
    }

    pub fn save(&mut self, path: Option<&Path>) -> std::io::Result<()> {
        let before = self.entries.len();
        self.entries.retain(|p, _| self.seen.contains(p));
        self.dirty |= before != self.entries.len();
        let Some(path) = path.filter(|_| self.dirty) else {
            return Ok(());
        };
        let dir = path
            .parent()
            .ok_or_else(|| std::io::Error::other("index has no directory"))?;
        // port from v1 0cb7a8a0:src/cache.rs: unique temporary + atomic rename.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let tmp = path.with_extension(format!(
            "tmp{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let written = std::fs::create_dir_all(dir)
            .and_then(|()| std::fs::write(&tmp, serde_json::to_vec(self)?))
            .and_then(|()| std::fs::rename(&tmp, path));
        if written.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        written
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_metadata_skips_reads_and_invalidates_changed_removed_and_corrupt_files() {
        let dir = std::env::temp_dir().join(format!("kontra-catalog-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (file, index) = (dir.join("Library.ufs"), dir.join("index.json"));
        std::fs::write(&file, b"catalog").unwrap();
        let mut cache = Cache::default();
        let value = Metadata::Bank(
            vec!["Presets/Piano.uvip".into()],
            Some("Content access needed".into()),
            [1; 16],
        );
        assert_eq!(
            cache.memo(&file, || Some(value.clone())),
            Some(value.clone())
        );
        cache.save(Some(&index)).unwrap();
        let mut cache = Cache::load(Some(&index));
        assert_eq!(
            cache.memo(&file, || panic!("unchanged bank must not be reopened")),
            Some(value)
        );
        assert_eq!((cache.stats.changed, cache.stats.reads), (0, 0));
        std::fs::write(&file, b"changed catalog").unwrap();
        let next = Metadata::Bank(vec!["Presets/Organ.uvip".into()], None, [2; 16]);
        assert_eq!(cache.memo(&file, || Some(next.clone())), Some(next));
        assert_eq!((cache.stats.changed, cache.stats.reads), (1, 1));
        let changed = std::fs::metadata(&file).unwrap().modified().unwrap()
            + std::time::Duration::from_secs(1);
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(changed))
            .unwrap();
        cache
            .memo(&file, || Some(Metadata::Bank(Vec::new(), None, [3; 16])))
            .unwrap();
        assert_eq!(
            (cache.stats.changed, cache.stats.reads),
            (2, 2),
            "mtime invalidates even when the size stays the same"
        );
        cache.save(Some(&index)).unwrap();
        std::fs::remove_file(&file).unwrap();
        let mut cache = Cache::load(Some(&index));
        assert!(
            cache
                .memo(&file, || panic!("removed bank must not be opened"))
                .is_none()
        );
        cache.save(Some(&index)).unwrap();
        assert!(Cache::load(Some(&index)).entries.is_empty());
        std::fs::write(&index, b"{broken").unwrap();
        assert!(Cache::load(Some(&index)).entries.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
