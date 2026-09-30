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
        let picture = Picture {
            frames,
            resizable: layout.resizable,
        };
        out.insert(name.to_owned(), Arc::new(picture));
    }
    out
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
}

impl Layout {
    fn parse(text: &str) -> Self {
        let mut layout = Self {
            frames: 1,
            horizontal: false,
            resizable: false,
        };
        for (key, value) in text.lines().filter_map(|l| l.split_once(':')) {
            let value = value.trim();
            let yes = value.eq_ignore_ascii_case("yes");
            let number = value.parse().unwrap_or(0);
            match key.trim().to_lowercase().as_str() {
                "number of animations" => layout.frames = number,
                "horizontal animation" => layout.horizontal = yes,
                "horizontal resizable" | "vertical resizable" => layout.resizable |= yes,
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
/// The artwork as a backdrop to play over: shrunk to a few dozen pixels
/// (which blurs it once scaled back up), blurred again, mostly drained of
/// color and darkened well below the text drawn on it.
pub fn backdrop(image: &Image) -> Option<Image> {
    const W: usize = 48;
    let (sw, sh) = (image.width as usize, image.height as usize);
    if sw == 0 || sh == 0 {
        return None;
    }
    let h = (W * sh / sw).clamp(1, W);
    // Area average into W x h.
    let mut px = vec![[0f32; 3]; W * h];
    let mut n = vec![0f32; W * h];
    for (i, c) in image.rgba.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (i % sw, i / sw);
        let at = (y * h / sh) * W + x * W / sw;
        for k in 0..3 {
            px[at][k] += f32::from(c[k]);
        }
        n[at] += 1.;
    }
    for (p, n) in px.iter_mut().zip(&n) {
        p.iter_mut().for_each(|v| *v /= n.max(1.));
    }
    // Two passes of a 3x3 box.
    for _ in 0..2 {
        let from = px.clone();
        for y in 0..h {
            for x in 0..W {
                let mut sum = [0f32; 3];
                let mut count = 0.;
                for (dx, dy) in (-1i32..=1).flat_map(|dx| (-1i32..=1).map(move |dy| (dx, dy))) {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if (0..W as i32).contains(&nx) && (0..h as i32).contains(&ny) {
                        let q = from[ny as usize * W + nx as usize];
                        (0..3).for_each(|k| sum[k] += q[k]);
                        count += 1.;
                    }
                }
                px[y * W + x] = sum.map(|v| v / count);
            }
        }
    }
    // Every backdrop settles at the same dim average, bright artwork or dark.
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
    Image::rgba(W as u32, h as u32, rgba)
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
                resizable: false,
            }
        );
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
    fn backdrops_settle_dim_and_small() {
        let bright = super::Image::rgba(300, 100, [250u8, 200, 40, 255].repeat(300 * 100)).unwrap();
        let b = super::backdrop(&bright).unwrap();
        assert_eq!((b.width, b.height), (48, 16));
        let px = &b.rgba[..4];
        assert!(px[0] < 45 && px[1] < 40, "darkened: {px:?}");
        assert!(px[0] - px[2] < 25, "mostly drained of color: {px:?}");
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
