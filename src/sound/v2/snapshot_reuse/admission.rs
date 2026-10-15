//! Bounded off-audio evidence for one existing-owner admission attempt.
//! Counts describe observed calls/domains, never inferred filesystem/decode work.
use super::Refusal;
use sampler_ksp::model::SnapshotMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage {
    Availability,
    Policy,
    SetupIdentity,
    AuthorityBeforeRead,
    SnapshotRead,
    NativePayload,
    SavedDecode,
    SavedSchema,
    AuthorityAfterRead,
    Prepared,
    Worker,
    Owner,
    Submission,
    AudioAdmission,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    NotSubmitted,
    Prepared,
    Queued,
    Refused,
    Committed,
    Canceled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
struct Policy {
    native_mode: u8,
    reruns_init: bool,
    saves_engine: bool,
}
impl From<SnapshotMode> for Policy {
    fn from(mode: SnapshotMode) -> Self {
        Self {
            native_mode: match mode {
                SnapshotMode::NativeWithInit => 0,
                SnapshotMode::NativeWithoutInit => 1,
                SnapshotMode::ScriptWithInit => 2,
                SnapshotMode::ScriptWithoutInit => 3,
            },
            reruns_init: mode.reruns_init(),
            saves_engine: mode.saves_engine(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
struct Incoming {
    saved_slots: usize,
    saved_rows: usize,
    saved_row_utf8_bytes: usize,
    groups: usize,
    group_source_bytes: usize,
    group_rack_slots: usize,
    group_modulation_chunks: usize,
    effect_chunks: usize,
    effect_chunk_bytes: usize,
    other_file_entries: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
struct Prepared {
    script_value_homes: usize,
    text_slots: usize,
    persistence_callbacks: usize,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
struct SavedRefusal {
    category: &'static str,
    slot: Option<u8>,
    row_reason: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
struct Calls {
    resource_authority_checks: usize,
    snapshot_read_attempts: usize,
    saved_decode_attempts: usize,
    saved_schema_attempts: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Evidence {
    NotInstrumented,
    NotObserved,
}

/// These domains need connected counters at their actual owners. A zero in
/// `calls` only means no invocation at that named seam, not zero underlying I/O.
#[derive(Clone, Copy, Debug, serde::Serialize)]
struct Unmeasured {
    file_io: Evidence,
    sample_decode: Evidence,
    image_font_ir_decode: Evidence,
    script_init_execution: Evidence,
    runtime_owner_creation: Evidence,
    ui_rebuild: Evidence,
    callback_completion: Evidence,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct Report {
    schema: u8,
    stage: Stage,
    outcome: Outcome,
    refusal: Option<Refusal>,
    snapshot_policy: Option<Policy>,
    /// None means no successfully decoded incoming snapshot was observed.
    incoming: Option<Incoming>,
    /// None means the strict saved-state mapper did not succeed.
    prepared: Option<Prepared>,
    saved_refusal: Option<SavedRefusal>,
    /// None means the preparation worker's evidence was not returned.
    calls: Option<Calls>,
    audio_committed: Option<bool>,
    audio_refusal: Option<&'static str>,
    unmeasured: Unmeasured,
}

impl Report {
    pub(crate) fn new(policy: Option<SnapshotMode>) -> Self {
        Self {
            schema: 1,
            stage: Stage::Availability,
            outcome: Outcome::NotSubmitted,
            refusal: None,
            snapshot_policy: policy.map(Policy::from),
            incoming: None,
            prepared: None,
            saved_refusal: None,
            calls: Some(Calls::default()),
            audio_committed: None,
            audio_refusal: None,
            unmeasured: Unmeasured {
                file_io: Evidence::NotInstrumented,
                sample_decode: Evidence::NotInstrumented,
                image_font_ir_decode: Evidence::NotInstrumented,
                script_init_execution: Evidence::NotInstrumented,
                runtime_owner_creation: Evidence::NotInstrumented,
                ui_rebuild: Evidence::NotInstrumented,
                callback_completion: Evidence::NotObserved,
            },
        }
    }

    pub(crate) fn at(&mut self, stage: Stage) {
        self.stage = stage;
    }
    pub(crate) fn refuse(&mut self, reason: Refusal) {
        self.outcome = Outcome::Refused;
        self.refusal = Some(reason);
    }
    pub(super) fn worker_lost(&mut self, reason: Refusal) {
        self.calls = None;
        self.at(Stage::Worker);
        self.refuse(reason);
    }
    pub(super) fn authority_check(&mut self, stage: Stage) {
        self.at(stage);
        self.calls.as_mut().unwrap().resource_authority_checks += 1;
    }
    pub(super) fn snapshot_read(&mut self) {
        self.at(Stage::SnapshotRead);
        self.calls.as_mut().unwrap().snapshot_read_attempts += 1;
    }
    pub(super) fn incoming(&mut self, snapshot: &sampler_kontakt::SnapshotState) {
        let mut incoming = Incoming {
            saved_slots: snapshot.persistent.len(),
            groups: snapshot.groups.len(),
            effect_chunks: snapshot.effects.len(),
            other_file_entries: snapshot.other_files.len(),
            ..Default::default()
        };
        for rows in &snapshot.persistent {
            incoming.saved_rows = incoming.saved_rows.saturating_add(rows.len());
            for row in rows {
                incoming.saved_row_utf8_bytes =
                    incoming.saved_row_utf8_bytes.saturating_add(row.len());
            }
        }
        for group in &snapshot.groups {
            incoming.group_source_bytes = incoming
                .group_source_bytes
                .saturating_add(group.source.len());
            incoming.group_rack_slots = incoming.group_rack_slots.saturating_add(group.fx.1.len());
            incoming.group_modulation_chunks = incoming
                .group_modulation_chunks
                .saturating_add(group.modulation.len());
        }
        for (_, bytes) in &snapshot.effects {
            incoming.effect_chunk_bytes = incoming.effect_chunk_bytes.saturating_add(bytes.len());
        }
        self.incoming = Some(incoming);
        self.at(Stage::NativePayload);
    }
    pub(super) fn saved_decode(&mut self) {
        self.at(Stage::SavedDecode);
        self.calls.as_mut().unwrap().saved_decode_attempts += 1;
    }
    pub(super) fn saved_schema(&mut self) {
        self.at(Stage::SavedSchema);
        self.calls.as_mut().unwrap().saved_schema_attempts += 1;
    }
    pub(super) fn saved_schema_refusal(&mut self, error: &sampler_ksp::setup::SnapshotSavedError) {
        use sampler_ksp::setup::{SnapshotSavedError as E, SnapshotSavedRowError as R};
        let (category, slot, row_reason) = match error {
            E::InvalidSlots => ("invalid_slots", None, None),
            E::InvalidLayout => ("invalid_layout", None, None),
            E::Capacity => ("capacity", None, None),
            E::Row { slot, reason, .. } => (
                "row",
                Some(*slot),
                Some(match reason {
                    R::Duplicate => "duplicate",
                    R::Missing => "missing",
                    R::NotSnapshotPersistent => "not_snapshot_persistent",
                    R::Type => "type",
                    R::Count => "count",
                    R::Domain => "domain",
                    R::TextCapacity => "text_capacity",
                    R::MenuRosterUnverified => "menu_roster_unverified",
                }),
            ),
        };
        self.saved_refusal = Some(SavedRefusal {
            category,
            slot,
            row_reason,
        });
    }
    pub(super) fn prepared(&mut self, state: &sampler_core::ScriptStateBuffer) {
        self.prepared = Some(Prepared {
            script_value_homes: state.values.len(),
            text_slots: state.texts.len(),
            persistence_callbacks: state.callbacks.len(),
        });
        self.at(Stage::Prepared);
        self.outcome = Outcome::Prepared;
    }
    pub(crate) fn queued(&mut self) {
        self.at(Stage::Submission);
        self.outcome = Outcome::Queued;
    }
    pub(crate) fn canceled(&mut self) {
        self.outcome = Outcome::Canceled;
    }
    pub(crate) fn unrecognized_receipt(&mut self) {
        self.at(Stage::AudioAdmission);
        self.outcome = Outcome::Refused;
        self.audio_committed = None;
        self.audio_refusal = None;
    }
    /// Receipt evidence only. Callback waits/faults can settle after this receipt.
    pub(crate) fn audio(
        &mut self,
        committed: bool,
        refusal: Option<sampler_core::SnapshotOverlayRefusal>,
    ) {
        use sampler_core::SnapshotOverlayRefusal as R;
        self.at(Stage::AudioAdmission);
        self.audio_committed = Some(committed);
        self.outcome = if committed {
            Outcome::Committed
        } else {
            Outcome::Refused
        };
        self.audio_refusal = refusal.map(|reason| match reason {
            R::InactivePlan => "inactive_plan",
            R::RevisionConflict => "revision_conflict",
            R::StaleEpoch => "stale_epoch",
            R::ActiveEvents => "active_events",
            R::AudioTail => "audio_tail",
            R::HostState => "host_state",
            R::PendingCommands => "pending_commands",
            R::ActiveCallbacks => "active_callbacks",
            R::PendingServices => "pending_services",
            R::PendingStartup => "pending_startup",
            R::UnsafeCallback => "unsafe_callback",
            R::InvalidInput => "invalid_input",
            R::Capacity => "capacity",
        });
    }
    /// Serialization belongs to the existing load-trace producer, never audio.
    pub(crate) fn json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("fixed admission metadata serializes")
    }
}

#[cfg(test)]
mod tests;
