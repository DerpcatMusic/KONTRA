//! v1 clock conversions use the host's quarter-note duration, including listeners.
use sampler_core::{Limits, Outcome, Prepared, Runtime, ScriptInstanceId};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime(source: &str) -> Runtime {
    let script = sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let plan = script.bind(Prepared::new(48000, vec![], vec![], 0).unwrap()).unwrap();
    let limits = Limits::for_plan(&plan, 8, 0);
    Runtime::new(plan, limits).unwrap()
}

fn render(rt: &mut Runtime, frames: usize) {
    let mut block = [[0.; 2]; 64];
    while rt.now() < frames as u64 {
        let len = (frames as u64 - rt.now()).min(64) as usize;
        rt.render(&mut block[..len]).unwrap();
    }
    rt.flush_behaviors(|_, _, outcome| {
        assert!(!matches!(outcome, Outcome::Fault(_) | Outcome::FuelExhausted), "{outcome:?}");
        true
    });
}

#[test]
fn wait_ticks_reads_host_tempo_when_the_wait_begins() {
    for quarter in [250_000, 500_000, 1_000_000] {
        // Delay the clock-dependent wait until the host publishes its transport.
        let mut rt = runtime("on init declare $done end on
            on persistence_changed wait(1) wait_ticks(960) $done := 1 end on");
        rt.set_host_value(8, quarter).unwrap();
        let frames = quarter as usize * 48_000 / 1_000_000;
        let plan = rt.active_plan();
        support::without_heap(|| {
            render(&mut rt, frames - 64);
            assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(0), "{quarter}");
            render(&mut rt, frames + 64);
            assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(1), "{quarter}");
        });
    }
}

#[test]
fn beat_listener_reads_current_tempo_each_period() {
    let mut rt = runtime("on init declare $ticks end on
        on persistence_changed wait(1) set_listener($NI_SIGNAL_TIMER_BEAT,4) end on
        on listener inc($ticks) end on");
    rt.set_host_value(8, 250_000).unwrap();
    let plan = rt.active_plan();
    support::without_heap(|| {
        render(&mut rt, 48_000);
        let first = rt.script_cell(plan, ScriptInstanceId(0), 0).unwrap();
        assert!((14..=16).contains(&first), "fast tempo ticks: {first}");
        rt.set_host_value(8, 1_000_000).unwrap();
        render(&mut rt, 96_000);
        let second = rt.script_cell(plan, ScriptInstanceId(0), 0).unwrap() - first;
        assert!((3..=5).contains(&second), "slow tempo ticks: {second}");
    });
}
