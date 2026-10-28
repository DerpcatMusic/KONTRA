//! Time-varying sources, read from a voice's age: breakpoint envelopes and
//! LFOs. Evaluated in closed form once per span, so a voice carries no
//! modulator state and a refresh at any time gives the same value.

use kontra_ir as ir;

/// When a voice is being read, in seconds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Age {
    /// Since the voice started.
    pub held: f32,
    /// Its age at key-up, once the key is up.
    pub released: Option<f32>,
    /// Since the engine started (free-running LFOs).
    pub now: f32,
}

impl Age {
    pub(crate) const START: Self = Self {
        held: 0.0,
        released: None,
        now: 0.0,
    };
}

/// The envelope's level at `age`.
pub(crate) fn envelope(e: &ir::Breakpoints, age: Age) -> f32 {
    let Some((sustain, released)) = e.sustain.zip(age.released) else {
        return walk(e.start, e.points.iter().enumerate(), age.held, e.sustain);
    };
    let from = walk(
        e.start,
        e.points.iter().enumerate(),
        released,
        Some(sustain),
    );
    let after = e.points.iter().enumerate().skip(sustain + 1);
    walk(from, after, age.held - released, None)
}

/// From `level`, through `points` for `t` seconds; holds at `hold`.
fn walk<'a>(
    mut level: f32,
    points: impl Iterator<Item = (usize, &'a ir::Breakpoint)>,
    mut t: f32,
    hold: Option<usize>,
) -> f32 {
    for (n, p) in points {
        let span = p.ms.max(0.0) / 1000.0;
        if t < span {
            return segment(level, p.level, t / span, p.curve);
        }
        t -= span;
        level = p.level;
        if hold == Some(n) {
            break;
        }
    }
    level
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
pub(crate) fn lfo(l: &ir::Lfo, age: Age, seed: u32) -> f32 {
    let t = if l.retrigger { age.held } else { age.now };
    let cycles = f64::from(l.phase) + f64::from(l.hz) * f64::from(t);
    let p = cycles.rem_euclid(1.0) as f32;
    let v = match l.wave {
        ir::Wave::Sine => (std::f32::consts::TAU * p).sin(),
        ir::Wave::Triangle => 1.0 - 4.0 * ((p + 0.25).fract() - 0.5).abs(),
        ir::Wave::Saw => 2.0 * p - 1.0,
        ir::Wave::Square { width } => {
            if p < width {
                1.0
            } else {
                -1.0
            }
        }
        ir::Wave::Random => {
            // One draw per cycle: a hash of the cycle and the voice.
            let mut x = (cycles.floor() as i64 as u64) ^ (u64::from(seed) << 32);
            x = (x ^ (x >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
            x = (x ^ (x >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
            ((x >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
        }
    };
    let fade = if l.fade_ms > 0.0 {
        (age.held * 1000.0 / l.fade_ms).min(1.0)
    } else {
        1.0
    };
    v * fade
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(held: f32, released: Option<f32>) -> Age {
        Age {
            held,
            released,
            now: held,
        }
    }

    #[test]
    fn an_envelope_holds_at_sustain_and_releases_from_where_it_is() {
        let point = |ms, level| ir::Breakpoint {
            ms,
            level,
            curve: 0.0,
        };
        let e = ir::Breakpoints {
            start: 0.0,
            points: vec![point(100.0, 1.0), point(100.0, 0.5), point(200.0, 0.0)],
            sustain: Some(1),
        };
        assert!((envelope(&e, at(0.05, None)) - 0.5).abs() < 1e-5);
        assert!((envelope(&e, at(0.15, None)) - 0.75).abs() < 1e-5);
        assert!((envelope(&e, at(5.0, None)) - 0.5).abs() < 1e-5);
        // Key-up mid attack: release falls from 0.5 over the last 200 ms.
        assert!((envelope(&e, at(0.15, Some(0.05))) - 0.25).abs() < 1e-5);
        assert!(envelope(&e, at(1.0, Some(0.05))).abs() < 1e-6);
    }

    #[test]
    fn a_curved_segment_bows_but_keeps_its_ends() {
        assert!((segment(0.0, 1.0, 0.0, 3.0)).abs() < 1e-6);
        assert!((segment(0.0, 1.0, 1.0, 3.0) - 1.0).abs() < 1e-6);
        assert!(segment(0.0, 1.0, 0.5, 3.0) < 0.5);
    }

    #[test]
    fn an_lfo_cycles_at_its_rate() {
        let l = ir::Lfo {
            wave: ir::Wave::Sine,
            hz: 5.0,
            phase: 0.0,
            fade_ms: 0.0,
            retrigger: true,
        };
        assert!((lfo(&l, at(0.05, None), 0) - 1.0).abs() < 1e-4);
        assert!(lfo(&l, at(0.2, None), 0).abs() < 1e-4);
    }
}
