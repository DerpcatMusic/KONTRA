//! A decoded artwork cache, indexed by executable identity and content hash.
use mui::prelude::Image;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};

fn root() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("kontra/stock-art-v6"))
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

pub(super) struct Art {
    pub(super) main: [Image; 4],
    pub(super) font13: Option<Image>,
    pub(super) button: Option<Image>,
    pub(super) font12: Option<Image>,
    pub(super) field: Option<Image>,
    pub(super) outline: Option<Vec<u8>>,
}

pub(super) enum Cached {
    Images(Box<Art>),
    Missing,
}

pub(super) fn load(path: &Path, dims: &[(u32, u32); 8]) -> Option<Cached> {
    load_at(&root()?, path, dims)
}

fn load_at(root: &Path, path: &Path, dims: &[(u32, u32); 8]) -> Option<Cached> {
    let index = index(root, path)?;
    if !matches!(index.metadata().ok()?.len(), 7 | 64) {
        return None;
    }
    let mut hash = String::new();
    std::fs::File::open(index)
        .ok()?
        .take(65)
        .read_to_string(&mut hash)
        .ok()?;
    if hash == "missing" {
        return Some(Cached::Missing);
    }
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
    Some(Cached::Images(Box::new(Art {
        main: [image(0)?, image(1)?, image(2)?, image(3)?],
        font13: image(4),
        button: image(5),
        font12: image(6),
        field: image(7),
        outline: {
            let path = dir.join("outline.ttf");
            path.metadata()
                .ok()
                .filter(|m| m.len() == 4_000_340)
                .and_then(|_| std::fs::read(path).ok())
                .filter(|bytes| super::stock::valid_outline(bytes))
        },
    })))
}

/// Record only a complete, stable scan; bump the cache version when fingerprints change.
pub(super) fn missing(path: &Path) {
    if let Some(root) = root()
        && std::fs::create_dir_all(&root).is_ok()
    {
        write_index(&root, path, "missing");
    }
}

pub(super) fn save(path: &Path, hash: &str, images: &Art) {
    let Some(root) = root() else { return };
    save_at(&root, path, hash, images);
}

fn save_at(root: &Path, path: &Path, hash: &str, images: &Art) {
    let dir = root.join(hash);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let extra = [
        (4, images.font13.as_ref()),
        (5, images.button.as_ref()),
        (6, images.font12.as_ref()),
        (7, images.field.as_ref()),
    ];
    let present = extra.into_iter().filter_map(|(i, image)| Some((i, image?)));
    for (i, image) in images.main.iter().enumerate().chain(present) {
        let target = dir.join(format!("{i}.rgba"));
        let temporary = dir.join(format!("{i}.{}.tmp", std::process::id()));
        if std::fs::write(&temporary, &image.rgba).is_err()
            || std::fs::rename(temporary, target).is_err()
        {
            return;
        }
    }
    if let Some(font) = &images.outline {
        let target = dir.join("outline.ttf");
        let temporary = dir.join(format!("outline.{}.tmp", std::process::id()));
        if std::fs::write(&temporary, font).is_err() || std::fs::rename(temporary, target).is_err()
        {
            return;
        }
    }
    write_index(root, path, hash);
}

fn write_index(root: &Path, path: &Path, value: &str) {
    if let Some(index) = index(root, path) {
        let temporary = index.with_extension(format!("{}.tmp", std::process::id()));
        if std::fs::write(&temporary, value).is_ok() {
            let _ = std::fs::rename(temporary, index);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn decoded_cache_is_reused_and_invalidated_when_the_executable_changes() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("kontakt.exe");
        std::fs::write(&exe, b"first").unwrap();
        let image = || mui::prelude::Image::rgba(1, 1, vec![1, 2, 3, 255]).unwrap();
        let images = super::Art {
            main: [image(), image(), image(), image()],
            font13: Some(image()),
            button: Some(image()),
            font12: Some(image()),
            field: Some(image()),
            outline: None,
        };
        let root = dir.path().join("cache");
        super::save_at(&root, &exe, &"a".repeat(64), &images);
        let Some(super::Cached::Images(cached)) = super::load_at(&root, &exe, &[(1, 1); 8]) else {
            panic!("no cached images")
        };
        assert_eq!(&*cached.main[0].rgba, &[1, 2, 3, 255]);
        assert_eq!(
            cached.font13.as_ref().map(|font| &*font.rgba),
            Some(&[1, 2, 3, 255][..])
        );
        assert_eq!(
            cached.font12.as_ref().map(|font| &*font.rgba),
            Some(&[1, 2, 3, 255][..])
        );
        assert_eq!(
            cached.field.as_ref().map(|field| &*field.rgba),
            Some(&[1, 2, 3, 255][..])
        );
        let fallback = dir.path().join("fallback.exe");
        std::fs::write(&fallback, b"no font13").unwrap();
        let without_font = super::Art {
            main: [image(), image(), image(), image()],
            font13: None,
            button: Some(image()),
            font12: Some(image()),
            field: None,
            outline: None,
        };
        super::save_at(&root, &fallback, &"b".repeat(64), &without_font);
        let Some(super::Cached::Images(cached)) = super::load_at(&root, &fallback, &[(1, 1); 8])
        else {
            panic!("base artwork must remain available without the optional font");
        };
        assert!(cached.font13.is_none());
        assert!(cached.field.is_none());
        assert_eq!(
            cached.button.as_ref().map(|image| &*image.rgba),
            Some(&[1, 2, 3, 255][..])
        );
        let unsupported = dir.path().join("other.exe");
        std::fs::write(&unsupported, b"unsupported").unwrap();
        super::write_index(&root, &unsupported, "missing");
        assert!(matches!(
            super::load_at(&root, &unsupported, &[(1, 1); 8]),
            Some(super::Cached::Missing)
        ));
        std::fs::write(&unsupported, b"new version").unwrap();
        assert!(super::load_at(&root, &unsupported, &[(1, 1); 8]).is_none());
        std::fs::write(&exe, b"changed executable").unwrap();
        assert!(super::load_at(&root, &exe, &[(1, 1); 8]).is_none());
    }
}
