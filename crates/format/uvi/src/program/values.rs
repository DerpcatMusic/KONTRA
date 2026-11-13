//! UVI XML value conversion.

use roxmltree::Node;

/// UVI stores linear gains (a program gain of 1.9952623 is +6 dB).
pub(super) fn db(linear: impl Into<f64>) -> f32 {
    let linear = linear.into().max(1e-6);
    kontra_law::common::DB_GAIN
        .inverse_f64(linear)
        .unwrap_or(0.0) as f32
}

thread_local! {
    /// One attribute read as another value while an effect is rebuilt to
    /// see what the attribute moves ([`probing`]).
    static PROBE: std::cell::RefCell<Option<(roxmltree::NodeId, String, f64)>> =
        const { std::cell::RefCell::new(None) };
}

/// Runs `f` with `node`'s attribute `name` read as `value`.
pub(super) fn probing<T>(node: Node, name: &str, value: f64, f: impl FnOnce() -> T) -> T {
    PROBE.with(|p| *p.borrow_mut() = Some((node.id(), name.to_owned(), value)));
    let out = f();
    PROBE.with(|p| *p.borrow_mut() = None);
    out
}

pub(super) fn num(node: Node, name: &str, default: f64) -> Result<f64, String> {
    let probed = PROBE.with(|p| {
        p.borrow()
            .as_ref()
            .filter(|(id, n, _)| *id == node.id() && n == name)
            .map(|&(_, _, v)| v)
    });
    if let Some(v) = probed {
        return Ok(v);
    }
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
