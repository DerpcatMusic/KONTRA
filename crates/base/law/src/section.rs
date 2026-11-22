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

impl Section {
    /// Unity pass-through section.
    pub const IDENTITY: Self = Self {
        hz: 1000.0,
        num: [1.0, 0.0, 0.0],
        den: [1.0, 0.0, 0.0],
    };
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
///
/// The moving slots store stages and sections in separate arrays so the
/// struct avoids 9 bytes of inter-element alignment padding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sections {
    pub fixed: &'static [(Stage, Section)],
    moving_stages: [Option<Stage>; 3],
    moving_sections: [Section; 3],
}

impl Sections {
    /// None.
    pub const NONE: Self = Self {
        fixed: &[],
        moving_stages: [None; 3],
        moving_sections: [Section::IDENTITY; 3],
    };

    /// Builds a section set from a static fixed list and up to three moving
    /// sections.
    #[must_use]
    pub const fn new(
        fixed: &'static [(Stage, Section)],
        moving: [Option<(Stage, Section)>; 3],
    ) -> Self {
        const fn split(slot: Option<(Stage, Section)>) -> (Option<Stage>, Section) {
            match slot {
                Some((stage, section)) => (Some(stage), section),
                None => (None, Section::IDENTITY),
            }
        }
        let [m0, m1, m2] = moving;
        let (s0, c0) = split(m0);
        let (s1, c1) = split(m1);
        let (s2, c2) = split(m2);
        Self {
            fixed,
            moving_stages: [s0, s1, s2],
            moving_sections: [c0, c1, c2],
        }
    }

    /// Every section, fixed first.
    pub fn iter(&self) -> impl Iterator<Item = (Stage, Section)> + '_ {
        self.fixed.iter().copied().chain(
            self.moving_stages
                .iter()
                .zip(&self.moving_sections)
                .filter_map(|(&stage, &section)| Some((stage?, section))),
        )
    }
}
