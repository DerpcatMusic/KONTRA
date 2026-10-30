//! Kontakt's measured presentation laws, supplied to the shared UI model.
//!
//! The model carries these values and applies them, but defines none: a
//! second format supplies its own [`Laws`] without the model knowing it.

use kontra_ui_model::{KindTag, Laws, PaintRanks, PictureFrames};

/// Kontakt's presentation laws.
///
/// Pictures: switch/button/dropdown strips are off, on, off pressed, on
/// pressed, off hover, on hover (NI Kontakt 4 Player Developer Guide §5.4.2).
/// A short legacy strip retains its off/on fallback; a held pointer wins over
/// hover. Painting: the documented back-to-front type priority within one Z
/// layer (KSP Control Parameters).
pub const LAWS: Laws = Laws {
    pictures: PictureFrames {
        states: [0, 2, 4, 1, 3, 5],
        short: [0, 1],
    },
    paint: PaintRanks { ranks: RANKS },
};

/// Documented Kontakt back-to-front type priority within one Z layer, lowest
/// first.
///
/// Image is the shared-IR representation of a picture label; panels paint no
/// pixels and retain their position before ordinary controls.
const RANKS: &[(KindTag, u8)] = &[
    (KindTag::Panel, 0),
    (KindTag::FileSelector, 1),
    (KindTag::Waveform, 2),
    (KindTag::Wavetable, 3),
    (KindTag::LevelMeter, 4),
    (KindTag::Label, 5),
    (KindTag::Image, 5),
    (KindTag::Knob, 6),
    (KindTag::Slider, 7),
    (KindTag::Switch, 8),
    (KindTag::Button, 9),
    (KindTag::ValueEdit, 10),
    (KindTag::Menu, 11),
    (KindTag::Table, 12),
    (KindTag::Xy, 13),
    (KindTag::TextEdit, 14),
    (KindTag::MouseArea, 15),
];

#[cfg(test)]
mod tests {
    use kontra_ui_model::{
        Active, Interface, Kind, Orientation, Page, PageRef, PictureState, Pointer, Range, Rect,
        Size, Source, Widget, WidgetRef,
    };

    use super::*;

    #[test]
    fn the_six_picture_states_show_off_on_pressed_and_hover_frames() {
        let strip = LAWS.pictures;
        let got: Vec<_> = [Active::Off, Active::On]
            .into_iter()
            .flat_map(|on| {
                [Pointer::Idle, Pointer::Held, Pointer::Hovered]
                    .map(move |pointer| strip.frame(PictureState::new(on, pointer), 6))
            })
            .collect();
        assert_eq!(got, [0, 2, 4, 1, 3, 5]);
        for frames in 0..=2 {
            assert_eq!(
                strip.frame(PictureState::new(Active::On, Pointer::Held), frames),
                frames.saturating_sub(1)
            );
            assert_eq!(
                strip.frame(PictureState::new(Active::Off, Pointer::Hovered), frames),
                0
            );
        }
    }

    #[test]
    fn layers_use_native_type_priority_before_declaration_order() {
        let mut ui = Interface {
            source: Source::Ksp { slot: 0 },
            laws: LAWS,
            pages: vec![Page {
                size: Size {
                    width: 100.,
                    height: 100.,
                },
                ..Page::default()
            }],
            ..Interface::default()
        };
        ui.widgets = vec![
            Widget::new(
                "menu",
                PageRef(0),
                Rect::new(0, 0, 20, 20),
                Kind::Menu { items: vec![] },
            ),
            Widget::new(
                "slider",
                PageRef(0),
                Rect::new(0, 0, 20, 20),
                Kind::Slider {
                    range: Range::default(),
                    orientation: Orientation::Vertical,
                },
            ),
            Widget::new(
                "first-label",
                PageRef(0),
                Rect::new(0, 0, 20, 20),
                Kind::Label,
            ),
            Widget::new(
                "second-label",
                PageRef(0),
                Rect::new(0, 0, 20, 20),
                Kind::Label,
            ),
            Widget::new(
                "meter",
                PageRef(0),
                Rect::new(0, 0, 20, 20),
                Kind::LevelMeter {
                    orientation: Orientation::Vertical,
                    channel: None,
                    bus: None,
                },
            ),
        ];
        // NI's documented top-to-bottom priority is Menu, Slider, Label, Meter.
        // The two labels retain their declaration-order tie break.
        assert_eq!(
            ui.draw_order(PageRef(0)),
            [
                WidgetRef(4),
                WidgetRef(2),
                WidgetRef(3),
                WidgetRef(1),
                WidgetRef(0)
            ]
        );
        ui.widgets[4].z = 1;
        assert_eq!(
            ui.draw_order(PageRef(0)),
            [
                WidgetRef(2),
                WidgetRef(3),
                WidgetRef(1),
                WidgetRef(0),
                WidgetRef(4)
            ]
        );
        ui.source = Source::PerformanceView;
        assert_eq!(
            ui.draw_order(PageRef(0)),
            [
                WidgetRef(2),
                WidgetRef(3),
                WidgetRef(1),
                WidgetRef(0),
                WidgetRef(4)
            ]
        );
    }
}
