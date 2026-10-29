//! UVI XML value conversion.

use roxmltree::Node;

/// UVI stores linear gains (a program gain of 1.9952623 is +6 dB).
pub(super) fn db(linear: impl Into<f64>) -> f32 {
    (20.0 * linear.into().max(1e-6).log10()) as f32
}

pub(super) fn num(node: Node, name: &str, default: f64) -> Result<f64, String> {
    match node.attribute(name) {
        None => Ok(default),
        Some(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("{name}={text:?} is not a number")),
    }
}

pub(super) fn flag(node: Node, name: &str) -> bool {
    node.attribute(name)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .is_some_and(|v| v != 0.0)
}

/// A MIDI note or velocity attribute, clamped to 0..=127.
pub(super) fn midi_or(node: Node, name: &str, default: u8) -> u8 {
    node.attribute(name)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map_or(default, |v| v.round().clamp(0.0, 127.0) as u8)
}
