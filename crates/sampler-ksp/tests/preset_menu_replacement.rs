//! Authored sparse-slot menus: native ordinals and semantic values are distinct.
//! No sample assets, installed libraries, native applications or file I/O.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::*;
use sampler_ir::Saved;
use sampler_ksp::{Environment, ScriptView, model::Location, setup::*};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const NATIVE: ControlId = ControlId(0x7020);
const SOURCE: &str = r#"on init
    set_snapshot_type(3)
    declare ui_menu $menu
    add_menu_item($menu,"same",20) add_menu_item($menu,"same",80) add_menu_item($menu,"same",-7)
    make_persistent($menu)
    declare ui_menu $other
    add_menu_item($other,"B",300) add_menu_item($other,"A",0)
    make_persistent($other)
    declare $saved := 1 make_persistent($saved)
    declare %array[2] := (1,2) make_persistent(%array)
    declare @text := "initial" make_persistent(@text)
    declare $private := 17 make_instr_persistent($private)
    declare $ordinary := 19 declare $calls declare $seen declare $pgs_seen declare @caption
    declare ui_switch $mutate declare ui_switch $label declare ui_switch $select declare ui_switch $roster
    pgs_create_key(KEEP,1) pgs_set_key_val(KEEP,0,7)
end on
on ui_control($mutate)
    set_menu_item_value(get_ui_id($menu),0,41) set_menu_item_value(get_ui_id($menu),1,-105)
    set_menu_item_value(get_ui_id($other),0,-301) set_menu_item_value(get_ui_id($other),1,0)
    set_menu_item_str(get_ui_id($menu),1,"renamed") set_menu_item_visibility(get_ui_id($menu),1,0)
    $private := 71 $ordinary := 91 pgs_set_key_val(KEEP,0,97)
    set_control_par_str(get_ui_id($mutate),$CONTROL_PAR_PICTURE,"retained.png")
end on
on ui_control($roster)
    set_menu_item_value(get_ui_id($menu),1,-106)
end on
on ui_control($label)
    set_menu_item_str(get_ui_id($menu),1,"caption only")
end on
on ui_control($select)
    $menu := 20 $other := 300 $saved := 999 %array[0] := 998 %array[1] := 997 @text := "ui"
end on
on persistence_changed
    inc($calls) $seen := $menu + $other + $saved + %array[0] + %array[1]
    $pgs_seen := pgs_get_key_val(KEEP,0) @caption := get_menu_item_str(get_ui_id($menu),1)
end on"#;

fn bounds() -> SnapshotSavedLimits {
    SnapshotSavedLimits {
        value_entries: SNAPSHOT_OVERLAY_VALUE_CAPACITY,
        text_bytes: 8 << 20,
    }
}
fn rows(a: bool) -> Vec<SnapshotSavedSlot> {
    [0, 4]
        .into_iter()
        .map(|slot| SnapshotSavedSlot {
            slot,
            values: vec![
                ("$menu".into(), Saved::Int(if a { 1 } else { 2 })),
                ("$other".into(), Saved::Int(if a { 0 } else { 1 })),
                ("$saved".into(), Saved::Int(if a { 11 } else { -3 })),
                (
                    "%array".into(),
                    Saved::Ints(if a { vec![5, -2] } else { vec![0, 0] }),
                ),
                (
                    "@text".into(),
                    Saved::Text(if a { "A title" } else { "" }.into()),
                ),
            ],
        })
        .collect()
}
struct Fixture {
    rt: Runtime,
    views: Vec<ScriptView>,
    owner: Arc<AtomicU64>,
    request: Arc<AtomicU64>,
}
impl Fixture {
    fn new() -> Self {
        let scripts: Vec<_> = [0, 4]
            .into_iter()
            .map(|slot| {
                sampler_ksp::compile_with(
                    SOURCE,
                    48000,
                    sampler_ksp::Limits::LIBRARY,
                    &[],
                    &Environment {
                        slot,
                        ..Default::default()
                    },
                )
                .unwrap()
            })
            .collect();
        let views = scripts.iter().map(|s| s.view()).collect();
        let base = Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_controls(vec![ControlDefinition {
                id: NATIVE,
                domain: ControlDomain::Integer { min: 0, max: 100 },
                default: ControlValue::Integer(17),
            }])
            .unwrap();
        let prepared = sampler_ksp::bind_modules(scripts, base).unwrap();
        let limits = Limits::for_plan(&prepared, 8, 8);
        Self {
            rt: Runtime::new(prepared, limits).unwrap(),
            views,
            owner: Arc::new(AtomicU64::new(1)),
            request: Arc::new(AtomicU64::new(1)),
        }
    }
    fn overlay(&self, a: bool, linearized: bool) -> NativeSnapshotOverlay {
        let mapped = snapshot_saved_state_with_menus(&self.views, &rows(a), bounds()).unwrap();
        let mut overlay = if linearized {
            NativeSnapshotOverlay::linearized(self.rt.active_plan(), mapped.state, Box::new([]))
        } else {
            NativeSnapshotOverlay::new(
                self.rt
                    .snapshot_overlay_fence(self.rt.active_plan())
                    .unwrap(),
                mapped.state,
                Box::new([]),
            )
        };
        overlay.menus = mapped.menus;
        overlay.native = Box::from([ControlWrite {
            id: NATIVE,
            value: ControlValue::Integer(if a { 76 } else { 0 }),
        }]);
        overlay.epochs = Box::from([
            SnapshotOverlayEpoch {
                sequence: self.owner.clone(),
                expected: 1,
            },
            SnapshotOverlayEpoch {
                sequence: self.request.clone(),
                expected: 1,
            },
        ]);
        overlay
    }
    fn context(&self) -> ControlContext {
        ControlContext {
            performance: self.rt.performance(0).unwrap(),
            origin: ChannelAddress {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
            },
            channels: 1,
        }
    }
    fn edit(&mut self, name: &str) {
        for slot in [0, 4] {
            self.rt
                .invoke_widget(
                    self.context(),
                    self.rt.active_plan(),
                    None,
                    &[WidgetEdit {
                        id: sampler_ksp::derived_control_id(slot, name),
                        index: 0,
                        value: WidgetValue::Integer(1),
                        interaction: WidgetInteraction::default(),
                    }],
                )
                .unwrap();
        }
    }
    fn service(&mut self) {
        sampler_ksp::service_ui_effects(&mut self.rt, &mut self.views).unwrap();
    }
    fn cell(&self, instance: usize, name: &str) -> i64 {
        let Location::Cells { offset, len: 1 } = self.field(instance, name) else {
            panic!("scalar")
        };
        self.rt
            .script_cell(
                self.rt.active_plan(),
                ScriptInstanceId(instance as u16),
                offset,
            )
            .unwrap()
    }
    fn field(&self, instance: usize, name: &str) -> Location {
        self.views[instance]
            .setup()
            .fields
            .iter()
            .find(|field| field.name == name)
            .unwrap()
            .storage
            .clone()
    }
    fn text(&self, instance: usize, name: &str) -> String {
        let Location::Texts { offset, len: 1 } = self.field(instance, name) else {
            panic!("text")
        };
        self.rt
            .script_text(
                self.rt.active_plan(),
                ScriptInstanceId(instance as u16),
                offset,
            )
            .unwrap()
            .as_str()
            .into()
    }
    fn signature(&self) -> Vec<(ScriptStateAddress, ScriptStateValue, Option<String>)> {
        let mut state = capture_roster(
            &self.views,
            &[],
            SetupCaptureLimits {
                value_entries: 8192,
                text_bytes: 8 << 20,
                sparse_entries: 1 << 20,
            },
        )
        .unwrap()
        .values;
        self.rt
            .capture_script_state(self.rt.active_plan(), &mut state)
            .unwrap();
        let mut signature: Vec<_> = state
            .values
            .iter()
            .map(|row| {
                (
                    row.address,
                    row.value,
                    state.text(row.value).map(|s| s.as_str().into()),
                )
            })
            .collect();
        signature.push((
            ScriptStateAddress::Control(NATIVE),
            ScriptStateValue::Control(
                self.rt
                    .control_base_value(self.rt.active_plan(), NATIVE)
                    .unwrap(),
            ),
            None,
        ));
        signature
    }
    fn untouched(
        &self,
        before: &[(ScriptStateAddress, ScriptStateValue, Option<String>)],
        overlay: &NativeSnapshotOverlay,
    ) {
        assert_eq!(self.signature(), before);
        assert!(overlay.committed.is_none());
        assert!(
            overlay
                .state
                .callbacks
                .iter()
                .all(|c| c.behavior.is_none() && c.outcome.is_none())
        );
    }
}

#[test]
fn sparse_menus_a_b_a_use_current_values_and_keep_labels_private_state_and_typed_readback() {
    let mut f = Fixture::new();
    let owner = f.rt.id();
    let plan = f.rt.active_plan();
    let setups: Vec<_> = f.views.iter().map(|v| v.setup() as *const _).collect();
    let initial_calls = [f.cell(0, "$calls"), f.cell(1, "$calls")];
    assert_eq!(
        initial_calls,
        [1, 1],
        "initial persistence callback per slot"
    );
    for (index, a) in [true, false, true].into_iter().enumerate() {
        // Prepare while the model still has its old menu labels/values. The
        // later audio-owned edit, NOT this frontend model, owns interpretation.
        let mut incoming = f.overlay(a, true);
        if index == 0 {
            f.edit("$mutate");
        }
        f.edit("$select");
        // Deliberately defer MODEL projection while draining the ordinary effect
        // outbox. Admission must read the sparse audio roster, not stale labels.
        let mut deferred = Vec::new();
        f.rt.drain_effects(|effect| {
            deferred.push(*effect);
            true
        });
        if index == 1 {
            f.edit("$label");
            f.service();
        }
        support::without_heap(|| {
            f.rt.apply_snapshot_overlay(&mut incoming).unwrap();
        });
        for effect in &deferred {
            let instance = effect.instance.unwrap();
            f.views[usize::from(instance.0)].apply_ui_effect_for(plan, instance, effect);
        }
        assert_eq!(incoming.refusal, None);
        assert!(incoming.committed.is_some());
        assert_eq!(incoming.state.callbacks.len(), 2);
        assert!(
            incoming
                .state
                .callbacks
                .iter()
                .all(|c| c.outcome == Some(Outcome::Finished))
        );
        let expected_menu = if a { -105 } else { -7 };
        let expected_other = if a { -301 } else { 0 };
        let expected_saved = if a { 11 } else { -3 };
        for (instance, slot) in [0, 4].into_iter().enumerate() {
            let mut output = [
                WidgetEdit {
                    id: sampler_ksp::derived_control_id(slot, "$menu"),
                    index: 0,
                    value: WidgetValue::Integer(0),
                    interaction: WidgetInteraction::default(),
                },
                WidgetEdit {
                    id: sampler_ksp::derived_control_id(slot, "$other"),
                    index: 0,
                    value: WidgetValue::Integer(0),
                    interaction: WidgetInteraction::default(),
                },
            ];
            support::without_heap(|| {
                f.rt.capture_widgets(plan, &mut output).unwrap();
            });
            assert_eq!(output[0].value, WidgetValue::Integer(expected_menu));
            assert_eq!(output[1].value, WidgetValue::Integer(expected_other));
            assert_eq!(f.cell(instance, "$saved"), expected_saved);
            assert_eq!(
                f.cell(instance, "$seen"),
                expected_menu + expected_other + expected_saved + if a { 3 } else { 0 }
            );
            assert_eq!(
                f.cell(instance, "$calls"),
                initial_calls[instance] + index as i64 + 1
            );
            assert_eq!(f.cell(instance, "$private"), 71);
            assert_eq!(f.cell(instance, "$ordinary"), 91);
            assert_eq!(f.cell(instance, "$pgs_seen"), 97);
            assert_eq!(f.text(instance, "@text"), if a { "A title" } else { "" });
            assert_eq!(
                f.text(instance, "@caption"),
                if index == 0 {
                    "renamed"
                } else {
                    "caption only"
                }
            );
            assert_eq!(f.views[instance].setup() as *const _, setups[instance]);
            assert_eq!(
                f.views[instance].model().interface.widgets[0].menu[1].value,
                -105
            );
            assert!(!f.views[instance].model().interface.widgets[0].menu[1].visible);
            assert_eq!(
                f.views[instance]
                    .model()
                    .interface
                    .widgets
                    .iter()
                    .find(|w| w.name == "$mutate")
                    .unwrap()
                    .text("$CONTROL_PAR_PICTURE"),
                Some("retained.png")
            );
            let Location::Cells { offset, len: 2 } = f.field(instance, "%array") else {
                panic!("array")
            };
            assert_eq!(
                f.rt.script_cell(plan, ScriptInstanceId(instance as u16), offset)
                    .unwrap(),
                if a { 5 } else { 0 }
            );
            assert_eq!(
                f.rt.script_cell(plan, ScriptInstanceId(instance as u16), offset + 1)
                    .unwrap(),
                if a { -2 } else { 0 }
            );
        }
        assert_eq!(f.rt.id(), owner);
        assert_eq!(f.rt.active_plan(), plan);
    }
}

#[test]
fn malformed_late_position_negative_wrong_type_missing_or_duplicate_rows_never_write_a_prefix() {
    let mut f = Fixture::new();
    let before = f.signature();
    for bad in [
        Saved::Int(-1),
        Saved::Real(1.0),
        Saved::Ints(vec![1]),
        Saved::Int(i64::MAX),
    ] {
        let mut input = rows(false);
        input[1].values[1].1 = bad;
        assert!(snapshot_saved_state_with_menus(&f.views, &input, bounds()).is_err());
        assert_eq!(f.signature(), before);
    }
    let mut missing = rows(false);
    missing[1].values.pop();
    assert!(snapshot_saved_state_with_menus(&f.views, &missing, bounds()).is_err());
    let mut duplicate = rows(false);
    let repeated = duplicate[1].values[1].clone();
    duplicate[1].values.push(repeated);
    assert!(snapshot_saved_state_with_menus(&f.views, &duplicate, bounds()).is_err());
    for position in [-1, 2, i32::MAX] {
        let mut incoming = f.overlay(false, true);
        // Last sorted menu row fails AFTER every earlier row/domain was checked.
        let last = incoming.menus.last_mut().unwrap();
        last.position = if position == 2 { 99 } else { position };
        let fence = f.rt.snapshot_overlay_fence(f.rt.active_plan()).unwrap();
        support::without_heap(|| {
            assert_eq!(
                f.rt.apply_snapshot_overlay(&mut incoming),
                Err(Error::InvalidInput)
            );
        });
        assert_eq!(incoming.refusal, Some(SnapshotOverlayRefusal::InvalidInput));
        assert_eq!(
            f.rt.snapshot_overlay_fence(f.rt.active_plan()).unwrap(),
            fence
        );
        f.untouched(&before, &incoming);
    }
    for source in [
        "on init set_snapshot_type(3) declare ui_menu $menu make_persistent($menu) end on",
        "on init set_snapshot_type(3) declare ui_menu $menu add_menu_item($menu,\"zero\",0) make_persistent($menu) end on",
    ] {
        let script =
            sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let views = vec![script.view()];
        let empty = views[0].model().interface.widgets[0].menu.is_empty();
        let prepared = script
            .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
            .unwrap();
        let limits = Limits::for_plan(&prepared, 4, 4);
        let mut rt = Runtime::new(prepared, limits).unwrap();
        let mapped = snapshot_saved_state_with_menus(
            &views,
            &[SnapshotSavedSlot {
                slot: 0,
                values: vec![("$menu".into(), Saved::Int(0))],
            }],
            bounds(),
        )
        .unwrap();
        let mut overlay = NativeSnapshotOverlay::new(
            rt.snapshot_overlay_fence(rt.active_plan()).unwrap(),
            mapped.state,
            Box::new([]),
        );
        overlay.menus = mapped.menus;
        let menu = sampler_ksp::derived_control_id(0, "$menu");
        support::without_heap(|| {
            if empty {
                assert_eq!(
                    rt.apply_snapshot_overlay(&mut overlay),
                    Err(Error::InvalidInput)
                );
            } else {
                rt.apply_snapshot_overlay(&mut overlay).unwrap();
                assert_eq!(
                    rt.widget_value(rt.active_plan(), menu, 0),
                    Ok(WidgetValue::Integer(0))
                );
            }
        });
    }
    let mut nonmenu = f.overlay(false, true);
    nonmenu.menus = Box::from([SnapshotMenuPosition::new(
        sampler_ksp::derived_control_id(0, "$select"),
        0,
    )]);
    support::without_heap(|| {
        assert_eq!(
            f.rt.apply_snapshot_overlay(&mut nonmenu),
            Err(Error::InvalidInput)
        );
    });
    f.untouched(&before, &nonmenu);
    let mut overlap = f.overlay(false, true);
    overlap.native = Box::from([ControlWrite {
        id: overlap.menus[0].control,
        value: ControlValue::Integer(20),
    }]);
    support::without_heap(|| {
        assert_eq!(
            f.rt.apply_snapshot_overlay(&mut overlap),
            Err(Error::InvalidInput)
        );
    });
    f.untouched(&before, &overlap);
}

#[test]
fn stale_captured_roster_control_owner_and_request_refuse_while_fifo_linearized_edits_precede_replacement()
 {
    for change in ["$roster", "$label", "$select"] {
        let mut f = Fixture::new();
        let mut incoming = f.overlay(true, false);
        if change == "$select" {
            f.edit(change);
        } else {
            let plan = f.rt.active_plan();
            let revision = f.rt.control_revision(plan).unwrap();
            let id = sampler_ksp::derived_control_id(0, change);
            let program =
                f.rt.widget_definitions(plan)
                    .unwrap()
                    .iter()
                    .find(|w| w.id == id)
                    .unwrap()
                    .program
                    .unwrap();
            f.rt.start_plan_behavior(plan, program).unwrap();
            assert_eq!(
                f.rt.control_revision(plan).unwrap(),
                revision,
                "roster/label only changed script fence"
            );
        }
        f.service();
        let before = f.signature();
        support::without_heap(|| {
            assert_eq!(
                f.rt.apply_snapshot_overlay(&mut incoming),
                Err(Error::RevisionConflict)
            );
        });
        assert_eq!(
            incoming.refusal,
            Some(SnapshotOverlayRefusal::RevisionConflict)
        );
        f.untouched(&before, &incoming);
    }
    for owner in [false, true] {
        let mut f = Fixture::new();
        let mut incoming = f.overlay(true, true);
        if owner {
            f.owner.fetch_add(1, Ordering::Release);
        } else {
            f.request.fetch_add(1, Ordering::Release);
        }
        let before = f.signature();
        support::without_heap(|| {
            assert_eq!(
                f.rt.apply_snapshot_overlay(&mut incoming),
                Err(Error::RevisionConflict)
            );
        });
        assert_eq!(incoming.refusal, Some(SnapshotOverlayRefusal::StaleEpoch));
        f.untouched(&before, &incoming);
    }
    let mut f = Fixture::new();
    // Queue payload was prepared BEFORE the roster edit. FIFO order determines
    // the current roster, then the preset replaces the earlier selected scalar.
    let incoming = f.overlay(true, true);
    let plan = f.rt.active_plan();
    let context = f.context();
    let (mut rt, mut client) = f.rt.with_control_updates(4, 8199).unwrap();
    for name in ["$mutate", "$select"] {
        client
            .submit(ControlRequest {
                plan,
                expected_revision: None,
                operation: ControlOperation::InvokeWidget(
                    context,
                    vec![WidgetEdit {
                        id: sampler_ksp::derived_control_id(0, name),
                        index: 0,
                        value: WidgetValue::Integer(1),
                        interaction: WidgetInteraction::default(),
                    }],
                ),
            })
            .unwrap();
    }
    client
        .submit(ControlRequest {
            plan,
            expected_revision: None,
            operation: ControlOperation::NativeSnapshotOverlay(incoming),
        })
        .unwrap();
    support::without_heap(|| {
        rt.poll_control_update().unwrap();
        rt.poll_control_update().unwrap();
    });
    sampler_ksp::service_ui_effects(&mut rt, &mut f.views).unwrap();
    let first = client.reply().unwrap();
    let second = client.reply().unwrap();
    assert!(first.result.is_ok() && second.result.is_ok());
    support::without_heap(|| {
        rt.poll_control_update().unwrap();
    });
    let reply = client.reply().unwrap();
    assert!(reply.result.is_ok());
    let ControlOperation::NativeSnapshotOverlay(overlay) = reply.command.operation else {
        panic!("overlay receipt")
    };
    assert!(overlay.committed.is_some());
    assert_eq!(
        rt.widget_value(plan, sampler_ksp::derived_control_id(0, "$menu"), 0),
        Ok(WidgetValue::Integer(-105))
    );
    assert_eq!(
        rt.widget_value(plan, sampler_ksp::derived_control_id(4, "$menu"), 0),
        Ok(WidgetValue::Integer(80))
    );
}
