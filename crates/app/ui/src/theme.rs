//! The look, kept from v2: graphite neutrals, square edges, hairline
//! separators, no shadows. One text size sets the scale; every space and
//! control size derives from it. The accent is light and achromatic and
//! marks focus and the current view only, so the colours on screen are the
//! parts', the library's and real warnings.

use mui::prelude::*;
use mui::scene::TypeScale;

/// Body text. Everything below derives from it.
pub const TEXT: f64 = 12.;
/// Captions, readouts, section titles.
pub const SMALL: f64 = TEXT - 1.;
/// Between the parts of one control.
pub const TIGHT: f64 = TEXT / 3.;
/// Between sibling controls.
pub const SPACE: f64 = TEXT * 2. / 3.;
/// Around a panel's content.
pub const INSET: f64 = TEXT;
/// A control's height: one line with room to aim at.
pub const CONTROL: f64 = TEXT * 2.;
/// The browser's width.
pub const SIDEBAR: f64 = TEXT * 22.;

/// The face every view draws text with.
pub const NOTO_SANS: &[u8] = include_bytes!("../../../../assets/NotoSans.ttf");

/// A [`Ui`] in KONTRA's look.
#[must_use]
pub fn ui() -> Ui {
    crate::pictures::stock::prewarm();
    let ui = Ui::new(Theme {
        palette: Palette {
            neutral: Pigment::new(260.0, 0.008),
            warning: Pigment::new(105.0, 0.15),
            step: 0.035,
            ..Palette::NEUTRAL
        },
        corners: Corners {
            field: 0.,
            box_: 0.,
            selector: 0.,
            concave: 0.,
        },
        text: TEXT + 1.,
        control: 3.,
        type_scale: TypeScale {
            title: TEXT * 5. / 3.,
            body: TEXT + 1.,
            caption: SMALL,
        },
        ..Theme::DEFAULT
    });
    match Font::new(NOTO_SANS) {
        Ok(font) => ui.font(font),
        Err(_) => ui,
    }
}

/// Focus and the current view.
#[must_use]
pub fn accent() -> Color {
    Color::oklch(0.86, 0., 0.)
}

/// Part `slot`'s colour: hues a golden angle apart, so neighbours never
/// look alike, at one quiet lightness. The amber band is skipped: it reads
/// as a warning. The first part is blue.
#[must_use]
pub fn part_color(slot: usize) -> Color {
    Color::oklch(0.7, 0.1, part_hue(slot))
}

fn part_hue(slot: usize) -> f32 {
    const AMBER: (f64, f64) = (40., 105.);
    let arc = 360. - (AMBER.1 - AMBER.0);
    let t = (145. + slot as f64 * 137.507_764) % arc;
    ((AMBER.1 + t) % 360.) as f32
}

/// Every boundary.
#[must_use]
pub fn hairline() -> Fill {
    Role::Ink.alpha(0.08)
}

/// Secondary text and glyphs: a solid grey, so nothing shows through.
#[must_use]
pub fn secondary() -> Fill {
    Fill::from(Color::oklch(0.72, 0., 0.))
}

/// A surface `steps` above the ground, opaque so text grounds on it.
#[must_use]
pub fn lift(steps: i32) -> Fill {
    Role::Level(steps).into()
}

/// State changes land in about a tenth of a second: felt, not watched.
#[must_use]
pub fn quick() -> Spring {
    Spring::new(0.12, 1.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_colours_skip_amber_and_differ() {
        let hues: Vec<f32> = (0..16).map(part_hue).collect();
        assert!(hues.iter().all(|h| !(40.0..105.0).contains(h)), "{hues:?}");
        assert!(hues.windows(2).all(|w| w.first() != w.last()), "{hues:?}");
    }
}
