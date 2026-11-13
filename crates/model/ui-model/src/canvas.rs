//! What a page's own painter drew in a widget ([`crate::Kind::Canvas`]):
//! paths in the widget's points, each stroked, filled or both.

use crate::Rgba;

/// One step of a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move([f64; 2]),
    Line([f64; 2]),
    /// Two control points, then the end.
    Cubic([f64; 6]),
    Close,
}

/// A path, filled first, then stroked `width` points wide.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paint {
    pub path: Vec<Seg>,
    pub fill: Option<Rgba>,
    pub stroke: Option<(Rgba, f64)>,
}
