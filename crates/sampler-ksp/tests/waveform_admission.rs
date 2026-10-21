//! Initialization metadata is provisional until the caller admits physical sources.
use sampler_ksp::{Environment, Limits, WaveformSourceReason};
use std::collections::BTreeMap;

const SOURCE: &str = "on init
    declare ui_waveform $first(1,1)
    declare ui_waveform $second(1,1)
    attach_zone($first,27,3)
    set_ui_wf_property($first,$UI_WF_PROP_PLAY_CURSOR,0,12000)
    set_ui_wf_property($first,$UI_WF_PROP_TABLE_VAL,2,77)
    attach_zone($second,91,0)
    set_key_color(60,$KEY_COLOR_RED)
    end on";
fn domain() -> BTreeMap<u32, [i32; 3]> {
    [(27, [3, 36, 60]), (91, [8, 61, 96])].into()
}
fn initial() -> sampler_ksp::Initialized {
    sampler_ksp::initialize(
        SOURCE,
        Limits::LIBRARY,
        &Environment {
            zones: domain(),
            ..Default::default()
        },
    )
    .unwrap()
}
fn compile(initial: sampler_ksp::Initialized) -> sampler_ksp::Script {
    sampler_ksp::compile_initialized(SOURCE, 48000, Limits::LIBRARY, &[], initial).unwrap()
}

#[test]
fn admitted_sources_preserve_exact_ids_independent_of_map_insertion_order() {
    let mut initial = initial();
    let reordered = [(91, [8, 61, 96]), (27, [3, 36, 60])].into();
    assert!(initial.admit_source_zones(reordered).is_empty());
    let script = compile(initial);
    let face = script.ui(&|_| None).unwrap();
    assert_eq!(face.widgets[0].waveform.as_ref().unwrap().zone, 27);
    assert_eq!(face.widgets[0].waveform.as_ref().unwrap().cursor_us, 12000);
    assert_eq!(
        face.widgets[0].waveform.as_ref().unwrap().table,
        vec![0, 0, 77]
    );
    assert_eq!(face.widgets[1].waveform.as_ref().unwrap().zone, 91);
}

#[test]
fn missing_source_removes_only_its_attachment_and_properties_without_retargeting() {
    let mut initial = initial();
    let rejected = initial.admit_source_zones([(91, [8, 61, 96])].into());
    assert_eq!(rejected.len(), 1);
    assert_eq!(
        (rejected[0].zone, rejected[0].reason),
        (27, WaveformSourceReason::Unavailable)
    );
    // A second reconciliation cannot resurrect the removed request.
    assert!(initial.admit_source_zones(domain()).is_empty());
    let script = compile(initial);
    let face = script.ui(&|_| None).unwrap();
    assert!(face.widgets[0].waveform.is_none());
    assert_eq!(face.widgets[1].waveform.as_ref().unwrap().zone, 91);
    assert!(script.model().interface.keys[60].color.is_some());
}

#[test]
fn captured_source_metadata_cannot_silently_bind_to_a_different_owner() {
    let mut initial = initial();
    let rejected = initial.admit_source_zones([(27, [8, 61, 96]), (91, [8, 61, 96])].into());
    assert_eq!(rejected.len(), 1);
    assert_eq!(
        (rejected[0].zone, rejected[0].reason),
        (27, WaveformSourceReason::IdentityChanged)
    );
    let face = compile(initial).ui(&|_| None).unwrap();
    assert!(face.widgets[0].waveform.is_none());
    assert_eq!(face.widgets[1].waveform.as_ref().unwrap().zone, 91);
}

#[test]
fn captured_domain_cannot_authorize_sources_absent_from_the_prepared_plan() {
    let script = compile(initial());
    let foreign = sampler_core::Prepared::new(48000, vec![], vec![], 1).unwrap();
    assert!(matches!(
        script.bind(foreign),
        Err(sampler_core::Error::NotReady)
    ));
}

#[cfg(feature = "cache")]
#[test]
fn restored_initializer_reconciles_against_this_plan_without_rerunning_or_rekeying() {
    let initial = initial();
    let cached = initial.capture_initialized().unwrap();
    let bytes = serde_json::to_vec(&cached).unwrap();
    let cached = serde_json::from_slice(&bytes).unwrap();
    let mut restored = sampler_ksp::restore_initialized(SOURCE, Limits::LIBRARY, cached).unwrap();
    let rejected = restored.admit_source_zones([(91, [8, 61, 96])].into());
    assert_eq!(rejected[0].reason, WaveformSourceReason::Unavailable);
    let recaptured = restored.capture_initialized().unwrap();
    let mut reloaded =
        sampler_ksp::restore_initialized(SOURCE, Limits::LIBRARY, recaptured).unwrap();
    assert!(reloaded.admit_source_zones(domain()).is_empty());
    let face = compile(reloaded).ui(&|_| None).unwrap();
    assert!(face.widgets[0].waveform.is_none());
    assert_eq!(face.widgets[1].waveform.as_ref().unwrap().zone, 91);
}
