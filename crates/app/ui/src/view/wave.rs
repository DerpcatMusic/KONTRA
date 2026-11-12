//! A waveform display (`ui_waveform`) as Kontakt 8 draws one: the zone's
//! sample in columns about a bright centre line, each column fading from
//! the centre to its own peak, and an orange play cursor across the full
//! height (oracle, Areia Pads, 2026-10-30: scratch/laneJ/burst3/pads.png).

use kontra_ui_model as model;
use mui::prelude::*;

/// Grey levels measured: the centre line, a column beside it, its tip.
const CENTRE: f32 = 1.0;
const NEAR: f32 = 0.82;
const TIP: f32 = 0.28;
/// Bands a column fades through, centre to tip.
const BANDS: usize = 4;
/// The cursor when the script sets no `WAVE_CURSOR_COLOR`.
const CURSOR: u32 = 0xDFA054;

/// `wave` drawn into a `w`×`h` box; `None` until its sample is read.
pub(super) fn draw(
    wave: &model::Waveform,
    c: &model::Colors,
    (w, h, s): (f64, f64, f64),
) -> Option<El> {
    if wave.peaks.is_empty() {
        return None;
    }
    let peaks = wave.peaks.clone();
    let tint = c.wave.map(super::color);
    let cursor = (wave.cursor_us >= 0 && wave.length_us > 0)
        .then(|| (wave.cursor_us as f64 / wave.length_us as f64).clamp(0., 1.));
    let ink = super::color(c.wave_cursor.unwrap_or(model::Rgba::rgb(CURSOR)));
    let rect = |x: f64, y: f64, w: f64, h: f64| crate::kit::rect_path(x, y, x + w, y + h);
    let grey = move |l: f32| match tint {
        Some(t) => t.with_alpha(l),
        None => Color::srgb(l, l, l),
    };
    let el = canvas(move |z| {
        let (w, h) = (z.width, z.height);
        let mid = (h / 2.).floor();
        let col = w / peaks.len() as f64;
        let mut d = Vec::new();
        // Outermost band first: each nearer band paints over it, brighter.
        for band in (0..BANDS).rev() {
            let reach = (band + 1) as f64 / BANDS as f64;
            let t = band as f32 / (BANDS - 1) as f32;
            let fill = Fill::from(grey(NEAR + (TIP - NEAR) * t));
            for (i, [low, high]) in peaks.iter().enumerate() {
                let top = mid - f64::from(*high) / 127. * mid * reach;
                let bottom = mid - f64::from(*low) / 127. * mid * reach;
                if bottom > top {
                    d.push(Draw::fill(
                        rect(i as f64 * col, top, col.max(1.), bottom - top),
                        fill.clone(),
                    ));
                }
            }
        }
        d.push(Draw::fill(rect(0., mid, w, s), Fill::from(grey(CENTRE))));
        if let Some(at) = cursor {
            d.push(Draw::fill(
                rect((at * w).min(w - s), 0., s, h),
                Fill::from(ink),
            ));
        }
        d
    });
    Some(el.w(w).h(h))
}
