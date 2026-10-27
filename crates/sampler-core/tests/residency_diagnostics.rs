use sampler_core::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering::*},
};
use std::time::{Duration, Instant};
mod support;

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
        2,
    )
    .unwrap()
    .with_groups(1, vec![Some(0)])
    .unwrap()
    .with_source_groups(vec![37])
    .unwrap()
    .with_script_instances(vec![vec![]])
    .unwrap()
    .with_stages(vec![Stage::default()])
    .unwrap()
}
fn runtime(plan: Prepared) -> Runtime {
    let limits = Limits::for_plan(&plan, 8, 8);
    Runtime::new(plan, limits).unwrap()
}
fn until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "residency diagnostic fixture timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn settle(rt: &mut Runtime) {
    until(|| {
        support::without_heap(|| rt.service_residency().unwrap());
        rt.residency_progress(rt.active_plan()).unwrap().pending == 0
    });
}
struct Gate(Arc<AtomicBool>);
impl Drop for Gate {
    fn drop(&mut self) {
        self.0.store(true, Release);
    }
}
fn gated_pcm() -> (Pcm, Arc<AtomicBool>, Gate) {
    let pcm = Pcm::new(48_000, vec![[0.25; 2]; 128].into()).unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let (worker_entered, worker_release) = (entered.clone(), release.clone());
    pcm.set_residency_loader(move || {
        worker_entered.store(true, Release);
        while !worker_release.load(Acquire) {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(vec![(0, vec![[0.25; 2]; 128].into())])
    })
    .unwrap();
    (pcm, entered, Gate(release))
}

#[test]
fn worker_ready_is_not_audio_committed_and_queries_do_not_advance_jobs() {
    let (pcm, entered, gate) = gated_pcm();
    let mut rt = runtime(prepared(pcm));
    // Drop the unblock guard before Runtime joins its worker on a test panic.
    let unblock = gate;
    let plan = rt.active_plan();
    rt.request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    settle(&mut rt);
    rt.request_group_residency(plan, 0, true, ScriptInstanceId(0))
        .unwrap();
    until(|| entered.load(Acquire));
    support::without_heap(|| {
        let p = rt.residency_progress(plan).unwrap();
        assert_eq!((p.pending, p.in_flight, p.worker_ready), (1, 1, 0));
        assert_eq!((p.accepted, p.consumed, p.committed), (2, 1, 1));
        assert_eq!(p.canceled, None);
        let g = rt.group_residency_progress(plan, 0).unwrap();
        assert_eq!((g.group, g.source_group, g.asset_count), (0, 37, Some(1)));
        assert_eq!(
            (g.loaded, g.pending, g.latest_requested_reload),
            (false, 1, Some(true))
        );
    });
    unblock.0.store(true, Release);
    until(|| rt.residency_progress(plan).unwrap().worker_ready == 1);
    let p = rt.residency_progress(plan).unwrap();
    assert_eq!(
        (p.pending, p.worker_ready, p.consumed, p.committed),
        (1, 1, 1, 1)
    );
    assert!(!rt.group_resident(plan, 0).unwrap());
    assert!(!rt.group_residency_progress(plan, 0).unwrap().loaded);
    settle(&mut rt);
    let p = rt.residency_progress(plan).unwrap();
    assert_eq!(
        (
            p.pending,
            p.accepted,
            p.consumed,
            p.committed,
            p.failed,
            p.stale
        ),
        (0, 2, 2, 2, 0, 0)
    );
    assert!(rt.group_residency_progress(plan, 0).unwrap().loaded);
}

#[test]
fn queue_rejection_survives_settlement_and_a_fresh_note_reaches_audio() {
    let (pcm, entered, gate) = gated_pcm();
    let mut rt = runtime(prepared(pcm));
    // Drop the unblock guard before Runtime joins its worker on a test panic.
    let unblock = gate;
    let plan = rt.active_plan();
    rt.request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    settle(&mut rt);
    rt.request_group_residency(plan, 0, true, ScriptInstanceId(0))
        .unwrap();
    until(|| entered.load(Acquire));
    let capacity = rt.residency_progress(plan).unwrap().job_capacity;
    assert_eq!(capacity, 1024);
    support::without_heap(|| {
        for _ in 1..capacity {
            rt.request_group_residency(plan, 0, true, ScriptInstanceId(0))
                .unwrap();
        }
        assert_eq!(
            rt.request_group_residency(plan, 0, true, ScriptInstanceId(0)),
            Err(Error::Capacity)
        );
        let p = rt.residency_progress(plan).unwrap();
        assert_eq!(
            (p.pending, p.queued, p.in_flight),
            (capacity, capacity - 1, 1)
        );
        assert_eq!((p.rejected, p.rejected_queue), (1, 1));
        assert_eq!(
            p.first_rejection,
            Some(ResidencyRejection {
                group: 0,
                reload: true,
                reason: ResidencyRejectionReason::QueueCapacity,
            })
        );
    });
    unblock.0.store(true, Release);
    settle(&mut rt);
    assert_eq!(
        rt.request_group_residency(plan, 1, false, ScriptInstanceId(0)),
        Err(Error::InvalidInput)
    );
    let p = rt.residency_progress(plan).unwrap();
    assert_eq!(
        (p.pending, p.accepted, p.consumed, p.committed, p.stale),
        (0, 1025, 1025, 2, 1023)
    );
    assert_eq!((p.rejected, p.rejected_queue, p.rejected_group), (2, 1, 1));
    assert_eq!(
        p.first_rejection.unwrap().reason,
        ResidencyRejectionReason::QueueCapacity
    );
    let mut audio = [[0.; 2]; 8];
    support::without_heap(|| {
        let note = rt
            .note_on(
                Input {
                    protocol: Protocol::Clap,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: Some(999),
                },
                60,
                1.,
            )
            .unwrap();
        rt.forward_attack(note).unwrap();
        rt.render(&mut audio).unwrap();
    });
    assert!(audio.iter().any(|f| f[0] != 0.));
    assert_eq!(rt.voice_count(), 1);
}

#[test]
fn failed_reload_is_distinct_from_superseded_completion() {
    let pcm = Pcm::new(48_000, vec![[0.25; 2]; 128].into()).unwrap();
    pcm.set_residency_loader(|| Err(Error::NotReady)).unwrap();
    let mut rt = runtime(prepared(pcm));
    let plan = rt.active_plan();
    rt.request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    settle(&mut rt);
    rt.request_group_residency(plan, 0, true, ScriptInstanceId(0))
        .unwrap();
    settle(&mut rt);
    let p = rt.residency_progress(plan).unwrap();
    assert_eq!(
        (p.accepted, p.consumed, p.committed, p.failed, p.stale),
        (2, 2, 1, 1, 0)
    );
    let g = rt.group_residency_progress(plan, 0).unwrap();
    assert_eq!(
        (
            g.loaded,
            g.pending,
            g.latest_requested_reload,
            g.retention_owned
        ),
        (false, 0, Some(true), Some(false))
    );

    let (pcm, entered, gate) = gated_pcm();
    let mut rt = runtime(prepared(pcm));
    // Drop the unblock guard before Runtime joins its worker on a test panic.
    let unblock = gate;
    let plan = rt.active_plan();
    rt.request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    settle(&mut rt);
    rt.request_group_residency(plan, 0, true, ScriptInstanceId(0))
        .unwrap();
    until(|| entered.load(Acquire));
    rt.request_group_residency(plan, 0, false, ScriptInstanceId(0))
        .unwrap();
    unblock.0.store(true, Release);
    settle(&mut rt);
    let p = rt.residency_progress(plan).unwrap();
    assert_eq!(
        (p.accepted, p.consumed, p.committed, p.failed, p.stale),
        (3, 3, 2, 0, 1)
    );
    assert!(!rt.group_resident(plan, 0).unwrap());
}

#[test]
fn foreign_and_invalid_queries_are_pure_and_physical_group_ids_remain_exact() {
    let rt = runtime(prepared(
        Pcm::new(48_000, vec![[0.25; 2]; 128].into()).unwrap(),
    ));
    let other = runtime(Prepared::new(48_000, vec![], vec![], 0).unwrap());
    let plan = rt.active_plan();
    let before = rt.residency_progress(plan).unwrap();
    support::without_heap(|| {
        assert_eq!(
            rt.residency_progress(other.active_plan()),
            Err(Error::StaleHandle)
        );
        assert_eq!(
            rt.group_residency_progress(other.active_plan(), 0),
            Err(Error::StaleHandle)
        );
        assert_eq!(
            rt.group_residency_progress(plan, 37),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            rt.group_residency_progress(plan, 0).unwrap().source_group,
            37
        );
        assert_eq!(rt.residency_progress(plan).unwrap(), before);
    });
    assert!(
        !other
            .residency_progress(other.active_plan())
            .unwrap()
            .managed
    );
}

#[test]
fn runtime_startup_requests_are_pending_even_before_the_part_is_published() {
    let purge = Program::new(vec![
        Instruction::SetLocal {
            local: 0,
            value: 37,
        },
        Instruction::SetLocal { local: 1, value: 0 },
        Instruction::Op(Op::Purge {
            group: 0,
            local: 1,
            write: true,
        }),
        Instruction::End,
    ])
    .unwrap()
    .with_script_instance(ScriptInstanceId(0))
    .with_wait_lifetime(WaitLifetime::Callback);
    let plan = prepared(Pcm::new(48_000, vec![[0.25; 2]; 128].into()).unwrap())
        .with_programs(vec![purge], None)
        .unwrap()
        .with_plan_programs(vec![PlanProgram {
            program: 0,
            stage: 0,
        }])
        .unwrap();
    let mut rt = runtime(plan);
    let plan = rt.active_plan();
    let p = rt.residency_progress(plan).unwrap();
    assert_eq!((p.accepted, p.pending, p.consumed), (1, 1, 0));
    assert!(!rt.group_resident(plan, 0).unwrap());
    settle(&mut rt);
    let p = rt.residency_progress(plan).unwrap();
    assert_eq!((p.pending, p.consumed, p.committed), (0, 1, 1));
    assert!(!rt.group_residency_progress(plan, 0).unwrap().loaded);
}

#[test]
fn startup_purge_refusal_is_observable_even_when_the_script_gets_minus_one_without_a_fault() {
    let mut code = vec![Instruction::SetLocal {
        local: 0,
        value: 37,
    }];
    for _ in 0..1025 {
        code.extend([
            Instruction::SetLocal { local: 1, value: 0 },
            Instruction::Op(Op::Purge {
                group: 0,
                local: 1,
                write: true,
            }),
        ]);
    }
    code.push(Instruction::End);
    let purge = Program::new(code)
        .unwrap()
        .with_script_instance(ScriptInstanceId(0))
        .with_wait_lifetime(WaitLifetime::Callback);
    let plan = prepared(Pcm::new(48_000, vec![[0.25; 2]; 128].into()).unwrap())
        .with_programs(vec![purge], None)
        .unwrap()
        .with_plan_programs(vec![PlanProgram {
            program: 0,
            stage: 0,
        }])
        .unwrap();
    let mut rt = runtime(plan);
    let plan = rt.active_plan();
    assert_eq!(rt.take_fault(), None);
    let p = rt.residency_progress(plan).unwrap();
    assert_eq!(
        (p.accepted, p.pending, p.consumed, p.rejected_queue),
        (1024, 1024, 0, 1)
    );
    assert_eq!(
        p.first_rejection.unwrap().reason,
        ResidencyRejectionReason::QueueCapacity
    );
    settle(&mut rt);
    let p = rt.residency_progress(plan).unwrap();
    assert_eq!(
        (
            p.pending,
            p.consumed,
            p.committed,
            p.stale,
            p.rejected_queue
        ),
        (0, 1024, 1, 1023, 1)
    );
    assert_eq!(rt.take_fault(), None);
}
