//! Native CC messages never complete a MIDI1 parameter transaction.
use super::*;
use crate::articulate::{Articulate, Articulation, Mode, Mpe, Route};

fn native_cc(events: &mut EventList, raw: bool, controller: u8, value: u32) {
    if raw {
        events
            .try_push_exact(ExactEvent::new(
                0,
                ExactEventBody::Ump {
                    port: 0,
                    packet: moose::core::events::RawUmp::new(
                        [0x40b4_0000 | (u32::from(controller) << 8), value, 0, 0],
                        2,
                    )
                    .unwrap(),
                },
            ))
            .unwrap();
    } else {
        events.push(Event::on_port(
            0,
            0,
            EventBody::ControlChange2 {
                group: 0,
                channel: 4,
                cc: controller,
                value,
            },
        ));
    }
}

fn native_parameter(
    events: &mut EventList,
    raw: bool,
    registered: bool,
    relative: bool,
    index: u8,
    value: u32,
) {
    if raw {
        let status = match (registered, relative) {
            (true, false) => 2,
            (false, false) => 3,
            (true, true) => 4,
            (false, true) => 5,
        };
        events
            .try_push_exact(ExactEvent::new(
                0,
                ExactEventBody::Ump {
                    port: 0,
                    packet: moose::core::events::RawUmp::new(
                        [0x4004_0000 | (status << 20) | u32::from(index), value, 0, 0],
                        2,
                    )
                    .unwrap(),
                },
            ))
            .unwrap();
    } else {
        let body = match (registered, relative) {
            (true, false) => EventBody::RegisteredController {
                group: 0,
                channel: 4,
                bank: 0,
                index,
                value,
            },
            (false, false) => EventBody::AssignableController {
                group: 0,
                channel: 4,
                bank: 0,
                index,
                value,
            },
            (true, true) => EventBody::RelativeRegisteredController {
                group: 0,
                channel: 4,
                bank: 0,
                index,
                delta: value as i32,
            },
            (false, true) => EventBody::RelativeAssignableController {
                group: 0,
                channel: 4,
                bank: 0,
                index,
                delta: value as i32,
            },
        };
        events.push(Event::on_port(0, 0, body));
    }
}

fn seen(p: &Performance) -> Vec<i32> {
    let saved = p.dsp.rack.parts[0].instrument().unwrap().script().unwrap().persistence();
    let crate::ksp::Value::IntArray(values) = &saved[0]["%seen"] else {
        panic!("callback snapshots")
    };
    values.clone()
}

fn compare(a: &Performance, b: &Performance) {
    assert_eq!(a.pcm.len(), b.pcm.len());
    assert!(
        a.pcm
            .iter()
            .zip(&b.pcm)
            .all(|(a, b)| (0..2).all(|ch| (a[ch] - b[ch]).abs() < 2e-5)),
        "mixed protocols changed complete source PCM"
    );
    assert_eq!(a.ended, b.ended);
    assert_eq!(a.ended_frames, b.ended_frames);
}

fn performance(partition: &'static [usize], raw: bool) -> Performance {
    let mut p = Performance::new(false, partition);
    let art = Articulate {
        source: "mixed-protocol".into(),
        mode: Mode::Channel,
        articulations: (0..16)
            .map(|channel| Articulation {
                channel,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    p.dsp.routers[0].set_route(Route::new("mixed-protocol", &art, &Mpe::default()));
    p.dsp.rack.controls[0].channel = 7;
    p.dsp.rack.parts[0].instrument_mut().unwrap().set_home_channel(7);
    install_performance_script(
        &mut p,
        r#"on init
declare $first
declare %seen[5]
make_persistent(%seen)
end on
on note
if ($first=0)
$first := $EVENT_ID
end if
end on
on controller
inc(%seen[0])
%seen[1] := $MIDI_CHANNEL
if ($CC_NUM=96 or $CC_NUM=97)
%seen[2] := %CC[$CC_NUM]
end if
end on
on rpn
%seen[3] := $RPN_VALUE
%seen[1] := $MIDI_CHANNEL
end on
on nrpn
wait(1000)
%seen[4] := $RPN_VALUE
%seen[1] := $MIDI_CHANNEL
if ($RPN_VALUE=-268435456)
change_vol($first,-12000,0)
end if
end on"#,
    );
    let mut reference = Performance::new(false, partition);
    let mut wire = EventList::with_capacity(32);
    let mut oracle = EventList::with_capacity(16);
    for (controller, value) in [(101, 0), (100, 0), (6, 12), (38, 99)] {
        cc(&mut wire, 4, controller, value);
    }
    wire.push(Event::on_port(
        0,
        0,
        EventBody::PitchBend {
            group: 0,
            channel: 4,
            value: 12288,
        },
    ));
    for (id, channel, pan, tune) in [(490, 4, 0., 6.495), (491, 7, 1., 0.)] {
        note(&mut wire, ExactNoteKind::On, id, channel, 60);
        expression(&mut wire, id, channel, 1, pan);
        note(&mut oracle, ExactNoteKind::On, id, channel, 60);
        expression(&mut oracle, id, channel, 1, pan);
        expression(&mut oracle, id, channel, 2, tune);
    }
    p.stage(
        "mixed protocol routed onset",
        "MIDI1 RPN0 range12.99 bends physical4 only despite shared logical home7",
        &wire,
    );
    reference.stage(
        "independent pitch onset",
        "authored6.495 semitones left and source240Hz right",
        &oracle,
    );
    compare(&p, &reference);
    let before = seen(&p);
    let unsupported = p.dsp.unsupported_host_expression;
    wire.clear();
    oracle.clear();
    for (controller, value) in [
        (0, 127u8),
        (6, 1),
        (32, 127),
        (38, 1),
        (98, 0),
        (99, 0),
        (100, 127),
        (101, 127),
    ] {
        native_cc(
            &mut wire,
            raw,
            controller,
            (u32::from(value) << 25) | 0x1ff_ffff,
        );
    }
    p.stage(
        "native reserved CC ingress",
        "native selectors/data/bank messages cannot finish MIDI1 transaction or invoke callbacks",
        &wire,
    );
    reference.stage(
        "independent excluded CC reference",
        "pitched source and unaffected owner continue unchanged",
        &oracle,
    );
    compare(&p, &reference);
    assert_eq!(
        seen(&p),
        before,
        "excluded native CC invoked a user callback"
    );
    assert_eq!(
        p.dsp.unsupported_host_expression, unsupported,
        "valid deliberately ignored typed/raw native CC must not diagnose unsupported input"
    );
    for (controller, value) in [(96, 127), (97, 0)] {
        wire.clear();
        native_cc(
            &mut wire,
            raw,
            controller,
            if value == 127 { u32::MAX } else { 0 },
        );
        p.stage(
            "native raw relative-number CC",
            "native96/97 deliver raw callback but do not step MIDI1 RPN0",
            &wire,
        );
        reference.stage(
            "independent unstepped reference",
            "same6.495 semitones without native CC decoder mutation",
            &oracle,
        );
        compare(&p, &reference);
        let values = seen(&p);
        assert_eq!(values[2], value);
        assert_eq!(values[1], 7, "callback retains routed logical channel");
    }
    assert_eq!(seen(&p)[0], before[0] + 2);
    for (controller, value, tune) in [(6, 6, 3.495), (96, 127, 3.5), (97, 0, 3.495)] {
        wire.clear();
        oracle.clear();
        cc(&mut wire, 4, controller, value);
        expression(&mut oracle, 490, 4, 2, tune);
        p.stage(
            "real MIDI1 data and relative updates",
            "armed original physical MIDI1 decoder remains selected after native messages",
            &wire,
        );
        reference.stage(
            "independent MIDI1 pitch update",
            "half6.99,7.00,6.99 sensitivity in semitones",
            &oracle,
        );
        compare(&p, &reference);
    }
    for (relative, index, value, tune) in [
        (false, 1, 0x9000_0000, 3.62),
        (true, 1, 0x1000_0000, 3.745),
        (false, 2, 65u32 << 25, 4.745),
        (true, 2, (-(1i32 << 25)) as u32, 3.745),
    ] {
        wire.clear();
        oracle.clear();
        native_parameter(&mut wire, raw, true, relative, index, value);
        expression(&mut oracle, 490, 4, 2, tune);
        p.stage(
            "native unified registered parameter",
            "native exact fine/coarse updates stay on their own parameter path",
            &wire,
        );
        reference.stage(
            "independent native parameter pitch",
            "fine+.125,+.25 then coarse+1,0 added to original bend",
            &oracle,
        );
        compare(&p, &reference);
        assert_eq!(
            seen(&p)[3],
            if relative {
                value as i32
            } else {
                (value >> 18) as i32
            }
        );
        assert_eq!(seen(&p)[1], 7);
    }
    wire.clear();
    oracle.clear();
    native_parameter(&mut wire, raw, false, false, 123, 0x1234_5678);
    p.stage(
        "native unified assignable absolute",
        "unknown native NRPN reaches callback without pitch mapping",
        &wire,
    );
    reference.stage(
        "independent assignable absolute reference",
        "both pitched sources continue",
        &oracle,
    );
    compare(&p, &reference);
    assert_eq!(seen(&p)[4], (0x1234_5678u32 >> 18) as i32);
    wire.clear();
    native_parameter(&mut wire, raw, false, true, 123, (-0x1000_0000i32) as u32);
    p.stage(
        "waited signed native NRPN",
        "negative full32 delta authors -12dB at original48sample wait boundary",
        &wire,
    );
    reference.stage_for(
        "independent wait prefix",
        "source continues before callback action",
        &oracle,
        48,
        0,
    );
    expression(&mut oracle, 490, 4, 0, 10f64.powf(-12. / 20.));
    reference.stage_for(
        "independent delayed volume oracle",
        "exact host expression arrives after48samples on left owner only",
        &oracle,
        SETTLE + WINDOW - 48,
        SETTLE - 48,
    );
    compare(&p, &reference);
    assert_eq!(seen(&p)[4], -0x1000_0000);
    assert!((pcm_fundamental(p.measured(), 0) - 240. * 2f64.powf(3.745 / 12.)).abs() < 2e-5);
    assert!((pcm_fundamental(p.measured(), 1) - 240.).abs() < 2e-5);
    wire.clear();
    oracle.clear();
    for (id, channel) in [(490, 4), (491, 7)] {
        note(&mut wire, ExactNoteKind::Off, id, channel, 60);
        note(&mut oracle, ExactNoteKind::Off, id, channel, 60);
    }
    p.stage(
        "mixed protocol exact roots release",
        "original physical addresses own all waited callbacks and retire once",
        &wire,
    );
    reference.stage(
        "independent owner end reference",
        "same two source envelopes retire without protocol state",
        &oracle,
    );
    compare(&p, &reference);
    assert_eq!(p.ended, [address(490, 4, 60), address(491, 7, 60)]);
    p
}

#[test]
fn mixed_native_cc_and_midi1_parameter_transactions_preserve_routed_plugin_pcm() {
    let fixed = performance(&[128], false);
    for (partition, raw) in [
        (&[128][..], true),
        (&[1, 7, 31, 127][..], false),
        (&[1, 7, 31, 127][..], true),
    ] {
        compare(&fixed, &performance(partition, raw));
    }
}
