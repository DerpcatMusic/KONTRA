use super::*;
use crate::{
    audio::Sample,
    engine::HostNote,
    import::{Group, Zone},
    test_support::allocations,
};

fn owner(port: u8, id: i32) -> HostNote {
    HostNote {
        port,
        channel: 4,
        key: 60,
        id,
        clap: true,
    }
}

fn address(note: HostNote) -> ExactNoteAddress {
    ExactNoteAddress::from_raw_signed(
        i16::from(note.port),
        i16::from(note.channel),
        i16::from(note.key),
        note.id,
    )
}

fn onset(note: HostNote, offset: u32) -> EventList {
    let mut events = EventList::with_capacity(1);
    events
        .try_push_exact(ExactEvent::new(
            offset,
            ExactEventBody::Note {
                kind: ExactNoteKind::On,
                address: address(note),
                velocity: 0.8,
            },
        ))
        .unwrap();
    events
}

fn install_tone(dsp: &mut Dsp) {
    dsp.rack.parts[0]
        .instrument_mut()
        .unwrap()
        .set_bank(Some(Box::new(
            Bank::from_samples(
                vec![Group::default()],
                vec![Zone::default()],
                vec![(
                    PathBuf::new(),
                    Sample {
                        rate: 48000,
                        frames: vec![[0.25; 2]; 4096],
                    },
                )],
            )
            .unwrap(),
        )));
}

fn process(
    dsp: &mut Dsp,
    params: &SamplerParams,
    events: &EventList,
    output: &mut EventList,
) -> (ProcessStatus, [f32; 64]) {
    let (status, (left, _)) = process_stereo(dsp, params, events, output);
    (status, left)
}

fn process_stereo(
    dsp: &mut Dsp,
    params: &SamplerParams,
    events: &EventList,
    output: &mut EventList,
) -> (ProcessStatus, ([f32; 64], [f32; 64])) {
    let (mut left, mut right) = ([1.; 64], [1.; 64]);
    let mut channels = [&mut left[..], &mut right[..]];
    let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, 64);
    let transport = TransportInfo::default();
    let mut cx = ProcessContext::new(&transport, 48000., 64, output);
    let mut status = ProcessStatus::Normal;
    assert_eq!(
        allocations(|| {
            status = Sampler::process(dsp, params, &mut buffer, events, &mut cx);
        }),
        0
    );
    (status, (left, right))
}

fn ends(output: &EventList) -> Vec<(ExactNoteAddress, u32)> {
    output
        .lossless_iter()
        .filter_map(|event| match event {
            LosslessEventRef::Exact(event) => match *event.body() {
                ExactEventBody::Note {
                    kind: ExactNoteKind::End,
                    address,
                    ..
                } => Some((address, event.sample_offset())),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[test]
fn rejected_terminal_only_note_ends_keep_exact_routes_and_frames_without_heap() {
    // Empty slot, unmatched input port, and filtered original keyswitch each
    // terminate without producing an engine owner.
    for case in 0..3 {
        let params = SamplerParams::new();
        let mut dsp = Dsp {
            until_poll: usize::MAX,
            ..Dsp::default()
        };
        dsp.rack.parts[0].reset(48000.);
        dsp.align.clock = 1024;
        let mut note = owner(if case == 1 { 3 } else { 0 }, 101 + case);
        if case != 0 {
            install_tone(&mut dsp);
        }
        if case == 2 {
            note.key = 12;
            let setup = articulate::Articulate {
                source: "test.nki".into(),
                keep_original: false,
                articulations: vec![articulate::Articulation {
                    key: Some(12),
                    remap: Some(100),
                    ..Default::default()
                }],
                ..Default::default()
            };
            dsp.routers[0].set_route(Route::new("test.nki", &setup, &articulate::Mpe::default()));
        }
        let input = onset(note, 23);
        let none = EventList::with_capacity(0);
        let mut rejected = EventList::with_capacity(0);
        assert!(
            process(&mut dsp, &params, &input, &mut rejected)
                .1
                .iter()
                .all(|x| *x == 0.)
        );
        assert_eq!(dsp.pending_host_note_ends, [(note, 1047)]);
        assert_eq!(dsp.host_note_end_rejections, 1);
        assert!(
            dsp.rack
                .parts
                .iter()
                .all(|part| !part.host_note_present(note))
        );
        process(&mut dsp, &params, &none, &mut rejected);
        assert_eq!(
            dsp.pending_host_note_ends,
            [(note, 1047)],
            "retry re-dated the original terminal frame"
        );
        assert_eq!(dsp.host_note_end_rejections, 2);
        let mut output = EventList::with_capacity(1);
        process(&mut dsp, &params, &none, &mut output);
        assert_eq!(ends(&output), [(address(note), 0)]);
        assert!(dsp.pending_host_note_ends.is_empty());
        output.clear();
        process(&mut dsp, &params, &none, &mut output);
        assert!(ends(&output).is_empty());
    }
}

#[test]
fn terminal_only_completion_preserves_offset_and_defers_exclusive_boundary() {
    for offset in [23, 64] {
        let params = SamplerParams::new();
        let mut dsp = Dsp {
            until_poll: usize::MAX,
            ..Dsp::default()
        };
        let note = owner(3, 110);
        let mut output = EventList::with_capacity(1);
        process(&mut dsp, &params, &onset(note, offset), &mut output);
        if offset == 64 {
            assert!(ends(&output).is_empty());
            assert_eq!(dsp.pending_host_note_ends, [(note, 64)]);
            process(&mut dsp, &params, &EventList::with_capacity(0), &mut output);
        }
        assert_eq!(
            ends(&output),
            [(address(note), if offset == 64 { 0 } else { offset })]
        );
    }
}

#[test]
fn rejected_terminal_identity_fences_duplicate_onsets_until_ack_then_allows_new_generation() {
    let params = SamplerParams::new();
    params.attack.set_value(0.0001);
    let mut dsp = Dsp {
        until_poll: usize::MAX,
        ..Dsp::default()
    };
    dsp.rack.parts[0].reset(48000.);
    let note = owner(0, 120);
    let input = onset(note, 7);
    let mut rejected = EventList::with_capacity(0);
    process(&mut dsp, &params, &input, &mut rejected);
    install_tone(&mut dsp);
    assert!(
        process(&mut dsp, &params, &input, &mut rejected)
            .1
            .iter()
            .all(|x| *x == 0.)
    );
    assert_eq!(dsp.pending_host_note_ends, [(note, 7)]);
    assert!(!dsp.rack.parts[0].host_note_present(note));
    let mut output = EventList::with_capacity(2);
    assert!(
        process(&mut dsp, &params, &input, &mut output)
            .1
            .iter()
            .all(|x| *x == 0.)
    );
    assert_eq!(ends(&output), [(address(note), 0)]);
    output.clear();
    assert!(
        process(&mut dsp, &params, &input, &mut output)
            .1
            .iter()
            .any(|x| *x != 0.)
    );
    assert!(dsp.rack.parts[0].host_note_present(note));
    assert!(ends(&output).is_empty());
}

#[test]
fn anonymous_terminal_onsets_remain_distinct_through_partial_backpressure() {
    let params = SamplerParams::new();
    let mut dsp = Dsp {
        until_poll: usize::MAX,
        ..Dsp::default()
    };
    let note = owner(3, -1);
    let mut input = EventList::with_capacity(2);
    for offset in [7, 11] {
        input
            .try_push_exact(ExactEvent::new(
                offset,
                ExactEventBody::Note {
                    kind: ExactNoteKind::On,
                    address: address(note),
                    velocity: 0.8,
                },
            ))
            .unwrap();
    }
    let mut output = EventList::with_capacity(1);
    process(&mut dsp, &params, &input, &mut output);
    assert_eq!(ends(&output), [(address(note), 7)]);
    assert_eq!(dsp.pending_host_note_ends, [(note, 11)]);
    assert_eq!(dsp.host_note_end_rejections, 1);
    output.clear();
    let none = EventList::with_capacity(0);
    process(&mut dsp, &params, &none, &mut output);
    assert_eq!(ends(&output), [(address(note), 0)]);
    assert!(dsp.pending_host_note_ends.is_empty());
    output.clear();
    process(&mut dsp, &params, &none, &mut output);
    assert!(ends(&output).is_empty());
}

#[test]
fn terminal_capacity_failure_keeps_retained_identities_and_reset_retries_without_heap() {
    let params = SamplerParams::new();
    let mut dsp = Dsp {
        until_poll: usize::MAX,
        ..Dsp::default()
    };
    dsp.align.clock = 4096;
    let capacity = dsp.pending_host_note_ends.capacity();
    for id in 0..capacity {
        dsp.pending_host_note_ends.push((owner(3, id as i32), 4090));
    }
    let retained = dsp.pending_host_note_ends.clone();
    let mut rejected = EventList::with_capacity(0);
    let (status, _) = process(&mut dsp, &params, &onset(owner(3, 0), 0), &mut rejected);
    assert!(matches!(status, ProcessStatus::Normal));
    assert_eq!(
        dsp.pending_host_note_ends, retained,
        "duplicate onset consumed terminal capacity"
    );
    let mut output = EventList::with_capacity(capacity);
    output.push(Event::new(
        0,
        EventBody::NoteOn {
            group: 0,
            channel: 0,
            note: 60,
            velocity: 100,
        },
    ));
    let (status, pcm) = process(
        &mut dsp,
        &params,
        &onset(owner(3, capacity as i32), 0),
        &mut output,
    );
    assert!(matches!(status, ProcessStatus::Error));
    assert!(pcm.iter().all(|x| *x == 0.));
    assert!(output.lossless_iter().next().is_none());
    assert_eq!(dsp.pending_host_note_ends, retained);
    assert_eq!(
        allocations(|| Sampler::reset(&mut dsp, &params, &AudioConfig::new(48000., 64))),
        0
    );
    assert_eq!(dsp.pending_host_note_ends.len(), capacity);
    assert!(dsp.pending_host_note_ends.iter().all(|(_, end)| *end == 0));
    process(&mut dsp, &params, &EventList::with_capacity(0), &mut output);
    assert_eq!(ends(&output).len(), capacity);
    assert!(dsp.pending_host_note_ends.is_empty());
    output.clear();
    process(&mut dsp, &params, &EventList::with_capacity(0), &mut output);
    assert!(ends(&output).is_empty());
}

#[test]
fn retained_terminal_duplicates_do_not_reserve_shared_source_roots_or_producers() {
    let params = SamplerParams::new();
    params.attack.set_value(0.0001);
    let (mut actual, mut reference) = (Dsp::default(), Dsp::default());
    for dsp in [&mut actual, &mut reference] {
        dsp.until_poll = usize::MAX;
        dsp.rack.reset(48000.);
    }
    let a = owner(0, 201);
    let b = HostNote {
        id: 202,
        key: 64,
        ..a
    };
    let none = EventList::with_capacity(0);
    let mut rejected = EventList::with_capacity(0);
    process(&mut actual, &params, &onset(a, 7), &mut rejected);
    process(&mut reference, &params, &none, &mut rejected);
    assert_eq!(actual.pending_host_note_ends, [(a, 7)]);
    let bank = |shared: bool| {
        Bank::from_samples(
            vec![Group {
                source_mode: Some(0),
                native_volume_env: !shared,
                native_shared_volume_slot: shared.then_some(0),
                volume_env: Some(crate::import::Ahdsr {
                    attack_curve: 1.,
                    attack_ms: 0.,
                    hold_ms: 0.,
                    decay_ms: 0.,
                    sustain: 1.,
                    release_ms: 2.,
                    unknown_flag: 0,
                    unknown_tail: Vec::new(),
                }),
                modulators: vec![crate::modulation::Modulator {
                    name: "Primary".into(),
                    targets: vec!["volume".into()],
                    assignments: None,
                    volume_env: true,
                    bypassed: false,
                    flex: false,
                    primary_flex: false,
                    envelope: None,
                    kind: "ahdsr".into(),
                }],
                ..Default::default()
            }],
            vec![Zone {
                root: 60,
                low_key: 60,
                high_key: 64,
                ..Default::default()
            }],
            vec![(
                PathBuf::new(),
                Sample {
                    rate: 48000,
                    frames: (0..192)
                        .map(|age| {
                            let value = (age as f32 * 0.17).sin() * 0.25;
                            [value; 2]
                        })
                        .collect(),
                },
            )],
        )
        .unwrap()
    };
    actual.rack.parts[0]
        .instrument_mut()
        .unwrap()
        .set_bank(Some(Box::new(bank(true))));
    reference.rack.parts[0]
        .instrument_mut()
        .unwrap()
        .set_bank(Some(Box::new(bank(false))));
    assert!(
        actual.rack.parts[0]
            .instrument()
            .unwrap()
            .shared_source_required()
    );
    let mut input = EventList::with_capacity(4);
    for (note, offset) in [(a, 0), (a, 1), (b, 17)] {
        input
            .try_push_exact(ExactEvent::new(
                offset,
                ExactEventBody::Note {
                    kind: ExactNoteKind::On,
                    address: address(note),
                    velocity: 0.8,
                },
            ))
            .unwrap();
    }
    input
        .try_push_exact(ExactEvent::new(
            48,
            ExactEventBody::Note {
                kind: ExactNoteKind::Off,
                address: address(b),
                velocity: 0.5,
            },
        ))
        .unwrap();
    // Ordinary Native::release waits for the next consumer point. The saved
    // shared source instead resolves the authored Off48 countdown before
    // source point floor((48-17)/32)=0. Transcribe its f32 recurrence and the
    // clone's 32-frame interpolation, without reading any production cache.
    let release_points = (2f32 * 0.001 * (48000. / 32.)) as usize;
    assert_eq!((48usize - 17) / 32, 0);
    let r = (43f64 / 3.).powf(-1. / release_points as f64) as f32;
    let mut points = [0.; 6 * 64 / 32];
    let mut state = 1.075f32;
    for point in &mut points[..release_points] {
        *point = state - 0.075;
        state *= r;
    }
    // The first range publishes ceil((64-17)/32)=2 source points. Subsequent
    // ranges publish two each. Expiry of the three-point Release latches Done
    // in the second range; the waveform retires after consuming that range,
    // even though interpolation has not yet reached the zero endpoint.
    let first_range_points = (64usize - 17).div_ceil(32);
    let terminal_range = release_points
        .saturating_sub(first_range_points)
        .div_ceil(64 / 32);
    let terminal_frame = (terminal_range + 1) * 64;
    assert_eq!(terminal_frame, 128);
    let reference_input = onset(b, 17);
    let (mut output, mut expected_output) =
        (EventList::with_capacity(8), EventList::with_capacity(8));
    let mut returned = Vec::new();
    let mut sounded = false;
    let mut sounded_at_retirement = false;
    for block in 0usize..6 {
        let (status, pcm) = process_stereo(
            &mut actual,
            &params,
            if block == 0 { &input } else { &none },
            &mut output,
        );
        let (expected_status, mut expected) = process_stereo(
            &mut reference,
            &params,
            if block == 0 { &reference_input } else { &none },
            &mut expected_output,
        );
        assert_eq!(status, ProcessStatus::Normal);
        assert_eq!(expected_status, ProcessStatus::Normal);
        for sample in 0..64 {
            let frame = block * 64 + sample;
            let amplitude = if frame < 17 || frame >= terminal_frame {
                0.
            } else {
                let age = frame - 17;
                let point = age / 32;
                let previous = points[point.saturating_sub(1)];
                previous + (points[point] - previous) * ((age & 31) as f32 * (1. / 32.))
            };
            expected.0[sample] *= amplitude;
            expected.1[sample] *= amplitude;
            if frame + 1 == terminal_frame {
                sounded_at_retirement = pcm.0[sample] != 0. && expected.0[sample] != 0.;
            }
        }
        for (actual, expected) in pcm
            .0
            .iter()
            .chain(&pcm.1)
            .zip(expected.0.iter().chain(&expected.1))
        {
            // The independent oracle applies amplitude after dry PCM mixing;
            // allow only the rounding from that f32 product reassociation.
            let rounding = 4. * f32::EPSILON * actual.abs().max(expected.abs()).max(1e-6);
            assert!(
                (actual - expected).abs() <= rounding,
                "shared source PCM differs in block {block}: {actual} vs {expected}"
            );
        }
        sounded |= pcm.0.iter().chain(&pcm.1).any(|x| *x != 0.);
        returned.extend(
            ends(&output)
                .into_iter()
                .map(|(root, offset)| (root, block as u32 * 64 + offset)),
        );
        assert!(
            ends(&expected_output).is_empty(),
            "dry reference remains physically held"
        );
        output.clear();
        expected_output.clear();
    }
    assert!(sounded);
    assert!(
        sounded_at_retirement,
        "expiry cuts an audible interpolation endpoint"
    );
    assert_eq!(
        returned,
        [(address(a), 0), (address(b), terminal_frame as u32)]
    );
    assert!(actual.pending_host_note_ends.is_empty());
    assert!(!actual.rack.parts[0].host_note_present(a));
    assert!(!actual.rack.parts[0].host_note_present(b));
}

#[test]
fn deferred_release_without_a_zone_ends_at_exact_pedal_up_frame_without_heap() {
    for release_pedal in [false, true] {
        let params = SamplerParams::new();
        params.attack.set_value(0.0001);
        let mut dsp = Dsp {
            until_poll: usize::MAX,
            ..Dsp::default()
        };
        dsp.rack.parts[0].reset(48000.);
        dsp.rack.parts[0]
            .instrument_mut()
            .unwrap()
            .set_bank(Some(Box::new(
                Bank::from_samples(
                    vec![Group::default()],
                    vec![Zone::default()],
                    vec![(
                        PathBuf::new(),
                        Sample {
                            rate: 48000,
                            frames: vec![[0.25; 2]; 520],
                        },
                    )],
                )
                .unwrap(),
            )));
        let note = owner(0, 301);
        let mut input = EventList::with_capacity(2);
        input.push(Event::on_port(
            0,
            note.port,
            EventBody::ControlChange {
                group: 0,
                channel: note.channel,
                cc: 64,
                value: 127,
            },
        ));
        input
            .try_push_exact(ExactEvent::new(
                0,
                ExactEventBody::Note {
                    kind: ExactNoteKind::On,
                    address: address(note),
                    velocity: 0.8,
                },
            ))
            .unwrap();
        let mut output = EventList::with_capacity(8);
        let render = |dsp: &mut Dsp, events: &EventList, output: &mut EventList, frames| {
            let (mut left, mut right) = ([0.; 576], [0.; 576]);
            let mut channels = [&mut left[..frames], &mut right[..frames]];
            let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, frames);
            let transport = TransportInfo::default();
            let mut cx = ProcessContext::new(&transport, 48000., frames, output);
            let mut status = ProcessStatus::Normal;
            assert_eq!(
                allocations(|| {
                    status = Sampler::process(dsp, &params, &mut buffer, events, &mut cx);
                }),
                0
            );
            assert_eq!(status, ProcessStatus::Normal);
            left
        };
        let pcm = render(&mut dsp, &input, &mut output, 576);
        assert!(pcm[..520].iter().any(|x| x.abs() > 0.001));
        assert!(pcm[520..].iter().all(|x| *x == 0.));
        assert_eq!(dsp.rack.parts[0].instrument().unwrap().active_voices(), 0);
        assert_eq!(
            dsp.rack.parts[0]
                .instrument()
                .unwrap()
                .host_audio_end_at(note),
            Some(520)
        );
        assert!(ends(&output).is_empty(), "physical key still owns the root");
        assert!(dsp.rack.parts[0].host_note_pending(note));
        input.clear();
        input
            .try_push_exact(ExactEvent::new(
                600 - 576,
                ExactEventBody::Note {
                    kind: ExactNoteKind::Off,
                    address: address(note),
                    velocity: 0.5,
                },
            ))
            .unwrap();
        if release_pedal {
            input.push(Event::on_port(
                713 - 576,
                note.port,
                EventBody::ControlChange {
                    group: 0,
                    channel: note.channel,
                    cc: 64,
                    value: 0,
                },
            ));
        }
        assert!(
            render(&mut dsp, &input, &mut output, 256)
                .iter()
                .all(|x| *x == 0.)
        );
        let engine = dsp.rack.parts[0].instrument().unwrap();
        assert_eq!(engine.pending_work()[2], usize::from(!release_pedal));
        if release_pedal {
            assert_eq!(ends(&output), [(address(note), 713 - 576)]);
            assert!(!dsp.rack.parts[0].host_note_present(note));
        } else {
            assert!(
                ends(&output).is_empty(),
                "deferred work still owns the root"
            );
            assert!(dsp.rack.parts[0].host_note_pending(note));
        }
        output.clear();
        input.clear();
        render(&mut dsp, &input, &mut output, 64);
        assert!(
            ends(&output).is_empty(),
            "terminal identity was emitted twice"
        );
    }
}
