//! Direct starts admit preceding due script commands before observing their key.
use super::*;
use crate::engine::{EventChange, NoteEvent};

const SCRIPT: &str = "on init\ndeclare $child := 0\nend on
on note
if ($EVENT_NOTE = 63)
ignore_event($EVENT_ID)
wait(1000)
$child := play_note(67,$EVENT_VELOCITY,0,-1)
end if
end on
on release
if ($EVENT_NOTE = 63)
note_off($child)
end if
end on";

fn admission(sizes: &[usize]) -> [Vec<f32>; 2] {
    let (mut actual, mut expected) = (
        engine(Some(source(GlideMode::Time)), Some(SCRIPT), None),
        engine(None, None, None),
    );
    let roots = [note(40, 4, 60), note(41, 7, 63)];
    let (mut ids, mut reference_ids) = ([None; 2], [None; 2]);
    let mut pcm = [vec![0.; END], vec![0.; END]];
    let mut reference = [vec![0.; END], vec![0.; END]];
    let (mut at, mut block) = (0, 0);
    assert_eq!(crate::plugin::tests::allocations(|| {
        // Reclaim an unrendered exact root, then reuse its snapshot slot. Its
        // same raw60 history leaves the independently expected first delta0.
        let canceled = note(39, 4, 60);
        assert!(actual.host_note_on(canceled, 4, 60, 127, 0.));
        actual.host_note_choke(pattern(canceled));
        assert_eq!(actual.commands.snapshot_counts()[1], 0);
        // This root's Observe/Start are queued. Public start_event must create
        // it first, then capture (60,72), and render must never replay either.
        assert!(actual.host_note_on(roots[0], 4, 60, 127, 0.));
        assert!(expected.host_note_on(roots[0], 4, 60, 127, 0.));
        ids[0] = actual.start_event(&NoteEvent::new(7, 72, 127));
        reference_ids[0] = expected.start_event(&NoteEvent::new(7, 72, 127));
        assert!(ids[0].is_some() && reference_ids[0].is_some());
        assert_eq!(actual.active_voices(), 2);
        assert_eq!(actual.commands.snapshot_counts()[1], 1,
            "applied Start must retain its snapshot until its row is removed");
        while at < END {
            match at {
                128 | 176 => {
                    let gain = if at == 128 { 0. } else { 1. };
                    actual.change_event(ids[0].unwrap(), EventChange::Volume(gain));
                    expected.change_event(reference_ids[0].unwrap(), EventChange::Volume(gain));
                }
                192 => {
                    assert!(actual.host_note_on(roots[1], 7, 63, 127, 0.));
                }
                208 => {
                    ids[1] = actual.start_event(&NoteEvent::new(4, 84, 127));
                    reference_ids[1] = expected.start_event(&NoteEvent::new(4, 84, 127));
                    assert!(ids[1].is_some() && reference_ids[1].is_some());
                    assert_eq!(actual.active_voices(), 3, "waited child must stay future");
                }
                // The child observes67 after direct84. It uses the live pair
                // (84,67), rather than the pair present when its parent waited.
                240 => assert!(expected.host_note_on(roots[1], 7, 67, 127, 0.)),
                HELD => {
                    for root in roots {
                        actual.host_note_off(pattern(root));
                        expected.host_note_off(pattern(root));
                    }
                    for i in 0..2 {
                        actual.release_event(ids[i].unwrap());
                        expected.release_event(reference_ids[i].unwrap());
                    }
                }
                _ => {}
            }
            let boundary = [128, 176, 192, 208, 240, HELD, END].into_iter()
                .filter(|b| *b > at).min().unwrap();
            let count = sizes[block % sizes.len()].min(boundary - at);
            let [l, r] = &mut pcm;
            actual.render_checked(&mut l[at..at + count], &mut r[at..at + count]).unwrap();
            assert_eq!(actual.commands.snapshot_counts(), [0, 0, 0],
                "completed render must reclaim every payload without growing a pool");
            for frame in at..at + count {
                for (i, start, interval) in [(0, 0, 12), (1, 208, 21)] {
                    if frame >= start {
                        expected.change_event(reference_ids[i].unwrap(), EventChange::Tune(f64::from(
                            tuning(frame - start, interval, GlideMode::Time, false))));
                    }
                }
                if frame >= 240 {
                    expected.host_expression(pattern(roots[1]), HostExpression::Tune(
                        tuning(frame - 240, -17, GlideMode::Time, false)));
                }
                let [l, r] = &mut reference;
                expected.render_checked(&mut l[frame..frame + 1], &mut r[frame..frame + 1]).unwrap();
            }
            at += count;
            block += 1;
        }
        actual.mark_host_notes();
        expected.mark_host_notes();
    }), 0);
    close(&pcm, &reference, "ordered queued and direct admission");
    assert_eq!(actual.active_voices(), 0);
    assert_eq!(expected.active_voices(), 0);
    for root in roots {
        assert_eq!(actual.host_note_end_at(root), expected.host_note_end_at(root));
        assert!(actual.host_note_end_at(root).is_some());
    }
    pcm
}

#[test]
fn queued_and_direct_starts_keep_live_history_waits_mute_and_exact_ends_without_heap() {
    let fixed = admission(&[128]);
    let fragmented = admission(&[1, 7, 31, 128]);
    close(&fragmented, &fixed, "direct admission host fragmentation");
}
