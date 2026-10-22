//! Native saved fade law, copied from pinned v1 0cb7a8a0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KontaktLfoFade {
    remaining: u32,
    value: f32,
    factor: f32,
    /// Clamp of the running value: legacy `0..=1`, alternate `0.3..=1.3`.
    floor: f32,
    ceiling: f32,
}

impl KontaktLfoFade {
    /// Saved unsynchronized false-mode Digital Multi fade; native control rate is audio / 32.
    pub fn from_saved(ms: f32, rate: f32) -> Result<Self, crate::Error> {
        Self::from_saved_mode(ms, rate, false)
    }

    /// As [`Self::from_saved`], with the native v73 alternate switch: the running value
    /// clamps to `0.3..=1.3` instead of `0..=1`, so the gain reaches 1 rather than 0.7.
    pub fn from_saved_alternate(ms: f32, rate: f32) -> Result<Self, crate::Error> {
        Self::from_saved_mode(ms, rate, true)
    }

    fn from_saved_mode(ms: f32, rate: f32, alternate: bool) -> Result<Self, crate::Error> {
        if !(0. ..=5000.).contains(&ms) || !(10. ..=384_000.).contains(&rate) {
            return Err(crate::Error::InvalidInput);
        }
        Ok(Self::with_mode(ms, rate, alternate))
    }

    pub(crate) fn gains(mut self) -> Box<[f32]> {
        (0..=self.remaining).map(|_| self.next()).collect()
    }

    pub(crate) fn new(ms: f32, rate: f32) -> Self {
        Self::with_mode(ms, rate, false)
    }

    pub(crate) fn with_mode(ms: f32, rate: f32, alternate: bool) -> Self {
        // Native time getter returns milliseconds; DSP runs at rate / 32.
        let remaining = (ms * (rate / 32.) * 0.001) as u32;
        let factor = if remaining == 0 {
            1.
        } else {
            (1. + 1. / f64::from(0.3f32)).powf(1. / f64::from(remaining)) as f32
        };
        let (floor, ceiling) = if alternate { (0.3, 1.3) } else { (0., 1.) };
        Self {
            remaining,
            value: 0.3,
            factor,
            floor,
            ceiling,
        }
    }

    pub fn next(&mut self) -> f32 {
        if self.remaining == 0 {
            return 1.;
        }
        let gain = self.value - 0.3;
        // Native post stage (0x140b06c90): the running value clamps to the mode's
        // range, and the gain is `value - 0.3`. Only N points are scaled.
        self.value = (self.value * self.factor).clamp(self.floor, self.ceiling);
        self.remaining -= 1;
        gain
    }
}

#[cfg(test)]
mod tests {
    use super::KontaktLfoFade;

    /// 100 ms at 44.1 kHz is 137 scaled points; the factor carries 0.3 to 1.3 in exactly that span.
    /// After the scaled points the native stage passes samples through (gain 1), so legacy steps
    /// from 0.7 to 1 at the end while the alternate ramp meets 1 continuously.
    #[test]
    fn alternate_mode_reaches_full_gain_where_legacy_stops_at_0_7() {
        let (l, a) = (
            KontaktLfoFade::new(100., 44_100.).gains(),
            KontaktLfoFade::with_mode(100., 44_100., true).gains(),
        );
        let (lf, af) = (&l[..137], &a[..137]);
        assert_eq!(lf[0], 0.);
        assert!(
            lf.iter().all(|g| *g <= 0.7 + 1e-6),
            "legacy ceiling is 1, gain 0.7"
        );
        assert!((lf[136] - 0.7).abs() < 1e-6);
        assert_eq!(l[137], 1., "legacy steps to unity after the scaled points");
        // The ramp ends at 1.3 after the 137th update, one step past the last scaled point.
        assert!((af[136] - (1.3 / f32::powf(4.333_333, 1. / 137.) - 0.3)).abs() < 1e-5);
        assert!(
            (af[136] - 1.).abs() < 0.02,
            "alternate meets gain 1 within one tick"
        );
        assert!(af.iter().all(|g| (0. ..=1. + 1e-6).contains(g)));
        assert!(
            af.windows(2).all(|w| w[1] >= w[0]),
            "alternate ramp is monotone"
        );
    }

    #[test]
    fn zero_length_fade_is_unity_in_both_modes() {
        assert_eq!(KontaktLfoFade::new(0., 44_100.).next(), 1.);
        assert_eq!(KontaktLfoFade::with_mode(0., 44_100., true).next(), 1.);
    }
}
