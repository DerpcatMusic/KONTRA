//! Transfer-function stages: the memoryless nonlinearities of the filter family.
//!
//! They are composed by the cores around the linear sections in [`super::linear`]: a core
//! places one where its native loop saturates (a state, a feedback path, an input), so the same
//! curve serves every family that places it there. Nothing here owns state.

/// How a knee's slope is applied: the native families round `c (1 - |c| w)` differently.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Slope {
    /// `w` precomputed: `(1 - |c| w) c`.
    Fused(f32),
    /// `(|c| 0.25) inv` with `inv = 1 / L`: the SV2 and SV Multi order.
    Split { inv: f32 },
    /// The clamp plus a fraction of the excess: `c + s (v - c)` (the legacy BFilter knee).
    Excess(f32),
}

/// Clamp to `+-limit`, then bend the top. Kontakt's SV state saturator (`limit = 2L`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Knee {
    pub limit: f32,
    pub slope: Slope,
}

impl Knee {
    /// The SV4 and SV6 state knee for a state limit `l`.
    #[inline(always)]
    pub(crate) fn state(l: f32) -> Self {
        Self { limit: l + l, slope: Slope::Fused(0.25 / l) }
    }

    /// The SV2 and SV Multi state knee for a state limit `l`.
    #[inline(always)]
    pub(crate) fn state_split(l: f32) -> Self {
        Self { limit: l + l, slope: Slope::Split { inv: 1.0 / l } }
    }

    /// The legacy BFilter knee: clamp to `+-1`, then pass `slope` of the excess.
    #[inline(always)]
    pub(crate) fn excess(slope: f32) -> Self {
        Self { limit: 1.0, slope: Slope::Excess(slope) }
    }

    /// The excess slope of an [`Slope::Excess`] knee (0 for the other shapes).
    pub(crate) fn excess_slope(self) -> f32 {
        match self.slope {
            Slope::Excess(slope) => slope,
            _ => 0.0,
        }
    }

    #[inline(always)]
    pub(crate) fn apply(self, v: f32) -> f32 {
        let c = v.clamp(-self.limit, self.limit);
        match self.slope {
            Slope::Fused(w) => (1.0 - c.abs() * w) * c,
            Slope::Split { inv } => (1.0 - (c.abs() * 0.25) * inv) * c,
            Slope::Excess(slope) => (v - c) * slope + c,
        }
    }
}

/// `v (v^2 + 27) / (9 v^2 + 27)`: the Padé tanh-like curve of the ladder and Monark inputs.
#[inline(always)]
pub(crate) fn pade(v: f32) -> f32 {
    let v2 = v * v;
    ((v2 + 27.0) * v) / ((v2 * 9.0) + 27.0)
}
