//! Same-source setup metadata and bounded value-capture boundary oracles.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::*;
use sampler_ksp::model::{Location, Persistence, SnapshotMode};
use sampler_ksp::setup::*;
use sampler_ksp::{Environment, Limits as KspLimits, Script, ScriptView};
fn compile(source: &str, slot: u8) -> Script {
    sampler_ksp::compile_with(
        source,
        48_000,
        KspLimits::LIBRARY,
        &[],
        &Environment {
            slot,
            ..Default::default()
        },
    )
    .unwrap()
}
fn limits() -> SetupCaptureLimits {
    SetupCaptureLimits {
        value_entries: 1 << 16,
        text_bytes: 8 << 20,
        sparse_entries: 1 << 16,
    }
}
#[test]
fn explicit_global_mode_writes_follow_all_five_physical_slots_not_model_defaults() {
    let sources = [
        "on init set_snapshot_type(1) end on",
        "on init end on",
        "on init if (0=1) set_snapshot_type(0) end if end on",
        "on init set_snapshot_type(2) set_snapshot_type(3) end on",
        "on init end on",
    ];
    let views: Vec<_> = sources
        .iter()
        .enumerate()
        .map(|(i, s)| compile(s, i as u8).view())
        .collect();
    assert_eq!(views[1].model().snapshot_mode, SnapshotMode::NativeWithInit);
    assert_eq!(views[1].model().snapshot_mode_write, None);
    assert_eq!(views[2].model().snapshot_mode_write, None);
    let first = snapshot_policy(&views[..3]).unwrap();
    assert_eq!(
        (first.mode, first.writer.unwrap().0),
        (SnapshotMode::NativeWithoutInit, 0)
    );
    let all = snapshot_policy(&views).unwrap();
    assert_eq!(
        (all.mode, all.writer.unwrap().0),
        (SnapshotMode::ScriptWithoutInit, 3)
    );
    let reversed: Vec<_> = views.iter().rev().cloned().collect();
    assert_eq!(snapshot_policy(&reversed).unwrap(), all);
    let mut explicit = views;
    explicit[4] = compile("on init set_snapshot_type(0) end on", 4).view();
    assert_eq!(snapshot_policy(&explicit).unwrap().writer.unwrap().0, 4);
    assert_eq!(
        snapshot_policy(&explicit).unwrap().mode,
        SnapshotMode::NativeWithInit
    );
    assert!(snapshot_policy(&[explicit[0].clone(), explicit[0].clone()]).is_err());
    assert!(snapshot_policy(&[compile("on init end on", 5).view()]).is_err());
}
#[test]
fn exact_source_layout_bindings_and_context_are_separate_setup_gates() {
    let source = "on init declare $private := 1 declare @text declare polyphonic $note declare ui_button $button end on";
    let a = compile(source, 0);
    let same = compile(source, 0);
    assert_eq!(a.setup().identity, same.setup().identity);
    let comment = compile(
        &format!("{source}\n{{ same declarations, different source }}"),
        0,
    );
    assert_ne!(a.setup().identity.source, comment.setup().identity.source);
    assert_eq!(a.setup().identity.layout, comment.setup().identity.layout);
    let rebound = sampler_ksp::compile_with(
        source,
        48_000,
        KspLimits::LIBRARY,
        &[("$button", ControlId(901))],
        &Environment::default(),
    )
    .unwrap();
    assert_ne!(a.setup().identity.layout, rebound.setup().identity.layout);
    let context = sampler_ksp::compile_with(
        source,
        48_000,
        KspLimits::LIBRARY,
        &[],
        &Environment {
            groups: vec!["different physical group".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_ne!(a.setup().identity.context, context.setup().identity.context);
    assert!(a.setup().fields.iter().all(|f| f.name != "$note"));
    assert_eq!(
        a.setup()
            .fields
            .iter()
            .find(|f| f.name == "@text")
            .unwrap()
            .kind,
        SetupValueKind::Text
    );
    let initialized =
        sampler_ksp::initialize(source, KspLimits::LIBRARY, &Environment::default()).unwrap();
    assert!(
        sampler_ksp::compile_initialized(
            &format!("{source}\n"),
            48_000,
            KspLimits::LIBRARY,
            &[],
            initialized
        )
        .is_err()
    );
}
#[cfg(feature = "cache")]
#[test]
fn cached_source_identity_and_explicit_zero_write_cannot_be_confused_with_absence() {
    let source = "on init set_snapshot_type(0) declare $x := 4 end on";
    let init =
        sampler_ksp::initialize(source, KspLimits::LIBRARY, &Environment::default()).unwrap();
    let bytes = serde_json::to_vec(&init.capture_initialized().unwrap()).unwrap();
    let cached = serde_json::from_slice(&bytes).unwrap();
    assert!(
        sampler_ksp::restore_initialized(&format!("{source}\n"), KspLimits::LIBRARY, cached)
            .is_err()
    );
    let cached = serde_json::from_slice(&bytes).unwrap();
    let restored = sampler_ksp::restore_initialized(source, KspLimits::LIBRARY, cached).unwrap();
    let restored =
        sampler_ksp::compile_initialized(source, 48_000, KspLimits::LIBRARY, &[], restored)
            .unwrap();
    assert_eq!(
        restored.setup().identity,
        compile(source, 0).setup().identity
    );
    assert_eq!(
        restored.model().snapshot_mode_write.unwrap().mode,
        SnapshotMode::NativeWithInit
    );
    let mut old: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    old.as_object_mut().unwrap().remove("source_digest");
    assert!(serde_json::from_value::<sampler_ksp::CachedInit>(old).is_err());
}
const SOURCE: &str = "on init
 set_snapshot_type(1)
 declare $saved := 11 make_persistent($saved)
 declare $instrument := 22 make_instr_persistent($instrument)
 declare $private := 33 declare @private_text := \"initial\"
 declare polyphonic $per_note
 declare ui_switch $tab declare $calls
 set_control_par(get_ui_id($tab),$CONTROL_PAR_HIDE,0)
 set_control_par_str(get_ui_id($tab),$CONTROL_PAR_PICTURE,\"initial.png\")
 end on
 on ui_control($tab)
 $saved := 111 $instrument := 222 $private := 333 @private_text := \"live\"
 set_control_par(get_ui_id($tab),$CONTROL_PAR_HIDE,1)
 set_control_par_str(get_ui_id($tab),$CONTROL_PAR_PICTURE,\"live.png\")
 end on
 on persistence_changed inc($calls) end on
 on note $per_note := 77 end on";
fn runtime() -> (Runtime, ScriptView) {
    let script = compile(SOURCE, 0);
    let view = script.view();
    let plan = script
        .bind(Prepared::new(48_000, vec![], vec![], 0).unwrap())
        .unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    (Runtime::new(plan, limits).unwrap(), view)
}
fn cell(view: &ScriptView, name: &str) -> u32 {
    let field = view.setup().fields.iter().find(|f| f.name == name).unwrap();
    let Location::Cells { offset, len: 1 } = field.storage else {
        panic!("numeric scalar")
    };
    offset
}
#[test]
fn persistent_only_loses_private_setup_and_value_capture_is_explicitly_not_ui_store_capture() {
    let (mut live, mut view) = runtime();
    let plan = live.active_plan();
    let tab = sampler_ksp::derived_control_id(0, "$tab");
    let context = ControlContext {
        performance: live.performance(0).unwrap(),
        origin: ChannelAddress {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        },
        channels: 1,
    };
    live.invoke_control(
        context,
        plan,
        None,
        ControlWrite {
            id: tab,
            value: ControlValue::Integer(1),
        },
    )
    .unwrap();
    let mut hide_key = None;
    live.drain_effects(|e| {
        if view.service(e.service) == Some("set_control_par") {
            hide_key = Some([
                e.args[0] as i32,
                e.args[1] as i32,
                sampler_ksp::PROPERTY_TAG,
                sampler_ksp::PROPERTY_TAG,
            ]);
        }
        view.apply_ui_effect(e);
        true
    });
    assert_eq!(
        view.model().interface.widgets[0].text("$CONTROL_PAR_PICTURE"),
        Some("live.png")
    );
    let hide_key = hide_key.unwrap();
    assert_eq!(
        live.script_store(plan, ScriptInstanceId(0), hide_key)
            .unwrap(),
        Some(1)
    );
    let mut persistent = sampler_ksp::persistent_state_buffer(&[view.clone()]).unwrap();
    live.capture_script_state(plan, &mut persistent).unwrap();
    let private = cell(&view, "$private");
    assert!(!persistent.values.iter().any(|e| e.address
        == ScriptStateAddress::Cell {
            instance: ScriptInstanceId(0),
            index: private
        }));
    let (mut partial, _) = runtime();
    let target = partial.active_plan();
    partial
        .restore_script_state(target, None, &mut persistent)
        .unwrap();
    assert_eq!(
        partial
            .script_cell(target, ScriptInstanceId(0), private)
            .unwrap(),
        33
    );
    let mut roster = capture_roster(
        &[view.clone()],
        live.widget_definitions(plan).unwrap(),
        limits(),
    )
    .unwrap();
    assert!(
        roster
            .stores
            .iter()
            .any(|s| s.domain == SetupStoreDomain::PrivateNumeric)
    );
    assert!(
        roster
            .stores
            .iter()
            .any(|s| s.domain == SetupStoreDomain::PrivateTextProperties)
    );
    assert!(
        roster
            .stores
            .iter()
            .any(|s| s.domain == SetupStoreDomain::SharedPgsNumeric)
    );
    assert!(
        view.setup()
            .fields
            .iter()
            .find(|f| f.name == "$instrument")
            .is_some_and(|f| f.persistence == Persistence::Instrument)
    );
    let (mut candidate, fresh_view) = runtime();
    let candidate_plan = candidate.active_plan();
    assert_eq!(view.setup().identity, fresh_view.setup().identity);
    support::without_heap(|| {
        live.capture_script_state(plan, &mut roster.values).unwrap();
        candidate
            .restore_script_state(candidate_plan, None, &mut roster.values)
            .unwrap();
    });
    assert_eq!(
        candidate
            .script_cell(candidate_plan, ScriptInstanceId(0), private)
            .unwrap(),
        333
    );
    assert_eq!(
        candidate
            .script_text(candidate_plan, ScriptInstanceId(0), 0)
            .unwrap()
            .as_str(),
        "live"
    );
    assert_eq!(
        candidate.control_base_value(candidate_plan, tab).unwrap(),
        ControlValue::Integer(1)
    );
    // This is the boundary the future full transaction MUST close. Existing value
    // restoration retains neither private numeric/text UI stores nor live models.
    assert_eq!(
        candidate
            .script_store(candidate_plan, ScriptInstanceId(0), hide_key)
            .unwrap(),
        Some(0)
    );
    assert_eq!(
        fresh_view.model().interface.widgets[0].text("$CONTROL_PAR_PICTURE"),
        Some("initial.png")
    );
    let bad = SetupCaptureLimits {
        value_entries: 1,
        ..limits()
    };
    assert!(matches!(
        capture_roster(&[view.clone()], live.widget_definitions(plan).unwrap(), bad),
        Err(Error::Capacity)
    ));
    let bad = SetupCaptureLimits {
        text_bytes: 0,
        ..limits()
    };
    assert!(matches!(
        capture_roster(&[view.clone()], live.widget_definitions(plan).unwrap(), bad),
        Err(Error::Capacity)
    ));
    let bad = SetupCaptureLimits {
        sparse_entries: 0,
        ..limits()
    };
    assert!(matches!(
        capture_roster(&[view], live.widget_definitions(plan).unwrap(), bad),
        Err(Error::Capacity)
    ));
}
