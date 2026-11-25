//! A picture font cut from an installed outline face: its 256 Windows-1252
//! glyphs hinted at whole pixels per em, each in a cell its device advance
//! wide (the face's `hdmx`), a red marker pixel starting each on the top row,
//! as `glyphs` reads an installed atlas.

use mui::prelude::Image;
use resvg::tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, HintingInstance, HintingOptions, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::{FontRef, MetadataProvider};

/// `face` at `ppem` pixels per em, its baseline `base` rows below the cell
/// top and `below` rows under it.
pub(super) fn atlas(face: &[u8], ppem: u8, base: u32, below: u32) -> Option<Image> {
    let font = FontRef::new(face).ok()?;
    let hdmx = font.hdmx().ok()?;
    let widths = hdmx.record_for_size(ppem)?.widths();
    let outlines = font.outline_glyphs();
    let size = Size::new(f32::from(ppem));
    let hinting = HintingInstance::new(
        &outlines,
        size,
        LocationRef::default(),
        HintingOptions::default(),
    )
    .ok()?;
    let map = font.charmap();
    let glyphs: Vec<_> = (0..=255u8)
        .map(|b| {
            let c = cp1252(b);
            let g = map.map(c).unwrap_or_default();
            let advance = widths.get(g.to_u32() as usize).copied().unwrap_or(0);
            (g, u32::from(advance).max(1))
        })
        .collect();
    let width: u32 = glyphs.iter().map(|&(_, w)| w).sum();
    let height = 1 + base + below;
    let mut pixmap = Pixmap::new(width, height)?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    let mut x = 0;
    let mut starts = Vec::with_capacity(256);
    for &(g, w) in &glyphs {
        starts.push(x);
        let mut pen = Pen(PathBuilder::new());
        if let Some(glyph) = outlines.get(g)
            && glyph
                .draw(DrawSettings::hinted(&hinting, false), &mut pen)
                .is_ok()
            && let Some(path) = pen.0.finish()
        {
            // Font y points up; the cell's baseline sits `base` rows down,
            // under the marker row.
            let at = Transform::from_row(1., 0., 0., -1., x as f32, (1 + base) as f32);
            pixmap.fill_path(&path, &paint, FillRule::Winding, at, None);
        }
        x += w;
    }
    let mut rgba = pixmap.take();
    // The marker row is not drawn: clear what a tall glyph inked into it,
    // then mark where each glyph starts.
    let (top, _) = rgba.as_chunks_mut::<4>();
    for (x, p) in top.iter_mut().take(width as usize).enumerate() {
        *p = if starts.contains(&(x as u32)) {
            [255, 0, 0, 255]
        } else {
            [0; 4]
        };
    }
    // Premultiplied white keeps its coverage in every channel; unpremultiply.
    for p in rgba.as_chunks_mut::<4>().0.iter_mut().skip(width as usize) {
        if p[3] > 0 {
            *p = [255, 255, 255, p[3]];
        }
    }
    Image::rgba(width, height, rgba)
}

/// The character a Windows-1252 byte stands for.
fn cp1252(b: u8) -> char {
    match b {
        0x80..=0x9f => (super::EXTENDED.get(usize::from(b - 0x80)))
            .copied()
            .unwrap_or('?'),
        _ => char::from(b),
    }
}

struct Pen(PathBuilder);

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.0.quad_to(cx, cy, x, y);
    }
    fn curve_to(&mut self, ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32) {
        self.0.cubic_to(ax, ay, bx, by, x, y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
mod tests {
    /// Noto ships `hdmx`-less, so a face without one makes no atlas and the
    /// label keeps its own typeface.
    #[test]
    fn a_face_without_device_widths_makes_no_atlas() {
        let noto = include_bytes!("../../../../assets/NotoSans.ttf");
        assert!(super::atlas(noto, 11, 8, 4).is_none());
        assert!(super::atlas(b"not a face", 11, 8, 4).is_none());
    }
}
