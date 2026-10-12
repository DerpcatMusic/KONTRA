//! Authored contracts recovered from preserved v1 root/velocity ownership.
use super::*;
use crate::sound::event::HostPattern;

fn core(script: Option<&str>) -> V2Core {
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[0.25; 2]; 4800].into_boxed_slice()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::new(0, 0, 0, 1., 256).unwrap(),
            playback: Playback::default(),
        }],
        8,
    )
    .unwrap();
    let plan = if let Some(source) = script {
        sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[])
            .unwrap()
            .bind(plan)
            .unwrap()
    } else {
        plan
    };
    let limits = Limits::for_plan(&plan, 16, 16);
    let part = Part::new(
        Runtime::new(plan, limits).unwrap(),
        MixTree::instrument("host contract"),
    )
    .unwrap();
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, Some(Box::new(part)));
    core
}
fn note(channel: u8, id: i32) -> HostNote {
    HostNote {
        port: 0,
        channel,
        key: 60,
        id,
        clap: true,
    }
}
fn on(note: HostNote) -> Event {
    Event::NoteOn {
        note,
        velocity: 0.87654321,
        tune: 0.,
    }
}
fn pattern(note: HostNote) -> HostPattern {
    HostPattern {
        port: 0,
        channel: i32::from(note.channel),
        key: 60,
        id: note.id,
        clap: note.clap,
    }
}

#[test]
fn physical_host_tuples_do_not_alias_when_routed_channels_collapse() {
    let mut core = core(None);
    let (a, b) = (note(1, 7), note(2, 7));
    let mut exercise = || {
        core.event(0, on(a));
        core.event(0, on(b));
        assert_eq!(
            core.held.len(),
            2,
            "same host ID on distinct channels is independent"
        );
        assert_ne!(core.held[0].input, core.held[1].input);
        core.event(0, Event::Expression(pattern(a), NoteExpression::Gain(0.25)));
        let rt = &core.parts[0].as_ref().unwrap().runtime;
        let gain = |index: usize| {
            rt.expression(rt.expression_id(core.held[index].id).unwrap())
                .unwrap()
                .gain
        };
        let expressions = [gain(0), gain(1)];
        assert_eq!(expressions, [0.25, 1.]);
        core.event(0, Event::NoteOffVelocity(pattern(a), 0.23456789));
        let rt = &core.parts[0].as_ref().unwrap().runtime;
        assert!(!rt.input_held(core.held[0].id).unwrap());
        assert!(rt.input_held(core.held[1].id).unwrap());
    };
    #[cfg(feature = "plugin")]
    assert_eq!(crate::plugin::tests::allocations(|| exercise()), 0);
    #[cfg(not(feature = "plugin"))]
    exercise();
}

#[test]
fn anonymous_host_starts_stack_but_duplicate_named_tuples_remain_owned() {
    let mut core = core(None);
    let anonymous = note(0, -1);
    for _ in 0..3 {
        core.event(0, on(anonymous));
    }
    assert_eq!(core.held.len(), 3);
    let named = note(0, 8);
    core.event(0, on(named));
    core.event(0, on(named));
    assert_eq!(
        core.held.len(),
        4,
        "duplicate named tuple cannot acquire a second owner"
    );
    core.event(0, Event::NoteOffVelocity(pattern(named), 0.25));
    // Core renders at most MAX_BLOCK frames per call. A 256-frame tail
    // therefore spans two host blocks; it must remain owned halfway through.
    core.render(MAX_BLOCK);
    assert_eq!(core.parts[0].as_ref().unwrap().runtime.now(), 128);
    assert_eq!(core.voices().active, 4);
    let mut deliveries = 0;
    assert_eq!(
        core.end_block(MAX_BLOCK, &mut |_| {
            deliveries += 1;
            false
        }),
        0
    );
    assert_eq!(deliveries, 0, "a sounding release has no NOTE_END");
    core.render(MAX_BLOCK);
    assert_eq!(core.parts[0].as_ref().unwrap().runtime.now(), 256);
    assert_eq!(core.voices().active, 3);
    assert_eq!(
        core.end_block(MAX_BLOCK, &mut |note| {
            assert_eq!(note, named);
            deliveries += 1;
            false
        }),
        1
    );
    assert_eq!(deliveries, 1);
    core.event(0, on(named));
    assert_eq!(
        core.held.len(),
        4,
        "unaccepted NOTE_END still owns the full tuple"
    );
    let mut ended = 0;
    assert_eq!(
        core.end_block(0, &mut |n| {
            assert_eq!(n, named);
            ended += 1;
            true
        }),
        0
    );
    assert_eq!(ended, 1);
    core.end_block(0, &mut |_| {
        ended += 1;
        true
    });
    assert_eq!(ended, 1, "accepted NOTE_END is delivered exactly once");
    core.event(0, on(named));
    assert_eq!(
        core.held.len(),
        4,
        "named tuple can be reused after terminal acceptance"
    );
}

#[test]
fn host_release_velocity_reaches_physical_context_and_ksp_release_metadata() {
    let mut core = core(Some(
        "on init declare ui_knob $released(0,127,1) end on on release $released := get_event_par($EVENT_ID,$EVENT_PAR_REL_VELOCITY) end on",
    ));
    let host = note(0, 9);
    core.event(0, on(host));
    let root = core.held[0].id;
    core.event(0, Event::NoteOffVelocity(pattern(host), 0.23456789));
    let rt = &core.parts[0].as_ref().unwrap().runtime;
    assert_eq!(
        rt.release_context(root).unwrap().key.unwrap().velocity,
        Some(0.23456789)
    );
    core.render(16);
    let knob = sampler_ui_ir::ControlId(sampler_ksp::derived_control_id(0, "$released").0);
    assert_eq!(core.control_value(0, knob), Some(30.));
}

#[test]
fn invalid_normalized_host_release_cannot_close_a_physical_owner() {
    for value in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
        let mut core = core(None);
        let host = note(0, 10);
        core.event(0, on(host));
        core.event(0, Event::NoteOffVelocity(pattern(host), value));
        let rt = &core.parts[0].as_ref().unwrap().runtime;
        assert!(rt.input_held(core.held[0].id).unwrap());
        assert!(rt.release_context(core.held[0].id).unwrap().key.is_none());
    }
}

#[test]
fn concrete_negative_vst3_ids_are_distinct_from_anonymous_starts() {
    let mut core = core(None);
    let note = HostNote {
        clap: false,
        ..note(0, -2)
    };
    core.event(0, on(note));
    core.event(0, on(note));
    assert_eq!(core.held.len(), 1, "VST3 reserves only -1 as anonymous");
    assert!(core.owns(note));
}

#[test]
fn exact_host_choke_stops_only_the_matched_owner_without_a_release_callback() {
    let mut core = core(Some(
        "on init declare ui_knob $released(0,127,1) end on on release inc($released) end on",
    ));
    let (a, b) = (note(0, 21), note(0, 22));
    core.event(0, on(a));
    core.event(0, on(b));
    core.event(0, Event::Choke(pattern(a)));
    assert_eq!(
        core.voices().active,
        1,
        "CHOKE must stop the matched tail immediately"
    );
    let rt = &core.parts[0].as_ref().unwrap().runtime;
    assert!(!rt.input_held(core.held[0].id).unwrap());
    assert!(rt.input_held(core.held[1].id).unwrap());
    assert_eq!(
        rt.release_context(core.held[0].id)
            .unwrap()
            .key
            .unwrap()
            .velocity,
        None
    );
    core.render(16);
    let knob = sampler_ui_ir::ControlId(sampler_ksp::derived_control_id(0, "$released").0);
    assert_eq!(
        core.control_value(0, knob),
        Some(0.),
        "CHOKE is not a physical release callback"
    );
    let mut ended = 0;
    assert_eq!(
        core.end_block(16, &mut |note| {
            assert_eq!(note, a);
            ended += 1;
            true
        }),
        0
    );
    assert_eq!(ended, 1);
    assert!(core.owns(b));
}

#[test]
fn exact_host_choke_cancels_deferred_note_work_before_it_can_write_or_restart() {
    let mut core = core(Some(
        "on init declare ui_knob $late(0,127,1) end on on note wait(100000) inc($late) end on",
    ));
    let host = note(0, 23);
    core.event(0, on(host));
    core.event(0, Event::Choke(pattern(host)));
    let mut ended = 0;
    for _ in 0..64 {
        core.render(MAX_BLOCK);
        core.end_block(MAX_BLOCK, &mut |note| {
            assert_eq!(note, host);
            ended += 1;
            true
        });
    }
    assert_eq!(core.parts[0].as_ref().unwrap().runtime.now(), 8192);
    assert_eq!(ended, 1);
    let knob = sampler_ui_ir::ControlId(sampler_ksp::derived_control_id(0, "$late").0);
    assert_eq!(core.control_value(0, knob), Some(0.));
    assert_eq!(core.voices().active, 0);
    assert!(!core.owns(host));
}
