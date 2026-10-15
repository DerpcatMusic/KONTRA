//! Explicit, wall-bounded observations of the ordinary independent UVI owner.
//! An acknowledgement proves earlier event/command service, not future work.
use super::*;
use crate::sound::v2::uvi_diagnostics::UviDiagnosticObservation;
use sampler_uvi::scripted::{
    DiagnosticAcknowledgement, DiagnosticPoll, DiagnosticProgress, UiBridge,
};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    phase_timeout_ms: u16,
    total_timeout_ms: u16,
    pace_ms: u8,
    #[serde(default)]
    controllers: Vec<Controller>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Controller {
    /// MIDI channel numbering is zero based.
    channel: u8,
    cc: u8,
    value: u8,
}

impl Config {
    pub(super) fn validate(&self, note_frames: usize) -> Result<(), &'static str> {
        if !(1..=10000).contains(&self.phase_timeout_ms)
            || !(1..=10000).contains(&self.total_timeout_ms)
            || self.phase_timeout_ms > self.total_timeout_ms
            || !(1..=20).contains(&self.pace_ms)
            || !(128..=96000).contains(&note_frames)
            || self.controllers.len() > 32
            || self
                .controllers
                .iter()
                .any(|c| c.channel > 15 || c.cc > 127 || c.value > 127)
        {
            return Err(
                "UVI probe requires phase/total wall limits 1..10000 ms with phase <= total, pace 1..20 ms, 128..96000 note frames and at most 32 valid explicit MIDI controllers",
            );
        }
        Ok(())
    }
}

struct Bounds {
    started: Instant,
    deadline: Instant,
    phase: Duration,
}
impl Bounds {
    fn new(config: &Config) -> Self {
        let started = Instant::now();
        Self {
            started,
            deadline: started + Duration::from_millis(u64::from(config.total_timeout_ms)),
            phase: Duration::from_millis(u64::from(config.phase_timeout_ms)),
        }
    }
    fn phase_deadline(&self) -> Instant {
        (Instant::now() + self.phase).min(self.deadline)
    }
    fn pause(&self, ms: u8, deadline: Instant) {
        let remaining = deadline
            .min(self.deadline)
            .saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            std::thread::sleep(Duration::from_millis(u64::from(ms)).min(remaining));
        }
    }
}

#[derive(Default)]
struct Service {
    zero_frame_blocks: usize,
    effects: usize,
    note_ends: usize,
}

/// Unlike switch_render(0), this really wakes/drains the installed driver and
/// admits core transactions. No frames or script-clock time are added.
fn service_zero(
    core: &mut V2Core,
    owner: &mut SwitchOwner,
    service: &mut Service,
) -> Result<(), std::io::ErrorKind> {
    core.begin_block(&BlockInfo {
        frames: 0,
        offline: true,
        ..Default::default()
    });
    let _ = core.render(0);
    core.end_block(0, &mut |_| {
        service.note_ends += 1;
        true
    });
    service.zero_frame_blocks += 1;
    service.effects += core
        .service_ui_read_only(0, &mut owner.scripts.views)
        .map_err(|e| e.kind())?;
    Ok(())
}

fn progress_value(p: DiagnosticProgress) -> Value {
    json!({"phase":format!("{:?}",p.phase),"events_enqueued":p.events_enqueued,
        "events_rejected":p.events_rejected,"events_executed":p.events_executed,
        "commands_collected":p.commands_collected,"commands_pushed":p.commands_pushed,
        "commands_drained":p.commands_drained,"owner_acknowledged":p.owner_acknowledged,
        "marker_drained":p.marker_drained})
}

fn observation_value(o: &UviDiagnosticObservation) -> Value {
    let mut result =
        json!({"handles_notes":o.handles_notes,"progress":o.progress.map(progress_value)});
    #[cfg(feature = "shots")]
    {
        result["owner_audit"] = json!(o.owner_audit);
    }
    result
}

fn acknowledgement_value(a: DiagnosticAcknowledgement) -> Value {
    let s = a.samples;
    json!({"request_id":a.request_id,"events_executed":a.events_executed,
        "commands_collected":a.commands_collected,"commands_pushed":a.commands_pushed,
        "ui_revision":a.ui_revision,"runtime_faults":a.runtime_faults,
        "runtime_budgets":a.runtime_budgets,"next_due_ms":a.next_due_ms,
        "samples":{"requested":s.requested,"successful":s.successful,"failed":s.failed,
            "cancelled":s.cancelled,"pending":s.pending,"queued_metadata":s.queued_metadata,
            "worker_active":s.worker_active,"ready":s.ready,"submitted":s.submitted,
            "counts_conserved":a.sample_counts_conserved()}})
}

fn sample_condition(a: DiagnosticAcknowledgement) -> &'static str {
    if !a.sample_counts_conserved() {
        "conservation_failure"
    } else if a.samples.failed != 0 {
        "failed_samples"
    } else if a.samples.cancelled != 0 {
        "cancelled_samples"
    } else if a.samples.pending != 0 {
        "pending_samples"
    } else {
        "observed_settled"
    }
}

struct Phase {
    status: &'static str,
    report: Value,
    ack: Option<DiagnosticAcknowledgement>,
    progress: Option<DiagnosticProgress>,
}
impl Phase {
    fn failed(status: &'static str, detail: Value) -> Self {
        Self {
            status,
            report: json!({"status":status,"detail":detail}),
            ack: None,
            progress: None,
        }
    }
    fn drained(&self) -> bool {
        self.status == "acknowledged_and_drained"
    }
}

/// One bounded nonblocking request, followed by ordinary zero-frame core/UI
/// service. Completion and command drain are tested separately on every poll.
fn acknowledge(
    core: &mut V2Core,
    owner: &mut SwitchOwner,
    bridge: &Arc<UiBridge>,
    bounds: &Bounds,
    deadline: Instant,
    service: &mut Service,
) -> Phase {
    let started = Instant::now();
    let timeout = deadline
        .min(bounds.deadline)
        .saturating_duration_since(started);
    if timeout.is_zero() {
        return Phase::failed("wall_limit", Value::Null);
    }
    let drain_baseline = core.uvi_diagnostic_progress(0, owner.request, bridge)
        .ok().and_then(|o| o.progress).map(|p| p.commands_drained);
    let mut token = match core.request_uvi_diagnostics(0, owner.request, bridge, timeout) {
        Ok(token) => token,
        Err(e) => return Phase::failed("request_failed", json!(format!("{e:?}"))),
    };
    let mut ack = None;
    let mut last = None;
    let mut trace = Vec::new();
    let mut trace_omitted = 0usize;
    let mut polls = 0usize;
    let mut observation = Value::Null;
    let mut drained = false;
    let mut first_command_drain = None;
    let mut detail = Value::Null;
    let status = loop {
        if Instant::now() >= deadline.min(bounds.deadline) {
            break if ack.is_some() {
                "command_drain_wall_limit"
            } else {
                "wall_limit"
            };
        }
        // Validate BEFORE touching the core as well as in poll. A replacement
        // cannot be serviced using this retained old owner as a readiness proof.
        if let Err(e) = core.uvi_diagnostic_progress(0, owner.request, bridge) {
            detail = json!(format!("{e:?}"));
            break "owner_unavailable";
        }
        if let Err(e) = service_zero(core, owner, service) {
            detail = json!(format!("{e:?}"));
            break "service_failed";
        }
        let reply = match core.poll_uvi_diagnostics(&mut token) {
            Ok(reply) => reply,
            Err(e) => {
                detail = json!(format!("{e:?}"));
                break "owner_unavailable";
            }
        };
        polls += 1;
        drained = reply.commands_drained;
        observation = observation_value(&reply.observation);
        if let Some(p) = reply.observation.progress {
            if first_command_drain.is_none()
                && drain_baseline.is_some_and(|before| p.commands_drained > before) {
                first_command_drain = Some(json!({"wall_ms":started.elapsed().as_secs_f64()*1000.,
                    "commands_drained":p.commands_drained,"active_voices":core.voices().active,
                    "owner_phase":format!("{:?}",p.phase),
                    "scope":"First observed increase from the request's counter baseline; off-audio poll timing, not an exact command timestamp or native/host latency"}));
            }
            if last.is_none_or(|old: DiagnosticProgress| old.phase != p.phase
                || old.commands_drained != p.commands_drained
                || old.events_executed != p.events_executed) {
                if trace.len() < 16 {
                    trace.push(json!({"wall_ms":started.elapsed().as_secs_f64()*1000.,
                        "active_voices":core.voices().active,"progress":progress_value(p)}));
                } else {
                    trace_omitted += 1;
                }
            }
            last = Some(p);
        }
        match reply.poll {
            DiagnosticPoll::Complete(a) => {
                ack = Some(a);
                if drained {
                    break "acknowledged_and_drained";
                }
            }
            DiagnosticPoll::Pending => {}
            DiagnosticPoll::TimedOut => break "owner_timeout",
            DiagnosticPoll::Cancelled => break "observation_cancelled",
            DiagnosticPoll::OwnerStopped => break "owner_stopped",
        }
        bounds.pause(1, deadline);
    };
    Phase {
        status,
        ack,
        progress: last,
        report: json!({"status":status,"detail":detail,
        "request_id":token.id(),"acknowledgement":ack.map(acknowledgement_value),
        "commands_drained":drained,"observation":observation,"polls":polls,
        "command_drain_baseline":drain_baseline,"first_command_drain_observed":first_command_drain,
        "wall_ms":started.elapsed().as_secs_f64()*1000.,"clock_frames":0,
        "phase_observations":trace,"phase_observations_omitted":trace_omitted}),
    }
}

fn input_counts(
    before: DiagnosticProgress,
    after: Option<DiagnosticProgress>,
    expected: usize,
) -> Value {
    let Some(after) = after else {
        return json!({"expected":expected,"status":"unobserved"});
    };
    let accepted = after.events_enqueued.checked_sub(before.events_enqueued);
    let rejected = after.events_rejected.checked_sub(before.events_rejected);
    let executed = after.events_executed.checked_sub(before.events_executed);
    let status = if rejected.is_some_and(|n| n != 0) {
        "rejected_input"
    } else if accepted.is_some_and(|n| n >= expected as u64)
        && executed.is_some_and(|n| n >= expected as u64)
    {
        "observed"
    } else {
        "input_unverified"
    };
    json!({"expected":expected,"accepted":accepted,"rejected":rejected,"executed":executed,
        "status":status,"before":progress_value(before),"after":progress_value(after),
        "scope":"Counter deltas from enabled monitoring; asynchronous independent snapshots, not an event-ID ledger"})
}

fn sample_settlement(
    core: &mut V2Core,
    owner: &mut SwitchOwner,
    bridge: &Arc<UiBridge>,
    bounds: &Bounds,
    service: &mut Service,
) -> Phase {
    let deadline = bounds.phase_deadline();
    let started = Instant::now();
    let mut observations = Vec::new();
    let mut omitted = 0usize;
    let mut previous = None;
    let mut requests = 0usize;
    let mut phase = acknowledge(core, owner, bridge, bounds, deadline, service);
    loop {
        requests += 1;
        if !phase.drained() {
            break;
        }
        let a = phase.ack.unwrap();
        let s = a.samples;
        let counts = (
            s.requested,
            s.successful,
            s.failed,
            s.cancelled,
            s.pending,
            s.queued_metadata,
            s.worker_active,
            s.ready,
            s.submitted,
        );
        if previous != Some(counts) {
            if observations.len() < 16 {
                observations.push(acknowledgement_value(a));
            } else {
                omitted += 1;
            }
            previous = Some(counts);
        }
        let condition = sample_condition(a);
        if condition != "pending_samples" {
            phase.status = condition;
            break;
        }
        if Instant::now() >= deadline {
            phase.status = "pending_samples_wall_limit";
            break;
        }
        bounds.pause(1, deadline);
        let next = acknowledge(core, owner, bridge, bounds, deadline, service);
        if !next.drained() {
            // Preserve the last coherent sample counts as well as the later
            // failed observation. Losing an ack cannot turn pending into ready.
            phase.report["last_sample_acknowledgement"] = acknowledgement_value(a);
            phase.report["terminal_observation"] = next.report;
            phase.status = next.status;
            break;
        }
        phase = next;
    }
    phase.report["status"] = json!(phase.status);
    phase.report["sample_observations"] = json!(observations);
    phase.report["sample_observations_omitted"] = json!(omitted);
    phase.report["requests"] = json!(requests);
    phase.report["settlement_wall_ms"] = json!(started.elapsed().as_secs_f64() * 1000.);
    phase
}

#[derive(Default)]
struct Audio {
    frames: usize,
    blocks: usize,
    peak: f64,
    squares: f64,
    nonzero_frames: u64,
    nonzero_blocks: u64,
    voices: u64,
    effects: u64,
    note_ends: u64,
    chain: Sha256,
    first_signal: Option<Value>,
    failure: Option<&'static str>,
}
impl Audio {
    fn new() -> Self {
        let mut audio = Self::default();
        audio
            .chain
            .update(b"kontra-uvi-indexed-block-pcm-sha256-v1\0");
        audio
    }
    fn push(&mut self, block: &Value) {
        let frames = block["frames"].as_u64().unwrap_or(0) as usize;
        let rms = block["rms_all_output_channels"].as_f64().unwrap_or(0.);
        self.squares += rms * rms * frames as f64;
        self.peak = self.peak.max(block["peak"].as_f64().unwrap_or(0.));
        self.nonzero_frames += block["nonzero_frames"].as_u64().unwrap_or(0);
        self.nonzero_blocks += block["nonzero_blocks"].as_u64().unwrap_or(0);
        self.voices = self
            .voices
            .max(block["max_active_voices"].as_u64().unwrap_or(0));
        self.effects += block["effects_applied"].as_u64().unwrap_or(0);
        self.note_ends += block["note_ends"].as_u64().unwrap_or(0);
        self.chain.update((self.blocks as u64).to_le_bytes());
        self.chain.update((frames as u64).to_le_bytes());
        // The parent provides a full PCM digest for this one bounded block.
        // This chain covers fixed-width ASCII digests, not the concatenated PCM.
        self.chain
            .update(block["pcm_sha256"].as_str().unwrap_or("").as_bytes());
        self.frames += frames;
        self.blocks += 1;
        if !block["service_failure_kind"].is_null() {
            self.failure = Some("service_failed");
        }
        if block["complete"] != true {
            self.failure = Some("render_incomplete");
        }
    }
}

/// Called only for an explicitly configured UVI note condition. This function
/// owns input admission; the parent must not send a second unconditional note.
pub(super) fn probe_note(
    core: &mut V2Core,
    owner: &mut SwitchOwner,
    config: &Config,
    note: HostNote,
    velocity: u8,
    note_frames: usize,
) -> Result<Value, &'static str> {
    config.validate(note_frames)?;
    if note.key > 127 || note.channel > 15 || !(1..=127).contains(&velocity) {
        return Err("Invalid explicit UVI probe note input");
    }
    let bridge = owner
        .scripts
        .uvi
        .clone()
        .ok_or("Explicit UVI probe requires a UVI owner")?;
    let bounds = Bounds::new(config);
    let mut service = Service::default();
    let mut audio = Audio::new();
    let mut issues = Vec::<String>::new();
    let mut result = json!({"status":"partial","requested_frames":note_frames,"key":note.key,
        "velocity":velocity,"note_channel":note.channel,"audio_generation":owner.request,
        "config":{"phase_timeout_ms":config.phase_timeout_ms,"total_timeout_ms":config.total_timeout_ms,
            "pace_ms":config.pace_ms,"controllers":config.controllers.iter().map(|c|json!({"channel":c.channel,"cc":c.cc,"value":c.value})).collect::<Vec<_>>()},
        "activation_readiness":"uvi_owner_unverified","note_submitted":false,
        "initial_owner_sample_condition":{"status":"not_observed"},
        "note_phase":{"status":"not_submitted"},"samples_before_pcm":{"status":"not_observed"},
        "final_owner_sample_condition":{"status":"not_observed"},
        "controller_phase":{"status":"not_requested","inputs":[],"input_count":0},
        "scope":"Ordinary unseeded UVI owner and ordered core command service; paced discarded PCM for this exact explicit input/window",
        "completion_scope":"Acknowledgements do not prove future waits/listeners, async-data completion, all sample residency, visual/native parity or library compatibility",
        "stop_scope":"Wall thresholds checked between bounded core/UI calls; dropping a token abandons observation, not Lua/IO; an outer process timeout is required"});
    let monitored = core.enable_uvi_diagnostics(0, owner.request, &bridge);
    if let Err(e) = &monitored {
        issues.push(format!("enable_failed:{e:?}"));
    }
    let mut note_proven = false;
    if let Ok(mut before) = monitored {
        let handles_notes = core
            .uvi_diagnostic_progress(0, owner.request, &bridge)
            .map(|o| o.handles_notes)
            .unwrap_or(false);
        result["note_route"] = json!(if handles_notes {
            "script_on_note"
        } else {
            "direct_sampler_no_on_note"
        });
        result["lua_note_callback_expected"] = json!(handles_notes);
        result["monitoring_before_input"] = progress_value(before);
        let initial = acknowledge(
            core,
            owner,
            &bridge,
            &bounds,
            bounds.phase_deadline(),
            &mut service,
        );
        let mut proceed = initial.drained();
        result["initial_owner_sample_condition"] = initial.report;
        if !proceed {
            issues.push(format!("initial:{}", initial.status));
        }
        if let Some(a) = initial.ack {
            result["initial_sample_condition"] = json!(sample_condition(a));
            if !a.sample_counts_conserved() || a.runtime_faults != 0 || a.runtime_budgets != 0 {
                issues.push("initial_fault_or_sample_conservation_failure".into());
                proceed = false;
            }
        }
        if let Some(p) = initial.progress {
            before = p;
        }
        if proceed && !config.controllers.is_empty() {
            core.begin_block(&BlockInfo {
                frames: 0,
                offline: true,
                ..Default::default()
            });
            for c in &config.controllers {
                core.play(0, Event::midi1(0xb0 | c.channel, c.cc, c.value));
            }
            let phase = acknowledge(
                core,
                owner,
                &bridge,
                &bounds,
                bounds.phase_deadline(),
                &mut service,
            );
            let counts = input_counts(before, phase.progress, config.controllers.len());
            proceed = phase.drained() && counts["status"] == "observed";
            if !proceed {
                issues.push(format!("controllers:{}:{}", phase.status, counts["status"]));
            }
            if phase.ack.is_some_and(|a| {
                a.runtime_faults != 0 || a.runtime_budgets != 0 || !a.sample_counts_conserved()
            }) {
                issues.push("controller_fault_or_sample_conservation_failure".into());
                proceed = false;
            }
            result["controller_phase"] = json!({"status":if proceed {"observed"}else{"partial"},"inputs":result["config"]["controllers"],
                "input_count":config.controllers.len(),"counts":counts,"owner":phase.report});
            if let Some(p) = phase.progress {
                before = p;
            }
        }
        if proceed && Instant::now() < bounds.deadline {
            core.begin_block(&BlockInfo {
                frames: 0,
                offline: true,
                ..Default::default()
            });
            core.play(
                0,
                Event::NoteOn {
                    note,
                    velocity: f64::from(velocity) / 127.,
                    tune: 0.,
                },
            );
            owner.held = Some(note);
            result["note_submitted"] = json!(true);
            let phase = acknowledge(
                core,
                owner,
                &bridge,
                &bounds,
                bounds.phase_deadline(),
                &mut service,
            );
            // Both routes enqueue an owner event. Without authored onNote,
            // the physical attack is forwarded directly; event execution is
            // still required, and is not evidence of an authored callback.
            let counts = input_counts(before, phase.progress, 1);
            note_proven = phase.drained()
                && counts["status"] == "observed"
                && (handles_notes || core.owns(note));
            if !note_proven {
                issues.push(format!("note:{}:{}", phase.status, counts["status"]));
            }
            if phase.ack.is_some_and(|a| {
                a.runtime_faults != 0 || a.runtime_budgets != 0 || !a.sample_counts_conserved()
            }) {
                issues.push("note_fault_or_sample_conservation_failure".into());
                note_proven = false;
            }
            result["note_phase"] = json!({"counts":counts,"owner":phase.report,
                "physical_host_note_owned":core.owns(note),"lua_note_callback_expected":handles_notes});
        }
        if note_proven {
            result["activation_readiness"] = json!(if handles_notes {
                "earlier_note_owner_and_commands_observed"
            } else {
                "direct_note_admitted_owner_commands_observed"
            });
            let samples = sample_settlement(core, owner, &bridge, &bounds, &mut service);
            if samples.status != "observed_settled" {
                issues.push(format!("samples:{}", samples.status));
            }
            result["samples_before_pcm"] = samples.report;
            // A pending task is explicit partial evidence. Ordinary clock
            // advancement may still expose authored future child activity.
            while audio.frames < note_frames && Instant::now() < bounds.deadline {
                if let Err(e) = core.uvi_diagnostic_progress(0, owner.request, &bridge) {
                    issues.push(format!("pcm_owner_unavailable:{e:?}"));
                    break;
                }
                let n = (note_frames - audio.frames).min(128);
                let block = match switch_render(core, owner, n) {
                    Ok(block) => block,
                    Err(e) => {
                        issues.push(format!("pcm:{e}"));
                        break;
                    }
                };
                audio.push(&block);
                if audio.first_signal.is_none()
                    && block["nonzero_frames"].as_u64().is_some_and(|n| n != 0)
                {
                    audio.first_signal = Some(
                        json!({"block":audio.blocks-1,"frames_through_block":audio.frames,
                        "wall_ms":bounds.started.elapsed().as_secs_f64()*1000.,"peak":block["peak"],
                        "max_active_voices":block["max_active_voices"],"runtime_problems":block["runtime_problems"],
                        "owner":core.uvi_diagnostic_progress(0,owner.request,&bridge).ok().as_ref().map(observation_value),
                        "sample_condition_at_last_ack":samples.ack.map(sample_condition)}),
                    );
                }
                if let Some(e) = audio.failure {
                    result["last_pcm_service_failure_kind"] = block["service_failure_kind"].clone();
                    issues.push(format!("pcm:{e}"));
                    break;
                }
                if audio.frames < note_frames {
                    bounds.pause(config.pace_ms, bounds.deadline);
                }
            }
        }
        if Instant::now() < bounds.deadline {
            let final_phase = acknowledge(
                core,
                owner,
                &bridge,
                &bounds,
                bounds.phase_deadline(),
                &mut service,
            );
            if !final_phase.drained() {
                issues.push(format!("final:{}", final_phase.status));
            }
            if let Some(a) = final_phase.ack {
                result["final_sample_condition"] = json!(sample_condition(a));
                if sample_condition(a) != "observed_settled" {
                    issues.push(format!("final_samples:{}", sample_condition(a)));
                }
                if a.runtime_faults != 0 || a.runtime_budgets != 0 {
                    issues.push("final_owner_fault_or_budget".into());
                }
            }
            result["final_owner_sample_condition"] = final_phase.report;
        } else {
            issues.push("total_wall_limit".into());
            result["final_owner_sample_condition"] =
                json!({"status":"total_wall_limit","observed":false});
        }
    }
    let problems = core.problems(0);
    if problems.script_overruns != 0
        || problems.nonfinite != 0
        || problems.capacity_drops != 0
        || problems.underruns != 0
        || problems.offline_failures != 0
        || problems.lua_faults != 0
    {
        issues.push("runtime_problems_observed".into());
    }
    if audio.frames != note_frames {
        issues.push("pcm_window_incomplete".into());
    }
    let complete = note_proven && audio.frames == note_frames && issues.is_empty();
    result["status"] = json!(if complete { "observed" } else { "partial" });
    result["complete"] = json!(complete);
    result["frames"] = json!(audio.frames);
    result["blocks"] = json!(audio.blocks);
    result["peak"] = json!(audio.peak);
    result["rms_all_output_channels"] = json!(if audio.frames == 0 {
        0.
    } else {
        (audio.squares / audio.frames as f64).sqrt()
    });
    result["nonzero_frames"] = json!(audio.nonzero_frames);
    result["nonzero_blocks"] = json!(audio.nonzero_blocks);
    result["max_active_voices"] = json!(audio.voices);
    result["active_voices_after"] = json!(core.voices().active);
    result["held_note_owned"] = json!(owner.held.is_some_and(|note| core.owns(note)));
    result["note_ends"] = json!(audio.note_ends + service.note_ends as u64);
    result["effects_applied"] = json!(audio.effects + service.effects as u64);
    result["zero_frame_service_blocks"] = json!(service.zero_frame_blocks);
    result["service_clock_frames"] = json!(0);
    result["pcm_block_digest_chain_sha256"] = json!(format!("{:x}", audio.chain.finalize()));
    result["pcm_digest_scope"] = json!(
        "v1 domain + LE u64 block index + LE u64 frame count + 64 ASCII hex bytes of each parent block PCM SHA256; all output channels; not a concatenated-PCM digest"
    );
    result["first_signal"] = json!(audio.first_signal);
    result["runtime_problems"] = json!(problems);
    result["issues"] = json!(issues);
    result["wall_ms"] = json!(bounds.started.elapsed().as_secs_f64() * 1000.);
    result["zero_output_interpretation"] = json!(if note_proven {
        "Owner execution observed; zero within this explicit input/window can be authored suppression or pending future work; compatibility remains unverified"
    } else {
        "Owner execution unverified; zero/absent PCM is not an instrument silence result"
    });
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::tree::MixTree;
    use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};
    use sampler_uvi::{
        OscGroup,
        script::{Config as ScriptConfig, Files},
        scripted::{Driver, Script, ScriptThread},
    };
    use std::sync::mpsc;

    fn config(phase: u16) -> Config {
        Config {
            phase_timeout_ms: phase,
            total_timeout_ms: 2000,
            pace_ms: 1,
            controllers: vec![],
        }
    }
    fn note() -> HostNote {
        HostNote {
            port: 0,
            channel: 0,
            key: 60,
            id: 7,
            clap: true,
        }
    }
    fn plan() -> Prepared {
        Prepared::new(
            48000,
            vec![Pcm::new(48000, vec![[0.5; 2]; 16000].into()).unwrap()],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: Some(60),
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::new(0, 0, 0, 1., 1).unwrap(),
                playback: Playback::default(),
            }],
            1,
        )
        .unwrap()
        .with_groups(1, vec![Some(0)])
        .unwrap()
    }
    fn install(runtime: Runtime, thread: ScriptThread) -> (V2Core, SwitchOwner) {
        let bridge = thread.ui().clone();
        let mut part = Part::new(
            runtime,
            MixTree::instrument("authored UVI inspector fixture"),
        )
        .unwrap();
        part.set_uvi_inspector_fixture_script(Driver::new(
            thread,
            vec![OscGroup {
                layer: 1,
                osc: 1,
                group: 0,
                keygroup: 0,
                oscillator: 0,
            }],
            48000,
        ));
        part.epoch = 7;
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, Some(Box::new(part)));
        let mut owner = SwitchOwner {
            request: 7,
            ..Default::default()
        };
        owner.scripts.uvi = Some(bridge);
        (core, owner)
    }
    fn fixture(
        body: &str,
        files: impl Files + Send + 'static,
        cfg: ScriptConfig,
    ) -> (V2Core, SwitchOwner) {
        let prepared = plan();
        let limits = Limits::for_plan(&prepared, 8, 8);
        let runtime = Runtime::new(prepared, limits).unwrap();
        let xml = format!(
            "<UVI4><Program><Layers><Layer/></Layers><EventProcessors><ScriptProcessor><script><![CDATA[{body}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
        );
        let (thread, _) = ScriptThread::spawn(xml, files, cfg).unwrap();
        install(runtime, thread)
    }
    struct Held {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Files for Held {
        fn script(&self, _: &str) -> Option<String> {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(3)).unwrap();
            Some("return {}".into())
        }
    }

    #[test]
    fn held_actual_on_note_is_partial_then_release_drains_ui_and_real_child_pcm() {
        let (entered, waiting) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let (mut core, mut owner) = fixture(
            "local done=OnOffButton('Done',false); function onNote(e) require('hold'); for i=1,4097 do Label('Authored') end; done:setValue(true); playNote(e.note,e.velocity) end",
            Held {
                entered,
                release: held,
            },
            ScriptConfig::default(),
        );
        // Keep the sender alive until core drops on assertion failures too.
        struct Unblock(mpsc::Sender<()>);
        impl Drop for Unblock {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let unblock = Unblock(release);
        let initial_revision = owner.scripts.uvi.as_ref().unwrap().revision();
        assert!(!core.scripts_running(0));
        let report = probe_note(&mut core, &mut owner, &config(50), note(), 64, 4800).unwrap();
        waiting.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(report["status"], "partial");
        assert_eq!(report["complete"], false);
        assert_eq!(report["frames"], 0);
        assert_eq!(report["peak"], 0.);
        assert_eq!(report["activation_readiness"], "uvi_owner_unverified");
        assert_eq!(report["note_phase"]["counts"]["accepted"], 1);
        assert_eq!(report["note_phase"]["counts"]["executed"], 0);
        assert_eq!(core.voices().active, 0);
        unblock.0.send(()).unwrap();
        let bridge = owner.scripts.uvi.as_ref().unwrap().clone();
        let bounds = Bounds::new(&config(1500));
        let mut service = Service::default();
        let phase = acknowledge(
            &mut core,
            &mut owner,
            &bridge,
            &bounds,
            bounds.phase_deadline(),
            &mut service,
        );
        assert!(phase.drained(), "{}", phase.report);
        let a = phase.ack.unwrap();
        assert!(
            a.ui_revision > initial_revision,
            "UI export precedes owner ack"
        );
        assert!(a.commands_pushed > 0);
        assert!(a.sample_counts_conserved());
        let mut peak = 0f32;
        let allocations = crate::plugin::tests::allocations(|| {
            core.begin_block(&BlockInfo {
                frames: 128,
                offline: true,
                ..Default::default()
            });
            let output = core.render(128);
            for bus in output.buses {
                for channel in bus {
                    for sample in &channel[..128] {
                        peak = peak.max(sample.abs());
                    }
                }
            }
            core.end_block(128, &mut |_| true);
        });
        assert_eq!(allocations, 0);
        assert!(peak > 0.01);
        assert!(core.voices().active > 0);
    }

    #[test]
    fn future_wait_ack_has_due_work_and_paced_clock_exposes_later_child() {
        let (mut core, mut owner) = fixture(
            "function onNote(e) wait(20); playNote(e.note,e.velocity) end",
            (),
            ScriptConfig::default(),
        );
        let report = probe_note(&mut core, &mut owner, &config(1000), note(), 64, 2048).unwrap();
        assert_eq!(report["status"], "observed", "{report}");
        let ack = &report["note_phase"]["owner"]["acknowledgement"];
        assert_eq!(ack["commands_pushed"], 0);
        assert_eq!(ack["next_due_ms"], 20.);
        assert!(
            report["first_signal"]["frames_through_block"]
                .as_u64()
                .unwrap()
                > 128
        );
        assert!(report["peak"].as_f64().unwrap() > 0.01);
        assert_eq!(report["service_clock_frames"], 0);
        assert!(report.get("pcm_sha256").is_none());
    }

    #[test]
    fn explicit_controllers_are_acknowledged_before_note_and_default_is_empty() {
        let body = "local gate=false; function onController(e) if e.controller==11 and e.value==100 then gate=true end end; function onNote(e) if gate then playNote(e.note,e.velocity) end end";
        let (mut core, mut owner) = fixture(body, (), ScriptConfig::default());
        let empty = probe_note(&mut core, &mut owner, &config(1000), note(), 64, 128).unwrap();
        assert_eq!(empty["status"], "observed", "{empty}");
        assert_eq!(empty["controller_phase"]["input_count"], 0);
        assert_eq!(empty["peak"], 0.);
        assert_eq!(empty["note_phase"]["counts"]["accepted"], 1);
        let (mut core, mut owner) = fixture(body, (), ScriptConfig::default());
        let mut cfg = config(1000);
        cfg.controllers.push(Controller {
            channel: 0,
            cc: 11,
            value: 100,
        });
        let explicit = probe_note(&mut core, &mut owner, &cfg, note(), 64, 128).unwrap();
        assert_eq!(explicit["status"], "observed", "{explicit}");
        assert_eq!(explicit["controller_phase"]["counts"]["accepted"], 1);
        assert_eq!(explicit["controller_phase"]["counts"]["executed"], 1);
        assert_eq!(
            explicit["note_phase"]["counts"]["before"]["events_executed"],
            1
        );
        assert!(explicit["peak"].as_f64().unwrap() > 0.01);
    }

    #[test]
    fn no_on_note_uses_direct_sampler_with_real_owner_event_acknowledgement() {
        let (mut core, mut owner) = fixture("", (), ScriptConfig::default());
        let report = probe_note(&mut core, &mut owner, &config(1000), note(), 64, 128).unwrap();
        assert_eq!(report["status"], "observed", "{report}");
        assert_eq!(report["note_route"], "direct_sampler_no_on_note");
        assert_eq!(report["lua_note_callback_expected"], false);
        assert_eq!(report["note_phase"]["counts"]["expected"], 1);
        assert_eq!(report["note_phase"]["counts"]["accepted"], 1);
        assert_eq!(report["note_phase"]["counts"]["executed"], 1);
        assert_eq!(report["note_phase"]["physical_host_note_owned"], true);
        assert_eq!(
            report["activation_readiness"],
            "direct_note_admitted_owner_commands_observed"
        );
        assert!(report["peak"].as_f64().unwrap() > 0.01);
        assert_eq!(report["note_phase"]["owner"]["commands_drained"], true);
    }

    #[test]
    fn limits_and_unknown_fields_are_rejected_without_inputs_or_loading() {
        let base = json!({"phase_timeout_ms":100,"total_timeout_ms":1000,"pace_ms":1});
        let valid: Config = serde_json::from_value(base.clone()).unwrap();
        assert!(valid.validate(128).is_ok());
        assert!(valid.controllers.is_empty());
        for (key, value) in [
            ("phase_timeout_ms", json!(0)),
            ("total_timeout_ms", json!(10001)),
            ("pace_ms", json!(0)),
            ("pace_ms", json!(21)),
            ("controllers", json!([{"channel":16,"cc":11,"value":127}])),
            ("controllers", json!([{"channel":0,"cc":128,"value":127}])),
        ] {
            let mut bad = base.clone();
            bad[key] = value;
            let cfg: Config = serde_json::from_value(bad).unwrap();
            assert!(cfg.validate(4800).is_err());
        }
        assert!(valid.validate(127).is_err());
        assert!(valid.validate(96001).is_err());
        let mut unknown = base.clone();
        unknown["seed"] = json!(1);
        assert!(serde_json::from_value::<Config>(unknown).is_err());
        let mut unknown = base;
        unknown["controllers"] = json!([{"channel":0,"cc":11,"value":127,"expand":true}]);
        assert!(serde_json::from_value::<Config>(unknown).is_err());
    }

    #[test]
    fn stale_epoch_foreign_bridge_and_seeded_owners_never_admit_probe_input() {
        let (mut core, mut owner) = fixture(
            "function onNote(e) playNote(e.note,e.velocity) end",
            (),
            ScriptConfig::default(),
        );
        owner.request = 8;
        let stale = probe_note(&mut core, &mut owner, &config(100), note(), 64, 128).unwrap();
        assert_eq!(stale["note_submitted"], false);
        assert!(stale["issues"].to_string().contains("StaleEpoch"));
        owner.request = 7;
        let (_candidate, foreign) = fixture("", (), ScriptConfig::default());
        owner.scripts.uvi = foreign.scripts.uvi.clone();
        let foreign = probe_note(&mut core, &mut owner, &config(100), note(), 64, 128).unwrap();
        assert_eq!(foreign["note_submitted"], false);
        assert!(foreign["issues"].to_string().contains("StaleOwner"));
        owner.scripts.uvi = None;
        assert!(probe_note(&mut core, &mut owner, &config(100), note(), 64, 128).is_err());
        let (mut core, mut owner) = fixture(
            "",
            (),
            ScriptConfig {
                audit_seed: Some(17),
                ..ScriptConfig::default()
            },
        );
        let seeded = probe_note(&mut core, &mut owner, &config(100), note(), 64, 128).unwrap();
        assert_eq!(seeded["note_submitted"], false);
        assert_eq!(seeded["zero_frame_service_blocks"], 0);
        assert!(
            seeded["initial_owner_sample_condition"]
                .to_string()
                .contains("SeededOwner")
        );
    }

    #[test]
    fn zero_frame_service_keeps_future_clock_and_has_no_audio_heap_calls() {
        let (mut core, mut owner) = fixture(
            "function onNote(e) wait(2); playNote(e.note,e.velocity) end",
            (),
            ScriptConfig::default(),
        );
        let bridge = owner.scripts.uvi.as_ref().unwrap().clone();
        core.enable_uvi_diagnostics(0, 7, &bridge).unwrap();
        core.begin_block(&BlockInfo {
            frames: 0,
            offline: true,
            ..Default::default()
        });
        core.play(
            0,
            Event::NoteOn {
                note: note(),
                velocity: 0.5,
                tune: 0.,
            },
        );
        let cfg = config(1000);
        let bounds = Bounds::new(&cfg);
        let mut service = Service::default();
        let phase = acknowledge(
            &mut core,
            &mut owner,
            &bridge,
            &bounds,
            bounds.phase_deadline(),
            &mut service,
        );
        assert!(phase.drained());
        assert_eq!(phase.ack.unwrap().next_due_ms, Some(2.));
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                for _ in 0..32 {
                    service_zero(&mut core, &mut owner, &mut service).unwrap();
                }
            }),
            0
        );
        let phase = acknowledge(
            &mut core,
            &mut owner,
            &bridge,
            &bounds,
            bounds.phase_deadline(),
            &mut service,
        );
        assert_eq!(phase.ack.unwrap().commands_pushed, 0);
        assert_eq!(phase.ack.unwrap().next_due_ms, Some(2.));
        assert_eq!(core.voices().active, 0);
    }

    #[test]
    fn submitted_dynamic_sample_is_pending_until_ordinary_core_commit_and_fresh_ack() {
        use sampler_core::{SampleRevisionLimits, SampleTarget, SampleTargetId};
        use sampler_uvi::script::{SampleResource, SampleSource};
        struct Sources;
        impl Files for Sources {
            fn script(&self, _: &str) -> Option<String> {
                None
            }
            fn sample_source(&self) -> Option<SampleSource> {
                Some(Arc::new(|_, _| {
                    struct Source;
                    impl sampler_kontakt::AssetSource for Source {
                        fn open(&self) -> std::io::Result<sampler_kontakt::SampleReader> {
                            Ok(sampler_kontakt::SampleReader::custom(
                                48000,
                                256,
                                |_, out| {
                                    out.fill([0.25; 2]);
                                    Ok(())
                                },
                            ))
                        }
                    }
                    Ok(SampleResource {
                        source: Arc::new(Source),
                        reader: None,
                        playback: Playback::default(),
                        base_note: 60,
                        fine_tune: 0.,
                        geometry_error: None,
                        format_code: 0,
                        channels: 2,
                        bits: 32,
                        tempo: 0.,
                        timings: Default::default(),
                    })
                }))
            }
        }
        let xml = "<UVI4><Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer BaseNote='60'/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script><![CDATA[
            local osc=Program.layers[1].keygroups[1].oscillators[1]
            local ready=OnOffButton('Ready',false)
            function onInit() loadSample(osc,'authored',function(t) assert(t.success); ready:setValue(true) end) end
            function onNote(e) playNote(e.note,e.velocity) end
        ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>";
        let target = sampler_uvi::parse_program_xml(xml)
            .unwrap()
            .descendants()
            .find(|n| n.has_tag_name("SamplePlayer"))
            .unwrap()
            .id()
            .get_usize() as u32;
        let prepared = plan()
            .with_sample_targets(
                vec![SampleTarget {
                    id: SampleTargetId(target),
                    regions: vec![0].into(),
                }],
                SampleRevisionLimits {
                    slots: 4,
                    resident_bytes: 1 << 20,
                },
            )
            .unwrap();
        let limits = Limits::for_plan(&prepared, 8, 8);
        let (runtime, control) = Runtime::new(prepared, limits)
            .unwrap()
            .with_sample_updates(4)
            .unwrap();
        let (mut thread, loaded) =
            ScriptThread::spawn(xml.into(), Sources, ScriptConfig::default()).unwrap();
        thread.bind_sample_control(control).unwrap();
        // Before installation no core render/commit service can run. A fully
        // decoded and submitted sample is still NOT successful/settled.
        let started = Instant::now();
        loop {
            assert!(started.elapsed() < Duration::from_secs(3));
            let mut token = loaded
                .ui
                .request_diagnostics(Duration::from_secs(1))
                .unwrap();
            let a = loop {
                match token.poll() {
                    DiagnosticPoll::Complete(a) => break a,
                    DiagnosticPoll::Pending => std::thread::sleep(Duration::from_millis(1)),
                    other => panic!("authored sample request: {other:?}"),
                }
            };
            thread.drain(&mut Vec::new());
            assert!(token.commands_drained());
            assert!(a.sample_counts_conserved());
            if a.samples.submitted == 1 {
                assert_eq!(a.samples.successful, 0);
                assert_eq!(sample_condition(a), "pending_samples");
                break;
            }
        }
        let (mut core, mut owner) = install(runtime, thread);
        let report = probe_note(&mut core, &mut owner, &config(1000), note(), 64, 128).unwrap();
        assert_eq!(report["status"], "observed", "{report}");
        let samples = &report["samples_before_pcm"]["acknowledgement"]["samples"];
        assert_eq!(samples["requested"], 1);
        assert_eq!(samples["successful"], 1);
        assert_eq!(samples["pending"], 0);
        assert_eq!(samples["counts_conserved"], true);
        assert_eq!(report["final_sample_condition"], "observed_settled");
        assert!(report["peak"].as_f64().unwrap() > 0.01);
        assert!(loaded.ui.values().iter().any(|(_, v)| *v == 1.));
    }

    #[test]
    fn queued_sample_metadata_remains_explicit_partial_after_owner_execution() {
        struct QueuedSource(Arc<AtomicBool>);
        impl Files for QueuedSource {
            fn script(&self, _: &str) -> Option<String> {
                None
            }
            fn sample_source(&self) -> Option<sampler_uvi::script::SampleSource> {
                let invoked = self.0.clone();
                Some(Arc::new(move |_, _| {
                    invoked.store(true, Ordering::Release);
                    Err("authored source must remain queued without SampleControl".into())
                }))
            }
        }
        let invoked = Arc::new(AtomicBool::new(false));
        let prepared = plan();
        let limits = Limits::for_plan(&prepared, 8, 8);
        let runtime = Runtime::new(prepared, limits).unwrap();
        let xml="<UVI4><Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer BaseNote='60'/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script><![CDATA[
            function onInit() loadSample(Program.layers[1].keygroups[1].oscillators[1],'authored') end
            function onNote(e) playNote(e.note,e.velocity) end
        ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>";
        // Deliberately do not bind SampleControl: owner acknowledgement can
        // observe queued metadata, but cannot manufacture its core completion.
        let (thread, _) = ScriptThread::spawn(
            xml.into(),
            QueuedSource(invoked.clone()),
            ScriptConfig::default(),
        )
        .unwrap();
        let (mut core, mut owner) = install(runtime, thread);
        let report = probe_note(&mut core, &mut owner, &config(50), note(), 64, 128).unwrap();
        assert_eq!(report["status"], "partial", "{report}");
        assert!(!report["issues"].as_array().unwrap().is_empty());
        assert_eq!(report["note_submitted"], true);
        assert_eq!(
            report["activation_readiness"],
            "earlier_note_owner_and_commands_observed"
        );
        assert_eq!(report["frames"], 128);
        assert_eq!(report["final_sample_condition"], "pending_samples");
        let samples = &report["final_owner_sample_condition"]["acknowledgement"]["samples"];
        assert_eq!(samples["requested"], 1);
        assert_eq!(samples["successful"], 0);
        assert_eq!(samples["queued_metadata"], 1);
        assert_eq!(samples["pending"], 1);
        assert_eq!(samples["counts_conserved"], true);
        assert!(
            !invoked.load(Ordering::Acquire),
            "metadata must not reach a worker before host binding"
        );
    }
}
