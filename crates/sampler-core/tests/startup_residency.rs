use sampler_core::*;
use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering::*},
    },
    time::{Duration, Instant},
};
mod support;

fn pcm() -> Pcm {
    Pcm::new(48_000, vec![[0.25; 2]; 4096].into()).unwrap()
}
fn prepared(pcm: Pcm) -> Prepared {
    Prepared::new(
        48_000,
        vec![pcm],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap()
    .with_groups(1, vec![Some(0)])
    .unwrap()
    .with_source_groups(vec![37])
    .unwrap()
    .with_script_instances(vec![vec![0]])
    .unwrap()
    .with_stages(vec![Stage::default()])
    .unwrap()
}
fn roster(plan: Prepared, n: usize) -> Prepared {
    let mut midi = MidiObject::default();
    midi.reserve_async_ids_through(n as i32).unwrap();
    plan.with_midi_object(midi)
        .with_initial_group_residency(
            (0..n)
                .map(|i| InitialGroupResidency {
                    source_group: 37,
                    reload: i + 1 == n,
                    async_id: (i + 1) as i32,
                    instance: ScriptInstanceId(0),
                })
                .collect(),
        )
        .unwrap()
}
fn drain(rt: &mut Runtime) {
    for _ in 0..8192 {
        if !rt.startup_work_pending(rt.active_plan()).unwrap() {
            return;
        }
        support::without_heap(|| rt.render(&mut []).unwrap());
    }
    panic!("startup completion drain timed out");
}

#[test]
fn automatic_constructor_admits_over_capacity_without_live_refusals_or_audio_heap() {
    let plan = roster(prepared(pcm()), 2050);
    let limits = Limits::for_plan(&plan, 8, 8);
    let mut rt = Runtime::new(plan, limits).unwrap();
    let before = rt.residency_progress(rt.active_plan()).unwrap();
    assert_eq!(
        (
            before.accepted,
            before.consumed,
            before.pending,
            before.rejected_queue
        ),
        (2050, 2050, 0, 0)
    );
    assert_eq!(before.startup_completions_pending, 2050);
    support::without_heap(|| rt.render(&mut []).unwrap());
    assert_eq!(
        rt.residency_progress(rt.active_plan())
            .unwrap()
            .startup_completions_pending,
        1986
    );
    drain(&mut rt);
    assert!(rt.group_resident(rt.active_plan(), 0).unwrap());
    let next = rt
        .request_group_residency(rt.active_plan(), 0, true, ScriptInstanceId(0))
        .unwrap();
    assert_eq!(next, 2051);
    let deadline = Instant::now() + Duration::from_secs(3);
    while rt.residency_progress(rt.active_plan()).unwrap().pending != 0 {
        assert!(Instant::now() < deadline);
        support::without_heap(|| rt.service_residency().unwrap());
        std::thread::yield_now();
    }
}

#[test]
fn canceled_constructor_keeps_the_other_owner_eligibility_revision_and_pinned_pcm() {
    let pcm = pcm();
    let old = prepared(pcm.clone());
    let budget = Limits::for_plan(&old, 8, 8);
    let mut rt = Runtime::new(old, budget).unwrap();
    let old_plan = rt.active_plan();
    let old_group = rt.group_residency_progress(old_plan, 0).unwrap();
    let retained = pcm.resident_frames().unwrap();
    let plan = roster(prepared(pcm.clone()), 2050);
    let limits = Limits::for_plan(&plan, 8, 8);
    let polls = Cell::new(0);
    let canceled = || {
        let n = polls.get() + 1;
        polls.set(n);
        n >= 25
    };
    assert!(matches!(
        Runtime::new_cancelable(plan, limits, &canceled),
        Err(Error::Canceled)
    ));
    assert_eq!(rt.group_residency_progress(old_plan, 0).unwrap(), old_group);
    assert_eq!(pcm.residency_owners(), 1);
    assert_eq!(retained[0], [0.25; 2]);
    let mut audio = [[0.; 2]; 32];
    support::without_heap(|| {
        rt.trigger(
            Input {
                protocol: Protocol::Clap,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(19),
            },
            60,
            1.,
        )
        .unwrap();
        rt.render(&mut audio).unwrap();
    });
    assert!(audio.iter().flatten().any(|v| v.abs() > 0.01));
}

#[test]
fn update_cancellation_returns_the_candidate_without_publishing_or_touching_current_groups() {
    let pcm = pcm();
    let plan = prepared(pcm.clone());
    let limits = Limits::for_plan(&plan, 8, 8);
    let (mut rt, mut control) = Runtime::with_plan_updates(plan, limits, 2, 1).unwrap();
    let active = rt.active_plan();
    let original = rt.group_residency_progress(active, 0).unwrap();
    let candidate = roster(prepared(pcm.clone()), 2050);
    let polls = Cell::new(0);
    let rejected = control
        .submit_cancelable(Box::new(candidate), &|| {
            polls.set(polls.get() + 1);
            polls.get() >= 25
        })
        .unwrap_err();
    assert_eq!(rejected.reason, PlanError::Canceled);
    assert_eq!(rt.poll_plan_update(), Ok(None));
    assert_eq!(rt.active_plan(), active);
    assert_eq!(rt.group_residency_progress(active, 0).unwrap(), original);
    // A retry creates a new owner and complete spool rather than exposing the
    // canceled private owner's intermediate revisions or dropping descriptors.
    control.submit(rejected.prepared).unwrap();
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
    });
    let p = rt.residency_progress(rt.active_plan()).unwrap();
    assert_eq!((p.accepted, p.consumed, p.rejected_queue), (2050, 2050, 0));
    drain(&mut rt);
}

#[test]
fn retained_plan_readmission_gets_a_fresh_spool_and_foreign_queries_are_pure() {
    let pcm = pcm();
    let plan = roster(prepared(pcm.clone()), 3);
    let limits = Limits::for_plan(&plan, 8, 8);
    let (mut rt, mut control) = Runtime::with_plan_updates(plan, limits, 2, 1).unwrap();
    let first = rt.active_plan();
    drain(&mut rt);
    control.submit(Box::new(prepared(pcm.clone()))).unwrap();
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
    });
    let retired = control.retired().unwrap();
    assert_eq!(rt.startup_work_pending(first), Err(Error::StaleHandle));
    let old = rt.group_residency_progress(rt.active_plan(), 0).unwrap();
    control.submit(retired.prepared).unwrap();
    // Preparation happened off audio while this exact current owner remained.
    assert_eq!(
        rt.group_residency_progress(rt.active_plan(), 0).unwrap(),
        old
    );
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
    });
    assert_eq!(
        rt.residency_progress(rt.active_plan())
            .unwrap()
            .startup_completions_pending,
        3
    );
    drain(&mut rt);
    assert!(rt.group_resident(rt.active_plan(), 0).unwrap());
}

#[test]
fn panic_cancels_undelivered_completions_without_a_late_success_or_audio_drop() {
    let callback = Program::new(vec![
        Instruction::SetLocal {
            local: 0,
            value: 99,
        },
        Instruction::WriteScriptCell { cell: 0, local: 0 },
        Instruction::End,
    ])
    .unwrap()
    .with_wait_lifetime(WaitLifetime::Callback)
    .with_script_instance(ScriptInstanceId(0));
    let plan = roster(
        prepared(pcm())
            .with_programs(vec![callback], None)
            .unwrap()
            .with_stages(vec![Stage::default()])
            .unwrap()
            .with_signal_programs(vec![SignalProgram {
                signal: MIDI_ASYNC_SIGNAL,
                program: 0,
                stage: 0,
            }])
            .unwrap(),
        8,
    );
    let limits = Limits::for_plan(&plan, 8, 8);
    let mut rt = Runtime::new(plan, limits).unwrap();
    assert!(rt.startup_work_pending(rt.active_plan()).unwrap());
    support::without_heap(|| {
        rt.panic();
        rt.render(&mut []).unwrap();
    });
    assert!(!rt.startup_work_pending(rt.active_plan()).unwrap());
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
        Ok(0)
    );
}

#[test]
fn invalid_rosters_reject_before_mutating_physical_or_group_authority() {
    let pcm = pcm();
    let _keeper = prepared(pcm.clone());
    let plan = prepared(pcm.clone());
    let owners = pcm.residency_owners();
    let request = InitialGroupResidency {
        source_group: 0,
        reload: false,
        async_id: 1,
        instance: ScriptInstanceId(0),
    };
    let mut midi = MidiObject::default();
    midi.reserve_async_ids_through(1).unwrap();
    assert!(matches!(
        plan.with_midi_object(midi)
            .with_initial_group_residency(vec![request]),
        Err(Error::InvalidInput)
    ));
    assert_eq!(pcm.residency_owners(), owners - 1);
    assert!(pcm.resident_frames().is_some());
}

#[test]
fn callback_capacity_retries_a_later_start_instead_of_losing_it() {
    let waiting = Program::new(vec![Instruction::Wait(1), Instruction::End])
        .unwrap()
        .with_wait_lifetime(WaitLifetime::Callback)
        .with_script_instance(ScriptInstanceId(0));
    let later = Program::new(vec![
        Instruction::SetLocal {
            local: 0,
            value: 17,
        },
        Instruction::WriteScriptCell { cell: 0, local: 0 },
        Instruction::End,
    ])
    .unwrap()
    .with_wait_lifetime(WaitLifetime::Callback)
    .with_script_instance(ScriptInstanceId(0));
    let plan = prepared(pcm())
        .with_programs(vec![waiting, later], None)
        .unwrap()
        .with_stages(vec![Stage::default()])
        .unwrap()
        .with_plan_programs(vec![
            PlanProgram {
                program: 0,
                stage: 0,
            },
            PlanProgram {
                program: 1,
                stage: 0,
            },
        ])
        .unwrap();
    let mut budget = Limits::for_plan(&plan, 8, 8);
    budget.behaviors = 1;
    budget.behavior_cells = plan.behavior_local_count();
    let mut rt = Runtime::new(plan, budget).unwrap();
    assert!(rt.startup_work_pending(rt.active_plan()).unwrap());
    support::without_heap(|| rt.render(&mut [[0.; 2]; 1]).unwrap());
    // A boundary starts programs before applying due resumes. The waiting
    // callback completes at now=1, then the next boundary retries the start.
    drain(&mut rt);
    assert!(!rt.startup_work_pending(rt.active_plan()).unwrap());
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
        Ok(17)
    );
}

#[test]
fn cancellation_during_a_worker_reload_never_publishes_its_completed_result() {
    let source = Pcm::streamed(48_000, 128).unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let canceled = Arc::new(AtomicBool::new(false));
    let (worker_entered, worker_release) = (entered.clone(), release.clone());
    source
        .set_residency_loader(move || {
            worker_entered.store(true, Release);
            while !worker_release.load(Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(vec![(0, vec![[0.125; 2]; 128].into())])
        })
        .unwrap();
    let plan = roster(prepared(source.clone()), 1);
    let limits = Limits::for_plan(&plan, 8, 8);
    let thread = std::thread::spawn({
        let canceled = canceled.clone();
        move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !entered.load(Acquire) && Instant::now() < deadline {
                std::thread::yield_now();
            }
            canceled.store(true, Release);
            release.store(true, Release);
        }
    });
    assert!(matches!(
        Runtime::new_cancelable(plan, limits, &|| canceled.load(Acquire)),
        Err(Error::Canceled)
    ));
    thread.join().unwrap();
    assert_eq!(source.residency_owners(), 0);
    assert!(!source.resident_covers(0..128));
}

#[test]
fn metadata_count_limit_rejects_before_changing_a_retained_source() {
    let pcm = pcm();
    let _keeper = prepared(pcm.clone());
    let plan = prepared(pcm.clone());
    let owners = pcm.residency_owners();
    let n = InitialGroupResidency::MAX_REQUESTS + 1;
    let mut midi = MidiObject::default();
    midi.reserve_async_ids_through(n as i32).unwrap();
    let requests = (0..n)
        .map(|i| InitialGroupResidency {
            source_group: 37,
            reload: false,
            async_id: (i + 1) as i32,
            instance: ScriptInstanceId(0),
        })
        .collect();
    assert!(matches!(
        plan.with_midi_object(midi)
            .with_initial_group_residency(requests),
        Err(Error::Capacity)
    ));
    assert_eq!(pcm.residency_owners(), owners - 1);
    assert!(pcm.resident_covers(0..128));
}
