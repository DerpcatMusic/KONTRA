//! Native saved fade law, copied from pinned v1 0cb7a8a0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KontaktLfoFade {
    remaining: u32,
    value: f32,
    factor: f32,
}

impl KontaktLfoFade {
    /// Saved unsynchronized false-mode Digital Multi fade; native control rate is audio / 32.
    pub fn from_saved(ms: f32, rate: f32) -> Result<Self, crate::Error> {
        if !(0. ..=5000.).contains(&ms) || !(10. ..=384_000.).contains(&rate) {
            return Err(crate::Error::InvalidInput);
        }
        Ok(Self::new(ms, rate))
    }

    pub(crate) fn gains(mut self) -> Box<[f32]> {
        (0..=self.remaining).map(|_| self.next()).collect()
    }

    pub(crate) fn new(ms: f32, rate: f32) -> Self {
        // Native time getter returns milliseconds; DSP runs at rate / 32.
        let remaining = (ms * (rate / 32.) * 0.001) as u32;
        let factor = if remaining == 0 {
            1.
        } else {
            (1. + 1. / f64::from(0.3f32)).powf(1. / f64::from(remaining)) as f32
        };
        Self {
            remaining,
            value: 0.3,
            factor,
        }
    }

    pub fn next(&mut self) -> f32 {
        if self.remaining == 0 {
            return 1.;
        }
        let gain = self.value - 0.3;
        // v71/v72 set the native legacy switch false: its ceiling is 1,
        // unlike v73's optional .3..1.3 mode. Only N points are scaled.
        self.value = (self.value * self.factor).clamp(0., 1.);
        self.remaining -= 1;
        gain
    }
}
