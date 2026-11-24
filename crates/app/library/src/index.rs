//! The library index: every library the player has, kept between sessions
//! as the catalog's source of truth. A scan only updates it. A library that
//! cannot be read stays listed, offline. A library found by its
//! [`identity`] in another folder is relocated, not added twice.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Catalog, Library, Root};

/// Bumped when what is kept changes meaning; an older file is dropped.
const VERSION: u32 = 1;
/// How much of each end of a file its fingerprint reads.
const CHUNK: u64 = 64 * 1024;

#[derive(Default, Serialize, Deserialize)]
struct File {
    version: u32,
    libraries: Vec<Library>,
}

/// `path`'s modification time in nanoseconds.
#[must_use]
pub fn mtime(path: &Path) -> Option<u128> {
    let time = path.metadata().ok()?.modified().ok()?;
    Some(time.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos())
}

/// Every stamped folder still has the time it was stamped with.
pub(crate) fn unchanged(stamps: &[(PathBuf, u128)]) -> bool {
    stamps.iter().all(|(p, t)| mtime(p) == Some(*t))
}

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}
const FNV: u64 = 0xcbf2_9ce4_8422_2325;

/// A file's size and a hash of its first and last 64 KiB.
fn fingerprint(path: &Path) -> Option<(u64, u64)> {
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    let mut buf = Vec::new();
    (&mut f).take(CHUNK).read_to_end(&mut buf).ok()?;
    if size > CHUNK {
        f.seek(SeekFrom::Start(size - CHUNK.min(size - CHUNK)))
            .ok()?;
        f.take(CHUNK).read_to_end(&mut buf).ok()?;
    }
    Some((size, fnv(FNV, &buf)))
}

fn relative(dir: &Path, p: &Path) -> PathBuf {
    p.strip_prefix(dir).unwrap_or(p).to_path_buf()
}

fn under(dir: &Path, rel: &Path) -> PathBuf {
    if rel.as_os_str().is_empty() {
        dir.to_path_buf()
    } else {
        dir.join(rel)
    }
}

/// Who a library is, whatever folder it sits in or is renamed to:
/// - a `.nicnt` library: `nicnt:{product}|{company}`;
/// - a lone bank: `bank:{ext}:{size}:{hash of its first/last 64 KiB}`;
/// - any other folder: `dir:{hash of its presets' relative paths}:{size and
///   hash of its first preset file}`.
#[must_use]
pub fn identity(dir: &Path, product: Option<&str>, presets: &[PathBuf]) -> String {
    if let Some(product) = product {
        return format!("nicnt:{product}");
    }
    if dir.is_file() {
        let ext = crate::ext(dir);
        let (size, fp) = fingerprint(dir).unwrap_or_default();
        return format!("bank:{ext}:{size}:{fp:016x}");
    }
    let names = presets.iter().fold(FNV, |h, p| {
        let rel = relative(dir, p).to_string_lossy().replace('\\', "/");
        fnv(fnv(h, rel.as_bytes()), &[0])
    });
    let first = presets.iter().find(|p| p.is_file());
    let (size, fp) = first.and_then(|p| fingerprint(p)).unwrap_or_default();
    format!("dir:{names:016x}:{size}:{fp:016x}")
}

/// The libraries saved at `path`, their paths made whole again; none when
/// the file is missing, broken or of another version.
#[must_use]
pub fn load(path: &Path) -> Vec<Library> {
    let Some(file) = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<File>(&b).ok())
        .filter(|f| f.version == VERSION)
    else {
        return Vec::new();
    };
    (file.libraries.into_iter())
        .map(|mut l| {
            let dir = l.dir.clone();
            let whole = |v: &mut Vec<PathBuf>| v.iter_mut().for_each(|p| *p = under(&dir, p));
            whole(&mut l.presets);
            whole(&mut l.art);
            l.stamps.iter_mut().for_each(|(p, _)| *p = under(&dir, p));
            l
        })
        .collect()
}

/// Writes `libraries` to `path`, each one's files relative to its folder,
/// through a temporary file renamed into place.
///
/// # Errors
/// When the file can't be written.
pub fn save(path: &Path, libraries: &[Library]) -> std::io::Result<()> {
    let libraries = (libraries.iter())
        .map(|l| {
            let rel = |v: &[PathBuf]| v.iter().map(|p| relative(&l.dir, p)).collect();
            Library {
                presets: rel(&l.presets),
                art: rel(&l.art),
                stamps: (l.stamps.iter())
                    .map(|(p, t)| (relative(&l.dir, p), *t))
                    .collect(),
                ..l.clone()
            }
        })
        .collect();
    let file = File {
        version: VERSION,
        libraries,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let written =
        std::fs::write(&tmp, serde_json::to_vec(&file)?).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Folds what a scan found into what the index knew, as (old folder, new
/// folder) for each library found moved:
/// - a library found at a new folder, its old one gone, is relocated;
/// - one not found, still under a root, stays listed offline, with why;
/// - one under no root any more (the player removed it) is dropped, as is
///   one whose folder is readable and no longer a library.
pub fn reconcile(
    previous: &[Library],
    catalog: &mut Catalog,
    roots: &[Root],
) -> Vec<(PathBuf, PathBuf)> {
    let mut moved = Vec::new();
    let mut offline = Vec::new();
    for prev in previous {
        if let Some(now) = catalog.libraries.iter_mut().find(|l| l.dir == prev.dir) {
            if now.moved_from.is_empty() {
                now.moved_from.clone_from(&prev.moved_from);
            }
            continue;
        }
        let gone = !prev.dir.exists();
        if gone && !prev.id.is_empty() {
            let elsewhere = (catalog.libraries.iter_mut())
                .find(|l| l.id == prev.id && !previous.iter().any(|p| p.dir == l.dir));
            if let Some(now) = elsewhere {
                now.moved_from.clone_from(&prev.moved_from);
                now.moved_from.push(prev.dir.clone());
                moved.push((prev.dir.clone(), now.dir.clone()));
                continue;
            }
        }
        let Some(root) = roots.iter().find(|r| prev.dir.starts_with(&r.path)) else {
            continue;
        };
        let status = catalog.roots.iter().find(|s| s.path == root.path);
        let unreadable = status.and_then(|s| s.error.as_ref().map(|_| s.summary()));
        if !gone && unreadable.is_none() {
            continue;
        }
        offline.push(Library {
            offline: Some(unreadable.unwrap_or_else(|| "not found".into())),
            ..prev.clone()
        });
    }
    let mut all = std::mem::take(&mut catalog.libraries);
    all.extend(offline);
    catalog.libraries = crate::unique(all);
    moved
}

/// Whether anything a scan saw changed since: a root's time, an offline
/// library's folder come back (a drive mounted), or an online library's
/// stamped folder. Only stats; run it off the audio and UI threads.
// ponytail: polls with stat, no OS file-watch API (the `notify` crate is
// not in the lockfile); a few thousand stats per check.
#[must_use]
pub fn stale(catalog: &Catalog) -> bool {
    catalog.roots.iter().any(|r| mtime(&r.path) != r.stamp)
        || catalog.libraries.iter().any(|l| match &l.offline {
            Some(_) => l.dir.exists(),
            None => !l.stamps.is_empty() && !unchanged(&l.stamps),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cache, Progress};

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kontra-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn library(at: &Path) {
        std::fs::create_dir_all(at.join("Pads")).unwrap();
        std::fs::write(at.join("Lead.sfz"), b"<region> sample=a.wav").unwrap();
        std::fs::write(at.join("Pads/Warm.sfz"), b"<region> sample=b.wav").unwrap();
    }

    fn root(path: &Path) -> Vec<Root> {
        vec![Root {
            path: path.to_path_buf(),
            single: false,
            auto: false,
        }]
    }

    fn scan(roots: &[Root], previous: &[Library]) -> Catalog {
        let mut cache = Cache::default();
        let inspect = |_: &Path| {
            Ok(crate::BankListing {
                presets: vec!["000:000 Grand.sf2p".into()],
                protection: crate::ProtectionStatus::Open,
            })
        };
        let mut c =
            crate::scan_incremental(roots, &Progress::default(), &mut cache, &inspect, previous)
                .unwrap();
        reconcile(previous, &mut c, roots);
        c
    }

    #[test]
    fn index_round_trips_and_keeps_relative_presets() {
        let dir = tmp("round");
        library(&dir.join("libs/Synth"));
        let catalog = scan(&root(&dir.join("libs")), &[]);
        assert_eq!(catalog.libraries.len(), 1);
        let file = dir.join("index.json");
        save(&file, &catalog.libraries).unwrap();
        let json = std::fs::read_to_string(&file).unwrap();
        assert!(
            json.contains("\"Lead.sfz\""),
            "presets are relative: {json}"
        );
        assert_eq!(load(&file), catalog.libraries);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_unreadable_library_stays_offline_not_dropped() {
        let dir = tmp("offline");
        let drive = dir.join("drive");
        library(&drive.join("Synth"));
        let roots = root(&drive);
        let before = scan(&roots, &[]);
        // The drive is unmounted: the root itself is gone.
        std::fs::rename(&drive, dir.join("away")).unwrap();
        let after = scan(&roots, &before.libraries);
        assert_eq!(after.libraries.len(), 1);
        let l = &after.libraries[0];
        assert_eq!(l.offline.as_deref(), Some("not found"));
        assert_eq!(
            l.presets, before.libraries[0].presets,
            "presets stay listed"
        );
        // Mounted again: stale, and the next scan brings it back online.
        std::fs::rename(dir.join("away"), &drive).unwrap();
        assert!(stale(&after));
        let back = scan(&roots, &after.libraries);
        assert_eq!(back.libraries[0].offline, None);
        // A root the player removed drops its libraries.
        assert!(scan(&[], &back.libraries).libraries.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_unchanged_library_is_not_walked_again() {
        let dir = tmp("incremental");
        let lib = dir.join("libs/Synth");
        library(&lib);
        let roots = root(&dir.join("libs"));
        let mut known = scan(&roots, &[]).libraries;
        assert!(!stale(&scan(&roots, &[])));
        // A marker only a reuse keeps: the walk is skipped.
        let marker = lib.join("Pads/Marker.sfz");
        known[0].presets.push(marker.clone());
        let again = scan(&roots, &known);
        assert!(again.libraries[0].presets.contains(&marker));
        // A folder that changes is walked again.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(lib.join("Pads/Cold.sfz"), b"<region> sample=c.wav").unwrap();
        assert!(stale(&again));
        let walked = scan(&roots, &again.libraries);
        assert!(!walked.libraries[0].presets.contains(&marker));
        assert!(
            walked.libraries[0]
                .presets
                .contains(&lib.join("Pads/Cold.sfz"))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn identity_survives_a_folder_rename() {
        let dir = tmp("identity");
        let libs = dir.join("libs");
        library(&libs.join("Synth v1.0"));
        std::fs::write(libs.join("Piano.sf2"), vec![7u8; 200_000]).unwrap();
        let first = scan(&root(&libs), &[]);
        std::fs::rename(libs.join("Synth v1.0"), libs.join("Synth (renamed)")).unwrap();
        std::fs::rename(libs.join("Piano.sf2"), libs.join("Grand.sf2")).unwrap();
        let second = scan(&root(&libs), &first.libraries);
        let ids = |c: &Catalog| {
            let mut v: Vec<String> = c.libraries.iter().map(|l| l.id.clone()).collect();
            v.sort();
            v
        };
        assert_eq!(ids(&first).len(), 2);
        assert_eq!(ids(&first), ids(&second));
        assert!(second.libraries.iter().all(|l| l.offline.is_none()));
        assert!(identity(Path::new("/x"), Some("Komplete|NI"), &[]).starts_with("nicnt:"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_moved_library_is_relocated_by_identity() {
        let dir = tmp("relocate");
        let (old, new) = (dir.join("a/HDD/Synth"), dir.join("b/SSD/Vendor/Synth"));
        library(&old);
        let roots = vec![root(&dir.join("a")), root(&dir.join("b"))].concat();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        let before = scan(&roots, &[]);
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::rename(&old, &new).unwrap();
        let mut c = crate::scan_incremental(
            &roots,
            &Progress::default(),
            &mut Cache::default(),
            &|_: &Path| Err(crate::BankError::failed("-")),
            &before.libraries,
        )
        .unwrap();
        let moved = reconcile(&before.libraries, &mut c, &roots);
        assert_eq!(moved, [(old.clone(), new.clone())]);
        assert_eq!(c.libraries.len(), 1, "relocated, not listed twice");
        assert_eq!(c.libraries[0].dir, new);
        assert_eq!(c.libraries[0].moved_from, [old]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
