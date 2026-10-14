use super::*;

fn instrument() -> ir::Instrument {
    let mut instrument = ir::Instrument::default();
    instrument.behaviors.push(ir::Behavior {
        name: "authored zone initialization".into(),
        language: ir::Language::Ksp,
        slot: Some(0),
        state: vec![],
        requires: vec![],
        source: "on init
            declare ui_waveform $valid(1,1)
            declare ui_waveform $removed(1,1)
            attach_zone($valid,3,0)
            set_ui_wf_property($valid,$UI_WF_PROP_PLAY_CURSOR,0,get_zone_par(3,$ZONE_PAR_LOW_KEY))
            attach_zone($removed,2,0)
            set_ui_wf_property($removed,$UI_WF_PROP_TABLE_VAL,2,77)
            end on"
            .into(),
    });
    instrument
}
fn admit_reordered_sources(instrument: &mut ir::Instrument) {
    instrument.groups.push(ir::Group::default());
    instrument.source_indices.groups = vec![None, None, Some(ir::GroupRef(0))];
    let mut third = ir::Zone::new(ir::AssetRef(0));
    third.group = Some(ir::GroupRef(0));
    third.keys = ir::KeyRange { low: 60, high: 61 };
    let mut first = third.clone();
    first.keys = ir::KeyRange { low: 36, high: 48 };
    instrument.zones = vec![third, first];
    instrument.source_indices.zones = vec![Some(ir::ZoneRef(1)), None, Some(ir::ZoneRef(0))];
}

#[test]
fn early_metadata_survives_one_initializer_and_final_sparse_reordered_admission() {
    let mut instrument = instrument();
    take_script_init_runs();
    let metadata = [(1, [2, 36, 48]), (2, [2, 50, 59]), (3, [2, 60, 61])].into();
    let initialized = initialize_scripts_with_zones(&mut instrument, None, vec![], &[], &metadata);
    assert_eq!(take_script_init_runs(), 1);
    admit_reordered_sources(&mut instrument);
    let (scripts, faces, _) =
        compile_ui_initialized(&mut instrument, &Options::default(), Some(initialized));
    assert_eq!(
        take_script_init_runs(),
        0,
        "UI projection consumes the original initialized state"
    );
    assert_eq!(scripts.len(), 1);
    assert_eq!(faces[0].widgets[0].waveform.as_ref().unwrap().zone, 3);
    assert_eq!(faces[0].widgets[0].waveform.as_ref().unwrap().cursor_us, 60);
    assert!(faces[0].widgets[1].waveform.is_none());
    assert!(
        instrument
            .unsupported
            .iter()
            .any(|u| u.feature == "waveform source unavailable after admission")
    );
    assert!(
        !scripts[0]
            .warnings()
            .iter()
            .any(|w| w.message.contains("source zone is absent"))
    );
}

#[test]
fn an_unbound_final_source_cannot_be_accepted_by_its_positive_identity() {
    let mut instrument = instrument();
    let metadata = [(2, [2, 50, 59]), (3, [2, 60, 61])].into();
    let initialized = initialize_scripts_with_zones(&mut instrument, None, vec![], &[], &metadata);
    admit_reordered_sources(&mut instrument);
    instrument.zones[0].asset = None;
    let (scripts, faces, _) =
        compile_ui_initialized(&mut instrument, &Options::default(), Some(initialized));
    assert_eq!(scripts.len(), 1);
    assert!(faces[0].widgets.iter().all(|w| w.waveform.is_none()));
    assert_eq!(
        instrument
            .unsupported
            .iter()
            .filter(|u| u.feature == "waveform source unavailable after admission")
            .count(),
        2
    );
}

#[test]
fn late_host_state_reinitializes_against_the_current_physical_domain() {
    let mut instrument = instrument();
    let metadata = [(2, [2, 50, 59]), (3, [2, 60, 61])].into();
    let mut initialized =
        initialize_scripts_with_zones(&mut instrument, None, vec![], &[], &metadata);
    admit_reordered_sources(&mut instrument);
    admit_initialized_zone_sources(&mut instrument, &mut initialized);
    // A reload with a current domain is independent of a prior rejected ID.
    let (scripts, faces, _) = compile_ui_initialized(&mut instrument, &Options::default(), None);
    assert_eq!(scripts.len(), 1);
    assert_eq!(faces[0].widgets[0].waveform.as_ref().unwrap().zone, 3);
    assert!(faces[0].widgets[1].waveform.is_none());
}

#[test]
#[ignore = "requires KONTRA_PROFILE_KONTAKT_ZONE_INIT owned installed instrument; metadata/UI only"]
fn owned_instrument_keeps_requested_waveform_source_and_reports_numeric_identity() {
    let path = std::env::var_os("KONTRA_PROFILE_KONTAKT_ZONE_INIT").expect("owned instrument path");
    let mut converted = crate::read(Path::new(&path)).expect("owned metadata translation");
    let source_zones = converted.instrument.source_indices.zones.clone();
    let raw = converted
        .instrument
        .kontakt_objects
        .as_ref()
        .unwrap()
        .zones
        .clone();
    let (scripts, faces, _) = converted.compile_ui(&Options::default());
    assert_eq!(scripts.len(), 1);
    assert!(
        !scripts
            .iter()
            .flat_map(|s| s.warnings())
            .any(|w| w.message.contains("source zone is absent"))
    );
    let mut rows = Vec::new();
    for face in faces {
        for widget in face.widgets {
            let Some(wave) = widget.waveform else {
                continue;
            };
            let id = usize::try_from(wave.zone).unwrap();
            let ir = source_zones
                .get(id - 1)
                .and_then(|r| *r)
                .expect("exact final source identity");
            let physical = &raw[id - 1];
            let zone = &converted.instrument.zones[ir.0];
            assert!(zone.asset.is_some());
            rows.push(serde_json::json!({"requested_id":id,"ir_zone":ir.0,"source_group":physical.group,"source_keys":[physical.low_key,physical.high_key],"admitted_asset":zone.asset.unwrap().0}));
        }
    }
    assert!(!rows.is_empty());
    println!(
        "ZONE_INIT_AUDIT {}",
        serde_json::json!({"script_count":scripts.len(),"physical_zones":source_zones.len(),"source_holes":source_zones.iter().filter(|z|z.is_none()).count(),"attachments":rows,"native_id_rekeyed":false,"native_persisted_identity_equivalence":"unverified","payload_written":false})
    );
}
