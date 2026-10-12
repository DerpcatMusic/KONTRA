use sampler_core::*;
mod support;

fn limits() -> Limits {
    Limits {
        notes: 4,
        channels: 0,
        performances: 1,
        families: 8,
        voices: 8,
        expressions: 4,
        decisions: 0,
        commands: 8,
        behaviors: 0,
        behavior_cells: 0,
        behavior_fuel: 0,
        note_cells: 0,
    }
}

fn input(id: i32, key: u8) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: Some(id),
    }
}

fn plan() -> Prepared {
    Prepared::new(
        48000,
        vec![
            Pcm::new(48000, vec![[1., -0.25]].into_boxed_slice()).unwrap(),
            Pcm::new(48000, vec![[0.; 2]].into_boxed_slice()).unwrap(),
        ],
        (0..2)
            .map(|sample| Region {
                sample,
                key_low: 60 + sample as u8,
                key_high: 60 + sample as u8,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::default(),
                playback: Playback::default(),
            })
            .collect(),
        2,
    )
    .unwrap()
}

fn delay(frames: u32, feedback: [[f64; 2]; 2], dry: f64, wet: f64) -> Processor {
    Processor::Delay(Delay::new(frames, feedback, dry, wet).unwrap())
}

fn bus(processors: Vec<Processor>, tail_frames: u32) -> Bus {
    Bus {
        processors,
        sends: vec![BusSend {
            bus: None,
            gain: 1.,
        }],
        tail_frames,
    }
}

// Independent difference equation over the entire timeline, without ring storage,
// validity counters, processor compilation or native DSP calls.
fn reference(
    input: &[[f64; 2]],
    delay: usize,
    feedback: [[f64; 2]; 2],
    dry: f64,
    wet: f64,
) -> Vec<[f64; 2]> {
    let mut history = vec![[0.; 2]; input.len()];
    let mut output = history.clone();
    for (i, frame) in input.iter().enumerate() {
        let delayed = i.checked_sub(delay).map_or([0.; 2], |n| history[n]);
        for c in 0..2 {
            history[i][c] = frame[c] + feedback[c][0] * delayed[0] + feedback[c][1] * delayed[1];
            output[i][c] = dry * frame[c] + wet * delayed[c];
        }
    }
    output
}

fn near(a: Frame, b: [f64; 2]) {
    for (actual, expected) in a.into_iter().zip(b) {
        assert!(
            (f64::from(actual) - expected).abs() < 2e-7,
            "{a:?} != {b:?}"
        );
    }
}

#[test]
fn serial_voice_and_bus_delays_match_stereo_recurrence_across_partitions() {
    let diagonal = [[-0.5, 0.], [0., 0.25]];
    let cross = [[0., 0.5], [0.25, 0.]];
    for frames in [1, 3, 67] {
        let mut source = vec![[0.; 2]; 201];
        source[0] = [1., -0.25];
        let pre = reference(&source, frames, diagonal, 0.25, 0.75);
        let post = reference(&pre, 5, cross, 0., 1.);
        let mut voices = vec![[0.; 2]; 420];
        for i in 0..post.len() {
            for c in 0..2 {
                voices[i][c] += post[i][c];
                voices[i + 2][c] += post[i][c] * 0.5;
            }
        }
        let mut expected = reference(&voices, 7, diagonal, 0.5, 1.);
        expected[413..].fill([0.; 2]);
        for block in [1, 7, 64, 129] {
            let prepared = plan()
                .with_voice_chains(
                    vec![
                        VoiceChain::new(
                            vec![delay(frames as u32, diagonal, 0.25, 0.75)],
                            vec![delay(5, cross, 0., 1.)],
                            200,
                        )
                        .unwrap(),
                    ],
                    vec![Some(0); 2],
                )
                .unwrap()
                .with_buses(
                    vec![bus(vec![delay(7, diagonal, 0.5, 1.)], 210)],
                    vec![Some(0); 2],
                )
                .unwrap();
            let mut rt = Runtime::new(prepared, limits()).unwrap();
            let mut audio = [[0.; 2]; 420];
            support::without_heap(|| {
                rt.trigger(input(1, 60), 60, 1.).unwrap();
                rt.render(&mut audio[..2]).unwrap();
                rt.trigger(input(2, 60), 60, 0.5).unwrap();
                for chunk in audio[2..].chunks_mut(block) {
                    rt.render(chunk).unwrap();
                }
                for (a, b) in audio.into_iter().zip(&expected) {
                    near(a, *b);
                }
                assert_eq!(rt.voice_count(), 0);
                rt.note_off(input(1, 60), None).unwrap();
                rt.note_off(input(2, 60), None).unwrap();
                rt.flush_ended(|_| true);
                assert_eq!(rt.note_count(), 0);
            });
        }
    }
}

#[test]
fn silent_delay_gap_retains_old_bus_generation_without_retaining_host_notes() {
    let prepared = plan()
        .with_buses(
            vec![bus(vec![delay(7, [[0.; 2]; 2], 0., 1.)], 8)],
            vec![Some(0); 2],
        )
        .unwrap();
    let (mut rt, mut transfer) = Runtime::with_plan_updates(prepared, limits(), 2, 1).unwrap();
    let replacement = plan()
        .with_buses(
            vec![bus(vec![delay(3, [[0.; 2]; 2], 0., 0.5)], 4)],
            vec![Some(0); 2],
        )
        .unwrap();
    let request = transfer.submit(Box::new(replacement)).unwrap();
    support::without_heap(|| {
        rt.trigger(input(1, 60), 60, 1.).unwrap();
        let mut first = [[0.; 2]; 1];
        rt.render(&mut first).unwrap();
        assert_eq!(first, [[0.; 2]; 1]);
        rt.note_off(input(1, 60), None).unwrap();
        rt.flush_ended(|ended| {
            assert_eq!(ended, input(1, 60));
            true
        });
        assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
        assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.trigger(input(2, 60), 60, 1.).unwrap();
        let mut audio = [[0.; 2]; 8];
        rt.render(&mut audio).unwrap();
        for (i, value) in audio.into_iter().enumerate() {
            near(
                value,
                match i {
                    3 => [0.5, -0.125],
                    6 => [1., -0.25],
                    _ => [0.; 2],
                },
            );
        }
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    drop(transfer.retired().unwrap());
}

#[test]
fn panic_and_reused_voice_slots_invalidate_delay_history_without_heap_work() {
    for shared in [false, true] {
        let processor = || delay(67, [[0.5, 0.], [0., 0.5]], 0., 1.);
        let prepared = if shared {
            plan()
                .with_buses(vec![bus(vec![processor()], 140)], vec![Some(0); 2])
                .unwrap()
        } else {
            plan()
                .with_voice_chains(
                    vec![VoiceChain::new(vec![], vec![processor()], 140).unwrap()],
                    vec![Some(0); 2],
                )
                .unwrap()
        };
        let mut rt = Runtime::new(prepared, limits()).unwrap();
        support::without_heap(|| {
            rt.trigger(input(1, 60), 60, 1.).unwrap();
            rt.render(&mut [[0.; 2]; 30]).unwrap();
            rt.panic();
            rt.flush_ended(|_| true);
            rt.trigger(input(2, 61), 61, 1.).unwrap();
            let mut audio = [[0.; 2]; 180];
            rt.render(&mut audio).unwrap();
            assert_eq!(audio, [[0.; 2]; 180]);
            rt.note_off(input(2, 61), None).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
        });
    }
}

#[test]
fn delayed_overflow_resets_history_without_recurring_faults() {
    for shared in [false, true] {
        // The first echo overflows f32, after finite feedback has entered the
        // ring. Clearing validity must prevent that history returning later.
        let processor = delay(3, [[0.5, 0.], [0., 0.5]], 0., f64::MAX);
        let prepared = if shared {
            plan()
                .with_buses(vec![bus(vec![processor], 20)], vec![Some(0), None])
                .unwrap()
        } else {
            plan()
                .with_voice_chains(
                    vec![VoiceChain::new(vec![], vec![processor], 20).unwrap()],
                    vec![Some(0), None],
                )
                .unwrap()
        };
        let mut rt = Runtime::new(prepared, limits()).unwrap();
        support::without_heap(|| {
            rt.trigger(input(1, 60), 60, 1.).unwrap();
            let mut first = [[0.; 2]; 5];
            rt.render(&mut first).unwrap();
            assert_eq!(first, [[0.; 2]; 5]);
            assert_eq!(rt.nonfinite_frames(), 1);
            let mut rest = [[0.; 2]; 30];
            rt.render(&mut rest).unwrap();
            assert_eq!(rest, [[0.; 2]; 30]);
            assert_eq!(rt.nonfinite_frames(), 1);
        });
    }
}

#[test]
fn invalid_or_unbounded_feedback_is_rejected_before_preparation() {
    for feedback in [
        [[1., 0.], [0., 0.]],
        [[0., 0.], [0., -1.]],
        [[0.5, -0.5], [0., 0.]],
        [[f64::NAN, 0.], [0., 0.]],
        [[0., 0.], [f64::INFINITY, 0.]],
    ] {
        assert!(Delay::new(1, feedback, 1., 1.).is_err());
    }
    assert!(Delay::new(0, [[0.; 2]; 2], 1., 1.).is_err());
    assert!(Delay::new(1, [[0.; 2]; 2], f64::NAN, 1.).is_err());
    assert!(Delay::new(1, [[0.; 2]; 2], 1., f64::INFINITY).is_err());
    let prepared = plan()
        .with_voice_chains(
            vec![VoiceChain::new(vec![], vec![delay(16, [[0.; 2]; 2], 0., 1.)], 16).unwrap()],
            vec![Some(0); 2],
        )
        .unwrap();
    let mut budget = limits();
    budget.voices = usize::MAX;
    assert!(matches!(
        Runtime::new(prepared, budget),
        Err(Error::Capacity)
    ));
}
