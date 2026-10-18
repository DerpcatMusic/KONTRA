use super::*;

fn fixture(code: &str, looping: bool) -> (Runtime, Driver<ScriptHost>) {
    let playback = if looping {
        sampler_core::Playback {
            loop_range: Some(sampler_core::Loop {
                start: 0,
                end: 128,
                mode: sampler_core::LoopMode::UntilRelease,
                shape: sampler_core::LoopShape::Wrap,
                passes: None,
            }),
            ..Default::default()
        }
    } else {
        sampler_core::Playback {
            end: Some(128),
            ..Default::default()
        }
    };
    let plan = Prepared::new(
        48000,
        vec![sampler_core::Pcm::new(48000, vec![[0.25; 2]; 256].into_boxed_slice()).unwrap()],
        vec![sampler_core::Region {
            sample: 0,
            key_low: 0,
            key_high: 127,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: sampler_core::Envelope::new(0, 0, 0, 1., 64).unwrap(),
            playback,
        }],
        128,
    )
    .unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    let xml = format!(
        "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{code}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    let host = ScriptHost::new(&xml, (), crate::script::Config::default()).unwrap();
    (
        Runtime::new(plan, limits).unwrap(),
        Driver::new(host, vec![], 48000),
    )
}

#[test]
fn duration_zero_child_retires_at_source_end_without_releasing_held_input() {
    let (mut rt, mut driver) = fixture(
        "function onNote(e) playNote(e.note,e.velocity,0) end",
        false,
    );
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    };
    let physical = rt.note_on(input, 60, 1.).unwrap();
    driver.note_on(&mut rt, physical, 60, 1.).unwrap();
    rt.render(&mut [[0.; 2]; 256]).unwrap();
    rt.flush_ended(|_| true);
    assert_eq!(rt.voice_count(), 0);
    assert_eq!(
        rt.note_count(),
        1,
        "only the still-held physical input may remain"
    );
    assert!(rt.note(physical).unwrap().2);
    driver.note_off(&mut rt, 60).unwrap();
    rt.flush_ended(|_| true);
    let mut keys = [0; 128];
    rt.pressed_keys(&mut keys);
    assert_eq!(keys, [0; 128]);
    assert_eq!(rt.note_count(), 0);
}

#[test]
fn duration_zero_root_retires_at_source_end_without_a_physical_input() {
    let (mut rt, mut driver) = fixture("playNote(60,100,0)", false);
    driver.wake(&mut rt).unwrap();
    assert_eq!(rt.voice_count(), 1);
    rt.render(&mut [[0.; 2]; 256]).unwrap();
    rt.flush_ended(|_| true);
    assert_eq!(rt.voice_count(), 0);
    assert_eq!(
        rt.note_count(),
        0,
        "init-generated notes have no physical key-up owner"
    );
    let mut keys = [0; 128];
    rt.pressed_keys(&mut keys);
    assert_eq!(keys, [0; 128]);
}

#[test]
fn duration_zero_loop_waits_for_script_release_and_keeps_its_tail() {
    let (mut rt, mut driver) = fixture(
        "local voice; function onNote(e) voice=playNote(e.note,e.velocity,0) end; function onController(e) releaseVoice(voice) end",
        true,
    );
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    };
    let physical = rt.note_on(input, 60, 1.).unwrap();
    assert_eq!(rt.retire_when_silent(physical), Err(Error::InvalidInput));
    driver.note_on(&mut rt, physical, 60, 1.).unwrap();
    rt.render(&mut [[0.; 2]; 256]).unwrap();
    driver.note_off(&mut rt, 60).unwrap();
    rt.flush_ended(|_| true);
    assert_eq!(
        rt.voice_count(),
        1,
        "duration zero keeps a loop until its explicit release"
    );
    driver.input(
        &rt,
        HostInput::Controller {
            cc: 1,
            value: 0,
            channel: 0,
        },
    );
    driver.wake(&mut rt).unwrap();
    let mut tail = [[0.; 2]; 32];
    rt.render(&mut tail).unwrap();
    assert!(tail.iter().any(|frame| frame[0] > 0.));
    assert_eq!(rt.voice_count(), 1);
    rt.render(&mut [[0.; 2]; 256]).unwrap();
    rt.flush_ended(|_| true);
    assert_eq!((rt.voice_count(), rt.note_count()), (0, 0));
}

#[test]
fn player_flushes_source_owned_notes_after_eof() {
    let (rt, driver) = fixture("playNote(60,100,0)", false);
    let mut player = Player {
        rt,
        driver,
        horizon: None,
        _stream: None,
        feed: MidiFeed::default(),
    };
    player.render(&mut [[0.; 2]; 256]).unwrap();
    assert_eq!(
        (
            player.runtime().voice_count(),
            player.runtime().note_count()
        ),
        (0, 0)
    );
}

#[test]
fn w10_cold_keys_have_prepared_note_queues() {
    let (_, driver) = fixture("", false);
    for key in 0..128 {
        assert!(
            driver
                .held
                .get(&key)
                .is_some_and(|ids| ids.capacity() >= TRACKED),
            "key {key} must not allocate its first or overlapping held-note queue on the callback"
        );
    }
}

#[test]
fn w10_held_note_budget_returns_capacity_without_growing() {
    let (mut rt, mut driver) = fixture("", false);
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    };
    let note = rt.note_on(input, 60, 1.).unwrap();
    let held = driver.held.get_mut(&60).unwrap();
    held.extend(0..TRACKED as u64);
    let capacity = held.capacity();
    assert_eq!(driver.note_on(&mut rt, note, 60, 1.), Err(Error::Capacity));
    assert_eq!(driver.held[&60].capacity(), capacity);
    assert!(driver.notes.is_empty());
    rt.render(&mut [[0.; 2]; 128]).unwrap();
    rt.flush_ended(|_| true);
    assert_eq!(
        rt.note_count(),
        0,
        "rejected physical notes cannot remain held"
    );
}

#[test]
fn midi_release_pairs_the_physical_note_across_same_key_channels() {
    let (mut rt, mut driver) = fixture(
        "function onNote(e) playNote(e.note,e.velocity,-1) end",
        true,
    );
    let input = |channel| Input {
        protocol: Protocol::Midi1,
        port: 0,
        group: 0,
        channel,
        key: 60,
        external_id: None,
    };
    let first = rt.note_on(input(0), 60, 1.).unwrap();
    driver.note_on(&mut rt, first, 60, 1.).unwrap();
    let second = rt.note_on(input(1), 60, 1.).unwrap();
    driver.note_on(&mut rt, second, 60, 1.).unwrap();
    driver.note_off_note(&mut rt, second, 60).unwrap();
    assert!(
        rt.key_down(first).unwrap(),
        "another channel's same-key gate stays held"
    );
    assert!(!rt.key_down(second).unwrap());
    driver.note_off_note(&mut rt, first, 60).unwrap();
    assert!(!rt.key_down(first).unwrap());
    assert!(driver.held[&60].is_empty());
}

#[test]
fn normalized_physical_release_keeps_core_precision_and_script_velocity_projection() {
    let (mut rt, mut driver) = fixture(
        "function onNote(e) postEvent(e) end function onRelease(e) seen=e.velocity end",
        true,
    );
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    };
    let physical = rt.note_on(input, 60, 0.87654321).unwrap();
    driver.note_on(&mut rt, physical, 60, 0.87654321).unwrap();
    driver
        .note_off_note_velocity(&mut rt, physical, 60, Some(0.23456789))
        .unwrap();
    assert_eq!(
        rt.release_context(physical).unwrap().key.unwrap().velocity,
        Some(0.23456789)
    );
    assert_eq!(driver.host.global_text("seen"), "30");
}

#[test]
fn retired_parent_play_is_never_reinterpreted_as_a_generated_root() {
    let (mut rt, mut driver) = fixture("", true);
    let command = Play {
        id: 71,
        parent: Some(70),
        creator: crate::script::PlayCreator::NoteOn(70),
        origin: crate::script::PlayOrigin::Helper,
        at_ms: 0.,
        key: 60,
        velocity: 127,
        duration_ms: Some(0.),
        layers: Default::default(),
        osc: None,
        vol: 1.,
        pan: 0.,
        tune: 0.,
    };
    driver.play(&mut rt, &command, false).unwrap();
    assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
}

#[test]
fn script_note_callbacks_keep_the_original_physical_channel_and_overlapping_note_pairing() {
    use crate::script::MidiOut;
    let (mut rt, mut driver) = fixture(
        "function onNote(e) postEvent{type=Event.Controller,controller=20,value=e.channel-1,channel=e.channel} end function onRelease(e) postEvent{type=Event.Controller,controller=21,value=e.channel-1,channel=e.channel}; postEvent{type=Event.Controller,controller=22,value=e.velocity,channel=e.channel} end", true);
    let input = |id| Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(id) };
    let first = rt.note_on(input(1), 60, 1.).unwrap();
    rt.set_input_source(first, Input { protocol: Protocol::Clap, port: 3, channel: 13, ..input(1) }).unwrap();
    driver.note_on(&mut rt, first, 60, 1.).unwrap();
    let second = rt.note_on(input(2), 60, 1.).unwrap();
    rt.set_input_source(second, Input { protocol: Protocol::Vst3, port: 5, channel: 4, ..input(2) }).unwrap();
    driver.note_on(&mut rt, second, 60, 1.).unwrap();
    let mut midi = Vec::new(); driver.drain_midi(|m| midi.push(m));
    assert_eq!(midi, vec![MidiOut { status: 0xbd, a: 20, b: 13 }, MidiOut { status: 0xb4, a: 20, b: 4 }]);
    // A later adapter mapping cannot change the channel of an admitted callback.
    rt.set_input_source(first, Input { protocol: Protocol::Clap, channel: 2, ..input(1) }).unwrap();
    driver.note_off_note_velocity(&mut rt, second, 60, Some(53. / 127.)).unwrap();
    assert!(rt.input_held(first).unwrap()); assert!(!rt.input_held(second).unwrap());
    driver.note_off_note_velocity(&mut rt, first, 60, Some(17. / 127.)).unwrap();
    midi.clear(); driver.drain_midi(|m| midi.push(m));
    assert_eq!(midi, vec![MidiOut { status: 0xb4, a: 21, b: 4 }, MidiOut { status: 0xb4, a: 22, b: 53 },
        MidiOut { status: 0xbd, a: 21, b: 13 }, MidiOut { status: 0xbd, a: 22, b: 17 }]);
    assert!(driver.held[&60].is_empty());
}

#[test]
fn a_retired_core_note_keeps_its_physical_script_channel_until_the_real_keyup() {
    use crate::script::MidiOut;
    let (mut rt, mut driver) = fixture("function onNote(e) end function onRelease(e) postEvent{type=Event.Controller,controller=21,value=e.channel-1,channel=e.channel} end", true);
    let input = Input { protocol: Protocol::Midi2, port: 2, group: 3, channel: 15, key: 60, external_id: Some(91) };
    let physical = rt.note_on(input, 60, 1.).unwrap();
    driver.note_on(&mut rt, physical, 60, 1.).unwrap();
    rt.cancel_input_note(physical).unwrap(); rt.flush_ended(|_| true);
    assert_eq!(rt.note(physical), Err(Error::StaleHandle));
    driver.prune(&rt);
    driver.note_off_note(&mut rt, physical, 60).unwrap();
    let mut midi = Vec::new(); driver.drain_midi(|m| midi.push(m));
    assert_eq!(midi, vec![MidiOut { status: 0xbf, a: 21, b: 15 }]);
    assert!(driver.held[&60].is_empty()); assert_eq!(rt.note_count(), 0);
    driver.prune(&rt); assert!(driver.notes.is_empty());
}

#[test]
fn release_command_capacity_failure_cannot_leave_the_original_physical_key_down() {
    let (mut rt, mut driver) = fixture("function onNote(e) end function onRelease(e) for i=1,12 do playNote(e.note,e.velocity,0) end end", true);
    let input = Input { protocol: Protocol::Midi1, port: 2, group: 3, channel: 11, key: 60, external_id: Some(91) };
    let physical = rt.note_on(input, 60, 1.).unwrap();
    driver.note_on(&mut rt, physical, 60, 1.).unwrap();
    assert_eq!(driver.note_off_note(&mut rt, physical, 60), Err(Error::Capacity));
    assert!(!rt.input_held(physical).unwrap(), "real keyup must reach core even when an onRelease play exceeds capacity");
    assert!(driver.held[&60].is_empty());
}

#[test]
fn queued_script_owner_receives_the_same_captured_channel_and_release_velocity() {
    use crate::script::MidiOut;
    use std::time::{Duration, Instant};
    let (mut rt, _) = fixture("", true);
    let code = "function onNote(e) postEvent{type=Event.Controller,controller=20,value=e.channel-1,channel=e.channel} end function onRelease(e) postEvent{type=Event.Controller,controller=22,value=e.velocity,channel=e.channel} end";
    let xml = format!("<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{code}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>");
    let (thread, loaded) = ScriptThread::spawn(xml, (), crate::script::Config::default()).unwrap();
    let mut driver = Driver::new(thread, vec![], 48000);
    let input = Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(67) };
    let physical = rt.note_on(input, 60, 1.).unwrap();
    rt.set_input_source(physical, Input { protocol: Protocol::Clap, port: 7, channel: 14, ..input }).unwrap();
    driver.note_on(&mut rt, physical, 60, 1.).unwrap();
    rt.set_input_source(physical, Input { channel: 2, ..input }).unwrap();
    driver.note_off_note_velocity(&mut rt, physical, 60, Some(53. / 127.)).unwrap();
    let mut midi = Vec::new(); let deadline = Instant::now() + Duration::from_secs(3);
    while midi.len() < 2 {
        driver.wake(&mut rt).unwrap(); driver.drain_midi(|m| midi.push(m));
        assert!(Instant::now() < deadline, "ordinary queued script owner did not drain within fixture bound");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(midi, vec![MidiOut { status: 0xbe, a: 20, b: 14 }, MidiOut { status: 0xbe, a: 22, b: 53 }]);
    assert_eq!(loaded.ui.runtime_faults(), (0, 0));
    assert!(!matches!(rt.input_held(physical), Ok(true))); assert_eq!(rt.now(), 0);
}

#[test]
fn no_creator_negative_duration_releases_immediately_instead_of_becoming_manual() {
    for code in ["playNote(60,100,-1)", "function onInit() playNote(60,100,-1) end", "spawn(function() playNote(60,100,-1) end)"] {
        let (mut rt, mut driver)=fixture(code,true);
        driver.wake(&mut rt).unwrap();
        for _ in 0..8 { rt.render(&mut [[0.;2];128]).unwrap(); }
        rt.flush_ended(|_|true);
        assert_eq!((rt.voice_count(),rt.note_count()),(0,0),"{code}");
    }
}

#[test]
fn callback_spawn_is_manual_even_when_spawned_by_an_on_note_creator() {
    let (mut rt,mut driver)=fixture("function onNote(e) spawn(function() playNote(60,100,-1) end) end",true);
    let input=Input {protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:None};
    let physical=rt.note_on(input,60,1.).unwrap();
    driver.note_on(&mut rt,physical,60,1.).unwrap();
    driver.note_off_note(&mut rt,physical,60).unwrap();
    for _ in 0..8 {rt.render(&mut [[0.;2];128]).unwrap();}
    assert_eq!(rt.voice_count(),1,"spawn has a manual event creator, not the released onNote creator");
}

#[test]
fn direct_on_note_helper_follows_real_physical_release() {
    let (mut rt,mut driver)=fixture("function onNote(e) playNote(e.note,e.velocity,-1) end",true);
    let input=Input {protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:None};
    let physical=rt.note_on(input,60,1.).unwrap();
    driver.note_on(&mut rt,physical,60,1.).unwrap();
    rt.render(&mut [[0.;2];128]).unwrap();
    assert_eq!(rt.voice_count(),1);
    driver.note_off_note(&mut rt,physical,60).unwrap();
    for _ in 0..8 {rt.render(&mut [[0.;2];128]).unwrap();}
    rt.flush_ended(|_|true);
    assert_eq!((rt.voice_count(),rt.note_count()),(0,0));
}

#[test]
fn late_on_note_uses_a_real_pinned_closed_parent_and_releases_immediately() {
    // The canonical capture owner holds exactly this checked Core lease while
    // an external callback turn remains pending. This tests the branch with
    // that real lease; it does not claim legacy Driver enqueue supplies it.
    let (mut rt,mut driver)=fixture("function onNote(e) wait(10); playNote(e.note,e.velocity,-1) end",true);
    let input=Input {protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:None};
    let physical=rt.note_on(input,60,1.).unwrap();
    rt.pin(physical).unwrap();
    driver.note_on(&mut rt,physical,60,1.).unwrap();
    driver.note_off_note(&mut rt,physical,60).unwrap();
    rt.render(&mut [[0.;2];480]).unwrap();
    driver.wake(&mut rt).unwrap();
    assert!(rt.note(physical).is_ok(),"the real capture pin preserves the original identity");
    rt.unpin(physical).unwrap();
    for _ in 0..8 {rt.render(&mut [[0.;2];128]).unwrap();}
    rt.flush_ended(|_|true);
    assert_eq!((rt.voice_count(),rt.note_count()),(0,0));
}
