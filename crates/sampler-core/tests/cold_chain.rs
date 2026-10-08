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
    let limits = Limits::for_plan(&plan, voices, voices);
    let (cache, worker) = StreamCache::new(2).unwrap();
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
fn choked_cold_chain_preserves_its_tail_budget_through_partial_hold_expiry() {
    let (mut rt, _worker) = player(Envelope::default(), 1);
    let note = rt.live_notes().next().unwrap();
    let family = rt.note_families(note).unwrap().next().unwrap();
    rt.choke_family(family, 16).unwrap();
    let mut audio = [[0.; 2]; 2390];
    support::without_heap(|| rt.render(&mut audio).unwrap());
    assert_eq!(rt.voice_count(), 1);
    let mut boundary = [[0.; 2]; 20];
    support::without_heap(|| rt.render(&mut boundary).unwrap());
    assert_eq!(rt.voice_count(), 1, "only ten active fade frames elapsed");
    support::without_heap(|| rt.render(&mut boundary[..6]).unwrap());
    assert_eq!(rt.voice_count(), 0);
}

#[test]
fn cold_chain_hold_expiry_inside_a_block_advances_only_its_suffix() {
    let (mut rt, _worker) = player(Envelope::one_shot(16, 0, 0), 2);
    let mut audio = [[0.; 2]; 2390];
    support::without_heap(|| rt.render(&mut audio).unwrap());
    assert_eq!(rt.voice_count(), 2);
    let mut boundary = [[0.; 2]; 20];
    support::without_heap(|| rt.render(&mut boundary).unwrap());
    assert_eq!(
        rt.voice_count(),
        2,
        "ten held frames leave six envelope frames"
    );
    support::without_heap(|| rt.render(&mut boundary[..6]).unwrap());
    assert_eq!(rt.voice_count(), 0);
}
