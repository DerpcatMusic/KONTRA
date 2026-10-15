//! Finite control-side admission, separate from the live request queue.
use super::Residency;
use crate::{Error, Prepared, Runtime, ScriptInstanceId};

/// A frontend-reserved async identity for an exact physical source group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitialGroupResidency {
    pub source_group: u32,
    pub reload: bool,
    pub async_id: i32,
    pub instance: ScriptInstanceId,
}
impl InitialGroupResidency {
    pub const MAX_REQUESTS: usize = 262_144;
    /// Descriptor/spool/validation metadata, separate from physical PCM leases.
    pub const MAX_METADATA_BYTES: usize = 32 << 20;
}

#[derive(Clone, Copy)]
struct ResultRecord {
    id: i32,
    instance: ScriptInstanceId,
    success: bool,
}
#[derive(Default)]
pub(crate) struct State {
    results: Box<[ResultRecord]>,
    next: usize,
}
impl State {
    pub fn pending(&self) -> usize {
        self.results.len() - self.next
    }
    pub fn cancel(&mut self) -> usize {
        let pending = self.pending();
        self.next = self.results.len();
        pending
    }
}

impl Prepared {
    /// Attach bounded compiler-owned startup work. Runtime construction and
    /// PlanControl submission automatically prepare it on their control owner.
    pub fn with_initial_group_residency(
        mut self,
        requests: Vec<InitialGroupResidency>,
    ) -> Result<Self, Error> {
        self.validate_initial_residency(&requests)?;
        self.initial_residency = requests.into_boxed_slice();
        Ok(self)
    }

    fn validate_initial_residency(&self, requests: &[InitialGroupResidency]) -> Result<(), Error> {
        let bytes = requests
            .len()
            .checked_mul(
                size_of::<InitialGroupResidency>() + size_of::<ResultRecord>() + size_of::<usize>(),
            )
            .and_then(|n| {
                (self.group_count as usize)
                    .checked_mul(size_of::<(u32, usize)>())
                    .and_then(|groups| n.checked_add(groups))
            })
            .ok_or(Error::Capacity)?;
        if requests.len() > InitialGroupResidency::MAX_REQUESTS
            || bytes > InitialGroupResidency::MAX_METADATA_BYTES
        {
            return Err(Error::Capacity);
        }
        if requests.is_empty() {
            return Ok(());
        }
        if self.residency.is_none() {
            return Err(Error::InvalidInput);
        }
        let mut ids = Vec::new();
        ids.try_reserve_exact(requests.len())
            .map_err(|_| Error::Capacity)?;
        for r in requests {
            if r.async_id <= 0
                || r.async_id > self.midi_object.async_id_high_water()
                || usize::from(r.instance.0) >= self.script_initial.len()
                || self.midi_object.has_async_job(r.async_id)
            {
                return Err(Error::InvalidInput);
            }
            ids.push(r.async_id);
        }
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return Err(Error::InvalidInput);
        }
        let groups = self.initial_group_map()?;
        if requests.iter().any(|r| {
            groups
                .binary_search_by_key(&r.source_group, |g| g.0)
                .is_err()
        }) {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }

    fn initial_group_map(&self) -> Result<Vec<(u32, usize)>, Error> {
        let mut groups = Vec::new();
        groups
            .try_reserve_exact(self.group_count as usize)
            .map_err(|_| Error::Capacity)?;
        for dense in 0..self.group_count as usize {
            groups.push((
                self.group_source_ids
                    .get(dense)
                    .copied()
                    .unwrap_or(dense as u32),
                dense,
            ));
        }
        groups.sort_unstable_by_key(|g| g.0);
        if groups.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(Error::InvalidInput);
        }
        Ok(groups)
    }

    pub(crate) fn prepare_initial_residency(
        &mut self,
        canceled: &dyn Fn() -> bool,
    ) -> Result<State, Error> {
        if canceled() {
            return Err(Error::Canceled);
        }
        self.validate_initial_residency(&self.initial_residency)?;
        if self.initial_residency.is_empty() {
            return Ok(State::default());
        }
        let groups = self.initial_group_map()?;
        let mut results = Vec::new();
        results
            .try_reserve_exact(self.initial_residency.len())
            .map_err(|_| Error::Capacity)?;
        // A fresh owner prevents cancellation or re-admission from changing the
        // previous owner's group eligibility/revisions. Pcm ownership is shared.
        let candidate = Residency::new(self)?;
        let mut submitted = 0;
        while results.len() < self.initial_residency.len() {
            if canceled() {
                return Err(Error::Canceled);
            }
            while submitted < self.initial_residency.len() && candidate.room() {
                if canceled() {
                    return Err(Error::Canceled);
                }
                let r = self.initial_residency[submitted];
                let dense = groups[groups
                    .binary_search_by_key(&r.source_group, |g| g.0)
                    .unwrap()]
                .1;
                candidate.request(dense, r.reload, r.async_id, r.instance)?;
                submitted += 1;
            }
            let mut consumed = false;
            while let Some(c) = candidate.peek() {
                results.push(ResultRecord {
                    id: c.id,
                    instance: c.instance,
                    success: c.success,
                });
                candidate.consume(c);
                consumed = true;
            }
            if !consumed && results.len() < self.initial_residency.len() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        if canceled() {
            return Err(Error::Canceled);
        }
        self.residency = Some(candidate);
        Ok(State {
            results: results.into_boxed_slice(),
            next: 0,
        })
    }
}

impl Runtime {
    pub(super) fn service_initial_residency(&mut self, plan: crate::PlanId) -> Result<(), Error> {
        // Same finite boundary as physical completion service; callback capacity
        // retains a record rather than rejecting an already accepted operation.
        for _ in 0..64 {
            let g = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
            if g.initial_residency.pending() == 0 {
                break;
            }
            let c = g.initial_residency.results[g.initial_residency.next];
            let has_callback = g.prepared.signal_programs.iter().any(|p| {
                p.signal == crate::MIDI_ASYNC_SIGNAL
                    && g.prepared.programs[p.program].script_instance == Some(c.instance)
            });
            if has_callback
                && self.behaviors.available() == 0
                && !self.behaviors.slots.iter().any(|s| {
                    s.value
                        .is_some_and(|b| b.outcome == Some(crate::Outcome::Finished))
                })
            {
                break;
            }
            let callback = match self.preflight_async_callback(plan, c.instance) {
                Ok(callback) => callback,
                Err(Error::Capacity) => break,
                Err(error) => return Err(error),
            };
            // Preflight is mutation-free; the admitted callback retains its own
            // generation reference before this completion releases its reference.
            self.finish_async_callback(plan, c.id, c.success, callback)?;
            let generation = self.plans.get_mut(plan.0).unwrap();
            generation.initial_residency.next += 1;
            generation.callbacks -= 1;
        }
        Ok(())
    }
}
