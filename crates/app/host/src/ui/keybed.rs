//! The keybed as a KSP script colours it (`set_key_color`), in the colours
//! Kontakt 8 draws: measured on its own keyboard with a probe that coloured
//! one key per `$KEY_COLOR_*` (oracle, 2026-10-29,
//! scratch/laneJ/keys/keys{,2}.png).

use kontra_engine::Key;
use kontra_ui_model::Rgba;

/// A white key's colour by `$KEY_COLOR_*` value; `None` draws the key as
/// it is (NONE 0, WHITE 1, INACTIVE 20). BLACK (7) is drawn inverted, see
/// [`keybed`].
const WHITE_KEY: [Option<u32>; 21] = [
    None,
    None,
    Some(0xDACD81), // WARM_YELLOW
    Some(0xB6DA81), // LIME
    Some(0xDA918F), // RED
    Some(0x95B0DA), // BLUE
    Some(0x81D2D2), // CYAN
    None,           // BLACK
    Some(0xDAA491), // ORANGE
    Some(0xD9B285), // LIGHT_ORANGE
    Some(0xDADA81), // YELLOW
    Some(0x98DA98), // GREEN
    Some(0x81D2B6), // MINT
    Some(0x81C7DA), // TURQUOISE
    Some(0xAAA3DA), // PLUM
    Some(0xBEA2DA), // VIOLET
    Some(0xCD9CDA), // PURPLE
    Some(0xDA98D4), // MAGENTA
    Some(0xDA93A9), // FUCHSIA
    Some(0x81C7DA), // DEFAULT: Kontakt's mapped-key blue
    None,           // INACTIVE
];

/// `$KEY_COLOR_BLACK`.
const BLACK: i32 = 7;

/// A black key draws its colour 98 darker in every channel (measured on
/// all 15 colours, within 1).
const BLACK_KEY_DARKER: u8 = 98;

/// The keys the script coloured and the colour each draws, by note.
pub(crate) fn keybed(keys: &[Key]) -> Vec<(u8, Rgba)> {
    (0u8..)
        .zip(keys)
        .filter_map(|(note, key)| Some((note, color(note, key.color)?)))
        .collect()
}

fn color(note: u8, value: i32) -> Option<Rgba> {
    let black_key = matches!(note % 12, 1 | 3 | 6 | 8 | 10);
    if value == BLACK {
        // Inverted: a white key draws black, a black key white.
        return Some(Rgba::rgb(if black_key { 0xC6C6C6 } else { 0x2D2D2D }));
    }
    let white = Rgba::rgb((*WHITE_KEY.get(usize::try_from(value).ok()?)?)?);
    let dark = |c: u8| c.saturating_sub(BLACK_KEY_DARKER);
    Some(if black_key {
        Rgba {
            r: dark(white.r),
            g: dark(white.g),
            b: dark(white.b),
            a: white.a,
        }
    } else {
        white
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kontakt 8's keyboard pixels for the probe's keys.
    #[test]
    fn script_key_colours_match_kontakt() {
        let rgb = |c: Rgba| (c.r, c.g, c.b);
        assert_eq!(
            color(40, 4).map(rgb),
            Some((218, 145, 143)),
            "red, white key"
        );
        assert_eq!(
            color(66, 4).map(rgb),
            Some((120, 47, 45)),
            "red, black key: 46 measured"
        );
        assert_eq!(
            color(39, 3).map(rgb),
            Some((84, 120, 31)),
            "lime, black key"
        );
        assert_eq!(
            color(55, 19).map(rgb),
            Some((129, 199, 218)),
            "default blue"
        );
        assert_eq!(
            color(43, 7).map(rgb),
            Some((45, 45, 45)),
            "black on a white key"
        );
        assert_eq!(color(38, 0), None, "none");
        assert_eq!(color(56, 20), None, "inactive");
        assert_eq!(color(56, 99), None, "out of range");
    }
}
