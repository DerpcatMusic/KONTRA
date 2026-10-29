//! A decoded artwork cache, indexed by executable identity and content hash.
use mui::prelude::Image;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};

fn root() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("kontra/stock-art-v1"))
}

fn index(root: &Path, path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;
    let meta = canonical.metadata().ok()?;
    let mut h = std::hash::DefaultHasher::new();
    canonical.hash(&mut h);
    meta.len().hash(&mut h);
    meta.modified().ok()?.hash(&mut h);
    Some(root.join(format!("index-{:016x}", h.finish())))
}

pub(super) fn load(path: &Path, dims: &[(u32, u32); 4]) -> Option<[Image; 4]> {
    load_at(&root()?, path, dims)
}

fn load_at(root: &Path, path: &Path, dims: &[(u32, u32); 4]) -> Option<[Image; 4]> {
    let index = index(root, path)?;
    if index.metadata().ok()?.len() != 64 {
        return None;
    }
    let mut hash = String::new();
    std::fs::File::open(index)
        .ok()?
        .take(65)
        .read_to_string(&mut hash)
        .ok()?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let dir = root.join(hash);
    let image = |i: usize| {
        let (w, h) = *dims.get(i)?;
        let expected = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        let path = dir.join(format!("{i}.rgba"));
        if path.metadata().ok()?.len() != expected as u64 {
            return None;
        }
        let mut data = Vec::with_capacity(expected);
        std::fs::File::open(path)
            .ok()?
            .take(expected as u64 + 1)
            .read_to_end(&mut data)
            .ok()?;
        if data.len() != expected {
            return None;
        }
        Image::rgba(w, h, data)
    };
    Some([image(0)?, image(1)?, image(2)?, image(3)?])
}

pub(super) fn save(path: &Path, hash: &str, images: &[Image; 4]) {
    let Some(root) = root() else { return };
    save_at(&root, path, hash, images);
}

fn save_at(root: &Path, path: &Path, hash: &str, images: &[Image; 4]) {
    let dir = root.join(hash);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    for (i, image) in images.iter().enumerate() {
        let target = dir.join(format!("{i}.rgba"));
        let temporary = dir.join(format!("{i}.{}.tmp", std::process::id()));
        if std::fs::write(&temporary, &image.rgba).is_err()
            || std::fs::rename(temporary, target).is_err()
        {
            return;
        }
    }
    if let Some(index) = index(root, path) {
        let temporary = index.with_extension(format!("{}.tmp", std::process::id()));
        if std::fs::write(&temporary, hash).is_ok() {
            let _ = std::fs::rename(temporary, index);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn decoded_cache_is_reused_and_invalidated_when_the_executable_changes() {
        let dir = std::env::temp_dir().join(format!("kontra-stock-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("kontakt.exe");
        std::fs::write(&exe, b"first").unwrap();
        let image = || mui::prelude::Image::rgba(1, 1, vec![1, 2, 3, 255]).unwrap();
        let images = [image(), image(), image(), image()];
        let root = dir.join("cache");
        super::save_at(&root, &exe, &"a".repeat(64), &images);
        let cached = super::load_at(&root, &exe, &[(1, 1); 4]).unwrap();
        assert_eq!(&*cached[0].rgba, &[1, 2, 3, 255]);
        std::fs::write(&exe, b"changed executable").unwrap();
        assert!(super::load_at(&root, &exe, &[(1, 1); 4]).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
