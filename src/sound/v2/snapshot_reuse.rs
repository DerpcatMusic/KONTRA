//! Off-audio admission for retaining the exact Kontakt script/UI owner.
//! Native graph changes deliberately take the ordinary private load path until
//! their complete incoming state has a represented live transaction.
use super::*;
use crate::sound::resource_revision::ResourceRevision;
use sampler_ksp::setup::{
    SetupIdentity, SnapshotSavedLimits, SnapshotSavedSlot, snapshot_policy, snapshot_saved_state,
};
pub(crate) mod admission;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Refusal {
    NoSetup,
    InitRequired,
    NativeState,
    SourceOrResources,
    SetupChanged,
    SnapshotRead,
    SavedState,
    MenuRosterUnverified,
    Queue,
    Worker,
    OwnerChanged,
}

/// Captured before parsing/initialization, then checked after preparation.
/// This prevents a later file revision being attributed to older code.
pub(crate) struct Seed {
    base: PathBuf,
    rate: u32,
    authority: ResourceRevision,
}

pub(crate) struct Descriptor {
    base: PathBuf,
    rate: u32,
    authority: ResourceRevision,
    identities: Vec<SetupIdentity>,
    mode: sampler_ksp::model::SnapshotMode,
}

pub(crate) struct Ready {
    pub state: sampler_core::ScriptStateBuffer,
    pub mode: sampler_ksp::model::SnapshotMode,
}

fn authority(base: &Path) -> Option<ResourceRevision> {
    let resources = sampler_kontakt::Resources::for_program(base, 0).ok()?;
    ResourceRevision::capture(base, 0, &resources)
}

impl Seed {
    pub(crate) fn capture(base: &Path, rate: u32) -> Option<Self> {
        let _span = sampler_kontakt::audit::Span::new("snapshot_reuse_seed");
        if !base
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("nki"))
        {
            return None;
        }
        let base = base.canonicalize().ok()?;
        Some(Self {
            authority: authority(&base)?,
            base,
            rate,
        })
    }

    pub(crate) fn finish(self, views: &[sampler_ksp::ScriptView]) -> Option<Arc<Descriptor>> {
        let _span = sampler_kontakt::audit::Span::new("snapshot_reuse_authority");
        if views.is_empty() || authority(&self.base).as_ref() != Some(&self.authority) {
            return None;
        }
        let policy = snapshot_policy(views).ok()?;
        Some(Arc::new(Descriptor {
            base: self.base,
            rate: self.rate,
            authority: self.authority,
            identities: views.iter().map(|view| view.setup().identity).collect(),
            mode: policy.mode,
        }))
    }
}

/// Dedicated preparation keeps the existing owner's effect/control handoff
/// serviced even when cold namespace discovery or file reads take a long time.
fn prepare_waiting<T: Send>(
    job: impl FnOnce() -> T + Send,
    service: &mut dyn FnMut(),
) -> Result<T, Refusal> {
    let (tx, rx) = std::sync::mpsc::channel::<Progress>();
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("sampler-snapshot".into())
            .stack_size(LOADER_STACK)
            .spawn_scoped(scope, move || {
                let result = job();
                drop(tx);
                result
            })
            .map_err(|_| Refusal::Worker)?;
        load_wait::relay(rx, &mut |_| {}, Some(service));
        worker.join().map_err(|_| Refusal::Worker)
    })
}

impl Descriptor {
    pub(crate) fn prepare_serviced(
        self: Arc<Self>,
        path: &Path,
        rate: u32,
        views: Vec<sampler_ksp::ScriptView>,
        service: &mut dyn FnMut(),
    ) -> Result<Ready, Refusal> {
        self.prepare_serviced_reported(path, rate, views, service).0
    }
    pub(crate) fn prepare_serviced_reported(
        self: Arc<Self>,
        path: &Path,
        rate: u32,
        views: Vec<sampler_ksp::ScriptView>,
        service: &mut dyn FnMut(),
    ) -> (Result<Ready, Refusal>, admission::Report) {
        let mode = self.mode;
        match prepare_waiting(move || self.prepare_reported(path, rate, &views), service) {
            Ok(reported) => reported,
            Err(reason) => {
                let mut report = admission::Report::new(Some(mode));
                report.worker_lost(reason);
                (Err(reason), report)
            }
        }
    }
    pub(crate) fn prepare(
        &self,
        path: &Path,
        rate: u32,
        views: &[sampler_ksp::ScriptView],
    ) -> Result<Ready, Refusal> {
        self.prepare_reported(path, rate, views).0
    }
    pub(crate) fn prepare_reported(
        &self,
        path: &Path,
        rate: u32,
        views: &[sampler_ksp::ScriptView],
    ) -> (Result<Ready, Refusal>, admission::Report) {
        let mut report = admission::Report::new(Some(self.mode));
        let result = self.prepare_inner(path, rate, views, &mut report);
        if let Err(reason) = &result {
            report.refuse(*reason);
        }
        (result, report)
    }
    fn prepare_inner(
        &self,
        path: &Path,
        rate: u32,
        views: &[sampler_ksp::ScriptView],
        report: &mut admission::Report,
    ) -> Result<Ready, Refusal> {
        let _span = sampler_kontakt::audit::Span::new("snapshot_reuse_prepare");
        report.at(admission::Stage::Policy);
        if self.mode.reruns_init() {
            return Err(Refusal::InitRequired);
        }
        report.at(admission::Stage::SetupIdentity);
        if rate != self.rate
            || views.len() != self.identities.len()
            || views
                .iter()
                .zip(&self.identities)
                .any(|(view, expected)| view.setup().identity != *expected)
            || snapshot_policy(views).map(|policy| policy.mode).ok() != Some(self.mode)
        {
            return Err(Refusal::SetupChanged);
        }
        report.authority_check(admission::Stage::AuthorityBeforeRead);
        if authority(&self.base).as_ref() != Some(&self.authority) {
            return Err(Refusal::SourceOrResources);
        }
        report.snapshot_read();
        let snapshot = sampler_kontakt::read_snapshot_for_base(path, &self.base)
            .map_err(|_| Refusal::SnapshotRead)?;
        report.incoming(&snapshot);
        // Do not mistake equal IR indices or scalar rosters for a proof that
        // every native group/source/rack field is unchanged. Modes 1 and 3 can
        // reuse here only when there is no incoming native payload to apply.
        if !snapshot.groups.is_empty() || !snapshot.effects.is_empty() {
            return Err(Refusal::NativeState);
        }
        report.saved_decode();
        let rows = snapshot.saved_values().map_err(|_| Refusal::SavedState)?;
        let incoming: Vec<_> = rows
            .into_iter()
            .enumerate()
            .map(|(slot, values)| {
                Ok(SnapshotSavedSlot {
                    slot: u8::try_from(slot).map_err(|_| Refusal::SavedState)?,
                    values,
                })
            })
            .collect::<Result<_, Refusal>>()?;
        report.saved_schema();
        let state = snapshot_saved_state(
            views,
            &incoming,
            SnapshotSavedLimits {
                value_entries: sampler_core::SNAPSHOT_OVERLAY_VALUE_CAPACITY,
                text_bytes: sampler_core::SNAPSHOT_OVERLAY_VALUE_CAPACITY
                    * sampler_core::TEXT_CAPACITY,
            },
        )
        .map_err(|error| {
            report.saved_schema_refusal(&error);
            match error {
                sampler_ksp::setup::SnapshotSavedError::Row {
                    reason: sampler_ksp::setup::SnapshotSavedRowError::MenuRosterUnverified,
                    ..
                } => Refusal::MenuRosterUnverified,
                _ => Refusal::SavedState,
            }
        })?;
        report.prepared(&state);
        // Revalidate after all file reads. No candidate acquires live ownership
        // if the base/resource authority changed while it was being prepared.
        report.authority_check(admission::Stage::AuthorityAfterRead);
        if authority(&self.base).as_ref() != Some(&self.authority) {
            return Err(Refusal::SourceOrResources);
        }
        report.at(admission::Stage::Prepared);
        Ok(Ready {
            state,
            mode: self.mode,
        })
    }
}

impl ControlIngress {
    pub(crate) fn submit_snapshot_overlay(
        &mut self,
        state: sampler_core::ScriptStateBuffer,
        epochs: Box<[sampler_core::SnapshotOverlayEpoch]>,
    ) -> Result<u64, Refusal> {
        let mut overlay =
            sampler_core::NativeSnapshotOverlay::linearized(self.plan, state, Box::new([]));
        overlay.epochs = epochs;
        self.client
            .submit(sampler_core::ControlRequest {
                plan: self.plan,
                expected_revision: None,
                operation: sampler_core::ControlOperation::NativeSnapshotOverlay(overlay),
            })
            .map_err(|_| Refusal::Queue)
    }

    pub(crate) fn take_snapshot_reply(
        &mut self,
        request: u64,
    ) -> Option<sampler_core::ControlReply> {
        self.settle();
        // Older cancelled requests still return all producer-owned buffers here,
        // so their destruction never occurs in the audio callback.
        self.snapshot_replies
            .retain(|reply| reply.request >= request);
        let index = self
            .snapshot_replies
            .iter()
            .position(|reply| reply.request == request)?;
        self.snapshot_replies.remove(index)
    }
}

#[cfg(test)]
impl V2Core {
    pub(crate) fn snapshot_test_tone_history(&mut self, slot: usize, value: f64) {
        self.parts[slot].as_mut().unwrap().tone_history[0][0][0] = value;
    }
    pub(crate) fn snapshot_test_owner(&self, slot: usize) -> Option<(usize, sampler_core::PlanId)> {
        let part = self.parts.get(slot)?.as_ref()?;
        Some((
            (&part.runtime as *const Runtime) as usize,
            part.runtime.active_plan(),
        ))
    }
}

#[cfg(test)]
#[path = "../../../crates/sampler-kontakt/tests/support/snapshot_reuse.rs"]
mod fixture;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_preparation_services_caller_before_a_blocked_worker_can_finish() {
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let caller = std::thread::current().id();
        let mut services = 0;
        let result = prepare_waiting(
            move || {
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                17
            },
            &mut || {
                assert_eq!(std::thread::current().id(), caller);
                services += 1;
                let _ = release_tx.try_send(());
            },
        )
        .unwrap();
        assert_eq!(result, 17);
        assert!(services > 0);
    }

    const SOURCE: &str =
        "on init set_snapshot_type(3) declare $saved make_persistent($saved) end on";
    fn views(source: &str) -> Vec<sampler_ksp::ScriptView> {
        vec![
            sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[])
                .unwrap()
                .view(),
        ]
    }
    fn files() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = fixture::directory();
        let base = dir.path().join("Authority.nki");
        let saved = dir.path().join("Saved.nksn");
        std::fs::write(
            &base,
            fixture::base_instrument("Authority", &[Some(SOURCE), None, None, None, None]),
        )
        .unwrap();
        std::fs::write(
            &saved,
            fixture::script_only_snapshot("Authority", "", &[&["$saved 7"], &[], &[], &[], &[]]),
        )
        .unwrap();
        (dir, base, saved)
    }
    #[test]
    fn unchanged_authority_admits_but_new_loose_resource_invalidates() {
        let (dir, base, saved) = files();
        let views = views(SOURCE);
        let descriptor = Seed::capture(&base, 48000).unwrap().finish(&views).unwrap();
        assert_eq!(
            descriptor
                .prepare(&saved, 48000, &views)
                .unwrap()
                .state
                .values
                .len(),
            1
        );
        std::fs::write(
            dir.path().join("Resources/wallpaper.png"),
            b"authored metadata only",
        )
        .unwrap();
        assert!(matches!(
            descriptor.prepare(&saved, 48000, &views),
            Err(Refusal::SourceOrResources)
        ));
    }
    #[test]
    fn source_change_during_initialization_and_later_rate_or_setup_changes_refuse() {
        let (_dir, base, saved) = files();
        let initial = views(SOURCE);
        let seed = Seed::capture(&base, 48000).unwrap();
        std::fs::write(
            &base,
            fixture::base_instrument(
                "Authority",
                &[Some("on init end on"), None, None, None, None],
            ),
        )
        .unwrap();
        assert!(seed.finish(&initial).is_none());
        std::fs::write(
            &base,
            fixture::base_instrument("Authority", &[Some(SOURCE), None, None, None, None]),
        )
        .unwrap();
        let descriptor = Seed::capture(&base, 48000)
            .unwrap()
            .finish(&initial)
            .unwrap();
        assert!(matches!(
            descriptor.prepare(&saved, 44100, &initial),
            Err(Refusal::SetupChanged)
        ));
        assert!(matches!(
            descriptor.prepare(
                &saved,
                48000,
                &views(
                    "on init set_snapshot_type(3) declare $other make_persistent($other) end on"
                )
            ),
            Err(Refusal::SetupChanged)
        ));
    }
}
