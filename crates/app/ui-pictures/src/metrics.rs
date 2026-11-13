//! How large a font's em is for a line height: UVI's `fontSize` is JUCE's
//! font height (ascent + descent), not the em size the renderer takes.

use mui::prelude::Font;
use skrifa::instance::{LocationRef, Size};
use skrifa::metrics::Metrics;

/// Em per unit of line height for `font` (`unitsPerEm / (ascent - descent)`,
/// the vertical metrics HarfBuzz and JUCE read); `None` when it cannot say.
#[must_use]
pub fn em_per_height(font: &Font) -> Option<f64> {
    let face = skrifa::FontRef::from_index(font.as_ref(), font.index()).ok()?;
    let m = Metrics::new(&face, Size::unscaled(), LocationRef::default());
    let height = f64::from(m.ascent - m.descent);
    (height > 0.).then(|| f64::from(m.units_per_em) / height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_height_holds_less_than_an_em_of_noto() {
        let noto = Font::new(include_bytes!("../../../../assets/NotoSans.ttf").to_vec());
        let ratio = noto.ok().as_ref().and_then(em_per_height);
        // Noto Sans: 1000 units per em over 1069 + 293.
        assert_eq!(ratio.map(|r| (r * 1000.).round()), Some(734.));
    }
}
