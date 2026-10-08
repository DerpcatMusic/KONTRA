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
}

impl Source {
    pub fn of(instrument: &Path) -> Self {
        let uvi = instrument.ancestors().any(|p|p.extension().is_some_and(|e|e.eq_ignore_ascii_case("ufs")||e.eq_ignore_ascii_case("uvip"))).then(||sampler_uvi::Resources::of(instrument));
        Self {kontakt:sampler_kontakt::Resources::of(instrument),uvi}
    }

    pub(crate) fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        match &self.uvi {Some(uvi)=>uvi.read(path),None=>self.kontakt.read(path)}
    }
    pub(super) fn native_names(&mut self)->Vec<String> {
        let mut names=self.kontakt.names("resources/native_ui/");names.extend(self.kontakt.names("native_ui/"));names.sort();names.dedup();names
    }
    pub fn font(&mut self, asset: &ir::Asset) -> Option<moose::mui::mui::prelude::Font> {
        moose::mui::mui::prelude::Font::new(self.read(&asset.path)?).ok()
    }

    /// Compatibility probe: one frame, never an eagerly decoded strip.
    pub fn load(&mut self, asset:&ir::Asset)->Option<Arc<Picture>> {self.load_frame(asset,0,[u32::MAX;2],None,||false)}
    pub(super) fn load_frame(&mut self,asset:&ir::Asset,frame:usize,target:[u32;2],window:Option<[u32;4]>,canceled:impl Fn()->bool)->Option<Arc<Picture>> {
        let bytes=self.read(&asset.path)?;
        let meta=match asset.kind {ir::AssetKind::Image(m)=>m,ir::AssetKind::BitmapFont=>ir::ImageMeta::default(),_=>return None};
        let image=super::picture_decode::decode(&bytes,meta,frame,target,window,canceled)?;
        if matches!(asset.kind, ir::AssetKind::BitmapFont) {
            let sidecar=format!("{}.txt",asset.path.rsplit_once('.')?.0);
            let text=self.read(&sidecar)?;
            if sampler_ksp::ui::picture_meta(&String::from_utf8_lossy(&text)).frames!=1 {return None;}
            return Some(Arc::new(Picture::new(font_frames(&image)?)));
        }
        Some(Arc::new(Picture::prepared(Arc::new(image),frame,meta.frames.max(1) as usize,window)))
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


/// Wake signature for authored resource preparation, independent of Logs visibility.
pub(crate) fn revision()->u64 {super::picture_worker::revision()}
