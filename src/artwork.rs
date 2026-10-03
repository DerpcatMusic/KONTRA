//! Read local library artwork once, on the import worker. No copies on disk.
use crate::resources::{Resources as Pictures, read_file};
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
                    .and_then(|mut f| f.read_to_end(&mut bytes))
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
    let loose = pictures.iter().take(12).filter_map(|p| decode(&read_file(p).ok()?));
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
            let Ok(mut archive) = ni_file::nkr::Archive::read_index(&mut f) else { continue };
            // Only what reads without a key.
            let mut names: Vec<String> = (archive.entries.keys())
                .filter(|n| n.ends_with(".png"))
                .cloned()
                .collect();
            names.sort();
            let mut eligible = 0;
            for name in names {
                let Ok(Some(entry)) = archive.member(&mut f, &name) else { continue };
                if !entry.valid || entry.encoded || entry.size >= 4 << 20 { continue; }
                archive.entries.insert(name.clone(), entry);
                eligible += 1;
                let bytes = archive.read_entry(&mut f, &name);
                if let Ok(bytes) = bytes {
                    hues.extend(decode(&bytes).and_then(|i| tint(&i)));
                }
                if eligible == 12 { break; }
            }
        }
    }
    // The hue most of them share: the median of the circle, near enough.
    hues.sort_by(f32::total_cmp);
    hues.get(hues.len() / 2).copied()
}

/// A PNG or JPEG picture the player chose, at most 32 MiB.
pub fn decode_file(path: &Path) -> Option<Image> {
    decode_report(&read_file(path).ok()?).ok()
}

/// Resolve the selected preset's named wallpaper, never an arbitrary picture in its NKR.
pub fn performance(
    instrument: &crate::import::Instrument,
    computed: Option<&crate::ksp::Interface>,
) -> Result<Option<Arc<Picture>>, String> {
    let Some(name) = computed
        .map(|i| i.wallpaper.clone()).filter(|s| !s.is_empty())
        .or_else(|| wallpaper(&instrument.scripts))
    else {
        return Ok(None);
    };
    let filename = picture_name(&name).ok_or("Invalid instrument wallpaper name")?;
    let mut source = Pictures::of(&instrument.path, "pictures");
    let bytes = source
        .read(&filename)?
        .ok_or_else(|| format!("Instrument wallpaper {filename} was not found"))?;
    let image = decode_report(&bytes).map_err(|e| format!("Instrument wallpaper {filename}: {e}"))?;
    let text = source
        .read(&format!("{}.txt", filename.rsplit_once('.').unwrap().0))?
        .unwrap_or_default();
    let layout = Layout::parse(&String::from_utf8_lossy(&text));
    let (fw, fh) = layout.frame_size(&image);
    if fw == 0 || fh == 0 {
        return Err("Invalid instrument wallpaper frames".into());
    }
    // Retain the original atlas once. Pixel offsets can start between animation frames.
    Ok(Some(Arc::new(Picture {
        frames: vec![Arc::new(image)],
        stretch: layout.stretch,
        atlas: Some([fw, fh]),
    })))
}

/// `name` as a picture file name, unless it tries to leave the pictures folder.
fn picture_name(name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    Some(if [".png", ".jpg", ".jpeg"].iter().any(|suffix| lower.ends_with(suffix)) {
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
    /// Wallpaper frame dimensions in the full image retained in `frames[0]`.
    /// Control pictures use separate frames and leave this unset.
    pub atlas: Option<[u32; 2]>,
}

impl Picture {
    /// Select a wallpaper frame and an additional authored vertical pixel offset.
    /// The returned origin is sampled/drawn from the same image, without copying pixels.
    pub fn wallpaper(&self, state: i32, offset: i32) -> Option<(Arc<Image>, [u32; 2])> {
        if let Some([fw, fh]) = self.atlas {
            let image = self.frames.first()?.clone();
            if fw == 0 || fh == 0 { return None; }
            let across = (image.width / fw).max(1);
            let down = (image.height / fh).max(1);
            let state = (state.max(0) as u32).min(across.saturating_mul(down).saturating_sub(1));
            let origin = [state % across * fw, (state / across * fh).saturating_add(offset.max(0) as u32).min(image.height)];
            Some((image, origin))
        } else {
            let frame = self.frames.get((state.max(0) as usize).min(self.frames.len().saturating_sub(1)))?.clone();
            let y = (offset.max(0) as u32).min(frame.height);
            Some((frame, [0, y]))
        }
    }

    /// The size it draws at on a control `w` by `h`: its own, but along a
    /// way it stretches.
    pub fn size(&self, w: f64, h: f64) -> (f64, f64) {
        let f = &self.frames[0];
        let [fw, fh] = self.atlas.unwrap_or([f.width, f.height]);
        (
            if self.stretch[0] { w } else { f64::from(fw) },
            if self.stretch[1] { h } else { f64::from(fh) },
        )
    }
}

/// The control pictures named in `names` that the preset's library has.
pub fn pictures<'a>(path: &Path, names: impl IntoIterator<Item = &'a str>) -> HashMap<String, Arc<Picture>> {
    pictures_report(path, names).0
}

/// Font resources share the existing off-thread picture loading/cache path.
/// The internal prefix cannot collide with a valid plain picture filename.
pub fn font_key(name: &str) -> String { format!("@font/{name}") }

pub fn picture_names(interface: &crate::ksp::Interface) -> impl Iterator<Item = std::borrow::Cow<'_, str>> {
    interface.controls.iter().filter_map(|c| match c.properties.get("$CONTROL_PAR_PICTURE") {
        Some(crate::ksp::Value::Text(name)) if !name.is_empty() => Some(std::borrow::Cow::Borrowed(name.as_str())),
        _ => None,
    }).chain(interface.fonts.iter().map(|name| std::borrow::Cow::Owned(font_key(name))))
}

/// Kontakt bitmap fonts place 256 Windows-1252 glyphs side by side. A
/// fully red pixel starts each glyph on the metadata row, which is not drawn.
/// https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands
pub(crate) fn font_frames(image: &Image) -> Result<Vec<Arc<Image>>, String> {
    if image.height < 2 { return Err("bitmap font has no glyph rows".into()); }
    let starts: Vec<u32> = (0..image.width).filter(|&x| {
        let at = x as usize * 4;
        image.rgba[at..at + 3] == [255, 0, 0]
    }).collect();
    if starts.len() != 256 || starts[0] != 0 {
        return Err(format!("bitmap font needs 256 red glyph markers starting at x=0; found {}", starts.len()));
    }
    starts.iter().enumerate().map(|(n, &x)| {
        crop(image, x, 1, starts.get(n + 1).copied().unwrap_or(image.width) - x, image.height - 1)
            .ok_or_else(|| "invalid bitmap glyph dimensions".into())
    }).collect()
}

/// Unicode text is indexed by the font's documented Windows-1252 byte order.
/// Characters outside that alphabet use its authored question-mark glyph.
pub(crate) fn font_glyph(c: char) -> usize {
    const EXTENDED: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
        '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match c as u32 {
        0..=127 | 160..=255 => c as usize,
        _ => EXTENDED.iter().position(|&glyph| glyph == c).map_or(b'?' as usize, |n| n + 128),
    }
}

/// Preserve why a named control picture failed instead of silently discarding it.
pub fn pictures_report<'a>(
    path: &Path,
    names: impl IntoIterator<Item = &'a str>,
) -> (HashMap<String, Arc<Picture>>, Vec<String>) {
    let mut source = Pictures::of(path, "pictures");
    let mut out = HashMap::new();
    let mut errors = Vec::new();
    let mut attempted = std::collections::HashSet::new();
    for name in names {
        if name.is_empty() || !attempted.insert(name) { continue; }
        let result = (|| -> Result<Picture, String> {
            let font = name.strip_prefix("@font/");
            let file = picture_name(font.unwrap_or(name)).ok_or_else(|| format!("Picture {name:?}: invalid resource name"))?;
            let bytes = source.read(&file)?.ok_or_else(|| format!("Picture {file:?}: not found in the library resources or archives"))?;
            let image = decode_report(&bytes).map_err(|e| format!("Picture {file:?}: {e}"))?;
            let sidecar = format!("{}.txt", file.rsplit_once('.').unwrap().0);
            let text = source.read(&sidecar)?;
            if font.is_some() && text.is_none() { return Err(format!("Bitmap font {file:?}: required {sidecar:?} is missing")); }
            let layout = Layout::parse(&String::from_utf8_lossy(&text.unwrap_or_default()));
            let frames = if font.is_some() {
                if layout.frames != 1 { return Err(format!("Bitmap font {file:?}: animated font strips are unsupported")); }
                font_frames(&image).map_err(|e| format!("Bitmap font {file:?}: {e}"))?
            } else { layout.cut(&image) };
            if frames.is_empty() { return Err(format!("Picture {file:?}: invalid frame dimensions in {sidecar:?}")); }
            Ok(Picture { frames, stretch: layout.stretch, atlas: None })
        })();
        match result {
            Ok(picture) => { out.insert(name.to_owned(), Arc::new(picture)); }
            Err(e) => errors.push(if name.starts_with("@font/") {
                format!("{name}: {e}; using bundled Noto Sans")
            } else { format!("{name}: {e}") }),
        }
    }
    (out, errors)
}

/// A copy of the `w` by `h` pixels at `x`, `y`; `None` when empty.
pub fn crop(image: &Image, x: u32, y: u32, w: u32, h: u32) -> Option<Arc<Image>> {
    if w == 0 || h == 0 || x.checked_add(w)? > image.width || y.checked_add(h)? > image.height {
        return None;
    }
    if x == 0 && y == 0 && w == image.width && h == image.height {
        return Some(Arc::new(image.clone()));
    }
    let stride = image.width as usize * 4;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact((w as usize).checked_mul(h as usize)?.checked_mul(4)?).ok()?;
    for row in y as usize..(y + h) as usize {
        let at = row * stride + x as usize * 4;
        rgba.extend_from_slice(&image.rgba[at..at + w as usize * 4]);
    }
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

    fn frame_size(&self, image: &Image) -> (u32, u32) {
        if self.horizontal { (image.width / self.frames, image.height) }
        else { (image.width, image.height / self.frames) }
    }

    fn cut(&self, image: &Image) -> Vec<Arc<Image>> {
        let n = self.frames;
        let (fw, fh) = self.frame_size(image);
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
    decode_report(bytes).ok()
}
fn decode_report(bytes: &[u8]) -> Result<Image, String> {
    if bytes.starts_with(b"\x89PNG") { return decode_png_report(bytes); }
    if bytes.starts_with(&[0xff, 0xd8]) {
        use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
        let options = DecoderOptions::default()
            .jpeg_set_out_colorspace(ColorSpace::RGBA)
            .set_max_width(usize::MAX)
            .set_max_height(usize::MAX);
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
        decoder.decode_headers().map_err(|e| format!("JPEG header: {e}"))?;
        let info = decoder.info().ok_or("JPEG dimensions are missing")?;
        let rgba = decoder.decode().map_err(|e| format!("JPEG pixels: {e}"))?;
        return Image::rgba(u32::from(info.width), u32::from(info.height), rgba)
            .ok_or_else(|| "Invalid JPEG dimensions or RGBA length".into());
    }
    Err("Unsupported image format: expected PNG or JPEG".into())
}
fn decode_png_report(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: usize::MAX });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("PNG header: {e}"))?;
    let size = reader.output_buffer_size().ok_or("PNG dimensions overflow")?;
    let mut data = Vec::new();
    data.try_reserve_exact(size).map_err(|e| format!("PNG pixel allocation ({size} bytes): {e}"))?;
    data.resize(size, 0);
    let info = reader.next_frame(&mut data).map_err(|e| format!("PNG pixels: {e}"))?;
    data.truncate(info.buffer_size());
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        _ => return Err(format!("Unsupported PNG colour type: {:?}", info.color_type)),
    };
    if channels != 4 {
        let pixels = data.len() / channels;
        let size = pixels.checked_mul(4).ok_or("PNG RGBA dimensions overflow")?;
        data.try_reserve_exact(size - data.len()).map_err(|e| format!("PNG RGBA allocation ({size} bytes): {e}"))?;
        data.resize(size, 0);
        // Expand backwards in the decode buffer: no second full pixel copy.
        for i in (0..pixels).rev() {
            let at = i * channels;
            let c = match channels {
                3 => [data[at], data[at + 1], data[at + 2], 255],
                2 => [data[at], data[at], data[at], data[at + 1]],
                _ => [data[at], data[at], data[at], 255],
            };
            data[i * 4..i * 4 + 4].copy_from_slice(&c);
        }
    }
    Image::rgba(info.width, info.height, data).ok_or_else(|| "Invalid PNG dimensions or RGBA length".into())
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
    resample(image, w, h, x0, y0, cw, ch)
}

/// Resize the complete picture, retaining its edges even when its aspect changes.
pub fn resize(image: &Image, w: u32, h: u32) -> Option<Image> {
    if w == 0 || h == 0 || image.width == 0 || image.height == 0 { return None; }
    resample(image, w, h, 0., 0., image.width as f64, image.height as f64)
}

fn resample(image: &Image, w: u32, h: u32, x0: f64, y0: f64, cw: f64, ch: f64) -> Option<Image> {
    let px = image.rgba.as_chunks::<4>().0;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact((w as usize).checked_mul(h as usize)?.checked_mul(4)?).ok()?;
    for y in 0..h {
        let (top, bottom) = (y0 + f64::from(y) * ch / f64::from(h), y0 + f64::from(y + 1) * ch / f64::from(h));
        for x in 0..w {
            let (left, right) = (x0 + f64::from(x) * cw / f64::from(w), x0 + f64::from(x + 1) * cw / f64::from(w));
            // Fractional coverage matters: merely touching a pixel must not give
            // it a full vote. Accumulate premultiplied RGB, then restore straight
            // RGBA so transparent sprite edges retain their visible color.
            let (mut sum, mut area) = ([0f64; 4], 0f64);
            for sy in top as usize..(bottom.ceil() as usize).min(image.height as usize).max(top as usize + 1) {
                for sx in left as usize..(right.ceil() as usize).min(image.width as usize).max(left as usize + 1) {
                    let c = px[sy * image.width as usize + sx];
                    let weight = (bottom.min(sy as f64 + 1.) - top.max(sy as f64))
                        * (right.min(sx as f64 + 1.) - left.max(sx as f64));
                    let alpha = f64::from(c[3]) * weight;
                    (0..3).for_each(|k| sum[k] += f64::from(c[k]) * alpha);
                    sum[3] += alpha;
                    area += weight;
                }
            }
            let rgb = if sum[3] > 0. { [sum[0], sum[1], sum[2]].map(|s| s / sum[3]) } else { [0.; 3] };
            // Retain byte truncation without losing one unit to floating-point
            // error (in particular, an opaque area's alpha must stay 255).
            rgba.extend(rgb.into_iter().chain([sum[3] / area]).map(|v| (v + 1e-9).clamp(0., 255.) as u8));
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
    const LEVEL: f32 = 64.;
    const CONTRAST: f32 = 0.55;
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
        assert!(super::decode_report(b"not an image").err().unwrap().starts_with("Unsupported image format:"));
    }

    #[test]
    fn declared_png_and_jpeg_resources_keep_names_metadata_and_failure_reasons() {
        let root = std::env::temp_dir().join(format!("kontra-artwork-formats-{}", std::process::id()));
        let folder = root.join("Resources/pictures");
        std::fs::create_dir_all(&folder).unwrap();
        let instrument = crate::import::Instrument { path: root.join("Patch.nki"), ..Default::default() };
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.write_header().unwrap().write_image_data(&[24, 96, 176, 78].repeat(4)).unwrap();
        }
        // Authored 2x2 solid-color JPEG, with no library asset contents.
        let jpeg = include_bytes!("../tests/fixtures/wallpaper-solid.jpg");
        for (name, bytes, alpha) in [("wall.PNG", png.as_slice(), 78), ("wall.jpg", jpeg.as_slice(), 255), ("wall.jpeg", jpeg.as_slice(), 255)] {
            std::fs::write(folder.join(name), bytes).unwrap();
            std::fs::write(folder.join("wall.txt"), "Number of Animations: 2\nHorizontal Animation: yes\nHorizontal Resizable: yes\n").unwrap();
            let mut ui = crate::ksp::Interface::default();
            ui.wallpaper = name.into();
            let wall = super::performance(&instrument, Some(&ui)).unwrap().unwrap();
            assert_eq!(wall.atlas, Some([1, 2]), "{name}: the stem's sidecar is read even for .jpeg");
            assert_eq!(wall.stretch, [true, false]);
            assert!(wall.frames[0].rgba.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == alpha));
            let (pictures, errors) = super::pictures_report(&instrument.path, [name]);
            assert!(errors.is_empty(), "{name}: {errors:?}");
            assert_eq!(pictures[name].frames.len(), 2);
        }
        let mut ui = crate::ksp::Interface::default();
        for name in ["../wall.jpg", r"..\wall.jpg", "/wall.jpg"] {
            ui.wallpaper = name.into();
            assert!(super::performance(&instrument, Some(&ui)).err().unwrap().contains("Invalid instrument wallpaper name"));
        }
        ui.wallpaper = "absent.jpg".into();
        let missing = super::performance(&instrument, Some(&ui)).err().unwrap();
        assert!(missing.contains("absent.jpg was not found"), "{missing}");
        std::fs::write(folder.join("bad.jpg"), &jpeg[..20]).unwrap();
        std::fs::write(folder.join("bad.png"), &png[..12]).unwrap();
        for (name, kind) in [("bad.jpg", "JPEG header:"), ("bad.png", "PNG header:")] {
            ui.wallpaper = name.into();
            let reason = super::performance(&instrument, Some(&ui)).err().unwrap();
            assert!(reason.contains(name) && reason.contains(kind), "{reason}");
            let (pictures, errors) = super::pictures_report(&instrument.path, [name]);
            assert!(pictures.is_empty());
            assert_eq!(errors.len(), 1);
            assert!(errors[0].contains(kind), "{}", errors[0]);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn png_filmstrips_have_no_fixed_size_ceiling() {
        let (w, h) = (4096, 4097); // RGBA exceeds the previous 64 MiB ceiling.
        let mut bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut bytes, w, h);
            e.set_color(png::ColorType::Grayscale);
            let mut e = e.write_header().unwrap();
            e.write_image_data(&vec![123; w as usize * h as usize]).unwrap();
        }
        let image = super::decode_report(&bytes).unwrap();
        assert_eq!(image.rgba.len(), w as usize * h as usize * 4);
        assert_eq!(&image.rgba[..4], &[123, 123, 123, 255]);
        assert_eq!(&image.rgba[image.rgba.len() - 4..], &[123, 123, 123, 255]);
        let frame = super::Layout::parse("").cut(&image).pop().unwrap();
        assert!(std::sync::Arc::ptr_eq(&frame.rgba, &image.rgba), "a full frame shares its pixels");
        assert!(super::crop(&image, u32::MAX, 0, 2, 1).is_none());
    }

    #[test]
    fn wallpaper_atlases_keep_exact_offsets_without_frame_copies() {
        use std::sync::Arc;
        let image = Arc::new(super::Image::rgba(2, 6, [10, 20, 30, 255].repeat(12)).unwrap());
        let vertical = super::Picture { frames: vec![image.clone()], stretch: [false; 2], atlas: Some([2, 3]) };
        let (same, origin) = vertical.wallpaper(0, 2).unwrap();
        assert!(Arc::ptr_eq(&same, &image));
        assert_eq!(origin, [0, 2], "offset need not be a frame boundary");
        assert_eq!(vertical.wallpaper(1, 1).unwrap().1, [0, 4]);
        assert_eq!(vertical.wallpaper(-1, -20).unwrap().1, [0, 0]);
        assert_eq!(vertical.wallpaper(i32::MAX, i32::MAX).unwrap().1, [0, 6]);
        let across = super::Picture { frames: vec![image], stretch: [false; 2], atlas: Some([1, 6]) };
        assert_eq!(across.wallpaper(1, 2).unwrap().1, [1, 2], "horizontal animation state remains independent of vertical pixel offset");
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
        assert_eq!(super::picture_name("../x"), None);
        assert_eq!(super::picture_name("knob").as_deref(), Some("knob.png"));
    }

    #[test]
    fn fractional_resizing_preserves_coverage_and_transparent_edge_color() {
        let stripes: Vec<u8> = (0..8).flat_map(|x| [if x % 2 == 0 { 0 } else { 255 }; 3]
            .into_iter().chain([255])).collect();
        let image = super::Image::rgba(8, 1, stripes).unwrap();
        let resized = super::resize(&image, 7, 1).unwrap();
        for (pixel, expected) in resized.rgba.as_chunks::<4>().0.iter().zip([31, 191, 95, 127, 159, 63, 223]) {
            assert!(pixel[..3].iter().all(|&c| c.abs_diff(expected) <= 1), "fractional coverage: {pixel:?} vs {expected}");
            assert_eq!(pixel[3], 255);
        }
        assert_eq!(super::resize(&image, 4, 1).unwrap().rgba.as_ref(), &[127, 127, 127, 255].repeat(4), "whole-area averages remain unchanged");
        let edge = super::Image::rgba(2, 1, vec![255, 80, 20, 255, 0, 0, 0, 0]).unwrap();
        assert_eq!(super::resize(&edge, 1, 1).unwrap().rgba.as_ref(), &[255, 80, 20, 127], "transparent black must not darken visible color");
        let clear = super::Image::rgba(1, 1, vec![90, 80, 70, 0]).unwrap();
        assert_eq!(super::resize(&clear, 1, 1).unwrap().rgba.as_ref(), &[0, 0, 0, 0]);
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
