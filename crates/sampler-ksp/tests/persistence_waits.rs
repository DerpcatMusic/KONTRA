//! Wait-capable load callbacks use the live scheduler, never static polling.
use sampler_core::{Limits, MidiCompletion, Outcome, Prepared, Runtime, ScriptInstanceId};
use sampler_ksp::{Environment, model::PersistenceCompletion};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn compile(source: &str) -> sampler_ksp::Script {
    let script = sampler_ksp::compile_with(
        source,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        &Environment {
            evaluation_budget: Some(64),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        script
            .warnings()
            .iter()
            .all(|warning| (warning.kind == sampler_ksp::Kind::Approximate
                && warning.builtin == Some("wait_ticks"))
                || (warning.kind == sampler_ksp::Kind::Warning
                    && warning.message
                        == "undeclared $ENGINE_PAR_CUTOFF treated as an opaque vendor constant")),
        "{:?}",
        script.warnings()
    );
    script
}

fn runtime(script: sampler_ksp::Script) -> Runtime {
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    Runtime::new(plan, limits).unwrap()
}

#[test]
fn persistence_waits_advance_the_clock_without_static_budget_warning_or_prefix_replay() {
    for source in [
        "on init declare $calls declare $done end on
         on persistence_changed inc($calls)
           while ($ENGINE_UPTIME < 10) wait(1000) end while
           inc($done) end on",
        "on init declare $calls declare $done end on
         function pause while ($ENGINE_UPTIME < 10) wait(1000) end while end function
         on persistence_changed inc($calls) call pause inc($done) end on",
        "on init declare $calls declare $done end on
         on persistence_changed inc($calls) wait_ticks(10) inc($done) end on",
        "on init declare $calls declare $done end on
         on persistence_changed inc($calls) if ($done = 0)
           select ($calls) case 1 wait(1000) end select end if inc($done) end on",
    ] {
        let script = compile(source);
        assert_eq!(
            script.model().persistence_completion,
            PersistenceCompletion::Scheduled
        );
        let mut rt = runtime(script);
        let plan = rt.active_plan();
        support::without_heap(|| {
            assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(1));
            assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(0));
            assert!(rt.pending_commands() > 0, "wait must really suspend");
            for _ in 0..20 {
                rt.render(&mut [[0.; 2]; 64]).unwrap();
            }
            assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(1));
            assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
            let mut completed = 0;
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                completed += 1;
                true
            });
            assert_eq!(completed, 1);
            assert_eq!(rt.pending_commands(), 0);
        });
    }
}

#[test]
fn persistence_async_wait_suspends_until_its_live_job_completes() {
    let script = compile(
        "on init declare $calls declare $done declare $job end on
        on persistence_changed inc($calls) $job:=mf_reset() wait_async($job) inc($done) end on",
    );
    assert_eq!(
        script.model().persistence_completion,
        PersistenceCompletion::Scheduled
    );
    let mut rt = runtime(script);
    let plan = rt.active_plan();
    support::without_heap(|| {
        let mut job = None;
        rt.drain_effects(|effect| {
            assert_eq!(effect.args[0], 2);
            assert!(job.replace(effect.args[1] as i32).is_none());
            true
        });
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(1));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(0));
        let mut output = MidiCompletion::empty(job.expect("live reset"), ScriptInstanceId(0));
        rt.complete_midi(plan, &mut output).unwrap();
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
    });
}

#[test]
fn init_async_completion_that_waits_runs_on_the_live_scheduler() {
    let script = compile(
        "on init declare $calls declare $done declare $id declare $status declare $empty
        mf_set_buffer_size(3) declare $job := mf_reset() wait_async($job) $empty:=mf_get_buffer_size()
        mf_set_buffer_size(2) mf_insert_event(0,0,$MIDI_COMMAND_NOTE_ON,60,100) wait_async($job)
        declare $remaining:=mf_get_buffer_size() end on
        on async_complete inc($calls) $id:=$NI_ASYNC_ID $status:=$NI_ASYNC_EXIT_STATUS
        wait(1000) $done:=mf_get_buffer_size() end on",
    );
    let mut rt = runtime(script);
    let plan = rt.active_plan();
    support::without_heap(|| {
        let mut job = None;
        rt.drain_effects(|effect| {
            assert_eq!(
                effect.args[0], 5,
                "completed init job needs notification only"
            );
            assert!(job.replace(effect.args[1] as i32).is_none());
            true
        });
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(0));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 4), Ok(0));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 6), Ok(1));
        let mut output =
            MidiCompletion::empty(job.expect("deferred completion"), ScriptInstanceId(0));
        rt.complete_midi(plan, &mut output).unwrap();
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(1));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(0));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(1));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 3), Ok(1));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 4), Ok(0));
        for _ in 0..2 {
            rt.render(&mut [[0.; 2]; 64]).unwrap();
        }
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
    });
}

#[test]
fn unreachable_wait_does_not_defer_a_static_persistence_callback() {
    let script = compile(
        "on init declare $calls end on function unused wait(1000) end function
        on persistence_changed inc($calls) end on",
    );
    assert_eq!(
        script.model().persistence_completion,
        PersistenceCompletion::Completed
    );
    let rt = runtime(script);
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
        Ok(1)
    );
    assert_eq!(rt.pending_commands(), 0);
}

#[test]
fn authored_engine_start_precedes_a_scheduled_persistence_callback() {
    use sampler_core::{
        ControlDefinition, ControlDomain, ControlId, ControlValue, EngineParameterAddress,
        EngineParameterBinding, EngineParameterLaw,
    };
    let script = compile(
        "on init declare $seen declare $done
        set_engine_par($ENGINE_PAR_CUTOFF,250000,0,3,-1) end on
        on persistence_changed $seen:=get_engine_par($ENGINE_PAR_CUTOFF,0,3,-1)
        wait(1000) set_engine_par($ENGINE_PAR_CUTOFF,750000,0,3,-1) inc($done) end on",
    );
    let id = ControlId(42);
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_controls(vec![ControlDefinition {
            id,
            domain: ControlDomain::Real { min: 0., max: 1. },
            default: ControlValue::Real(0.4),
        }])
        .unwrap()
        .with_engine_parameters(
            vec![EngineParameterBinding {
                address: EngineParameterAddress {
                    parameter: sampler_core::engine_parameter_id("ENGINE_PAR_CUTOFF").unwrap(),
                    group: 0,
                    slot: 3,
                    generic: -1,
                },
                control: id,
                law: EngineParameterLaw::Linear { low: 0., high: 1. },
            }],
            vec![],
        )
        .unwrap();
    let plan = script.bind(plan).unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    let mut rt = Runtime::new(plan, limits).unwrap();
    let plan = rt.active_plan();
    support::without_heap(|| {
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(250000));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(0));
        assert_eq!(rt.control_value(plan, id), Ok(ControlValue::Real(0.25)));
        for _ in 0..2 {
            rt.render(&mut [[0.; 2]; 64]).unwrap();
        }
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
        assert_eq!(rt.control_value(plan, id), Ok(ControlValue::Real(0.75)));
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
    });
}

#[test]
fn scheduled_persistence_keeps_ui_effects_before_and_after_the_wait() {
    let script = compile(
        "on init declare ui_label $label(1,1) end on
        on persistence_changed set_text($label,\"before\") wait(1000)
        set_text($label,\"after\") end on",
    );
    let mut view = script.view();
    let mut rt = runtime(script);
    rt.drain_effects(|effect| {
        assert!(view.apply_ui_effect(effect));
        true
    });
    assert_eq!(
        view.model().interface.widgets[0].text("$CONTROL_PAR_TEXT"),
        Some("before")
    );
    support::without_heap(|| {
        for _ in 0..2 {
            rt.render(&mut [[0.; 2]; 64]).unwrap();
        }
    });
    rt.drain_effects(|effect| {
        assert!(view.apply_ui_effect(effect));
        true
    });
    assert_eq!(
        view.model().interface.widgets[0].text("$CONTROL_PAR_TEXT"),
        Some("after")
    );
}

#[cfg(feature = "scan")]
#[test]
fn scan_reports_scheduled_persistence_separately_from_static_completion() {
    sampler_ksp::scan::begin();
    sampler_ksp::scan::attempt("scheduled-persistence-test");
    compile("on init end on on persistence_changed wait(1000) end on");
    let records: Vec<_> = sampler_ksp::scan::take()
        .into_iter()
        .filter(|record| record.attempt == "scheduled-persistence-test")
        .collect();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].persistence_changed.completion, "scheduled");
    assert!(records[0].persistence_changed.fault.is_none());
    sampler_ksp::scan::attempt("standalone");
}
