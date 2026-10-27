//! A ready group job must not prevent the callbacks holding its admission room
//! from resuming. Source-free reproduction of an internal scripted preset load.
use sampler_core::*;
use std::time::{Duration, Instant};
mod support;

fn runtime(wait: bool) -> Runtime {
    let mut code = if wait {
        vec![Instruction::Wait(32)]
    } else {
        let mut code = vec![Instruction::SetLocal { local: 0, value: 0 }];
        code.extend((0..96).map(|_| Instruction::AddLocal { local: 0, value: 1 }));
        code.push(Instruction::Op(Op::Emit { service: 99, args: 0, count: 1, text: None }));
        code
    };
    code.push(Instruction::End);
    let callback = |code| Program::new(code).unwrap()
        .with_script_instance(ScriptInstanceId(0))
        .with_wait_lifetime(WaitLifetime::Callback);
    let completion = callback(vec![
        Instruction::Op(Op::ReadHost { local: 0, slot: 6 }),
        Instruction::Op(Op::ReadHost { local: 1, slot: 7 }),
        Instruction::Op(Op::Emit { service: 77, args: 0, count: 2, text: None }),
        Instruction::End,
    ]);
    let pcm = Pcm::new(48_000, vec![[0.25; 2]; 128].into()).unwrap();
    let region = Region {
        sample: 0, key_low: 60, key_high: 60, root_key: None,
        velocity_low: 0., velocity_high: 1., gain: 1.,
        envelope: Envelope::default(), playback: Playback::default(),
    };
    let regions = (0..4).map(|index| Region {
        key_low: 60 + index, key_high: 60 + index, ..region.clone()
    }).collect();
    let plan = Prepared::new(48_000, vec![pcm], regions, 4).unwrap()
        .with_groups(4, vec![Some(0), Some(1), Some(2), Some(3)]).unwrap()
        .with_script_instances(vec![vec![]]).unwrap()
        .with_programs(vec![callback(code), completion], None).unwrap()
        .with_stages(vec![Stage::default()]).unwrap()
        .with_signal_programs(vec![SignalProgram { signal: MIDI_ASYNC_SIGNAL, program: 1, stage: 0 }]).unwrap();
    let mut limits = Limits::for_plan(&plan, 4, 4);
    limits.behaviors = if wait { 1 } else { 2 };
    limits.behavior_cells = plan.behavior_local_count() * limits.behaviors;
    Runtime::new(plan, limits).unwrap()
}

fn run(wait: bool) {
    let mut rt = runtime(wait);
    let plan = rt.active_plan();
    let mut owner = None;
    support::without_heap(|| {
        rt.begin_behavior_block(if wait { 256 } else { 1 }, None);
        owner = Some(rt.start_plan_behavior(plan, 0).unwrap());
    });
    let owner = owner.unwrap();
    let mut occupancy = 0;
    rt.visit_behavior_progress(|p| {
        occupancy += 1;
        assert_eq!(p.waiting, wait);
        assert_eq!(p.yielded_at.is_some(), !wait);
    });
    assert_eq!(occupancy, 1);
    let mut jobs = Vec::with_capacity(4);
    for group in 0..4 {
        support::without_heap(|| jobs.push(rt.request_group_residency(plan, group, true, ScriptInstanceId(0)).unwrap()));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while rt.residency_progress(plan).unwrap().worker_ready != 4 {
        assert!(Instant::now() < deadline, "source-free residency worker timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
    let before = rt.residency_progress(plan).unwrap();
    assert_eq!((before.accepted, before.consumed, before.pending), (4, 0, 4));
    let mut done = Vec::with_capacity(4);
    let mut listener = Vec::with_capacity(1);
    for block in 0..16 {
        let at = rt.now();
        support::without_heap(|| {
            rt.begin_behavior_block(256, None);
            assert_eq!(rt.render(&mut [[0.; 2]; 64]), Ok(()),
                "ready residency completion must defer admission and let occupied callbacks run");
            assert_eq!(rt.now(), at + 64, "callback pressure must not freeze sample time");
            // Ordinary hosts may drain a small number of UI effects per block.
            let mut count = 0;
            rt.drain_effects(|e| {
                if count == 16 { return false; }
                count += 1;
                match e.service {
                    77 => done.push((e.args[0] as i32, e.args[1])),
                    99 => listener.push(e.args[0]),
                    _ => panic!("unexpected fixture service"),
                }
                true
            });
        });
        let p = rt.residency_progress(plan).unwrap();
        assert_eq!(p.accepted, p.consumed + p.pending as u64, "job ownership must be conserved");
        if done.len() == 4 { break; }
        assert!(block < 15, "finite callbacks failed to release admission room");
    }
    assert_eq!(done, jobs.iter().map(|&id| (id, 1)).collect::<Vec<_>>(), "every accepted job completes exactly once and in order");
    assert_eq!(listener, if wait { vec![] } else { vec![96] });
    let p = rt.residency_progress(plan).unwrap();
    assert_eq!((p.accepted, p.consumed, p.committed, p.pending), (4, 4, 4, 0));
    assert!(p.completion_admission_failures > 0, "fixture must actually exercise callback backpressure");
    assert_eq!(rt.behavior_outcome(owner), Err(Error::StaleHandle), "completed owner must be reclaimed for async callbacks");
    let note = rt.note_on(Input { protocol: Protocol::Clap, port: 0, group: 0, channel: 0, key: 60, external_id: Some(1) }, 60, 1.).unwrap();
    rt.forward_attack(note).unwrap();
    let mut pcm = [[0.; 2]; 16];
    support::without_heap(|| {
        rt.begin_behavior_block(256, None);
        rt.render(&mut pcm).unwrap();
    });
    assert_eq!(pcm, [[0.25; 2]; 16], "callback pressure must preserve playable source PCM");
}

#[test]
fn physical_completions_do_not_starve_a_preempted_callback_at_full_arena() { run(false); }

#[test]
fn physical_completions_do_not_freeze_an_intentional_wait_at_full_arena() { run(true); }
