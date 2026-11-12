//! What the parts' scripts ask the control side to build, off the audio
//! thread: effect units, and plans with their zone edits applied.

use std::sync::Arc;

use kontra_engine::{Engine, Prepared, ZoneEdit};
use kontra_protocol::Ring;
use kontra_stream::StreamClient;

use super::{Command, Plan, Rack};

/// A part's plan with its scripts' zone edits applied, each with whether it
/// took. It comes back from the audio side holding the plan it replaced.
pub struct ZoneSwap {
    pub plan: Arc<Plan>,
    pub edits: Box<[(ZoneEdit, bool)]>,
}

/// A part's (index, load) edits since the last tick, each with whether it took.
type Batch = (usize, u64, Vec<(ZoneEdit, bool)>);

/// The zone-edit channels between the rack and its audio side.
pub struct ZoneRings {
    /// Zone edits the parts' scripts made: (part, its load, edit).
    pub edits: Ring<(usize, u64, ZoneEdit)>,
    /// Swaps done or refused, to free here.
    pub done: Ring<Box<ZoneSwap>>,
}

impl ZoneRings {
    pub fn new(parts: usize) -> Self {
        Self {
            // A callback's edits arrive in one block: an engine queues 1024.
            edits: Ring::new(1024 * parts),
            done: Ring::new(4 * parts),
        }
    }
}

impl Rack {
    /// Builds what scripts asked for since the last tick and sends it in.
    pub(super) fn script_requests(&mut self) {
        self.effect_requests();
        self.zone_edits();
    }

    /// Effect types scripts loaded.
    fn effect_requests(&mut self) {
        while let Some((part, generation, request)) = self.channels.requests.recv() {
            let Some(Some(p)) = self.parts.get_mut(part) else {
                continue;
            };
            if p.script.generation != generation {
                continue;
            }
            let here = (request.at, request.slot);
            let (asked, ty) = (request.response.is_some(), request.ty);
            let unit = Prepared::new(request, p.plan.as_deref(), self.rate);
            // A response that fails to decode leaves the slot playing what it
            // had, so the save keeps what it had too.
            if let Some(name) = unit.response_name() {
                p.slot_responses.retain(|&(at, slot, _)| (at, slot) != here);
                p.slot_responses.push((here.0, here.1, name.to_owned()));
            } else if let Some(ty) = ty.filter(|_| !asked) {
                // None: the slot's own effect built again (a reverb's
                // network), which saves no type.
                p.slot_types.retain(|&(at, slot, _)| (at, slot) != here);
                p.slot_types.push((here.0, here.1, ty));
                p.slot_responses.retain(|&(at, slot, _)| (at, slot) != here);
            }
            let unit = Box::new(Some(unit));
            self.send(Command::Effect {
                part,
                generation,
                unit,
            });
        }
    }

    /// Each part's zone edits since the last tick, applied together to a
    /// copy of its plan (KSP manual: a callback's `set_zone_par` calls run
    /// together after it), which becomes the part's plan.
    fn zone_edits(&mut self) {
        while self.channels.zones.done.recv().is_some() {}
        let mut batches: Vec<Batch> = Vec::new();
        while let Some((part, generation, edit)) = self.channels.zones.edits.recv() {
            match batches
                .iter_mut()
                .find(|b| (b.0, b.1) == (part, generation))
            {
                Some(b) => b.2.push((edit, false)),
                None => batches.push((part, generation, vec![(edit, false)])),
            }
        }
        for (part, generation, mut edits) in batches {
            let Some(Some(p)) = self.parts.get_mut(part) else {
                continue;
            };
            let base = p
                .plan
                .as_deref()
                .filter(|_| p.script.generation == generation);
            let Some(mut plan) = base.cloned() else {
                continue;
            };
            for (e, took) in &mut edits {
                *took = plan.edit_zone(e.zone, e.par, e.value);
            }
            // A zone moved into a wavetable group plays its table.
            let plan =
                Arc::new(kontra_prepare::tables(&plan, &self.store.client()).unwrap_or(plan));
            p.plan = Some(Arc::clone(&plan));
            let swap = Box::new(ZoneSwap {
                plan,
                edits: edits.into(),
            });
            self.send(Command::Zones {
                part,
                generation,
                swap,
            });
        }
    }
}

/// Gives the zones `on init` moved into a wavetable group their tables,
/// before the engine plays.
pub(super) fn init_tables(engine: &mut Engine, client: &StreamClient) {
    if let Some(plan) = kontra_prepare::tables(engine.plan(), client) {
        engine.swap_plan(&mut Arc::new(plan), &[]);
    }
}
