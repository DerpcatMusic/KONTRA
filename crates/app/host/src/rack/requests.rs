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

/// A purged group whose heads were read.
pub struct Woken {
    pub part: usize,
    pub generation: u64,
    pub unpurge: kontra_engine::Unpurge,
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
        self.unpurges();
    }

    /// Purged groups scripts asked for: their samples load here, the zones
    /// join the part's plan between blocks, and the group plays once its
    /// heads are read (a thread reads them; the audio side never waits).
    fn unpurges(&mut self) {
        while let Some((part, generation, u)) = self.channels.unpurges.recv() {
            let Some(Some(p)) = self.parts.get_mut(part) else {
                continue;
            };
            let Some(plan) = p
                .plan
                .as_deref()
                .filter(|_| p.script.generation == generation)
            else {
                continue;
            };
            let samples: Vec<_> = (plan.dormant.iter())
                .filter(|d| d.group == u.group)
                .flat_map(|d| d.samples.iter())
                .collect();
            let files = kontra_stream::Files::new(samples.iter().map(|s| s.file.as_path()));
            let waked = kontra_prepare::wake(plan, u.group, &mut self.store, &|s| files.open(s));
            let Some((grown, diagnostics, count)) = waked else {
                // Already loaded by an earlier ask: nothing more to read.
                self.send(Command::Unpurged {
                    part,
                    generation,
                    unpurge: u,
                    took: true,
                });
                continue;
            };
            let name = grown
                .groups
                .get(u.group as usize)
                .map_or("", |g| g.name.as_str());
            self.notices.push(format!(
                "part {} ({}): loading purged group {name} ({count} samples)",
                part + 1,
                p.name
            ));
            self.notices.extend(diagnostics.iter().map(|d| {
                format!(
                    "part {} ({}): {}: {}",
                    part + 1,
                    p.name,
                    d.feature,
                    d.detail
                )
            }));
            p.diagnostics.extend(diagnostics);
            let grown = Arc::new(grown);
            p.plan = Some(Arc::clone(&grown));
            let swap = Box::new(ZoneSwap {
                plan: grown,
                edits: Box::default(),
            });
            self.send(Command::Zones {
                part,
                generation,
                swap,
            });
            let done = self.woken.0.clone();
            let heads = self.store.take_heads();
            let lanes = std::thread::available_parallelism().map_or(4, |n| n.get() * 3);
            let read = move || {
                if let Some(heads) = heads {
                    heads.load(lanes);
                }
                let _ = done.send(Woken {
                    part,
                    generation,
                    unpurge: u,
                });
            };
            // A failed spawn leaves the heads to stream on demand: the group
            // still plays.
            if std::thread::Builder::new()
                .name("kontra-unpurge".into())
                .spawn(read)
                .is_err()
            {
                self.send(Command::Unpurged {
                    part,
                    generation,
                    unpurge: u,
                    took: true,
                });
            }
        }
        while let Ok(w) = self.woken.1.try_recv() {
            self.send(Command::Unpurged {
                part: w.part,
                generation: w.generation,
                unpurge: w.unpurge,
                took: true,
            });
        }
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
