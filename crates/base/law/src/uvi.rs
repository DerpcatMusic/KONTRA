//! UVI parameter curves.

use crate::Law;

/// The key/velocity fade edge (low-key branch at 0x140f28cba).
/// Finite curves are clamped to 1..=2; non-finite curves use the default of 2.
#[must_use]
pub fn fade_edge(curve: f32) -> Law {
    let curve = if curve.is_finite() {
        curve.clamp(1.0, 2.0)
    } else {
        2.0
    };
    Law::Sine { power: 2.0 / curve }
}

#[cfg(test)]
mod tests {
    use super::fade_edge;

    #[test]
    fn fade_edge_preserves_the_sampled_quarter_sine() {
        for curve in [1.0, 1.5, 2.0] {
            let law = fade_edge(curve);
            for n in 0..=127 {
                let x = n as f32 / 127.0;
                let expected = (std::f32::consts::FRAC_PI_2 * x).sin().powf(2.0 / curve);
                assert_eq!(law.eval(x), expected, "curve {curve}, input {x}");
            }
            assert_eq!(law.eval(-1.0), 0.0);
            assert_eq!(law.eval(2.0), 1.0);
        }
    }

    #[test]
    fn fade_edge_validates_curve() {
        assert_eq!(fade_edge(-1.0), fade_edge(1.0));
        assert_eq!(fade_edge(3.0), fade_edge(2.0));
        assert_eq!(fade_edge(f32::NAN), fade_edge(2.0));
        assert_eq!(fade_edge(f32::INFINITY), fade_edge(2.0));
    }
}
