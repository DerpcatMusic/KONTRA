//! Serialized loader service for restored and newly selected native rack parts.
use super::*;
use crate::uvi::worker::Status;

const STARTING: &str = "Loading UVI instrument…";
const PREPARING: &str = "Preparing UVI playback…";
const READY: &str = "UVI instrument";
const FAILED: &str = "UVI playback failed. Open Logs for the cause.";
const UNSUPPORTED: &str = "The current audio configuration is unsupported by UVI playback.";

#[cfg(feature = "uvi")]
#[derive(Clone, PartialEq, Eq)]
pub(super) struct UviLoadKey {
    pub(super) request: library::UviRequest,
    pub(super) epoch: u64,
    pub(super) rate: u64,
    pub(super) max_host_frames: usize,
    pub(super) catalog: u64,
    /// Exact active Kontakt identity/generation, without changing that engine.
    pub(super) target: Option<((String, u32, String), u64)>,
    pub(super) target_uvi: Option<library::UviSource>,
}

#[cfg(feature = "uvi")]
pub(super) struct UviPrepared {
    pub(super) key: UviLoadKey,
    pub(super) generation: u64,
    pub(super) worker: Option<crate::uvi::worker::Worker>,
    /// Bounded local code survives worker retirement only for this load key.
    pub(super) failed_lua: Option<Arc<crate::uvi::lua_failure::Context>>,
    pub(super) status: &'static str,
    pub(super) ui: Option<uvi_ui::Mailbox>,
}

#[cfg(feature = "uvi")]
pub(super) fn uvi_load_key(params: &SamplerParams, selection: &Selection) -> Option<UviLoadKey> {
    let request = selection.uvi_requested.clone()?;
    let target = request.slot.and_then(|slot| {
        let slot = slot as usize;
        Some((selection.parts.get(slot)?.source(), params.shared.part(slot)?.generation.load(Ordering::Acquire)))
    });
    let target_uvi = request.slot.and_then(|slot| selection.parts.get(slot as usize)).and_then(|p| p.uvi.clone());
    Some(UviLoadKey { request, epoch: params.shared.uvi_epoch.load(Ordering::Acquire),
        rate: params.shared.rate.load(Ordering::Acquire),
        max_host_frames: params.shared.uvi_max_host_frames.load(Ordering::Acquire),
        catalog: params.shared.libraries.wanted(),
        target_uvi, target })
}

#[cfg(feature = "uvi")]
pub(super) fn prepare_uvi(params: &SamplerParams, selection: &Selection) {
    use crate::uvi::worker::{Status, Worker};
    const STARTING: &str = "Loading the UVI instrument; the current instrument is still playing.";
    const READY: &str = "Preparing the UVI player for the rack…";
    const FAILED: &str = "The UVI instrument could not be loaded; the current instrument is still playing.";
    let key = uvi_load_key(params, selection);
    let retired = {
        let mut prepared = params.shared.uvi_prepared.lock().unwrap();
        if prepared.as_ref().map(|p| &p.key) != key.as_ref() { prepared.take() } else { None }
    };
    // Stop/join and all graph/Lua/resource destruction run only on Load.
    drop(retired);
    let Some(key) = key else {
        let mut view = params.shared.view.lock().unwrap();
        if params.selection.read().unwrap().uvi_requested.is_none() {
            view.uvi_attempted = None; view.uvi_status.clear(); view.uvi_ui = None;
        }
        return;
    };
    let missing = params.shared.uvi_prepared.lock().unwrap().is_none();
    if missing {
        let rate = f64::from_bits(key.rate);
        let configured = if key.request.new && key.request.slot.is_some()
            || key.request.slot.is_some() && key.target.is_none() {
            Err("The UVI destination changed. Select its program again.")
        } else if !(8000. ..=192000.).contains(&rate) || rate.fract() != 0. {
            Err("The current sample rate is unsupported by this UVI instrument.")
        } else {
            params.shared.libraries.uvi_worker_config(&key.request.source, rate as u32)
        };
        if uvi_load_key(params, &params.selection.read().unwrap()) != Some(key.clone()) { return; }
        let generation = params.shared.uvi_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let mut trace = crate::diagnostics::LoadTrace::new(&key.request.source.bank, 0, key.request.slot.map(|slot| slot as usize));
        trace.detail("backend", "uvi");
        trace.detail("member", key.request.source.member.clone());
        trace.detail("epoch", key.epoch);
        trace.detail("generation", generation);
        trace.stage("uvi_controller_setup");
        let (worker, status, ui) = match configured {
            Ok(config) => {
                let assets = match crate::uvi::ui_assets::UiAssets::open(&config) {
                    Ok(assets) => Some(assets),
                    Err(error) => { trace.issue("ui", "uvi_artwork_authority_unavailable", format!("{error:#}")); None }
                };
                match Worker::start_hosted(config, key.epoch, generation) {
                    Ok(worker) => (Some(worker), STARTING, Some(uvi_ui::Mailbox::new(
                        crate::uvi::worker::Stamp { epoch: key.epoch, generation, frame: 0 }, assets))),
                    Err(error) => { trace.fail(format!("Starting UVI worker: {error:#}")); (None, FAILED, None) },
                }
            },
            Err(reason) => { trace.fail(reason); (None, reason, None) },
        };
        trace.finish(if worker.is_some() { "worker_started" } else { "failed" });
        let prepared = UviPrepared { key: key.clone(), generation, worker, failed_lua: None, status, ui };
        if uvi_load_key(params, &params.selection.read().unwrap()) != Some(key.clone()) { drop(prepared); return; }
        *params.shared.uvi_prepared.lock().unwrap() = Some(prepared);
    }
    let (status, failed, generation, ui) = {
        let mut prepared = params.shared.uvi_prepared.lock().unwrap();
        let p = prepared.as_mut().unwrap();
        let failed = match p.worker.as_ref().map(Worker::status) {
            Some(Status::Ready) => { p.status = READY; None },
            Some(Status::Failed | Status::Stopped) => {
                p.status = FAILED;
                p.failed_lua = p.worker.as_ref().and_then(Worker::private_lua_failure);
                p.worker.take()
            },
            _ => None,
        };
        let ui = match (&p.worker, &mut p.ui) {
            (Some(worker), Some(ui)) => ui.poll(worker),
            _ => None,
        };
        let display = if p.status == STARTING {
            p.worker.as_ref().and_then(Worker::initialization_progress)
                .map(|progress| format!("{}; the current instrument is still playing.", uvi_load::loading_status(Some(progress))))
                .unwrap_or_else(|| p.status.to_owned())
        } else { p.status.to_owned() };
        (display, failed, p.generation, ui)
    };
    if let Some(worker) = &failed {
        crate::diagnostics::event(crate::diagnostics::LogLevel::Error, "uvi", "uvi_staging_failed",
            serde_json::json!({"path":key.request.source.bank, "member":key.request.source.member,
                "slot":key.request.slot, "epoch":key.epoch, "generation":generation,
                "reason":worker.private_failure(),
                "worker":{"lua_failure":worker.private_lua_failure().map(|context|context.metadata())}}));
    }
    drop(failed);
    if uvi_load_key(params, &params.selection.read().unwrap()) != Some(key.clone()) { return; }
    let mut view = params.shared.view.lock().unwrap();
    if params.selection.read().unwrap().uvi_requested.as_ref() == Some(&key.request)
        && params.shared.uvi_epoch.load(Ordering::Acquire) == key.epoch {
        if view.uvi_attempted.as_ref() != Some(&key.request) { view.uvi_ui = None; }
        view.uvi_attempted = Some(key.request);
        if let Some(ui) = ui { view.uvi_ui = Some(ui); }
        if view.uvi_status != status { view.uvi_status = status; }
    }
}

/// Commit the requested identity only after native initialization succeeded.
/// The controller remains on Load; its endpoint is published separately after
/// common mixer delay storage has been acknowledged by the audio thread.
#[cfg(feature = "uvi")]
pub(super) fn install_prepared_uvi(params: &SamplerParams) {
    use crate::uvi::worker::Status;
    let ready = {
        let prepared = params.shared.uvi_prepared.lock().unwrap();
        prepared.as_ref().filter(|p| p.worker.as_ref().is_some_and(|w| w.status() == Status::Ready))
            .map(|p| p.key.clone())
    };
    let Some(key) = ready else { return };
    let before = params.selection.read().unwrap().clone();
    if uvi_load_key(params, &before) != Some(key.clone()) { return }
    if !key.request.new && key.request.slot.is_none()
        && let Some(slot) = before.parts.iter().enumerate().find_map(|(slot, p)| {
            (p.uvi.as_ref() == Some(&key.request.source)
                && params.shared.part(slot).is_some_and(|a| !a.uvi_failed.load(Ordering::Acquire)
                    && a.uvi_generation.load(Ordering::Acquire) != 0)).then_some(slot)
        }) {
        let mut current = params.selection.write().unwrap();
        if uvi_load_key(params, &current) != Some(key) { return }
        current.uvi_requested = None;
        params.shared.focus_request.store(slot as u64, Ordering::Release);
        drop(current);
        params.shared.uvi_prepared.lock().unwrap().take();
        return;
    }
    let retry = (!key.request.new).then(|| before.parts.iter().position(|p|
        p.uvi.as_ref() == Some(&key.request.source))).flatten();
    let slot = key.request.slot.map(|s| s as usize).or(retry).unwrap_or_else(||
        before.parts.iter().position(Part::is_empty).unwrap_or(before.parts.len()));
    params.shared.ensure_parts(slot + 1);
    let atoms = params.shared.part(slot).unwrap();
    let mut current = params.selection.write().unwrap();
    if uvi_load_key(params, &current) != Some(key.clone()) { return }
    let mut prepared = params.shared.uvi_prepared.lock().unwrap().take().unwrap();
    let generation = prepared.generation;
    let part_generation = atoms.generation.load(Ordering::Acquire).wrapping_add(1);
    let rate = f64::from_bits(key.rate) as u32;
    let result = params.shared.uvi_controls.lock().unwrap().adopt_prepared(
        prepared.worker.take().unwrap(), prepared.ui.take().unwrap(), key.request.source.clone(),
        key.epoch, generation, part_generation, slot, rate, key.max_host_frames, UVI_LEAD_PACKETS);
    if result.is_err() {
        drop(current);
        params.shared.view.lock().unwrap().uvi_status = "The UVI player could not be prepared.".into();
        return;
    }
    let mut part = if key.request.slot.is_some() || retry.is_some() { current.parts[slot].clone() }
        else {
            let settings = params.shared.libraries.settings();
            let (port, channel) = settings.new_input.unwrap_or_else(|| current.next_input());
            Part { port, channel, output: settings.new_output.unwrap_or(0),
                output_manual: settings.new_output.is_some(), ..Default::default() }
        };
    part.path.clear(); part.snapshot.clear(); part.program = 0;
    part.uvi = Some(key.request.source.clone());
    part.uvi_state.clear();
    part.name.clear();
    part.group = u32::MAX; part.articulate = Default::default(); part.mpe = Default::default();
    part.tune = 0.;
    part.edits = Default::default(); part.script_state.clear(); part.ir_settings.clear();
    part.engine_state.clear(); part.delay_state.clear();
    if slot == current.parts.len() { current.parts.push(part); } else { current.parts[slot] = part; }
    if !current.order.contains(&(slot as u32)) { current.order.push(slot as u32); }
    current.uvi_requested = None;
    atoms.generation.store(part_generation, Ordering::Release);
    params.shared.focus_request.store(slot as u64, Ordering::Release);
    drop(current);
    let mut view = params.shared.view.lock().unwrap();
    view.parts[slot] = PartView {
        uvi_activation: Some(uvi_load::Activation { source: key.request.source, saved_state: NativeState::default(), epoch: key.epoch,
            generation, part_generation, rate, max_host_frames: key.max_host_frames, published: false }),
        status: "Preparing UVI playback…".into(), loading: true, ..Default::default()
    };
    view.uvi_attempted = None; view.uvi_status.clear(); view.uvi_ui = None;
}

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

/// Only our two native callback sources are eligible. The runtime checkout
/// must match the digest captured by build.rs before a line is shown as code.
fn read_rust_excerpt(
    root: &std::path::Path,
    source_file: &str,
    expected_hash: &str,
    line: u32,
) -> Result<serde_json::Value, &'static str> {
    use std::io::Read;
    use sha2::{Digest, Sha256};
    if !matches!(source_file, "src/plugin.rs" | "src/plugin/uvi.rs") {
        return Err("Native source file is outside the fixed diagnostic whitelist");
    }
    if expected_hash.is_empty() { return Err("Build-time source digest is unavailable"); }
    let file = std::fs::File::open(root.join(source_file))
        .map_err(|_| "Native source file is unavailable on this installation")?;
    const MAXIMUM: u64 = 2 * 1024 * 1024;
    if file.metadata().map_err(|_| "Native source metadata is unavailable")?.len() > MAXIMUM {
        return Err("Native source exceeds the 2 MiB inspection limit");
    }
    let mut bytes = Vec::new();
    file.take(MAXIMUM + 1).read_to_end(&mut bytes)
        .map_err(|_| "Native source could not be read")?;
    if bytes.len() as u64 > MAXIMUM { return Err("Native source exceeds the 2 MiB inspection limit"); }
    if format!("{:x}", Sha256::digest(&bytes)) != expected_hash {
        return Err("Native source differs from the source used to build this binary");
    }
    let source = std::str::from_utf8(&bytes).map_err(|_| "Native source is not UTF-8")?;
    let mut excerpt = crate::diagnostics::script_excerpt(source, 0, line, None)
        .ok_or("Reported native line is unavailable in the verified source")?;
    excerpt.as_object_mut().unwrap().remove("script_slot");
    excerpt["origin"] = serde_json::json!("KONTRA Rust source verified against this build");
    excerpt["source_kind"] = serde_json::json!("rust");
    Ok(excerpt)
}

fn rust_excerpt(source_file: &str, line: u32) -> Result<serde_json::Value, &'static str> {
    let digest = match source_file {
        "src/plugin.rs" => option_env!("KONTRA_PLUGIN_SOURCE_SHA256"),
        "src/plugin/uvi.rs" => option_env!("KONTRA_UVI_SLOT_SOURCE_SHA256"),
        _ => None,
    }.unwrap_or("");
    read_rust_excerpt(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), source_file, digest, line)
}

fn terminal(part: &PartView, activation: &Activation) -> bool {
    part.load_report.as_ref().is_some_and(|report| {
        report["terminal_failure"]["epoch"].as_u64() == Some(activation.epoch)
            && report["terminal_failure"]["generation"].as_u64() == Some(activation.generation)
    })
}

fn capacity_context(worker: Option<&serde_json::Value>) -> String {
    let Some(worker) = worker else {
        return "Pending native request queue filled; worker timing context is unavailable.".into();
    };
    let configured = worker["configured_pending_packets"].as_u64()
        .map_or_else(String::new, |count| format!(" ({count} packets configured)"));
    let status = worker["status"].as_str().unwrap_or("unavailable");
    let errors = worker["stats"]["errors"].as_u64().map_or_else(|| "unavailable".into(), |count| count.to_string());
    let timing = &worker["timing"];
    let mean = timing["mean_recorded_render_ns_per_completed_packet"].as_u64();
    let budget = timing["packet_budget_ns"].as_u64();
    let maximum = worker["stats"]["max_render_ns"].as_u64().map_or_else(|| "unavailable".into(),
        |ns| format!("{:.2} ms", ns as f64 / 1_000_000.));
    let comparison = mean.zip(budget).map_or_else(|| "Packet render comparison is unavailable.".into(), |(mean,budget)|
        format!("Recorded render wall time: mean {:.2} ms, maximum {maximum}; packet budget {:.2} ms.",mean as f64 / 1_000_000.,budget as f64 / 1_000_000.));
    format!("Pending native request queue filled{configured}. Worker observed {status}; {errors} recorded worker errors. {comparison} CPU time and scheduler delays are not distinguished; see Info.")
}

fn failure_reason(reason: String, endpoint: Option<uvi::Failure>, worker: Option<&serde_json::Value>) -> String {
    use crate::uvi::{bridge::BridgeError, worker::PacketError};
    let reason = endpoint.map_or(reason, |fault| format!("{:?} at {} (frame {})",
        fault.error, fault.stage.as_str(), fault.frame));
    let reason = if endpoint.is_some_and(|fault| matches!(fault.error,
        uvi::Error::Bridge(BridgeError::RequestCapacity))) {
        format!("{reason}. {}",capacity_context(worker))
    } else { reason };
    // A worker snapshot is observed after the audio fault. Its failure text
    // explains a worker-origin failure, but cannot replace an independent
    // captured endpoint cause (for example queue capacity or invalid input).
    let worker_origin = endpoint.is_none_or(|fault| matches!(fault.error,
        uvi::Error::Bridge(BridgeError::Worker(PacketError::Failed | PacketError::Stopped))));
    worker.filter(|_| worker_origin).and_then(|report| report["failure"].as_str())
        .unwrap_or(&reason).chars().take(4096).collect()
}

/// Preserve the first cause before cancelling its controller. A repeated poll
/// of a failed audio atom must not evict that cause from the session journal.
fn fail(
    params: &SamplerParams,
    slot: usize,
    activation: &Activation,
    code: &str,
    reason: String,
    status: &str,
) {
    if params.shared.view.lock().unwrap().parts.get(slot)
        .is_some_and(|part| terminal(part, activation)) {
        return;
    }
    let (worker,lua_failure) = {
        let controls=params.shared.uvi_controls.lock().unwrap();
        (controls.failure_context(activation.epoch,activation.generation),
            controls.private_lua_failure(activation.epoch,activation.generation))
    };
    let endpoint = params.shared.part(slot)
        .and_then(|part| part.uvi_failure(activation.epoch, activation.generation));
    let reason = failure_reason(reason, endpoint, worker.as_ref());
    let endpoint = endpoint.map(|fault| {
        let mut details = serde_json::json!({"error":format!("{:?}",fault.error),
            "error_code":fault.error.code(), "stage":fault.stage.as_str(), "frame":fault.frame,
            "source_file":fault.source, "line":fault.line, "source_kind":"rust",
            "source_revision":crate::build_info::BUILD.source_revision});
        match rust_excerpt(fault.source, fault.line) {
            Ok(excerpt) => details["source_excerpt"] = excerpt,
            Err(reason) => details["source_excerpt_unavailable"] = serde_json::json!(reason),
        }
        details
    });
    let failure = serde_json::json!({"code":code, "reason":reason, "slot":slot,
        "epoch":activation.epoch, "generation":activation.generation,
        "sample_rate":activation.rate, "max_host_frames":activation.max_host_frames,
        "path":activation.source.bank, "member":activation.source.member,
        "worker":worker, "endpoint":endpoint});
    {
        let mut view = params.shared.view.lock().unwrap();
        let selection = params.selection.read().unwrap();
        if !activation.context_matches(params, slot, &selection) { return; }
        let Some(part) = view.parts.get_mut(slot) else { return; };
        if terminal(part, activation) || !part.uvi_activation.as_ref().is_some_and(|current|
            (current.epoch,current.generation)==(activation.epoch,activation.generation)) { return; }
        let report = Arc::make_mut(part.load_report.get_or_insert_with(|| Arc::new(serde_json::json!({}))));
        report["status"] = serde_json::json!("failed");
        report["failure"] = serde_json::json!(reason);
        report["terminal_failure"] = failure.clone();
        part.uvi_lua_failure = lua_failure;
        part.status = status.into();
        part.loading = false;
        // Keep the adopted, owned panel for inspection after runtime failure.
        // A new source/epoch replaces PartView before publishing its controls.
        if !activation.published { part.uvi_ui = None; }
        if let Some(atoms) = params.shared.part(slot)
            && atoms.uvi_generation.load(Ordering::Acquire) == activation.generation {
            atoms.uvi_failed.store(true, Ordering::Release);
        }
    }
    crate::diagnostics::event(crate::diagnostics::LogLevel::Error, "uvi", code, failure);
    cancel(params, activation);
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
                .map_err(|error| format!("Starting UVI playback: {error:?}")),
            Err(reason) => {
                let mut trace =
                    crate::diagnostics::LoadTrace::new(&activation.source.bank, 0, Some(slot));
                trace.detail("backend", "uvi");
                trace.detail("member", activation.source.member.clone());
                trace.detail("epoch", activation.epoch);
                trace.detail("generation", activation.generation);
                trace.stage("uvi_configuration");
                trace.fail(reason.clone());
                trace.finish("failed");
                Err(reason.to_owned())
            }
        };
        if !activation.current(params, slot) {
            cancel(params, &activation);
            continue;
        }
        if let Err(reason) = started {
            // Keep the activation identity to avoid retrying a failed bank on
            // every audio poll. Selecting/resetting its context starts fresh work.
            fail(params, slot, &activation, "uvi_start_failed", reason, FAILED);
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
        if params.shared.view.lock().unwrap().parts.get(slot)
            .is_some_and(|part| terminal(part, &activation)) {
            continue;
        }
        if !activation.current(params, slot) {
            cancel(params, &activation);
            continue;
        }
        if let Err(error) = delay_ready
            && !activation.published
        {
            // Terminal allocation/layout failure is distinct from callback ack.
            fail(params, slot, &activation, "uvi_delay_admission_failed",
                format!("{error:?}"), UNSUPPORTED);
            continue;
        }
        let (status, progress, activity, ui, ready) = {
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
                (None, None, None, None, Ok(None))
            } else {
                let status = registry.status(activation.epoch, activation.generation);
                let progress = (status == Some(Status::Starting)).then(|| registry.initialization_progress(activation.epoch, activation.generation)).flatten();
                let activity = registry.load_activity(activation.epoch, activation.generation);
                let ui = registry.poll_ui(activation.epoch, activation.generation);
                let ready = if delay_ready == Ok(true)
                    && !activation.published
                    && status == Some(Status::Ready)
                {
                    registry.take_ready(activation.epoch, activation.generation)
                } else {
                    Ok(None)
                };
                (status, progress, activity, ui, ready)
            }
        };
        if !activation.current(params, slot) {
            drop(ready);
            cancel(params, &activation);
            continue;
        }
        if let Some(activity) = activity {
            let mut view = params.shared.view.lock().unwrap();
            let current = params.selection.read().unwrap();
            if activation.context_matches(params, slot, &current)
                && let Some(part) = view.parts.get_mut(slot)
                && part.uvi_activation.as_ref().is_some_and(|old|
                    (old.epoch, old.generation) == (activation.epoch, activation.generation)) {
                part.uvi_activity = Some(activity);
            }
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
                fail(params, slot, &activation, "uvi_endpoint_failed", format!("{error:?}"), FAILED);
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
                let (code, reason) = if failed {
                    ("uvi_audio_endpoint_failed", "Audio endpoint failed; callback cause unavailable")
                } else {
                    ("uvi_worker_failed", "UVI worker stopped before playback became ready")
                };
                fail(params, slot, &activation, code, reason.into(), FAILED);
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
    fn worker_snapshot_cannot_replace_an_independent_endpoint_cause() {
        use crate::uvi::{bridge::BridgeError, worker::PacketError};
        let worker = serde_json::json!({"failure":"later worker failure", "status":"failed",
            "stats":{"errors":1}, "configured_pending_packets":27});
        let mut endpoint = uvi::Failure {
            error:uvi::Error::Bridge(BridgeError::RequestCapacity), frame:78464,
            stage:uvi::FailureStage::Process, epoch:4, generation:2,
            source:"src/plugin/uvi.rs", line:1,
        };
        let reason = failure_reason("fallback".into(),Some(endpoint),Some(&worker));
        assert!(reason.starts_with("Bridge(RequestCapacity) at process (frame 78464)"));
        assert!(reason.contains("27 packets configured"));
        assert!(!reason.contains("later worker failure"));
        endpoint.error = uvi::Error::InvalidInput;
        assert_eq!(failure_reason("fallback".into(),Some(endpoint),Some(&worker)),
            "InvalidInput at process (frame 78464)");
        for error in [PacketError::Failed, PacketError::Stopped] {
            endpoint.error = uvi::Error::Bridge(BridgeError::Worker(error));
            assert_eq!(failure_reason("fallback".into(),Some(endpoint),Some(&worker)),"later worker failure");
        }
        assert_eq!(failure_reason("fallback".into(),None,Some(&worker)),"later worker failure");
        assert_eq!(failure_reason("fallback".into(),None,None),"fallback");
        assert_eq!(failure_reason("x".repeat(5000),None,None).len(),4096);
    }

    #[test]
    fn full_request_queue_does_not_claim_cpu_overload_from_wall_time() {
        // Measured LFO v3 still filled the queue with mean wall time below budget.
        let worker=serde_json::json!({"status":"ready","stats":{"errors":0,"max_render_ns":38688600},
            "configured_pending_packets":27,"timing":{"mean_recorded_render_ns_per_completed_packet":5122110,
                "packet_budget_ns":5333333}});
        let cause=capacity_context(Some(&worker));
        assert!(cause.contains("27 packets configured"));
        assert!(cause.contains("Worker observed ready; 0 recorded worker errors"));
        assert!(cause.contains("5.12 ms") && cause.contains("5.33 ms") && cause.contains("maximum 38.69 ms"));
        assert!(cause.contains("CPU time and scheduler delays are not distinguished"));
        assert!(capacity_context(None).contains("unavailable"));
        assert!(capacity_context(Some(&serde_json::json!({}))).contains("comparison is unavailable"));
    }

    #[test]
    fn rust_fault_excerpt_requires_exact_build_source_and_stays_bounded() {
        use sha2::{Digest, Sha256};
        let root = std::env::temp_dir().join(format!("kontra-native-excerpt-{}-{}",std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(root.join("src/plugin")).unwrap();
        let file = root.join("src/plugin/uvi.rs");
        let source = format!("before\n{}\nafter\n", "authored_operation();".repeat(100));
        let digest = format!("{:x}",Sha256::digest(source.as_bytes()));
        std::fs::write(&file,&source).unwrap();
        let excerpt = read_rust_excerpt(&root,"src/plugin/uvi.rs",&digest,2).unwrap();
        assert_eq!(excerpt["source_kind"],"rust");
        assert!(excerpt.get("script_slot").is_none());
        assert!(excerpt["text"].as_str().unwrap().contains(">      2 | authored_operation();"));
        assert!(excerpt["text"].as_str().unwrap().len()<3000);
        assert_eq!(excerpt["truncated"],true);
        assert!(read_rust_excerpt(&root,"vendor/private.rs",&digest,2).is_err());
        assert!(read_rust_excerpt(&root,"src/plugin.rs",&digest,2).is_err());
        assert!(read_rust_excerpt(&root,"src/plugin/uvi.rs","",2).is_err());
        assert!(read_rust_excerpt(&root,"src/plugin/uvi.rs",&digest,999).is_err());
        std::fs::write(&file,"modified checkout").unwrap();
        assert!(read_rust_excerpt(&root,"src/plugin/uvi.rs",&digest,2).unwrap_err().contains("differs"));
        std::fs::write(&file,vec![b'x';2*1024*1024+1]).unwrap();
        assert!(read_rust_excerpt(&root,"src/plugin/uvi.rs",&digest,2).unwrap_err().contains("2 MiB"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terminal_failure_keeps_first_cause_and_allows_a_new_activation() {
        let _diagnostics = crate::diagnostics::acquire();
        let params = SamplerParams::default();
        params.shared.ensure_parts(1);
        params.shared.rate.store(48000f64.to_bits(), Ordering::Release);
        params.shared.uvi_epoch.store(4, Ordering::Release);
        params.shared.uvi_max_host_frames.store(512, Ordering::Release);
        let source = library::UviSource {
            bank: "endpoint-failure-test.ufs".into(), bank_uuid: [0;16], member: "preset.uvip".into(),
        };
        params.selection.write().unwrap().parts = vec![Part { uvi: Some(source.clone()), ..Default::default() }];
        let mut activation = Activation {
            source, saved_state: NativeState::default(), epoch:4, generation:2,
            part_generation:params.shared.part(0).unwrap().generation.load(Ordering::Acquire),
            rate:48000, max_host_frames:512, published:true,
        };
        params.shared.view.lock().unwrap().parts[0] = PartView {
            uvi_activation:Some(activation.clone()), loading:true,
            load_report:Some(Arc::new(serde_json::json!({"initialization":{"stages":["resources"]}}))),
            ..Default::default()
        };
        fail(&params,0,&activation,"uvi_audio_endpoint_failed","WrongFrame at note_on".into(),FAILED);
        let first = params.shared.view.lock().unwrap().parts[0].load_report.clone().unwrap();
        for _ in 0..1200 {
            fail(&params,0,&activation,"uvi_audio_endpoint_failed","generic repeated callback".into(),FAILED);
        }
        {
            let view = params.shared.view.lock().unwrap();
            let part = &view.parts[0];
            assert!(terminal(part,&activation));
            assert!(!part.loading);
            assert!(Arc::ptr_eq(part.load_report.as_ref().unwrap(),&first));
            assert_eq!(first["failure"],"WrongFrame at note_on");
            assert_eq!(first["initialization"]["stages"][0],"resources");
        }
        let old = activation.clone();
        activation.generation += 1;
        params.shared.view.lock().unwrap().parts[0] = PartView {
            uvi_activation:Some(activation.clone()), loading:true, ..Default::default()
        };
        assert!(!terminal(&params.shared.view.lock().unwrap().parts[0],&activation));
        fail(&params,0,&old,"uvi_audio_endpoint_failed","stale fault".into(),FAILED);
        assert!(params.shared.view.lock().unwrap().parts[0].loading);
        assert!(params.shared.view.lock().unwrap().parts[0].load_report.is_none());
        fail(&params,0,&activation,"uvi_audio_endpoint_failed","InvalidInput at render".into(),FAILED);
        let next=params.shared.view.lock().unwrap().parts[0].load_report.clone().unwrap();
        assert_eq!(next["failure"],"InvalidInput at render");
        assert_eq!(next["terminal_failure"]["generation"],3);
        activation.epoch += 1;
        params.shared.uvi_epoch.store(activation.epoch,Ordering::Release);
        params.shared.view.lock().unwrap().parts[0] = PartView {
            uvi_activation:Some(activation.clone()), loading:true, ..Default::default()
        };
        fail(&params,0,&activation,"uvi_audio_endpoint_failed","New epoch failure".into(),FAILED);
        assert_eq!(params.shared.view.lock().unwrap().parts[0].load_report.as_ref().unwrap()["terminal_failure"]["epoch"],5);
        let snapshot=serde_json::to_value(crate::diagnostics::snapshot()).unwrap();
        let events=snapshot["events"].as_array().unwrap();
        let failures:Vec<_>=events.iter().filter(|event| event["code"]=="uvi_audio_endpoint_failed"
            && event["data"]["path"]=="endpoint-failure-test.ufs").collect();
        assert_eq!(failures.len(),3,"one failure per activation, including reload and host reset");
    }

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
