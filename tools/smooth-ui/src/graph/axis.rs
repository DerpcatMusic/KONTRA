//! The plot's axes: where a frequency and a depth sit on the plot.
//!
//! The frequency grid and the zero line are soothe3's own, held in
//! `tokens::plot`; the mappings here read them rather than assume a law.

use crate::tokens::plot;
use mui::prelude::*;
pub use smooth_core::plot::{PLOT_BINS, SWEEP_HIGH, SWEEP_LOW, axis_frequency, axis_position, plot_frequency};

/// A point in the plot's own space.
pub fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// The plot's own size: the right column's width, and the height the toolbar
/// and footer leave.
pub const PLOT_W: f64 = plot::W;
pub const PLOT_H: f64 = plot::H;

/// Where the zero line sits, and the band the grid starts below.
pub const ZERO: f64 = plot::ZERO;
pub const GRID_TOP: f64 = plot::GRID_TOP;

/// The frequency grid, in the plot's space, and what each line is worth.
pub const FREQ: [(f64, f64, &str); 8] = plot::FREQ;

/// The level grid. Only three lines carry a number.
pub const LEVEL_STEP: f64 = plot::LEVEL_STEP;

/// The display ranges the chip offers: grid lines per label, and the name.
pub const RANGES: [(u32, &str); 4] = [(1, "-3"), (2, "-6"), (3, "-9"), (6, "-18")];
pub const DEFAULT_RANGE: usize = 2;

/// How many pixels a decibel is worth on the plot, for the white curve and
/// the handles, and for the reduction.
pub const PIXELS_PER_DECIBEL: f64 = plot::PIXELS_PER_DECIBEL;
pub const REDUCTION_PIXELS_PER_DECIBEL: f64 = plot::REDUCTION_PIXELS_PER_DECIBEL;
/// A band's depth control, in decibels on the curve per unit.
pub const BAND_DB_PER_UNIT: f64 = plot::BAND_DB_PER_UNIT;

/// Where a frequency sits on the plot's axis: the reference's own,
/// `ln(f + 150)` evenly across the plot's width.
pub fn x_for_frequency(frequency: f64) -> f64 {
    PLOT_W * f64::from(axis_position(frequency as f32))
}

/// The frequency at a place on the axis.
pub fn frequency_for_x(x: f64) -> f64 {
    f64::from(axis_frequency((x / PLOT_W) as f32))
}

/// Where a depth sits on the plot's axis: a boost is above the zero line.
pub fn y_for_depth(depth: f64) -> f64 {
    ZERO - depth * BAND_DB_PER_UNIT * PIXELS_PER_DECIBEL
}

/// The depth at a place on the axis.
pub fn depth_for_y(y: f64) -> f64 {
    ((ZERO - y) / (BAND_DB_PER_UNIT * PIXELS_PER_DECIBEL)).clamp(-10.0, 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_axis_round_trips() {
        for frequency in [30.0, 100.0, 1000.0, 7500.0, 16000.0] {
            let back = frequency_for_x(x_for_frequency(frequency));
            assert!((back / frequency - 1.0).abs() < 1.0e-4, "{frequency} -> {back}");
        }
        assert!((depth_for_y(y_for_depth(-3.5)) + 3.5).abs() < 1.0e-9);
    }

    #[test]
    fn the_axis_is_where_the_reference_drew_its_grid() {
        // The grid lines were read off the reference; the formula the code
        // gives is the same to a pixel and a half.
        for (x, frequency, name) in FREQ {
            let ours = x_for_frequency(frequency);
            assert!((ours - x).abs() < 3.0, "{name}: {ours} against {x}");
        }
    }
}
