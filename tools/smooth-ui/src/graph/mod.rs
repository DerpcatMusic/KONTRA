//! SMOOTH's visualizer: the reduction, the depth curves, and the
//! handles that shape them.
//!
//! Four parts: `axis` (where a frequency and a depth sit), `band` (what the
//! editor holds for each band), `data` (the engine's frame, as it came)
//! and `draw` (the canvas). `gesture` decides what a press, drag, wheel
//! notch, reset or key does; this file reads the pointer and the keys and
//! hands those decisions to the plug-in, which applies them through the host.

mod axis;
mod band;
mod data;
mod draw;
mod gesture;
mod model;

pub use axis::*;
pub use band::*;
pub use data::PlotView;
pub use gesture::{
    Axis, Change, Drag, Gesture, WIDTH_STEP, band_under, depth_after, frequency_after,
};
pub use model::{band_curve, curve};

use draw::PLOT_ID;
use gesture::{drag_scale, handle_under, key_change, wheel_change};
use mui::input::Key;
use mui::prelude::*;

pub fn response(
    ui: &mut Ui,
    bands: &mut [Band],
    selected: &mut usize,
    view: &PlotView,
    range: usize,
    show_frequency: bool,
    gesture: &mut Gesture,
) -> El {
    let mods = ui.pointer().mods;
    let plot = ui.get(PLOT_ID);
    let local = ui.local(PLOT_ID).map(|p| (p.x, p.y));
    let inside = local.filter(|&(x, y)| (0.0..PLOT_W).contains(&x) && (0.0..PLOT_H).contains(&y));

    // A press on a handle takes its band. The press selects it; a double
    // click or a Ctrl press resets it; and a drag moves it once the pointer
    // has gone. The tag is latched at the press, so it names the band the
    // gesture took even when the pointer has left it.
    match (held(ui), local) {
        (Some(index), Some(p)) if index < bands.len() => {
            *selected = index;
            if gesture.drag.is_none_or(|d| d.index != index) {
                let band = bands[index];
                let at = (x_for_frequency(band.frequency), y_for_depth(band.depth));
                gesture.drag = Some(Drag::press(index, p, at));
                if plot.double_clicked || mods.ctrl {
                    gesture.changes.push(Change::Reset { index });
                }
                ui.focus(PLOT_ID);
            }
            if let Some(mut drag) = gesture.drag {
                if let Some((x, y)) = drag.follow(p, drag_scale(mods)) {
                    let band = &mut bands[index];
                    band.frequency = frequency_for_x(x);
                    band.depth = depth_for_y(y);
                }
                gesture.drag = Some(drag);
            }
        }
        _ => gesture.drag = None,
    }

    // A double click on bare plot adds the next free band where it landed.
    if plot.double_clicked
        && held(ui).is_none()
        && let Some(point) = local
        && handle_under(bands, point).is_none()
        && let Some(index) = bands.iter().position(|band| !band.active)
    {
        let frequency = frequency_for_x(point.0);
        let depth = depth_for_y(point.1);
        bands[index] = Band {
            frequency,
            depth,
            active: true,
            ..Band::at(1000.0, 0.0, shape_for_drop(frequency, depth * BAND_DB_PER_UNIT), true)
        };
        *selected = index;
    }

    // What the pointer is on: the dragged band, else the band under it.
    let hovered = gesture
        .drag
        .map(|d| d.index)
        .or_else(|| inside.and_then(|p| band_under(bands, *selected, p)));

    // The wheel over a band moves it; over bare plot it moves the selected one.
    // Nothing moves when no band is selected.
    let wheel = plot.wheel.y;
    if wheel != 0.0 {
        let target = hovered.unwrap_or(*selected);
        if bands.get(target).is_some_and(|band| band.active) {
            gesture.changes.push(wheel_change(target, wheel, mods));
        }
    }

    // The keys, while the plot holds the focus: arrows step the selected band,
    // Delete takes it out of the plot.
    if ui.focused(PLOT_ID) && *selected < bands.len() && bands[*selected].active {
        for press in ui.keys(PLOT_ID).to_vec() {
            if matches!(press.key, Key::Delete | Key::Backspace) {
                bands[*selected].active = false;
            } else if let Some(change) = key_change(*selected, press) {
                gesture.changes.push(change);
            }
        }
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
            hovered: hovered == Some(index),
        })
        .collect();
    // The frequency under the pointer, when the preference asks for it.
    let cursor = inside
        .filter(|_| show_frequency)
        .map(|p| (p.0, p.1, frequency_for_x(p.0)));
    let white = model::curve(bands);
    let own = bands
        .get(*selected)
        .filter(|band| band.active)
        .map(|band| (model::band_curve(band), tints[*selected % tints.len()]));
    // The pointer says what the plot will do: a handle grabs, a drag grabs harder.
    let shape = if gesture.drag.is_some() {
        Cursor::Grabbing
    } else if hovered.is_some() {
        Cursor::Grab
    } else {
        Cursor::Arrow
    };
    draw::plot(&handles, view, range, cursor, &white, own.as_ref().map(|(c, tint)| (c, *tint)), shape)
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
