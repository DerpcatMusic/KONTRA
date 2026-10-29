//! Time-varying sources, read from a voice's age: breakpoint envelopes and
//! LFOs. Evaluated in closed form once per span; a voice carries only the
//! cycles live rate routes added to its LFOs ([`Age::warp`]).

use kontra_ir as ir;
use kontra_time::SampleTime;

use crate::ledger::Note;
use crate::voice_gain::{self, GROUP_PAN_TAU_MS};
use crate::{Engine, MAX_SPAN};

pub(crate) use kontra_plan::modulation::{
    Age, CHAOS_START, Chaos, WARPS, chaos_step, envelope, lfo,
};

impl Engine {
    /// A voice's age now, from when it started and when its key went up.
    pub(crate) fn age(&self, started: SampleTime, released: Option<SampleTime>) -> Age {
        let seconds = |t: SampleTime| t.0 as f32 / self.rate as f32;
        Age {
            held: seconds(SampleTime(self.now.0.saturating_sub(started.0))),
            released: released.map(|r| seconds(SampleTime(r.0.saturating_sub(started.0)))),
            now: seconds(self.now),
            warp: [0.0; WARPS],
            chaos: [0.0; WARPS],
        }
    }

    /// Re-reads envelopes and LFOs for voices whose group has any, gliding
    /// voices' lagging pitch and group pan, and every voice while a script modulation
    /// ramps; once per [`MAX_SPAN`] grid step.
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
            let gliding = slot.bend[0] != slot.bend[1] || slot.pan[0] != slot.pan[1];
            let timed = ramping
                || gliding
                || self
                    .groups
                    .get(zone.group as usize)
                    .is_some_and(|g| !g.envelopes.is_empty() || !g.lfos.is_empty());
            #[cfg(feature = "trace")]
            let pending = slot.pending_trace_cc.replace(0);
            let n = slot.note.and_then(|n| self.notes.get(n).copied());
            let (Some(n), true, true) = (n, timed, slot.voice.is_active()) else {
                continue;
            };
            #[cfg(feature = "trace")]
            if pending != 0
                && let Some(trace) = &self.trace
            {
                trace.routes.begin_mask(i, slot.zone, pending);
            }
            let rate = self.rate;
            let span = (MAX_SPAN - self.now.0 % MAX_SPAN) as f32 / rate as f32;
            let chaos = self.chaos(zone, slot.chaos, span);
            let age = Age {
                warp: slot.warp,
                chaos: chaos.map(|c| c[3]),
                ..self.age(slot.started, slot.voice.released_at().map(SampleTime))
            };
            let warp = self.warp(zone, &n, age, span);
            let to = self.group_pan(zone);
            let pan = voice_gain::glide(slot.pan[0], to, GROUP_PAN_TAU_MS, MAX_SPAN as f32, rate);
            let (gain, lag) = self.voice_gain(zone, &n, slot.factor, age, pan);
            let mix = self.voice_mix(zone, &n, age);
            let (step, bend) = self.step(zone, &n, age);
            let law = self.plan.route_pitch;
            self.retune_rack(i, zone, &n, age);
            #[cfg(feature = "trace")]
            if let Some(trace) = &self.trace {
                trace.routes.end();
            }
            if let Some(slot) = self.voices.get_mut(i) {
                slot.voice.set_gain(gain);
                slot.voice.set_lag(lag);
                if let Some(mix) = mix {
                    slot.voice.set_mix(mix);
                }
                let at = voice_gain::glide(
                    slot.bend[0],
                    bend.semitones,
                    bend.tau_ms,
                    MAX_SPAN as f32,
                    rate,
                );
                slot.bend = [at, bend.semitones];
                slot.pan = [pan, to];
                slot.warp = warp;
                slot.chaos = chaos;
                slot.voice
                    .set_step(voice_gain::bent(law, step, bend.instant + at));
            }
        }
    }
}

impl Engine {
    /// `age`'s warps `span` seconds on: each LFO's live rate routes, read
    /// now, held over the span (the rate steps, the phase stays continuous).
    fn warp(&self, zone: &crate::Zone, note: &Note, age: Age, span: f32) -> [f32; WARPS] {
        let mut warp = age.warp;
        let Some(g) = self.groups.get(zone.group as usize) else {
            return warp;
        };
        // Per LFO: Hz added, and its range once a route reaches it.
        let mut added = [(0.0, None); WARPS];
        for m in g.mods.iter().filter(|m| !m.bypass) {
            if let ir::ModKind::LfoHz {
                source,
                lfo,
                depth,
                range_hz: [lo, hi],
            } = m.kind
                && let Some((hz, range)) = added.get_mut(usize::from(lfo))
            {
                let input = self.source(zone, source, note, age);
                let value = input.unwrap_or(0.0) * depth * (hi - lo);
                #[cfg(feature = "trace")]
                self.trace_route(zone, m, input, value, crate::RouteUnit::LfoHertz);
                *hz += value;
                *range = Some([lo, hi.max(lo)]);
            }
        }
        for ((w, (hz, range)), lfo) in warp.iter_mut().zip(added).zip(&g.lfos) {
            if let (Some([lo, hi]), ir::Rate::Hz(f)) = (range, lfo.rate) {
                *w = (*w + ((f + hz).clamp(lo, hi) - f) * span).rem_euclid(1.0);
            }
        }
        warp
    }
}

impl Engine {
    /// A voice's chaotic LFOs `span` seconds on: each attractor's state
    /// (x, y, z) one Euler step further, and its level last.
    pub(crate) fn chaos(
        &self,
        zone: &crate::Zone,
        mut states: [Chaos; WARPS],
        span: f32,
    ) -> [Chaos; WARPS] {
        let Some(g) = self.groups.get(zone.group as usize) else {
            return states;
        };
        for (state, l) in states.iter_mut().zip(&g.lfos) {
            *state = chaos_step(l, *state, span, self.tempo);
        }
        states
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lorenz_lfo_wanders_both_lobes_within_its_scale() {
        let l = ir::Lfo {
            wave: ir::Wave::Chaos {
                attractor: ir::Attractor::Lorenz {
                    sigma: 10.0,
                    rho: 28.0,
                    beta: 8.0 / 3.0,
                },
                step: 1.0,
                scale: 0.05,
            },
            rate: ir::Rate::Hz(4.5),
            phase: 0.0,
            fade_ms: 0.0,
            clock: ir::Clock::Voice,
        };
        let span = MAX_SPAN as f32 / 48_000.0;
        let mut state = CHAOS_START;
        let (mut lo, mut hi) = (0f32, 0f32);
        for _ in 0..3 * 1500 {
            state = chaos_step(&l, state, span, 120.0);
            (lo, hi) = (lo.min(state[3]), hi.max(state[3]));
        }
        // x swings about ±20 between the two lobes: ±1 at this scale.
        assert!(lo < -0.6 && hi > 0.6 && lo > -1.2 && hi < 1.2, "{lo} {hi}");
    }

    fn at(held: f32, released: Option<f32>) -> Age {
        Age {
            held,
            released,
            now: held,
            ..Age::START
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
            stages: false,
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
            stages: false,
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
        let sine = multi([1.0, 0.0, 0.0, 0.0, 0.0, 0.0], false);
        assert!((lfo(&sine, at(0.05, None), 0, 120.0) + 1.0).abs() < 1e-4);
        let both = multi([1.0, 0.0, 1.0, 0.0, 0.0, 0.0], true);
        // At a quarter cycle: (−1 + 0) / 2.
        assert!((lfo(&both, at(0.05, None), 0, 120.0) + 0.5).abs() < 1e-4);
    }

    #[test]
    fn a_multi_mixes_sine_triangle_ramp_and_pulse_by_sign() {
        // `s·sin + t·tri + w·(1 − 2p) + q·pulse`, the triangle from +1, as
        // weights `[−s, 0, −t, −q, 0, w]`.
        let (s, t, w, q, width) = (0.5, 0.25, 0.75, 0.2, 0.3);
        let l = ir::Lfo {
            wave: ir::Wave::Multi {
                weights: [-s, 0.0, -t, -q, 0.0, w],
                width,
                normalize: false,
            },
            rate: ir::Rate::Hz(1.0),
            phase: 0.0,
            fade_ms: 0.0,
            clock: ir::Clock::Voice,
        };
        for p in [0.0f32, 0.1, 0.29, 0.31, 0.5, 0.7, 0.95] {
            let tri = if p < 0.5 {
                1.0 - 4.0 * p
            } else {
                4.0 * p - 3.0
            };
            let pulse = if p < width { 1.0 } else { -1.0 };
            let want =
                s * (std::f32::consts::TAU * p).sin() + t * tri + w * (1.0 - 2.0 * p) + q * pulse;
            let got = lfo(&l, at(p, None), 0, 120.0);
            assert!((got - want).abs() < 1e-3, "{p}: {got} {want}");
        }
    }
}
