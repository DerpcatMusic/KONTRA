//! The one trait every implementation (ours or a candidate) plugs into.

/// A mono processing block. Parameters are named, values are normalised or in engine units.
pub trait Block {
    /// Process `input` into `out` (same length). State carries across calls.
    fn process(&mut self, input: &[f32], out: &mut [f32]);
    /// Set a named parameter between blocks. False if the key is unknown.
    fn set(&mut self, key: &str, value: f64) -> bool;
    /// Clear all state (filters, envelopes, delay lines).
    fn reset(&mut self);
    /// Latency in frames, 0 if none.
    fn latency(&self) -> usize {
        0
    }
}

/// Builds an implementation at a sample rate with initial named parameters.
pub type Factory = fn(rate: f64, params: &[(&str, f64)]) -> Box<dyn Block>;

/// One side of a pair: a label for the report and a constructor.
#[derive(Clone, Copy)]
pub struct Impl {
    pub label: &'static str,
    pub make: Factory,
}

/// Look up a named value in a parameter list.
pub fn param(params: &[(&str, f64)], key: &str, default: f64) -> f64 {
    params.iter().find(|(k, _)| *k == key).map_or(default, |(_, v)| *v)
}
