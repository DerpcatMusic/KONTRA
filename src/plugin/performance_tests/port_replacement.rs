use super::*;
use crate::articulate::{Articulation, Mode};

fn performance(partition: &'static [usize], scripted: bool) -> Performance {
    use crate::modulation::{ModAssignment, ModSource, ModTarget};
    let mut group = Group { mods: vec![ModAssignment {
        cc_default: None,
        name: "authored bend fallback".into(), source: ModSource::PitchBend,
        target: ModTarget::Pitch, intensity: 1., invert: false, lag_ms: 0, shaper: None,
    }], ..Group::default() };
    if scripted {
        use ni_file::kontakt::objects::{StartCriteriaList, StartCriteriaParams};
        // Raw native eligibility proves replay on the new home and clearing
        // after old release callbacks, independently of accepted CC defaults.
        group.start_criteria = StartCriteriaList {
            mask: 3,
            items: [(1,73), (2,0)].map(|(controller, value)| StartCriteriaParams {
                mode:2, next_criteria:0, key_min:0, key_max:127,
                controller, cc_min:value, cc_max:value, cycle_class:0,
                slice_zone_idx:-1, slice_zone_slice_idx:-1, sequencer_only:false,
            }).into(),
            unknown_tail: Vec::new(),
        };
    }
    let mut actual = Performance::with_group(false, partition, group);
    let mut oracle = Performance::new(false, partition);
    // Channel articulation sends physical4 to the selected home. Replacement
    // therefore changes port and delivery channel together, without changing
    // the physical controller owner or independently authored audio oracle.
    let art = Articulate {
        source: "port-replacement".into(), mode: Mode::Channel,
        articulations: (0..16).map(|channel| Articulation { channel, ..Default::default() }).collect(),
        ..Default::default()
    };
    for p in [&mut actual, &mut oracle] {
        p.dsp.routers[0].set_route(Route::new("port-replacement", &art, &Mpe::default()));
    }
    if scripted {
        for p in [&mut actual, &mut oracle] {
            install_performance_script(p, "on init\nset_controller(1,73)\nend on\non note\nwait(1000)\nend on\non release\nset_controller(2,99)\nend on\non controller\nwait(1000)\nend on\non rpn\nwait(1000)\nend on");
        }
    }
    let mut wire = EventList::with_capacity(256);
    let mut expected = EventList::with_capacity(16);
    let empty = EventList::with_capacity(0);
    let address = |port, id| ExactNoteAddress::from_raw_signed(port, 4, 60, id);
    let note = |events: &mut EventList, port, id, kind| {
        events.try_push_exact(ExactEvent::new(0, ExactEventBody::Note {
            kind, address: address(port, id), velocity: 1.,
        })).unwrap();
    };
    let expression = |events: &mut EventList, port, id, expression_id, value| {
        events.try_push_exact(ExactEvent::new(0, ExactEventBody::NoteExpression {
            expression_id, address: address(port, id), value,
        })).unwrap();
    };
    let control = |events: &mut EventList, port, cc, value| {
        events.push(Event::on_port(0, port, EventBody::ControlChange {
            group: 0, channel: 4, cc, value,
        }));
    };
    let bend = |events: &mut EventList, port, value| {
        events.push(Event::on_port(0, port, EventBody::PitchBend {
            group: 0, channel: 4, value,
        }));
    };
    let route = |p: &mut Performance, port, channel| {
        let mut controls = Mix { parts: p.dsp.rack.controls.clone(), buses: p.dsp.rack.bus_controls };
        controls.parts[0].port = port;
        controls.parts[0].channel = channel;
        p.params.shared.controls.push(controls).ok().unwrap();
    };
    let compare = |a: &Performance, b: &Performance| {
        assert_eq!(a.pcm.len(), b.pcm.len());
        let error = a.pcm.iter().zip(&b.pcm).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(), (a[1]-b[1]).abs()]).fold(0f32, f32::max);
        assert!(error < 2e-5, "complete port-replacement PCM differs from independent authored pitch by {error}");
        assert_eq!(a.ended, b.ended);
        assert_eq!(a.ended_frames, b.ended_frames);
    };
    let play = |actual: &mut Performance, oracle: &mut Performance, wire: &EventList, expected: &EventList, name, frames| {
        actual.stage_for(name, "physical port configuration follows its input owner", wire, frames, 0);
        oracle.stage_for(name, "independently authored semitone reference", expected, frames, 0);
        compare(actual, oracle);
    };

    // These settings are deliberately absent from the independent oracle.
    // MIDI1 fine/coarse tuning gives12.5st; RPN0 supplies24st sensitivity;
    // native RPN7 supplies6st per-note sensitivity instead of the local48st default.
    legacy_tuning(&mut wire, 4, 1, 96, Some(0));
    legacy_tuning(&mut wire, 4, 2, 76, Some(0));
    legacy_tuning(&mut wire, 4, 0, 24, Some(0));
    wire.push(Event::on_port(0, 0, EventBody::RegisteredController {
        group: 0, channel: 4, bank: 0, index: 7, value: 6u32 << 25,
    }));
    play(&mut actual, &mut oracle, &wire, &empty, "old port negotiates independent configuration", 256);
    wire.clear();
    bend(&mut wire, 0, 12288);
    note(&mut wire, 0, 400, ExactNoteKind::On);
    note(&mut expected, 0, 400, ExactNoteKind::On);
    expression(&mut expected, 0, 400, 2, 24.5);
    play(&mut actual, &mut oracle, &wire, &expected, "old port audible tuning plus negotiated bend", 4096);
    assert!(rms(&actual.pcm[1024..4352]).iter().all(|v| *v > 0.001));
    wire.clear(); expected.clear();
    note(&mut wire, 0, 400, ExactNoteKind::Off);
    note(&mut expected, 0, 400, ExactNoteKind::Off);
    play(&mut actual, &mut oracle, &wire, &expected, "old exact owner releases before replacement", 4096);
    assert!(actual.pcm[actual.pcm.len()-128..].iter().all(|v| *v == [0.;2]));

    // A same-port channel selector change and host/audio reset keep the existing
    // configuration policy. Only replacement of the physical port clears it.
    for p in [&mut actual, &mut oracle] {
        route(p, 0, 4);
        assert_eq!(super::super::tests::allocations(||
            Sampler::reset(&mut p.dsp, &p.params, &AudioConfig::new(RATE as f64, 128))), 0);
    }
    wire.clear(); expected.clear();
    bend(&mut wire, 0, 12288);
    note(&mut wire, 0, 401, ExactNoteKind::On);
    note(&mut expected, 0, 401, ExactNoteKind::On);
    expression(&mut expected, 0, 401, 2, 24.5);
    play(&mut actual, &mut oracle, &wire, &expected, "same-port and host reset retain negotiation", 4096);
    // Keep401 sounding while399 waits for aligned onset. Rerouting must cancel
    // both lifetimes, preserving the sounding root's and pre-On root's tuples.
    for p in [&mut actual, &mut oracle] {
        p.dsp.align.plan.on = true;
        p.dsp.align.plan.transport_only = false;
        p.dsp.align.plan.parts.fill(timing::Holds::of(&timing::Timing::default(), &[], 20.));
    }
    wire.clear(); expected.clear();
    // Negotiate an unrelated upper MPE zone. Physical4's pitch remains intact,
    // but this manager would incorrectly own service callbacks after reroute
    // unless the replacement clears it before any new-port input arrives.
    for (cc,value) in [(101,0),(100,6),(6,1)] {
        wire.push(Event::on_port(0,0,EventBody::ControlChange {group:0,channel:15,cc,value}));
    }
    note(&mut wire, 0, 399, ExactNoteKind::On);
    note(&mut expected, 0, 399, ExactNoteKind::On);
    play(&mut actual, &mut oracle, &wire, &expected, "old port retains pending exact root", 128);
    assert!(actual.dsp.rack.parts[0].instrument().unwrap().mpe_zones().manager(15).is_some());
    let replaced_at = actual.pcm.len();
    for p in [&mut actual, &mut oracle] { route(p, 1, 7); p.dsp.align.plan.on = false; }

    wire.clear(); expected.clear();
    bend(&mut wire, 1, 12288);
    note(&mut wire, 1, 401, ExactNoteKind::On);
    expression(&mut wire, 1, 401, 2, 2.);
    note(&mut expected, 1, 401, ExactNoteKind::On);
    // The unchanged authored bend assignment is12st full scale, so the fresh
    // half bend adds6st, alongside this root's exact2st overlay.
    expression(&mut expected, 1, 401, 2, 8.);
    // Reject the first outgoing NOTE_END. Store one complete PCM frame from
    // this real process call; both root identities must survive for retry.
    for p in [&mut actual, &mut oracle] {
        let (mut left, mut right) = ([0.;1], [0.;1]);
        let mut channels = [&mut left[..], &mut right[..]];
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, 1);
        let transport = TransportInfo::default();
        let mut rejected = EventList::with_capacity(0);
        let mut cx = ProcessContext::new(&transport, RATE as f64, 1, &mut rejected)
            .with_process_mode(moose::core::ProcessMode::Realtime);
        assert_eq!(super::super::tests::allocations(|| {
            assert_eq!(Sampler::process(&mut p.dsp, &p.params, &mut buffer, &empty, &mut cx), ProcessStatus::Normal);
        }), 0);
        assert!(rejected.lossless_iter().next().is_none());
        p.pcm.push([left[0], right[0]]);
        p.input.push(json!({"frame":replaced_at,"stage":"port replacement with rejected NOTE_END",
            "event":"no new input; original port0 sounding and aligned roots canceled"}));
        assert_eq!(p.dsp.host_note_end_rejections, 1);
        let old = crate::engine::HostNote {port:0,channel:4,key:60,id:401,clap:true};
        assert!(p.dsp.rack.parts[0].host_note_present(old));
        assert_eq!(p.dsp.rack.parts[0].host_note_end_at(old), Some(replaced_at as u64));
        assert!(p.dsp.rack.parts[0].instrument().unwrap().mpe_zones().zones().iter().all(|zone|zone.members==0),
            "old negotiated MPE manager must clear before any new-port feed");
    }
    compare(&actual, &oracle);
    play(&mut actual, &mut oracle, &wire, &expected, "replacement clears received tuning and RPN0; exact ends retry", 4095);
    assert_eq!(actual.ended, [address(0,400), address(0,401), address(0,399)]);
    assert_eq!(&actual.ended_frames[1..3], &[replaced_at+1, replaced_at+1]);
    assert!(rms(&actual.pcm[replaced_at+1024..]).iter().all(|v| *v > 0.001));

    // Every wire CC, plus channel/native expression and anonymous key-up on the
    // old port, is filtered before the new port's physical state and callbacks.
    wire.clear(); expected.clear();
    for cc in 0..128 { control(&mut wire, 0, cc, 127); }
    bend(&mut wire, 0, 0);
    wire.push(Event::on_port(0, 0, EventBody::ChannelPressure { group:0, channel:4, pressure:127 }));
    wire.push(Event::on_port(0, 0, EventBody::RegisteredController {
        group:0, channel:4, bank:0, index:2, value:0,
    }));
    wire.push(Event::on_port(0, 0, EventBody::PerNotePitchBend { group:0, channel:4, note:60, value:0 }));
    wire.push(Event::on_port(0, 0, EventBody::NoteOff2 { group:0, channel:4, note:60,
        velocity:0, attribute_type:0, attribute:0 }));
    expression(&mut wire, 0, 401, 0, 0.);
    expression(&mut wire, 0, 401, 2, -48.);
    note(&mut wire, 0, 401, ExactNoteKind::Off);
    play(&mut actual, &mut oracle, &wire, &empty, "late old-port traffic preserves new exact root", 4096);
    assert_eq!(actual.dsp.rack.parts[0].active_voices(), 1);
    assert_eq!(actual.ended.len(), 3);
    wire.clear(); expected.clear();
    note(&mut wire, 1, 401, ExactNoteKind::Off);
    note(&mut expected, 1, 401, ExactNoteKind::Off);
    play(&mut actual, &mut oracle, &wire, &expected, "new port exact owner ends independently", 4096);

    // Anonymous MIDI2 onset binds the replacement port's compatibility48st
    // range, not the previous port's negotiated6st range. Clear channel bend
    // and let the independently authored exact reference supply24st.
    // Returning to home4 on the same new port also proves that old release
    // CC2=99 was cleared after cleanup: native CC2=0 eligibility must admit.
    for p in [&mut actual, &mut oracle] { route(p, 1, 4); }
    wire.clear(); expected.clear();
    bend(&mut wire, 1, 8192);
    wire.push(Event::on_port(0, 1, EventBody::PerNotePitchBend { group:0, channel:4, note:60, value:0xc000_0000 }));
    wire.push(Event::on_port(0, 1, EventBody::NoteOn2 { group:0, channel:4, note:60,
        velocity:u16::MAX, attribute_type:0, attribute:0 }));
    note(&mut expected, 1, 402, ExactNoteKind::On);
    expression(&mut expected, 1, 402, 2, 24.);
    actual.stage_for("replacement clears native RPN7 range", "half of new port48st range is24st", &wire, 4096, 0);
    oracle.stage_for("independent per-note range oracle", "exact authored24st pitch", &expected, 4096, 0);
    let error = actual.pcm.iter().zip(&oracle.pcm).flat_map(|(a,b)|
        [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
    assert!(error < 2e-5, "replacement native range complete PCM mismatch: {error}");
    wire.clear(); expected.clear();
    wire.push(Event::on_port(0, 1, EventBody::NoteOff2 { group:0, channel:4, note:60,
        velocity:0, attribute_type:0, attribute:0 }));
    note(&mut expected, 1, 402, ExactNoteKind::Off);
    actual.stage_for("anonymous replacement root releases", "no invented exact NOTE_END", &wire, 4096, 0);
    oracle.stage_for("per-note range oracle releases", "exact reference retires once", &expected, 4096, 0);
    let error = actual.pcm.iter().zip(&oracle.pcm).flat_map(|(a,b)|
        [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
    assert!(error < 2e-5, "complete release PCM mismatch: {error}");
    assert!(actual.pcm[actual.pcm.len()-128..].iter().all(|v| *v == [0.;2]));
    assert_eq!(actual.ended, [address(0,400), address(0,401), address(0,399), address(1,401)]);
    assert_eq!(actual.ended_frames, oracle.ended_frames[..4]);
    assert_eq!(oracle.ended[4], address(1,402));
    assert_eq!(actual.dsp.unsupported_host_expression, 0);
    assert_eq!(actual.dsp.unsupported_note_brightness, 0);
    assert_eq!(actual.dsp.host_note_end_rejections, 1);
    assert!(actual.dsp.rack.parts[0].instrument().unwrap().script().is_none_or(|rt| rt.diagnostics().is_empty()));
    actual
}

#[test]
fn replacing_input_port_clears_only_received_configuration_in_complete_plugin_pcm() {
    for scripted in [false, true] {
        let fixed = performance(&[128], scripted);
        let fragmented = performance(&[1,7,31,127,13,64], scripted);
        assert_eq!(fixed.pcm.len(), fragmented.pcm.len());
        let error = fixed.pcm.iter().zip(&fragmented.pcm).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(), (a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(error < 2e-5, "port replacement depends on callback partition: {error}");
        assert_eq!(fixed.ended, fragmented.ended);
        assert_eq!(fixed.ended_frames, fragmented.ended_frames);
        fixed.export(if scripted { "port-replacement-scripted" } else { "port-replacement" });
    }
}
