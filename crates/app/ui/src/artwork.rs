//! A library's artwork as the rack header's banner, after v2's
//! `src/artwork.rs` `banner` (itself after Kontakt 8's): cropped to cover
//! the two header rows, blurred so a logo stops competing with the title,
//! brought to one dim level with its contrast and colour held down so the
//! names read over any of it, and fading from opaque at the left to
//! nothing at the right.

use mui::prelude::Image;

use crate::theme::{CONTROL, TEXT, TIGHT};

/// The banner's size in points (two header rows), drawn from pixels at
/// twice that for crispness, as v2 did.
pub const BANNER: (f64, f64) = (TEXT * 36., 2. * (CONTROL + 2. * TIGHT));

/// `image` as a header banner (see the module docs); `None` for an empty one.
#[must_use]
pub fn banner(image: &Image) -> Option<Image> {
    // The level every banner sits at, how much of its contrast and colour
    // stays, and the brightest it gets (of 255): v2's.
    const LEVEL: f32 = 64.;
    const CONTRAST: f32 = 0.55;
    const COLOR: f32 = 0.55;
    const PEAK: f32 = 96.;
    let [w, h] = [BANNER.0, BANNER.1].map(|v| (v * 2.).round() as u32);
    let crop = blur(&cover(image, w, h)?, (h / 24).max(2) as usize, 3)?;
    let luma = |[r, g, b]: [f32; 3]| 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let pixels = crop.rgba.as_chunks::<4>().0;
    let rgb = |&[r, g, b, _]: &[u8; 4]| [r, g, b].map(f32::from);
    let mean = pixels.iter().map(|c| luma(rgb(c))).sum::<f32>() / pixels.len().max(1) as f32;
    let gain = LEVEL / mean.max(1.);
    let rgba: Vec<u8> = (pixels.iter().enumerate())
        .flat_map(|(i, c)| {
            let c = rgb(c);
            let l = luma(c);
            // Contrast around the level, then a soft knee under the peak;
            // colour only ever loses strength, dark artwork lifted or not.
            let lit = LEVEL + (l * gain - LEVEL) * CONTRAST;
            let lit = PEAK * (1. - (-lit.max(0.) / PEAK).exp()) * 1.3;
            let [r, g, b] = c.map(|v| (lit + (v - l) * COLOR * gain.min(1.)).clamp(0., 255.) as u8);
            let t = (i as u32 % w) as f32 / (w - 1).max(1) as f32;
            let s = ((t - 0.1) / 0.9).clamp(0., 1.);
            let fade = 1. - s * s * (3. - 2. * s);
            [r, g, b, (255. * fade) as u8]
        })
        .collect();
    Image::rgba(w, h, rgba)
}

/// The middle of `image` that covers `w` x `h`, resampled to it.
fn cover(image: &Image, w: u32, h: u32) -> Option<Image> {
    let (sw, sh) = (f64::from(image.width), f64::from(image.height));
    if sw == 0. || sh == 0. || w == 0 || h == 0 {
        return None;
    }
    let scale = (f64::from(w) / sw).max(f64::from(h) / sh);
    let (cw, ch) = (
        (f64::from(w) / scale) as usize,
        (f64::from(h) / scale) as usize,
    );
    let (x0, y0) = ((sw as usize - cw) / 2, (sh as usize - ch) / 2);
    let stride = image.width as usize * 4;
    let mut rgba = Vec::with_capacity(cw.max(1) * ch.max(1) * 4);
    for y in y0..y0 + ch.max(1) {
        rgba.extend_from_slice(
            image
                .rgba
                .get(y * stride + x0 * 4..y * stride + (x0 + cw.max(1)) * 4)?,
        );
    }
    let crop = Image::rgba(cw.max(1) as u32, ch.max(1) as u32, rgba)?;
    crate::pictures::resample(&crop, w, h)
}

/// `image` blurred by `passes` of a box `radius` pixels each way, across
/// then down: three passes are near enough a Gaussian. Edges repeat.
fn blur(image: &Image, radius: usize, passes: usize) -> Option<Image> {
    let (w, h) = (image.width as usize, image.height as usize);
    let mut px: Vec<[f32; 4]> = (image.rgba.as_chunks::<4>().0.iter())
        .map(|c| c.map(f32::from))
        .collect();
    let scale = 1. / (2 * radius + 1) as f32;
    let r = radius as isize;
    for _ in 0..passes {
        for across in [true, false] {
            let (n, lines) = if across { (w, h) } else { (h, w) };
            let at = |line: usize, i: usize| if across { line * w + i } else { i * w + line };
            let mut out = vec![[0f32; 4]; n];
            for line in 0..lines {
                let get = |i: isize| {
                    let i = i.clamp(0, n as isize - 1) as usize;
                    px.get(at(line, i)).copied().unwrap_or_default()
                };
                let mut sum = [0f32; 4];
                for i in -r..=r {
                    let c = get(i);
                    sum.iter_mut().zip(c).for_each(|(s, v)| *s += v);
                }
                for (i, o) in out.iter_mut().enumerate() {
                    *o = sum.map(|v| v * scale);
                    let (add, drop) = (get(i as isize + r + 1), get(i as isize - r));
                    (sum.iter_mut().zip(add).zip(drop)).for_each(|((s, a), d)| *s += a - d);
                }
                for (i, o) in out.iter().enumerate() {
                    if let Some(p) = px.get_mut(at(line, i)) {
                        *p = *o;
                    }
                }
            }
        }
    }
    let rgba: Vec<u8> = (px.iter())
        .flat_map(|c| c.map(|v| v.round().clamp(0., 255.) as u8))
        .collect();
    Image::rgba(w as u32, h as u32, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v2's `banners_fade_right_and_stay_dim`: white artwork dims under a
    /// title, and the right end is clear.
    #[test]
    fn a_banner_fades_right_and_stays_dim() {
        let white = Image::rgba(64, 32, vec![255u8; 64 * 32 * 4]);
        let b = white.as_ref().and_then(banner);
        assert!(b.is_some(), "no banner");
        let Some(b) = b else { return };
        let px = b.rgba.as_chunks::<4>().0;
        let [r, _, _, a] = px.first().copied().unwrap_or_default();
        assert!(r < 140 && a == 255, "white artwork dims: {r} {a}");
        let [_, _, _, a] = px.get(b.width as usize - 1).copied().unwrap_or_default();
        assert_eq!(a, 0, "the right end shows the header through");
    }
}
