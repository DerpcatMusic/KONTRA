//! Vendor-neutral unit laws: a level in a log unit as linear gain.

use crate::Law;

/// A level in dB as linear gain, `10^(dB/20)`.
pub const DB_GAIN: Law = Law::PowIndex {
    base: 10.0,
    origin: 0.0,
    divisor: 20.0,
    scale: 1.0,
};

/// A level in millibels (1/1000 dB, KSP's volume unit) as linear gain.
pub const MILLIBEL_GAIN: Law = Law::PowIndex {
    base: 10.0,
    origin: 0.0,
    divisor: 20_000.0,
    scale: 1.0,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_levels_keep_the_hand_written_rounding() {
        for mdb in (-200_000..=24_000).step_by(3) {
            let db = mdb as f32 / 1000.0;
            assert_eq!(
                DB_GAIN.eval_unclamped(db).to_bits(),
                10f32.powf(db / 20.0).to_bits()
            );
            let mdb = mdb as f32;
            assert_eq!(
                MILLIBEL_GAIN.eval_unclamped(mdb).to_bits(),
                10f32.powf(mdb / 20_000.0).to_bits()
            );
        }
    }
}
