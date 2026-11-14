//! Waveform (`ui_waveform`) and wavetable (`ui_wavetable`) displays as
//! Kontakt 8 draws them (oracle, scratch/lanes/ui/probe/wave.ksp at 100 %,
//! 2026-11-14), rasterised once per size and look into device pixels; the
//! play cursor rides over them.
//!
//! A waveform sits in the stock rim (1 px #2C2C2C, 1 px #393939) on a
//! #3F3F3F field under its `BG_COLOR` (#1C1C1C) at `BG_ALPHA`. Its wave, in
//! `WAVE_COLOR` (white), is either filled columns about a full-strength
//! centre line, each fading from about 0.76 alpha at the centre to 0.34 at
//! its own peak (0.76 - 0.42 d^2.5), or, as X-ray (`WF_VIS_MODE` 2 and 3,
//! which draw alike), the sample's line seen through: each point lit
//! 1 - e^(-0.107 n) by the n samples crossing it.
//!
//! A wavetable draws on its background with no rim: in 2D its current
//! frame, in 3D every frame, the others behind it in `WAVETABLE_COLOR`
//! fading by 0.81 a frame from 0.37 (or at `WAVETABLE_ALPHA`), graded by
//! frame to the `_END_` colour and alpha. `PARALLAX_X`/`_Y` (millionths)
//! shrink each frame's box to 1 / (1 + |p|) and step the frames across the
//! rest, the current one at the positive end. A frame's wave fills 0.9 of
//! its box's half height.

use std::sync::Arc;

use kontra_ui_model as model;
use mui::prelude::*;

/// The cursor when the script sets no `WAVE_CURSOR_COLOR`.
const CURSOR: u32 = 0xDFA054;
const RIM: [u32; 2] = [0x2C_2C2C, 0x39_3939];
const FIELD: u32 = 0x3F_3F3F;
const BACKGROUND: u32 = 0x1C_1C1C;
/// The X-ray's ink a sample leaves on each point it crosses.
const XRAY: f64 = 0.107;
/// A stock wavetable's frame behind the current one: alpha, and its fade a frame.
const FRAME_ALPHA: f64 = 0.37;
const FRAME_FADE: f64 = 0.81;

/// RGB 0 to 1.
type Ink = [f64; 3];

fn ink(c: model::Rgba) -> Ink {
    [c.r, c.g, c.b].map(|v| f64::from(v) / 255.)
}

fn rgb(c: u32) -> Ink {
    ink(model::Rgba::rgb(c))
}

/// An opaque picture in device pixels, painted over.
struct Canvas {
    w: usize,
    h: usize,
    px: Vec<Ink>,
}

impl Canvas {
    fn new(w: usize, h: usize, fill: Ink) -> Self {
        Self {
            w,
            h,
            px: vec![fill; w * h],
        }
    }
    fn over(&mut self, x: usize, y: usize, c: Ink, a: f64) {
        if x >= self.w || a <= 0. {
            return;
        }
        if let Some(p) = self.px.get_mut(y * self.w + x) {
            let a = a.min(1.);
            for (d, s) in p.iter_mut().zip(c) {
                *d += (s - *d) * a;
            }
        }
    }
    fn rect(&mut self, (x0, y0, x1, y1): (usize, usize, usize, usize), c: Ink, a: f64) {
        for y in y0..y1.min(self.h) {
            for x in x0..x1.min(self.w) {
                self.over(x, y, c, a);
            }
        }
    }
    fn image(self) -> Option<Arc<Image>> {
        let bytes: Vec<u8> = (self.px.iter())
            .flat_map(|p| {
                let [r, g, b] = p.map(|v| (v.clamp(0., 1.) * 255.).round() as u8);
                [r, g, b, 255]
            })
            .collect();
        Image::rgba(self.w as u32, self.h as u32, bytes).map(Arc::new)
    }
}

/// What a display draws besides its samples: kind, colours and alphas.
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub(super) struct Look {
    table: Option<(bool, [i32; 2])>,
    background: Option<model::Rgba>,
    wave: Option<model::Rgba>,
    wave_end: Option<model::Rgba>,
    frames: Option<model::Rgba>,
    frames_end: Option<model::Rgba>,
}

impl Look {
    pub(super) fn of(w: &model::Widget) -> Self {
        let c = &w.colors;
        Self {
            table: match w.kind {
                model::Kind::Wavetable { three_d, parallax } => Some((three_d, parallax)),
                _ => None,
            },
            background: c.background,
            wave: c.wave,
            wave_end: c.wave_end,
            frames: c.wavetable,
            frames_end: c.wavetable_end,
        }
    }
}

/// `wave` as `look` draws it into `w`×`h` device pixels at `s` per point;
/// `None` until its samples are read.
pub(super) fn raster(
    wave: &model::Waveform,
    look: Look,
    (w, h, s): (usize, usize, f64),
) -> Option<Arc<Image>> {
    if w == 0 || h == 0 {
        return None;
    }
    let alpha = |i: usize, or: f64| {
        (wave.alpha.get(i).copied().flatten()).map_or(or, |a| f64::from(a) / 255.)
    };
    let bg = look.background.map_or(rgb(BACKGROUND), ink);
    match look.table {
        None => {
            if wave.peaks.is_empty() {
                return None;
            }
            let mut c = Canvas::new(w, h, rgb(RIM[0]));
            let edge = |n: f64| (n * s).round() as usize;
            let (r1, r2) = (edge(1.), edge(2.));
            c.rect(
                (r1, r1, w.saturating_sub(r1), h.saturating_sub(r1)),
                rgb(RIM[1]),
                1.,
            );
            let inner = (r2, r2, w.saturating_sub(r2), h.saturating_sub(r2));
            c.rect(inner, rgb(FIELD), 1.);
            c.rect(inner, bg, alpha(0, 1.));
            let tint = look.wave.map_or([1.; 3], ink);
            if wave.xray {
                xray(&mut c, wave, inner, tint, s);
            } else {
                filled(&mut c, wave, inner, tint, s);
            }
            c.image()
        }
        Some((three_d, parallax)) => {
            if wave.frames.is_empty() {
                return None;
            }
            let mut c = Canvas::new(w, h, rgb(FIELD));
            c.rect((0, 0, w, h), bg, alpha(0, 1.));
            frames(&mut c, wave, look, (three_d, parallax), s, &alpha);
            c.image()
        }
    }
}

/// The peaks of pixel column `x` of `of` columns: the lowest and highest
/// level (±1) of the sample columns under it.
fn column(peaks: &[[i8; 2]], x: usize, of: usize) -> (usize, usize, [f64; 2]) {
    let n = peaks.len();
    let (a, b) = (x * n / of, ((x + 1) * n / of).max(x * n / of + 1).min(n));
    let span = peaks.get(a..b).unwrap_or_default();
    let (lo, hi) = span
        .iter()
        .fold((0i8, 0i8), |(l, h), &[pl, ph]| (l.min(pl), h.max(ph)));
    (a, b, [f64::from(lo) / 127., f64::from(hi) / 127.])
}

/// Mode 1: each column filled from the centre to its peaks, fading out
/// along each, over a centre line.
fn filled(
    c: &mut Canvas,
    wave: &model::Waveform,
    (x0, y0, x1, y1): (usize, usize, usize, usize),
    tint: Ink,
    s: f64,
) {
    let (cols, rows) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    let half = rows as f64 / 2.;
    let mid = (rows / 2) as f64;
    let profile = |d: f64| 0.76 - 0.42 * d.clamp(0., 1.).powf(2.5);
    for x in 0..cols {
        let (_, _, [lo, hi]) = column(&wave.peaks, x, cols);
        // Above the centre to the high peak, below it to the low one.
        for (reach, up) in [(hi.max(0.) * half, true), ((-lo).max(0.) * half, false)] {
            if reach <= 0. {
                continue;
            }
            let rows_in = reach.ceil() as usize;
            for k in 0..rows_in {
                let cover = (reach - k as f64).min(1.);
                let d = (k as f64 + 0.5) / reach;
                let y = if up {
                    mid - 1. - k as f64
                } else {
                    mid + 1. + k as f64
                };
                if (0. ..rows as f64).contains(&y) {
                    c.over(x0 + x, y0 + y as usize, tint, profile(d) * cover);
                }
            }
        }
    }
    let line = (s.round() as usize).max(1);
    let top = y0 + mid as usize;
    c.rect((x0, top, x1, top + line), tint, 1.);
}

/// X-ray: each point lit by the samples crossing it.
fn xray(
    c: &mut Canvas,
    wave: &model::Waveform,
    (x0, y0, x1, y1): (usize, usize, usize, usize),
    tint: Ink,
    s: f64,
) {
    let bins = model::WAVE_BINS;
    let (cols, rows) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    if wave.density.len() < wave.peaks.len() * bins || rows == 0 {
        return;
    }
    // A pixel row spans this many bands; Kontakt's ink is per point.
    let per_row = bins as f64 / rows as f64;
    // The share of a sample column one pixel column holds (below 1 when
    // the display is wider than the columns read).
    let ratio = wave.peaks.len() as f64 / cols.max(1) as f64;
    for x in 0..cols {
        let (a, b, _) = column(&wave.peaks, x, cols);
        let share = ratio / (b - a).max(1) as f64;
        let mut sums = vec![0f64; bins];
        for col in a..b {
            for (sum, &n) in sums.iter_mut().zip(
                wave.density
                    .get(col * bins..(col + 1) * bins)
                    .unwrap_or_default(),
            ) {
                *sum += f64::from(n);
            }
        }
        for y in 0..rows {
            let (t0, t1) = (y as f64 * per_row, (y + 1) as f64 * per_row);
            let n: f64 = (t0.floor() as usize..(t1.ceil() as usize).min(bins))
                .map(|band| {
                    let overlap = (t1.min(band as f64 + 1.) - t0.max(band as f64)).max(0.);
                    sums.get(band).copied().unwrap_or(0.) * overlap
                })
                .sum();
            // Kontakt's ink counts the samples in a point; this pixel is
            // 1/s of a point each way.
            let crossing = n * share * s * s;
            let lit = 1. - (-XRAY * crossing).exp();
            c.over(x0 + x, y0 + y, tint, lit);
        }
    }
}

/// A wavetable's frames, back to front, the current one last.
fn frames(
    c: &mut Canvas,
    wave: &model::Waveform,
    look: Look,
    (three_d, parallax): (bool, [i32; 2]),
    s: f64,
    alpha: &dyn Fn(usize, f64) -> f64,
) {
    let (w, h) = (c.w as f64, c.h as f64);
    let n = wave.frames.len();
    // ponytail: the current frame is the first; the group's wavetable
    // position (and its modulation) is not read yet.
    let current = 0;
    let p = parallax.map(|v| if three_d { f64::from(v) / 1e6 } else { 0. });
    let (bw, bh) = (w / (1. + p[0].abs()), h / (1. + p[1].abs()));
    let (tx, ty) = (w - bw, h - bh);
    let box_at = |k: usize| {
        let t = if n > 1 { k as f64 / (n - 1) as f64 } else { 0. };
        let front = |p: f64, travel: f64| if p > 0. { travel } else { 0. };
        (
            front(p[0], tx) - p[0].signum() * tx * t,
            front(p[1], ty) - p[1].signum() * ty * t,
        )
    };
    let lerp = |a: Ink, b: Ink, t: f64| {
        [0, 1, 2].map(|i| {
            a.get(i).copied().unwrap_or(0.) * (1. - t) + b.get(i).copied().unwrap_or(0.) * t
        })
    };
    let table = look.frames.map_or([1.; 3], ink);
    let table_end = look.frames_end.map_or(table, ink);
    let set = wave.alpha.get(3).copied().flatten().is_some();
    if three_d {
        for k in (0..n).rev().filter(|&k| k != current) {
            let t = if n > 1 { k as f64 / (n - 1) as f64 } else { 0. };
            let distance = k.abs_diff(current) as f64;
            let start = if set {
                alpha(3, 1.)
            } else {
                FRAME_ALPHA * FRAME_FADE.powf(distance - 1.)
            };
            let a = start + (alpha(4, start) - start) * t;
            let cycle = wave.frames.get(k).map(Vec::as_slice).unwrap_or_default();
            line(c, cycle, box_at(k), (bw, bh), s, &|_| {
                (lerp(table, table_end, t), a)
            });
        }
    }
    let tint = look.wave.map_or([1.; 3], ink);
    let tint_end = look.wave_end.map_or(tint, ink);
    let (a0, a1) = (alpha(1, 1.), alpha(2, alpha(1, 1.)));
    // ponytail: the current wave's _END_ gradient runs across it; unprobed.
    let cycle = wave
        .frames
        .get(current)
        .map(Vec::as_slice)
        .unwrap_or_default();
    line(c, cycle, box_at(current), (bw, bh), s, &|t| {
        (lerp(tint, tint_end, t), a0 + (a1 - a0) * t)
    });
}

/// `cycle` as a one-point line through a box at `at` of `size`, inked by
/// `paint` along it (0 at its left, 1 at its right).
fn line(
    c: &mut Canvas,
    cycle: &[i8],
    (bx, by): (f64, f64),
    (bw, bh): (f64, f64),
    s: f64,
    paint: &dyn Fn(f64) -> (Ink, f64),
) {
    let m = cycle.len();
    if m < 2 || bw < 1. {
        return;
    }
    let mid = by + bh / 2.;
    let amp = 0.9 * bh / 2.;
    let y_at = |u: f64| {
        let f = (u.clamp(0., 1.) * (m - 1) as f64).min((m - 1) as f64);
        let (i, frac) = (f.floor() as usize, f.fract());
        let v = |i: usize| f64::from(cycle.get(i.min(m - 1)).copied().unwrap_or(0)) / 127.;
        mid - (v(i) * (1. - frac) + v(i + 1) * frac) * amp
    };
    let half = s / 2.;
    for x in bx.floor().max(0.) as usize..((bx + bw).ceil() as usize).min(c.w) {
        let (u0, u1) = (((x as f64 - bx) / bw), ((x as f64 + 1. - bx) / bw));
        if u1 <= 0. || u0 >= 1. {
            continue;
        }
        let (ya, yb) = (y_at(u0), y_at(u1));
        let (top, bottom) = (ya.min(yb) - half, ya.max(yb) + half);
        let (ink, a) = paint((u0 + u1) / 2.);
        for y in top.floor().max(0.) as usize..(bottom.ceil() as usize).min(c.h) {
            let cover = (bottom.min(y as f64 + 1.) - top.max(y as f64)).clamp(0., 1.);
            c.over(x, y, ink, a * cover);
        }
    }
}

/// The play cursor over `wave` in a `w`×`h` box at `s`; `None` when hidden.
pub(super) fn cursor(
    wave: &model::Waveform,
    c: &model::Colors,
    (w, h, s): (f64, f64, f64),
) -> Option<El> {
    let at = (wave.cursor_us >= 0 && wave.length_us > 0)
        .then(|| (wave.cursor_us as f64 / wave.length_us as f64).clamp(0., 1.))?;
    let ink = super::color(c.wave_cursor.unwrap_or(model::Rgba::rgb(CURSOR)));
    Some(
        block(s, h)
            .radius(0.)
            .fill(Fill::from(ink))
            .at((at * w).min(w - s), 0.),
    )
}
