//! Time-varying sources, read from a voice's age: breakpoint envelopes and
//! LFOs. Evaluated in closed form once per span, so a voice carries no
//! modulator state and a refresh at any time gives the same value.

use kontra_ir as ir;
use kontra_time::SampleTime;

use crate::Engine;

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

impl Engine {
    /// A voice's age now, from when it started and when its key went up.
    pub(crate) fn age(&self, started: SampleTime, released: Option<SampleTime>) -> Age {
        let seconds = |t: SampleTime| t.0 as f32 / self.rate as f32;
        Age {
            held: seconds(SampleTime(self.now.0.saturating_sub(started.0))),
            released: released.map(|r| seconds(SampleTime(r.0.saturating_sub(started.0)))),
            now: seconds(self.now),
        }
    }

    /// Re-reads envelopes and LFOs for voices whose group has any, and every
    /// voice while a script modulation ramps; once per [`MAX_SPAN`] grid step.
    pub(crate) fn refresh_timed(&mut self) {
        let plan = self.plan.clone();
        let ramping = self.now.0 < self.ramping_until;
        for i in 0..self.voices.len() {
            let Some(slot) = self.voices.get(i) else {
                continue;
            };
            let Some(zone) = plan.zones.get(slot.zone) else {
                continue;
            };
            let timed = ramping
                || self
                    .groups
                    .get(zone.group as usize)
                    .is_some_and(|g| !g.envelopes.is_empty() || !g.lfos.is_empty());
            let n = slot.note.and_then(|n| self.notes.get(n).copied());
            let (Some(n), true, true) = (n, timed, slot.voice.is_active()) else {
                continue;
            };
            let age = self.age(slot.started, slot.released_at);
            let gain = self.voice_gain(zone, &n, slot.factor, age);
            let step = self.step(zone, &n, age);
            self.retune_rack(i, zone, &n, age);
            if let Some(slot) = self.voices.get_mut(i) {
                slot.voice.set_gain(gain);
                slot.voice.set_step(step);
            }
        }
    }
}

/// The envelope's level at `age`.
pub(crate) fn envelope(e: &ir::Breakpoints, age: Age) -> f32 {
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
pub(crate) fn lfo(l: &ir::Lfo, age: Age, seed: u32, tempo: f32) -> f32 {
    let t = match l.clock {
        ir::Clock::Voice => age.held,
        ir::Clock::Free => age.now,
    };
    let hz = match l.rate {
        ir::Rate::Hz(hz) => hz,
        ir::Rate::Beats(beats) => tempo / 60.0 / beats.max(1e-3),
    };
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
        ir::Wave::Multi {
            weights: [w0, w1, w2, w3, w4],
            width,
            normalize,
        } => {
            let saw = 2.0 * (2.0 * p).fract() - 1.0;
            let triangle = if p < 0.5 {
                4.0 * (p - 0.25)
            } else {
                4.0 * (0.75 - p)
            };
            let y = -w0 * sine + w1 * saw + w2 * triangle - w3 * pulse(width) + w4 * random();
            let sum = w0.abs() + w1.abs() + w2.abs() + w3.abs() + w4.abs();
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
            loop_from: None,
        };
        assert!((envelope(&e, at(0.05, None)) - 0.5).abs() < 1e-5);
        assert!((envelope(&e, at(0.15, None)) - 0.75).abs() < 1e-5);
        assert!((envelope(&e, at(5.0, None)) - 0.5).abs() < 1e-5);
        // Key-up mid attack: release falls from 0.5 over the last 200 ms.
        assert!((envelope(&e, at(0.15, Some(0.05))) - 0.25).abs() < 1e-5);
        assert!(envelope(&e, at(1.0, Some(0.05))).abs() < 1e-6);
    }

    #[test]
    fn a_looped_envelope_cycles_while_held() {
        let point = |ms, level| ir::Breakpoint {
            ms,
            level,
            curve: 0.0,
        };
        let e = ir::Breakpoints {
            start: 0.0,
            points: vec![point(100.0, 1.0), point(100.0, 0.0), point(100.0, 1.0)],
            sustain: Some(2),
            loop_from: Some(1),
        };
        assert!((envelope(&e, at(0.25, None)) - 0.5).abs() < 1e-5);
        assert!(
            (envelope(&e, at(0.35, None)) - 0.5).abs() < 1e-5,
            "back down"
        );
        assert!((envelope(&e, at(0.45, None)) - 0.5).abs() < 1e-5, "and up");
        assert!((envelope(&e, at(0.40, None)) - 0.0).abs() < 1e-5);
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
            rate: ir::Rate::Hz(5.0),
            phase: 0.0,
            fade_ms: 0.0,
            clock: ir::Clock::Voice,
        };
        assert!((lfo(&l, at(0.05, None), 0, 120.0) - 1.0).abs() < 1e-4);
        assert!(lfo(&l, at(0.2, None), 0, 120.0).abs() < 1e-4);
        // A Multi's sine enters inverted; two full weights normalise to half each.
        let multi = |weights, normalize| ir::Lfo {
            wave: ir::Wave::Multi {
                weights,
                width: 0.5,
                normalize,
            },
            ..l
        };
        let sine = multi([1.0, 0.0, 0.0, 0.0, 0.0], false);
        assert!((lfo(&sine, at(0.05, None), 0, 120.0) + 1.0).abs() < 1e-4);
        let both = multi([1.0, 0.0, 1.0, 0.0, 0.0], true);
        // At a quarter cycle: (−1 + 0) / 2.
        assert!((lfo(&both, at(0.05, None), 0, 120.0) + 0.5).abs() < 1e-4);
    }
}
