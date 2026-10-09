//! Generated timer drivers remain live across fuel preemption and real waits.
use sampler_core::{ControlValue, Limits, Outcome, Prepared, Runtime};

#[test]
fn preempted_listener_driver_keeps_ticking_after_one_second() {
    let script = sampler_ksp::compile(
        "on init
           declare ui_knob $ticks(0,10000,1)
           set_listener($NI_SIGNAL_TIMER_MS,10000)
         end on
         on listener inc($ticks) end on",
        48000, sampler_ksp::Limits::LIBRARY, &[],
    ).unwrap();
    let id = script.controls()[0].definition.id;
    let plan = script.bind(Prepared::new(48000, vec![], vec![], 0).unwrap()).unwrap();
    let limits = Limits { behavior_fuel: 8, ..Limits::for_plan(&plan, 8, 0) };
    let mut rt = Runtime::new(plan, limits).unwrap();
    let mut faults = Vec::new();
    let mut second = 0;
    for block in 0..1500 {
        rt.render(&mut [[0.; 2]; 64]).unwrap();
        rt.flush_behaviors_at(|_, _, outcome, program| {
            if matches!(outcome, Outcome::FuelExhausted | Outcome::Fault(_)) {
                faults.push((program, outcome));
            }
            true
        });
        if block == 749 {
            let ControlValue::Integer(value) = rt.control_value(rt.active_plan(), id).unwrap() else { panic!("integer ticks") };
            second = value;
        }
    }
    let ControlValue::Integer(final_ticks) = rt.control_value(rt.active_plan(), id).unwrap() else { panic!("integer ticks") };
    assert!(faults.is_empty(), "periodic driver must not age across waits: {faults:?}");
    assert!(second > 0 && final_ticks > second, "timer stopped after one second");
    assert!(rt.preemptions() > 0, "regression must exercise fuel preemption");
    assert_eq!(rt.now(), 96000);
}
