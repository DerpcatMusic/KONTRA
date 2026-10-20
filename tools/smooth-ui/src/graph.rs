//! SMOOTH's visualizer: the frequency response, and the handles that shape it.
//!
//! The plot is one canvas -- the fill, the grid, the curve -- with the tick
//! labels and the range chip over it in a stack. Every tick is a fixed cell
//! between two fixed gaps, so the labels land on the grid lines without a
//! single coordinate in the layout: the grid the canvas draws and the row the
//! labels sit in are built from the same `FREQ` table.
//!
//! That table, and the zero line it is read against, are soothe3's own axis:
//! they are measurements of the reference, held in `tokens::plot`. Everything
//! else here is a token.

use crate::graph_data::{CURVE, REDUCTION};
use crate::skin::*;
use crate::theme;
use crate::tokens::{icon, plot, space, stroke, ty};
use mui::prelude::*;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// How many points the live plot carries.
pub const PLOT_BINS: usize = 128;

/// What the engine is doing, published from the audio thread.
///
/// The engine writes it; the editor reads it. The values are the plot's own:
/// a log-spaced sweep from 20 Hz to 20 kHz, the spectrum in decibels, and the
/// reduction each channel took.
#[derive(Debug)]
pub struct Telemetry {
    spectrum: [AtomicU32; PLOT_BINS],
    reduction: [[AtomicU32; PLOT_BINS]; 2],
    /// Bumped on every publish, so the editor can tell a live plot from an
    /// engine that has not run yet.
    sequence: AtomicU64,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            spectrum: std::array::from_fn(|_| AtomicU32::new(0)),
            reduction: [
                std::array::from_fn(|_| AtomicU32::new(0)),
                std::array::from_fn(|_| AtomicU32::new(0)),
            ],
            sequence: AtomicU64::new(0),
        }
    }
}

impl Telemetry {
    /// Publish one frame of the engine's plot.
    pub fn publish(&self, data: [[f32; PLOT_BINS]; 3]) {
        for (slot, value) in self.spectrum.iter().zip(data[0]) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        for channel in 0..2 {
            for (slot, value) in self.reduction[channel].iter().zip(data[channel + 1]) {
                slot.store(value.to_bits(), Ordering::Relaxed);
            }
        }
        self.sequence.fetch_add(1, Ordering::Relaxed);
    }

    /// Whether the engine has published anything yet.
    pub fn live(&self) -> bool {
        self.sequence.load(Ordering::Relaxed) > 0
    }

    /// How many frames the engine has published.
    pub fn sequence(&self) -> u64 {
        self.sequence.load(Ordering::Relaxed)
    }

    /// The spectrum, in decibels.
    pub fn spectrum(&self) -> [f32; PLOT_BINS] {
        std::array::from_fn(|index| f32::from_bits(self.spectrum[index].load(Ordering::Relaxed)))
    }

    /// The reduction a channel took, in decibels.
    pub fn reduction(&self, channel: usize) -> [f32; PLOT_BINS] {
        let channel = channel.min(1);
        std::array::from_fn(|index| {
            f32::from_bits(self.reduction[channel][index].load(Ordering::Relaxed))
        })
    }
}

/// The frequency a plot point stands for: the engine's own log sweep.
fn plot_frequency(index: usize) -> f64 {
    SWEEP_LOW * (SWEEP_HIGH / SWEEP_LOW).powf(index as f64 / (PLOT_BINS - 1).max(1) as f64)
}

/// The sweep the telemetry is published on, in hertz.
const SWEEP_LOW: f64 = 20.0;
const SWEEP_HIGH: f64 = 20_000.0;

/// A point in the plot's own space. The plot is a plane, not a dial, so it
/// reads in x and y rather than in radius and degrees.
fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// The plot's own size: the right column's width, and the height the toolbar
/// and footer leave.
pub const PLOT_W: f64 = plot::W;
pub const PLOT_H: f64 = plot::H;

/// Where the zero line sits, and the band the grid starts below.
const ZERO: f64 = plot::ZERO;
const GRID_TOP: f64 = plot::GRID_TOP;

/// The frequency grid, in the plot's space, and what each line is worth.
pub const FREQ: [(f64, f64, &str); 8] = plot::FREQ;

/// The level grid. Only three lines carry a number.
pub const LEVEL: [f64; 6] = plot::LEVEL;

/// Where a band's depth is dragged.
#[derive(Clone, Copy)]
pub struct Handle {
    /// Centre, in the plot's space.
    pub x: f64,
    pub y: f64,
    /// Which band it belongs to: the only thing that tells them apart.
    pub tint: u32,
    /// The low bands are tall pills; the narrow ones are dots.
    pub pill: bool,
}

/// One band, as the editor holds it: where it sits and how deep it cuts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    /// The centre, in hertz.
    pub frequency: f64,
    /// The depth, in decibels. Negative cuts.
    pub depth: f64,
}

/// How many bands the editor draws.
pub const BANDS: usize = 4;

/// The four bands at rest, as the reference shows them.
pub const AT_REST_BANDS: [Band; BANDS] = [
    Band {
        frequency: 168.0,
        depth: 0.0,
    },
    Band {
        frequency: 650.0,
        depth: -2.3,
    },
    Band {
        frequency: 3210.0,
        depth: 3.2,
    },
    Band {
        frequency: 9410.0,
        depth: 0.0,
    },
];

/// Where a frequency sits on the plot's axis.
///
/// The axis is the reference's own: the frequency grid is not evenly spaced
/// in decibels, so the mapping is read off the grid rather than assumed.
pub fn x_for_frequency(frequency: f64) -> f64 {
    let frequency = frequency.clamp(SWEEP_LOW, SWEEP_HIGH);
    if frequency <= FREQ[0].1 {
        // Below the first grid line, the axis runs on the same slope.
        return FREQ[0].0
            - (FREQ[0].1.ln() - frequency.ln()) / (FREQ[1].1.ln() - FREQ[0].1.ln())
                * (FREQ[1].0 - FREQ[0].0);
    }
    for pair in FREQ.windows(2) {
        let (x0, f0, _) = pair[0];
        let (x1, f1, _) = pair[1];
        if frequency <= f1 {
            return x0 + (x1 - x0) * (frequency.ln() - f0.ln()) / (f1.ln() - f0.ln());
        }
    }
    let (x0, f0, _) = FREQ[FREQ.len() - 2];
    let (x1, f1, _) = FREQ[FREQ.len() - 1];
    x1 + (x1 - x0) * (frequency.ln() - f1.ln()) / (f1.ln() - f0.ln())
}

/// The frequency at a place on the axis.
pub fn frequency_for_x(x: f64) -> f64 {
    if x <= FREQ[0].0 {
        let (x1, f1, _) = FREQ[1];
        return (FREQ[0].1.ln() - (FREQ[0].0 - x) / (x1 - FREQ[0].0) * (f1.ln() - FREQ[0].1.ln()))
            .exp();
    }
    for pair in FREQ.windows(2) {
        let (x0, f0, _) = pair[0];
        let (x1, f1, _) = pair[1];
        if x <= x1 {
            return (f0.ln() + (f1.ln() - f0.ln()) * (x - x0) / (x1 - x0)).exp();
        }
    }
    let (x0, f0, _) = FREQ[FREQ.len() - 2];
    let (x1, f1, _) = FREQ[FREQ.len() - 1];
    (f1.ln() + (f1.ln() - f0.ln()) * (x - x1) / (x1 - x0)).exp()
}

/// How many pixels a decibel is worth on the plot.
const PIXELS_PER_DECIBEL: f64 = plot::PIXELS_PER_DECIBEL;

/// Where a depth sits on the plot's axis. A cut is below the zero line.
pub fn y_for_depth(depth: f64) -> f64 {
    ZERO + depth * PIXELS_PER_DECIBEL
}

/// The depth at a place on the axis.
pub fn depth_for_y(y: f64) -> f64 {
    ((y - ZERO) / PIXELS_PER_DECIBEL).clamp(-10.0, 10.0)
}

/// The response plot: the reduction fill under the curve, the grid it is read
/// against, and the handles that shape it.
///
/// The handles are live: a drag on one moves it, and the band it stands for
/// follows.
pub fn response(ui: &Ui, bands: &mut [Band], telemetry: &Telemetry) -> El {
    // A drag on a handle moves it. The tag is latched at the press, so it
    // names the band the gesture took even when the pointer has left it.
    if let Some(index) = held(ui)
        && let Some(point) = ui.local(PLOT_ID)
        && let Some(band) = bands.get_mut(index)
    {
        band.frequency = frequency_for_x(point.x);
        band.depth = depth_for_y(point.y);
    }
    let live = telemetry.live();
    let tints = band_tints();
    let handles: Vec<Handle> = bands
        .iter()
        .enumerate()
        .map(|(index, band)| Handle {
            x: x_for_frequency(band.frequency),
            y: y_for_depth(band.depth),
            tint: tints[index % tints.len()],
            pill: index == 0 || index == BANDS - 1,
        })
        .collect();
    plot(&handles, telemetry, live)
}

/// The band the plot's handle gesture is holding, if any.
///
/// The plot is not a widget, so nothing in the scene reports a gesture for
/// it: the press latches the shape's tag, and the tag names the band. A
/// caller that owns the band parameters -- the plug-in's host bridge --
/// asks this once per frame instead of re-deriving the tag, and a frame
/// where the pointer has let go answers `None`.
pub fn held(ui: &Ui) -> Option<usize> {
    if !ui.pointer().buttons.contains(mui::input::Button::Primary) {
        return None;
    }
    ui.tag(PLOT_ID)
        .and_then(|tag| tag.strip_prefix("band-"))
        .and_then(|text| text.parse::<usize>().ok())
        .filter(|index| *index < BANDS)
}

/// The colours the bands wear, from the active theme's own four.
fn band_tints() -> [u32; 4] {
    let t = theme::t();
    [t.red, t.magenta, t.cyan, t.green]
}

/// The name the plot carries, so the pointer can say which band it is on.
const PLOT_ID: &str = "plot";

/// The plot itself, drawn from the handles it is given.
fn plot(handles: &[Handle], telemetry: &Telemetry, live: bool) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let mut draws: Vec<Draw> = Vec::new();

    // The response: the reduction the engine took, or the reference's own
    // trace when there is no engine to ask.
    let reduction: Vec<(f64, f64)> = if live {
        let values = telemetry.reduction(0);
        (0..PLOT_BINS)
            .map(|index| {
                let x = x_for_frequency(plot_frequency(index));
                // A cut goes down.
                let y = ZERO + values[index] as f64 * PIXELS_PER_DECIBEL;
                (x, y)
            })
            .collect()
    } else {
        REDUCTION.iter().map(|&(x, y)| (x, y)).collect()
    };
    let curve: Vec<(f64, f64)> = if live {
        let values = telemetry.reduction(0);
        (0..PLOT_BINS)
            .map(|index| {
                let x = x_for_frequency(plot_frequency(index));
                let y = ZERO + values[index] as f64 * PIXELS_PER_DECIBEL;
                (x, y)
            })
            .collect()
    } else {
        CURVE.iter().map(|&(x, y)| (x, y)).collect()
    };

    // The area under the reduction, closed on the plot's baseline.
    let mut area: Vec<Point> = reduction.iter().map(|&(x, y)| pt(x, y)).collect();
    area.push(pt(PLOT_W, PLOT_H));
    area.push(pt(0.0, PLOT_H));
    draws.push(Draw::fill(
        Path::polyline(area, true),
        Gradient::vertical(c(t.fill_top), c(t.fill_bottom)),
    ));
    draws.push(Draw::stroke(
        Path::polyline(
            reduction.iter().map(|&(x, y)| pt(x, y)).collect::<Vec<_>>(),
            false,
        ),
        ca(t.plot_ink, t.veil_soft),
        stroke::RULE,
    ));

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
    draws.push(Draw::stroke(
        Path::polyline(
            curve.iter().map(|&(x, y)| pt(x, y)).collect::<Vec<_>>(),
            false,
        ),
        c(t.plot_ink),
        stroke::CURVE,
    ));

    for (index, h) in handles.iter().enumerate() {
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
