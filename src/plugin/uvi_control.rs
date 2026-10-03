//! Loader-owned native controllers. Audio endpoints return a retirement receipt
//! only after their Slot/AudioPort has been destroyed outside the callback.
//! The plugin retains an Arc lease to this registry after all Dsp endpoints, and
//! declares Shared's endpoint queues before its registry lease. That teardown
//! ordering is required in addition to the normal loader receipt checks below.

use super::{
    uvi::Slot,
    uvi_ui::{Mailbox, Published},
};
use crate::{
    library::UviSource,
    uvi::{
        bridge::{Bridge, BridgeError},
        ui_assets::UiAssets,
        worker::{Stamp, StartConfig, Status, Worker},
    },
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Clone, Copy, Debug)]
pub(super) enum Error {
    InvalidIdentity,
    DuplicateActivation,
    MissingActivation,
    Cancelled,
    Start,
    NotReady,
    WorkerFailed,
    EndpointTaken,
    Bridge(BridgeError),
    Slot(super::uvi::Error),
}

/// Move through ready/audio/retired ownership; never destroy in process/reset.
/// Its receipt means endpoint destruction, not merely removal from an active
/// rack slot. Core host-note ownership must finish before loader retirement.
pub(super) struct Audio {
    slot: Option<Box<Slot>>,
    stamp: Stamp,
    destination: usize,
    part_generation: u64,
    retired: Arc<AtomicBool>,
}
impl Audio {
    pub fn slot(&self) -> &Slot {
        self.slot
            .as_deref()
            .expect("UVI endpoint is present until retirement")
    }
    pub fn slot_mut(&mut self) -> &mut Slot {
        self.slot
            .as_deref_mut()
            .expect("UVI endpoint is present until retirement")
    }
    pub fn epoch(&self) -> u64 {
        self.stamp.epoch
    }
    pub fn generation(&self) -> u64 {
        self.stamp.generation
    }
    pub fn destination(&self) -> usize {
        self.destination
    }
    pub fn part_generation(&self) -> u64 {
        self.part_generation
    }
    pub fn latency_frames(&self) -> u32 {
        self.slot().latency_frames()
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        // Loader/host teardown only. Slot owns the AudioPort and all endpoint
        // allocations; publishing before this drop would permit premature join.
        drop(self.slot.take());
        self.retired.store(true, Ordering::Release);
    }
}

struct Control {
    worker: Worker,
    mailbox: Mailbox,
    slot: usize,
    source: UviSource,
    part_generation: u64,
    rate: u32,
    maximum: usize,
    lead: usize,
    retired: Arc<AtomicBool>,
    exported: bool,
    cancelled: bool,
}
impl Control {
    fn removable(&self) -> bool {
        !self.exported || self.retired.load(Ordering::Acquire)
    }
}

#[derive(Default)]
pub(super) struct Registry {
    controls: HashMap<(u64, u64), Control>,
}
impl Registry {
    /// Serialized loader only. Native initialization stays on Worker; artwork
    /// authority stays in this loader-owned mailbox. No controller enters Dsp.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        config: StartConfig,
        source: UviSource,
        epoch: u64,
        generation: u64,
        part_generation: u64,
        slot: usize,
        max_host_frames: usize,
        worker_lead_packets: usize,
    ) -> Result<(), Error> {
        if epoch == 0
            || generation == 0
            || config.bank != source.bank
            || config.member != source.member
            || config.expected_bank_uuid != Some(source.bank_uuid)
        {
            return Err(Error::InvalidIdentity);
        }
        let key = (epoch, generation);
        if self.controls.contains_key(&key) {
            return Err(Error::DuplicateActivation);
        }
        let rate = config.sample_rate;
        let assets = UiAssets::open(&config).ok();
        let worker = Worker::start_hosted(config, epoch, generation).map_err(|_| Error::Start)?;
        let stamp = Stamp {
            epoch,
            generation,
            frame: 0,
        };
        self.controls.insert(
            key,
            Control {
                worker,
                mailbox: Mailbox::new(stamp, assets),
                slot,
                source,
                part_generation,
                rate,
                maximum: max_host_frames,
                lead: worker_lead_packets,
                retired: Arc::new(AtomicBool::new(false)),
                exported: false,
                cancelled: false,
            },
        );
        Ok(())
    }

    /// Transfer an already initialized staging controller and its existing UI
    /// authority. The caller first rechecks its complete UviLoadKey (including
    /// catalog, source, sample rate and destination); no resource is reopened.
    #[allow(clippy::too_many_arguments)]
    pub fn adopt_prepared(
        &mut self,
        worker: Worker,
        mailbox: Mailbox,
        source: UviSource,
        epoch: u64,
        generation: u64,
        part_generation: u64,
        slot: usize,
        sample_rate: u32,
        max_host_frames: usize,
        worker_lead_packets: usize,
    ) -> Result<(), Error> {
        let actual = worker.activation_stamp();
        if epoch == 0
            || generation == 0
            || actual.epoch != epoch
            || actual.generation != generation
            || source.bank.as_os_str().is_empty()
            || source.member.is_empty()
            || !(8000..=192000).contains(&sample_rate)
        {
            return Err(Error::InvalidIdentity);
        }
        let key = (epoch, generation);
        if self.controls.contains_key(&key) {
            return Err(Error::DuplicateActivation);
        }
        match worker.status() {
            Status::Ready => {}
            Status::Starting => return Err(Error::NotReady),
            Status::Failed | Status::Stopped => return Err(Error::WorkerFailed),
        }
        self.controls.insert(
            key,
            Control {
                worker,
                mailbox,
                slot,
                source,
                part_generation,
                rate: sample_rate,
                maximum: max_host_frames,
                lead: worker_lead_packets,
                retired: Arc::new(AtomicBool::new(false)),
                exported: false,
                cancelled: false,
            },
        );
        Ok(())
    }

    /// Check all destination/source/context metadata before publishing a ready
    /// handoff. Caller additionally validates its current catalog/selection.
    #[allow(clippy::too_many_arguments)]
    pub fn matches(
        &self,
        epoch: u64,
        generation: u64,
        source: &UviSource,
        part_generation: u64,
        slot: usize,
        rate: u32,
        maximum: usize,
        lead: usize,
    ) -> bool {
        self.controls
            .get(&(epoch, generation))
            .is_some_and(|control| {
                !control.cancelled
                    && control.source == *source
                    && control.part_generation == part_generation
                    && control.slot == slot
                    && control.rate == rate
                    && control.maximum == maximum
                    && control.lead == lead
            })
    }

    pub fn status(&self, epoch: u64, generation: u64) -> Option<Status> {
        self.controls
            .get(&(epoch, generation))
            .map(|control| control.worker.status())
    }

    /// Exactly one audio endpoint is extracted. None is normal while starting
    /// or after export; queue-full handoff retries retain the same Audio value.
    pub fn take_ready(&mut self, epoch: u64, generation: u64) -> Result<Option<Audio>, Error> {
        let control = self
            .controls
            .get_mut(&(epoch, generation))
            .ok_or(Error::MissingActivation)?;
        if control.cancelled {
            return Err(Error::Cancelled);
        }
        if control.exported {
            return Ok(None);
        }
        match control.worker.status() {
            Status::Starting => return Ok(None),
            Status::Failed | Status::Stopped => return Err(Error::WorkerFailed),
            Status::Ready => {}
        }
        let port = control
            .worker
            .take_audio_port()
            .ok_or(Error::EndpointTaken)?;
        let bridge = Bridge::new(port, epoch, generation, control.maximum, control.lead)
            .map_err(Error::Bridge)?;
        let slot = Slot::new(bridge, epoch, generation).map_err(Error::Slot)?;
        let audio = Audio {
            slot: Some(Box::new(slot)),
            stamp: Stamp {
                epoch,
                generation,
                frame: 0,
            },
            destination: control.slot,
            part_generation: control.part_generation,
            retired: control.retired.clone(),
        };
        control.exported = true;
        Ok(Some(audio))
    }

    /// A superseded request may discard an unexported worker immediately. Once
    /// an endpoint was exported, cancellation suppresses UI/adoption but keeps
    /// its controller alive until endpoint retirement is actually acknowledged.
    pub fn cancel(&mut self, epoch: u64, generation: u64) {
        let key = (epoch, generation);
        if let Some(control) = self.controls.get_mut(&key) {
            control.cancelled = true;
            if control.removable() {
                self.controls.remove(&key);
            }
        }
    }

    /// Loader only: removing a Control stops/joins Worker and releases artwork.
    pub fn poll_retired(&mut self) -> usize {
        let before = self.controls.len();
        self.controls
            .retain(|_, control| !control.exported || !control.retired.load(Ordering::Acquire));
        before - self.controls.len()
    }

    pub fn poll_ui(&mut self, epoch: u64, generation: u64) -> Option<Arc<Published>> {
        let control = self.controls.get_mut(&(epoch, generation))?;
        if control.cancelled {
            return None;
        }
        control.mailbox.poll(&control.worker)
    }

    /// Off audio. False means exported endpoints still need their ordinary
    /// audio-to-loader retirement; never wait for them while holding audio.
    pub fn shutdown(&mut self) -> bool {
        self.controls.retain(|_, control| {
            control.cancelled = true;
            !control.removable()
        });
        self.controls.is_empty()
    }
}
impl Drop for Registry {
    fn drop(&mut self) {
        // The plugin's final Arc lease is structurally ordered after endpoints.
        debug_assert!(
            self.controls.values().all(Control::removable),
            "UVI controller registry dropped before endpoint retirement"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn fixture() -> (StartConfig, UviSource) {
        let (mut config, _) = crate::uvi::worker::tests::authored_bank_with_script(
            "function onNote(e)postEvent(e)end function onRelease(e)postEvent(e)end",
        );
        config.expected_bank_uuid = Some([0; 16]);
        let source = UviSource {
            bank: config.bank.clone(),
            bank_uuid: [0; 16],
            member: config.member.clone(),
        };
        (config, source)
    }

    #[test]
    fn exported_controller_waits_for_endpoint_receipt_even_after_cancel_and_shutdown() {
        let (config, source) = fixture();
        let path = source.bank.clone();
        let mut registry = Registry::default();
        registry
            .prepare(config, source.clone(), 1, 2, 0, 3, 257, 2)
            .unwrap();
        assert!(registry.matches(1, 2, &source, 0, 3, 48000, 257, 2));
        assert!(!registry.matches(1, 2, &source, 1, 3, 48000, 257, 2));
        assert!(!registry.matches(1, 2, &source, 0, 4, 48000, 257, 2));
        assert!(!registry.matches(1, 2, &source, 0, 3, 44100, 257, 2));
        registry.controls[&(1, 2)]
            .worker
            .wait_ready(Duration::from_secs(5))
            .unwrap();
        let audio = registry.take_ready(1, 2).unwrap().unwrap();
        assert_eq!(
            (
                audio.epoch(),
                audio.generation(),
                audio.destination(),
                audio.part_generation()
            ),
            (1, 2, 3, 0)
        );
        assert_eq!(audio.latency_frames(), 1024);
        assert!(registry.take_ready(1, 2).unwrap().is_none());
        let receipt = registry.controls[&(1, 2)].retired.clone();
        registry.cancel(1, 2);
        assert!(!receipt.load(Ordering::Acquire));
        assert_eq!(registry.status(1, 2), Some(Status::Ready));
        assert!(registry.poll_ui(1, 2).is_none());
        assert_eq!(registry.poll_retired(), 0);
        assert!(!registry.shutdown());
        assert_eq!(registry.status(1, 2), Some(Status::Ready));
        drop(audio); // actual Slot and AudioPort destruction, off callback
        assert!(receipt.load(Ordering::Acquire));
        assert_eq!(registry.poll_retired(), 1);
        assert_eq!(registry.status(1, 2), None);
        assert!(registry.shutdown());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn staged_controller_can_cancel_without_a_receipt_and_rejects_wrong_source() {
        let (config, source) = fixture();
        let path = source.bank.clone();
        let mut registry = Registry::default();
        let mut wrong = source.clone();
        wrong.member = "wrong.uvip".into();
        assert!(matches!(
            registry.prepare(config, wrong, 1, 2, 0, 0, 256, 1),
            Err(Error::InvalidIdentity)
        ));
        assert!(registry.controls.is_empty());
        std::fs::remove_file(path).unwrap();
        let (config, source) = fixture();
        let path = source.bank.clone();
        registry
            .prepare(config, source, 1, 2, 0, 0, 256, 1)
            .unwrap();
        assert!(registry.status(1, 2).is_some());
        registry.cancel(1, 2); // no endpoint was ever exported
        assert_eq!(registry.status(1, 2), None);
        assert!(registry.shutdown());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn prepared_controller_moves_once_and_final_registry_lease_follows_endpoint_drop() {
        let (config, source) = fixture();
        let path = source.bank.clone();
        let worker = Worker::start_hosted(config, 5, 6).unwrap();
        worker.wait_ready(Duration::from_secs(5)).unwrap();
        let mailbox = Mailbox::new(
            Stamp {
                epoch: 5,
                generation: 6,
                frame: 0,
            },
            None,
        );
        let mut registry = Registry::default();
        registry
            .adopt_prepared(worker, mailbox, source, 5, 6, 0, 0, 48000, 256, 1)
            .unwrap();
        let audio = registry.take_ready(5, 6).unwrap().unwrap();
        let receipt = registry.controls[&(5, 6)].retired.clone();
        let owner = Arc::new(std::sync::Mutex::new(registry));
        let weak = Arc::downgrade(&owner);
        struct DspLease {
            // Mirrors the required plugin ownership ordering, using the actual
            // endpoint/controller values rather than a synthetic drop protocol.
            _audio: Audio,
            _controllers_last: Arc<std::sync::Mutex<Registry>>,
        }
        let dsp = DspLease {
            _audio: audio,
            _controllers_last: owner.clone(),
        };
        drop(owner); // parameters/Shared may go away before Dsp
        assert!(weak.upgrade().is_some());
        assert!(!receipt.load(Ordering::Acquire));
        drop(dsp); // endpoint receipt precedes final registry/controller drop
        assert!(receipt.load(Ordering::Acquire));
        assert!(weak.upgrade().is_none());
        std::fs::remove_file(path).unwrap();
    }
}
