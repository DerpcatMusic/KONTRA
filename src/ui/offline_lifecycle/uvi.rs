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
    /// Explicitly requested, note-relative physical output-pair windows.
    #[serde(default)]
    output_windows: Vec<OutputWindow>,
    /// Explicit compiled IR indices, never native UVI identities.
    #[serde(default)]
    gain_groups: Vec<u32>,
    /// Default-off bounded query of this actual installed owner.
    #[serde(default)]
    live_graph: Option<LiveGraphConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveGraphConfig {
    query: sampler_uvi::script::LiveGraphQuery,
    expected_player_node: u32,
    expected_location_sha256: String,
    expected_location_utf8_bytes: usize,
    #[serde(default)]
    held_checkpoints: Vec<usize>,
}
impl LiveGraphConfig {
    fn validate(&self, frames: usize) -> Result<(), &'static str> {
        self.query.validate()?;
        if !self.query.player_nodes.contains(&self.expected_player_node)
            || self.expected_location_sha256.len()!=64
            || !self.expected_location_sha256.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !(1..=65536).contains(&self.expected_location_utf8_bytes)
            || self.held_checkpoints.len()>3
            || self.held_checkpoints.iter().enumerate().any(|(i,&n)|n<128 || n>frames
                || (i!=0 && self.held_checkpoints[i-1]>=n)) {
            return Err("invalid_live_graph_config");
        }
        Ok(())
    }
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
            || !(128..=MAX_NOTE_FRAMES).contains(&note_frames)
            || self.controllers.len() > 32
            || self
                .controllers
                .iter()
                .any(|c| c.channel > 15 || c.cc > 127 || c.value > 127)
        {
            return Err(
                "UVI probe requires phase/total wall limits 1..10000 ms with phase <= total, pace 1..20 ms, 128..288000 note frames and at most 32 valid explicit MIDI controllers",
            );
        }
        if self.gain_groups.len()>8 || self.gain_groups.iter().enumerate()
            .any(|(i,g)|*g>65535 || self.gain_groups[..i].contains(g)) {
            return Err("UVI gain readback requires at most eight unique compiled group indices <=65535");
        }
        if let Some(graph)=&self.live_graph { graph.validate(note_frames)?; }
        validate_output_windows(&self.output_windows, note_frames)?;
        Ok(())
    }
}

// A finite six-second observation can cross the selected Clarinet file end.
// Wall time, output-window storage and correlation work keep their own caps.
pub(super) const MAX_NOTE_FRAMES: usize = 288_000;

// Work and memory bounds are checked with the complete plan before loading.
const OUTPUT_WINDOW_FRAMES: usize = 96_000;
const OUTPUT_WINDOW_CORRELATION_WORK: usize = 128_000_000;
const OUTPUT_WINDOW_RATE: f64 = 48_000.;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputWindow {
    start_frame: usize,
    end_frame: usize,
    output_pair: usize,
    min_lag: usize,
    max_lag: usize,
    max_peaks: usize,
}
fn validate_output_windows(windows: &[OutputWindow], note_frames: usize) -> Result<(), &'static str> {
    if windows.len() > 8 {
        return Err("At most eight explicit output windows are allowed");
    }
    let mut frames = 0usize;
    let mut work = 0usize;
    for w in windows {
        let len = w.end_frame.checked_sub(w.start_frame)
            .filter(|len| *len >= 8 && w.end_frame <= note_frames)
            .ok_or("Output window must contain at least eight frames within the held note")?;
        if w.output_pair >= crate::sound::BUSES || w.min_lag == 0
            || w.min_lag > w.max_lag || w.max_lag > 2048
            || w.max_lag >= len / 2 || !(1..=8).contains(&w.max_peaks)
        {
            return Err("Invalid output pair, lag bounds or peak count");
        }
        frames = frames.checked_add(len).ok_or("Output window frame budget overflow")?;
        let operations = len.checked_mul(w.max_lag - w.min_lag + 3)
            .and_then(|v| v.checked_mul(2)).ok_or("Output correlation work budget overflow")?;
        work = work.checked_add(operations).ok_or("Output correlation work budget overflow")?;
        if frames > OUTPUT_WINDOW_FRAMES || work > OUTPUT_WINDOW_CORRELATION_WORK {
            return Err("Output windows exceed the frame or correlation work budget");
        }
    }
    Ok(())
}

struct WindowSamples {
    config: OutputWindow,
    samples: Vec<[f32; 2]>,
}
pub(super) struct OutputWindowCapture {
    frames: usize,
    windows: Vec<WindowSamples>,
}
impl OutputWindowCapture {
    fn new(windows: &[OutputWindow]) -> Self {
        Self {
            frames: 0,
            windows: windows.iter().cloned().map(|config| {
                let samples = Vec::with_capacity(config.end_frame - config.start_frame);
                WindowSamples { config, samples }
            }).collect(),
        }
    }
    /// Runs after core.render on this off-audio diagnostic host. Capacities
    /// already cover every intersection; PCM never enters serde or a log.
    pub(super) fn observe(&mut self, buses: &[crate::sound::Block; crate::sound::BUSES], frames: usize) {
        let end = self.frames + frames;
        for window in &mut self.windows {
            let start = self.frames.max(window.config.start_frame);
            let stop = end.min(window.config.end_frame);
            if start >= stop { continue; }
            let channels = &buses[window.config.output_pair];
            for frame in start - self.frames..stop - self.frames {
                window.samples.push([channels[0][frame], channels[1][frame]]);
            }
        }
        self.frames = end;
    }
    fn finish(self) -> Value {
        let started = Instant::now();
        let windows: Vec<_> = self.windows.into_iter().enumerate().map(|(index,w)| {
            let expected = w.config.end_frame - w.config.start_frame;
            json!({"index":index,"start_frame":w.config.start_frame,"end_frame":w.config.end_frame,
                "output_pair":w.config.output_pair,"sample_rate":OUTPUT_WINDOW_RATE,
                "frames":w.samples.len(),"requested_frames":expected,"complete":w.samples.len()==expected,
                "min_lag":w.config.min_lag,"max_lag":w.config.max_lag,"max_peaks":w.config.max_peaks,
                "channels":[output_channel_metrics(&w.samples,0,&w.config),output_channel_metrics(&w.samples,1,&w.config)]})
        }).collect();
        json!({"windows":windows,"analysis_wall_ms":started.elapsed().as_secs_f64()*1000.,
            "frame_budget":OUTPUT_WINDOW_FRAMES,"correlation_work_budget":OUTPUT_WINDOW_CORRELATION_WORK,
            "pcm_exported":false,"scope":"Exact half-open note-relative PCM intersections, each explicit physical output pair/channel independently; bounded DC-removed normalized autocorrelation candidates, not a native-parity or perceptual pitch verdict"})
    }
}

fn output_channel_metrics(samples: &[[f32; 2]], channel: usize, config: &OutputWindow) -> Value {
    let finite = samples.iter().all(|s| s[channel].is_finite());
    if samples.is_empty() || !finite {
        return json!({"channel":channel,"frames":samples.len(),"finite":finite,"measured":false,"reason":if finite {"no_frames"}else{"nonfinite"}});
    }
    let count = samples.len() as f64;
    let dc = samples.iter().map(|s| f64::from(s[channel])).sum::<f64>() / count;
    let square_sum = samples.iter().map(|s| f64::from(s[channel]).powi(2)).sum::<f64>();
    let rms = (square_sum / count).sqrt();
    // A centered second pass preserves tiny AC on top of a large DC offset.
    let ac_rms = (samples.iter().map(|s| (f64::from(s[channel])-dc).powi(2)).sum::<f64>() / count).sqrt();
    let peak = samples.iter().map(|s| f64::from(s[channel]).abs()).fold(0., f64::max);
    let mut result = json!({"channel":channel,"frames":samples.len(),"finite":true,"measured":true,
        "dc":dc,"rms":rms,"ac_rms":ac_rms,"peak":peak,
        "rms_dbfs":(rms>0.).then(||20.*rms.log10()),"ac_rms_dbfs":(ac_rms>0.).then(||20.*ac_rms.log10()),
        "periodicity":{"status":"no_ac_energy","peaks":[]}});
    if ac_rms == 0. { return result; }
    if samples.len() <= config.max_lag * 2 {
        result["periodicity"]["status"] = json!("insufficient_observed_frames");
        return result;
    }
    // Include one neighboring lag at either end so requested boundary peaks
    // have a real local-maximum test and parabolic interpolation.
    let first_lag = config.min_lag.saturating_sub(1);
    let last_lag = config.max_lag + 1;
    let correlation: Vec<f64> = (first_lag..=last_lag).map(|lag| {
        let (mut product, mut a2, mut b2) = (0.,0.,0.);
        for i in 0..samples.len()-lag {
            let a = f64::from(samples[i][channel])-dc;
            let b = f64::from(samples[i+lag][channel])-dc;
            product += a*b; a2 += a*a; b2 += b*b;
        }
        if a2 > 0. && b2 > 0. { (product/(a2*b2).sqrt()).clamp(-1.,1.) } else { 0. }
    }).collect();
    let mut peaks: Vec<_> = (config.min_lag..=config.max_lag).filter_map(|lag| {
        let i = lag-first_lag;
        let (before,score,after) = (correlation[i-1],correlation[i],correlation[i+1]);
        if score <= 0. || score <= before || score < after { return None; }
        let curvature = before-2.*score+after;
        let fraction = if curvature < 0. { (0.5*(before-after)/curvature).clamp(-0.5,0.5) } else { 0. };
        Some((lag,score,lag as f64+fraction))
    }).collect();
    peaks.sort_by(|a,b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    peaks.truncate(config.max_peaks);
    result["periodicity"] = json!({"status":"measured","peaks":peaks.into_iter().map(|(lag,score,refined)|
        json!({"lag_frames":lag,"refined_lag_frames":refined,"hz":OUTPUT_WINDOW_RATE/refined,"normalized_correlation":score})).collect::<Vec<_>>(),
        "candidate_scope":"Positive local maxima sorted by correlation, then lag; harmonics/octave alternatives retained, no octave correction"});
    result
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
    acknowledge_query(core, owner, bridge, bounds, deadline, service, None)
}
fn acknowledge_query(
    core: &mut V2Core, owner: &mut SwitchOwner, bridge: &Arc<UiBridge>,
    bounds: &Bounds, deadline: Instant, service: &mut Service,
    query: Option<&sampler_uvi::script::LiveGraphQuery>,
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
    let requested = match query {
        Some(q) => core.request_uvi_live_graph(0, owner.request, bridge, timeout, q.clone()),
        None => core.request_uvi_diagnostics(0, owner.request, bridge, timeout),
    };
    let mut token = match requested {
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
    let graph = if query.is_some() && status == "acknowledged_and_drained" {
        core.take_uvi_live_graph(&mut token).ok().flatten().map(|g|g.value)
    } else { None };
    let mut phase = Phase {
        status,
        ack,
        progress: last,
        report: json!({"status":status,"detail":detail,
        "request_id":token.id(),"acknowledgement":ack.map(acknowledgement_value),
        "commands_drained":drained,"observation":observation,"polls":polls,
        "command_drain_baseline":drain_baseline,"first_command_drain_observed":first_command_drain,
        "wall_ms":started.elapsed().as_secs_f64()*1000.,"clock_frames":0,
        "phase_observations":trace,"phase_observations_omitted":trace_omitted}),
    };
    if query.is_some() { phase.report["live_graph_owner"] = graph.unwrap_or_else(||json!({"status":"unavailable"})); }
    phase
}

/// Reuse the ordinary owner/FIFO-marker service for an inspector UI edit.
/// This does not invent an edit counter: the diagnostic marker follows the
/// accepted edit on the same UI queue and acknowledges the owner attempt.
pub(super) fn acknowledge_ui_edit(
    core: &mut V2Core, owner: &mut SwitchOwner, bridge: &Arc<UiBridge>, timeout_ms: u16,
) -> Value {
    let started = Instant::now();
    let bounds = Bounds { started, deadline: started + Duration::from_millis(u64::from(timeout_ms)), phase: Duration::from_millis(u64::from(timeout_ms)) };
    let mut service = Service::default();
    let phase = acknowledge(core, owner, bridge, &bounds, bounds.deadline, &mut service);
    json!({"owner_acknowledged":phase.ack.is_some(),"ordered_drained":phase.drained(),
        "phase":phase.report,"zero_frame_service_blocks":service.zero_frame_blocks,
        "effects_applied":service.effects,"clock_frames":0})
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

// All helpers below are off-audio numeric projections, never loaders/services.
#[derive(Default)]
struct GraphCaptures { rows:Vec<Value>, bytes:usize, omitted:usize }
fn graph_size(v:&Value,limit:usize)->bool {
    struct Count(usize,usize);
    impl std::io::Write for Count {
        fn write(&mut self,b:&[u8])->std::io::Result<usize>{self.0=self.0.checked_add(b.len()).ok_or(std::io::ErrorKind::OutOfMemory)?;if self.0>self.1{return Err(std::io::ErrorKind::OutOfMemory.into())}Ok(b.len())}
        fn flush(&mut self)->std::io::Result<()>{Ok(())}
    }
    serde_json::to_writer(&mut Count(0,limit),v).is_ok()
}
impl GraphCaptures {
    fn push(&mut self,mut row:Value) {
        if self.rows.len()==8 {self.omitted+=1;return}
        if !graph_size(&row,256<<10) {row=json!({"phase":row["phase"],"status":"refused","reason":"phase_receipt_size"});}
        // Logical serialized limit; this does not describe allocator RSS.
        let bytes=serde_json::to_vec(&row).expect("bounded numeric graph receipt").len();
        if self.bytes.checked_add(bytes).is_none_or(|n|n>(2<<20)) {self.omitted+=1;return}
        self.bytes+=bytes;self.rows.push(row);
    }
}
fn graph_join(records:&[Value], regions:&Value, complete:bool, group:Option<u64>, expected:Option<(&str,usize)>)->Value {
    if !complete {return json!({"status":"refused","reason":"incomplete_region_matches"})}
    let Some(regions)=regions.as_array() else{return json!({"status":"unavailable","reason":"no_region_identity"})};
    let matches:Vec<_>=records.iter().filter(|r|regions.iter().any(|n|n==&r["region"])
        && group.is_none_or(|g|r["worker_group"].as_u64()==Some(g))).collect();
    if matches.len()!=1 {return json!({"status":if matches.is_empty(){"unavailable"}else{"ambiguous"},
        "reason":if matches.is_empty(){"unavailable_dynamic_or_static_provenance"}else{"multiple_target_recipes"},"matching_count":matches.len()})}
    let r=matches[0];
    if r["location_sha256"].as_str().is_none() || r["location_utf8_bytes"].as_u64().is_none(){
        return json!({"status":"unavailable","reason":"unsupported_or_missing_asset_location","recipe":r});
    }
    if expected.is_some_and(|(h,n)|r["location_sha256"]!=h || r["location_utf8_bytes"]!=n) {
        return json!({"status":"refused","reason":"resource_identity_mismatch","recipe":r});
    }
    json!({"status":"joined","recipe":r})
}
// Exact ordinary AssetLocation contract: a Kontakt file-table ID is not a path.
fn graph_location_bytes(location:&sampler_ir::AssetLocation)->Option<&[u8]>{
    match location{sampler_ir::AssetLocation::Path(path)=>Some(path.as_bytes()),
        sampler_ir::AssetLocation::KontaktFile{..}=>None}
}
fn graph_worker_ir(owner:&SwitchOwner,q:&sampler_uvi::script::LiveGraphQuery,nodes:&[u32])->Result<(Vec<Value>,Value,Vec<(i32,i32,i32)>),&'static str>{
    use sampler_ir::Processor as P;
    let ir=owner.instrument.as_ref().ok_or("worker_ir_unavailable")?;
    if ir.source_indices.sample_targets.len()>262144 || ir.chains.len()>262144 {return Err("worker_ir_scan_bound")}
    let mut records=Vec::new();let mut chains=std::collections::BTreeSet::new();let mut steps=0usize;let mut bytes=0usize;
    for &node in &q.player_nodes {
        let targets:Vec<_>=ir.source_indices.sample_targets.iter().filter(|t|t.node==node).collect();
        if targets.len()!=1 {return Err("worker_target_missing_or_ambiguous")}
        if targets[0].zones.len()>64 {return Err("worker_target_region_bound")}
        for region in &targets[0].zones {
            let z=ir.zones.get(region.0).ok_or("worker_region_missing")?;
            let asset=z.asset.and_then(|a|ir.assets.get(a.0));
            let location=asset.and_then(|a|graph_location_bytes(&a.location));
            if let Some(b)=location {
                bytes=bytes.checked_add(b.len()).ok_or("worker_location_budget")?;
                if b.len()>65536 || bytes>(16<<20) {return Err("worker_location_budget")}
            }
            let group=z.group.and_then(|g|ir.groups.get(g.0));
            for chain in [z.chain,group.and_then(|g|g.chain)].into_iter().flatten(){chains.insert(chain.0);}
            records.push(json!({"player_node":node,"region":region.0,"worker_group":z.group.map(|g|g.0),
                "worker_asset":z.asset.map(|a|a.0),
                "location_identity_status":if location.is_some(){"path"}else{"unavailable_nonpath_or_missing_asset"},"location_sha256":location.map(|b|format!("{:x}",Sha256::digest(b))),
                "location_utf8_bytes":location.map(|b|b.len()),"low_key":z.keys.low,"high_key":z.keys.high,
                "low_velocity":z.velocities.low,"high_velocity":z.velocities.high,
                "zone_gain":z.gain.linear(),"zone_chain":z.chain.map(|c|c.0),
                "group_muted":group.map(|g|g.muted),"group_gain":group.map(|g|g.gain.linear()),
                "group_chain":group.and_then(|g|g.chain).map(|c|c.0)}));
        }
    }
    let mut mixes=Vec::new();
    for(index,c)in ir.chains.iter().enumerate(){for p in c.pre_amplitude.iter().chain(c.post_amplitude.iter()){
        steps+=1;if steps>262144{return Err("worker_processor_scan_bound")}
        if let P::Mix{address,..}=p {if u32::try_from(address.generic).ok().is_some_and(|n|nodes.contains(&n)){
            chains.insert(index);let a=(address.group,address.slot,address.generic);
            if !mixes.contains(&a){if mixes.len()==32{return Err("worker_mix_bound")}mixes.push(a)}
        }}
    }}
    if chains.len()>32 {return Err("worker_chain_bound")}
    let mut rows=Vec::new();
    for index in chains {
        let c=ir.chains.get(index).ok_or("worker_chain_missing")?;
        if c.pre_amplitude.len()+c.post_amplitude.len()>32 {return Err("worker_chain_processor_bound")}
        let processors:Vec<_>=c.pre_amplitude.iter().map(|p|("pre",p)).chain(c.post_amplitude.iter().map(|p|("post",p))).enumerate().map(|(ordinal,(section,p))|{
            let setting=match p {
                P::Mix{count,address,dry,wet,bypass}=>json!({"kind":"mix","count":count,"dry":dry,"wet":wet,"bypass":bypass,
                    "group":address.group,"slot":address.slot,"generic":address.generic}),
                P::StereoMatrix(m)=>json!({"kind":"truncated_stereo_matrix","matrix":m}),
                P::Gain(g)=>json!({"kind":"gain","linear":g.linear()}),
                P::Branch{count,gain,first,last}=>json!({"kind":"branch","count":count,"linear":gain.linear(),"first":first,"last":last}),
                P::Filter(_)=>json!({"kind":"filter","transfer_not_observed":true}),
                P::Space(_)=>json!({"kind":"space","transfer_not_observed":true}),
                P::Dynamics(_)=>json!({"kind":"dynamics","transfer_not_observed":true}),
                _=>json!({"kind":"other","unsupported_scope_proof":true}),
            };json!({"ordinal":ordinal,"section":section,"settings":setting})
        }).collect();
        let scope=match c.scope {sampler_ir::Scope::Voice=>json!({"kind":"voice"}),sampler_ir::Scope::Group(g)=>json!({"kind":"group","index":g.0}),sampler_ir::Scope::Bus(b)=>json!({"kind":"bus","index":b.0}),sampler_ir::Scope::Master=>json!({"kind":"master"})};
        rows.push(json!({"chain":index,"scope":scope,"processors":processors}));
    }
    Ok((records,json!({"chains":rows,"inventory_complete":true,"complete":false,
        "route_linkage":{"complete":false,"reason":"compiled_route_linkage_unavailable"},
        "scope":"Bounded initialized chain inventory only; owner-to-compiled send/output/bus linkage is not proved"}),mixes))
}
fn capture_graph(
    core:&V2Core,owner:&SwitchOwner,bridge:&Arc<UiBridge>,config:&LiveGraphConfig,
    phase:&mut Phase,label:&str,frames:usize,captures:&mut GraphCaptures,
){
    let graph=phase.report.as_object_mut().and_then(|r|r.remove("live_graph_owner")).unwrap_or(Value::Null);
    let mut row=json!({"phase":label,"note_relative_output_frames":frames,"owner":graph,
        "request_id":phase.ack.map(|a|a.request_id),"acknowledgement":phase.ack.map(acknowledgement_value),
        "status":"partial","owner_identity":{"slot":0,"epoch":owner.request,"arc_guard_required":true},"scope":"Owner command prefix plus bracketed pure core reads, not a cross-thread atomic snapshot or future-state proof"});
    let observe=||->Result<Value,&'static str>{
        if !phase.drained(){return Err("owner_or_marker_unavailable")}
        let a=phase.ack.ok_or("owner_ack_unavailable")?;
        if row["owner"]["status"]!="observed" || row["owner"]["inventory_complete"]!=true {return Err("owner_graph_unavailable")}
        let nodes:Vec<_>=row["owner"]["nodes"].as_array().ok_or("owner_node_rows_unavailable")?.iter()
            .filter_map(|n|n["node"].as_u64().and_then(|n|u32::try_from(n).ok())).collect();
        let(records,compiled,mixes)=graph_worker_ir(owner,&config.query,&nodes)?;
        let before=core.uvi_diagnostic_progress(0,owner.request,bridge).map_err(|_|"owner_stale_before")?;
        let runtime=core.diagnostic_live_graph_runtime(0,owner.request,&config.query.player_nodes,&nodes,&mixes)?;
        let after=core.uvi_diagnostic_progress(0,owner.request,bridge).map_err(|_|"owner_stale_after")?;
        let agrees=|p:Option<DiagnosticProgress>|p.is_some_and(|p|p.events_executed==a.events_executed
            &&p.commands_collected==a.commands_collected&&p.commands_pushed==a.commands_pushed
            &&p.commands_drained==a.commands_pushed&&p.marker_drained>=a.request_id);
        let mut joins=Vec::new();
        for t in runtime["targets"].as_array().ok_or("core_target_rows_unavailable")? {
            let node=t["player_node"].as_u64();let subset:Vec<_>=records.iter().filter(|r|r["player_node"].as_u64()==node).cloned().collect();
            let expected=(node==Some(u64::from(config.expected_player_node))).then_some((config.expected_location_sha256.as_str(),config.expected_location_utf8_bytes));
            joins.push(json!({"kind":"current_target","player_node":node,"asset_id":t["current_pcm_revision_asset_id"],
                "join":graph_join(&subset,&t["matching_regions"],t["region_matches_complete"]==true,None,expected)}));
        }
        for v in runtime["voices"].as_array().ok_or("core_voice_rows_unavailable")? {
            let group=v["gain_state"]["group"].as_u64();
            let join=if group.is_some(){graph_join(&records,&v["matching_regions"],v["region_matches_complete"]==true,group,None)}
                else{json!({"status":"unavailable","reason":"voice_group_unavailable"})};
            joins.push(json!({"kind":"occupied_voice","physical":v["voice"]["voice"],"pinned_asset_id":v["voice"]["asset"],"join":join}));
        }
        Ok(json!({"runtime":runtime,"worker_recipes":records,"compiled_topology":compiled,"source_joins":joins,
            "prefix_counters_agree":agrees(before.progress)&&agrees(after.progress),
            "sample_counts_conserved":a.sample_counts_conserved(),"sample_tasks_settled":a.samples.pending==0,
            "ack_health_clean":a.runtime_faults==0 && a.runtime_budgets==0 && a.samples.failed==0 && a.samples.cancelled==0,
            "progress_before":before.progress.map(progress_value),"progress_after":after.progress.map(progress_value)}))
    };
    match observe(){
        Ok(observation)=>{
            let coherent=observation["ack_health_clean"]==true&&observation["prefix_counters_agree"]==true&&observation["sample_counts_conserved"]==true
                &&observation["sample_tasks_settled"]==true&&observation["runtime"]["voices_complete"]==true
                &&observation["compiled_topology"]["complete"]==true&&row["owner"]["complete"]==true;
            let joined=observation["source_joins"].as_array().is_some_and(|j|j.iter().all(|r|r["join"]["status"]=="joined"));
            row["status"]=json!(if coherent&&joined{"observed_finite_state"}else{"partial_or_incoherent"});
            if observation["compiled_topology"]["complete"]!=true {
                row["reason"]=json!("compiled_route_linkage_unavailable");
            }
            row["core"]=observation;
        },
        Err(reason)=>{row["reason"]=json!(reason);},
    }
    captures.push(row);
}
fn request_graph_capture(
    core:&mut V2Core,owner:&mut SwitchOwner,bridge:&Arc<UiBridge>,bounds:&Bounds,service:&mut Service,
    config:&LiveGraphConfig,label:&str,frames:usize,captures:&mut GraphCaptures,
){
    if captures.rows.len()==8 {captures.omitted+=1;return}
    let mut phase=acknowledge_query(core,owner,bridge,bounds,bounds.phase_deadline(),service,Some(&config.query));
    capture_graph(core,owner,bridge,config,&mut phase,label,frames,captures);
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
    // Allocate the complete finite PCM budget before any note or render.
    let mut output_windows = OutputWindowCapture::new(&config.output_windows);
    let mut issues = Vec::<String>::new();
    let mut graph_captures = GraphCaptures::default();
    let mut next_graph_checkpoint = 0usize;
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
        let mut initial = acknowledge_query(
            core,
            owner,
            &bridge,
            &bounds,
            bounds.phase_deadline(),
            &mut service,
            config.live_graph.as_ref().map(|g|&g.query),
        );
        let mut proceed = initial.drained();
        if let Some(g)=&config.live_graph {
            capture_graph(core,owner,&bridge,g,&mut initial,"installed_initial_ack",0,&mut graph_captures);
        }
        result["initial_owner_sample_condition"] = initial.report;
        if !config.gain_groups.is_empty() {
            result["gain_after_initial_ack"]=switch_gain_state(core,owner,&config.gain_groups);
        }
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
            let mut phase = acknowledge_query(
                core,
                owner,
                &bridge,
                &bounds,
                bounds.phase_deadline(),
                &mut service,
                config.live_graph.as_ref().map(|g|&g.query),
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
            if let Some(g)=&config.live_graph {
                capture_graph(core,owner,&bridge,g,&mut phase,"controller_ack",0,&mut graph_captures);
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
            let mut phase = acknowledge_query(
                core,
                owner,
                &bridge,
                &bounds,
                bounds.phase_deadline(),
                &mut service,
                config.live_graph.as_ref().map(|g|&g.query),
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
            if let Some(g)=&config.live_graph {
                capture_graph(core,owner,&bridge,g,&mut phase,"note_ack",0,&mut graph_captures);
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
            if !config.gain_groups.is_empty() {
                result["gain_before_pcm"]=switch_gain_state(core,owner,&config.gain_groups);
            }
            if let Some(g)=&config.live_graph {
                request_graph_capture(core,owner,&bridge,&bounds,&mut service,g,
                    "samples_settled_before_pcm",0,&mut graph_captures);
            }
            // A pending task is explicit partial evidence. Ordinary clock
            // advancement may still expose authored future child activity.
            while audio.frames < note_frames && Instant::now() < bounds.deadline {
                if let Err(e) = core.uvi_diagnostic_progress(0, owner.request, &bridge) {
                    issues.push(format!("pcm_owner_unavailable:{e:?}"));
                    break;
                }
                let mut n = (note_frames - audio.frames).min(128);
                if let Some(checkpoint)=config.live_graph.as_ref().and_then(|g|g.held_checkpoints.get(next_graph_checkpoint)) {
                    n=n.min(checkpoint.saturating_sub(audio.frames));
                }
                if n==0 { issues.push("live_graph_checkpoint_not_advanced".into()); break; }
                let observer = (!config.output_windows.is_empty()).then_some(&mut output_windows);
                let block = match switch_render_observed(core, owner, n, observer) {
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
                if let Some(g)=&config.live_graph {
                    if g.held_checkpoints.get(next_graph_checkpoint)==Some(&audio.frames) {
                        request_graph_capture(core,owner,&bridge,&bounds,&mut service,g,
                            "held_checkpoint",audio.frames,&mut graph_captures);
                        next_graph_checkpoint+=1;
                    }
                }
                if audio.frames < note_frames {
                    bounds.pause(config.pace_ms, bounds.deadline);
                }
            }
        }
        if Instant::now() < bounds.deadline {
            let mut final_phase = acknowledge_query(
                core,
                owner,
                &bridge,
                &bounds,
                bounds.phase_deadline(),
                &mut service,
                config.live_graph.as_ref().map(|g|&g.query),
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
            if let Some(g)=&config.live_graph {
                capture_graph(core,owner,&bridge,g,&mut final_phase,"final_ack",audio.frames,&mut graph_captures);
            }
            result["final_owner_sample_condition"] = final_phase.report;
        } else {
            issues.push("total_wall_limit".into());
            result["final_owner_sample_condition"] =
                json!({"status":"total_wall_limit","observed":false});
        }
    }
    if !config.gain_groups.is_empty() {
        result["gain_after_pcm"]=switch_gain_state(core,owner,&config.gain_groups);
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
    let graph_complete = config.live_graph.as_ref().map(|g| {
        let expected=4+usize::from(!config.controllers.is_empty())+g.held_checkpoints.len();
        graph_captures.omitted==0 && graph_captures.rows.len()==expected
            && graph_captures.rows.iter().all(|r|r["status"]=="observed_finite_state")
    });
    if graph_complete==Some(false) {issues.push("live_graph_observation_incomplete".into());}
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
    if !config.output_windows.is_empty() {
        result["output_windows"] = output_windows.finish();
    }
    result["first_signal"] = json!(audio.first_signal);
    result["runtime_problems"] = json!(problems);
    if config.live_graph.is_some() {
        result["live_graph"] = json!({"schema_version":1,"complete":graph_complete,
            "status":if graph_complete==Some(true){"observed_finite_states"}else{"partial"},"phases":graph_captures.rows,
            "serialized_phase_bytes":graph_captures.bytes,"omitted":graph_captures.omitted,
            "admission":"not_decided; finite observed state does not prove native scope or all future states"});
    }
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
            output_windows: vec![],
            gain_groups: vec![],
            live_graph: None,
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
        assert!(report.get("output_windows").is_none(), "default remains disabled");
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


    fn output_window(start: usize, end: usize) -> OutputWindow {
        OutputWindow { start_frame:start,end_frame:end,output_pair:0,min_lag:16,max_lag:128,max_peaks:4 }
    }

    #[test]
    fn output_window_limits_are_checked_before_loading_and_do_not_enable_by_default() {
        let base=json!({"phase_timeout_ms":100,"total_timeout_ms":1000,"pace_ms":1});
        let disabled:Config=serde_json::from_value(base.clone()).unwrap();
        assert!(disabled.output_windows.is_empty());
        let valid=output_window(1,12001);
        assert!(validate_output_windows(std::slice::from_ref(&valid),96000).is_ok());
        assert!(validate_output_windows(&vec![valid.clone();9],96000).is_err());
        assert!(validate_output_windows(&vec![output_window(0,20000);5],96000).is_err());
        let mut too_much_work=output_window(0,96000);too_much_work.max_lag=2048;
        assert!(validate_output_windows(&[too_much_work],96000).is_err());
        for (field,value) in [("start_frame",json!(12001)),("end_frame",json!(96001)),
            ("output_pair",json!(crate::sound::BUSES)),("min_lag",json!(0)),
            ("max_lag",json!(2049)),("max_peaks",json!(0)),("max_peaks",json!(9))] {
            let mut row=json!({"start_frame":1,"end_frame":12001,"output_pair":0,"min_lag":16,"max_lag":128,"max_peaks":4});
            row[field]=value;
            let window:OutputWindow=serde_json::from_value(row).unwrap();
            assert!(validate_output_windows(&[window],96000).is_err(),"{field}");
        }
        let mut unknown=base;
        unknown["output_windows"]=json!([{"start_frame":0,"end_frame":12000,"output_pair":0,"min_lag":16,"max_lag":128,"max_peaks":4,"dump_pcm":true}]);
        assert!(serde_json::from_value::<Config>(unknown).is_err());
        assert!(validate_output_windows(&[output_window(usize::MAX,0)],96000).is_err());
    }

    #[test]
    fn exact_output_intersections_preserve_pair_channel_and_preallocated_storage() {
        let mut w=output_window(3,14);w.min_lag=1;w.max_lag=3;w.output_pair=2;
        assert!(validate_output_windows(std::slice::from_ref(&w),128).is_ok());
        let mut capture=OutputWindowCapture::new(&[w]);
        let capacity=capture.windows[0].samples.capacity();
        let pointer=capture.windows[0].samples.as_ptr();
        let mut buses=[[[0.;crate::sound::MAX_BLOCK];2];crate::sound::BUSES];
        for begin in [0,8] {
            for i in 0..8 {let value=(begin+i) as f32;buses[2][0][i]=value;buses[2][1][i]=-value;}
            capture.observe(&buses,8);
        }
        assert_eq!(capture.windows[0].samples,(3..14).map(|i|[i as f32,-(i as f32)]).collect::<Vec<_>>());
        assert_eq!(capture.windows[0].samples.capacity(),capacity);
        assert_eq!(capture.windows[0].samples.as_ptr(),pointer);
        let report=capture.finish();let row=&report["windows"][0];
        assert_eq!(row["frames"],11);assert_eq!(row["complete"],true);
        assert_eq!(row["channels"][0]["dc"],8.);assert_eq!(row["channels"][1]["dc"],-8.);
        assert_eq!(row["channels"][0]["ac_rms"],row["channels"][1]["ac_rms"]);
        assert!(row.get("samples").is_none());
    }

    #[test]
    fn output_metrics_separate_dc_from_tone_and_preserve_antiphase_pitch() {
        let w=output_window(0,12000);
        let tone:Vec<_>=(0..12000).map(|i| {
            let s=(i as f64*480.*std::f64::consts::TAU/48000.).sin() as f32*0.25;
            [s+0.125,-s-0.125]
        }).collect();
        let left=output_channel_metrics(&tone,0,&w);let right=output_channel_metrics(&tone,1,&w);
        assert!((left["dc"].as_f64().unwrap()-0.125).abs()<1e-7);
        assert!((left["ac_rms"].as_f64().unwrap()-0.25/2f64.sqrt()).abs()<1e-7);
        assert_eq!(left["ac_rms"],right["ac_rms"]);
        for channel in [&left,&right] {
            assert!(channel["periodicity"]["peaks"].as_array().unwrap().iter().any(|p|
                (p["hz"].as_f64().unwrap()-480.).abs()<0.1 && p["normalized_correlation"].as_f64().unwrap()>0.999));
        }
        let dc=output_channel_metrics(&vec![[0.25;2];12000],0,&w);
        assert_eq!(dc["ac_rms"],0.);assert_eq!(dc["periodicity"]["status"],"no_ac_energy");
        let silence=output_channel_metrics(&vec![[0.;2];12000],0,&w);
        assert!(silence["rms_dbfs"].is_null());assert_eq!(silence["peak"],0.);
        let partial=output_channel_metrics(&tone[..128],0,&w);
        assert_eq!(partial["periodicity"]["status"],"insufficient_observed_frames");
    }

    #[test]
    fn paced_authored_note_reports_early_silence_and_later_body_in_distinct_windows() {
        let (mut core,mut owner)=fixture(
            "function onNote(e) wait(20); playNote(e.note,e.velocity) end",(),ScriptConfig::default());
        let mut cfg=config(1000);
        let mut early=output_window(0,128);early.min_lag=1;early.max_lag=16;
        cfg.output_windows=vec![early,output_window(1024,2048)];
        let report=probe_note(&mut core,&mut owner,&cfg,note(),64,2048).unwrap();
        assert_eq!(report["status"],"observed","{report}");
        let windows=report["output_windows"]["windows"].as_array().unwrap();
        assert_eq!(windows.len(),2);assert_eq!(windows[0]["complete"],true);assert_eq!(windows[1]["complete"],true);
        assert_eq!(windows[0]["channels"][0]["peak"],0.);
        assert!(windows[1]["channels"][0]["rms"].as_f64().unwrap()>0.01);
        assert_eq!(report["output_windows"]["pcm_exported"],false);
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
        assert!(valid.validate(MAX_NOTE_FRAMES + 1).is_err());
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
        for field in ["gain_after_initial_ack","gain_before_pcm","gain_after_pcm"] {
            assert!(report.get(field).is_none(),"unexpected opt-in readback: {field}");
        }
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

#[cfg(test)]
mod gain_readback_tests {
    use super::*;
    #[test]
    fn gain_readback_is_default_off_and_rejects_unbounded_or_duplicate_indices() {
        let make=|extra:Value| {
            let mut value=json!({"phase_timeout_ms":100,"total_timeout_ms":200,"pace_ms":1});
            if !extra.is_null() {value["gain_groups"]=extra;}
            serde_json::from_value::<Config>(value).unwrap()
        };
        assert!(make(Value::Null).gain_groups.is_empty());
        assert!(make(json!([123,334])).validate(96000).is_ok());
        for invalid in [json!([123,123]),json!([65536]),json!([0,1,2,3,4,5,6,7,8])] {
            assert!(make(invalid).validate(96000).is_err());
        }
    }
}

#[cfg(test)]
mod held_note_bound_tests {
    use super::*;

    #[test]
    fn six_second_uvi_observation_keeps_independent_capture_and_work_caps() {
        let mut config: Config = serde_json::from_value(json!({
            "phase_timeout_ms":3000,"total_timeout_ms":10000,"pace_ms":1,
        })).unwrap();
        assert!(config.validate(MAX_NOTE_FRAMES).is_ok());
        assert!(config.validate(MAX_NOTE_FRAMES + 1).is_err());
        assert!(config.validate(127).is_err());
        let window = |start,end| OutputWindow {
            start_frame:start,end_frame:end,output_pair:0,
            min_lag:16,max_lag:800,max_peaks:8,
        };
        config.output_windows = vec![window(0,6000),window(180000,192000),
            window(228000,240000),window(276000,288000)];
        assert!(config.validate(MAX_NOTE_FRAMES).is_ok());
        assert!(config.validate(96000).is_err());
        config.output_windows = vec![window(0,96000),window(192000,288000)];
        assert!(config.validate(MAX_NOTE_FRAMES).is_err());
        assert_eq!(OUTPUT_WINDOW_FRAMES,96000);
        assert_eq!(OUTPUT_WINDOW_CORRELATION_WORK,128_000_000);
    }
    #[test]
    fn live_graph_config_bounds_and_default_off() {
        let mut c:Config=serde_json::from_value(json!({
            "phase_timeout_ms":200,"total_timeout_ms":2000,"pace_ms":1,
        })).unwrap();assert!(c.live_graph.is_none());
        c.live_graph=Some(LiveGraphConfig{query:sampler_uvi::script::LiveGraphQuery{
            keygroup_node:1,matrix_node:2,player_nodes:vec![3,4,5]},expected_player_node:3,
            expected_location_sha256:"a".repeat(64),expected_location_utf8_bytes:60,held_checkpoints:vec![48000,240000]});
        assert!(c.validate(288000).is_ok());assert!(c.validate(96000).is_err());
        c.live_graph.as_mut().unwrap().held_checkpoints=vec![128,128];assert!(c.validate(288000).is_err());
        c.live_graph.as_mut().unwrap().held_checkpoints=vec![];c.live_graph.as_mut().unwrap().expected_location_sha256="../path".into();
        assert!(c.validate(288000).is_err());
    }
    #[test]
    fn live_graph_join_never_guesses_missing_ambiguous_or_wrong_resource() {
        let row=json!({"region":1,"worker_group":7,"location_sha256":"a","location_utf8_bytes":60});
        assert_eq!(graph_join(&[row.clone()],&json!([1]),true,Some(7),Some(("a",60)))["status"],"joined");
        assert_eq!(graph_join(&[row.clone()],&json!([1]),false,Some(7),None)["status"],"refused");
        assert_eq!(graph_join(&[row.clone()],&json!([]),true,Some(7),None)["status"],"unavailable");
        assert_eq!(graph_join(&[row.clone(),row.clone()],&json!([1]),true,Some(7),None)["status"],"ambiguous");
        assert_eq!(graph_join(&[row.clone()],&json!([1]),true,Some(8),None)["status"],"unavailable");
        assert_eq!(graph_join(&[row],&json!([1]),true,Some(7),Some(("b",60)))["reason"],"resource_identity_mismatch");
    }
    #[test]
    fn live_graph_receipt_storage_is_bounded_and_overflow_is_explicit() {
        let mut captures=GraphCaptures::default();
        captures.push(json!({"phase":"fixture","numeric":vec![0u8;300000]}));
        assert_eq!(captures.rows[0]["reason"],"phase_receipt_size");
        for _ in 0..8 {captures.push(json!({"phase":"fixture"}));}
        assert_eq!(captures.rows.len(),8);assert_eq!(captures.omitted,1);assert!(captures.bytes<=(2<<20));
    }

    #[test]
    fn live_graph_asset_location_path_is_exact_and_nonpath_never_joins() {
        let path=sampler_ir::AssetLocation::Path("exact/path".into());
        assert_eq!(graph_location_bytes(&path),Some(&b"exact/path"[..]));
        let table=sampler_ir::AssetLocation::KontaktFile{id:41};
        assert!(graph_location_bytes(&table).is_none());
        let record=json!({"region":1,"worker_group":7,"location_sha256":null,"location_utf8_bytes":null});
        assert_eq!(graph_join(&[record],&json!([1]),true,Some(7),None)["reason"],"unsupported_or_missing_asset_location");
    }
    #[test]
    fn live_graph_chain_inventory_cannot_claim_unprojected_send_linkage_complete() {
        let mut ir=sampler_ir::Instrument::default();
        ir.groups.push(sampler_ir::Group::default());
        ir.buses.push(sampler_ir::Bus{name:"authored route fixture".into(),chain:None,sends:vec![],
            output:sampler_ir::Output::Master,gain:sampler_ir::Gain::Linear(1.)});
        ir.assets.push(sampler_ir::Asset{
            location:sampler_ir::AssetLocation::Path("authored/fixture.wav".into()),
            encoding:sampler_ir::Encoding::Wav,residency:sampler_ir::AssetResidency::Resident,
            root_key:Some(60),loops:vec![],
        });
        let mut zone=sampler_ir::Zone::new(sampler_ir::AssetRef(0));
        zone.group=Some(sampler_ir::GroupRef(0));ir.zones.push(zone);
        ir.source_indices.sample_targets.push(sampler_ir::SourceSampleTarget{node:3,zones:vec![sampler_ir::ZoneRef(0)]});
        let q=sampler_uvi::script::LiveGraphQuery{keygroup_node:1,matrix_node:2,player_nodes:vec![3]};
        for pre_fader in [false,true] {
            ir.groups[0].sends=vec![sampler_ir::GroupSend{to:sampler_ir::BusRef(0),gain:sampler_ir::Gain::Linear(1.),pre_fader}];
            let owner=SwitchOwner{instrument:Some(Arc::new(ir.clone())),..Default::default()};
            let(_,topology,_)=graph_worker_ir(&owner,&q,&[]).unwrap();
            assert_eq!(topology["inventory_complete"],true);assert_eq!(topology["complete"],false);
            assert_eq!(topology["route_linkage"]["reason"],"compiled_route_linkage_unavailable");
        }
    }

}
