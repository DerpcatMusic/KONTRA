//! Library pictures. Kontakt's: `Resources/pictures/<name>.png` and the
//! `.txt` beside it that says how the image animates and stretches. A UVI
//! script's: a path from its folder inside the program's bank.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

    /// Whether it found anything to read a picture from at all: a folder or
    /// container at or under the instrument's library root. An instrument
    /// whose file sits outside its library has none, and then every picture
    /// is missing.
    #[must_use]
    pub fn found(&self) -> bool {
        match self {
            Self::Kontakt(r) => r.found(),
            Self::Uvi(r) => r.found(),
        }
    }

    /// The library of the first of `paths` that has resources, tried in
    /// order: a part's own file, then the instrument it was loaded from (a
    /// preset may sit outside its library, or have moved).
    #[must_use]
    pub fn open_any(paths: impl IntoIterator<Item = Option<PathBuf>>) -> Option<Self> {
        paths
            .into_iter()
            .flatten()
            .map(|path| Self::open(&path))
            .find(Library::found)
    }

    /// The bytes of the file at a tree asset path.
    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        match self {
            Self::Kontakt(r) => r.read(path.strip_prefix("Resources/").unwrap_or(path)),
            Self::Uvi(r) => r.read(path),
        }
    }
}

/// Why a picture could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    /// The part has no instrument file: one built in memory, or a state that
    /// lost it.
    NoPath,
    /// Nothing at the instrument's library root holds resources: the file is
    /// not inside its library, the library moved, or it is not installed.
    NoLibrary,
    /// The library was found and holds no file at that path.
    NoFile,
}

impl fmt::Display for Missing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoPath => "the part has no instrument path",
            Self::NoLibrary => "no picture library at the instrument's library root",
            Self::NoFile => "the library has no file at that path",
        })
    }
}

enum Opened {
    Unopened,
    Absent(Missing),
    Ready(Library),
}

/// A face's pictures, read by the worker that asks: the library of the first
/// of its `paths` that has resources, opened once. Every failure goes to
/// `told` with the reason and the asset path, so a page that draws no
/// pictures says why instead of drawing stock shapes and nothing else. The
/// reader runs off the UI thread; `told` must be safe there.
pub struct Loader {
    paths: Vec<PathBuf>,
    opened: Mutex<Opened>,
    told: Box<dyn Fn(Missing, &str) + Send + Sync>,
}

impl Loader {
    /// A reader of `paths` (a part's file, then the instrument it came from),
    /// telling `told` of every picture it could not read.
    #[must_use]
    pub fn new(
        paths: impl IntoIterator<Item = Option<PathBuf>>,
        told: Box<dyn Fn(Missing, &str) + Send + Sync>,
    ) -> Self {
        Self {
            paths: paths.into_iter().flatten().collect(),
            opened: Mutex::new(Opened::Unopened),
            told,
        }
    }

    /// The bytes of the picture at the tree asset path, or `None` (told to
    /// `told` why).
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let missing = {
            let mut opened = self.opened.lock().unwrap_or_else(|e| e.into_inner());
            if let Opened::Unopened = *opened {
                let found = (self.paths.iter())
                    .map(|at| Library::open(at))
                    .find(Library::found);
                *opened = match found {
                    Some(library) => Opened::Ready(library),
                    None => Opened::Absent(if self.paths.is_empty() {
                        Missing::NoPath
                    } else {
                        Missing::NoLibrary
                    }),
                };
            }
            match &mut *opened {
                Opened::Ready(library) => match library.read(path) {
                    Some(bytes) => return Some(bytes),
                    None => Missing::NoFile,
                },
                Opened::Absent(missing) => *missing,
                Opened::Unopened => Missing::NoLibrary,
            }
        };
        (self.told)(missing, path);
        None
    }
}

impl Library {
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
