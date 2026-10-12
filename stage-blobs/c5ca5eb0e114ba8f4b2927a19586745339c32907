//! Own tiny NKA fixtures only. NI archived contract is cited in the lane receipt.
use sampler_core::*;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "kontra-nka-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().replace('\\', "/")
    }
    fn write(&self, name: &str, text: &str) -> String {
        let path = self.path(name);
        std::fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn script(source: &str) -> sampler_ksp::Script {
    sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap()
}
fn runtime(source: &str) -> (Runtime, usize) {
    let script = script(source);
    let entry = script
        .entries()
        .iter()
        .find(|e| e.kind == sampler_ksp::EntryKind::PgsChanged)
        .map_or(0, |e| e.program);
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let limits = Limits::for_plan(&plan, 8, 0);
    (Runtime::new(plan, limits).unwrap(), entry)
}
fn cell(rt: &Runtime, index: u32) -> i64 {
    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), index)
        .unwrap()
}
fn effects(rt: &mut Runtime) -> Vec<Effect> {
    let mut effects = Vec::with_capacity(16);
    support::without_heap(|| {
        rt.drain_effects(|e| {
            effects.push(*e);
            true
        })
    });
    effects
}
fn run(rt: &mut Runtime, entry: usize) -> BehaviorId {
    let plan = rt.active_plan();
    let mut id = None;
    support::without_heap(|| {
        id = Some(rt.start_plan_behavior(plan, entry).unwrap());
    });
    id.unwrap()
}
fn capture(rt: &Runtime, effect: &Effect) -> ArrayFileCompletion {
    let mut output = ArrayFileCompletion::from_effect(effect).unwrap();
    support::without_heap(|| rt.capture_array_file(effect.plan, &mut output).unwrap());
    output
}
fn complete(rt: &mut Runtime, plan: PlanId, output: &mut ArrayFileCompletion) {
    support::without_heap(|| rt.complete_array_file(plan, output).unwrap());
}

#[test]
fn nka_sync_init_reads_integer_real_and_string_arrays_before_following_statements() {
    let fixture = Fixture::new();
    let integer = fixture.write("integer.nka", "%integer\n-2147483648\n2147483647\n");
    let real = fixture.write("real.nka", "?real\n0.25\n-3.5\n");
    let text = fixture.write("text.nka", "!text\nhello α\n\n");
    let source = format!(
        r#"on init
        declare %integer[2] declare ?real[2] declare !text[2]
        declare $id := load_array_str(%integer, "{integer}")
        load_array_str(?real, "{real}") load_array_str(!text, "{text}")
        declare $seen := %integer[1] declare ~seen_real := ?real[1]
        declare @seen_text := !text[0]
    end on"#
    );
    let (mut rt, _) = runtime(&source);
    assert_eq!(
        (cell(&rt, 0), cell(&rt, 1), cell(&rt, 5)),
        (i32::MIN as i64, i32::MAX as i64, i32::MAX as i64)
    );
    assert_eq!(real_bits(-3.5), cell(&rt, 6));
    assert_eq!(
        rt.script_text(rt.active_plan(), ScriptInstanceId(0), 2)
            .unwrap()
            .as_str(),
        "hello α"
    );
    assert!(cell(&rt, 4) > 0);
    assert!(
        effects(&mut rt).is_empty(),
        "sync init must not enqueue a host request"
    );
}

#[test]
fn nka_sync_save_roundtrips_all_three_types_and_explicit_load_does_not_persist() {
    let fixture = Fixture::new();
    let paths = [
        fixture.path("i.nka"),
        fixture.path("r.nka"),
        fixture.path("s.nka"),
    ];
    let source = format!(
        r#"on init
        declare %i[2] := (7, -2) declare ?r[2] := (0.25, -0.5) declare !s[2]
        !s[0] := "hello" !s[1] := ""
        save_array_str(%i, "{}") save_array_str(?r, "{}") save_array_str(!s, "{}")
        %i[0] := 0 ?r[0] := 0.0 !s[0] := ""
        load_array_str(%i, "{}") load_array_str(?r, "{}") load_array_str(!s, "{}")
    end on"#,
        paths[0], paths[1], paths[2], paths[0], paths[1], paths[2]
    );
    let mut env = sampler_ksp::Environment::default();
    env.persisted_arrays
        .insert("%i".into(), vec![sampler_ksp::model::Value::Int(99); 2]);
    let init = sampler_ksp::initialize(&source, sampler_ksp::Limits::LIBRARY, &env).unwrap();
    let script =
        sampler_ksp::compile_initialized(&source, 48000, sampler_ksp::Limits::LIBRARY, &[], init)
            .unwrap();
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let limits = Limits::for_plan(&plan, 8, 0);
    let rt = Runtime::new(plan, limits).unwrap();
    assert_eq!((cell(&rt, 0), cell(&rt, 2)), (7, real_bits(0.25)));
    assert_eq!(
        rt.script_text(rt.active_plan(), ScriptInstanceId(0), 0)
            .unwrap()
            .as_str(),
        "hello"
    );
    assert_eq!(std::fs::read_to_string(&paths[0]).unwrap(), "%i\n7\n-2\n");
    assert_eq!(std::fs::read_to_string(&paths[2]).unwrap(), "!s\nhello\n\n");
}

#[test]
fn nka_runtime_load_preserves_non_ui_identity_and_resumes_wait_after_one_completion_without_heap() {
    let fixture = Fixture::new();
    let path = fixture.write("load.nka", "%data\n23\n24\n");
    let source = format!(
        r#"on init declare %data[2] := (7,8)
        declare $job declare $seen declare $status declare $count declare $after end on
        on pgs_changed $job := load_array_str(%data, "{path}") wait_async($job) $after := %data[0] end on
        on async_complete $seen := $NI_ASYNC_ID $status := $NI_ASYNC_EXIT_STATUS inc($count) end on"#
    );
    let (mut rt, entry) = runtime(&source);
    let behavior = run(&mut rt, entry);
    let effect = effects(&mut rt).pop().unwrap();
    assert_eq!(effect.service, ARRAY_FILE_SERVICE);
    assert_eq!(cell(&rt, 0), 7, "no synchronous runtime read");
    assert_eq!(rt.behavior_outcome(behavior), Ok(None));
    let mut output = capture(&rt, &effect);
    std::thread::spawn(move || {
        output.perform().unwrap();
        output
    })
    .join()
    .map(|mut output| {
        complete(&mut rt, effect.plan, &mut output);
        assert_eq!(output.success, true);
        support::without_heap(|| {
            assert_eq!(
                rt.complete_array_file(effect.plan, &mut output),
                Err(Error::StaleHandle)
            )
        });
    })
    .unwrap();
    assert_eq!(
        (
            cell(&rt, 0),
            cell(&rt, 1),
            cell(&rt, 3),
            cell(&rt, 4),
            cell(&rt, 5),
            cell(&rt, 6)
        ),
        (23, 24, cell(&rt, 2), 1, 1, 23)
    );
    assert_eq!(rt.behavior_outcome(behavior), Ok(Some(Outcome::Finished)));
}

#[test]
fn nka_async_save_snapshot_is_request_time_not_delayed_host_capture_time() {
    let fixture = Fixture::new();
    let path = fixture.path("saved.nka");
    let source = format!(
        r#"on init declare %data[2] := (1,2) declare $job declare $status end on
        on pgs_changed $job := save_array_str(%data, "{path}") %data[0] := 99 end on
        on async_complete $status := $NI_ASYNC_EXIT_STATUS end on"#
    );
    let (mut rt, entry) = runtime(&source);
    run(&mut rt, entry);
    assert_eq!(cell(&rt, 0), 99);
    let effect = effects(&mut rt).pop().unwrap();
    let mut output = capture(&rt, &effect);
    output.perform().unwrap();
    complete(&mut rt, effect.plan, &mut output);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "%data\n1\n2\n");
    assert_eq!(cell(&rt, 3), 1);
}

#[test]
fn nka_real_and_string_runtime_arrays_keep_typed_ownership() {
    let fixture = Fixture::new();
    let real = fixture.write("real.nka", "?r\n0.125\n-1.5\n");
    let text = fixture.write("text.nka", "!s\none\n二\n");
    let source = format!(
        r#"on init declare ?r[2] declare !s[2] declare $a declare $b declare $count end on
        on pgs_changed $a := load_array_str(?r, "{real}") $b := load_array_str(!s, "{text}") end on
        on async_complete inc($count) end on"#
    );
    let (mut rt, entry) = runtime(&source);
    run(&mut rt, entry);
    for effect in effects(&mut rt) {
        let mut output = capture(&rt, &effect);
        output.perform().unwrap();
        complete(&mut rt, effect.plan, &mut output);
    }
    assert_eq!(
        (cell(&rt, 0), cell(&rt, 1), cell(&rt, 4)),
        (real_bits(0.125), real_bits(-1.5), 2)
    );
    assert_eq!(
        rt.script_text(rt.active_plan(), ScriptInstanceId(0), 1)
            .unwrap()
            .as_str(),
        "二"
    );
}

#[test]
fn nka_persistence_assignment_defers_entire_prefix_until_live_activation() {
    let fixture = Fixture::new();
    let path = fixture.write("persist.nka", "%data\n42\n");
    let source = format!(
        r#"on init declare %data[1] declare $prefix declare $job declare $seen end on
        on persistence_changed inc($prefix) $job := load_array_str(%data,"{path}") end on
        on async_complete $seen := %data[0] end on"#
    );
    let initialized =
        sampler_ksp::initialize(&source, sampler_ksp::Limits::LIBRARY, &Default::default())
            .unwrap();
    let compiled = sampler_ksp::compile_initialized(
        &source,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        initialized,
    )
    .unwrap();
    assert_eq!(
        compiled.model().persistence_completion,
        sampler_ksp::model::PersistenceCompletion::Scheduled
    );
    let plan = compiled
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let limits = Limits::for_plan(&plan, 8, 0);
    let mut rt = Runtime::new(plan, limits).unwrap();
    let effect = effects(&mut rt).pop().unwrap();
    assert_eq!(cell(&rt, 1), 1, "scheduled prefix runs once, not twice");
    let mut output = capture(&rt, &effect);
    output.perform().unwrap();
    complete(&mut rt, effect.plan, &mut output);
    assert_eq!(cell(&rt, 3), 42);
}

#[test]
fn nka_failed_read_completes_with_zero_status_and_preserves_the_array() {
    let fixture = Fixture::new();
    let path = fixture.write("bad.nka", "%other\n9\n10\n");
    let source = format!(
        r#"on init declare %data[2] := (3,4) declare $job declare $status := -1 declare $count end on
        on pgs_changed $job := load_array_str(%data,"{path}") end on
        on async_complete $status := $NI_ASYNC_EXIT_STATUS inc($count) end on"#
    );
    let (mut rt, entry) = runtime(&source);
    run(&mut rt, entry);
    let effect = effects(&mut rt).pop().unwrap();
    let mut output = capture(&rt, &effect);
    assert!(output.perform().is_err());
    complete(&mut rt, effect.plan, &mut output);
    assert_eq!(
        (cell(&rt, 0), cell(&rt, 1), cell(&rt, 3), cell(&rt, 4)),
        (3, 4, 0, 1)
    );
}

#[test]
fn nka_wrong_instance_and_cancelled_job_cannot_install_or_complete_a_new_owner() {
    let fixture = Fixture::new();
    let path = fixture.write("epoch.nka", "%data\n8\n");
    let source = format!(
        r#"on init declare %data[1] := (1) declare $job declare $count end on
        on pgs_changed $job := load_array_str(%data,"{path}") end on on async_complete inc($count) end on"#
    );
    let (mut rt, entry) = runtime(&source);
    run(&mut rt, entry);
    let effect = effects(&mut rt).pop().unwrap();
    let mut output = capture(&rt, &effect);
    output.perform().unwrap();
    output.instance = ScriptInstanceId(1);
    support::without_heap(|| {
        assert_eq!(
            rt.complete_array_file(effect.plan, &mut output),
            Err(Error::InvalidInput)
        )
    });
    output.instance = ScriptInstanceId(0);
    support::without_heap(|| rt.panic());
    support::without_heap(|| {
        assert_eq!(
            rt.complete_array_file(effect.plan, &mut output),
            Err(Error::StaleHandle)
        )
    });
    assert_eq!((cell(&rt, 0), cell(&rt, 2)), (1, 0));
}

#[test]
fn nka_job_queue_is_bounded_and_midi_ids_remain_distinct_and_complete_independently() {
    let fixture = Fixture::new();
    let path = fixture.write("bound.nka", "%data\n4\n");
    let source = format!(
        r#"on init declare %data[1] declare $job declare $midi declare $rejected declare $n declare $count end on
        on pgs_changed $midi := mf_set_buffer_size(1)
            while ($n < 8) $job := load_array_str(%data,"{path}") inc($n) end while
            $rejected := load_array_str(%data,"{path}")
        end on on async_complete inc($count) end on"#
    );
    let (mut rt, entry) = runtime(&source);
    run(&mut rt, entry);
    assert_eq!(cell(&rt, 3), -1);
    let effects = effects(&mut rt);
    assert_eq!(effects.len(), 9);
    let midi = effects.iter().find(|e| e.service == MIDI_SERVICE).unwrap();
    let mut completion = MidiCompletion::empty(midi.args[1] as i32, ScriptInstanceId(0));
    support::without_heap(|| rt.complete_midi(midi.plan, &mut completion).unwrap());
    let mut ids = std::collections::BTreeSet::from([completion.job]);
    for effect in effects.iter().filter(|e| e.service == ARRAY_FILE_SERVICE) {
        assert!(ids.insert(effect.args[0] as i32));
        let mut output = capture(&rt, effect);
        output.perform().unwrap();
        complete(&mut rt, effect.plan, &mut output);
    }
    assert_eq!(cell(&rt, 5), 9);
}

#[test]
fn nka_completion_is_plan_scoped_across_adoption_and_never_edits_the_replacement() {
    let fixture = Fixture::new();
    let path = fixture.write("old-plan.nka", "%data\n8\n");
    let source = format!(
        r#"on init declare %data[1] := (1) declare $job declare $count end on
        on pgs_changed $job := load_array_str(%data,"{path}") end on on async_complete inc($count) end on"#
    );
    let old = script(&source);
    let entry = old
        .entries()
        .iter()
        .find(|e| e.kind == sampler_ksp::EntryKind::PgsChanged)
        .unwrap()
        .program;
    let old = old
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let limits = Limits::for_plan(&old, 8, 0);
    let (mut rt, mut owner) = Runtime::with_plan_updates(old, limits, 2, 1).unwrap();
    let old_plan = rt.active_plan();
    run(&mut rt, entry);
    let effect = effects(&mut rt).pop().unwrap();
    let mut output = capture(&rt, &effect);
    output.perform().unwrap();
    let new = script("on init declare %data[1] := (99) declare $job declare $count end on")
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let request = owner.submit(Box::new(new)).unwrap();
    support::without_heap(|| assert_eq!(rt.poll_plan_update(), Ok(Some(request))));
    let new_plan = rt.active_plan();
    assert_ne!(old_plan, new_plan);
    support::without_heap(|| {
        assert_eq!(
            rt.complete_array_file(new_plan, &mut output),
            Err(Error::StaleHandle)
        )
    });
    complete(&mut rt, old_plan, &mut output);
    assert_eq!(rt.script_cell(old_plan, ScriptInstanceId(0), 0), Ok(8));
    assert_eq!(rt.script_cell(new_plan, ScriptInstanceId(0), 0), Ok(99));
    assert_eq!(rt.script_cell(new_plan, ScriptInstanceId(0), 2), Ok(0));
}

#[test]
fn nka_runtime_concatenated_path_truncation_is_not_used_as_a_different_path() {
    let source = format!(
        r#"on init declare %data[1] := (7) declare $job declare $status := -1 declare @path := "/" end on
        on pgs_changed @path := @path & "{}" & "suffix.nka" $job := load_array_str(%data,@path) end on
        on async_complete $status := $NI_ASYNC_EXIT_STATUS end on"#,
        "x".repeat(TEXT_CAPACITY)
    );
    let (mut rt, entry) = runtime(&source);
    run(&mut rt, entry);
    let effect = effects(&mut rt).pop().unwrap();
    let mut output = capture(&rt, &effect);
    assert_eq!(output.perform(), Err(Error::Capacity));
    complete(&mut rt, effect.plan, &mut output);
    assert_eq!((cell(&rt, 0), cell(&rt, 2)), (7, 0));
}

#[test]
fn nka_unreachable_invalid_typed_ranges_and_snapshot_memory_limit_are_rejected_at_preparation() {
    let program = Program::new(vec![
        Instruction::End,
        Instruction::Op(Op::ArrayFile {
            array: 9,
            path: TextRef::Cell(0),
            write: false,
            local: 0,
        }),
    ])
    .unwrap()
    .with_script_instance(ScriptInstanceId(0))
    .with_wait_lifetime(WaitLifetime::Callback);
    let prepared = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_script_instances(vec![vec![0]])
        .unwrap()
        .with_script_resources(vec![ScriptResources {
            texts: vec![String::new()],
            ..Default::default()
        }])
        .unwrap();
    assert!(matches!(
        prepared.with_programs(vec![program], None),
        Err(Error::InvalidInput)
    ));
    let prepared = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_script_instances(vec![vec![0]])
        .unwrap();
    assert!(matches!(
        prepared.with_script_resources(vec![ScriptResources {
            array_files: vec![ArrayFileArray {
                offset: u32::MAX,
                ..array(ArrayFileKind::Integer, "%a", 1)
            }],
            ..Default::default()
        }]),
        Err(Error::Capacity)
    ));
    let prepared = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_script_instances(vec![vec![]])
        .unwrap();
    assert!(matches!(
        prepared.with_script_resources(vec![ScriptResources {
            array_files: vec![array(
                ArrayFileKind::Text,
                "!a",
                ARRAY_FILE_MAX_CELLS as u32
            )],
            texts: vec![String::new(); ARRAY_FILE_MAX_CELLS],
            ..Default::default()
        }]),
        Err(Error::Capacity)
    ));
}

#[test]
fn nka_unapproved_note_callback_is_rejected_without_file_effect() {
    let source = r#"on init declare %data[1] declare $job end on on note $job := load_array_str(%data,"/not-opened.nka") end on"#;
    let script = script(source);
    assert!(
        script
            .warnings()
            .iter()
            .any(|w| w.builtin == Some("load_array_str"))
    );
}

fn array(kind: ArrayFileKind, name: &str, len: u32) -> ArrayFileArray {
    ArrayFileArray {
        key: 0,
        name: Text::try_new(name).unwrap(),
        kind,
        offset: 0,
        len,
    }
}

#[test]
fn nka_codec_strict_header_type_shape_utf8_numeric_and_payload_failures_preserve_previous_values() {
    let fixture = Fixture::new();
    for (index, bytes) in [
        b"%other\n1\n2\n".as_slice(),
        b"?data\n1\n2\n",
        b"data\n1\n2\n",
        b"%data\n1\n",
        b"%data\n1\n2\n3\n",
        b"%data\n2147483648\n2\n",
        b"%data\n1\ninvalid\n",
        b"%data\n1\n\xff\n",
    ]
    .into_iter()
    .enumerate()
    {
        let path = fixture.path(&format!("bad{index}.nka"));
        std::fs::write(&path, bytes).unwrap();
        let mut output = ArrayFileCompletion::synchronous(
            array(ArrayFileKind::Integer, "%data", 2),
            &path,
            false,
            vec![7, 8],
            vec![],
        )
        .unwrap();
        assert!(output.perform().is_err());
        assert!(!output.success);
        assert_eq!(output.numbers(), &[7, 8]);
    }
    let path = fixture.write("nan.nka", "?r\nNaN\n");
    let mut output = ArrayFileCompletion::synchronous(
        array(ArrayFileKind::Real, "?r", 1),
        &path,
        false,
        vec![real_bits(2.0)],
        vec![],
    )
    .unwrap();
    assert!(output.perform().is_err());
    assert_eq!(output.numbers(), &[real_bits(2.0)]);
    let path = fixture.path("oversized.nka");
    std::fs::write(&path, vec![b'x'; ARRAY_FILE_MAX_BYTES + 1]).unwrap();
    let mut output = ArrayFileCompletion::synchronous(
        array(ArrayFileKind::Integer, "%data", 2),
        &path,
        false,
        vec![7, 8],
        vec![],
    )
    .unwrap();
    assert_eq!(output.perform(), Err(Error::Capacity));
    assert_eq!(output.numbers(), &[7, 8]);
}

#[test]
fn nka_save_failures_preserve_old_bytes_and_do_not_create_missing_parent() {
    let fixture = Fixture::new();
    let path = fixture.write("old.nka", "old bytes");
    let mut invalid = ArrayFileCompletion::synchronous(
        array(ArrayFileKind::Text, "!s", 1),
        &path,
        true,
        vec![],
        vec![Text::new("two\nlines")],
    )
    .unwrap();
    assert!(invalid.perform().is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old bytes");
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    let mut readonly = ArrayFileCompletion::synchronous(
        array(ArrayFileKind::Integer, "%i", 1),
        &path,
        true,
        vec![1],
        vec![],
    )
    .unwrap();
    assert!(readonly.perform().is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old bytes");
    // Return to a writable own fixture so cleanup works on Windows as well.
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(&path, permissions).unwrap();
    let missing = fixture.path("absent/new.nka");
    let mut output = ArrayFileCompletion::synchronous(
        array(ArrayFileKind::Integer, "%i", 1),
        &missing,
        true,
        vec![1],
        vec![],
    )
    .unwrap();
    assert!(output.perform().is_err());
    assert!(!fixture.0.join("absent").exists());
    assert_eq!(
        std::fs::read_dir(&fixture.0).unwrap().count(),
        1,
        "no abandoned temporary"
    );
}

#[test]
fn nka_explicit_path_and_string_limits_reject_instead_of_truncating() {
    for path in [
        "relative.nka".to_owned(),
        "/bad\\path.nka".to_owned(),
        "/bad\0path.nka".to_owned(),
        format!("/{}", "x".repeat(TEXT_CAPACITY)),
    ] {
        assert!(
            ArrayFileCompletion::synchronous(
                array(ArrayFileKind::Integer, "%a", 1),
                &path,
                false,
                vec![],
                vec![]
            )
            .is_err()
        );
    }
    let fixture = Fixture::new();
    let path = fixture.write(
        "long.nka",
        &format!("!s\n{}\n", "x".repeat(TEXT_CAPACITY + 1)),
    );
    let mut output = ArrayFileCompletion::synchronous(
        array(ArrayFileKind::Text, "!s", 1),
        &path,
        false,
        vec![],
        vec![Text::new("prior")],
    )
    .unwrap();
    assert_eq!(output.perform(), Err(Error::Capacity));
    assert_eq!(output.texts()[0].as_str(), "prior");
}

#[cfg(feature = "cache")]
#[test]
fn nka_external_init_is_not_cached_or_replayed_from_an_old_cache() {
    let fixture = Fixture::new();
    let path = fixture.write("cache.nka", "%a\n5\n");
    let source = format!(r#"on init declare %a[1] load_array_str(%a,"{path}") end on"#);
    let init = sampler_ksp::initialize(&source, sampler_ksp::Limits::LIBRARY, &Default::default())
        .unwrap();
    assert!(init.capture_initialized().is_none());
    let original = sampler_ksp::initialize(
        "on init declare %a[1] end on",
        sampler_ksp::Limits::LIBRARY,
        &Default::default(),
    )
    .unwrap();
    assert!(
        sampler_ksp::restore_initialized(
            &source,
            sampler_ksp::Limits::LIBRARY,
            original.capture_initialized().unwrap()
        )
        .is_err()
    );
}
