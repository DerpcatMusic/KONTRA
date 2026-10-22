//! The linear core of the filter family: zero-delay (trapezoidal, TPT) state-variable stages.
//!
//! A stage is `(g, k)`-parameterised: `g = tan(pi f / rate)` is the live prewarped cutoff and
//! `k` the damping. [`Zdf`] keeps the part derived from `k` (and `g` at derivation), so a core
//! that holds a section between control updates (SV4) can run it with the live `g`, exactly as
//! the native loop does. Its state `[h, b, l]` passes a [`Knee`] on the band and low states; the
//! knee is the caller's choice, so a family can place a different transfer function there.
//!
//! Kontakt's SV2, SV4, SV6 and the SV Multi pair are configurations of this stage; the generic
//! `svf` section cascade and the Chamberlin sections are other cores of the same recurrence.

use super::shaper::{Knee, Soft};

/// A zero-delay one-pole cell (the ladder's stage): `g` the prewarped cutoff, `r = 1 / (1 + g)`.
/// One sample updates its state `s` and returns the stage output, in the native loop's order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Tpt1 {
    pub g: f32,
    pub r: f32,
}

impl Tpt1 {
    #[inline(always)]
    pub(crate) fn tick(self, s: &mut f32, x: f32) -> f32 {
        let previous = *s;
        let predicted = (2.0 * previous + self.g * x) * self.r;
        *s = previous + self.g * (x - predicted);
        previous + *s
    }
}

/// The `k`-dependent coefficients of one zero-delay stage.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Zdf {
    /// `k + 2g` (the `a + g` of the native order).
    a2: f32,
    /// `g (k + g)`.
    gk: f32,
    /// `gk + 1`.
    den: f32,
    /// `1 / (gk + 1)`.
    d: f32,
}

impl Zdf {
    /// Coefficients for prewarped `g` and damping `k`.
    #[inline(always)]
    pub(crate) fn new(g: f32, k: f32) -> Self {
        let a = k + g;
        let gk = g * a;
        let den = gk + 1.0;
        Self { a2: g + a, gk, den, d: 1.0 / den }
    }

    /// One sample, with the reciprocal `1 / (gk + 1)` (the SV4 and SV6 order): updates
    /// `s = [h, b, l]` (band and low through `knee`) and returns the raw `(hp, bp, lp)`.
    #[inline(always)]
    pub(crate) fn tick(self, g: f32, s: &mut [f32], x: f32, knee: Knee) -> [f32; 3] {
        self.step::<false>(g, s, x, knee)
    }

    /// One sample, dividing by `gk + 1` (the SV2 and SV Multi order).
    #[inline(always)]
    pub(crate) fn tick_divided(self, g: f32, s: &mut [f32], x: f32, knee: Knee) -> [f32; 3] {
        self.step::<true>(g, s, x, knee)
    }

    #[inline(always)]
    fn step<const DIVIDE: bool>(self, g: f32, s: &mut [f32], x: f32, knee: Knee) -> [f32; 3] {
        let [h, b, lo] = [s[0], s[1], s[2]];
        let num = x - (((b * self.a2) + lo) + self.gk * h);
        let hp = if DIVIDE { num / self.den } else { num * self.d };
        let bp = (h + hp) * g + b;
        let lp = (bp + b) * g + lo;
        s[0] = hp;
        s[1] = knee.apply(bp);
        s[2] = knee.apply(lp);
        [hp, bp, lp]
    }
}

/// The explicit (Chamberlin) stage: `hp = soft(b) + (y k - (b d + l))`, `b += hp g`,
/// `l += b g + eps`; the output blends the input toward `mode = wl l + wb b + wh hp` by `wet`.
/// The band state passes `shaper` (a transfer stage) in the loop, as the Versatile and Pro-53
/// feedback do.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Explicit {
    pub g: f32,
    pub d: f32,
    /// Input scale.
    pub k: f32,
    pub eps: f32,
    /// Shaper on the band state.
    pub shaper: Soft,
    /// Low, band and high pass weights.
    pub weights: [f32; 3],
    pub wet: f32,
}

impl Explicit {
    /// One frame on `s = [b, l]`: returns the blended output and the new low state.
    #[inline(always)]
    pub(crate) fn tick(&self, s: &mut [f32], y: f32) -> (f32, f32) {
        let (b, l) = (s[0], s[1]);
        let hp = self.shaper.apply(b) + (y * self.k - (b * self.d + l));
        let b = hp * self.g + b;
        let l = (b * self.g + self.eps) + l;
        s[0] = b;
        s[1] = l;
        let mode = (b * self.weights[1] + hp * self.weights[2]) + l * self.weights[0];
        (y + (mode - y) * self.wet, l)
    }
}
