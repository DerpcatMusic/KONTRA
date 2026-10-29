//! Closed-form time-varying modulation sources.

use kontra_ir as ir;

/// When a voice is being read, in seconds.
#[derive(Clone, Copy, Debug)]
pub struct Age {
    /// Since the voice started.
    pub held: f32,
    /// Its age at key-up, once the key is up.
    pub released: Option<f32>,
    /// Since the engine started (free-running LFOs).
    pub now: f32,
    /// Cycles added to the group's first [`WARPS`] LFOs by their live rate
    /// routes ([`ir::ModKind::LfoHz`]), integrated span by span.
    pub warp: [f32; WARPS],
    /// The voice's chaotic LFOs' levels this span (at the current span).
    pub chaos: [f32; WARPS],
}

/// How many of a group's LFOs a live rate route can move.
pub const WARPS: usize = 8;

impl Age {
    pub const START: Self = Self {
        held: 0.0,
        released: None,
        now: 0.0,
        warp: [0.0; WARPS],
        chaos: [0.0; WARPS],
    };
}

/// `state` of `l`, a chaotic LFO, `span` seconds on: one Euler step; any
/// other LFO's as it was.
pub fn chaos_step(l: &ir::Lfo, state: Chaos, span: f32, tempo: f32) -> Chaos {
    let ir::Wave::Chaos {
        attractor,
        step,
        scale,
    } = l.wave
    else {
        return state;
    };
    let h = span * hz(l, tempo) * step;
    let [x, y, z, _] = state;
    let [x, y, z] = match attractor {
        ir::Attractor::Lorenz { sigma, rho, beta } => [
            x + sigma * (y - x) * h,
            y + (x * (rho - z) - y) * h,
            z + (x * y - beta * z) * h,
        ],
        ir::Attractor::Rossler { a, b, c } => [
            x - (y + z) * h,
            y + (x + a * y) * h,
            z + (b + z * (x - c)) * h,
        ],
    };
    [x, y, z, x * scale]
}

/// An attractor's state (x, y, z) and its level (at the current span).
pub type Chaos = [f32; 4];

/// Where every attractor starts.
pub const CHAOS_START: Chaos = [1.0, 1.0, 1.0, 0.0];

/// An LFO's cycles a second at `tempo`.
pub fn hz(l: &ir::Lfo, tempo: f32) -> f32 {
    match l.rate {
        ir::Rate::Hz(hz) => hz,
        ir::Rate::Beats(beats) => tempo / 60.0 / beats.max(1e-3),
        ir::Rate::Steps { beats, count } => tempo / 60.0 / (beats * count).max(1e-3),
    }
}

/// The envelope's level at `age`.
pub fn envelope(e: &ir::Breakpoints, age: Age) -> f32 {
    let Some((sustain, released)) = e.sustain.zip(age.released) else {
        return held(e, age.held);
    };
    let after = e.points.get(sustain + 1..).unwrap_or_default();
    walk(held(e, released), after, age.held - released).0
}

/// The level `t` seconds after the start with the key down: through the
/// points to the sustain point, then holding there or cycling its loop.
fn held(e: &ir::Breakpoints, t: f32) -> f32 {
    let Some(sustain) = e.sustain else {
        return walk(e.start, &e.points, t).0;
    };
    let (level, left) = walk(e.start, e.points.get(..=sustain).unwrap_or(&e.points), t);
    let cycle = e.loop_from.and_then(|from| e.points.get(from..=sustain));
    let (Some(left), Some(cycle)) = (left, cycle) else {
        return level;
    };
    let period: f32 = cycle.iter().map(|p| p.ms / 1000.0).sum();
    if period > 0.0 {
        walk(level, cycle, left % period).0
    } else {
        level
    }
}

/// From `level`, through `points` for `t` seconds: the level reached, and the
/// time left over once past the last point.
fn walk(mut level: f32, points: &[ir::Breakpoint], mut t: f32) -> (f32, Option<f32>) {
    for p in points {
        let span = p.ms.max(0.0) / 1000.0;
        if t < span {
            return (segment(level, p.level, t / span, p.curve), None);
        }
        t -= span;
        level = p.level;
    }
    (level, Some(t))
}

fn segment(from: f32, to: f32, x: f32, k: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let covered = if k.abs() < 1e-6 {
        x
    } else {
        (k * x).exp_m1() / k.exp_m1()
    };
    from + (to - from) * covered
}

/// The oscillator's output at `age`, −1..=1, faded in.
pub fn lfo(l: &ir::Lfo, age: Age, seed: u32, tempo: f32) -> f32 {
    let t = match l.clock {
        ir::Clock::Voice => age.held,
        ir::Clock::Free => age.now,
    };
    let hz = hz(l, tempo);
    let cycles = f64::from(l.phase) + f64::from(hz) * f64::from(t);
    let p = cycles.rem_euclid(1.0) as f32;
    let half = (2.0 * cycles).floor();
    let random = || {
        // A new draw each half cycle: a hash of the step and the voice.
        let mut x = (half as i64 as u64) ^ (u64::from(seed) << 32);
        x = (x ^ (x >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
        x = (x ^ (x >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        ((x >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    };
    let pulse = |width: f32| if p < width { 1.0 } else { -1.0 };
    let sine = (std::f32::consts::TAU * p).sin();
    let v = match l.wave {
        ir::Wave::Sine => sine,
        ir::Wave::Triangle => 1.0 - 4.0 * ((p + 0.25).fract() - 0.5).abs(),
        ir::Wave::Saw => 1.0 - 2.0 * p,
        ir::Wave::Square { width } => pulse(width),
        ir::Wave::Random => random(),
        // Integrated per voice, not a function of time (at the current span).
        ir::Wave::Chaos { .. } => 0.0,
        ir::Wave::Multi {
            weights: [w0, w1, w2, w3, w4, w5],
            width,
            normalize,
        } => {
            let saw = 2.0 * (2.0 * p).fract() - 1.0;
            let triangle = if p < 0.5 {
                4.0 * (p - 0.25)
            } else {
                4.0 * (0.75 - p)
            };
            let y = -w0 * sine + w1 * saw + w2 * triangle - w3 * pulse(width)
                + w4 * random()
                + w5 * (1.0 - 2.0 * p);
            let sum = [w0, w1, w2, w3, w4, w5]
                .iter()
                .map(|w| w.abs())
                .sum::<f32>();
            if normalize { y / sum.max(1.0) } else { y }
        }
    };
    // The native fade (0x140b06c90) runs a state from 0.3 to 1.3 by a constant
    // ratio and scales by `state − 0.3`: an exponential rise that ends at 1.
    let fade = if l.fade_ms > 0.0 {
        let x = (age.held * 1000.0 / l.fade_ms).min(1.0);
        0.3 * ((13.0f32 / 3.0).powf(x) - 1.0)
    } else {
        1.0
    };
    v * fade
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_curved_segment_bows_but_keeps_its_ends() {
        assert!((segment(0.0, 1.0, 0.0, 3.0)).abs() < 1e-6);
        assert!((segment(0.0, 1.0, 1.0, 3.0) - 1.0).abs() < 1e-6);
        assert!(segment(0.0, 1.0, 0.5, 3.0) < 0.5);
    }
}
