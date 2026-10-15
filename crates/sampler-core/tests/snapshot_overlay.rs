//! Authored audio/host admission boundaries; no installed instruments.
use sampler_core::*;
mod support;

const MODE: ControlId = ControlId(200);

fn prepared(tail: bool) -> Prepared {
    let base = if tail {
        Prepared::new(
            48_000,
            vec![Pcm::new(48_000, Box::from([[1.; 2]])).unwrap()],
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
        .with_buses(
            vec![Bus {
                processors: vec![Processor::Biquad(
                    Biquad::new(48_000, FilterKind::LowPass, 12_000., 1.).unwrap(),
                )],
                sends: vec![BusSend {
                    bus: None,
                    gain: 1.,
                }],
                tail_frames: 8,
            }],
            vec![Some(0)],
        )
        .unwrap()
    } else {
        Prepared::new(48_000, vec![], vec![], 0).unwrap()
    };
    base.with_controls(vec![ControlDefinition {
        id: MODE,
        domain: ControlDomain::Integer { min: 0, max: 100 },
        default: ControlValue::Integer(3),
    }])
    .unwrap()
}

fn runtime(tail: bool) -> Runtime {
    let prepared = prepared(tail);
    let limits = Limits::for_plan(&prepared, 4, 4);
    Runtime::new(prepared, limits).unwrap()
}

fn overlay(rt: &Runtime, value: i64) -> NativeSnapshotOverlay {
    NativeSnapshotOverlay::new(
        rt.snapshot_overlay_fence(rt.active_plan()).unwrap(),
        ScriptStateBuffer::default(),
        Box::from([ControlWrite {
            id: MODE,
            value: ControlValue::Integer(value),
        }]),
    )
}

fn input() -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(17),
    }
}

fn leave_tail(rt: &mut Runtime) {
    rt.trigger(input(), 60, 1.).unwrap();
    rt.render(&mut [[0.; 2]; 1]).unwrap();
    rt.note_off(input(), None).unwrap();
    rt.flush_ended(|ended| {
        assert_eq!(ended, input());
        true
    });
    assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
}

#[test]
fn snapshot_overlay_refuses_lingering_audio_tail_without_changing_its_samples() {
    assert_lingering_tail_rejected(false);
}

#[test]
fn snapshot_overlay_refuses_an_older_generations_lingering_audio_tail() {
    assert_lingering_tail_rejected(true);
}

fn assert_lingering_tail_rejected(replace_plan: bool) {
    let make = || {
        let prepared = prepared(true);
        let limits = Limits::for_plan(&prepared, 4, 4);
        Runtime::with_plan_updates(prepared, limits, 2, 1).unwrap()
    };
    let (mut rt, mut plans) = make();
    let (mut reference, mut reference_plans) = make();
    support::without_heap(|| {
        leave_tail(&mut rt);
        leave_tail(&mut reference);
    });
    if replace_plan {
        let old = rt.active_plan();
        let request = plans.submit(Box::new(prepared(false))).unwrap();
        let reference_request = reference_plans.submit(Box::new(prepared(false))).unwrap();
        support::without_heap(|| {
            assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
            assert_eq!(reference.poll_plan_update(), Ok(Some(reference_request)));
            assert_ne!(rt.active_plan(), old);
            assert_eq!(
                rt.plan_count(),
                2,
                "old tail retains its prepared generation"
            );
        });
    }
    let plan = rt.active_plan();
    let identity = rt.id();
    let fence = rt.snapshot_overlay_fence(plan).unwrap();
    let mut incoming = overlay(&rt, 99);
    support::without_heap(|| {
        assert_eq!(
            rt.apply_snapshot_overlay(&mut incoming),
            Err(Error::NotReady)
        );
        assert_eq!(incoming.refusal, Some(SnapshotOverlayRefusal::AudioTail));
        assert!(incoming.committed.is_none());
        assert_eq!(rt.snapshot_overlay_fence(plan).unwrap(), fence);
        assert_eq!(rt.control_value(plan, MODE), Ok(ControlValue::Integer(3)));
        assert_eq!(rt.id(), identity);
        let mut actual = [[0.; 2]; 9];
        let mut expected = [[0.; 2]; 9];
        rt.render(&mut actual).unwrap();
        reference.render(&mut expected).unwrap();
        assert!(actual.iter().any(|frame| frame[0] != 0.));
        assert_eq!(
            actual, expected,
            "refusal must neither clear nor advance the old tail"
        );
        assert_eq!(rt.apply_snapshot_overlay(&mut incoming), Ok((1, 1)));
        assert_eq!(rt.control_value(plan, MODE), Ok(ControlValue::Integer(99)));
    });
}

fn submit(client: &mut ControlClient, incoming: NativeSnapshotOverlay) -> u64 {
    client
        .submit(ControlRequest {
            plan: incoming.expected.plan,
            expected_revision: None,
            operation: ControlOperation::NativeSnapshotOverlay(incoming),
        })
        .unwrap()
}

#[test]
fn snapshot_overlay_host_gate_is_pure_once_and_unused_for_ordinary_controls() {
    let rt = runtime(false);
    let (mut rt, mut client) = rt.with_control_updates(2, 8).unwrap();
    let plan = rt.active_plan();
    let fence = rt.snapshot_overlay_fence(plan).unwrap();
    let request = submit(&mut client, overlay(&rt, 99));
    let mut calls = 0;
    support::without_heap(|| {
        assert_eq!(
            rt.poll_control_update_with_snapshot_gate(|| {
                calls += 1;
                false
            }),
            Ok(Some(request))
        );
        assert_eq!(calls, 1);
        assert_eq!(rt.snapshot_overlay_fence(plan).unwrap(), fence);
        assert_eq!(rt.control_value(plan, MODE), Ok(ControlValue::Integer(3)));
        assert_eq!(rt.now(), 0);
    });
    let reply = client.reply().unwrap();
    assert_eq!(reply.result, Err(Error::NotReady));
    let ControlOperation::NativeSnapshotOverlay(incoming) = reply.command.operation else {
        panic!()
    };
    assert_eq!(incoming.refusal, Some(SnapshotOverlayRefusal::HostState));
    assert!(incoming.committed.is_none());
    client
        .submit(ControlRequest {
            plan,
            expected_revision: None,
            operation: ControlOperation::Edit(Box::from([ControlWrite {
                id: MODE,
                value: ControlValue::Integer(7),
            }])),
        })
        .unwrap();
    support::without_heap(|| {
        rt.poll_control_update_with_snapshot_gate(|| {
            panic!("ordinary edits never inspect host snapshot state")
        })
        .unwrap();
        assert_eq!(rt.control_value(plan, MODE), Ok(ControlValue::Integer(7)));
    });
    assert!(client.reply().unwrap().result.is_ok());
}

#[test]
fn snapshot_overlay_preflights_before_host_gate_without_applying_due_work() {
    let rt = runtime(false);
    let (mut rt, mut client) = rt.with_control_updates(2, 8).unwrap();
    let plan = rt.active_plan();
    submit(&mut client, overlay(&rt, 101));
    support::without_heap(|| {
        rt.poll_control_update_with_snapshot_gate(|| {
            panic!("invalid payload never reaches host gate")
        })
        .unwrap();
    });
    let reply = client.reply().unwrap();
    assert_eq!(reply.result, Err(Error::InvalidInput));
    let ControlOperation::NativeSnapshotOverlay(incoming) = reply.command.operation else {
        panic!()
    };
    assert_eq!(incoming.refusal, Some(SnapshotOverlayRefusal::InvalidInput));
    rt.schedule_event(
        1,
        Event::Control(
            plan,
            ControlWrite {
                id: MODE,
                value: ControlValue::Integer(9),
            },
        ),
    )
    .unwrap();
    rt.render(&mut [[0.; 2]; 1]).unwrap();
    let fence = rt.snapshot_overlay_fence(plan).unwrap();
    assert_eq!(rt.control_value(plan, MODE), Ok(ControlValue::Integer(3)));
    submit(&mut client, overlay(&rt, 99));
    support::without_heap(|| {
        rt.poll_control_update_with_snapshot_gate(|| {
            panic!("pending work never reaches host gate")
        })
        .unwrap();
        assert_eq!(rt.snapshot_overlay_fence(plan).unwrap(), fence);
        assert_eq!(rt.control_value(plan, MODE), Ok(ControlValue::Integer(3)));
        assert_eq!(rt.now(), 1);
    });
    let reply = client.reply().unwrap();
    assert_eq!(reply.result, Err(Error::NotReady));
    let ControlOperation::NativeSnapshotOverlay(incoming) = reply.command.operation else {
        panic!()
    };
    assert_eq!(
        incoming.refusal,
        Some(SnapshotOverlayRefusal::PendingCommands)
    );
    support::without_heap(|| {
        rt.render(&mut []).unwrap();
        assert_eq!(rt.control_value(plan, MODE), Ok(ControlValue::Integer(9)));
    });
}
