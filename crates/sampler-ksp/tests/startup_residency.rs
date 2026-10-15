//! Finite startup batches use control-side admission, not the live queue limit.
use sampler_core::*;
use sampler_ksp::{
    Environment,
    model::{Location, ResidencyPass},
};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn prepared(pcm: Vec<Pcm>, groups: usize) -> Prepared {
    Prepared::new(
        48_000,
        pcm,
        (0..groups)
            .map(|g| Region {
                sample: if groups == 2 { g } else { 0 },
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::default(),
                playback: Playback::default(),
            })
            .collect(),
        groups,
    )
    .unwrap()
    .with_groups(groups as u32, (0..groups).map(|g| Some(g as u32)).collect())
    .unwrap()
}
fn cell(script: &sampler_ksp::Script, name: &str) -> u32 {
    match script
        .setup()
        .fields
        .iter()
        .find(|f| f.name == name)
        .unwrap()
        .storage
    {
        Location::Cells { offset, .. } => offset,
        _ => panic!("expected numeric home"),
    }
}
fn limits(plan: &Prepared, callbacks: usize) -> sampler_core::Limits {
    let mut limits = sampler_core::Limits::for_plan(plan, 8, 8);
    limits.behaviors = callbacks;
    limits.behavior_cells = plan.behavior_local_count() * callbacks;
    limits
}
fn drain(rt: &mut Runtime) {
    for _ in 0..4096 {
        if !rt.startup_work_pending(rt.active_plan()).unwrap() {
            return;
        }
        support::without_heap(|| rt.render(&mut []).unwrap());
    }
    panic!("finite startup did not settle");
}

#[test]
fn more_than_live_queue_capacity_keeps_every_real_id_with_two_callback_slots_and_fresh_pcm() {
    const GROUPS: usize = 1025;
    let source = r#"on init
        declare $i declare %ids[2050] declare %seen[2050]
        declare $calls declare $bad declare $later
        set_listener($NI_SIGNAL_TIMER_MS,1000)
        end on
        on persistence_changed
        $i:=0
        while ($i<1025)
            %ids[$i]:=purge_group($i,0)
            inc($i)
        end while
        $i:=0
        while ($i<1025)
            %ids[1025+$i]:=purge_group($i,($i=1024))
            inc($i)
        end while
        end on
        on async_complete
        if ($NI_ASYNC_ID>0 and $NI_ASYNC_ID<=2050)
            inc(%seen[$NI_ASYNC_ID-1])
        else
            inc($bad)
        end if
        inc($calls)
        end on
        on listener
        $later:=get_purge_state(1024)
        end on"#;
    let script = sampler_ksp::compile_with(
        source,
        48_000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        &Environment {
            groups: (0..GROUPS).map(|g| g.to_string()).collect(),
            ..Default::default()
        },
    )
    .unwrap();
    let (ids, seen, calls, bad, later) = (
        cell(&script, "%ids"),
        cell(&script, "%seen"),
        cell(&script, "$calls"),
        cell(&script, "$bad"),
        cell(&script, "$later"),
    );
    let plan = script
        .bind(prepared(
            vec![Pcm::new(48_000, vec![[0.25; 2]; 4096].into()).unwrap()],
            GROUPS,
        ))
        .unwrap();
    let budget = limits(&plan, 2);
    let mut rt = Runtime::new(plan, budget).unwrap();
    let progress = rt.residency_progress(rt.active_plan()).unwrap();
    assert_eq!(
        (
            progress.accepted,
            progress.consumed,
            progress.rejected_queue
        ),
        (2050, 2050, 0)
    );
    assert_eq!(progress.startup_completions_pending, 2050);
    drain(&mut rt);
    for i in 0..2050 {
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), ids + i),
            Ok(i64::from(i + 1))
        );
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), seen + i),
            Ok(1)
        );
    }
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), calls),
        Ok(2050)
    );
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), bad),
        Ok(0)
    );
    assert!(!rt.group_resident(rt.active_plan(), 0).unwrap());
    assert!(rt.group_resident(rt.active_plan(), 1024).unwrap());
    let mut silence = [[0.; 2]; 512];
    for _ in 0..96 {
        support::without_heap(|| rt.render(&mut silence).unwrap());
    }
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), later),
        Ok(1)
    );
    let mut audio = [[0.; 2]; 128];
    support::without_heap(|| {
        rt.trigger(
            Input {
                protocol: Protocol::Clap,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(997),
            },
            60,
            1.,
        )
        .unwrap();
        rt.render(&mut audio).unwrap();
    });
    assert!(audio.iter().flatten().any(|s| s.abs() > 0.01));
    assert_eq!(rt.stats().voices, 1);
    assert_eq!(rt.take_fault(), None);
}

#[test]
fn stored_return_ids_match_success_and_failure_callbacks() {
    let source = "on init declare $a declare $b declare $sa:=-7 declare $sb:=-7 declare $calls end on on persistence_changed $a:=purge_group(0,1) $b:=purge_group(1,1) end on on async_complete if ($NI_ASYNC_ID=$a) $sa:=$NI_ASYNC_EXIT_STATUS end if if ($NI_ASYNC_ID=$b) $sb:=$NI_ASYNC_EXIT_STATUS end if inc($calls) end on";
    let script = sampler_ksp::compile(source, 48_000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let homes = ["$a", "$b", "$sa", "$sb", "$calls"].map(|n| cell(&script, n));
    let absent = Pcm::streamed(48_000, 128).unwrap();
    absent
        .set_residency_loader(|| Err(Error::NotReady))
        .unwrap();
    let present = Pcm::new(48_000, vec![[0.25; 2]; 128].into()).unwrap();
    let plan = script.bind(prepared(vec![absent, present], 2)).unwrap();
    let budget = limits(&plan, 1);
    let mut rt = Runtime::new(plan, budget).unwrap();
    assert_eq!(rt.residency_progress(rt.active_plan()).unwrap().failed, 1);
    drain(&mut rt);
    for (home, expected) in homes.into_iter().zip([1, 2, 0, 1, 2]) {
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), home),
            Ok(expected)
        );
    }
    assert!(!rt.group_resident(rt.active_plan(), 0).unwrap());
    assert!(rt.group_resident(rt.active_plan(), 1).unwrap());
    assert_eq!(rt.take_fault(), None);
}

#[test]
fn base_snapshot_passes_reserve_distinct_ids_and_failed_pass_rolls_them_back() {
    // Missing host transport is an evaluator failure, unlike KSP integer
    // division by zero, which returns zero and does not abort this pass.
    let source = "on init declare $mode make_persistent($mode) declare $id declare $bad end on on persistence_changed if ($mode=0) $id:=purge_group(0,0) $bad:=$NI_BPM end if $id:=purge_group(0,1) end on";
    let mut environment = Environment::default();
    environment.instrument_persisted = Some(Default::default());
    environment
        .persisted
        .insert("$mode".into(), sampler_ksp::model::Value::Int(1));
    let init = sampler_ksp::initialize(source, sampler_ksp::Limits::LIBRARY, &environment).unwrap();
    let requests = init.initial_residency_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        (requests[0].async_id, requests[0].pass),
        (1, ResidencyPass::SnapshotPersistence)
    );
    assert_eq!(init.midi_object().async_id_high_water(), 1);
}

#[test]
fn independent_module_id_collisions_reject_instead_of_rewriting_captured_cells() {
    let source = "on init declare $id end on on persistence_changed $id:=purge_group(0,1) end on";
    let scripts = [0, 4].map(|slot| {
        sampler_ksp::compile_with(
            source,
            48_000,
            sampler_ksp::Limits::LIBRARY,
            &[],
            &Environment {
                slot,
                ..Default::default()
            },
        )
        .unwrap()
    });
    assert!(matches!(
        sampler_ksp::bind_modules(
            scripts.into(),
            prepared(
                vec![Pcm::new(48_000, vec![[0.25; 2]; 128].into()).unwrap()],
                1
            )
        ),
        Err(Error::InvalidInput)
    ));
}

#[test]
fn shared_async_domain_routes_distinct_physical_slots_to_exact_dense_instances() {
    let source = "on init declare $id declare $seen end on on persistence_changed $id:=purge_group(0,1) end on on async_complete $seen:=$NI_ASYNC_ID end on";
    let first = sampler_ksp::initialize(
        source,
        sampler_ksp::Limits::LIBRARY,
        &Environment::default(),
    )
    .unwrap();
    let second = sampler_ksp::initialize(
        source,
        sampler_ksp::Limits::LIBRARY,
        &Environment {
            slot: 4,
            midi_object: first.midi_object().clone(),
            ..Default::default()
        },
    )
    .unwrap();
    let scripts = [first, second]
        .into_iter()
        .map(|init| {
            sampler_ksp::compile_initialized(
                source,
                48_000,
                sampler_ksp::Limits::LIBRARY,
                &[],
                init,
            )
            .unwrap()
        })
        .collect();
    let plan = sampler_ksp::bind_modules(
        scripts,
        prepared(
            vec![Pcm::new(48_000, vec![[0.25; 2]; 128].into()).unwrap()],
            1,
        ),
    )
    .unwrap();
    let budget = limits(&plan, 1);
    let mut rt = Runtime::new(plan, budget).unwrap();
    drain(&mut rt);
    for (instance, id) in [(0, 1), (1, 2)] {
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(instance), 0),
            Ok(id)
        );
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(instance), 1),
            Ok(id)
        );
    }
}

#[test]
fn aggregate_roster_budget_applies_across_physical_slots_before_binding() {
    let source = "on init declare $i declare $id end on on persistence_changed while ($i<131073) $id:=purge_group(0,0) inc($i) end while end on";
    let first = sampler_ksp::initialize(
        source,
        sampler_ksp::Limits::LIBRARY,
        &Environment::default(),
    )
    .unwrap();
    let second = sampler_ksp::initialize(
        source,
        sampler_ksp::Limits::LIBRARY,
        &Environment {
            slot: 4,
            midi_object: first.midi_object().clone(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(first.initial_residency_requests().len(), 131073);
    assert_eq!(second.initial_residency_requests().len(), 131073);
    let scripts = [first, second]
        .into_iter()
        .map(|init| {
            sampler_ksp::compile_initialized(
                source,
                48_000,
                sampler_ksp::Limits::LIBRARY,
                &[],
                init,
            )
            .unwrap()
        })
        .collect();
    assert!(matches!(
        sampler_ksp::bind_modules(
            scripts,
            prepared(
                vec![Pcm::new(48_000, vec![[0.25; 2]; 128].into()).unwrap()],
                1
            )
        ),
        Err(Error::Capacity)
    ));
}

#[cfg(feature = "cache")]
#[test]
fn cached_roundtrips_keep_reserved_ids_and_reject_missing_or_corrupt_provenance() {
    let source = "on init declare $mode make_persistent($mode) declare $id declare %seen[2] end on on persistence_changed $id:=purge_group(0,$mode) end on on async_complete %seen[$NI_ASYNC_ID-1]:=$NI_ASYNC_ID end on";
    let mut environment = Environment {
        slot: 4,
        instrument_persisted: Some(Default::default()),
        ..Default::default()
    };
    environment
        .persisted
        .insert("$mode".into(), sampler_ksp::model::Value::Int(1));
    let init = sampler_ksp::initialize(source, sampler_ksp::Limits::LIBRARY, &environment).unwrap();
    let expected = init.initial_residency_requests().to_vec();
    assert_eq!(expected.len(), 2);
    assert_eq!(
        (expected[0].pass, expected[1].pass),
        (
            ResidencyPass::InstrumentPersistence,
            ResidencyPass::SnapshotPersistence
        )
    );
    let wire = serde_json::to_value(init.capture_initialized().unwrap()).unwrap();
    for _ in 0..2 {
        let restored = sampler_ksp::restore_initialized(
            source,
            sampler_ksp::Limits::LIBRARY,
            serde_json::from_value(wire.clone()).unwrap(),
        )
        .unwrap();
        assert_eq!(restored.initial_residency_requests(), expected);
        assert_eq!(restored.midi_object().async_id_high_water(), 2);
        let script = sampler_ksp::compile_initialized(
            source,
            48_000,
            sampler_ksp::Limits::LIBRARY,
            &[],
            restored,
        )
        .unwrap();
        let seen = cell(&script, "%seen");
        let id = cell(&script, "$id");
        let plan = script
            .bind(prepared(
                vec![Pcm::new(48_000, vec![[0.25; 2]; 128].into()).unwrap()],
                1,
            ))
            .unwrap();
        let budget = limits(&plan, 1);
        let mut rt = Runtime::new(plan, budget).unwrap();
        drain(&mut rt);
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), id),
            Ok(2)
        );
        for i in 0..2 {
            assert_eq!(
                rt.script_cell(rt.active_plan(), ScriptInstanceId(0), seen + i),
                Ok(i64::from(i + 1))
            );
        }
    }
    for malformed in ["schema", "id", "offset", "slot"] {
        let mut wire = wire.clone();
        match malformed {
            "schema" => {
                wire.as_object_mut().unwrap().remove("residency_schema");
            }
            "id" => wire["residency"][0]["async_id"] = 0.into(),
            "offset" => wire["residency"][0]["offset"] = (source.len() as u64).into(),
            _ => wire["residency"][0]["source_slot"] = 3.into(),
        }
        let restored = serde_json::from_value::<sampler_ksp::CachedInit>(wire)
            .ok()
            .and_then(|cache| {
                sampler_ksp::restore_initialized(source, sampler_ksp::Limits::LIBRARY, cache).ok()
            });
        assert!(restored.is_none(), "malformed {malformed} accepted");
    }
}
