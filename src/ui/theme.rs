//! The look: one accent, graphite neutrals, an 8 px grid, one bundled face.

use moose::mui::mui::prelude::*;
use moose::mui::mui::{layout::Insets, scene::TypeScale};

/// The spacing grid. Everything pads and gaps by these.
pub const GAP: f64 = 8.0;
pub const HALF: f64 = 4.0;
pub const WIDE: f64 = 16.0;

/// Padding per side, in CSS order.
pub fn edges(top: f64, right: f64, bottom: f64, left: f64) -> Insets {
    Insets {
        left,
        right,
        top,
        bottom,
    }
}

/// Fixed strip heights.
pub const TOP_BAR: f64 = 48.0;
pub const SIDEBAR: f64 = 288.0;

/// The accent's hue and chroma (OKLCH). Selection, focus, values, played keys.
const ACCENT_HUE: f32 = 64.0;
const ACCENT_CHROMA: f32 = 0.15;

pub fn accent() -> Color {
    Color::oklch(0.76, ACCENT_CHROMA, ACCENT_HUE)
}

pub fn ui() -> Ui {
    Ui::new(Theme {
        palette: Palette {
            neutral: Pigment::new(260.0, 0.008),
            primary: Pigment::new(ACCENT_HUE, ACCENT_CHROMA),
            secondary: Pigment::new(ACCENT_HUE, ACCENT_CHROMA),
            tertiary: Pigment::new(ACCENT_HUE, ACCENT_CHROMA),
            step: 0.035,
            ..Palette::NEUTRAL
        },
        corners: Corners {
            field: 6.,
            box_: 8.,
            selector: 6.,
            ..Corners::DEFAULT
        },
        text: 13.,
        control: 3.,
        type_scale: TypeScale {
            title: 20.,
            body: 13.,
            caption: 11.,
        },
        ..Theme::DEFAULT
    })
    .font(
        Font::new(include_bytes!("../../assets/NotoSans.ttf").as_slice())
            .expect("bundled Noto Sans"),
    )
}

/// A quiet text button; `selected` gives it the accent's soft fill.
pub fn action(ui: &mut Ui, id: impl Into<Id>, label: &str, selected: bool) -> (bool, El) {
    let b = button(ui, id, label)
        .size(S)
        .variant(if selected {
            Variant::Soft
        } else {
            Variant::Ghost
        })
        .role(if selected { Role::Primary } else { Role::Ink });
    (b.changed, b.el.el().radius(6).text_size(12))
}

/// A view switch: ink when current, dim otherwise, with an accent underline.
pub fn tab(ui: &mut Ui, id: impl Into<Id>, label: &str, current: bool) -> (bool, El) {
    let b = button(ui, id, label)
        .size(S)
        .variant(Variant::Ghost)
        .role(if current { Role::Ink } else { Role::Dim });
    let label = b.el.el().radius(6).text_size(12);
    let line = block(Len::Pct(100.), 2).pill().fill(if current {
        Role::Primary.alpha(1.)
    } else {
        Role::Dim.alpha(0.)
    });
    (b.changed, col![label, line].gap(2).shrink(0))
}

/// A horizontal hairline.
pub fn rule() -> El {
    block(Len::Pct(100.), 1)
        .fill(Role::Ink.alpha(0.07))
        .shrink(0)
}

/// A vertical hairline.
pub fn vrule() -> El {
    block(1, Len::Pct(100.))
        .fill(Role::Ink.alpha(0.07))
        .shrink(0)
}

/// A small uppercase section label.
pub fn section(label: &str) -> El {
    caption(label.to_uppercase())
        .fill(Role::Dim)
        .text_weight(Weight::SEMIBOLD)
        .shrink(0)
}

/// A labelled number to drag, type or step.
pub fn number(
    ui: &mut Ui,
    id: impl Into<Id>,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    display: String,
) -> El {
    let reserve = display.clone();
    let c = drag_value(ui, id, label, value, range).size(S);
    row![
        caption(label).fill(Role::Dim),
        c.el.value_text(display)
            .el()
            .radius(6)
            .min_w(44)
            .reserve(reserve)
    ]
    .gap(HALF)
    .align(Align::Center)
    .shrink(0)
}

/// A dim label beside an ink value that keeps its width as digits change.
pub fn stat(label: &str, value: String, widest: &str) -> El {
    row![
        caption(label).fill(Role::Dim),
        caption(value).reserve(widest.to_owned())
    ]
    .gap(HALF)
    .align(Align::Center)
    .shrink(0)
}

/// A soft, rounded notice. `role` is Warning or Danger.
pub fn banner(role: Role, text: impl Into<String>) -> El {
    row![
        block(3, Len::Pct(100.)).pill().fill(role.alpha(1.)),
        body(text.into()).text_size(12).lines(4).flex(1).min_w(0)
    ]
    .gap(GAP)
    .align(Align::Stretch)
    .pad((GAP + HALF, GAP))
    .fill(role.alpha(0.12))
    .radius(8)
    .shrink(0)
}

pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[(note % 12) as usize], note as i16 / 12 - 2)
}

/// A library folder name without the vendor noise.
pub fn library_label(name: &str) -> String {
    name.replace("Performance Samples ", "")
        .replace(" Library", "")
}

pub fn megabytes(bytes: usize) -> String {
    format!("{:.0} MB", bytes as f64 / 1_048_576.)
}

#[cfg(test)]
mod tests {
    #[test]
    fn note_names_follow_kontakt() {
        // Kontakt calls MIDI 60 "C3".
        assert_eq!(super::note_name(60), "C3");
        assert_eq!(super::note_name(0), "C-2");
        assert_eq!(super::note_name(127), "G8");
    }
}
