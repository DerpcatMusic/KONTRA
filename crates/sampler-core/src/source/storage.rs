//! Immutable logical source storage, separate from oscillator clocks and signal layouts.
//!
//! This first constructor admits complete resident families. It does not bind a
//! family to Runtime, mutate residency, or provide independently published heads.
use crate::{
    AssetId, Error, Pcm,
    prepare::{ResidentFrames, ResidentPin},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// An original-channel dimension. Resource budgets impose their own checked limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelCount(NonZeroUsize);
impl ChannelCount {
    pub fn new(channels: usize) -> Result<Self, Error> {
        NonZeroUsize::new(channels)
            .map(Self)
            .ok_or(Error::InvalidInput)
    }
    pub fn get(self) -> usize {
        self.0.get()
    }
    pub fn pack_count(self) -> usize {
        self.get() / 2 + self.get() % 2
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum StorageIdentity {
    Stereo(AssetId),
    Channels(u64),
}

/// Process-local immutable storage identity. No caller-supplied numeric identity.
/// Stereo identities retain the actual legacy AssetId in a disjoint namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceStorageId(StorageIdentity);
impl SourceStorageId {
    fn new_family() -> Result<Self, Error> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let mut current = NEXT.load(Ordering::Relaxed);
        loop {
            let next = current.checked_add(1).ok_or(Error::Capacity)?;
            match NEXT.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return Ok(Self(StorageIdentity::Channels(current))),
                Err(observed) => current = observed,
            }
        }
    }
}

struct ResidentFamily {
    id: SourceStorageId,
    channels: ChannelCount,
    rate: u32,
    frames: usize,
    packs: Box<[Pcm]>,
    // One immutable complete cohort, pinned off audio. A borrowed read view
    // never reacquires independently changing pack publications.
    residents: Box<[ResidentPin]>,
}

/// One immutable resident source revision, physically packed in original order.
/// Construct, clone and drop on control. Audio only borrows SourceResidentView.
/// Cloning retains this same logical identity and immutable resident cohort.
#[derive(Clone)]
pub struct SourceStorageFamily(Arc<ResidentFamily>);
impl std::fmt::Debug for SourceStorageFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceStorageFamily")
            .field("id", &self.id())
            .field("channels", &self.channels())
            .field("rate", &self.sample_rate())
            .field("frames", &self.frame_count())
            .field("pack_count", &self.0.packs.len())
            .finish()
    }
}
impl SourceStorageFamily {
    /// Adopt a complete resident original-channel cohort without copying PCM.
    /// Every pack is a distinct actual resident asset with identical geometry.
    /// The final unused lane for odd channel counts must be canonical +0.
    /// Streamed/headed/pending assets are not resident-family publications.
    pub fn new_resident(channels: ChannelCount, packs: Vec<Pcm>) -> Result<Self, Error> {
        if packs.len() != channels.pack_count() {
            return Err(Error::InvalidInput);
        }
        let first = packs.first().ok_or(Error::InvalidInput)?;
        let rate = first.sample_rate();
        let frames = first.frame_count();
        let mut residents = Vec::new();
        residents
            .try_reserve_exact(packs.len())
            .map_err(|_| Error::Capacity)?;
        for (index, pack) in packs.iter().enumerate() {
            if !pack.requires_resident()
                || pack.sample_rate() != rate
                || pack.frame_count() != frames
                || packs[..index]
                    .iter()
                    .any(|previous| previous.asset_id() == pack.asset_id())
            {
                return Err(Error::InvalidInput);
            }
            let resident = pack.pin_resident().ok_or(Error::NotReady)?;
            if resident.len() != frames {
                return Err(Error::InvalidInput);
            }
            if index + 1 == packs.len()
                && channels.get() % 2 != 0
                && resident.iter().any(|frame| frame[1].to_bits() != 0)
            {
                return Err(Error::InvalidInput);
            }
            residents.push(resident);
        }
        // Mint only after every geometry, identity and resident-payload check.
        let id = SourceStorageId::new_family()?;
        Ok(Self(Arc::new(ResidentFamily {
            id,
            channels,
            rate,
            frames,
            packs: packs.into_boxed_slice(),
            residents: residents.into_boxed_slice(),
        })))
    }
    pub fn id(&self) -> SourceStorageId {
        self.0.id
    }
    pub fn channels(&self) -> ChannelCount {
        self.0.channels
    }
    pub fn sample_rate(&self) -> u32 {
        self.0.rate
    }
    pub fn frame_count(&self) -> usize {
        self.0.frames
    }
    pub fn as_source(&self) -> SourceStorageRef<'_> {
        SourceStorageRef::Channels(self)
    }
    /// Borrow the constructor's complete immutable resident cohort. No allocation,
    /// reader acquisition, decoder call, or owned Arc enters this read view.
    pub fn resident_view(&self) -> SourceResidentView<'_> {
        SourceResidentView {
            id: self.id(),
            channels: self.channels(),
            rate: self.sample_rate(),
            frames: self.frame_count(),
            data: ResidentViewData::Channels(&self.0),
        }
    }
}

/// One logical source per IR AssetRef. Physical packs are never extra IR assets.
/// Construct, clone and destroy owned storage on control; rendering borrows it.
#[derive(Clone, Debug)]
pub enum SourceStorage {
    Stereo(Pcm),
    Channels(SourceStorageFamily),
}
impl From<Pcm> for SourceStorage {
    fn from(pcm: Pcm) -> Self {
        Self::Stereo(pcm)
    }
}
impl From<SourceStorageFamily> for SourceStorage {
    fn from(family: SourceStorageFamily) -> Self {
        Self::Channels(family)
    }
}
impl SourceStorage {
    pub fn as_source(&self) -> SourceStorageRef<'_> {
        match self {
            Self::Stereo(pcm) => SourceStorageRef::Stereo(pcm),
            Self::Channels(family) => SourceStorageRef::Channels(family),
        }
    }
}

/// A borrowed logical source. Stereo retains the existing two-lane Pcm contract;
/// original mono duplication in that specialization is not a wide-layout rule.
#[derive(Clone, Copy, Debug)]
pub enum SourceStorageRef<'a> {
    Stereo(&'a Pcm),
    Channels(&'a SourceStorageFamily),
}
impl<'a> SourceStorageRef<'a> {
    pub fn id(self) -> SourceStorageId {
        match self {
            Self::Stereo(pcm) => SourceStorageId(StorageIdentity::Stereo(pcm.asset_id())),
            Self::Channels(family) => family.id(),
        }
    }
    pub fn channels(self) -> ChannelCount {
        match self {
            Self::Stereo(_) => ChannelCount(NonZeroUsize::MIN.saturating_add(1)),
            Self::Channels(family) => family.channels(),
        }
    }
    pub fn sample_rate(self) -> u32 {
        match self {
            Self::Stereo(pcm) => pcm.sample_rate(),
            Self::Channels(f) => f.sample_rate(),
        }
    }
    pub fn frame_count(self) -> usize {
        match self {
            Self::Stereo(pcm) => pcm.frame_count(),
            Self::Channels(f) => f.frame_count(),
        }
    }
    /// Control-side complete dependency enumeration. Families were validated
    /// before publication, so no missing member can yield a successful prefix.
    /// This query does not install a family residency or mutation coordinator.
    pub fn physical_dependencies(self, mut visit: impl FnMut(&Pcm)) {
        match self {
            Self::Stereo(pcm) => visit(pcm),
            Self::Channels(family) => {
                for pcm in &family.0.packs {
                    visit(pcm);
                }
            }
        }
    }
    /// Legacy-only view. A wide family never silently resolves to pack zero.
    pub fn stereo(self) -> Option<&'a Pcm> {
        match self {
            Self::Stereo(pcm) => Some(pcm),
            Self::Channels(_) => None,
        }
    }
    pub fn resident_view(self) -> Option<SourceResidentView<'a>> {
        match self {
            Self::Stereo(pcm) => {
                let resident = pcm.resident_frames()?;
                Some(SourceResidentView {
                    id: self.id(),
                    channels: self.channels(),
                    rate: pcm.sample_rate(),
                    frames: resident.len(),
                    data: ResidentViewData::Stereo(resident),
                })
            }
            Self::Channels(family) => Some(family.resident_view()),
        }
    }
}

enum ResidentViewData<'a> {
    Stereo(ResidentFrames<'a>),
    Channels(&'a ResidentFamily),
}

/// Borrowed original-channel PCM from one complete immutable resident cohort.
/// Dropping it releases at most a borrowing snapshot guard, never an owned PCM
/// buffer, decoder, family Arc or allocation. No logical clock lives here.
pub struct SourceResidentView<'a> {
    id: SourceStorageId,
    channels: ChannelCount,
    rate: u32,
    frames: usize,
    data: ResidentViewData<'a>,
}
impl SourceResidentView<'_> {
    pub fn id(&self) -> SourceStorageId {
        self.id
    }
    pub fn channels(&self) -> ChannelCount {
        self.channels
    }
    pub fn sample_rate(&self) -> u32 {
        self.rate
    }
    pub fn frame_count(&self) -> usize {
        self.frames
    }
    pub fn sample(&self, channel: usize, frame: usize) -> Option<f32> {
        if channel >= self.channels.get() || frame >= self.frames {
            return None;
        }
        Some(match &self.data {
            ResidentViewData::Stereo(resident) => resident[frame][channel],
            ResidentViewData::Channels(family) => family.residents[channel / 2][frame][channel % 2],
        })
    }
    /// Validate the entire request before writing any destination element.
    pub fn copy_channel(&self, channel: usize, start: usize, out: &mut [f32]) -> Result<(), Error> {
        let end = start.checked_add(out.len()).ok_or(Error::InvalidInput)?;
        if channel >= self.channels.get() || end > self.frames {
            return Err(Error::InvalidInput);
        }
        match &self.data {
            ResidentViewData::Stereo(resident) => {
                for (to, frame) in out.iter_mut().zip(&resident[start..end]) {
                    *to = frame[channel];
                }
            }
            ResidentViewData::Channels(family) => {
                for (to, frame) in out
                    .iter_mut()
                    .zip(&family.residents[channel / 2][start..end])
                {
                    *to = frame[channel % 2];
                }
            }
        }
        Ok(())
    }
    /// Copy one original frame to a complete, preallocated original-channel row.
    pub fn copy_frame(&self, frame: usize, out: &mut [f32]) -> Result<(), Error> {
        if frame >= self.frames || out.len() != self.channels.get() {
            return Err(Error::InvalidInput);
        }
        match &self.data {
            ResidentViewData::Stereo(resident) => out.copy_from_slice(&resident[frame]),
            ResidentViewData::Channels(family) => {
                for (channel, to) in out.iter_mut().enumerate() {
                    *to = family.residents[channel / 2][frame][channel % 2];
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Frame;
    fn pcm(rate: u32, frames: usize, values: Frame) -> Pcm {
        Pcm::new(rate, vec![values; frames].into_boxed_slice()).unwrap()
    }
    fn six() -> Vec<Pcm> {
        vec![
            pcm(48000, 7, [1., 2.]),
            pcm(48000, 7, [3., 4.]),
            pcm(48000, 7, [5., 6.]),
        ]
    }
    #[test]
    fn resident_family_exposes_all_original_ordinals_and_complete_dependencies() {
        let family =
            SourceStorageFamily::new_resident(ChannelCount::new(6).unwrap(), six()).unwrap();
        let view = family.resident_view();
        assert_eq!(view.channels().get(), 6);
        assert_eq!((view.sample_rate(), view.frame_count()), (48000, 7));
        for channel in 0..6 {
            assert_eq!(view.sample(channel, 3), Some((channel + 1) as f32));
        }
        assert_eq!(view.sample(6, 0), None);
        assert_eq!(view.sample(0, 7), None);
        let mut frame = [0.; 6];
        view.copy_frame(4, &mut frame).unwrap();
        assert_eq!(frame, [1., 2., 3., 4., 5., 6.]);
        let mut ids = Vec::new();
        family
            .as_source()
            .physical_dependencies(|p| ids.push(p.asset_id()));
        assert_eq!(ids.len(), 3);
        assert!(ids.iter().enumerate().all(|(i, id)| !ids[..i].contains(id)));
        assert!(family.as_source().stereo().is_none());
    }
    #[test]
    fn invalid_resident_family_geometry_and_identity_never_publish() {
        let channels = ChannelCount::new(6).unwrap();
        assert!(ChannelCount::new(0).is_err());
        assert!(SourceStorageFamily::new_resident(channels, vec![]).is_err());
        assert!(
            SourceStorageFamily::new_resident(channels, vec![pcm(48000, 7, [1., 2.])]).is_err()
        );
        let same = pcm(48000, 7, [1., 2.]);
        assert!(
            SourceStorageFamily::new_resident(
                channels,
                vec![same.clone(), same, pcm(48000, 7, [5., 6.])]
            )
            .is_err()
        );
        let mut rate = six();
        rate[1] = pcm(44100, 7, [3., 4.]);
        assert!(SourceStorageFamily::new_resident(channels, rate).is_err());
        let mut length = six();
        length[1] = pcm(48000, 6, [3., 4.]);
        assert!(SourceStorageFamily::new_resident(channels, length).is_err());
        let mut pending = six();
        pending[1] = Pcm::pending_resident(48000, 7).unwrap();
        assert!(matches!(
            SourceStorageFamily::new_resident(channels, pending),
            Err(Error::NotReady)
        ));
        let mut headed = six();
        headed[1] = Pcm::headed(48000, 7, &[[3., 4.]; 7]).unwrap();
        assert!(matches!(
            SourceStorageFamily::new_resident(channels, headed),
            Err(Error::InvalidInput)
        ));
    }
    #[test]
    fn odd_original_layout_requires_canonical_unused_lane() {
        let channels = ChannelCount::new(3).unwrap();
        let family = SourceStorageFamily::new_resident(
            channels,
            vec![pcm(48000, 4, [1., 2.]), pcm(48000, 4, [3., 0.])],
        )
        .unwrap();
        assert_eq!(family.channels().pack_count(), 2);
        assert_eq!(family.resident_view().sample(2, 3), Some(3.));
        assert_eq!(family.resident_view().sample(3, 3), None);
        for unused in [1., -0.] {
            assert!(
                SourceStorageFamily::new_resident(
                    channels,
                    vec![pcm(48000, 4, [1., 2.]), pcm(48000, 4, [3., unused])]
                )
                .is_err()
            );
        }
    }
    #[test]
    fn immutable_family_identity_clones_and_legacy_identity_stay_disjoint() {
        let packs = six();
        let source = packs[0].clone();
        let family =
            SourceStorageFamily::new_resident(ChannelCount::new(6).unwrap(), packs.clone())
                .unwrap();
        let clone = family.clone();
        assert_eq!(family.id(), clone.id());
        let separately_built =
            SourceStorageFamily::new_resident(ChannelCount::new(6).unwrap(), packs).unwrap();
        assert_ne!(family.id(), separately_built.id());
        assert_ne!(family.id(), SourceStorageRef::Stereo(&source).id());
        let stereo = SourceStorage::from(source.clone());
        assert_eq!(
            stereo.as_source().id(),
            SourceStorageRef::Stereo(&source).id()
        );
        assert_eq!(
            stereo.as_source().resident_view().unwrap().sample(1, 2),
            Some(2.)
        );
        assert_eq!(clone.resident_view().sample(5, 2), Some(6.));
    }
    #[test]
    fn resident_copy_preflights_all_bounds_before_destination_write() {
        let family =
            SourceStorageFamily::new_resident(ChannelCount::new(6).unwrap(), six()).unwrap();
        let view = family.resident_view();
        let mut samples = [99.; 3];
        for (channel, start) in [(6, 0), (2, 5), (2, usize::MAX)] {
            assert!(view.copy_channel(channel, start, &mut samples).is_err());
            assert_eq!(samples, [99.; 3]);
        }
        view.copy_channel(4, 2, &mut samples).unwrap();
        assert_eq!(samples, [5.; 3]);
        let mut short = [99.; 5];
        assert!(view.copy_frame(0, &mut short).is_err());
        assert_eq!(short, [99.; 5]);
        let mut row = [99.; 6];
        assert!(view.copy_frame(7, &mut row).is_err());
        assert_eq!(row, [99.; 6]);
        view.copy_channel(0, 7, &mut []).unwrap();
    }
    #[test]
    fn captured_resident_cohort_cannot_mix_with_a_later_partial_publication() {
        let packs = six();
        let middle = packs[1].clone();
        middle.residency_acquire();
        let family =
            SourceStorageFamily::new_resident(ChannelCount::new(6).unwrap(), packs).unwrap();
        middle.residency_release();
        // A pack alias can retire its current publication, but cannot alter
        // this immutable source revision's complete captured generation.
        assert!(!middle.residency_collect());
        assert!(middle.resident_frames().is_none());
        {
            let view = family.resident_view();
            let mut frame = [0.; 6];
            view.copy_frame(0, &mut frame).unwrap();
            assert_eq!(frame, [1., 2., 3., 4., 5., 6.]);
        }
        // Reclaim only after the immutable family owner is released on control.
        // This is not a Runtime group-purge implementation or completion claim.
        drop(family);
        assert!(middle.residency_collect());
    }
    #[test]
    fn resident_view_does_not_create_a_stereo_fallback_for_missing_storage() {
        let pcm = Pcm::streamed(48000, 7).unwrap();
        let source = SourceStorage::from(pcm);
        assert!(source.as_source().resident_view().is_none());
        assert_eq!(source.as_source().channels().get(), 2);
    }
}
