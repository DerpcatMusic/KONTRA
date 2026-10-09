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
        assert!(driver.held.get(&key).is_some_and(|ids| ids.capacity() >= TRACKED),
            "key {key} must not allocate its first or overlapping held-note queue on the callback");
    }
}

#[test]
fn w10_held_note_budget_returns_capacity_without_growing() {
    let (mut rt, mut driver) = fixture("", false);
    let input = Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: None };
    let note = rt.note_on(input, 60, 1.).unwrap();
    let held = driver.held.get_mut(&60).unwrap();
    held.extend(0..TRACKED as u64);
    let capacity = held.capacity();
    assert_eq!(driver.note_on(&mut rt, note, 60, 1.), Err(Error::Capacity));
    assert_eq!(driver.held[&60].capacity(), capacity);
    assert!(driver.notes.is_empty());
    rt.render(&mut [[0.; 2]; 128]).unwrap();
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0, "rejected physical notes cannot remain held");
}

#[test]
fn midi_release_pairs_the_physical_note_across_same_key_channels() {
    let (mut rt, mut driver) = fixture("function onNote(e) playNote(e.note,e.velocity,-1) end", true);
    let input = |channel| Input { protocol: Protocol::Midi1, port: 0, group: 0, channel, key: 60, external_id: None };
    let first = rt.note_on(input(0), 60, 1.).unwrap();
    driver.note_on(&mut rt, first, 60, 1.).unwrap();
    let second = rt.note_on(input(1), 60, 1.).unwrap();
    driver.note_on(&mut rt, second, 60, 1.).unwrap();
    driver.note_off_note(&mut rt, second, 60).unwrap();
    assert!(rt.key_down(first).unwrap(), "another channel's same-key gate stays held");
    assert!(!rt.key_down(second).unwrap());
    driver.note_off_note(&mut rt, first, 60).unwrap();
    assert!(!rt.key_down(first).unwrap());
    assert!(driver.held[&60].is_empty());
}
