//! `set_zone_par`: a script moves a zone to another group, key or velocity
//! range. KSP manual: in `on init` at once (answers -1); elsewhere an async
//! id, the edits of a callback applied together after it, then `on
//! async_complete`. The host applies them to a copy of the plan off the
//! audio thread; [`Engine::swap_plan`] puts it in between blocks.

use std::sync::Arc;

use kontra_ir::ZonePar;

use crate::queue::Exit;
use kontra_vm::Text;

use crate::{Action, Engine, Plan};

/// Edits waiting for the host, at most (Conflux sets 297 per selection).
pub(crate) const ZONE_EDITS: usize = 1024;

/// What a waiting zone edit does.
// The path is copied inline: the audio thread queues it and may not allocate.
#[expect(
    clippy::large_enum_variant,
    reason = "queued on the audio thread: no Box"
)]
#[derive(Clone, Copy, Debug)]
pub enum Edit {
    /// `set_zone_par`.
    Par(ZonePar, i32),
    /// `set_sample` on a user zone: the file to load.
    Sample(Text),
    /// `set_loop_par` on a user zone: loop index, parameter, value.
    Loop(i32, i32, i32),
}

/// One `set_zone_par`, `set_sample` or `set_loop_par` waiting for the host.
#[derive(Clone, Copy, Debug)]
pub struct ZoneEdit {
    pub zone: u32,
    pub what: Edit,
    pub script: u8,
    pub id: i32,
}

impl ZoneEdit {
    /// Applies the edit to `plan`; whether it took. A sample's file is read
    /// here (its header), so call it off the audio thread. The user zone's
    /// playing zone is built after, by `kontra_prepare::user_zones`.
    pub fn apply(&self, plan: &mut Plan) -> bool {
        match self.what {
            Edit::Par(par, value) => plan.edit_zone(self.zone, par, value),
            Edit::Loop(index, par, value) => plan.set_user_loop(self.zone, index, par, value),
            Edit::Sample(path) => describe(path.as_str())
                .is_some_and(|s| plan.set_user_sample(self.zone, Arc::new(s))),
        }
    }
}

/// The sample file `path` names, from its header.
pub(crate) fn describe(path: &str) -> Option<crate::ir::Sample> {
    kontra_stream::describe(std::path::Path::new(path)).ok()
}

/// A `purge_group(group, 1)` of a group nothing of is loaded, waiting for the
/// host to load it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unpurge {
    pub group: u32,
    /// The script and async id waiting on the load; none for one asked for
    /// while the engine was built (a snapshot picking its sounds).
    pub waiter: Option<(u8, i32)>,
}

impl Engine {
    /// Un-purges `group`: at once when it is loaded (KSP `purge_group`);
    /// when it is dormant ([`Plan::dormant`]) the host loads it and the
    /// async id completes then, so the first note after it plays the group.
    pub(crate) fn unpurge(&mut self, group: u32) -> i32 {
        let dormant = self.plan.dormant.iter().any(|d| d.group == group);
        if !dormant || self.building {
            if let Some(g) = self.groups.get_mut(group as usize) {
                g.purged = false;
            }
            // Built from `on init` or a restore (a snapshot picks its
            // sounds there): nothing of a dormant group is loaded yet, so
            // the host still loads it, once; no script waits on that.
            let asked = self.unpurges.iter().any(|u| u.group == group);
            if dormant && !asked {
                self.unpurges.push(Unpurge {
                    group,
                    waiter: None,
                });
            }
            return self.start_async(Exit::Done);
        }
        if self.unpurges.len() >= 16 {
            return self.start_async(Exit::Failed);
        }
        let id = self.next_async_id();
        let waiter = Some((self.running.slot, id));
        self.unpurges.push(Unpurge { group, waiter });
        id
    }

    /// Purges `group` (KSP `purge_group(g, 0)`). While the engine is built
    /// a load asked for earlier goes too: the script's last word stands.
    pub(crate) fn purge(&mut self, group: u32) -> i32 {
        if let Some(g) = self.groups.get_mut(group as usize) {
            g.purged = true;
        }
        if self.building {
            self.unpurges
                .retain(|u| u.group != group || u.waiter.is_some());
        }
        self.start_async(Exit::Done)
    }

    /// Hands each waiting un-purge to `f`; one it cannot take fails its
    /// async, or, when no script waits on it, is offered again next block.
    /// Audio thread, between blocks.
    pub fn drain_unpurges(&mut self, mut f: impl FnMut(Unpurge) -> bool) {
        let mut waiting = std::mem::take(&mut self.unpurges);
        waiting.retain(|&u| {
            if f(u) {
                return false;
            }
            match u.waiter {
                None => true,
                Some((script, id)) => {
                    self.queue_async(script, id, Exit::Failed);
                    false
                }
            }
        });
        self.unpurges = waiting;
    }

    /// The host loaded `group` (or could not): it plays from the next note
    /// and the script's async completes. A load no script waited on leaves
    /// the purge flag as the script last set it.
    pub fn unpurged(&mut self, u: Unpurge, took: bool) {
        let Some((script, id)) = u.waiter else {
            return;
        };
        if took && let Some(g) = self.groups.get_mut(u.group as usize) {
            g.purged = false;
        }
        self.queue_async(script, id, Exit::from(took));
    }

    pub(crate) fn set_zone_par(&mut self, zone: u32, par: ZonePar, value: i32) -> i32 {
        // Not settable: where a sample plays at, its tempo, the editor's selection.
        let carried = !matches!(
            par,
            ZonePar::StartRange | ZonePar::SampleRate | ZonePar::Selected | ZonePar::Bpm
        );
        if !carried {
            self.count_unhandled(Action::SetZonePar);
        }
        if self.building {
            // The engine under construction (`on init`, its first
            // `persistence_changed`) runs on the control thread, before it
            // plays: the edit lands at once. A zone the instrument lacks is
            // the script's no-op, not a gap. Every other run queues, a
            // recall's too (it runs on the audio thread).
            if carried {
                Arc::make_mut(&mut self.plan).edit_zone(zone, par, value);
            }
            return -1;
        }
        if !carried {
            return self.start_async(Exit::Done);
        }
        self.queue_zone_edit(zone, Edit::Par(par, value))
    }

    /// Queues `what` for the host: its async id. Every carried edit queues,
    /// unchanged ones too: the plan here lags the edits awaiting a swap, so
    /// comparing with it could drop the later of two edits that undo each
    /// other.
    pub(crate) fn queue_zone_edit(&mut self, zone: u32, what: Edit) -> i32 {
        let script = self.running.slot;
        if self.zone_edits.len() >= ZONE_EDITS {
            self.refuse_zone_edit(script);
            return self.start_async(Exit::Failed);
        }
        let id = self.next_async_id();
        self.zone_edits.push(ZoneEdit {
            zone,
            what,
            script,
            id,
        });
        id
    }

    /// Hands each queued edit to `f`; one it cannot take fails its async.
    /// Audio thread, between blocks.
    pub fn drain_zone_edits(&mut self, mut f: impl FnMut(ZoneEdit) -> bool) {
        let mut n = 0;
        while let Some(&e) = self.zone_edits.get(n) {
            if !f(e) {
                self.refuse_zone_edit(e.script);
                self.queue_async(e.script, e.id, Exit::Failed);
            }
            n += 1;
        }
        self.zone_edits.clear();
    }

    /// Swaps `plan` (this plan with `edits` applied, each with whether it
    /// took) in, leaving the old one in `plan`, and completes their async
    /// ids. Kontakt 8.13.1 (render rig, probes/zonepar): a sounding voice
    /// whose zone changes group stops, a short fade; one whose keys change
    /// plays on. A plan whose zones differ in number is refused and stays in
    /// `plan`. No allocation.
    pub fn swap_plan(&mut self, plan: &mut Arc<Plan>, edits: &[(ZoneEdit, bool)]) {
        // Zones only ever join at the end (a group loaded), so every index a
        // voice holds still names its zone.
        let same = plan.zones.len() >= self.plan.zones.len();
        if same {
            let fade = self.fade_frames();
            // A voice whose zone changed group, or got another sample
            // (`set_sample` on a user zone), stops.
            let place = |p: &Plan, zone: usize| {
                p.zones
                    .get(zone)
                    .map(|z| (z.group, Arc::as_ptr(&z.region.sample)))
            };
            for i in 0..self.voices.len() {
                let moved = self.voices.get(i).is_some_and(|s| {
                    s.voice.is_active() && place(&self.plan, s.zone) != place(plan, s.zone)
                });
                if moved {
                    self.steal(i, fade);
                }
            }
            std::mem::swap(&mut self.plan, plan);
            Arc::make_mut(&mut self.zone_index).refresh(&self.plan.zones);
            self.release_zones = self
                .plan
                .zones
                .iter()
                .any(|z| z.trigger == crate::Trigger::Release);
        }
        for &(e, took) in edits {
            self.queue_async(e.script, e.id, Exit::from(same && took));
        }
    }

    /// The plan the engine plays: the host's, with any `on init` zone edits.
    #[must_use]
    pub fn plan(&self) -> &Arc<Plan> {
        &self.plan
    }

    /// Per script slot, the zone edits refused because the queue was full.
    #[must_use]
    pub fn zone_edits_refused(&self) -> &[u32] {
        &self.zone_refused
    }

    /// A full queue fails the edit's async and counts it, with the refused
    /// events and per script ([`Self::zone_edits_refused`]).
    fn refuse_zone_edit(&mut self, script: u8) {
        self.stats.events_refused += 1;
        if let Some(n) = self.zone_refused.get_mut(usize::from(script)) {
            *n = n.saturating_add(1);
        }
    }
}
