//! A widget's value read as a number, the way every renderer reads it.

use crate::{Kind, Value, Widget};

impl Widget {
    /// Its value as a number: the source's, else its initial one.
    #[must_use]
    pub fn number(&self) -> f64 {
        match self.value {
            Some(Value::Integer(v)) => f64::from(v),
            Some(Value::Real(v)) => v,
            _ => self.initial_value,
        }
    }

    /// Cell `k` of a table's value.
    #[must_use]
    pub fn cell(&self, k: u32) -> Option<f64> {
        let k = k as usize;
        match &self.value {
            Some(Value::Integers(v)) => v.get(k).map(|&v| f64::from(v)),
            Some(Value::Reals(v)) => v.get(k).copied(),
            _ => None,
        }
    }

    /// Where its value lies in its range, 0..1; a switch is 0 or 1.
    #[must_use]
    pub fn position(&self) -> f64 {
        match &self.kind {
            Kind::Knob { range, .. }
            | Kind::Slider { range, .. }
            | Kind::ValueEdit { range, .. } => {
                let span = range.max - range.min;
                if span == 0. {
                    0.
                } else {
                    ((self.number() - range.min) / span).clamp(0., 1.)
                }
            }
            _ => f64::from(u8::from(self.number() != 0.)),
        }
    }
}
