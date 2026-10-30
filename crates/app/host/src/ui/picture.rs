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

/// A picture sidecar's lines (`Number of Animations: 31`, …).
#[must_use]
pub fn meta(txt: &str) -> ui::ImageMeta {
    let mut meta = ui::ImageMeta::default();
    for (key, value) in txt.lines().filter_map(|l| l.split_once(':')) {
        let value = value.trim();
        // A flag is yes when it starts with yes: Kontakt reads its own
        // sidecars loosely, and one ships the typo `Horizontal Resizable:
        // yesm` (Electric Mint's `dropdown_topright`). Read as no, that
        // picture does not stretch, and `layout` then gives its control the
        // picture's 32 px width instead of the 171 the script asked for --
        // which puts the preset menu's arrow under the field beside it.
        let yes = value.to_ascii_lowercase().starts_with("yes");
        let n = value.parse::<u32>().unwrap_or(0);
        match key.trim() {
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
}
