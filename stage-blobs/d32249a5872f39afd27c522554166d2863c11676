use super::*;

fn input(id: Option<i32>) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 2,
        group: 0,
        channel: 4,
        key: 60,
        external_id: id,
    }
}

fn limits() -> Limits {
    Limits {
        notes: 8,
        channels: 4,
        performances: 1,
        families: 8,
        decisions: 0,
        expressions: 8,
        voices: 8,
        commands: 8,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    }
}

#[test]
fn ownership_survives_source_end_children_and_rejected_terminal_delivery() {
    let samples = [Pcm::new(48000, Box::from([[0.25, -0.25]; 2])).unwrap()];
    let mut rt = fixture_runtime(48000, &samples, limits()).unwrap();
    let root = rt.note_on(input(Some(7)), 72, 0.123456789).unwrap();
    let linked = rt
        .child(root, 76, 1.0, true, Inheritance::Snapshot)
        .unwrap();
    let detached = rt
        .child(root, 79, 1.0, false, Inheritance::Snapshot)
        .unwrap();
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
    let samples = [Pcm::new(48000, Box::from([[1.0; 2]; 4])).unwrap()];
    let mut rt = fixture_runtime(
        48000,
        &samples,
        Limits {
            notes: 2,
            channels: 4,
            performances: 1,
            families: 2,
            decisions: 0,
            expressions: 2,
            voices: 1,
            commands: 1,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap();
    let a = rt.note_on(input(None), 60, 1.0).unwrap();
    let b = rt.note_on(input(None), 60, 1.0).unwrap();
    rt.start(a, 0, 100, 1.0).unwrap();
    assert_eq!(rt.start(b, 0, 50, 1.0), Err(Error::Capacity));
    assert_eq!(rt.release_at(a, 10), Err(Error::Capacity));
    assert_eq!(rt.note_on(input(Some(8)), 60, 1.0), Err(Error::Capacity));
    assert_eq!(rt.note_off(input(None), None), Ok(a)); // FIFO, even though b shares the key.
    assert_eq!(rt.pending_commands(), 0);
    assert_eq!(rt.voice_count(), 0);
    assert!(rt.note(b).unwrap().2);
    rt.note_off(input(None), None).unwrap();
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
        let samples = [Pcm::new(rate, Box::from([[0.5, -0.25]; 64])).unwrap()];
        let render = |partition: &[usize]| {
            let mut rt = fixture_runtime(rate, &samples, limits()).unwrap();
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
    let samples = [Pcm::new(48000, Box::from([[0.5; 2]; 4])).unwrap()];
    let mut rt = fixture_runtime(48000, &samples, limits()).unwrap();
    let note = rt.note_on(input(Some(-2)), 60, 0.0).unwrap(); // Signed IDs and native zero velocity survive.
    let voice = rt.start(note, 0, 16, 1.0).unwrap();
    rt.render(&mut [[0.0; 2]; 16]).unwrap();
    assert_eq!(rt.pending_commands(), 1);
    rt.render(&mut []).unwrap();
    assert_eq!(rt.pending_commands(), 0);
    let mut other = fixture_runtime(48000, &samples, limits()).unwrap();
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
    rt.notes.slots[0].generation = u64::MAX - 1;
    rt.note_on(input(Some(3)), 60, 1.0).unwrap();
    rt.panic();
    rt.flush_ended(|_| true);
    let fresh = rt.note_on(input(Some(3)), 60, 1.0).unwrap();
    assert_ne!(
        fresh.0.index, 0,
        "exhausted generation slot must never wrap"
    );
}

#[test]
fn invalid_preparation_and_atomic_failed_start() {
    assert!(matches!(
        fixture_runtime(0, &[], limits()),
        Err(Error::InvalidInput)
    ));
    assert!(matches!(
        Pcm::new(48000, Box::from([[f32::NAN, 0.0]])),
        Err(Error::InvalidInput)
    ));
    let samples = [Pcm::new(48000, Box::from([[1.0; 2]; 4])).unwrap()];
    assert!(matches!(
        fixture_runtime(0, &samples, limits()),
        Err(Error::InvalidInput)
    ));
    let mut rt = fixture_runtime(48000, &samples, limits()).unwrap();
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
    assert_eq!(
        rt.child(n, 60, 1.0, true, Inheritance::Snapshot),
        Err(Error::ClosedNote)
    );
    assert_eq!(rt.start(n, 0, 0, 1.0), Err(Error::ClosedNote));
}

#[test]
fn voice_scope_reuse_and_nonfinite_mix_are_observable() {
    let samples = [Pcm::new(48000, Box::from([[f32::MAX; 2]; 4])).unwrap()];
    let mut rt = fixture_runtime(48000, &samples, limits()).unwrap();
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

#[test]
fn families_separate_admission_voice_stop_and_note_release() {
    let samples = [Pcm::new(48000, Box::from([[1.0; 2]; 4])).unwrap()];
    let mut rt = fixture_runtime(48000, &samples, limits()).unwrap();
    let n = rt.note_on(input(Some(1)), 60, 1.0).unwrap();
    let f = rt.create_family(n).unwrap();
    let a = rt
        .start_family(
            f,
            0,
            0,
            0.25,
            crate::Envelope::default(),
            crate::Playback::default(),
        )
        .unwrap();
    let b = rt
        .start_family(
            f,
            0,
            10,
            0.5,
            crate::Envelope::default(),
            crate::Playback::default(),
        )
        .unwrap();
    let sibling = rt.create_family(n).unwrap();
    let c = rt
        .start_family(
            sibling,
            0,
            0,
            1.0,
            crate::Envelope::default(),
            crate::Playback::default(),
        )
        .unwrap();
    rt.finish_family(f).unwrap();
    assert_eq!(
        rt.start_family(
            f,
            0,
            0,
            1.0,
            crate::Envelope::default(),
            crate::Playback::default()
        ),
        Err(Error::ClosedFamily)
    );
    assert_eq!(rt.family_note(f), Ok(n));
    rt.stop_voice(a).unwrap();
    assert_eq!(rt.family_voice_count(f), Ok(1));
    assert!(rt.voice_active(b));
    rt.stop_family(f).unwrap();
    assert_eq!(rt.family_note(f), Err(Error::StaleHandle));
    assert_eq!(rt.pending_commands(), 0);
    assert!(rt.voice_active(c));
    assert!(rt.note(n).unwrap().2);
    rt.render(&mut [[0.0; 2]; 4]).unwrap();
    assert_eq!(rt.family_voice_count(sibling), Ok(0)); // Open until explicitly sealed.
    rt.finish_family(sibling).unwrap();
    assert_eq!(rt.family_count(), 0);
    let replacement = rt.create_family(n).unwrap();
    assert_eq!(rt.stop_family(f), Err(Error::StaleHandle));
    rt.start_family(
        replacement,
        0,
        10,
        1.0,
        crate::Envelope::default(),
        crate::Playback::default(),
    )
    .unwrap();
    rt.release(n).unwrap();
    assert_eq!(
        (rt.family_count(), rt.voice_count(), rt.pending_commands()),
        (0, 0, 0)
    );
    rt.flush_ended(|_| true);
    assert_eq!((rt.note_count(), rt.expression_count()), (0, 0));
}

#[test]
fn expression_inheritance_is_explicit_and_channel_reuse_is_isolated() {
    let samples = [Pcm::new(48000, Box::from([[1.0; 2]; 8])).unwrap()];
    let mut rt = fixture_runtime(48000, &samples, limits()).unwrap();
    let root = rt.note_on(input(None), 60, 1.0).unwrap();
    let e = rt.expression_id(root).unwrap();
    let first = Expression {
        gain: 0.5,
        pan: -0.5,
        pressure: 0x8000_0001,
        timbre: u32::MAX,
        pitch_semitones: 0.123456789,
    };
    rt.set_expression(e, first).unwrap();
    let linked = rt.child(root, 61, 1.0, false, Inheritance::Linked).unwrap();
    let snapshot = rt
        .child(root, 62, 1.0, false, Inheritance::Snapshot)
        .unwrap();
    let independent = rt
        .child(root, 63, 1.0, false, Inheritance::Independent)
        .unwrap();
    assert_eq!(rt.expression_id(linked), Ok(e));
    let snapshot_id = rt.expression_id(snapshot).unwrap();
    assert_eq!(rt.expression(snapshot_id), Ok(first));
    assert_eq!(
        rt.expression(rt.expression_id(independent).unwrap()),
        Ok(Expression::default())
    );
    let second = Expression {
        gain: 0.25,
        pressure: 0x8000_0002,
        pan: 1.0,
        // Keep this gain/pan PCM oracle at unity pitch. Fractional playback has
        // independent tone and filter-boundary checks in tests/resample.rs.
        pitch_semitones: 0.0,
        ..first
    };
    rt.set_expression(e, second).unwrap();
    assert_eq!(rt.expression(snapshot_id), Ok(first));
    assert_eq!(rt.expression(rt.expression_id(linked).unwrap()), Ok(second));
    let frozen = rt.detach_expression(linked).unwrap();
    assert_ne!(frozen, e);
    rt.set_expression(e, Expression::default()).unwrap();
    assert_eq!(rt.expression(frozen), Ok(second));
    rt.release(root).unwrap();
    let reused_channel = rt.note_on(input(None), 60, 1.0).unwrap();
    assert_ne!(rt.expression_id(reused_channel).unwrap(), e);
    rt.set_expression(
        rt.expression_id(reused_channel).unwrap(),
        Expression::default(),
    )
    .unwrap();
    rt.start(linked, 0, 0, 1.0).unwrap();
    let mut audio = [[0.0; 2]; 1];
    rt.render(&mut audio).unwrap();
    assert_eq!(audio, [[0.0, 0.25]]);
    let invalid = Expression {
        pan: f64::NAN,
        ..second
    };
    assert_eq!(rt.set_expression(frozen, invalid), Err(Error::InvalidInput));
    assert_eq!(rt.expression(frozen), Ok(second));
    rt.panic();
    rt.flush_ended(|_| true);
    assert_eq!(rt.expression_count(), 0);
    assert_eq!(rt.set_expression(frozen, first), Err(Error::StaleHandle));
}

#[test]
fn separate_budgets_reject_without_partial_ownership() {
    let pcm = [Pcm::new(48000, Box::from([[1.0; 2]; 2])).unwrap()];
    let mut rt = fixture_runtime(
        48000,
        &pcm,
        Limits {
            notes: 3,
            channels: 4,
            performances: 1,
            families: 1,
            decisions: 0,
            expressions: 1,
            voices: 1,
            commands: 1,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap();
    let n = rt.note_on(input(Some(1)), 60, 1.0).unwrap();
    assert_eq!(rt.note_on(input(Some(2)), 60, 1.0), Err(Error::Capacity));
    let child = rt.child(n, 60, 1.0, false, Inheritance::Linked).unwrap();
    let old = rt.expression_id(child).unwrap();
    assert_eq!(rt.detach_expression(child), Err(Error::Capacity));
    assert_eq!(rt.expression_id(child), Ok(old));
    assert_eq!(
        rt.child(n, 60, 1.0, false, Inheritance::Snapshot),
        Err(Error::Capacity)
    );
    assert_eq!(rt.note_count(), 2);
    let f = rt.create_family(n).unwrap();
    assert_eq!(rt.create_family(child), Err(Error::Capacity));
    rt.start_family(
        f,
        0,
        4,
        1.0,
        crate::Envelope::default(),
        crate::Playback::default(),
    )
    .unwrap();
    assert_eq!(
        rt.start_family(
            f,
            0,
            5,
            1.0,
            crate::Envelope::default(),
            crate::Playback::default()
        ),
        Err(Error::Capacity)
    );
    assert_eq!(rt.family_voice_count(f), Ok(1));
    rt.stop_family(f).unwrap();
    assert_eq!(
        (rt.family_count(), rt.voice_count(), rt.pending_commands()),
        (0, 0, 0)
    );
    rt.panic();
    rt.flush_ended(|_| false);
    assert_eq!((rt.note_count(), rt.expression_count()), (1, 1));
    rt.flush_ended(|_| true);
    assert_eq!((rt.note_count(), rt.expression_count()), (0, 0));
    // A full note pool must return an expression slot allocated during admission.
    let mut rt = fixture_runtime(
        48000,
        &pcm,
        Limits {
            notes: 1,
            channels: 4,
            performances: 1,
            expressions: 3,
            ..limits()
        },
    )
    .unwrap();
    rt.note_on(input(Some(1)), 60, 1.0).unwrap();
    for _ in 0..10 {
        assert_eq!(rt.note_on(input(Some(2)), 60, 1.0), Err(Error::Capacity));
    }
    assert_eq!(rt.expression_count(), 1);
}

#[test]
fn input_groups_and_new_handle_domains_do_not_alias() {
    let mut rt = fixture_runtime(48000, &[], limits()).unwrap();
    let a = Input {
        protocol: Protocol::Midi2,
        group: 0,
        ..input(None)
    };
    let b = Input { group: 15, ..a };
    let n = rt.note_on(a, 60, 1.0 / 65535.0).unwrap();
    let other = rt.note_on(b, 60, 1.0).unwrap();
    assert_eq!(rt.note_off(b, None), Ok(other));
    assert!(rt.note(n).unwrap().2);
    assert_eq!(
        rt.note_on(Input { group: 16, ..a }, 60, 1.0),
        Err(Error::InvalidInput)
    );
    let family = rt.create_family(n).unwrap();
    let expression = rt.expression_id(n).unwrap();
    let mut foreign = fixture_runtime(48000, &[], limits()).unwrap();
    let f = foreign.note_on(a, 60, 1.0).unwrap();
    foreign.create_family(f).unwrap();
    assert_eq!(foreign.stop_family(family), Err(Error::StaleHandle));
    assert_eq!(
        foreign.set_expression(expression, Expression::default()),
        Err(Error::StaleHandle)
    );
    rt.panic();
    rt.flush_ended(|_| true);
    rt.families.slots[0].generation = u64::MAX - 1;
    rt.expressions.slots[0].generation = u64::MAX - 1;
    let last = rt.note_on(a, 60, 1.0).unwrap();
    rt.create_family(last).unwrap();
    rt.panic();
    rt.flush_ended(|_| true);
    let n = rt.note_on(a, 60, 1.0).unwrap();
    assert_ne!(rt.expression_id(n).unwrap().0.index, 0);
    assert_ne!(rt.create_family(n).unwrap().0.index, 0);
}

#[test]
fn arena_capacity_matches_slots_through_quarantine_and_transfer_rollback() {
    for capacity in [0, 1, 63, 64, 65, 128, 129] {
        let mut arena = Arena::new(73, capacity);
        if capacity != 0 {
            arena.slots[0].generation = u64::MAX - 1;
            let last = arena.insert(19usize).unwrap();
            assert_eq!(last.generation, u64::MAX);
            let value = arena.take(last).unwrap();
            assert_eq!((arena.count(), arena.available()), (0, capacity - 1));
            arena.restore(last, value);
            assert_eq!(arena.get(last), Some(&19));
            assert_eq!((arena.count(), arena.available()), (1, capacity - 1));
            arena.remove(last);
            arena.remove(last); // Repeated/stale removal cannot free capacity twice.
        }
        let mut seed = 11u64;
        for value in 0..4096 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let index = (seed >> 32) as usize % (capacity + 1);
            let id = Handle {
                runtime: 73,
                index,
                generation: arena.slots.get(index).map_or(0, |slot| slot.generation),
            };
            match seed % 4 {
                0 => {
                    let expected = arena
                        .slots
                        .iter()
                        .position(|slot| slot.value.is_none() && slot.generation < u64::MAX);
                    let result = arena.insert(value);
                    assert_eq!(result.as_ref().ok().map(|id| id.index), expected);
                    if expected.is_none() {
                        assert_eq!(result, Err(Error::Capacity));
                    }
                }
                1 => arena.remove(id),
                2 => {
                    if let Some(value) = arena.take(id) {
                        assert!(arena.get(id).is_none());
                        arena.restore(id, value);
                        assert_eq!(arena.get(id), Some(&value));
                    }
                }
                _ => {
                    assert!(arena.take(Handle { runtime: 74, ..id }).is_none());
                    arena.remove(Handle {
                        generation: id.generation.wrapping_sub(1),
                        ..id
                    });
                }
            }
            assert_eq!(
                arena.count(),
                arena
                    .slots
                    .iter()
                    .filter(|slot| slot.value.is_some())
                    .count()
            );
            assert_eq!(
                arena.available(),
                arena
                    .slots
                    .iter()
                    .filter(|slot| slot.value.is_none() && slot.generation < u64::MAX)
                    .count()
            );
            for (index, slot) in arena.slots.iter().enumerate() {
                assert_eq!(
                    arena.free[index / 64] & (1 << (index % 64)) != 0,
                    slot.value.is_none() && slot.generation < u64::MAX,
                );
            }
            assert_eq!(
                arena
                    .free
                    .iter()
                    .map(|bits| bits.count_ones() as usize)
                    .sum::<usize>(),
                arena.available()
            );
        }
    }
}

#[test]
fn ownership_counters_match_reachable_state_under_mixed_operations() {
    fn check_links<T>(
        arena: &Arena<T>,
        mut next: Option<Index>,
        expected: usize,
        links: impl Fn(&T) -> Siblings,
        owned: impl Fn(&T) -> bool,
    ) {
        let mut previous = None;
        let mut count = 0;
        while let Some(index) = next {
            assert!(count < expected, "cycle or duplicate ownership link");
            let item = arena.slots[index.get()].value.as_ref().unwrap();
            assert!(owned(item));
            let siblings = links(item);
            assert_eq!(siblings.previous, previous);
            previous = Some(index);
            next = siblings.next;
            count += 1;
        }
        assert_eq!(count, expected, "unreachable owned slot");
    }
    let pcm = [Pcm::new(48000, Box::from([[0.25; 2]; 31])).unwrap()];
    let mut rt = fixture_runtime(48000, &pcm, limits()).unwrap();
    let mut seed = 12345u64;
    for step in 0..4000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let index = (seed >> 32) as usize % 8;
        let note = NoteId(rt.notes.id(index));
        let family = FamilyId(rt.families.id(index));
        let voice = VoiceId(rt.voices.id(index));
        match seed % 12 {
            0 => {
                let _ = rt.note_on(input(None), 60, 1.0);
            }
            1 => {
                let _ = rt.child(note, 61, 1.0, step % 2 == 0, Inheritance::Linked);
            }
            2 => {
                let _ = rt.create_family(note);
            }
            3 => {
                let _ = rt.start_family(
                    family,
                    0,
                    rt.now() + seed % 17,
                    1.0,
                    crate::Envelope::default(),
                    crate::Playback::default(),
                );
            }
            4 => {
                let _ = rt.finish_family(family);
            }
            5 => {
                let _ = rt.stop_voice(voice);
            }
            6 => {
                let _ = rt.release(note);
            }
            7 => {
                let _ = rt.detach_expression(note);
            }
            8 => {
                rt.render(&mut [[0.0; 2]; 7]).unwrap();
            }
            9 => {
                if (seed >> 16) & 1 == 0 {
                    let _ = rt.stop_family(family);
                } else {
                    let _ = rt.choke_family(family, ((seed >> 17) % 8) as u32);
                }
            }
            10 => rt.panic(),
            _ => {
                rt.flush_ended(|_| step % 3 == 0);
            }
        }
        assert!(rt.closed_notes.is_empty());
        for (i, slot) in rt.voices.slots.iter().enumerate() {
            assert_eq!(
                rt.voice_activity[i / 64] & (1 << (i % 64)) != 0,
                slot.value.is_some()
            );
        }
        for (i, slot) in rt.notes.slots.iter().enumerate() {
            if let Some(n) = slot.value {
                let id = NoteId(rt.notes.id(i));
                assert!(rt.expressions.get(n.expression.0).is_some());
                assert!(n.parent.is_none_or(|p| rt.notes.get(p.0).is_some()));
                if n.parent.is_none() {
                    assert_eq!(n.siblings.previous, None);
                    assert_eq!(n.siblings.next, None);
                }
                check_links(
                    &rt.notes,
                    n.first_child,
                    n.children,
                    |child| child.siblings,
                    |child| child.parent == Some(id),
                );
                check_links(
                    &rt.families,
                    n.first_family,
                    n.families,
                    |family| family.siblings,
                    |family| family.note == id,
                );
                assert_eq!(
                    n.children,
                    rt.notes
                        .slots
                        .iter()
                        .filter(|s| s.value.is_some_and(|child| child.parent == Some(id)))
                        .count()
                );
                assert_eq!(
                    n.families,
                    rt.families
                        .slots
                        .iter()
                        .filter(|s| s.value.is_some_and(|f| f.note == id))
                        .count()
                );
            }
        }
        for (i, slot) in rt.families.slots.iter().enumerate() {
            if let Some(f) = slot.value {
                assert!(rt.notes.get(f.note.0).is_some());
                check_links(
                    &rt.voices,
                    f.first_voice,
                    f.voices,
                    |voice| voice.siblings,
                    |voice| voice.family == FamilyId(rt.families.id(i)),
                );
                assert_eq!(
                    f.voices,
                    rt.voices
                        .slots
                        .iter()
                        .filter(|s| s
                            .value
                            .is_some_and(|v| v.family == FamilyId(rt.families.id(i))))
                        .count()
                );
                assert!(f.open || f.voices > 0);
            }
        }
        for (i, slot) in rt.expressions.slots.iter().enumerate() {
            if let Some(e) = slot.value {
                assert_eq!(
                    e.notes,
                    rt.notes
                        .slots
                        .iter()
                        .filter(|s| s
                            .value
                            .is_some_and(|n| n.expression == ExpressionId(rt.expressions.id(i))))
                        .count()
                );
                assert!(e.notes > 0);
            }
        }
    }
    rt.panic();
    rt.flush_ended(|_| true);
    assert_eq!(
        (
            rt.note_count(),
            rt.family_count(),
            rt.voice_count(),
            rt.expression_count()
        ),
        (0, 0, 0, 0)
    );
}

#[test]
fn sustain_pairs_physical_keys_fifo_and_sostenuto_captures_only_held_notes() {
    let pcm = [Pcm::new(48000, Box::from([[0.25; 2]; 64])).unwrap()];
    let mut rt = fixture_runtime(48000, &pcm, limits()).unwrap();
    let address = input(None).channel_address();
    let channel = rt.register_channel(address).unwrap();
    assert_eq!(rt.register_channel(address), Ok(channel));
    let a = rt.note_on(input(None), 60, 1.0).unwrap();
    rt.start(a, 0, 0, 1.0).unwrap();
    rt.sustain(channel, true).unwrap();
    assert_eq!(rt.note_off(input(None), None), Ok(a));
    assert_eq!(rt.key_down(a), Ok(false));
    assert!(rt.note(a).unwrap().2);
    let b = rt.note_on(input(None), 60, 1.0).unwrap();
    rt.start(b, 0, 0, 1.0).unwrap();
    rt.sostenuto(channel, true).unwrap(); // Captures b, not the pedal-held a.
    let c = rt.note_on(input(None), 60, 1.0).unwrap();
    rt.sostenuto(channel, true).unwrap(); // Repeated down is not a new capture.
    assert_eq!(rt.note_off(input(None), None), Ok(b));
    assert_eq!(rt.note_off(input(None), None), Ok(c));
    rt.sustain(channel, false).unwrap();
    assert!(!rt.note(a).unwrap().2);
    assert!(rt.note(b).unwrap().2);
    assert!(!rt.note(c).unwrap().2);
    assert_eq!(rt.voice_count(), 1);
    rt.sostenuto(channel, false).unwrap();
    assert_eq!(rt.voice_count(), 0);
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0);
    // A different port/group/channel/protocol must not inherit these pedal values.
    rt.sustain(channel, true).unwrap();
    for other in [
        Input {
            group: 1,
            ..input(None)
        },
        Input {
            port: 3,
            ..input(None)
        },
        Input {
            channel: 5,
            ..input(None)
        },
        Input {
            protocol: Protocol::Midi2,
            ..input(None)
        },
    ] {
        let n = rt.note_on(other, 60, 1.0).unwrap();
        rt.note_off(other, None).unwrap();
        assert!(!rt.note(n).unwrap().2);
        rt.flush_ended(|_| true);
    }
    rt.panic();
    assert_eq!(rt.pedals(channel), Ok((false, false)));
}

#[test]
fn mixed_timeline_is_partition_invariant_and_immediate_changes_follow_due_work() {
    let pcm = [Pcm::new(48000, Box::from([[1.0; 2]; 128])).unwrap()];
    let render = |partition: &[usize]| {
        let mut rt = fixture_runtime(
            48000,
            &pcm,
            Limits {
                commands: 16,
                behaviors: 0,
                behavior_fuel: 0,
                behavior_cells: 0,
                note_cells: 0,
                ..limits()
            },
        )
        .unwrap();
        let ch = rt.register_channel(input(None).channel_address()).unwrap();
        let n = rt.note_on(input(None), 60, 1.0).unwrap();
        rt.start(n, 0, 3, 1.0).unwrap();
        rt.schedule_event(5, Event::Sustain(ch, true)).unwrap();
        rt.schedule_event(8, Event::KeyUp(n, None)).unwrap();
        rt.schedule_event(
            10,
            Event::Expression(
                n,
                Expression {
                    gain: 0.5,
                    ..Expression::default()
                },
            ),
        )
        .unwrap();
        rt.schedule_event(
            10,
            Event::Expression(
                n,
                Expression {
                    gain: 0.25,
                    ..Expression::default()
                },
            ),
        )
        .unwrap();
        rt.schedule_event(17, Event::Sustain(ch, false)).unwrap();
        let mut output = [[0.0; 2]; 64];
        let mut at = 0;
        let mut i = 0;
        while at < output.len() {
            let len = partition[i % partition.len()].min(output.len() - at);
            rt.render(&mut output[at..at + len]).unwrap();
            at += len;
            i += 1;
        }
        rt.flush_ended(|_| true);
        assert_eq!(
            (
                rt.note_count(),
                rt.pending_commands(),
                rt.expression_count()
            ),
            (0, 0, 0)
        );
        output
    };
    let reference = render(&[1]);
    assert!(reference[..3].iter().all(|f| *f == [0.0; 2]));
    assert!(reference[3..10].iter().all(|f| *f == [1.0; 2]));
    assert!(reference[10..17].iter().all(|f| *f == [0.25; 2]));
    assert!(reference[17..].iter().all(|f| *f == [0.0; 2]));
    for blocks in [&[64][..], &[0, 3, 5, 9, 1, 0, 17], &[16], &[32]] {
        assert_eq!(render(blocks), reference);
    }
    let mut rt = fixture_runtime(48000, &pcm, limits()).unwrap();
    let n = rt.note_on(input(None), 60, 1.0).unwrap();
    rt.start(n, 0, 0, 1.0).unwrap();
    rt.schedule_event(
        8,
        Event::Expression(
            n,
            Expression {
                gain: 0.5,
                ..Expression::default()
            },
        ),
    )
    .unwrap();
    rt.render(&mut [[0.0; 2]; 8]).unwrap(); // Due-at-end remains queued.
    rt.schedule_event(
        8,
        Event::Expression(
            n,
            Expression {
                gain: 0.25,
                ..Expression::default()
            },
        ),
    )
    .unwrap();
    let mut audio = [[0.0; 2]; 1];
    rt.render(&mut audio).unwrap();
    assert_eq!(audio, [[0.25; 2]]); // Earlier queued change precedes immediate change.
}

#[test]
fn scheduled_expression_has_private_lifetime_pins_and_cancellation() {
    let mut rt = fixture_runtime(
        48000,
        &[],
        Limits {
            commands: 1,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
            ..limits()
        },
    )
    .unwrap();
    let n = rt.note_on(input(Some(1)), 60, 1.0).unwrap();
    let e = Expression {
        gain: 0.125,
        ..Expression::default()
    };
    rt.schedule_event(10, Event::Expression(n, e)).unwrap();
    assert_eq!(rt.unpin(n), Err(Error::InvalidInput));
    assert_eq!(
        rt.schedule_event(11, Event::Expression(n, e)),
        Err(Error::Capacity)
    );
    assert_eq!(rt.notes.get(n.0).unwrap().work, 1);
    rt.release(n).unwrap(); // Queue-full cleanup cancels and releases scheduler pin.
    assert_eq!(rt.pending_commands(), 0);
    rt.flush_ended(|_| true);
    let replacement = rt.note_on(input(Some(1)), 60, 1.0).unwrap();
    rt.render(&mut [[0.0; 2]; 12]).unwrap();
    assert_eq!(
        rt.expression(rt.expression_id(replacement).unwrap()),
        Ok(Expression::default())
    );
    let linked = rt
        .child(replacement, 61, 1.0, false, Inheritance::Linked)
        .unwrap();
    rt.schedule_event(16, Event::Expression(linked, e)).unwrap();
    rt.detach_expression(linked).unwrap();
    rt.render(&mut [[0.0; 2]; 4]).unwrap();
    assert_eq!(rt.pending_commands(), 1);
    rt.render(&mut []).unwrap();
    assert_eq!(rt.expression(rt.expression_id(linked).unwrap()), Ok(e));
    assert_eq!(
        rt.expression(rt.expression_id(replacement).unwrap()),
        Ok(Expression::default())
    );
    rt.schedule_event(20, Event::Expression(linked, e)).unwrap();
    rt.panic();
    rt.flush_ended(|_| true);
    assert_eq!(
        (
            rt.note_count(),
            rt.expression_count(),
            rt.pending_commands()
        ),
        (0, 0, 0)
    );
}

#[test]
fn full_queue_cannot_drop_pedal_up_and_channel_domains_are_bounded() {
    let mut rt = fixture_runtime(
        48000,
        &[],
        Limits {
            channels: 1,
            performances: 1,
            commands: 1,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
            ..limits()
        },
    )
    .unwrap();
    let address = input(None).channel_address();
    let ch = rt.register_channel(address).unwrap();
    assert_eq!(
        rt.register_channel(ChannelAddress {
            group: 1,
            ..address
        }),
        Err(Error::Capacity)
    );
    assert_eq!(
        rt.register_channel(ChannelAddress {
            group: 16,
            ..address
        }),
        Err(Error::InvalidInput)
    );
    let n = rt.note_on(input(None), 60, 1.0).unwrap();
    rt.sustain(ch, true).unwrap();
    rt.note_off(input(None), None).unwrap();
    rt.schedule_event(100, Event::Expression(n, Expression::default()))
        .unwrap();
    rt.sustain(ch, false).unwrap();
    assert!(!rt.note(n).unwrap().2);
    assert_eq!(rt.pending_commands(), 0);
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0);
    rt.schedule_event(100, Event::Sustain(ch, true)).unwrap();
    rt.panic();
    rt.render(&mut [[0.0; 2]; 101]).unwrap();
    assert_eq!(rt.pedals(ch), Ok((false, false)));
    let mut foreign = fixture_runtime(48000, &[], limits()).unwrap();
    foreign.register_channel(address).unwrap();
    assert_eq!(foreign.sustain(ch, true), Err(Error::StaleHandle));
}

fn fixture_runtime(rate: u32, pcm: &[Pcm], limits: Limits) -> Result<Runtime, Error> {
    Runtime::new(Prepared::new(rate, pcm.to_vec(), Vec::new(), 0)?, limits)
}

#[test]
fn prepared_selection_matches_independent_linear_reference() {
    let samples = vec![
        Pcm::new(48000, Box::new([[0.25, -0.5]])).unwrap(),
        Pcm::new(48000, Box::new([[0.5, 0.125]])).unwrap(),
    ];
    let mut regions = Vec::new();
    let mut seed = 19u32;
    for i in 0..40 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let a = (seed % 128) as u8;
        let b = ((seed >> 16) % 128) as u8;
        regions.push(Region {
            playback: crate::Playback::default(),
            envelope: crate::Envelope::default(),
            sample: i % 2,
            key_low: a.min(b),
            key_high: a.max(b),
            root_key: None,
            velocity_low: if i % 2 == 0 { 0.0 } else { 0.5 },
            velocity_high: 1.0,
            gain: (i % 4) as f32 / 4.0,
        });
    }
    let plan = Prepared::new(48000, samples.clone(), regions.clone(), 5120).unwrap();
    assert_eq!(plan.sample_count(), 2);
    assert_eq!(plan.region_count(), regions.len());
    let mut rt = Runtime::new(
        plan,
        Limits {
            voices: 40,
            ..limits()
        },
    )
    .unwrap();
    for key in 0..128 {
        for velocity in [0.0, 0.25, 0.5, 0.500000001, 1.0] {
            let n = rt
                .trigger(
                    Input {
                        key,
                        ..input(Some(1))
                    },
                    key,
                    velocity,
                )
                .unwrap();
            let mut expected = [0.0; 2];
            let mut selected = 0;
            for r in &regions {
                if r.key_low <= key
                    && key <= r.key_high
                    && r.velocity_low <= velocity
                    && velocity <= r.velocity_high
                {
                    selected += 1;
                    for (c, out) in expected.iter_mut().enumerate() {
                        *out += samples[r.sample].resident_frames().unwrap()[0][c]
                            * (r.gain * velocity as f32);
                    }
                }
            }
            assert_eq!(rt.voice_count(), selected);
            assert_eq!(rt.family_count(), usize::from(selected != 0));
            let mut output = [[0.0; 2]; 1];
            rt.render(&mut output).unwrap();
            assert_eq!(output[0], expected);
            assert_eq!(rt.family_count(), 0);
            assert!(rt.note(n).unwrap().2); // EOF does not erase logical ownership.
            rt.release(n).unwrap();
            let mut ends = 0;
            rt.flush_ended(|_| {
                ends += 1;
                true
            });
            assert_eq!(ends, 1);
        }
    }
}

#[test]
fn prepared_validation_and_layer_admission_are_transactional() {
    let samples = vec![Pcm::new(48000, Box::new([[0.25; 2]; 4])).unwrap()];
    let region = Region {
        playback: crate::Playback::default(),
        envelope: crate::Envelope::default(),
        sample: 0,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.0,
        velocity_high: 1.0,
        gain: 1.0,
    };
    assert!(matches!(
        Prepared::new(48000, samples.clone(), vec![region], 0),
        Err(Error::Capacity)
    ));
    for invalid in [
        Region {
            playback: crate::Playback::default(),
            envelope: crate::Envelope::default(),
            sample: 1,
            ..region
        },
        Region {
            playback: crate::Playback::default(),
            key_high: 128,
            root_key: None,
            ..region
        },
        Region {
            playback: crate::Playback::default(),
            key_low: 61,
            ..region
        },
        Region {
            playback: crate::Playback::default(),
            velocity_low: f64::NAN,
            ..region
        },
        Region {
            playback: crate::Playback::default(),
            velocity_high: -1.0,
            ..region
        },
        Region {
            playback: crate::Playback::default(),
            gain: f32::INFINITY,
            ..region
        },
    ] {
        assert!(matches!(
            Prepared::new(48000, samples.clone(), vec![invalid], 128),
            Err(Error::InvalidInput)
        ));
    }
    let plan = Prepared::new(48000, samples, vec![region, region], 2).unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            voices: 1,
            ..limits()
        },
    )
    .unwrap();
    assert_eq!(rt.trigger(input(Some(1)), 60, 1.0), Err(Error::Capacity));
    assert_eq!(
        (
            rt.note_count(),
            rt.family_count(),
            rt.expression_count(),
            rt.voice_count()
        ),
        (0, 0, 0, 0)
    );
    let no_source = rt.trigger(input(Some(1)), 61, 1.0).unwrap();
    assert_eq!(rt.voice_count(), 0);
    rt.release(no_source).unwrap();
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0);
}

#[test]
fn native_wait_clock_overflow_faults_without_scheduling_or_losing_ownership() {
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(
            vec![crate::Program::new(vec![crate::Instruction::Wait(2)]).unwrap()],
            None,
        )
        .unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            behaviors: 1,
            behavior_fuel: 2,
            behavior_cells: 0,
            note_cells: 0,
            ..limits()
        },
    )
    .unwrap();
    rt.now = u64::MAX - 1;
    let note = rt.note_on(input(Some(9)), 60, 1.).unwrap();
    let id = rt.start_behavior(note, 0).unwrap();
    assert_eq!(
        rt.behavior_outcome(id),
        Ok(Some(crate::Outcome::Fault(Error::ClockOverflow)))
    );
    assert_eq!(rt.pending_commands(), 0);
    rt.flush_ended(|_| panic!("fault still owns the original identity"));
    rt.flush_behaviors(|_, _, _| true);
    assert!(rt.input_held(note).unwrap());
    assert_eq!(rt.note_off(input(Some(9)), None), Ok(note));
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0);
}

#[test]
fn release_clock_overflow_consumes_gate_without_partial_selection_or_lost_reservations() {
    let region = Region {
        sample: 0,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    };
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[1.; 2]])).unwrap()],
        vec![region; 2],
        2,
    )
    .unwrap()
    .with_releases(
        vec![Trigger::GateRelease; 2],
        ReleaseOptions::default(),
        ReleaseOptions {
            duration: Some(2),
            ..ReleaseOptions::default()
        },
    )
    .unwrap();
    let mut rt = Runtime::new(plan, limits()).unwrap();
    let note = rt.trigger(input(Some(9)), 60, 1.).unwrap();
    rt.now = u64::MAX - 1;
    rt.key_up(note, Some(0.5)).unwrap();
    assert!(!rt.note(note).unwrap().2);
    assert_eq!(
        rt.release_status(note, Trigger::GateRelease),
        Ok(ReleaseStatus::Failed(Error::ClockOverflow))
    );
    assert_eq!(rt.release_reserve(), ReleaseReserve::default());
    assert_eq!(
        (rt.voice_count(), rt.family_count(), rt.pending_commands()),
        (0, 0, 0)
    );
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0);
}
