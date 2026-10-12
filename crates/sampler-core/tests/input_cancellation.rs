//! Exact physical-owner cancellation reuses discard cleanup, never channel panic.
use sampler_core::*;
mod support;

fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}
fn runtime(threads: usize) -> Runtime {
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[1.; 2]; 4096].into_boxed_slice()).unwrap()],
        vec![],
        0,
    )
    .unwrap();
    let limits = Limits::for_plan(&plan, 64, 128);
    Runtime::new(plan, limits)
        .unwrap()
        .with_threads(Threads::Fixed(threads))
}
fn family(rt: &mut Runtime, note: NoteId, at: u64) -> FamilyId {
    let family = rt.create_family(note).unwrap();
    for _ in 0..16 {
        rt.start_family(
            family,
            0,
            at,
            1. / 64.,
            Envelope::new(0, 0, 0, 1., 256).unwrap(),
            Playback::default(),
        )
        .unwrap();
    }
    family
}

#[test]
fn exact_input_cancellation_stops_delayed_linked_work_and_preserves_pedals_and_siblings_without_heap()
 {
    for threads in [1, 2] {
        let mut rt = runtime(threads);
        support::without_heap(|| {
            let root = rt.note_on(input(7), 60, 1.).unwrap();
            let linked = rt.child(root, 60, 1., true, Inheritance::Linked).unwrap();
            let independent = rt
                .child(root, 60, 1., false, Inheritance::Independent)
                .unwrap();
            let sibling = rt.note_on(input(8), 60, 1.).unwrap();
            let root_family = family(&mut rt, root, 0);
            family(&mut rt, linked, 0);
            let independent_family = family(&mut rt, independent, 0);
            family(&mut rt, sibling, 0);
            rt.start_family(
                root_family,
                0,
                128,
                1.,
                Envelope::default(),
                Playback::default(),
            )
            .unwrap();
            let channel = rt.register_channel(input(7).channel_address()).unwrap();
            rt.sustain(channel, true).unwrap();
            rt.sostenuto(channel, true).unwrap();
            let mut before = [[0.; 2]; 64];
            rt.render(&mut before).unwrap();
            assert_eq!(before, [[1.; 2]; 64]);
            rt.key_up(root, Some(0.25)).unwrap();
            assert!(
                rt.note(root).unwrap().2,
                "pedals retain its gate before CHOKE"
            );
            rt.cancel_input_note(root).unwrap();
            assert_eq!(
                rt.voice_count(),
                32,
                "matched root and linked child stop; unrelated sources stay"
            );
            assert_eq!(
                rt.pending_commands(),
                0,
                "delayed starts cannot resurrect the cancelled owner"
            );
            assert_eq!(
                rt.release_context(root).unwrap().key.unwrap().velocity,
                Some(0.25),
                "original physical release record survives cancellation"
            );
            assert!(rt.input_held(sibling).unwrap());
            assert!(rt.note(independent).unwrap().2);
            let mut after = [[0.; 2]; 256];
            rt.render(&mut after).unwrap();
            assert_eq!(after, [[0.5; 2]; 256]);
            rt.stop_family(independent_family).unwrap();
            rt.release(independent).unwrap();
            rt.render(&mut []).unwrap();
            let mut attempts = 0;
            rt.flush_ended(|owner| {
                assert_eq!(owner, input(7));
                attempts += 1;
                false
            });
            assert_eq!(attempts, 1);
            let mut accepted = 0;
            rt.flush_ended(|owner| {
                assert_eq!(owner, input(7));
                accepted += 1;
                true
            });
            assert_eq!(accepted, 1);
            rt.flush_ended(|_| panic!("NOTE_END must occur once"));
            assert!(rt.input_held(sibling).unwrap());
            assert_eq!(rt.voice_count(), 16);
        });
        if threads > 1 {
            assert!(
                rt.parallel_blocks() > 0,
                "parallel path must actually execute"
            );
        }
    }
}

#[test]
fn generated_or_stale_notes_cannot_be_cancelled_as_physical_input_owners() {
    let mut rt = runtime(1);
    let generated = rt
        .generated_note(input(0).channel_address(), 60, 1.)
        .unwrap();
    assert_eq!(rt.cancel_input_note(generated), Err(Error::InvalidInput));
    rt.release(generated).unwrap();
    rt.flush_ended(|_| true);
    assert_eq!(rt.cancel_input_note(generated), Err(Error::StaleHandle));
}
