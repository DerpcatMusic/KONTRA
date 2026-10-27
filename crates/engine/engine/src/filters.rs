//! EQ bands on summed audio. A band with fixed settings is linear and time
//! invariant, so it commutes with each voice's gain matrix: voices whose
//! groups carry the same bands on the same bus share one filter chain, run
//! once per span instead of once per voice. Bus and instrument racks filter
//! their own sums.
//!
//! ponytail: settings are fixed at load (after the init callback). Live band
//! changes (scripts, per-voice modulation) need per-voice filters.

use kontra_dsp::eq::{Bell, Biquad};
use kontra_ir as ir;

use crate::{Engine, MAX_SPAN};

pub(crate) struct Chain {
    bus: usize,
    bells: Vec<ir::Bell>,
    filters: Box<[Biquad]>,
    pub(crate) mix: Box<[[f32; 2]]>,
    pub(crate) live: bool,
}

/// The active bands of a rack, in slot order.
pub(crate) fn bells(rack: &[Option<ir::Effect>]) -> Vec<ir::Bell> {
    rack.iter()
        .flatten()
        .filter(|e| !e.bypass)
        .filter_map(|e| match e.kind {
            ir::EffectKind::Eq(bands) => Some(bands),
            _ => None,
        })
        .flatten()
        .filter(|b| b.gain_db != 0.0)
        .collect()
}

pub(crate) fn biquads(bells: &[ir::Bell], rate: u32) -> Box<[Biquad]> {
    bells
        .iter()
        .map(|b| {
            let bell = Bell {
                hz: b.hz,
                q: b.q,
                gain_db: b.gain_db,
            };
            Biquad::bell(bell, rate)
        })
        .collect()
}

pub(crate) fn run(filters: &mut [Biquad], io: &mut [[f32; 2]]) {
    for f in filters {
        f.process(io);
    }
}

impl Engine {
    /// One chain per distinct (bands, bus) the zones' groups carry.
    pub(crate) fn build_chains(&mut self) {
        let mut chains: Vec<Chain> = Vec::new();
        let rate = self.rate;
        self.zone_chain = self
            .plan
            .zones
            .iter()
            .map(|z| {
                let group = self.groups.get(z.group as usize)?;
                let bells = bells(&group.inserts);
                if bells.is_empty() {
                    return None;
                }
                let at = chains
                    .iter()
                    .position(|c| c.bus == z.bus && c.bells == bells)
                    .unwrap_or_else(|| {
                        chains.push(Chain {
                            bus: z.bus,
                            filters: biquads(&bells, rate),
                            bells,
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
