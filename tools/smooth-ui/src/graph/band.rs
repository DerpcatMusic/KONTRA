//! The bands: what the editor holds for each, and the handle it draws.

use crate::theme;

#[derive(Clone, Copy)]
pub struct Handle {
    /// Which band it stands for.
    pub index: usize,
    /// Whether it is the band the panel is showing.
    pub selected: bool,
    /// Centre, in the plot's space.
    pub x: f64,
    pub y: f64,
    /// Which band it belongs to: the only thing that tells them apart.
    pub tint: u32,
    /// The low bands are tall pills; the narrow ones are dots.
    pub pill: bool,
}

/// One band, as the editor holds it: where it sits, how deep it cuts, and
/// the shape that says what it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    /// The centre, in hertz.
    pub frequency: f64,
    /// The depth, in decibels. Negative cuts.
    pub depth: f64,
    /// The bandwidth, as the host's own Q.
    pub q: f64,
    /// Which of the engine's eight shapes: low cut, low shelf, bell,
    /// bandpass, band reject, tilt, high shelf, high cut.
    pub shape: usize,
    /// The slope of a cut or shelf, in decibels per octave.
    pub slope: f64,
    /// Where the band sits between the stereo channels, 0..1.
    pub balance: f64,
    /// Whether the band is in the plot at all.
    pub active: bool,
    /// Whether it is switched off while it stays in the plot.
    pub bypass: bool,
}

impl Band {
    pub const fn at(frequency: f64, depth: f64, shape: usize, active: bool) -> Self {
        Self {
            frequency,
            depth,
            q: 0.707,
            shape,
            slope: 6.0,
            balance: 0.5,
            active,
            bypass: false,
        }
    }

    /// The low cut, the low shelf, the high shelf and the high cut stand as
    /// tall pills; the bands with a width are dots.
    pub fn is_edge(&self) -> bool {
        matches!(self.shape, 0 | 1 | 6 | 7)
    }
}

/// How many bands the editor draws.
pub const BANDS: usize = 8;

/// The names of the engine's eight shapes.
pub const SHAPES: [&str; 8] = [
    "low cut",
    "low shelf",
    "bell",
    "bandpass",
    "band reject",
    "tilt",
    "high shelf",
    "high cut",
];

/// The bands at rest: the reference's four, then four that are not in the plot.
pub const AT_REST_BANDS: [Band; BANDS] = [
    Band::at(168.0, 0.0, 0, true),
    Band::at(650.0, -2.3, 2, true),
    Band::at(3210.0, 3.2, 2, true),
    Band::at(9410.0, 0.0, 7, true),
    Band::at(1000.0, 0.0, 2, false),
    Band::at(1000.0, 0.0, 2, false),
    Band::at(1000.0, 0.0, 2, false),
    Band::at(1000.0, 0.0, 2, false),
];

/// The colours the bands wear, from the active theme's own four.
pub fn band_tints() -> [u32; 8] {
    let t = theme::t();
    [t.red, t.magenta, t.cyan, t.green, t.red, t.magenta, t.cyan, t.green]
}
