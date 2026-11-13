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
    pub control: Control,
    pub scale: Scale,
}

/// What a macro moves in its part's script.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    /// A KSP control by UI id, its value the script's integer.
    Integer(i32),
    /// A KSP real control by UI id, its value the script's real bits.
    Real(i32),
    /// A Lua widget by number, its value as is.
    Widget(u32),
}

/// How a macro's 0..1 spans the control's values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    /// Continuous over min..max.
    Range { min: f64, max: f64 },
    /// A menu: the nearest of its item values.
    Steps { values: [i32; STEPS], len: u8 },
}

impl MacroTarget {
    /// The control's value at host value `normalized`.
    #[must_use]
    pub fn plain(&self, normalized: f64) -> f64 {
        let n = normalized.clamp(0.0, 1.0);
        match self.scale {
            Scale::Range { min, max } => min + n * (max - min),
            Scale::Steps { values, len } => {
                let values = values.get(..usize::from(len)).unwrap_or_default();
                let (lo, hi) = (values.iter())
                    .fold((i32::MAX, i32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
                let v = f64::from(lo) + n * (f64::from(hi) - f64::from(lo));
                let distance = |x: i32| (f64::from(x) - v).abs();
                let nearest = |a: &&i32, b: &&i32| distance(**a).total_cmp(&distance(**b));
                values.iter().min_by(nearest).map_or(0.0, |&v| f64::from(v))
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
            control: Control::Integer(1),
            scale: Scale::Steps { values, len: 3 },
        };
        assert_eq!(t.plain(0.04), 0.0);
        assert_eq!(t.plain(0.4), 1.0);
        assert_eq!(t.plain(0.6), 10.0);
    }
}
