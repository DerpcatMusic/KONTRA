//! The producer half of same-owner preset application. Preparation and payload
//! retirement happen here; the exact live Runtime decides admission on audio.
use super::*;
use crate::sound::v2::snapshot_reuse::{Refusal, admission};

pub(super) struct Pending {
    request: SnapshotRequest,
    command: u64,
    trace: crate::diagnostics::LoadTrace,
    admission: admission::Report,
}

pub(super) fn same_base(a: &(String, u32, String), b: &(String, u32, String)) -> bool {
    a.0 == b.0 && a.1 == b.1
}

pub(super) fn source_current(
    params: &SamplerParams,
    request: &SnapshotRequest,
    source: &(String, u32, String),
) -> bool {
    let view = params.shared.view.lock_unpoisoned();
    let Some(view) = view.parts.get(request.slot) else {
        return false;
    };
    view.recall_revision == request.recall_revision
        && continuous_source(source, &request.source, view.active_source.as_ref())
}

fn continuous_source(
    source: &(String, u32, String),
    expected: &(String, u32, String),
    active: Option<&(String, u32, String)>,
) -> bool {
    same_base(source, expected) && (source == expected || active == Some(source))
}

pub(super) fn current(params: &SamplerParams, request: &SnapshotRequest, newest: bool) -> bool {
    let Some(atoms) = params.shared.part(request.slot) else {
        return false;
    };
    if atoms.generation.load(Ordering::Acquire) != request.generation
        || (newest && atoms.snapshot_sequence.load(Ordering::Acquire) != request.sequence)
    {
        return false;
    }
    let selection = params.selection.read_unpoisoned();
    selection
        .parts
        .get(request.slot)
        .is_some_and(|part| source_current(params, request, &part.source()))
}

/// Returning the original job makes the ordinary loader the explicit fallback.
/// There is no clone of the live owner, no replacement plan and no UI reset.
pub(super) fn try_submit(
    params: &SamplerParams,
    request: SnapshotRequest,
    mut trace: crate::diagnostics::LoadTrace,
) -> Result<(), (SnapshotRequest, crate::diagnostics::LoadTrace)> {
    let mut admission = admission::Report::new(None);
    let result = (|| {
        if !request.allow_reuse || request.path.is_empty() {
            return Err(Refusal::NoSetup);
        }
        admission.at(admission::Stage::Owner);
        let atoms = params
            .shared
            .part(request.slot)
            .ok_or(Refusal::OwnerChanged)?;
        let (descriptor, views) = {
            let scripts = atoms.scripts.lock_unpoisoned();
            admission.at(admission::Stage::Availability);
            (
                scripts.snapshot_reuse.clone().ok_or(Refusal::NoSetup)?,
                scripts.views.clone(),
            )
        };
        let (ready, report) = descriptor.prepare_serviced_reported(
            Path::new(&request.path),
            params.shared.rate() as u32,
            views,
            &mut || service_current_ui(params),
        );
        admission = report;
        let ready = ready?;
        admission.at(admission::Stage::Owner);
        if !current(params, &request, true) {
            return Err(Refusal::OwnerChanged);
        }
        let mut guard = atoms.ingress.lock_unpoisoned();
        let ingress = guard.as_mut().ok_or(Refusal::OwnerChanged)?;
        admission.at(admission::Stage::Submission);
        let command = ingress.submit_snapshot_overlay(
            ready.state,
            Box::new([
                sampler_core::SnapshotOverlayEpoch {
                    sequence: atoms.generation.clone(),
                    expected: request.generation,
                },
                sampler_core::SnapshotOverlayEpoch {
                    sequence: atoms.snapshot_sequence.clone(),
                    expected: request.sequence,
                },
            ]),
        )?;
        drop(guard);
        trace.detail("snapshot_mode", format!("{:?}", ready.mode));
        trace.detail("runtime_counter_scope", "Counters remain cumulative for the retained runtime owner; applying a snapshot does not reset them");
        Ok((atoms, command))
    })();
    match result {
        Ok((atoms, command)) => {
            admission.queued();
            trace.detail("snapshot_reuse_admission", admission.json());
            trace.detail("scope", "validated saved-state batch queued for the exact existing owner; callback completion and PCM remain separate");
            trace.stage("snapshot_apply_queued");
            let slot = request.slot;
            atoms.snapshot_pending.lock_unpoisoned().push_back(Pending {
                request,
                command,
                trace,
                admission,
            });
            params.shared.view.lock_unpoisoned().parts[slot].status = "Applying preset…".into();
            Ok(())
        }
        Err(reason) => {
            admission.refuse(reason);
            trace.detail("snapshot_reuse_admission", admission.json());
            trace.detail(
                "snapshot_reuse_fallback",
                serde_json::to_value(reason).unwrap_or_default(),
            );
            Err((request, trace))
        }
    }
}

pub(super) fn poll(params: &SamplerParams) {
    let parts = params.shared.with_parts(|parts| parts.to_vec());
    for atoms in &parts {
        let mut pending = std::mem::take(&mut *atoms.snapshot_pending.lock_unpoisoned());
        let mut waiting = std::collections::VecDeque::new();
        while let Some(mut job) = pending.pop_front() {
            // An owner replacement retires the old ingress and its replies
            // off audio. Never use its metadata for the newly installed one.
            if atoms.generation.load(Ordering::Acquire) != job.request.generation {
                job.admission.canceled();
                job.trace
                    .detail("snapshot_reuse_admission", job.admission.json());
                job.trace.finish("canceled");
                continue;
            }
            let reply = atoms
                .ingress
                .lock_unpoisoned()
                .as_mut()
                .and_then(|ingress| ingress.take_snapshot_reply(job.command));
            let Some(reply) = reply else {
                waiting.push_back(job);
                // Control replies are FIFO. Do not discard an earlier
                // command's receipt by looking for a later one first.
                waiting.append(&mut pending);
                break;
            };
            let sampler_core::ControlOperation::NativeSnapshotOverlay(overlay) =
                reply.command.operation
            else {
                job.admission.unrecognized_receipt();
                job.trace
                    .detail("snapshot_reuse_admission", job.admission.json());
                job.trace.finish("failed");
                continue;
            };
            job.admission
                .audio(overlay.committed.is_some(), overlay.refusal);
            job.trace
                .detail("snapshot_reuse_admission", job.admission.json());
            if reply.result.is_ok() && overlay.committed.is_some() {
                job.trace.detail("snapshot_apply", "retained_owner");
                job.trace.detail("callbacks", overlay.state.callbacks.len());
                job.trace.detail(
                    "callback_admission_outcomes",
                    format!("{:?}", overlay.state.callbacks),
                );
                // A newer preset request may have been queued after this
                // exact commit. Its source matches the base, so updating the
                // actual committed preset does not invalidate that request.
                if current(params, &job.request, false) {
                    publish(params, &job.request);
                    let report = job.trace.finish("applied");
                    params.shared.view.lock_unpoisoned().parts[job.request.slot].trace =
                        Some(report);
                } else {
                    job.trace.finish("canceled");
                }
            } else {
                job.trace
                    .detail("snapshot_apply_refusal", format!("{:?}", overlay.refusal));
                job.trace
                    .detail("control_result", format!("{:?}", reply.result));
                let retry = current(params, &job.request, true);
                job.trace
                    .finish(if retry { "reuse_refused" } else { "canceled" });
                if retry {
                    // Rejection is pure. A full replacement may now prepare
                    // privately, with no second attempt at the same fast path.
                    let mut request = params.shared.snapshot_request.lock_unpoisoned();
                    if request.is_none() {
                        job.request.allow_reuse = false;
                        *request = Some(job.request);
                    }
                }
            }
            // reply/overlay buffers and callback metadata drop here, off audio.
        }
        *atoms.snapshot_pending.lock_unpoisoned() = waiting;
    }
}

fn publish(params: &SamplerParams, request: &SnapshotRequest) {
    let mut selection = params.selection.write_unpoisoned();
    let streaming = selection.streaming;
    let Some(part) = selection.parts.get_mut(request.slot) else {
        return;
    };
    if !same_base(&part.source(), &request.source) {
        return;
    }
    let mut view = params.shared.view.lock_unpoisoned();
    let Some(view) = view.parts.get_mut(request.slot) else {
        return;
    };
    if view.generation != request.generation
        || view.recall_revision != request.recall_revision
        || !continuous_source(&part.source(), &request.source, view.active_source.as_ref())
    {
        return;
    }
    part.select_snapshot(request.path.clone());
    view.active_source = Some(part.source());
    view.attempted = Some((
        part.path.clone(),
        part.program,
        part.snapshot.clone(),
        params.shared.rate().to_bits(),
        part.mpe,
        part.mpe_upper,
        part.dynamics,
        part.streaming(streaming),
    ));
    view.status = "Preset applied".into();
    // The Runtime, interface Arcs, tree, decoded assets, ScriptViews and owner
    // generation are deliberately retained. Normal callback effects patch UI.
}
