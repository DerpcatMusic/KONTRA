//! Explicit authored MonoRetrigger against an independently scheduled Poly rig.
use super::*;

#[path = "note_mode_shared_volume_tests.rs"]
mod shared_volume;

#[path = "note_mode_shared_pitch_tests.rs"]
mod shared_pitch;
use crate::{
    audio::Sample,
    engine::{Engine, Expression, HostNote, HostPattern, NoteMode},
    import::{Group, Zone},
};

const END: usize = 1024;
fn rig(short: bool) -> Engine {
    let path = |index| std::path::PathBuf::from(format!("mono-{index}"));
    let samples = (0..2)
        .map(|index| {
            (
                path(index),
                Sample {
                    rate: 48000,
                    frames: (0..if short && index == 0 { 96 } else { 4096 })
                        .map(|i| [0.15 + i as f32 / 32768., -0.075 + i as f32 / 65536.])
                        .collect(),
                },
            )
        })
        .collect();
    let zones = [60, 62, 64, 65, 72]
        .into_iter()
        .map(|key| Zone {
            sample: path(usize::from(key != 60)),
            root: key,
            low_key: key,
            high_key: key,
            low_velocity: if key == 60 { 100 } else { 0 },
            ..Default::default()
        })
        .collect();
    let bank = Bank::from_samples(vec![Group::default()], zones, samples).unwrap();
    let mut e = Engine::default();
    e.reset(48000.);
    e.attack = 0.;
    e.release = 0.003;
    e.set_bank(Some(Box::new(bank)));
    e
}
fn host(id: i32, physical: u8, key: u8) -> HostNote {
    HostNote {
        port: 0,
        channel: physical,
        key,
        id,
        clap: true,
    }
}
fn pattern(n: HostNote) -> HostPattern {
    HostPattern {
        port: i32::from(n.port),
        channel: i32::from(n.channel),
        key: i32::from(n.key),
        id: n.id,
        clap: n.clap,
    }
}
fn last(e: &Engine) -> EventId {
    e.player.voices.last().unwrap().event
}
fn replay(e: &mut Engine, key: u8, physical: u8, host: Option<crate::engine::HostRef>) -> EventId {
    replay_bound(e, key, physical, host, None)
}
fn replay_bound(
    e: &mut Engine,
    key: u8,
    physical: u8,
    host: Option<crate::engine::HostRef>,
    per_note: Option<crate::engine::PerNoteInput>,
) -> EventId {
    e.start_event(&NoteEvent {
        owner: Some((physical, key)),
        input_channel: Some(physical),
        host_note: host,
        per_note,
        ..NoteEvent::new(7, key, 127)
    })
    .unwrap()
}
fn render(e: &mut Engine, pcm: &mut [Vec<f32>; 2], from: usize, to: usize, parts: &[usize]) {
    let (mut at, mut part) = (from, 0);
    while at < to {
        let n = parts[part % parts.len()].min(to - at);
        let [l, r] = pcm;
        e.render_checked(&mut l[at..at + n], &mut r[at..at + n])
            .unwrap();
        at += n;
        part += 1;
    }
}
fn equal(a: &[Vec<f32>; 2], b: &[Vec<f32>; 2]) {
    assert!(
        b.iter().flatten().any(|x| x.abs() > 0.1),
        "reference must contain real audio"
    );
    let error = a
        .iter()
        .flatten()
        .zip(b.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0., f32::max);
    let ulps = a
        .iter()
        .flatten()
        .zip(b.iter().flatten())
        .map(|(a, b)| a.to_bits().abs_diff(b.to_bits()))
        .max()
        .unwrap();
    eprintln!("authored mono complete PCM maximum error={error:e}, bit distance={ulps}");
    for ch in 0..2 {
        for (frame, (a, b)) in a[ch].iter().zip(&b[ch]).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "channel{ch} frame{frame}");
        }
    }
}

fn equal_partitions(a: &[Vec<f32>; 2], b: &[Vec<f32>; 2]) {
    // B ends at289. swap_remove runs at the fragment end:295 for split
    // output,384 for whole output, moving A196 into B's index at different
    // times. At most seven same-sign voices therefore sum in different f32
    // orders. Bound both six-addition sums by gamma6, converting their
    // rounded measured peak back to the exact sum bound. Each partition's
    // Mono-versus-independent-Poly comparison above remains bit-exact.
    let roundoff = f64::from(f32::EPSILON) / 2.;
    let gamma = 6. * roundoff / (1. - 6. * roundoff);
    for channel in 0..2 {
        let peak = a[channel]
            .iter()
            .chain(&b[channel])
            .map(|x| f64::from(x.abs()))
            .fold(0., f64::max);
        let bound = 2. * gamma / (1. - gamma) * peak;
        for (frame, (a, b)) in a[channel].iter().zip(&b[channel]).enumerate() {
            assert!(a.is_finite() && b.is_finite());
            assert!(if channel == 0 {
                *a >= 0. && *b >= 0.
            } else {
                *a <= 0. && *b <= 0.
            });
            assert!(
                (f64::from(*a) - f64::from(*b)).abs() <= bound,
                "channel{channel} frame{frame}: different summation order exceeded {bound:e}"
            );
        }
    }
}

fn performance(parts: &[usize], lanes: bool) -> [Vec<f32>; 2] {
    let (mut actual, mut expected) = (rig(false), rig(false));
    actual.set_lanes(lanes);
    expected.set_lanes(lanes);
    actual.set_note_mode(7, NoteMode::MonoRetrigger).unwrap();
    let b = host(900, 5, 64);
    let (mut pcm, mut reference) = (
        std::array::from_fn(|_| vec![0.; END]),
        std::array::from_fn(|_| vec![0.; END]),
    );
    let (mut a0, mut b0, mut a1, mut c0, mut a2) = (None, None, None, None, None);
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            let address = crate::engine::InputAddress {
                port: 0,
                channel: 4,
                key: 60,
            };
            for e in [&mut actual, &mut expected] {
                assert!(e.note_on_input_velocity_pitch(
                    address,
                    7,
                    60,
                    crate::midi::Velocity::FULL,
                    Some(60.)
                ));
                e.note_on_from(9, 9, 72, 127);
            }
            let original = actual.player.input_keys[4][60].per_note.unwrap();
            let reference_note = expected.player.input_keys[4][60].per_note;
            a0 = Some(expected.player.voices[0].event);
            let mut at = 0;
            for to in [
                64, 80, 84, 88, 96, 104, 112, 128, 160, 176, 196, 208, 256, END,
            ] {
                render(&mut actual, &mut pcm, at, to, parts);
                render(&mut expected, &mut reference, at, to, parts);
                at = to;
                match to {
                    64 => {
                        assert!(actual.host_note_on(b, 7, 64, 127, 0.));
                        assert!(expected.host_note_on(b, 7, 64, 127, 0.));
                        b0 = Some(last(&expected));
                        expected.fade_event(a0.unwrap(), STEAL_FADE, 0., true);
                    }
                    80 => {
                        for e in [&mut actual, &mut expected] {
                            e.note_on_from(7, 8, 100, 127);
                        }
                        assert!(actual.player.input_keys[8][100].accepted.is_none());
                    }
                    84 | 104 => {
                        // Reject the same physical key while shadowed, then while
                        // selected. Native receiver attributes remain current,
                        // while the accepted root retains its detached generation.
                        for e in [&mut actual, &mut expected] {
                            assert!(e.per_note_manage(address, true, true));
                            assert!(e.note_on_input_velocity_pitch(
                                address,
                                7,
                                60,
                                crate::midi::Velocity::from_midi1(1),
                                Some(71.375)
                            ));
                        }
                        let accepted = actual.player.input_keys[4][60];
                        assert_eq!(accepted.started, 0);
                        assert_eq!(accepted.onset_velocity.unwrap().midi1(), 127);
                        assert_eq!(accepted.per_note.unwrap().controls, original.controls);
                        assert_eq!(accepted.accepted.unwrap().ordinal, 1);
                        assert_eq!(actual.player.key_on[7][60], to as u64);
                        assert_eq!(actual.player.keys[7][60], 1);
                    }
                    88 => {
                        for e in [&mut actual, &mut expected] {
                            e.player.midi.set_expression(
                                7,
                                60,
                                Expression {
                                    gain: 0.6,
                                    pan: -0.3,
                                    tune: 0.25,
                                    ..Default::default()
                                },
                            );
                        }
                    }
                    96 => {
                        actual.host_note_off(pattern(b));
                        expected.host_note_off(pattern(b));
                        a1 = Some(replay_bound(&mut expected, 60, 4, None, reference_note));
                        expected.fade_event(b0.unwrap(), STEAL_FADE, 0., true);
                    }
                    112 => {
                        for e in [&mut actual, &mut expected] {
                            e.note_on_from(7, 6, 62, 127);
                        }
                        c0 = Some(last(&expected));
                        expected.fade_event(a1.unwrap(), STEAL_FADE, 0., true);
                    }
                    128 => {
                        actual.player.midi.set_expression(
                            7,
                            65,
                            Expression {
                                tune: 10000.,
                                ..Default::default()
                            },
                        );
                        actual.note_on_from(7, 3, 65, 127);
                        assert_eq!(
                            actual.take_stream_error().unwrap().kind,
                            super::super::stream::StreamErrorKind::UnrepresentablePitch
                        );
                        assert!(actual.player.input_keys[3][65].accepted.is_none());
                    }
                    160 => {
                        actual.note_off_from(7, 6, 62);
                        expected.note_off_from(7, 6, 62);
                        a2 = Some(replay_bound(&mut expected, 60, 4, None, reference_note));
                        expected.fade_event(c0.unwrap(), STEAL_FADE, 0., true);
                    }
                    176 => {
                        actual.cc(7, 123, 0);
                        expected.note_off_from(7, 4, 60);
                        assert!(
                            actual
                                .player
                                .input_keys
                                .iter()
                                .flatten()
                                .all(|k| k.channel != 7 || k.onset_velocity.is_none())
                        );
                    }
                    196 => {
                        actual.player.next_event = u32::MAX;
                        for e in [&mut actual, &mut expected] {
                            e.note_on_from(7, 2, 60, 127);
                        }
                        expected.fade_event(a2.unwrap(), STEAL_FADE, 0., true);
                        assert_ne!(
                            last(&actual),
                            actual.player.voices[0].event,
                            "wrap cannot alias an old root tail"
                        );
                    }
                    208 => {
                        let ordinal = actual.player.note_ordinal;
                        actual.player.note_ordinal = u64::MAX;
                        actual.note_on_from(7, 1, 64, 127);
                        actual.player.note_ordinal = ordinal;
                        assert!(actual.player.input_keys[1][64].accepted.is_none());
                    }
                    256 => {
                        for e in [&mut actual, &mut expected] {
                            e.note_off_from(7, 2, 60);
                            e.note_off_from(9, 9, 72);
                        }
                    }
                    _ => {}
                }
            }
            actual.mark_host_notes();
            expected.mark_host_notes();
            assert_eq!(actual.host_note_end_at(b), expected.host_note_end_at(b));
            assert_eq!(actual.host_note_end_at(b), Some(289));
            assert!(!actual.host_note_pending(b));
            assert_eq!(actual.active_voices(), 0);
            assert_eq!(expected.active_voices(), 0);
        }),
        0
    );
    equal(&pcm, &reference);
    pcm
}

#[test]
fn authored_mono_retrigger_keeps_canonical_fallback_expression_and_channel_pcm_without_heap() {
    for lanes in [false, true] {
        eprintln!("authored mono lanes={lanes}");
        let full = performance(&[128], lanes);
        let split = performance(&[1, 7, 31, 127], lanes);
        equal_partitions(&full, &split);
    }
}

#[test]
fn held_exact_root_survives_eof_and_wrapping_admission_then_fallback_pcm_without_heap() {
    let (mut actual, mut expected) = (rig(true), rig(true));
    actual.set_note_mode(7, NoteMode::MonoRetrigger).unwrap();
    let (a, b) = (host(910, 4, 60), host(911, 5, 64));
    let (mut pcm, mut reference) = (
        std::array::from_fn(|_| vec![0.; END]),
        std::array::from_fn(|_| vec![0.; END]),
    );
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            assert!(actual.host_note_on(a, 7, 60, 127, 0.));
            assert!(expected.host_note_on(a, 7, 60, 127, 0.));
            let original = last(&actual);
            render(&mut actual, &mut pcm, 0, 128, &[1, 7, 31]);
            render(&mut expected, &mut reference, 0, 128, &[1, 7, 31]);
            actual.mark_host_notes();
            assert!(actual.host_note_pending(a));
            assert!(actual.host_note_end_at(a).is_none());
            actual.player.next_event = u32::MAX;
            assert!(actual.host_note_on(b, 7, 64, 127, 0.));
            assert_ne!(last(&actual), original);
            assert!(expected.host_note_on(b, 7, 64, 127, 0.));
            let b_source = last(&expected);
            render(&mut actual, &mut pcm, 128, 160, &[1, 7, 31]);
            render(&mut expected, &mut reference, 128, 160, &[1, 7, 31]);
            actual.host_note_off(pattern(b));
            expected.host_note_off(pattern(b));
            let reference_root = expected
                .player
                .host_notes
                .active
                .iter()
                .copied()
                .find(|id| expected.player.host_notes.get(*id).unwrap().note == a)
                .unwrap();
            replay(&mut expected, 60, 4, Some(reference_root));
            expected.fade_event(b_source, STEAL_FADE, 0., true);
            assert_eq!(last(&actual), original);
            assert_eq!(actual.player.voices.last().unwrap().counter_start, 160);
            render(&mut actual, &mut pcm, 160, 288, &[1, 7, 31]);
            render(&mut expected, &mut reference, 160, 288, &[1, 7, 31]);
            actual.mark_host_notes();
            assert!(actual.host_note_end_at(a).is_none());
            actual.host_note_off(pattern(a));
            expected.host_note_off(pattern(a));
            render(&mut actual, &mut pcm, 288, END, &[1, 7, 31]);
            render(&mut expected, &mut reference, 288, END, &[1, 7, 31]);
            actual.mark_host_notes();
            expected.mark_host_notes();
            assert_eq!(actual.host_note_end_at(a), Some(288));
            assert_eq!(actual.host_note_end_at(a), expected.host_note_end_at(a));
            assert_eq!(actual.host_note_end_at(b), expected.host_note_end_at(b));
            assert!(!actual.host_note_pending(a));
            assert_eq!(actual.active_voices(), 0);
        }),
        0
    );
    equal(&pcm, &reference);
}

#[test]
fn mono_selected_source_survives_pedal_key_reuse_and_batched_mpe_cleanup_pcm_without_heap() {
    let (mut actual, mut expected) = (rig(false), rig(false));
    actual.set_note_mode(7, NoteMode::MonoRetrigger).unwrap();
    let (mut pcm, mut reference) = (
        std::array::from_fn(|_| vec![0.; END]),
        std::array::from_fn(|_| vec![0.; END]),
    );
    let (mut a, mut b, mut returned_a, mut reused_a, mut c) = (None, None, None, None, None);
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for e in [&mut actual, &mut expected] {
                e.note_on_from(7, 4, 60, 127);
            }
            a = Some(last(&expected));
            let mut at = 0;
            for to in [
                32, 64, 80, 88, 96, 112, 128, 160, 176, 256, 272, 304, 360, 512, END,
            ] {
                render(&mut actual, &mut pcm, at, to, &[1, 7, 31]);
                render(&mut expected, &mut reference, at, to, &[1, 7, 31]);
                at = to;
                match to {
                    32 => {
                        for e in [&mut actual, &mut expected] {
                            e.note_on_from(7, 5, 64, 127);
                        }
                        b = Some(last(&expected));
                        expected.fade_event(a.unwrap(), STEAL_FADE, 0., true);
                    }
                    64 => {
                        for e in [&mut actual, &mut expected] {
                            e.cc(5, 64, 127);
                        }
                    }
                    80 => {
                        for e in [&mut actual, &mut expected] {
                            e.note_off_from(7, 5, 64);
                        }
                        returned_a = Some(replay(&mut expected, 60, 4, None));
                        expected.fade_event(b.unwrap(), STEAL_FADE, 0., true);
                    }
                    88 => {
                        for e in [&mut actual, &mut expected] {
                            e.cc(4, 64, 127);
                        }
                    }
                    96 => {
                        for e in [&mut actual, &mut expected] {
                            e.note_off_from(7, 4, 60);
                        }
                    }
                    112 => {
                        for e in [&mut actual, &mut expected] {
                            e.note_on_from(7, 4, 60, 127);
                        }
                        reused_a = Some(last(&expected));
                        expected.fade_event(returned_a.unwrap(), STEAL_FADE, 0., true);
                    }
                    128 => {
                        for e in [&mut actual, &mut expected] {
                            e.all_sound_off_from(7, 1 << 5);
                        }
                    }
                    160 => {
                        for e in [&mut actual, &mut expected] {
                            e.note_off_from(7, 4, 60);
                        }
                    }
                    176 => {
                        for e in [&mut actual, &mut expected] {
                            e.note_on_from(7, 6, 62, 127);
                        }
                        c = Some(last(&expected));
                        expected.fade_event(reused_a.unwrap(), STEAL_FADE, 0., true);
                    }
                    256 => {
                        for e in [&mut actual, &mut expected] {
                            e.note_on_from(7, 1, 60, 127);
                        }
                        a = Some(last(&expected));
                        expected.fade_event(c.unwrap(), STEAL_FADE, 0., true);
                    }
                    272 => {
                        for e in [&mut actual, &mut expected] {
                            e.note_on_from(7, 2, 64, 127);
                        }
                        expected.fade_event(a.unwrap(), STEAL_FADE, 0., true);
                    }
                    304 => {
                        for e in [&mut actual, &mut expected] {
                            e.configure_mpe(Some((0, 2, 48)));
                        }
                        replay(&mut expected, 62, 6, None);
                    }
                    360 => {
                        for e in [&mut actual, &mut expected] {
                            e.note_off_from(7, 6, 62);
                        }
                    }
                    512 => {
                        for e in [&mut actual, &mut expected] {
                            e.cc(4, 64, 0);
                            e.cc(5, 64, 0);
                        }
                    }
                    _ => {}
                }
            }
            assert_eq!(actual.active_voices(), 0);
            assert_eq!(expected.active_voices(), 0);
            assert_eq!(
                actual.player.admission_clock, 8,
                "MPE cleanup must produce just one surviving-root restart"
            );
        }),
        0
    );
    equal(&pcm, &reference);
}

#[test]
fn mono_ready_candidate_cuts_native_release_before_pressure_and_preserves_rejected_pcm() {
    use crate::{
        engine::bank::VoiceGroup,
        import::{Loop, VoiceLimit},
    };
    let create = |cut| {
        let groups = vec![
            Group::default(),
            Group {
                release_trigger: true,
                release_trigger_note_monophonic: cut,
                key_tracking: false,
                pan: -1.,
                ..Default::default()
            },
        ];
        let zones = vec![
            Zone {
                root: 60,
                low_key: 60,
                high_key: 60,
                low_velocity: 100,
                pan: 1.,
                ..Default::default()
            },
            Zone {
                root: 72,
                low_key: 72,
                high_key: 72,
                low_velocity: 100,
                pan: 1.,
                ..Default::default()
            },
            Zone {
                group: 1,
                root: 60,
                low_key: 60,
                high_key: 60,
                loop_range: Some(Loop {
                    start: 0,
                    end: 64,
                    alternating: false,
                    until_release: false,
                    crossfade: 0,
                }),
                ..Default::default()
            },
        ];
        let mut bank = Bank::from_samples(
            groups,
            zones,
            vec![(
                Default::default(),
                Sample {
                    rate: 48000,
                    frames: (0..4096).map(|i| [0.2 + i as f32 / 32768.; 2]).collect(),
                },
            )],
        )
        .unwrap()
        .with_prepared_settings(|rows| rows[1].envelope = Some(Ahdsr::UNITY))
        .unwrap();
        bank.polyphony = VoiceGroup::from(&VoiceLimit {
            max_voices: 2,
            kill_mode: 2,
            prefer_released: false,
            fade_ms: 10,
            exclusion_group: -1,
        });
        let mut e = Engine::default();
        e.reset(48000.);
        e.attack = 0.;
        e.release = 0.003;
        e.set_bank(Some(Box::new(bank)));
        e
    };
    let (mut actual, mut expected) = (create(true), create(false));
    actual.set_note_mode(7, NoteMode::MonoRetrigger).unwrap();
    let roots = [
        host(920, 4, 60),
        host(921, 9, 72),
        host(922, 4, 60),
        host(923, 4, 60),
    ];
    let (mut pcm, mut reference) = (
        std::array::from_fn(|_| vec![0.; END]),
        std::array::from_fn(|_| vec![0.; END]),
    );
    let mut release = None;
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for e in [&mut actual, &mut expected] {
                assert!(e.host_note_on(roots[0], 7, 60, 127, 0.));
            }
            let mut at = 0;
            for to in [32, 224, 240, 256, 304, 384, 400, 512, END] {
                render(&mut actual, &mut pcm, at, to, &[1, 7, 31]);
                render(&mut expected, &mut reference, at, to, &[1, 7, 31]);
                at = to;
                match to {
                    32 => {
                        for e in [&mut actual, &mut expected] {
                            e.host_note_off(pattern(roots[0]));
                        }
                        release = Some(
                            expected
                                .player
                                .voices
                                .iter()
                                .find(|v| v.release_trigger)
                                .unwrap()
                                .event,
                        );
                    }
                    // The parent's exponential release has already retired. Its
                    // looped release source alone precedes the unrelated attack.
                    240 => {
                        for e in [&mut actual, &mut expected] {
                            assert!(e.host_note_on(roots[1], 9, 72, 127, 0.));
                        }
                    }
                    224 => {
                        for e in [&mut actual, &mut expected] {
                            assert!(e.host_note_on(roots[2], 7, 60, 1, 0.));
                        }
                        assert!(
                            actual
                                .player
                                .host_notes
                                .active
                                .iter()
                                .filter_map(|r| actual.player.host_notes.get(*r))
                                .find(|o| o.note == roots[2])
                                .unwrap()
                                .accepted
                                .is_none()
                        );
                    }
                    256 => {
                        expected.fade_event(release.unwrap(), 0.001, 0., true);
                        for e in [&mut actual, &mut expected] {
                            assert!(e.host_note_on(roots[3], 7, 60, 127, 0.));
                        }
                    }
                    304 => {
                        actual.mark_host_notes();
                        expected.mark_host_notes();
                        assert_eq!(actual.host_note_end_at(roots[0]), Some(304));
                        assert_eq!(
                            actual.host_note_end_at(roots[0]),
                            expected.host_note_end_at(roots[0])
                        );
                        assert!(
                            actual.host_note_pending(roots[1]),
                            "unrelated pressure candidate must survive"
                        );
                    }
                    384 => {
                        for e in [&mut actual, &mut expected] {
                            e.host_note_off(pattern(roots[3]));
                        }
                    }
                    400 => {
                        for e in [&mut actual, &mut expected] {
                            e.host_note_off(pattern(roots[2]));
                        }
                    }
                    512 => {
                        for e in [&mut actual, &mut expected] {
                            e.cc_from(7, 4, 120, 0);
                            e.cc_from(9, 9, 120, 0);
                            // Sound-off stops audio while retaining physical key
                            // ownership. Its actual Off arrives before the fade ends.
                            e.host_note_off(pattern(roots[1]));
                        }
                    }
                    _ => {}
                }
            }
            actual.mark_host_notes();
            expected.mark_host_notes();
            for root in roots {
                assert_eq!(
                    actual.host_note_end_at(root),
                    expected.host_note_end_at(root)
                );
                assert!(!actual.host_note_pending(root));
            }
            assert_eq!(actual.host_note_end_at(roots[1]), Some(752));
            assert_eq!(actual.active_voices(), 0);
        }),
        0
    );
    equal(&pcm, &reference);
    // Two untouched right-hand sources sum to about .404. Erroneously
    // fading H for10ms at256 leaves only about .377 at this boundary.
    assert!(
        pcm[1][320] > 0.39,
        "pressure must preserve the unrelated right-hand attack"
    );
}

#[test]
fn mono_rejected_and_compatible_bank_upgrades_preserve_fallback_pcm_without_heap() {
    use crate::{
        engine::{Streaming, zone},
        import::Instrument,
        ksp::engine::{ZoneEdit, ZonePar},
    };
    let files = tempfile::tempdir().unwrap();
    let path = files.path().join("mono-upgrade.wav");
    let mut wav = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for frame in 0..4096 {
        for value in [0.15 + frame as f32 / 32768., -0.075 + frame as f32 / 65536.] {
            wav.write_sample(value).unwrap();
        }
    }
    wav.finalize().unwrap();
    let instrument = Instrument {
        groups: vec![Group::default()],
        zones: [60, 62, 64]
            .into_iter()
            .map(|key| Zone {
                sample: path.clone(),
                root: key,
                low_key: key,
                high_key: key,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let resident = || {
        Box::new(
            Bank::load_counting(
                &instrument,
                crate::engine::MEMORY_LIMIT,
                Streaming::RamOnly,
                &[],
                &std::sync::atomic::AtomicU32::new(0),
            )
            .unwrap(),
        )
    };
    let mut initial = resident();
    // A real prepared no-op mapping establishes revision1. An older preload
    // cannot replace it; a fill rebased against this exact parent can.
    let job = initial
        .zone_job(ZoneEdit {
            zone: 0,
            par: ZonePar::Group,
            value: 0,
            slot: 0,
            id: -1,
        })
        .unwrap();
    let base = job.base.clone();
    let mut prepared = zone::Prepared::build(vec![job], base, &|| false);
    assert!(initial.install_zone_map(&mut prepared));
    let parent = initial.zone_state.as_ref().unwrap().clone();
    let context = initial.zone_context.as_ref().unwrap().clone();
    assert_eq!(parent.revision, 1);
    let mut stale = Some(resident());
    let mut fill = resident();
    fill.rebase_zone_preload(&context, parent.clone(), &|| false)
        .unwrap();
    let mut streamed = Box::new(Bank::load_bare(&instrument).unwrap());
    // The fill publishes its own prepared State. A subsequent handoff must
    // name that exact parent, rather than the initial revision it replaced.
    streamed
        .rebase_zone_preload(
            fill.zone_context.as_ref().unwrap(),
            fill.zone_state.as_ref().unwrap().clone(),
            &|| false,
        )
        .unwrap();
    assert_eq!(streamed.streamed_samples(), 1);
    let (mut actual, mut expected) = (Engine::default(), Engine::default());
    for e in [&mut actual, &mut expected] {
        e.reset(48000.);
        e.attack = 0.;
        e.release = 0.003;
    }
    actual.set_bank(Some(initial));
    expected.set_bank(Some(resident()));
    let mut reference_stream = Box::new(Bank::load_bare(&instrument).unwrap());
    let reference_bank = expected.bank.as_deref().unwrap();
    reference_stream
        .rebase_zone_preload(
            reference_bank.zone_context.as_ref().unwrap(),
            reference_bank.zone_state.as_ref().unwrap().clone(),
            &|| false,
        )
        .unwrap();
    actual.set_note_mode(7, NoteMode::MonoRetrigger).unwrap();
    let (mut fill, mut streamed, mut reference_stream) =
        (Some(fill), Some(streamed), Some(reference_stream));
    let (mut rejected, mut retired_fill, mut retired_stream, mut retired_reference) =
        (None, None, None, None);
    let (mut pcm, mut reference) = (
        std::array::from_fn(|_| vec![0.; END]),
        std::array::from_fn(|_| vec![0.; END]),
    );
    let (b, c) = (host(930, 5, 64), host(931, 6, 62));
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for e in [&mut actual, &mut expected] {
                e.note_on_from(7, 4, 60, 127);
            }
            let a = last(&expected);
            let accepted = actual.player.input_keys[4][60].accepted.unwrap();
            let mut b_source = None;
            let mut at = 0;
            for to in [32, 64, 80, 96, 384, 512, END] {
                render(&mut actual, &mut pcm, at, to, &[1, 7, 31, 127]);
                render(&mut expected, &mut reference, at, to, &[1, 7, 31, 127]);
                at = to;
                match to {
                    32 => {
                        for e in [&mut actual, &mut expected] {
                            assert!(e.host_note_on(b, 7, 64, 127, 0.));
                        }
                        b_source = Some(last(&expected));
                        expected.fade_event(a, STEAL_FADE, 0., true);
                    }
                    64 => {
                        rejected = actual.upgrade_bank(stale.take().unwrap());
                        assert_eq!(actual.note_mode(7), Some(NoteMode::MonoRetrigger));
                        assert_eq!(
                            actual.player.input_keys[4][60].accepted.unwrap().ordinal,
                            accepted.ordinal
                        );
                        assert!(!actual.zone_upgrade_ready(rejected.as_deref().unwrap()));
                    }
                    80 => {
                        assert!(actual.zone_upgrade_ready(fill.as_deref().unwrap()));
                        retired_fill = actual.upgrade_bank(fill.take().unwrap());
                        assert_eq!(actual.note_mode(7), Some(NoteMode::MonoRetrigger));
                    }
                    96 => {
                        actual.host_note_off(pattern(b));
                        expected.host_note_off(pattern(b));
                        replay(&mut expected, 60, 4, None);
                        expected.fade_event(b_source.unwrap(), STEAL_FADE, 0., true);
                        assert_eq!(last(&actual), accepted.event);
                        assert_eq!(actual.player.voices.last().unwrap().counter_start, 96);
                    }
                    384 => {
                        assert!(actual.zone_upgrade_ready(streamed.as_deref().unwrap()));
                        retired_stream = actual.upgrade_bank(streamed.take().unwrap());
                        retired_reference = expected.set_bank(reference_stream.take());
                        assert_eq!(actual.note_mode(7), Some(NoteMode::Poly));
                        assert_eq!(actual.active_voices(), 0);
                        assert!(actual.player.input_keys[4][60].accepted.is_none());
                        for e in [&mut actual, &mut expected] {
                            assert!(e.host_note_on(c, 7, 62, 127, 0.));
                        }
                    }
                    512 => {
                        for e in [&mut actual, &mut expected] {
                            e.host_note_off(pattern(c));
                        }
                    }
                    _ => {}
                }
            }
            for e in [&mut actual, &mut expected] {
                e.mark_host_notes();
                assert_eq!(e.host_note_end_at(b), Some(289));
                assert_eq!(e.host_note_end_at(c), Some(705));
                assert!(!e.host_note_pending(c));
                assert_eq!(e.active_voices(), 0);
            }
        }),
        0
    );
    equal(&pcm, &reference);
    drop((
        rejected,
        retired_fill,
        retired_stream,
        retired_reference,
        prepared,
    ));
}
