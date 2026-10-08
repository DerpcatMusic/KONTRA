//! A script interface's pictures, decoded from the library the loader
//! already read their layouts from.

use super::ir_view::Picture;
use moose::mui::mui::scene::Image;
use sampler_ui_ir as ir;
use std::{path::Path, sync::Arc};

/// Where an instrument's resources come from.
pub struct Source(sampler_kontakt::Resources, #[cfg(feature = "shots")] pub Scan);

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
        Self(sampler_kontakt::Resources::of(instrument), #[cfg(feature = "shots")] Scan::default())
    }

    /// `asset` decoded and cut into its frames.
    pub fn load(&mut self, asset: &ir::Asset) -> Option<Arc<Picture>> {
        let ir::AssetKind::Image(meta) = &asset.kind else {
            #[cfg(feature = "shots")]
            { self.1.fonts += 1; }
            return None;
        };
        let bytes = self.0.read(&asset.path);
        #[cfg(feature = "shots")]
        { self.1.lookups += 1; self.1.lookup_ok += usize::from(bytes.is_some()); }
        let image = crate::artwork::decode(&bytes?);
        #[cfg(feature = "shots")]
        { self.1.decodes += 1; self.1.decode_ok += usize::from(image.is_some()); }
        let image = image?;
        let n = meta.frames.max(1);
        let vertical = meta.axis == ir::Orientation::Vertical;
        let (fw, fh) = if vertical { (image.width, image.height / n) } else { (image.width / n, image.height) };
        let frames: Vec<_> = (0..n).map_while(|f| if vertical { crop(&image, 0, f * fh, fw, fh) } else { crop(&image, f * fw, 0, fw, fh) }).collect();
        (!frames.is_empty()).then(|| Arc::new(Picture { frames }))
    }
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
