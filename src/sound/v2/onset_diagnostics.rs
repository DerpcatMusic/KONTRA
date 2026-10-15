//! Explicit shots-only Kontakt onset capture. No library-specific branches.
//! All observations and reductions execute between production render calls.
mod capture;
mod runway;
pub mod config;
#[cfg(test)]
mod tests;

use super::{Part, PrepareOption, V2Core, V2Loader};
use crate::sound::{
    BlockInfo, Core, CoreError, LoadRequest, Loaded, ScriptUi, Streaming, event::Event,
};
use config::{Config, Settlement};
use sampler_core::{ControlValue, PlanId, ScriptInstanceId};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

type Load = Loaded<Option<Box<Part>>>;

impl V2Loader {
    fn prepare_onset(
        &self,
        request: &LoadRequest,
        capacity: usize,
        scope: sampler_core::trace::SignalTraceScope,
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(Load, sampler_core::trace::TraceReader), CoreError> {
        let (loaded, reader) = self.prepare_waiting(
            request,
            &mut |_| {},
            canceled,
            None,
            PrepareOption::SignalTrace { capacity, scope },
        )?;
        Ok((
            loaded,
            reader.ok_or_else(|| CoreError::Invalid("explicit trace reader absent".into()))?,
        ))
    }
}

struct Owner {
    epoch: u64,
    plan: PlanId,
    reader: sampler_core::trace::TraceReader,
    canceled: bool,
}
impl Owner {
    fn check<'a>(&mut self, core: &'a V2Core) -> Result<&'a Part, &'static str> {
        let part = core.parts.first().and_then(Option::as_deref);
        if self.canceled
            || part.is_none_or(|p| p.epoch != self.epoch || p.runtime.active_plan() != self.plan)
        {
            self.canceled = true;
            return Err("Trace owner was canceled or replaced");
        }
        Ok(part.unwrap())
    }
    fn observe(
        &mut self,
        core: &V2Core,
        capture: &mut capture::Capture,
    ) -> Result<(), &'static str> {
        capture.observe_voices(&self.check(core)?.runtime)
    }
    fn drain(&mut self, core: &V2Core, capture: &mut capture::Capture) -> Result<(), &'static str> {
        self.check(core)?;
        capture.drain(&self.reader)
    }
}

fn numeric(value: ControlValue) -> Value {
    match value {
        ControlValue::Integer(v) => json!([0, v]),
        ControlValue::Real(v) => json!([1, v]),
        ControlValue::Toggle(v) => json!([2, u8::from(v)]),
    }
}

/// Read current values, never defaults. Names, texts and resource paths are omitted.
fn state(part: &Part) -> Result<Value, &'static str> {
    let r = &part.runtime;
    let plan = r.active_plan();
    let performance = r
        .performance(0)
        .map_err(|_| "Performance state is absent")?;
    let mut input = Vec::with_capacity(128);
    let mut effective = Vec::with_capacity(128);
    for cc in 0..128 {
        input.push(
            r.input_controller(performance, cc)
                .map_err(|_| "Input controller capture failed")?,
        );
        effective.push(
            r.controller(performance, cc)
                .map_err(|_| "Effective controller capture failed")?,
        );
    }
    let definitions = r
        .control_definitions(plan)
        .map_err(|_| "Control schema is stale")?;
    if definitions.len() > 65536 {
        return Err("Numeric control capture capacity exceeded");
    }
    let mut controls = Vec::with_capacity(definitions.len());
    for definition in definitions {
        let id = definition.id.0;
        controls.push(json!({"id":[(id >> 64) as u64,id as u64],
            "effective":numeric(r.control_value(plan,definition.id).map_err(|_| "Effective control read failed")?),
            "base":numeric(r.control_base_value(plan,definition.id).map_err(|_| "Base control read failed")?)}));
    }
    let residency = r
        .residency_progress(plan)
        .map_err(|_| "Residency owner is stale")?;
    if residency.groups_total > 65536 {
        return Err("Group capture capacity exceeded");
    }
    let mut groups = Vec::with_capacity(residency.groups_total as usize);
    for group in 0..residency.groups_total {
        let p = r
            .group_residency_progress(plan, group)
            .map_err(|_| "Group state capture failed")?;
        groups.push(
            json!({"group":p.group,"source_group":p.source_group,"loaded":p.loaded,
            "pending":p.pending,"revision":p.logical_revision,"assets":p.asset_count,
            "mute":null}),
        );
    }
    Ok(
        json!({"input_controllers":input,"effective_controllers":effective,"controls":controls,
        "groups":groups,"articulation":r.articulation(performance).map_err(|_| "Articulation read failed")?,
        "group_mute_state":null,"full_effective_start_routes":null}),
    )
}

#[derive(Serialize)]
struct CallbackProgress {
    program: usize,
    role: Option<&'static str>,
    listener_callback: Option<usize>,
    owner: &'static str,
    active_plan_owner: bool,
    pc: usize,
    yielded_at: Option<u64>,
    waiting: bool,
    outcome: &'static str,
}
struct Callbacks {
    completed_listeners: BTreeSet<usize>,
    finite: usize,
    strict_finite: usize,
    failures: usize,
    unclassified: usize,
    progress: Vec<CallbackProgress>,
    progress_limit: usize,
    progress_omitted: usize,
    observed_at: u64,
}
impl Default for Callbacks {
    fn default() -> Self {
        Self::with_limit(256)
    }
}
fn role(
    views: &[sampler_ksp::ScriptView],
    mut program: usize,
) -> Option<(&'static str, Option<usize>)> {
    let mut base = 0;
    for view in views {
        if program < view.program_count() {
            return Some((
                view.program_role(program)?,
                view.listener_callback(program).map(|p| p + base),
            ));
        }
        program -= view.program_count();
        base += view.program_count();
    }
    None
}
impl Callbacks {
    fn with_limit(limit: usize) -> Self {
        let limit = limit.min(256);
        Self {
            completed_listeners: BTreeSet::new(),
            finite: 0,
            strict_finite: 0,
            failures: 0,
            unclassified: 0,
            progress: Vec::with_capacity(limit),
            progress_limit: limit,
            progress_omitted: 0,
            observed_at: 0,
        }
    }
    fn report(&self) -> Value {
        json!({"finite":self.finite,"strict_finite":self.strict_finite,"unclassified":self.unclassified,"failures":self.failures,
            "completed_listener_programs":self.completed_listeners,
            "observed_at_frame":self.observed_at,"progress":self.progress,
            "progress_limit":self.progress_limit,"progress_omitted":self.progress_omitted,
            "scope":"post_render_before_terminal_owner_drain; owner class and active plan equality, no invented note identity"})
    }
    fn observe(&mut self, part: &Part, views: &[sampler_ksp::ScriptView]) {
        self.finite = 0;
        self.strict_finite = 0;
        self.unclassified = 0;
        self.progress.clear();
        self.progress_omitted = 0;
        self.observed_at = part.runtime.now();
        part.runtime.visit_behavior_progress(|p| {
            if p.outcome == Some(sampler_core::Outcome::Finished)
                && role(views, p.program).is_some_and(|(kind, _)| kind == "on listener")
            {
                self.completed_listeners.insert(p.program);
            }
        });
        part.runtime.visit_behavior_progress(|p| {
            let retained_role = role(views, p.program);
            if self.progress.len() < self.progress_limit {
                self.progress.push(CallbackProgress {
                    program: p.program,
                    role: retained_role.map(|r| r.0),
                    listener_callback: retained_role.and_then(|r| r.1),
                    owner: if matches!(p.owner, sampler_core::BehaviorOwner::Plan(_)) {
                        "plan"
                    } else {
                        "note"
                    },
                    active_plan_owner: p.owner
                        == sampler_core::BehaviorOwner::Plan(part.runtime.active_plan()),
                    pc: p.pc,
                    yielded_at: p.yielded_at,
                    waiting: p.waiting,
                    outcome: match p.outcome {
                        None => "pending",
                        Some(sampler_core::Outcome::Finished) => "finished",
                        Some(sampler_core::Outcome::Cancelled) => "cancelled",
                        Some(sampler_core::Outcome::Fault(_)) => "fault",
                        Some(sampler_core::Outcome::FuelExhausted) => "fuel_exhausted",
                    },
                });
            } else {
                self.progress_omitted += 1;
            }
            self.failures += usize::from(matches!(
                p.outcome,
                Some(sampler_core::Outcome::Fault(_) | sampler_core::Outcome::FuelExhausted)
            ));
            if p.outcome.is_some() || !matches!(p.owner, sampler_core::BehaviorOwner::Plan(_)) {
                return;
            }
            let known_completed_driver = matches!(retained_role,
                Some(("listener driver", Some(callback))) if self.completed_listeners.contains(&callback));
            if !known_completed_driver {
                self.finite += 1;
                self.unclassified += usize::from(retained_role.is_none());
            }
            // A driver is background only while actually parked in this owner.
            // Exact queued Resume provenance is independently checked by core.
            if !known_completed_driver || p.owner != sampler_core::BehaviorOwner::Plan(part.runtime.active_plan())
                || !p.waiting || p.yielded_at.is_some() {
                self.strict_finite += 1;
            }
        });
    }
}

#[derive(Default, Serialize)]
struct SettlementEvidence {
    phase: &'static str,
    readiness_basis: &'static str,
    at_frame: u64,
    tick: u64,
    completed: bool,
    finite_work_quiet: bool,
    stable_tick_count: u8,
    numeric_state_stable_ticks: u8,
    numeric_state_comparisons: u64,
    numeric_state_sample_tick: Option<u64>,
    pending_state_sample_period_ticks: u64,
    numeric_state_change_ticks: u64,
    last_changed_state_domains: Vec<&'static str>,
    last_state_change_tick: Option<u64>,
    finite_callbacks: usize,
    unclassified_callbacks: usize,
    root_pending: u64,
    scheduled_work: Option<sampler_core::trace::onset::CommandProgress>,
    startup_pending: bool,
    residency_pending: u64,
    startup_completions_pending: u64,
    residency_accepted: u64,
    residency_consumed: u64,
    residency_committed: u64,
    residency_failed: u64,
    residency_stale: u64,
    residency_rejected: u64,
    completion_admission_failures: u64,
    queued_faults: u64,
    dropped_faults: u64,
}
fn changed_state_domains(previous: &Value, current: &Value) -> Vec<&'static str> {
    [
        "input_controllers",
        "effective_controllers",
        "controls",
        "groups",
        "articulation",
        "group_mute_state",
        "full_effective_start_routes",
    ]
    .into_iter()
    .filter(|domain| previous[*domain] != current[*domain])
    .collect()
}

fn selector(
    part: &mut Part,
    scripts: &ScriptUi,
    c: &Config,
    invoke: bool,
) -> Result<Value, &'static str> {
    let s = &c.selection;
    let mut views = scripts
        .views
        .iter()
        .enumerate()
        .filter(|(_, v)| v.slot() == s.slot);
    let (instance, view) = views.next().ok_or("Selector script slot is absent")?;
    if views.next().is_some() {
        return Err("Selector script slot is ambiguous");
    }
    let widget = view
        .model()
        .interface
        .widgets
        .get(s.selector_ordinal)
        .ok_or("Selector ordinal is absent")?;
    if widget.ui_id != s.selector_source {
        return Err("Initialized selector source identity does not match ordinal");
    }
    let plan = part.runtime.active_plan();
    let id = part
        .runtime
        .widget_id(plan, s.slot, s.selector_source)
        .map_err(|_| "Runtime selector source is absent")?;
    let definition = part
        .runtime
        .widget_definitions(plan)
        .map_err(|_| "Selector plan is stale")?
        .iter()
        .find(|w| w.id == id)
        .ok_or("Runtime selector definition is absent")?;
    if usize::from(definition.instance.0) != instance {
        return Err("Selector instance and source slot disagree");
    }
    let field = view
        .setup()
        .fields
        .iter()
        .find(|f| f.name == s.keyswitch_base_field)
        .ok_or("Initialized keyswitch-base field is absent; no saved-base fallback is permitted")?;
    if field.kind != sampler_ksp::setup::SetupValueKind::Integer {
        return Err("Keyswitch base is not an integer setup field");
    }
    let instance = ScriptInstanceId(
        instance
            .try_into()
            .map_err(|_| "Selector instance exceeds bound")?,
    );
    let base = match field.storage {
        sampler_ksp::model::Location::Cells { offset, len: 1 } => part
            .runtime
            .script_cell(plan, instance, offset)
            .map_err(|_| "Current keyswitch base read failed")?,
        sampler_ksp::model::Location::Control(id) => match part
            .runtime
            .control_value(plan, id)
            .map_err(|_| "Current keyswitch base control read failed")?
        {
            ControlValue::Integer(v) => v,
            _ => return Err("Current keyswitch base is not integer"),
        },
        _ => return Err("Keyswitch base is not a scalar numeric setup field"),
    };
    if !(0..=127).contains(&base)
        || base.checked_add(i64::from(s.articulation)) != Some(i64::from(s.keyswitch))
    {
        return Err("Current keyswitch base and declared articulation/key disagree");
    }
    if invoke {
        let performance = part
            .runtime
            .performance(0)
            .map_err(|_| "Selector performance is absent")?;
        let revision = part
            .runtime
            .control_revision(plan)
            .map_err(|_| "Selector revision is stale")?;
        part.runtime
            .invoke_widget(
                sampler_core::ControlContext {
                    performance,
                    origin: sampler_core::ChannelAddress {
                        protocol: sampler_core::Protocol::Native,
                        port: 0,
                        group: 0,
                        channel: 0,
                    },
                    channels: 1,
                },
                plan,
                Some(revision),
                &[sampler_core::WidgetEdit {
                    id,
                    index: 0,
                    value: sampler_core::WidgetValue::Integer(1),
                    interaction: Default::default(),
                }],
            )
            .map_err(|_| "Typed selector invocation failed")?;
    }
    let value = part
        .runtime
        .widget_value(plan, id, 0)
        .map_err(|_| "Current selector value read failed")?;
    let value = match value {
        sampler_core::WidgetValue::Integer(v) => json!([0, v]),
        sampler_core::WidgetValue::Real(v) => json!([1, v]),
        _ => return Err("Selector has no numeric value"),
    };
    Ok(
        json!({"slot":s.slot,"ordinal":s.selector_ordinal,"source":s.selector_source,"base":base,
        "keyswitch":s.keyswitch,"value":value}),
    )
}

struct Host {
    core: V2Core,
    scripts: ScriptUi,
    owner: Owner,
    capture: capture::Capture,
    callbacks: Callbacks,
    origin: Instant,
    frames: u64,
    tick: u64,
    pacing_failures: u64,
    max_late_us: u64,
    max_callback_us: u64,
    service_effects: u64,
    pending_event: Option<Event>,
    last_settlement: SettlementEvidence,
    runway: runway::Audit,
}
impl Host {
    fn block(&mut self, c: &Config, deadline: Instant) -> Result<(), &'static str> {
        self.owner.check(&self.core)?;
        let due = self.origin + Duration::from_secs_f64(self.frames as f64 / f64::from(c.rate));
        if due >= deadline || Instant::now() >= deadline {
            return Err("Diagnostic wall deadline exceeded");
        }
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        let late = Instant::now().saturating_duration_since(due);
        self.max_late_us = self
            .max_late_us
            .max(late.as_micros().min(u128::from(u64::MAX)) as u64);
        self.pacing_failures += u64::from(late > c.pacing_tolerance);
        let runway_start = self.owner.check(&self.core)?.runtime.now();
        let underruns_before = self.owner.check(&self.core)?.runtime.stream_underruns();
        let runway_before = self.runway.before(&self.owner.check(&self.core)?.runtime, c.block as u32)?;
        // This is the real production effect service; unsupported I/O is refused.
        self.service_effects +=
            self.core
                .service_ui_read_only(0, &mut self.scripts.views)
                .map_err(|_| "Read-only production UI service failed")? as u64;
        let at = Instant::now();
        self.core.begin_block(&BlockInfo {
            frames: c.block,
            offline: false,
            ..Default::default()
        });
        if let Some(event) = self.pending_event.take() {
            self.core.play(0, event);
        }
        {
            let output = self.core.render(c.block);
            for sample in output
                .buses
                .iter()
                .flat_map(|bus| bus.iter())
                .flat_map(|channel| &channel[..c.block])
            {
                if !sample.is_finite() {
                    return Err("Nonfinite production output");
                }
            }
        }
        let callback_elapsed = at.elapsed();
        self.max_callback_us = self
            .max_callback_us
            .max(callback_elapsed.as_micros().min(u128::from(u64::MAX)) as u64);
        self.runway.record(&self.owner.check(&self.core)?.runtime, runway_start, c.block as u32,
            underruns_before, late, c.pacing_tolerance, callback_elapsed, runway_before)?;
        self.callbacks
            .observe(self.owner.check(&self.core)?, &self.scripts.views);
        self.owner.drain(&self.core, &mut self.capture)?;
        self.owner.observe(&self.core, &mut self.capture)?;
        self.core.end_block(c.block, &mut |_| true);
        self.service_effects +=
            self.core
                .service_ui_read_only(0, &mut self.scripts.views)
                .map_err(|_| "Read-only production UI service failed")? as u64;
        self.frames += c.block as u64;
        self.tick += 1;
        Ok(())
    }
    fn settle(
        &mut self,
        c: &Config,
        deadline: Instant,
        phase: &'static str,
    ) -> Result<Value, &'static str> {
        let mut settlement = Settlement::default();
        let mut previous_numeric: Option<Value> = None;
        let mut previous_basis: Option<Value> = None;
        let mut previous_sampled_state: Option<Value> = None;
        self.last_settlement = SettlementEvidence {
            phase,
            readiness_basis: if c.natural_revisit_first {
                "finite_activation_exact_future_listener_timers_and_stable_group_articulation"
            } else {
                "finite_activation_and_stable_numeric_state"
            },
            pending_state_sample_period_ticks: 128,
            ..Default::default()
        };
        loop {
            self.block(c, deadline)?;
            let part = self.owner.check(&self.core)?;
            let residency = part
                .runtime
                .residency_progress(self.owner.plan)
                .map_err(|_| "Residency state is stale")?;
            let startup = part
                .runtime
                .startup_work_pending(self.owner.plan)
                .map_err(|_| "Startup owner is stale")?;
            let (faults, dropped) = part.fault_inbox.counts();
            let scheduled_work = part
                .runtime
                .onset_command_progress(self.owner.plan, c.row_capacity)
                .map_err(|_| "Scheduled-work owner is stale")?;
            let commands_quiet = if c.natural_revisit_first {
                scheduled_work.non_background_complete()
            } else {
                part.runtime.pending_commands() == 0
            };
            let finite_callbacks = if c.natural_revisit_first {
                self.callbacks.strict_finite
            } else {
                self.callbacks.finite
            };
            let quiet = finite_callbacks == 0
                && !startup
                && commands_quiet
                && residency.pending == 0
                && residency.startup_completions_pending == 0
                && (!c.natural_revisit_first || self.callbacks.progress_omitted == 0);
            let evidence = &mut self.last_settlement;
            evidence.at_frame = part.runtime.now();
            evidence.tick = self.tick;
            evidence.finite_work_quiet = quiet;
            evidence.finite_callbacks = finite_callbacks;
            evidence.unclassified_callbacks = self.callbacks.unclassified;
            evidence.root_pending = part.runtime.pending_commands() as u64;
            evidence.scheduled_work = Some(scheduled_work);
            evidence.startup_pending = startup;
            evidence.residency_pending = residency.pending as u64;
            evidence.startup_completions_pending = residency.startup_completions_pending as u64;
            evidence.residency_accepted = residency.accepted;
            evidence.residency_consumed = residency.consumed;
            evidence.residency_committed = residency.committed;
            evidence.residency_failed = residency.failed;
            evidence.residency_stale = residency.stale;
            evidence.residency_rejected = residency.rejected;
            evidence.completion_admission_failures = residency.completion_admission_failures;
            evidence.queued_faults = faults as u64;
            evidence.dropped_faults = dropped as u64;
            if self.callbacks.failures != 0
                || residency.failed != 0
                || residency.rejected != 0
                || residency.completion_admission_failures != 0
                || faults != 0
                || dropped != 0
            {
                return Err("Production startup/callback/residency failure");
            }
            let actual = if quiet { state(part)? } else { Value::Null };
            evidence.numeric_state_stable_ticks = if !quiet {
                0
            } else if previous_numeric.as_ref() == Some(&actual) {
                evidence.numeric_state_stable_ticks.saturating_add(1)
            } else {
                1
            };
            // Sample numeric domains while blocked, too, without cloning the
            // entire large-instrument state at audio-block frequency. These are
            // observed sample ticks, never proof of stability between samples.
            let sampled = if quiet {
                Some(actual.clone())
            } else if previous_sampled_state.is_none() || self.tick % 128 == 0 {
                Some(state(part)?)
            } else {
                None
            };
            if let Some(sampled) = sampled {
                evidence.numeric_state_sample_tick = Some(self.tick);
                if let Some(previous) = &previous_sampled_state {
                    evidence.numeric_state_comparisons += 1;
                    let changed = changed_state_domains(previous, &sampled);
                    if !changed.is_empty() {
                        evidence.numeric_state_change_ticks += 1;
                        evidence.last_changed_state_domains = changed;
                        evidence.last_state_change_tick = Some(self.tick);
                    }
                }
                previous_sampled_state = Some(sampled);
            }
            // Opt-in natural capture allows authored continuous modulation after
            // finite activation, exact command provenance and group/articulation
            // stability. Full numeric state is still recorded and must match for
            // a verified cold/warm comparison. Legacy exact mode is unchanged.
            let basis = if quiet && c.natural_revisit_first {
                json!([actual["groups"], actual["articulation"]])
            } else {
                actual.clone()
            };
            evidence.stable_tick_count = if !quiet {
                0
            } else if previous_basis.as_ref() == Some(&basis) {
                evidence.stable_tick_count.saturating_add(1)
            } else {
                1
            };
            previous_numeric = Some(actual.clone());
            previous_basis = Some(basis.clone());
            if settlement.observe(self.tick, basis, quiet)? {
                evidence.completed = true;
                return Ok(
                    json!({"observed_quiet_ticks":3,"tick":self.tick,"state":actual,
                    "root_pending":part.runtime.pending_commands(),"startup_pending":startup,
                    "finite_callbacks":self.callbacks.finite,"residency_pending":residency.pending,
                    "startup_completions_pending":residency.startup_completions_pending,
                    "stream_worker_pending":null,"completed_listener_programs":self.callbacks.completed_listeners,
                    "readiness_basis":evidence.readiness_basis,
                    "numeric_state_stable_ticks":evidence.numeric_state_stable_ticks,
                    "last_changed_state_domains":evidence.last_changed_state_domains,
                    "full_numeric_state_stability_required":!c.natural_revisit_first}),
                );
            }
        }
    }
}

/// Opt-in saved-state experiment only. The CLI must enforce a separate outer
/// process timeout: synchronous loader I/O and interpreter work cannot be killed
/// by this cooperative between-block deadline.
pub fn run(plan: &Value) -> Result<Value, &'static str> {
    let c = Config::parse(plan)?;
    let started = Instant::now();
    let deadline = started + c.total_deadline;
    let request = LoadRequest {
        path: c.path.clone(),
        sample_rate: f64::from(c.rate),
        streaming: Streaming::Auto,
        ..Default::default()
    };
    let startup_deadline = started + c.startup_deadline;
    let (mut loaded, reader) = V2Loader
        .prepare_onset(&request, c.row_capacity, c.graph_scope, &|| {
            Instant::now() >= startup_deadline
        })
        .map_err(|_| "Production Kontakt loader failed; authored payload omitted")?;
    if Instant::now() >= startup_deadline {
        return Err("Loader exceeded the startup deadline");
    }
    let mut part = loaded
        .part
        .take()
        .ok_or("Production loader returned no owner")?;
    let loader_runway = runway::loader(&part, loaded.report.stream_startup.as_ref());
    // Epoch belongs to this fresh diagnostic host; PlanId comes from Runtime.
    part.epoch = 1;
    let owner = Owner {
        epoch: part.epoch,
        plan: part.runtime.active_plan(),
        reader,
        canceled: false,
    };
    let mut core = V2Core::with_parts(1, f64::from(c.rate));
    drop(core.install(0, Some(part)));
    let mut host = Host {
        core,
        scripts: loaded.scripts,
        owner,
        capture: capture::Capture::new(c.row_capacity),
        callbacks: Callbacks::with_limit(c.row_capacity),
        origin: Instant::now(),
        frames: 0,
        tick: 0,
        pacing_failures: 0,
        max_late_us: 0,
        max_callback_us: 0,
        service_effects: 0,
        pending_event: None,
        last_settlement: SettlementEvidence::default(),
        runway: runway::Audit::new(),
    };
    let result = capture_run(&mut host, &c, startup_deadline, deadline);
    let error = result.as_ref().err().copied();
    let details = result.unwrap_or(Value::Null);
    let final_drain_error = host.owner.drain(&host.core, &mut host.capture).err();
    let final_observation = host.owner.check(&host.core).and_then(state);
    let final_state_error = final_observation.as_ref().err().copied();
    let final_state = final_observation.ok();
    let summary = host.capture.report();
    let problems = host.core.problems(0);
    Ok(
        json!({"schema":1,"status":"partial","error":error,"path":c.path,
        "state_origin":"saved_state_diagnostic","input_origin":"synthetic_native",
        "execution_protocol":"midi1_via_core_play","raw_host_events":null,
        "rate":c.rate,"block":c.block,"key":c.key,"velocity":c.velocity,
        "hold_ms":c.hold.as_millis(),"spacing_ms":c.spacing.as_millis(),"frames":host.frames,
        "elapsed_ms":started.elapsed().as_secs_f64()*1000.,"details":details,
        "graph_scope":c.graph_scope,"capture":summary,"final_state":final_state,"final_state_error":final_state_error,
        "final_drain_error":final_drain_error,"runtime_problems":problems,
        "last_settlement":host.last_settlement,"last_callback_observation":host.callbacks.report(),
        "pacing_failures":host.pacing_failures,"max_lateness_us":host.max_late_us,
        "max_callback_us":host.max_callback_us,"ui_effects_serviced":host.service_effects,
        "loader_runway":loader_runway,"stream_runway":host.runway.report(),
        "missing_apis":["read_only_effective_group_mutes_and_complete_start_route_parameters",
            "complete_stream_worker_pending_and_completion_observation"],
        "native_parity":null,"user_cause":null,"source_acoustic_onset":null,
        "scope":"Numeric production-path diagnostic. Missing state proof prevents a complete or matched-native verdict."}),
    )
}

fn capture_run(
    host: &mut Host,
    c: &Config,
    startup_deadline: Instant,
    deadline: Instant,
) -> Result<Value, &'static str> {
    let startup = host.settle(c, startup_deadline, "startup")?;
    let selection_before = selector(
        host.core.parts[0].as_deref_mut().unwrap(),
        &host.scripts,
        c,
        true,
    )?;
    let selected = host.settle(
        c,
        (Instant::now() + c.phase_deadline).min(deadline),
        "post_selector",
    )?;
    let selection_after = selector(
        host.core.parts[0].as_deref_mut().unwrap(),
        &host.scripts,
        c,
        false,
    )?;
    if selection_after["value"] != json!([0, 1]) {
        return Err("Selector did not retain the requested numeric state");
    }
    // The declared key is independently checked against the current base above.
    // The selector invocation is sufficient; no redundant keyswitch is injected.
    let first_frame = host.frames;
    let spacing_frames = (c.spacing.as_secs_f64() * f64::from(c.rate)).round() as u64;
    let hold_frames = (c.hold.as_secs_f64() * f64::from(c.rate)).round() as u64;
    let tail_frames = (c.tail_deadline.as_secs_f64() * f64::from(c.rate)).round() as u64;
    let mut trials = Vec::new();
    let mut intervention = Value::Null;
    let mut first_state = None;
    let last_trial = u16::from(c.selection.rr_cycle) * if c.natural_revisit_first { 2 } else { 1 };
    let mut natural_readiness = Value::Null;
    for trial in 0..=last_trial {
        let due = first_frame + u64::from(trial) * spacing_frames;
        if trial == last_trial {
            intervention = host.capture.warm_first_windows(
                host.core.parts[0].as_deref_mut().unwrap(),
                host.owner.plan,
                true,
            )?;
        }
        while host.frames < due {
            host.block(c, deadline)?;
        }
        if trial == last_trial {
            intervention["before_attack"] = host.capture.warm_first_windows(
                host.core.parts[0].as_deref_mut().unwrap(),
                host.owner.plan,
                false,
            )?;
        }
        if c.natural_revisit_first && trial == u16::from(c.selection.rr_cycle) {
            natural_readiness = host.capture.inspect_first_windows(
                host.core.parts[0].as_deref_mut().unwrap(),
                host.owner.plan,
            )?;
        }
        let actual_state = state(host.owner.check(&host.core)?)?;
        let matches_first_state = first_state
            .as_ref()
            .is_none_or(|first| *first == actual_state);
        if first_state.is_none() {
            first_state = Some(actual_state);
        }
        let at = host.frames;
        let prior_births = host.capture.birth_count();
        let problems_before_attack = host.core.problems(0);
        let pacing_before_attack = host.pacing_failures;
        host.runway.attack(trial, at);
        host.pending_event = Some(Event::midi1(0x90, c.key, c.velocity));
        while host.frames < at + hold_frames {
            host.block(c, deadline)?;
        }
        host.pending_event = Some(Event::midi1(0x80, c.key, 0));
        trials.push(json!({"trial":trial,"note_on_frame":at,"note_off_frame":host.frames,
            "schedule_rounding_frames":at-due,"birth_start":prior_births,
            "birth_end":host.capture.birth_count(),"observed_numeric_state_matches_first":matches_first_state,
            "runtime_problems_before_attack":problems_before_attack,
            "runtime_problems_after_hold":host.core.problems(0),
            "pacing_failures_before_attack":pacing_before_attack,
            "pacing_failures_after_hold":host.pacing_failures}));
        if trial == 0 {
            host.capture.mark_first_trial(prior_births);
        }
    }
    let tail_end = host.frames + tail_frames;
    while host.frames < tail_end {
        host.block(c, deadline)?;
    }
    let part = host.owner.check(&host.core)?;
    let residency = part
        .runtime
        .residency_progress(host.owner.plan)
        .map_err(|_| "Tail residency owner is stale")?;
    let tail_scheduled_work = part
        .runtime
        .onset_command_progress(host.owner.plan, c.row_capacity)
        .map_err(|_| "Tail scheduled-work owner is stale")?;
    let tail_commands_quiet = if c.natural_revisit_first {
        tail_scheduled_work.non_background_complete()
    } else {
        part.runtime.pending_commands() == 0
    };
    let tail_finite = if c.natural_revisit_first {
        host.callbacks.strict_finite
    } else {
        host.callbacks.finite
    };
    let complete_tail = host.core.voices().active == 0
        && tail_finite == 0
        && tail_commands_quiet
        && residency.pending == 0
        && residency.startup_completions_pending == 0
        && !part
            .runtime
            .startup_work_pending(host.owner.plan)
            .map_err(|_| "Tail startup owner is stale")?;
    let mut matching = host.capture.compare_trials(&trials);
    let forced_numeric_match = trials
        .last()
        .is_some_and(|t| t["observed_numeric_state_matches_first"] == true);
    matching["observed_numeric_state_matches_first"] = json!(forced_numeric_match);
    matching["observed_selection_and_state_match"] = json!(
        matching["births_and_cursors_match"] == true
            && matching["observed_gain_and_signal_parameters_match"] == true
            && forced_numeric_match
    );
    let mut natural_matching = if c.natural_revisit_first {
        host.capture
            .compare_trials(&trials[..=usize::from(c.selection.rr_cycle)])
    } else {
        Value::Null
    };
    if c.natural_revisit_first {
        natural_matching["observed_numeric_state_matches_first"] = trials
            [usize::from(c.selection.rr_cycle)]["observed_numeric_state_matches_first"]
            .clone();
        natural_matching["observed_selection_and_state_match"] = json!(
            natural_matching["births_and_cursors_match"] == true
                && natural_matching["observed_gain_and_signal_parameters_match"] == true
                && natural_matching["observed_numeric_state_matches_first"] == true
        );
    }
    Ok(
        json!({"startup":startup,"post_selector":selected,"selector_before":selection_before,
        "selector_after":selection_after,"trials":trials,"warm_intervention":intervention,
        "first_trial_state":first_state,"birth_phase_coverage":host.capture.birth_phases(&trials),
        "natural_revisit_readiness":natural_readiness,
        "natural_observed_selection_match":natural_matching,
        "natural_revisit_trial":if c.natural_revisit_first { Some(c.selection.rr_cycle) } else { None },
        "forced_revisit_trial":last_trial,
        "exact_resolved_rr_revisit":matching["births_and_cursors_match"],
        "exact_asset_window_revisit_verified":matching["births_and_cursors_match"] == true
            && intervention["before_attack"]["verified_ready"] == true
            && (!c.natural_revisit_first || forced_numeric_match),
        "observed_selection_match":matching,"full_parameter_match":null,"complete_tail":complete_tail,
        "tail_scheduled_work":tail_scheduled_work,
        "tail_scope":"finite voice/callback/startup/residency work; exact parked listener timers remain explicit"}),
    )
}
