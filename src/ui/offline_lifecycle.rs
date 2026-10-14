//! Explicit production plan-activation witness, without a plugin or audio device.
use crate::sound::{
    BlockInfo, Core, CoreLoader, LoadRequest, Streaming,
    v2::{V2Core, V2Loader},
};
use sampler_ui_ir::{Binding, ControlId, Interface};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub struct Witness {
    pub initial: Vec<Interface>,
    pub settled: Vec<Interface>,
    pub initial_values: Vec<(ControlId, f64)>,
    pub settled_values: Vec<(ControlId, f64)>,
    pub resource_path: PathBuf,
    pub report: Value,
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
    while frames < 96_000 && step_started.elapsed() < Duration::from_secs(5) && effects <= 65_536 {
        match core.service_ui_read_only(0, views) {
            Ok(count) => effects += count,
            Err(error) => {
                service_failure = Some(format!("{:?}", error.kind()));
                break;
            }
        }
        if !core.scripts_running(0) {
            break;
        }
        let mut progress = core.ui_lifecycle_progress(0);
        classify_callbacks(&mut progress, views);
        observe_listener_completions(&progress, &mut completed_listeners);
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

    fn fixture(body: &str) -> (V2Core, Vec<sampler_ksp::ScriptView>) {
        let source = format!(
            "on init make_perfview declare ui_label $drawer(1,1) move_control_px($drawer,10,20) end on on persistence_changed {body} end on"
        );
        let script =
            sampler_ksp::compile(&source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        install(vec![script])
    }

    fn install(scripts: Vec<sampler_ksp::Script>) -> (V2Core, Vec<sampler_ksp::ScriptView>) {
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
        let mut core = V2Core::with_parts(1, 48_000.);
        core.install(0, Some(Box::new(part)));
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
