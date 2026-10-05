use sampler_core::{Error, Expression, Inheritance, Input, Limits, Pcm, Protocol, Runtime};
mod support;

#[test]
fn ownership_pressure_render_and_retirement_do_no_heap_work() {
    let pcm = [Pcm {
        rate: 48000,
        frames: &[[0.25; 2]; 16],
    }];
    let mut rt = Runtime::new(
        48000,
        &pcm,
        Limits {
            notes: 8,
            expressions: 4,
            families: 4,
            voices: 4,
            commands: 4,
        },
    )
    .unwrap();
    let input = Input {
        protocol: Protocol::Midi2,
        port: 0,
        group: 15,
        channel: 15,
        key: 60,
        external_id: None,
    };
    support::without_heap(|| {
        for _ in 0..100 {
            let root = rt.note_on(input, 60, 1.0 / 65535.0).unwrap();
            let child = rt.child(root, 67, 1.0, false, Inheritance::Linked).unwrap();
            let snapshot = rt
                .child(root, 72, 1.0, true, Inheritance::Snapshot)
                .unwrap();
            let reused = rt.note_on(input, 60, 1.0).unwrap();
            let e = rt.expression_id(root).unwrap();
            rt.set_expression(
                e,
                Expression {
                    pressure: u32::MAX - 1,
                    gain: 0.5,
                    ..Expression::default()
                },
            )
            .unwrap();
            let detached = rt.detach_expression(child).unwrap();
            assert_eq!(rt.expression(detached).unwrap().pressure, u32::MAX - 1);
            assert_eq!(rt.note_on(input, 60, 1.0), Err(Error::Capacity));
            let family = rt.create_family(child).unwrap();
            let a = rt.start_family(family, 0, rt.now() + 1, 1.0).unwrap();
            rt.start_family(family, 0, rt.now() + 2, 1.0).unwrap();
            rt.finish_family(family).unwrap();
            rt.start(snapshot, 0, rt.now() + 3, 1.0).unwrap();
            rt.start(reused, 0, rt.now() + 4, 1.0).unwrap();
            assert_eq!(rt.release_at(root, rt.now() + 5), Err(Error::Capacity));
            rt.release(root).unwrap(); // Cancels linked release; detached family survives.
            rt.stop_voice(a).unwrap();
            assert_eq!(rt.family_voice_count(family), Ok(1));
            rt.render(&mut [[0.0; 2]; 8]).unwrap();
            rt.stop_family(family).unwrap();
            rt.panic();
            rt.flush_ended(|_| false);
            assert_eq!(rt.note_count(), 2);
            rt.flush_ended(|_| true);
            assert_eq!(
                (
                    rt.note_count(),
                    rt.family_count(),
                    rt.voice_count(),
                    rt.expression_count(),
                    rt.pending_commands()
                ),
                (0, 0, 0, 0, 0)
            );
        }
    });
}
