//! Actual MIDI1 receiver transactions; the exterior oracle contains literal pitches.
use super::*;

pub(super) fn controls(events: &mut EventList, origin: usize, at: usize) {
    // Interleaved physical selectors must not share an RPN transaction.
    let rows: &[(u8, u8, u8)] = match at {
        7 => &[(1, 101, 0), (0, 101, 0), (1, 100, 0), (0, 100, 0)],
        8 => &[(1, 6, 12), (1, 38, 50)],
        9 => &[(0, 6, 4), (0, 38, 25)],
        // Both extremes belong to this accepted 128-frame source span.
        12 => &[(1, 6, 80)],
        13 => &[(1, 6, 12), (1, 38, 50)],
        20 => &[(1, 96, 0)],
        21 => &[(1, 97, 0)],
        24 => &[(0, 96, 0)],
        25 => &[(0, 97, 0)],
        // A new coarse byte clears the earlier fine byte.
        75 => &[(1, 38, 99), (1, 6, 8)],
        80 => &[(1, 101, 127), (1, 100, 127), (1, 6, 100)],
        85 => &[(1, 99, 0), (1, 98, 0), (1, 6, 90)],
        864 => &[(2, 101, 0), (2, 100, 0), (2, 6, 7), (2, 38, 25)],
        // CC121 at1376 retains sensitivity but clears both selected decoders.
        1393 => &[(1, 6, 100), (0, 38, 99)],
        1450 => &[(1, 101, 0), (1, 100, 0), (1, 6, 2)],
        1451 => &[(0, 101, 0), (0, 100, 0), (0, 6, 3), (0, 38, 40)],
        _ => &[],
    };
    for &(channel, cc, value) in rows {
        // This typed wire body enters Midi1Decoder::cc, not legacy In::Cc's
        // parameter-only receiver entry point.
        push_cc(events, origin, at, channel, cc, value);
    }
    if at == 27 {
        for cc in [101, 100, 6, 38, 96, 97] {
            events.push(Event::on_port(
                (at - origin) as u32,
                0,
                EventBody::ControlChange2 {
                    group: 0,
                    channel: 1,
                    cc,
                    value: u32::MAX,
                },
            ));
        }
    }
    if at == 1392 {
        bend(events, origin, at, 1, 8960);
        bend(events, origin, at, 0, 10240);
    }
}

pub(super) fn tune(i: usize, frame: usize) -> f32 {
    // Released members retain the semitones captured at their physical Off;
    // manager pitch remains live. These are handwritten audible values.
    let manager = match frame {
        0..=8 => 0.5,
        9..=23 => 1.0625,
        24 => 1.065,
        25..=399 => 1.0625,
        400..=1391 => 0.,
        1392..=1450 => 1.0625,
        _ => 0.85,
    };
    let member = match i {
        0 => match frame {
            0..=7 => 4.5,
            8..=11 => 1.171875,
            12 => 7.5,
            13..=19 => 1.171875,
            20 => 1.1728125,
            21..=31 => 1.171875,
            _ => 0.,
        },
        1 => match frame {
            0..=7 => -3.,
            8..=11 => -0.78125,
            12 => -5.,
            13..=19 => -0.78125,
            20 => -0.781875,
            21..=74 => -0.78125,
            _ => -0.5,
        },
        2 | 3 => 0.,
        4 => -0.5,
        5 => 0.453125,
        6 if frame < 1450 => 0.6796875,
        6 => 0.1875,
        _ => unreachable!(),
    };
    member + manager
}

#[test]
fn raw_midi1_mpe_sensitivity_transactions_preserve_literal_complete_pcm_and_exact_ends_without_heap()
 {
    let fixed = performance_mpe_with_rpn(false, true);
    let split = performance_mpe_with_rpn(true, true);
    assert!(
        fixed
            .iter()
            .flatten()
            .zip(split.iter().flatten())
            .all(|(a, b)| (a - b).abs() < 4e-6)
    );
    // The RPN variant must exercise audible sensitivity, not just receiver state.
    let unchanged = performance_mpe(false);
    assert!(
        fixed[8..128]
            .iter()
            .flatten()
            .zip(unchanged[8..128].iter().flatten())
            .any(|(a, b)| (a - b).abs() > 0.01)
    );
}

#[test]
fn unsupported_parameter_roles_and_earlier_large_mpe_range_reject_before_source_or_root_publication()
 {
    for case in 0..5 {
        let mut actual = actual_dsp_manager(true);
        if case >= 3 {
            // ±50 baseline fits128 frames; an earlier ±82 does not. Returning
            // to ±4 before span end cannot erase the required source coverage.
            let bank = Bank::from_samples(
                vec![saved(true, false)],
                vec![Zone {
                    root: 60,
                    low_key: 60,
                    high_key: 60,
                    ..Default::default()
                }],
                vec![(
                    Default::default(),
                    Sample {
                        rate: 48000,
                        frames: (0..4096).map(|age| signal(60, age)).collect(),
                    },
                )],
            )
            .unwrap();
            actual.rack.parts[0].instrument_mut().unwrap().set_bank(Some(Box::new(bank)));
        }
        let mut events = EventList::with_capacity(16);
        mpe_note(&mut events, 0, 0, 0, ExactNoteKind::On);
        let (channel, index) = match case {
            0 => (0, 6),
            1 => (1, 1),
            2 => (15, 0),
            _ => (1, 0),
        };
        for (cc, value) in [(101, 0), (100, index), (6, if case == 3 { 80 } else { 2 })] {
            push_cc(&mut events, 0, 8, channel, cc, value);
        }
        if case == 3 {
            push_cc(&mut events, 0, 9, 1, 6, 2);
        }
        let mut output = EventList::with_capacity(16);
        let (mut l, mut r) = ([0.; 128], [0.; 128]);
        let transport = TransportInfo::default();
        let p = params();
        assert_eq!(
            super::super::super::tests::allocations(|| {
                let mut channels = [l.as_mut_slice(), r.as_mut_slice()];
                let mut audio = AudioBuffer::from_slices_checked(&[], &mut channels, 128);
                let mut cx = ProcessContext::new(&transport, 48000., 128, &mut output)
                    .with_process_mode(moose::core::ProcessMode::Realtime);
                assert_eq!(
                    Sampler::process(&mut actual, &p, &mut audio, &events, &mut cx),
                    if case == 4 {
                        ProcessStatus::Normal
                    } else {
                        ProcessStatus::Error
                    }
                );
            }),
            0
        );
        if case == 4 {
            // Same finite source and start, with only the legal low range.
            // Its complete unbent PCM is an independent admission companion.
            assert_eq!(actual.rack.parts[0].instrument().unwrap().shared_source_reason(), None);
            assert_eq!(actual.rack.parts[0].active_voices(), 1);
            for frame in 0..128 {
                let expected = signal(60, frame);
                assert!(
                    (l[frame] - expected[0]).abs() < 4e-6 && (r[frame] - expected[1]).abs() < 4e-6
                );
            }
            assert_eq!(output.lossless_iter().count(), 0);
            continue;
        }
        assert_eq!(
            actual.rack.parts[0].instrument().unwrap().shared_source_reason(),
            Some(if case == 3 {
                "shared AHDSR source EOF/readiness is not sealed for the complete range"
            } else {
                "shared AHDSR only unchanged-role MPE RPN0 sensitivity is admitted"
            })
        );
        assert_eq!(actual.rack.parts[0].active_voices(), 0);
        assert!(actual.rack.parts[0].host_note_at(0).is_none());
        assert_eq!(output.lossless_iter().count(), 0);
        assert!(l.iter().chain(&r).all(|x| *x == 0.));
    }
}
