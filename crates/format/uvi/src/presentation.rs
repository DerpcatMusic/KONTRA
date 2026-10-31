//! The presentation laws the Falcon Lua frontend installs.
//!
//! Two of these are carried rather than measured for UVI. The strip table is
//! the table the shared model applied to every picture strip before a format
//! could supply its own: Kontakt's six responsive states (NI Kontakt 4 Player
//! Developer Guide §5.4.2). The readout law is v1's UVI unit table. Both are
//! kept here so no UVI render changes with the law seam; a UVI measurement can
//! replace them later.

use kontra_ui_model::{Display, Formatting, Laws, Layering, PaintRanks, PictureFrames};

/// The laws the Falcon Lua frontend installs.
///
/// `paint` is unused: [`Layering::Nested`] orders siblings, so a page never
/// consults type ranks.
pub const LAWS: Laws = Laws {
    pictures: PictureFrames {
        states: [0, 2, 4, 1, 3, 5],
        short: [0, 1],
    },
    paint: PaintRanks { ranks: &[] },
    layering: Layering::Nested,
    formatting: Formatting { render: text },
};

/// A value as v1's UVI readout showed it.
///
/// The unit name decides the scale and suffix; an unknown or numeric-only
/// name prints the raw value. Carried from the shared model, which held this
/// branch before readouts became a law:
/// Port v1 4bffbb18:src/ui/uvi_instrument.rs::unit_text/number_text.
fn text(display: &Display, value: f64, integer: bool) -> String {
    let number = |value: f64, integer: bool| {
        if integer {
            format!("{value:.0}")
        } else {
            format!("{value:.3}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned()
        }
    };
    let (value, suffix) = match display.unit.as_str() {
        "Percent" => (value, "%"),
        "PercentNormalized" => (value * 100., "%"),
        "Seconds" if value.abs() < 1. => (value * 1000., "ms"),
        "Seconds" => (value, "s"),
        "MilliSeconds" if value.abs() > 1000. => (value / 1000., "s"),
        "MilliSeconds" => (value, "ms"),
        "Hertz" if value.abs() > 1000. => (value / 1000., "kHz"),
        "Hertz" => (value, "Hz"),
        "Decibels" => (value, "dB"),
        "LinearGain" if value <= 0. => return "-inf dB".into(),
        "LinearGain" => (20. * value.log10(), "dB"),
        "Pan" if value == 0. => return "Center".into(),
        "SemiTones" => (value, "st"),
        // shortcut: v1 leaves these units numeric; calibrate with native readouts before extending.
        "" | "Generic" | "UviFilter" | "Pan" | "Megabyte" | "Cents" | "MidiKey" => {
            return number(value, integer);
        }
        suffix => (value, suffix),
    };
    format!("{} {suffix}", number(value + 0., false))
}

#[cfg(test)]
mod tests {
    use kontra_ui_model::{Active, Display, PictureState, Pointer};

    use super::*;

    #[test]
    fn picture_strips_keep_the_frames_the_shared_model_used() {
        let strip = LAWS.pictures;
        let got: Vec<_> = [Active::Off, Active::On]
            .into_iter()
            .flat_map(|on| {
                [Pointer::Idle, Pointer::Held, Pointer::Hovered]
                    .map(move |pointer| strip.frame(PictureState::new(on, pointer), 6))
            })
            .collect();
        assert_eq!(got, [0, 2, 4, 1, 3, 5], "six-frame strip");
        assert_eq!(
            strip.frame(PictureState::new(Active::Off, Pointer::Held), 2),
            0
        );
        assert_eq!(
            strip.frame(PictureState::new(Active::On, Pointer::Held), 2),
            1
        );
        assert_eq!(
            strip.frame(PictureState::new(Active::On, Pointer::Idle), 1),
            0,
            "a one-frame strip shows its only frame"
        );
    }

    #[test]
    fn readouts_use_v1_units_and_scales() {
        let display = |unit: &str| Display {
            unit: unit.into(),
            ..Display::default()
        };
        assert_eq!(
            display("Percent").format_value(0.25, false, &LAWS.formatting),
            "0.25 %"
        );
        assert_eq!(
            display("PercentNormalized").format_value(0.25, false, &LAWS.formatting),
            "25 %"
        );
        assert_eq!(
            display("Seconds").format_value(0.5, false, &LAWS.formatting),
            "500 ms"
        );
        assert_eq!(
            display("MilliSeconds").format_value(1500., false, &LAWS.formatting),
            "1.5 s"
        );
        assert_eq!(
            display("Hertz").format_value(2000., false, &LAWS.formatting),
            "2 kHz"
        );
        assert_eq!(
            display("LinearGain").format_value(0., false, &LAWS.formatting),
            "-inf dB"
        );
        assert_eq!(
            display("Pan").format_value(0., false, &LAWS.formatting),
            "Center"
        );
        assert_eq!(
            display("SemiTones").format_value(3., true, &LAWS.formatting),
            "3 st"
        );
        assert_eq!(
            display("Generic").format_value(1.125, false, &LAWS.formatting),
            "1.125"
        );
        assert_eq!(
            display("Generic").format_value(1.6, true, &LAWS.formatting),
            "2"
        );
        assert_eq!(
            display("").format_value(2.5, false, &LAWS.formatting),
            "2.5"
        );
    }

    #[test]
    fn uvi_layers_nest() {
        assert_eq!(LAWS.layering, Layering::Nested);
    }
}
