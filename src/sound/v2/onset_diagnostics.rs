//! Explicit shots-only Kontakt onset capture. No library-specific branches.
//! All observations and reductions execute between production render calls.
mod capture;
pub mod config;
#[cfg(test)]
mod tests;

use super::{Part, PrepareOption, V2Core, V2Loader};
use crate::sound::{
    BlockInfo, Core, CoreError, LoadRequest, Loaded, ScriptUi, Streaming, event::Event,
};
use config::{Config, Settlement};
use sampler_core::{ControlValue, PlanId, ScriptInstanceId};
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

#[derive(Default)]
struct Callbacks {
    completed_listeners: BTreeSet<usize>,
    finite: usize,
    failures: usize,
    unclassified: usize,
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
    fn observe(&mut self, part: &Part, views: &[sampler_ksp::ScriptView]) {
        self.finite = 0;
        self.unclassified = 0;
        part.runtime.visit_behavior_progress(|p| {
            if p.outcome == Some(sampler_core::Outcome::Finished)
                && role(views, p.program).is_some_and(|(kind, _)| kind == "on listener")
            {
                self.completed_listeners.insert(p.program);
            }
        });
        part.runtime.visit_behavior_progress(|p| {
            self.failures += usize::from(matches!(
                p.outcome,
                Some(sampler_core::Outcome::Fault(_) | sampler_core::Outcome::FuelExhausted)
            ));
            if p.outcome.is_some() || !matches!(p.owner, sampler_core::BehaviorOwner::Plan(_)) {
                return;
            }
            match role(views, p.program) {
                Some(("listener driver", Some(callback)))
                    if self.completed_listeners.contains(&callback) => {}
                Some(_) => self.finite += 1,
                None => {
                    self.unclassified += 1;
                    self.finite += 1;
                }
            }
        });
    }
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
        self.max_callback_us = self
            .max_callback_us
            .max(at.elapsed().as_micros().min(u128::from(u64::MAX)) as u64);
        self.callbacks
            .observe(self.owner.check(&self.core)?, &self.scripts.views);
        self.owner.drain(&self.core, &mut self.capture)?;
        self.core.end_block(c.block, &mut |_| true);
        self.service_effects +=
            self.core
                .service_ui_read_only(0, &mut self.scripts.views)
                .map_err(|_| "Read-only production UI service failed")? as u64;
        self.frames += c.block as u64;
        self.tick += 1;
        Ok(())
    }
    fn settle(&mut self, c: &Config, deadline: Instant) -> Result<Value, &'static str> {
        let mut settlement = Settlement::default();
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
            if self.callbacks.failures != 0
                || residency.failed != 0
                || residency.rejected != 0
                || residency.completion_admission_failures != 0
                || faults != 0
                || dropped != 0
            {
                return Err("Production startup/callback/residency failure");
            }
            let quiet = self.callbacks.finite == 0
                && !startup
                && part.runtime.pending_commands() == 0
                && residency.pending == 0
                && residency.startup_completions_pending == 0;
            let actual = if quiet { state(part)? } else { Value::Null };
            if settlement.observe(self.tick, actual.clone(), quiet)? {
                return Ok(
                    json!({"observed_quiet_ticks":3,"tick":self.tick,"state":actual,
                    "root_pending":part.runtime.pending_commands(),"startup_pending":startup,
                    "finite_callbacks":self.callbacks.finite,"residency_pending":residency.pending,
                    "startup_completions_pending":residency.startup_completions_pending,
                    "stream_worker_pending":null,"completed_listener_programs":self.callbacks.completed_listeners}),
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
        callbacks: Callbacks::default(),
        origin: Instant::now(),
        frames: 0,
        tick: 0,
        pacing_failures: 0,
        max_late_us: 0,
        max_callback_us: 0,
        service_effects: 0,
        pending_event: None,
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
        "pacing_failures":host.pacing_failures,"max_lateness_us":host.max_late_us,
        "max_callback_us":host.max_callback_us,"ui_effects_serviced":host.service_effects,
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
    let startup = host.settle(c, startup_deadline)?;
    let selection_before = selector(
        host.core.parts[0].as_deref_mut().unwrap(),
        &host.scripts,
        c,
        true,
    )?;
    let selected = host.settle(c, (Instant::now() + c.phase_deadline).min(deadline))?;
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
    for trial in 0..=c.selection.rr_cycle {
        let due = first_frame + u64::from(trial) * spacing_frames;
        if trial == c.selection.rr_cycle {
            intervention = host.capture.warm_first_windows(
                host.core.parts[0].as_deref_mut().unwrap(),
                host.owner.plan,
                true,
            )?;
        }
        while host.frames < due {
            host.block(c, deadline)?;
        }
        if trial == c.selection.rr_cycle {
            intervention["before_attack"] = host.capture.warm_first_windows(
                host.core.parts[0].as_deref_mut().unwrap(),
                host.owner.plan,
                false,
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
        host.pending_event = Some(Event::midi1(0x90, c.key, c.velocity));
        while host.frames < at + hold_frames {
            host.block(c, deadline)?;
        }
        host.pending_event = Some(Event::midi1(0x80, c.key, 0));
        trials.push(json!({"trial":trial,"note_on_frame":at,"note_off_frame":host.frames,
            "schedule_rounding_frames":at-due,"birth_start":prior_births,
            "birth_end":host.capture.birth_count(),"observed_numeric_state_matches_first":matches_first_state}));
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
    let complete_tail = host.core.voices().active == 0
        && host.callbacks.finite == 0
        && part.runtime.pending_commands() == 0
        && residency.pending == 0
        && residency.startup_completions_pending == 0
        && !part
            .runtime
            .startup_work_pending(host.owner.plan)
            .map_err(|_| "Tail startup owner is stale")?;
    let matching = host.capture.compare_trials(&trials);
    Ok(
        json!({"startup":startup,"post_selector":selected,"selector_before":selection_before,
        "selector_after":selection_after,"trials":trials,"warm_intervention":intervention,
        "first_trial_state":first_state,
        "exact_resolved_rr_revisit":matching["births_and_cursors_match"],
        "exact_asset_window_revisit_verified":matching["births_and_cursors_match"] == true
            && intervention["before_attack"]["verified_ready"] == true,
        "observed_selection_match":matching,"full_parameter_match":null,"complete_tail":complete_tail}),
    )
}
