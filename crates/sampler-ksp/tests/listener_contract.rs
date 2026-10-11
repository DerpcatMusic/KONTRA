//! Source-only listener regressions: NOT_RUN / NOT_COMPILED; native UNKNOWN.
use sampler_core::{Limits, Outcome, Prepared, Runtime, ScriptInstanceId};
use sampler_ksp::{LISTENER_TAG, Script};

fn compile(source: &str) -> Result<Script, sampler_ksp::Error> {
    sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[])
}

#[test]
fn listener_registration_accepts_only_documented_signals_and_ranges() {
    for (signal, lo, hi) in [(1, 1000, i32::MAX), (2, 1, 24), (3, 1, 1), (4, 1, 1)] {
        for value in [lo, hi] {
            let script =
                compile(&format!("on init set_listener({signal},{value}) end on")).unwrap();
            assert_eq!(script.model().listeners.get(&signal), Some(&value));
        }
        for value in [-1, 0, lo - 1] {
            assert!(compile(&format!("on init set_listener({signal},{value}) end on")).is_err());
        }
        if hi < i32::MAX {
            assert!(compile(&format!("on init set_listener({signal},{}) end on", hi + 1)).is_err());
        }
    }
    for signal in [-1, 0, 5, i32::MAX] {
        assert!(compile(&format!("on init set_listener({signal},1000) end on")).is_err());
    }
}

#[test]
fn listener_init_evaluator_checks_nonconstant_arguments_before_registration() {
    for (signal, value) in [(1, 999), (1, 0), (2, 25), (2, 0), (3, 0), (4, 2), (5, 1)] {
        assert!(
            compile(&format!(
                "on init declare $s := {signal} declare $p := {value}
            set_listener($s,$p) end on"
            ))
            .is_err()
        );
    }
    let script = compile(
        "on init declare $s := 1 declare $p := 1000
        set_listener($s,$p) end on",
    )
    .unwrap();
    assert_eq!(script.model().listeners.get(&1), Some(&1000));
}

#[test]
fn set_listener_context_follows_function_callers() {
    let function = "function register set_listener($NI_SIGNAL_TIMER_MS,1000) end function";
    assert!(compile(&format!("{function} on init call register end on")).is_ok());
    for callback in [
        "note",
        "release",
        "controller",
        "listener",
        "persistence_changed",
        "async_complete",
        "ui_update",
        "ui_controls",
        "pgs_changed",
        "rpn",
        "nrpn",
        "poly_at",
    ] {
        for body in ["set_listener(1,1000)", "call register"] {
            let error = compile(&format!(
                "{function} on init end on on {callback} {body} end on"
            ))
            .err()
            .expect("non-init registration must fail");
            assert!(error.message.contains("on init"), "{callback}: {error}");
        }
    }
    assert!(
        compile(
            "on init declare ui_knob $k(0,1,1) end on
        on ui_control($k) set_listener(1,1000) end on"
        )
        .is_err()
    );
}

#[test]
fn change_listener_par_accepts_zero_only_for_timer_signals() {
    for (signal, values) in [(1, vec![0, 1000, i32::MAX]), (2, vec![0, 1, 24])] {
        for value in values {
            let initial = if signal == 1 { 1000 } else { 1 };
            let script = compile(&format!(
                "on init set_listener({signal},{initial})
                change_listener_par({signal},{value}) end on"
            ))
            .unwrap();
            assert_eq!(script.model().listeners.get(&signal), Some(&value));
        }
    }
    for (signal, value) in [(1, -1), (1, 999), (2, -1), (2, 25), (3, 0), (4, 1), (5, 0)] {
        assert!(
            compile(&format!(
                "on init change_listener_par({signal},{value}) end on"
            ))
            .is_err()
        );
        assert!(
            compile(&format!(
                "on note change_listener_par({signal},{value}) end on"
            ))
            .is_err()
        );
        assert!(
            compile(&format!(
                "on init declare $s := {signal} declare $p := {value}
            change_listener_par($s,$p) end on"
            ))
            .is_err()
        );
    }
}

#[test]
fn change_listener_par_is_available_in_every_supported_callback() {
    for callback in [
        "note",
        "release",
        "controller",
        "listener",
        "persistence_changed",
        "async_complete",
        "ui_update",
        "ui_controls",
        "pgs_changed",
        "rpn",
        "nrpn",
        "poly_at",
    ] {
        assert!(
            compile(&format!(
                "on init set_listener(1,1000) end on
            on {callback} change_listener_par(1,0) end on"
            ))
            .is_ok(),
            "{callback}"
        );
    }
    assert!(
        compile(
            "on init declare ui_knob $k(0,1,1) set_listener(1,1000) end on
        on ui_control($k) change_listener_par(1,0) end on"
        )
        .is_ok()
    );
}

#[test]
fn live_listener_updates_validate_before_writing_the_store() {
    for (signal, value, expected) in [
        (1, -1, 10000),
        (1, 999, 10000),
        (1, 0, 0),
        (1, 1000, 1000),
        (1, i32::MAX, i32::MAX),
        (2, -1, 4),
        (2, 25, 4),
        (2, 0, 0),
        (2, 1, 1),
        (2, 24, 24),
    ] {
        let script = compile(&format!(
            "on init declare $s := {signal} declare $p := {value}
            set_listener(1,10000) set_listener(2,4) end on
            on persistence_changed wait(1) change_listener_par($s,$p) end on"
        ))
        .unwrap();
        let plan = script
            .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
            .unwrap();
        let limits = Limits::for_plan(&plan, 8, 0);
        let mut rt = Runtime::new(plan, limits).unwrap();
        rt.render(&mut [[0.; 2]; 64]).unwrap();
        rt.flush_behaviors(|_, _, outcome| {
            assert!(!matches!(
                outcome,
                Outcome::Fault(_) | Outcome::FuelExhausted
            ));
            true
        });
        assert_eq!(
            rt.script_store(
                rt.active_plan(),
                ScriptInstanceId(0),
                [LISTENER_TAG, signal, 0, LISTENER_TAG]
            ),
            Ok(Some(i64::from(expected)))
        );
        let other = if signal == 1 { 2 } else { 1 };
        assert_eq!(
            rt.script_store(
                rt.active_plan(),
                ScriptInstanceId(0),
                [LISTENER_TAG, other, 0, LISTENER_TAG]
            ),
            Ok(Some(if other == 1 { 10000 } else { 4 }))
        );
    }
}

#[test]
fn live_listener_updates_do_not_create_unsupported_signal_keys() {
    for signal in [-1, 0, 3, 4, 5, i32::MAX] {
        let script = compile(&format!(
            "on init declare $s := {signal}
            set_listener(1,10000) end on
            on persistence_changed wait(1) change_listener_par($s,0) end on"
        ))
        .unwrap();
        let plan = script
            .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
            .unwrap();
        let limits = Limits::for_plan(&plan, 8, 0);
        let mut rt = Runtime::new(plan, limits).unwrap();
        rt.render(&mut [[0.; 2]; 64]).unwrap();
        assert_eq!(
            rt.script_store(
                rt.active_plan(),
                ScriptInstanceId(0),
                [LISTENER_TAG, signal, 0, LISTENER_TAG]
            ),
            Ok(None)
        );
        assert_eq!(
            rt.script_store(
                rt.active_plan(),
                ScriptInstanceId(0),
                [LISTENER_TAG, 1, 0, LISTENER_TAG]
            ),
            Ok(Some(10000))
        );
    }
}
