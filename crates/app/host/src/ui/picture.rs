//! Library pictures. Kontakt's: `Resources/pictures/<name>.png` and the
//! `.txt` beside it that says how the image animates and stretches. A UVI
//! script's: a path from its folder inside the program's bank.

use std::path::Path;

use kontra_format_kontakt::Resources;
use kontra_ui_model as ui;

/// A library's resource files, read by tree asset path.
pub enum Library {
    Kontakt(Resources),
    Uvi(Box<kontra_format_uvi::Resources>),
}

impl Library {
    /// The library the instrument at `path` belongs to.
    #[must_use]
    pub fn open(path: &Path) -> Self {
        if kontra_format_uvi::is_uvi(path) {
            Self::Uvi(Box::new(kontra_format_uvi::Resources::open(path)))
        } else {
            Self::Kontakt(Resources::open(path))
        }
    }

    /// The bytes of the file at a tree asset path.
    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        match self {
            Self::Kontakt(r) => r.read(path.strip_prefix("Resources/").unwrap_or(path)),
            Self::Uvi(r) => r.read(path),
        }
    }

    /// The layout of the picture at `path`: its sidecar, and one frame's
    /// size from the image header; `None` when the library has neither.
    // ponytail: reads the whole PNG for its 24-byte header; read a prefix
    // if tree builds show up in the load time.
    pub fn meta(&mut self, path: &str) -> Option<ui::ImageMeta> {
        let stem = path.strip_suffix(".png").unwrap_or(path);
        let txt = self.read(&format!("{stem}.txt"));
        let size = self.read(path).map(|png| size(&png));
        if txt.is_none() && size.is_none() {
            return None;
        }
        let mut meta = meta(&String::from_utf8_lossy(&txt.unwrap_or_default()));
        meta.size = size.flatten().map(|(w, h)| {
            let frames = f64::from(meta.frames);
            match meta.axis {
                ui::Orientation::Horizontal => ui::Size {
                    width: (w / frames).floor(),
                    height: h,
                },
                ui::Orientation::Vertical => ui::Size {
                    width: w,
                    height: (h / frames).floor(),
                },
            }
        });
        Some(meta)
    }
}

/// A PNG's pixel size, from its header.
fn size(png: &[u8]) -> Option<(f64, f64)> {
    if png.get(..8)? != b"\x89PNG\r\n\x1a\n" || png.get(12..16)? != b"IHDR" {
        return None;
    }
    let be = |at: usize| {
        Some(f64::from(u32::from_be_bytes(
            png.get(at..at + 4)?.try_into().ok()?,
        )))
    };
    Some((be(16)?, be(20)?))
}

/// The sidecar's keys, in the order Kontakt writes them.
const KEYS: [&str; 9] = [
    "Has Alpha Channel",
    "Number of Animations",
    "Horizontal Animation",
    "Horizontal Resizable",
    "Vertical Resizable",
    "Fixed Top",
    "Fixed Bottom",
    "Fixed Left",
    "Fixed Right",
];

/// A picture sidecar's fields (`Number of Animations: 31`, …).
///
/// Kontakt reads its own sidecars loosely: a field can run straight into the
/// next one with no line break — XPERIMENTA Harpsichord's `micsfader` carries
/// `Number of Animations: 127Horizontal Animation:  no` — so a value ends
/// where the next key starts, not at the end of its line. Read line by line,
/// that strip's 127 frames came out as one, and every fader showing it drew
/// the whole sprite sheet over the page instead of the one frame its position
/// names: five different mic levels, five identical drawings.
fn fields(txt: &str) -> Vec<(&'static str, &str)> {
    let lower = txt.to_ascii_lowercase();
    let mut at: Vec<(usize, &'static str)> = Vec::new();
    for &key in &KEYS {
        let needle = key.to_ascii_lowercase();
        for (i, _) in lower.match_indices(&needle) {
            at.push((i, key));
        }
    }
    at.sort_unstable_by_key(|&(i, _)| i);
    let mut fields = Vec::with_capacity(at.len());
    for (n, &(start, key)) in at.iter().enumerate() {
        let end = at.get(n + 1).map_or(txt.len(), |&(next, _)| next);
        // The colon must belong to *this* key. Searching the rest of the sidecar
        // for the first one took the colon out of the value of a key that has
        // none: `Fixed Right` followed by `Height: 9` set the right margin to 9.
        // Kontakt writes `Key: value` either on one line or with the next key
        // running straight into the value, so the colon is the first thing after
        // the key and any spaces or tabs — never past a line break.
        let after = start + key.len();
        let rest = txt.get(after..end).unwrap_or_default();
        let Some(value) = rest.trim_start_matches([' ', '\t']).strip_prefix(':') else {
            continue;
        };
        fields.push((key, value.trim()));
    }
    fields
}

/// A field's number: its leading digits. Kontakt reads `127Horizontal
/// Animation:  no` as 127 frames, and `0` before any run of digits as none.
fn number(value: &str) -> u32 {
    let digits: String = value
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap_or(0)
}

/// A picture sidecar's lines (`Number of Animations: 31`, …).
#[must_use]
pub fn meta(txt: &str) -> ui::ImageMeta {
    let mut meta = ui::ImageMeta::default();
    for (key, value) in fields(txt) {
        // A flag is yes when it starts with yes: Kontakt reads its own
        // sidecars loosely, and one ships the typo `Horizontal Resizable:
        // yesm` (Electric Mint's `dropdown_topright`). Read as no, that
        // picture does not stretch, and `layout` then gives its control the
        // picture's 32 px width instead of the 171 the script asked for --
        // which puts the preset menu's arrow under the field beside it.
        let yes = value.to_ascii_lowercase().starts_with("yes");
        let n = number(value);
        match key {
            "Has Alpha Channel" => meta.alpha = yes,
            "Number of Animations" => meta.frames = n.max(1),
            "Horizontal Animation" if yes => meta.axis = ui::Orientation::Horizontal,
            "Horizontal Resizable" => meta.stretch[0] = yes,
            "Vertical Resizable" => meta.stretch[1] = yes,
            "Fixed Top" => meta.margins.top = n,
            "Fixed Bottom" => meta.margins.bottom = n,
            "Fixed Left" => meta.margins.left = n,
            "Fixed Right" => meta.margins.right = n,
            _ => {}
        }
    }
    meta
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_sidecar_says_frames_axis_and_stretch_and_the_header_sizes_one_frame() {
        let m = super::meta(
            "Has Alpha Channel: no\nNumber of Animations: 31\nHorizontal Animation: yes\nVertical Resizable: yes\nFixed Top: 4\n",
        );
        assert_eq!(
            (m.frames, m.axis, m.alpha, m.stretch, m.margins.top),
            (
                31,
                super::ui::Orientation::Horizontal,
                false,
                [false, true],
                4
            )
        );
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(62u32.to_be_bytes());
        png.extend(31u32.to_be_bytes());
        assert_eq!(super::size(&png), Some((62., 31.)));
    }

    /// Electric Mint's `dropdown_topright` sidecar says `yesm`, and Kontakt
    /// stretches that picture: its menu keeps the script's 171 px width and
    /// the arrow sits at the field's right end.
    #[test]
    fn a_sidecar_flag_with_trailing_junk_is_still_yes() {
        let m = super::meta(
            "Horizontal Resizable: yesm\r\nVertical Resizable: no\r\nHorizontal Animation: yesm\r\n",
        );
        assert_eq!(m.stretch, [true, false], "yesm stretches, no does not");
        assert_eq!(m.axis, super::ui::Orientation::Horizontal);
        let m = super::meta("Has Alpha Channel: yesm\nHorizontal Resizable: no\n");
        assert_eq!((m.alpha, m.stretch), (true, [false, false]));
    }

    /// XPERIMENTA Harpsichord's `micsfader` sidecar runs its animation count
    /// into the next key: `Number of Animations: 127Horizontal Animation:
    /// no`. Read line by line that is not a number, so the 127-frame fader
    /// strip became one frame, its control took the whole 111×6096 sprite as
    /// its size, and five different mic levels drew the identical sheet.
    /// Kontakt reads the leading digits, and `Horizontal Animation: no`
    /// means frames stacked down — 6096/127 = 48 px each.
    #[test]
    fn a_sidecar_field_that_runs_into_the_next_one_keeps_its_number() {
        let m = super::meta(
            "Has Alpha Channel: yes \nNumber of Animations: 127Horizontal Animation:  no\nVertical Resizable: no\nHorizontal Resizable: no\nFixed Top: 0\nFixed Bottom: 0\nFixed Left: 0\nFixed Right:  0\n",
        );
        assert_eq!(m.frames, 127, "127 frames, not one");
        assert_eq!(m.axis, super::ui::Orientation::Vertical);
        assert_eq!((m.alpha, m.stretch), (true, [false, false]));
        assert_eq!(m.margins.right, 0, "a field ends where the next key starts");
    }

    /// A key with no colon at all must not borrow the colon from the line
    /// below: `Fixed Right` followed by `Height: 9` used to set the right
    /// margin to 9, because the search for the field's colon ran past the end
    /// of the key's own text.
    #[test]
    fn a_key_without_a_colon_takes_no_value_from_the_next_line() {
        let m = super::meta("Fixed Right\nHeight: 9\nFixed Top: 3\n");
        assert_eq!(m.margins.right, 0, "the colon belongs to `Height`");
        assert_eq!(m.margins.top, 3, "the well-formed field still reads");
        // A colon with only spaces before it still counts: Kontakt's writer
        // puts the value after `Key:` and a sidecar may pad the colon.
        let m = super::meta("Fixed Left : 7\n");
        assert_eq!(m.margins.left, 7);
    }

    /// A field still ends at the next key, so a line that is not one of
    /// Kontakt's (`Height: 1` after `Fixed Right: 0`) cannot be read as that
    /// field's number.
    #[test]
    fn a_field_reads_its_own_number_and_not_the_next_line_s() {
        let m = super::meta(
            "Number of Animations: 100\nVertical Resizable: no\nFixed Right:  0\nHeight: 1\n",
        );
        assert_eq!((m.frames, m.margins.right), (100, 0));
        assert_eq!(super::number("025"), 25, "a padded count keeps its value");
    }
}
