//! Linear filter sections around a nonlinear stage, as a translator
//! supplies them: each an analog prototype the engine maps to its own rate
//! (bilinear, prewarped at the section's corner), and the stage it runs at.

/// `H(s') = (num₀ + num₁·s' + num₂·s'²) / (den₀ + den₁·s' + den₂·s'²)` with
/// `s' = s / (2π·hz)`, made digital by the bilinear transform prewarped so
/// that `s' = j` lands on `hz`. `num = [1, 0, 0]`, `den = [1, 1.414, 1]` is a
/// Butterworth low-pass at `hz`; `den₂ = 0` makes a first-order section.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    pub hz: f32,
    pub num: [f32; 3],
    pub den: [f32; 3],
}

/// Where a [`Section`] runs, in signal order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// At the base rate, before the oversampled stage.
    In,
    /// Oversampled, before the curve.
    Pre,
    /// Oversampled, after the curve.
    Post,
    /// At the base rate, after the oversampled stage.
    Out,
}

/// A stage's sections: a fixed list and up to three that move with a
/// control. Sections at one stage commute, so their order there is free.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sections {
    pub fixed: &'static [(Stage, Section)],
    pub moving: [Option<(Stage, Section)>; 3],
}

impl Sections {
    /// None.
    pub const NONE: Self = Self {
        fixed: &[],
        moving: [None; 3],
    };

    /// Every section, fixed first.
    pub fn iter(&self) -> impl Iterator<Item = (Stage, Section)> + '_ {
        self.fixed
            .iter()
            .copied()
            .chain(self.moving.iter().flatten().copied())
    }
}
