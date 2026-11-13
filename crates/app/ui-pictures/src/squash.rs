//! One line of words drawn narrower than its font sets them: JUCE fits a
//! line too wide for its box by squashing it across, down to 0.7 of its
//! width (`drawFittedText`'s minimum horizontal scale), before anything else.

use mui::prelude::{Color, Draw, Fill, Font, Path, Point};
use skrifa::instance::{LocationRef, Size};
use skrifa::metrics::{GlyphMetrics, Metrics};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};

/// How far JUCE squashes a line across before it gives up on fitting it.
pub const MIN_SQUASH: f64 = 0.7;

/// `words` set in `font` at `em` points: each glyph and its x, and the
/// line's width, ascent and descent (below the baseline, positive).
struct Set {
    glyphs: Vec<(GlyphId, f64)>,
    width: f64,
    ascent: f64,
    descent: f64,
}

fn set(face: &FontRef<'_>, words: &str, em: f64) -> Set {
    let size = Size::new(em as f32);
    let advances = GlyphMetrics::new(face, size, LocationRef::default());
    let map = face.charmap();
    let mut x = 0.;
    let glyphs = words
        .chars()
        .map(|c| {
            let g = map.map(c).unwrap_or_default();
            let at = x;
            x += f64::from(advances.advance_width(g).unwrap_or(0.));
            (g, at)
        })
        .collect();
    let m = Metrics::new(face, size, LocationRef::default());
    Set {
        glyphs,
        width: x,
        ascent: f64::from(m.ascent),
        descent: -f64::from(m.descent),
    }
}

/// How wide `words` run in `font` at `em` points; `None` if it cannot say.
#[must_use]
pub fn line_width(font: &Font, words: &str, em: f64) -> Option<f64> {
    let face = FontRef::from_index(font.as_ref(), font.index()).ok()?;
    Some(set(&face, words, em).width)
}

/// `words` in `font` at `em` points, `sx` of their width, from x 0 with
/// their line centred on `mid`, filled with `ink`.
#[must_use]
pub fn squashed(font: &Font, words: &str, em: f64, sx: f64, mid: f64, ink: Color) -> Vec<Draw> {
    let Ok(face) = FontRef::from_index(font.as_ref(), font.index()) else {
        return Vec::new();
    };
    let line = set(&face, words, em);
    let base = mid + (line.ascent - line.descent) / 2.;
    let outlines = face.outline_glyphs();
    let settings = || DrawSettings::unhinted(Size::new(em as f32), LocationRef::default());
    line.glyphs
        .iter()
        .filter_map(|&(g, x)| {
            let mut pen = Pen {
                path: Path::default(),
                at: (x, base, sx),
                inked: false,
            };
            outlines.get(g)?.draw(settings(), &mut pen).ok()?;
            pen.inked.then(|| Draw::fill(pen.path, Fill::from(ink)))
        })
        .collect()
}

/// Glyph outlines in view points: font y points up, the view's down.
struct Pen {
    path: Path,
    at: (f64, f64, f64),
    /// Whether it drew anything (a space draws nothing).
    inked: bool,
}

impl Pen {
    fn p(&self, x: f32, y: f32) -> Point {
        let (x0, base, sx) = self.at;
        Point::new((x0 + f64::from(x)) * sx, base - f64::from(y))
    }
    fn with(&mut self, f: impl FnOnce(Path) -> Path) {
        self.path = f(std::mem::take(&mut self.path));
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.inked = true;
        let p = self.p(x, y);
        self.with(|path| path.move_to(p));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.with(|path| path.line_to(p));
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (c, p) = (self.p(cx, cy), self.p(x, y));
        self.with(|path| path.quad_to(c, p));
    }
    fn curve_to(&mut self, ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32) {
        let (a, b, p) = (self.p(ax, ay), self.p(bx, by), self.p(x, y));
        self.with(|path| path.cubic_to(a, b, p));
    }
    fn close(&mut self) {
        self.with(Path::close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_squashed_line_runs_sx_of_its_width() {
        let bytes = include_bytes!("../../../../assets/NotoSans.ttf").to_vec();
        let Some(noto) = Font::new(bytes).ok() else {
            return;
        };
        let wide = line_width(&noto, "Mode: Velocity", 12.).unwrap_or(0.);
        assert!(wide > 60. && wide < 100.);
        let draws = squashed(
            &noto,
            "Mode: Velocity",
            12.,
            0.8,
            10.,
            Color::srgb(1., 1., 1.),
        );
        // Every glyph but the space has an outline.
        assert_eq!(draws.len(), 13);
    }
}
