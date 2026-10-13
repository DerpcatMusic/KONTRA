//! Generation-owned allocation membership. Lists include fade overlap; only
//! nonstolen members count against a domain's logical polyphony.
use crate::{Error, Index, PlanId, Prepared, Runtime, VoiceId};

/// A compiled allocation domain qualified by its retained plan generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationDomainId {
    plan: PlanId,
    index: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocationCounts {
    pub live: usize,
    pub fading: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Link {
    pub domain: Option<usize>,
    pub previous: Option<Index>,
    pub next: Option<Index>,
}

#[derive(Default)]
struct Domain {
    head: Option<Index>,
    live: usize,
    members: usize,
}

pub(crate) struct AllocationState {
    domains: Box<[Domain]>,
}

impl AllocationState {
    pub fn new(plan: &Prepared) -> Self {
        Self {
            domains: (0..1 + plan.voice_limits.len() + plan.group_count as usize)
                .map(|_| Domain::default())
                .collect(),
        }
    }
}

impl Runtime {
    /// None selects instrument polyphony; Some selects a compiled voice/source
    /// limit from Prepared::with_voice_limits. IDs cannot alias another plan.
    pub fn allocation_domain(
        &self,
        plan: PlanId,
        limit: Option<usize>,
    ) -> Result<AllocationDomainId, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        if limit.is_some_and(|i| i >= generation.prepared.voice_limits.len()) {
            return Err(Error::InvalidInput);
        }
        Ok(AllocationDomainId {
            plan,
            index: limit.map_or(0, |i| i + 1),
        })
    }

    pub fn allocation_counts(&self, id: AllocationDomainId) -> Result<AllocationCounts, Error> {
        let generation = self.plans.get(id.plan.0).ok_or(Error::StaleHandle)?;
        let domain = &generation.allocation.domains[id.index];
        Ok(AllocationCounts {
            live: domain.live,
            fading: domain.members - domain.live,
        })
    }

    pub(crate) fn domain_live(&self, plan: PlanId, domain: usize) -> usize {
        self.plans.get(plan.0).unwrap().allocation.domains[domain].live
    }

    pub(crate) fn domain_head(&self, plan: PlanId, domain: usize) -> Option<Index> {
        self.plans.get(plan.0).unwrap().allocation.domains[domain].head
    }

    pub(crate) fn domain_next(&self, index: Index, domain: usize) -> Option<Index> {
        self.voices.slots[index.get()]
            .value
            .as_ref()
            .unwrap()
            .allocation
            .iter()
            .find(|l| l.domain == Some(domain))
            .unwrap()
            .next
    }

    pub(crate) fn domain_members(
        &self,
        plan: PlanId,
        domain: usize,
    ) -> impl Iterator<Item = (Index, &crate::Voice)> {
        let mut next = self.plans.get(plan.0).unwrap().allocation.domains[domain].head;
        std::iter::from_fn(move || {
            let index = next?;
            let voice = self.voices.slots[index.get()].value.as_ref().unwrap();
            next = voice
                .allocation
                .iter()
                .find(|l| l.domain == Some(domain))
                .unwrap()
                .next;
            Some((index, voice))
        })
    }

    pub(crate) fn link_allocation(&mut self, id: VoiceId, slot: usize, domain: usize) {
        let voice = self.voices.get(id.0).unwrap();
        // A group can use the same domain for both its voice and source budget.
        if voice.allocation.iter().any(|l| l.domain == Some(domain)) {
            return;
        }
        let plan = voice.plan;
        let head = self.plans.get(plan.0).unwrap().allocation.domains[domain].head;
        if let Some(head) = head {
            self.voices
                .at_mut(head)
                .allocation
                .iter_mut()
                .find(|l| l.domain == Some(domain))
                .unwrap()
                .previous = Some(Index::new(id.0.index));
        }
        self.voices.get_mut(id.0).unwrap().allocation[slot] = Link {
            domain: Some(domain),
            previous: None,
            next: head,
        };
        let state = &mut self.plans.get_mut(plan.0).unwrap().allocation.domains[domain];
        state.head = Some(Index::new(id.0.index));
        state.members += 1;
        state.live += 1;
    }

    pub(crate) fn unlink_allocation(&mut self, voice: crate::Voice) {
        for link in voice.allocation.iter().filter(|l| l.domain.is_some()) {
            let domain = link.domain.unwrap();
            if let Some(previous) = link.previous {
                self.voices
                    .at_mut(previous)
                    .allocation
                    .iter_mut()
                    .find(|l| l.domain == Some(domain))
                    .unwrap()
                    .next = link.next;
            } else {
                self.plans.get_mut(voice.plan.0).unwrap().allocation.domains[domain].head =
                    link.next;
            }
            if let Some(next) = link.next {
                self.voices
                    .at_mut(next)
                    .allocation
                    .iter_mut()
                    .find(|l| l.domain == Some(domain))
                    .unwrap()
                    .previous = link.previous;
            }
            let state = &mut self.plans.get_mut(voice.plan.0).unwrap().allocation.domains[domain];
            state.members -= 1;
            state.live -= usize::from(!voice.stolen);
        }
    }

    pub(crate) fn steal_voice(&mut self, index: Index, fade: u32) {
        let voice = self.voices.at_mut(index);
        if voice.stolen {
            return;
        }
        voice.stolen = true;
        let state = &mut self.plans.get_mut(voice.plan.0).unwrap().allocation;
        for link in voice.allocation.iter().filter(|l| l.domain.is_some()) {
            state.domains[link.domain.unwrap()].live -= 1;
        }
        self.stolen += 1;
        self.choke_voice(index, fade);
    }
}
