//! A shared chain of fixed filters on summed voices, or a group's own chain
//! with the rack its sum runs (the engine fills and runs them: filters.rs).

use kontra_effects::MAX_SPAN;
use kontra_effects::summed::{Stage, stages};
use kontra_ir as ir;

use crate::rack::Rack;

pub struct Chain {
    pub bus: usize,
    pub sections: Vec<ir::EffectKind>,
    pub filters: Box<[Stage]>,
    /// A group's own chain, never shared: its rack runs the slots its
    /// voices leave to the sum (group_racks.rs).
    pub group: Option<(usize, Rack)>,
    pub mix: Box<[[f32; 2]]>,
    pub live: bool,
}

impl Chain {
    /// The group whose own chain this is, if any.
    pub fn owner(&self) -> Option<usize> {
        self.group.as_ref().map(|(g, _)| *g)
    }

    pub fn new(
        bus: usize,
        sections: Vec<ir::EffectKind>,
        group: Option<(usize, Rack)>,
        rate: u32,
    ) -> Self {
        Self {
            bus,
            filters: stages(&sections, rate),
            sections,
            group,
            mix: vec![[0.0; 2]; MAX_SPAN as usize].into(),
            live: false,
        }
    }
}

/// Chain `c`, or its twin on `bus` when a script moved the group there.
pub fn on_bus(chains: &[Chain], c: usize, bus: usize) -> Option<usize> {
    let chain = chains.get(c)?;
    if chain.bus == bus {
        return Some(c);
    }
    chains
        .iter()
        .position(|t| t.bus == bus && t.sections == chain.sections && t.owner() == chain.owner())
}
