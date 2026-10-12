use super::*;

fn programs(events: &mut EventList) {
    // MIDI1 typed, original two-byte MIDI, and MT2 UMP: semantic companions
    // carry one request, while bare packets retain the same unsupported result.
    let body = EventBody::ProgramChange { group: 0, channel: 4, program: 127 };
    events.push(Event::on_port(0, 0, body));
    for raw in [true, false] {
        for companion in [true, false] {
            let original = if raw {
                ExactEventBody::Midi1 { port: 0,
                    message: moose::core::events::RawMidi1::new([0xc4, 127, 0], 2).unwrap() }
            } else {
                ExactEventBody::Ump { port: 0,
                    packet: moose::core::events::RawUmp::new([0x20c4_7f00, 0, 0, 0], 1).unwrap() }
            };
            let token = events.try_push_exact_token(ExactEvent::new(0, original)).unwrap();
            if companion {
                events.try_push_exact_companion(token, Event::on_port(0, 0, body)).unwrap();
            }
        }
    }
    for mode in 0..3 {
        for bank in [None, Some((37, 91))] {
            let body = EventBody::ProgramChange2 { group: 0, channel: 4, program: 127, bank };
            if mode == 0 { events.push(Event::on_port(0, 0, body)); continue; }
            // Independent wire layout: statusC, channel4, optional bank flag;
            // no CC0/32 companions or fabricated bank/cache updates.
            let (msb, lsb) = bank.unwrap_or((0, 0));
            let words = [0x40c4_0000 | u32::from(bank.is_some()),
                (127 << 24) | (u32::from(msb) << 8) | u32::from(lsb), 0, 0];
            let token = events.try_push_exact_token(ExactEvent::new(0,
                ExactEventBody::Ump { port: 0,
                    packet: moose::core::events::RawUmp::new(words, 2).unwrap() })).unwrap();
            if mode == 1 {
                events.try_push_exact_companion(token, Event::on_port(0, 0, body)).unwrap();
            }
        }
    }
}

fn performance(scripted: bool, partition: &'static [usize]) -> Performance {
    let (mut actual, mut reference) = (Performance::new(scripted, partition), Performance::new(scripted, partition));
    actual.params.shared.midi_thru.store(true, Ordering::Relaxed);
    reference.params.shared.midi_thru.store(true, Ordering::Relaxed);
    let (mut input, mut expected, empty, mut output) = (EventList::with_capacity(24),
        EventList::with_capacity(16), EventList::with_capacity(0), EventList::with_capacity(32));
    let transport = TransportInfo::default();
    let mut relayed = [0; 2];
    for (from, to) in [(0,256), (256,512), (512,768), (768,1024),
        (1024,2048), (2048,4096), (4096,8192)] {
        input.clear(); expected.clear();
        for events in [&mut input, &mut expected] {
            match from {
                0 => for (id, channel, pan) in [(930,4,0.), (931,7,1.)] {
                    note(events, ExactNoteKind::On, id, channel, 60);
                    expression(events, id, channel, 1, pan);
                },
                256 => cc(events, 4, 1, 37), // existing ordinary MIDI-thru
                512 => expression(events, 930, 4, 0, 0.5),
                768 => note(events, ExactNoteKind::Off, 930, 4, 60),
                1024 => expression(events, 931, 7, 2, 12.),
                2048 => note(events, ExactNoteKind::Off, 931, 7, 60),
                // A late edit to the retired ID must not revive either root.
                4096 => expression(events, 930, 4, 0, 4.),
                _ => unreachable!(),
            }
        }
        if from == 256 { programs(&mut input); }
        for (index, p, events) in [(0, &mut actual, &input), (1, &mut reference, &expected)] {
            let mut at = from;
            while at < to {
                let n = partition[p.block % partition.len()].min(to - at);
                p.block += 1;
                let events = if at == from { events } else { &empty };
                let (mut left, mut right) = ([0.;128], [0.;128]);
                let mut channels = [&mut left[..n], &mut right[..n]];
                let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, n);
                let mut cx = ProcessContext::new(&transport, RATE as f64, n, &mut output);
                assert_eq!(super::super::tests::allocations(|| {
                    Sampler::process(&mut p.dsp, &p.params, &mut buffer, events, &mut cx);
                }), 0);
                for event in output.lossless_iter() {
                    match event {
                        LosslessEventRef::Typed(event) => {
                            if let EventBody::ControlChange { cc:1, value:37, .. } = event.body {
                                assert_eq!(event.port, 0); relayed[index] += 1;
                            }
                            assert!(!matches!(event.body, EventBody::ProgramChange { .. } | EventBody::ProgramChange2 { .. }));
                        },
                        LosslessEventRef::Exact(event) => {
                            if let ExactEventBody::Note { kind:ExactNoteKind::End, address, .. } = *event.body() {
                                assert!(!p.ended.contains(&address));
                                p.ended.push(address); p.ended_frames.push(at + event.sample_offset() as usize);
                            }
                        },
                    }
                }
                output.clear();
                p.pcm.extend(left[..n].iter().zip(&right[..n]).map(|(&l,&r)| [l,r]));
                at += n;
            }
        }
    }
    assert_eq!(relayed, [1,1]);
    assert_eq!(actual.dsp.unsupported_host_expression, 11, "MIDI1 and MIDI2 typed/companion/bare programs each count once");
    assert_eq!(reference.dsp.unsupported_host_expression, 0);
    assert_eq!(actual.ended, [address(930,4,60), address(931,7,60)]);
    assert_eq!(actual.ended, reference.ended); assert_eq!(actual.ended_frames, reference.ended_frames);
    let error = actual.pcm.iter().zip(&reference.pcm).flat_map(|(a,b)|
        [(a[0]-b[0]).abs(), (a[1]-b[1]).abs()]).fold(0_f32, f32::max);
    assert!(error < 1e-7, "unsupported program/bank request changed complete PCM: {error}");
    assert!(actual.pcm[256..768].iter().any(|frame| frame.iter().any(|v|v.abs()>0.005)));
    assert!(actual.pcm[6144..].iter().flatten().all(|v|v.abs()<1e-8));
    for e in [actual.dsp.rack.parts[0].instrument().unwrap(), reference.dsp.rack.parts[0].instrument().unwrap()] {
        assert_eq!(e.active_voices(),0); assert_eq!(e.dropped_commands(),0);
        assert_eq!(e.unsupported_native_controls(),0);
        assert_eq!((e.cc_state()[4][0],e.cc_state()[4][32]),(0,0));
    }
    actual
}

#[test]
fn native_program_changes_report_unsupported_without_changing_plugin_pcm_or_exact_owners() {
    for scripted in [false,true] {
        let fixed = performance(scripted, &[128]);
        let split = performance(scripted, &[1,7,31,127,13,64]);
        assert_eq!(fixed.ended_frames, split.ended_frames);
        let error = fixed.pcm.iter().zip(&split.pcm).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(), (a[1]-b[1]).abs()]).fold(0_f32, f32::max);
        assert!(error < 2e-5, "program diagnostic changed partitioned PCM: {error}");
    }
}
