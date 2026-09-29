//! Read local library artwork once, on the import worker. No copies on disk.
use moose::mui::mui::scene::Image;
use std::{
    collections::HashMap,
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

pub fn scan(root: &Path, files: &[PathBuf]) -> HashMap<String, Arc<Image>> {
    let names: std::collections::BTreeSet<_> = files
        .iter()
        .filter_map(|p| {
            p.strip_prefix(root)
                .ok()?
                .components()
                .next()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
        })
        .collect();
    names
        .into_iter()
        .filter_map(|name| {
            let folder = root.join(&name);
            let mut candidates = vec![folder.join("wallpaper.png")];
            if let Ok(entries) = std::fs::read_dir(&folder) {
                let mut containers: Vec<_> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().is_some_and(|e| {
                            e.eq_ignore_ascii_case("nicnt") || e.eq_ignore_ascii_case("nkr")
                        })
                    })
                    .collect();
                containers.sort_by_key(|p| {
                    (
                        !p.extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("nicnt")),
                        p.clone(),
                    )
                });
                candidates.extend(containers);
            }
            for path in candidates {
                let mut bytes = Vec::new();
                if File::open(path)
                    .and_then(|f| f.take(8 * 1024 * 1024).read_to_end(&mut bytes))
                    .is_err()
                {
                    continue;
                }
                // NICNT product wallpaper is a complete PNG following the product metadata.
                for (start, _) in bytes
                    .windows(8)
                    .enumerate()
                    .filter(|(_, b)| *b == b"\x89PNG\r\n\x1a\n")
                {
                    if let Some(image) = decode(&bytes[start..]) {
                        if image.width >= 180 && image.height >= 60 {
                            return Some((name, Arc::new(image)));
                        }
                    }
                }
            }
            None
        })
        .collect()
}
/// Resolve the selected preset's named wallpaper, never an arbitrary PNG in its NKR.
pub fn performance(
    instrument: &crate::import::Instrument,
    computed: Option<&str>,
) -> Result<Option<Arc<Image>>, String> {
    let Some(name) = computed
        .map(str::to_owned)
        .or_else(|| wallpaper(&instrument.scripts))
    else {
        return Ok(None);
    };
    let filename = png_name(&name).ok_or("Invalid instrument wallpaper name")?;
    let bytes = Pictures::of(&instrument.path)
        .read(&filename)?
        .ok_or_else(|| format!("Instrument wallpaper {filename} was not found"))?;
    decode(&bytes)
        .map(|i| Some(Arc::new(i)))
        .ok_or_else(|| "Invalid instrument wallpaper PNG".into())
}

/// `name` as a picture file name, unless it tries to leave the pictures folder.
fn png_name(name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return None;
    }
    Some(if name.to_lowercase().ends_with(".png") {
        name.to_owned()
    } else {
        format!("{name}.png")
    })
}

/// A control picture, cut into its animation frames.
#[derive(Debug)]
pub struct Picture {
    pub frames: Vec<Arc<Image>>,
    /// Stretches to the control; otherwise it keeps its own size.
    pub resizable: bool,
    /// Per frame, a stretchable picture's nine pieces, row by row: corners
    /// and edges keep the sidecar's fixed sizes. Empty when nothing is fixed.
    pub pieces: Vec<[Option<Arc<Image>>; 9]>,
    /// Fixed top, bottom, left and right edges, in picture pixels.
    pub fixed: [u32; 4],
}

/// The control pictures named in `names` that the preset's library has.
pub fn pictures<'a>(
    path: &Path,
    names: impl IntoIterator<Item = &'a str>,
) -> HashMap<String, Arc<Picture>> {
    let mut source = Pictures::of(path);
    let mut out = HashMap::new();
    for name in names {
        if out.contains_key(name) {
            continue;
        }
        let Some(file) = png_name(name) else { continue };
        let Some(image) = source.read(&file).ok().flatten().and_then(|b| decode(&b)) else {
            continue;
        };
        let sidecar = format!("{}.txt", &file[..file.len() - 4]);
        let text = source.read(&sidecar).ok().flatten().unwrap_or_default();
        let layout = Layout::parse(&String::from_utf8_lossy(&text));
        let frames = layout.cut(&image);
        if frames.is_empty() {
            continue;
        }
        let pieces = if layout.resizable && layout.fixed.iter().any(|&f| f > 0) {
            frames.iter().map(|f| nine(f, layout.fixed)).collect()
        } else {
            Vec::new()
        };
        let picture = Picture {
            frames,
            resizable: layout.resizable,
            pieces,
            fixed: layout.fixed,
        };
        out.insert(name.to_owned(), Arc::new(picture));
    }
    out
}

/// `image` cut into three rows and columns at its fixed edges.
fn nine(image: &Image, [top, bottom, left, right]: [u32; 4]) -> [Option<Arc<Image>>; 9] {
    let (w, h) = (image.width, image.height);
    let (left, right) = (left.min(w), right.min(w - left.min(w)));
    let (top, bottom) = (top.min(h), bottom.min(h - top.min(h)));
    let cols = [(0, left), (left, w - left - right), (w - right, right)];
    let rows = [(0, top), (top, h - top - bottom), (h - bottom, bottom)];
    std::array::from_fn(|i| {
        let ((x, cw), (y, rh)) = (cols[i % 3], rows[i / 3]);
        crop(image, x, y, cw, rh)
    })
}

/// A copy of the `w` by `h` pixels at `x`, `y`; `None` when empty.
fn crop(image: &Image, x: u32, y: u32, w: u32, h: u32) -> Option<Arc<Image>> {
    if w == 0 || h == 0 || x + w > image.width || y + h > image.height {
        return None;
    }
    let stride = image.width as usize * 4;
    let rgba: Vec<u8> = (y as usize..(y + h) as usize)
        .flat_map(|row| {
            let at = row * stride + x as usize * 4;
            image.rgba[at..at + w as usize * 4].iter().copied()
        })
        .collect();
    Image::rgba(w, h, rgba).map(Arc::new)
}

/// A picture's sidecar `.txt`: how many frames it holds and how they run.
#[derive(Debug, PartialEq)]
struct Layout {
    frames: u32,
    horizontal: bool,
    resizable: bool,
    /// Top, bottom, left, right.
    fixed: [u32; 4],
}

impl Layout {
    fn parse(text: &str) -> Self {
        let mut layout = Self {
            frames: 1,
            horizontal: false,
            resizable: false,
            fixed: [0; 4],
        };
        for (key, value) in text.lines().filter_map(|l| l.split_once(':')) {
            let value = value.trim();
            let yes = value.eq_ignore_ascii_case("yes");
            let number = value.parse().unwrap_or(0);
            match key.trim().to_lowercase().as_str() {
                "number of animations" => layout.frames = number,
                "horizontal animation" => layout.horizontal = yes,
                "horizontal resizable" | "vertical resizable" => layout.resizable |= yes,
                "fixed top" => layout.fixed[0] = number,
                "fixed bottom" => layout.fixed[1] = number,
                "fixed left" => layout.fixed[2] = number,
                "fixed right" => layout.fixed[3] = number,
                _ => {}
            }
        }
        layout.frames = layout.frames.max(1);
        layout
    }

    fn cut(&self, image: &Image) -> Vec<Arc<Image>> {
        let n = self.frames;
        let (fw, fh) = if self.horizontal {
            (image.width / n, image.height)
        } else {
            (image.width, image.height / n)
        };
        (0..n)
            .map_while(|f| {
                let (x, y) = if self.horizontal {
                    (f * fw, 0)
                } else {
                    (0, f * fh)
                };
                crop(image, x, y, fw, fh)
            })
            .collect()
    }
}

/// Where a preset's pictures come from: `Resources/pictures` folders near
/// it, else a resource container (`.nkr`) in or one folder below them.
struct Pictures {
    /// Loose picture files by lowercase name, nearest first.
    files: HashMap<String, PathBuf>,
    /// Containers to try in order, opened on first use.
    containers: Vec<PathBuf>,
    open: Vec<(File, ni_file::nkr::Archive)>,
    key: Option<Option<ni_file::nis::LibraryKey>>,
    instrument: PathBuf,
}

impl Pictures {
    fn of(instrument: &Path) -> Self {
        let mut files = HashMap::new();
        let mut containers = Vec::new();
        let nkrs = |dir: &Path| {
            let mut found: Vec<_> = std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkr")))
                .collect();
            found.sort();
            found
        };
        for folder in instrument.ancestors().skip(1).take(4) {
            for relative in ["Resources/pictures", "resources/pictures", "pictures"] {
                for e in std::fs::read_dir(folder.join(relative))
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    files.entry(name).or_insert_with(|| e.path());
                }
            }
            containers.extend(nkrs(folder));
            // Some libraries keep their container with the samples.
            let mut subfolders: Vec<_> = std::fs::read_dir(folder)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            subfolders.sort();
            for sub in subfolders {
                containers.extend(nkrs(&sub));
            }
        }
        containers.dedup();
        Self {
            files,
            containers,
            open: Vec::new(),
            key: None,
            instrument: instrument.into(),
        }
    }

    /// The bytes of `Resources/pictures/<file>`, if the library has it.
    fn read(&mut self, file: &str) -> Result<Option<Vec<u8>>, String> {
        if let Some(path) = self.files.get(&file.to_lowercase()) {
            return read_bounded(path).map(Some);
        }
        let member = format!("Resources/pictures/{file}");
        for n in 0.. {
            if n == self.open.len() {
                let Some(path) = (!self.containers.is_empty()).then(|| self.containers.remove(0))
                else {
                    return Ok(None);
                };
                let Ok(mut f) = File::open(&path) else {
                    continue;
                };
                let Ok(archive) = ni_file::nkr::Archive::read(&mut f) else {
                    continue;
                };
                self.open.push((f, archive));
            }
            let (f, archive) = &mut self.open[n];
            let Some(entry) = archive.find(&member) else {
                continue;
            };
            if entry.size > 32 * 1024 * 1024 {
                return Err(format!("{file} exceeds 32 MiB"));
            }
            let key = match &self.key {
                Some(key) => key,
                None => self.key.insert(
                    crate::import::library_key(&self.instrument).map_err(|e| e.to_string())?,
                ),
            };
            return archive
                .read_entry_with_key(f, &member, key.as_ref())
                .map(Some)
                .map_err(|e| e.to_string());
        }
        Ok(None)
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(32 * 1024 * 1024).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}
fn wallpaper(scripts: &[String]) -> Option<String> {
    scripts
        .iter()
        .flat_map(|s| s.lines())
        .filter_map(|line| {
            let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
            let value = compact
                .strip_prefix("set_control_par_str($INST_WALLPAPER_ID,$CONTROL_PAR_PICTURE,\"")?;
            // Preserve spaces in the original filename.
            if !value.ends_with("\")") {
                return None;
            }
            let start = line.find('"')? + 1;
            let end = line.rfind('"')?;
            Some(line[start..end].to_owned())
        })
        .last()
        .filter(|name| !name.is_empty())
}
fn decode(bytes: &[u8]) -> Option<Image> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let size = reader.output_buffer_size()?;
    if size > 32 * 1024 * 1024 {
        return None;
    }
    let mut data = vec![0; size];
    let info = reader.next_frame(&mut data).ok()?;
    let data = &data[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|v| [*v, *v, *v, 255]).collect(),
        png::ColorType::GrayscaleAlpha => data
            .chunks_exact(2)
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        _ => return None,
    };
    Image::rgba(info.width, info.height, rgba)
}
#[cfg(test)]
mod tests {
    #[test]
    fn named_wallpaper() {
        assert_eq!(
            super::wallpaper(&[
                "set_control_par_str($INST_WALLPAPER_ID, $CONTROL_PAR_PICTURE, \"A B\")".into()
            ]),
            Some("A B".into())
        );
        assert_eq!(
            super::wallpaper(&[
                "set_control_par_str($INST_WALLPAPER_ID,$CONTROL_PAR_PICTURE,@dynamic)".into()
            ]),
            None
        );
    }
    #[test]
    fn png_decode_checks_input_and_keeps_alpha() {
        let mut bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut bytes, 1, 1);
            e.set_color(png::ColorType::Rgba);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&[12, 34, 56, 78]).unwrap();
        }
        assert_eq!(
            super::decode(&bytes).unwrap().rgba.as_ref(),
            &[12, 34, 56, 78]
        );
        assert!(super::decode(&bytes[..12]).is_none());
        assert!(super::decode(b"not an image").is_none());
    }

    #[test]
    fn picture_frames() {
        let layout = super::Layout::parse(
            "Has Alpha Channel: yes\nNumber of Animations: 3\nHorizontal Animation: no\nVertical Resizable: no\nHorizontal Resizable: no\n",
        );
        assert_eq!(
            layout,
            super::Layout {
                frames: 3,
                horizontal: false,
                resizable: false,
                fixed: [0; 4],
            }
        );
        // A 1x3 strip: red, green, blue.
        let image =
            super::Image::rgba(1, 3, vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]).unwrap();
        let frames = layout.cut(&image);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[1].rgba.as_ref(), &[0, 255, 0, 255]);
        assert_eq!(super::Layout::parse("").frames, 1);
        // A 3x3 picture with one fixed pixel on every edge cuts into single pixels.
        let rgba: Vec<u8> = (0..9u8).flat_map(|n| [n, 0, 0, 255]).collect();
        let pieces = super::nine(&super::Image::rgba(3, 3, rgba).unwrap(), [1, 1, 1, 1]);
        let firsts: Vec<u8> = pieces.iter().map(|p| p.as_ref().unwrap().rgba[0]).collect();
        assert_eq!(firsts, (0..9).collect::<Vec<u8>>());
        let edges = super::nine(
            &super::Image::rgba(3, 3, vec![0; 36]).unwrap(),
            [0, 0, 1, 1],
        );
        assert!(
            edges[0].is_none() && edges[3].is_some(),
            "no fixed rows, one middle row"
        );
        assert_eq!(super::png_name("../x"), None);
        assert_eq!(super::png_name("knob").as_deref(), Some("knob.png"));
    }
}
