//! Full-resident storage owns its physical admission for exactly as long as
//! its immutable generation exists. Audio readers only borrow this payload.
use super::{Error, Frame, HeadBudget, Pcm, head_budget::Lease};
use std::sync::OnceLock;

#[derive(Debug)]
pub(super) struct ResidentData {
    frames: Box<[Frame]>,
    lease: OnceLock<Lease>,
}
impl ResidentData {
    pub(super) fn unbudgeted(frames: Box<[Frame]>) -> Self {
        Self {
            frames,
            lease: OnceLock::new(),
        }
    }
    pub(super) fn frames(&self) -> &[Frame] {
        &self.frames
    }
    pub(super) fn charge(&self, budget: &HeadBudget) -> Result<(), Error> {
        if let Some(lease) = self.lease.get() {
            return if lease.belongs_to(budget) {
                Ok(())
            } else {
                Err(Error::RevisionConflict)
            };
        }
        let bytes = self
            .frames
            .len()
            .checked_mul(size_of::<Frame>())
            .ok_or(Error::Capacity)?;
        self.lease
            .set(budget.reserve(bytes)?)
            .map_err(|_| Error::RevisionConflict)
    }
}

/// Control-only full PCM decode staging. Admission happens before allocation;
/// dropping a failed/canceled decode returns its quota. Finishing transfers the
/// same buffer and lease into an immutable Pcm, without copying.
#[derive(Debug)]
pub struct ResidentBuffer {
    rate: u32,
    data: ResidentData,
    budget: HeadBudget,
}
impl ResidentBuffer {
    pub fn new(rate: u32, frames: usize, budget: &HeadBudget) -> Result<Self, Error> {
        if rate == 0 || frames == 0 {
            return Err(Error::InvalidInput);
        }
        let bytes = frames
            .checked_mul(size_of::<Frame>())
            .ok_or(Error::Capacity)?;
        if bytes > isize::MAX as usize {
            return Err(Error::Capacity);
        }
        let lease = budget.reserve(bytes)?;
        let mut out = Vec::new();
        out.try_reserve_exact(frames).map_err(|_| Error::Capacity)?;
        out.resize(frames, [0.; 2]);
        let data = ResidentData {
            frames: out.into_boxed_slice(),
            lease: OnceLock::from(lease),
        };
        Ok(Self {
            rate,
            data,
            budget: budget.clone(),
        })
    }
    pub fn bytes(&self) -> usize {
        self.data.frames.len() * size_of::<Frame>()
    }
    pub fn frames_mut(&mut self) -> &mut [Frame] {
        &mut self.data.frames
    }
    pub fn finish(self) -> Result<Pcm, Error> {
        let (rate, data, budget) = self.into_parts()?;
        let pcm = Pcm::create(rate, data.frames.len(), Some(data))?;
        pcm.bind_resident_budget(&budget)?;
        Ok(pcm)
    }
    pub(super) fn into_parts(self) -> Result<(u32, ResidentData, HeadBudget), Error> {
        if self.data.frames.iter().flatten().any(|f| !f.is_finite()) {
            return Err(Error::InvalidInput);
        }
        Ok((self.rate, self.data, self.budget))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_purge_guard_and_budgeted_reload_retain_exact_physical_lifetime() {
        let budget = HeadBudget::new(128);
        let mut buffer = ResidentBuffer::new(48000, 16, &budget).unwrap();
        buffer.frames_mut().fill([0.25; 2]);
        let pcm = buffer.finish().unwrap();
        let reload_budget = budget.clone();
        pcm.set_budgeted_resident_loader(move || {
            let mut buffer = ResidentBuffer::new(48000, 16, &reload_budget)?;
            buffer.frames_mut().fill([0.5; 2]);
            Ok(buffer)
        })
        .unwrap();
        pcm.residency_acquire();
        let audio_guard = pcm.resident_frames().unwrap();
        pcm.residency_release();
        assert!(!pcm.residency_collect());
        assert!(pcm.resident_frames().is_none());
        assert_eq!(&*audio_guard, &[[0.25; 2]; 16]);
        assert_eq!(budget.status().held_bytes, 128);
        pcm.residency_acquire();
        assert_eq!(pcm.residency_reload(), Err(Error::Capacity));
        drop(audio_guard);
        assert_eq!(
            budget.status().held_bytes,
            128,
            "reader drop never reclaims the payload"
        );
        pcm.residency_reload().unwrap();
        assert_eq!(&*pcm.resident_frames().unwrap(), &[[0.5; 2]; 16]);
        assert_eq!(budget.status().held_bytes, 128);
        assert_eq!(budget.status().peak_bytes, 128);
        pcm.residency_release();
        assert!(pcm.residency_collect());
        assert_eq!(budget.status().held_bytes, 0);
    }
    #[test]
    fn raw_and_foreign_reloaders_cannot_bypass_full_physical_admission() {
        let budget = HeadBudget::new(128);
        let raw = Pcm::new(48000, vec![[0.25; 2]; 16].into()).unwrap();
        raw.set_residency_loader(|| Ok(vec![(0, vec![[0.5; 2]; 16].into())]))
            .unwrap();
        assert_eq!(
            raw.bind_resident_budget(&budget),
            Err(Error::RevisionConflict)
        );
        assert_eq!(budget.status().held_bytes, 0);
        let pcm = ResidentBuffer::new(48000, 16, &budget)
            .unwrap()
            .finish()
            .unwrap();
        assert_eq!(
            pcm.set_residency_loader(|| Ok(vec![(0, vec![[0.5; 2]; 16].into())])),
            Err(Error::RevisionConflict)
        );
        let foreign = HeadBudget::new(128);
        let foreign_worker = foreign.clone();
        pcm.set_budgeted_resident_loader(move || ResidentBuffer::new(48000, 16, &foreign_worker))
            .unwrap();
        pcm.residency_acquire();
        pcm.residency_release();
        assert!(pcm.residency_collect());
        pcm.residency_acquire();
        assert_eq!(pcm.residency_reload(), Err(Error::RevisionConflict));
        assert!(pcm.resident_frames().is_none());
        assert_eq!(budget.status().held_bytes, 0);
        assert_eq!(foreign.status().held_bytes, 0);
    }
    #[test]
    fn purge_during_budgeted_full_reload_discards_staging_before_publication() {
        let budget = HeadBudget::new(128);
        let pcm = ResidentBuffer::new(48000, 16, &budget)
            .unwrap()
            .finish()
            .unwrap();
        let (entered, received) = std::sync::mpsc::channel();
        let (resume, resumed) = std::sync::mpsc::channel();
        let resumed = std::sync::Mutex::new(resumed);
        let loading_budget = budget.clone();
        pcm.set_budgeted_resident_loader(move || {
            let mut out = ResidentBuffer::new(48000, 16, &loading_budget)?;
            out.frames_mut().fill([0.5; 2]);
            entered.send(()).unwrap();
            resumed
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            Ok(out)
        })
        .unwrap();
        pcm.residency_acquire();
        pcm.residency_release();
        assert!(pcm.residency_collect());
        pcm.residency_acquire();
        std::thread::scope(|scope| {
            let loading = scope.spawn(|| pcm.residency_reload());
            received
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            assert_eq!(budget.status().held_bytes, 128);
            pcm.residency_release();
            resume.send(()).unwrap();
            assert_eq!(loading.join().unwrap(), Err(Error::NotReady));
        });
        assert!(pcm.resident_frames().is_none());
        assert_eq!(budget.status().held_bytes, 0);
        assert_eq!(budget.status().peak_bytes, 128);
    }
}
