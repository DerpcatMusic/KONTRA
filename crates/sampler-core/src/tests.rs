use super::*;

fn input(id: Option<i32>) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 2,
        channel: 4,
        key: 60,
        external_id: id,
    }
}

fn limits() -> Limits {
    Limits {
        notes: 8,
        voices: 8,
        commands: 8,
    }
}

#[test]
fn ownership_survives_source_end_children_and_rejected_terminal_delivery() {
    let samples = [Pcm {
        rate: 48000,
        frames: &[[0.25, -0.25]; 2],
    }];
    let mut rt = Runtime::new(48000, &samples, limits()).unwrap();
    let root = rt.note_on(input(Some(7)), 72, 0.123456789).unwrap();
    let linked = rt.child(root, 76, 1.0, true).unwrap();
    let detached = rt.child(root, 79, 1.0, false).unwrap();
    rt.start(root, 0, 0, 1.0).unwrap();
    rt.start(linked, 0, 10, 1.0).unwrap();
    let independent = rt.start(detached, 0, 10, 1.0).unwrap();
    rt.pin(root).unwrap();
    rt.render(&mut [[0.0; 2]; 2]).unwrap();
    assert_eq!(rt.note(root).unwrap(), (72, 0.123456789, true));
    rt.release(root).unwrap();
    assert!(!rt.note(linked).unwrap().2);
    assert!(rt.note(detached).unwrap().2);
    assert_eq!(rt.pending_commands(), 1);
    assert!(rt.voice_active(independent));
    rt.flush_ended(|_| panic!("detached child still owns the root"));
    rt.render(&mut [[0.0; 2]; 10]).unwrap();
    assert_eq!(rt.voice_count(), 0);
    rt.release(detached).unwrap();
    rt.flush_ended(|_| panic!("continuation still pins the root"));
    assert_eq!(rt.note_count(), 1);
    rt.unpin(root).unwrap();
    let mut attempts = 0;
    rt.flush_ended(|i| {
        assert_eq!(i, input(Some(7)));
        attempts += 1;
        false
    });
    assert_eq!(attempts, 1);
    assert_eq!(
        rt.note_on(input(Some(7)), 60, 1.0),
        Err(Error::DuplicateInput)
    );
    rt.flush_ended(|i| {
        assert_eq!(i.key, 60);
        attempts += 1;
        true
    });
    rt.flush_ended(|_| panic!("terminal was already accepted"));
    assert_eq!(attempts, 2);
    assert_eq!(rt.note_count(), 0);
    let replacement = rt.note_on(input(Some(7)), 60, 1.0).unwrap();
    assert_ne!(root, replacement);
    assert_eq!(rt.release(root), Err(Error::StaleHandle));
    assert!(rt.note(replacement).unwrap().2);
}

#[test]
fn cleanup_does_not_need_queue_space_and_no_source_notes_retry() {
    let samples = [Pcm {
        rate: 48000,
        frames: &[[1.0; 2]; 4],
    }];
    let mut rt = Runtime::new(
        48000,
        &samples,
        Limits {
            notes: 2,
            voices: 1,
            commands: 1,
        },
    )
    .unwrap();
    let a = rt.note_on(input(None), 60, 1.0).unwrap();
    let b = rt.note_on(input(None), 60, 1.0).unwrap();
    rt.start(a, 0, 100, 1.0).unwrap();
    assert_eq!(rt.start(b, 0, 50, 1.0), Err(Error::Capacity));
    assert_eq!(rt.release_at(a, 10), Err(Error::Capacity));
    assert_eq!(rt.note_on(input(Some(8)), 60, 1.0), Err(Error::Capacity));
    assert_eq!(rt.note_off(input(None)), Ok(a)); // FIFO, even though b shares the key.
    assert_eq!(rt.pending_commands(), 0);
    assert_eq!(rt.voice_count(), 0);
    assert!(rt.note(b).unwrap().2);
    rt.note_off(input(None)).unwrap();
    rt.flush_ended(|_| false);
    assert_eq!(rt.note_count(), 2);
    let mut ends = 0;
    rt.flush_ended(|_| {
        ends += 1;
        true
    });
    assert_eq!(ends, 2);
    let mut audio = [[-1.0; 2]; 200];
    rt.render(&mut audio).unwrap();
    assert!(audio.iter().all(|f| *f == [0.0; 2]));
}

#[test]
fn scheduling_is_partition_invariant_at_two_rates() {
    for rate in [44100, 48000] {
        let samples = [Pcm {
            rate,
            frames: &[[0.5, -0.25]; 64],
        }];
        let render = |partition: &[usize]| {
            let mut rt = Runtime::new(rate, &samples, limits()).unwrap();
            let a = rt.note_on(input(Some(1)), 60, 1.0).unwrap();
            let b = rt.note_on(input(Some(2)), 60, 1.0).unwrap();
            let c = rt.note_on(input(Some(3)), 60, 1.0).unwrap();
            rt.start(a, 0, 17, 1.0).unwrap();
            rt.release_at(a, 33).unwrap();
            rt.start(b, 0, 33, 0.5).unwrap();
            rt.release_at(b, 65).unwrap();
            rt.start(c, 0, 70, 1.0).unwrap();
            rt.release_at(c, 69).unwrap();
            let mut audio = vec![[0.0; 2]; 2048];
            let mut at = 0;
            let mut step = 0;
            while at < audio.len() {
                let len = partition[step % partition.len()].min(audio.len() - at);
                rt.render(&mut audio[at..at + len]).unwrap();
                at += len;
                step += 1;
            }
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.voice_count(), rt.pending_commands()),
                (0, 0, 0)
            );
            audio
        };
        let expected = render(&[1]);
        assert!(expected[..17].iter().all(|f| *f == [0.0; 2]));
        assert!(expected[17..33].iter().all(|f| *f == [0.5, -0.25]));
        assert!(expected[33..65].iter().all(|f| *f == [0.25, -0.125]));
        assert!(expected[65..].iter().all(|f| *f == [0.0; 2]));
        for block in [16, 32, 64, 128, 256, 512, 1024] {
            assert_eq!(render(&[block]), expected);
        }
        assert_eq!(render(&[0, 7, 1, 127, 33, 0, 256]), expected);
    }
}

#[test]
fn boundary_order_overflow_and_handle_domains_are_explicit() {
    let samples = [Pcm {
        rate: 48000,
        frames: &[[0.5; 2]; 4],
    }];
    let mut rt = Runtime::new(48000, &samples, limits()).unwrap();
    let note = rt.note_on(input(Some(-2)), 60, 0.0).unwrap(); // Signed IDs and native zero velocity survive.
    let voice = rt.start(note, 0, 16, 1.0).unwrap();
    rt.render(&mut [[0.0; 2]; 16]).unwrap();
    assert_eq!(rt.pending_commands(), 1);
    rt.render(&mut []).unwrap();
    assert_eq!(rt.pending_commands(), 0);
    let mut other = Runtime::new(48000, &samples, limits()).unwrap();
    other.note_on(input(Some(-2)), 60, 0.0).unwrap();
    assert_eq!(other.release(note), Err(Error::StaleHandle));
    assert_eq!(other.stop_voice(voice), Err(Error::StaleHandle));
    assert_eq!(rt.release_at(note, 15), Err(Error::PastEvent));
    rt.now = u64::MAX;
    let mut output = [[123.0; 2]; 1];
    assert_eq!(rt.render(&mut output), Err(Error::ClockOverflow));
    assert_eq!(output, [[123.0; 2]; 1]);
    rt.panic();
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0);
    rt.notes.slots[0].generation = u64::MAX;
    let fresh = rt.note_on(input(Some(3)), 60, 1.0).unwrap();
    assert_ne!(
        fresh.0.index, 0,
        "exhausted generation slot must never wrap"
    );
}

#[test]
fn invalid_preparation_and_atomic_failed_start() {
    assert!(matches!(
        Runtime::new(0, &[], limits()),
        Err(Error::InvalidInput)
    ));
    let invalid = [Pcm {
        rate: 48000,
        frames: &[[f32::NAN, 0.0]],
    }];
    assert!(matches!(
        Runtime::new(48000, &invalid, limits()),
        Err(Error::InvalidInput)
    ));
    let samples = [Pcm {
        rate: 48000,
        frames: &[[1.0; 2]; 4],
    }];
    assert!(matches!(
        Runtime::new(44100, &samples, limits()),
        Err(Error::InvalidInput)
    ));
    let mut rt = Runtime::new(48000, &samples, limits()).unwrap();
    assert_eq!(rt.note_on(input(None), 128, 1.0), Err(Error::InvalidInput));
    assert_eq!(
        rt.note_on(input(None), 60, f64::NAN),
        Err(Error::InvalidInput)
    );
    let n = rt.note_on(input(None), 60, 1.0).unwrap();
    assert_eq!(rt.start(n, 1, 0, 1.0), Err(Error::InvalidInput));
    assert_eq!(rt.start(n, 0, 0, f32::INFINITY), Err(Error::InvalidInput));
    assert_eq!((rt.voice_count(), rt.pending_commands()), (0, 0));
    rt.release(n).unwrap();
    assert_eq!(rt.child(n, 60, 1.0, true), Err(Error::ClosedNote));
    assert_eq!(rt.start(n, 0, 0, 1.0), Err(Error::ClosedNote));
}

#[test]
fn voice_scope_reuse_and_nonfinite_mix_are_observable() {
    let samples = [Pcm {
        rate: 48000,
        frames: &[[f32::MAX; 2]; 4],
    }];
    let mut rt = Runtime::new(48000, &samples, limits()).unwrap();
    let root = rt.note_on(input(Some(1)), 60, 1.0).unwrap();
    let a = rt.start(root, 0, 0, 1.0).unwrap();
    let b = rt.start(root, 0, 0, 1.0).unwrap();
    let mut audio = [[0.0; 2]; 1];
    rt.render(&mut audio).unwrap();
    assert_eq!(audio, [[0.0; 2]; 1]);
    assert_eq!(rt.nonfinite_frames(), 1);
    rt.stop_voice(a).unwrap();
    assert!(rt.voice_active(b));
    let replacement = rt.start(root, 0, rt.now(), 0.0).unwrap();
    assert_eq!(rt.stop_voice(a), Err(Error::StaleHandle));
    assert!(rt.voice_active(replacement));
    rt.render(&mut audio).unwrap();
    assert_eq!(audio, [[f32::MAX; 2]; 1]);
    assert_eq!(rt.nonfinite_frames(), 1);
    rt.release(root).unwrap();
    assert_eq!(rt.voice_count(), 0);
}
