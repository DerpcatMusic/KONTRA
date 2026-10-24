//! Authored full-file preset mechanism and independent static destination oracle.
#[path = "support/zone_routing.rs"]
mod fixture;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::{Input, Limits, Protocol, Runtime, ScriptInstanceId, ZoneParameter};
use sampler_kontakt::{Options, load_read, read_with_snapshot};
fn read(path: &std::path::Path, snapshot: Option<&std::path::Path>) -> sampler_kontakt::Loaded {
    let converted = match snapshot {
        Some(s) => read_with_snapshot(path, &sampler_kontakt::read_snapshot(s).unwrap()).unwrap(),
        None => sampler_kontakt::read(path).unwrap(),
    };
    load_read(
        converted,
        &Options {
            mpe: None,
            library: Some(path.into()),
            ..Options::default()
        },
        |_| {},
        || false,
    )
    .unwrap()
}
fn runtime(loaded: sampler_kontakt::Loaded) -> Runtime {
    let limits = Limits::for_plan(&loaded.plan, 8, 16);
    Runtime::new(loaded.plan, limits).unwrap()
}
fn settle(rt: &mut Runtime) {
    for _ in 0..16 {
        rt.render(&mut [[0.; 2]; 64]).unwrap();
    }
    rt.flush_behaviors(|_, _, outcome| {
        assert_eq!(outcome, sampler_core::Outcome::Finished);
        true
    });
    assert_eq!(rt.pending_commands(), 0);
}
fn audio(rt: &mut Runtime) -> Vec<[f32; 2]> {
    let mut frames = vec![[0.; 2]; 256];
    support::without_heap(|| {
        let note = rt
            .trigger(
                Input {
                    protocol: Protocol::Clap,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: Some(71),
                },
                60,
                1.,
            )
            .unwrap();
        rt.render(&mut frames).unwrap();
        rt.release(note).unwrap();
    });
    frames
}
fn cell(rt: &Runtime, offset: u32) -> i64 {
    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), offset)
        .unwrap()
}
fn offsets(script: &sampler_ksp::ScriptView) -> std::collections::BTreeMap<String, u32> {
    script
        .setup()
        .fields
        .iter()
        .filter_map(|f| match f.storage {
            sampler_ksp::model::Location::Cells { offset, .. } => Some((f.name.clone(), offset)),
            _ => None,
        })
        .collect()
}
#[test]
fn full_nki_async_snapshot_a_b_a_routes_actual_source_and_matches_static_destination_pcm() {
    let f = fixture::fixture();
    let mut rendered = Vec::new();
    for (snapshot, selected) in [(&f.a, 0u32), (&f.b, 7), (&f.a, 0)] {
        let loaded = read(&f.base, Some(snapshot));
        assert_eq!(
            loaded
                .instrument
                .source_indices
                .zone_catalog
                .iter()
                .map(|z| z.id)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(7), Some(2018)]
        );
        assert_eq!(loaded.instrument.source_indices.zones.len(), 3);
        assert!(
            loaded.instrument.zones.len() > 3,
            "extra recipes remain outside physical roster"
        );
        for route in &loaded.instrument.zone_routing {
            let physical = loaded
                .instrument
                .source_indices
                .zone_catalog
                .iter()
                .position(|z| z.id == Some(route.source_zone))
                .unwrap();
            let primary = loaded.instrument.source_indices.zones[physical].unwrap();
            for variant in &route.variants {
                let Some(zone) = variant.zone else {
                    continue;
                };
                let template = &loaded.instrument.zones[zone.0];
                assert_eq!(template.asset, loaded.instrument.zones[primary.0].asset);
                assert_eq!(
                    loaded.instrument.groups[template.group.unwrap().0]
                        .wavetable
                        .is_some(),
                    variant.group == 0
                );
            }
        }
        assert_eq!(loaded.scripts.len(), 1);
        let fields = offsets(&loaded.scripts[0]);
        assert_eq!(
            loaded.scripts[0].setup().zone_routing,
            sampler_ksp::ZoneRoutingCapability::RequiresPreparedRoutes
        );
        let mut rt = runtime(loaded);
        support::without_heap(|| settle(&mut rt));
        assert_eq!(cell(&rt, fields["$count"]), 3);
        assert_eq!(cell(&rt, fields["$zero"]), 0);
        assert_eq!(cell(&rt, fields["$sparse"]), 2018);
        assert!(cell(&rt, fields["$job"]) > 0);
        assert_eq!(cell(&rt, fields["$after"]), 0);
        assert_eq!(cell(&rt, fields["$completions"]), 6);
        assert_eq!(
            rt.zone_parameter_in(rt.active_plan(), selected, ZoneParameter::Group),
            Ok(0)
        );
        assert_eq!(
            rt.zone_parameter_in(rt.active_plan(), selected, ZoneParameter::LowKey),
            Ok(60)
        );
        assert_eq!(
            rt.zone_parameter_in(rt.active_plan(), selected, ZoneParameter::HighKey),
            Ok(60)
        );
        let output = audio(&mut rt);
        assert!(output.iter().flatten().any(|v| *v != 0.));
        let static_path = fixture::static_base(f.directory.path(), selected as i32);
        let mut expected = runtime(read(&static_path, None));
        settle(&mut expected);
        assert_eq!(
            output,
            audio(&mut expected),
            "actual selected asset+WT template must match independent static destination"
        );
        rendered.push(output);
    }
    assert_ne!(rendered[0], rendered[1]);
    assert_eq!(rendered[0], rendered[2]);
}
#[test]
fn native_identity_import_names_the_first_duplicate_and_keeps_missing_without_ordinal_alias() {
    let f = fixture::fixture();
    let duplicate = f.directory.path().join("duplicate.nki");
    std::fs::write(
        &duplicate,
        fixture::base(None, 0, [Some(0), Some(7), Some(7)]),
    )
    .unwrap();
    // Kontakt keeps a shared identity on every zone that stores it (get_zone_id
    // reads it) and resolves lookups by identity to the first of them.
    let shared = sampler_kontakt::read(&duplicate).unwrap();
    assert_eq!(
        shared
            .instrument
            .source_indices
            .zone_catalog
            .iter()
            .map(|z| z.id)
            .collect::<Vec<_>>(),
        [Some(0), Some(7), Some(7)]
    );
    let missing = f.directory.path().join("missing.nki");
    std::fs::write(
        &missing,
        fixture::base(None, 0, [Some(0), None, Some(2018)]),
    )
    .unwrap();
    let converted = sampler_kontakt::read(&missing).unwrap();
    assert_eq!(
        converted
            .instrument
            .source_indices
            .zone_catalog
            .iter()
            .map(|z| z.id)
            .collect::<Vec<_>>(),
        vec![Some(0), None, Some(2018)]
    );
    assert!(
        converted
            .instrument
            .unsupported
            .iter()
            .any(|u| u.feature == "native source zone identity")
    );
}
