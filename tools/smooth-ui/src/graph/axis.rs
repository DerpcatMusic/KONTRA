//! The plot's axes: where a frequency and a depth sit on the plot.
//!
//! The frequency grid and the zero line are soothe3's own, held in
//! `tokens::plot`; the mappings here read them rather than assume a law.

use crate::tokens::plot;
use mui::prelude::*;
pub use smooth_core::plot::{PLOT_BINS, SWEEP_HIGH, SWEEP_LOW, plot_frequency};

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
pub const LEVEL: [f64; 6] = plot::LEVEL;

/// How many pixels a decibel is worth on the plot.
pub const PIXELS_PER_DECIBEL: f64 = plot::PIXELS_PER_DECIBEL;

/// Where a frequency sits on the plot's axis.
///
/// The axis is the reference's own: the frequency grid is not evenly spaced
/// in decibels, so the mapping is read off the grid rather than assumed.
pub fn x_for_frequency(frequency: f64) -> f64 {
    let frequency = frequency.clamp(f64::from(SWEEP_LOW), f64::from(SWEEP_HIGH));
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


/// Where a depth sits on the plot's axis. A cut is below the zero line.
pub fn y_for_depth(depth: f64) -> f64 {
    ZERO + depth * PIXELS_PER_DECIBEL
}

/// The depth at a place on the axis.
pub fn depth_for_y(y: f64) -> f64 {
    ((y - ZERO) / PIXELS_PER_DECIBEL).clamp(-10.0, 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_axis_round_trips() {
        for frequency in [30.0, 100.0, 1000.0, 7500.0, 16000.0] {
            let back = frequency_for_x(x_for_frequency(frequency));
            assert!((back / frequency - 1.0).abs() < 1.0e-9, "{frequency} -> {back}");
        }
        assert!((depth_for_y(y_for_depth(-3.5)) + 3.5).abs() < 1.0e-9);
    }
}
