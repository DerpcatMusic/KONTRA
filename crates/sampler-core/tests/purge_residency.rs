use sampler_core::*;
mod support;

fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}
fn region(sample: usize) -> Region {
    Region {
        sample,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback {
            loop_range: Some(Loop {
                start: 0,
                end: 128,
                mode: LoopMode::Continuous,
                shape: LoopShape::Wrap,
                passes: None,
            }),
            ..Default::default()
        },
    }
}
fn program(mode: i64) -> Program {
    program_at(0, mode)
}
fn program_at(group: i64, mode: i64) -> Program {
    Program::new(vec![
        Instruction::SetLocal {
            local: 0,
            value: group,
        },
        Instruction::SetLocal {
            local: 1,
            value: mode,
        },
        Instruction::Op(Op::Purge {
            group: 0,
            local: 1,
            write: true,
        }),
        Instruction::End,
    ])
    .unwrap()
    .with_script_instance(ScriptInstanceId(0))
    .with_wait_lifetime(WaitLifetime::Callback)
}
fn runtime(pcm: Pcm, shared: bool) -> Runtime {
    let regions = if shared {
        vec![region(0), region(0)]
    } else {
        vec![region(0)]
    };
    let members = if shared {
        vec![Some(0), Some(1)]
    } else {
        vec![Some(0)]
    };
    let plan = Prepared::new(48000, vec![pcm], regions, 2)
        .unwrap()
        .with_groups(if shared { 2 } else { 1 }, members)
        .unwrap();
    from_plan(plan)
}
fn from_plan(plan: Prepared) -> Runtime {
    from_programs(plan, vec![program(0), program(1), completion_program()])
}
fn from_programs(plan: Prepared, programs: Vec<Program>) -> Runtime {
    let plan = plan
        .with_script_instances(vec![vec![]])
        .unwrap()
        .with_programs(programs, None)
        .unwrap()
        .with_stages(vec![Stage::default()])
        .unwrap()
        .with_signal_programs(vec![SignalProgram {
            signal: MIDI_ASYNC_SIGNAL,
            program: 2,
            stage: 0,
        }])
        .unwrap();
    let limits = Limits {
        notes: 8,
        channels: 1,
        performances: 1,
        families: 16,
        expressions: 8,
        voices: 32,
        decisions: 8,
        commands: 16,
        behaviors: 8,
        behavior_fuel: 1024,
        behavior_cells: 32,
        note_cells: 0,
    };
    Runtime::new(plan, limits).unwrap()
}
#[test]
fn purge_retires_voices_and_refuses_new_group_starts_without_script_gain_work() {
    let pcm = Pcm::new(48000, vec![[0.25; 2]; 128].into()).unwrap();
    let mut rt = runtime(pcm, false);
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    rt.forward_attack(note).unwrap();
    assert_eq!(rt.voice_count(), 1);
    let plan = rt.active_plan();
    support::without_heap(|| {
        rt.start_plan_behavior(plan, 0).unwrap();
    });
    assert_eq!(
        rt.voice_count(),
        0,
        "purge must retire source/FX work, not attenuate it"
    );
    let note = rt.note_on(input(2), 60, 1.).unwrap();
    rt.forward_attack(note).unwrap();
    assert_eq!(rt.voice_count(), 0);
}

fn completion_program() -> Program {
    Program::new(vec![
        Instruction::Op(Op::ReadHost { local: 0, slot: 6 }),
        Instruction::Op(Op::ReadHost { local: 1, slot: 7 }),
        Instruction::SetLocal { local: 2, value: 0 },
        Instruction::Op(Op::Purge {
            group: 2,
            local: 3,
            write: false,
        }),
        Instruction::Op(Op::Emit {
            service: 77,
            args: 0,
            count: 4,
            text: None,
        }),
        Instruction::End,
    ])
    .unwrap()
    .with_script_instance(ScriptInstanceId(0))
    .with_wait_lifetime(WaitLifetime::Callback)
}
fn drain(rt: &mut Runtime) -> Vec<(i32, bool, bool)> {
    let mut done = vec![];
    rt.drain_effects(|e| {
        assert_eq!(e.service, 77);
        let a = e.args();
        done.push((a[0] as i32, a[1] == 1, a[3] == 1));
        true
    });
    done
}
fn wait(rt: &mut Runtime, count: usize) -> Vec<(i32, bool, bool)> {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut done = vec![];
    while done.len() < count {
        // Poll is a nonblocking audio boundary, and must work without voices/time.
        support::without_heap(|| rt.render(&mut []).unwrap());
        done.extend(drain(rt));
        assert!(
            std::time::Instant::now() < until,
            "residency completion timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    done
}
#[test]
fn purge_frees_full_pcm_and_mipmaps_then_reload_publishes_exact_async_id_and_state() {
    let pcm = Pcm::mipmapped(48000, vec![[0.25; 2]; 128].into()).unwrap();
    pcm.set_residency_loader(|| Ok(vec![(0, vec![[0.25; 2]; 128].into())]))
        .unwrap();
    let before = pcm.resident_bytes();
    assert_eq!(before, 1984);
    let mut rt = runtime(pcm.clone(), false);
    let plan = rt.active_plan();
    let id = rt.start_plan_behavior(plan, 0).unwrap();
    let purge = rt.behavior_local(id, 1).unwrap() as i32;
    assert!(purge > 0);
    assert_eq!(wait(&mut rt, 1), vec![(purge, true, false)]);
    assert_eq!(pcm.resident_bytes(), 0);
    let id = rt.start_plan_behavior(plan, 1).unwrap();
    let reload = rt.behavior_local(id, 1).unwrap() as i32;
    assert!(reload > purge);
    assert_eq!(wait(&mut rt, 1), vec![(reload, true, true)]);
    assert_eq!(pcm.resident_bytes(), 128 * std::mem::size_of::<Frame>());
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    rt.forward_attack(note).unwrap();
    let mut out = [[0.; 2]; 8];
    rt.render(&mut out).unwrap();
    assert_eq!(out, [[0.25; 2]; 8]);
}
#[test]
fn shared_asset_keeps_other_groups_resident_and_playable_until_the_last_owner_purges() {
    let pcm = Pcm::new(48000, vec![[0.25; 2]; 128].into()).unwrap();
    let mut rt = runtime(pcm.clone(), true);
    let plan = rt.active_plan();
    rt.start_plan_behavior(plan, 0).unwrap();
    wait(&mut rt, 1);
    assert_eq!(pcm.resident_bytes(), 1024);
    assert!(!rt.group_resident(plan, 0).unwrap());
    assert!(rt.group_resident(plan, 1).unwrap());
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    rt.forward_attack(note).unwrap();
    assert_eq!(rt.voice_count(), 1);
    let mut audio = [[0.; 2]; 8];
    rt.render(&mut audio).unwrap();
    assert_eq!(audio, [[0.25; 2]; 8]);
    let last = rt
        .request_group_residency(plan, 1, false, ScriptInstanceId(0))
        .unwrap();
    assert_eq!(rt.voice_count(), 0);
    assert_eq!(wait(&mut rt, 1), vec![(last, true, false)]);
    assert_eq!(pcm.resident_bytes(), 0);
}
#[test]
fn purge_completion_waits_for_borrowed_memory_reclamation() {
    let pcm = Pcm::new(48000, vec![[0.25; 2]; 128].into()).unwrap();
    let borrowed = pcm.resident_frames().unwrap();
    let mut rt = runtime(pcm.clone(), false);
    let plan = rt.active_plan();
    let id = rt
        .request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    rt.render(&mut []).unwrap();
    assert!(drain(&mut rt).is_empty());
    assert_eq!(borrowed[0], [0.25; 2]);
    drop(borrowed);
    assert_eq!(wait(&mut rt, 1), vec![(id, true, false)]);
    assert_eq!(pcm.resident_bytes(), 0);
}
#[test]
fn a_new_purge_cancels_an_in_flight_reload_without_resurrecting_samples() {
    let pcm = Pcm::new(48000, vec![[0.25; 2]; 128].into()).unwrap();
    let (entered, rx) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let gate = std::sync::Mutex::new(gate);
    pcm.set_residency_loader(move || {
        entered.send(()).unwrap();
        gate.lock().unwrap().recv().unwrap();
        Ok(vec![(0, vec![[0.5; 2]; 128].into())])
    })
    .unwrap();
    let mut rt = runtime(pcm.clone(), false);
    let plan = rt.active_plan();
    rt.request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    wait(&mut rt, 1);
    let reload = rt
        .request_group_residency(plan, 0, true, ScriptInstanceId(0))
        .unwrap();
    let entered = rx.recv_timeout(std::time::Duration::from_secs(3)).is_ok();
    // Always unblock the worker before assertions/runtime destruction.
    if !entered {
        let _ = release.send(());
        assert!(entered);
    }
    assert!(!rt.group_resident(plan, 0).unwrap());
    let purge = rt
        .request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    rt.render(&mut []).unwrap();
    assert!(drain(&mut rt).is_empty());
    release.send(()).unwrap();
    assert_eq!(
        wait(&mut rt, 2),
        vec![(reload, false, false), (purge, true, false)]
    );
    assert_eq!(pcm.resident_bytes(), 0);
}
#[test]
fn shared_asset_ownership_spans_prepared_plan_generations() {
    let pcm = Pcm::new(48000, vec![[0.25; 2]; 128].into()).unwrap();
    let mut a = runtime(pcm.clone(), false);
    let mut b = runtime(pcm.clone(), false);
    let plan = a.active_plan();
    a.request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    wait(&mut a, 1);
    assert_eq!(pcm.resident_bytes(), 1024);
    let note = b.note_on(input(1), 60, 1.).unwrap();
    b.forward_attack(note).unwrap();
    let mut audio = [[0.; 2]; 8];
    b.render(&mut audio).unwrap();
    assert_eq!(audio, [[0.25; 2]; 8]);
    drop(b);
    assert_eq!(
        pcm.resident_bytes(),
        0,
        "last other plan's retirement releases shared residency"
    );
}
#[test]
fn saved_muted_group_can_unmute_without_reloading_its_definition() {
    let pcm = Pcm::new(48000, (0..128).map(|i| [i as f32 / 256.; 2]).collect()).unwrap();
    let mut zone = region(0);
    zone.playback.start = 16;
    zone.playback.loop_range = Some(Loop {
        start: 32,
        end: 96,
        mode: LoopMode::Continuous,
        shape: LoopShape::Wrap,
        passes: None,
    });
    let plan = Prepared::new(48000, vec![pcm], vec![zone], 1)
        .unwrap()
        .with_groups(1, vec![Some(0)])
        .unwrap()
        .with_group_mutes(vec![true])
        .unwrap()
        .with_source_groups(vec![37])
        .unwrap()
        .with_voice_chains(
            vec![VoiceChain::new(vec![Processor::Gain(2.)], vec![], 0).unwrap()],
            vec![Some(0)],
        )
        .unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: 8,
            channels: 1,
            performances: 1,
            families: 16,
            expressions: 8,
            voices: 32,
            decisions: 8,
            commands: 16,
            behaviors: 8,
            behavior_fuel: 1024,
            behavior_cells: 32,
            note_cells: 0,
        },
    )
    .unwrap();
    let plan = rt.active_plan();
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    rt.forward_attack(note).unwrap();
    assert_eq!(rt.voice_count(), 0);
    rt.set_group_muted(plan, 0, false).unwrap();
    let note = rt.note_on(input(2), 60, 1.).unwrap();
    rt.forward_attack(note).unwrap();
    assert_eq!(rt.voice_count(), 1);
    let mut audio = [[0.; 2]; 256];
    rt.render(&mut audio).unwrap();
    assert_eq!(
        audio[0], [0.125; 2],
        "the sample-start offset survives saved mute"
    );
    assert_eq!(
        audio[255],
        [79. / 128.; 2],
        "the authored loop and pre-amp chain survive saved mute"
    );
}

#[test]
fn streamed_preload_is_freed_and_reload_restores_the_onset() {
    let pcm = Pcm::headed(48000, 128, &[[0.25; 2]; 128]).unwrap();
    pcm.set_residency_loader(|| Ok(vec![(0, vec![[0.25; 2]; 128].into())]))
        .unwrap();
    let bytes = pcm.head_bytes();
    assert_eq!(bytes, 256);
    let mut rt = runtime(pcm.clone(), false);
    let (cache, _worker) = StreamCache::new(4).unwrap();
    rt = rt.with_stream_cache(cache);
    let plan = rt.active_plan();
    let purge = rt
        .request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    assert_eq!(wait(&mut rt, 1)[0].0, purge);
    assert_eq!(pcm.head_bytes(), 0);
    let reload = rt
        .request_group_residency(plan, 0, true, ScriptInstanceId(0))
        .unwrap();
    assert_eq!(wait(&mut rt, 1)[0].0, reload);
    assert_eq!(pcm.head_bytes(), bytes);
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    rt.forward_attack(note).unwrap();
    let mut audio = [[0.; 2]; 8];
    rt.render(&mut audio).unwrap();
    assert_eq!(audio, [[0.25; 2]; 8]);
}
#[test]
fn failed_reload_completes_with_failure_and_keeps_group_purged() {
    let pcm = Pcm::new(48000, vec![[0.25; 2]; 128].into()).unwrap();
    pcm.set_residency_loader(|| Err(Error::NotReady)).unwrap();
    let mut rt = runtime(pcm.clone(), false);
    let plan = rt.active_plan();
    rt.request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    wait(&mut rt, 1);
    let reload = rt
        .request_group_residency(plan, 0, true, ScriptInstanceId(0))
        .unwrap();
    assert_eq!(wait(&mut rt, 1), vec![(reload, false, false)]);
    assert_eq!(pcm.resident_bytes(), 0);
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    rt.forward_attack(note).unwrap();
    assert_eq!(rt.voice_count(), 0);
}

#[test]
fn purge_discards_in_flight_page_results_and_cold_starts_cannot_allocate_voices() {
    let pcm = Pcm::headed(48000, PAGE_FRAMES * 2, &[[0.25; 2]; 128]).unwrap();
    let (mut cache, mut worker) = StreamCache::new(4).unwrap();
    let key = PageKey {
        asset: pcm.asset_id(),
        index: 1,
    };
    assert_eq!(cache.request(&pcm, 1, 0), Ok(PageStatus::Pending));
    let mut job = worker.next_job().unwrap();
    job.frames_mut().fill([0.5; 2]);
    let mut rt = runtime(pcm.clone(), false).with_stream_cache(cache);
    rt.set_cold_starts(true);
    let plan = rt.active_plan();
    rt.request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    assert_eq!(
        rt.stream_cache_mut().unwrap().status(key),
        PageStatus::Missing
    );
    worker.complete(job, Ok(())).unwrap();
    assert_eq!(
        rt.stream_cache_mut().unwrap().poll(),
        Some(PageUpdate::Discarded(key))
    );
    assert_eq!(
        rt.stream_cache_mut().unwrap().status(key),
        PageStatus::Missing
    );
    wait(&mut rt, 1);
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    rt.forward_attack(note).unwrap();
    assert_eq!(rt.voice_count(), 0);
    assert_eq!(pcm.head_bytes(), 0);
    assert!(
        !pcm.take_cold(),
        "deliberate purge cannot generate cold recovery demand"
    );
}

#[test]
fn failed_multi_asset_reload_rolls_back_partial_residency_before_completion() {
    let a = Pcm::new(48000, vec![[0.25; 2]; 128].into()).unwrap();
    let b = Pcm::new(48000, vec![[0.5; 2]; 128].into()).unwrap();
    a.set_residency_loader(|| Ok(vec![(0, vec![[0.25; 2]; 128].into())]))
        .unwrap();
    b.set_residency_loader(|| Err(Error::NotReady)).unwrap();
    let plan = Prepared::new(
        48000,
        vec![a.clone(), b.clone()],
        vec![region(0), region(1)],
        2,
    )
    .unwrap()
    .with_groups(1, vec![Some(0), Some(0)])
    .unwrap();
    let mut rt = from_plan(plan);
    let plan = rt.active_plan();
    rt.request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    wait(&mut rt, 1);
    let id = rt
        .request_group_residency(plan, 0, true, ScriptInstanceId(0))
        .unwrap();
    assert_eq!(wait(&mut rt, 1), vec![(id, false, false)]);
    assert_eq!(a.resident_bytes() + b.resident_bytes(), 0);
    assert_eq!(a.residency_owners() + b.residency_owners(), 0);
}

#[test]
fn purge_addresses_physical_groups_without_truncating_invalid_indices() {
    let pcm = Pcm::new(48000, vec![[0.25; 2]; 128].into()).unwrap();
    let plan = Prepared::new(48000, vec![pcm], vec![region(0)], 1)
        .unwrap()
        .with_groups(1, vec![Some(0)])
        .unwrap()
        .with_source_groups(vec![37])
        .unwrap();
    let mut rt = from_programs(
        plan,
        vec![
            program_at(37, 0),
            program_at(0, 0),
            completion_program(),
            program_at((1i64 << 32) + 37, 0),
        ],
    );
    let plan = rt.active_plan();
    for program in [1, 3] {
        let id = rt.start_plan_behavior(plan, program).unwrap();
        assert_eq!(rt.behavior_local(id, 1).unwrap(), -1);
        assert!(rt.group_resident(plan, 0).unwrap());
    }
    let id = rt.start_plan_behavior(plan, 0).unwrap();
    assert!(rt.behavior_local(id, 1).unwrap() > 0);
    assert!(!rt.group_resident(plan, 0).unwrap());
    wait(&mut rt, 1);
    let pcm = Pcm::new(48000, vec![[0.25; 2]; 128].into()).unwrap();
    let plan = Prepared::new(48000, vec![pcm], vec![region(0)], 1).unwrap().with_groups(1, vec![Some(0)]).unwrap();
    let mut rt = from_programs(plan, vec![program_at(1i64 << 32, 0), program(1), completion_program()]);
    let plan = rt.active_plan();
    let id = rt.start_plan_behavior(plan, 0).unwrap();
    assert_eq!(rt.behavior_local(id, 1).unwrap(), -1);
    assert!(rt.group_resident(plan, 0).unwrap());
}
