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
        vec![Pcm {
            rate: 48000,
            frames: vec![[1.0; 2]; frames].into_boxed_slice(),
        }],
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
            expressions: 2,
            families: 2,
            voices: 2,
            commands: 4,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
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
        rt.schedule_event(12, Event::KeyUp(note)).unwrap();
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
        rt.key_up(a).unwrap(); // next held sample would be 0.5
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
        rt.key_up(n).unwrap();
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
