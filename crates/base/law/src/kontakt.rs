//! Kontakt controller and modulation laws.
//!
//! The modulation compiler at 0x140873120 emits plain and `fcubic` routes;
//! the source classification is decoded by the format translator.

use crate::Law;

/// Normalized controller or script input used by a volume route.
pub const CONTROLLER_VOLUME_POWER: u8 = 1;
/// Normalized voice source used by a volume route.
pub const VOICE_VOLUME_POWER: u8 = 3;

/// Saved modulation-shaper points as an evaluator law.
///
/// Curvature uses the existing measured approximation; its exact binary law
/// is not established. Falling segments reverse its geometric direction.
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
                (x, y, 17.0 * curve * direction)
            })
            .collect(),
    )
}
