//! Kontakt controller and modulation laws.
//!
//! The modulation compiler at 0x140873120 emits plain and `fcubic` routes;
//! the source classification is decoded by the format translator.

use crate::Law;

/// Normalized controller or script input used by a volume route.
pub const CONTROLLER_VOLUME_POWER: u8 = 1;
/// Normalized voice source used by a volume route.
pub const VOICE_VOLUME_POWER: u8 = 3;

/// Controller source taper in a volume route.
pub const CONTROLLER_VOLUME: Law = Law::Power {
    lo: 0.0,
    hi: 1.0,
    exponent: CONTROLLER_VOLUME_POWER as f32,
};
/// Voice source taper in a volume route.
pub const VOICE_VOLUME: Law = Law::Power {
    lo: 0.0,
    hi: 1.0,
    exponent: VOICE_VOLUME_POWER as f32,
};

/// Maps normalized input to a bipolar value (0x14071a840 on 0..=1).
pub const BIPOLAR: Law = Law::Linear { lo: -1.0, hi: 1.0 };

/// Saved modulation-shaper points as an evaluator law.
///
/// Kontakt 0x14087c7f0 scales curvature by 16 and reverses it on a falling
/// segment. The evaluator uses the exponential curve without native's fast
/// exponential approximation.
#[must_use]
pub fn modulation_shaper(points: &[(f32, f32, f32)]) -> Law {
    Law::Breakpoints(
        points
            .iter()
            .enumerate()
            .map(|(i, &(x, y, curve))| {
                let direction = points
                    .get(i + 1)
                    .map_or(1.0, |next| if next.1 < y { -1.0 } else { 1.0 });
                (x, y, 16.0 * curve * direction)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The normalized bipolar mapping agrees with native endpoints and midpoint.
    #[test]
    fn bipolar_curve_maps_normalized_endpoints_and_midpoint() {
        for (x, expected) in [(0.0, -1.0), (0.5, 0.0), (1.0, 1.0)] {
            assert_eq!(BIPOLAR.eval(x), expected, "input {x}");
        }
    }
}
