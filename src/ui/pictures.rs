//! A script interface's pictures, from the library: loose `Resources/pictures`
//! folders near the instrument, else its resource container (`.nkr`).
//!
//! ponytail: the UI reads these itself because the v2 loader hands over
//! interfaces with only loose-folder `.txt` metadata; move this into the
//! loader (or behind a `Core` resource call) when it reads containers.

use super::ir_view::Picture;
use moose::mui::mui::scene::Image;
use sampler_ui_ir as ir;
use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Where an instrument's resources come from.
pub struct Source {
    /// Loose files by lowercase library-relative path (`resources/pictures/x.png`).
    files: HashMap<String, PathBuf>,
    /// Containers not opened yet, nearest first.
    containers: Vec<PathBuf>,
    open: Vec<(File, ni_file::nkr::Archive)>,
    key: Option<Option<Arc<dyn ni_file::nis::LibraryKey>>>,
    instrument: PathBuf,
}

impl Source {
    pub fn of(instrument: &Path) -> Self {
        let entries = |dir: &Path| -> Vec<PathBuf> {
            let mut v: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
            v.sort();
            v
        };
        let named = |p: &Path, name: &str| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name));
        let nkr = |p: &PathBuf| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkr"));
        let (mut files, mut containers) = (HashMap::new(), Vec::new());
        // Up to four folders up, further only while nothing was found.
        for (depth, folder) in instrument.ancestors().skip(1).take(8).enumerate() {
            if depth >= 4 && !(files.is_empty() && containers.is_empty()) {
                break;
            }
            for res in entries(folder).into_iter().filter(|p| named(p, "Resources")) {
                for pics in entries(&res).into_iter().filter(|p| named(p, "pictures")) {
                    for f in entries(&pics) {
                        let name = f.file_name().unwrap().to_string_lossy().to_lowercase();
                        files.entry(format!("resources/pictures/{name}")).or_insert(f);
                    }
                }
            }
            let here = entries(folder);
            containers.extend(here.iter().filter(|p| nkr(p)).cloned());
            // Some libraries keep their container with the samples.
            for sub in here.iter().filter(|p| p.is_dir()) {
                containers.extend(entries(sub).into_iter().filter(nkr));
            }
        }
        containers.dedup();
        Self { files, containers, open: Vec::new(), key: None, instrument: instrument.into() }
    }

    /// The bytes at library-relative `path`, if the library has them.
    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        if let Some(f) = self.files.get(&path.to_lowercase()) {
            return std::fs::read(f).ok();
        }
        for n in 0.. {
            while n == self.open.len() {
                if self.containers.is_empty() {
                    return None;
                }
                let at = self.containers.remove(0);
                if let Some(open) = File::open(&at).ok().and_then(|mut f| Some((ni_file::nkr::Archive::read_index(&mut f).ok()?, f))) {
                    self.open.push((open.1, open.0));
                }
            }
            let (f, archive) = &mut self.open[n];
            let Ok(Some(entry)) = archive.member(&mut *f, path) else { continue };
            let needs_key = entry.encoded && entry.key_index != 0xff;
            archive.entries.insert(entry.name.to_lowercase(), entry);
            let key = match &self.key {
                _ if !needs_key => None,
                Some(key) => key.clone(),
                None => self.key.insert(sampler_kontakt::library_key(&self.instrument).ok()).clone(),
            };
            return archive.read_entry_with_key(f, path, key.as_deref()).ok();
        }
        None
    }

    /// Fill in what the loader could not: each image's `.txt` layout and
    /// frame size, read from the file header without decoding pixels.
    pub fn describe(&mut self, face: &mut ir::Interface) {
        for a in &mut face.assets {
            let ir::AssetKind::Image(meta) = &mut a.kind else { continue };
            let txt = a.path.rsplit_once('.').map_or(a.path.clone(), |(stem, _)| format!("{stem}.txt"));
            if let Some(text) = self.read(&txt) {
                *meta = sampler_ksp::ui::picture_meta(&String::from_utf8_lossy(&text));
            }
            if let Some((w, h)) = self.read(&a.path).as_deref().and_then(dimensions) {
                let n = meta.frames.max(1);
                meta.size = Some(match meta.axis {
                    ir::Orientation::Vertical => ir::Size { width: w, height: h / n },
                    ir::Orientation::Horizontal => ir::Size { width: w / n, height: h },
                });
            }
        }
    }

    /// `asset` decoded and cut into its frames.
    pub fn load(&mut self, asset: &ir::Asset) -> Option<Arc<Picture>> {
        let ir::AssetKind::Image(meta) = &asset.kind else { return None };
        let image = crate::artwork::decode(&self.read(&asset.path)?)?;
        let n = meta.frames.max(1);
        let vertical = meta.axis == ir::Orientation::Vertical;
        let (fw, fh) = if vertical { (image.width, image.height / n) } else { (image.width / n, image.height) };
        let frames: Vec<_> = (0..n).map_while(|f| if vertical { crop(&image, 0, f * fh, fw, fh) } else { crop(&image, f * fw, 0, fw, fh) }).collect();
        (!frames.is_empty()).then(|| Arc::new(Picture { frames }))
    }
}

/// Width and height from a PNG or JPEG header.
fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG") {
        let be = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
        return Some((be(16)?, be(20)?));
    }
    // ponytail: JPEG control pictures are rare; decode for their size.
    crate::artwork::decode(bytes).map(|i| (i.width, i.height))
}

fn crop(image: &Image, x: u32, y: u32, w: u32, h: u32) -> Option<Arc<Image>> {
    if w == 0 || h == 0 || x.checked_add(w)? > image.width || y.checked_add(h)? > image.height {
        return None;
    }
    if (x, y, w, h) == (0, 0, image.width, image.height) {
        return Some(Arc::new(image.clone()));
    }
    let stride = image.width as usize * 4;
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for row in y as usize..(y + h) as usize {
        let at = row * stride + x as usize * 4;
        rgba.extend_from_slice(&image.rgba[at..at + w as usize * 4]);
    }
    Image::rgba(w, h, rgba).map(Arc::new)
}

#[cfg(test)]
mod tests {
    #[test]
    fn png_header_gives_the_size() {
        let mut head = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        head.extend(300u32.to_be_bytes());
        head.extend(62u32.to_be_bytes());
        assert_eq!(super::dimensions(&head), Some((300, 62)));
    }
}
