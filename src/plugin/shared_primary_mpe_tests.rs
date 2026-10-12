//! Static routed MPE with exterior literal envelope gains and pitch controls.
use super::*;
use crate::articulate::{Articulate, In, Mpe, Route, Zone as MpeZone};
use crate::engine::{Engine, HostExpression, HostNote, HostPattern};
#[path = "shared_primary_mpe_rpn_tests.rs"]
mod shared_primary_mpe_rpn_tests;
const MEMBER: [u8; 7] = [1, 2, 1, 1, 2, 2, 1];
fn mpe_address(i: usize) -> ExactNoteAddress {
    ExactNoteAddress::from_raw_signed(0, i16::from(MEMBER[i]), i16::from(KEY[i]), 100 + i as i32)
}
fn mpe_note(events: &mut EventList, origin: usize, at: usize, i: usize, kind: ExactNoteKind) {
    events
        .try_push_exact(ExactEvent::new(
            (at - origin) as u32,
            ExactEventBody::Note {
                kind,
                address: mpe_address(i),
                velocity: 1.,
            },
        ))
        .unwrap();
}
fn pressure(events: &mut EventList, origin: usize, at: usize, ch: u8, value: u8) {
    events.push(Event::on_port(
        (at - origin) as u32,
        0,
        EventBody::ChannelPressure {
            group: 0,
            channel: ch,
            pressure: value,
        },
    ));
}
fn bend(events: &mut EventList, origin: usize, at: usize, ch: u8, value: u16) {
    events.push(Event::on_port(
        (at - origin) as u32,
        0,
        EventBody::PitchBend {
            group: 0,
            channel: ch,
            value,
        },
    ));
}
fn source_bank(shared: bool, key: Option<u8>) -> Bank {
    let keys = key.map_or(vec![60, 62], |key| vec![key]);
    Bank::from_samples(
        keys.iter().map(|_| saved(shared, false)).collect(),
        keys.iter()
            .enumerate()
            .map(|(group, &key)| Zone {
                group,
                sample: format!("mpe-{key}").into(),
                low_key: key,
                high_key: key,
                root: key,
                ..Default::default()
            })
            .collect(),
        keys.iter()
            .map(|&key| {
                (
                    format!("mpe-{key}").into(),
                    Sample {
                        rate: 48000,
                        frames: (0..131072).map(|age| signal(key, age)).collect(),
                    },
                )
            })
            .collect(),
    )
    .unwrap()
}
fn actual_dsp() -> Dsp {
    actual_dsp_manager(false)
}
fn actual_dsp_manager(midi1: bool) -> Dsp {
    let mut dsp = Dsp::default();
    dsp.until_poll = usize::MAX;
    dsp.rack.reset(48000.);
    dsp.rack.parts[0].instrument_mut().unwrap().set_bank(Some(Box::new(source_bank(true, None))));
    dsp.routers[0].set_route(Route::new(
        "",
        &Articulate::default(),
        &Mpe {
            zone: MpeZone::Lower,
            members: 2,
            bend_range: 48,
            ..Default::default()
        },
    ));
    // Commit the real Router's initial topology before establishing the native
    // manager sensitivity. No future protocol state is written during rendering.
    crate::articulate::feed_on_port(
        &mut dsp.routers[0],
        dsp.rack.parts[0].instrument_mut().unwrap(),
        In::Cc(15, 1, 0),
        0,
        0,
    );
    if midi1 {
        for (cc, value) in [(101, 0), (100, 0), (6, 2)] {
            crate::articulate::feed_on_port(
                &mut dsp.routers[0],
                dsp.rack.parts[0].instrument_mut().unwrap(),
                In::Midi1Cc(0, cc, value),
                0,
                0,
            );
        }
    } else {
        assert!(dsp.rack.parts[0].instrument_mut().unwrap().native_parameter_from(
            0,
            0,
            crate::midi::NativeParameter {
                registered: true,
                address: 0,
                data: crate::midi::NativeParameterData::Absolute(2 << 25)
            }
        ));
    }
    dsp
}
fn dry(key: u8) -> Engine {
    let mut e = Engine::default();
    e.reset(48000.);
    e.set_bank(Some(Box::new(source_bank(false, Some(key)))));
    e
}
fn plain_note(i: usize) -> HostNote {
    HostNote {
        port: 0,
        channel: 0,
        key: KEY[i],
        id: 100 + i as i32,
        clap: true,
    }
}
fn plain_pattern(i: usize) -> HostPattern {
    HostPattern {
        port: 0,
        channel: 0,
        key: i32::from(KEY[i]),
        id: 100 + i as i32,
        clap: true,
    }
}
fn pitch(i: usize, frame: usize) -> f32 {
    // Manager quarter bend over the explicitly configured two semitones = .5.
    // Member +768/8192 over48 =4.5; member -512/8192 over48 =-3.
    match i {
        0 => {
            if frame < 32 {
                5.
            } else {
                0.5
            }
        }
        1 => -2.5,
        2 | 3 | 6 => 0.,
        4 => -3.,
        5 => 3.,
        _ => unreachable!(),
    }
}
fn pressure_level(i: usize) -> f32 {
    // Independently stated native unhandled-pressure floor at MIDI byte0.
    if matches!(i, 0 | 2 | 3 | 5) {
        0.25118864
    } else {
        1.
    }
}
fn performance_mpe(split: bool) -> Vec<[f32; 2]> {
    performance_mpe_with_rpn(split, false)
}
fn performance_mpe_with_rpn(split: bool, rpn: bool) -> Vec<[f32; 2]> {
    let mut actual = actual_dsp_manager(rpn);
    let params = params();
    let mut dry: [Engine; 7] = std::array::from_fn(|i| dry(KEY[i]));
    let gains: [Vec<f32>; 7] = std::array::from_fn(levels);
    let mut pcm = vec![[0.; 2]; FINISH];
    let mut ended = Vec::with_capacity(7);
    let transport = TransportInfo::default();
    for pair in HOST.windows(2) {
        let (origin, end) = (pair[0], pair[1]);
        let n = end - origin;
        let mut events = EventList::with_capacity(2048);
        for at in origin..end {
            // Equal-time controls precede exact On in original ingress order.
            if at == 0 {
                pressure(&mut events, origin, at, 1, 0);
                pressure(&mut events, origin, at, 2, 127);
                pressure(&mut events, origin, at, 0, 80);
                bend(&mut events, origin, at, 0, 10240);
                bend(&mut events, origin, at, 1, 8960);
                bend(&mut events, origin, at, 2, 7680);
                push_cc(&mut events, origin, at, 1, 74, 37);
                push_cc(&mut events, origin, at, 2, 74, 61);
            }
            if rpn {
                shared_primary_mpe_rpn_tests::controls(&mut events, origin, at);
            }
            if at == 32 {
                bend(&mut events, origin, at, 1, 8192);
            }
            if at == 64 {
                push_cc(&mut events, origin, at, 1, 74, 107);
            }
            if at == 400 {
                bend(&mut events, origin, at, 0, 8192);
            }
            if at == 864 {
                pressure(&mut events, origin, at, 2, 0);
                bend(&mut events, origin, at, 2, 8704);
                push_cc(&mut events, origin, at, 2, 74, 110);
            }
            for i in 0..7 {
                if at == ON[i] {
                    mpe_note(&mut events, origin, at, i, ExactNoteKind::On);
                }
            }
            for (frame, i) in [
                (48, 0),
                (112, 1),
                (560, 2),
                (816, 3),
                (848, 4),
                (1184, 5),
                (2240, 6),
            ] {
                if at == frame {
                    mpe_note(&mut events, origin, at, i, ExactNoteKind::Off);
                }
            }
            for (frame, ch, cc, value) in [
                (784, 0, 66, 127),
                (912, 0, 66, 0),
                (1168, 0, 64, 127),
                (1216, 1, 64, 0),
                (1248, 0, 64, 0),
                (1376, 0, 121, 0),
            ] {
                if at == frame {
                    push_cc(&mut events, origin, at, ch, cc, value);
                }
            }
            // An old exact ID must not release the new same-member/key owner.
            if at == 1160 {
                mpe_note(&mut events, origin, at, 4, ExactNoteKind::Off);
            }
            if split && [1, 8, 39, 166].contains(&((at - origin) % 173)) {
                push_cc(&mut events, origin, at, 15, 1, 0);
            }
        }
        let mut output = EventList::with_capacity(32);
        let (mut l, mut r) = (vec![0.; n], vec![0.; n]);
        assert_eq!(
            super::super::tests::allocations(|| {
                let mut channels = [l.as_mut_slice(), r.as_mut_slice()];
                let mut audio = AudioBuffer::from_slices_checked(&[], &mut channels, n);
                let mut cx = ProcessContext::new(&transport, 48000., n, &mut output)
                    .with_process_mode(moose::core::ProcessMode::Realtime);
                assert_eq!(
                    Sampler::process(&mut actual, &params, &mut audio, &events, &mut cx),
                    ProcessStatus::Normal
                );
            }),
            0
        );
        assert_eq!(actual.rack.parts[0].instrument().unwrap().shared_source_reason(), None);
        for event in output.lossless_iter() {
            if let LosslessEventRef::Exact(e) = event {
                if let ExactEventBody::Note {
                    kind: ExactNoteKind::End,
                    address,
                    ..
                } = *e.body()
                {
                    ended.push((address, origin + e.sample_offset() as usize));
                }
            }
        }
        let mut expected = vec![[0.; 2]; n];
        assert_eq!(
            super::super::tests::allocations(|| {
                for frame in origin..end {
                    for i in 0..7 {
                        if frame == ON[i] {
                            assert!(dry[i].host_note_on(plain_note(i), 0, KEY[i], 127, 0.));
                        }
                        if frame < ON[i] {
                            continue;
                        }
                        dry[i].host_expression(
                            plain_pattern(i),
                            HostExpression::Tune(if rpn {
                                shared_primary_mpe_rpn_tests::tune(i, frame)
                            } else {
                                pitch(i, frame)
                            }),
                        );
                        let (mut x, mut y) = ([0.; 1], [0.; 1]);
                        dry[i].render(&mut x, &mut y);
                        // The literal primary source is an audio-clock scalar,
                        // independent of both source pitch and event-gain smoothing.
                        let gain = gains[i][frame] * pressure_level(i);
                        expected[frame - origin][0] += x[0] * gain;
                        expected[frame - origin][1] += y[0] * gain;
                    }
                }
            }),
            0
        );
        for i in 0..n {
            pcm[origin + i] = [l[i], r[i]];
            assert!(
                (l[i] - expected[i][0]).abs() < 4e-6 && (r[i] - expected[i][1]).abs() < 4e-6,
                "static MPE exterior PCM frame{}: {:?} vs {:?}",
                origin + i,
                pcm[origin + i],
                expected[i]
            );
        }
    }
    assert_eq!(ended.len(), 7);
    for i in 0..7 {
        assert_eq!(
            ended.iter().find(|x| x.0 == mpe_address(i)).map(|x| x.1),
            Some(END[i])
        );
    }
    assert!(pcm[32..128].iter().flatten().any(|x| x.abs() > 0.01));
    assert!(pcm[2433..].iter().flatten().all(|x| *x == 0.));
    pcm
}
#[test]
fn static_routed_shared_primary_mpe_preserves_live_controls_pedals_and_exact_ids_in_complete_pcm_without_heap()
 {
    let a = performance_mpe(false);
    let b = performance_mpe(true);
    assert!(
        a.iter()
            .zip(b)
            .all(|(a, b)| (a[0] - b[0]).abs() < 4e-6 && (a[1] - b[1]).abs() < 4e-6)
    );
}

fn midi_note(events: &mut EventList, at: u32, key: u8, on: bool) {
    events.push(Event::on_port(
        at,
        0,
        if on {
            EventBody::NoteOn {
                group: 0,
                channel: 1,
                note: key,
                velocity: 100,
            }
        } else {
            EventBody::NoteOff {
                group: 0,
                channel: 1,
                note: key,
                velocity: 64,
            }
        },
    ));
}
#[test]
fn first_static_mpe_setup_clears_old_pedal_and_obeys_original_velocity_prefix_order_in_plugin_pcm()
{
    // Zero: HostOn resets old state, then a later ordinary MIDI1 On is plain.
    // One: first MIDI1 On consumes the OLD prefix before role reset.
    // Two: first CC88 resets roles then arms the NEW prefix for the following On.
    let level = levels(2);
    for mode in 0..3 {
        let mut actual = Dsp::default();
        actual.until_poll = usize::MAX;
        actual.rack.reset(48000.);
        actual.rack.parts[0].instrument_mut().unwrap().set_bank(Some(Box::new(source_bank(true, None))));
        actual.rack.parts[0].instrument_mut().unwrap().cc(1, 64, 127);
        actual.rack.parts[0].instrument_mut().unwrap().receive_midi1_cc(1, 88, 1).unwrap();
        actual.routers[0].set_route(Route::new(
            "",
            &Articulate::default(),
            &Mpe {
                zone: MpeZone::Lower,
                members: 2,
                bend_range: 48,
                ..Default::default()
            },
        ));
        let params = params();
        let mut reference = [dry(60), dry(62)];
        let mut ended = Vec::with_capacity(1);
        let transport = TransportInfo::default();
        for pair in [0, 32, 64, 128, 160, 192, 256, 384].windows(2) {
            let (origin, end) = (pair[0], pair[1]);
            let n = end - origin;
            let mut events = EventList::with_capacity(8);
            if origin == 0 {
                if mode == 0 {
                    events
                        .try_push_exact(ExactEvent::new(
                            0,
                            ExactEventBody::Note {
                                kind: ExactNoteKind::On,
                                address: ExactNoteAddress::from_raw_signed(0, 1, 60, 300),
                                velocity: 1.,
                            },
                        ))
                        .unwrap();
                } else {
                    if mode == 2 {
                        push_cc(&mut events, 0, 0, 1, 88, 7);
                    }
                    midi_note(&mut events, 0, 60, true);
                }
            }
            if (origin..end).contains(&48) {
                if mode == 0 {
                    events
                        .try_push_exact(ExactEvent::new(
                            (48 - origin) as u32,
                            ExactEventBody::Note {
                                kind: ExactNoteKind::Off,
                                address: ExactNoteAddress::from_raw_signed(0, 1, 60, 300),
                                velocity: 1.,
                            },
                        ))
                        .unwrap();
                } else {
                    midi_note(&mut events, (48 - origin) as u32, 60, false);
                }
            }
            if mode == 0 && origin == 128 {
                midi_note(&mut events, 0, 62, true);
            }
            if mode == 0 && (origin..end).contains(&176) {
                midi_note(&mut events, (176 - origin) as u32, 62, false);
            }
            let (mut l, mut r) = (vec![0.; n], vec![0.; n]);
            let mut output = EventList::with_capacity(8);
            assert_eq!(
                super::super::tests::allocations(|| {
                    let mut channels = [l.as_mut_slice(), r.as_mut_slice()];
                    let mut audio = AudioBuffer::from_slices_checked(&[], &mut channels, n);
                    let mut cx = ProcessContext::new(&transport, 48000., n, &mut output)
                        .with_process_mode(moose::core::ProcessMode::Realtime);
                    assert_eq!(
                        Sampler::process(&mut actual, &params, &mut audio, &events, &mut cx),
                        ProcessStatus::Normal
                    );
                }),
                0
            );
            assert_eq!(actual.rack.parts[0].instrument().unwrap().shared_source_reason(), None);
            if origin == 0 || mode == 0 && origin == 128 {
                assert_eq!(
                    actual.rack.parts[0].active_voices(),
                    1,
                    "accepted source binding"
                );
            }
            for event in output.lossless_iter() {
                if let LosslessEventRef::Exact(e) = event {
                    if matches!(
                        e.body(),
                        ExactEventBody::Note {
                            kind: ExactNoteKind::End,
                            ..
                        }
                    ) {
                        ended.push(origin + e.sample_offset() as usize);
                    }
                }
            }
            let mut expected = vec![[0.; 2]; n];
            assert_eq!(
                super::super::tests::allocations(|| {
                    for frame in origin..end {
                        for i in 0..if mode == 0 { 2 } else { 1 } {
                            let onset = i * 128;
                            if frame < onset {
                                continue;
                            }
                            if frame == onset {
                                let velocity = if mode == 0 {
                                    if i == 0 { 1. } else { 100. / 127. }
                                } else {
                                    (12800. + if mode == 1 { 1. } else { 7. }) / 16383.
                                };
                                assert!(reference[i].host_note_on_velocity(
                                    HostNote {
                                        port: 0,
                                        channel: 0,
                                        key: if i == 0 { 60 } else { 62 },
                                        id: 400 + i as i32,
                                        clap: true
                                    },
                                    0,
                                    if i == 0 { 60 } else { 62 },
                                    crate::midi::Velocity::new(velocity).unwrap(),
                                    0.
                                ));
                            }
                            let (mut x, mut y) = ([0.; 1], [0.; 1]);
                            reference[i].render(&mut x, &mut y);
                            let gain = level.get(512 + frame - onset).copied().unwrap_or(0.);
                            expected[frame - origin][0] += x[0] * gain;
                            expected[frame - origin][1] += y[0] * gain;
                        }
                    }
                }),
                0
            );
            for i in 0..n {
                assert!(
                    (l[i] - expected[i][0]).abs() < 4e-6 && (r[i] - expected[i][1]).abs() < 4e-6,
                    "startup mode{mode} frame{}",
                    origin + i
                );
            }
        }
        assert_eq!(
            actual.rack.parts[0].active_voices(),
            0,
            "old sustain must not hold the new MPE source"
        );
        assert_eq!(ended, if mode == 0 { vec![128] } else { Vec::new() });
    }
}
