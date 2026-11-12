//! `set_zone_par`: a script moves a zone to another group, key or velocity
//! range. KSP manual: in `on init` at once (answers -1); elsewhere an async
//! id, the edits of a callback applied together after it, then `on
//! async_complete`. The host applies them to a copy of the plan off the
//! audio thread; [`Engine::swap_plan`] puts it in between blocks.

use std::sync::Arc;

use kontra_ir::ZonePar;

use crate::queue::Exit;
use crate::script::Kind;
use crate::{Action, Engine, Plan};

/// Edits waiting for the host, at most (Conflux sets three per wavetable).
pub(crate) const ZONE_EDITS: usize = 1024;

/// One `set_zone_par` waiting for the host.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneEdit {
    pub zone: u32,
    pub par: ZonePar,
    pub value: i32,
    pub script: u8,
    pub id: i32,
}

impl Engine {
    pub(crate) fn set_zone_par(&mut self, zone: u32, par: ZonePar, value: i32) -> i32 {
        let carried = matches!(
            par,
            ZonePar::Group
                | ZonePar::LowKey
                | ZonePar::HighKey
                | ZonePar::LowVelocity
                | ZonePar::HighVelocity
        );
        if !carried {
            self.count_unhandled(Action::SetZonePar);
        }
        if self.running.kind == Kind::Init {
            // Init runs before the engine plays, on the control thread. A
            // zone the instrument lacks is the script's no-op, not a gap.
            if carried {
                Arc::make_mut(&mut self.plan).edit_zone(zone, par, value);
            }
            return -1;
        }
        if !carried {
            return self.start_async(Exit::Done);
        }
        let script = self.running.slot;
        let now = self.plan.zone_facts.iter().find(|z| z.id == zone);
        let pending = self
            .zone_edits
            .iter()
            .any(|e| (e.zone, e.par) == (zone, par));
        // Conflux sets every zone each time; only changes go to the host.
        if now.map(|z| z.get(par)) == Some(value) && !pending {
            return self.start_async(Exit::Done);
        }
        if self.zone_edits.len() >= ZONE_EDITS {
            self.refuse_zone_edit(script);
            return self.start_async(Exit::Failed);
        }
        let id = self.next_async_id();
        let edit = ZoneEdit {
            zone,
            par,
            value,
            script,
            id,
        };
        self.zone_edits.push(edit);
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
        let same = plan.zones.len() == self.plan.zones.len();
        if same {
            let fade = self.fade_frames();
            let group = |p: &Plan, zone: usize| p.zones.get(zone).map(|z| z.group);
            for i in 0..self.voices.len() {
                let moved = self.voices.get(i).is_some_and(|s| {
                    s.voice.is_active() && group(&self.plan, s.zone) != group(plan, s.zone)
                });
                if moved {
                    self.steal(i, fade);
                }
            }
            std::mem::swap(&mut self.plan, plan);
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
