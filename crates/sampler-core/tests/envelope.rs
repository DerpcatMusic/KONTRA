use sampler_core::{
    Envelope, Error, Event, Input, Limits, Pcm, Prepared, Protocol, Region, Runtime,
};
mod support;

fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}
fn runtime(envelope: Envelope, frames: usize) -> Runtime {
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[1.0; 2]; frames].into_boxed_slice()).unwrap()],
        vec![Region {
            playback: sampler_core::Playback::default(),
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.0,
            velocity_high: 1.0,
            gain: 1.0,
            envelope,
        }],
        1,
    )
    .unwrap();
    Runtime::new(
        plan,
        Limits {
            notes: 2,
            channels: 1,
            performances: 1,
            expressions: 2,
            families: 2,
            decisions: 0,
            voices: 2,
            commands: 4,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}

#[test]
fn ahdsr_is_sample_exact_and_partition_independent() {
    // A4 H2 D4 S0.5, key-up at frame 12, R4. Boundary values are exact binary fractions.
    let expected = [
        0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 0.875, 0.75, 0.625, 0.5, 0.5, 0.5, 0.375, 0.25, 0.125,
        0.0, 0.0,
    ];
    for partition in [1, 2, 3, 7, 18] {
        let mut rt = runtime(Envelope::new(4, 2, 4, 0.5, 4).unwrap(), 64);
        let note = rt.trigger(input(1), 60, 1.0).unwrap();
        rt.schedule_event(12, Event::KeyUp(note, None)).unwrap();
        let mut result = [[0.0; 2]; 18];
        for block in result.chunks_mut(partition) {
            rt.render(&mut []).unwrap();
            rt.render(block).unwrap();
        }
        assert_eq!(result.map(|f| f[0]), expected);
        assert!(result.iter().all(|f| f[0] == f[1]));
        assert_eq!((rt.voice_count(), rt.family_count()), (0, 0));
        let mut ends = 0;
        rt.flush_ended(|i| {
            assert_eq!(i, input(1));
            ends += 1;
            true
        });
        assert_eq!((ends, rt.note_count(), rt.expression_count()), (1, 0, 0));
    }
}

#[test]
fn release_captures_current_level_and_preserves_owners_without_heap_work() {
    let mut rt = runtime(Envelope::new(8, 0, 0, 1.0, 4).unwrap(), 64);
    support::without_heap(|| {
        let a = rt.trigger(input(1), 60, 1.0).unwrap();
        let expression = rt.expression_id(a).unwrap();
        rt.render(&mut [[0.0; 2]; 4]).unwrap();
        rt.key_up(a, None).unwrap(); // next held sample would be 0.5
        rt.flush_ended(|_| panic!("tail must retain the logical note"));
        assert_eq!(
            (rt.voice_count(), rt.family_count(), rt.note_count()),
            (1, 1, 1)
        );
        assert!(rt.expression(expression).is_ok());
        let mut first = [[0.0; 2]; 2];
        rt.render(&mut first).unwrap();
        assert_eq!(first, [[0.5; 2], [0.375; 2]]);
        // Cleanup for an unrelated release must not restart the first tail.
        let b = rt.note_on(input(2), 60, 1.0).unwrap();
        rt.release(b).unwrap();
        let mut second = [[0.0; 2]; 3];
        rt.render(&mut second).unwrap();
        assert_eq!(second, [[0.25; 2], [0.125; 2], [0.0; 2]]);
        rt.flush_ended(|_| false);
        assert_eq!(rt.note_count(), 2);
        rt.flush_ended(|_| true);
        assert_eq!((rt.note_count(), rt.expression_count()), (0, 0));
        assert_eq!(rt.expression(expression), Err(Error::StaleHandle));
    });
}

#[test]
fn pedals_delay_release_and_panic_cancels_tails_and_delayed_sources() {
    let envelope = Envelope::new(0, 0, 0, 1.0, 8).unwrap();
    let mut rt = runtime(envelope, 64);
    let channel = rt.register_channel(input(1).channel_address()).unwrap();
    support::without_heap(|| {
        let n = rt.trigger(input(1), 60, 1.0).unwrap();
        rt.sustain(channel, true).unwrap();
        rt.key_up(n, None).unwrap();
        let mut held = [[0.0; 2]; 3];
        rt.render(&mut held).unwrap();
        assert_eq!(held, [[1.0; 2]; 3]);
        rt.sustain(channel, false).unwrap();
        rt.render(&mut [[0.0; 2]; 2]).unwrap();
        assert_eq!(rt.voice_count(), 1);
        rt.panic();
        assert_eq!(
            (rt.voice_count(), rt.family_count(), rt.pending_commands()),
            (0, 0, 0)
        );
        let mut silent = [[1.0; 2]; 3];
        rt.render(&mut silent).unwrap();
        assert_eq!(silent, [[0.0; 2]; 3]);
        rt.flush_ended(|_| true);
        let n = rt.note_on(input(2), 60, 1.0).unwrap();
        let f = rt.create_family(n).unwrap();
        rt.start_family(
            f,
            0,
            rt.now() + 10,
            1.0,
            envelope,
            sampler_core::Playback::default(),
        )
        .unwrap();
        rt.finish_family(f).unwrap();
        rt.release(n).unwrap();
        assert_eq!(
            (rt.voice_count(), rt.family_count(), rt.pending_commands()),
            (0, 0, 0)
        );
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
}

#[test]
fn invalid_levels_and_eof_and_zero_release_have_explicit_outcomes() {
    for level in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        assert!(Envelope::new(0, 0, 0, level, 0).is_err());
    }
    let mut rt = runtime(
        Envelope::new(u32::MAX, u32::MAX, u32::MAX, 0.0, u32::MAX).unwrap(),
        2,
    );
    let n = rt.trigger(input(1), 60, 1.0).unwrap();
    rt.render(&mut [[0.0; 2]; 2]).unwrap();
    assert_eq!(rt.voice_count(), 0); // source EOF wins over envelope lifetime
    assert_eq!(rt.note_count(), 1); // logical gate still held
    rt.release(n).unwrap();
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0);
    let mut rt = runtime(Envelope::default(), 64);
    let n = rt.trigger(input(1), 60, 1.0).unwrap();
    rt.release(n).unwrap();
    assert_eq!(rt.voice_count(), 0);
}

#[test]
fn curved_dahdsr_matches_analytic_stages_and_release_capture_at_every_partition() {
    use sampler_core::EnvelopeCurve;
    let bend = |k: f64, t: f64| {
        if k.abs() < 1e-8 {
            t
        } else {
            (k * t).exp_m1() / k.exp_m1()
        }
    };
    for curves in [
        [-32., 32., -8.],
        [32., -32., 8.],
        [1e-320, -1e-320, 1e-12],
        [0.; 3],
    ] {
        let [attack, decay, release] = curves.map(|c| EnvelopeCurve::exponential(c).unwrap());
        let shape = Envelope::new(137, 5, 149, 0.25, 173)
            .unwrap()
            .with_delay(3)
            .with_curves(attack, decay, release);
        let held = |frame: usize| -> f32 {
            match frame {
                0..3 => 0.,
                3..140 => bend(curves[0], (frame - 3) as f64 / 137.) as f32,
                140..145 => 1.,
                145..294 => (1. - 0.75 * bend(curves[1], (frame - 145) as f64 / 149.)) as f32,
                _ => 0.25,
            }
        };
        for off in [0, 2, 3, 17, 140, 143, 199, 294] {
            let expected: [f32; 512] = std::array::from_fn(|frame| {
                if frame < off {
                    held(frame)
                } else if frame < off + 173 {
                    (f64::from(held(off)) * (1. - bend(curves[2], (frame - off) as f64 / 173.)))
                        as f32
                } else {
                    0.
                }
            });
            let mut baseline = [[0.; 2]; 512];
            for block in [1, 7, 64, 127] {
                let mut rt = runtime(shape, 1024);
                let mut output = [[0.; 2]; 512];
                support::without_heap(|| {
                    let note = rt.trigger(input(1), 60, 1.).unwrap();
                    rt.schedule_event(off as u64, Event::KeyUp(note, None))
                        .unwrap();
                    for chunk in output.chunks_mut(block) {
                        rt.render(&mut []).unwrap();
                        rt.render(chunk).unwrap();
                    }
                    rt.flush_ended(|_| false);
                    assert_eq!(rt.note_count(), 1);
                    rt.flush_ended(|_| true);
                    assert_eq!(
                        (rt.voice_count(), rt.note_count(), rt.family_count()),
                        (0, 0, 0)
                    );
                });
                for (frame, (&[left, right], expected)) in output.iter().zip(expected).enumerate() {
                    assert_eq!(left, right);
                    assert!(
                        (left - expected).abs() <= 2e-7,
                        "{curves:?}, off {off}, block {block}, frame {frame}: {left} != {expected}"
                    );
                    assert!(left.is_finite() && (0.0..=1.0).contains(&left));
                }
                if block == 1 {
                    baseline = output;
                } else {
                    assert_eq!(output, baseline);
                }
            }
        }
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -32.01, 32.01] {
        assert!(matches!(
            EnvelopeCurve::exponential(invalid),
            Err(Error::InvalidInput)
        ));
    }
}

#[test]
fn one_shot_delay_and_zero_stages_finish_without_a_key_up_and_ignore_early_gate_release() {
    let expected = [
        0., 0., 0., 0.25, 0.5, 0.75, 1., 1., 1., 0.75, 0.5, 0.25, 0., 0.,
    ];
    for off in [None, Some(0), Some(5), Some(10)] {
        for block in [1, 7, 14] {
            let mut rt = runtime(Envelope::one_shot(4, 2, 4).with_delay(2), 64);
            support::without_heap(|| {
                let note = rt.trigger(input(1), 60, 1.).unwrap();
                if let Some(off) = off {
                    rt.schedule_event(off, Event::KeyUp(note, None)).unwrap();
                }
                let mut output = [[0.; 2]; 14];
                for chunk in output.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                }
                assert_eq!(output.map(|x| x[0]), expected);
                assert_eq!((rt.voice_count(), rt.family_count()), (0, 0));
                if off.is_none() {
                    assert!(rt.key_down(note).unwrap());
                    rt.flush_ended(|_| panic!("AHD completion is not physical key release"));
                    rt.key_up(note, None).unwrap();
                }
                rt.flush_ended(|_| true);
                assert_eq!(rt.note_count(), 0);
            });
        }
    }
    for shape in [
        Envelope::one_shot(0, 0, 0),
        Envelope::one_shot(0, 0, 0).with_delay(2),
    ] {
        let mut rt = runtime(shape, 64);
        let note = rt.trigger(input(1), 60, 1.).unwrap();
        let mut output = [[1.; 2]; 5];
        rt.render(&mut output).unwrap();
        assert_eq!(output, [[0.; 2]; 5]);
        assert_eq!(rt.voice_count(), 0);
        rt.key_up(note, None).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    }
}

#[test]
fn curved_release_choke_captures_the_next_level_and_never_extends_one_shot_completion() {
    use sampler_core::{EnvelopeCurve, Playback};
    let curve = EnvelopeCurve::exponential(-4.).unwrap();
    for shape in [
        Envelope::new(0, 0, 0, 1., 128)
            .unwrap()
            .with_curves(curve, curve, curve),
        Envelope::one_shot(0, 0, 128).with_curves(curve, curve, curve),
    ] {
        let mut rt = runtime(Envelope::default(), 512);
        support::without_heap(|| {
            let note = rt.note_on(input(1), 60, 1.).unwrap();
            let family = rt.create_family(note).unwrap();
            rt.start_family(family, 0, 0, 1., shape, Playback::default())
                .unwrap();
            rt.key_up(note, None).unwrap();
            rt.render(&mut [[0.; 2]; 64]).unwrap();
            let level = (1. - (-2f64).exp_m1() / (-4f64).exp_m1()) as f32;
            rt.choke_family(family, 4).unwrap();
            rt.choke_family(family, 100).unwrap();
            let mut output = [[0.; 2]; 5];
            rt.render(&mut output).unwrap();
            for (index, frame) in output.iter().enumerate() {
                let expected = (f64::from(level) * (1. - index.min(4) as f64 / 4.)) as f32;
                assert!((frame[0] - expected).abs() <= 2e-7);
            }
            assert_eq!((rt.voice_count(), rt.family_count()), (0, 0));
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
    let mut rt = runtime(Envelope::default(), 64);
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    let family = rt.create_family(note).unwrap();
    rt.start_family(
        family,
        0,
        0,
        1.,
        Envelope::one_shot(0, 0, 4),
        Playback::default(),
    )
    .unwrap();
    rt.choke_family(family, 100).unwrap();
    let mut output = [[0.; 2]; 6];
    rt.render(&mut output).unwrap();
    assert_eq!(output.map(|f| f[0]), [1., 0.75, 0.5, 0.25, 0., 0.]);
    assert_eq!(rt.voice_count(), 0);
}
