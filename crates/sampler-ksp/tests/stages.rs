use sampler_core::*;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn compile(source: &str) -> sampler_ksp::Script {
    sampler_ksp::compile(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 8192,
            instructions: 512,
            variables: 16,
            array_cells: 16,
        },
        &[],
    )
    .unwrap()
}

fn plan(sources: &[&str]) -> Prepared {
    let samples = [0.125, 0.25].map(|value| Pcm::new(48000, Box::from([[value; 2]; 32])).unwrap());
    let regions = (0..2)
        .map(|sample| Region {
            sample,
            key_low: 0,
            key_high: 127,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        })
        .collect();
    sampler_ksp::bind_modules(
        sources.iter().map(|source| compile(source)).collect(),
        Prepared::new(48000, samples.into(), regions, 256)
            .unwrap()
            .with_groups(2, vec![Some(0), Some(1)])
            .unwrap(),
    )
    .unwrap()
}

fn limits(plan: &Prepared, behaviors: usize) -> Limits {
    Limits {
        notes: 8,
        performances: 1,
        channels: 1,
        families: 8,
        voices: 8,
        expressions: 8,
        decisions: 0,
        commands: 16,
        behaviors,
        behavior_cells: plan.behavior_local_count() * behaviors,
        behavior_fuel: 1024,
        note_cells: plan.note_cell_count() * 8,
    }
}
fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}

#[test]
fn note_modules_forward_copies_and_late_edits_keep_local_properties_and_groups() {
    let plan = plan(&[
        "on init declare $seen end on on note
            change_note($EVENT_ID,61)
            disallow_group($ALL_GROUPS) allow_group(0)
            wait(125)
            change_note($EVENT_ID,70)
            disallow_group($ALL_GROUPS) allow_group(1)
            $seen := $EVENT_NOTE
        end on",
        "on init declare $unused := 9 end on",
        "on init declare $seen end on on note
            $seen := $EVENT_NOTE
            change_note($EVENT_ID,62)
            disallow_group($ALL_GROUPS) allow_group(1)
            wait(250)
            $seen := $EVENT_NOTE
        end on",
    ]);
    let budget = limits(&plan, 2);
    let mut rt = Runtime::new(plan, budget).unwrap();
    support::without_heap(|| {
        let note = rt.trigger(input(1), 60, 1.).unwrap();
        assert_eq!(rt.note_pitch(note), Ok(NotePitch::Key(62)));
        assert_eq!(
            rt.note_event_at(note, 0).unwrap().unwrap().pitch,
            NotePitch::Key(61)
        );
        assert_eq!(
            rt.note_event_at(note, 1).unwrap().unwrap().pitch,
            NotePitch::Key(61)
        );
        assert_eq!(
            rt.note_event_at(note, 2).unwrap().unwrap().pitch,
            NotePitch::Key(62)
        );
        assert_eq!(rt.voice_count(), 1);
        let mut audio = [[0.; 2]; 8];
        rt.render(&mut audio).unwrap();
        assert!(audio.iter().all(|frame| *frame == [0.25; 2]));
        assert_eq!(
            rt.note_event_at(note, 0).unwrap().unwrap().pitch,
            NotePitch::Key(70)
        );
        assert_eq!(
            rt.note_event_at(note, 2).unwrap().unwrap().pitch,
            NotePitch::Key(62)
        );
        assert!(rt.note_group_allowed_at(note, 0, 0).unwrap());
        assert!(!rt.note_group_allowed_at(note, 2, 0).unwrap());
        assert!(rt.note_group_allowed_at(note, 2, 1).unwrap());
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
            Ok(70)
        );
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(2), 0),
            Ok(62)
        );
        assert_eq!(rt.note_off(input(1), None), Ok(note));
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
    });
}

#[test]
fn generated_notes_enter_following_modules_and_keep_the_creating_event_view() {
    let plan = plan(&[
        "on init declare $child declare $calls end on on note
            inc($calls)
            ignore_event($EVENT_ID)
            disallow_group($ALL_GROUPS) allow_group(1)
            $child := play_note(64,127,0,1000)
            change_note($child,71)
        end on",
        "on init declare $calls declare $seen end on on note
            inc($calls)
            $seen := $EVENT_NOTE
            change_note($EVENT_ID,65)
        end on",
    ]);
    let budget = limits(&plan, 2);
    let mut rt = Runtime::new(plan, budget).unwrap();
    support::without_heap(|| {
        let parent = rt.trigger(input(1), 60, 1.).unwrap();
        let generation = rt.active_plan();
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), 1), Ok(1));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 0), Ok(1));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 1), Ok(64));
        let alias = rt.script_cell(generation, ScriptInstanceId(0), 0).unwrap() as i32;
        let child = rt.resolve_source_event(generation, alias).unwrap().unwrap();
        assert_eq!(
            rt.note_event_at(child, 0).unwrap().unwrap().pitch,
            NotePitch::Key(71)
        );
        assert_eq!(
            rt.note_event_at(child, 1).unwrap().unwrap().pitch,
            NotePitch::Key(65)
        );
        assert_eq!(rt.note_pitch(child), Ok(NotePitch::Key(65)));
        assert_eq!(rt.note_event_at(parent, 1), Ok(None));
        assert_eq!(rt.forward_attack_at(parent, 1), Err(Error::InvalidInput));
        let mut audio = [[0.; 2]; 4];
        rt.render(&mut audio).unwrap();
        assert!(audio.iter().all(|frame| *frame == [0.25; 2]));
        rt.note_off(input(1), None).unwrap();
        assert!(rt.key_down(child).unwrap());
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
    });
}

#[test]
fn generated_notes_and_controllers_cross_the_same_module_boundaries() {
    let plan = plan(&[
        "on init declare $child end on on controller
            ignore_controller
            set_controller(2,%CC[1])
            $child := play_note(64,127,0,1000)
        end on",
        "on init declare $unused end on",
        "on init declare $raw declare $remap end on on note
            $raw := %CC[1]
            $remap := %CC[2]
            set_controller(3,$remap)
        end on",
        "on init declare $number declare $value declare $calls end on on controller
            $number := $CC_NUM
            $value := %CC[$CC_NUM]
            inc($calls)
        end on",
    ]);
    let budget = limits(&plan, 4);
    let mut rt = Runtime::new(plan, budget).unwrap();
    support::without_heap(|| {
        let generation = rt.active_plan();
        let domain = rt.performance(0).unwrap();
        rt.dispatch_controller(domain, input(1).channel_address(), 1, 1, u32::MAX)
            .unwrap();
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(2), 0), Ok(0));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(2), 1), Ok(127));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(3), 0), Ok(3));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(3), 1), Ok(127));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(3), 2), Ok(2));
        assert_eq!(rt.input_controller(domain, 3), Ok(0));
        assert_eq!(rt.controller(domain, 3), Ok(u32::MAX));
        assert_eq!(rt.note_count(), 1);
        assert_eq!(rt.voice_count(), 2);
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
    });
}

#[test]
fn release_group_edits_and_generated_children_do_not_overwrite_a_waiting_note_view() {
    let plan = plan(&["on init declare $child end on
        on note
            disallow_group($ALL_GROUPS) allow_group(0)
            wait(1000)
        end on
        on release
            ignore_event($EVENT_ID)
            disallow_group($ALL_GROUPS) allow_group(1)
            wait(125)
            $child := play_note(64,127,0,1000)
            note_off($EVENT_ID)
        end on"]);
    let budget = limits(&plan, 2);
    let mut rt = Runtime::new(plan, budget).unwrap();
    support::without_heap(|| {
        let parent = rt.trigger(input(1), 60, 1.).unwrap();
        rt.note_off(input(1), None).unwrap();
        assert!(rt.note_group_allowed(parent, 0).unwrap());
        assert!(!rt.note_group_allowed(parent, 1).unwrap());
        let mut audio = [[0.; 2]; 8];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio[0], [0.125; 2]);
        assert_eq!(audio[7], [0.25; 2]);
        let generation = rt.active_plan();
        let alias = rt.script_cell(generation, ScriptInstanceId(0), 0).unwrap() as i32;
        let child = rt.resolve_source_event(generation, alias).unwrap().unwrap();
        assert!(!rt.note_group_allowed(child, 0).unwrap());
        assert!(rt.note_group_allowed(child, 1).unwrap());
        assert!(rt.note_group_allowed(parent, 0).unwrap());
        rt.flush_behaviors(|_, _, outcome| {
            assert!(matches!(outcome, Outcome::Finished | Outcome::Cancelled));
            true
        });
    });
}

#[test]
fn release_modules_hold_independently_and_note_held_tracks_the_incoming_stage() {
    let plan = plan(&[
        "on init declare $released end on
        on note change_note($EVENT_ID,61) end on
        on release
            ignore_event($EVENT_ID)
            wait(125)
            note_off($EVENT_ID)
            $released := 1
        end on",
        "on init declare $held declare $seen declare $released end on
        on note
            change_note($EVENT_ID,62)
            wait(63)
            $held := $NOTE_HELD
        end on
        on release
            $seen := $EVENT_NOTE
            $released := $NOTE_HELD
            ignore_event($EVENT_ID)
            disallow_group($ALL_GROUPS) allow_group(1)
            wait(125)
            play_note(64,127,0,1000)
            note_off($EVENT_ID)
        end on",
        "on init declare $releases end on on release inc($releases) end on",
    ]);
    let budget = limits(&plan, 6);
    let mut rt = Runtime::new(plan, budget).unwrap();
    support::without_heap(|| {
        let note = rt.trigger(input(1), 60, 1.).unwrap();
        let generation = rt.active_plan();
        rt.note_off(input(1), None).unwrap();
        assert!(!rt.key_down(note).unwrap());
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 0), Ok(1));
        assert!(rt.release_context(note).unwrap().gate.is_none());
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), 0), Ok(1));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 1), Ok(62));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 2), Ok(0));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(2), 0), Ok(0));
        assert!(rt.release_context(note).unwrap().gate.is_none());
        let mut audio = [[0.; 2]; 6];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio[0], [0.375; 2]);
        assert_eq!(audio[5], [0.25; 2]);
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(2), 0), Ok(1));
        assert!(rt.release_context(note).unwrap().gate.is_some());
        assert!(rt.note_group_allowed_at(note, 1, 0).unwrap());
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
    });
}

#[test]
fn consumed_notes_release_only_reached_modules_and_generated_notes_skip_the_creator() {
    let plan = plan(&[
        "on init declare $releases end on
        on note ignore_event($EVENT_ID) play_note(64,127,0,125) end on
        on release inc($releases) end on",
        "on init declare $notes declare $releases end on
        on note inc($notes) end on
        on release inc($releases) end on",
    ]);
    let budget = limits(&plan, 4);
    let mut rt = Runtime::new(plan, budget).unwrap();
    support::without_heap(|| {
        let parent = rt.trigger(input(1), 60, 1.).unwrap();
        let generation = rt.active_plan();
        assert_eq!(rt.note_event_at(parent, 1), Ok(None));
        rt.note_off(input(1), None).unwrap();
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), 0), Ok(1));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 0), Ok(1));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 1), Ok(0));
        rt.render(&mut [[0.; 2]; 7]).unwrap();
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), 0), Ok(1));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 1), Ok(1));
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        rt.flush_ended(|_| true);
        assert_eq!(
            (rt.note_count(), rt.voice_count(), rt.pending_commands()),
            (0, 0, 0)
        );
    });
}
