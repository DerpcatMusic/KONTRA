//! On-disk Luau bytecode cache for control-thread script loads.
//!
//! Keyed by the BLAKE3 of (crate version, source bytes), so a Luau upgrade
//! (which bumps the crate) or an edited script misses. Each file carries a
//! header with its own BLAKE3 over the bytecode; a truncated, edited or
//! otherwise corrupt file is treated as a miss, recompiled and rewritten.
//! Writes go to a temp file and rename into place, so readers never see a
//! partial file. Cache I/O failures never fail a load.
//!
//! Not for the audio thread: this does file I/O. Callers run it at load time.

use std::{
    fs,
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 8] = b"KSLUAU01";
const HEADER: usize = MAGIC.len() + 32;

/// Cache directory: `KONTRA_SCRIPT_CACHE` if set (empty disables), else
/// `$XDG_CACHE_HOME/kontra/luau` or `$HOME/.cache/kontra/luau`.
pub fn default_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("KONTRA_SCRIPT_CACHE") {
        return (!dir.is_empty()).then(|| PathBuf::from(dir));
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("kontra").join("luau"))
}

/// Luau bytecode for `source`: read from `dir` when a valid entry exists,
/// otherwise compiled and (best effort) stored there. `dir = None` compiles
/// without touching the disk.
pub fn bytecode(dir: Option<&Path>, source: &[u8]) -> mlua::Result<Vec<u8>> {
    let Some(dir) = dir else {
        return compile(source);
    };
    let key = blake3::Hasher::new()
        .update(env!("CARGO_PKG_VERSION").as_bytes())
        .update(&[0])
        .update(source)
        .finalize();
    let path = dir.join(format!("{}.luauc", key.to_hex()));
    if let Some(bytes) = read_valid(&path) {
        return Ok(bytes);
    }
    let bytes = compile(source)?;
    store(dir, &path, &bytes);
    Ok(bytes)
}

fn compile(source: &[u8]) -> mlua::Result<Vec<u8>> {
    crate::vm::compiler().compile(source)
}

fn read_valid(path: &Path) -> Option<Vec<u8>> {
    let file = fs::read(path).ok()?;
    if file.len() < HEADER || &file[..MAGIC.len()] != MAGIC {
        return None;
    }
    let (sum, body) = file[MAGIC.len()..].split_at(32);
    (blake3::hash(body).as_bytes() == sum).then(|| body.to_vec())
}

fn store(dir: &Path, path: &Path, bytes: &[u8]) {
    let write = || -> std::io::Result<()> {
        fs::create_dir_all(dir)?;
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        let mut file = Vec::with_capacity(HEADER + bytes.len());
        file.extend_from_slice(MAGIC);
        file.extend_from_slice(blake3::hash(bytes).as_bytes());
        file.extend_from_slice(bytes);
        fs::write(&tmp, file)?;
        fs::rename(&tmp, path)
    };
    // Best effort: a read-only or full cache dir costs only the compile.
    let _ = write();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ks-cache-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn miss_stores_and_hit_returns_identical_bytes() {
        let dir = scratch("hit");
        let src = b"return 1 + 2";
        let first = bytecode(Some(&dir), src).unwrap();
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(bytecode(Some(&dir), src).unwrap(), first);
        assert_eq!(first, bytecode(None, src).unwrap());
    }

    #[test]
    fn corrupt_entry_is_recompiled_and_repaired() {
        let dir = scratch("corrupt");
        let src = b"local a = 7 return a";
        let good = bytecode(Some(&dir), src).unwrap();
        let path = fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        let mut bad = fs::read(&path).unwrap();
        let last = bad.len() - 1;
        bad[last] ^= 0xff;
        fs::write(&path, &bad).unwrap();
        assert!(read_valid(&path).is_none());
        assert_eq!(bytecode(Some(&dir), src).unwrap(), good);
        assert!(read_valid(&path).is_some());
        fs::write(&path, b"junk").unwrap();
        assert_eq!(bytecode(Some(&dir), src).unwrap(), good);
    }

    #[test]
    fn unwritable_dir_still_compiles() {
        let file = std::env::temp_dir().join(format!("ks-cache-file-{}", std::process::id()));
        fs::write(&file, b"not a dir").unwrap();
        assert!(bytecode(Some(&file.join("sub")), b"return 3").is_ok());
        let _ = fs::remove_file(&file);
    }
}
