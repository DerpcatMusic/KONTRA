//! Explicit production plan-activation witness, without a plugin or audio device.
use crate::sound::{
    BlockInfo, Core, CoreError, CoreLoader, LoadRequest, ScriptUi, Streaming,
    event::{Event, HostNote},
    v2::{ControlIngress, Part, V2Core, V2Loader},
};
use sampler_ui_ir::{Binding, ControlId, Interface};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

mod uvi;
#[cfg(test)]
mod snapshot_tests;

pub struct Witness {
    pub initial: Vec<Interface>,
    pub settled: Vec<Interface>,
    pub initial_values: Vec<(ControlId, f64)>,
    pub settled_values: Vec<(ControlId, f64)>,
    pub resource_path: PathBuf,
    pub report: Value,
}

impl Witness {
    /// Scripted full-loader replacements, with real callbacks, UI admission and
    /// discarded PCM. The request-order gate belongs to this diagnostic host;
    /// the plugin's worker/publication policy is deliberately not claimed.
    pub fn preset_switch_probe(plan: &Value) -> Result<Value, &'static str> {
        switch_probe(plan)
    }

    /// Explicit private PNG capture from the already-rendered post-action UI.
    /// The directory must be new and absolute. No artwork enters the report.
    pub fn preset_switch_probe_with_ui_artifacts(
        plan: &Value,
        directory: &Path,
        slot: u8,
        page: u32,
    ) -> Result<Value, &'static str> {
        switch_probe_inner(plan, Some((directory, sampler_ui_ir::Source::Ksp { slot }, page)))
    }

    /// Source-explicit capture; a Lua or performance-view face never aliases a KSP slot.
    pub fn preset_switch_probe_with_source_ui_artifacts(
        plan: &Value,
        directory: &Path,
        source: sampler_ui_ir::Source,
        page: u32,
    ) -> Result<Value, &'static str> {
        switch_probe_inner(plan, Some((directory, source, page)))
    }
}

const UI_ARTIFACT_IMAGES: usize = 24;
const UI_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;

struct UiArtifacts {
    directory: PathBuf,
    source: sampler_ui_ir::Source,
    page: u32,
    attempted_images: usize,
    reserved_bytes: usize,
    written_images: usize,
    written_bytes: usize,
}

impl UiArtifacts {
    #[cfg(test)]
    fn new(directory: &Path, slot: u8, page: u32) -> Result<Self, &'static str> {
        Self::new_source(directory, sampler_ui_ir::Source::Ksp { slot }, page)
    }

    fn new_source(directory: &Path, source: sampler_ui_ir::Source, page: u32) -> Result<Self, &'static str> {
        if source == sampler_ui_ir::Source::Generated {
            return Err("UI artifact capture requires an authored source");
        }
        if !directory.is_absolute() {
            return Err("UI artifact directory must be an absolute, new private directory");
        }
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(directory).map_err(
            |_| "UI artifact directory could not be created; existing directories are refused",
        )?;
        Ok(Self {
            directory: directory.to_path_buf(),
            source,
            page,
            attempted_images: 0,
            reserved_bytes: 0,
            written_images: 0,
            written_bytes: 0,
        })
    }

    fn target(&self, faces: &[Interface]) -> Result<usize, &'static str> {
        let mut targets = faces.iter().enumerate().filter_map(|(index, face)| {
            (face.source == self.source).then_some(index)
        });
        let first = targets.next().ok_or("slot_not_present")?;
        if targets.next().is_some() {
            return Err("ambiguous_slot");
        }
        Ok(first)
    }

    fn source_name(&self) -> &'static str {
        match self.source {
            sampler_ui_ir::Source::Ksp { .. } => "ksp",
            sampler_ui_ir::Source::FalconLua => "uvi",
            sampler_ui_ir::Source::PerformanceView => "performance-view",
            sampler_ui_ir::Source::Generated => "generated",
        }
    }

    fn slot(&self) -> Option<u8> {
        match self.source { sampler_ui_ir::Source::Ksp { slot } => Some(slot), _ => None }
    }

    fn outcome(&self, step: usize, generation: u64, status: &str) -> Value {
        json!({"status":status,"step":step,"generation":generation,"source":self.source_name(),"slot":self.slot(),"page":self.page,"presentation":"Bitmap"})
    }

    fn capture(&mut self, step: usize, generation: u64, report: &Value) -> Value {
        use std::io::Write;
        let mut shots = report["shots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|shot| {
                shot["page"].as_u64() == Some(u64::from(self.page))
                    && shot["presentation"] == "Bitmap"
            });
        let Some(shot) = shots.next() else {
            return self.outcome(step, generation, "shot_not_rendered");
        };
        if shots.next().is_some() {
            return self.outcome(step, generation, "ambiguous_shot");
        }
        let mut result = self.outcome(step, generation, "png_missing");
        for field in ["status", "pending_assets", "width", "height", "scale"] {
            let key = if field == "status" {
                "render_status"
            } else {
                field
            };
            result[key] = shot[field].clone();
        }
        let Some(encoded) = shot["png_hex"].as_str() else {
            return result;
        };
        if self.attempted_images >= UI_ARTIFACT_IMAGES {
            result["status"] = json!("image_limit");
            return result;
        }
        if !encoded.len().is_multiple_of(2) {
            result["status"] = json!("malformed_png_hex");
            return result;
        }
        let bytes = encoded.len() / 2;
        if bytes > UI_ARTIFACT_BYTES.saturating_sub(self.reserved_bytes) {
            result["status"] = json!("byte_limit");
            return result;
        }
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        };
        let mut png = Vec::with_capacity(bytes);
        for pair in encoded.as_bytes().chunks_exact(2) {
            let (Some(high), Some(low)) = (digit(pair[0]), digit(pair[1])) else {
                result["status"] = json!("malformed_png_hex");
                return result;
            };
            png.push((high << 4) | low);
        }
        if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
            result["status"] = json!("invalid_png_signature");
            return result;
        }
        // Reserve before opening/writing: partial files and refused overwrites
        // consume the same finite budget as a successful capture.
        self.attempted_images += 1;
        self.reserved_bytes += png.len();
        let selection = self.slot().map_or_else(|| self.source_name().to_owned(), |slot| format!("slot-{slot:02}"));
        let filename = format!(
            "step-{step:03}-generation-{generation:06}-{selection}-page-{:03}-bitmap.png",
            self.page
        );
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(self.directory.join(filename));
        let mut file = match file {
            Ok(file) => file,
            Err(error) => {
                result["status"] = json!(if error.kind() == std::io::ErrorKind::AlreadyExists {
                    "already_exists"
                } else {
                    "write_failed"
                });
                return result;
            }
        };
        if file.write_all(&png).is_err() {
            result["status"] = json!("write_failed");
            return result;
        }
        self.written_images += 1;
        self.written_bytes += png.len();
        result["status"] = json!("written");
        result["bytes"] = json!(png.len());
        result["png_sha256"] = json!(sha256(&png));
        result
    }

    fn summary(&self) -> Value {
        json!({"source":self.source_name(),"slot":self.slot(),"page":self.page,"presentation":"Bitmap",
            "image_limit":UI_ARTIFACT_IMAGES,"byte_limit":UI_ARTIFACT_BYTES,
            "attempted_images":self.attempted_images,"reserved_bytes":self.reserved_bytes,
            "written_images":self.written_images,"written_bytes":self.written_bytes,
            "scope":"Private post-action CPU projection of the exact authored source/page; fresh diagnostic asset owner, no popup/pointer/native-host or warm-cache equivalence"})
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SwitchPlan {
    schema: u32,
    #[serde(default)]
    render_ui: bool,
    #[serde(default)]
    residency_wait_ms: Option<u16>,
    #[serde(default)]
    uvi: Option<uvi::Config>,
    #[serde(default = "default_note_key")]
    key: u8,
    #[serde(default = "default_note_velocity")]
    velocity: u8,
    #[serde(default = "default_note_frames")]
    note_frames: usize,
    steps: Vec<SwitchStep>,
}

fn default_note_key() -> u8 {
    60
}
fn default_note_velocity() -> u8 {
    64
}
fn default_note_frames() -> usize {
    4800
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum SwitchStep {
    Load {
        path: PathBuf,
        #[serde(default)]
        base: Option<PathBuf>,
        #[serde(default)]
        program: u32,
        #[serde(default)]
        cancel_at_progress: Option<u16>,
    },
    Prepare {
        path: PathBuf,
        #[serde(default)]
        base: Option<PathBuf>,
        #[serde(default)]
        program: u32,
        #[serde(default)]
        cancel_at_progress: Option<u16>,
    },
    Publish {
        request: u64,
    },
    /// Exercise the same prepared mapping and native queued transaction as
    /// the plugin. Refusal is reported; this action never hides a full reload.
    Snapshot {
        path: PathBuf,
        #[serde(default)]
        release_held: bool,
    },
    UiEdit {
        generation: u64,
        control: String,
        value: f64,
    },
    WidgetEdit {
        generation: u64,
        slot: u8,
        source_id: i32,
        #[serde(default)]
        index: u32,
        #[serde(default)]
        require_visible_enabled: bool,
        value: SwitchValue,
    },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum SwitchValue {
    Integer(i32),
    Real(f64),
}

impl SwitchValue {
    fn ui(self) -> sampler_ui_ir::Value {
        match self {
            Self::Integer(value) => sampler_ui_ir::Value::Integer(value),
            Self::Real(value) => sampler_ui_ir::Value::Real(value),
        }
    }
}

type PreparedSwitch = crate::sound::Loaded<Option<Box<Part>>>;

#[derive(Default)]
struct SwitchOwner {
    request: u64,
    path: PathBuf,
    program: u32,
    resource_path: PathBuf,
    scripts: ScriptUi,
    stream: Option<std::sync::Arc<crate::sound::Stream>>,
    controls: Vec<ControlId>,
    held: Option<HostNote>,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn fingerprint(value: &impl std::fmt::Debug) -> String {
    sha256(format!("{value:?}").as_bytes())
}

fn switch_control_id(value: &str) -> Option<ControlId> {
    let value = value.strip_prefix("0x")?;
    if value.len() != 32 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(ControlId(u128::from_str_radix(value, 16).ok()?))
}

fn switch_state(
    core: &V2Core,
    owner: &mut SwitchOwner,
) -> (Vec<Interface>, Vec<(ControlId, f64)>, Value) {
    let faces = owner.scripts.interfaces();
    let values: Vec<_> = owner
        .controls
        .iter()
        .filter_map(|&id| core.control_value(0, id).map(|v| (id, v)))
        .collect();
    // Identity comparison includes order, binding and kind. It is evidence,
    // never permission to reuse a runtime, resource resolver or UI instance.
    let identities: Vec<_> = faces
        .iter()
        .map(|face| {
            face.widgets
                .iter()
                .map(|w| {
                    (
                        &w.name,
                        w.source_id,
                        &w.binding,
                        std::mem::discriminant(&w.kind),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let menus: Vec<_> = faces.iter().enumerate().flat_map(|(face, f)| f.widgets.iter().enumerate().filter_map(move |(widget, w)| {
        let sampler_ui_ir::Kind::Menu { items } = &w.kind else { return None };
        let control = match w.binding { Binding::Control(id) => Some(id), _ => None };
        Some(json!({"interface":face,"widget":widget,"control":control.map(|id|format!("0x{:032x}",id.0)),
            "selected_value":control.and_then(|id|core.control_value(0,id)),"hidden":w.hidden,"enabled":w.enabled,
            "items":items.iter().map(|item|json!({"value":item.value,"visible":item.visible,"caption_sha256":sha256(item.text.as_bytes())})).collect::<Vec<_>>()}))
    })).collect();
    let state = json!({"audio_generation":core.epoch(0),"request":owner.request,"selected_path":owner.path,"program":owner.program,
        "resolved_owner":owner.resource_path,"interfaces":faces.len(),"widgets":faces.iter().map(|f|f.widgets.len()).sum::<usize>(),
        "visible_widgets":faces.iter().map(|face|(0..face.widgets.len()).filter(|&i|face.visible(sampler_ui_ir::WidgetRef(i))).count()).sum::<usize>(),
        "control_identity_sha256":fingerprint(&identities),"ui_state_sha256":fingerprint(&faces),"control_state_sha256":fingerprint(&values),
        "menus":menus,"native_equivalence":"unknown"});
    (faces, values, state)
}

fn switch_projection(
    owner: &SwitchOwner,
    faces: &[Interface],
    values: &[(ControlId, f64)],
    artifacts: &mut Option<UiArtifacts>,
    step: usize,
) -> (Value, Option<Value>) {
    let target = artifacts.as_ref().map(|sink| sink.target(faces));
    let mut capture = match &target {
        Some(Err(status)) => Some(
            artifacts
                .as_ref()
                .unwrap()
                .outcome(step, owner.request, status),
        ),
        _ => None,
    };
    let projection = json!(faces.iter().enumerate().map(|(index, face)| {
        let report=super::offline_preview::render(face,&owner.resource_path,values,owner.program,faces);
        if target == Some(Ok(index)) {
            capture = artifacts.as_mut().map(|sink| sink.capture(step, owner.request, &report));
        }
        // Pixels reach only the explicit private sink, never the JSON report.
        json!({"status":report["status"],"timings":report["timings"],"asset_summary":report["asset_summary"],
            "asset_probe_dropped":report["asset_probe_dropped"],"shots":report["shots"].as_array().into_iter().flatten().map(|s|
                json!({"status":s["status"],"pending_assets":s["pending_assets"],"preparation_ms":s["preparation_ms"],"paint_and_encode_ms":s["paint_and_encode_ms"]})).collect::<Vec<_>>()})
    }).collect::<Vec<_>>());
    (projection, capture)
}

#[derive(Clone, Copy)]
struct SwitchWidgetTarget<'a> {
    widget: &'a sampler_ui_ir::Widget,
    effective_visible: bool,
}

fn switch_widget_target(
    faces: &[Interface],
    slot: u8,
    source_id: i32,
) -> Vec<SwitchWidgetTarget<'_>> {
    faces
        .iter()
        .filter(|face| face.source == sampler_ui_ir::Source::Ksp { slot })
        .flat_map(|face| {
            face.widgets
                .iter()
                .enumerate()
                .filter_map(move |(index, widget)| {
                    (widget.source_id == Some(source_id)).then(|| SwitchWidgetTarget {
                        widget,
                        effective_visible: face.pages.get(widget.page.0).is_some()
                            && face.visible(sampler_ui_ir::WidgetRef(index)),
                    })
                })
        })
        .collect()
}

fn switch_widget_blocked_reason(
    targets: &[SwitchWidgetTarget<'_>],
    require_visible_enabled: bool,
) -> Option<&'static str> {
    if !require_visible_enabled {
        return None;
    }
    match targets {
        [] => Some("target_not_found"),
        [target] if !target.effective_visible => Some("target_not_effectively_visible"),
        [target] if !target.widget.enabled => Some("target_disabled"),
        [_] => None,
        _ => Some("ambiguous_target"),
    }
}

fn switch_submit_widget(
    ingress: Option<&mut ControlIngress>,
    targets: &[SwitchWidgetTarget<'_>],
    slot: u8,
    cell: u32,
    value: SwitchValue,
    require_visible_enabled: bool,
) -> (Option<bool>, Option<&'static str>) {
    let blocked_reason = switch_widget_blocked_reason(targets, require_visible_enabled);
    if blocked_reason.is_some() {
        return (Some(false), blocked_reason);
    }
    let submitted = match (targets, ingress) {
        ([target], Some(ingress)) => Some(ingress.submit_ui_widgets(
            slot,
            target.widget,
            vec![(cell, value.ui())],
            Default::default(),
        )),
        _ => None,
    };
    (submitted, None)
}

/// Fixed-frame render, irrespective of whether finite callbacks are pending.
/// No samples or artwork survive the numeric report.
fn switch_render(
    core: &mut V2Core,
    owner: &mut SwitchOwner,
    frames: usize,
) -> Result<Value, &'static str> {
    let start = Instant::now();
    let mut rendered = 0usize;
    let mut peak = 0f32;
    let mut square_sum = 0f64;
    let mut nonzero_frames = 0usize;
    let mut nonzero_blocks = 0usize;
    let mut note_ends = 0usize;
    let mut effects = 0usize;
    let mut max_voices = core.voices().active;
    let mut hash = Sha256::new();
    let mut completed_callbacks = BTreeMap::new();
    let mut service_failure = None;
    while rendered < frames && start.elapsed() < Duration::from_secs(5) {
        let n = (frames - rendered).min(128);
        core.begin_block(&BlockInfo {
            frames: n,
            offline: true,
            ..Default::default()
        });
        let output = core.render(n);
        let mut block_signal = false;
        for frame in 0..n {
            let mut frame_signal = false;
            for bus in output.buses {
                for channel in bus {
                    let sample = channel[frame];
                    if !sample.is_finite() {
                        return Err("Nonfinite preset-switch output");
                    }
                    peak = peak.max(sample.abs());
                    square_sum += f64::from(sample) * f64::from(sample);
                    frame_signal |= sample != 0.;
                    hash.update(sample.to_le_bytes());
                }
            }
            nonzero_frames += usize::from(frame_signal);
            block_signal |= frame_signal;
        }
        nonzero_blocks += usize::from(block_signal);
        let mut progress = core.ui_lifecycle_progress(0);
        classify_callbacks(&mut progress, &owner.scripts.views);
        observe_activation_completions(&progress, &mut completed_callbacks);
        core.end_block(n, &mut |_| {
            note_ends += 1;
            true
        });
        rendered += n;
        max_voices = max_voices.max(core.voices().active);
        match core.service_ui_read_only(0, &mut owner.scripts.views) {
            Ok(count) => effects += count,
            Err(e) => {
                service_failure = Some(format!("{:?}", e.kind()));
                break;
            }
        }
    }
    let samples = rendered * crate::sound::BUSES * 2;
    Ok(
        json!({"requested_frames":frames,"frames":rendered,"complete":rendered==frames,"peak":peak,
        "rms_all_output_channels":if samples==0 {0.}else{(square_sum/samples as f64).sqrt()},
        "nonzero_frames":nonzero_frames,"nonzero_blocks":nonzero_blocks,"pcm_sha256":format!("{:x}",hash.finalize()),
        "audio_generation":core.epoch(0),"max_active_voices":max_voices,"active_voices_after":core.voices().active,
        "held_note_owned":owner.held.is_some_and(|note|core.owns(note)),"note_ends":note_ends,
        "effects_applied":effects,"service_failure_kind":service_failure,"runtime_problems":core.problems(0),
        "completed_callback_programs_observed":completion_summary(&completed_callbacks),
        "wall_ms":start.elapsed().as_secs_f64()*1000.,"scope":"All output pairs; fixed-frame offline render and real read-only UI drain, discarded PCM; wall threshold checked between blocks"}),
    )
}

fn switch_error(error: &CoreError) -> Value {
    match error {
        CoreError::Canceled => json!({"kind":"Canceled"}),
        CoreError::Load(e) => {
            json!({"kind":"Load","stage":e.stage.map(|s|format!("{s:?}")),"source_kind":e.kind.map(|k|format!("{k:?}"))})
        }
        CoreError::Unsupported(_) => json!({"kind":"Unsupported"}),
        CoreError::Capacity(_) => json!({"kind":"Capacity"}),
        CoreError::Invalid(_) => json!({"kind":"Invalid"}),
    }
}

fn switch_marker(step: usize, phase: &str) {
    eprintln!("PRESET_SWITCH {}", json!({"step":step,"phase":phase}));
}

fn switch_residency_state(core: &V2Core, owner: &SwitchOwner) -> Value {
    let Ok(p) = core.diagnostic_residency_progress(0, owner.request) else {
        return json!({"status":"unavailable"});
    };
    let mut groups = Vec::new();
    for group in 0..p.groups_total.min(4096) {
        let Ok(g) = core.diagnostic_group_residency_progress(0, owner.request, group as u32) else {
            return json!({"status":"group_query_failed"});
        };
        groups.push(json!({"group":g.group,"source_group":g.source_group,"loaded":g.loaded,
            "pending":g.pending,"revision":g.logical_revision,"requested_reload":g.latest_requested_reload}));
    }
    json!({"status":"observed","managed":p.managed,"groups_total":p.groups_total,
        "groups_loaded":p.groups_loaded,"groups_unloaded":p.groups_unloaded,
        "groups_with_pending":p.groups_with_pending,"groups":groups,
        "groups_omitted":p.groups_total.saturating_sub(4096),"job_capacity":p.job_capacity,
        "queued":p.queued,"in_flight":p.in_flight,"worker_ready":p.worker_ready,"pending":p.pending,
        "startup_completions_pending":p.startup_completions_pending,
        "accepted":p.accepted,"consumed":p.consumed,"committed":p.committed,"failed":p.failed,
        "stale":p.stale,"canceled":p.canceled,"rejected":p.rejected,
        "rejected_queue":p.rejected_queue,"rejected_group":p.rejected_group,
        "rejected_async_id":p.rejected_async_id,"rejected_callback_counter":p.rejected_callback_counter,
        "rejected_revision":p.rejected_revision,"completion_admission_failures":p.completion_admission_failures,
        "first_rejection":p.first_rejection.map(|r|json!({"group":r.group,"reload":r.reload,"reason":format!("{:?}",r.reason)})),
        "counters_saturated":p.counters_saturated,
        "scope":"Bounded off-audio observations of this residency owner; worker fields can change during query. Loaded is eligibility, not PCM/native parity. Purged groups are not forced resident"})
}

fn switch_stream_state(owner: &SwitchOwner) -> Value {
    let Some(stream) = &owner.stream else {
        return json!({"status":"not_streamed"});
    };
    json!({"status":"observed",
        "startup":crate::sound::report::StreamStartup::of(
            &stream.streamer, &stream.assets, &stream.report),
        "scope":"Current prepared physical asset/head metadata after this stage; includes actual hint intersections. Source request geometry is unchanged, dynamic UVI sample jobs are reported separately. No source reads or PCM retained"})
}

fn switch_settle_residency(
    core: &mut V2Core,
    owner: &mut SwitchOwner,
    limit_ms: u16,
) -> Result<Value, &'static str> {
    let before = switch_residency_state(core, owner);
    let started = Instant::now();
    let mut callback_frames = 0usize;
    let mut polls = 0usize;
    let mut effects = 0usize;
    let status = loop {
        let p = core.diagnostic_residency_progress(0, owner.request)
            .map_err(|_| "Residency owner was replaced during explicit settlement")?;
        let mut callbacks = core.ui_lifecycle_progress(0);
        classify_callbacks(&mut callbacks, &owner.scripts.views);
        let finite = callbacks["pending_non_background_plan_callbacks"].as_u64().unwrap_or(1);
        if p.pending == 0 && p.startup_completions_pending == 0
            && finite == 0 && callbacks["pending_startup_work"] != true {
            break if p.rejected != 0 || p.failed != 0 { "settled_with_failures" } else { "settled" };
        }
        if started.elapsed() >= Duration::from_millis(u64::from(limit_ms)) {
            break "wall_limit";
        }
        if callback_frames >= 96000 || effects > 65536 {
            break "callback_limit";
        }
        // Admit actual completed jobs at the ordinary zero-frame boundary.
        // This never changes a group's authored target or forces an unpurge.
        core.begin_block(&BlockInfo { frames:0, offline:true, ..Default::default() });
        let _ = core.render(0);
        core.end_block(0, &mut |_| true);
        effects += core.service_ui_read_only(0, &mut owner.scripts.views)
            .map_err(|_| "Read-only residency completion effect service failed")?;
        if finite != 0 {
            let rendered = switch_render(core, owner, 128)?;
            effects += rendered["effects_applied"].as_u64().unwrap_or(0) as usize;
            callback_frames += 128;
        }
        polls += 1;
        std::thread::sleep(Duration::from_millis(1));
    };
    Ok(json!({"status":status,"before":before,"after":switch_residency_state(core,owner),
        "wall_ms":started.elapsed().as_secs_f64()*1000.,"wall_limit_ms":limit_ms,
        "polls":polls,"callback_frames":callback_frames,"effects_applied":effects,
        "scope":"Explicit wall-bounded residency settlement before a fresh note; zero-frame commits plus bounded finite callbacks. Rejections/failures stay visible; no blanket unpurge or queue-policy change"}))
}

fn switch_probe(plan: &Value) -> Result<Value, &'static str> {
    switch_probe_inner(plan, None)
}

fn switch_probe_inner(
    plan: &Value,
    artifact_request: Option<(&Path, sampler_ui_ir::Source, u32)>,
) -> Result<Value, &'static str> {
    let plan: SwitchPlan =
        serde_json::from_value(plan.clone()).map_err(|_| "Invalid preset-switch plan schema")?;
    if plan.schema != 1
        || plan.steps.is_empty()
        || plan.steps.len() > 24
        || plan.key > 127
        || !(1..=127).contains(&plan.velocity)
        || !(128..=96000).contains(&plan.note_frames)
        || plan.residency_wait_ms.is_some_and(|ms| ms == 0 || ms > 10000)
    {
        return Err(
            "Preset-switch schema 1 requires 1..24 steps, valid MIDI key/velocity, 128..96000 note frames and optional residency_wait_ms in 1..10000",
        );
    }
    if let Some(config) = &plan.uvi {
        config.validate(plan.note_frames)?;
    }
    for step in &plan.steps {
        match step {
            SwitchStep::Load {
                path,
                base,
                cancel_at_progress,
                ..
            }
            | SwitchStep::Prepare {
                path,
                base,
                cancel_at_progress,
                ..
            } => {
                if path.as_os_str().is_empty()
                    || cancel_at_progress.is_some_and(|p| p > 1000)
                    || (base.is_some()
                        && !path
                            .extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("nksn")))
                {
                    return Err(
                        "Invalid preset-switch source, explicit snapshot base or cancellation threshold",
                    );
                }
            }
            SwitchStep::Snapshot { path, .. } if path.as_os_str().is_empty()
                || !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("nksn")) => {
                return Err("Snapshot action requires a nonempty NKSN path");
            }
            SwitchStep::UiEdit { control, value, .. }
                if !value.is_finite() || switch_control_id(control).is_none() =>
            {
                return Err(
                    "Preset-switch UI edit requires a finite value and 0x-prefixed 32-hex-digit control ID",
                );
            }
            SwitchStep::WidgetEdit {
                value: SwitchValue::Real(value),
                ..
            } if !value.is_finite() => {
                return Err("Preset-switch typed real widget value must be finite");
            }
            _ => {}
        }
    }
    if artifact_request.is_some() && !plan.render_ui {
        return Err("UI artifact capture requires render_ui: true in the plan");
    }
    let mut artifacts = artifact_request
        .map(|(directory, source, page)| UiArtifacts::new_source(directory, source, page))
        .transpose()?;
    let mut core = V2Core::with_parts(1, 48000.);
    let mut owner = SwitchOwner::default();
    let mut latest_request = 0u64;
    let mut prepared = BTreeMap::<u64, (PreparedSwitch, PathBuf, u32)>::new();
    let mut ingresses = BTreeMap::<u64, ControlIngress>::new();
    let mut ingress_faces = BTreeMap::<u64, Vec<Interface>>::new();
    let mut previous_states =
        BTreeMap::<(PathBuf, u32), (Vec<Interface>, Vec<(ControlId, f64)>)>::new();
    let mut records = Vec::new();
    for (index, operation) in plan.steps.into_iter().enumerate() {
        switch_marker(index, "begin");
        let prepare_only = matches!(&operation, SwitchStep::Prepare { .. });
        let (_, _, before) = switch_state(&core, &mut owner);
        let mut record = json!({"step":index,"before":before});
        let mut publish = None;
        match operation {
            SwitchStep::Load {
                path,
                base,
                program,
                cancel_at_progress,
            }
            | SwitchStep::Prepare {
                path,
                base,
                program,
                cancel_at_progress,
            } => {
                latest_request += 1;
                let generation = latest_request;
                record["request"] = json!(generation);
                record["selected_path"] = json!(path);
                record["program"] = json!(program);
                record["cancel_at_progress"] = json!(cancel_at_progress);
                let canceled = AtomicBool::new(cancel_at_progress == Some(0));
                let mut progress_events = Vec::new();
                let start = Instant::now();
                switch_marker(index, "prepare");
                let request = LoadRequest {
                    path: base.as_deref().unwrap_or(&path).to_path_buf(),
                    snapshot: base.map(|_| path.clone()),
                    program,
                    sample_rate: 48000.,
                    streaming: Streaming::Auto,
                    ..Default::default()
                };
                let result = V2Loader.prepare(&request,&mut |progress| {
                    if progress_events.len()<2048 { progress_events.push(json!({"progress":progress.0,"elapsed_ms":start.elapsed().as_secs_f64()*1000.})); }
                    if cancel_at_progress.is_some_and(|p|progress.0>=p) { canceled.store(true,Ordering::Release); }
                },&||canceled.load(Ordering::Acquire));
                record["prepare_ms"] = json!(start.elapsed().as_secs_f64() * 1000.);
                record["progress_events"] = json!(progress_events);
                record["cancellation_requested"] = json!(canceled.load(Ordering::Acquire));
                match result {
                    Ok(loaded) if !canceled.load(Ordering::Acquire) => {
                        record["resolved_owner"] = json!(
                            loaded
                                .scripts
                                .resource_path
                                .as_ref()
                                .unwrap_or(&request.path)
                        );
                        record["load_metrics"] = json!({"samples":loaded.report.decoded.samples,"zones":loaded.report.decoded.zones,
                            "source_conversion_gap_count":loaded.report.missing.len(),"stream_startup":loaded.report.stream_startup,
                            "head_bytes":loaded.stream.as_ref().map(|s|s.report.head_bytes),"pool_bytes":loaded.stream.as_ref().map(|s|s.report.pool_bytes),
                            "full_sample_bytes":loaded.stream.as_ref().map(|s|s.report.full_bytes),
                            "ui_interfaces_initial":loaded.interfaces.len(),"scripts":loaded.scripts.views.len(),
                            "uvi_fault_counts":{"first":loaded.report.uvi_faults.first,
                                "init":loaded.report.uvi_faults.init,"runtime":loaded.report.uvi_faults.runtime,
                                "setter_type_mismatch_count":loaded.report.uvi_faults.setter_type_mismatches.len()}});
                        publish = Some((generation, loaded, path, program));
                    }
                    Ok(_) => record["outcome"] = json!("canceled_before_publish"),
                    Err(error) => {
                        record["outcome"] = json!(if matches!(error, CoreError::Canceled) {
                            "canceled"
                        } else {
                            "failed"
                        });
                        record["error"] = switch_error(&error);
                    }
                }
            }
            SwitchStep::Snapshot { path, release_held } => {
                record["selected_path"] = json!(path);
                record["strategy"] = json!("same_owner_queued_snapshot");
                record["gate_scope"] = json!("Diagnostic host uses a fixed owner epoch; plugin request and host recall fences are tested separately");
                if release_held {
                    if let Some(note) = owner.held.take() {
                        core.play(0, Event::NoteOff(crate::sound::event::HostPattern { port: i32::from(note.port), channel: i32::from(note.channel), key: i32::from(note.key), id: note.id, clap: note.clap }));
                    }
                    record["release_before_snapshot"] = switch_render(&mut core, &mut owner, 4096)?;
                }
                let start = Instant::now();
                switch_marker(index, "snapshot_reuse_prepare");
                let ready = owner.scripts.snapshot_reuse.as_ref()
                    .ok_or(crate::sound::v2::snapshot_reuse::Refusal::NoSetup)
                    .and_then(|descriptor| descriptor.prepare(&path, 48000, &owner.scripts.views));
                record["prepare_ms"] = json!(start.elapsed().as_secs_f64()*1000.);
                match ready {
                    Err(reason) => {
                        record["outcome"] = json!("snapshot_reuse_refused");
                        record["refusal"] = serde_json::to_value(reason).unwrap_or_default();
                    }
                    Ok(ready) => {
                        let epoch = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(owner.request));
                        let command = ingresses.get_mut(&owner.request)
                            .ok_or("Snapshot action has no current ingress")?
                            .submit_snapshot_overlay(ready.state, Box::new([sampler_core::SnapshotOverlayEpoch {
                                sequence: epoch, expected: owner.request,
                            }])).map_err(|_| "Snapshot action queue refused payload")?;
                        record["snapshot_mode"] = json!(format!("{:?}", ready.mode));
                        switch_marker(index, "snapshot_reuse_apply");
                        record["apply_render"] = switch_render(&mut core, &mut owner, 128)?;
                        let reply = ingresses.get_mut(&owner.request).unwrap().take_snapshot_reply(command)
                            .ok_or("Snapshot action did not receive its bounded native reply")?;
                        let sampler_core::ControlOperation::NativeSnapshotOverlay(overlay) = reply.command.operation else {
                            return Err("Snapshot action received a foreign reply");
                        };
                        record["admission_result"] = json!(format!("{:?}", reply.result));
                        record["callback_admission_outcomes"] = json!(format!("{:?}", overlay.state.callbacks));
                        if reply.result.is_ok() && overlay.committed.is_some() {
                            owner.path = path;
                            record["outcome"] = json!("snapshot_applied_same_owner");
                            record["activation"] = step(&mut core, &mut owner.scripts.views)?;
                            let (faces, values, state) = switch_state(&core, &mut owner);
                            record["after_activation"] = state;
                            if plan.render_ui {
                                let (projection, capture) = switch_projection(&owner, &faces, &values, &mut artifacts, index);
                                record["ui_projection"] = projection;
                                if let Some(capture) = capture { record["ui_artifact"] = capture; }
                            }
                            let note = HostNote { port: 0, channel: 0, key: plan.key, id: (index+10000) as i32, clap: true };
                            core.play(0, Event::NoteOn { note, velocity: f64::from(plan.velocity)/127., tune: 0. });
                            owner.held = Some(note);
                            record["fresh_note"] = switch_render(&mut core, &mut owner, plan.note_frames)?;
                        } else {
                            record["outcome"] = json!("snapshot_reuse_refused");
                            record["refusal"] = json!(format!("{:?}", overlay.refusal));
                        }
                    }
                }
            }
            SwitchStep::Publish { request } => {
                record["request"] = json!(request);
                if request != latest_request {
                    record["prepared_completion_present"] =
                        json!(prepared.remove(&request).is_some());
                    record["outcome"] = json!("stale_completion_discarded_by_probe_gate");
                } else if let Some((loaded, path, program)) = prepared.remove(&request) {
                    publish = Some((request, loaded, path, program));
                } else {
                    record["outcome"] = json!("no_prepared_request");
                }
            }
            SwitchStep::UiEdit {
                generation,
                control,
                value,
            } => {
                let id =
                    switch_control_id(&control).ok_or("Invalid preset-switch control identity")?;
                let before_value = core.control_value(0, id);
                let submitted = ingresses
                    .get_mut(&generation)
                    .map(|ingress| ingress.submit(id, value));
                record["outcome"] = json!("ui_edit_observed");
                record["ui_edit"] = json!({"generation":generation,"active_generation":core.epoch(0),"stale":generation!=core.epoch(0),
                    "control":control,"requested_value":value,"old_ingress_present":submitted.is_some(),"submitted":submitted,"active_value_before":before_value});
                if owner.request != 0 {
                    switch_marker(index, "ui_edit_drain");
                    record["ui_edit"]["render"] = switch_render(&mut core, &mut owner, 512)?;
                }
                record["ui_edit"]["active_value_after"] = json!(core.control_value(0, id));
                record["ui_edit"]["active_value_changed"] =
                    json!(before_value != core.control_value(0, id));
            }
            SwitchStep::WidgetEdit {
                generation,
                slot,
                source_id,
                index: cell,
                require_visible_enabled,
                value,
            } => {
                let (live_faces, _, _) = switch_state(&core, &mut owner);
                let faces: &[Interface] = if generation == owner.request {
                    &live_faces
                } else {
                    ingress_faces
                        .get(&generation)
                        .map_or(&[][..], Vec::as_slice)
                };
                let targets = switch_widget_target(faces, slot, source_id);
                let target = if targets.len() == 1 {
                    Some(targets[0])
                } else {
                    None
                };
                let (submitted, blocked_reason) = switch_submit_widget(
                    ingresses.get_mut(&generation),
                    &targets,
                    slot,
                    cell,
                    value,
                    require_visible_enabled,
                );
                record["outcome"] = json!("widget_edit_observed");
                record["widget_edit"] = json!({"generation":generation,"active_generation":core.epoch(0),"stale":generation!=core.epoch(0),
                    "slot":slot,"source_id":source_id,"index":cell,"typed_value":format!("{value:?}"),"target_count":targets.len(),
                    "target_hidden":target.map(|target|target.widget.hidden),"target_enabled":target.map(|target|target.widget.enabled),
                    "target_effective_visible":target.map(|target|target.effective_visible),"target_page":target.map(|target|target.widget.page.0),
                    "require_visible_enabled":require_visible_enabled,"blocked_reason":blocked_reason,
                    "ingress_present":ingresses.contains_key(&generation),"submitted":submitted,
                    "scope":"Exact script-slot/source-ID resolution, one typed scalar/cell edit through production submit_ui_widgets/InvokeWidget; no file/text/drop services"});
                if owner.request != 0 {
                    switch_marker(index, "ui_edit_drain");
                    record["widget_edit"]["render"] = switch_render(&mut core, &mut owner, 512)?;
                    switch_marker(index, "activation");
                    record["widget_edit"]["callback_settle"] =
                        step(&mut core, &mut owner.scripts.views)?;
                    let (faces, values, state) = switch_state(&core, &mut owner);
                    record["widget_edit"]["after_callbacks"] = state;
                    if plan.render_ui {
                        switch_marker(index, "ui_projection_assets");
                        let (projection, capture) =
                            switch_projection(&owner, &faces, &values, &mut artifacts, index);
                        record["ui_projection"] = projection;
                        if let Some(capture) = capture {
                            record["ui_artifact"] = capture;
                        }
                    }
                }
            }
        }
        if let Some((generation, mut loaded, path, program)) = publish {
            if prepare_only {
                if prepared.len() >= 2 {
                    return Err("Preset-switch probe permits at most two staged preparations");
                }
                prepared.insert(generation, (loaded, path, program));
                record["outcome"] = json!("prepared_only");
            } else {
                switch_marker(index, "publish");
                let previous_held = owner.held;
                let held_before = previous_held.is_some_and(|note| core.owns(note));
                let resource_path = loaded
                    .scripts
                    .resource_path
                    .clone()
                    .unwrap_or_else(|| path.clone());
                let same_base = owner.request != 0
                    && owner.resource_path == resource_path
                    && owner.program == program;
                if let Some(part) = &mut loaded.part {
                    part.epoch = generation;
                    if let Some(ingress) = part.ui_controls.take() {
                        ingresses.insert(generation, ingress);
                        ingress_faces.insert(generation, loaded.interfaces.clone());
                    }
                }
                let publish_start = Instant::now();
                let retired = core.install(0, loaded.part);
                record["publish_ms"] = json!(publish_start.elapsed().as_secs_f64() * 1000.);
                drop(retired);
                owner = SwitchOwner {
                    request: generation,
                    path,
                    program,
                    resource_path,
                    scripts: loaded.scripts,
                    stream: loaded.stream,
                    controls: loaded.controls.into_iter().map(|(id, _)| id).collect(),
                    held: previous_held,
                };
                switch_marker(index, "activation");
                record["activation"] = step(&mut core, &mut owner.scripts.views)?;
                if owner.scripts.uvi.is_some() {
                    record["activation"]["status"] = json!("unverified");
                    record["activation"]["activation_readiness"] = json!("uvi_owner_not_observed");
                    record["activation"]["readiness_scope"] = json!("KSP callback rows do not observe the independent UVI owner; an explicit owner acknowledgement and paced note condition are required");
                }
                if let Some(limit) = plan.residency_wait_ms {
                    switch_marker(index, "activation");
                    record["residency_settlement"] = switch_settle_residency(&mut core, &mut owner, limit)?;
                }
                record["stream_after_activation"] = switch_stream_state(&owner);
                // Full replacement may end held notes. This is a transition
                // observation, not the fresh-note compatibility verdict.
                switch_marker(index, "post_publish_held");
                record["held_transition"] = json!({"held_before":held_before,"same_base":same_base,"strategy":"full_prepare_install",
                    "after_publish":switch_render(&mut core,&mut owner,512)?,
                    "interpretation":"Full runtime replacement can cut held voices; inspect fresh-note output after activation separately"});
                let (faces, values, state) = switch_state(&core, &mut owner);
                let previous = previous_states.insert(
                    (owner.path.clone(), owner.program),
                    (faces.clone(), values.clone()),
                );
                record["repeated_preset"] = json!({"observed_before":previous.is_some(),"complete_ui_equal":previous.as_ref().map(|(old,_)|old==&faces),
                    "controls_equal":previous.as_ref().map(|(_,old)|old==&values),"scope":"Exact initialized/activated state of this selected path and program; differences may be authored randomness or listener timing"});
                record["after_activation"] = state;
                if plan.render_ui {
                    switch_marker(index, "ui_projection_assets");
                    let (projection, capture) =
                        switch_projection(&owner, &faces, &values, &mut artifacts, index);
                    record["ui_projection"] = projection;
                    if let Some(capture) = capture {
                        record["ui_artifact"] = capture;
                    }
                }
                switch_marker(index, "fresh_note");
                record["residency_at_note"] = switch_residency_state(&core, &owner);
                record["stream_at_note"] = switch_stream_state(&owner);
                let note = HostNote {
                    port: 0,
                    channel: 0,
                    key: plan.key,
                    id: generation as i32,
                    clap: true,
                };
                if let Some(config) = &plan.uvi {
                    record["fresh_note"] = uvi::probe_note(
                        &mut core, &mut owner, config, note, plan.velocity, plan.note_frames,
                    )?;
                } else {
                    core.begin_block(&BlockInfo {
                        frames: 128,
                        offline: true,
                        ..Default::default()
                    });
                    core.play(
                        0,
                        Event::NoteOn {
                            note,
                            velocity: f64::from(plan.velocity) / 127.,
                            tune: 0.,
                        },
                    );
                    owner.held = Some(note);
                    record["fresh_note"] = switch_render(&mut core, &mut owner, plan.note_frames)?;
                    record["fresh_note"]["key"] = json!(plan.key);
                    record["fresh_note"]["velocity"] = json!(plan.velocity);
                    record["fresh_note"]["activation_readiness"] =
                        record["activation"]["activation_readiness"].clone();
                }
                record["stream_after_note"] = switch_stream_state(&owner);
                record["outcome"] = json!("published_and_probed");
            }
        }
        if owner.request != 0
            && record["outcome"] != "published_and_probed"
            && record["outcome"] != "ui_edit_observed"
            && record["outcome"] != "widget_edit_observed"
        {
            switch_marker(index, "retained_owner_audio");
            record["retained_owner_audio"] = switch_render(&mut core, &mut owner, 512)?;
        }
        let (_, _, after) = switch_state(&core, &mut owner);
        record["old_owner_preserved"] =
            json!(record["before"]["audio_generation"] == after["audio_generation"]);
        record["control_structure_same"] =
            json!(record["before"]["control_identity_sha256"] == after["control_identity_sha256"]);
        record["after"] = after;
        records.push(record);
        switch_marker(index, "end");
    }
    let mut report = json!({"format":"kontra-preset-switch-probe","schema":1,"steps":records,
        "build":crate::build_info::BUILD,"source_sha256":{"offline_lifecycle.rs":sha256(include_bytes!("offline_lifecycle.rs")),"sound/v2.rs":sha256(include_bytes!("../sound/v2.rs"))},
        "scope":"Real V2Loader and full Core::install for load actions; explicit snapshot actions retain the same owner through the production mapper/queued transaction. Deferred callbacks, UI command queues, streaming/DSP and optional CPU UI projection run without a plugin instance or audio device",
        "publication_gate_scope":"Request ordering and audio generation assignment are diagnostic-host policy. Explicit snapshot actions use the production mapper and queued native transaction without Core::install; plugin worker/recall races remain separate tests",
        "audio_scope":"Held-note transitions and fresh notes after bounded activation are separate; controllers remain at loader defaults",
        "stage_metrics_scope":"KONTRA_AUDIT_LOAD numeric stderr spans, grouped by PRESET_SWITCH phase markers; wrapper can count actual spans without reparsing sources",
        "native_equivalence":"unknown","universal_compatibility":"not_established"});
    if let Some(artifacts) = artifacts {
        report["ui_artifacts"] = artifacts.summary();
    }
    Ok(report)
}

/// The real loader, metadata getters, bounded scheduler and read-only UI outbox.
/// Output is measured then discarded. Authored plan-start/listener callbacks can
/// run; this host never injects note, controller, transport or UI input.
pub fn run(path: &Path, program: u32, base: Option<&Path>) -> Result<Witness, &'static str> {
    if !path.extension().is_some_and(|e| {
        e.eq_ignore_ascii_case("nki")
            || e.eq_ignore_ascii_case("nksn")
            || e.eq_ignore_ascii_case("nkm")
    }) {
        return Err(
            "Production UI lifecycle witness currently supports Kontakt instruments, multis and snapshots",
        );
    }
    if base.is_some()
        && !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nksn"))
    {
        return Err("Explicit lifecycle base requires a snapshot");
    }
    if let Some(base) = base {
        sampler_kontakt::validate_snapshot_base(path, base)
            .map_err(|_| "Snapshot and explicit base identities do not match")?;
    }
    let started = Instant::now();
    let request = LoadRequest {
        path: base.unwrap_or(path).to_path_buf(),
        snapshot: base.map(|_| path.to_path_buf()),
        program,
        sample_rate: 48_000.,
        streaming: Streaming::Auto,
        ..Default::default()
    };
    let mut loaded = V2Loader.prepare(&request, &mut |_| {}, &|| false).map_err(
        |_| "Production lifecycle loader rejected this source; authored payload omitted",
    )?;
    let load_ms = started.elapsed().as_secs_f64() * 1000.;
    if loaded.scripts.views.is_empty() {
        return Err("No compiled Kontakt script interface is available for lifecycle observation");
    }
    let resource_path = loaded
        .scripts
        .resource_path
        .clone()
        .ok_or("Production resource owner is unavailable")?;
    let initial = loaded.interfaces;
    let initial_values = loaded.controls;
    let control_ids: Vec<_> = initial
        .iter()
        .flat_map(|f| &f.widgets)
        .flat_map(|w| {
            match w.binding {
                Binding::Control(id) => Some(id),
                _ => None,
            }
            .into_iter()
            .chain(w.components.iter().copied())
        })
        .collect();
    let mut core = V2Core::with_parts(1, 48_000.);
    core.install(0, loaded.part);
    let mut report = step(&mut core, &mut loaded.scripts.views)?;

    let settled_values = control_ids
        .into_iter()
        .filter_map(|id| core.control_value(0, id).map(|value| (id, value)))
        .collect();
    let settled = loaded.scripts.interfaces();
    let changed_widgets = initial
        .iter()
        .zip(&settled)
        .map(|(a, b)| {
            a.widgets
                .iter()
                .zip(&b.widgets)
                .filter(|(a, b)| a != b)
                .count()
        })
        .sum::<usize>();
    report["load_ms"] = json!(load_ms);
    report["changed_widgets"] = json!(changed_widgets);
    report["source_conversion_gap_count"] = json!(loaded.report.missing.len());
    let mut gap_kinds = std::collections::BTreeMap::<&str, usize>::new();
    let mut gap_reasons = std::collections::BTreeMap::<String, usize>::new();
    for missing in &loaded.report.missing {
        // Feature families are translator metadata; omit source locations,
        // authored values, diagnostic text and variable names after the colon.
        *gap_kinds
            .entry(missing.feature.split(':').next().unwrap_or("unknown"))
            .or_default() += 1;
        *gap_reasons
            .entry(format!("{:?}", missing.reason))
            .or_default() += 1;
    }
    report["source_conversion_gap_kinds"] = json!(gap_kinds);
    report["source_conversion_gap_reasons"] = json!(gap_reasons);
    report["stream_full_sample_bytes"] = json!(loaded.stream.as_ref().map(|s| s.report.full_bytes));
    report["stream_sample_head_bytes"] = json!(loaded.stream.as_ref().map(|s| s.report.head_bytes));
    report["stream_pool_bytes"] = json!(loaded.stream.as_ref().map(|s| s.report.pool_bytes));
    report["total_ms"] = json!(started.elapsed().as_secs_f64() * 1000.);
    Ok(Witness {
        initial,
        settled,
        initial_values,
        settled_values,
        resource_path,
        report,
    })
}

fn step(core: &mut V2Core, views: &mut [sampler_ksp::ScriptView]) -> Result<Value, &'static str> {
    let step_started = Instant::now();
    let mut frames = 0u64;
    let mut effects = 0usize;
    let mut blocks = 0u32;
    let mut peak = 0f32;
    let mut max_voices = core.voices().active;
    let mut service_failure = None;
    let mut background_ready = false;
    let mut completed_listeners = std::collections::BTreeSet::new();
    let mut completed_callbacks = BTreeMap::new();
    while frames < 96_000 && step_started.elapsed() < Duration::from_secs(5) && effects <= 65_536 {
        match core.service_ui_read_only(0, views) {
            Ok(count) => effects += count,
            Err(error) => {
                service_failure = Some(format!("{:?}", error.kind()));
                break;
            }
        }
        let mut progress = core.ui_lifecycle_progress(0);
        classify_callbacks(&mut progress, views);
        observe_listener_completions(&progress, &mut completed_listeners);
        observe_activation_completions(&progress, &mut completed_callbacks);
        if !core.scripts_running(0) {
            break;
        }
        if ["live_callback_failures", "queued_faults", "dropped_faults"]
            .iter()
            .any(|key| progress[*key].as_u64().is_some_and(|n| n != 0))
        {
            break;
        }
        background_ready = listener_cycles_observed(&progress, &completed_listeners);
        if background_ready {
            break;
        }
        core.begin_block(&BlockInfo {
            frames: 128,
            offline: true,
            ..Default::default()
        });
        let output = core.render(128);
        for bus in output.buses {
            for channel in bus {
                for sample in &channel[..128] {
                    if !sample.is_finite() {
                        return Err("Nonfinite production lifecycle output");
                    }
                    peak = peak.max(sample.abs());
                }
            }
        }
        // end_block consumes terminal outcomes. Observe only static callback
        // roles and program IDs before that drain, without consuming anything.
        let mut terminal = core.ui_lifecycle_progress(0);
        classify_callbacks(&mut terminal, views);
        observe_listener_completions(&terminal, &mut completed_listeners);
        observe_activation_completions(&terminal, &mut completed_callbacks);
        core.end_block(128, &mut |_| true);
        frames += 128;
        blocks += 1;
        max_voices = max_voices.max(core.voices().active);
    }
    let pending = core.scripts_running(0);
    let problems = core.problems(0);
    let mut progress = core.ui_lifecycle_progress(0);
    classify_callbacks(&mut progress, views);
    let callback_failure = ["live_callback_failures", "queued_faults", "dropped_faults"]
        .iter()
        .any(|key| progress[*key].as_u64().is_some_and(|n| n != 0));
    let runtime_failure = problems.script_overruns != 0
        || problems.nonfinite != 0
        || problems.capacity_drops != 0
        || problems.underruns != 0
        || problems.offline_failures != 0;
    let failed = service_failure.is_some() || callback_failure || runtime_failure;
    let ready = !failed && (!pending || only_listener_drivers_pending(&progress));
    Ok(
        json!({"status":if failed || (pending && !background_ready) {"partial"}else if background_ready {"background_active"}else{"completed"},
        "activation_readiness":if ready {"finite_callbacks_not_pending"}else{"pending_or_failed"},
        "completed_listener_callbacks_observed":completed_listeners,
        "completed_callback_programs_observed":completion_summary(&completed_callbacks),
        "background_observation":if background_ready {"first_callbacks_observed"}else if progress["observed_pending_listener_drivers"].as_u64().is_some_and(|n|n!=0) {"pending_first_callbacks"}else{"none_active"},
        "readiness_scope":"Finite activation and ongoing listener work are distinct. Early stop requires a completed callback from each exact active driver mapping plus its UI effect drain. Later ticks can still change the UI; one cycle is not visual stability or native equivalence",
        "scope":"Production V2Loader/Part/V2Core plan activation and shared read-only KSP UI effect host; streaming and DSP runtime are instantiated, no plugin/DAW/audio device",
        "input_scope":"No host note/controller/transport/gesture input. Authored plan-start and listener callbacks may execute; output is discarded, not auditioned",
        "step_ms":step_started.elapsed().as_secs_f64()*1000.,"blocks":blocks,"frames":frames,"advanced_ms":frames as f64/48.,
        "audio_frame_limit":96_000,"wall_stop_threshold_ms":5000,"effect_stop_threshold":65_536,"stop_scope":"Wall/effect thresholds checked between bounded blocks and shared effect drains; outer read-only launcher timeout bounds the process",
        "effects_applied":effects,"pending_plan_callbacks":pending,"service_failure_kind":service_failure,"callback_progress":progress,
        "max_active_voices":max_voices,"output_observed":frames!=0,"discarded_output_peak":peak,"runtime_problems":problems,
        "native_equivalence":"unknown"}),
    )
}

/// `bind_modules` installs the views consecutively in a fresh program table.
/// Use its retained entry metadata, never infer a callback from a sampled PC.
fn classify_callbacks(progress: &mut Value, views: &[sampler_ksp::ScriptView]) {
    let mut kinds = std::collections::BTreeMap::<String, usize>::new();
    let mut unclassified = 0usize;
    let mut drivers = 0usize;
    for row in progress["callbacks"].as_array_mut().into_iter().flatten() {
        let Some(mut local) = row["program"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
        else {
            unclassified += 1;
            continue;
        };
        let mut found = false;
        for view in views {
            if local < view.program_count() {
                row["script_slot"] = json!(view.slot());
                row["local_program"] = json!(local);
                if let Some(kind) = view.program_role(local) {
                    row["callback_kind"] = json!(kind);
                    if let Some(callback) = view.listener_callback(local) {
                        row["listener_callback_program"] = json!(
                            row["program"].as_u64().unwrap() - local as u64 + callback as u64
                        );
                    }
                    if row["outcome"] == "pending" {
                        *kinds.entry(kind.to_owned()).or_default() += 1;
                        drivers += usize::from(kind == "listener driver" && row["owner"] == "plan");
                    }
                    found = true;
                }
                break;
            }
            local -= view.program_count();
        }
        unclassified += usize::from(!found);
    }
    progress["observed_pending_callback_kinds"] = json!(kinds);
    progress["observed_pending_listener_drivers"] = json!(drivers);
    progress["pending_non_background_plan_callbacks"] = json!(
        progress["pending_plan_callbacks"]
            .as_u64()
            .unwrap_or(0)
            .saturating_sub(drivers as u64)
    );
    progress["unclassified_callback_rows"] = json!(unclassified);
    progress["callback_metadata_scope"] = json!(
        "Retained ScriptView entry kinds and consecutive bind_modules program offsets; only bounded observed rows are classified. A recurring listener can legitimately remain waiting; absence of pending work is not proof of native equivalence"
    );
}

fn only_listener_drivers_pending(progress: &Value) -> bool {
    progress["pending_plan_callbacks"]
        .as_u64()
        .is_some_and(|n| n != 0)
        && progress["pending_non_background_plan_callbacks"] == 0
        && progress["pending_startup_work"] != true
}

fn observe_listener_completions(progress: &Value, observed: &mut std::collections::BTreeSet<u64>) {
    for row in progress["callbacks"].as_array().into_iter().flatten() {
        if row["owner"] == "plan"
            && row["callback_kind"] == "on listener"
            && row["outcome"] == "finished"
            && let Some(program) = row["program"].as_u64()
        {
            observed.insert(program);
        }
    }
}

fn observe_activation_completions(progress: &Value, observed: &mut BTreeMap<u64, String>) {
    for row in progress["callbacks"].as_array().into_iter().flatten() {
        if row["owner"] == "plan"
            && row["outcome"] == "finished"
            && let (Some(program), Some(kind)) =
                (row["program"].as_u64(), row["callback_kind"].as_str())
        {
            observed.insert(program, kind.to_owned());
        }
    }
}

fn completion_summary(observed: &BTreeMap<u64, String>) -> Value {
    let mut kinds = BTreeMap::<&str, usize>::new();
    for kind in observed.values() {
        *kinds.entry(kind).or_default() += 1;
    }
    json!({"programs":observed.keys().collect::<Vec<_>>(),"kinds":kinds,
        "persistence_programs":observed.values().filter(|kind|kind.contains("persistence")).count(),
        "scope":"Exact retained roles, terminal outcomes observed before owner drain; unique program IDs in this window, not callback invocation counts. Absence is not proof a callback was never scheduled"})
}

fn listener_cycles_observed(progress: &Value, observed: &std::collections::BTreeSet<u64>) -> bool {
    only_listener_drivers_pending(progress)
        && progress["callbacks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| {
                row["owner"] == "plan"
                    && row["outcome"] == "pending"
                    && row["callback_kind"] == "listener driver"
            })
            .all(|row| {
                row["listener_callback_program"]
                    .as_u64()
                    .is_some_and(|program| observed.contains(&program))
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::{tree::MixTree, v2::Part};
    use sampler_core::{Limits, Prepared, Runtime};

    #[test]
    fn residency_settlement_waits_for_real_commit_then_requires_a_fresh_note() {
        use sampler_core::{Envelope, Pcm, Playback, Region, ScriptInstanceId, Stage};
        use std::sync::Arc;
        struct Release(Arc<AtomicBool>);
        impl Drop for Release {
            fn drop(&mut self) { self.0.store(true, Ordering::Release); }
        }
        let pcm = Pcm::new(48000, vec![[0.25;2];1024].into()).unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let (worker_entered, worker_release) = (entered.clone(), release.clone());
        pcm.set_residency_loader(move || {
            worker_entered.store(true, Ordering::Release);
            while !worker_release.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(vec![(0,vec![[0.25;2];1024].into())])
        }).unwrap();
        let prepared = Prepared::new(48000,vec![pcm],vec![Region {
            sample:0,key_low:60,key_high:60,root_key:None,velocity_low:0.,velocity_high:1.,
            gain:1.,envelope:Envelope::default(),playback:Playback::default(),
        }],2).unwrap().with_groups(1,vec![Some(0)]).unwrap()
            .with_script_instances(vec![vec![]]).unwrap().with_stages(vec![Stage::default()]).unwrap();
        let limits = Limits::for_plan(&prepared,8,8);
        let mut runtime = Runtime::new(prepared,limits).unwrap();
        let plan = runtime.active_plan();
        runtime.request_group_residency(plan,0,false,ScriptInstanceId(0)).unwrap();
        let start = Instant::now();
        while runtime.residency_progress(plan).unwrap().pending != 0 {
            assert!(start.elapsed() < Duration::from_secs(2));
            runtime.service_residency().unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut core = V2Core::with_parts(1,48000.);
        // Unblock before Core/Runtime joins the worker on any assertion failure.
        let unblock = Release(release);
        runtime.request_group_residency(plan,0,true,ScriptInstanceId(0)).unwrap();
        let mut part = Part::new(runtime,MixTree::instrument("residency witness")).unwrap();
        part.epoch = 7;
        core.install(0,Some(Box::new(part)));
        let mut owner = SwitchOwner {request:7,..Default::default()};
        let start = Instant::now();
        while !entered.load(Ordering::Acquire) {
            assert!(start.elapsed() < Duration::from_secs(2));
            std::thread::sleep(Duration::from_millis(1));
        }
        core.play(0,Event::midi1(0x90,60,100));
        assert_eq!(switch_render(&mut core,&mut owner,128).unwrap()["peak"],0.);
        let blocked = switch_settle_residency(&mut core,&mut owner,1).unwrap();
        assert_eq!(blocked["status"],"wall_limit");
        assert_eq!(blocked["after"]["pending"],1);
        assert_eq!(blocked["after"]["groups_loaded"],0);
        unblock.0.store(true,Ordering::Release);
        let settled = switch_settle_residency(&mut core,&mut owner,1000).unwrap();
        assert_eq!(settled["status"],"settled");
        assert_eq!(settled["after"]["pending"],0);
        assert_eq!(settled["after"]["groups_loaded"],1);
        assert_eq!(switch_render(&mut core,&mut owner,128).unwrap()["peak"],0.,
            "a previously rejected attack is not replayed by residency completion");
        core.play(0,Event::midi1(0x90,60,100));
        assert!(switch_render(&mut core,&mut owner,128).unwrap()["peak"].as_f64().unwrap()>0.);
    }

    fn artifact_shot(page: u32, presentation: &str, color: [u8; 4]) -> (Value, Vec<u8>) {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&color).unwrap();
            writer.finish().unwrap();
        }
        let encoded: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        (
            json!({"page":page,"presentation":presentation,"width":1,"height":1,"scale":1.,
                "status":"ok","pending_assets":0,"png_hex":encoded}),
            bytes,
        )
    }

    #[test]
    fn ui_artifacts_keep_lua_and_performance_sources_separate_from_ksp_slots() {
        use sampler_ui_ir::Source;
        let directory = tempfile::tempdir().unwrap();
        let faces = vec![
            Interface { source: Source::Ksp { slot: 0 }, ..Default::default() },
            Interface { source: Source::FalconLua, ..Default::default() },
            Interface { source: Source::PerformanceView, ..Default::default() },
        ];
        let mut lua = UiArtifacts::new_source(&directory.path().join("lua"), Source::FalconLua, 0).unwrap();
        assert_eq!(lua.target(&faces), Ok(1));
        assert_eq!(lua.outcome(0, 1, "observed")["source"], "uvi");
        assert!(lua.outcome(0, 1, "observed")["slot"].is_null());
        assert_eq!(lua.target(&faces[..1]), Err("slot_not_present"));
        let mut ambiguous = faces.clone();
        ambiguous.push(faces[1].clone());
        assert_eq!(lua.target(&ambiguous), Err("ambiguous_slot"));
        let (shot, bytes) = artifact_shot(0, "Bitmap", [255, 0, 0, 255]);
        let capture = lua.capture(0, 1, &json!({"shots":[shot]}));
        assert_eq!(capture["status"], "written");
        assert_eq!(std::fs::read(directory.path().join("lua/step-000-generation-000001-uvi-page-000-bitmap.png")).unwrap(), bytes);
        let performance = UiArtifacts::new_source(&directory.path().join("performance"), Source::PerformanceView, 0).unwrap();
        assert_eq!(performance.target(&faces), Ok(2));
        assert!(UiArtifacts::new_source(&directory.path().join("generated"), Source::Generated, 0).is_err());
        assert!(!directory.path().join("generated").exists());
    }

    #[test]
    fn ui_artifacts_select_exact_script_slot_and_bitmap_page() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("new-captures");
        let mut sink = UiArtifacts::new(&output, 2, 1).unwrap();
        let mut faces = vec![
            Interface::default(),
            Interface {
                source: sampler_ui_ir::Source::Ksp { slot: 0 },
                ..Default::default()
            },
            Interface {
                source: sampler_ui_ir::Source::Ksp { slot: 2 },
                ..Default::default()
            },
        ];
        assert_eq!(sink.target(&faces), Ok(2));
        faces.pop();
        assert_eq!(sink.target(&faces), Err("slot_not_present"));
        faces.push(Interface {
            source: sampler_ui_ir::Source::Ksp { slot: 2 },
            ..Default::default()
        });
        faces.push(faces[2].clone());
        assert_eq!(sink.target(&faces), Err("ambiguous_slot"));
        assert_eq!(std::fs::read_dir(&output).unwrap().count(), 0);

        let (other_page, _) = artifact_shot(0, "Bitmap", [255, 0, 0, 255]);
        let (other_mode, _) = artifact_shot(1, "Vector", [0, 255, 0, 255]);
        let (mut wanted, bytes) = artifact_shot(1, "Bitmap", [0, 0, 255, 255]);
        // Partial renders may still provide useful pixels, with honest status.
        wanted["status"] = json!("partial");
        wanted["pending_assets"] = json!(2);
        let report = json!({"shots":[other_page,other_mode,wanted]});
        let result = sink.capture(3, 7, &report);
        assert_eq!(result["status"], "written");
        assert_eq!(result["render_status"], "partial");
        assert_eq!(result["pending_assets"], 2);
        assert_eq!(result["png_sha256"], sha256(&bytes));
        assert!(result.get("png_hex").is_none());
        assert_eq!(
            std::fs::read(output.join("step-003-generation-000007-slot-02-page-001-bitmap.png"))
                .unwrap(),
            bytes
        );
        assert_eq!(sink.summary()["written_images"], 1);
        assert_eq!(sink.summary()["reserved_bytes"], bytes.len());
        assert_eq!(
            sink.capture(4, 7, &json!({"shots":[]}))["status"],
            "shot_not_rendered"
        );
    }

    #[test]
    fn ui_artifacts_never_overwrite_directory_or_image() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("new-captures");
        let mut sink = UiArtifacts::new(&output, 0, 0).unwrap();
        assert!(UiArtifacts::new(&output, 0, 0).is_err());
        let path = output.join("step-000-generation-000001-slot-00-page-000-bitmap.png");
        std::fs::write(&path, b"earlier evidence").unwrap();
        let (shot, bytes) = artifact_shot(0, "Bitmap", [1, 2, 3, 255]);
        let result = sink.capture(0, 1, &json!({"shots":[shot]}));
        assert_eq!(result["status"], "already_exists");
        assert_eq!(std::fs::read(path).unwrap(), b"earlier evidence");
        assert_eq!(sink.attempted_images, 1);
        assert_eq!(sink.reserved_bytes, bytes.len());
        assert_eq!(sink.written_images, 0);
    }

    #[test]
    fn ui_artifacts_bound_images_bytes_and_reject_malformed_pixels() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("new-captures");
        let mut sink = UiArtifacts::new(&output, 0, 0).unwrap();
        let (shot, _) = artifact_shot(0, "Bitmap", [1, 2, 3, 255]);
        let report = json!({"shots":[shot]});
        sink.attempted_images = UI_ARTIFACT_IMAGES;
        assert_eq!(sink.capture(0, 1, &report)["status"], "image_limit");
        sink.attempted_images = 0;
        sink.reserved_bytes = UI_ARTIFACT_BYTES - 1;
        assert_eq!(sink.capture(0, 1, &report)["status"], "byte_limit");
        sink.reserved_bytes = 0;
        for (encoded, expected) in [
            ("0", "malformed_png_hex"),
            ("gg", "malformed_png_hex"),
            ("0000", "invalid_png_signature"),
        ] {
            let mut malformed = report.clone();
            malformed["shots"][0]["png_hex"] = json!(encoded);
            assert_eq!(sink.capture(0, 1, &malformed)["status"], expected);
        }
        assert_eq!(sink.attempted_images, 0);
        assert_eq!(sink.reserved_bytes, 0);
        assert_eq!(std::fs::read_dir(output).unwrap().count(), 0);
    }

    #[test]
    fn ui_artifacts_require_explicit_render_before_creating_any_directory() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("must-not-exist");
        assert!(
            Witness::preset_switch_probe_with_ui_artifacts(
                &json!({"schema":1,"steps":[{"op":"load","path":"absent.nki"}]}),
                &output,
                0,
                0,
            )
            .is_err()
        );
        assert!(!output.exists());
        assert!(UiArtifacts::new(Path::new("relative-not-created"), 0, 0).is_err());
    }

    fn probe_wave(directory: &Path, name: &str, amplitude: f32) -> PathBuf {
        let path = directory.join(name);
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for _ in 0..8192 {
            writer.write_sample(amplitude).unwrap();
        }
        writer.finalize().unwrap();
        path
    }

    #[test]
    fn switch_probe_repeated_a_b_a_separates_held_cut_from_fresh_signal() {
        let directory = tempfile::tempdir().unwrap();
        let a = probe_wave(directory.path(), "a.wav", 0.125);
        let b = probe_wave(directory.path(), "b.wav", 0.375);
        let report = switch_probe(&json!({"schema":1,"note_frames":512,"steps":[
            {"op":"load","path":a},{"op":"load","path":b},{"op":"load","path":a}
        ]}))
        .unwrap();
        let steps = report["steps"].as_array().unwrap();
        assert_eq!(steps[0]["fresh_note"]["audio_generation"], 1);
        assert_eq!(steps[1]["fresh_note"]["audio_generation"], 2);
        assert_eq!(steps[2]["fresh_note"]["audio_generation"], 3);
        assert!(
            steps
                .iter()
                .all(|s| s["fresh_note"]["nonzero_frames"].as_u64().unwrap() > 0)
        );
        assert!(
            steps[1]["fresh_note"]["peak"].as_f64().unwrap()
                > steps[0]["fresh_note"]["peak"].as_f64().unwrap()
        );
        assert_eq!(steps[1]["held_transition"]["held_before"], true);
        assert_eq!(steps[1]["held_transition"]["after_publish"]["peak"], 0.);
        assert_eq!(steps[2]["repeated_preset"]["observed_before"], true);
        assert_eq!(steps[2]["repeated_preset"]["complete_ui_equal"], true);
        assert_eq!(
            steps[2]["fresh_note"]["pcm_sha256"],
            steps[0]["fresh_note"]["pcm_sha256"]
        );
        assert_eq!(report["universal_compatibility"], "not_established");
        assert!(report.get("ui_artifacts").is_none());
    }

    #[test]
    fn switch_probe_failed_canceled_and_reordered_preparations_preserve_actual_owner() {
        let directory = tempfile::tempdir().unwrap();
        let a = probe_wave(directory.path(), "a.wav", 0.25);
        let b = probe_wave(directory.path(), "b.wav", 0.5);
        let report = switch_probe(&json!({"schema":1,"note_frames":512,"steps":[
            {"op":"load","path":a},
            {"op":"load","path":directory.path().join("absent.nki")},
            {"op":"load","path":b,"cancel_at_progress":0},
            {"op":"prepare","path":b},
            {"op":"load","path":a},
            {"op":"publish","request":4}
        ]}))
        .unwrap();
        let steps = report["steps"].as_array().unwrap();
        assert_eq!(steps[1]["outcome"], "failed");
        assert_eq!(steps[2]["outcome"], "canceled");
        assert_eq!(steps[3]["outcome"], "prepared_only");
        assert_eq!(
            steps[5]["outcome"],
            "stale_completion_discarded_by_probe_gate"
        );
        assert_eq!(steps[5]["prepared_completion_present"], true);
        for index in [1, 2, 3, 5] {
            assert_eq!(steps[index]["old_owner_preserved"], true);
            assert!(
                steps[index]["retained_owner_audio"]["nonzero_frames"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
            assert_eq!(
                steps[index]["before"]["resolved_owner"],
                steps[index]["after"]["resolved_owner"]
            );
        }
        assert_eq!(steps[5]["after"]["audio_generation"], 5);
        assert!(
            report["publication_gate_scope"]
                .as_str()
                .unwrap()
                .contains("diagnostic-host")
        );
    }

    #[test]
    fn switch_probe_rejects_unknown_and_unbounded_plan_fields_before_loading() {
        assert!(
            switch_probe(
                &json!({"schema":1,"steps":[{"op":"load","path":"a.wav","library_hack":true}]})
            )
            .is_err()
        );
        assert!(
            switch_probe(
                &json!({"schema":1,"note_frames":96001,"steps":[{"op":"load","path":"a.wav"}]})
            )
            .is_err()
        );
        assert!(
            switch_probe(
                &json!({"schema":1,"steps":[{"op":"load","path":"a.nki","base":"base.nki"}]})
            )
            .is_err()
        );
        assert_eq!(
            switch_control_id("0xffffffffffffffffffffffffffffffff"),
            Some(ControlId(u128::MAX))
        );
        assert!(switch_control_id("ffffffffffffffffffffffffffffffff").is_none());
        assert!(switch_control_id("0x0007").is_none());
    }

    #[test]
    fn typed_widget_action_runs_real_waiting_tab_callback_and_hides_authored_panel() {
        let script=sampler_ksp::compile(
            "on init make_perfview declare ui_button $tab declare ui_label $drawer(1,1) end on on ui_control($tab) wait(1000) set_control_par(get_ui_id($drawer),$CONTROL_PAR_HIDE,$HIDE_WHOLE_CONTROL) end on",
            48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
        let (part, views) = prepared_fixture(vec![script]);
        let mut part = part.unwrap();
        part.epoch = 1;
        let mut ingress = part.ui_controls.take().unwrap();
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, Some(part));
        let mut owner = SwitchOwner {
            request: 1,
            scripts: ScriptUi {
                views,
                ..Default::default()
            },
            ..Default::default()
        };
        let (faces, _, before) = switch_state(&core, &mut owner);
        let source_id = faces[0]
            .widgets
            .iter()
            .find(|w| w.name == "$tab")
            .unwrap()
            .source_id
            .unwrap();
        assert!(switch_widget_target(&faces, 1, source_id).is_empty());
        let targets = switch_widget_target(&faces, 0, source_id);
        assert_eq!(targets.len(), 1);
        assert!(ingress.submit_ui_widgets(
            0,
            targets[0].widget,
            vec![(0, SwitchValue::Integer(1).ui())],
            Default::default()
        ));
        switch_render(&mut core, &mut owner, 512).unwrap();
        let settle = step(&mut core, &mut owner.scripts.views).unwrap();
        assert_eq!(
            settle["activation_readiness"],
            "finite_callbacks_not_pending"
        );
        let (faces, _, after) = switch_state(&core, &mut owner);
        assert!(
            faces[0]
                .widgets
                .iter()
                .find(|w| w.name == "$drawer")
                .unwrap()
                .hidden
        );
        assert_ne!(before["ui_state_sha256"], after["ui_state_sha256"]);
        assert!(before["visible_widgets"].as_u64() > after["visible_widgets"].as_u64());
    }

    #[test]
    fn guarded_widget_action_blocks_hidden_parent_and_disabled_but_runs_visible_callback() {
        let source = r#"on init
            make_perfview
            declare $calls := 0
            declare ui_panel $panel
            declare ui_button $child
            declare ui_button $open
            declare ui_label $receipt(1,1)
            set_control_par(get_ui_id($child),$CONTROL_PAR_PARENT_PANEL,get_ui_id($panel))
            set_control_par(get_ui_id($panel),$CONTROL_PAR_HIDE,$HIDE_WHOLE_CONTROL)
            end on
            on ui_control($open)
                wait(1000)
                if ($open = 1)
                    set_control_par(get_ui_id($panel),$CONTROL_PAR_HIDE,$HIDE_NOTHING)
                else
                    set_control_par(get_ui_id($panel),$CONTROL_PAR_HIDE,$HIDE_WHOLE_CONTROL)
                end if
            end on
            on ui_control($child)
                inc($calls)
                move_control_px($receipt,$calls,10)
            end on"#;
        let script =
            sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let (part, views) = prepared_fixture(vec![script]);
        let mut part = part.unwrap();
        part.epoch = 1;
        let mut ingress = part.ui_controls.take().unwrap();
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, Some(part));
        let mut owner = SwitchOwner {
            request: 1,
            scripts: ScriptUi {
                views,
                ..Default::default()
            },
            ..Default::default()
        };
        let (faces, _, _) = switch_state(&core, &mut owner);
        let id = |name: &str| {
            faces[0]
                .widgets
                .iter()
                .find(|w| w.name == name)
                .unwrap()
                .source_id
                .unwrap()
        };
        let (child, open) = (id("$child"), id("$open"));
        let targets = switch_widget_target(&faces, 0, child);
        assert!(!targets[0].widget.hidden);
        assert!(
            !targets[0].effective_visible,
            "the authored parent is hidden"
        );
        assert_eq!(
            switch_submit_widget(
                Some(&mut ingress),
                &targets,
                0,
                0,
                SwitchValue::Integer(1),
                true
            ),
            (Some(false), Some("target_not_effectively_visible"))
        );
        assert_eq!(switch_widget_blocked_reason(&targets, false), None);
        switch_render(&mut core, &mut owner, 512).unwrap();
        let (faces, _, _) = switch_state(&core, &mut owner);
        let receipt_x = |faces: &[Interface]| {
            faces[0]
                .widgets
                .iter()
                .find(|w| w.name == "$receipt")
                .unwrap()
                .rect
                .x
        };
        assert_eq!(receipt_x(&faces), 0.);

        let targets = switch_widget_target(&faces, 0, open);
        assert_eq!(
            switch_submit_widget(
                Some(&mut ingress),
                &targets,
                0,
                0,
                SwitchValue::Integer(1),
                true
            ),
            (Some(true), None)
        );
        switch_render(&mut core, &mut owner, 512).unwrap();
        step(&mut core, &mut owner.scripts.views).unwrap();
        let (faces, _, _) = switch_state(&core, &mut owner);
        assert!(switch_widget_target(&faces, 0, child)[0].effective_visible);

        // Exercise the frontend's explicit disabled flag on the same authored
        // control identity; no unsupported KSP enabled setter is fabricated.
        let mut disabled = faces.clone();
        disabled[0]
            .widgets
            .iter_mut()
            .find(|w| w.source_id == Some(child))
            .unwrap()
            .enabled = false;
        let targets = switch_widget_target(&disabled, 0, child);
        assert_eq!(
            switch_submit_widget(
                Some(&mut ingress),
                &targets,
                0,
                0,
                SwitchValue::Integer(1),
                true
            ),
            (Some(false), Some("target_disabled"))
        );
        switch_render(&mut core, &mut owner, 512).unwrap();
        assert_eq!(receipt_x(&switch_state(&core, &mut owner).0), 0.);

        let targets = switch_widget_target(&faces, 0, child);
        assert_eq!(
            switch_submit_widget(
                Some(&mut ingress),
                &targets,
                0,
                0,
                SwitchValue::Integer(1),
                true
            ),
            (Some(true), None)
        );
        switch_render(&mut core, &mut owner, 512).unwrap();
        assert_eq!(receipt_x(&switch_state(&core, &mut owner).0), 1.);

        // An explicit direct audit retains its old ability to invoke a hidden
        // target when the new precondition is absent/false.
        let targets = switch_widget_target(&faces, 0, open);
        assert_eq!(
            switch_submit_widget(
                Some(&mut ingress),
                &targets,
                0,
                0,
                SwitchValue::Integer(0),
                true
            ),
            (Some(true), None)
        );
        switch_render(&mut core, &mut owner, 512).unwrap();
        step(&mut core, &mut owner.scripts.views).unwrap();
        let (faces, _, _) = switch_state(&core, &mut owner);
        let targets = switch_widget_target(&faces, 0, child);
        assert!(!targets[0].effective_visible);
        assert_eq!(
            switch_submit_widget(
                Some(&mut ingress),
                &targets,
                0,
                0,
                SwitchValue::Integer(0),
                false
            ),
            (Some(true), None)
        );
        switch_render(&mut core, &mut owner, 512).unwrap();
        assert_eq!(receipt_x(&switch_state(&core, &mut owner).0), 2.);
    }

    #[test]
    fn same_shape_replacement_cannot_route_old_ui_ingress_to_new_menu() {
        let compiled = |default: i32| {
            sampler_ksp::compile(&format!(
            "on init make_perfview declare ui_menu $choice add_menu_item($choice,\"first\",30) add_menu_item($choice,\"last\",100) $choice := {default} end on on ui_control($choice) end on"
        ),48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap()
        };
        let (first, first_views) = prepared_fixture(vec![compiled(30)]);
        let id = match first_views[0].ui(&|_| None).unwrap().widgets[0].binding {
            Binding::Control(id) => id,
            _ => panic!("fixture has scalar menu binding"),
        };
        let mut first = first.unwrap();
        first.epoch = 1;
        let mut stale = first.ui_controls.take().unwrap();
        assert!(stale.submit(id, 100.)); // queued before replacement, not drained
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, Some(first));
        let (second, second_views) = prepared_fixture(vec![compiled(30)]);
        let mut second = second.unwrap();
        second.epoch = 2;
        let mut current = second.ui_controls.take().unwrap();
        let retired = core.install(0, Some(second));
        drop(retired);
        let mut owner = SwitchOwner {
            request: 2,
            controls: vec![id],
            scripts: ScriptUi {
                views: second_views,
                ..Default::default()
            },
            ..Default::default()
        };
        let _ = stale.submit(id, 100.); // admitted or disconnected, never retargeted
        switch_render(&mut core, &mut owner, 512).unwrap();
        assert_eq!(core.control_value(0, id), Some(30.));
        assert!(current.submit(id, 100.));
        switch_render(&mut core, &mut owner, 512).unwrap();
        assert_eq!(core.control_value(0, id), Some(100.));
        let (_, _, state) = switch_state(&core, &mut owner);
        assert_eq!(state["menus"][0]["selected_value"], 100.);
        assert_eq!(state["menus"][0]["items"][0]["value"], 30);
        assert_eq!(state["menus"][0]["items"][1]["value"], 100);
        assert!(!state.to_string().contains("first"));
    }

    #[test]
    fn different_structure_replacement_keeps_real_menu_state_distinct() {
        let first=sampler_ksp::compile("on init make_perfview declare ui_menu $choice add_menu_item($choice,\"first\",30) $choice := 30 end on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
        let (mut core, views) = install(vec![first]);
        let mut owner = SwitchOwner {
            scripts: ScriptUi {
                views,
                ..Default::default()
            },
            ..Default::default()
        };
        let (_, _, before) = switch_state(&core, &mut owner);
        let second = sampler_ksp::compile(
            "on init make_perfview declare ui_label $drawer(1,1) end on",
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[],
        )
        .unwrap();
        let (part, views) = prepared_fixture(vec![second]);
        drop(core.install(0, part));
        owner.scripts.views = views;
        let (_, _, after) = switch_state(&core, &mut owner);
        assert_ne!(
            before["control_identity_sha256"],
            after["control_identity_sha256"]
        );
        assert_eq!(before["menus"].as_array().unwrap().len(), 1);
        assert!(after["menus"].as_array().unwrap().is_empty());
    }

    fn fixture(body: &str) -> (V2Core, Vec<sampler_ksp::ScriptView>) {
        let source = format!(
            "on init make_perfview declare ui_label $drawer(1,1) move_control_px($drawer,10,20) end on on persistence_changed {body} end on"
        );
        let script =
            sampler_ksp::compile(&source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        install(vec![script])
    }

    fn prepared_fixture(
        scripts: Vec<sampler_ksp::Script>,
    ) -> (Option<Box<Part>>, Vec<sampler_ksp::ScriptView>) {
        let views = scripts.iter().map(sampler_ksp::Script::view).collect();
        let plan =
            sampler_ksp::bind_modules(scripts, Prepared::new(48000, vec![], vec![], 0).unwrap())
                .unwrap();
        let limits = Limits::for_plan(&plan, 4, 8);
        let part = Part::new(
            Runtime::new(plan, limits).unwrap(),
            MixTree::instrument("authored UI lifecycle"),
        )
        .unwrap();
        (Some(Box::new(part)), views)
    }

    fn install(scripts: Vec<sampler_ksp::Script>) -> (V2Core, Vec<sampler_ksp::ScriptView>) {
        let (part, views) = prepared_fixture(scripts);
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, part);
        (core, views)
    }

    #[test]
    fn completed_persistence_applies_real_outbox_geometry_and_hide_writes() {
        let (mut core, mut views) = fixture(
            "wait(1000) move_control_px($drawer,10,-100) set_control_par(get_ui_id($drawer),$CONTROL_PAR_HIDE,$HIDE_WHOLE_CONTROL)",
        );
        assert_eq!(views[0].ui(&|_| None).unwrap().widgets[0].rect.y, 20.);
        let report = step(&mut core, &mut views).unwrap();
        let face = views[0].ui(&|_| None).unwrap();
        assert_eq!(report["status"], "completed");
        assert!(report["effects_applied"].as_u64().unwrap() >= 2);
        assert!(report["frames"].as_u64().unwrap() > 0);
        assert_eq!(face.widgets[0].rect.y, -100.);
        assert!(face.widgets[0].hidden);
        assert_eq!(report["callback_progress"]["queued_faults"], 0);
        assert_eq!(report["max_active_voices"], 0);
        assert_eq!(
            report["completed_callback_programs_observed"]["persistence_programs"],
            1
        );
    }

    #[test]
    fn pending_persistence_stops_at_the_audio_frame_bound_without_hiding_it() {
        let (mut core, mut views) = fixture("wait(5000000) move_control_px($drawer,10,-100)");
        let report = step(&mut core, &mut views).unwrap();
        assert_eq!(report["status"], "partial");
        assert_eq!(report["frames"], 96_000);
        assert_eq!(report["pending_plan_callbacks"], true);
        assert_eq!(views[0].ui(&|_| None).unwrap().widgets[0].rect.y, 20.);
    }

    #[test]
    fn fault_after_end_block_remains_observable_after_the_live_callback_disappears() {
        // A negative dynamic wait is rejected by the real scheduler after the
        // first wait resumes; KSP division and array bounds are nonfaulting.
        let (mut core, mut views) = fixture("wait(1000) wait(-1) move_control_px($drawer,10,-100)");
        let report = step(&mut core, &mut views).unwrap();
        assert_eq!(report["status"], "partial");
        assert!(
            report["callback_progress"]["queued_faults"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(report["callback_progress"]["pending_plan_callbacks"], 0);
        assert_eq!(
            core.ui_lifecycle_progress(0)["queued_faults"],
            report["callback_progress"]["queued_faults"],
            "observation must not consume faults"
        );
        assert_eq!(views[0].ui(&|_| None).unwrap().widgets[0].rect.y, 20.);
    }

    #[test]
    fn retained_callback_roles_follow_multiple_script_offsets_and_listener_drivers() {
        let compile = |source: &str, slot| {
            sampler_ksp::compile_with(
                source,
                48000,
                sampler_ksp::Limits::LIBRARY,
                &[],
                &sampler_ksp::Environment {
                    slot,
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let first = compile("on init end on on persistence_changed wait(1000) end on", 2);
        let second = compile(
            "on init make_perfview declare ui_label $late(1,1) move_control_px($late,10,20) set_listener($NI_SIGNAL_TIMER_MS,10000) end on on listener set_control_par(get_ui_id($late),$CONTROL_PAR_HIDE,$HIDE_WHOLE_CONTROL) end on",
            5,
        );
        let views = vec![first.view(), second.view()];
        let driver = (0..views[1].program_count())
            .find(|&local| views[1].program_role(local) == Some("listener driver"))
            .unwrap();
        assert_eq!(
            views[1].callback(driver),
            None,
            "driver is not an authored entry"
        );
        assert_eq!(
            views[1].callback(views[1].listener_callback(driver).unwrap()),
            Some("on listener")
        );
        let persistence = first
            .entries()
            .iter()
            .find(|e| e.kind == sampler_ksp::EntryKind::PersistenceChanged)
            .unwrap()
            .program;
        let mut progress = json!({"callbacks":[
            {"program":persistence,"outcome":"pending","owner":"plan"},
            {"program":views[0].program_count()+driver,"outcome":"pending","owner":"plan"},
            {"program":usize::MAX,"outcome":"pending","owner":"plan"}],"rows_omitted":3,"pending_plan_callbacks":3});
        classify_callbacks(&mut progress, &views);
        assert_eq!(progress["callbacks"][0]["script_slot"], 2);
        assert_eq!(progress["callbacks"][1]["script_slot"], 5);
        assert_eq!(progress["callbacks"][1]["local_program"], driver);
        assert_eq!(
            progress["observed_pending_callback_kinds"],
            json!({"on persistence_changed":1,"listener driver":1})
        );
        assert_eq!(progress["unclassified_callback_rows"], 1);
        assert!(!only_listener_drivers_pending(&progress));
        assert_eq!(
            progress["rows_omitted"], 3,
            "a bounded trace cannot classify omitted work"
        );

        let (mut core, mut views) = install(vec![first, second]);
        let report = step(&mut core, &mut views).unwrap();
        assert_eq!(
            report["callback_progress"]["observed_pending_callback_kinds"],
            json!({"listener driver":1})
        );
        assert_eq!(
            report["callback_progress"]["callbacks"][0]["script_slot"],
            5
        );
        assert_eq!(report["callback_progress"]["queued_faults"], 0);
        assert_eq!(report["callback_progress"]["unclassified_callback_rows"], 0);
        assert_eq!(
            report["status"], "background_active",
            "ongoing listener work stays explicit"
        );
        assert_eq!(
            report["activation_readiness"],
            "finite_callbacks_not_pending"
        );
        assert!(report["frames"].as_u64().unwrap() > 0);
        assert!(
            report["frames"].as_u64().unwrap() >= 480,
            "the necessary timer-delayed UI write must run"
        );
        assert!(views[1].ui(&|_| None).unwrap().widgets[0].hidden);
        assert_eq!(report["background_observation"], "first_callbacks_observed");
        let mut waiting_startup = report["callback_progress"].clone();
        assert!(only_listener_drivers_pending(&waiting_startup));
        waiting_startup["pending_startup_work"] = json!(true);
        assert!(!only_listener_drivers_pending(&waiting_startup),
            "an empty finite callback list must not hide unadmitted startup work");
        assert!(
            report["frames"].as_u64().unwrap() < 96_000,
            "finite persistence work must not wait for an eternal periodic driver"
        );
    }

    #[test]
    fn ongoing_listener_does_not_hide_a_finite_callback_fault() {
        let sources = [
            "on init end on on persistence_changed wait(1000) wait(-1) end on",
            "on init set_listener($NI_SIGNAL_TIMER_MS,10000) end on on listener end on",
        ];
        let scripts = sources
            .iter()
            .enumerate()
            .map(|(slot, source)| {
                sampler_ksp::compile_with(
                    source,
                    48000,
                    sampler_ksp::Limits::LIBRARY,
                    &[],
                    &sampler_ksp::Environment {
                        slot: slot as u8,
                        ..Default::default()
                    },
                )
                .unwrap()
            })
            .collect();
        let (mut core, mut views) = install(scripts);
        let report = step(&mut core, &mut views).unwrap();
        assert_eq!(
            report["callback_progress"]["observed_pending_listener_drivers"],
            1
        );
        assert_eq!(
            report["callback_progress"]["pending_non_background_plan_callbacks"],
            0
        );
        assert_eq!(report["callback_progress"]["queued_faults"], 1);
        assert_eq!(report["status"], "partial");
        assert_eq!(report["activation_readiness"], "pending_or_failed");
    }

    #[test]
    fn faulted_first_listener_is_not_reported_as_an_observed_completed_cycle() {
        let script = sampler_ksp::compile(
            "on init set_listener($NI_SIGNAL_TIMER_MS,10000) end on on listener wait(-1) end on",
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[],
        )
        .unwrap();
        let (mut core, mut views) = install(vec![script]);
        let report = step(&mut core, &mut views).unwrap();
        assert_eq!(report["status"], "partial");
        assert_eq!(report["callback_progress"]["queued_faults"], 1);
        assert_eq!(report["completed_listener_callbacks_observed"], json!([]));
        assert_eq!(report["background_observation"], "pending_first_callbacks");
        assert!(report["frames"].as_u64().unwrap() < 96_000);
    }
}
