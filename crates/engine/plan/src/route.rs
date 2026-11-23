//! How long a key has been held, as a route source reads it.

/// 1 at the key's press, falling to 0 once it has been held `span_ms`.
pub fn held_time(span_ms: u32, held: f32) -> f32 {
    if span_ms == 0 {
        return 1.0;
    }
    let t = span_ms as f32;
    ((t - held * 1000.0) / t).max(0.0)
}
