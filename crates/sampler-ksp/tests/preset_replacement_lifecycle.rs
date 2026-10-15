//! Complete incoming preset replacement on one authored, retained setup.
//! No installed libraries, sample payloads, file reads or native-parity claim.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

use sampler_core::*;
use sampler_ir::Saved;
use sampler_ksp::{Environment, ScriptView, model::Location, setup::*};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const NATIVE: ControlId = ControlId(0x7010);
const SOURCE: &str = r#"on init
    set_snapshot_type(3)
    declare ui_knob $level(0,127,1) make_persistent($level)
    declare $number := 1 make_persistent($number)
    declare ~real := 1.5 make_persistent(~real)
    declare %ints[2] := (1,2) make_persistent(%ints)
    declare ?reals[2] := (0.25,0.5) make_persistent(?reals)
    declare @title := "initial" make_persistent(@title)
    declare !texts[2] := ("first","second") make_persistent(!texts)
    declare $instrument := 2 make_instr_persistent($instrument)
    declare $ordinary := 3 declare @ordinary_text := "initial"
    declare ui_switch $edit declare $calls declare $seen declare $pgs_seen
    pgs_create_key(SHARED,1) pgs_set_key_val(SHARED,0,7)
    set_control_par(get_ui_id($edit),$CONTROL_PAR_HIDE,0)
    set_control_par_str(get_ui_id($edit),$CONTROL_PAR_PICTURE,"initial.png")
end on
on ui_control($edit)
    $instrument := 82 $ordinary := 93 @ordinary_text := "retained"
    $level := 126 $number := 999 %ints[0] := 998 %ints[1] := 997
    @title := "live edit longer than either preset"
    set_control_par(get_ui_id($edit),$CONTROL_PAR_HIDE,1)
    set_control_par_str(get_ui_id($edit),$CONTROL_PAR_PICTURE,"retained.png")
    pgs_set_key_val(SHARED,0,91)
end on
on persistence_changed
    inc($calls)
    $seen := $level + $number + %ints[0] + %ints[1]
    $pgs_seen := pgs_get_key_val(SHARED,0)
end on"#;

fn rows(a: bool) -> Vec<(&'static str, Saved)> {
    if a {
        vec![
            ("$level", Saved::Int(87)),
            ("$number", Saved::Int(42)),
            ("~real", Saved::Real(-2.75)),
            ("%ints", Saved::Ints(vec![13, -7])),
            ("?reals", Saved::Reals(vec![0.125, -0.0])),
            ("@title", Saved::Text("longer preset title".into())),
            ("!texts", Saved::Texts(vec!["left".into(), "right".into()])),
        ]
    } else {
        vec![
            ("$level", Saved::Int(0)),
            ("$number", Saved::Int(-21)),
            ("~real", Saved::Real(0.0)),
            ("%ints", Saved::Ints(vec![0, 0])),
            ("?reals", Saved::Reals(vec![-0.0, 0.0])),
            ("@title", Saved::Text("".into())),
            ("!texts", Saved::Texts(vec!["".into(), "x".into()])),
        ]
    }
}

fn saved(values: Vec<(&str, Saved)>) -> SnapshotSavedSlot {
    SnapshotSavedSlot {
        slot: 0,
        values: values
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect(),
    }
}
fn limits() -> SnapshotSavedLimits {
    SnapshotSavedLimits {
        value_entries: SNAPSHOT_OVERLAY_VALUE_CAPACITY,
        text_bytes: 8 << 20,
    }
}

struct Fixture {
    runtime: Runtime,
    views: Vec<ScriptView>,
    owner: Arc<AtomicU64>,
    sequence: Arc<AtomicU64>,
}
impl Fixture {
    fn new() -> Self {
        let script = sampler_ksp::compile_with(
            SOURCE,
            48_000,
            sampler_ksp::Limits::LIBRARY,
            &[],
            &Environment::default(),
        )
        .unwrap();
        let views = vec![script.view()];
        let base = Prepared::new(48_000, vec![], vec![], 0)
            .unwrap()
            .with_controls(vec![ControlDefinition {
                id: NATIVE,
                domain: ControlDomain::Integer { min: 0, max: 100 },
                default: ControlValue::Integer(17),
            }])
            .unwrap();
        let prepared = script.bind(base).unwrap();
        let capacity = Limits::for_plan(&prepared, 4, 4);
        Self {
            runtime: Runtime::new(prepared, capacity).unwrap(),
            views,
            owner: Arc::new(AtomicU64::new(1)),
            sequence: Arc::new(AtomicU64::new(1)),
        }
    }
    fn overlay(&self, a: bool) -> NativeSnapshotOverlay {
        let state = snapshot_saved_state(&self.views, &[saved(rows(a))], limits()).unwrap();
        let mut overlay = NativeSnapshotOverlay::linearized(
            self.runtime.active_plan(),
            state,
            Box::from([ControlWrite {
                id: NATIVE,
                value: ControlValue::Integer(if a { 67 } else { 0 }),
            }]),
        );
        overlay.epochs = Box::from([
            SnapshotOverlayEpoch {
                sequence: self.owner.clone(),
                expected: self.owner.load(Ordering::Acquire),
            },
            SnapshotOverlayEpoch {
                sequence: self.sequence.clone(),
                expected: self.sequence.load(Ordering::Acquire),
            },
        ]);
        overlay
    }
    fn cell(&self, name: &str) -> i64 {
        let Location::Cells { offset, len: 1 } = self.field(name) else {
            panic!("scalar cell")
        };
        self.runtime
            .script_cell(self.runtime.active_plan(), ScriptInstanceId(0), offset)
            .unwrap()
    }
    fn field(&self, name: &str) -> Location {
        self.views[0]
            .setup()
            .fields
            .iter()
            .find(|f| f.name == name)
            .unwrap()
            .storage
            .clone()
    }
    fn edit(&mut self) {
        let plan = self.runtime.active_plan();
        self.runtime
            .invoke_control(
                ControlContext {
                    performance: self.runtime.performance(0).unwrap(),
                    origin: ChannelAddress {
                        protocol: Protocol::Native,
                        port: 0,
                        group: 0,
                        channel: 0,
                    },
                    channels: 1,
                },
                plan,
                None,
                ControlWrite {
                    id: sampler_ksp::derived_control_id(0, "$edit"),
                    value: ControlValue::Integer(1),
                },
            )
            .unwrap();
        sampler_ksp::service_ui_effects(&mut self.runtime, &mut self.views).unwrap();
    }
    fn assert_incoming(&self, a: bool) {
        let plan = self.runtime.active_plan();
        // Independent value oracle: read every declared home, not just a knob
        // or a fingerprint of the mapper's own generated transaction.
        for (name, expected) in rows(a) {
            match (self.field(name), expected) {
                (Location::Control(id), Saved::Int(value)) => assert_eq!(
                    self.runtime.control_base_value(plan, id).unwrap(),
                    ControlValue::Integer(value),
                    "{name}"
                ),
                (Location::Cells { offset, len }, Saved::Int(value)) => {
                    assert_eq!(len, 1);
                    assert_eq!(
                        self.runtime
                            .script_cell(plan, ScriptInstanceId(0), offset)
                            .unwrap(),
                        value,
                        "{name}"
                    );
                }
                (Location::Cells { offset, len }, Saved::Real(value)) => {
                    assert_eq!(len, 1);
                    assert_eq!(
                        self.runtime
                            .script_cell(plan, ScriptInstanceId(0), offset)
                            .unwrap(),
                        real_bits(value),
                        "{name}"
                    );
                }
                (Location::Cells { offset, len }, Saved::Ints(values)) => {
                    assert_eq!(len as usize, values.len());
                    for (at, value) in values.into_iter().enumerate() {
                        assert_eq!(
                            self.runtime
                                .script_cell(plan, ScriptInstanceId(0), offset + at as u32)
                                .unwrap(),
                            value,
                            "{name}[{at}]"
                        );
                    }
                }
                (Location::Cells { offset, len }, Saved::Reals(values)) => {
                    assert_eq!(len as usize, values.len());
                    for (at, value) in values.into_iter().enumerate() {
                        assert_eq!(
                            self.runtime
                                .script_cell(plan, ScriptInstanceId(0), offset + at as u32)
                                .unwrap(),
                            real_bits(value),
                            "{name}[{at}]"
                        );
                    }
                }
                (Location::Texts { offset, len }, Saved::Text(value)) => {
                    assert_eq!(len, 1);
                    assert_eq!(
                        self.runtime
                            .script_text(plan, ScriptInstanceId(0), offset)
                            .unwrap()
                            .as_str(),
                        value,
                        "{name}"
                    );
                }
                (Location::Texts { offset, len }, Saved::Texts(values)) => {
                    assert_eq!(len as usize, values.len());
                    for (at, value) in values.into_iter().enumerate() {
                        assert_eq!(
                            self.runtime
                                .script_text(plan, ScriptInstanceId(0), offset + at as u32)
                                .unwrap()
                                .as_str(),
                            value,
                            "{name}[{at}]"
                        );
                    }
                }
                _ => panic!("unexpected authored home for {name}"),
            }
        }
        assert_eq!(
            self.runtime.control_base_value(plan, NATIVE).unwrap(),
            ControlValue::Integer(if a { 67 } else { 0 })
        );
        assert_eq!(
            self.cell("$seen"),
            if a { 135 } else { -21 },
            "callback observes the complete incoming batch"
        );
    }
    fn dense_signature(&self) -> Vec<(ScriptStateAddress, String)> {
        let mut roster = capture_roster(
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
        self.runtime
            .capture_script_state(self.runtime.active_plan(), &mut roster)
            .unwrap();
        let mut values: Vec<_> = roster
            .values
            .iter()
            .map(|e| {
                (
                    e.address,
                    roster
                        .text(e.value)
                        .map_or_else(|| format!("{:?}", e.value), |s| s.as_str().to_owned()),
                )
            })
            .collect();
        values.push((
            ScriptStateAddress::Control(NATIVE),
            format!(
                "{:?}",
                self.runtime
                    .control_base_value(self.runtime.active_plan(), NATIVE)
                    .unwrap()
            ),
        ));
        values
    }
}

#[test]
fn a_b_a_replaces_every_incoming_home_without_accumulating_state_or_recreating_setup() {
    let mut f = Fixture::new();
    let runtime = f.runtime.id();
    let plan = f.runtime.active_plan();
    let setup = f.views[0].setup() as *const _;
    let initial_calls = f.cell("$calls");
    let mut a_state = None;
    for (index, a) in [true, false, true, false, true, false, true]
        .into_iter()
        .enumerate()
    {
        f.edit();
        let mut incoming = f.overlay(a);
        support::without_heap(|| {
            f.runtime.apply_snapshot_overlay(&mut incoming).unwrap();
        });
        assert!(incoming.committed.is_some());
        assert!(
            incoming
                .state
                .callbacks
                .iter()
                .all(|c| c.outcome == Some(Outcome::Finished))
        );
        f.assert_incoming(a);
        assert_eq!(f.cell("$calls"), initial_calls + index as i64 + 1);
        assert_eq!(f.cell("$instrument"), 82);
        assert_eq!(f.cell("$ordinary"), 93);
        assert_eq!(f.cell("$pgs_seen"), 91);
        let Location::Texts { offset, len: 1 } = f.field("@ordinary_text") else {
            panic!("private text")
        };
        assert_eq!(
            f.runtime
                .script_text(plan, ScriptInstanceId(0), offset)
                .unwrap()
                .as_str(),
            "retained"
        );
        assert_eq!(f.runtime.id(), runtime);
        assert_eq!(f.runtime.active_plan(), plan);
        assert_eq!(f.views[0].setup() as *const _, setup);
        assert_eq!(
            f.views[0].model().interface.widgets[1].text("$CONTROL_PAR_PICTURE"),
            Some("retained.png")
        );
        assert_eq!(
            f.views[0].model().interface.widgets[1]
                .properties
                .get("$CONTROL_PAR_HIDE"),
            Some(&sampler_ksp::model::Value::Int(1))
        );
        // Compare only incoming homes; callback counters intentionally evolve.
        let mut captured = snapshot_saved_state(&f.views, &[saved(rows(a))], limits()).unwrap();
        f.runtime.capture_script_state(plan, &mut captured).unwrap();
        let signature: Vec<_> = captured
            .values
            .iter()
            .map(|e| {
                (
                    e.address,
                    e.value,
                    captured.text(e.value).map(|s| s.as_str().to_owned()),
                )
            })
            .collect();
        if a {
            if let Some(expected) = &a_state {
                assert_eq!(&signature, expected);
            } else {
                a_state = Some(signature);
            }
        }
    }
}

#[test]
fn malformed_last_row_or_late_native_domain_failure_never_applies_a_prefix() {
    let mut f = Fixture::new();
    let mut initial = f.overlay(true);
    f.runtime.apply_snapshot_overlay(&mut initial).unwrap();
    let before = f.dense_signature();
    let fence = f
        .runtime
        .snapshot_overlay_fence(f.runtime.active_plan())
        .unwrap();
    for bad in [Saved::Texts(vec!["short".into()]), Saved::Ints(vec![7, 8])] {
        let mut values = rows(false);
        values.last_mut().unwrap().1 = bad;
        assert!(snapshot_saved_state(&f.views, &[saved(values)], limits()).is_err());
        assert_eq!(f.dense_signature(), before);
    }
    let mut missing = rows(false);
    missing.pop();
    assert!(snapshot_saved_state(&f.views, &[saved(missing)], limits()).is_err());
    let mut incoming = f.overlay(false);
    incoming.native[0].value = ControlValue::Integer(101);
    support::without_heap(|| {
        assert_eq!(
            f.runtime.apply_snapshot_overlay(&mut incoming),
            Err(Error::InvalidInput)
        );
    });
    assert_eq!(incoming.refusal, Some(SnapshotOverlayRefusal::InvalidInput));
    assert!(incoming.committed.is_none());
    assert!(
        incoming
            .state
            .callbacks
            .iter()
            .all(|c| c.behavior.is_none() && c.outcome.is_none())
    );
    assert_eq!(f.dense_signature(), before);
    assert_eq!(
        f.runtime
            .snapshot_overlay_fence(f.runtime.active_plan())
            .unwrap(),
        fence
    );
}

#[test]
fn owner_and_request_cancellation_after_preparation_each_refuse_wholly() {
    for cancel_owner in [false, true] {
        let mut f = Fixture::new();
        f.edit();
        let before = f.dense_signature();
        let fence = f
            .runtime
            .snapshot_overlay_fence(f.runtime.active_plan())
            .unwrap();
        let mut incoming = f.overlay(false);
        let epoch = if cancel_owner { &f.owner } else { &f.sequence };
        epoch.fetch_add(1, Ordering::Release);
        support::without_heap(|| {
            assert_eq!(
                f.runtime.apply_snapshot_overlay(&mut incoming),
                Err(Error::RevisionConflict)
            );
        });
        assert_eq!(incoming.refusal, Some(SnapshotOverlayRefusal::StaleEpoch));
        assert!(incoming.committed.is_none());
        assert_eq!(
            f.runtime
                .snapshot_overlay_fence(f.runtime.active_plan())
                .unwrap(),
            fence
        );
        assert_eq!(f.dense_signature(), before);
        assert!(
            incoming
                .state
                .callbacks
                .iter()
                .all(|c| c.behavior.is_none() && c.outcome.is_none())
        );
    }
}
