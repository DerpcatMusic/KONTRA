//! The look, kept from v2: graphite neutrals, square edges, hairline
//! separators, no shadows. One text size sets the scale; every space and
//! control size derives from it. The accent is light and achromatic and
//! marks focus and the current view only, so the colours on screen are the
//! parts', the library's and real warnings.
//!
//! The colours are tokens ([`Look`]), one set for the whole process: Settings
//! edits them, every open editor takes them on its next frame ([`sync`]),
//! and the host saves them as hex ([`Look::tokens`]).

use std::collections::BTreeMap;
use std::sync::{PoisonError, RwLock};

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

/// The editable colours. Every grey derives from `ground` (its lightness,
/// and the hue and chroma they all lean to); MUI's roles follow from these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub ground: Color,
    /// Focus and the current view.
    pub accent: Color,
    /// Secondary text and glyphs.
    pub secondary: Color,
    pub warning: Color,
}

/// The tokens Settings lists: key, label.
pub const TOKENS: [(&str, &str); 4] = [
    ("ground", "Background"),
    ("accent", "Accent"),
    ("secondary", "Secondary text"),
    ("warning", "Warning"),
];

/// The most a tint leans the greys toward the accent's hue.
const TINT: f32 = 0.04;

impl Look {
    /// The default: neutral greys, as KONTRA up to v0.6.20.
    pub const GRAPHITE: Self = Self {
        ground: Color::oklch(Mode::Dark.ground(), 0., 0.),
        accent: Color::oklch(0.86, 0., 0.),
        secondary: Color::oklch(0.72, 0., 0.),
        warning: Color::oklch(0.75, 0.15, 105.),
    };

    /// The presets Settings offers, the default first.
    pub const PRESETS: [(&str, Self); 4] = [
        ("Graphite", Self::GRAPHITE),
        (
            "Midnight",
            Self {
                ground: Color::oklch(0.2, 0.02, 260.),
                accent: Color::oklch(0.78, 0.12, 250.),
                secondary: Color::oklch(0.72, 0.01, 260.),
                ..Self::GRAPHITE
            },
        ),
        (
            "Ember",
            Self {
                ground: Color::oklch(0.21, 0.012, 50.),
                accent: Color::oklch(0.8, 0.13, 55.),
                secondary: Color::oklch(0.73, 0.008, 60.),
                ..Self::GRAPHITE
            },
        ),
        (
            "Moss",
            Self {
                ground: Color::oklch(0.21, 0.012, 150.),
                accent: Color::oklch(0.8, 0.12, 150.),
                secondary: Color::oklch(0.73, 0.008, 150.),
                ..Self::GRAPHITE
            },
        ),
    ];

    /// The token `key` names ([`TOKENS`]).
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Color> {
        Some(match key {
            "ground" => self.ground,
            "accent" => self.accent,
            "secondary" => self.secondary,
            "warning" => self.warning,
            _ => return None,
        })
    }

    pub fn set(&mut self, key: &str, c: Color) {
        match key {
            "ground" => self.ground = c,
            "accent" => self.accent = c,
            "secondary" => self.secondary = c,
            "warning" => self.warning = c,
            _ => {}
        }
    }

    /// How far the greys lean toward the accent, 0..1.
    #[must_use]
    pub fn tint(&self) -> f32 {
        (self.ground.chroma() / TINT).clamp(0., 1.)
    }

    /// Leans the greys `t` (0..1) of the way toward the accent's hue.
    #[must_use]
    pub fn with_tint(self, t: f32) -> Self {
        let hue = if self.accent.chroma() > 0.005 {
            self.accent.hue()
        } else {
            self.ground.hue()
        };
        Self {
            ground: Color::oklch(self.ground.lightness(), t.clamp(0., 1.) * TINT, hue),
            ..self
        }
    }

    /// The tokens as `#rrggbb`, for the host to save.
    #[must_use]
    pub fn tokens(&self) -> BTreeMap<String, String> {
        TOKENS
            .iter()
            .filter_map(|&(k, _)| Some((k.to_owned(), hex(self.get(k)?))))
            .collect()
    }

    /// The default with each saved token that parses put back.
    #[must_use]
    pub fn from_tokens(tokens: &BTreeMap<String, String>) -> Self {
        let mut look = Self::GRAPHITE;
        for (k, v) in tokens {
            if let Some(c) = parse_hex(v) {
                look.set(k, c);
            }
        }
        look
    }

    /// The MUI theme these colours make.
    #[must_use]
    pub fn theme(&self) -> Theme {
        let pigment = |c: Color| Pigment::new(c.hue(), c.chroma());
        Theme {
            palette: Palette {
                neutral: pigment(self.ground),
                primary: pigment(self.accent),
                warning: pigment(self.warning),
                step: 0.035,
                ground: Some(self.ground.lightness()),
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
        }
    }
}

/// `c` as `#rrggbb`.
#[must_use]
pub fn hex(c: Color) -> String {
    let [r, g, b, _] = c.to_srgb().components;
    let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
}

/// `#rrggbb` (the `#` optional) as a colour.
#[must_use]
pub fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    let v = |i| Some(f32::from(byte(i)?) / 255.);
    Some(Color::srgb(v(0)?, v(2)?, v(4)?))
}

static LOOK: RwLock<Look> = RwLock::new(Look::GRAPHITE);

/// The colours in use.
#[must_use]
pub fn look() -> Look {
    *LOOK.read().unwrap_or_else(PoisonError::into_inner)
}

/// Uses `look` everywhere from the next frame on.
pub fn set_look(look: Look) {
    *LOOK.write().unwrap_or_else(PoisonError::into_inner) = look;
}

/// Puts the current [`look`] on `ui` when it changed.
pub fn sync(ui: &mut Ui) {
    let theme = look().theme();
    if ui.theme().palette != theme.palette {
        ui.set_theme(theme);
    }
}

/// A [`Ui`] in KONTRA's look.
#[must_use]
pub fn ui() -> Ui {
    kontra_ui_pictures::stock::prewarm();
    let mut ui = Ui::new(look().theme());
    let mut limits = ui.layout_limits();
    limits.nodes = 65_536;
    let _ = ui.set_layout_limits(limits);
    match Font::from_static(NOTO_SANS) {
        Ok(font) => ui.font(font),
        Err(_) => ui,
    }
}

/// Focus and the current view.
#[must_use]
pub fn accent() -> Color {
    look().accent
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
    Fill::from(look().secondary)
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

    #[test]
    fn default_look_is_neutral_grey() {
        let theme = Look::GRAPHITE.theme();
        assert!(theme.palette.neutral.chroma.abs() < 1e-6, "{theme:?}");
        assert!(Look::GRAPHITE.ground.chroma().abs() < 1e-6);
    }

    #[test]
    fn tokens_round_trip_through_hex() {
        for (name, look) in Look::PRESETS {
            let back = Look::from_tokens(&look.tokens());
            for (k, _) in TOKENS {
                let (a, b) = (look.get(k).unwrap(), back.get(k).unwrap());
                assert_eq!(hex(a), hex(b), "{name} {k}");
            }
        }
        let mut odd = Look::GRAPHITE.tokens();
        odd.insert("accent".into(), "not a colour".into());
        odd.insert("unknown".into(), "#ffffff".into());
        assert_eq!(Look::from_tokens(&odd).accent, Look::GRAPHITE.accent);
        assert_eq!(parse_hex("#ff8000").map(hex).as_deref(), Some("#ff8000"));
    }

    #[test]
    fn tint_leans_the_greys_toward_the_accent() {
        let look = Look::PRESETS[1].1.with_tint(1.);
        assert!((look.tint() - 1.).abs() < 1e-3);
        assert!((look.ground.hue() - look.accent.hue()).abs() < 1.);
        assert!(Look::GRAPHITE.with_tint(0.).ground.chroma() < 1e-6);
    }
}
