//! UVI parameter curves.

use crate::Law;

/// The key/velocity fade edge (low-key branch at 0x140f28cba).
#[must_use]
pub fn fade_edge(curve: f32) -> Law {
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
        }
    }
}
