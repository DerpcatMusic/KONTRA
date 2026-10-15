//! Offline residency observations, between render calls and tied to one owner.
use super::V2Core;
use sampler_core::{Error, GroupResidencyProgress, ResidencyProgress};

impl V2Core {
    /// Does not service completions or change group eligibility. Counts describe
    /// the residency owner and worker observations may change during the query.
    pub fn diagnostic_residency_progress(
        &self,
        slot: usize,
        expected_epoch: u64,
    ) -> Result<ResidencyProgress, Error> {
        let part = self
            .parts
            .get(slot)
            .and_then(Option::as_ref)
            .filter(|part| part.epoch == expected_epoch)
            .ok_or(Error::StaleHandle)?;
        part.runtime.residency_progress(part.runtime.active_plan())
    }

    /// Dense group identity comes from this exact prepared owner, never a source
    /// filename index or another generation's physical group number.
    pub fn diagnostic_group_residency_progress(
        &self,
        slot: usize,
        expected_epoch: u64,
        group: u32,
    ) -> Result<GroupResidencyProgress, Error> {
        let part = self
            .parts
            .get(slot)
            .and_then(Option::as_ref)
            .filter(|part| part.epoch == expected_epoch)
            .ok_or(Error::StaleHandle)?;
        part.runtime
            .group_residency_progress(part.runtime.active_plan(), group)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::Core;
    use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};

    fn owner(epoch: u64) -> Box<super::super::Part> {
        let plan = Prepared::new(
            48000,
            vec![Pcm::new(48000, vec![[0.25; 2]; 8].into()).unwrap()],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::default(),
                playback: Playback::default(),
            }],
            1,
        )
        .unwrap()
        .with_groups(1, vec![Some(0)])
        .unwrap()
        .with_source_groups(vec![37])
        .unwrap();
        let limits = Limits::for_plan(&plan, 8, 8);
        let mut part = super::super::Part::new(
            Runtime::new(plan, limits).unwrap(),
            super::super::MixTree::instrument("diagnostic owner"),
        )
        .unwrap();
        part.epoch = epoch;
        Box::new(part)
    }

    #[test]
    fn residency_queries_reject_replaced_owners_and_keep_dense_group_identity() {
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, Some(owner(73)));
        assert_eq!(
            core.diagnostic_residency_progress(0, 72),
            Err(Error::StaleHandle)
        );
        assert_eq!(
            core.diagnostic_residency_progress(1, 73),
            Err(Error::StaleHandle)
        );
        let group = core.diagnostic_group_residency_progress(0, 73, 0).unwrap();
        assert_eq!(
            (group.group, group.source_group, group.loaded),
            (0, 37, true)
        );
        assert_eq!(
            core.diagnostic_group_residency_progress(0, 73, 37),
            Err(Error::InvalidInput)
        );
        core.install(0, Some(owner(74)));
        assert_eq!(
            core.diagnostic_group_residency_progress(0, 73, 0),
            Err(Error::StaleHandle)
        );
        assert_eq!(
            core.diagnostic_residency_progress(0, 74).unwrap().pending,
            0
        );
    }
}
