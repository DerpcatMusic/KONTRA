//! A group whose zones are wavetables: single cycles one after another,
//! played at the note's frequency, morphed by position and read through
//! two forms, each a phase warp and a shaper of the level read.

/// Where a group's voices read their table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wavetable {
    /// 0 the first cycle ..= 1 the last; between two, a linear morph.
    pub position: f32,
    /// Where in the cycle a note starts, 0..1.
    pub phase: f32,
    /// How far past `phase` a note's start may fall, 0..=1: each note adds
    /// its random draw times this (1 starts anywhere, `phase` then moot).
    pub phase_random: f32,
    /// The level the table plays at, linear: 1 plays the cycles as their
    /// samples are.
    pub gain: f32,
    pub inharmonic: Inharmonic,
    pub modosc: ModOsc,
    /// Applied in order.
    pub warps: [Warp; 2],
}

/// A second oscillator, a sine, that moves the table's read or joins its
/// output, per voice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModOsc {
    pub kind: ModOscKind,
    /// 0..=1; routes and scripts move it.
    pub amount: f32,
    /// 0..=1: its frequency over the note's, geometric from `tune_range.0`
    /// to `tune_range.1`.
    pub tune: f32,
    pub tune_range: (f32, f32),
    /// The FM kinds' frequency swing at amount 1, Hz.
    pub fm_hz: f32,
}

impl ModOsc {
    pub const OFF: Self = Self {
        kind: ModOscKind::Off,
        amount: 0.0,
        tune: 0.5,
        tune_range: (1.0, 1.0),
        fm_hz: 0.0,
    };
}

/// What the mod oscillator `m` does to the table's read at amount `a`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModOscKind {
    Off,
    /// The table's frequency moves by `fm_hz·a·m`.
    Fm,
    /// The oscillator sounds; the table's output `y` moves its frequency by
    /// `2·fm_hz·a·y`.
    FmBy,
    /// The table's frequency moves by its own last output, `0.62·fm_hz·a·y`.
    FmFeedback,
    /// The table's phase moves by `a·m` cycles.
    Pm,
    /// The table's phase moves by its own last output, `0.5·a·y` cycles.
    PmFeedback,
    /// The output times `1 − a + 0.24·a·m`.
    Ring,
    /// The output plus `2a` of the oscillator at the table's level.
    Mix,
    /// The output crossfaded to the oscillator by `a`.
    Blend,
}

/// How far apart the partials above the first lie, in fundamentals: partial
/// `n` plays at `f·(1 + (n − 1)·s)`, each at its level, `s` geometric from
/// `range.0` at amount 0 to `range.1` at 1; harmonic (`s` = 1) when off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inharmonic {
    pub on: bool,
    /// 0..=1; routes and scripts move it.
    pub amount: f32,
    pub range: (f32, f32),
}

impl Inharmonic {
    pub const OFF: Self = Self {
        on: false,
        amount: 0.5,
        range: (1.0, 1.0),
    };
}

/// A value of a [`Wavetable`] that routes and scripts move, each 0..=1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableValue {
    Position,
    /// The amount of warp n (0 or 1).
    Warp(u8),
    /// [`Inharmonic::amount`].
    Inharmonic,
    /// [`ModOsc::amount`] and [`ModOsc::tune`].
    ModAmount,
    ModTune,
}

/// One form: a bend of the read phase, then a shaper of what it read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Warp {
    pub shape: WarpShape,
    pub shaper: FormShaper,
    /// 0..1.
    pub amount: f32,
}

impl Warp {
    pub const OFF: Self = Self {
        shape: WarpShape::Off,
        shaper: FormShaper::Off,
        amount: 0.0,
    };
}

/// How a form shapes the level `s` it read, at the unwarped phase `p` (0..1)
/// and its amount `a`; applied in order after both phase warps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FormShaper {
    Off,
    /// Fades in over the first quarter cycle and out over the last, through
    /// the window `w` of [`FormShaper::Window`].
    Edges,
    /// `w(p)`: `32x³` up to a quarter, `1 − 32(½ − x)³` to the middle, with
    /// `x` folded about it.
    Window,
    /// `(8a² + 1)·s`, clipped to ±1.
    Saturate,
    /// Folds `(3.75a² + 0.25)·s` back into ±1 as a triangle.
    Fold,
    /// Scales every other one of `2n` slices of the cycle by `1 − a`.
    Blinds(u8),
    /// `sin(πp)`.
    Arch,
    /// Rises to the middle and falls back, linearly.
    Peak,
    /// `1 − p`.
    Fall,
    /// Moves `s` by `a` (down in the first half), wrapped into ±1.
    Wrap,
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
