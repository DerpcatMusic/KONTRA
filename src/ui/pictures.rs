//! A script interface's pictures, decoded from the library the loader
//! already read their layouts from.

use super::ir_view::Picture;
use moose::mui::mui::scene::Image;
use sampler_ui_ir as ir;
use std::{path::Path, sync::Arc};

/// Where an instrument's resources come from.
pub struct Source {
    kontakt: sampler_kontakt::Resources,
    uvi: Option<sampler_uvi::Resources>,
    #[cfg(feature = "shots")] pub scan: Scan,
}

#[cfg(feature = "shots")]
#[derive(Clone, Copy, Default)]
pub struct Scan {
    pub lookups: usize,
    pub lookup_ok: usize,
    pub decodes: usize,
    pub decode_ok: usize,
    pub fonts: usize,
}

impl Source {
    pub fn of(instrument: &Path) -> Self {
        let uvi = instrument.ancestors().any(|p|p.extension().is_some_and(|e|e.eq_ignore_ascii_case("ufs")||e.eq_ignore_ascii_case("uvip"))).then(||sampler_uvi::Resources::of(instrument));
        Self {kontakt:sampler_kontakt::Resources::of(instrument),uvi, #[cfg(feature="shots")] scan: Scan::default()}
    }

    pub(crate) fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        match &self.uvi {Some(uvi)=>uvi.read(path),None=>self.kontakt.read(path)}
    }
    pub fn font(&mut self, asset: &ir::Asset) -> Option<moose::mui::mui::prelude::Font> {
        moose::mui::mui::prelude::Font::new(self.read(&asset.path)?).ok()
    }

    /// `asset` decoded and cut into its frames.
    pub fn load(&mut self, asset: &ir::Asset) -> Option<Arc<Picture>> {
        let bytes = self.read(&asset.path);
        #[cfg(feature = "shots")]
        { self.scan.lookups += 1; self.scan.lookup_ok += usize::from(bytes.is_some()); }
        let image = crate::artwork::decode(&bytes?);
        #[cfg(feature = "shots")]
        { self.scan.decodes += 1; self.scan.decode_ok += usize::from(image.is_some()); }
        let image = image?;
        if matches!(asset.kind, ir::AssetKind::BitmapFont) {
            // Kontakt requires a sidecar and one font frame.
            let sidecar = format!("{}.txt", asset.path.rsplit_once('.')?.0);
            let text = self.read(&sidecar)?;
            if sampler_ksp::ui::picture_meta(&String::from_utf8_lossy(&text)).frames != 1 { return None; }
            return Some(Arc::new(Picture { frames: font_frames(&image)? }));
        }
        let ir::AssetKind::Image(meta) = &asset.kind else { return None };
        let n = meta.frames.max(1);
        let vertical = meta.axis == ir::Orientation::Vertical;
        let (fw, fh) = if vertical { (image.width, image.height / n) } else { (image.width / n, image.height) };
        let frames: Vec<_> = (0..n).map_while(|f| if vertical { crop(&image, 0, f * fh, fw, fh) } else { crop(&image, f * fw, 0, fw, fh) }).collect();
        (!frames.is_empty()).then(|| Arc::new(Picture { frames }))
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
    if image.height < 2 { return None; }
    let starts: Vec<u32> = (0..image.width).filter(|&x| {
        let at = x as usize * 4;
        image.rgba[at..at + 3] == [255, 0, 0]
    }).collect();
    if starts.len() != 256 || starts[0] != 0 {
        return None;
    }
    starts.iter().enumerate().map(|(n, &x)| {
        crop(image, x, 1, starts.get(n + 1).copied().unwrap_or(image.width) - x, image.height - 1)
    }).collect()
}

/// Unicode text is indexed by the font's documented Windows-1252 byte order.
/// Characters outside that alphabet use its authored question-mark glyph.
pub(crate) fn font_glyph(c: char) -> usize {
    const EXTENDED: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
        '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match c as u32 {
        0..=127 | 160..=255 => c as usize,
        _ => EXTENDED.iter().position(|&glyph| glyph == c).map_or(b'?' as usize, |n| n + 128),
    }
}


/// Cuts are reused between frames; retained source Arcs prevent address reuse.
/// The process-wide pixel budget also bounds nine-slice and wallpaper windows.
pub(crate) fn cut(image: &Arc<Image>, area: [u32; 4]) -> Option<Arc<Image>> {
    use std::{collections::HashMap, sync::{Mutex, OnceLock}};
    type Cuts = HashMap<(usize, [u32;4]), (Arc<Image>, Arc<Image>)>;
    static CACHE: OnceLock<Mutex<(Cuts, usize)>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(|| Mutex::new((HashMap::new(), 0))).lock().ok()?;
    let key = (Arc::as_ptr(image) as usize, area);
    if let Some((_, cut)) = cache.0.get(&key) { return Some(cut.clone()); }
    let [x,y,w,h] = area;
    let piece = crop(image,x,y,w,h)?;
    let bytes = piece.rgba.len() + image.rgba.len();
    if bytes <= 64 << 20 {
        if cache.1 + bytes > 64 << 20 { cache.0.clear(); cache.1 = 0; }
        cache.1 += bytes;
        cache.0.insert(key,(image.clone(),piece.clone()));
    }
    Some(piece)
}
