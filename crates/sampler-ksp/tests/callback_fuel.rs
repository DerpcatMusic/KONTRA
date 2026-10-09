//! Synthetic finite work/wait/work pattern; the tester's scripts are unavailable locally.
use sampler_core::{
    Input, Limits, MidiCompletion, Outcome, Prepared, Protocol, Runtime, ScriptInstanceId,
};

#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

#[test]
fn finite_ksp_loops_survive_long_waits_at_the_host_block_budget() {
    for rate in [44100, 48000] {
        for frames in [32, 64, 128] {
            let script = sampler_ksp::compile(
                "on init declare $i declare $done end on
             on note
               $i := 0
               while ($i < 20000) inc($i) inc($done) end while
               wait(2000000)
               $i := 0
               while ($i < 20000) inc($i) inc($done) end while
             end on",
                rate,
                sampler_ksp::Limits::LIBRARY,
                &[],
            )
            .unwrap();
            let plan = script
                .bind(Prepared::new(rate, vec![], vec![], 0).unwrap())
                .unwrap();
            let limits = Limits::for_plan(&plan, 4, 4);
            let mut rt = Runtime::new(plan, limits).unwrap();
            // Match src/sound/v2.rs; each callback keeps the default 1<<20 fuel.
            rt.set_behavior_block_fuel((frames * 128).max(8192));
            let mut faults = 0;
            let mut first_fault = None;
            let mut finished = 0;
            let mut audio = [[0.; 2]; 128];
            support::without_heap(|| {
                rt.trigger(
                    Input {
                        protocol: Protocol::Native,
                        port: 0,
                        group: 0,
                        channel: 0,
                        key: 60,
                        external_id: None,
                    },
                    60,
                    1.,
                )
                .unwrap();
                for _ in 0..(rate as usize * 4).div_ceil(frames) {
                    rt.render(&mut audio[..frames]).unwrap();
                    rt.flush_behaviors_at(|_, _, outcome, _| {
                        if matches!(outcome, Outcome::FuelExhausted | Outcome::Fault(_)) {
                            faults += 1;
                            first_fault.get_or_insert(outcome);
                        }
                        if outcome == Outcome::Finished {
                            finished += 1;
                        }
                        true
                    });
                    if faults > 0 || finished > 0 {
                        break;
                    }
                }
                assert_eq!(first_fault, None, "rate={rate}, frames={frames}");
                assert_eq!(
                    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 1),
                    Ok(40000)
                );
                assert_eq!(finished, 1);
                assert!(
                    rt.preemptions() > 0,
                    "host fuel must preempt the finite loops"
                );
                assert!(
                    rt.now() >= u64::from(rate) * 2,
                    "the callback must really wait"
                );
            });
            eprintln!(
                "rate={rate}, frames={frames}, samples={}, preemptions={}, completed={finished}, faults={faults}",
                rt.now(),
                rt.preemptions()
            );
        }
    }
}

#[test]
fn admitted_async_wait_ends_age_but_invalid_or_disabled_wait_keeps_runaway_guard() {
    for mode in ["pending", "invalid", "disabled"] {
        let source = if mode == "pending" {
            "on init declare $i declare $done declare $job end on
             on note
               while ($i < 20000) inc($i) inc($done) end while
               $job := mf_reset() wait_async($job)
               $i := 0
               while ($i < 20000) inc($i) inc($done) end while
             end on"
        } else if mode == "invalid" {
            "on init end on on note while (1 = 1) wait_async(-1) end while end on"
        } else {
            "on init declare $job declare $id end on on note
             $job := mf_reset() $id := $NI_CALLBACK_ID wait_async($job)
             while (1 = 1) wait_async($job) end while end on
             on controller stop_wait($id,1) end on"
        };
        let script =
            sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let plan = script
            .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
            .unwrap();
        let limits = Limits::for_plan(&plan, 4, 4);
        let mut rt = Runtime::new(plan, limits).unwrap();
        rt.set_behavior_block_fuel(8192);
        let plan = rt.active_plan();
        let mut job = None;
        let mut outcome = None;
        support::without_heap(|| {
            let input = Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: None,
            };
            rt.trigger(input, 60, 1.).unwrap();
            if mode == "disabled" {
                rt.dispatch_controller(
                    rt.performance(0).unwrap(),
                    input.channel_address(),
                    1,
                    1,
                    1,
                )
                .unwrap();
            }
            for _ in 0..1600 {
                rt.render(&mut [[0.; 2]; 64]).unwrap();
                rt.drain_effects(|effect| {
                    job = Some(effect.args[1] as i32);
                    true
                });
                rt.flush_behaviors_at(|_, _, result, _| {
                    outcome = Some(result);
                    true
                });
            }
            assert!(rt.preemptions() > 0, "mode={mode}, outcome={outcome:?}");
            if mode == "pending" {
                assert_eq!(outcome, None, "async callback must remain suspended");
                assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(20000));
                let mut output =
                    MidiCompletion::empty(job.expect("admitted MIDI job"), ScriptInstanceId(0));
                rt.complete_midi(plan, &mut output).unwrap();
                for _ in 0..100 {
                    rt.render(&mut [[0.; 2]; 64]).unwrap();
                    rt.flush_behaviors_at(|_, _, result, _| {
                        outcome = Some(result);
                        true
                    });
                    if outcome.is_some() {
                        break;
                    }
                }
                assert_eq!(outcome, Some(Outcome::Finished));
                assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(40000));
            } else {
                assert_eq!(job.is_some(), mode == "disabled");
                assert_eq!(outcome, Some(Outcome::FuelExhausted));
            }
        });
    }
}
