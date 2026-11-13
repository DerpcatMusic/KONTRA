//! Host macros the audio thread moves at their frame: the script control a
//! macro drives, and how the host's 0..1 maps onto the control's value.

/// Host macros the audio side keeps a target for.
pub const MACROS: usize = 8;
/// Menu item values a target keeps; a bigger menu stays on the tick.
pub const STEPS: usize = 16;

/// The script control a host macro moves, and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MacroTarget {
    pub part: usize,
    pub slot: u8,
    pub id: i32,
    pub scale: Scale,
}

/// How a macro's 0..1 becomes the control's script value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    /// An integer control over min..max, rounded.
    Int { min: f64, max: f64 },
    /// A real control over min..max, as the script's real bits.
    Real { min: f64, max: f64 },
    /// A menu: the nearest of its item values.
    Steps { values: [i32; STEPS], len: u8 },
}

impl MacroTarget {
    /// The control's script value at host value `normalized`.
    #[must_use]
    pub fn value(&self, normalized: f64) -> i64 {
        let n = normalized.clamp(0.0, 1.0);
        match self.scale {
            Scale::Int { min, max } => (min + n * (max - min)).round() as i64,
            Scale::Real { min, max } => (min + n * (max - min)).to_bits() as i64,
            Scale::Steps { values, len } => {
                let values = values.get(..usize::from(len)).unwrap_or_default();
                let (lo, hi) = (values.iter())
                    .fold((i32::MAX, i32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
                let v = f64::from(lo) + n * (f64::from(hi) - f64::from(lo));
                let nearest = |a: &&i32, b: &&i32| {
                    (f64::from(**a) - v)
                        .abs()
                        .total_cmp(&(f64::from(**b) - v).abs())
                };
                values.iter().min_by(nearest).map_or(0, |&v| i64::from(v))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_menu_target_takes_its_nearest_item() {
        let mut values = [0; STEPS];
        values[..3].copy_from_slice(&[0, 1, 10]);
        let t = MacroTarget {
            part: 0,
            slot: 0,
            id: 1,
            scale: Scale::Steps { values, len: 3 },
        };
        assert_eq!(t.value(0.04), 0);
        assert_eq!(t.value(0.4), 1);
        assert_eq!(t.value(0.6), 10);
    }
}
