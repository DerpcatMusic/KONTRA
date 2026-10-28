//! EQ bands and filters on summed audio. A section with fixed settings is
//! linear and time invariant, so it commutes with each voice's gain matrix:
//! voices whose groups carry the same sections on the same bus share one
//! chain, run once per span instead of once per voice. Bus and instrument
//! racks filter their own sums.
//!
//! ponytail: settings are fixed at load (after the init callback). Inserts
//! a route moves, and those up to a group's last drive effect, run on each
//! voice instead (voice_rack.rs).

use kontra_dsp::eq::{Bell, Biquad, Side};
use kontra_dsp::filter::{Formant, Pass, Shape, VowelSet, Vowels};
use kontra_ir as ir;

use crate::{Engine, MAX_SPAN};

pub(crate) struct Chain {
    bus: usize,
    sections: Vec<ir::EffectKind>,
    filters: Box<[Stage]>,
    pub(crate) mix: Box<[[f32; 2]]>,
    pub(crate) live: bool,
}

impl Chain {
    fn new(bus: usize, sections: Vec<ir::EffectKind>, rate: u32) -> Self {
        Self {
            bus,
            filters: stages(&sections, rate),
            sections,
            mix: vec![[0.0; 2]; MAX_SPAN as usize].into(),
            live: false,
        }
    }
}

/// Chain `c`, or its twin on `bus` when a script moved the group there.
fn on_bus(chains: &[Chain], c: usize, bus: usize) -> Option<usize> {
    let chain = chains.get(c)?;
    if chain.bus == bus {
        return Some(c);
    }
    chains
        .iter()
        .position(|t| t.bus == bus && t.sections == chain.sections)
}

/// The rack's active EQs (their audible bands) and filters, in slot order.
pub(crate) fn sections(rack: &[Option<ir::Effect>]) -> Vec<ir::EffectKind> {
    rack.iter()
        .flatten()
        .filter(|e| !e.bypass)
        .filter_map(|e| match e.kind {
            ir::EffectKind::Eq(bands) => bands.iter().any(|b| b.gain_db != 0.0).then_some(e.kind),
            kind @ (ir::EffectKind::Filter(_) | ir::EffectKind::Formant(_)) => Some(kind),
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
#[derive(Clone)]
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
        for f in &mut self.biquads {
            f.process(io);
        }
    }
}

/// A section's runner: biquads in series, or a formant's parallel bands.
pub(crate) enum Stage {
    Biquad(Biquad),
    Formant(Formant),
}

impl Stage {
    fn is_silent(&self) -> bool {
        match self {
            Self::Biquad(b) => b.is_silent(),
            Self::Formant(f) => f.is_silent(),
        }
    }
}

pub(crate) fn stages(sections: &[ir::EffectKind], rate: u32) -> Box<[Stage]> {
    let mut out = Vec::new();
    for section in sections {
        match section {
            ir::EffectKind::Eq(bands) => {
                out.extend(
                    bands
                        .iter()
                        .filter(|b| b.gain_db != 0.0)
                        .map(|b| Stage::Biquad(band(b, rate))),
                );
            }
            ir::EffectKind::Filter(f) => {
                let shape = shape(f);
                out.extend(
                    kontra_dsp::filter::sections(&shape, rate)
                        .into_iter()
                        .map(Stage::Biquad),
                );
            }
            ir::EffectKind::Formant(f) => {
                let v = Vowels {
                    talk: f.talk,
                    sharp: f.sharp,
                    size: f.size,
                    set: match f.set {
                        ir::VowelSet::First => VowelSet::First,
                        ir::VowelSet::Second => VowelSet::Second,
                    },
                };
                out.push(Stage::Formant(Formant::new(v, rate)));
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

pub(crate) fn run(stages: &mut [Stage], io: &mut [[f32; 2]]) {
    for stage in stages {
        match stage {
            Stage::Biquad(b) => b.process(io),
            Stage::Formant(f) => f.process(io),
        }
    }
}

impl Engine {
    /// One chain per distinct (sections, bus) the zones' groups carry, less
    /// the inserts that run per voice.
    pub(crate) fn build_chains(&mut self) {
        self.build_voice_racks();
        let shared: Vec<Vec<ir::EffectKind>> = self
            .groups
            .iter()
            .zip(&self.voice_units)
            .map(|(g, units)| {
                let mut inserts = g.inserts.clone();
                for u in units.iter() {
                    if let Some(slot) = inserts.get_mut(usize::from(u.insert)) {
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
                let bus = self.groups.get(z.group as usize).map_or(0, |g| g.bus);
                let at = chains
                    .iter()
                    .position(|c| c.bus == bus && c.sections == sections)
                    .unwrap_or_else(|| {
                        chains.push(Chain::new(bus, sections, rate));
                        chains.len() - 1
                    });
                u16::try_from(at).ok()
            })
            .collect();
        // A twin of each chain on every other bus, so a script that moves a
        // group to another bus allocates nothing on the audio thread.
        for n in 0..chains.len() {
            let Some(sections) = chains.get(n).map(|c| c.sections.clone()) else {
                continue;
            };
            for bus in 0..self.buses.len() {
                if !chains
                    .iter()
                    .any(|c| c.bus == bus && c.sections == sections)
                {
                    chains.push(Chain::new(bus, sections.clone(), rate));
                }
            }
        }
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
            let group = self.plan.zones.get(slot.zone).map(|z| z.group as usize);
            let bus = group.and_then(|g| self.groups.get(g)).map_or(0, |g| g.bus);
            let chain = self.zone_chain.get(slot.zone).copied().flatten();
            let chain = chain.and_then(|c| on_bus(&self.chains, usize::from(c), bus));
            let mix = match chain.and_then(|c| self.chains.get_mut(c)) {
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
            if slot.rack.is_empty() {
                slot.voice.render(&self.stream, self.now.0, mix);
            } else {
                let group = self.plan.zones.get(slot.zone).map(|z| z.group as usize);
                let inserts = group
                    .and_then(|g| self.groups.get(g))
                    .map(|g| &g.inserts[..])
                    .unwrap_or_default();
                let (rack, spare) = (&mut slot.rack, &mut *self.scratch);
                slot.voice
                    .render_through(&self.stream, self.now.0, mix, |io| {
                        crate::voice_rack::process(rack, inserts, io, spare);
                    });
            }
            self.stats.missing_frames += slot.voice.missing_frames() - before;
        }
        self.mix_chains(len);
    }

    /// Filters each chain that sounded or still rings, into its bus.
    pub(crate) fn mix_chains(&mut self, len: usize) {
        for chain in &mut self.chains {
            if !chain.live && chain.filters.iter().all(Stage::is_silent) {
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
