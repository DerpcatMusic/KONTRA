//! Time-varying sources, read from a voice's age: breakpoint envelopes and
//! LFOs. Evaluated in closed form once per span; a voice carries only the
//! cycles live rate routes added to its LFOs ([`Age::warp`]).

use kontra_ir as ir;
use kontra_time::SampleTime;

use crate::ledger::Note;
use crate::voice_gain::{self, GROUP_PAN_TAU_MS};
use crate::{Engine, MAX_SPAN};
use std::cell::Cell;

/// Source values read more than once while one voice refreshes (per route,
/// and again for the next refresh's ramp). Live only between [`Memo::begin`]
/// and [`Memo::end`]: no event or script write lands inside one voice's
/// refresh, so a repeat read at the same age is the same value.
#[derive(Default)]
pub(crate) struct Memo {
    live: Cell<bool>,
    lfo: [Slot; MEMO],
    envelope: [Slot; MEMO],
    zone_envelope: [Cell<Option<ZoneRead>>; 16],
}

/// A cached zone read for the current voice, including both position clocks.
#[derive(Clone, Copy)]
struct ZoneRead {
    source: u8,
    position: [u64; 2],
    value: Option<f32>,
}

/// A source's value remembered at one age; `None` inside is "no value".
type Aged = Option<(u64, Option<f32>)>;

/// Up to two ages (now, and the next refresh) per source.
#[derive(Default)]
struct Slot([Cell<Aged>; 2]);

impl Memo {
    pub(crate) fn begin(&self) {
        for s in self.lfo.iter().chain(&self.envelope) {
            for c in &s.0 {
                c.set(None);
            }
        }
        for entry in &self.zone_envelope {
            entry.set(None);
        }
        self.live.set(true);
    }

    pub(crate) fn end(&self) {
        self.live.set(false);
    }

    pub(crate) fn lfo(&self, n: u8, age: Age, f: impl FnOnce() -> Option<f32>) -> Option<f32> {
        self.read(self.lfo.get(usize::from(n)), age, f)
    }

    pub(crate) fn envelope(&self, n: u8, age: Age, f: impl FnOnce() -> Option<f32>) -> Option<f32> {
        self.read(self.envelope.get(usize::from(n)), age, f)
    }

    pub(crate) fn zone_envelope(
        &self,
        source: u8,
        age: Age,
        f: impl FnOnce() -> Option<f32>,
    ) -> Option<f32> {
        if !self.live.get() {
            return f();
        }
        let position = [age.sample_pos.to_bits(), age.unpitched_pos.to_bits()];
        let Some(entry) = self
            .zone_envelope
            .get(usize::from(source) % self.zone_envelope.len())
        else {
            return f();
        };
        if let Some(read) = entry.get()
            && read.source == source
            && read.position == position
        {
            return read.value;
        }
        let value = f();
        entry.set(Some(ZoneRead {
            source,
            position,
            value,
        }));
        value
    }

    fn read(&self, slot: Option<&Slot>, age: Age, f: impl FnOnce() -> Option<f32>) -> Option<f32> {
        let Some(slot) = slot.filter(|_| self.live.get()) else {
            return f();
        };
        let key = u64::from(age.held.to_bits()) << 32 | u64::from(age.now.to_bits());
        if let Some((_, v)) = slot.0.iter().filter_map(Cell::get).find(|&(k, _)| k == key) {
            return v;
        }
        let v = f();
        if let Some(c) = slot.0.iter().find(|c| c.get().is_none()) {
            c.set(Some((key, v)));
        }
        v
    }
}

pub(crate) use kontra_plan::modulation::{
    Age, CHAOS_START, Chaos, EnvScale, chaos_step, lfo, smoothed_shared, stretched,
};

/// How many of a group's LFOs and envelopes [`Memo`] remembers; later
/// ones are read afresh each time.
const MEMO: usize = 8;

/// A voice's timed state ([`crate::Slot::warp`], `chaos`, `stretch`), lent
/// out of its slot while the engine reads it and changes the voice.
#[derive(Default)]
pub(crate) struct Timed {
    pub(crate) warp: Box<[f32]>,
    pub(crate) chaos: Box<[Chaos]>,
    pub(crate) stretch: Box<[EnvScale]>,
}

impl Timed {
    /// `timing` reading this state.
    pub(crate) fn age<'a>(&'a self, timing: Age<'static>) -> Age<'a> {
        Age {
            warp: &self.warp,
            chaos: &self.chaos,
            stretch: &self.stretch,
            ..timing
        }
    }
}

impl Engine {
    /// Voice `i`'s timed state, taken out of its slot (no allocation): give
    /// it back by assigning the slot's fields.
    pub(crate) fn lend_timed(&mut self, i: usize) -> Timed {
        let Some(s) = self.voices.get_mut(i) else {
            return Timed::default();
        };
        Timed {
            warp: std::mem::take(&mut s.warp),
            chaos: std::mem::take(&mut s.chaos),
            stretch: std::mem::take(&mut s.stretch),
        }
    }

    /// A voice's age now, from when it started and when its key went up.
    pub(crate) fn age(&self, started: SampleTime, released: Option<SampleTime>) -> Age<'static> {
        let seconds = |t: SampleTime| t.0 as f32 / self.rate as f32;
        Age {
            held: seconds(SampleTime(self.now.0.saturating_sub(started.0))),
            released: released.map(|r| seconds(SampleTime(r.0.saturating_sub(started.0)))),
            now: seconds(self.now),
            ..Age::START
        }
    }

    /// The full modulation age of an active voice slot, including its
    /// sample-position coordinates for per-zone envelopes.
    pub(crate) fn slot_age<'a>(&self, slot: &'a crate::Slot) -> Age<'a> {
        Age {
            warp: &slot.warp,
            chaos: &slot.chaos,
            stretch: &slot.stretch,
            ..self.slot_timing(slot)
        }
    }

    /// [`Self::slot_age`] without the slot's timed state, for a caller that
    /// lends that out ([`Self::lend_timed`]) while it changes the engine.
    pub(crate) fn slot_timing(&self, slot: &crate::Slot) -> Age<'static> {
        let elapsed = self.now.0.saturating_sub(slot.started.0) as f64;
        Age {
            start_frame: slot.start_frame,
            sample_pos: slot.start_frame + slot.voice.position(),
            unpitched_pos: slot.start_frame + elapsed,
            ..self.age(slot.started, slot.voice.released_at().map(SampleTime))
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
            let gliding = slot.bend.iter().any(|b| b[0] != b[1])
                || slot.pan[0] != slot.pan[1]
                || slot.slide.is_some();
            let timed = ramping
                || gliding
                || !zone.envelopes.is_empty()
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
            // This span's warps, worked out beside the current ones.
            let mut next = std::mem::take(&mut self.warp_scratch);
            let mut timed = self.lend_timed(i);
            let Some(slot) = self.voices.get(i) else {
                continue;
            };
            #[cfg(feature = "trace")]
            if pending != 0
                && let Some(trace) = &self.trace
            {
                trace.routes.begin_mask(i, slot.zone, pending);
            }
            self.memo.begin();
            let rate = self.rate;
            let span = (MAX_SPAN - self.now.0 % MAX_SPAN) as f32 / rate as f32;
            self.chaos(zone, &mut timed.chaos, span);
            let age = timed.age(self.slot_timing(slot));
            self.warp(zone, &n, age, span, &mut next);
            let to = self.group_pan(zone);
            let pan = voice_gain::glide(slot.pan[0], to, GROUP_PAN_TAU_MS, MAX_SPAN as f32, rate);
            let (gain, lag) = self.voice_gain(zone, &n, slot.factor, age, pan);
            let mix = self.voice_mix(zone, &n, age);
            let read = self.voice_read(zone, &n, age);
            let (step, bend) = self.step(zone, &n, age);
            let law = self.plan.route_pitch;
            self.retune_rack(i, zone, &n, age);
            self.memo.end();
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
                if let Some(read) = read {
                    slot.voice.set_read(read);
                }
                let mut at = 0.0;
                for (b, (st, tau_ms)) in slot.bend.iter_mut().zip(bend.lagging) {
                    *b = [
                        voice_gain::glide(b[0], st, tau_ms, MAX_SPAN as f32, rate),
                        st,
                    ];
                    at += b[0];
                }
                slot.pan = [pan, to];
                self.warp_scratch = std::mem::replace(&mut timed.warp, next);
                (slot.warp, slot.chaos, slot.stretch) = (timed.warp, timed.chaos, timed.stretch);
                slot.voice
                    .set_step(voice_gain::bent(law, step, bend.instant + at));
                if slot.slide.is_some() {
                    self.set_pitch(i, step, bend.instant + at);
                }
            }
        }
    }
}

impl Engine {
    /// `age`'s warps `span` seconds on, into `out` (one per LFO): each
    /// LFO's live rate routes, read now, held over the span (the rate steps,
    /// the phase stays continuous).
    fn warp(&self, zone: &crate::Zone, note: &Note, age: Age, span: f32, out: &mut [f32]) {
        for (w, was) in out.iter_mut().zip(age.warp) {
            *w = *was;
        }
        let Some(g) = self.groups.get(zone.group as usize) else {
            return;
        };
        for (n, (w, lfo)) in out.iter_mut().zip(&g.lfos).enumerate() {
            // Hz added and its range once a route reaches it, places its
            // synced rate moves along the note ladder, and its rate as its
            // knob routes move it: by the sources fixed for the note (as it
            // starts, `moved_lfo`) and by every source now.
            let (mut hz, mut range, mut places) = (0.0, None, 0.0f32);
            let nominal = kontra_plan::modulation::hz(lfo, self.tempo);
            let mut rated = [nominal; 2];
            let onset = Age::at_start(age.start_frame);
            for m in g.mods.iter().filter(|m| !m.bypass) {
                let ir::ModRoute {
                    source,
                    target: ir::ParamAddr::LfoParam { lfo: of, param },
                    depth,
                    law,
                    ..
                } = m.route
                else {
                    continue;
                };
                if usize::from(of) != n {
                    continue;
                }
                match (param, law) {
                    (ir::LfoParam::Division, _) => {
                        let input = self.source(zone, source, note, age);
                        places += input.unwrap_or(0.0) * depth * 29.0;
                    }
                    // An LFO source here would recurse.
                    (ir::LfoParam::Rate, ir::RouteLaw::Range([lo, hi]))
                        if !matches!(source, ir::Source::Lfo(_)) =>
                    {
                        let octaves = m.route.span;
                        let fixed = source.fixed_for_note();
                        for (live, (r, at)) in [false, true]
                            .into_iter()
                            .zip(rated.iter_mut().zip([onset, age]))
                        {
                            if !(live || fixed) {
                                continue;
                            }
                            let x = self.source(zone, source, note, at).unwrap_or(0.0);
                            *r *= (x * depth * octaves).exp2();
                            if let ir::Rate::Hz(_) = lfo.rate {
                                *r = r.clamp(lo, hi.max(lo));
                            }
                        }
                    }
                    (ir::LfoParam::Hz, ir::RouteLaw::Range([lo, hi])) => {
                        let input = self.source(zone, source, note, age);
                        let value = input.unwrap_or(0.0) * depth * (hi - lo);
                        #[cfg(feature = "trace")]
                        self.trace_route(zone, m, input, value, crate::RouteUnit::LfoHertz);
                        hz += value;
                        range = Some([lo, hi.max(lo)]);
                    }
                    _ => {}
                }
            }
            // The phase follows the live rate from note-on on.
            *w = (*w + (rated[1] - rated[0]) * span).rem_euclid(1.0);
            if let (Some([lo, hi]), ir::Rate::Hz(f)) = (range, lfo.rate) {
                *w = (*w + ((f + hz).clamp(lo, hi) - f) * span).rem_euclid(1.0);
            }
            if places != 0.0
                && let ir::Rate::Beats(b) = lfo.rate
            {
                let moved = ir::Lfo {
                    rate: ir::Rate::Beats(ir::ladder_beats(b, places)),
                    ..*lfo
                };
                let extra = kontra_plan::modulation::hz(&moved, self.tempo)
                    - kontra_plan::modulation::hz(lfo, self.tempo);
                *w = (*w + extra * span).rem_euclid(1.0);
            }
        }
    }
}

impl Engine {
    /// A voice's chaotic LFOs `span` seconds on: each attractor's state
    /// (x, y, z) one Euler step further, and its level last.
    pub(crate) fn chaos(&self, zone: &crate::Zone, states: &mut [Chaos], span: f32) {
        let Some(g) = self.groups.get(zone.group as usize) else {
            return;
        };
        for (state, l) in states.iter_mut().zip(&g.lfos) {
            *state = chaos_step(l, *state, span, self.tempo);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontra_plan::modulation::envelope;

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

    #[test]
    fn zone_envelope_memo_distinguishes_positions_sources_and_voices() {
        let memo = Memo::default();
        let onset = Age::at_start(128.0);
        let later = Age {
            sample_pos: 256.0,
            ..onset
        };
        let reread = Cell::new(0);
        let read = || {
            reread.set(reread.get() + 1);
            Some(0.25)
        };

        memo.begin();
        assert_eq!(memo.zone_envelope(1, onset, read), Some(0.25));
        assert_eq!(memo.zone_envelope(1, onset, read), Some(0.25));
        assert_eq!(reread.get(), 1);
        assert_eq!(memo.zone_envelope(1, later, || Some(0.5)), Some(0.5));
        assert_eq!(memo.zone_envelope(17, later, || None), None);
        assert_eq!(memo.zone_envelope(17, later, read), None);
        assert_eq!(reread.get(), 1);
        memo.end();

        assert_eq!(memo.zone_envelope(17, later, read), Some(0.25));
        memo.begin();
        assert_eq!(memo.zone_envelope(17, later, || Some(1.0)), Some(1.0));
        memo.end();
    }

    fn at(held: f32, released: Option<f32>) -> Age<'static> {
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
