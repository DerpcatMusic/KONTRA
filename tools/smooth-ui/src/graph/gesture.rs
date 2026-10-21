//! The plot's gestures, decided: what a press, a drag, a wheel notch, a key
//! or a reset does to a band.
//!
//! The maths is pure -- no host, no pointer -- so each rule is tested on its
//! own. `mod.rs` reads the pointer and the keys, and the plug-in applies the
//! resulting [`Change`]s through the host as gestures.

use super::axis::{
    PLOT_BINS, PLOT_W, plot_frequency, x_for_frequency, y_for_depth,
};
use super::band::Band;
use super::draw::white_y;
use super::model::band_curve;
use crate::tokens::plot;
use mui::input::{Key, KeyPress, Mods};

/// How far a pointer can be from a handle and still be on it, in plot pixels.
const SLOP: f64 = 3.0;
/// How far a pointer can be from the selected band's own curve and still be on it.
const CURVE_SLOP: f64 = 6.0;
/// The pointer's travel that makes a press a drag, in plot pixels.
pub const DRAG_SLOP: f64 = 2.0;

/// One wheel notch, in the window's scroll units. The window reports a
/// notch as five text rows of twelve points (`mui-baseview`'s `notch_lines`),
/// so a notch is sixty; a trackpad's pixels are the same scale.
pub const NOTCH: f64 = 60.0;
/// What one notch moves a band's width, as a share of the host's Q range.
pub const WIDTH_STEP: f64 = 0.02;
/// What one notch moves a band's depth, in the depth parameter's units.
pub const DEPTH_STEP: f64 = 0.5;
/// What one arrow press moves a band's centre, in octaves.
pub const FREQUENCY_STEP: f64 = 1.0 / 12.0;
/// What a fine gesture keeps of a full one.
pub const FINE: f64 = 0.2;

/// The depth parameter's range, in its own units.
pub const DEPTH_RANGE: (f64, f64) = (-10.0, 10.0);
/// The frequency parameter's range, in hertz.
pub const FREQUENCY_RANGE: (f64, f64) = (20.0, 20_000.0);

/// Which of a band's controls a step moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// The bandwidth, Q. Up is wider.
    Width,
    /// The depth, boost or cut. Up is a boost.
    Depth,
    /// The centre frequency. Up is higher.
    Frequency,
}

/// A change the plot asks of a band. The plug-in applies it through the host,
/// inside a gesture, so undo and automation see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    /// Move one control by `notches`, already scaled for fine, positive is up.
    Step { index: usize, axis: Axis, notches: f64 },
    /// Depth back to flat and width back to its default.
    Reset { index: usize },
}

impl Change {
    /// The band the change is for.
    pub fn index(&self) -> usize {
        match *self {
            Change::Step { index, .. } | Change::Reset { index } => index,
        }
    }
}

/// A press on a band, in progress: where the pointer took it, and where the
/// handle is now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub index: usize,
    /// Where the press landed, in plot pixels.
    pub press: (f64, f64),
    /// Where the pointer was last frame.
    pub last: (f64, f64),
    /// Where the handle is, in plot pixels.
    pub at: (f64, f64),
    /// Whether the pointer has gone beyond the slop. A press that has not
    /// moved is a select, and changes nothing.
    pub moved: bool,
}

impl Drag {
    /// A press on band `index` at `press`, with its handle at `at`.
    pub fn press(index: usize, press: (f64, f64), at: (f64, f64)) -> Self {
        Self { index, press, last: press, at, moved: false }
    }

    /// Take this frame's pointer. Returns the handle's place once the drag
    /// has moved: a full drag puts the handle under the pointer, a fine one
    /// moves it by `fine` of the pointer's travel since last frame.
    pub fn follow(&mut self, p: (f64, f64), fine: f64) -> Option<(f64, f64)> {
        if !self.moved && (p.0 - self.press.0).hypot(p.1 - self.press.1) > DRAG_SLOP {
            self.moved = true;
        }
        if self.moved {
            self.at = if fine >= 1.0 {
                p
            } else {
                (self.at.0 + fine * (p.0 - self.last.0), self.at.1 + fine * (p.1 - self.last.1))
            };
            self.at.0 = self.at.0.clamp(0.0, PLOT_W);
        }
        self.last = p;
        self.moved.then_some(self.at)
    }
}

/// What the plot holds between frames: the drag in progress, and the changes
/// the frame asked for, for the plug-in to apply.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gesture {
    pub drag: Option<Drag>,
    pub changes: Vec<Change>,
}

/// The factor a wheel notch keeps: Shift or Ctrl is fine.
pub fn wheel_scale(mods: Mods) -> f64 {
    if mods.shift || mods.ctrl { FINE } else { 1.0 }
}

/// The factor a drag keeps: Shift is fine.
pub fn drag_scale(mods: Mods) -> f64 {
    if mods.shift { FINE } else { 1.0 }
}

/// The change one wheel delta makes to a band. The window's delta is
/// negative when the wheel scrolls up, so an up notch is positive here:
/// wider, or with Alt a boost.
pub fn wheel_change(index: usize, wheel_y: f64, mods: Mods) -> Change {
    let axis = if mods.alt { Axis::Depth } else { Axis::Width };
    Change::Step { index, axis, notches: -wheel_y / NOTCH * wheel_scale(mods) }
}

/// The change an arrow key makes: up and down move the depth, left and right
/// the centre frequency. Anything else changes nothing.
pub fn key_change(index: usize, press: KeyPress) -> Option<Change> {
    let (axis, notches) = match press.key {
        Key::Up => (Axis::Depth, 1.0),
        Key::Down => (Axis::Depth, -1.0),
        Key::Right => (Axis::Frequency, 1.0),
        Key::Left => (Axis::Frequency, -1.0),
        _ => return None,
    };
    Some(Change::Step { index, axis, notches: notches * wheel_scale(press.mods) })
}

/// A depth moved by `notches`, held to the parameter's range.
pub fn depth_after(depth: f64, notches: f64) -> f64 {
    (depth + notches * DEPTH_STEP).clamp(DEPTH_RANGE.0, DEPTH_RANGE.1)
}

/// A centre frequency moved by `notches` of [`FREQUENCY_STEP`] octaves, held
/// to the parameter's range.
pub fn frequency_after(frequency: f64, notches: f64) -> f64 {
    (frequency * 2.0_f64.powf(notches * FREQUENCY_STEP)).clamp(FREQUENCY_RANGE.0, FREQUENCY_RANGE.1)
}

/// The band a point is on: a handle first, topmost first, then the selected
/// band's own curve, so the wheel over a band's line is that band.
pub fn band_under(bands: &[Band], selected: usize, p: (f64, f64)) -> Option<usize> {
    handle_under(bands, p).or_else(|| curve_under(bands, selected, p))
}

/// The handle a point is on. The pills are the edge bands; the rest are dots.
pub fn handle_under(bands: &[Band], p: (f64, f64)) -> Option<usize> {
    bands
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, band)| band.active)
        .find(|(_, band)| {
            let (dx, dy) = (
                (p.0 - x_for_frequency(band.frequency)).abs(),
                (p.1 - y_for_depth(band.depth)).abs(),
            );
            if band.is_edge() {
                dx <= plot::PILL_W / 2.0 + SLOP && dy <= plot::PILL_H / 2.0 + SLOP
            } else {
                dx.hypot(dy) <= plot::DOT_R + SLOP
            }
        })
        .map(|(index, _)| index)
}

/// The selected band, when the point is on its own curve.
fn curve_under(bands: &[Band], selected: usize, p: (f64, f64)) -> Option<usize> {
    let band = bands.get(selected).filter(|band| band.active)?;
    let curve = band_curve(band);
    let x_of = |i: usize| x_for_frequency(f64::from(plot_frequency(i)));
    let nearest = (0..PLOT_BINS).min_by(|&a, &b| (x_of(a) - p.0).abs().total_cmp(&(x_of(b) - p.0).abs()))?;
    ((white_y(curve[nearest]) - p.1).abs() <= CURVE_SLOP).then_some(selected)
}

#[cfg(test)]
mod tests {
    use super::super::axis::{depth_for_y, frequency_for_x};
    use super::*;
    use crate::graph::{AT_REST_BANDS, Band};

    fn none() -> Mods {
        Mods::default()
    }

    fn press(key: Key, mods: Mods) -> KeyPress {
        KeyPress { key, mods }
    }

    #[test]
    fn a_wheel_up_widens_and_a_wheel_down_narrows() {
        // The window reports up as a negative delta.
        assert_eq!(wheel_change(2, -NOTCH, none()), Change::Step { index: 2, axis: Axis::Width, notches: 1.0 });
        assert_eq!(wheel_change(2, NOTCH, none()), Change::Step { index: 2, axis: Axis::Width, notches: -1.0 });
    }

    #[test]
    fn alt_turns_the_wheel_to_depth_and_shift_or_ctrl_make_it_fine() {
        let alt = Mods { alt: true, ..none() };
        assert!(matches!(wheel_change(0, -NOTCH, alt), Change::Step { axis: Axis::Depth, notches, .. } if notches == 1.0));
        for fine in [Mods { shift: true, ..none() }, Mods { ctrl: true, ..none() }] {
            let Change::Step { notches, .. } = wheel_change(0, -NOTCH, fine) else { panic!() };
            assert!((notches - FINE).abs() < 1e-12);
        }
    }

    #[test]
    fn a_wheel_delta_is_scaled_by_its_size_so_a_trackpad_is_smooth() {
        let Change::Step { notches, .. } = wheel_change(0, -NOTCH / 4.0, none()) else { panic!() };
        assert!((notches - 0.25).abs() < 1e-12);
    }

    #[test]
    fn arrows_move_depth_and_frequency_and_fine_keys_move_less() {
        let up = key_change(1, press(Key::Up, none()));
        assert_eq!(up, Some(Change::Step { index: 1, axis: Axis::Depth, notches: 1.0 }));
        let left = key_change(1, press(Key::Left, Mods { shift: true, ..none() }));
        assert_eq!(left, Some(Change::Step { index: 1, axis: Axis::Frequency, notches: -FINE }));
        assert_eq!(key_change(1, press(Key::Char('a'), none())), None);
    }

    #[test]
    fn depth_and_frequency_are_held_to_their_ranges() {
        assert_eq!(depth_after(9.9, 10.0), DEPTH_RANGE.1);
        assert_eq!(depth_after(-10.0, -1.0), DEPTH_RANGE.0);
        assert!((depth_after(0.0, 2.0) - 2.0 * DEPTH_STEP).abs() < 1e-12);
        assert!((frequency_after(1000.0, 12.0) - 2000.0).abs() < 1e-6);
        assert_eq!(frequency_after(19_000.0, 12.0), FREQUENCY_RANGE.1);
        assert_eq!(frequency_after(21.0, -12.0), FREQUENCY_RANGE.0);
    }

    #[test]
    fn a_handle_is_hit_inside_its_shape_and_a_pill_is_taller_than_a_dot() {
        let bands = AT_REST_BANDS;
        // Band 1 is a dot at 650 Hz, 3.6 units up.
        let dot = (x_for_frequency(650.0), y_for_depth(3.6));
        assert_eq!(handle_under(&bands, dot), Some(1));
        assert_eq!(handle_under(&bands, (dot.0 + plot::DOT_R + 1.0, dot.1)), Some(1));
        assert_eq!(handle_under(&bands, (dot.0 + plot::DOT_R + 10.0, dot.1)), None);
        // Band 0 is the low cut, a pill: it reaches well above and below its centre.
        let pill = (x_for_frequency(168.0), y_for_depth(0.0));
        assert_eq!(handle_under(&bands, (pill.0, pill.1 + plot::PILL_H / 2.0)), Some(0));
        assert_eq!(handle_under(&bands, (pill.0, pill.1 + plot::PILL_H / 2.0 + 20.0)), None);
    }

    #[test]
    fn the_topmost_handle_wins_and_an_inactive_band_is_not_hit() {
        let mut bands = AT_REST_BANDS;
        bands[5] = Band { active: true, ..Band::at(1000.0, 0.0, 2, true) };
        bands[6] = Band { active: true, ..Band::at(1000.0, 0.0, 2, true) };
        let p = (x_for_frequency(1000.0), y_for_depth(0.0));
        assert_eq!(handle_under(&bands, p), Some(6));
        bands[6].active = false;
        assert_eq!(handle_under(&bands, p), Some(5));
    }

    #[test]
    fn the_selected_band_is_hit_on_its_own_curve_and_nowhere_else() {
        let mut bands = AT_REST_BANDS;
        // A bell 3 units up at 1 kHz. Forty pixels off its handle, its curve is the hit.
        bands[4] = Band { active: true, ..Band::at(1000.0, 3.0, 2, true) };
        let curve = band_curve(&bands[4]);
        let x = x_for_frequency(1000.0) + 40.0;
        let nearest = (0..PLOT_BINS)
            .min_by(|&a, &b| {
                (x_for_frequency(f64::from(plot_frequency(a))) - x).abs()
                    .total_cmp(&(x_for_frequency(f64::from(plot_frequency(b))) - x).abs())
            })
            .unwrap();
        let on_curve = (x, white_y(curve[nearest]));
        assert_eq!(handle_under(&bands, on_curve), None, "not on the handle");
        assert_eq!(band_under(&bands, 4, on_curve), Some(4));
        assert_eq!(band_under(&bands, 0, on_curve), None, "another band's curve is not this one");
        assert_eq!(band_under(&bands, 4, (x, y_for_depth(-9.0))), None, "far below the curve");
    }

    #[test]
    fn a_press_that_does_not_move_changes_nothing_and_a_drag_puts_the_handle_under_the_pointer() {
        let mut drag = Drag::press(1, (100.0, 50.0), (100.0, 50.0));
        assert_eq!(drag.follow((100.5, 50.5), 1.0), None, "within the slop");
        assert_eq!(drag.follow((140.0, 60.0), 1.0), Some((140.0, 60.0)));
        assert_eq!(drag.follow((150.0, 70.0), 1.0), Some((150.0, 70.0)));
    }

    #[test]
    fn a_fine_drag_moves_the_handle_by_a_share_of_the_travel() {
        let mut drag = Drag::press(1, (100.0, 50.0), (100.0, 50.0));
        drag.follow((120.0, 50.0), FINE);
        // The first frame past the slop moves the handle a fifth of the step from its last place.
        let at = drag.follow((140.0, 50.0), FINE).unwrap();
        assert!((at.0 - (100.0 + FINE * 20.0 + FINE * 20.0)).abs() < 1e-9, "{at:?}");
    }

    #[test]
    fn a_handle_dragged_off_the_plot_stays_on_it() {
        let mut drag = Drag::press(0, (10.0, 10.0), (10.0, 10.0));
        let at = drag.follow((-500.0, 10.0), 1.0).unwrap();
        assert_eq!(at.0, 0.0);
        let at = drag.follow((PLOT_W + 400.0, 10.0), 1.0).unwrap();
        assert_eq!(at.0, PLOT_W);
        assert!(frequency_for_x(at.0) <= FREQUENCY_RANGE.1 && depth_for_y(at.1) <= DEPTH_RANGE.1);
    }

    #[test]
    fn changes_name_their_band() {
        assert_eq!(Change::Reset { index: 3 }.index(), 3);
        assert_eq!(Change::Step { index: 5, axis: Axis::Width, notches: 1.0 }.index(), 5);
    }
}
