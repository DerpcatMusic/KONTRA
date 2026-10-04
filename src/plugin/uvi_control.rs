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
        Arc,
        atomic::{AtomicBool, Ordering},
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
    State,
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
    state_pending: Option<(u64, Stamp)>,
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
        self.prepare_with_state(
            config,
            source,
            epoch,
            generation,
            part_generation,
            slot,
            max_host_frames,
            worker_lead_packets,
            &[],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_with_state(
        &mut self,
        config: StartConfig,
        source: UviSource,
        epoch: u64,
        generation: u64,
        part_generation: u64,
        slot: usize,
        max_host_frames: usize,
        worker_lead_packets: usize,
        saved: &[u8],
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
        let mut trace = crate::diagnostics::LoadTrace::new(&source.bank, 0, Some(slot));
        trace.detail("backend", "uvi");
        trace.detail("member", source.member.clone());
        trace.detail("epoch", epoch);
        trace.detail("generation", generation);
        trace.stage("uvi_controller_setup");
        let assets = match UiAssets::open(&config) {
            Ok(assets) => Some(assets),
            Err(error) => {
                trace.issue(
                    "ui",
                    "uvi_artwork_authority_unavailable",
                    format!("{error:#}"),
                );
                None
            }
        };
        let worker = if saved.is_empty() {
            Worker::start_hosted(config, epoch, generation)
        } else {
            let state = match crate::uvi::state::SavedState::decode(saved) {
                Ok(state) => state,
                Err(error) => {
                    trace.fail(format!("UVI saved state: {error:#}"));
                    trace.finish("failed");
                    return Err(Error::State);
                }
            };
            Worker::start_hosted_with_state(config, epoch, generation, state)
        };
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                trace.fail(format!("Starting UVI worker: {error:#}"));
                trace.finish("failed");
                return Err(Error::Start);
            }
        };
        trace.finish("worker_started");
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
                state_pending: None,
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
                state_pending: None,
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

    /// Control/editor thread only. Includes measured packet counters, never a
    /// claim that every decoded node has executed or matches Falcon.
    pub fn diagnostic_report(&self) -> serde_json::Value {
        self.report(None)
    }

    /// Explicit support export only. One deadline bounds all rack workers.
    pub fn runtime_diagnostic_report(&self, timeout: std::time::Duration) -> serde_json::Value {
        self.report(Some(timeout))
    }

    fn report(&self, timeout: Option<std::time::Duration>) -> serde_json::Value {
        let started = std::time::Instant::now();
        let mut controls: Vec<_> = self.controls.iter().collect();
        controls.sort_by_key(|(identity, _)| **identity);
        serde_json::json!(
            controls
                .into_iter()
                .map(|(&(epoch, generation), control)| {
                    serde_json::json!({
                        "epoch":epoch, "generation":generation, "slot":control.slot,
                        "source":control.source, "sample_rate":control.rate,
                        "max_host_frames":control.maximum, "lead_packets":control.lead,
                        "endpoint_exported":control.exported, "cancelled":control.cancelled,
                        "endpoint_retired":control.retired.load(Ordering::Acquire),
                        "worker":if let Some(timeout)=timeout.filter(|_|!control.cancelled && !control.retired.load(Ordering::Acquire)) {
                            control.worker.runtime_diagnostic_report(timeout.saturating_sub(started.elapsed()))
                        } else {control.worker.diagnostic_report()},
                    })
                })
                .collect::<Vec<_>>()
        )
    }

    pub fn status(&self, epoch: u64, generation: u64) -> Option<Status> {
        self.controls
            .get(&(epoch, generation))
            .map(|control| control.worker.status())
    }

    pub fn initialization_progress(&self, epoch: u64, generation: u64) -> Option<(&'static str, std::time::Duration)> {
        self.controls.get(&(epoch,generation)).filter(|control| !control.cancelled)
            .and_then(|control|control.worker.initialization_progress())
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
        let slot = Slot::new(bridge, epoch, generation, control.rate).map_err(Error::Slot)?;
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

    /// Serialized control lane only. Explicit saves replace an older pending
    /// request; its reply cannot overwrite the newer state.
    pub fn request_state(&mut self, stamp: Stamp, force: bool) -> Result<bool, Error> {
        let control = self
            .controls
            .get_mut(&(stamp.epoch, stamp.generation))
            .ok_or(Error::MissingActivation)?;
        if control.cancelled || !control.exported || control.retired.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        if !force && control.state_pending.is_some() {
            return Ok(false);
        }
        let request = control
            .worker
            .request_state_snapshot(stamp)
            .map_err(|_| Error::State)?;
        control.state_pending = Some((request, stamp));
        Ok(true)
    }

    pub fn poll_state(
        &mut self,
        epoch: u64,
        generation: u64,
    ) -> Option<Result<(Stamp, Vec<u8>), Error>> {
        let control = self.controls.get_mut(&(epoch, generation))?;
        if control.cancelled {
            return None;
        }
        let reply = control.worker.poll_state_snapshot()?;
        let (request, minimum) = control.state_pending?;
        if reply.request != request
            || reply.stamp.epoch != epoch
            || reply.stamp.generation != generation
            || reply.stamp.frame < minimum.frame
        {
            return None;
        }
        control.state_pending = None;
        Some(reply.snapshot.map_err(|_| Error::State).and_then(|state| {
            state
                .encode()
                .map(|bytes| (reply.stamp, bytes))
                .map_err(|_| Error::State)
        }))
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
/// Support exports have a finite context budget. Keep aggregate evidence and
/// full worker-owned snapshots, while explicitly reporting any omitted rows in
/// this export copy. Processing, rejection and bypass evidence is retained first.
pub(super) fn bound_node_reports(report: &mut serde_json::Value, maximum_bytes: usize) {
    fn rows(value: &serde_json::Value) -> usize {
        match value {
            serde_json::Value::Object(fields) => fields
                .iter()
                .map(|(key, value)| {
                    if key == "nodes" {
                        value.as_array().map_or(0, Vec::len)
                    } else {
                        rows(value)
                    }
                })
                .sum(),
            serde_json::Value::Array(values) => values.iter().map(rows).sum(),
            _ => 0,
        }
    }
    fn trim(value: &mut serde_json::Value) -> usize {
        match value {
            serde_json::Value::Object(fields) => {
                let previous_total = fields
                    .get("node_coverage")
                    .and_then(|v| v["total_rows"].as_u64());
                let mut removed = 0;
                if let Some(nodes) = fields
                    .get_mut("nodes")
                    .and_then(serde_json::Value::as_array_mut)
                {
                    let total = previous_total.unwrap_or(nodes.len() as u64);
                    nodes.sort_by_key(|node| {
                        !(node["processed_blocks"].as_u64().is_some_and(|n| n > 0)
                            || node["retained_voice_instances"]
                                .as_u64()
                                .is_some_and(|n| n > 0)
                            || node["currently_bypassed"].as_bool() == Some(true)
                            || node["preflight_rejections"]
                                .as_array()
                                .is_some_and(|rejections| !rejections.is_empty()))
                    });
                    let keep = nodes.len() / 2;
                    removed = nodes.len() - keep;
                    nodes.truncate(keep);
                    fields.insert("node_coverage".into(),serde_json::json!({"total_rows":total,
                        "included_rows":keep,"omitted_rows":total-keep as u64,"complete":total==keep as u64,
                        "reason":"support_context_size_limit","order":"processing_bypass_or_rejection_evidence_first"}));
                }
                removed
                    + fields
                        .iter_mut()
                        .filter(|(key, _)| key.as_str() != "nodes")
                        .map(|(_, value)| trim(value))
                        .sum::<usize>()
            }
            serde_json::Value::Array(values) => values.iter_mut().map(trim).sum(),
            _ => 0,
        }
    }
    let total = rows(report);
    loop {
        let included = rows(report);
        report["node_report_coverage"] = serde_json::json!({"included_rows":included,
            "omitted_rows":total-included,"complete":total==included,"maximum_bytes":maximum_bytes,
            "size_limit_met":true,"full_node_reports":"uvi-diagnose or uvi-play --worker"});
        if serde_json::to_vec(&*report).is_ok_and(|bytes| bytes.len() <= maximum_bytes) {
            break;
        }
        if trim(report) == 0 {
            report["node_report_coverage"]["size_limit_met"] = serde_json::json!(false);
            break;
        }
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
    #[test]
    fn native_state_mailbox_roundtrips_controls_and_original_parameter_without_ui_replay() {
        use crate::uvi::host::{UiEdit, UiEditValue, UiModifiers, UiValue};
        let (mut config, _) = crate::uvi::worker::tests::authored_bank_with_script(
            "n=Knob('n',0.25,0,1);function n:changed()Program:setParameter('Gain',self.value)end;function onSave()return {saved=true}end;function onLoad(s)assert(s.saved and Program:getParameter('Gain')==0.75)end",
        );
        config.expected_bank_uuid = Some([0; 16]);
        let restored_config = StartConfig {
            bank: config.bank.clone(),
            expected_bank_uuid: config.expected_bank_uuid,
            member: config.member.clone(),
            metadata_namespace: config.metadata_namespace.clone(),
            program_namespace: config.program_namespace.clone(),
            content_key: config.content_key,
            content_bank: config.content_bank.clone(),
            sample_rate: config.sample_rate,
        };
        let source = UviSource {
            bank: config.bank.clone(),
            bank_uuid: [0; 16],
            member: config.member.clone(),
        };
        let mut registry = Registry::default();
        registry
            .prepare(config, source.clone(), 10, 20, 1, 0, 256, 1)
            .unwrap();
        registry.controls[&(10, 20)]
            .worker
            .wait_ready(Duration::from_secs(3))
            .unwrap();
        let processor = registry.controls[&(10, 20)].worker.ui_processors()[0];
        let mut audio = registry.take_ready(10, 20).unwrap().unwrap();
        audio
            .slot_mut()
            .push_ui(UiEdit {
                processor,
                widget: 1,
                value: UiEditValue::Number(0.75),
                modifiers: UiModifiers::default(),
            })
            .unwrap();
        let mut left = [0.; 256];
        let mut right = [0.; 256];
        audio
            .slot_mut()
            .process_mode(&mut left, &mut right, true)
            .unwrap();
        let stamp = Stamp {
            epoch: 10,
            generation: 20,
            frame: 256,
        };
        assert!(registry.request_state(stamp, false).unwrap());
        assert!(
            !registry.request_state(stamp, false).unwrap(),
            "pending requests coalesce"
        );
        assert!(
            registry.request_state(stamp, true).unwrap(),
            "explicit save supersedes pending request"
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let bytes = loop {
            if let Some(reply) = registry.poll_state(10, 20) {
                let (actual, bytes) = reply.unwrap();
                assert!(actual.frame >= 256);
                break bytes;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        registry.cancel(10, 20);
        drop(audio);
        registry.poll_retired();
        registry
            .prepare_with_state(
                restored_config,
                source.clone(),
                10,
                21,
                2,
                0,
                256,
                1,
                &bytes,
            )
            .unwrap();
        registry.controls[&(10, 21)]
            .worker
            .wait_ready(Duration::from_secs(3))
            .unwrap();
        let worker = &registry.controls[&(10, 21)].worker;
        let request = worker.request_ui_snapshot(processor).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(reply) = worker.poll_ui_snapshot() {
                assert_eq!(reply.request, request);
                assert!(reply.snapshot.unwrap().widgets[0].value == Some(UiValue::Number(0.75)));
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        registry.cancel(10, 21);
        std::fs::remove_file(source.bank).unwrap();
    }
    #[test]
    fn support_context_bounds_two_large_workers_and_declares_omitted_rows() {
        let workers: Vec<_> = (0..2)
            .map(|slot| {
                let program_nodes: Vec<_> = (0..4000)
                    .map(|node| {
                        serde_json::json!({"id":node,
                "kind":"MinBlepGenerator","name":"bounded authored name",
                "parent":0,"initially_bypassed":false,
                "preflight_rejections":if node==3998 {vec!["authored rejection"]} else {vec![]}})
                    })
                    .collect();
                let runtime_nodes: Vec<_> = (0..4000)
                    .map(|node| {
                        serde_json::json!({"node":node,
                "kind":"MinBlepGenerator","processed_blocks":if node==3999 {7}else{0},
                "currently_bypassed":false,"retained_voice_instances":if node==3999 {2}else{0},
                "evidence_source":"renderer_native_dispatch"})
                    })
                    .collect();
                serde_json::json!({"slot":slot,"worker":{"stats":{"rendered_blocks":7},
                "program":{"parsed":true,"counts":{"nodes":4000},"nodes":program_nodes},
                "runtime_snapshot":{"stamp":{"frame":1792},"evidence":{"nodes":runtime_nodes,
                    "audibility_verified":false,"falcon_numerical_fidelity_verified":false}}}})
            })
            .collect();
        let mut report = serde_json::json!({"rack_workers":workers});
        bound_node_reports(&mut report, 64 * 1024);
        assert!(serde_json::to_vec(&report).unwrap().len() <= 64 * 1024);
        assert_eq!(report["node_report_coverage"]["complete"], false);
        assert!(
            report["node_report_coverage"]["omitted_rows"]
                .as_u64()
                .unwrap()
                > 0
        );
        for control in report["rack_workers"].as_array().unwrap() {
            let worker = &control["worker"];
            assert_eq!(worker["stats"]["rendered_blocks"], 7);
            assert_eq!(worker["program"]["counts"]["nodes"], 4000);
            assert_eq!(worker["program"]["node_coverage"]["complete"], false);
            assert!(
                worker["program"]["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|node| node["id"] == 3998)
            );
            assert_eq!(worker["runtime_snapshot"]["stamp"]["frame"], 1792);
            let evidence = &worker["runtime_snapshot"]["evidence"];
            assert_eq!(evidence["node_coverage"]["complete"], false);
            assert!(
                evidence["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|node| node["node"] == 3999),
                "processed/retained nodes survive truncation ahead of unknown or unused rows"
            );
            assert_eq!(evidence["falcon_numerical_fidelity_verified"], false);
        }
    }
}
