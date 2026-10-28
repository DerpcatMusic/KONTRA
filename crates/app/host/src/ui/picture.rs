//! Kontakt pictures: `Resources/pictures/<name>.png` and the `.txt` beside
//! it that says how the image animates and stretches.

use std::path::Path;

use kontra_format_kontakt::Resources;
use kontra_ui_model as ui;

/// A library's resource files, read by tree path (`Resources/…`).
pub struct Library(Resources);

impl Library {
    /// The library the instrument at `path` belongs to.
    #[must_use]
    pub fn open(path: &Path) -> Self {
        Self(Resources::open(path))
    }

    /// The bytes of the file at a tree asset path.
    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        self.0.read(path.strip_prefix("Resources/").unwrap_or(path))
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
        let yes = value.eq_ignore_ascii_case("yes");
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
}
