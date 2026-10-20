//! SMOOTH's visualizer: the reduction, the depth curves, and the
//! handles that shape them.
//!
//! Four parts: `axis` (where a frequency and a depth sit), `band` (what the
//! editor holds for each band), `data` (the engine's frame, as it came)
//! and `draw` (the canvas). This file is the interaction: a press on a
//! handle takes a band, a drag moves it, a double click on bare plot adds one.

mod axis;
mod band;
mod data;
mod draw;

pub use axis::*;
pub use band::*;
pub use data::PlotView;

use draw::PLOT_ID;
use mui::prelude::*;

pub fn response(
    ui: &Ui,
    bands: &mut [Band],
    selected: &mut usize,
    view: &PlotView,
    range: usize,
    show_frequency: bool,
) -> El {
    // A drag on a handle moves it. The tag is latched at the press, so it
    // names the band the gesture took even when the pointer has left it.
    if let Some(index) = held(ui)
        && let Some(point) = ui.local(PLOT_ID)
        && let Some(band) = bands.get_mut(index)
    {
        band.frequency = frequency_for_x(point.x);
        band.depth = depth_for_y(point.y);
        *selected = index;
    }
    // A double click on bare plot adds the next free band where it landed.
    if ui.get(PLOT_ID).double_clicked
        && let Some(point) = ui.local(PLOT_ID)
        && !bands.iter().any(|band| {
            band.active
                && (x_for_frequency(band.frequency) - point.x).hypot(y_for_depth(band.depth) - point.y)
                    < crate::tokens::plot::DOT_R * 2.0
        })
        && let Some(index) = bands.iter().position(|band| !band.active)
    {
        let frequency = frequency_for_x(point.x);
        let depth = depth_for_y(point.y);
        bands[index] = Band {
            frequency,
            depth,
            active: true,
            ..Band::at(1000.0, 0.0, shape_for_drop(frequency, depth * BAND_DB_PER_UNIT), true)
        };
        *selected = index;
    }
    let tints = band_tints();
    let handles: Vec<Handle> = bands
        .iter()
        .enumerate()
        .filter(|(_, band)| band.active)
        .map(|(index, band)| Handle {
            index,
            selected: index == *selected,
            x: x_for_frequency(band.frequency),
            y: y_for_depth(band.depth),
            tint: tints[index % tints.len()],
            pill: band.is_edge(),
        })
        .collect();
    // The frequency under the pointer, when the preference asks for it.
    let cursor = ui
        .local(PLOT_ID)
        .filter(|p| show_frequency && (0.0..PLOT_W).contains(&p.x) && (0.0..PLOT_H).contains(&p.y))
        .map(|p| (p.x, p.y, frequency_for_x(p.x)));
    draw::plot(&handles, view, range, cursor)
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
