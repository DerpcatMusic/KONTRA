use sampler_core::*;
mod support;

const LEVEL: ControlId = ControlId(9);

fn player(envelope: Envelope, voices: usize) -> (Runtime, StreamWorker) {
    let pcm = Pcm::streamed(48000, PAGE_FRAMES).unwrap();
    let parameter = Parameter::Control(ControlRange {
        control: LEVEL,
        low: 0.,
        high: 1.,
        ramp_frames: 0,
    });
    let plan = Prepared::new(
        48000,
        vec![pcm],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope,
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap()
    .with_controls(vec![ControlDefinition {
        id: LEVEL,
        domain: ControlDomain::Real { min: 0., max: 1. },
        default: ControlValue::Real(0.2),
    }])
    .unwrap()
    .with_voice_chains(
        vec![
            VoiceChain::new(
                vec![],
                vec![
                    Processor::Gainer {
                        dry: 0.,
                        gain: parameter,
                    },
                    Processor::StereoModeller(StereoSettings {
                        width: parameter,
                        pan: parameter,
                        pseudo: false,
                    }),
                    Processor::Biquad(
                        Biquad::new(48000, FilterKind::LowPass, 3000., 0.707).unwrap(),
                    ),
                ],
                0,
            )
            .unwrap(),
        ],
        vec![Some(0)],
    )
    .unwrap();
    let plan = plan
        .with_programs(
            vec![
                Program::new(vec![Instruction::Wait(10000), Instruction::End]).unwrap(),
                Program::new(vec![Instruction::Play {
                    transpose: 127,
                    velocity: Velocity::Scale(1.),
                    inheritance: Inheritance::Linked,
                    duration: sampler_core::Duration::FramesOrGate(1),
                }])
                .unwrap(),
            ],
            None,
        )
        .unwrap();
    let limits = Limits::for_plan(&plan, voices, voices);
    let (cache, worker) = StreamCache::new(voices.max(2)).unwrap();
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    rt.set_cold_starts(true);
    for id in 0..voices {
        rt.trigger(
            Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(id as i32),
            },
            60,
            1.,
        )
        .unwrap();
    }
    rt.service_streaming(256).unwrap();
    (rt, worker)
}

fn publish(rt: &mut Runtime, worker: &mut StreamWorker) {
    let mut job = worker.next_job().unwrap();
    job.frames_mut().fill([0.25, 0.125]);
    worker.complete(job, Ok(())).unwrap();
    assert!(rt.service_streaming(256).unwrap());
}

fn target(rt: &mut Runtime) {
    rt.edit_controls(
        rt.active_plan(),
        None,
        &[ControlWrite {
            id: LEVEL,
            value: ControlValue::Real(0.8),
        }],
    )
    .unwrap();
}

#[test]
fn cold_chain_holds_short_finite_envelopes_and_uninitialized_smoothing() {
    for envelope in [
        Envelope::default(),
        Envelope::one_shot(16, 0, 0),
        Envelope::new(512, 0, 128, 0.5, 64).unwrap(),
    ] {
        for (voices, threads) in [(1, 1), (2, 1), (64, 2)] {
            let (mut immediate, mut iw) = player(envelope, voices);
            let (mut delayed, mut dw) = player(envelope, voices);
            immediate.set_threads(Threads::Fixed(threads));
            delayed.set_threads(Threads::Fixed(threads));
            let mut silence = [[0.; 2]; 128];
            support::without_heap(|| delayed.render(&mut silence).unwrap());
            assert_eq!(silence, [[0.; 2]; 128]);
            assert_eq!(
                delayed.voice_count(),
                voices,
                "hold must not start an envelope tail"
            );
            target(&mut immediate);
            target(&mut delayed);
            publish(&mut immediate, &mut iw);
            publish(&mut delayed, &mut dw);
            let mut expected = [[0.; 2]; 256];
            let mut actual = expected;
            support::without_heap(|| {
                immediate.render(&mut expected).unwrap();
                delayed.render(&mut actual).unwrap();
            });
            assert!(expected.iter().flatten().any(|x| *x != 0.));
            assert_eq!(
                actual, expected,
                "voice count {voices}: envelope and smoothing must wait"
            );
            assert_eq!(delayed.stream_underruns(), 0);
            if threads > 1 {
                assert!(delayed.parallel_blocks() > 0);
            }
        }
    }
}

#[test]
fn choked_cold_chain_ends_immediately_without_waiting_for_storage() {
    let (mut rt, mut worker) = player(Envelope::default(), 1);
    let note = rt.live_notes().next().unwrap();
    let family = rt.note_families(note).unwrap().next().unwrap();
    support::without_heap(|| rt.choke_family(family, 16).unwrap());
    assert_eq!(
        rt.voice_count(),
        0,
        "an onset that never sounded has no fade tail"
    );
    publish(&mut rt, &mut worker);
    let mut audio = [[0.; 2]; 2416];
    support::without_heap(|| rt.render(&mut audio).unwrap());
    assert_eq!(
        audio, [[0.; 2]; 2416],
        "late completion must not resurrect a choke"
    );
    assert_eq!(
        rt.input_held(note),
        Ok(true),
        "choke preserves physical key ownership"
    );
}

#[test]
fn late_cold_chain_starts_its_finite_envelope_only_when_data_arrives() {
    let (mut rt, mut worker) = player(Envelope::one_shot(16, 0, 0), 2);
    let mut audio = [[0.; 2]; 2390];
    support::without_heap(|| rt.render(&mut audio).unwrap());
    let mut boundary = [[0.; 2]; 20];
    support::without_heap(|| rt.render(&mut boundary).unwrap());
    assert_eq!(
        rt.voice_count(),
        2,
        "a storage wait consumes no envelope frames"
    );
    publish(&mut rt, &mut worker);
    support::without_heap(|| rt.render(&mut boundary[..15]).unwrap());
    assert_eq!(
        rt.voice_count(),
        2,
        "fifteen active frames leave one envelope frame"
    );
    support::without_heap(|| rt.render(&mut boundary[..1]).unwrap());
    assert_eq!(
        rt.voice_count(),
        0,
        "finite envelope ends after sixteen active frames"
    );
}

fn held_lifecycle(event: usize) {
    // A long release and a DSP chain make silent-tail retention visible.
    let (mut rt, mut worker) = player(Envelope::new(128, 0, 0, 1., 10000).unwrap(), 2);
    let note = rt.live_notes().next().unwrap();
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(0),
    };
    let family = rt.note_families(note).unwrap().next().unwrap();
    let mut audio = [[0.; 2]; 128];
    support::without_heap(|| rt.render(&mut audio).unwrap());
    assert_eq!(rt.voice_count(), 2);
    support::without_heap(|| match event {
        0 => rt.choke_family(family, 10000).unwrap(),
        1 => {
            rt.note_off(input, None).unwrap();
        }
        2 => {
            let callback = rt.start_behavior(note, 0).unwrap();
            rt.cancel_behavior(callback).unwrap();
            assert_eq!(rt.behavior_outcome(callback), Ok(Some(Outcome::Cancelled)));
        }
        3 => {
            rt.all_sound_off(input.channel_address()).unwrap();
        }
        4 => {
            let callback = rt.start_behavior(note, 1).unwrap();
            assert_eq!(
                rt.behavior_outcome(callback),
                Ok(Some(Outcome::Fault(Error::InvalidInput)))
            );
            assert_eq!(rt.take_fault(), Some((1, Error::InvalidInput)));
        }
        _ => unreachable!(),
    });
    let expected = if event == 3 {
        0
    } else if event == 1 {
        2
    } else {
        1
    };
    assert_eq!(
        rt.voice_count(),
        expected,
        "held lifecycle event {event} must end immediately"
    );
    publish(&mut rt, &mut worker);
    support::without_heap(|| rt.render(&mut audio).unwrap());
    assert_eq!(
        rt.voice_count(),
        expected,
        "a late page must not resurrect event {event}"
    );
    if event == 1 {
        assert!(
            audio.iter().flatten().any(|x| *x != 0.),
            "released late onset must sound"
        );
    }
    support::without_heap(|| {
        rt.all_sound_off(input.channel_address()).unwrap();
        rt.flush_behaviors(|_, _, _| true);
        rt.all_notes_off(input.channel_address()).unwrap();
        rt.flush_ended(|_| true);
    });
    assert_eq!(
        (rt.voice_count(), rt.note_count(), rt.pending_commands()),
        (0, 0, 0)
    );
}

#[test]
fn held_cold_choke_ends_before_storage_arrives() {
    held_lifecycle(0)
}
#[test]
fn released_cold_note_off_waits_then_sounds_in_release() {
    held_lifecycle(1)
}
#[test]
fn held_cold_cancel_ends_before_storage_arrives() {
    held_lifecycle(2)
}
#[test]
fn held_cold_all_sound_off_ends_before_storage_arrives() {
    held_lifecycle(3)
}
#[test]
fn held_cold_callback_fault_ends_before_storage_arrives() {
    held_lifecycle(4)
}

#[test]
fn held_cold_terminal_decode_fault_ends_before_storage_arrives() {
    let (mut rt, mut worker) = player(Envelope::new(128, 0, 0, 1., 10000).unwrap(), 2);
    let job = worker.next_job().unwrap();
    worker
        .complete(job, Err(DecodeFailure::InvalidSamples))
        .unwrap();
    support::without_heap(|| {
        assert_eq!(
            rt.service_streaming(256),
            Err(StreamError::DecodeFailed(DecodeFailure::InvalidSamples))
        )
    });
    assert_eq!(
        rt.voice_count(),
        0,
        "failed storage must not retain silent onsets"
    );
}

#[test]
fn cold_note_off_follows_sustain_until_pedal_up_then_starts_late_in_release() {
    let (mut rt, mut worker) = player(Envelope::new(128, 0, 0, 1., 10000).unwrap(), 2);
    let note = rt.live_notes().next().unwrap();
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(0),
    };
    let channel = rt.register_channel(input.channel_address()).unwrap();
    support::without_heap(|| {
        rt.sustain(channel, true).unwrap();
        assert_eq!(rt.note_off(input, None), Ok(note));
        assert!(rt.release_context(note).unwrap().gate.is_none());
        assert_eq!(rt.voice_count(), 2);
        rt.sustain(channel, false).unwrap();
        assert_eq!(
            rt.release_context(note).unwrap().gate.unwrap().cause,
            ReleaseCause::Pedal
        );
        assert_eq!(rt.voice_count(), 2);
        rt.render(&mut [[0.; 2]; 256]).unwrap();
        assert_eq!(
            rt.voice_count(),
            2,
            "storage delay does not clock the release"
        );
    });
    publish(&mut rt, &mut worker);
    let mut audio = [[0.; 2]; 256];
    support::without_heap(|| rt.render(&mut audio).unwrap());
    assert!(audio.iter().flatten().any(|x| *x != 0.));
    assert_eq!(rt.voice_count(), 2);
}
