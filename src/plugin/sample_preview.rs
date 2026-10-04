//! Exact initial-source tickets and the serialized dry sample preparation lane.
//! PCM never enters editor feedback; instrument note/script audition is separate.
use super::*;
use moose::core::custom_state::{PersistField, StateCursor, StateField};
use std::sync::{LockResult, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockResult};

/// Existing Selection bytes and locks, with one non-persisted preview coordinator.
/// Public reads retain the existing lock/poison behavior. Public replacement and
/// persisted restoration fence playback before assignment; raw writes stay crate
/// private so the audited source-commit/capture distinction remains explicit.
pub struct SelectionStore {
    value: RwLock<Selection>,
    control: Arc<preview::Control>,
    pending: Mutex<Option<Request>>,
    prepared: Mutex<Option<Box<preview::Prepared>>>,
    feedback: Mutex<Option<Arc<Feedback>>>,
    #[cfg(test)]
    decode_panic: AtomicBool,
}
impl Default for SelectionStore {
    fn default() -> Self {
        Self { value: RwLock::default(), control: Arc::default(), pending: Mutex::default(),
            prepared: Mutex::default(), feedback: Mutex::default(),
            #[cfg(test)]
            decode_panic: AtomicBool::new(false),
        }
    }
}
impl PersistField for SelectionStore {
    fn persist_write(&self, buf: &mut Vec<u8>) { self.value.persist_write(buf); }
    fn persist_read(&self, cursor: &mut StateCursor) {
        // Use the unchanged migration codec. None rejects; Some, including an
        // admitted default/partial legacy document, is a restoration transaction.
        if let Some(value) = Selection::read_field(cursor) { self.replace(value); }
    }
}
impl SelectionStore {
    pub fn read(&self) -> LockResult<RwLockReadGuard<'_, Selection>> { self.value.read() }
    pub fn try_read(&self) -> TryLockResult<RwLockReadGuard<'_, Selection>> { self.value.try_read() }
    pub(crate) fn write(&self) -> LockResult<RwLockWriteGuard<'_, Selection>> { self.value.write() }
    /// An admitted whole-owner replacement. A poisoned lock preserves the owner.
    /// This experimental backend replaces the former public raw-write Rust API.
    pub fn replace(&self, value: Selection) -> bool {
        let Ok(mut current) = self.value.write() else { return false };
        self.control.stop();
        *current = value;
        true
    }
    fn publish_feedback(&self, feedback: Feedback) -> bool {
        let mut current = self.feedback.lock().unwrap();
        if !self.control.current(feedback.id) { return false; }
        *current = Some(Arc::new(feedback));
        true
    }
    pub(crate) fn preview_control(&self) -> &Arc<preview::Control> { &self.control }
    pub(crate) fn preview_source_commit(&self, slot: usize) { self.control.stop_slot(slot); }
    /// Call before the accepted editor assignment under its Selection write lock.
    /// Capture reconciliation has already happened; presentation/mix edits stay.
    pub(crate) fn preview_selection_commit(&self, before: &Selection, after: &Selection) {
        // The accepted Selection write guard excludes new begin(slot). Global
        // Stop may stale this metadata; stop_slot still rechecks the exact ID.
        let Some(slot) = self.control.owner_slot() else { return };
        let same = match (before.parts.get(slot), after.parts.get(slot)) {
            (Some(a), Some(b)) => a.path == b.path && a.program == b.program
                && a.snapshot == b.snapshot && a.uvi == b.uvi
                && a.script_state == b.script_state && a.ir_settings == b.ir_settings
                && a.engine_state == b.engine_state && a.delay_state == b.delay_state
                && a.uvi_state == b.uvi_state,
            (None, None) => true,
            _ => false,
        };
        if !same { self.control.stop_slot(slot); }
    }
}

#[derive(Clone)]
pub(crate) enum Ticket {
    Kontakt { instrument: Arc<Instrument>, index: usize },
    #[cfg(feature = "uvi")]
    Uvi { zones: Arc<Vec<crate::uvi::program::SampleZone>>, stamp: crate::uvi::worker::Stamp,
        index: usize, player: crate::uvi::program::NodeId },
}
impl Ticket {
    pub(crate) fn kontakt(instrument: Arc<Instrument>, index: usize) -> Option<Self> {
        instrument.zones.get(index)?;
        Some(Self::Kontakt { instrument, index })
    }
    #[cfg(feature = "uvi")]
    pub(crate) fn uvi(mapping: Arc<crate::uvi::mapping::Inspection>, index: usize) -> Option<Self> {
        let player = mapping.zones.get(index)?.player;
        Some(Self::Uvi { zones: mapping.zones.clone(), stamp: mapping.stamp, index, player })
    }
    pub(crate) fn same_target(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Kontakt { instrument: a, index: ai }, Self::Kontakt { instrument: b, index: bi }) =>
                ai == bi && Arc::ptr_eq(a, b),
            #[cfg(feature = "uvi")]
            (Self::Uvi { zones: a, stamp: ast, index: ai, player: ap },
                Self::Uvi { zones: b, stamp: bst, index: bi, player: bp }) =>
                ai == bi && ap == bp && ast == bst && Arc::ptr_eq(a, b),
            #[cfg(feature = "uvi")]
            _ => false,
        }
    }
}

pub(crate) struct Feedback {
    pub(crate) id: u64,
    pub(crate) source_rate: Option<u32>,
    pub(crate) source_frames: Option<u64>,
    pub(crate) source_channels: Option<usize>,
    pub(crate) host_rate: u32,
    pub(crate) output_frames: Option<usize>,
    pub(crate) reason: Option<String>,
}
struct Request {
    id: u64, slot: usize, ticket: Ticket, generation: u64, rate: u32,
    source: (String, u32, String), script_epoch: u64,
    #[cfg(feature = "uvi")]
    native: Option<library::UviSource>,
}
impl Request {
    fn matches(&self, params: &SamplerParams, selection: &Selection, view: &View) -> bool {
        if !params.selection.control.current(self.id) || params.shared.rate() != f64::from(self.rate)
            || params.shared.part(self.slot).is_none_or(|p| p.generation.load(Ordering::Acquire) != self.generation) {
            return false;
        }
        let Some(part) = selection.parts.get(self.slot) else { return false };
        let Some(v) = view.parts.get(self.slot) else { return false };
        match &self.ticket {
            Ticket::Kontakt { instrument, index } => part.uvi.is_none() && part.matches_source(&self.source)
                && v.attempted.as_ref() == Some(&self.source) && v.script_epoch == self.script_epoch
                && !v.loading && v.instrument.as_ref().is_some_and(|i| Arc::ptr_eq(i, instrument))
                && instrument.zones.get(*index).is_some_and(|zone| zone.available && !zone.sample.as_os_str().is_empty()),
            #[cfg(feature = "uvi")]
            Ticket::Uvi { zones, stamp, index, player } => part.uvi == self.native
                && v.uvi_mapping_inspection(params, self.slot, selection).is_some_and(|(mapping, _)|
                    mapping.stamp == *stamp && Arc::ptr_eq(&mapping.zones, zones)
                    && mapping.zones.get(*index).is_some_and(|zone| zone.player == *player && !zone.sample_path.is_empty())),
        }
    }
    fn current(&self, params: &SamplerParams) -> bool {
        let view = params.shared.view.lock().unwrap();
        let selection = params.selection.read().unwrap();
        self.matches(params, &selection, &view)
    }
    fn decode(&self, params: &SamplerParams) -> anyhow::Result<crate::preview_prepare::DecodedPreview> {
        #[cfg(test)]
        if params.selection.decode_panic.swap(false, Ordering::AcqRel) { panic!("controlled malformed codec geometry"); }
        let canceled = || !params.selection.control.current(self.id);
        anyhow::ensure!(!canceled(), "Sample preview was canceled");
        match &self.ticket {
            Ticket::Kontakt { instrument, index } => {
                anyhow::ensure!(crate::cache::current(&instrument.dependencies), "The selected sample source changed");
                let source = crate::audio::Sources::default().source(&instrument.zones[*index].sample)?;
                let decoded = crate::preview_prepare::prepare_kontakt(&source, &instrument.dependencies, self.rate, &canceled)?;
                anyhow::ensure!(crate::cache::current(&instrument.dependencies), "The selected sample source changed");
                Ok(decoded)
            }
            #[cfg(feature = "uvi")]
            Ticket::Uvi { zones, index, .. } => {
                let source = self.native.as_ref().ok_or_else(|| anyhow::anyhow!("The selected UVI source changed"))?;
                let version = crate::cache::version(&source.bank)
                    .ok_or_else(|| anyhow::anyhow!("The selected UVI bank could not be inspected"))?;
                let config = params.shared.libraries.uvi_worker_config(source, self.rate).map_err(anyhow::Error::msg)?;
                anyhow::ensure!(!canceled(), "Sample preview was canceled");
                let library = crate::uvi::library::Library::open(&config.bank, &config.metadata_namespace, config.content_key)?;
                anyhow::ensure!(library.bank.header.uuid == source.bank_uuid, "The selected UVI bank identity changed");
                if let Some(identity) = &config.content_bank {
                    anyhow::ensure!(identity == &library.bank.header.bank_name
                        || std::fs::canonicalize(identity).ok() == Some(std::fs::canonicalize(&config.bank)?),
                        "UVI content state belongs to a different bank");
                }
                let decoded = crate::preview_prepare::prepare_uvi(&library, &source.member,
                    &zones[*index].sample_path, self.rate, &canceled)?;
                anyhow::ensure!(crate::cache::version(&source.bank) == Some(version), "The selected UVI bank changed");
                Ok(decoded)
            }
        }
    }
}

impl SamplerParams {
    /// UI submits an immutable initial-source ticket, never a filename authority.
    /// View -> Selection remains held through ID issuance, closing commit races.
    pub(crate) fn request_sample_preview(&self, slot: usize, ticket: Ticket) -> Option<u64> {
        let view = self.shared.view.lock().unwrap();
        let selection = self.selection.read().unwrap();
        let part = selection.parts.get(slot)?;
        let v = view.parts.get(slot)?;
        let rate = self.shared.rate();
        if !(8000. ..=192000.).contains(&rate) || rate.fract() != 0. { return None; }
        let generation = self.shared.part(slot)?.generation.load(Ordering::Acquire);
        // Prevent a new Kontakt preview from accepting restored state before its
        // loader establishes the matching baseline. Internal capture preserves an
        // already admitted request; it does not freeze those state bytes forever.
        if matches!(&ticket, Ticket::Kontakt { .. }) && (part.script_state != v.script_state
            || part.ir_settings != v.ir_settings || part.engine_state.as_slice() != v.engine_state.as_ref()
            || part.delay_state.as_slice() != v.delay_state.as_ref()) { return None; }
        let mut request = Request { id: 0, slot, ticket, generation, rate: rate as u32,
            source: part.source(), script_epoch: v.script_epoch,
            #[cfg(feature = "uvi")]
            native: part.uvi.clone(),
        };
        // Validate without issuing an ID: a rejected foreign/stale ticket must
        // not interrupt the current preview. matches' ID clause is separate.
        let valid = match &request.ticket {
            Ticket::Kontakt { instrument, index } => part.uvi.is_none() && !v.loading
                && v.attempted.as_ref() == Some(&request.source)
                && v.instrument.as_ref().is_some_and(|i| Arc::ptr_eq(i, instrument))
                && instrument.zones.get(*index).is_some_and(|zone| zone.available && !zone.sample.as_os_str().is_empty()),
            #[cfg(feature = "uvi")]
            Ticket::Uvi { zones, stamp, index, player } => v.uvi_mapping_inspection(self, slot, &selection)
                .is_some_and(|(mapping, _)| mapping.stamp == *stamp && Arc::ptr_eq(&mapping.zones, zones)
                    && mapping.zones.get(*index).is_some_and(|zone| zone.player == *player && !zone.sample_path.is_empty())),
        };
        if !valid { return None; }
        let id = self.selection.control.begin(slot)?;
        request.id = id;
        if !self.selection.publish_feedback(Feedback { id, source_rate: None,
            source_frames: None, source_channels: None, host_rate: request.rate, output_frames: None, reason: None }) { return None; }
        *self.selection.pending.lock().unwrap() = Some(request);
        self.selection.control.current(id).then_some(id)
    }
    pub(crate) fn stop_sample_preview(&self, id: u64) -> bool { self.selection.control.stop_id(id) }
    pub(crate) fn sample_preview_feedback(&self, id: u64) -> Option<Arc<Feedback>> {
        if !self.selection.control.current(id) { return None; }
        self.selection.feedback.lock().unwrap().as_ref().filter(|feedback| feedback.id == id).cloned()
            .filter(|_| self.selection.control.current(id))
    }
    pub(crate) fn sample_preview_pending(&self) -> bool {
        // Mailbox-full PCM retries on the callback ready_taken wake, not every
        // editor frame. This signal admits only genuinely new preparation work.
        self.selection.pending.lock().unwrap().is_some()
    }
}

/// One producer, separate from long whole-instrument Load tasks. Callback frees
/// remain on Load via Retired; a taken ready entry wakes this task for backpressure.
pub(crate) struct Prepare;
impl BackgroundTask for Prepare {
    type Params = SamplerParams;
    const SERIALIZED: bool = true;
    fn run(self, params: &SamplerParams) {
        let control = params.selection.preview_control();
        loop {
            // Brief queue locks only. Never retain these while taking View or
            // Selection locks or decoding; admission uses the opposite order.
            let request = params.selection.pending.lock().unwrap().take();
            if let Some(request) = request {
                // Superseded off-audio payload is freed here, not on GUI/audio.
                params.selection.prepared.lock().unwrap().take();
                if !request.current(params) { continue; }
                // Research/codec parsers can panic on malformed frame geometry.
                // Contain it on this off-audio lane; never publish panic payloads.
                let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| request.decode(params)))
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("Sample preview decoder failed")));
                if !request.current(params) { continue; }
                match decoded {
                    Ok(decoded) => {
                        let frames = decoded.frames.len();
                        let prepared = preview::Prepared::new(request.id, request.rate, decoded.frames);
                        match prepared {
                            Ok(prepared) => {
                                params.selection.publish_feedback(Feedback { id: request.id,
                                    source_rate: Some(decoded.source_rate), source_frames: Some(decoded.source_frames),
                                    source_channels: decoded.source_channels, host_rate: request.rate,
                                    output_frames: Some(frames), reason: None });
                                *params.selection.prepared.lock().unwrap() = Some(prepared);
                            }
                            Err(error) => failure(params, &request, error),
                        }
                    }
                    Err(error) => failure(params, &request, error),
                }
            }
            let prepared = params.selection.prepared.lock().unwrap().take();
            let Some(prepared) = prepared else { break };
            if !control.current(prepared.id) { continue; }
            if let Err(prepared) = params.shared.preview.publish(control, prepared) {
                if control.current(prepared.id) { *params.selection.prepared.lock().unwrap() = Some(prepared); }
                break;
            }
        }
    }
}
fn failure(params: &SamplerParams, request: &Request, error: anyhow::Error) {
    if !params.selection.control.current(request.id) || crate::preview_prepare::is_canceled(&error) { return; }
    let unsupported = crate::preview_prepare::unsupported_reason(&error);
    let status = if unsupported.is_some() { preview::Status::Unsupported } else { preview::Status::Failed };
    params.selection.publish_feedback(Feedback { id: request.id,
        source_rate: None, source_frames: None, source_channels: None, host_rate: request.rate,
        output_frames: None, reason: Some(unsupported.map(str::to_owned).unwrap_or_else(|| error.to_string().chars().take(256).collect())) });
    params.selection.control.set_status(request.id, status);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persisted(selection: &Selection) -> Vec<u8> {
        let mut value = Vec::new(); selection.write_field(&mut value);
        let mut outer = Vec::new();
        1u32.write_field(&mut outer); "selection".to_owned().write_field(&mut outer);
        (value.len() as u32).write_field(&mut outer); outer.extend(value); outer
    }
    fn owner() -> Selection {
        Selection { parts: vec![Part { path: "/virtual/preview.nki".into(), ..Default::default() }],
            ..Default::default() }
    }
    fn fixture() -> (SamplerParams, Arc<Instrument>) {
        let params = SamplerParams::default(); params.shared.ensure_parts(1);
        params.shared.rate.store(48000f64.to_bits(), Ordering::Release);
        let selected = owner(); params.selection.replace(selected.clone());
        let instrument = Arc::new(Instrument { path: selected.parts[0].path.clone().into(),
            zones: vec![crate::import::Zone { sample: "/virtual/preview.wav".into(), ..Default::default() };
                2], ..Default::default() });
        let mut view = params.shared.view.lock().unwrap();
        view.parts[0].instrument = Some(instrument.clone());
        view.parts[0].attempted = Some(selected.parts[0].source());
        view.parts[0].script_epoch = 7; drop(view);
        (params, instrument)
    }

    #[test]
    fn persisted_reject_and_admitted_migration_keep_exact_existing_codec_policy() {
        let store = SelectionStore::default(); assert!(store.replace(owner()));
        let id = store.control.begin(0).unwrap();
        let mut original = Vec::new(); store.persist_write(&mut original);
        let mut raw = Vec::new(); store.read().unwrap().write_field(&mut raw);
        assert_eq!(original, raw, "coordinator adds no persisted bytes");
        for rejected in [vec![], vec![4, 0, 0, 0, 0], vec![0, 0, 0, 0]] {
            assert!(Selection::read_field(&mut StateCursor::new(&rejected)).is_none());
            store.persist_read(&mut StateCursor::new(&rejected));
            assert!(store.control.current(id)); assert!(*store.read().unwrap() == owner());
        }
        // The existing legacy decoder admits zero stored fields as defaults.
        let admitted: Vec<u8> = [4u32, 0].into_iter().flat_map(u32::to_le_bytes).collect();
        assert!(Selection::read_field(&mut StateCursor::new(&admitted)) == Some(Selection::default()));
        store.persist_read(&mut StateCursor::new(&admitted));
        assert!(!store.control.current(id)); assert!(*store.read().unwrap() == Selection::default());
    }

    #[test]
    fn direct_load_persist_samepath_replacement_fences_before_loader_generation_changes() {
        let (params, instrument) = fixture();
        let generation = params.shared.part(0).unwrap().generation.load(Ordering::Acquire);
        let id = params.request_sample_preview(0, Ticket::kontakt(instrument.clone(), 0).unwrap()).unwrap();
        let before = params.selection.read().unwrap().clone();
        params.load_persist(&persisted(&before));
        assert!(*params.selection.read().unwrap() == before);
        assert!(!params.selection.control.current(id), "equivalent admitted restore is a replacement");
        assert_eq!(params.shared.part(0).unwrap().generation.load(Ordering::Acquire), generation);
        assert!(params.shared.view.lock().unwrap().parts[0].instrument.as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &instrument)));
    }

    #[test]
    fn capture_writeback_and_other_slot_edits_preserve_but_source_restore_commit_cancels() {
        let store = SelectionStore::default(); store.replace(owner());
        let id = store.control.begin(0).unwrap();
        let before = store.read().unwrap().clone();
        let mut after = before.clone(); after.parts[0].gain = -6.; after.parts[0].name = "Renamed".into();
        after.parts.push(Part { path: "/virtual/other.nki".into(), ..Default::default() });
        { let mut current = store.write().unwrap(); store.preview_selection_commit(&current, &after); *current = after; }
        assert!(store.control.current(id));
        // Existing internal native capture owns this write: no replacement fence.
        store.write().unwrap().parts[0].uvi_state = vec![1, 2].into();
        assert!(store.control.current(id));
        let mut after = store.read().unwrap().clone(); after.parts[0].snapshot = "samepath.nksn".into();
        { let mut current = store.write().unwrap(); store.preview_selection_commit(&current, &after); *current = after; }
        assert!(!store.control.current(id));
    }

    #[test]
    fn source_cas_rejection_and_foreign_samepath_ticket_preserve_current_receipt() {
        let (params, instrument) = fixture();
        let id = params.request_sample_preview(0, Ticket::kontakt(instrument.clone(), 0).unwrap()).unwrap();
        let before = params.selection.read().unwrap().clone();
        params.selection.write().unwrap().parts[0].gain = -3.;
        let mut edited = before.clone(); edited.parts[0].path = "/virtual/rejected.nki".into();
        { let mut current = params.selection.write().unwrap();
            if *current == before { params.selection.preview_selection_commit(&current, &edited); *current = edited; } }
        assert!(params.selection.control.current(id));
        let foreign = Arc::new(Instrument { path: instrument.path.clone(), zones: instrument.zones.clone(), ..Default::default() });
        assert!(params.request_sample_preview(0, Ticket::kontakt(foreign, 0).unwrap()).is_none());
        assert!(params.selection.control.current(id));
        let first = Ticket::kontakt(instrument.clone(), 0).unwrap();
        let second = Ticket::kontakt(instrument.clone(), 1).unwrap();
        assert!(!first.same_target(&second), "duplicate paths remain distinct zones");
        assert!(first.same_target(&Ticket::kontakt(instrument, 0).unwrap()));
    }

    #[test]
    fn superseded_or_stopped_producer_cannot_replace_new_feedback_or_revive_receipt() {
        let (params, instrument) = fixture();
        let first = params.request_sample_preview(0, Ticket::kontakt(instrument.clone(), 0).unwrap()).unwrap();
        let second = params.request_sample_preview(0, Ticket::kontakt(instrument, 1).unwrap()).unwrap();
        assert!(!params.stop_sample_preview(first));
        assert!(!params.selection.publish_feedback(Feedback { id: first, source_rate: Some(1),
            source_frames: Some(1), source_channels: Some(1), host_rate: 48000, output_frames: Some(1), reason: None }));
        assert_eq!(params.sample_preview_feedback(second).unwrap().id, second);
        params.selection.control.stop(); // reset after begin, before service takes queued work
        Prepare.run(&params);
        assert!(params.sample_preview_feedback(second).is_none());
        assert!(!params.sample_preview_pending()); assert!(!params.selection.control.current(second));
    }

    #[test]
    fn real_selected_wav_preparation_publishes_dimensions_before_dry_audio() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("kontra-dry-preview-{}-{}.wav", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let mut wav = hound::WavWriter::create(&path, hound::WavSpec { channels: 2, sample_rate: 48000,
            bits_per_sample: 32, sample_format: hound::SampleFormat::Float }).unwrap();
        for _ in 0..8 { wav.write_sample(0.25f32).unwrap(); wav.write_sample(-0.5f32).unwrap(); }
        wav.finalize().unwrap();
        let (params, initial) = fixture();
        let mut instrument = Instrument { path: initial.path.clone(), zones: initial.zones.clone(), ..Default::default() };
        instrument.zones[1].sample = path.clone(); let instrument = Arc::new(instrument);
        params.shared.view.lock().unwrap().parts[0].instrument = Some(instrument.clone());
        let id = params.request_sample_preview(0, Ticket::kontakt(instrument, 1).unwrap()).unwrap();
        Prepare.run(&params);
        let feedback = params.sample_preview_feedback(id).unwrap();
        assert_eq!((feedback.source_rate, feedback.source_frames, feedback.source_channels, feedback.output_frames),
            (Some(48000), Some(8), Some(2), Some(8)));
        assert!(feedback.reason.is_none()); assert_eq!(params.selection.control.status(id), Some(preview::Status::Ready));
        let mut cursor = preview::Cursor::default();
        assert!(cursor.poll(&params.selection.control, &params.shared.preview, &params.shared.discard, 48000.).ready_taken);
        let mut out = [[0.; 2]; 8]; assert!(cursor.render(&params.selection.control, 48000., &mut out));
        assert_eq!(out, [[0.25, -0.5]; 8], "raw stereo source, no program envelopes or script notes");
        assert!(cursor.poll(&params.selection.control, &params.shared.preview, &params.shared.discard, 48000.).retired);
        drop(params.shared.discard.pop()); // off-audio disposal
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod fault_tests {
    use super::*;

    #[test]
    fn poisoned_replacement_and_persisted_assignment_preserve_owner_and_receipt() {
        let store = SelectionStore::default();
        let original = Selection { parts: vec![Part { path: "/virtual/poison.nki".into(), ..Default::default() }], ..Default::default() };
        store.replace(original.clone()); let id = store.control.begin(0).unwrap();
        let mut bytes = Vec::new(); Selection::default().write_field(&mut bytes);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _current = store.write().unwrap(); panic!("controlled Selection poison");
        }));
        assert!(panicked.is_err()); assert!(!store.replace(Selection::default()));
        store.persist_read(&mut StateCursor::new(&bytes));
        assert!(store.control.current(id));
        match store.read() { Err(error) => assert!(*error.into_inner() == original),
            Ok(_) => panic!("Selection lock must retain poison"), }
    }

    #[test]
    fn codec_panic_is_contained_and_publishes_current_generic_failure_only() {
        let params = SamplerParams::default(); params.shared.ensure_parts(1);
        params.shared.rate.store(48000f64.to_bits(), Ordering::Release);
        let part = Part { path: "/virtual/codec-panic.nki".into(), ..Default::default() };
        params.selection.replace(Selection { parts: vec![part.clone()], ..Default::default() });
        let instrument = Arc::new(Instrument { path: part.path.clone().into(),
            zones: vec![crate::import::Zone { sample: "/virtual/panic.wav".into(), ..Default::default() }], ..Default::default() });
        { let mut view = params.shared.view.lock().unwrap(); view.parts[0].attempted = Some(part.source());
            view.parts[0].instrument = Some(instrument.clone()); }
        let id = params.request_sample_preview(0, Ticket::kontakt(instrument.clone(), 0).unwrap()).unwrap();
        params.selection.decode_panic.store(true, Ordering::Release);
        Prepare.run(&params);
        assert_eq!(params.selection.control.status(id), Some(preview::Status::Failed));
        let feedback = params.sample_preview_feedback(id).unwrap();
        assert_eq!(feedback.reason.as_deref(), Some("Sample preview decoder failed"));
        assert!(feedback.output_frames.is_none()); assert!(feedback.source_frames.is_none());
        let fresh = params.request_sample_preview(0, Ticket::kontakt(instrument, 0).unwrap()).unwrap();
        failure(&params, &Request { id, slot: 0, ticket: params.selection.pending.lock().unwrap().as_ref().unwrap().ticket.clone(),
            generation: 0, rate: 48000, source: part.source(), script_epoch: 0,
            #[cfg(feature = "uvi")] native: None }, anyhow::anyhow!("superseded private diagnostic"));
        assert_eq!(params.sample_preview_feedback(fresh).unwrap().reason, None);
        assert_eq!(params.selection.control.status(fresh), Some(preview::Status::Preparing));
    }
}
