//! Read local library artwork once, on the import worker. No copies on disk.
use moose::mui::mui::scene::Image;
use std::{
    collections::HashMap,
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Each library's own artwork, by library name: a `wallpaper.png`, else
/// the product wallpaper in its `.nicnt`, else a panel-sized picture in a
/// resource container (`.nkr`) beside it.
pub fn scan(libraries: &[crate::library::Library]) -> HashMap<String, Arc<Image>> {
    libraries
        .iter()
        .filter_map(|library| {
            let (name, folder) = (library.name.clone(), &library.dir);
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
/// The dominant hue of a library's own pictures, for a library with no
/// artwork: loose pictures in its `Resources` folders, else those in a
/// resource container it can read. At most a dozen are looked at.
pub fn own_hue(dir: &Path) -> Option<f32> {
    let mut pictures: Vec<PathBuf> = ["Resources/pictures", "resources/pictures", "Resources", "resources"]
        .iter()
        .flat_map(|sub| std::fs::read_dir(dir.join(sub)).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
        .collect();
    pictures.sort();
    pictures.dedup();
    let loose = pictures.iter().take(12).filter_map(|p| decode(&read_bounded(p).ok()?));
    let mut hues: Vec<f32> = loose.filter_map(|i| tint(&i)).collect();
    if hues.is_empty() {
        let mut nkrs: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkr")))
            .collect();
        nkrs.sort();
        for nkr in nkrs.iter().take(2) {
            let Ok(mut f) = File::open(nkr) else { continue };
            let Ok(archive) = ni_file::nkr::Archive::read(&mut f) else { continue };
            // Only what reads without a key.
            let mut names: Vec<&String> = (archive.entries.iter())
                .filter(|(n, e)| !e.encoded && e.size < 4 << 20 && n.to_lowercase().ends_with(".png"))
                .map(|(n, _)| n)
                .collect();
            names.sort();
            for name in names.into_iter().take(12) {
                let Ok(bytes) = archive.read_entry(&mut f, name) else { continue };
                hues.extend(decode(&bytes).and_then(|i| tint(&i)));
            }
        }
    }
    // The hue most of them share: the median of the circle, near enough.
    hues.sort_by(f32::total_cmp);
    hues.get(hues.len() / 2).copied()
}

/// A PNG or JPEG picture the player chose, at most 32 MiB.
pub fn decode_file(path: &Path) -> Option<Image> {
    let bytes = read_bounded(path).ok()?;
    if bytes.starts_with(b"\x89PNG") {
        return decode(&bytes);
    }
    use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_max_width(8192)
        .set_max_height(8192);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(&bytes[..]), options);
    let rgba = decoder.decode().ok()?;
    let info = decoder.info()?;
    Image::rgba(u32::from(info.width), u32::from(info.height), rgba)
}

/// Resolve the selected preset's named wallpaper, never an arbitrary PNG in its NKR.
pub fn performance(
    instrument: &crate::import::Instrument,
    computed: Option<&str>,
) -> Result<Option<Arc<Image>>, String> {
    // A script that computed no name (an index it had not set yet) may
    // still name one plainly.
    let Some(name) = computed
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
        .or_else(|| wallpaper(&instrument.scripts))
    else {
        return Ok(None);
    };
    let filename = png_name(&name).ok_or("Invalid instrument wallpaper name")?;
    let bytes = Pictures::of(&instrument.path, "pictures")
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
    /// Stretches to the control across and down (its sidecar's
    /// "Horizontal" and "Vertical Resizable"); otherwise it keeps its own
    /// size that way.
    pub stretch: [bool; 2],
}

impl Picture {
    /// The size it draws at on a control `w` by `h`: its own, but along a
    /// way it stretches.
    pub fn size(&self, w: f64, h: f64) -> (f64, f64) {
        let f = &self.frames[0];
        (
            if self.stretch[0] { w } else { f64::from(f.width) },
            if self.stretch[1] { h } else { f64::from(f.height) },
        )
    }
}

/// The control pictures named in `names` that the preset's library has.
pub fn pictures<'a>(
    path: &Path,
    names: impl IntoIterator<Item = &'a str>,
) -> HashMap<String, Arc<Picture>> {
    let mut source = Pictures::of(path, "pictures");
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
        let picture = Picture {
            frames,
            stretch: layout.stretch,
        };
        out.insert(name.to_owned(), Arc::new(picture));
    }
    out
}

/// A copy of the `w` by `h` pixels at `x`, `y`; `None` when empty.
pub fn crop(image: &Image, x: u32, y: u32, w: u32, h: u32) -> Option<Arc<Image>> {
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
    stretch: [bool; 2],
}

impl Layout {
    fn parse(text: &str) -> Self {
        let mut layout = Self {
            frames: 1,
            horizontal: false,
            stretch: [false; 2],
        };
        for (key, value) in text.lines().filter_map(|l| l.split_once(':')) {
            let value = value.trim();
            let yes = value.eq_ignore_ascii_case("yes");
            let number = value.parse().unwrap_or(0);
            match key.trim().to_lowercase().as_str() {
                "number of animations" => layout.frames = number,
                "horizontal animation" => layout.horizontal = yes,
                "horizontal resizable" => layout.stretch[0] = yes,
                "vertical resizable" => layout.stretch[1] = yes,
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

/// `<dir>/Resources/<name>` and `<dir>/<name>`, matching each folder name
/// without case: libraries made on Windows or macOS spell them freely.
fn resource_dirs(dir: &Path, name: &str) -> Vec<PathBuf> {
    let child = |dir: &Path, want: &str| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(want))
            .map(|e| e.path())
            .collect()
    };
    let mut out: Vec<PathBuf> = child(dir, "Resources")
        .iter()
        .flat_map(|r| child(r, name))
        .collect();
    out.extend(child(dir, name));
    out
}

/// The text of a script slot linked to `name` (as Kontakt stores it: a bare
/// file name or any Windows or POSIX path ending in one) from the library's
/// `Resources/scripts`, loose or in its resource container.
pub fn linked_script(instrument: &Path, name: &str) -> Result<Option<Vec<u8>>, String> {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    if file.is_empty() || file == "." || file == ".." {
        return Ok(None);
    }
    Pictures::of(instrument, "scripts").read(file)
}

/// Where a preset's pictures come from: `Resources/pictures` folders near
/// it, else a resource container (`.nkr`) in or one folder below them.
struct Pictures {
    /// Loose picture files by lowercase name, nearest first.
    files: HashMap<String, PathBuf>,
    /// Containers to try in order, opened on first use.
    containers: Vec<PathBuf>,
    open: Vec<(File, ni_file::nkr::Archive)>,
    key: Option<Option<std::sync::Arc<dyn ni_file::nis::LibraryKey>>>,
    instrument: PathBuf,
    /// The `Resources` subfolder: `pictures` or `scripts`.
    folder: &'static str,
}

impl Pictures {
    fn of(instrument: &Path, folder_name: &'static str) -> Self {
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
            for dir in resource_dirs(folder, folder_name) {
                for e in std::fs::read_dir(dir)
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
            folder: folder_name,
        }
    }

    /// The bytes of `Resources/pictures/<file>`, if the library has it.
    fn read(&mut self, file: &str) -> Result<Option<Vec<u8>>, String> {
        if let Some(path) = self.files.get(&file.to_lowercase()) {
            return read_bounded(path).map(Some);
        }
        let member = format!("Resources/{}/{file}", self.folder);
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
                _ if !entry.encoded || entry.key_index == 0xff => &None,
                Some(key) => key,
                None => self.key.insert(
                    crate::access::library_key(&self.instrument).map_err(|e| e.to_string())?,
                ),
            };
            return archive
                .read_entry_with_key(f, &member, key.as_deref())
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
pub(crate) fn decode(bytes: &[u8]) -> Option<Image> {
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
/// `image` cropped to cover `w` × `h` from its middle and shrunk to it by
/// area averaging, once, so a list of thumbnails scales nothing per frame.
pub fn thumbnail(image: &Image, w: u32, h: u32) -> Option<Image> {
    let (sw, sh) = (image.width as f64, image.height as f64);
    if sw == 0. || sh == 0. || w == 0 || h == 0 {
        return None;
    }
    let scale = (f64::from(w) / sw).max(f64::from(h) / sh);
    let (cw, ch) = (f64::from(w) / scale, f64::from(h) / scale);
    let (x0, y0) = ((sw - cw) / 2., (sh - ch) / 2.);
    let px = image.rgba.as_chunks::<4>().0;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let (top, bottom) = (y0 + f64::from(y) * ch / f64::from(h), y0 + f64::from(y + 1) * ch / f64::from(h));
        for x in 0..w {
            let (left, right) = (x0 + f64::from(x) * cw / f64::from(w), x0 + f64::from(x + 1) * cw / f64::from(w));
            let (mut sum, mut n) = ([0u32; 4], 0u32);
            for sy in top as usize..(bottom.ceil() as usize).min(image.height as usize).max(top as usize + 1) {
                for sx in left as usize..(right.ceil() as usize).min(image.width as usize).max(left as usize + 1) {
                    let c = px[sy * image.width as usize + sx];
                    (0..4).for_each(|k| sum[k] += u32::from(c[k]));
                    n += 1;
                }
            }
            rgba.extend(sum.map(|s| (s / n.max(1)) as u8));
        }
    }
    Image::rgba(w, h, rgba)
}

/// The artwork as a header banner, after Kontakt 8's: cropped to cover
/// `w` x `h`, every banner brought to one dim level with its contrast and
/// color held down so a white title reads over any of it, and fading from
/// opaque at the left to nothing at the right. `blurred` softens it first,
/// so a logo in the artwork stops competing with the title.
pub fn banner(image: &Image, w: u32, h: u32, blurred: bool) -> Option<Image> {
    // The level every banner sits at, how much of its contrast and color
    // stays, and the brightest it gets (of 255).
    const LEVEL: f32 = 52.;
    const CONTRAST: f32 = 0.45;
    const COLOR: f32 = 0.55;
    const PEAK: f32 = 96.;
    let crop = thumbnail(image, w, h)?;
    let crop = if blurred { blur(&crop, (h / 24).max(2) as usize, 3)? } else { crop };
    let luma = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let pixels = crop.rgba.as_chunks::<4>().0;
    let rgb = |c: &[u8; 4]| [0, 1, 2].map(|k| f32::from(c[k]));
    let mean = pixels.iter().map(|c| luma(rgb(c))).sum::<f32>() / pixels.len() as f32;
    let gain = LEVEL / mean.max(1.);
    let rgba = pixels
        .iter()
        .enumerate()
        .flat_map(|(i, c)| {
            let c = rgb(c);
            let l = luma(c);
            // Contrast around the level, then a soft knee under the peak;
            // color only ever loses strength, dark artwork lifted or not.
            let lit = LEVEL + (l * gain - LEVEL) * CONTRAST;
            let lit = PEAK * (1. - (-lit.max(0.) / PEAK).exp()) * 1.3;
            let [r, g, b] = c.map(|v| (lit + (v - l) * COLOR * gain.min(1.)).clamp(0., 255.) as u8);
            let t = (i as u32 % w) as f32 / (w - 1).max(1) as f32;
            let s = ((t - 0.1) / 0.9).clamp(0., 1.);
            let fade = 1. - s * s * (3. - 2. * s);
            [r, g, b, (255. * fade) as u8]
        })
        .collect::<Vec<u8>>();
    Image::rgba(w, h, rgba)
}

/// The artwork as a backdrop to play over, mostly drained of color and
/// darkened well below the text drawn on it. `blurred` shrinks it to a few
/// dozen pixels first (which blurs it once scaled back up) and blurs it
/// again; sharp, it keeps up to a wide rack's worth of pixels.
pub fn backdrop(image: &Image, blurred: bool) -> Option<Image> {
    let (sw, sh) = (image.width, image.height);
    if sw == 0 || sh == 0 {
        return None;
    }
    let w = if blurred { 48 } else { sw.min(1280) };
    let h = (u64::from(w) * u64::from(sh) / u64::from(sw)).clamp(1, u64::from(w)) as u32;
    let small = thumbnail(image, w, h)?;
    let small = if blurred { blur(&small, 1, 2)? } else { small };
    // Every backdrop settles at the same dim average, bright artwork or dark.
    let px: Vec<[f32; 3]> = small.rgba.as_chunks::<4>().0.iter().map(|c| [0, 1, 2].map(|k| f32::from(c[k]))).collect();
    let luma = |[r, g, b]: [f32; 3]| 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let mean = px.iter().map(|p| luma(*p)).sum::<f32>() / px.len() as f32;
    let gain = (30. / mean.max(1.)).min(1.);
    let rgba = px
        .iter()
        .flat_map(|&[r, g, b]| {
            let luma = luma([r, g, b]);
            let tone = |c: f32| ((luma + (c - luma) * 0.45) * gain).round().clamp(0., 255.) as u8;
            [tone(r), tone(g), tone(b), 255]
        })
        .collect::<Vec<u8>>();
    Image::rgba(w, h, rgba)
}

/// `image` blurred by `passes` of a box `radius` pixels each way, across
/// then down: three passes are near enough a Gaussian. Edges repeat.
fn blur(image: &Image, radius: usize, passes: usize) -> Option<Image> {
    let (w, h) = (image.width as usize, image.height as usize);
    let mut px: Vec<[f32; 4]> = image.rgba.as_chunks::<4>().0.iter().map(|c| c.map(f32::from)).collect();
    let scale = 1. / (2 * radius + 1) as f32;
    for _ in 0..passes {
        for across in [true, false] {
            let (n, lines) = if across { (w, h) } else { (h, w) };
            let at = |line: usize, i: usize| if across { line * w + i } else { i * w + line };
            let mut out = vec![[0f32; 4]; n];
            for line in 0..lines {
                let get = |i: isize| px[at(line, i.clamp(0, n as isize - 1) as usize)];
                let r = radius as isize;
                let mut sum = [0f32; 4];
                for i in -r..=r {
                    let c = get(i);
                    (0..4).for_each(|k| sum[k] += c[k]);
                }
                for (i, o) in out.iter_mut().enumerate() {
                    *o = sum.map(|v| v * scale);
                    let (add, drop) = (get(i as isize + r + 1), get(i as isize - r));
                    (0..4).for_each(|k| sum[k] += add[k] - drop[k]);
                }
                for (i, o) in out.iter().enumerate() {
                    px[at(line, i)] = *o;
                }
            }
        }
    }
    let rgba: Vec<u8> = px.iter().flat_map(|c| c.map(|v| v.round().clamp(0., 255.) as u8)).collect();
    Image::rgba(w as u32, h as u32, rgba)
}

/// The artwork's identity color as an OKLCH hue in degrees: the most
/// common hue among its colorful pixels, ignoring near-black, near-white and
/// grey. `None` for artwork without a clear color.
pub fn tint(image: &Image) -> Option<f32> {
    const BINS: usize = 36;
    let mut weight = [0f32; BINS];
    let mut sums = [(0f32, 0f32); BINS];
    let pixels = image.rgba.chunks_exact(4);
    // A few thousand samples decide it as well as every pixel would.
    let step = (pixels.len() / 20_000).max(1);
    for px in pixels.step_by(step) {
        let [r, g, b, a] = [px[0], px[1], px[2], px[3]];
        let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
        if a < 128 || hi < 48 || lo > 208 || hi - lo < 40 {
            continue;
        }
        let (l, a, b) = oklab(r, g, b);
        let chroma = a.hypot(b);
        let hue = b.atan2(a).to_degrees().rem_euclid(360.);
        let bin = (hue / 360. * BINS as f32) as usize % BINS;
        weight[bin] += chroma * l;
        sums[bin].0 += chroma;
        sums[bin].1 += hue * chroma;
    }
    let total: f32 = weight.iter().sum();
    let (bin, top) = weight
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    // One colored logo on a grey picture is not the library's color.
    if total <= 0. || *top < total * 0.12 || *top < 1. {
        return None;
    }
    let (chroma, hue) = sums[bin];
    Some(hue / chroma)
}

/// sRGB bytes to OKLab.
fn oklab(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let lin = |c: u8| {
        let c = f32::from(c) / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    (
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    )
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
                stretch: [false; 2],
            }
        );
        // Windows line ends, spacing and case as libraries write them.
        let strip = super::Layout::parse("Has Alpha Channel: yes\r\nnumber of animations : 4\r\nHorizontal Animation: YES\r\nVertical Resizable: yes\r\nHorizontal Resizable: no\r\n");
        assert_eq!(strip, super::Layout { frames: 4, horizontal: true, stretch: [false, true] }, "a divider stretches down only");
        let across = super::Image::rgba(4, 1, (0..16).collect::<Vec<u8>>()).unwrap();
        assert_eq!(strip.cut(&across).iter().map(|f| f.rgba[0]).collect::<Vec<_>>(), [0, 4, 8, 12], "horizontal frames run left to right");
        assert_eq!(super::Layout::parse("Number of Animations: 0").frames, 1, "none is one");
        // A 1x3 strip: red, green, blue.
        let image =
            super::Image::rgba(1, 3, vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]).unwrap();
        let frames = layout.cut(&image);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[1].rgba.as_ref(), &[0, 255, 0, 255]);
        assert_eq!(super::Layout::parse("").frames, 1);
        assert_eq!(super::png_name("../x"), None);
        assert_eq!(super::png_name("knob").as_deref(), Some("knob.png"));
    }

    #[test]
    fn thumbnails_cover_from_the_middle() {
        // Wide artwork: left third red, middle green, right third blue.
        let rgba: Vec<u8> = (0..60)
            .flat_map(|x| match x / 20 {
                0 => [255, 0, 0, 255],
                1 => [0, 255, 0, 255],
                _ => [0, 0, 255, 255],
            })
            .collect::<Vec<u8>>()
            .repeat(20);
        let image = super::Image::rgba(60, 20, rgba).unwrap();
        let t = super::thumbnail(&image, 2, 2).unwrap();
        assert_eq!((t.width, t.height), (2, 2));
        assert_eq!(&t.rgba[..4], &[0, 255, 0, 255], "a square crop keeps the middle");
    }

    #[test]
    fn banners_fade_right_and_stay_dim() {
        let white = super::Image::rgba(40, 10, vec![255u8; 40 * 10 * 4]).unwrap();
        for blurred in [false, true] {
            let b = super::banner(&white, 20, 4, blurred).unwrap();
            let px = b.rgba.as_chunks::<4>().0;
            assert_eq!((px[0][3], px[19][3]), (255, 0), "opaque at the left, gone at the right");
            assert!(px.iter().all(|p| p[..3].iter().all(|&c| c < 110)), "white artwork dims under a title");
        }
    }

    #[test]
    fn backdrops_settle_dim_and_small() {
        let bright = super::Image::rgba(300, 100, [250u8, 200, 40, 255].repeat(300 * 100)).unwrap();
        for (blurred, size) in [(true, (48, 16)), (false, (300, 100))] {
            let b = super::backdrop(&bright, blurred).unwrap();
            assert_eq!((b.width, b.height), size);
            let px = &b.rgba[..4];
            assert!(px[0] < 45 && px[1] < 40, "darkened: {px:?}");
            assert!(px[0] - px[2] < 25, "mostly drained of color: {px:?}");
        }
    }

    #[test]
    fn blur_spreads_an_edge() {
        let half: Vec<u8> = (0..10).flat_map(|x| if x < 5 { [0u8, 0, 0, 255] } else { [200, 200, 200, 255] }).collect();
        let image = super::Image::rgba(10, 1, half).unwrap();
        let b = super::blur(&image, 2, 3).unwrap();
        let px = b.rgba.as_chunks::<4>().0;
        assert!(px[4][0] > 40 && px[5][0] < 160, "the edge softens: {:?}", &px[3..7]);
        assert!(px[0][0] < 40 && px[9][0] > 160 && px[0][3] == 255, "the ends keep their color: {px:?}");
    }

    #[test]
    fn tint_finds_the_dominant_color() {
        let px = |rgb: [u8; 3], n: usize| rgb.into_iter().chain([255]).cycle().take(4 * n);
        // Mostly black and white with a strong blue, and a little red.
        let rgba: Vec<u8> = px([0, 0, 0], 400)
            .chain(px([255, 255, 255], 300))
            .chain(px([30, 80, 220], 250))
            .chain(px([220, 30, 30], 50))
            .collect();
        let image = super::Image::rgba(1000, 1, rgba).unwrap();
        let hue = super::tint(&image).unwrap();
        assert!((250.0..275.0).contains(&hue), "blue, not {hue}");
        let grey: Vec<u8> = px([128, 128, 128], 100).collect();
        assert_eq!(super::tint(&super::Image::rgba(100, 1, grey).unwrap()), None);
    }
}
