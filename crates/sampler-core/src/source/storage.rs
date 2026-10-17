//! Immutable logical source storage, separate from oscillator clocks and signal layouts.
//!
//! Resident geometry and physical identities are immutable. The existing group
//! worker publishes or retires a complete resident cohort; audio borrows one
//! coherent publication and never acquires independent pack generations.
use crate::{
    AssetId, Error, Pcm,
    prepare::{ResidentFrames, ResidentPin},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
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

pub(crate) struct ResidentFamily {
    id: SourceStorageId,
    channels: ChannelCount,
    rate: u32,
    frames: usize,
    packs: Box<[Pcm]>,
    // Each complete cohort is pinned and published off audio. A borrowed view
    // never reacquires independently changing pack publications.
    residents: sampler_pool::Snapshot<Option<Box<[ResidentPin]>>>,
    residency_mutation: Mutex<()>,
}

impl ResidentFamily {
    /// Residency worker only. Geometry and physical identity never change;
    /// publication contains every original pack or no pack. No reader can see
    /// a successful prefix while a group reload is being staged.
    pub(crate) fn refresh_residency(&self) -> Result<(), Error> {
        let _mutation = self
            .residency_mutation
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let permitted = self.packs.iter().all(Pcm::residency_permitted);
        let resident = self.residents.read().is_some();
        if !permitted && resident {
            self.residents.replace(None, |_| ());
        } else if permitted && !resident {
            let mut pins = Vec::new();
            pins.try_reserve_exact(self.packs.len())
                .map_err(|_| Error::Capacity)?;
            for (index, pack) in self.packs.iter().enumerate() {
                let pin = pack.pin_resident().ok_or(Error::NotReady)?;
                if pin.len() != self.frames
                    || index + 1 == self.packs.len()
                        && self.channels.get() % 2 != 0
                        && pin.iter().any(|frame| frame[1].to_bits() != 0)
                {
                    return Err(Error::InvalidInput);
                }
                pins.push(pin);
            }
            // A racing purge may have revoked ownership while pins were staged.
            if !self.packs.iter().all(Pcm::residency_permitted) {
                return Err(Error::NotReady);
            }
            self.residents
                .replace(Some(pins.into_boxed_slice()), |_| ());
        }
        self.residents.collect();
        Ok(())
    }
    /// Reclaim old cohorts only on the existing residency worker/control side.
    pub(crate) fn collect_residency(&self) {
        self.residents.collect();
        // Clearing a retired cohort releases all physical Snapshot pins. Some
        // sibling assets may no longer have a worker of their own; reclaim
        // their retired buffers here, after releasing the cohort mutex.
        for pack in &self.packs { pack.collect_retired_resident(); }
    }
}
impl std::fmt::Debug for ResidentFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResidentFamily")
            .field("id", &self.id)
            .field("channels", &self.channels)
            .field("rate", &self.rate)
            .field("frames", &self.frames)
            .finish_non_exhaustive()
    }
}

/// One resident logical source with immutable geometry and physical pack IDs.
/// Construct, clone and drop on control. Audio borrows one complete cohort.
/// Clones retain the same source identity and residency publication. Group purge
/// retires its cohort; reload publishes all packs before the group is eligible.
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
        let family = Arc::new(ResidentFamily {
            id,
            channels,
            rate,
            frames,
            packs: packs.into_boxed_slice(),
            residents: sampler_pool::Snapshot::new(Some(residents.into_boxed_slice())),
            residency_mutation: Mutex::new(()),
        });
        // A physical alias may outlive every Prepared that contains this family.
        // Its ordinary worker must still discover and retire this cohort's pins.
        // Weak-only registration is fallible and entirely off audio. Failure
        // leaves only dead prunable entries, never a published partial family.
        for pack in &family.packs {
            pack.register_resident_family(&family)?;
        }
        Ok(Self(family))
    }
    pub(crate) fn refresh_residency(&self) -> Result<(), Error> {
        self.0.refresh_residency()
    }
    pub(crate) fn collect_residency(&self) {
        self.0.collect_residency();
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
    /// Borrow one complete resident cohort, or an unavailable publication after
    /// group purge. Metadata remains queryable; sample reads then return None.
    /// No allocation, decoder call, or owned Arc enters this read view.
    pub fn resident_view(&self) -> SourceResidentView<'_> {
        SourceResidentView {
            id: self.id(),
            channels: self.channels(),
            rate: self.sample_rate(),
            frames: self.frame_count(),
            data: ResidentViewData::Channels(self.0.residents.read()),
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
            Self::Channels(family) => {
                let view = family.resident_view();
                if view.ready() { Some(view) } else { None }
            }
        }
    }
}

enum ResidentViewData<'a> {
    Stereo(ResidentFrames<'a>),
    Channels(sampler_pool::SnapshotRead<'a, Option<Box<[ResidentPin]>>>),
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
    fn ready(&self) -> bool {
        match &self.data {
            ResidentViewData::Stereo(_) => true,
            ResidentViewData::Channels(pins) => pins.is_some(),
        }
    }
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
    /// Internal physical read window. Signal/source identities never resolve to a pack alias.
    pub(crate) fn pair(&self, pair: usize) -> Option<&[crate::Frame]> {
        match &self.data {
            ResidentViewData::Stereo(resident) => (pair == 0).then_some(&resident[..]),
            ResidentViewData::Channels(pins) => pins.as_ref()?.get(pair).map(|p| &p[..]),
        }
    }
    pub fn sample(&self, channel: usize, frame: usize) -> Option<f32> {
        if channel >= self.channels.get() || frame >= self.frames {
            return None;
        }
        Some(match &self.data {
            ResidentViewData::Stereo(resident) => resident[frame][channel],
            ResidentViewData::Channels(pins) => pins.as_ref()?[channel / 2][frame][channel % 2],
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
            ResidentViewData::Channels(pins) => {
                let pins = pins.as_ref().ok_or(Error::NotReady)?;
                for (to, frame) in out.iter_mut().zip(&pins[channel / 2][start..end]) {
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
            ResidentViewData::Channels(pins) => {
                let pins = pins.as_ref().ok_or(Error::NotReady)?;
                for (channel, to) in out.iter_mut().enumerate() {
                    *to = pins[channel / 2][frame][channel % 2];
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
        let captured = family.resident_view();
        middle.residency_release();
        // A physical alias can retire the current publication, but cannot
        // change the complete cohort that an existing audio/read borrow holds.
        assert!(!middle.residency_collect());
        assert!(middle.resident_frames().is_none());
        let mut frame = [0.; 6];
        captured.copy_frame(0, &mut frame).unwrap();
        assert_eq!(frame, [1., 2., 3., 4., 5., 6.]);
        // New reads see unavailable, never a mix of old and new physical packs.
        assert!(family.as_source().resident_view().is_none());
        drop(captured);
        assert!(middle.residency_collect());
        assert!(family.as_source().resident_view().is_none());
    }
    #[test]
    fn resident_view_does_not_create_a_stereo_fallback_for_missing_storage() {
        let pcm = Pcm::streamed(48000, 7).unwrap();
        let source = SourceStorage::from(pcm);
        assert!(source.as_source().resident_view().is_none());
        assert_eq!(source.as_source().channels().get(), 2);
    }
}
