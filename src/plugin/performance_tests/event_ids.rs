//! Public native event IDs must produce the same owner lifecycle and sound as names.
use super::*;

const NOTE_SCRIPT: &str = r#"on note
%EVENT_PAR[15] := 931
set_event_par($EVENT_ID,$EVENT_PAR_0,27)
set_event_par_arr($EVENT_ID,$EVENT_PAR_CUSTOM,42,3)
set_event_par_arr($EVENT_ID,$EVENT_PAR_MOD_VALUE_ID,250000,2)
set_event_par_arr($EVENT_ID,$EVENT_PAR_MOD_VALUE_EX_ID,1250000,3)
set_event_par_arr($EVENT_ID,$EVENT_PAR_ALLOW_GROUP,0,0)
set_event_par_arr($EVENT_ID,$EVENT_PAR_ALLOW_GROUP,1,0)
set_event_par($EVENT_ID,$EVENT_PAR_NOTE,60)
set_event_par($EVENT_ID,$EVENT_PAR_VELOCITY,64)
set_event_par($EVENT_ID,$EVENT_PAR_MIDI_CHANNEL,7)
set_event_par($EVENT_ID,$EVENT_PAR_VOLUME,-12000)
set_event_par($EVENT_ID,$EVENT_PAR_PAN,-1000)
set_event_par($EVENT_ID,$EVENT_PAR_TUNE,1200000)
if (get_event_par($EVENT_ID,$EVENT_PAR_SOURCE) # -1 or get_event_par($EVENT_ID,$EVENT_PAR_MIDI_CHANNEL) # 7 or get_event_par($EVENT_ID,$EVENT_PAR_NOTE) # 60 or get_event_par($EVENT_ID,$EVENT_PAR_VELOCITY) # 64 or get_event_par($EVENT_ID,$EVENT_PAR_PLAY_POS) # 0 or get_event_par($EVENT_ID,$EVENT_PAR_ZONE_ID) # -1 or get_event_par_arr($EVENT_ID,$EVENT_PAR_CUSTOM,15) # 931 or get_event_par($EVENT_ID,$EVENT_PAR_0) # 27 or get_event_par_arr($EVENT_ID,$EVENT_PAR_CUSTOM,3) # 42 or get_event_par_arr($EVENT_ID,$EVENT_PAR_ALLOW_GROUP,0) # 1 or get_event_par_arr($EVENT_ID,$EVENT_PAR_MOD_VALUE_ID,2) # 250000 or get_event_par_arr($EVENT_ID,$EVENT_PAR_MOD_VALUE_EX_ID,3) # 1250000)
set_event_par($EVENT_ID,$EVENT_PAR_VOLUME,-120000)
end if
wait(40000)
if (get_event_par($EVENT_ID,$EVENT_PAR_VOLUME) # -12000 or get_event_par($EVENT_ID,$EVENT_PAR_PAN) # -1000 or get_event_par($EVENT_ID,$EVENT_PAR_TUNE) # 1200000 or get_event_par($EVENT_ID,$EVENT_PAR_ZONE_ID) # 0)
set_event_par($EVENT_ID,$EVENT_PAR_VOLUME,-120000)
end if
set_event_par($EVENT_ID,$EVENT_PAR_PAN,1000)
set_event_par($EVENT_ID,$EVENT_PAR_TUNE,0)
end on"#;

const RELEASE_SCRIPT: &str = r#"on init
SET_CONDITION(NO_SYS_SCRIPT_RLS_TRIG)
end on
on note
disallow_group($ALL_GROUPS)
allow_group(0)
end on
on release
ignore_event($EVENT_ID)
set_event_par($EVENT_ID,$EVENT_PAR_REL_VELOCITY,127)
if (get_event_par($EVENT_ID,$EVENT_PAR_REL_VELOCITY) # 127)
set_event_par($EVENT_ID,$EVENT_PAR_REL_VELOCITY,0)
end if
wait(40000)
disallow_group($ALL_GROUPS)
allow_group(1)
play_note($EVENT_NOTE,$EVENT_VELOCITY,0,0)
note_off($EVENT_ID)
end on"#;

fn native_literals(script: &str) -> String {
    // Traced ABI numbers, deliberately independent of builtins::event_par.
    let mut script = script.to_owned();
    for (name, id) in [
        ("MOD_VALUE_EX_ID", 34),
        ("MOD_VALUE_ID", 30),
        ("REL_VELOCITY", 27),
        ("MIDI_CHANNEL", 11),
        ("ALLOW_GROUP", 10),
        ("PLAY_POS", 1026),
        ("ZONE_ID", 1027),
        ("VELOCITY", 9),
        ("VOLUME", 4),
        ("CUSTOM", 28),
        ("SOURCE", 8),
        ("TUNE", 6),
        ("NOTE", 7),
        ("PAN", 5),
        ("0", 0),
    ] {
        script = script.replace(&format!("$EVENT_PAR_{name}"), &id.to_string());
    }
    script
}

fn same_pcm(a: &Performance, b: &Performance) {
    assert_eq!(a.ended, b.ended);
    assert_eq!(a.ended_frames, b.ended_frames);
    assert_eq!(a.pcm.len(), b.pcm.len());
    assert!(
        a.pcm
            .iter()
            .zip(&b.pcm)
            .all(|(a, b)| (0..2).all(|ch| (a[ch] - b[ch]).abs() < 2e-5))
    );
}

fn note_lifecycle(partition: &'static [usize], script: &str) -> Performance {
    use crate::modulation::ModSource;
    let group = linear_volume_group(ModSource::Velocity);
    let mut p = Performance::with_group(false, partition, group.clone());
    let mut reference = Performance::with_group(false, partition, group);
    // KSP semantic equivalence needs an unbudgeted script clock; dedicated
    // realtime budget tests retain Realtime and verify callback deferral.
    p.process_mode = moose::core::ProcessMode::Offline;
    reference.process_mode = moose::core::ProcessMode::Offline;
    install_performance_script(&mut p, script);
    let mut events = EventList::with_capacity(8);
    note(&mut events, ExactNoteKind::On, 480, 4, 62);
    p.stage_for(
        "event IDs pending note",
        "native pan5/tune6/velocity9 start before the wait",
        &events,
        1920,
        320,
    );
    events.clear();
    events
        .try_push_exact(ExactEvent::new(
            0,
            ExactEventBody::Note {
                kind: ExactNoteKind::On,
                address: address(480, 4, 60),
                velocity: 64. / 127.,
            },
        ))
        .unwrap();
    expression(&mut events, 480, 4, 0, 10f64.powf(-12. / 20.));
    expression(&mut events, 480, 4, 1, 0.);
    expression(&mut events, 480, 4, 2, 12.);
    reference.stage_for(
        "independent host expression oracle",
        "same velocity, -12dB, left pan and octave without event IDs",
        &events,
        1920,
        320,
    );
    // This pre-wait stage is shorter than the usual measurement window.
    let pending = &p.pcm[320..];
    let pending_reference = &reference.pcm[320..];
    assert_eq!(pending.len(), 1600);
    assert_eq!(pending.len(), pending_reference.len());
    assert!(
        pending
            .iter()
            .zip(pending_reference)
            .all(|(a, b)| (0..2).all(|ch| (a[ch] - b[ch]).abs() < 2e-6))
    );
    // The plugin master defaults to -12 dB, in addition to the event -12 dB.
    let gain = 64. / 127. * 10f64.powf(-24. / 20.);
    assert!((amplitude(pending, 0, 480.) - 0.02 * gain).abs() < 1e-5);
    assert!((amplitude(pending, 0, 960.) - 0.006 * gain).abs() < 1e-5);
    assert!(rms(pending)[1] < 1e-8);
    events.clear();
    p.stage(
        "event IDs resumed note",
        "waited live pan moves right and tune returns to 240Hz",
        &events,
    );
    assert!(rms(p.measured())[0] < 1e-8);
    // The source fundamental and second harmonic are .02 and .006.
    assert!((amplitude(p.measured(), 1, 240.) - 0.02 * gain).abs() < 1e-5);
    assert!((amplitude(p.measured(), 1, 480.) - 0.006 * gain).abs() < 1e-5);
    reference.stage(
        "independent owner held",
        "host owner reaches the same Off clock",
        &events,
    );
    note(&mut events, ExactNoteKind::Off, 480, 4, 62);
    p.stage(
        "event IDs physical owner release",
        "changed logical channel retains the original exact host owner",
        &events,
    );
    events.clear();
    note(&mut events, ExactNoteKind::Off, 480, 4, 60);
    reference.stage(
        "independent owner release",
        "same envelope retires the reference exact owner",
        &events,
    );
    assert_eq!(p.ended, [address(480, 4, 62)]);
    assert_eq!(p.ended_frames, reference.ended_frames);
    p
}

#[test]
fn named_and_native_literal_event_ids_reach_waited_plugin_pcm_and_exact_ends() {
    let literal_note = native_literals(NOTE_SCRIPT);
    let literal_release = native_literals(RELEASE_SCRIPT);
    let fixed = note_lifecycle(&[128], NOTE_SCRIPT);
    for partition in [&[128][..], &[1, 7, 31, 127][..]] {
        let named = note_lifecycle(partition, NOTE_SCRIPT);
        let literal = note_lifecycle(partition, &literal_note);
        same_pcm(&fixed, &named);
        same_pcm(&named, &literal);
        let (_, oracle) = release_bridge_pair(partition, None, [1., 1.]);
        let (named, gain) = release_bridge_pair(partition, Some(RELEASE_SCRIPT), [0.5001, 0.5002]);
        let (literal, literal_gain) =
            release_bridge_pair(partition, Some(&literal_release), [0.5001, 0.5002]);
        same_pcm(&named, &literal);
        for ch in 0..2 {
            ratio(
                gain[ch] / oracle[ch],
                1.,
                "release27 sets full-scale inherited release velocity",
            );
            ratio(
                literal_gain[ch] / oracle[ch],
                1.,
                "literal release27 reaches the generated child",
            );
        }
        assert_eq!(named.ended_frames, [37520, 37520]);
    }
}
