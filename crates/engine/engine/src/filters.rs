//! EQ bands and filters on summed audio. A section with fixed settings is
//! linear and time invariant, so it commutes with each voice's gain matrix:
//! voices whose groups carry the same sections on the same bus share one
//! chain, run once per span instead of once per voice. Bus and instrument
//! racks filter their own sums.
//!
//! ponytail: settings are fixed at load (after the init callback). Live
//! changes (scripts) need per-voice filters. A group's filter or EQ that a
//! route moves (cutoff, resonance, band gain) runs per voice, one filter and
//! one EQ per group; a second moved insert of the same kind stays shared.

use kontra_dsp::eq::{Bell, Biquad, Side};
use kontra_dsp::filter::{Pass, Shape, Svf};
use kontra_ir as ir;

use crate::ledger::Note;
use crate::modulators::Age;
use crate::{Engine, MAX_SPAN, Zone};

/// ponytail: the lowest damping (`1/Q`) a resonance route may reach; the
/// stages go unstable below it.
const MIN_DAMPING: f32 = 0.01;

pub(crate) struct Chain {
    bus: usize,
    sections: Vec<ir::EffectKind>,
    filters: Box<[Biquad]>,
    pub(crate) mix: Box<[[f32; 2]]>,
    pub(crate) live: bool,
}

/// The rack's active EQs (their audible bands) and filters, in slot order.
pub(crate) fn sections(rack: &[Option<ir::Effect>]) -> Vec<ir::EffectKind> {
    rack.iter()
        .flatten()
        .filter(|e| !e.bypass)
        .filter_map(|e| match e.kind {
            ir::EffectKind::Eq(bands) => bands.iter().any(|b| b.gain_db != 0.0).then_some(e.kind),
            kind @ ir::EffectKind::Filter(_) => Some(kind),
            _ => None,
        })
        .collect()
}

/// One EQ band as its biquad at `rate`.
fn band(b: &ir::Bell, rate: u32) -> Biquad {
    match b.shape {
        ir::Shape::Peak => {
            let band = Bell {
                hz: b.hz,
                q: b.q,
                gain_db: b.gain_db,
            };
            kontra_dsp::eq::bell(band, rate)
        }
        ir::Shape::LowShelf => kontra_dsp::eq::shelf(b.hz, b.gain_db, b.q, Side::Low, rate),
        ir::Shape::HighShelf => kontra_dsp::eq::shelf(b.hz, b.gain_db, b.q, Side::High, rate),
    }
}

/// A group's EQ run on one voice: its four bands, moved by their routes.
pub(crate) struct VoiceEq {
    biquads: [Biquad; 4],
}

impl VoiceEq {
    pub(crate) fn new(bells: [ir::Bell; 4], rate: u32) -> Self {
        Self {
            biquads: bells.map(|b| band(&b, rate)),
        }
    }

    /// Moves the bands' coefficients and keeps their state.
    pub(crate) fn retune(&mut self, bells: [ir::Bell; 4], rate: u32) {
        for (f, b) in self.biquads.iter_mut().zip(&bells) {
            f.retune(&band(b, rate));
        }
    }

    pub(crate) fn process(&mut self, io: &mut [[f32; 2]]) {
        run(&mut self.biquads, io);
    }
}

/// Moves a voice's filter and EQ to this note's settings at its age: the
/// filter's cutoff and damping, the EQ's band gains. Coefficients only.
pub(crate) fn retune(
    slot: &mut crate::Slot,
    tuned: Option<(f32, [f32; 3])>,
    bells: Option<[ir::Bell; 4]>,
    rate: u32,
) {
    if let (Some(f), Some((hz, damping))) = (slot.filter.as_mut(), tuned) {
        f.set_cutoff(hz, rate);
        f.set_damping(damping);
    }
    if let (Some(eq), Some(bells)) = (slot.eq.as_mut(), bells) {
        eq.retune(bells, rate);
    }
}

pub(crate) fn biquads(sections: &[ir::EffectKind], rate: u32) -> Box<[Biquad]> {
    let mut out = Vec::new();
    for section in sections {
        match section {
            ir::EffectKind::Eq(bands) => {
                out.extend(
                    bands
                        .iter()
                        .filter(|b| b.gain_db != 0.0)
                        .map(|b| band(b, rate)),
                );
            }
            ir::EffectKind::Filter(f) => {
                let shape = shape(f);
                out.extend(kontra_dsp::filter::sections(&shape, rate));
            }
            _ => {}
        }
    }
    out.into()
}

pub(crate) fn shape(f: &ir::Filter) -> Shape {
    let pass = match f.pass {
        ir::Pass::Low => Pass::Low,
        ir::Pass::High => Pass::High,
        ir::Pass::Band => Pass::Band,
        ir::Pass::Notch => Pass::Notch,
    };
    Shape {
        pass,
        hz: f.hz,
        stages: f.stages,
        damping: f.damping,
        gain: f.gain,
    }
}

pub(crate) fn run(filters: &mut [Biquad], io: &mut [[f32; 2]]) {
    for f in filters {
        f.process(io);
    }
}

impl Engine {
    /// The filter of `zone`'s group that runs per voice, with its cutoff in
    /// Hz and damping for this note at `age`.
    pub(crate) fn voice_filter(
        &self,
        zone: &Zone,
        note: &Note,
        age: Age,
    ) -> Option<(ir::Filter, f32, [f32; 3])> {
        let insert = (*self.voice_filters.get(zone.group as usize)?)?;
        let group = self.groups.get(zone.group as usize)?;
        let effect = group.inserts.get(usize::from(insert))?.as_ref()?;
        let ir::EffectKind::Filter(f) = effect.kind else {
            return None;
        };
        let (mut octaves, mut q) = (0.0f32, 0.0f32);
        for m in group.mods.iter().filter(|m| !m.bypass) {
            match m.kind {
                ir::ModKind::Cutoff {
                    source,
                    insert: i,
                    depth,
                } if i == insert => {
                    octaves += self.source(zone, source, note, age).unwrap_or(0.0) * depth;
                }
                ir::ModKind::Resonance {
                    source,
                    insert: i,
                    depth,
                } if i == insert => {
                    q += self.source(zone, source, note, age).unwrap_or(0.0) * depth;
                }
                _ => {}
            }
        }
        let hz = f.hz * (f.octaves * octaves).exp2();
        let damping = f
            .damping
            .map(|k| (k * (-f.q_octaves * q).exp2()).max(MIN_DAMPING));
        Some((f, hz, damping))
    }

    /// The per-voice filter and EQ a note starts with.
    pub(crate) fn voice_fx(
        &self,
        zone: &Zone,
        note: &Note,
        age: Age,
    ) -> (Option<Svf>, Option<VoiceEq>) {
        let rate = self.rate;
        let filter = self.voice_filter(zone, note, age).map(|(f, hz, damping)| {
            let mut svf = Svf::new(shape(&f));
            svf.set_cutoff(hz, rate);
            svf.set_damping(damping);
            svf
        });
        let eq = self
            .voice_bells(zone, note, age)
            .map(|bells| VoiceEq::new(bells, rate));
        (filter, eq)
    }

    /// The EQ bands of `zone`'s group that run per voice, each gain moved by
    /// its routes, for this note at `age`.
    pub(crate) fn voice_bells(&self, zone: &Zone, note: &Note, age: Age) -> Option<[ir::Bell; 4]> {
        let insert = (*self.voice_eqs.get(zone.group as usize)?)?;
        let group = self.groups.get(zone.group as usize)?;
        let effect = group.inserts.get(usize::from(insert))?.as_ref()?;
        let ir::EffectKind::Eq(mut bells) = effect.kind else {
            return None;
        };
        let (mut shift, mut limit) = ([0.0f32; 4], [f32::INFINITY; 4]);
        for m in group.mods.iter().filter(|m| !m.bypass) {
            if let ir::ModKind::EqGain {
                source,
                insert: i,
                band,
                depth_db,
                limit_db,
                ..
            } = m.kind
                && i == insert
            {
                let amount = self.source(zone, source, note, age).unwrap_or(0.0) * depth_db;
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
        Some(bells)
    }

    /// One chain per distinct (sections, bus) the zones' groups carry. A
    /// filter or EQ whose route moves it runs per voice instead.
    pub(crate) fn build_chains(&mut self) {
        self.voice_filters = self
            .groups
            .iter()
            .map(|g| {
                let moved = |n: usize| {
                    g.mods.iter().any(|m| match m.kind {
                        ir::ModKind::Cutoff { insert, .. }
                        | ir::ModKind::Resonance { insert, .. } => usize::from(insert) == n,
                        _ => false,
                    })
                };
                let filter = g.inserts.iter().enumerate().position(|(n, e)| {
                    e.is_some_and(|e| matches!(e.kind, ir::EffectKind::Filter(_)) && !e.bypass)
                        && moved(n)
                })?;
                u8::try_from(filter).ok()
            })
            .collect();
        self.voice_eqs = self
            .groups
            .iter()
            .map(|g| {
                let moved = |n: usize| {
                    g.mods.iter().any(|m| {
                        matches!(m.kind, ir::ModKind::EqGain { insert, .. } if usize::from(insert) == n)
                    })
                };
                let eq = g.inserts.iter().enumerate().position(|(n, e)| {
                    e.is_some_and(|e| matches!(e.kind, ir::EffectKind::Eq(_)) && !e.bypass)
                        && moved(n)
                })?;
                u8::try_from(eq).ok()
            })
            .collect();
        let shared: Vec<Vec<ir::EffectKind>> = self
            .groups
            .iter()
            .zip(self.voice_filters.iter().zip(&self.voice_eqs))
            .map(|(g, (own, voice_eq))| {
                let mut inserts = g.inserts.clone();
                for n in [*own, *voice_eq].into_iter().flatten() {
                    if let Some(slot) = inserts.get_mut(usize::from(n)) {
                        *slot = None;
                    }
                }
                sections(&inserts)
            })
            .collect();
        let mut chains: Vec<Chain> = Vec::new();
        let rate = self.rate;
        self.zone_chain = self
            .plan
            .zones
            .iter()
            .map(|z| {
                let sections = shared.get(z.group as usize)?.clone();
                if sections.is_empty() {
                    return None;
                }
                let at = chains
                    .iter()
                    .position(|c| c.bus == z.bus && c.sections == sections)
                    .unwrap_or_else(|| {
                        chains.push(Chain {
                            bus: z.bus,
                            filters: biquads(&sections, rate),
                            sections,
                            mix: vec![[0.0; 2]; MAX_SPAN as usize].into(),
                            live: false,
                        });
                        chains.len() - 1
                    });
                u16::try_from(at).ok()
            })
            .collect();
        self.chains = chains;
    }

    /// Renders every voice into its chain or bus mix, then the chains.
    pub(crate) fn render_span(&mut self, len: usize) {
        if self.now.0.is_multiple_of(MAX_SPAN) {
            self.refresh_timed();
            self.rearm_timers();
        }
        for bus in &mut self.buses {
            if let Some(mix) = bus.mix.get_mut(..len) {
                mix.fill([0.0; 2]);
            }
        }
        for slot in self.voices.iter_mut().filter(|s| s.voice.is_active()) {
            let bus = self.plan.zones.get(slot.zone).map_or(0, |z| z.bus);
            let chain = self.zone_chain.get(slot.zone).copied().flatten();
            let mix = match chain.and_then(|c| self.chains.get_mut(usize::from(c))) {
                Some(chain) => {
                    chain.live = true;
                    chain.mix.get_mut(..len)
                }
                None => self.buses.get_mut(bus).and_then(|b| b.mix.get_mut(..len)),
            };
            let Some(mix) = mix else {
                continue;
            };
            let before = slot.voice.missing_frames();
            match self.scratch.get_mut(..len) {
                Some(scratch) if slot.filter.is_some() || slot.eq.is_some() => {
                    scratch.fill([0.0; 2]);
                    slot.voice.render(&self.stream, self.now.0, scratch);
                    if let Some(filter) = slot.filter.as_mut() {
                        filter.process(scratch);
                    }
                    if let Some(eq) = slot.eq.as_mut() {
                        eq.process(scratch);
                    }
                    for (m, x) in mix.iter_mut().zip(scratch.iter()) {
                        m[0] += x[0];
                        m[1] += x[1];
                    }
                }
                _ => slot.voice.render(&self.stream, self.now.0, mix),
            }
            self.stats.missing_frames += slot.voice.missing_frames() - before;
        }
        self.mix_chains(len);
    }

    /// Filters each chain that sounded or still rings, into its bus.
    pub(crate) fn mix_chains(&mut self, len: usize) {
        for chain in &mut self.chains {
            if !chain.live && chain.filters.iter().all(Biquad::is_silent) {
                continue;
            }
            chain.live = false;
            let Some(mix) = chain.mix.get_mut(..len) else {
                continue;
            };
            run(&mut chain.filters, mix);
            if let Some(out) = self
                .buses
                .get_mut(chain.bus)
                .and_then(|b| b.mix.get_mut(..len))
            {
                for (o, m) in out.iter_mut().zip(mix.iter()) {
                    o[0] += m[0];
                    o[1] += m[1];
                }
            }
            mix.fill([0.0; 2]);
        }
    }
}
