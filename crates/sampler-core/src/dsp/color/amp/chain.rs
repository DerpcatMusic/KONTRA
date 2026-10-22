//! The shared amp chain: input gain, one-pole pre high-pass, a piecewise-linear shaper table
//! over -1..1, one-pole post low-pass, and a fixed latency. A product preamp is a `Spec`
//! preset. The presets are fitted to native traces by tools/kontakt-oracle/fit_chain.py.
//! Filters are one-pole (bilinear-free recursion y += a (x - y)), so each stage is a few
//! multiplies and the state is a handful of f64 values per channel.
use super::super::super::Planar;

/// Shaper knots over -1..1 (linear interpolation between them).
pub(crate) const KNOTS: usize = 17;
/// Largest latency a spec may ask for, in samples. The ring is inline in the color state
/// (2 channels of f64), so it stays small: Jump needs 48. ponytail: raise this if a preset
/// needs more latency, and mind the 3 KB color state budget (`color_state_holds_one_shaper_at_a_time`).
pub(crate) const MAX_DELAY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spec {
    /// Pre high-pass corner, Hz; 0 = no pre-filter.
    pub pre_fc: f64,
    /// Post low-pass corner, Hz; 0 = no post-filter.
    pub post_fc: f64,
    /// Input gain applied before the shaper.
    pub gain: f64,
    /// Latency in samples (< MAX_DELAY).
    pub delay: usize,
    /// Shaper output at the knots -1, -1 + 2/(KNOTS-1), ..., 1.
    pub table: [f64; KNOTS],
}

impl Spec {
    /// The identity shaper: output equals its input.
    pub(crate) fn identity() -> Self {
        let mut table = [0.0; KNOTS];
        for (k, t) in table.iter_mut().enumerate() {
            *t = -1.0 + 2.0 * k as f64 / (KNOTS - 1) as f64;
        }
        Self { pre_fc: 0.0, post_fc: 0.0, gain: 1.0, delay: 0, table }
    }
}

/// One-pole coefficient for a corner frequency at a sample rate; 0 Hz disables the stage.
fn coefficient(fc: f64, rate: f64) -> f64 {
    if fc > 0.0 { 1.0 - (-2.0 * std::f64::consts::PI * fc / rate).exp() } else { 0.0 }
}

/// Table value at `u` in -1..=1, linear between knots.
fn shape(table: &[f64; KNOTS], u: f64) -> f64 {
    let pos = (u.clamp(-1.0, 1.0) + 1.0) * (KNOTS - 1) as f64 / 2.0;
    let i = (pos as usize).min(KNOTS - 2);
    let frac = pos - i as f64;
    table[i] + (table[i + 1] - table[i]) * frac
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct State {
    pre: f64,
    post: f64,
    ring: [f64; MAX_DELAY],
    at: usize,
}

impl Default for State {
    fn default() -> Self {
        Self { pre: 0.0, post: 0.0, ring: [0.0; MAX_DELAY], at: 0 }
    }
}

impl State {
    pub(crate) fn finite(&self) -> bool {
        self.pre.is_finite() && self.post.is_finite() && self.ring.iter().all(|v| v.is_finite())
    }
}

/// A `Spec` resolved for a sample rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Engine {
    spec: Spec,
    a_pre: f64,
    a_post: f64,
}

impl Engine {
    pub(crate) fn new(spec: Spec, rate: u32) -> Self {
        assert!(spec.delay < MAX_DELAY, "amp chain latency {} exceeds {MAX_DELAY}", spec.delay);
        Self { spec, a_pre: coefficient(spec.pre_fc, f64::from(rate)), a_post: coefficient(spec.post_fc, f64::from(rate)) }
    }

    /// One sample through the chain. No allocation; the ring is fixed-size.
    pub(crate) fn sample(&self, s: &mut State, x: f64) -> f64 {
        let hp = if self.spec.pre_fc > 0.0 {
            s.pre += self.a_pre * (x - s.pre);
            x - s.pre
        } else {
            x
        };
        let shaped = shape(&self.spec.table, self.spec.gain * hp);
        s.post = if self.spec.post_fc > 0.0 { s.post + self.a_post * (shaped - s.post) } else { shaped };
        if self.spec.delay == 0 {
            return s.post;
        }
        let out = s.ring[s.at];
        s.ring[s.at] = s.post;
        s.at = (s.at + 1) % self.spec.delay;
        out
    }

    pub(crate) fn process(&self, s: &mut [State; 2], block: &mut Planar, len: usize) {
        for i in 0..len {
            for c in 0..2 {
                block[c][i] = self.sample(&mut s[c], block[c][i]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The identity shaper with no filters and no latency is a null path (bit-exact on knots).
    #[test]
    fn identity_chain_is_a_null_path() {
        let engine = Engine::new(Spec::identity(), 48_000);
        let mut s = State::default();
        for k in 0..=64 {
            let x = -1.0 + 2.0 * k as f64 / 64.0;
            assert!((engine.sample(&mut s, x) - x).abs() < 1e-12, "x = {x}");
        }
    }

    /// Latency delays the output by exactly `delay` samples.
    #[test]
    fn delay_is_exact() {
        let spec = Spec { delay: 5, ..Spec::identity() };
        let engine = Engine::new(spec, 48_000);
        let mut s = State::default();
        let out: Vec<f64> = (0..12).map(|i| engine.sample(&mut s, if i == 0 { 0.5 } else { 0.0 })).collect();
        assert_eq!(out[5], 0.5);
        assert!(out.iter().enumerate().all(|(i, v)| (i == 5) == (*v != 0.0)));
    }
}
