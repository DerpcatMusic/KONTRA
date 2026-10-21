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

    #[inline(always)]
    pub(crate) fn apply(self, v: f32) -> f32 {
        let c = v.clamp(-self.limit, self.limit);
        let w = match self.slope {
            Slope::Fused(w) => c.abs() * w,
            Slope::Split { inv } => (c.abs() * 0.25) * inv,
        };
        (1.0 - w) * c
    }
}

/// `v (v^2 + 27) / (9 v^2 + 27)`: the Padé tanh-like curve of the ladder and Monark inputs.
#[inline(always)]
pub(crate) fn pade(v: f32) -> f32 {
    let v2 = v * v;
    ((v2 + 27.0) * v) / ((v2 * 9.0) + 27.0)
}

/// `(a - c |s|) s` with `s` the input clamped to +-4: Pro-53's feedback `NL` and the Versatile
/// band shaper. A transfer stage: it owns no state and is placed where a core saturates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Soft {
    pub a: f32,
    pub c: f32,
}

impl Soft {
    #[inline(always)]
    pub(crate) fn apply(self, v: f32) -> f32 {
        let s = v.clamp(-4.0, 4.0);
        (self.a - s.abs() * self.c) * s
    }
}

/// `v / sqrt(v^2 + 1)`: the bounded soft clip of the Dual SKF resonance loop (a transfer stage).
#[inline(always)]
pub(crate) fn unit_sqrt(v: f32) -> f32 {
    let n = 1.0 / ((v * v) + 1.0).sqrt();
    n * v
}
