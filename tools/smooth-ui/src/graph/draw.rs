//! The plot, drawn: the reduction, the spectrum, the depth curves, the grid
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
use super::data::{FLOOR_DB, PlotView};
use crate::skin::*;
use crate::theme;
use crate::tokens::{icon, plot, space, stroke, ty};
use mui::prelude::*;

/// The name the plot carries, so the pointer can say which band it is on.
pub const PLOT_ID: &str = "plot";

/// The plot itself, drawn from the engine's data and the handles it is given.
pub fn plot(handles: &[Handle], view: &PlotView) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let mut draws: Vec<Draw> = Vec::new();

    // The reduction the engine is taking, hanging from the zero line: the
    // material is what is being removed, so a notch is a resonance being
    // pulled down. Silent, it is the flat shelf under the zero line.
    let fill: Vec<(f64, f64)> = (0..PLOT_BINS)
        .map(|index| {
            let x = x_for_frequency(f64::from(plot_frequency(index)));
            let y = (ZERO + f64::from(view.reduction[0][index]) * PIXELS_PER_DECIBEL).min(PLOT_H);
            (x, y)
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

    // The area under the reduction, closed on the plot's baseline.
    let mut area: Vec<Point> = fill.iter().map(|&(x, y)| pt(x, y)).collect();
    area.push(pt(PLOT_W, PLOT_H));
    area.push(pt(0.0, PLOT_H));
    draws.push(Draw::fill(
        Path::polyline(area, true),
        Gradient::vertical(c(t.fill_top), c(t.fill_bottom)),
    ));
    draws.push(Draw::stroke(
        Path::polyline(fill.iter().map(|&(x, y)| pt(x, y)).collect::<Vec<_>>(), false),
        ca(t.plot_ink, t.veil_soft),
        stroke::RULE,
    ));

    // The input spectrum: a line of the signal itself, so the reduction can
    // be read against what it is acting on.
    if view.live && view.spectrum.iter().any(|db| *db > FLOOR_DB + 1.0) {
        let line: Vec<Point> = (0..PLOT_BINS)
            .map(|index| {
                let frequency = plot_frequency(index);
                pt(
                    x_for_frequency(f64::from(frequency)),
                    spectrum_y(view.spectrum[index], frequency),
                )
            })
            .collect();
        draws.push(Draw::stroke(
            Path::polyline(line, false),
            ca(t.plot_ink, SPECTRUM_ALPHA),
            stroke::HAIR,
        ));
    }

    for &y in &LEVEL {
        if (y - ZERO).abs() > plot::ZERO_MARGIN {
            draws.push(Draw::stroke(
                line(pt(0.0, y), pt(PLOT_W, y)),
                ca(t.plot_ink, t.veil),
                stroke::HAIR,
            ));
        }
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
    depth_curves(&mut draws, view);

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
        // The tag is how a drag knows which band it took.
        draws.push(Draw::hit(outer, format!("band-{index}")).at(centre));
    }

    stack![
        block(PLOT_W, PLOT_H).fill(Gradient::vertical(c(t.graph_top), c(t.graph_bottom))),
        canvas(move |_| draws.clone()).full().id(PLOT_ID),
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
        ticks_col()
            .anchor(Align::End, Align::Start)
            .w(plot::LEVEL_CELL * 2.0 + space::TIGHT),
        range_chip()
            .anchor(Align::End, Align::End)
            .offset(-space::XS, -(space::L + space::TIGHT)),
    ]
    .size(PLOT_W, PLOT_H)
    .shrink(0)
}

/// The running range: the span the plot is showing, in the corner.
fn range_chip() -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    stack![
        block(plot::RANGE_W, plot::RANGE_H)
            .radius(SKIN.corners.field)
            .fill(ca(t.range_chip, t.chip_alpha)),
        label("-9", ty::MICRO, c(t.ink_quiet))
            .anchor(Align::Start, Align::Center)
            .offset(space::XS, space::NONE),
        glyph(Icon::Down, icon::SMALL, c(t.ink_quiet))
            .anchor(Align::End, Align::Center)
            .offset(-space::TIGHT, space::NONE),
    ]
    .size(plot::RANGE_W, plot::RANGE_H)
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
                label(name, ty::TICK, c(t.ink_quiet)),
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
fn ticks_col() -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let cell = plot::LEVEL_CELL;
    let mut cells: Vec<El> = Vec::new();
    let mut top = 0.0;
    for &(y, name) in &[(LEVEL[3], "-3"), (LEVEL[4], "-6")] {
        cells.push(block(0.0, y - cell / 2.0 - top).shrink(0));
        cells.push(
            centred(
                label(name, ty::MICRO, c(t.ink_quiet)),
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

/// How visible the spectrum line is.
const SPECTRUM_ALPHA: f32 = 0.55;

/// The spectrum's own scale: the top of the plot is this level, and the floor
/// is [`SPECTRUM_SPAN_DB`] below it. Each octave above a kilohertz is lifted
/// by [`SPECTRUM_TILT_DB`], so broadband programme reads level instead of
/// sloping off.
const SPECTRUM_TOP_DB: f32 = -10.0;
const SPECTRUM_SPAN_DB: f32 = 90.0;
const SPECTRUM_TILT_DB: f32 = 3.0;

/// Where a spectrum point sits.
fn spectrum_y(db: f32, frequency: f32) -> f64 {
    let lifted = db + SPECTRUM_TILT_DB * (frequency / 1000.0).log2();
    let fraction = ((SPECTRUM_TOP_DB - lifted) / SPECTRUM_SPAN_DB).clamp(0.0, 1.0);
    GRID_TOP + f64::from(fraction) * (PLOT_H - GRID_TOP)
}

/// The depth curves: what the bands ask of the reduction. The second
/// channel's, when it differs, is drawn darker under the first.
fn depth_curves(draws: &mut Vec<Draw>, view: &PlotView) {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let trace = |channel: usize| -> Vec<Point> {
        (0..PLOT_BINS)
            .map(|index| {
                let x = x_for_frequency(f64::from(plot_frequency(index)));
                let y = (ZERO + f64::from(view.depth[channel][index]) * PIXELS_PER_DECIBEL)
                    .clamp(GRID_TOP, PLOT_H);
                pt(x, y)
            })
            .collect()
    };
    let differs = view.depth[0]
        .iter()
        .zip(&view.depth[1])
        .any(|(a, b)| (a - b).abs() > 0.05);
    if differs {
        draws.push(Draw::stroke(
            Path::polyline(trace(1), false),
            ca(t.plot_ink, t.veil_soft),
            stroke::CURVE,
        ));
    }
    draws.push(Draw::stroke(
        Path::polyline(trace(0), false),
        c(t.plot_ink),
        stroke::CURVE,
    ));
}
