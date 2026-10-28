//! EQ bands and filters on summed audio. A section with fixed settings is
//! linear and time invariant, so it commutes with each voice's gain matrix:
//! voices whose groups carry the same sections on the same bus share one
//! chain, run once per span instead of once per voice. Bus and instrument
//! racks filter their own sums.
//!
//! ponytail: settings are fixed at load (after the init callback). Live
//! changes (scripts, per-voice cutoff modulation) need per-voice filters.

use kontra_dsp::eq::{Bell, Biquad};
use kontra_dsp::filter::{Pass, Shape};
use kontra_ir as ir;

use crate::ledger::Note;
use crate::modulators::Age;
use crate::{Engine, MAX_SPAN, Zone};

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

pub(crate) fn biquads(sections: &[ir::EffectKind], rate: u32) -> Box<[Biquad]> {
    let mut out = Vec::new();
    for section in sections {
        match section {
            ir::EffectKind::Eq(bands) => {
                let audible = bands.iter().filter(|b| b.gain_db != 0.0);
                out.extend(audible.map(|b| {
                    let band = Bell {
                        hz: b.hz,
                        q: b.q,
                        gain_db: b.gain_db,
                    };
                    kontra_dsp::eq::bell(band, rate)
                }));
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
    /// The filter of `zone`'s group that runs per voice, and its cutoff in Hz
    /// for this note at `age`.
    pub(crate) fn cutoff(&self, zone: &Zone, note: &Note, age: Age) -> Option<(ir::Filter, f32)> {
        let insert = (*self.voice_filters.get(zone.group as usize)?)?;
        let group = self.groups.get(zone.group as usize)?;
        let effect = group.inserts.get(usize::from(insert))?.as_ref()?;
        let ir::EffectKind::Filter(f) = effect.kind else {
            return None;
        };
        let octaves: f32 = group
            .mods
            .iter()
            .filter_map(|m| match m.kind {
                ir::ModKind::Cutoff {
                    source,
                    insert: i,
                    depth,
                } if i == insert => Some(self.source(zone, source, note, age)? * depth),
                _ => None,
            })
            .sum();
        Some((f, f.hz * (f.octaves * octaves).exp2()))
    }

    /// One chain per distinct (sections, bus) the zones' groups carry. A
    /// filter whose cutoff a route moves runs per voice instead.
    pub(crate) fn build_chains(&mut self) {
        self.voice_filters = self
            .groups
            .iter()
            .map(|g| {
                let moved = |n: usize| {
                    g.mods.iter().any(|m| {
                        matches!(m.kind, ir::ModKind::Cutoff { insert, .. } if usize::from(insert) == n)
                    })
                };
                let filter = g.inserts.iter().enumerate().position(|(n, e)| {
                    e.is_some_and(|e| matches!(e.kind, ir::EffectKind::Filter(_)) && !e.bypass)
                        && moved(n)
                })?;
                u8::try_from(filter).ok()
            })
            .collect();
        let shared: Vec<Vec<ir::EffectKind>> = self
            .groups
            .iter()
            .zip(self.voice_filters.iter())
            .map(|(g, own)| {
                let mut inserts = g.inserts.clone();
                if let Some(slot) = own.and_then(|n| inserts.get_mut(usize::from(n))) {
                    *slot = None;
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
