//! A protected bank's content key, searched for once (about 2 s of CPU) and
//! kept: in memory for this process, and in a private file under the user's
//! cache dir for the next one. Both are keyed by the bank's path, size and
//! modification time; a changed bank, or a cache file that can't be read,
//! searches again. A key is never written to a log or an error.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::Result;
use uvi_file::access::recover_content_key;
use uvi_file::ufs::{Directory, Ufs};

/// A bank file's size and modification time in nanoseconds.
type Stamp = (u64, u128);

/// Tags the file layout; another first word is a miss.
const FORMAT: &str = "kontra-uvi-key-1";

pub(crate) fn content_key(path: &Path, ufs: &Ufs, directory: &Directory) -> Result<u64> {
    type Keys = Mutex<HashMap<(PathBuf, Stamp), u64>>;
    static KEYS: OnceLock<Keys> = OnceLock::new();
    let meta = std::fs::metadata(path)?;
    let time = meta.modified()?.duration_since(std::time::UNIX_EPOCH)?;
    let id = (path.to_path_buf(), (meta.len(), time.as_nanos()));
    let keys = KEYS.get_or_init(Keys::default);
    if let Some(&key) = keys.lock().ok().as_ref().and_then(|k| k.get(&id)) {
        return Ok(key);
    }
    let dir = dirs::cache_dir().map(|d| d.join("kontra").join("uvi-keys"));
    let key = match dir.as_deref().and_then(|d| load(d, &id.0, id.1)) {
        Some(key) => key,
        None => {
            let key = recover_content_key(path, ufs, directory)?;
            if let Some(d) = &dir {
                // A cache that can't be written costs the next load a search.
                let _ = save(d, &id.0, id.1, key);
            }
            key
        }
    };
    if let Ok(mut k) = keys.lock() {
        k.insert(id, key);
    }
    Ok(key)
}

/// The cache file for the bank at `path`.
fn file(dir: &Path, path: &Path) -> PathBuf {
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    dir.join(format!("{:016x}", h.finish()))
}

/// The key saved for this bank at this stamp.
pub(crate) fn load(dir: &Path, path: &Path, stamp: Stamp) -> Option<u64> {
    let text = std::fs::read_to_string(file(dir, path)).ok()?;
    let mut f = text.trim_end_matches('\n').splitn(5, '\t');
    let (format, len, time, key) = (f.next()?, f.next()?, f.next()?, f.next()?);
    let saved = (len.parse().ok()?, time.parse().ok()?);
    (format == FORMAT && saved == stamp && f.next()? == path.to_str()?)
        .then(|| u64::from_str_radix(key, 16).ok())?
}

/// Saves `key` readable by this user alone, through a temporary file
/// renamed into place.
pub(crate) fn save(dir: &Path, path: &Path, stamp: Stamp, key: u64) -> std::io::Result<()> {
    let Some(name) = path.to_str() else {
        return Ok(());
    };
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        open.mode(0o600);
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)?;
    let target = file(dir, path);
    let tmp = target.with_extension(format!("tmp{}", std::process::id()));
    let written = open
        .open(&tmp)
        .and_then(|mut f| writeln!(f, "{FORMAT}\t{}\t{}\t{key:016x}\t{name}", stamp.0, stamp.1));
    let done = written.and_then(|()| std::fs::rename(&tmp, &target));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_key_returns_only_for_the_same_bank_and_stamp() {
        let dir = std::env::temp_dir().join(format!("kontra-uvi-keys-{}", std::process::id()));
        let bank = Path::new("/libs/Winds.ufs");
        let stamp = (1_000, 42);
        assert_eq!(load(&dir, bank, stamp), None);
        save(&dir, bank, stamp, 0xDEAD_BEEF_0123).unwrap();
        assert_eq!(load(&dir, bank, stamp), Some(0xDEAD_BEEF_0123));
        assert_eq!(load(&dir, bank, (1_000, 43)), None, "bank changed");
        assert_eq!(load(&dir, Path::new("/libs/Other.ufs"), stamp), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file(&dir, bank))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::write(file(&dir, bank), "garbage").unwrap();
        assert_eq!(load(&dir, bank, stamp), None, "unreadable");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
