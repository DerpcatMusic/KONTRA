//! A group whose zones are wavetables: single cycles one after another,
//! played at the note's frequency, morphed by position and read through
//! two phase warps.

/// Where a group's voices read their table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wavetable {
    /// 0 the first cycle ..= 1 the last; between two, a linear morph.
    pub position: f32,
    /// Where in the cycle a note starts, 0..1.
    pub phase: f32,
    /// Applied in order.
    pub warps: [Warp; 2],
}

/// One bend of the read phase.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Warp {
    pub shape: WarpShape,
    /// 0..1.
    pub amount: f32,
}

impl Warp {
    pub const OFF: Self = Self {
        shape: WarpShape::Off,
        amount: 0.0,
    };
}

/// Which way a warp leans as its amount rises: down, up, or down below half
/// and up above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Minus,
    Plus,
    Both,
}

/// How a warp bends the read phase `p` (0..1) by its amount `a` (0..1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WarpShape {
    Off,
    /// `(15a³ + 1)·p`, wrapped.
    Sync,
    /// Towards `(1 − cos πp)/2`.
    Cosine,
    /// Towards `asin(2p − 1)/π + ½`.
    Arcsine,
    /// Towards the cosine below `a = ½`, the arcsine above, by `|2a − 1|`.
    CosineArcsine,
    /// Reads the cycle in `1 − 0.9375a` of the phase, then silence.
    Pulse,
    /// Moves the cycle's midpoint by up to `reach` of a half.
    Skew {
        reach: f32,
        side: Side,
    },
    /// Reverses the phase before (or, past `a = ½`, after) a point.
    Flip,
    /// Folds at the midpoint, then skews.
    Mirror,
    /// `⌊2^((1 − a)·15 + 1)⌋` steps.
    Quantize,
    /// Rises over `1 − a`, falls over `a`.
    Seesaw,
    /// `p^(9a + 1)`.
    Exp,
    /// `1 − (1 − p)^(9a + 1)`.
    Log,
    /// Log below `a = ½`, exp above, both up to the power 10.
    LogExp,
    /// A rational bend about the midpoint, by up to 0.9.
    Bend {
        side: Side,
    },
}
