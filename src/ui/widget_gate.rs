//! Gate receipts contain only counts, source coordinates, hashes and failure enums.
//! Authored resources, values and serialized host state stay in this process.
use super::{native_ui, tests::Harness};
use crate::{
    plugin::{Load, Part, SamplerParams, Selection},
    sound::{Core, v2::V2Core},
};
use moose::{mui::mui::prelude::*, prelude::BackgroundTask};
use sampler_ui_ir as ir;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};

type Values = BTreeMap<String, ir::Value>;
thread_local! {
    static SUBMITTED: std::cell::RefCell<BTreeSet<String>> = const { std::cell::RefCell::new(BTreeSet::new()) };
}

pub(super) fn submitted_control(id: ir::ControlId) {
    SUBMITTED.with(|edits| {
        edits.borrow_mut().insert(format!("control-{}", id.0));
    });
}

pub(super) fn submitted_widget(widget: &ir::Widget, indices: &[u32]) {
    match &widget.binding {
        ir::Binding::Control(id) => submitted_control(*id),
        ir::Binding::Variable { script, name } => {
            let id = sampler_ksp::derived_control_id(*script, name);
            SUBMITTED.with(|edits| {
                let mut edits = edits.borrow_mut();
                if matches!(widget.kind, ir::Kind::Table { .. } | ir::Kind::Xy { .. }) {
                    edits.extend(
                        indices
                            .iter()
                            .map(|index| format!("typed-{}-{index}", id.0)),
                    );
                } else {
                    edits.insert(format!("typed-{}", id.0));
                }
            });
        }
        _ => {}
    }
    for index in indices {
        if let Some(id) = widget.components.get(*index as usize) {
            submitted_control(*id);
        }
    }
}

fn typed_values(out: &mut Values, id: ir::ControlId, value: &ir::Value) {
    match value {
        ir::Value::Integers(values) => {
            out.extend(values.iter().enumerate().map(|(index, value)| {
                (
                    format!("typed-{}-{index}", id.0),
                    ir::Value::Integer(*value),
                )
            }))
        }
        ir::Value::Reals(values) => out.extend(
            values
                .iter()
                .enumerate()
                .map(|(index, value)| (format!("typed-{}-{index}", id.0), ir::Value::Real(*value))),
        ),
        value => {
            out.insert(format!("typed-{}", id.0), value.clone());
        }
    }
}
fn advance_editor_frame(core: &mut V2Core, mut observe: impl FnMut(&mut V2Core)) {
    let mut remaining = 800;
    while remaining > 0 {
        let frames = remaining.min(crate::sound::MAX_BLOCK);
        core.render(frames);
        observe(core);
        remaining -= frames;
    }
}

#[test]
fn editor_frame_advances_the_full_engine_clock() {
    let plan = sampler_core::Prepared::new(48000, vec![], vec![], 0).unwrap();
    let limits = sampler_core::Limits::for_plan(&plan, 128, 8);
    let runtime = sampler_core::Runtime::new(plan, limits).unwrap();
    let part =
        crate::sound::v2::Part::new(runtime, crate::sound::tree::MixTree::default()).unwrap();
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, Some(Box::new(part)));
    let before = core.clock(0);
    for _ in 0..60 {
        advance_editor_frame(&mut core, |_| {});
    }
    assert_eq!(
        core.clock(0) - before,
        48000,
        "one second of editor time must advance one second of script time"
    );
}

struct Gate {
    params: Arc<SamplerParams>,
    core: V2Core,
    editor: Harness,
    observing: Option<Values>,
    observed_changes: BTreeSet<String>,
    submitted: BTreeSet<String>,
    frame: usize,
    current_target: Option<usize>,
    first_native_diagnostic: Option<(usize, Option<usize>)>,
    fault_events: Vec<serde_json::Value>,
    fault_cursor: usize,
    preemption_observations: Vec<serde_json::Value>,
    preemptions: u64,
    progress_truncated: bool,
    phase: &'static str,
}
impl Gate {
    fn load(selection: Selection) -> Self {
        let params = Arc::new(SamplerParams::new());
        *params.selection.write().unwrap() = selection;
        Load.run(&params);
        let mut core = V2Core::with_parts(1, 48000.);
        params.shared.widget_gate_install(&mut core);
        native_ui::gate_clear();
        let editor = Harness::new(&params, 1500., 1100.);
        let mut gate = Self {
            params,
            core,
            editor,
            observing: None,
            observed_changes: BTreeSet::new(),
            submitted: BTreeSet::new(),
            frame: 0,
            current_target: None,
            first_native_diagnostic: None,
            fault_events: Vec::new(),
            fault_cursor: 0,
            preemption_observations: Vec::new(),
            preemptions: 0,
            progress_truncated: false,
            phase: "load",
        };
        gate.settle();
        gate
    }
    fn tick(&mut self, input: Input) {
        self.editor.tick(input);
        let submitted = SUBMITTED.with(|edits| std::mem::take(&mut *edits.borrow_mut()));
        if self.observing.is_some() {
            self.submitted.extend(submitted);
        }
        self.frame += 1;
        if self.first_native_diagnostic.is_none() && !native_ui::gate_diagnostics().is_empty() {
            self.first_native_diagnostic = Some((self.frame, self.current_target));
        }
        // One 60 Hz editor frame advances the 48 kHz engine by the same time.
        let atoms = self.params.shared.part(0).unwrap();
        advance_editor_frame(&mut self.core, |core| {
            let sample = core.clock(0);
            let total = core.widget_gate_preemptions(0);
            core.widget_gate_behavior_progress(0, |p| {
                let fault = matches!(p.outcome, Some(sampler_core::Outcome::FuelExhausted | sampler_core::Outcome::Fault(_)));
                if !fault && (p.yielded_at.is_none() || total == self.preemptions) { return; }
                if self.preemption_observations.len() >= 128 { self.progress_truncated = true; return; }
                self.preemption_observations.push(serde_json::json!({"frame": self.frame, "target": self.current_target,
                    "phase": self.phase, "sample": sample, "aggregate_preemptions": total,
                    "program": p.program, "callback": atoms.widget_gate_callback(p.program), "pc": p.pc,
                    "owner": match p.owner { sampler_core::BehaviorOwner::Note(_) => "note", sampler_core::BehaviorOwner::Plan(_) => "plan" },
                    "waiting": p.waiting, "first_preemption_sample": p.yielded_at, "callers": p.callers,
                    "outcome": p.outcome.map(|outcome| format!("{outcome:?}"))}));
            });
            self.preemptions = total;
            for (program, outcome) in core.scan_runtime_faults(0) {
                self.fault_events.push(serde_json::json!({"frame": self.frame, "target": self.current_target,
                    "phase": self.phase, "sample": sample, "program": program,
                    "callback": atoms.widget_gate_callback(program), "outcome": format!("{outcome:?}")}));
            }
        });
        self.params.shared.widget_gate_readback(&mut self.core);
        Load.run(&self.params);
        if let Some(before) = self.observing.as_ref() {
            let keys = changed(before, &self.values(), &self.submitted);
            self.observed_changes.extend(keys);
        }
    }
    fn settle(&mut self) {
        for _ in 0..8 {
            self.tick(Input::default());
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn pointer(&mut self, p: Point, down: bool) {
        for _ in 0..2 {
            self.tick(Input {
                pointer: PointerInput {
                    pos: Some(p),
                    buttons: if down {
                        Buttons::PRIMARY
                    } else {
                        Buttons::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            });
        }
    }
    fn key(&mut self, key: Key, mods: Mods) {
        self.tick(Input {
            keys: vec![KeyPress { key, mods }],
            ..Default::default()
        });
        self.tick(Input::default());
    }
    fn values(&self) -> Values {
        let atoms = self.params.shared.part(0).unwrap();
        let mut out = Values::new();
        // Read the engine or Lua owner, never the renderer's optimistic preview.
        for (id, _) in atoms.control_values() {
            let value = atoms
                .widget_gate_uvi_value(id)
                .or_else(|| self.core.control_value(0, id));
            if let Some(value) = value {
                out.insert(format!("control-{}", id.0), ir::Value::Real(value));
            }
        }
        let backend = self.core.widget_gate_values(0);
        let view = self.params.shared.view.lock().unwrap();
        for face in view.parts[0].interfaces.iter() {
            for definition in &face.widgets {
                if let ir::Binding::Variable { script, name } = &definition.binding {
                    let id = ir::ControlId(sampler_ksp::derived_control_id(*script, name).0);
                    if let Some(value) = backend.get(&id) {
                        typed_values(&mut out, id, value);
                    }
                }
            }
        }
        out
    }
    fn targets(&self) -> Vec<(String, String)> {
        let mut targets: BTreeMap<String, String> = native_ui::gate_targets(0)
            .into_iter()
            .filter(|(id, _)| {
                self.editor
                    .ui
                    .scene()
                    .unwrap()
                    .surface(id)
                    .is_some_and(|s| !s.disabled)
            })
            .map(|(id, kind)| (id, format!("native-{kind}")))
            .collect();
        let view = self.params.shared.view.lock().unwrap();
        let part = &view.parts[0];
        for (source, face) in part.interfaces.iter().enumerate() {
            for (n, widget) in face.widgets.iter().enumerate() {
                if !widget.enabled || !face.visible(ir::WidgetRef(n)) {
                    continue;
                }
                let kind = match widget.kind {
                    ir::Kind::Knob { .. } => "knob",
                    ir::Kind::Slider { .. } => "slider",
                    ir::Kind::Button { .. } => "button",
                    ir::Kind::Switch => "switch",
                    ir::Kind::Menu { .. } => "menu",
                    ir::Kind::ValueEdit { .. } => "value-edit",
                    ir::Kind::TextEdit => "text",
                    ir::Kind::Table { .. } => "table",
                    ir::Kind::Xy { .. } => "xy",
                    ir::Kind::MouseArea => "mouse-area",
                    ir::Kind::FileSelector { .. } => "file-selector",

                    _ => continue,
                };
                let id = format!("part-0-epoch-{}-script-{source}-ir-{n}", part.generation);
                if self
                    .editor
                    .ui
                    .scene()
                    .unwrap()
                    .surface(&id)
                    .is_some_and(|s| !s.disabled)
                {
                    targets.insert(id, kind.into());
                }
            }
        }
        targets.into_iter().collect()
    }
    fn hit(&mut self, id: &str) -> Option<Point> {
        self.phase = "hit-test";
        let frame = self.editor.ui.scene()?.surface(id)?.frame;
        if frame.size.width <= 0. || frame.size.height <= 0. {
            return None;
        }
        for y in [0.5, 0.2, 0.8] {
            for x in [0.5, 0.2, 0.8] {
                let at = Point::new(
                    frame.x + frame.size.width * x,
                    frame.y + frame.size.height * y,
                );
                self.pointer(at, false);
                if self.editor.ui.get(id).hovered {
                    return Some(at);
                }
            }
        }
        None
    }
    fn faults(&mut self) -> usize {
        let p = self.core.problems(0);
        let lua = self
            .core
            .scan_lua(0)
            .map_or(0, |f| f.init_count + f.runtime_count + f.budget_hits);
        let runtime = self.fault_events.len() - self.fault_cursor;
        self.fault_cursor = self.fault_events.len();
        runtime + lua + (p.nonfinite + p.script_overruns + p.lua_faults) as usize
    }
    fn gesture(&mut self, id: &str, kind: &str, at: Point, attempt: usize) -> Vec<String> {
        self.phase = "gesture";
        self.observing = Some(self.values());
        self.observed_changes.clear();
        self.submitted.clear();
        match kind {
            "text" | "native-text" | "value-edit" => {
                self.editor.ui.focus(id);
                if kind == "value-edit" {
                    self.key(Key::Enter, Mods::default());
                }
                self.key(
                    Key::Char('a'),
                    Mods {
                        ctrl: true,
                        ..Default::default()
                    },
                );
                self.tick(Input {
                    text: if kind == "value-edit" {
                        "1"
                    } else {
                        "Gate name"
                    }
                    .into(),
                    ..Default::default()
                });
                self.key(Key::Enter, Mods::default());
            }
            "knob" | "slider" | "native-drag" if attempt >= 4 => {
                self.tick(Input {
                    pointer: PointerInput {
                        pos: Some(at),
                        ..Default::default()
                    },
                    wheel: Vec2::new(0., if attempt == 4 { -120. } else { 120. }),
                    ..Default::default()
                });
            }
            "knob" | "slider" | "native-drag" | "table" | "xy" | "mouse-area" => {
                let delta = match attempt {
                    0 => Vec2::new(0., -80.),
                    1 => Vec2::new(80., 0.),
                    2 => Vec2::new(0., 80.),
                    _ => Vec2::new(-80., 0.),
                };
                self.pointer(at, true);
                self.pointer(Point::new(at.x + delta.x, at.y + delta.y), true);
                self.pointer(Point::new(at.x + delta.x, at.y + delta.y), false);
            }
            "file-selector" => {
                let prefix = format!("{id}-file-");
                let rows: Vec<_> = self
                    .editor
                    .ui
                    .scene()
                    .unwrap()
                    .surfaces()
                    .filter(|surface| {
                        !surface.disabled && surface.key.as_str().starts_with(&prefix)
                    })
                    .map(|surface| surface.key.to_string())
                    .collect();
                if let Some(row) = rows.get(attempt % rows.len().max(1)) {
                    if let Some(point) = self.hit(row) {
                        self.pointer(point, true);
                        self.pointer(point, false);
                    }
                }
            }
            _ => {
                self.pointer(at, true);
                self.pointer(at, false);
                self.settle();
                if kind == "menu" || kind == "native-click" {
                    // A menu choice is another real pointer target. Do not call callbacks directly.
                    let popup: Vec<_> = if kind == "native-click" {
                        self.targets()
                            .into_iter()
                            .filter(|(key, action)| {
                                key.contains("/popover/") && action == "native-click"
                            })
                            .map(|(key, _)| key)
                            .collect()
                    } else {
                        self.editor
                            .ui
                            .scene()
                            .unwrap()
                            .surfaces()
                            .filter(|s| !s.disabled && s.key.as_str().contains("-item-"))
                            .map(|s| s.key.to_string())
                            .collect()
                    };
                    if !popup.is_empty() {
                        let child = &popup[attempt % popup.len()];
                        if let Some(p) = self.hit(child) {
                            self.pointer(p, true);
                            self.pointer(p, false);
                        }
                    }
                }
            }
        }
        self.settle();
        self.observing = None;
        std::mem::take(&mut self.observed_changes)
            .into_iter()
            .collect()
    }
}

fn changed(before: &Values, after: &Values, submitted: &BTreeSet<String>) -> Vec<String> {
    after
        .iter()
        .filter(|(key, value)| {
            submitted.contains(*key) && before.get(*key).is_some_and(|old| old != *value)
        })
        .map(|(key, _)| key.clone())
        .collect()
}

#[test]
fn gestures_require_the_submitted_owner_not_an_unrelated_listener() {
    let before = BTreeMap::from([
        ("control-1".into(), ir::Value::Real(0.)),
        ("typed-2".into(), ir::Value::Integer(0)),
    ]);
    let after = BTreeMap::from([
        ("control-1".into(), ir::Value::Real(1.)),
        ("typed-2".into(), ir::Value::Integer(1)),
    ]);
    let submitted = BTreeSet::from(["control-1".into()]);
    assert_eq!(changed(&before, &after, &submitted), ["control-1"]);
    assert!(changed(&before, &after, &BTreeSet::new()).is_empty());
    assert!(changed(&before, &before, &submitted).is_empty());
    assert!(!retained(&["control-1".into()], &after, &before));
}

#[test]
fn gestures_require_the_touched_cell_not_another_array_cell() {
    let mut before = Values::new();
    let mut after = Values::new();
    typed_values(
        &mut before,
        ir::ControlId(9),
        &ir::Value::Integers(vec![0, 0]),
    );
    typed_values(
        &mut after,
        ir::ControlId(9),
        &ir::Value::Integers(vec![0, 1]),
    );
    let submitted = BTreeSet::from(["typed-9-0".into()]);
    assert!(changed(&before, &after, &submitted).is_empty());
    typed_values(
        &mut after,
        ir::ControlId(9),
        &ir::Value::Integers(vec![1, 1]),
    );
    assert_eq!(changed(&before, &after, &submitted), ["typed-9-0"]);
    assert!(!retained(&["typed-9-0".into()], &after, &before));
}
fn retained(keys: &[String], saved: &Values, reloaded: &Values) -> bool {
    !keys.is_empty()
        && keys
            .iter()
            .all(|key| saved.get(key).is_some() && saved.get(key) == reloaded.get(key))
}

fn load_failure(status: &str) -> &'static str {
    if !status.starts_with("Load failed:") {
        "none"
    } else if status.contains("schema changed") {
        "script-schema-changed"
    } else if status.contains("type changed") {
        "script-value-type-changed"
    } else if status.contains("Script persistence: InvalidInput") {
        "script-restore-invalid-input"
    } else if status.contains("Script persistence: Capacity") {
        "script-restore-capacity"
    } else {
        "load-failed"
    }
}

#[test]
fn load_failure_receipt_keeps_authored_error_text_private() {
    assert_eq!(
        load_failure("Load failed: Saved script state schema changed"),
        "script-schema-changed"
    );
    assert_eq!(
        load_failure("Load failed: authored private text"),
        "load-failed"
    );
    assert_eq!(load_failure("loaded"), "none");
}

#[test]
fn persistence_receipt_requires_each_changed_parameter() {
    let a = BTreeMap::from([
        ("control-1".into(), ir::Value::Real(1.)),
        ("typed-0-2".into(), ir::Value::Text("edited".into())),
    ]);
    let b = BTreeMap::from([
        ("control-1".into(), ir::Value::Real(1.)),
        ("typed-0-2".into(), ir::Value::Text("old".into())),
    ]);
    assert!(retained(&["control-1".into()], &a, &b));
    assert!(!retained(&["control-1".into(), "typed-0-2".into()], &a, &b));
    assert!(!retained(&[], &a, &b));
}

fn source_range(total: usize, selected: Option<usize>) -> Option<std::ops::Range<usize>> {
    match selected {
        Some(source) if source < total => Some(source..source + 1),
        Some(_) => None,
        None => Some(0..total),
    }
}

#[test]
fn source_shards_reject_unknown_sources_and_preserve_full_default() {
    assert_eq!(source_range(2, None), Some(0..2));
    assert_eq!(source_range(2, Some(1)), Some(1..2));
    assert_eq!(source_range(2, Some(2)), None);
}

#[test]
#[ignore = "release gate: set KONTRA_WIDGET_GATE_PATH and KONTRA_WIDGET_GATE_PROGRAM; all private values stay in RAM"]
fn original_widget_gestures() {
    let path = std::env::var("KONTRA_WIDGET_GATE_PATH").expect("gate path required");
    let program = std::env::var("KONTRA_WIDGET_GATE_PROGRAM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut selection = Selection::default();
    selection.parts = vec![Part {
        path,
        program,
        view: 1,
        ..Default::default()
    }];
    if std::env::var("KONTRA_WIDGET_GATE_CONDITION").as_deref() == Ok("product-warm") {
        drop(Gate::load(selection.clone()));
    }
    let mut gate = Gate::load(selection);
    let deadline = Instant::now() + Duration::from_secs(180);
    let paint_deadline = Instant::now() + Duration::from_secs(15);
    while gate.targets().is_empty() && Instant::now() < paint_deadline {
        gate.settle();
        if gate.params.shared.view.lock().unwrap().parts[0]
            .tree
            .is_none()
        {
            break;
        }
    }
    // Visit every authored source/page through the editor's actual selectors.
    let sources = gate.params.shared.view.lock().unwrap().parts[0]
        .interfaces
        .len()
        .max(1);
    let mut seen = BTreeSet::new();
    let mut results = Vec::new();
    let initial_load_failure =
        load_failure(&gate.params.shared.view.lock().unwrap().parts[0].status);
    let mut initial_faults = gate.faults() + usize::from(initial_load_failure != "none");
    let mut exhausted = false;
    let requested_source = std::env::var("KONTRA_WIDGET_GATE_SOURCE")
        .ok()
        .map(|s| s.parse::<usize>().expect("numeric gate source required"));
    let source_scope = source_range(sources, requested_source).expect("gate source out of range");
    for source in source_scope {
        gate.current_target = None;
        gate.phase = "source-navigation";
        let selector = format!("face-0-{source}");
        if gate.editor.ui.scene().unwrap().surface(&selector).is_some() {
            gate.editor.press(&selector);
            gate.settle();
        }
        let pages = gate.params.shared.view.lock().unwrap().parts[0]
            .interfaces
            .get(source)
            .map_or(1, |face| face.pages.len().max(1));
        for page in 0..pages {
            gate.current_target = None;
            gate.phase = "page-navigation";
            let selector = format!("face-page-0-{page}");
            if gate.editor.ui.scene().unwrap().surface(&selector).is_some() {
                gate.editor.press(&selector);
                gate.settle();
            }
            loop {
                let candidates = gate.targets();
                let Some((id, kind)) = candidates.into_iter().find(|(id, _)| !seen.contains(id))
                else {
                    break;
                };
                seen.insert(id.clone());
                gate.current_target = Some(results.len());
                let mut reason = "parameter-unchanged";
                let mut keys = Vec::new();
                let mut submitted_parameters = 0;
                if Instant::now() >= deadline {
                    exhausted = true;
                    reason = "probe-budget";
                } else if let Some(at) = gate.hit(&id) {
                    for attempt in 0..6 {
                        keys = gate.gesture(&id, &kind, at, attempt);
                        submitted_parameters = submitted_parameters.max(gate.submitted.len());
                        if !keys.is_empty() {
                            reason = "persistence-pending";
                            break;
                        }
                        if gate.editor.ui.scene().unwrap().surface(&id).is_none() {
                            reason = "navigation-only";
                            break;
                        }
                    }
                    if reason == "parameter-unchanged" && submitted_parameters == 0 {
                        reason = "widget-edit-not-submitted";
                    }
                } else {
                    reason = "occluded-or-outside-viewport";
                }
                let faults = gate.faults();
                initial_faults += faults;
                if faults > 0 {
                    reason = "script-or-render-fault";
                }
                results.push((serde_json::json!({"target_sha256": sha2::Sha256::digest(id.as_bytes()).iter().map(|b|format!("{b:02x}")).collect::<String>(), "source": source, "page": page, "kind": kind, "reason": reason, "submitted_parameters": submitted_parameters, "parameter_changes": keys.len(), "value_changed": !keys.is_empty(), "parameter_reached": !keys.is_empty(), "persistence": false}), keys));
                if exhausted {
                    break;
                }
            }
            if exhausted {
                break;
            }
        }
        if exhausted {
            break;
        }
    }
    gate.current_target = None;
    gate.phase = "pre-save";
    gate.settle();
    let saved_values = gate.values();
    let mut saved = gate.params.selection.read().unwrap().clone();
    gate.params.shared.capture_ui_controls(&mut saved);
    // Exercise the same serialized host Part boundary; never write it to disk.
    let state = serde_json::to_vec(&saved.parts).unwrap();
    saved.parts = serde_json::from_slice(&state).unwrap();
    let native_diagnostics = native_ui::gate_diagnostics();
    let first_native_diagnostic = gate.first_native_diagnostic;
    let fault_events = std::mem::take(&mut gate.fault_events);
    let preemption_observations = std::mem::take(&mut gate.preemption_observations);
    let progress_truncated = gate.progress_truncated;
    let captured_state_bytes = saved.parts[0].script_state.len();
    let captured_controls = saved.parts[0].control_values.len();
    drop(gate);
    let mut reloaded = Gate::load(saved);
    let reload_values = reloaded.values();
    let faults = reloaded.faults()
        + initial_faults
        + native_diagnostics.len()
        + native_ui::gate_diagnostics().len();
    let reload_status = reloaded.params.shared.view.lock().unwrap().parts[0]
        .status
        .clone();
    let reload_failure = load_failure(&reload_status);
    let reload_failed = reload_failure != "none";
    for (row, keys) in &mut results {
        if row["reason"] == "persistence-pending" {
            let pass = retained(keys, &saved_values, &reload_values);
            row["persistence"] = pass.into();
            row["reason"] = if faults > 0 {
                "script-or-render-fault"
            } else if reload_failed {
                "save-reload-load-failed"
            } else if pass {
                "passed"
            } else {
                "save-reload-mismatch"
            }
            .into();
        }
    }
    let rows: Vec<_> = results.into_iter().map(|(row, _)| row).collect();
    let passed = rows.iter().filter(|r| r["reason"] == "passed").count();
    let total = rows.len();
    let status = if faults > 0 || reload_failed {
        "FAIL"
    } else if exhausted || total == 0 || requested_source.is_some() {
        "UNKNOWN"
    } else if passed == total {
        "PASS"
    } else {
        "FAIL"
    };
    let problems = reloaded.core.problems(0);
    println!(
        "\n{}",
        serde_json::json!({"widget_gate_schema": 1, "program": program, "status": status, "passed": passed, "total": total, "requested_source": requested_source, "sources_total": sources, "scope_complete": !exhausted && total > 0 && native_diagnostics.is_empty(), "coverage_complete": requested_source.is_none() && !exhausted && total > 0 && native_diagnostics.is_empty(), "faults": faults, "fault_events": fault_events, "preemption_observations": preemption_observations, "progress_truncated": progress_truncated, "reload_fault_events": reloaded.fault_events, "reload_preemption_observations": reloaded.preemption_observations, "reload_progress_truncated": reloaded.progress_truncated, "reload_problem_counters": {"nonfinite": problems.nonfinite, "script_overruns": problems.script_overruns, "lua_faults": problems.lua_faults}, "native_diagnostics": native_diagnostics, "first_native_diagnostic_frame_and_target": first_native_diagnostic, "reload_first_native_diagnostic_frame_and_target": reloaded.first_native_diagnostic, "captured_state_bytes": captured_state_bytes, "captured_controls": captured_controls, "initial_load_failure": initial_load_failure, "reload_failure": reload_failure, "saved_parameters": saved_values.len(), "reloaded_parameters": reload_values.len(), "targets": rows})
    );
}

#[test]
#[ignore = "owner attribution: two engine seconds without gestures; library state stays in RAM"]
fn original_idle_script_attribution() {
    let path = std::env::var("KONTRA_WIDGET_GATE_PATH").expect("gate path required");
    let program = std::env::var("KONTRA_WIDGET_GATE_PROGRAM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut selection = Selection::default();
    selection.parts = vec![Part {
        path,
        program,
        view: 1,
        ..Default::default()
    }];
    let mut gate = Gate::load(selection);
    gate.phase = "post-load-idle";
    for _ in 0..120 {
        if !gate.fault_events.is_empty() {
            break;
        }
        gate.tick(Input::default());
    }
    println!(
        "\n{}",
        serde_json::json!({"widget_idle_attribution_schema": 1, "program": program,
        "sample": gate.core.clock(0), "frame": gate.frame, "gestures": 0,
        "initial_load_failure": load_failure(&gate.params.shared.view.lock().unwrap().parts[0].status),
        "fault_events": gate.fault_events, "preemption_observations": gate.preemption_observations,
        "progress_truncated": gate.progress_truncated, "native_diagnostics": native_ui::gate_diagnostics()})
    );
}

use sha2::Digest;

#[test]
#[ignore = "real Original host recall; private state stays in RAM"]
fn original_host_state_roundtrip() {
    let mut selection = Selection::default();
    selection.parts = vec![Part {
        path: std::env::var("KONTRA_WIDGET_GATE_PATH").unwrap(),
        view: 1,
        ..Default::default()
    }];
    let gate = Gate::load(selection);
    let expected = gate.values();
    assert!(!expected.is_empty(), "Original must expose engine state");
    let mut saved = gate.params.selection.read().unwrap().clone();
    gate.params.shared.capture_ui_controls(&mut saved);
    let state = serde_json::to_vec(&saved.parts).unwrap();
    saved.parts = serde_json::from_slice(&state).unwrap();
    drop(gate);
    let recalled = Gate::load(saved);
    let failure = load_failure(&recalled.params.shared.view.lock().unwrap().parts[0].status);
    assert_eq!(failure, "none", "host recall must load all Original state");
    assert_eq!(
        expected.len(),
        recalled.values().len(),
        "host recall retains all parameter addresses"
    );
    assert!(
        expected == recalled.values(),
        "host recall preserves exact parameter values"
    );
}
