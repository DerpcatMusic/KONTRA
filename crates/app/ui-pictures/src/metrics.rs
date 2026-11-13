//! How large a font's em is for a line height: UVI's `fontSize` is JUCE's
//! font height (ascent + descent), not the em size the renderer takes.

use mui::prelude::Font;
use skrifa::instance::{LocationRef, Size};
use skrifa::metrics::Metrics;

/// Em per unit of line height for `font` (`unitsPerEm / (ascent - descent)`,
/// the vertical metrics HarfBuzz and JUCE read); 1 when it cannot say.
#[must_use]
pub fn em_per_height(font: &Font) -> f64 {
    let Ok(face) = skrifa::FontRef::from_index(font.as_ref(), font.index()) else {
        return 1.;
    };
    let m = Metrics::new(&face, Size::unscaled(), LocationRef::default());
    let height = f64::from(m.ascent - m.descent);
    if height > 0. {
        f64::from(m.units_per_em) / height
    } else {
        1.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_height_holds_less_than_an_em_of_noto() {
        let noto = Font::new(include_bytes!("../../../../assets/NotoSans.ttf").to_vec());
        let ratio = noto.ok().as_ref().map(em_per_height);
        // Noto Sans: 1000 units per em over 1069 + 293.
        assert_eq!(ratio.map(|r| (r * 1000.).round()), Some(734.));
    }
}
