//! A shared chain of filters on summed voices, or a group's own chain with
//! the rack its sum runs (the engine fills and runs them: filters.rs).
//!
//! Each summed insert is one stage that follows its settings in place, so a
//! script's write after init reaches the sum: a group's own chain follows it
//! directly, and a group on a shared chain moves to its private one first.

use kontra_effects::MAX_SPAN;
use kontra_effects::summed::{Stage, live_stage, retune, sounds};
use kontra_ir as ir;

use crate::rack::Rack;

pub struct Chain {
    pub bus: usize,
    /// Per stage, the insert slot it runs and the settings it last took.
    /// Chains with equal sections (and owner) are interchangeable.
    pub sections: Vec<(u8, ir::Effect)>,
    pub filters: Box<[Stage]>,
    /// Per stage, whether it is heard ([`sounds`]).
    pub on: Box<[bool]>,
    /// One group's spare chain, never shared and without twins: the group
    /// takes it once a write moves its inserts off its shared chain's, and
    /// the engine points it at the group's bus.
    pub private: bool,
    /// A group's own chain, never shared: its rack runs the slots its
    /// voices leave to the sum (group_racks.rs).
    pub group: Option<(usize, Rack)>,
    pub mix: Box<[[f32; 2]]>,
    pub live: bool,
    /// Its filters still ring: they last ran and were not silent after.
    /// Only running changes them, so a silent chain is known without a look.
    pub ringing: bool,
}

impl Chain {
    /// The group whose own chain this is, if any.
    pub fn owner(&self) -> Option<usize> {
        self.group.as_ref().map(|(g, _)| *g)
    }

    pub fn new(
        bus: usize,
        sections: Vec<(u8, ir::Effect)>,
        group: Option<(usize, Rack)>,
        rate: u32,
    ) -> Self {
        let filters = sections
            .iter()
            .filter_map(|(_, e)| live_stage(&e.kind, rate))
            .collect();
        Self {
            bus,
            filters,
            on: sections.iter().map(|(_, e)| sounds(e)).collect(),
            sections,
            private: false,
            group,
            mix: vec![[0.0; 2]; MAX_SPAN as usize].into(),
            live: false,
            ringing: false,
        }
    }

    /// Runs the heard stages on the first `len` frames of its mix.
    pub fn run(&mut self, len: usize) {
        let Some(io) = self.mix.get_mut(..len) else {
            return;
        };
        for (stage, _) in self.filters.iter_mut().zip(&self.on).filter(|(_, on)| **on) {
            stage.process(io);
        }
        self.ringing = !self.filters.iter().all(Stage::is_silent);
    }

    /// Whether `inserts` hold what this chain runs.
    pub fn matches(&self, inserts: &[Option<ir::Effect>]) -> bool {
        self.sections
            .iter()
            .all(|(n, e)| inserts.get(usize::from(*n)).copied().flatten() == Some(*e))
    }

    /// Follows `inserts`' settings in place, state kept, as a voice's units
    /// do. An insert gone or switched to another type falls silent.
    // ponytail: a type switch mutes the stage rather than rebuilding the
    // chain (that allocates); a script that swaps a summed insert's type after
    // init needs a stage per type kept at load.
    pub fn follow(&mut self, inserts: &[Option<ir::Effect>], rate: u32) {
        let stages = self.filters.iter_mut().zip(self.on.iter_mut());
        for ((n, saved), (stage, on)) in self.sections.iter_mut().zip(stages) {
            let now = inserts.get(usize::from(*n)).copied().flatten();
            if now == Some(*saved) {
                continue;
            }
            match now {
                Some(e) if retune(stage, &e.kind, rate) => {
                    *saved = e;
                    *on = sounds(&e);
                }
                _ => {
                    // Marked bypassed so the insert's return is seen as a move.
                    saved.bypass = true;
                    *on = false;
                }
            }
        }
    }
}

/// Chain `c`, or its twin on `bus` when a script moved the group there. A
/// private chain has no twins: it follows its group's bus itself.
pub fn on_bus(chains: &[Chain], c: usize, bus: usize) -> Option<usize> {
    let chain = chains.get(c)?;
    if chain.bus == bus || chain.private {
        return Some(c);
    }
    chains.iter().position(|t| {
        t.bus == bus && !t.private && t.sections == chain.sections && t.owner() == chain.owner()
    })
}
