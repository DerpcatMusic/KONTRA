#[test]
fn first_event_command_is_admitted_in_init_and_callbacks() {
    sampler_ksp::compile(
        "on init declare $track := -1 mf_get_first($track) end on
         on note mf_get_first($track) end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
}

#[test]
fn first_selects_the_earliest_track_or_whole_file_event_in_init_and_live_callbacks_without_heap() {
    let source = "on init declare $track := 1 mf_get_first($track)
        declare $initial := mf_get_id() declare $all declare $missing end on
        on note mf_get_first(-1) $all := mf_get_id()
        mf_get_first(99) $missing := mf_get_id() end on";
    let mut rt = runtime(&[source], object());
    let plan = rt.active_plan();
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(103));
    support::without_heap(|| {
        note(&mut rt);
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(102));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 3), Ok(-1));
        assert_eq!(rt.take_fault(), None);
    });
    let empty = runtime(&[source], MidiObject::default());
    assert_eq!(
        empty.script_cell(empty.active_plan(), ScriptInstanceId(0), 1),
        Ok(-1)
    );
}

#[test]
fn cursor_is_shared_by_script_slots_in_initialization_and_native_stage_dispatch_without_heap() {
    let mut rt = runtime(
        &[
            "on init mf_get_first(1) end on on note mf_get_first(0) end on",
            "on init declare $initial := mf_get_id() declare $live end on on note $live := mf_get_id() end on",
        ],
        object(),
    );
    let plan = rt.active_plan();
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(1), 0), Ok(103));
    support::without_heap(|| {
        note(&mut rt);
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(1), 1), Ok(102));
        assert_eq!(rt.take_fault(), None);
    });
}

#[test]
fn midi_event_identity_and_position_validation_reject_invalid_objects() {
    for event in [
        MidiObjectEvent {
            id: -1,
            track: 0,
            position: 0,
            ..Default::default()
        },
        MidiObjectEvent {
            id: 1,
            track: -1,
            position: 0,
            ..Default::default()
        },
        MidiObjectEvent {
            id: 1,
            track: 0,
            position: -1,
            ..Default::default()
        },
    ] {
        assert_eq!(MidiObject::new(vec![event]), Err(Error::InvalidInput));
    }
    let event = MidiObjectEvent {
        id: 1,
        track: 0,
        position: 0,
        ..Default::default()
    };
    assert_eq!(
        MidiObject::new(vec![event, event]),
        Err(Error::InvalidInput)
    );
}
use sampler_core::*;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn object() -> MidiObject {
    MidiObject::new(vec![
        MidiObjectEvent {
            id: 101,
            track: 1,
            position: 400,
            ..Default::default()
        },
        MidiObjectEvent {
            id: 102,
            track: 0,
            position: 32,
            ..Default::default()
        },
        MidiObjectEvent {
            id: 103,
            track: 1,
            position: 32,
            ..Default::default()
        },
        MidiObjectEvent {
            id: 104,
            track: 0,
            position: 128,
            ..Default::default()
        },
    ])
    .unwrap()
}

fn runtime(sources: &[&str], mut object: MidiObject) -> Runtime {
    let scripts = sources
        .iter()
        .enumerate()
        .map(|(slot, source)| {
            let env = sampler_ksp::Environment {
                slot: slot as u8,
                midi_object: object.clone(),
                ..Default::default()
            };
            let initialized =
                sampler_ksp::initialize(source, sampler_ksp::Limits::LIBRARY, &env).unwrap();
            object = initialized.midi_object().clone();
            sampler_ksp::compile_initialized(
                source,
                48000,
                sampler_ksp::Limits::LIBRARY,
                &[],
                initialized,
            )
            .unwrap()
        })
        .collect();
    let plan = sampler_ksp::bind_modules(scripts, Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    Runtime::new(plan, limits).unwrap()
}

fn note(rt: &mut Runtime) {
    rt.trigger(
        Input {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: None,
        },
        60,
        1.,
    )
    .unwrap();
}

#[test]
fn documented_midi_behavior_groups_are_admitted() {
    sampler_ksp::compile(
        r#"on init
      declare $id declare $value declare @name
      mf_get_first(-1) mf_get_next(-1) mf_get_next_at(-1, 0)
      mf_get_prev(-1) mf_get_prev_at(-1, 100) mf_get_last(-1)
      $value := mf_get_command() $value := mf_get_byte_one()
      $value := mf_get_byte_two() $value := mf_get_channel()
      $value := mf_get_pos() $value := mf_get_length()
      $value := mf_get_track_idx() $value := mf_get_num_tracks()
      $value := mf_get_event_par($CURRENT_EVENT, $EVENT_PAR_POS)
      mf_set_event_par($CURRENT_EVENT, $EVENT_PAR_POS, 20)
      mf_set_mark($ALL_EVENTS, $MARK_1, 1)
      $value := mf_get_mark(by_track(0), $MARK_1)
      mf_set_buffer_size(2)
      $id := mf_insert_event(0, 0, $MIDI_COMMAND_NOTE_ON, 60, 100)
      mf_remove_event($id) $value := mf_get_buffer_size()
      mf_set_num_export_areas(2)
      $value := mf_set_export_area("test", -1, -1, -1, -1)
      mf_copy_export_area(1) @name := mf_get_last_filename()
    end on"#,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
}

#[test]
fn navigation_getters_and_edits_share_the_live_object_without_heap() {
    let source = r#"on init
      declare $a declare $b declare $c declare $d declare $e
      declare $f declare $g declare $h declare $i declare $j
    end on
    on note
      mf_get_first(1) $a:=mf_get_id()
      mf_get_next(-1) $b:=mf_get_id()
      mf_get_last(-1) $c:=mf_get_id()
      mf_get_prev(-1) $d:=mf_get_id()
      mf_get_next_at(-1,32) $e:=mf_get_id()
      mf_get_prev_at(-1,128) $f:=mf_get_id()
      mf_set_pos(500) $g:=mf_get_pos()
      mf_set_command($MIDI_COMMAND_CC) mf_set_byte_one(7)
      mf_set_byte_two(99) mf_set_channel(3) mf_set_length(12)
      mf_set_track_idx(2)
      $h:=mf_get_event_par($CURRENT_EVENT,$EVENT_PAR_MIDI_BYTE_2)
      $i:=mf_get_command() $j:=mf_get_num_tracks()
    end on"#;
    let mut rt = runtime(&[source], object());
    let plan = rt.active_plan();
    support::without_heap(|| {
        note(&mut rt);
        assert_eq!(rt.take_fault(), None);
    });
    for (cell, value) in [103, 104, 101, 104, 104, 103, 500, 99, 176, 3]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            rt.script_cell(plan, ScriptInstanceId(0), cell as u32),
            Ok(value)
        );
    }
}

#[test]
fn buffer_insert_remove_marks_and_export_validation_work_in_init_and_live_without_heap() {
    let source = r#"on init
      mf_set_buffer_size(3) declare $id:=mf_insert_event(0,10,$MIDI_COMMAND_NOTE_ON,60,100)
      mf_set_event_par($id,$EVENT_PAR_NOTE_LENGTH,20)
      declare $before:=mf_get_buffer_size() declare $marked declare $after
      declare $invalid declare $valid
      mf_set_num_export_areas(2)
    end on on note
      mf_set_mark(by_track(0),$MARK_1,1)
      $marked:=mf_get_mark($id,$MARK_1)
      mf_set_event_par(by_marks($MARK_1),$EVENT_PAR_MIDI_BYTE_2,300)
      $invalid:=mf_set_export_area("invalid",-1,-1,-1,-1)
      mf_set_event_par($ALL_EVENTS,$EVENT_PAR_MIDI_BYTE_2,90)
      $valid:=mf_set_export_area("valid",100,0,2,0)
      mf_copy_export_area(1)
      mf_remove_event(by_marks($MARK_1)) $after:=mf_get_buffer_size()
    end on"#;
    let mut rt = runtime(&[source], MidiObject::default());
    let plan = rt.active_plan();
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(2));
    support::without_heap(|| {
        note(&mut rt);
        assert_eq!(rt.take_fault(), None);
    });
    for (cell, value) in [(2, 1), (3, 3), (4, 1), (5, 0)] {
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), cell), Ok(value));
    }
}

#[test]
fn async_buffer_reset_completion_is_slot_owned_retained_across_wait_and_cancelled_on_panic() {
    let source = r#"on init declare $job declare $seen declare $status declare $late end on
      on note $job:=mf_set_buffer_size(2) end on
      on async_complete $seen:=$NI_ASYNC_ID $status:=$NI_ASYNC_EXIT_STATUS
        wait(1000) $late:=$NI_ASYNC_ID end on"#;
    let mut rt = runtime(&[source], MidiObject::default());
    let plan = rt.active_plan();
    let (new_rt, mut client) = rt.with_control_updates(2, 1).unwrap();
    rt = new_rt;
    let mut effects = Vec::new();
    note(&mut rt);
    rt.drain_effects(|e| {
        effects.push(*e);
        true
    });
    let effect = effects.pop().unwrap();
    assert_eq!(effect.service, MIDI_SERVICE);
    let output = MidiCompletion::empty(effect.args[1] as i32, ScriptInstanceId(0));
    client
        .submit(ControlRequest {
            plan,
            expected_revision: None,
            operation: ControlOperation::MidiComplete(output),
        })
        .unwrap();
    support::without_heap(|| {
        assert!(rt.poll_control_update().unwrap().is_some());
        assert_eq!(rt.take_fault(), None);
    });
    assert!(client.reply().unwrap().result.is_ok());
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(1));
    support::without_heap(|| {
        rt.render(&mut [[0.; 2]; 49]).unwrap();
    });
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 3), Ok(1));
    note(&mut rt);
    rt.drain_effects(|e| {
        effects.push(*e);
        true
    });
    let effect = effects.pop().unwrap();
    support::without_heap(|| rt.panic());
    let mut cancelled = MidiCompletion::empty(effect.args[1] as i32, ScriptInstanceId(0));
    assert_eq!(
        rt.complete_midi(plan, &mut cancelled),
        Err(Error::StaleHandle)
    );
}

#[test]
fn midi_files_pair_note_lengths_preserve_empty_tracks_and_merge_replace_without_audio_io() {
    let path = std::env::temp_dir().join(format!("kontra-w5-midi-{}.mid", std::process::id()));
    let mut bytes = b"MThd\0\0\0\x06\0\x01\0\x02\0\x60MTrk\0\0\0\x0c".to_vec();
    bytes.extend_from_slice(&[0, 0x93, 60, 100, 96, 0x83, 60, 0, 0, 0xff, 0x2f, 0]);
    bytes.extend_from_slice(b"MTrk\0\0\0\x04\0\xff\x2f\0");
    std::fs::write(&path, &bytes).unwrap();
    let source = format!(
        r#"on init
        declare $ok:=mf_insert_file("{}",0,0,0)
        mf_get_first(-1) declare $length:=mf_get_length()
        declare $channel:=mf_get_channel() declare $tracks:=mf_get_num_tracks()
        declare @name:=mf_get_last_filename()
        declare $job declare $status declare $live
      end on on note $job:=mf_insert_file("{}",1,200,2) end on
      on async_complete $status:=$NI_ASYNC_EXIT_STATUS
        mf_get_last(-1) $live:=mf_get_pos() end on"#,
        path.display(),
        path.display()
    );
    let mut rt = runtime(&[&source], MidiObject::default());
    let plan = rt.active_plan();
    for (cell, value) in [(0, 1), (1, 96), (2, 3), (3, 2)] {
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), cell), Ok(value));
    }
    assert_eq!(
        rt.script_text(plan, ScriptInstanceId(0), 0)
            .unwrap()
            .as_str(),
        path.file_name().unwrap().to_str().unwrap()
    );
    note(&mut rt);
    let mut effects = Vec::new();
    rt.drain_effects(|e| {
        effects.push(*e);
        true
    });
    let effect = effects.pop().unwrap();
    let mut payload = MidiCompletion::empty(effect.args[1] as i32, ScriptInstanceId(0));
    payload.read_file(effect.text.unwrap().as_str());
    assert!(payload.success);
    support::without_heap(|| rt.complete_midi(plan, &mut payload).unwrap());
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 5), Ok(1));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 6), Ok(200));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn midi_instruction_stays_within_the_shared_code_size_budget() {
    assert!(std::mem::size_of::<Instruction>() <= 32);
}

#[test]
fn reset_requested_in_init_completes_asynchronously_after_plan_activation() {
    let source = "on init mf_set_buffer_size(1) declare $event:=mf_insert_event(0,0,144,60,100) declare $job:=mf_reset() declare $status declare $empty end on on async_complete $status:=$NI_ASYNC_EXIT_STATUS $empty:=mf_get_buffer_size() end on";
    let mut rt = runtime(&[source], MidiObject::default());
    let plan = rt.active_plan();
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
    support::without_heap(|| rt.render(&mut [[0.; 2]; 1]).unwrap());
    let mut effect = None;
    rt.drain_effects(|e| {
        effect = Some(*e);
        true
    });
    let effect = effect.unwrap();
    let mut output = MidiCompletion::empty(effect.args[1] as i32, ScriptInstanceId(0));
    support::without_heap(|| rt.complete_midi(plan, &mut output).unwrap());
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(1));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 3), Ok(0));
    assert_eq!(rt.complete_midi(plan, &mut output), Err(Error::StaleHandle));
}

#[test]
fn wait_async_waits_for_midi_jobs_and_invalid_ids_continue_in_init_and_runtime() {
    let source = "on init mf_set_buffer_size(1) declare $job:=mf_reset() declare $status declare $continued wait_async($job) declare $empty:=mf_get_buffer_size() end on on note $job:=mf_set_buffer_size(2) wait_async($job) $continued:=mf_get_buffer_size() wait_async(-1) end on on async_complete $status:=$NI_ASYNC_EXIT_STATUS end on";
    let mut rt = runtime(&[source], MidiObject::default());
    let plan = rt.active_plan();
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 3), Ok(0));
    support::without_heap(|| note(&mut rt));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(0));
    let mut effect = None;
    rt.drain_effects(|e| {
        effect = Some(*e);
        true
    });
    let effect = effect.unwrap();
    let mut output = MidiCompletion::empty(effect.args[1] as i32, ScriptInstanceId(0));
    support::without_heap(|| rt.complete_midi(plan, &mut output).unwrap());
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(2));
    assert_eq!(rt.take_fault(), None);
}

#[test]
fn recursive_initial_async_completion_is_bounded() {
    let error=sampler_ksp::initialize("on init declare $job:=mf_reset() wait_async($job) end on on async_complete $job:=mf_reset() wait_async($job) end on",sampler_ksp::Limits::LIBRARY,&Default::default()).err().unwrap();
    assert_eq!(error.message, "async completion nesting limit");
}

#[test]
fn file_export_keeps_empty_tracks_and_malformed_files_fail_explicitly() {
    let path = std::env::temp_dir().join(format!(
        "kontra-w5-midi-roundtrip-{}.mid",
        std::process::id()
    ));
    let bytes =
        b"MThd\0\0\0\x06\0\x01\0\x02\0\x60MTrk\0\0\0\x04\0\xff\x2f\0MTrk\0\0\0\x04\0\xff\x2f\0";
    std::fs::write(&path, bytes).unwrap();
    let mut input = MidiCompletion::empty(0, ScriptInstanceId(0));
    input.read_file(path.to_str().unwrap());
    assert!(input.success);
    assert_eq!(input.tracks, 2);
    input.save_file(path.to_str().unwrap()).unwrap();
    let mut output = MidiCompletion::empty(0, ScriptInstanceId(0));
    output.read_file(path.to_str().unwrap());
    assert!(output.success);
    assert_eq!(output.tracks, 2);
    for invalid in [b"not-midi".as_slice(), &bytes[..bytes.len() - 1]] {
        std::fs::write(&path, invalid).unwrap();
        let mut rejected = MidiCompletion::empty(0, ScriptInstanceId(0));
        rejected.read_file(path.to_str().unwrap());
        assert!(!rejected.success);
    }
    std::fs::remove_file(path).unwrap();
}
