//! Authored generic source-identity and init contracts; native extant-voice policy unknown.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::*;
use sampler_ksp::{Environment, model::Location};
fn environment() -> Environment {
    let zone_catalog = vec![
        sampler_ir::SourceZone {
            id: Some(0),
            home: sampler_ir::SourceZoneHome {
                group: 42,
                low_key: 60,
                high_key: 60,
            },
        },
        sampler_ir::SourceZone {
            id: Some(7),
            home: sampler_ir::SourceZoneHome {
                group: 11,
                low_key: 0,
                high_key: 127,
            },
        },
        sampler_ir::SourceZone {
            id: Some(2018),
            home: sampler_ir::SourceZoneHome {
                group: 92,
                low_key: 12,
                high_key: 84,
            },
        },
    ];
    Environment {
        zones: zone_catalog
            .iter()
            .map(|z| {
                (
                    z.id.unwrap(),
                    [
                        z.home.group as i32,
                        i32::from(z.home.low_key),
                        i32::from(z.home.high_key),
                    ],
                )
            })
            .collect(),
        zone_catalog,
        groups: (0..93).map(|n| format!("group{n}")).collect(),
        ..Default::default()
    }
}
fn plan() -> Prepared {
    let sample = Pcm::new(48000, vec![[0.25; 2]; 4096].into_boxed_slice()).unwrap();
    let region = || Region {
        sample: 0,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::new(0, 0, 0, 1., 0).unwrap(),
        playback: Playback::default(),
    };
    Prepared::new(48000, vec![sample], vec![region(), region()], 64)
        .unwrap()
        .with_groups(2, vec![Some(0), Some(1)])
        .unwrap()
        .with_source_groups(vec![42, 92])
        .unwrap()
        .with_source_zone_identities(
            vec![
                SourceZoneIdentity {
                    id: Some(0),
                    home: SourceZoneHome {
                        group: 42,
                        low_key: 60,
                        high_key: 60,
                    },
                    region: Some(0),
                },
                SourceZoneIdentity {
                    id: Some(7),
                    home: SourceZoneHome {
                        group: 11,
                        low_key: 0,
                        high_key: 127,
                    },
                    region: None,
                },
                SourceZoneIdentity {
                    id: Some(2018),
                    home: SourceZoneHome {
                        group: 92,
                        low_key: 12,
                        high_key: 84,
                    },
                    region: Some(1),
                },
            ],
            3,
            4096,
        )
        .unwrap()
}
fn cells(script: &sampler_ksp::Script) -> std::collections::BTreeMap<String, u32> {
    script
        .setup()
        .fields
        .iter()
        .filter_map(|f| match f.storage {
            Location::Cells { offset, .. } => Some((f.name.clone(), offset)),
            _ => None,
        })
        .collect()
}
#[test]
fn getter_count_and_ordinal_id_keep_filtered_rows_and_zero_without_heap() {
    let source="on init declare $index := -1 declare $id := get_zone_id(0) declare $count := get_num_zones() declare $group := get_zone_par(0,$ZONE_PAR_GROUP) declare $low declare $high end on
 on controller inc($index) $id:=get_zone_id($index) $count:=get_num_zones() $group:=get_zone_par($id,$ZONE_PAR_GROUP) $low:=get_zone_par($id,$ZONE_PAR_LOW_KEY) $high:=get_zone_par($id,$ZONE_PAR_HIGH_KEY) end on";
    let script = sampler_ksp::compile_with(
        source,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        &environment(),
    )
    .unwrap();
    let fields = cells(&script);
    let plan = script.bind(plan()).unwrap();
    let limits = Limits::for_plan(&plan, 4, 8);
    let mut rt = Runtime::new(plan, limits).unwrap();
    let active = rt.active_plan();
    let domain = rt.performance(0).unwrap();
    let ch = Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    }
    .channel_address();
    support::without_heap(|| {
        let read = |rt: &Runtime, name: &str| {
            rt.script_cell(active, ScriptInstanceId(0), fields[name])
                .unwrap()
        };
        assert_eq!(
            (read(&rt, "$id"), read(&rt, "$count"), read(&rt, "$group")),
            (0, 3, 42)
        );
        for (id, group, low, high) in [
            (0, 42, 60, 60),
            (7, 11, 0, 127),
            (2018, 92, 12, 84),
            (-1, -1, -1, -1),
        ] {
            rt.dispatch_controller(domain, ch, 1, 1, 1).unwrap();
            assert_eq!(
                (
                    read(&rt, "$id"),
                    read(&rt, "$count"),
                    read(&rt, "$group"),
                    read(&rt, "$low"),
                    read(&rt, "$high")
                ),
                (id, 3, group, low, high)
            );
        }
    });
}
#[test]
fn init_zone_write_is_sync_minus_one_readback_and_never_creates_a_source() {
    let source = "on init set_snapshot_type(3) declare $job:=set_zone_par(0,$ZONE_PAR_GROUP,11) declare $read:=get_zone_par(0,$ZONE_PAR_GROUP) end on";
    let initialized =
        sampler_ksp::initialize(source, sampler_ksp::Limits::LIBRARY, &environment()).unwrap();
    assert_eq!(initialized.zone_parameters(), &[(0, [11, 60, 60])].into());
    let script = sampler_ksp::compile_initialized(
        source,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        initialized,
    )
    .unwrap();
    let fields = cells(&script);
    // The host applies init homes before preparing DSP. Bind an equivalent authored home.
    let plan = Prepared::new(48000, vec![], vec![], 0).unwrap();
    let plan = script.bind(plan).unwrap();
    let limits = Limits::for_plan(&plan, 1, 1);
    let rt = Runtime::new(plan, limits).unwrap();
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), fields["$job"]),
        Ok(-1)
    );
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), fields["$read"]),
        Ok(11)
    );
    for call in [
        "set_zone_par(1,$ZONE_PAR_GROUP,0)",
        "set_zone_par(-1,$ZONE_PAR_GROUP,0)",
        "set_zone_par(0,$ZONE_PAR_GROUP,93)",
        "set_zone_par(0,$ZONE_PAR_LOW_KEY,61)",
        "set_zone_par(0,$ZONE_PAR_HIGH_KEY,128)",
        "set_zone_par(0,$ZONE_PAR_ROOT_KEY,60)",
    ] {
        assert!(
            sampler_ksp::initialize(
                &format!("on init set_snapshot_type(3) {call} end on"),
                sampler_ksp::Limits::LIBRARY,
                &environment()
            )
            .is_err(),
            "{call}"
        );
    }
}
#[test]
fn init_snapshot_policy_is_global_but_outside_init_cannot_change_it() {
    let source = "on init set_snapshot_type(3) declare $g end on on persistence_changed set_snapshot_type(0) $g:=set_zone_par(0,$ZONE_PAR_GROUP,11) end on";
    let initialized =
        sampler_ksp::initialize(source, sampler_ksp::Limits::LIBRARY, &environment()).unwrap();
    assert_eq!(
        initialized.snapshot_mode_write().unwrap().mode,
        sampler_ksp::model::SnapshotMode::ScriptWithoutInit
    );
    assert_eq!(initialized.zone_parameters().get(&0), Some(&[11, 60, 60]));
}
#[test]
fn waveform_zero_is_an_explicit_attachment_and_unavailable_id_never_aliases_it() {
    let source = "on init declare ui_waveform $wave(1,1) attach_zone($wave,0,0) set_ui_wf_property($wave,$UI_WF_PROP_PLAY_CURSOR,0,19) end on";
    let admitted = |source: &str| {
        let env = environment();
        let mut initialized =
            sampler_ksp::initialize(source, sampler_ksp::Limits::LIBRARY, &env).unwrap();
        let mut sources = env.zones;
        // ID7 remains in the physical catalog, but has no admitted primary.
        sources.remove(&7);
        assert!(initialized.admit_source_zones(sources).is_empty());
        sampler_ksp::compile_initialized(
            source,
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[],
            initialized,
        )
        .unwrap()
    };
    let script = admitted(source);
    let ui = script.model().interface.widgets[0].ui_id;
    let face = script.ui(&|_| None).unwrap();
    let wave = face.widgets[0].waveform.as_ref().unwrap();
    assert_eq!((wave.zone, wave.cursor_us), (0, 19));
    let prepared = script.bind(plan()).unwrap();
    let limits = Limits::for_plan(&prepared, 1, 1);
    let rt = Runtime::new(prepared, limits).unwrap();
    support::without_heap(|| {
        let read = |key| rt.script_store(rt.active_plan(), ScriptInstanceId(0), key);
        assert_eq!(read(waveform::attachment_key(ui)), Ok(Some(0)));
        assert_eq!(read(waveform::Property::Cursor.key(ui, 0)), Ok(Some(19)));
        assert_eq!(read(waveform::source_key(0)), Ok(Some(1)));
        assert_eq!(read(waveform::source_key(7)), Ok(None));
        assert_eq!(read(waveform::source_key(2018)), Ok(Some(1)));
    });
    let missing = source.replace("attach_zone($wave,0,0)", "attach_zone($wave,1,0)");
    let script = admitted(&missing);
    assert!(script.ui(&|_| None).unwrap().widgets[0].waveform.is_none());
    assert!(
        script
            .warnings()
            .iter()
            .any(|w| w.message.contains("source zone is absent"))
    );
    script.bind(plan()).unwrap();

    // An unadmitted native ID cannot borrow a positive legacy selection ID
    // that happens to resolve to a different prepared region.
    let unadmitted = source.replace("attach_zone($wave,0,0)", "attach_zone($wave,7,0)");
    let script = sampler_ksp::compile_with(
        &unadmitted,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        &environment(),
    )
    .unwrap();
    let conflicting = plan().with_source_zones(vec![7, 2018]).unwrap();
    assert!(matches!(script.bind(conflicting), Err(Error::NotReady)));
}

#[cfg(feature = "cache")]
#[test]
fn cached_source_catalog_keeps_order_policy_and_refuses_old_or_duplicate_identities() {
    let source = "on init declare $count:=get_num_zones() declare $id:=get_zone_id(0) end on";
    let mut env = environment();
    env.snapshot_mode = sampler_ksp::model::SnapshotMode::ScriptWithoutInit;
    let initialized = sampler_ksp::initialize(source, sampler_ksp::Limits::LIBRARY, &env).unwrap();
    let value = serde_json::to_value(initialized.capture_initialized().unwrap()).unwrap();
    let expected = sampler_ksp::compile_initialized(
        source,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        initialized,
    )
    .unwrap();
    let restored = sampler_ksp::restore_initialized(
        source,
        sampler_ksp::Limits::LIBRARY,
        serde_json::from_value(value.clone()).unwrap(),
    )
    .unwrap();
    let actual = sampler_ksp::compile_initialized(
        source,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        restored,
    )
    .unwrap();
    assert_eq!(
        expected.setup().identity.context,
        actual.setup().identity.context
    );
    for (case, mut invalid) in [value.clone(), value.clone(), value]
        .into_iter()
        .enumerate()
    {
        match case {
            0 => {
                invalid
                    .as_object_mut()
                    .unwrap()
                    .remove("zone_identity_schema");
            }
            1 => invalid["zone_catalog"][2][0] = serde_json::json!(7),
            _ => {
                let row = invalid["zones"][0].clone();
                invalid["zones"].as_array_mut().unwrap().push(row);
            }
        }
        assert!(
            sampler_ksp::restore_initialized(
                source,
                sampler_ksp::Limits::LIBRARY,
                serde_json::from_value(invalid).unwrap()
            )
            .is_err()
        );
    }
}

#[test]
fn source_sized_initializer_over_256_preserves_committed_readback_and_every_async_result() {
    let count = 100usize;
    let jobs = source_zone_job_capacity(count).unwrap();
    let mut env = Environment {
        groups: vec!["left".into(), "right".into()],
        ..Default::default()
    };
    for index in 0..count {
        let id = (index * 11) as u32;
        env.zones.insert(id, [0, 40, 80]);
        env.zone_catalog.push(sampler_ir::SourceZone {
            id: Some(id),
            home: sampler_ir::SourceZoneHome {
                group: 0,
                low_key: 40,
                high_key: 80,
            },
        });
    }
    let source = format!(
        r#"
on init
    set_snapshot_type(3)
    declare $i
    declare $zone
    declare $job
    declare $done
    declare $before
    declare $after
    declare %ids[{jobs}]
    declare %status[{jobs}]
end on
on persistence_changed
    while ($i < {count})
        $zone := get_zone_id($i)
        $job := set_zone_par($zone,$ZONE_PAR_GROUP,1)
        $job := set_zone_par($zone,$ZONE_PAR_LOW_KEY,100)
        $job := set_zone_par($zone,$ZONE_PAR_HIGH_KEY,110)
        inc($i)
    end while
    $before := get_zone_par($zone,$ZONE_PAR_GROUP)
end on
on async_complete
    %ids[$done] := $NI_ASYNC_ID
    %status[$done] := $NI_ASYNC_EXIT_STATUS
    inc($done)
    $after := get_zone_par(0,$ZONE_PAR_GROUP)
end on
"#
    );
    let initialized = sampler_ksp::initialize(&source, sampler_ksp::Limits::LIBRARY, &env).unwrap();
    assert_eq!(initialized.midi_object().async_id_high_water(), jobs as i32);
    for index in 0..count {
        assert_eq!(
            initialized.zone_parameters().get(&((index * 11) as u32)),
            Some(&[1, 100, 110])
        );
    }
    let script = sampler_ksp::compile_initialized(
        &source,
        48_000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        initialized,
    )
    .unwrap();
    let fields = cells(&script);
    let prepared = script
        .bind(Prepared::new(48_000, vec![], vec![], 0).unwrap())
        .unwrap();
    let limits = Limits::for_plan(&prepared, 1, 1);
    let rt = Runtime::new(prepared, limits).unwrap();
    let active = rt.active_plan();
    let read = |name: &str, index: u32| {
        rt.script_cell(active, ScriptInstanceId(0), fields[name] + index)
            .unwrap()
    };
    assert_eq!(read("$done", 0), jobs as i64);
    assert_eq!(read("$job", 0), jobs as i64);
    assert_eq!(read("$before", 0), 0);
    assert_eq!(read("$after", 0), 1);
    for index in 0..jobs as u32 {
        assert_eq!(read("%ids", index), i64::from(index + 1));
        assert_eq!(read("%status", index), 1);
    }
}
