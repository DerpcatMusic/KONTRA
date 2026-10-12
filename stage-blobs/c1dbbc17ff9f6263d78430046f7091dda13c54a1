//! A generated cover for a library with no artwork, after Kontakt 8's
//! plain covers: one flat, quiet color taken from the library's own
//! pictures (else a hue of its own name, never orange), the name set large
//! in the bundled Noto Sans at a heavy weight, and the vendor small above
//! it. The text is white or near-black, whichever reads better; never a
//! color. Made off the frame and kept as a PNG in the app's cache.

use super::theme::ORANGE;
use moose::mui::mui::prelude::Color;
use moose::mui::mui::scene::Image;
use mui_text::Font;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::OnceLock;

/// What a cover shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub name: String,
    pub vendor: String,
    pub hue: f32,
}

impl Spec {
    /// The cover for `name` by `vendor`, in the hue of its own pictures if
    /// it has one, else one of its name.
    pub fn new(name: &str, vendor: &str, hue: Option<f32>) -> Self {
        Self {
            name: name.into(),
            vendor: vendor.into(),
            hue: hue.map_or_else(|| hue_of(name), away_from_orange),
        }
    }

    /// A number standing for every pixel it renders to.
    pub fn key(&self, w: u32, h: u32, title: bool) -> u64 {
        let mut hasher = std::hash::DefaultHasher::new();
        (VERSION, &self.name, &self.vendor, self.hue.to_bits(), w, h, title).hash(&mut hasher);
        hasher.finish()
    }
}

/// Bumped when covers are drawn differently, so cached ones are redrawn.
const VERSION: u32 = 1;

/// A stable hue for `name`, spread over the circle with orange cut out.
pub fn hue_of(name: &str) -> f32 {
    // FNV-1a: the same on every machine and every run.
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
    let t = (hash >> 11) as f64 / (1u64 << 53) as f64;
    let arc = 360. - (ORANGE.end - ORANGE.start);
    (ORANGE.end + (t * f64::from(arc)) as f32) % 360.
}

/// `hue` moved out of the orange band to its nearer edge.
fn away_from_orange(hue: f32) -> f32 {
    let hue = hue.rem_euclid(360.);
    if !ORANGE.contains(&hue) {
        hue
    } else if hue - ORANGE.start < ORANGE.end - hue {
        ORANGE.start - 12.
    } else {
        ORANGE.end + 12.
    }
}

/// The cover's color: its hue, deep and muted, the same weight for every library.
pub fn background(hue: f32) -> Color {
    Color::oklch(0.46, 0.085, hue)
}

/// White or near-black, whichever stands out more from `bg`.
pub fn ink(bg: Color) -> Color {
    let (light, dark) = (Color::oklch(0.985, 0., 0.), Color::oklch(0.17, 0., 0.));
    if bg.contrast(light) >= bg.contrast(dark) { light } else { dark }
}

fn rgb(c: Color) -> [f32; 3] {
    let s = c.to_srgb().components;
    [s[0], s[1], s[2]].map(|v| v.clamp(0., 1.) * 255.)
}

fn font() -> &'static [Font] {
    static FONT: OnceLock<Vec<Font>> = OnceLock::new();
    FONT.get_or_init(|| Font::new(super::theme::NOTO_SANS).into_iter().collect())
}

/// Heavy and a little narrow: a display cut of the text face.
const TITLE: [(&str, f32); 2] = [("wght", 760.), ("wdth", 88.)];
const LABEL: [(&str, f32); 2] = [("wght", 600.), ("wdth", 100.)];

/// `text`'s advance at `size` px in the app's own face.
pub(super) fn advance(text: &str, size: f64) -> f64 {
    measure(text, &[]) * size / 100.
}

/// `text`'s advance at 100 px.
fn measure(text: &str, axes: &[(&str, f32)]) -> f64 {
    mui_text::shape_run(font(), text, 100., axes).map_or(0., |r| r.advance)
}

/// `words` in `n` lines of about even length, and the widest's advance at 100 px.
fn lines(words: &[&str], n: usize) -> Option<(Vec<String>, f64)> {
    if n == 0 || n > words.len() {
        return None;
    }
    let total: usize = words.iter().map(|w| w.len() + 1).sum();
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for (i, word) in words.iter().enumerate() {
        let left = words.len() - i;
        let lines_left = n - out.len();
        let full = line.len() >= total / n;
        if !line.is_empty() && lines_left > 1 && (full || left < lines_left) {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    out.push(line);
    let widest = out.iter().map(|l| measure(l, &TITLE)).fold(0., f64::max);
    (out.len() == n).then_some((out, widest))
}

/// A coverage accumulator: signed area per pixel, summed along each row
/// (the font-rs method). Fills non-zero for a typeface's contours.
struct Raster {
    w: usize,
    h: usize,
    a: Vec<f32>,
}

impl Raster {
    fn new(w: usize, h: usize) -> Self {
        Self { w, h, a: vec![0.; w * h + 2] }
    }

    fn line(&mut self, p0: (f32, f32), p1: (f32, f32)) {
        let clamp = |(x, y): (f32, f32)| (x.clamp(0., self.w as f32 - 1.), y.clamp(0., self.h as f32));
        let (p0, p1) = (clamp(p0), clamp(p1));
        if (p0.1 - p1.1).abs() <= f32::EPSILON {
            return;
        }
        let (dir, p0, p1) = if p0.1 < p1.1 { (1., p0, p1) } else { (-1., p1, p0) };
        let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
        let mut x = p0.0;
        for y in p0.1 as usize..self.h.min(p1.1.ceil() as usize) {
            let start = y * self.w;
            let dy = ((y + 1) as f32).min(p1.1) - (y as f32).max(p0.1);
            let next = x + dxdy * dy;
            let d = dy * dir;
            let (x0, x1) = if x < next { (x, next) } else { (next, x) };
            let (x0floor, x1ceil) = (x0.floor(), x1.ceil());
            let (x0i, x1i) = (x0floor as usize, x1ceil as usize);
            if x1i <= x0i + 1 {
                let xmf = 0.5 * (x + next) - x0floor;
                self.a[start + x0i] += d - d * xmf;
                self.a[start + x0i + 1] += d * xmf;
            } else {
                let s = (x1 - x0).recip();
                let x0f = x0 - x0floor;
                let a0 = 0.5 * s * (1. - x0f) * (1. - x0f);
                let x1f = x1 - x1ceil + 1.;
                let am = 0.5 * s * x1f * x1f;
                self.a[start + x0i] += d * a0;
                if x1i == x0i + 2 {
                    self.a[start + x0i + 1] += d * (1. - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.a[start + x0i + 1] += d * (a1 - a0);
                    for xi in x0i + 2..x1i - 1 {
                        self.a[start + xi] += d * s;
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    self.a[start + x1i - 1] += d * (1. - a2 - am);
                }
                self.a[start + x1i] += d * am;
            }
            x = next;
        }
    }

    /// `text` at `size` px with its baseline starting at `x`, `y`.
    fn text(&mut self, text: &str, size: f64, axes: &[(&str, f32)], x: f64, y: f64) {
        let Ok(run) = mui_text::text_run(font(), text, size, axes, 0.05) else { return };
        let Ok(contours) = run.path.flatten(0.05, 1 << 20) else { return };
        for contour in contours {
            let at = |p: &moose::mui::mui::geometry::Point| ((p.x + x) as f32, (p.y + y) as f32);
            for pair in contour.windows(2) {
                self.line(at(&pair[0]), at(&pair[1]));
            }
            if let (Some(first), Some(last)) = (contour.first(), contour.last()) {
                self.line(at(last), at(first));
            }
        }
    }

    /// Each pixel's coverage, 0..1.
    fn coverage(&self) -> impl Iterator<Item = f32> + '_ {
        self.a[..self.w * self.h].iter().scan(0f32, |acc, v| {
            *acc += v;
            Some(acc.abs().min(1.))
        })
    }
}

/// The cover `w` by `h` pixels; `title` sets the name (and vendor) on it.
/// A name too long to read at this size shows as its initials.
pub fn render(spec: &Spec, w: u32, h: u32, title: bool) -> Option<Image> {
    if w == 0 || h == 0 {
        return None;
    }
    let bg = background(spec.hue);
    let (back, fore) = (rgb(bg), rgb(ink(bg)));
    let (wf, hf) = (f64::from(w), f64::from(h));
    let mut text = Raster::new(w as usize, h as usize);
    let mut label = Raster::new(w as usize, h as usize);
    if title {
        let pad = (wf.min(hf) * 0.1).round().max(2.);
        let room = wf - 2. * pad;
        // The vendor, small and in capitals, when there is room for it.
        let small = (hf * 0.085).round();
        let vendor = spec.vendor.to_uppercase();
        let top = if !vendor.is_empty() && small >= 10. && measure(&vendor, &LABEL) * small / 100. <= room {
            label.text(&vendor, small, &LABEL, pad, pad + small * 0.8);
            pad + small * 1.6
        } else {
            pad
        };
        let words: Vec<&str> = spec.name.split_whitespace().collect();
        let tall = hf - top - pad;
        // The largest size any split of the name into up to three lines fits at.
        let best = (1..=3)
            .filter_map(|n| lines(&words, n))
            .map(|(lines, widest)| {
                let by_width = room * 100. / widest.max(1.);
                let by_height = tall / (lines.len() as f64 * 1.08);
                (by_width.min(by_height).min(hf * 0.3), lines)
            })
            .max_by(|a, b| a.0.total_cmp(&b.0));
        match best {
            Some((size, lines)) if size >= (hf * 0.2).max(16.) => {
                let size = size.floor();
                let step = size * 1.08;
                // Set from the foot up, left aligned.
                let foot = hf - pad - size * 0.22;
                for (n, line) in lines.iter().rev().enumerate() {
                    text.text(line, size, &TITLE, pad, foot - n as f64 * step);
                }
            }
            _ => {
                let initials: String =
                    words.iter().filter_map(|w| w.chars().find(|c| c.is_alphanumeric())).take(2).collect();
                let size = (hf * 0.46).min(wf * 0.8 / (initials.chars().count().max(1) as f64 * 0.62)).floor();
                let width = measure(&initials, &TITLE) * size / 100.;
                text.text(&initials, size, &TITLE, ((wf - width) / 2.).round(), ((hf + size * 0.7) / 2.).round());
            }
        }
    }
    let rgba: Vec<u8> = text
        .coverage()
        .zip(label.coverage())
        .flat_map(|(t, l)| {
            let a = t.max(l * 0.72);
            let px = [0, 1, 2].map(|k| (back[k] + (fore[k] - back[k]) * a).round() as u8);
            [px[0], px[1], px[2], 255]
        })
        .collect();
    Image::rgba(w, h, rgba)
}

fn cache_dir() -> Option<PathBuf> {
    if cfg!(test) {
        return Some(std::env::temp_dir().join(format!("kontra-test-{}", std::process::id())).join("covers"));
    }
    Some(crate::cache::dir()?.join("covers"))
}

/// [`render`], from the app's cache when drawn before; drawn, it is kept there.
pub fn cached(spec: &Spec, w: u32, h: u32, title: bool) -> Option<Image> {
    let path = cache_dir().map(|d| d.join(format!("{:016x}.png", spec.key(w, h, title))));
    if let Some(image) = (path.as_ref()).and_then(|p| std::fs::read(p).ok()).and_then(|b| crate::artwork::decode(&b)) {
        return Some(image);
    }
    let image = render(spec, w, h, title)?;
    if let Some(path) = path {
        let _ = save(&image, &path);
    }
    Some(image)
}

fn save(image: &Image, path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().unwrap_or(path))?;
    let tmp = path.with_extension("png.tmp");
    {
        let mut e = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&tmp)?), image.width, image.height);
        e.set_color(png::ColorType::Rgba);
        let mut w = e.write_header().map_err(std::io::Error::other)?;
        w.write_image_data(&image.rgba).map_err(std::io::Error::other)?;
    }
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_are_the_same_every_time_and_never_orange() {
        let spec = Spec::new("Pacific Ensemble Strings", "Performance Samples", None);
        let a = render(&spec, 240, 140, true).unwrap();
        let b = render(&spec, 240, 140, true).unwrap();
        assert_eq!(a.rgba, b.rgba, "deterministic");
        assert_eq!(spec.hue, Spec::new("Pacific Ensemble Strings", "", None).hue, "the hue is the name's");
        for n in 0..500 {
            assert!(!ORANGE.contains(&hue_of(&format!("Library {n}"))));
        }
        assert!(!ORANGE.contains(&Spec::new("x", "", Some(60.)).hue), "an orange picture moves off orange");
        assert_eq!(Spec::new("x", "", Some(200.)).hue, 200., "any other keeps its hue");
        // The name is drawn: ink covers some of it, the color the rest.
        let px = a.rgba.as_chunks::<4>().0;
        let plain = px[0];
        let inked = px.iter().filter(|p| **p != plain).count();
        assert!(inked > px.len() / 50 && inked < px.len() / 2, "{inked} of {}", px.len());
        // Cached, it comes back the same.
        let cached = cached(&spec, 240, 140, true).unwrap();
        assert_eq!(cached.rgba, a.rgba);
        assert_eq!(super::cached(&spec, 240, 140, true).unwrap().rgba, a.rgba, "read back from the cache");
    }

    #[test]
    fn cover_text_is_white_or_near_black_by_contrast() {
        let white = Color::oklch(0.985, 0., 0.);
        for hue in [20., 150., 200., 260., 320.] {
            let bg = background(hue);
            let ink = ink(bg);
            assert_eq!(ink.chroma(), 0., "never a colored text");
            assert!(bg.contrast(ink) >= 4.5, "hue {hue}: {}", bg.contrast(ink));
        }
        assert_eq!(ink(Color::oklch(0.3, 0.1, 260.)), white);
        assert!(ink(Color::oklch(0.9, 0.1, 110.)).lightness() < 0.2, "dark text on a light cover");
        // Rendered: the text pixels are the ink, not a tint of the cover.
        let spec = Spec::new("Kinder Piano", "", Some(260.));
        let image = render(&spec, 200, 120, true).unwrap();
        let px = image.rgba.as_chunks::<4>().0;
        let brightest = px.iter().map(|p| p[0].min(p[1]).min(p[2])).max().unwrap();
        assert!(brightest > 240, "full coverage is white: {brightest}");
    }

    #[test]
    fn a_long_name_on_a_thumbnail_shows_its_initials() {
        let spec = Spec::new("The Very Long Orchestral Collection Of Things", "", None);
        let thumb = render(&spec, 72, 42, true).unwrap();
        let px = thumb.rgba.as_chunks::<4>().0;
        let inked = px.iter().filter(|p| **p != px[0]).count();
        assert!(inked > 40, "initials drawn: {inked}");
        assert_eq!(render(&spec, 72, 42, false).unwrap().rgba.as_chunks::<4>().0.iter().filter(|p| **p != px[0]).count(), 0);
    }
}
