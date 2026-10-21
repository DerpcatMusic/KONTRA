//! The plot, drawn: the reduction, the white curve, the grid
//! and its labels.
//!
//! The plot is one canvas with the tick labels and the range chip over it in a
//! stack. Every tick is a fixed cell between two fixed gaps, so the labels
//! land on the grid lines without a coordinate in the layout: the grid the
//! canvas draws and the row the labels sit in are built from the same
//! `FREQ` table.

use super::axis::*;
use super::axis::pt;
use super::band::Handle;
use super::data::PlotView;
use crate::skin::*;
use crate::theme;
use crate::tokens::{icon, plot, space, stroke, ty};
use mui::prelude::*;

/// The name the plot carries, so the pointer can say which band it is on.
pub const PLOT_ID: &str = "plot";

/// The plot itself, drawn from the engine's data and the handles it is given.
pub fn plot(
    handles: &[Handle],
    view: &PlotView,
    range: usize,
    cursor: Option<(f64, f64, f64)>,
    white: &[f32; PLOT_BINS],
    own: Option<(&[f32; PLOT_BINS], u32)>,
    shape: Cursor,
) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let mut draws: Vec<Draw> = Vec::new();

    // The reduction the engine is taking, hanging from the zero line: the
    // material is what is being removed, so a notch is a resonance being
    // pulled down. Silent, it is the flat shelf under the zero line.
    let fill: Vec<(f64, f64)> = (0..PLOT_BINS)
        .map(|index| {
            let x = x_for_frequency(f64::from(plot_frequency(index)));
            (x, gain_y(view.gain[0][index]))
        })
        .collect();

    // The material's edge glows: the reference fades the fill into the
    // background over a few dozen pixels instead of cutting it off. Layered
    // bands, each a step higher and a step fainter, stand in for a blur; the
    // halo's own edge is averaged over a few samples so a step in the trace
    // cannot stack its bands into a bright column.
    let edge: Vec<f64> = (0..fill.len())
        .map(|i| {
            let lo = i.saturating_sub(plot::GLOW_SOFTEN);
            let hi = (i + plot::GLOW_SOFTEN).min(fill.len() - 1);
            let span = &fill[lo..=hi];
            span.iter().map(|p| p.1).sum::<f64>() / span.len() as f64
        })
        .collect();
    for layer in (1..=plot::GLOW_LAYERS).rev() {
        let lift = layer as f64 * plot::GLOW_STEP;
        let mut band: Vec<Point> = fill
            .iter()
            .zip(&edge)
            .map(|(&(x, _), &y)| pt(x, (y - lift).max(GRID_TOP)))
            .collect();
        band.push(pt(PLOT_W, PLOT_H));
        band.push(pt(0.0, PLOT_H));
        draws.push(Draw::fill(Path::polyline(band, true), ca(t.plot_ink, t.glow)));
    }

    // The drawn trace: the reduction averaged over a few bins, so its edge is a
    // line and not the engine's bin-rate teeth.
    let trace: Vec<(f64, f64)> = (0..fill.len())
        .map(|i| {
            let lo = i.saturating_sub(plot::TRACE_SOFTEN);
            let hi = (i + plot::TRACE_SOFTEN).min(fill.len() - 1);
            let span = &fill[lo..=hi];
            (fill[i].0, span.iter().map(|p| p.1).sum::<f64>() / span.len() as f64)
        })
        .collect();

    // The area under the reduction, closed on the plot's baseline.
    let mut area: Vec<Point> = trace.iter().map(|&(x, y)| pt(x, y)).collect();
    area.push(pt(PLOT_W, PLOT_H));
    area.push(pt(0.0, PLOT_H));
    draws.push(Draw::fill(
        Path::polyline(area, true),
        Gradient::vertical(c(t.fill_top), c(t.fill_bottom)),
    ));
    draws.push(Draw::stroke(
        Path::polyline(trace.iter().map(|&(x, y)| pt(x, y)).collect::<Vec<_>>(), false),
        ca(t.plot_ink, t.veil_soft),
        stroke::RULE,
    ));

    // The second channel, when it is taking something different: the
    // reference draws it as its own, darker, curve over the first.
    if view.gain[0].iter().zip(&view.gain[1]).any(|(a, b)| (a - b).abs() > 1.0e-3) {
        let edge: Vec<Point> = (0..PLOT_BINS)
            .map(|index| pt(x_for_frequency(f64::from(plot_frequency(index))), gain_y(view.gain[1][index])))
            .collect();
        let mut area = edge.clone();
        area.push(pt(PLOT_W, PLOT_H));
        area.push(pt(0.0, PLOT_H));
        draws.push(Draw::fill(Path::polyline(area, true), ca(t.plot_ink, t.glow)));
        draws.push(Draw::stroke(Path::polyline(edge, false), c(t.plot_ink), stroke::RULE));
    }

    for y in level_lines(range) {
        draws.push(Draw::stroke(
            line(pt(0.0, y), pt(PLOT_W, y)),
            ca(t.plot_ink, t.veil),
            stroke::HAIR,
        ));
    }
    for &(x, _, _) in &FREQ {
        draws.push(Draw::stroke(
            line(pt(x, GRID_TOP), pt(x, PLOT_H)),
            ca(t.plot_ink, t.veil),
            stroke::HAIR,
        ));
    }
    // The zero line is the brightest: it is what the handles rest on.
    draws.push(Draw::stroke(
        line(pt(0.0, ZERO), pt(PLOT_W, ZERO)),
        ca(t.plot_ink, t.veil_strong),
        stroke::HAIR,
    ));
    white_curve(&mut draws, white, own);

    for h in handles {
        let index = h.index;
        let centre = pt(h.x, h.y);
        let (outer, inner) = if h.pill {
            (
                Path::capsule(plot::PILL_W, plot::PILL_H).unwrap_or_default(),
                Path::capsule(plot::PILL_INNER_W, plot::PILL_INNER_H).unwrap_or_default(),
            )
        } else {
            (
                disc_path(0.0, 0.0, plot::DOT_R),
                disc_path(0.0, 0.0, plot::DOT_INNER_R),
            )
        };
        draws.push(Draw::fill(outer.clone(), ca(t.handle, t.rim)).at(centre));
        draws.push(Draw::fill(inner, c(h.tint)).at(centre));
        if h.selected {
            // The band the panel is showing wears a ring.
            draws.push(Draw::stroke(outer.clone(), c(t.plot_ink), stroke::RULE).at(centre));
        }
        // The hovered handle wears a ring, so the pointer's band is plain.
        if h.hovered {
            draws.push(Draw::stroke(outer.clone(), ca(t.plot_ink, t.veil_strong), stroke::RULE).at(centre));
        }
        // The tag is how a drag knows which band it took.
        draws.push(Draw::hit(outer, format!("band-{index}")).at(centre));
    }

    stack![
        block(PLOT_W, PLOT_H).fill(Gradient::vertical(c(t.graph_top), c(t.graph_bottom))),
        // Focusable, so a press on the plot keeps the keyboard there until
        // the next press elsewhere: the arrows and Delete act on the band.
        canvas(move |_| draws.clone()).full().id(PLOT_ID).focusable(),
        // The inset shadow: the plot is a well, and the light comes from
        // above and to the left.
        band(PLOT_W, plot::WELL_TOP, Align::Start, Align::Start).fill(Gradient::vertical(
            ca(t.well, t.well_alpha),
            ca(t.well, 0.0),
        )),
        block(plot::WELL_LEFT, PLOT_H)
            .anchor(Align::Start, Align::Start)
            .fill(Gradient::linear(
                90.0,
                [(0.0, ca(t.well, t.well_edge)), (1.0, ca(t.well, 0.0))],
            )),
        band(PLOT_W, stroke::HAIR, Align::Start, Align::Start).fill(ca(t.edge_light, 0.7)),
        ticks_row()
            .anchor(Align::Start, Align::Start)
            .h(plot::TICK_ROW_H),
        ticks_col(range)
            .anchor(Align::End, Align::Start)
            .w(plot::LEVEL_CELL * 2.0 + space::TIGHT),
        frequency_tag(cursor),
        range_chip(range)
            .anchor(Align::End, Align::End)
            .offset(-space::XS, -(space::L + space::TIGHT)),
    ]
    .size(PLOT_W, PLOT_H)
    .shrink(0)
    .cursor(shape)
}

/// The frequency under the pointer, in the reference's own formats, beside it.
fn frequency_tag(cursor: Option<(f64, f64, f64)>) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let Some((x, y, hertz)) = cursor else {
        return block(space::NONE, space::NONE).shrink(0);
    };
    let text = if hertz >= 10_000.0 {
        format!("{:.0} kHz", hertz / 1000.0)
    } else if hertz >= 1000.0 {
        format!("{:.1} kHz", hertz / 1000.0)
    } else {
        format!("{hertz:.0} Hz")
    };
    label(text.as_str(), ty::MICRO, c(t.ink_quiet))
        .anchor(Align::Start, Align::Start)
        .offset(x + space::M, y - space::L)
}

/// The running range: the span the plot is showing, in the corner.
fn range_chip(range: usize) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    stack![
        block(plot::RANGE_W, plot::RANGE_H)
            .radius(SKIN.corners.field)
            .fill(ca(t.range_chip, t.chip_alpha)),
        label(RANGES[range].1, ty::BODY, c(t.ink))
            .anchor(Align::Start, Align::Center)
            .offset(space::XS, space::NONE),
        glyph(Icon::Down, icon::SMALL, c(t.ink_quiet))
            .anchor(Align::End, Align::Center)
            .offset(-space::TIGHT, space::NONE),
    ]
    .size(plot::RANGE_W, plot::RANGE_H)
    .tipped("range")
    .cursor(Cursor::Hand)
}

/// A child of the plot, anchored where the design puts it.
fn band(w: f64, h: f64, x: Align, y: Align) -> El {
    block(w, h).anchor(x, y)
}

/// The frequency labels: one cell centred on each grid line, the gaps between
/// them fixed, so the cells land exactly where the canvas drew the lines.
fn ticks_row() -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let cell = plot::TICK_CELL;
    let mut cells: Vec<El> = vec![block(FREQ[0].0 - cell / 2.0, 0.0).shrink(0)];
    for (i, &(x, _, name)) in FREQ.iter().enumerate() {
        if i > 0 {
            cells.push(block(x - FREQ[i - 1].0 - cell, 0.0).shrink(0));
        }
        cells.push(
            centred(
                label(name, ty::BODY, c(t.ink)),
                cell,
                plot::TICK_ROW_H,
            )
            .shrink(0),
        );
    }
    cells.push(spacer());
    row(cells).w(PLOT_W).align(Align::Center)
}

/// The level labels: the same trick down the plot's right edge, for the two
/// lines that carry a number.
fn ticks_col(range: usize) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let cell = plot::LEVEL_CELL;
    let mut cells: Vec<El> = Vec::new();
    let mut top = 0.0;
    let k = RANGES[range].0;
    let names = [format!("-{k}"), format!("-{}", 2 * k)];
    for (y, name) in below_zero(range).into_iter().zip(&names) {
        cells.push(block(0.0, y - cell / 2.0 - top).shrink(0));
        cells.push(
            centred(
                label(name.as_str(), ty::BODY, c(t.ink)),
                plot::LEVEL_COL_W,
                cell,
            )
            .shrink(0),
        );
        top = y + cell / 2.0;
    }
    cells.push(spacer());
    col(cells).h(PLOT_H)
}

/// Where a gain sits: the reference's own mapping. The gain's floor is 0.05,
/// its decibels are held to -22..19.8, and a cut hangs below the zero line.
fn gain_y(gain: f32) -> f64 {
    let db = (20.0 * gain.max(0.05).log10()).clamp(-22.0, 19.8);
    (ZERO - f64::from(db) * REDUCTION_PIXELS_PER_DECIBEL).clamp(GRID_TOP, PLOT_H)
}

/// How far apart the grid's lines are at a range: a step of the range's own
/// size, three of which make the default label.
fn level_step(range: usize) -> f64 {
    LEVEL_STEP * f64::from(RANGES[range].0) / 3.0
}

/// The grid's lines above and below the zero line, which carries its own.
fn level_lines(range: usize) -> Vec<f64> {
    let step = level_step(range);
    let mut lines = Vec::new();
    let mut n = 1.0;
    while ZERO - n * step > GRID_TOP || ZERO + n * step < PLOT_H {
        for y in [ZERO - n * step, ZERO + n * step] {
            if (GRID_TOP..PLOT_H).contains(&y) {
                lines.push(y);
            }
        }
        n += 1.0;
    }
    lines
}

/// The two lines below zero that carry a number.
fn below_zero(range: usize) -> [f64; 2] {
    let step = level_step(range);
    [ZERO + step, ZERO + 2.0 * step]
}

/// Where a magnitude of the white curve sits: its own scale, 54.1 dB across
/// the plot with zero a little above the middle.
pub(super) fn white_y(magnitude: f32) -> f64 {
    let db = 20.0 * magnitude.max(2.0e-7).log10();
    (ZERO - f64::from(db) * PIXELS_PER_DECIBEL).clamp(0.0, PLOT_H)
}

/// The white curve: what the bands ask of the reduction, and the selected
/// band's own curve over it in its colour.
fn white_curve(draws: &mut Vec<Draw>, white: &[f32; PLOT_BINS], own: Option<(&[f32; PLOT_BINS], u32)>) {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let trace = |curve: &[f32; PLOT_BINS]| -> Vec<Point> {
        (0..PLOT_BINS)
            .map(|index| pt(x_for_frequency(f64::from(plot_frequency(index))), white_y(curve[index])))
            .collect()
    };
    draws.push(Draw::stroke(Path::polyline(trace(white), false), c(t.plot_ink), stroke::CURVE));
    if let Some((curve, tint)) = own {
        draws.push(Draw::stroke(Path::polyline(trace(curve), false), c(tint), stroke::SELECTED));
    }
}
