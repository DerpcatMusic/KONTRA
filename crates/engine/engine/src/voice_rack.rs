//! The engine's side of the per-voice racks (kontra-racks): building and
//! tuning each voice's units for its note.

use kontra_ir as ir;

use crate::ledger::Note;
use crate::modulators::Age;
use crate::{Engine, Zone};
pub(crate) use kontra_racks::voice_rack::{
    Kind, MIN_DAMPING, Ramp, Tune, Unit, comb_line, combs, process, start, template, tune_unit,
};

impl Engine {
    /// Every group's per-voice units, and room for the largest in each
    /// voice. Not real-time.
    pub(crate) fn build_voice_racks(&mut self) {
        let rate = self.rate;
        self.voice_units = self.groups.iter().map(|g| template(g, rate)).collect();
        let most = self.voice_units.iter().map(|u| u.len()).max().unwrap_or(0);
        let lines = self.voice_units.iter().map(|u| combs(u)).max().unwrap_or(0) * comb_line(rate);
        for slot in &mut self.voices {
            slot.rack = Vec::with_capacity(most);
            slot.comb_lines = vec![[0.0; 2]; lines];
            // A note an init callback started gets its units too; the next
            // refresh tunes them.
            let group = self.plan.zones.get(slot.zone).map(|z| z.group as usize);
            if slot.voice.is_active()
                && let Some(units) = group.and_then(|g| self.voice_units.get(g))
            {
                slot.rack.extend_from_slice(units);
            }
        }
    }

    /// Gives voice `i` its group's units, tuned for this note at `age`.
    pub(crate) fn start_rack(&mut self, i: usize, zone: &Zone, note: &Note, age: Age) {
        let template = self.voice_units.get(zone.group as usize).cloned();
        let seed = self.now.0 as u32 ^ (i as u32).wrapping_mul(0x9e37_79b9);
        if let Some(slot) = self.voices.get_mut(i) {
            slot.rack.clear();
            // Within the capacity reserved at build: no allocation.
            slot.rack
                .extend_from_slice(template.as_deref().unwrap_or_default());
            for u in &mut slot.rack {
                if let Kind::Crush(c) = &mut u.kind {
                    c.reseed(seed);
                }
            }
            start(&mut slot.rack, &mut slot.comb_lines);
        }
        self.retune_rack(i, zone, note, age);
    }

    /// Moves voice `i`'s routed units to this note's settings at `age`;
    /// their state carries on.
    pub(crate) fn retune_rack(&mut self, i: usize, zone: &Zone, note: &Note, age: Age) {
        let rate = self.rate;
        let count = self.voices.get(i).map_or(0, |s| s.rack.len());
        for n in 0..count {
            let Some(u) = self.voices.get(i).and_then(|s| s.rack.get(n)) else {
                continue;
            };
            let mut ramp = None;
            let effect = match u.kind {
                Kind::Filter(_) | Kind::Eq(_) | Kind::Comb(_) | Kind::Clocked(_) | Kind::Matrix => {
                    None
                }
                _ => self.voice_effect(zone, u.insert, note, age),
            };
            let tune = match u.kind {
                Kind::Matrix => None,
                Kind::Filter(_) => self
                    .voice_filter(zone, u.insert, note, age)
                    .map(|(hz, k)| Tune::Filter(hz, k)),
                Kind::Eq(_) => self.voice_eq_tune(zone, u.insert, note, age),
                Kind::Comb(_) => self
                    .voice_comb(zone, u.insert, note, age)
                    .map(|(hz, g)| Tune::Comb(hz, g)),
                Kind::Clocked(_) => self
                    .voice_clocked(zone, u.insert, note, age)
                    .map(Tune::Clocked),
                Kind::Formant(_)
                | Kind::Wah(_)
                | Kind::SpectralMorph(_)
                | Kind::Overdrive(_)
                | Kind::ColorSat(_) => effect.as_ref().map(Tune::Effect),
                Kind::Shaper(_) | Kind::Strip(..) | Kind::Crush(_) => {
                    // The next refresh, on the grid.
                    let len = (crate::MAX_SPAN - self.now.0 % crate::MAX_SPAN) as u32;
                    let dt = len as f32 / rate as f32;
                    let later = Age {
                        held: age.held + dt,
                        now: age.now + dt,
                        ..age
                    };
                    let next = self.voice_effect(zone, u.insert, note, later);
                    if let (Some(from), Some(to)) = (&effect, next)
                        && *from != to
                    {
                        ramp = Some(Ramp {
                            from: from.clone(),
                            to,
                            at: 0,
                            len,
                        });
                    }
                    effect.as_ref().map(Tune::Effect)
                }
            };
            if let (Some(u), Some(t)) =
                (self.voices.get_mut(i).and_then(|s| s.rack.get_mut(n)), tune)
            {
                tune_unit(u, t, ramp, rate);
            }
        }
    }

    /// The cutoff in Hz and damping of the filter in `insert` of `zone`'s
    /// group, moved by its routes for this note at `age`.
    pub(crate) fn voice_filter(
        &self,
        zone: &Zone,
        insert: u8,
        note: &Note,
        age: Age,
    ) -> Option<(f32, [f32; 3])> {
        let group = self.groups.get(zone.group as usize)?;
        let effect = group.inserts.get(usize::from(insert))?.as_ref()?;
        let ir::EffectKind::Filter(f) = effect.kind else {
            return None;
        };
        let mut offset_hz = 0.0;
        let (mut octaves, mut q, mut resonance) = (0.0f32, 0.0f32, 1.0f32);
        for m in group.mods.iter().filter(|m| !m.bypass) {
            let ir::ModRoute {
                source,
                target,
                depth,
                law,
                ..
            } = m.route;
            match (target, law) {
                (ir::ParamAddr::FilterCutoff { insert: i }, ir::RouteLaw::Linear)
                    if i == insert =>
                {
                    let input = self.routed(zone, source, note, age);
                    let value = input.unwrap_or(0.0) * depth;
                    #[cfg(feature = "trace")]
                    self.trace_route(zone, m, input, value, crate::RouteUnit::CutoffDepth);
                    octaves += value;
                }
                (
                    ir::ParamAddr::FilterCutoff { insert: i },
                    ir::RouteLaw::Range([min_hz, max_hz]),
                ) if i == insert => {
                    if let Some(x) = self.unit(zone, source, note, age) {
                        let x = if !source.is_bipolar() && source.script_channel().is_some() {
                            (x + 1.0) * 0.5
                        } else {
                            x
                        };
                        let delta = min_hz + (max_hz - min_hz) * x.clamp(0.0, 1.0);
                        offset_hz += delta * self.scale(zone, source, note, age);
                    }
                }
                (ir::ParamAddr::FilterResonance { insert: i }, _) if i == insert => {
                    match f.ladder {
                        Some(_) => {
                            let depth = depth * self.scale(zone, source, note, age);
                            let x = self.unit(zone, source, note, age);
                            let value = x.map_or(1.0, |x| ir::Curve::Signed.gain(depth, x));
                            #[cfg(feature = "trace")]
                            self.trace_route(
                                zone,
                                m,
                                x,
                                value,
                                crate::RouteUnit::ResonanceMultiplier,
                            );
                            resonance *= value;
                        }
                        None => {
                            let input = self.routed(zone, source, note, age);
                            let value = input.unwrap_or(0.0) * depth;
                            #[cfg(feature = "trace")]
                            self.trace_route(
                                zone,
                                m,
                                input,
                                value,
                                crate::RouteUnit::ResonanceDepth,
                            );
                            q += value;
                        }
                    }
                }
                _ => {}
            }
        }
        let hz = f.hz * (f.octaves * octaves).exp2();
        let hz = if offset_hz == 0.0 {
            hz
        } else {
            (hz + offset_hz).max(1.0)
        };
        let damping = f
            .damping
            .map(|k| ((2.0 - (2.0 - k) * resonance) * (-f.q_octaves * q).exp2()).max(MIN_DAMPING));
        Some((hz, damping))
    }

    /// The cutoff and resonance of the clocked filter in `insert` of `zone`'s
    /// group, moved by its routes for this note at `age`.
    pub(crate) fn voice_clocked(
        &self,
        zone: &Zone,
        insert: u8,
        note: &Note,
        age: Age,
    ) -> Option<[f32; 2]> {
        let group = self.groups.get(zone.group as usize)?;
        let effect = group.inserts.get(usize::from(insert))?.as_ref()?;
        let ir::EffectKind::Clocked(c) = effect.kind else {
            return None;
        };
        let (mut cut, mut res) = (c.cutoff, c.resonance);
        for m in group.mods.iter().filter(|m| !m.bypass) {
            let ir::ModRoute {
                source,
                target,
                depth,
                ..
            } = m.route;
            match target {
                ir::ParamAddr::FilterCutoff { insert: i } if i == insert => {
                    cut += self.routed(zone, source, note, age).unwrap_or(0.0) * depth;
                }
                ir::ParamAddr::FilterResonance { insert: i } if i == insert => {
                    res += self.routed(zone, source, note, age).unwrap_or(0.0) * depth;
                }
                _ => {}
            }
        }
        Some([cut.clamp(0.0, 1.0), res.clamp(0.0, 1.0)])
    }

    /// The pitch in Hz and feedback of the comb in `insert` of `zone`'s group,
    /// its cutoff routes applied for this note at `age`.
    pub(crate) fn voice_comb(
        &self,
        zone: &Zone,
        insert: u8,
        note: &Note,
        age: Age,
    ) -> Option<(f32, f32)> {
        let group = self.groups.get(zone.group as usize)?;
        let effect = group.inserts.get(usize::from(insert))?.as_ref()?;
        let ir::EffectKind::Comb(c) = effect.kind else {
            return None;
        };
        let (mut octaves, mut resonance) = (0.0f32, 1.0f32);
        for m in group.mods.iter().filter(|m| !m.bypass) {
            let ir::ModRoute {
                source,
                target,
                depth,
                law,
                ..
            } = m.route;
            match (target, law) {
                (ir::ParamAddr::FilterCutoff { insert: i }, ir::RouteLaw::Linear)
                    if i == insert =>
                {
                    let input = self.routed(zone, source, note, age);
                    let value = input.unwrap_or(0.0) * depth;
                    #[cfg(feature = "trace")]
                    self.trace_route(zone, m, input, value, crate::RouteUnit::CutoffDepth);
                    octaves += value;
                }
                // `Q` is the loop gain: scaled as a ladder's resonance is.
                (ir::ParamAddr::FilterResonance { insert: i }, _) if i == insert => {
                    let depth = depth * self.scale(zone, source, note, age);
                    let x = self.unit(zone, source, note, age);
                    resonance *= x.map_or(1.0, |x| ir::Curve::Signed.gain(depth, x));
                }
                _ => {}
            }
        }
        let hz = (c.hz * (c.octaves * octaves).exp2()).clamp(ir::COMB_LOWEST_HZ, 20_000.0);
        Some((hz, (c.feedback * resonance).clamp(0.0, 0.999)))
    }

    /// The effect in `insert` of `zone`'s group, its controls moved by their
    /// routes for this note at `age` (a script's change to its base follows
    /// too).
    pub(crate) fn voice_effect(
        &self,
        zone: &Zone,
        insert: u8,
        note: &Note,
        age: Age,
    ) -> Option<ir::EffectKind> {
        let group = self.groups.get(zone.group as usize)?;
        let mut kind = group
            .inserts
            .get(usize::from(insert))?
            .as_ref()?
            .kind
            .clone();
        for m in group.mods.iter().filter(|m| !m.bypass) {
            let r = m.route;
            if let ir::ParamAddr::EffectParam { insert: i, param } = r.target
                && i == insert
            {
                let (source, depth, unipolar) =
                    (r.source, r.depth, r.law == ir::RouteLaw::Unipolar);
                let input = self.source(zone, source, note, age);
                let x = input.unwrap_or(0.0);
                let x = if unipolar && source.is_bipolar() {
                    (x + 1.0) * 0.5
                } else {
                    x
                };
                let amount = x * depth * self.scale(zone, source, note, age);
                #[cfg(feature = "trace")]
                self.trace_route(zone, m, input, amount, crate::RouteUnit::EffectAmount);
                param.add(&mut kind, amount);
            }
        }
        Some(kind)
    }

    /// The bands (and optional console pass filters) of the EQ in `insert` of
    /// `zone`'s group, each gain moved by its routes for this note at `age`.
    pub(crate) fn voice_eq_tune(
        &self,
        zone: &Zone,
        insert: u8,
        note: &Note,
        age: Age,
    ) -> Option<Tune<'static>> {
        let group = self.groups.get(zone.group as usize)?;
        let effect = group.inserts.get(usize::from(insert))?.as_ref()?;
        let (mut bells, pass) = match effect.kind {
            ir::EffectKind::Eq(b) => (b, None),
            ir::EffectKind::EqPass {
                bands,
                hp_hz,
                lp_hz,
            } => (bands, Some((hp_hz, lp_hz))),
            _ => return None,
        };
        let (mut shift, mut limit) = ([0.0f32; 4], [f32::INFINITY; 4]);
        for m in group.mods.iter().filter(|m| !m.bypass) {
            let r = m.route;
            if let ir::ParamAddr::EqBandGain { insert: i, band } = r.target
                && i == insert
            {
                let (source, depth_db) = (r.source, r.depth);
                let limit_db = match r.law {
                    ir::RouteLaw::Clamp(l) => l,
                    _ => f32::INFINITY,
                };
                let input = self.routed(zone, source, note, age);
                let amount = input.unwrap_or(0.0) * depth_db;
                #[cfg(feature = "trace")]
                self.trace_route(zone, m, input, amount, crate::RouteUnit::EqDecibels);
                if let (Some(s), Some(l)) = (
                    shift.get_mut(usize::from(band)),
                    limit.get_mut(usize::from(band)),
                ) {
                    *s += amount;
                    *l = l.min(limit_db.abs());
                }
            }
        }
        for ((b, s), l) in bells.iter_mut().zip(shift).zip(limit) {
            // min/max, not clamp: a NaN limit must not panic on the audio thread.
            b.gain_db = (b.gain_db + s).min(l).max(-l);
        }
        if let Some(ir::EffectKind::Eq(moved) | ir::EffectKind::EqPass { bands: moved, .. }) =
            self.voice_effect(zone, insert, note, age)
        {
            for (b, m) in bells.iter_mut().zip(moved) {
                b.hz = m.hz;
            }
        }
        Some(match pass {
            Some((hp_hz, lp_hz)) => Tune::EqPass(bells, hp_hz, lp_hz),
            None => Tune::Eq(bells),
        })
    }

    pub(crate) fn voice_bells(
        &self,
        zone: &Zone,
        insert: u8,
        note: &Note,
        age: Age,
    ) -> Option<[ir::Bell; 4]> {
        match self.voice_eq_tune(zone, insert, note, age)? {
            Tune::Eq(b) | Tune::EqPass(b, _, _) => Some(b),
            _ => None,
        }
    }

    /// Groups with a filter that runs per voice, and each sounding voice's
    /// first such cutoff.
    pub(crate) fn voice_cutoffs(&self) -> (usize, Vec<f32>) {
        let is_filter = |u: &Unit| matches!(u.kind, Kind::Filter(_));
        let groups = self
            .voice_units
            .iter()
            .filter(|u| u.iter().any(is_filter))
            .count();
        let cutoffs = self
            .voices
            .iter()
            .filter(|s| s.voice.is_active())
            .filter_map(|s| {
                s.rack.iter().find_map(|u| match &u.kind {
                    Kind::Filter(f) => Some(f.cutoff()),
                    _ => None,
                })
            })
            .collect();
        (groups, cutoffs)
    }
}
