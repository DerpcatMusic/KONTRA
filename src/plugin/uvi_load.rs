//! Serialized loader service for restored and newly selected native rack parts.
use super::*;
use crate::uvi::worker::Status;

const STARTING: &str = "Loading UVI instrument…";
const PREPARING: &str = "Preparing UVI playback…";
const READY: &str = "UVI instrument";
const FAILED: &str = "UVI playback failed. Open Logs for the cause.";
const UNSUPPORTED: &str = "The current audio configuration is unsupported by UVI playback.";

pub(super) fn loading_status(progress: Option<(&'static str, std::time::Duration)>) -> String {
    let Some((phase,elapsed))=progress else { return STARTING.into(); };
    let activity=match phase {
        "bank_open" => "opening bank",
        "program_decode" => "reading program",
        "graph_diagnosis_and_preflight" | "preflight" | "player_preflight" => "checking playback",
        "resources" | "restore_validation_and_audio" => "loading samples",
        "modules" => "reading script modules",
        "lua_init" => "initializing controls",
        "renderer_init" | "restore_renderer_init" => "preparing sound",
        "restore_apply" => "restoring settings",
        "player_finalize" => "finishing initialization",
        _ => "starting",
    };
    format!("Loading UVI instrument: {activity}… ({} s)",elapsed.as_secs())
}

#[derive(Clone)]
pub(crate) struct Activation {
    pub(super) source: library::UviSource,
    pub(super) saved_state: NativeState,
    pub(super) epoch: u64,
    pub(super) generation: u64,
    pub(super) part_generation: u64,
    pub(super) rate: u32,
    pub(super) max_host_frames: usize,
    pub(super) published: bool,
}

impl Activation {
    pub(super) fn context_matches(
        &self,
        params: &SamplerParams,
        slot: usize,
        selection: &Selection,
    ) -> bool {
        selection.parts.get(slot).and_then(|part| part.uvi.as_ref()) == Some(&self.source)
            && selection
                .parts
                .get(slot)
                .is_some_and(|part| part.uvi_state == self.saved_state)
            && params.shared.uvi_epoch.load(Ordering::Acquire) == self.epoch
            && params.shared.rate() == f64::from(self.rate)
            && params.shared.uvi_max_host_frames.load(Ordering::Acquire) == self.max_host_frames
            && params
                .shared
                .part(slot)
                .is_some_and(|part| part.generation.load(Ordering::Acquire) == self.part_generation)
    }

    pub(super) fn current(&self, params: &SamplerParams, slot: usize) -> bool {
        self.context_matches(params, slot, &params.selection.read().unwrap())
    }
}

fn cancel(params: &SamplerParams, activation: &Activation) {
    params
        .shared
        .uvi_controls
        .lock()
        .unwrap()
        .cancel(activation.epoch, activation.generation);
}

fn update(
    params: &SamplerParams,
    slot: usize,
    activation: &Activation,
    status: &str,
    loading: bool,
    ui: Option<Arc<uvi_ui::Published>>,
) {
    // The existing loader uses View -> Selection. Never hold Registry across
    // either lock: polling a controller can perform artwork I/O or retire Lua.
    let mut view = params.shared.view.lock().unwrap();
    let selection = params.selection.read().unwrap();
    if !activation.context_matches(params, slot, &selection) {
        return;
    }
    let Some(part) = view.parts.get_mut(slot) else {
        return;
    };
    if !part.uvi_activation.as_ref().is_some_and(|current| {
        (current.epoch, current.generation) == (activation.epoch, activation.generation)
    }) {
        return;
    }
    if part.status != status { part.status = status.into(); }
    part.loading = loading;
    if !loading && status != READY {
        part.uvi_ui = None;
    }
    if let Some(ui) = ui.filter(|ui| {
        (ui.stamp.epoch, ui.stamp.generation) == (activation.epoch, activation.generation)
    }) {
        part.uvi_ui = Some(ui);
    }
}

pub(super) fn service(params: &SamplerParams) {
    params.shared.uvi_controls.lock().unwrap().poll_retired();
    let selection = params.selection.read().unwrap().clone();
    let count = selection
        .parts
        .len()
        .max(params.shared.view.lock().unwrap().parts.len());
    let epoch = params.shared.uvi_epoch.load(Ordering::Acquire);
    let rate = params.shared.rate();
    let maximum = params.shared.uvi_max_host_frames.load(Ordering::Acquire);
    // These are the worker/Bridge's existing supported configuration bounds.
    let supported = epoch != 0
        && (8000. ..=192000.).contains(&rate)
        && rate.fract() == 0.
        && (1..=65_536).contains(&maximum);
    // Admission remains valid across a host reset while native configuration
    // remains selected. Retracting the report during loading triggers another
    // reset in hosts that restart their engine when latency changes.
    if !supported || !selection.parts.iter().any(|part| part.uvi.is_some()) {
        params.shared.uvi_latency_admission.store(0, Ordering::Release);
    }
    for slot in 0..count {
        let source = selection.parts.get(slot).and_then(|part| part.uvi.clone());
        let previous = params
            .shared
            .view
            .lock()
            .unwrap()
            .parts
            .get(slot)
            .and_then(|part| part.uvi_activation.clone());
        let Some(source) = source.filter(|_| supported) else {
            let retired = {
                let mut view = params.shared.view.lock().unwrap();
                let current = params.selection.read().unwrap();
                // A native source selected after this iteration began is handled
                // by the next loader pass, rather than cancelled by old work.
                if current.parts.get(slot).and_then(|part| part.uvi.as_ref())
                    != selection.parts.get(slot).and_then(|part| part.uvi.as_ref())
                {
                    continue;
                }
                let Some(part) = view.parts.get_mut(slot) else {
                    continue;
                };
                part.uvi_ui = None;
                if selection
                    .parts
                    .get(slot)
                    .is_some_and(|part| part.uvi.is_some())
                {
                    part.status = UNSUPPORTED.into();
                    part.loading = false;
                }
                part.uvi_activation.take()
            };
            if let Some(retired) = retired {
                cancel(params, &retired);
            }
            continue;
        };
        if previous
            .as_ref()
            .is_some_and(|activation| activation.current(params, slot))
        {
            continue;
        }
        let Some(atoms) = params.shared.part(slot) else {
            continue;
        };
        let (activation, retired) = {
            let mut view = params.shared.view.lock().unwrap();
            let current = params.selection.read().unwrap();
            if current.parts.get(slot).and_then(|part| part.uvi.as_ref()) != Some(&source)
                || params.shared.uvi_epoch.load(Ordering::Acquire) != epoch
                || params.shared.rate() != rate
                || params.shared.uvi_max_host_frames.load(Ordering::Acquire) != maximum
            {
                continue;
            }
            let Some(part) = view.parts.get_mut(slot) else {
                continue;
            };
            let activation = Activation {
                saved_state: current.parts[slot].uvi_state.clone(),
                source,
                epoch,
                generation: params.shared.uvi_generation.fetch_add(1, Ordering::AcqRel) + 1,
                part_generation: atoms.generation.fetch_add(1, Ordering::AcqRel) + 1,
                rate: rate as u32,
                max_host_frames: maximum,
                published: false,
            };
            let retired = part.uvi_activation.take();
            // All Kontakt view caches belong to the former source; its actual
            // player remains on audio until the new endpoint is adopted.
            *part = PartView {
                uvi_activation: Some(activation.clone()),
                status: STARTING.into(),
                loading: true,
                ..Default::default()
            };
            (activation, retired)
        };
        if let Some(retired) = retired {
            cancel(params, &retired);
        }
        let configured = params
            .shared
            .libraries
            .uvi_worker_config(&activation.source, activation.rate);
        if !activation.current(params, slot) {
            continue;
        }
        let started = match configured {
            Ok(config) => params
                .shared
                .uvi_controls
                .lock()
                .unwrap()
                .prepare_with_state(
                    config,
                    activation.source.clone(),
                    activation.epoch,
                    activation.generation,
                    activation.part_generation,
                    slot,
                    activation.max_host_frames,
                    UVI_LEAD_PACKETS,
                    activation.saved_state.as_ref(),
                )
                .is_ok(),
            Err(reason) => {
                let mut trace =
                    crate::diagnostics::LoadTrace::new(&activation.source.bank, 0, Some(slot));
                trace.detail("backend", "uvi");
                trace.detail("member", activation.source.member.clone());
                trace.detail("epoch", activation.epoch);
                trace.detail("generation", activation.generation);
                trace.stage("uvi_configuration");
                trace.fail(reason);
                trace.finish("failed");
                false
            }
        };
        if !activation.current(params, slot) {
            cancel(params, &activation);
            continue;
        }
        if !started {
            cancel(params, &activation);
            // Keep the activation identity to avoid retrying a failed bank on
            // every audio poll. Selecting/resetting its context starts fresh work.
            update(params, slot, &activation, FAILED, false, None);
        }
    }

    // Root prepares common mixer delay buffers and waits for callback adoption.
    // Endpoint extraction/publishing must not precede that acknowledgement.
    let activations: Vec<_> = params
        .shared
        .view
        .lock()
        .unwrap()
        .parts
        .iter()
        .enumerate()
        .filter_map(|(slot, part)| {
            part.uvi_activation
                .clone()
                .map(|activation| (slot, activation))
        })
        .collect();
    let delay_ready = if activations.is_empty() {
        Ok(false)
    } else {
        uvi_delay_ready(params)
    };
    for (slot, activation) in activations {
        if !activation.current(params, slot) {
            cancel(params, &activation);
            continue;
        }
        if let Err(error) = delay_ready
            && !activation.published
        {
            crate::diagnostics::event(
                crate::diagnostics::LogLevel::Error,
                "uvi",
                "uvi_delay_admission_failed",
                serde_json::json!({"slot":slot, "epoch":activation.epoch, "generation":activation.generation,
                    "max_host_frames":maximum, "reason":format!("{error:?}")}),
            );
            // Terminal allocation/layout failure is distinct from callback ack.
            // Keep the failed identity, and preserve already exported players.
            cancel(params, &activation);
            update(params, slot, &activation, UNSUPPORTED, false, None);
            continue;
        }
        let (status, progress, ui, ready) = {
            let mut registry = params.shared.uvi_controls.lock().unwrap();
            if !registry.matches(
                activation.epoch,
                activation.generation,
                &activation.source,
                activation.part_generation,
                slot,
                activation.rate,
                activation.max_host_frames,
                UVI_LEAD_PACKETS,
            ) {
                (None, None, None, Ok(None))
            } else {
                let status = registry.status(activation.epoch, activation.generation);
                let progress = (status == Some(Status::Starting)).then(|| registry.initialization_progress(activation.epoch, activation.generation)).flatten();
                let ui = registry.poll_ui(activation.epoch, activation.generation);
                let ready = if delay_ready == Ok(true)
                    && !activation.published
                    && status == Some(Status::Ready)
                {
                    registry.take_ready(activation.epoch, activation.generation)
                } else {
                    Ok(None)
                };
                (status, progress, ui, ready)
            }
        };
        if !activation.current(params, slot) {
            drop(ready);
            cancel(params, &activation);
            continue;
        }
        match ready {
            Ok(Some(audio)) => {
                let mut view = params.shared.view.lock().unwrap();
                let current = params.selection.read().unwrap();
                let fresh = activation.context_matches(params, slot, &current)
                    && view
                        .parts
                        .get(slot)
                        .and_then(|part| part.uvi_activation.as_ref())
                        .is_some_and(|old| {
                            (old.epoch, old.generation) == (activation.epoch, activation.generation)
                        });
                if fresh {
                    params.shared.publish_part((
                        slot,
                        activation.part_generation,
                        Handoff::Uvi(audio),
                    ));
                    view.parts[slot].uvi_activation.as_mut().unwrap().published = true;
                } else {
                    // Dropping an endpoint is safe here, on Load; its retirement
                    // receipt permits the controller to be joined on this thread.
                    drop(audio);
                    drop(current);
                    drop(view);
                    cancel(params, &activation);
                    continue;
                }
            }
            Err(error) => {
                crate::diagnostics::event(
                    crate::diagnostics::LogLevel::Error,
                    "uvi",
                    "uvi_endpoint_failed",
                    serde_json::json!({"slot":slot, "epoch":activation.epoch, "generation":activation.generation,
                        "reason":format!("{error:?}")}),
                );
                cancel(params, &activation);
                update(params, slot, &activation, FAILED, false, None);
                continue;
            }
            Ok(None) => {}
        }
        let (installed, failed) = params.shared.part(slot).map_or((false, false), |part| {
            let installed = part.uvi_generation.load(Ordering::Acquire) == activation.generation;
            (
                installed,
                installed && part.uvi_failed.load(Ordering::Acquire),
            )
        });
        match status {
            Some(Status::Ready) if !failed => {
                update(
                    params,
                    slot,
                    &activation,
                    if installed { READY } else { PREPARING },
                    !installed,
                    ui,
                );
            }
            Some(Status::Starting) => update(params, slot, &activation, &loading_status(progress), true, ui),
            _ => {
                if failed {
                    crate::diagnostics::event(
                        crate::diagnostics::LogLevel::Error,
                        "uvi",
                        "uvi_audio_endpoint_failed",
                        serde_json::json!({"slot":slot, "epoch":activation.epoch, "generation":activation.generation,
                            "reason":"Audio endpoint failed; inspect the preceding worker fault and rack packet counters"}),
                    );
                }
                cancel(params, &activation);
                update(params, slot, &activation, FAILED, false, None);
            }
        }
    }
    super::uvi_state::poll(params);
    params.shared.uvi_controls.lock().unwrap().poll_retired();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_loading_progress_is_truthful_and_updates_in_whole_seconds() {
        let samples=loading_status(Some(("resources",std::time::Duration::from_millis(1900))));
        assert_eq!(samples,"Loading UVI instrument: loading samples… (1 s)");
        assert_eq!(samples,loading_status(Some(("resources",std::time::Duration::from_millis(1100)))));
        assert_ne!(samples,loading_status(Some(("resources",std::time::Duration::from_millis(2100)))));
        assert_eq!(loading_status(Some(("lua_init",std::time::Duration::from_secs(3)))),"Loading UVI instrument: initializing controls… (3 s)");
        assert!(!loading_status(Some(("renderer_init",std::time::Duration::ZERO))).contains("ready"));
        assert_eq!(loading_status(None),STARTING);
    }
}
