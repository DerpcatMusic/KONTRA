//! A script interface's pictures, decoded from the library the loader
//! already read their layouts from.

use super::ir_view::Picture;
use moose::mui::mui::scene::Image;
use sampler_ui_ir as ir;
use sampler_uvi::ResourceError;
use std::{path::Path, sync::Arc};

/// Where an instrument's resources come from.
pub struct Source {
    kontakt: sampler_kontakt::Resources,
    uvi: Option<sampler_uvi::Resources>,
    #[cfg(feature = "shots")]
    pub scan: Scan,

}

#[cfg(feature = "shots")]
#[derive(Clone, Copy, Default)]
pub struct Scan {
    pub lookups: usize,
    pub lookup_ok: usize,
    pub lookup_missing: usize,
    pub lookup_invalid: usize,
    pub lookup_ambiguous: usize,
    pub lookup_corrupt: usize,
    pub lookup_limit: usize,
    pub lookup_read: usize,
    pub lookup_unavailable: usize,
    pub decodes: usize,
    pub decode_ok: usize,
    pub fonts: usize,
    pub font_ok: usize,

}

impl Source {
    pub fn of(instrument: &Path) -> Self {
        let uvi = instrument
            .ancestors()
            .any(|p| {
                p.extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("ufs") || e.eq_ignore_ascii_case("uvip")
                })
            })
            .then(|| sampler_uvi::Resources::of(instrument));
        Self {
            kontakt: sampler_kontakt::Resources::of(instrument),
            uvi,
            #[cfg(feature = "shots")]
            scan: Scan::default(),
        }

    }

    pub(crate) fn read_result(&mut self, path: &str) -> Result<Option<Vec<u8>>, ResourceError> {
        let result = if path.is_empty() || path.len() > 4096 || path.contains('\0') {
            Err(ResourceError::InvalidPath)
        } else {
            match &self.uvi {
                Some(uvi) => uvi.read_result(path),
                None => self.kontakt.read_result(path).map_err(kontakt_error),
            }
        };
        #[cfg(feature = "shots")]
        {
            self.scan.lookups += 1;
            match &result {
                Ok(Some(_)) => self.scan.lookup_ok += 1,
                Ok(None) => self.scan.lookup_missing += 1,
                Err(ResourceError::InvalidPath) => self.scan.lookup_invalid += 1,
                Err(ResourceError::Ambiguous) => self.scan.lookup_ambiguous += 1,
                Err(ResourceError::Corrupt) => self.scan.lookup_corrupt += 1,
                Err(ResourceError::Limit) => self.scan.lookup_limit += 1,
                Err(ResourceError::Read) => self.scan.lookup_read += 1,
                Err(ResourceError::Unavailable) => self.scan.lookup_unavailable += 1,
            }
        }
        result
    }
    pub(super) fn native_names(&mut self) -> Vec<String> {
        let mut names = self.kontakt.names("resources/native_ui/");
        names.extend(self.kontakt.names("native_ui/"));
        names.sort();
        names.dedup();
        names
    }
    pub fn font(&mut self, asset: &ir::Asset) -> Option<moose::mui::mui::prelude::Font> {
        let font = self
            .read_result(&asset.path).ok().flatten()
            .and_then(|bytes| moose::mui::mui::prelude::Font::new(bytes).ok());
        #[cfg(feature = "shots")]
        {
            self.scan.fonts += 1;
            self.scan.font_ok += usize::from(font.is_some());
        }
        font

    }

    /// Compatibility probe: one frame, never an eagerly decoded strip.
    pub fn load(&mut self, asset: &ir::Asset) -> Option<Arc<Picture>> {
        self.load_frame(asset, 0, [u32::MAX; 2], None, || false)
    }
    pub(super) fn load_frame(
        &mut self,
        asset: &ir::Asset,
        frame: usize,
        target: [u32; 2],
        window: Option<[u32; 4]>,
        canceled: impl Fn() -> bool,
    ) -> Option<Arc<Picture>> {
        if canceled() {return None;}
        let bytes = self.read_result(&asset.path).ok().flatten()?;
        if canceled() {return None;}
        let meta = match asset.kind {
            ir::AssetKind::Image(m) => m,
            ir::AssetKind::BitmapFont => ir::ImageMeta::default(),
            _ => return None,

        };
        let image = super::picture_decode::decode(&bytes, meta, frame, target, window, canceled);

        #[cfg(feature = "shots")]
        {
            self.scan.decodes += 1;
            self.scan.decode_ok += usize::from(image.is_some());
        }

        let image = image?;
        if matches!(asset.kind, ir::AssetKind::BitmapFont) {
            let sidecar = format!("{}.txt", asset.path.rsplit_once('.')?.0);
            let text = self.read_result(&sidecar).ok().flatten()?;
            if sampler_ksp::ui::picture_meta(&String::from_utf8_lossy(&text)).frames != 1 {
                return None;
            }
            return Some(Arc::new(Picture::new(font_frames(&image)?)));
        }
        Some(Arc::new(Picture::prepared(
            Arc::new(image),
            frame,
            meta.frames.max(1) as usize,
            window,
        )))

    }
}

pub(super) fn resource_category(error: ResourceError) -> &'static str {
    match error {
        ResourceError::InvalidPath => "lookup-invalid",
        ResourceError::Ambiguous => "lookup-ambiguous",
        ResourceError::Corrupt => "lookup-corrupt",
        ResourceError::Limit => "lookup-limit",
        ResourceError::Read => "lookup-read",
        ResourceError::Unavailable => "lookup-unavailable",
    }
}

// Only fixed provider reasons are classified; paths and error payloads stay private.
fn kontakt_error(error: sampler_kontakt::LoadError) -> ResourceError {
    use sampler_kontakt::LoadError as E;
    match error {
        E::Io { .. } => ResourceError::Read,
        E::Decode { .. } => ResourceError::Corrupt,
        E::Invalid { reason, .. } => match reason.as_str() {
            "Invalid library-relative resource path" => ResourceError::InvalidPath,
            "Ambiguous loose resource name" | "Ambiguous resource name" => ResourceError::Ambiguous,
            "Resource exceeds 32 MiB" => ResourceError::Limit,
            "Resource container could not be indexed" => ResourceError::Unavailable,
            _ => ResourceError::Corrupt,
        },
        E::Staged { source, .. } => kontakt_error(*source),
        E::Access { .. } | E::Lower(_) | E::Canceled => ResourceError::Unavailable,
    }
}

pub(crate) fn crop(image: &Image, x: u32, y: u32, w: u32, h: u32) -> Option<Arc<Image>> {
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

pub(crate) fn font_frames(image: &Image) -> Option<Vec<Arc<Image>>> {
    if image.height < 2 {
        return None;
    }
    let starts: Vec<u32> = (0..image.width)
        .filter(|&x| {
            let at = x as usize * 4;
            image.rgba[at..at + 3] == [255, 0, 0]
        })
        .collect();
    if starts.len() != 256 || starts[0] != 0 {
        return None;
    }
    starts
        .iter()
        .enumerate()
        .map(|(n, &x)| {
            crop(
                image,
                x,
                1,
                starts.get(n + 1).copied().unwrap_or(image.width) - x,
                image.height - 1,
            )
        })
        .collect()
}

/// Unicode text is indexed by the font's documented Windows-1252 byte order.
/// Characters outside that alphabet use its authored question-mark glyph.
pub(crate) fn font_glyph(c: char) -> usize {
    const EXTENDED: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match c as u32 {
        0..=127 | 160..=255 => c as usize,
        _ => EXTENDED
            .iter()
            .position(|&glyph| glyph == c)
            .map_or(b'?' as usize, |n| n + 128),
    }
}

/// Wake signature for authored resource preparation, independent of Logs visibility.
pub(crate) fn revision() -> u64 {
    super::picture_worker::revision()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_lookup_distinguishes_absence_invalid_path_and_failed_authority() {
        let dir = std::env::temp_dir().join(format!("kontra-shared-resource-result-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Resources")).unwrap();
        std::fs::write(dir.join("Resources/fixture.bin"), b"synthetic").unwrap();
        for instrument in ["fixture.nki", "fixture.uvip"] {
            let mut source = Source::of(&dir.join(instrument));
            assert_eq!(source.read_result("Resources/fixture.bin").unwrap(), Some(b"synthetic".to_vec()));
            assert_eq!(source.read_result("Resources/absent.bin").unwrap(), None);
            assert_eq!(source.read_result("../foreign.bin"), Err(ResourceError::InvalidPath));
            assert_eq!(source.read_result("Resources/\0fixture.bin"), Err(ResourceError::InvalidPath));
            assert_eq!(source.read_result(&"x".repeat(4097)), Err(ResourceError::InvalidPath));
            #[cfg(feature = "shots")]
            assert_eq!((source.scan.lookups, source.scan.lookup_ok, source.scan.lookup_missing, source.scan.lookup_invalid), (5, 1, 1, 3));
        }
        std::fs::write(dir.join("broken.nkr"), b"not an archive").unwrap();
        let mut source = Source::of(&dir.join("fixture.nki"));
        assert_eq!(source.read_result("Resources/absent.bin"), Err(ResourceError::Unavailable));
        #[cfg(feature = "shots")]
        assert_eq!((source.scan.lookup_missing, source.scan.lookup_unavailable), (0, 1));
        let mut source = Source::of(&dir.join("broken.ufs/fixture.uvip"));
        assert_eq!(source.read_result("Resources/absent.bin"), Err(ResourceError::Unavailable));
        #[cfg(feature = "shots")]
        assert_eq!((source.scan.lookup_missing, source.scan.lookup_unavailable), (0, 1));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
