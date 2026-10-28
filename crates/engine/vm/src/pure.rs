//! Actions that need nothing from the host: arithmetic and array helpers.

use crate::{Action, HostValue};

/// Evaluates a pure action, or `None` when the host must. Reals travel as
/// their bits; actions over numbers end with a flag that is 1 when those
/// numbers are real. Arrays are `base, len` of `cells`.
pub(crate) fn eval(action: Action, ints: &[i64], cells: &mut [i64]) -> Option<HostValue> {
    let int = |i: usize| ints.get(i).map_or(0, |&v| v as i32);
    let real = |i: usize| ints.get(i).map_or(0.0, |&v| f64::from_bits(v as u64));
    let is_real = ints.last().is_some_and(|&flag| flag == 1);
    let unary = |f: fn(f64) -> f64| HostValue::Real(f(real(0)));
    let flag = |b: bool| HostValue::Int(i32::from(b));
    Some(match action {
        Action::Abs if is_real => HostValue::Real(real(0).abs()),
        Action::Abs => HostValue::Int(int(0).wrapping_abs()),
        Action::Min if is_real => HostValue::Real(real(0).min(real(1))),
        Action::Min => HostValue::Int(int(0).min(int(1))),
        Action::Max if is_real => HostValue::Real(real(0).max(real(1))),
        Action::Max => HostValue::Int(int(0).max(int(1))),
        Action::InRange if is_real => flag(real(1) <= real(0) && real(0) <= real(2)),
        Action::InRange => flag(int(1) <= int(0) && int(0) <= int(2)),
        Action::Sgn if is_real => HostValue::Int(match real(0) {
            r if r > 0.0 => 1,
            r if r < 0.0 => -1,
            _ => 0,
        }),
        Action::Sgn => HostValue::Int(int(0).signum()),
        Action::Signbit if is_real => flag(real(0).is_sign_negative()),
        Action::Signbit => flag(int(0) < 0),
        Action::Round => unary(f64::round),
        Action::Floor => unary(f64::floor),
        Action::Ceil => unary(f64::ceil),
        Action::Sqrt => unary(f64::sqrt),
        Action::Cbrt => unary(f64::cbrt),
        Action::Exp => unary(f64::exp),
        Action::Exp2 => unary(f64::exp2),
        Action::Log => unary(f64::ln),
        Action::Log2 => unary(f64::log2),
        Action::Log10 => unary(f64::log10),
        Action::Sin => unary(f64::sin),
        Action::Cos => unary(f64::cos),
        Action::Tan => unary(f64::tan),
        Action::Asin => unary(f64::asin),
        Action::Acos => unary(f64::acos),
        Action::Atan => unary(f64::atan),
        Action::Pow => HostValue::Real(real(0).powf(real(1))),
        // 14-bit values split into MIDI bytes.
        Action::Msb => HostValue::Int((int(0) >> 7) & 127),
        Action::Lsb => HostValue::Int(int(0) & 127),
        Action::Search => {
            let (base, len, lo, hi) = (int(0), int(1), int(3), int(4));
            let needle = ints.get(2).copied().unwrap_or(0);
            let found = range(base, len, lo, hi).find(|&at| cells.get(at) == Some(&needle));
            HostValue::Int(found.map_or(-1, |at| at as i32 - base))
        }
        Action::Sort => {
            let (base, len, descending, lo, hi) = (int(0), int(1), int(2) != 0, int(3), int(4));
            if let Some(items) = cells.get_mut(range(base, len, lo, hi)) {
                if is_real {
                    items.sort_unstable_by(|a, b| {
                        f64::from_bits(*a as u64).total_cmp(&f64::from_bits(*b as u64))
                    });
                } else {
                    items.sort_unstable_by_key(|&v| v as i32);
                }
                if descending {
                    items.reverse();
                }
            }
            HostValue::Nothing
        }
        Action::ArrayEqual => {
            let (a, b) = ((int(0), int(1)), (int(2), int(3)));
            let slice = |(base, len): (i32, i32)| {
                let base = usize::try_from(base).ok()?;
                cells.get(base..base + usize::try_from(len).ok()?)
            };
            flag(a.1 == b.1 && slice(a).is_some() && slice(a) == slice(b))
        }
        _ => return None,
    })
}

/// Cells `base + lo ..= base + hi`, clamped to the array.
fn range(base: i32, len: i32, lo: i32, hi: i32) -> std::ops::Range<usize> {
    let (lo, hi) = (lo.max(0), hi.min(len - 1));
    if lo > hi || base < 0 {
        return 0..0;
    }
    (base + lo) as usize..(base + hi + 1) as usize
}
