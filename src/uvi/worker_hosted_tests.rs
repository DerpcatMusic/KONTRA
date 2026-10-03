use super::super::program::parse_program;
use super::*;
fn stamp(frame: u64) -> Stamp {
    Stamp {
        epoch: 7,
        generation: 9,
        frame,
    }
}
fn root(token: u64) -> HostRoot {
    HostRoot {
        epoch: 7,
        generation: 9,
        token,
    }
}
fn completion(frame: u64, token: u64) -> StampedCompletion {
    StampedCompletion {
        stamp: stamp(frame),
        root: root(token),
    }
}
fn output(frame: u64) -> Output {
    Output {
        stamp: stamp(frame),
        audio: [[0.25; 2]; BLOCK_FRAMES],
        commands: 0,
        host_commands: 0,
        logs: 0,
        dropped_logs: 0,
        rejected_ui: 0,
    }
}
// Original authored UFS fixture, opened through the production bounded bank
// capability. No private prepared-Player constructor or commercial resources.
struct Fixture {
    program: super::super::program::Program,
    resources: Option<BankResources>,
    path: PathBuf,
}
impl Fixture {
    fn player(&mut self, activation: Option<(u64, u64)>) -> Player<'_> {
        let resources = self.resources.take().unwrap();
        match activation {
            Some((epoch, generation)) => Player::new_hosted(
                &self.program,
                Default::default(),
                resources,
                48000,
                epoch,
                generation,
            )
            .unwrap(),
            None => Player::new(&self.program, Default::default(), resources, 48000).unwrap(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.resources.take());
        std::fs::remove_file(&self.path).unwrap();
    }
}
fn fixture() -> Fixture {
    let (config, _) = super::tests::authored_bank_with_script(
        r#"
        function onInit()
            knob=Knob{name='authored',value=.25}
            knob.changed=function()error('UI prefix executed before admission')end
        end
        function onNote(e)postEvent(e);wait(1);playNote(72,90,1)end
        function onRelease(e)postEvent(e)end
    "#,
    );
    let library = Rc::new(Library::open(&config.bank, &config.metadata_namespace, None).unwrap());
    let loaded = library
        .program(&config.member, &config.program_namespace)
        .unwrap();
    let resources = BankResources::new(
        library.clone(),
        &loaded.path,
        library.samples(&loaded).unwrap(),
    )
    .unwrap();
    Fixture {
        program: loaded.program,
        resources: Some(resources),
        path: config.bank,
    }
}
fn check_callback(check: impl FnMut()) {
    #[cfg(feature = "plugin")]
    assert_eq!(crate::plugin::tests::allocations(check), 0);
    #[cfg(not(feature = "plugin"))]
    {
        let mut check = check;
        check();
    }
}
fn note(frame: u64) -> Input {
    Input {
        frame,
        kind: InputKind::NoteOn {
            channel: 0,
            note: 60,
            velocity: 100,
        },
    }
}
fn hosted_packet(frame: u64, token: u64) -> HostedRequest {
    HostedRequest::new(
        Request::new(stamp(frame), &[]).unwrap(),
        &[
            HostedInput::On {
                root: root(token),
                input: note(frame),
            },
            HostedInput::Off {
                root: root(token),
                frame: frame + 4,
            },
        ],
    )
    .unwrap()
}
fn played_end(player: &mut Player<'_>) -> super::super::player::Rendered {
    player
        .render_hosted(
            &[],
            &[
                HostedInput::On {
                    root: root(1),
                    input: Input {
                        frame: 0,
                        kind: InputKind::NoteOn {
                            channel: 0,
                            note: 60,
                            velocity: 100,
                        },
                    },
                },
                HostedInput::Off {
                    root: root(1),
                    frame: 4,
                },
            ],
            BLOCK_FRAMES,
        )
        .unwrap()
}
#[test]
fn completion_real_hosted_player_queue_full_retry_acknowledges_only_durable_transfer() {
    let mut fixture = fixture();
    let mut player = fixture.player(Some((7, 9)));
    let ended = played_end(&mut player);
    assert!(ended.audio.iter().any(|sample| sample[0] != 0.0));
    assert_eq!(ended.host_completions.len(), 1);
    assert_eq!(ended.host_completions[0].frame, 256);
    let shared = Shared::new_mode(7, 9, true);
    for token in 10..10 + COMPLETION_CAPACITY as u64 {
        shared
            .hosted
            .as_ref()
            .unwrap()
            .completions
            .push(completion(256, token))
            .unwrap()
    }
    let mut pending = ended.host_completions;
    assert_eq!(
        transfer_completions(&mut player, &shared, &mut pending).unwrap(),
        0
    );
    assert_eq!(pending.len(), 1);
    let retried = player.render(&[], BLOCK_FRAMES).unwrap();
    assert_eq!(retried.host_completions, pending);
    refresh_completions(&mut pending, retried.host_completions).unwrap();
    shared.hosted.as_ref().unwrap().completions.pop().unwrap();
    assert_eq!(
        transfer_completions(&mut player, &shared, &mut pending).unwrap(),
        1
    );
    assert!(pending.is_empty());
    assert!(
        player
            .render(&[], BLOCK_FRAMES)
            .unwrap()
            .host_completions
            .is_empty()
    );
    let delivered = (0..COMPLETION_CAPACITY)
        .filter_map(|_| shared.hosted.as_ref().unwrap().completions.pop())
        .find(|packet| packet.root == root(1))
        .unwrap();
    assert_eq!(delivered.stamp, stamp(256));
}
#[test]
fn completion_late_pcm_discard_does_not_discard_root_and_future_is_inline() {
    let shared = Arc::new(Shared::new_mode(7, 9, true));
    shared.outputs.push(output(0)).unwrap();
    shared.outputs.push(output(256)).unwrap();
    shared
        .hosted
        .as_ref()
        .unwrap()
        .completions
        .push(completion(256, 1))
        .unwrap();
    shared
        .hosted
        .as_ref()
        .unwrap()
        .completions
        .push(completion(1024, 2))
        .unwrap();
    let mut worker = Worker {
        shared,
        thread: None,
        cursor: Some(PacketCursor::default()),
    };
    let mut port = worker.take_audio_port().unwrap();
    assert_eq!(
        worker
            .realtime()
            .try_receive_completion(stamp(512))
            .unwrap_err(),
        PacketError::PortTaken
    );
    let mut rt = port.realtime();
    assert_eq!(
        rt.try_receive(stamp(512)).unwrap_err(),
        PacketError::Underrun
    );
    assert_eq!(
        rt.try_receive_completion(stamp(512)).unwrap(),
        Some(completion(256, 1))
    );
    assert_eq!(rt.try_receive_completion(stamp(512)).unwrap(), None);
    assert_eq!(rt.try_receive_completion(stamp(1023)).unwrap(), None);
    assert_eq!(
        rt.try_receive_completion(stamp(1024)).unwrap(),
        Some(completion(1024, 2))
    );
    assert_eq!(
        rt.try_receive_completion(stamp(1023)).unwrap_err(),
        PacketError::WrongFrame
    );
    assert_eq!(rt.stats().stale_packets, 2);
}
#[test]
fn completion_stale_activation_is_rejected_with_bounded_zero_allocation_poll() {
    let shared = Arc::new(Shared::new_mode(7, 9, true));
    for token in 1..=COMPLETION_CAPACITY as u64 {
        let mut packet = completion(1, token);
        if token % 2 == 0 {
            packet.stamp.epoch = 6
        } else {
            packet.stamp.generation = 8
        }
        shared
            .hosted
            .as_ref()
            .unwrap()
            .completions
            .push(packet)
            .unwrap();
    }
    let mut worker = Worker {
        shared,
        thread: None,
        cursor: Some(PacketCursor::default()),
    };
    let mut port = worker.take_audio_port().unwrap();
    check_callback(|| {
        let mut rt = port.realtime();
        assert_eq!(
            rt.try_receive_completion(Stamp {
                epoch: 6,
                ..stamp(256)
            })
            .unwrap_err(),
            PacketError::WrongEpoch
        );
        assert_eq!(
            rt.try_receive_completion(Stamp {
                generation: 8,
                ..stamp(256)
            })
            .unwrap_err(),
            PacketError::WrongGeneration
        );
        assert_eq!(rt.try_receive_completion(stamp(256)).unwrap(), None);
        assert_eq!(rt.try_receive_completion(stamp(256)).unwrap(), None);
    });
    assert_eq!(port.realtime().stats().stale_packets, 4096);
}
#[test]
fn completion_callback_delivery_and_pcm_reads_do_not_allocate_or_free() {
    let shared = Arc::new(Shared::new_mode(7, 9, true));
    shared.outputs.push(output(0)).unwrap();
    shared
        .hosted
        .as_ref()
        .unwrap()
        .completions
        .push(completion(256, 1))
        .unwrap();
    shared
        .hosted
        .as_ref()
        .unwrap()
        .completions
        .push(completion(512, 2))
        .unwrap();
    let mut worker = Worker {
        shared,
        thread: None,
        cursor: Some(PacketCursor::default()),
    };
    let mut port = worker.take_audio_port().unwrap();
    check_callback(|| {
        let mut rt = port.realtime();
        assert_eq!(rt.try_receive(stamp(0)).unwrap().stamp, stamp(0));
        assert_eq!(rt.try_receive_completion(stamp(255)).unwrap(), None);
        assert_eq!(
            rt.try_receive_completion(stamp(256)).unwrap(),
            Some(completion(256, 1))
        );
        assert_eq!(
            rt.try_receive_completion(stamp(512)).unwrap(),
            Some(completion(512, 2))
        );
        assert_eq!(rt.try_receive_completion(stamp(512)).unwrap(), None);
    });
}
#[test]
fn completion_stop_keeps_destruction_off_callback_and_cancels_inline_storage() {
    let shared = Arc::new(Shared::new_mode(7, 9, true));
    shared
        .hosted
        .as_ref()
        .unwrap()
        .completions
        .push(completion(1024, 1))
        .unwrap();
    let mut worker = Worker {
        shared: shared.clone(),
        thread: None,
        cursor: Some(PacketCursor::default()),
    };
    assert!(
        worker
            .realtime()
            .try_receive_completion(stamp(256))
            .unwrap()
            .is_none()
    );
    worker.stop();
    assert_eq!(worker.stats().cancelled_completions, 1);
    assert_eq!(
        worker
            .realtime()
            .try_receive_completion(stamp(1024))
            .unwrap_err(),
        PacketError::Stopped
    );
}
#[test]
fn completion_authored_worker_retries_while_idle_and_survives_late_pcm() {
    let shared = Arc::new(Shared::new_mode(7, 9, true));
    for token in 10..10 + COMPLETION_CAPACITY as u64 {
        shared
            .hosted
            .as_ref()
            .unwrap()
            .completions
            .push(completion(256, token))
            .unwrap()
    }
    let producer = shared.clone();
    let handle = thread::spawn(move || {
        let mut fixture = fixture();
        let mut player = fixture.player(Some((7, 9)));
        producer
            .status
            .store(Status::Ready as u8, Ordering::Release);
        let result = serve(&mut player, &producer, 48000);
        finish(&producer, result.err().map(|error| format!("{error:#}")));
    });
    let mut worker = Worker {
        shared,
        thread: Some(handle),
        cursor: Some(PacketCursor::default()),
    };
    worker.wait_ready(Duration::from_secs(5)).unwrap();
    let mut port = worker.take_audio_port().unwrap();
    port.realtime()
        .try_submit_hosted(hosted_packet(0, 1))
        .unwrap();
    let start = Instant::now();
    while worker.stats().rendered_blocks == 0 {
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::yield_now();
    }
    assert_eq!(
        port.realtime().try_receive(stamp(512)).unwrap_err(),
        PacketError::Underrun
    );
    // Make space, then pause: no subsequent audio request drives this retry.
    assert!(
        port.realtime()
            .try_receive_completion(stamp(512))
            .unwrap()
            .is_some()
    );
    let mut seen = false;
    while !seen {
        assert!(start.elapsed() < Duration::from_secs(5));
        if let Some(packet) = port.realtime().try_receive_completion(stamp(512)).unwrap() {
            if packet.root == root(1) {
                assert_eq!(packet.stamp, stamp(256));
                seen = true;
            }
        } else {
            thread::yield_now();
        }
    }
    assert_eq!(worker.stats().rendered_blocks, 1);
    worker.stop();
    assert_eq!(worker.status(), Status::Stopped);
    drop(port); // Endpoint destruction also occurs here, outside callback scope.
}

#[test]
fn completion_legacy_transport_and_player_are_opted_out() {
    let shared = Arc::new(Shared::new(7, 9));
    assert!(shared.hosted.is_none());
    let mut worker = Worker {
        shared,
        thread: None,
        cursor: Some(PacketCursor::default()),
    };
    let mut rt = worker.realtime();
    assert_eq!(
        rt.try_submit_hosted(hosted_packet(0, 1))
            .unwrap_err()
            .reason,
        PacketError::InvalidInput
    );
    assert_eq!(
        rt.try_receive_completion(stamp(0)).unwrap_err(),
        PacketError::InvalidInput
    );
    rt.try_submit(Request::new(stamp(0), &[note(0)]).unwrap())
        .unwrap();
    assert_eq!(rt.cursor.as_ref().unwrap().next_request, 256);
    assert_eq!(rt.shared.requests.len(), 1);
    let mut fixture = fixture();
    let mut player = fixture.player(None);
    assert!(
        player
            .render(&[note(0)], 256)
            .unwrap()
            .host_completions
            .is_empty()
    );
    assert!(player.render_hosted_with_ui(&[], &[], &[], 256).is_err());
    assert!(player.acknowledge_host_completions(&[root(1)]).is_err());
}
#[test]
fn completion_hosted_submission_full_retry_preserves_packet_and_cursor_without_allocations() {
    let shared = Arc::new(Shared::new_mode(7, 9, true));
    let mut worker = Worker {
        shared: shared.clone(),
        thread: None,
        cursor: Some(PacketCursor::default()),
    };
    let mut port = worker.take_audio_port().unwrap();
    assert_eq!(
        worker
            .realtime()
            .try_submit_hosted(hosted_packet(0, 1))
            .unwrap_err()
            .reason,
        PacketError::PortTaken
    );
    assert_eq!(
        port.realtime()
            .try_submit(Request::new(stamp(0), &[]).unwrap())
            .unwrap_err()
            .reason,
        PacketError::InvalidInput
    );
    check_callback(|| {
        let mut rt = port.realtime();
        for n in 0..QUEUE_CAPACITY {
            rt.try_submit_hosted(hosted_packet(n as u64 * 256, n as u64 + 1))
                .unwrap();
        }
        let failed = rt.try_submit_hosted(hosted_packet(2048, 9)).unwrap_err();
        assert_eq!(failed.reason, PacketError::Full);
        assert_eq!(failed.request.request.stamp, stamp(2048));
        assert_eq!(failed.request.root_count, 2);
        assert_eq!(rt.cursor.as_ref().unwrap().next_request, 2048);
        assert!(matches!(
            failed.request.roots[0],
            HostedInput::On {
                root: HostRoot { token: 9, .. },
                ..
            }
        ));
        shared.hosted.as_ref().unwrap().requests.pop().unwrap();
        rt.try_submit_hosted(failed.request).unwrap();
        assert_eq!(rt.cursor.as_ref().unwrap().next_request, 2304);
    });
    assert_eq!(
        shared.hosted.as_ref().unwrap().requests.len(),
        QUEUE_CAPACITY
    );
    assert!(shared.requests.is_empty());
}
#[test]
fn completion_rooted_packet_validation_is_bounded_and_does_not_admit_tokens() {
    let mut packet = hosted_packet(0, 1);
    for mutation in 0..5 {
        let mut bad = packet;
        match mutation {
            0 => bad.root_count = u16::MAX,
            1 => {
                bad.roots[0] = HostedInput::On {
                    root: HostRoot {
                        generation: 8,
                        ..root(1)
                    },
                    input: note(0),
                }
            }
            2 => {
                bad.roots[1] = HostedInput::Off {
                    root: root(1),
                    frame: 256,
                }
            }
            3 => {
                bad.roots[0] = HostedInput::Off {
                    root: root(0),
                    frame: 0,
                }
            }
            _ => {
                bad.roots[1] = HostedInput::On {
                    root: root(1),
                    input: note(4),
                }
            }
        }
        assert!(bad.validate().is_err());
    }
    let inputs = [note(0); MAX_INPUTS];
    assert_eq!(
        HostedRequest::new(Request::new(stamp(0), &inputs).unwrap(), &packet.roots[..2])
            .unwrap_err(),
        PacketError::TooManyInputs
    );
    packet.roots[0] = HostedInput::On {
        root: root(10),
        input: note(0),
    };
    packet.roots[1] = HostedInput::On {
        root: root(11),
        input: note(0),
    };
    assert!(packet.validate().is_ok());
    // Unknown Off is structurally accepted; the Session owns lifetime admission.
    assert!(
        HostedRequest::new(
            Request::new(stamp(0), &[]).unwrap(),
            &[HostedInput::Off {
                root: root(999),
                frame: 0
            }]
        )
        .is_ok()
    );
}
#[test]
fn completion_authoritative_unknown_off_aborts_worker_without_executing_valid_prefix() {
    let mut fixture = fixture();
    let processor = fixture
        .program
        .nodes
        .iter()
        .position(|node| node.kind == "ScriptProcessor")
        .unwrap();
    let mut player = fixture.player(Some((7, 9)));
    let shared = Shared::new_mode(7, 9, true);
    shared.status.store(Status::Ready as u8, Ordering::Release);
    let packet = HostedRequest::new(
        Request::new_with_ui(
            stamp(0),
            &[note(0)],
            &[UiInput {
                frame: 0,
                edit: super::super::host::UiEdit {
                    processor,
                    widget: 1,
                    value: super::super::host::UiEditValue::Number(0.75),
                    modifiers: super::super::host::UiModifiers::default(),
                },
            }],
        )
        .unwrap(),
        &[
            HostedInput::On {
                root: root(1),
                input: note(0),
            },
            HostedInput::Off {
                root: root(999),
                frame: 4,
            },
        ],
    )
    .unwrap();
    shared
        .hosted
        .as_ref()
        .unwrap()
        .requests
        .push(packet)
        .unwrap();
    let failure = serve(&mut player, &shared, 48000).unwrap_err();
    assert_eq!(player.current_frame(), 0);
    assert!(matches!(
        player.ui_snapshot(processor).unwrap().widgets[0].value,
        Some(super::super::host::UiValue::Number(0.25))
    ));
    assert!(shared.outputs.is_empty());
    assert!(shared.hosted.as_ref().unwrap().completions.is_empty());
    assert_eq!(shared.stats().rendered_blocks, 0);
    // Read-only Session admission did not poison or consume the valid On token.
    assert_eq!(played_end(&mut player).host_completions.len(), 1);
    finish(&shared, Some(failure.to_string()));
    assert_eq!(shared.status(), Status::Failed);
    assert_eq!(
        shared.activation(stamp(0)).unwrap_err(),
        PacketError::Failed
    );
}
#[test]
fn completion_untransferred_suffix_survives_authoritative_census_refresh() {
    let mut fixture = fixture();
    let mut player = fixture.player(Some((7, 9)));
    let entries: Vec<_> = (1..=3)
        .flat_map(|token| {
            let frame = (token - 1) * 8;
            [
                HostedInput::On {
                    root: root(token),
                    input: note(frame),
                },
                HostedInput::Off {
                    root: root(token),
                    frame: frame + 4,
                },
            ]
        })
        .collect();
    let mut pending = player
        .render_hosted(&[], &entries, 256)
        .unwrap()
        .host_completions;
    assert_eq!(pending.len(), 3);
    let shared = Shared::new_mode(7, 9, true);
    for token in 10..10 + COMPLETION_CAPACITY as u64 - 1 {
        shared
            .hosted
            .as_ref()
            .unwrap()
            .completions
            .push(completion(256, token))
            .unwrap();
    }
    assert_eq!(
        transfer_completions(&mut player, &shared, &mut pending).unwrap(),
        1
    );
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].root, root(2));
    let next = player
        .render_hosted(
            &[],
            &[
                HostedInput::On {
                    root: root(4),
                    input: note(300),
                },
                HostedInput::Off {
                    root: root(4),
                    frame: 304,
                },
            ],
            256,
        )
        .unwrap()
        .host_completions;
    assert_eq!(next[..2], pending);
    assert_eq!(
        next[2],
        HostCompletion {
            root: root(4),
            frame: 512
        }
    );
    let original = pending.clone();
    assert!(refresh_completions(&mut pending, vec![next[1]]).is_err());
    assert_eq!(pending, original);
    let mut changed = next.clone();
    changed[0].frame += 1;
    assert!(refresh_completions(&mut pending, changed).is_err());
    assert_eq!(pending, original);
    refresh_completions(&mut pending, next).unwrap();
    shared.hosted.as_ref().unwrap().completions.pop();
    shared.hosted.as_ref().unwrap().completions.pop();
    shared.hosted.as_ref().unwrap().completions.pop();
    assert_eq!(
        transfer_completions(&mut player, &shared, &mut pending).unwrap(),
        3
    );
    assert!(pending.is_empty());
    assert!(player.render(&[], 256).unwrap().host_completions.is_empty());
}
#[test]
fn completion_duplicate_preflight_and_postpush_ack_failure_never_retry_record() {
    let mut fixture = fixture();
    let mut player = fixture.player(Some((7, 9)));
    let ended = played_end(&mut player).host_completions[0];
    let shared = Shared::new_mode(7, 9, true);
    let mut pending = vec![ended, ended];
    assert!(transfer_completions(&mut player, &shared, &mut pending).is_err());
    assert!(shared.hosted.as_ref().unwrap().completions.is_empty());
    assert!(refresh_completions(&mut Vec::new(), pending).is_err());
    // Corrupt/stale internal census: queue accepted the record, ledger rejects
    // its unknown root. This is an activation abort, never a retry/second push.
    let mut pending = vec![HostCompletion {
        root: root(999),
        frame: 256,
    }];
    assert!(transfer_completions(&mut player, &shared, &mut pending).is_err());
    assert!(pending.is_empty());
    assert_eq!(shared.status(), Status::Failed);
    assert_eq!(shared.hosted.as_ref().unwrap().completions.len(), 1);
    assert!(transfer_completions(&mut player, &shared, &mut pending).is_err());
    assert_eq!(shared.hosted.as_ref().unwrap().completions.len(), 1);
    finish(
        &shared,
        Some("authored acknowledgement invariant failure".into()),
    );
    assert_eq!(shared.stats().cancelled_completions, 1);
}

#[test]
fn completion_real_start_hosted_merges_ui_rooted_and_ordinary_inputs_once() {
    use super::super::host::{UiEdit, UiEditValue, UiModifiers, UiValue};
    let (config, source) = super::tests::authored_bank_with_script(
        r#"
        local count=0
        function onInit() knob=Knob{name='authored',value=.25} end
        function onNote(e) assert(knob.value==.75);count=count+1;postEvent(e) end
        function onRelease(e)postEvent(e)end
        function onController(e)assert(count==2)end
    "#,
    );
    let path = config.bank.clone();
    let p = parse_program(&source).unwrap();
    let processor = p
        .nodes
        .iter()
        .position(|node| node.kind == "ScriptProcessor")
        .unwrap();
    let mut worker = Worker::start_hosted(config, 7, 9).unwrap();
    worker.wait_ready(Duration::from_secs(5)).unwrap();
    assert!(worker.shared.hosted.is_some());
    let mut port = worker.take_audio_port().unwrap();
    let request = Request::new_with_ui(
        stamp(0),
        &[
            note(0),
            Input {
                frame: 4,
                kind: InputKind::NoteOff {
                    channel: 0,
                    note: 60,
                },
            },
            Input {
                frame: 8,
                kind: InputKind::Controller {
                    channel: 0,
                    controller: 1,
                    value: 64,
                },
            },
        ],
        &[UiInput {
            frame: 0,
            edit: UiEdit {
                processor,
                widget: 1,
                value: UiEditValue::Number(0.75),
                modifiers: UiModifiers::default(),
            },
        }],
    )
    .unwrap();
    let request = HostedRequest::new(request, &hosted_packet(0, 1).roots[..2]).unwrap();
    port.realtime().try_submit_hosted(request).unwrap();
    let started = Instant::now();
    let pcm = loop {
        match port.realtime().try_receive(stamp(0)) {
            Ok(pcm) => break pcm,
            Err(PacketError::Underrun) => {
                assert!(started.elapsed() < Duration::from_secs(5));
                thread::yield_now();
            }
            Err(error) => panic!("{error:?}: {:?}", worker.private_failure()),
        }
    };
    assert_eq!(pcm.rejected_ui, 0);
    assert!(pcm.audio.iter().any(|sample| sample[0] != 0.0));
    assert!(
        port.realtime()
            .try_receive_completion(stamp(255))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        port.realtime().try_receive_completion(stamp(256)).unwrap(),
        Some(completion(256, 1))
    );
    let id = worker.request_ui_snapshot(processor).unwrap();
    let snapshot = loop {
        if let Some(reply) = worker.poll_ui_snapshot() {
            assert_eq!(reply.request, id);
            break reply.snapshot.unwrap();
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::yield_now();
    };
    assert!(matches!(
        snapshot.widgets[0].value,
        Some(UiValue::Number(0.75))
    ));
    assert_eq!(worker.stats().rendered_blocks, 1);
    worker.stop();
    drop(port);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn completion_queued_stale_root_aborts_activation_instead_of_advancing_rejected_block() {
    let (config, _) = super::tests::authored_bank_with_script(
        r#"
        function onNote(e)postEvent(e)end
        function onRelease(e)postEvent(e)end
    "#,
    );
    let path = config.bank.clone();
    let mut worker = Worker::start_hosted(config, 7, 9).unwrap();
    worker.wait_ready(Duration::from_secs(5)).unwrap();
    let mut port = worker.take_audio_port().unwrap();
    port.realtime()
        .try_submit_hosted(hosted_packet(0, 1))
        .unwrap();
    let started = Instant::now();
    loop {
        match port.realtime().try_receive(stamp(0)) {
            Ok(_) => break,
            Err(PacketError::Underrun) => {
                assert!(started.elapsed() < Duration::from_secs(5));
                thread::yield_now()
            }
            Err(error) => panic!("{error:?}: {:?}", worker.private_failure()),
        }
    }
    assert_eq!(
        port.realtime().try_receive_completion(stamp(256)).unwrap(),
        Some(completion(256, 1))
    );
    // Root1 has already ended/been acknowledged. Callback transport has no
    // duplicate ledger: a structurally valid packet transfers unchanged, and
    // the authoritative Session rejects it before its block advances.
    port.realtime()
        .try_submit_hosted(hosted_packet(256, 1))
        .unwrap();
    while worker.status() != Status::Failed {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::yield_now();
    }
    assert_eq!(worker.stats().rendered_blocks, 1);
    assert_eq!(
        port.realtime().try_receive(stamp(256)).unwrap_err(),
        PacketError::Failed
    );
    assert!(worker.shared.outputs.is_empty());
    worker.stop();
    drop(port);
    std::fs::remove_file(path).unwrap();
}
