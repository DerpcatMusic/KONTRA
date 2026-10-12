//! Native default internal-LFO volume target, independent of the source waveform.
//! Admission and the retained 32-frame source clock belong to the caller.

#[derive(Clone, Copy, Debug)]
pub(crate) struct Target {
    value: f32,
    initialized: bool,
    alpha: Option<f32>,
}

impl Target {
    /// Prepare the native positive-lag coefficient outside the control-point loop.
    /// Negative signed lag selects a different native ramp and is not admitted.
    pub fn new(lag_ms: i16, rate: f32) -> Option<Self> {
        if lag_ms < 0 {
            return None;
        }
        let rate = if rate.is_finite() && rate >= 10. {
            rate
        } else {
            44_100.
        };
        let alpha = (lag_ms != 0).then(|| {
            let points = f64::from(lag_ms) * f64::from(rate) * 0.00003125;
            (1. - (-(2. * std::f64::consts::LN_10) / points).exp()) as f32
        });
        Some(Self {
            value: 0.,
            initialized: false,
            alpha,
        })
    }

    /// Evaluate once per native control point. `negative` is the proved target
    /// flag0x02, not the separately serialized inversion/shaper metadata.
    /// No upper/lower clamp here: the final combined volume buffer is clamped
    /// nonnegative by the native audio consumer after all targets have run.
    pub fn point(&mut self, source: f32, input: f32, intensity: f32, negative: bool) -> f32 {
        let source = if negative { -source } else { source };
        let source = (source + 1.) * 0.5;
        match self.alpha {
            Some(alpha) if self.initialized => {
                let delta = source - self.value;
                if delta.abs() >= 1e-5 {
                    self.value += delta * alpha;
                }
            }
            _ => self.value = source,
        }
        self.initialized = true;
        input * (1. - (1. - self.value) * intensity)
    }
}

#[cfg(test)]
mod tests {
    use super::Target;

    #[test]
    fn native_bipolar_volume_depth_and_overshoot() {
        let mut target = Target::new(0, 48_000.).unwrap();
        for (source, ordinary, negative) in [(-1., 0., 1.), (0., 0.5, 0.5), (1., 1., 0.)] {
            assert_eq!(target.point(source, 1., 1., false), ordinary);
            assert_eq!(target.point(source, 1., 1., true), negative);
        }
        assert_eq!(target.point(-0.5, 1., 0.5, false), 0.625);
        assert_eq!(target.point(-0.5, 0.3, 0., false), 0.3);
        assert_eq!(target.point(1.0852983, 1., 1., false), 1.0426491);
        // Zero lag has no tiny-delta threshold; it is the direct expression.
        assert_ne!(
            target.point(0., 1., 1., false),
            target.point(0.000002, 1., 1., false)
        );
        assert!(Target::new(-15, 48_000.).is_none());
    }

    #[test]
    fn native_positive_lag_initialization_settling_and_retention() {
        let mut target = Target::new(15, 48_000.).unwrap();
        assert_eq!(target.alpha.unwrap(), 0.18508725);
        assert_eq!(target.point(-1., 1., 1., false), 0.);
        assert_eq!(target.point(1., 1., 1., false), 0.18508726);
        for _ in 1..22 {
            target.point(1., 1., 1., false);
        }
        let expected = (1. - (-(2. * std::f64::consts::LN_10) * 22. / 22.5).exp()) as f32;
        assert!((target.value - expected).abs() < 1e-6);
        // A source bypass is a caller omission, preserving both lag state and
        // source clock. Resume does not initialize the target a second time.
        let before = target.value;
        let result = target.point(before * 2. - 1., 1., 1., false);
        assert_eq!(target.value, before);
        assert!((result - before).abs() < f32::EPSILON);
        assert_eq!(
            Target::new(15, 0.).unwrap().alpha,
            Target::new(15, 44_100.).unwrap().alpha
        );
    }
}
