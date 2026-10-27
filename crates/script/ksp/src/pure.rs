//! Builtins that need nothing from the engine: arithmetic and array helpers.
//! A host calls [`call`] first and handles the builtin itself when it returns `None`.

use kontra_vm::{HostCall, HostValue};

use crate::Builtin;

/// Evaluates a pure builtin. Reals travel as their bits; builtins over
/// numbers end with a flag that is 1 when those numbers are real.
#[must_use]
pub fn call(call: &mut HostCall<'_>) -> Option<HostValue> {
    let b = Builtin::from_id(call.id)?;
    let ints = call.ints;
    let int = |i: usize| ints.get(i).map_or(0, |&v| v as i32);
    let real = |i: usize| ints.get(i).map_or(0.0, |&v| f64::from_bits(v as u64));
    let is_real = ints.last().is_some_and(|&flag| flag == 1);
    let unary = |f: fn(f64) -> f64| HostValue::Real(f(real(0)));
    let flag = |b: bool| HostValue::Int(i32::from(b));
    Some(match b {
        Builtin::Abs if is_real => HostValue::Real(real(0).abs()),
        Builtin::Abs => HostValue::Int(int(0).wrapping_abs()),
        Builtin::Min if is_real => HostValue::Real(real(0).min(real(1))),
        Builtin::Min => HostValue::Int(int(0).min(int(1))),
        Builtin::Max if is_real => HostValue::Real(real(0).max(real(1))),
        Builtin::Max => HostValue::Int(int(0).max(int(1))),
        Builtin::InRange if is_real => flag(real(1) <= real(0) && real(0) <= real(2)),
        Builtin::InRange => flag(int(1) <= int(0) && int(0) <= int(2)),
        Builtin::Sgn if is_real => HostValue::Int(match real(0) {
            r if r > 0.0 => 1,
            r if r < 0.0 => -1,
            _ => 0,
        }),
        Builtin::Sgn => HostValue::Int(int(0).signum()),
        Builtin::Signbit if is_real => flag(real(0).is_sign_negative()),
        Builtin::Signbit => flag(int(0) < 0),
        Builtin::Round => unary(f64::round),
        Builtin::Floor => unary(f64::floor),
        Builtin::Ceil => unary(f64::ceil),
        Builtin::Sqrt => unary(f64::sqrt),
        Builtin::Cbrt => unary(f64::cbrt),
        Builtin::Exp => unary(f64::exp),
        Builtin::Exp2 => unary(f64::exp2),
        Builtin::Log => unary(f64::ln),
        Builtin::Log2 => unary(f64::log2),
        Builtin::Log10 => unary(f64::log10),
        Builtin::Sin => unary(f64::sin),
        Builtin::Cos => unary(f64::cos),
        Builtin::Tan => unary(f64::tan),
        Builtin::Asin => unary(f64::asin),
        Builtin::Acos => unary(f64::acos),
        Builtin::Atan => unary(f64::atan),
        Builtin::Pow => HostValue::Real(real(0).powf(real(1))),
        // 14-bit values split into MIDI bytes.
        Builtin::Msb => HostValue::Int((int(0) >> 7) & 127),
        Builtin::Lsb => HostValue::Int(int(0) & 127),
        Builtin::Search => {
            let (base, len, lo, hi) = (int(0), int(1), int(3), int(4));
            let needle = ints.get(2).copied().unwrap_or(0);
            let found = range(base, len, lo, hi).find(|&at| call.cells.get(at) == Some(&needle));
            HostValue::Int(found.map_or(-1, |at| at as i32 - base))
        }
        Builtin::Sort => {
            let (base, len, descending, lo, hi) = (int(0), int(1), int(2) != 0, int(3), int(4));
            if let Some(items) = call.cells.get_mut(range(base, len, lo, hi)) {
                if is_real {
                    items.sort_by(|a, b| {
                        f64::from_bits(*a as u64).total_cmp(&f64::from_bits(*b as u64))
                    });
                } else {
                    items.sort_by_key(|&v| v as i32);
                }
                if descending {
                    items.reverse();
                }
            }
            HostValue::Nothing
        }
        Builtin::ArrayEqual => {
            let (a, b) = ((int(0), int(1)), (int(2), int(3)));
            let slice = |(base, len): (i32, i32)| {
                let base = usize::try_from(base).ok()?;
                call.cells.get(base..base + usize::try_from(len).ok()?)
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
