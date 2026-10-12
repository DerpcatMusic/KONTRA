use super::*;

fn parameter(events: &mut EventList, raw: bool, registered: bool, address: u16, delta: i32) {
    let bank = (address >> 7) as u8;
    let index = (address & 127) as u8;
    if raw {
        let status = if registered { 4 } else { 5 };
        let word =
            0x4000_0000 | (status << 20) | (4 << 16) | (u32::from(bank) << 8) | u32::from(index);
        events
            .try_push_exact(ExactEvent::new(
                0,
                ExactEventBody::Ump {
                    port: 0,
                    packet: moose::core::events::RawUmp::new([word, delta as u32, 0, 0], 2)
                        .unwrap(),
                },
            ))
            .unwrap();
    } else {
        let body = if registered {
            EventBody::RelativeRegisteredController {
                group: 0,
                channel: 4,
                bank,
                index,
                delta,
            }
        } else {
            EventBody::RelativeAssignableController {
                group: 0,
                channel: 4,
                bank,
                index,
                delta,
            }
        };
        events.push(Event::on_port(0, 0, body));
    }
}

fn performance(partition: &'static [usize], raw: bool) -> Performance {
    let mut p = Performance::new(false, partition);
    install_performance_script(
        &mut p,
        "on init\ndeclare $first\ndeclare %seen[3]\nmake_persistent(%seen)\nend on\non note\nif ($first=0)\n$first := $EVENT_ID\nend if\nend on\non rpn\nwait(1000)\nif ($RPN_ADDRESS=1)\n%seen[0] := $RPN_VALUE\nend if\nif ($RPN_ADDRESS=2)\n%seen[1] := $RPN_VALUE\nend if\nend on\non nrpn\nwait(1000)\n%seen[2] := $RPN_VALUE\nif ($RPN_ADDRESS=123 and $RPN_VALUE=-268435456)\nchange_vol($first,-12000,0)\nend if\nend on",
    );
    let mut events = EventList::with_capacity(16);
    for (id, channel, pan) in [(300, 4, 0.), (301, 7, 1.)] {
        note(&mut events, ExactNoteKind::On, id, channel, 60);
        expression(&mut events, id, channel, 1, pan);
    }
    let full = p.stage(
        "native relative baseline",
        "independent physical roots at 240 Hz",
        &events,
    );
    for (address, delta, semitones) in [
        (1, 0x1000_0000, 0.125),
        (2, 1 << 25, 1.125),
        (1, -0x1000_0000, 1.),
        (2, -(1 << 25), 0.),
    ] {
        events.clear();
        parameter(&mut events, raw, true, address, delta);
        p.stage(
            "native signed relative tuning",
            "physical4 changes; physical7 keeps 240 Hz",
            &events,
        );
        let expected = 240. * 2f64.powf(semitones / 12.);
        assert!((pcm_fundamental(p.measured(), 0) - expected).abs() < 2e-5);
        assert!((pcm_fundamental(p.measured(), 1) - 240.).abs() < 2e-5);
        let saved = p.dsp.rack.parts[0].instrument().unwrap().script().unwrap().persistence();
        let crate::ksp::Value::IntArray(seen) = &saved[0]["%seen"] else {
            panic!("signed callback snapshot")
        };
        assert_eq!(
            seen[address as usize - 1],
            delta,
            "waited callback must retain the signed32 delta"
        );
    }
    events.clear();
    parameter(&mut events, raw, false, 123, -0x1000_0000);
    let authored = p.stage(
        "native relative NRPN callback",
        "unknown parameter authors left-only -12dB without native pitch mapping",
        &events,
    );
    assert!((authored[0] / full[0] - 10f64.powf(-12. / 20.)).abs() < 1e-5);
    assert!((authored[1] / full[1] - 1.).abs() < 1e-5);
    for ch in 0..2 {
        assert!((pcm_fundamental(p.measured(), ch) - 240.).abs() < 2e-5);
    }
    let saved = p.dsp.rack.parts[0].instrument().unwrap().script().unwrap().persistence();
    let crate::ksp::Value::IntArray(seen) = &saved[0]["%seen"] else {
        panic!("NRPN callback snapshot")
    };
    assert_eq!(seen[2], -0x1000_0000);
    events.clear();
    for (id, channel) in [(300, 4), (301, 7)] {
        note(&mut events, ExactNoteKind::Off, id, channel, 60);
    }
    assert!(
        p.stage(
            "native relative exact ends",
            "callbacks leave both released roots silent",
            &events
        )
        .iter()
        .all(|v| *v < 1e-8)
    );
    assert_eq!(p.ended, [address(300, 4, 60), address(301, 7, 60)]);
    p
}

#[test]
fn native_relative_rpn_and_nrpn_keep_signed_callbacks_physical_pitch_and_exact_ends_in_pcm() {
    let reference = performance(&[128], false);
    for (partition, raw) in [
        (&[128][..], true),
        (&[1, 7, 31, 127, 13, 64][..], false),
        (&[1, 7, 31, 127, 13, 64][..], true),
    ] {
        let actual = performance(partition, raw);
        assert_eq!(actual.pcm.len(), reference.pcm.len());
        let error = actual
            .pcm
            .iter()
            .zip(&reference.pcm)
            .flat_map(|(a, b)| [(a[0] - b[0]).abs(), (a[1] - b[1]).abs()])
            .fold(0f32, f32::max);
        assert!(
            error < 2e-5,
            "native relative delivery or partition changed PCM: {error}"
        );
        assert_eq!(actual.ended_frames, reference.ended_frames);
    }
}

#[test]
fn unsupported_native_relative_sensitivity_and_mpe_configuration_leave_plugin_pcm_unchanged() {
    for raw in [false, true] {
        let mut actual = Performance::new(false, &[128]);
        let mut reference = Performance::new(false, &[128]);
        let mut events = EventList::with_capacity(8);
        note(&mut events, ExactNoteKind::On, 302, 4, 60);
        actual.stage(
            "unsupported relative baseline",
            "same untouched instrument",
            &events,
        );
        reference.stage(
            "unsupported relative reference",
            "same untouched instrument",
            &events,
        );
        events.clear();
        parameter(&mut events, raw, true, 0, 1 << 25);
        parameter(&mut events, raw, true, 6, -(1 << 25));
        actual.stage(
            "unsupported relative 0 and 6",
            "no fabricated absolute sensitivity or MPE configuration",
            &events,
        );
        events.clear();
        reference.stage(
            "untouched configuration reference",
            "empty wire input",
            &events,
        );
        assert_eq!(actual.dsp.rack.parts[0].unsupported_native_controls(), 2);
        // A later valid bend exercises the configuration, not just its current pitch.
        events.push(Event::on_port(
            0,
            0,
            EventBody::PitchBend2 {
                group: 0,
                channel: 4,
                value: 0xc000_0000,
            },
        ));
        actual.stage(
            "bend after unsupported deltas",
            "sensitivity and routing still match the untouched reference",
            &events,
        );
        reference.stage("untouched bend reference", "identical valid bend", &events);
        let error = actual
            .pcm
            .iter()
            .zip(&reference.pcm)
            .flat_map(|(a, b)| [(a[0] - b[0]).abs(), (a[1] - b[1]).abs()])
            .fold(0f32, f32::max);
        assert!(
            error < 1e-6,
            "unsupported relative configuration changed PCM: {error}"
        );
        events.clear();
        note(&mut events, ExactNoteKind::Off, 302, 4, 60);
        assert!(
            actual
                .stage(
                    "unsupported relative exact end",
                    "configuration diagnostics do not retain the root",
                    &events
                )
                .iter()
                .all(|v| *v < 1e-8)
        );
        reference.stage("untouched exact end", "same note retirement clock", &events);
        assert_eq!(actual.ended, [address(302, 4, 60)]);
        assert_eq!(actual.ended_frames, reference.ended_frames);
    }
}
