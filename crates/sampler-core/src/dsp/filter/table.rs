//! The filter family's exponential table, shared by AR and Daft. Native `FUN_1404d14d0` fills
//! `T[i] = f32(pow(2, i * (1/60) - 20))` for `i` in `0..2400`, computed in double. A lookup
//! truncates the position to a signed 16-bit index, a negative index reads zero, and inside the
//! table the value is `frac * (T[i+1] - T[i]) + T[i]` with the stored f32 difference. Past the
//! table the caller picks the tail: the AR family reads the last entry (its vectors), the Daft
//! cutoff reads the native tail `1.7526e-37` (`FUN_140b04c80`).

use std::sync::OnceLock;

/// Entries `0..=2400`; the lookup interpolates below 2400 only.
const LEN: usize = 2401;
/// Native tail value (`0x14a6b7100`).
const TAIL: f32 = f32::from_bits(0x026e_8d57);

/// What a lookup at or past index 2400 reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Past {
    /// The last entry (AR, dynamics).
    LastEntry,
    /// The native tail value (Daft cutoff).
    Tail,
}

fn table() -> &'static [f32; LEN] {
    static TABLE: OnceLock<Box<[f32; LEN]>> = OnceLock::new();
    TABLE.get_or_init(|| Box::new(std::array::from_fn(|i| 2f64.powf(i as f64 * (1.0 / 60.0) - 20.0) as f32)))
}

/// Warm the table while a plan is prepared, so the audio path never fills it.
pub(crate) fn prepare() {
    table();
}

/// The native lookup at `position` (table index plus fraction, in table units).
pub(crate) fn exponential(position: f32, past: Past) -> f32 {
    let index = position as i32 as i16;
    if index < 0 {
        return 0.;
    }
    if index >= 2400 {
        return match past {
            Past::LastEntry => table()[2400],
            Past::Tail => TAIL,
        };
    }
    let t = table();
    let i = index as usize;
    let fraction = position - i as f32;
    fraction * (t[i + 1] - t[i]) + t[i]
}
