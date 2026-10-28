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

use crate::{Engine, MAX_SPAN};

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
                let pass = match f.pass {
                    ir::Pass::Low => Pass::Low,
                    ir::Pass::High => Pass::High,
                    ir::Pass::Band => Pass::Band,
                    ir::Pass::Notch => Pass::Notch,
                };
                let shape = Shape {
                    pass,
                    hz: f.hz,
                    stages: f.stages,
                    damping: f.damping,
                    gain: f.gain,
                };
                out.extend(kontra_dsp::filter::sections(&shape, rate));
            }
            _ => {}
        }
    }
    out.into()
}

pub(crate) fn run(filters: &mut [Biquad], io: &mut [[f32; 2]]) {
    for f in filters {
        f.process(io);
    }
}

impl Engine {
    /// One chain per distinct (sections, bus) the zones' groups carry.
    pub(crate) fn build_chains(&mut self) {
        let mut chains: Vec<Chain> = Vec::new();
        let rate = self.rate;
        self.zone_chain = self
            .plan
            .zones
            .iter()
            .map(|z| {
                let group = self.groups.get(z.group as usize)?;
                let sections = sections(&group.inserts);
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
