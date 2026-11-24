//! What a scan read once (a `.nicnt`'s product, a bank's program list and
//! protection status), kept by path and reused while the file's size and time
//! are unchanged. Only metadata: no instrument, script or sample is ever held.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use kontra_format::ProtectionStatus;
use serde::{Deserialize, Serialize};

use crate::{BankError, BankInspector, BankListing};

/// Bumped when what is kept, or how it is read, changes; an older file is
/// dropped whole.
const VERSION: u32 = 3;

static SAVE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum Metadata {
    Product(String, String),
    NoProduct,
    Programs {
        members: Vec<String>,
        #[serde(default)]
        protection: ProtectionStatus,
    },
    Failed {
        reason: String,
        #[serde(default)]
        protection: ProtectionStatus,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    /// File length and modification time in nanoseconds.
    stamp: (u64, u128),
    metadata: Metadata,
}

/// The scan cache. [`Cache::load`] it before a scan and [`Cache::save`] it
/// after; a missing or broken file is an empty cache.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Cache {
    version: u32,
    entries: BTreeMap<PathBuf, Entry>,
    #[serde(skip)]
    seen: BTreeSet<PathBuf>,
    #[serde(skip)]
    dirty: bool,
    /// How many files were read rather than reused, since it was loaded.
    #[serde(skip)]
    pub reads: usize,
}

fn stamp(path: &Path) -> Option<(u64, u128)> {
    let meta = path.metadata().ok()?;
    let time = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some((meta.len(), time.as_nanos()))
}

impl Cache {
    /// The cache saved at `path`, or an empty one.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|c| c.version == VERSION)
            .unwrap_or_default()
    }

    /// Writes the cache to `path` if it changed, through a temporary file
    /// renamed into place so a crash never leaves half a cache.
    ///
    /// # Errors
    /// When the file can't be written.
    pub fn save(&mut self, path: &Path) -> std::io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        self.version = VERSION;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let seq = SAVE_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = path.with_extension(format!("tmp.{}.{seq}", std::process::id()));
        let written = std::fs::write(&tmp, serde_json::to_vec(self)?)
            .and_then(|()| std::fs::rename(&tmp, path));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        self.dirty = written.is_err();
        written
    }

    /// Drops what the last scan did not touch: removed files.
    pub(crate) fn forget_unseen(&mut self) {
        let before = self.entries.len();
        let seen = std::mem::take(&mut self.seen);
        self.entries.retain(|p, _| seen.contains(p));
        self.dirty |= before != self.entries.len();
    }

    /// Evicts cached entries currently marked [`ProtectionStatus::Protected`]
    /// when `should_recheck(path)` reports that external unlock/key state is
    /// now available, so the next scan re-evaluates them without touching the
    /// bank file's timestamp.
    pub fn invalidate_protected(&mut self, mut should_recheck: impl FnMut(&Path) -> bool) -> usize {
        let before = self.entries.len();
        self.entries.retain(|path, entry| {
            let is_protected = match &entry.metadata {
                Metadata::Programs { protection, .. } | Metadata::Failed { protection, .. } => {
                    protection.is_protected()
                }
                Metadata::Product(..) | Metadata::NoProduct => false,
            };
            !(is_protected && should_recheck(path))
        });
        let dropped = before - self.entries.len();
        if dropped > 0 {
            self.dirty = true;
        }
        dropped
    }

    fn memo(&mut self, path: &Path, read: impl FnOnce() -> Metadata) -> Metadata {
        self.seen.insert(path.to_path_buf());
        let Some(now) = stamp(path) else {
            return read();
        };
        if let Some(entry) = self.entries.get(path).filter(|e| e.stamp == now) {
            return entry.metadata.clone();
        }
        self.reads += 1;
        let metadata = read();
        self.entries.insert(
            path.to_path_buf(),
            Entry {
                stamp: now,
                metadata: metadata.clone(),
            },
        );
        self.dirty = true;
        metadata
    }

    pub(crate) fn product(&mut self, nicnt: &Path) -> Option<(String, String)> {
        match self.memo(nicnt, || {
            crate::product(nicnt).map_or(Metadata::NoProduct, |(n, c)| Metadata::Product(n, c))
        }) {
            Metadata::Product(name, company) => Some((name, company)),
            _ => None,
        }
    }

    pub(crate) fn bank_listing(
        &mut self,
        bank: &Path,
        inspect: BankInspector<'_>,
    ) -> Result<BankListing, BankError> {
        match self.memo(bank, || match inspect(bank) {
            Ok(listing) => Metadata::Programs {
                members: listing.presets,
                protection: listing.protection,
            },
            Err(err) => Metadata::Failed {
                reason: err.reason,
                protection: err.protection,
            },
        }) {
            Metadata::Programs {
                members,
                protection,
            } => Ok(BankListing {
                presets: members,
                protection,
            }),
            Metadata::Failed { reason, protection } => Err(BankError { reason, protection }),
            Metadata::Product(..) | Metadata::NoProduct => Err(BankError {
                reason: "not a bank".to_owned(),
                protection: ProtectionStatus::Open,
            }),
        }
    }
}
