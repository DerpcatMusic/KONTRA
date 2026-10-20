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

mod parity_run;
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
                            .map(|index| format!("typed-{}-{index}", { id.0 })),
                    );
                } else {
                    edits.insert(format!("typed-{}", { id.0 }));
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
    /// Script engine-parameter outcomes since the caller last cleared them.
    engine_outcomes: Vec<sampler_core::EngineParameterOutcome>,
    /// MIDI that returns a parity stimulus to rest.
    rest: Vec<[u8; 3]>,
    /// MIDI played right after each parity note-on.
    during: Vec<[u8; 3]>,
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
            engine_outcomes: Vec::new(),
            rest: Vec::new(),
            during: Vec::new(),
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
                if !fault && self.preemption_observations.len() >= 128 { self.progress_truncated = true; return; }
                self.preemption_observations.push(serde_json::json!({"frame": self.frame, "target": self.current_target,
                    "phase": self.phase, "sample": sample, "aggregate_preemptions": total,
                    "program": p.program, "callback": atoms.widget_gate_callback(p.program), "pc": p.pc,
                    "owner": match p.owner { sampler_core::BehaviorOwner::Note(_) => "note", sampler_core::BehaviorOwner::Plan(_) => "plan" },
                    "waiting": p.waiting, "first_preemption_sample": p.yielded_at, "callers": p.callers,
                    "outcome": p.outcome.map(|outcome| format!("{outcome:?}"))}));
            });
            self.preemptions = total;
            let outcomes = core.widget_gate_engine_outcomes(0);
            let room = 4096usize.saturating_sub(self.engine_outcomes.len());
            self.engine_outcomes.extend(outcomes.into_iter().take(room));
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

/// Reuses the product editor, library reader, CPU painter and real pointer
/// dispatch. Inventory contains UI metadata only, never library source/state.
#[test]
#[ignore = "installed Conflux page capture; set KONTRA_WIDGET_GATE_PATH and KONTRA_CONFLUX_SHOTS"]
fn conflux_authored_pages_and_inventory() {
    use serde_json::json;
    let path = std::env::var("KONTRA_WIDGET_GATE_PATH").expect("gate path required");
    let output = std::path::PathBuf::from(std::env::var_os("KONTRA_CONFLUX_SHOTS").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let program = std::env::var("KONTRA_WIDGET_GATE_PROGRAM").ok()
        .and_then(|s| s.parse().ok()).unwrap_or(0);
    let view = std::env::var("KONTRA_CONFLUX_VIEW").ok()
        .and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut selection = Selection::default();
    selection.parts = vec![Part { path: path.clone(), program, view, ..Default::default() }];
    let mut gate = Gate::load(selection);
    let deadline = Instant::now() + Duration::from_secs(30);
    while native_ui::gate_targets(0).is_empty() && Instant::now() < deadline {
        gate.settle();
    }
    // A screenshot of a placeholder is retained as evidence, but fails parity.
    let shoot = |gate: &Gate, name: &str| {
        moose::core::screenshot::save_png(&output.join(format!("{name}.png")),
            &super::tests::pixels(&gate.editor.ui, 1500, 1100), 1500, 1100);
    };
    shoot(&gate, "initial");
    let mut resources =
        sampler_kontakt::Resources::for_program(std::path::Path::new(&path), program)
            .expect("the selected program's authored resource owner must resolve");
    let resource_root = resources.root().to_path_buf();
    let names = resources.names("");
    let mut modules = Vec::new();
    let mut picture_sets = Vec::new();
    for name in &names {
        if name.ends_with(".nui") {
            let bytes = resources.read(name).expect("authored NativeUI module must resolve");
            let source = std::str::from_utf8(&bytes).unwrap();
            modules.push(json!({"path":name,"bytes":bytes.len(),"lines":source.lines().count(),
                "parameter_references":source.matches("parameters.").count(),
                "tap_handlers":source.matches("on_tap_gesture").count(),
                "drag_handlers":source.matches("on_drag_gesture").count()}));
        } else if name.ends_with(".txt") && name.contains("pictures/") {
            let bytes = resources.read(name).expect("picture metadata must resolve");
            let text = String::from_utf8_lossy(&bytes);
            picture_sets.push(json!({"path":name,"meta":format!("{:?}",sampler_ksp::ui::picture_meta(&text))}));
        }
    }
    let inventory = {
        let published = gate.params.shared.view.lock().unwrap();
        let part = &published.parts[0];
        let scripts = part.instrument.as_ref().map(|i| i.behaviors.iter().map(|b| {
            json!({"slot":b.slot,"bytes":b.source.len(),"lines":b.source.lines().count(),
                "ui_declarations":b.source.lines().filter(|l|l.trim_start().starts_with("declare ui_")).count(),
                "persistence_declarations":b.source.matches("make_persistent(").count(),
                "performance_view":sampler_ksp::nckp::view_name(&b.source)})
        }).collect::<Vec<_>>()).unwrap_or_default();
        json!({"path":path,"program":program,"view":view,"resource_root":resource_root,"scripts":scripts,
            "resources":names,"native_modules":modules,"picture_sets":picture_sets,
            "interfaces":part.interfaces.iter().map(|f|json!({"source":format!("{:?}",f.source),
                "native_entry":f.native_ui.as_ref().map(|n|&n.entry),
                "pages":f.pages.iter().map(|p|json!({"name":p.name,"size":[p.size.width,p.size.height]})).collect::<Vec<_>>(),
                "controls":f.widgets.iter().enumerate().map(|(n,w)|json!({"index":n,"name":w.name,"source_id":w.source_id,
                    "kind":format!("{:?}",w.kind),"parent":w.parent.map(|p|p.0),"hidden":w.hidden,"enabled":w.enabled,
                    "rect":[w.rect.x,w.rect.y,w.rect.width,w.rect.height],"default_axes":w.default_axes,
                    "text":w.text,"binding":format!("{:?}",w.binding),"tooltip":w.tooltip})).collect::<Vec<_>>()
            })).collect::<Vec<_>>(),"native_diagnostics":native_ui::gate_diagnostics()})
    };
    std::fs::write(
        output.join("inventory.json"),
        serde_json::to_vec_pretty(&inventory).unwrap(),
    )
    .unwrap();
    assert!(
        native_ui::gate_diagnostics().is_empty(),
        "authored graph failed: {:?}",
        native_ui::gate_diagnostics()
    );
    assert!(
        !native_ui::gate_targets(0).is_empty(),
        "authored graph did not paint; screenshot retained"
    );
    let snapshot = |gate: &Gate, label: &str| {
        json!({"page":label,"targets":gate.targets().len(),
        "captions":native_ui::gate_caption_targets(0),
        "surfaces":gate.editor.ui.scene().unwrap().surfaces().filter(|s|s.key.starts_with("nui-")).map(|s|
            json!({"id":s.key.to_string(),"frame":[s.frame.x,s.frame.y,s.frame.size.width,s.frame.size.height],"disabled":s.disabled})).collect::<Vec<_>>() })
    };
    let mut page_targets = vec![snapshot(&gate, "initial")];
    // Factory MIDI tools have their own page set. Never certify them by
    // clicking Conflux-looking tabs from a wrongly resolved `main` module.
    let is_conflux = inventory["interfaces"].as_array().unwrap().iter().any(|f| {
        f["controls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["name"] == "$Header__Page")
    });
    let pages = std::env::var("KONTRA_CONFLUX_PAGES").unwrap_or_else(|_| {
        if is_conflux {
            "PLAY,EDIT,SETTINGS".into()
        } else {
            String::new()
        }
    });
    for label in pages.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let targets = native_ui::gate_caption_targets(0);
        let id = targets.iter().find(|(text,_)| text.eq_ignore_ascii_case(label))
            .unwrap_or_else(|| panic!("authored {label} tab missing; captions={:?}",targets.iter().map(|(t,_)|t).collect::<Vec<_>>())).1.clone();
        let p = gate.hit(&id).expect("authored page tab must be clickable");
        gate.pointer(p, false);
        gate.pointer(p, true);
        gate.pointer(p, false);
        gate.settle();
        for _ in 0..12 { gate.settle(); }
        shoot(&gate, &label.to_ascii_lowercase());
        let targets = gate.targets();
        assert!(
            !targets.is_empty(),
            "authored {label} has no interactive controls"
        );
        page_targets.push(snapshot(&gate, label));
        assert!(
            native_ui::gate_diagnostics().is_empty(),
            "{label} graph must render"
        );
    }
    std::fs::write(output.join("pages.json"),serde_json::to_vec_pretty(&page_targets).unwrap()).unwrap();
    println!("CONFLUX_PAGES program={program} view={view} pages={} controls={}",page_targets.len(),
        inventory["interfaces"].as_array().unwrap().iter().map(|f|f["controls"].as_array().unwrap().len()).sum::<usize>());
}

#[test]
#[ignore = "installed multi resource ownership; filenames only, no source extraction"]
fn conflux_multi_authored_resource_references() {
    let path = std::path::PathBuf::from(std::env::var_os("KONTRA_WIDGET_GATE_PATH").unwrap());
    let chunks = sampler_kontakt::read_chunks(&path).unwrap();
    let tables = chunks.filename_tables().unwrap().unwrap();
    println!("CONFLUX_RESOURCE_NAMES special={:?} other={:?}",tables.special_filetable,tables.other_filetable);
    for (n,(_,p)) in sampler_kontakt::read_multi(&path).unwrap().programs.into_iter().enumerate() {
        let record = p.public_record().unwrap();
        let reference = record.resource_container_filename_ref;
        println!("CONFLUX_RESOURCE_OWNER program={n} version={} container_ref={reference:?} container={:?}",p.version(),
            reference.and_then(|r| tables.special_filetable.get(&(r as u32))));
    }
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

/// Every part tab of a real instrument, at the minimum and a large editor size,
/// for layout review. Pixels only; nothing from the library is written besides them.
#[test]
#[ignore = "installed library tab capture; set KONTRA_WIDGET_GATE_PATH and KONTRA_TAB_SHOTS"]
fn tab_layout_shots() {
    let path = std::env::var("KONTRA_WIDGET_GATE_PATH").expect("gate path required");
    let output = std::path::PathBuf::from(std::env::var_os("KONTRA_TAB_SHOTS").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let mut selection = Selection::default();
    selection.parts = vec![Part { path, ..Default::default() }];
    let mut gate = Gate::load(selection);
    for _ in 0..50 {
        gate.settle();
    }
    for (w, h) in [(900u16, 600u16), (1500, 1100)] {
        gate.editor.resize(Size::new(w as f64, h as f64));
        for view in ["Interface", "Articulations", "Mapping", "Sound", "Info", "Scripts"] {
            let id = format!("view-0-{view}");
            if gate.editor.ui.scene().unwrap().surface(&id).is_none() {
                gate.editor.press("header-0");
            }
            gate.editor.press(&id);
            for _ in 0..10 {
                gate.settle();
            }
            gate.editor.settle_art();
            moose::core::screenshot::save_png(&output.join(format!("{view}-{w}.png")),
                &super::tests::pixels(&gate.editor.ui, w, h), w.into(), h.into());
        }
        // Folded to the header alone, then unfolded for the next size.
        gate.editor.press("collapse-0");
        for _ in 0..30 {
            gate.settle();
        }
        moose::core::screenshot::save_png(&output.join(format!("Collapsed-{w}.png")),
            &super::tests::pixels(&gate.editor.ui, w, h), w.into(), h.into());
        gate.editor.press("collapse-0");
        for _ in 0..30 {
            gate.settle();
        }
    }
}


/// Opt-in numeric observations of the actual Part asset owner. Off by default
/// even in test builds; a request consumes at most one face on the UI thread.
pub(super) fn library_face_receipt(
    slot: usize, epoch: u64, source: usize, page: ir::PageRef,
    presentation: ir::Presentation, snapshot: Option<&sampler_uvi::scripted::UiSnapshot>,
    assets: &super::ir_view::Assets,
) {
    library_acceptance09::face_receipt(slot, epoch, source, page, presentation, snapshot, assets);
}

mod library_acceptance09 {
    use super::*;
    use crate::sound::{BlockInfo, MAX_BLOCK, event::Event};
    use sampler_uvi::scripted::{DiagnosticPoll, UiBridge, UiSnapshot};
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::{cell::RefCell, path::Path};

    const MAX_DRAWS: usize = 240;
    const MAX_FRAMES: u64 = 384_000;
    const NOTE_FRAMES: u64 = 288_000;
    const MAX_WORK: usize = 262_144;
    const MAX_ACTIONS: usize = 12;
    const MAX_RECEIPTS: usize = 8;
    const MAX_REPORT: usize = 2 << 20;
    const MAX_PROBES: usize = 512;
    const MAX_PAINTS: usize = 4;
    const WINDOWS: [(u64,u64);3] = [(0,6000),(180000,192000),(276000,288000)];

    fn hash(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
    fn charge_work(work:&mut usize,n:usize)->Result<(),&'static str>{
        *work=work.checked_add(n).ok_or("inspection_overflow")?;
        if *work>MAX_WORK{Err("inspection_bound")}else{Ok(())}
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Profile { Ao, Clarinet }
    impl Profile {
        fn key(self) -> &'static str { match self { Self::Ao => "ao-pizzicato2", Self::Clarinet => "vwinds-clarinet-a" } }
        fn path(self) -> &'static str { match self {
            Self::Ao => "/mnt/MAIN_STORAGE/Libraries/UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs/Presets/00 Orchestra/01 Strings/V Strings Pizzicato 2.uvip",
            Self::Clarinet => "/mnt/MAIN_STORAGE/Libraries/UVI/VWinds - Clarinets/VWinds-AClarinet.ufs/Presets/Clarinet A.uvip",
        } }
    }

    struct FaceCapture { epoch: u64, owner: UiSnapshot, requested: bool, report: Option<Value>, drawn: Option<(u64,usize,usize,u64)>, work_remaining:usize }
    // Preserve the original authored-test API, but production-helper capture
    // receives refusal work too; a bounded failed inspection is not free.
    fn requested_waves(assets:&super::super::ir_view::Assets,face:&ir::Interface,page:ir::PageRef,limit:usize)->Result<(Value,bool,usize),&'static str>{
        requested_waves_metered(assets,face,page,limit).map_err(|(reason,_)|reason)
    }
    fn requested_waves_metered(assets:&super::super::ir_view::Assets,face:&ir::Interface,page:ir::PageRef,limit:usize)->Result<(Value,bool,usize),(&'static str,usize)>{
        let mut work=0usize;
        fn reserve(work:&mut usize,n:usize,limit:usize)->Result<(),(&'static str,usize)>{
            let wanted=work.checked_add(n).ok_or(("wave_inspection_overflow",*work))?;
            if wanted>limit{return Err(("wave_inspection_bound",*work));}
            *work=wanted;Ok(())
        }
        reserve(&mut work,face.widgets.len(),limit)?; // Actual preflight roster pass.
        let mut wave_count=0usize;
        let mut ancestor_count=0usize;
        for w in &face.widgets {if w.uvi_wave_view_request.is_some(){
            wave_count+=1;if wave_count>MAX_PROBES{return Err(("wave_observation_row_bound",work));}
            let mut parent=w.parent;let mut depth=0;
            while let Some(id)=parent {
                depth+=1;
                if depth>face.widgets.len(){return Err(("wave_ancestry_or_inspection_bound",work));}
                reserve(&mut work,1,limit)?;
                ancestor_count=ancestor_count.checked_add(1).ok_or(("wave_inspection_overflow",work))?;
                parent=face.widgets.get(id.0).ok_or(("wave_parent_invalid",work))?.parent;
            }
        }}
        // Observer independently scans the entire roster and visible() visits
        // each wave plus ancestors. Its provider performs at most three cache
        // scans per wave (get, status/get, failed-source scan), each bounded by
        // the actual provider LIMIT. The final returned-row pass is separate.
        // Reserve all these passes BEFORE observer allocation/traversal.
        let provider=wave_count.checked_mul(3*crate::sound::waveform::uvi_manual::LIMIT)
            .ok_or(("wave_inspection_overflow",work))?;
        let observer=face.widgets.len().checked_add(ancestor_count)
            .and_then(|n|n.checked_add(wave_count)).and_then(|n|n.checked_add(provider))
            .and_then(|n|n.checked_add(wave_count)).ok_or(("wave_inspection_overflow",work))?;
        reserve(&mut work,observer,limit)?;
        let observations=assets.wave_view_observations(face);
        let rows=observations["rows"].as_array().ok_or(("wave_observation_schema",work))?;
        if rows.len()!=wave_count{return Err(("wave_observation_roster_changed",work));}
        let dropped=observations["request_limit_drops"].as_u64().unwrap_or(0);
        let mut actual_requested=0usize;let mut unresolved=0usize;
        for row in rows {
            let index=row["widget"].as_u64().and_then(|n|usize::try_from(n).ok()).ok_or(("wave_widget_identity",work))?;
            let w=face.widgets.get(index).ok_or(("wave_widget_identity",work))?;
            let status=row["status"].as_str().ok_or(("wave_status_schema",work))?;
            let demanded=matches!(status,"ready"|"source_unavailable"|"worker_unavailable"|"pending")
                ||(w.page==page&&row["visible"]==true&&w.uvi_wave_view_request.as_ref().is_some_and(|r|matches!(&r.sample,ir::UviWaveViewSample::Manual(_))));
            if demanded{actual_requested+=1;if status!="ready"{unresolved+=1;}}
        }
        Ok((json!({"provider":observations,"actually_requested_or_visible_manual":actual_requested,
            "unresolved_requested":unresolved,"request_limit_drops":dropped,"inspection_work":work,
            "scope":"Actual manual provider requests; hidden unrequested definitions do not fail"}),unresolved==0&&dropped==0,work))
    }
    thread_local! { static FACE: RefCell<Option<FaceCapture>> = const { RefCell::new(None) }; }
    struct CaptureGuard;
    impl Drop for CaptureGuard { fn drop(&mut self) { FACE.with(|v| *v.borrow_mut() = None); } }
    pub(super) fn face_receipt(
        slot: usize, epoch: u64, source: usize, page: ir::PageRef,
        presentation: ir::Presentation, snapshot: Option<&UiSnapshot>, assets: &super::super::ir_view::Assets,
    ) {
        FACE.with(|cell| {
            let mut capture = cell.borrow_mut();
            let Some(capture) = capture.as_mut().filter(|_| slot == 0) else { return; };
            let Some(snapshot) = snapshot else {
                capture.drawn=None;
                if capture.requested {capture.requested=false;capture.report=Some(json!({"status":"missing_drawn_owner"}));}
                return;
            };
            capture.drawn=Some((epoch,source,page.0,snapshot.generation()));
            if !capture.requested {return;}
            capture.requested = false;
            if capture.epoch != epoch || !capture.owner.same_owner(snapshot) || snapshot.interface().source != ir::Source::FalconLua {
                capture.report = Some(json!({"status":"stale_drawn_owner"})); return;
            }
            let probes = assets.probes();
            if probes.len() > MAX_PROBES { capture.report = Some(json!({"status":"asset_probe_bound"})); return; }
            let rows: Vec<_> = probes.iter().map(|p| json!({
                "asset":p.index,"path_sha256":hash(p.path.as_bytes()),"path_truncated":p.path_truncated,
                "kind":p.kind,"resolver":p.resolver,"status":p.status,"codec":p.codec,
                "encoded_bytes":p.encoded_bytes,"prepared_bytes":p.prepared_bytes,
                "width":p.width,"height":p.height,"frames":p.frames,"frame":p.frame,
                "target":p.target,"window":p.window,"strip_cache_hit":p.strip_cache_hit,
            })).collect();
            let (waves,waves_complete,wave_work)=match requested_waves_metered(assets,snapshot.interface(),page,capture.work_remaining){
                Ok(v)=>v,Err((reason,work))=>{capture.report=Some(json!({"status":reason,"requested_page_complete":false,"inspection_work":work}));return;}
            };
            let scan = assets.scan();
            let failures = scan.lookup_missing + scan.lookup_invalid + scan.lookup_ambiguous + scan.lookup_corrupt
                + scan.lookup_limit + scan.lookup_read + scan.lookup_unavailable + scan.preparation_oversized + scan.preparation_key_budget;
            let requested_complete = scan.lookups > 0 && assets.pending() == 0 && failures == 0 && assets.probe_dropped() == 0 && waves_complete
                && scan.decodes == scan.decode_ok && scan.fonts == scan.font_ok
                && probes.iter().all(|p| !p.path_truncated);
            capture.report = Some(json!({"status":"observed","epoch":epoch,"source_index":source,"page":page.0,
                "schema_generation":snapshot.generation(),"bitmap":presentation==ir::Presentation::Bitmap,
                "actual_owner_same":true,"declared_asset_inventory":snapshot.interface().assets.len(),
                "pending":assets.pending(),"retained_picture_payload_bytes":assets.bytes(),
                "probe_dropped":assets.probe_dropped(),"probe_count":probes.len(),"probes":rows,
                "lookups":scan.lookups,"found":scan.lookup_ok,"failure_count":failures,
                "image_decodes":scan.decodes,"image_decode_ok":scan.decode_ok,
                "fonts":scan.fonts,"font_ok":scan.font_ok,"completed":scan.preparation_completed,
                "prepared_bytes":scan.preparation_completed_bytes,"wanted_peak_bytes":scan.preparation_wanted_peak_bytes,
                "requested_page_complete":requested_complete,"requested_wave_views":waves,"inspection_work":wave_work,
                "scope":"Actual Part Assets request ledger; not an exhaustive declared-resource census or allocator/RSS bound"}));
        });
    }

    // Same actual positive host block remains open through all diagnostic-only
    // zero-service. A poll never installs a grant or closes/reopens the block.
    #[derive(Default)]
    struct ObservedHostBlock { frames: Option<usize> }
    impl ObservedHostBlock {
        fn finish(&mut self, core: &mut V2Core, note_ends: &mut usize) {
            if let Some(frames)=self.frames.take() {core.end_block(frames,&mut |_|{*note_ends+=1;true});}
        }
        fn begin(&mut self, core: &mut V2Core, frames: usize, note_ends: &mut usize) -> Result<(), &'static str> {
            if frames==0 || frames>MAX_BLOCK {return Err("invalid_observed_host_block");}
            self.finish(core,note_ends);
            core.begin_block(&BlockInfo {frames,offline:true,..Default::default()});
            self.frames=Some(frames);Ok(())
        }
        fn service(&self, core: &mut V2Core) -> Result<(), &'static str> {
            if self.frames.is_none() {return Err("zero_service_without_actual_host_block");}
            let at=core.clock(0);let _=core.render(0);
            if core.clock(0)!=at {return Err("zero_service_changed_clock");}Ok(())
        }
    }
    struct Target { index: usize, component: Option<usize>, id: ir::ControlId, schema: u64, surface: String, kind: &'static str }
    struct Run {
        gate: Gate, owner: Arc<UiBridge>, epoch: u64, players:Vec<u32>, started: Instant, observations: Instant,
        draws: usize, work: usize, actions: usize, receipts: usize, polls: usize, paints: usize,
        rendered: u64, owner_checks:bool, held_at: Option<u64>, audio: [[f64;3];3], nonfinite: usize, note_ends: usize,
        submitted: BTreeSet<String>, outcomes: Vec<Value>, rows: Vec<Value>, pixels: Vec<Value>, resources: Vec<Value>,
        host_block: ObservedHostBlock,
        pointer_held: Option<Point>, // Owns one reserved actual UI-only Up draw.
        pointer_down_delivered: bool,
        pointer_downs: usize, pointer_ups: usize, pointer_cleanup_ups: usize,
        pointer_cleanup_failure: Option<&'static str>,
    }
    impl Run {
        fn load(profile: Profile) -> Result<Self, &'static str> {
            let started = Instant::now();
            let mut selection = Selection::default();
            selection.parts = vec![Part { path:profile.path().into(), program:0, view:1, ..Default::default() }];
            let gate = Gate::load(selection);
            if started.elapsed() > Duration::from_secs(110) { return Err("load_wall_bound"); }
            let epoch = gate.core.epoch(0);
            let owner = gate.params.shared.part(0).ok_or("missing_part_atoms")?
                .widget_gate_uvi_owner(epoch).ok_or("missing_current_uvi_owner")?;
            gate.core.enable_uvi_diagnostics(0, epoch, &owner).map_err(|_|"installed_owner_mismatch")?;
            let players={
                let view = gate.params.shared.view.lock().unwrap();
                let part = &view.parts[0];
                let Some((path,program,_)) = &part.active_source else { return Err("active_source_unavailable"); };
                if path != profile.path() || *program != 0 || part.generation != epoch { return Err("loaded_identity_mismatch"); }
                if part.interfaces.iter().filter(|f|f.source == ir::Source::FalconLua).count() != 1 { return Err("ambiguous_lua_face"); }
                let instrument=part.instrument.as_ref().ok_or("loaded_instrument_metadata_missing")?;
                let players:Vec<u32>=if profile==Profile::Clarinet {
                    let expected=[9019,9073,9127];
                    if expected.iter().any(|id|!instrument.source_indices.sample_targets.iter().any(|t|t.node==*id)){return Err("clarinet_retained_source_targets_missing");}
                    expected.into()
                }else{instrument.source_indices.sample_targets.iter().take(3).map(|t|t.node).collect()};
                if players.is_empty() || players.iter().enumerate().any(|(i,p)|players[..i].contains(p)){return Err("source_target_metadata_missing_or_ambiguous");}
                players
            };
            Self::observe_gate(gate,owner,epoch,players,started)
        }
        fn observe_gate(gate: Gate, owner: Arc<UiBridge>, epoch: u64, players: Vec<u32>, started: Instant) -> Result<Self, &'static str> {
            let draws = gate.frame.checked_add(3).ok_or("draw_overflow")?; // Harness::new's actual three frames + Gate's eight.
            let rendered = gate.core.clock(0);
            let observations = Instant::now();
            FACE.with(|v| *v.borrow_mut() = Some(FaceCapture { epoch, owner:owner.snapshot(), requested:false, report:None,drawn:None,work_remaining:0 }));
            Ok(Self { gate,owner,epoch,players,started,observations,draws,work:0,actions:0,receipts:0,polls:0,paints:0,
                rendered,owner_checks:true,held_at:None,audio:[[0.;3];3],nonfinite:0,note_ends:0,submitted:BTreeSet::new(),outcomes:Vec::new(),
                rows:Vec::new(),pixels:Vec::new(),resources:Vec::new(),host_block:ObservedHostBlock::default(),
                pointer_held:None,pointer_down_delivered:false,pointer_downs:0,pointer_ups:0,pointer_cleanup_ups:0,pointer_cleanup_failure:None })
        }
        fn charge(&mut self, n: usize) -> Result<(), &'static str> {
            charge_work(&mut self.work,n)
        }
        fn current(&self) -> Result<UiSnapshot, &'static str> {
            self.gate.core.uvi_diagnostic_progress(0,self.epoch,&self.owner).map_err(|_|"stale_runtime_owner")?;
            let view = self.gate.params.shared.view.lock().unwrap();
            let part = &view.parts[0];
            if part.generation != self.epoch { return Err("stale_part_epoch"); }
            let drawn = part.uvi_snapshot.clone().ok_or("drawn_snapshot_unavailable")?;
            let now = self.owner.snapshot();
            if !drawn.same_owner(&now) || drawn.generation() != now.generation() { return Err("drawn_schema_not_current"); }
            let rendered=FACE.with(|v|v.borrow().as_ref().and_then(|v|v.drawn));
            if rendered.is_none_or(|(epoch,_,_,schema)|epoch!=self.epoch||schema!=drawn.generation()){return Err("part_has_not_drawn_current_schema");}
            Ok(drawn)
        }
        fn render(&mut self, frames: usize) -> Result<(), &'static str> {
            if self.rendered.checked_add(frames as u64).is_none_or(|n| n > MAX_FRAMES) { return Err("audio_frame_bound"); }
            let mut left = frames;
            while left != 0 {
                let n = left.min(MAX_BLOCK);
                let at = self.gate.core.clock(0);
                self.host_block.begin(&mut self.gate.core,n,&mut self.note_ends)?;
                let out = self.gate.core.render(n);
                for i in 0..n {
                    let mut sample = [0f64;2];
                    for (bus,on) in out.buses.iter().zip(out.live) {
                        if on { sample[0] += f64::from(bus[0][i]); sample[1] += f64::from(bus[1][i]); }
                    }
                    if sample.iter().any(|v| !v.is_finite()) { self.nonfinite += 1; continue; }
                    if let Some(start) = self.held_at {
                        let relative = (at + i as u64).saturating_sub(start);
                        for (index,(low,high)) in WINDOWS.iter().copied().enumerate() {
                            if relative >= low && relative < high {
                                self.audio[index][0] += sample[0]*sample[0]+sample[1]*sample[1];
                                self.audio[index][1] = self.audio[index][1].max(sample[0].abs().max(sample[1].abs()));
                                self.audio[index][2] += 1.;
                            }
                        }
                    }
                }
                // Matching end_block is deferred until the next actual block
                // or final cleanup; zero-service spends THIS remaining grant.
                if self.gate.core.clock(0) != at + n as u64 { return Err("clock_did_not_advance_exactly"); }
                self.rendered += n as u64; left -= n;
                // This opt-in off-audio witness paces the existing independent
                // owner. It is not an audio-thread wait or native timing proof.
                std::thread::sleep(Duration::from_millis(1));
            }
            self.gate.params.shared.widget_gate_readback(&mut self.gate.core);
            Ok(())
        }
        fn tick(&mut self, input: Input) -> Result<(), &'static str> {
            // While held, one slot is reserved for an unconditional actual Up.
            if self.draws.checked_add(usize::from(self.pointer_held.is_some())).is_none_or(|n|n>=MAX_DRAWS)
                || self.started.elapsed() > Duration::from_secs(110) { return Err("draw_or_wall_bound"); }
            self.draws += 1;
            let down=input.pointer.buttons==Buttons::PRIMARY;
            self.gate.editor.tick(input);
            if down&&self.pointer_held.is_some()&&!self.pointer_down_delivered{self.pointer_down_delivered=true;self.pointer_downs+=1;}
            let submitted = SUBMITTED.with(|v|std::mem::take(&mut *v.borrow_mut()));
            if submitted.len() > 32 { return Err("submitted_target_bound"); }
            self.submitted.extend(submitted);
            if self.submitted.len() > 32 { return Err("submitted_target_bound"); }
            self.render(800)?;
            self.gate.params.shared.widget_gate_readback(&mut self.gate.core);
            Load.run(&self.gate.params);
            Ok(())
        }
        fn release_pointer(&mut self, at: Point, cleanup: bool) -> Result<(), &'static str> {
            if self.pointer_held.is_none() {return Ok(());}
            if !self.pointer_down_delivered{self.pointer_held=None;return Ok(());}
            // The reserved draw is UI-only: no schema/current-owner check,
            // render, new fuel, Load, action retry or wall-bound suppression.
            if self.draws>=MAX_DRAWS {self.pointer_cleanup_failure=Some("reserved_up_draw_missing");return Err("reserved_up_draw_missing");}
            self.draws+=1;self.pointer_ups+=1;
            if cleanup {self.pointer_cleanup_ups+=1;}
            self.gate.editor.tick(Input {pointer:PointerInput {pos:Some(at),buttons:Buttons::default(),..Default::default()},..Default::default()});
            self.pointer_held=None;self.pointer_down_delivered=false;
            let submitted=SUBMITTED.with(|v|std::mem::take(&mut *v.borrow_mut()));
            if submitted.len()>32 || self.submitted.len().checked_add(submitted.len()).is_none_or(|n|n>32) {
                self.pointer_cleanup_failure=Some("up_submitted_target_bound");return Err("up_submitted_target_bound");
            }
            self.submitted.extend(submitted);Ok(())
        }
        fn pointer(&mut self, at: Point, down: bool) -> Result<(), &'static str> {
            if !down && self.pointer_held.is_some() {
                self.release_pointer(at,false)?;
                // One ordinary post-Up service draw. The physical release was
                // already delivered even if this fallible follow-up refuses.
                return self.tick(Input {pointer:PointerInput {pos:Some(at),..Default::default()},..Default::default()});
            }
            if down && self.pointer_held.is_none() {
                if self.draws.checked_add(3).is_none_or(|n|n>MAX_DRAWS) {return Err("press_and_up_draw_reservation_bound");}
                self.pointer_held=Some(at);self.pointer_down_delivered=false;
            }
            let result=(||{for _ in 0..2 {
                self.tick(Input {pointer:PointerInput {pos:Some(at),buttons:if down {Buttons::PRIMARY}else{Buttons::default()},..Default::default()},..Default::default()})?;
            }Ok(())})();
            if result.is_err() && self.pointer_held.is_some() {
                // Keep original refusal even if the separately reported cleanup
                // encounters a submission-accounting error AFTER actual Up.
                let _=self.release_pointer(at,true);
            }
            result
        }
        fn hit(&mut self, surface: &str) -> Result<Point, &'static str> {
            let r = self.gate.editor.ui.scene().and_then(|s|s.surface(surface)).filter(|s|!s.disabled).ok_or("target_scene_missing_or_disabled")?.frame;
            if [r.x,r.y,r.size.width,r.size.height].iter().any(|v|!v.is_finite()) || r.size.width <= 0. || r.size.height <= 0. { return Err("target_scene_empty_or_nonfinite"); }
            let at = Point::new(r.x+r.size.width/2.,r.y+r.size.height/2.);
            self.pointer(at,false)?;
            if !self.gate.editor.ui.get(surface).hovered { return Err("target_occluded"); }
            Ok(at)
        }
        fn adopt_current(&mut self)->Result<UiSnapshot,&'static str>{
            // Bounded ordinary repaints only. Never repeat a gesture or infer
            // init completion from a quiet/stable face.
            for _ in 0..4 {
                match self.current(){
                    Ok(snapshot)=>return Ok(snapshot),
                    Err("drawn_schema_not_current"|"part_has_not_drawn_current_schema")=>self.tick(Input::default())?,
                    Err(e)=>return Err(e),
                }
            }
            self.current()
        }
        fn target(&mut self, index: usize, expected_kind: &'static str) -> Result<Target, &'static str> {
            let snapshot = self.adopt_current()?;
            let face = snapshot.interface();
            let w = face.widgets.get(index).ok_or("target_index_missing")?;
            let kind = match w.kind { ir::Kind::Button{..}=>"button",ir::Kind::Menu{..}=>"menu",ir::Kind::Knob{..}=>"knob",ir::Kind::Table{..}=>"table",_=>"unsupported" };
            if kind != expected_kind { return Err("target_kind_mismatch"); }
            let component=if let ir::Kind::Table{columns,steps_shown,..}=&w.kind {
                let shown=steps_shown.unwrap_or(*columns).min(*columns) as usize;
                if shown==0{return Err("table_has_no_shown_columns");} Some(shown/2)
            }else{None};
            let id=if let Some(component)=component {
                *w.components.get(component).ok_or("table_component_unbound")?
            }else{let ir::Binding::Control(id)=w.binding else{return Err("target_control_unbound");};id};
            let drawn_page=FACE.with(|v|v.borrow().as_ref().and_then(|v|v.drawn)).ok_or("drawn_page_unavailable")?.2;
            if w.page.0!=drawn_page{return Err("target_not_on_drawn_page");}
            let mut node = Some(ir::WidgetRef(index));
            while let Some(at) = node {
                self.charge(1)?;
                let row = face.widgets.get(at.0).ok_or("target_parent_invalid")?;
                if row.hidden || !row.enabled { return Err("target_ancestor_hidden_or_disabled"); }
                node = row.parent;
            }
            let view = self.gate.params.shared.view.lock().unwrap();
            let source = view.parts[0].interfaces.iter().position(|f|f.source==ir::Source::FalconLua).ok_or("source_unavailable")?;
            Ok(Target {index,component,id,schema:snapshot.generation(),surface:format!("part-0-epoch-{}-script-{source}-ir-{index}",self.epoch),kind})
        }
        fn target_still_current(&self, t: &Target) -> Result<(), &'static str> {
            let s = self.current()?;
            if s.generation() != t.schema || s.interface().widgets.get(t.index).is_none_or(|w|{
                if let Some(component)=t.component{w.components.get(component)!=Some(&t.id)}else{w.binding!=ir::Binding::Control(t.id)}
            }) { Err("target_schema_changed_before_down") } else { Ok(()) }
        }
        fn click(&mut self, target: &Target) -> Result<Value, &'static str> {
            if self.actions >= MAX_ACTIONS { return Err("action_bound"); }
            self.actions += 1; self.submitted.clear();
            let before = self.owner.value(target.id);
            let at = self.hit(&target.surface)?;
            self.target_still_current(target)?;
            self.pointer(at,true)?;
            self.pointer(at,false)?; // Always deliver actual physical up, even if callback changed schema.
            for _ in 0..2 { self.tick(Input::default())?; }
            let ack = self.ack()?;
            let expected = format!("control-{}",target.id.0);
            Ok(json!({"widget":target.index,"control_id":target.id.0.to_string(),"schema_at_down":target.schema,
                "kind":target.kind,"at":[at.x,at.y],"submitted_exact_target":self.submitted.contains(&expected),
                "value_before":before,"value_after":self.owner.value(target.id),"owner":ack,
                "queue_scope":"Submitted is queue admission, owner marker is an ordered attempt, neither proves the intended DSP law"}))
        }
        fn ack(&mut self) -> Result<Value, &'static str> {
            if self.receipts >= MAX_RECEIPTS { return Err("owner_receipt_bound"); }
            self.receipts += 1;
            let deadline = Instant::now()+Duration::from_secs(3);
            let mut request = self.gate.core.request_uvi_diagnostics(0,self.epoch,&self.owner,Duration::from_secs(3)).map_err(|_|"owner_request_refused")?;
            loop {
                if self.observations.elapsed() > Duration::from_secs(10) || Instant::now() > deadline || self.polls >= 10_000 {
                    request.cancel(); return Err("owner_observation_timeout");
                }
                self.polls += 1;
                self.host_block.service(&mut self.gate.core)?;
                self.gate.params.shared.widget_gate_readback(&mut self.gate.core);
                let reply = self.gate.core.poll_uvi_diagnostics(&mut request).map_err(|_|"owner_changed_during_ack")?;
                if let DiagnosticPoll::Complete(a) = reply.poll {
                    if reply.commands_drained {
                        self.owner_checks &= a.sample_counts_conserved() && a.runtime_faults==0 && a.runtime_budgets==0 && a.samples.failed==0 && a.samples.cancelled==0;
                        return Ok(json!({"request_id":a.request_id,"ordered_drained":true,"events_executed":a.events_executed,
                            "commands_collected":a.commands_collected,"commands_pushed":a.commands_pushed,
                            "ui_revision":a.ui_revision,"runtime_faults":a.runtime_faults,"runtime_budgets":a.runtime_budgets,
                            "next_due_ms":a.next_due_ms,"sample_counts_conserved":a.sample_counts_conserved(),
                            "samples":{"requested":a.samples.requested,"successful":a.samples.successful,"failed":a.samples.failed,
                                "cancelled":a.samples.cancelled,"pending":a.samples.pending},
                            "initialization_completion":"unobservable_in_current_owner_ack"}));
                    }
                } else if !matches!(reply.poll, DiagnosticPoll::Pending) { return Err("owner_ack_terminal_without_completion"); }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        fn capture(&mut self) -> Result<(), &'static str> {
            if self.resources.len() >= MAX_PAINTS { return Err("resource_snapshot_bound"); }
            let remaining=MAX_WORK.checked_sub(self.work).ok_or("inspection_bound")?;
            FACE.with(|v| {if let Some(v)=v.borrow_mut().as_mut(){v.requested=true;v.report=None;v.work_remaining=remaining;}});
            let tick=self.tick(Input::default());
            // The Part hook can have performed real inspection before tick's
            // later frame/submission/clock refusal. Consume that one receipt
            // even on Err; never rerun the observer to reconstruct its work.
            let report=FACE.with(|v|v.borrow_mut().as_mut().and_then(|v|v.report.take()));
            let observed=if let Some(report)=report {
                let work=report["inspection_work"].as_u64().and_then(|n|usize::try_from(n).ok()).unwrap_or(0);
                let charged=self.charge(work);self.resources.push(report);charged
            }else{Err("part_asset_hook_not_reached")};
            tick?;observed
        }
        fn paint(&mut self) -> Result<(), &'static str> {
            if self.paints >= MAX_PAINTS { return Err("pixel_snapshot_bound"); }
            self.paints += 1;
            let bytes = super::super::tests::pixels(&self.gate.editor.ui,1500,1100);
            if bytes.len() != 6_600_000 { return Err("pixel_geometry_mismatch"); }
            let first = &bytes[..4];
            let nonbackground = bytes.chunks_exact(4).filter(|p|p.iter().zip(first).any(|(a,b)|a.abs_diff(*b)>8)).count();
            self.pixels.push(json!({"width":1500,"height":1100,"rgba_bytes":bytes.len(),"sha256":hash(&bytes),
                "nonbackground_pixels":nonbackground,"scope":"Actual current Part MUI CPU viewport, discarded pixels; no native-host equality"}));
            Ok(())
        }
        fn optional_visible(&mut self, kind: &'static str) -> Result<Option<Target>, &'static str> {
            let snapshot = self.adopt_current()?;
            let face = snapshot.interface();
            if face.widgets.len() > 48_000 { return Err("widget_inventory_bound"); }
            for (index,w) in face.widgets.iter().enumerate() {
                self.charge(1)?;
                let matches = match kind {"menu"=>matches!(w.kind,ir::Kind::Menu{..}),"knob"=>matches!(w.kind,ir::Kind::Knob{..}),"table"=>matches!(w.kind,ir::Kind::Table{..}),_=>false};
                if !matches { continue; }
                match self.target(index,kind) {
                    Ok(t) if self.gate.editor.ui.scene().and_then(|s|s.surface(&t.surface)).is_some_and(|s|!s.disabled) => return Ok(Some(t)),
                    Ok(_) | Err("target_ancestor_hidden_or_disabled") | Err("target_control_unbound") | Err("target_not_on_drawn_page") => {},
                    Err(e) => return Err(e),
                }
            }
            Ok(None)
        }
        fn menu(&mut self, target: &Target) -> Result<Value, &'static str> {
            let before = self.owner.value(target.id);
            let mut row = self.click(target)?;
            let mut visited = BTreeSet::new();
            let mut selected = None;
            for depth in 0..=2 {
                let snapshot = self.adopt_current()?;
                let w = snapshot.interface().widgets.get(target.index).ok_or("menu_gone")?;
                let ir::Kind::Menu{items} = &w.kind else { return Err("menu_changed_kind"); };
                self.charge(items.len())?;
                selected = items.iter().enumerate().find_map(|(i,item)| {
                    let key=format!("{}-item-{i}",target.surface);
                    (item.visible && Some(f64::from(item.value)) != before
                        && self.gate.editor.ui.scene().and_then(|s|s.surface(&key)).is_some_and(|s|!s.disabled))
                        .then_some((i,item.value,key))
                });
                if selected.is_some() || depth==2 {break;}
                let prefix=format!("{}-category-",target.surface);
                let mut category=None::<(String,f64)>;
                let mut inspected=0usize;
                if let Some(scene)=self.gate.editor.ui.scene(){
                    let work=&mut self.work;
                    for surface in scene.surfaces(){
                        charge_work(work,1)?;
                        inspected=inspected.checked_add(1).ok_or("scene_inspection_overflow")?;
                        if inspected>48_000{return Err("scene_inspection_bound");}
                        let key=surface.key.as_str();
                        if !surface.disabled && key.starts_with(&prefix) && !visited.contains(key)
                            && category.as_ref().is_none_or(|(_,x)|surface.frame.x>*x){
                            category=Some((key.to_owned(),surface.frame.x));
                        }
                    }
                }
                let Some((category,_))=category else{break;};
                if self.actions>=MAX_ACTIONS{return Err("action_bound");}self.actions+=1;
                let at=self.hit(&category)?;
                self.target_still_current(target)?;
                self.pointer(at,true)?;self.pointer(at,false)?;
                visited.insert(category);
            }
            row["actual_category_actions"]=json!(visited.len());
            if let Some((index,value,key)) = selected {
                if self.actions>=MAX_ACTIONS{return Err("action_bound");}self.actions+=1;
                let at = self.hit(&key)?;
                self.target_still_current(target)?;
                self.pointer(at,true)?;self.pointer(at,false)?;
                let ack = self.ack()?;
                row["leaf"] = json!({"original_item_index":index,"authored_value":value,"owner_value":self.owner.value(target.id),
                    "at":[at.x,at.y],"value_observed":self.owner.value(target.id)==Some(f64::from(value)),
                    "submitted_exact_target":self.submitted.contains(&format!("control-{}",target.id.0)),"owner":ack});
            } else { row["leaf"] = json!({"status":"no_distinct_enabled_leaf_in_actual_scene_within_two_category_actions"}); }
            if self.actions>=MAX_ACTIONS{return Err("action_bound");}self.actions+=1;
            self.tick(Input{keys:vec![KeyPress{key:Key::Escape,mods:Mods::default()}],..Default::default()})?;
            Ok(row)
        }
        fn drag(&mut self, target: &Target, table: bool) -> Result<Value, &'static str> {
            if self.actions >= MAX_ACTIONS { return Err("action_bound"); } self.actions+=1;
            self.submitted.clear();
            let at = self.hit(&target.surface)?;
            self.target_still_current(target)?;
            let old = self.owner.value(target.id);
            self.pointer(at,true)?;
            self.pointer(Point::new(at.x,at.y-20.),true)?;
            self.pointer(Point::new(at.x,at.y-20.),false)?;
            let ack = self.ack()?;
            Ok(json!({"widget":target.index,"control_id":target.id.0.to_string(),"table":table,
                "component":target.component,"value_before":old,"value_after":self.owner.value(target.id),"submitted_controls":self.submitted.len(),
                "submitted_exact_component":self.submitted.contains(&format!("control-{}",target.id.0)),
                "relative_pointer_travel_px":20,"owner":ack}))
        }
        fn graph(&self)->Value {
            self.gate.core.diagnostic_live_graph_runtime(0,self.epoch,&self.players,&[],&[])
                .map_or_else(|e|json!({"status":e,"selected_actual_player_nodes":self.players}),|v|v)
        }
        fn finish_audio(&mut self) -> Result<(), &'static str> {
            let held = self.held_at.ok_or("note_not_started")?;
            let done = self.gate.core.clock(0).checked_sub(held).ok_or("held_clock_reversed")?;
            if done > NOTE_FRAMES { return Err("held_note_frame_bound"); }
            self.render((NOTE_FRAMES-done) as usize)?;
            self.gate.core.play(0,Event::midi1(0x80,60,0));
            self.held_at=None;
            self.render(24_000)
        }
    }

    fn execute(profile: Profile, bank_hash: &str) -> Result<Value, &'static str> {
        if bank_hash.len()!=64 || !bank_hash.bytes().all(|c|c.is_ascii_hexdigit()) { return Err("root_identity_receipt_missing"); }
        if profile==Profile::Clarinet && bank_hash!="e6e16c03efc9e3a39484d6326b9ba5634314bc896d6523c83e661b1b279a884a" { return Err("root_clarinet_identity_mismatch"); }
        let _guard=CaptureGuard;
        let mut r=Run::load(profile)?;
        let execution=(|| -> Result<Value,&'static str> {
        // Initial helper load draws occurred before capture was armed.
        for _ in 0..2 {r.tick(Input::default())?;}
        let baseline=r.ack()?;
        r.rows.push(json!({"phase":"constructor_returned","owner":baseline,"all_on_init_completed":Value::Null}));
        r.capture()?;r.paint()?;
        let initial_problems=r.gate.core.problems(0);
        r.held_at=Some(r.gate.core.clock(0));
        r.gate.core.play(0,Event::midi1(0x90,60,96));
        let note_ack=r.ack()?;
        r.rows.push(json!({"phase":"note_on_ordered_attempt_before_pcm","owner":note_ack}));
        r.render(6000)?;
        let before_navigation=r.gate.core.voice_taps(0);
        let source_before=r.graph();
        if profile==Profile::Clarinet {
            let t=r.target(76,"menu")?;
            let current=r.current()?;let w=&current.interface().widgets[76];
            if w.menu_hierarchy.is_none() || [w.rect.x,w.rect.y,w.rect.width,w.rect.height]!=[541.,363.,100.,21.] {return Err("clarinet_retained_menu_shape_changed");}
            let row=r.menu(&t)?;r.rows.push(json!({"phase":"retained_visible_menu","action":row}));
            if let Some(t)=r.optional_visible("knob")? {let row=r.drag(&t,false)?;r.rows.push(json!({"phase":"visible_knob","action":row}));}
        } else if let Some(t)=r.optional_visible("menu")? {let row=r.menu(&t)?;r.rows.push(json!({"phase":"visible_menu","action":row}));}
        else {r.rows.push(json!({"phase":"visible_menu","status":"not_reachable_on_current_authored_page"}));}
        if let Some(t)=r.optional_visible("table")? {let row=r.drag(&t,true)?;r.rows.push(json!({"phase":"visible_table","action":row}));}
        else {r.rows.push(json!({"phase":"visible_table","status":"not_reachable_without_guessed_authored_navigation"}));}
        r.capture()?;r.paint()?;
        let after_navigation=r.gate.core.voice_taps(0);
        let voice_graph=r.graph();
        r.finish_audio()?;
        if profile==Profile::Ao {
            for _ in 0..2 {r.tick(Input::default())?;}
            let t=r.target(23,"button")?;
            let s=r.current()?;
            if s.interface().widgets[23].source_id!=Some(24) || t.id.0!=0x55564900000000000000001800000000u128 {return Err("ao_retained_layer_next_identity_changed");}
            let row=r.click(&t)?;r.rows.push(json!({"phase":"retained_layer_next_after_note_release","action":row}));
            r.capture()?;r.paint()?;
        }
        let final_ack=r.ack()?;
        let source_after=r.graph();
        let final_problems=r.gate.core.problems(0);
        let storage=r.owner.registry_storage().map_err(|_|"registry_status_unavailable")?;
        let work=r.owner.schema_work();
        let outcomes=r.gate.core.widget_gate_engine_outcomes(0);
        for o in outcomes.into_iter().take(64) {r.outcomes.push(json!({"write":o.write,"address":o.address.map(|a|json!({"parameter":a.parameter,"group":a.group,"slot":a.slot,"generic":a.generic})),"applied":o.result.is_ok()}));}
        let taps=|rows:[Option<sampler_core::VoiceTap>;16]| rows.into_iter().flatten().map(|v|json!({"group":v.group,"key":v.key,"velocity":v.velocity,"phase":v.phase,"level":v.level})).collect::<Vec<_>>();
        let samples=r.audio.iter().enumerate().map(|(i,v)|json!({"start":WINDOWS[i].0,"end":WINDOWS[i].1,"observed_frames":v[2],"rms_stereo":(v[0]/(v[2]*2.).max(1.)).sqrt(),"peak":v[1]})).collect::<Vec<_>>();
        let audio_nonzero=r.audio.iter().zip(WINDOWS).all(|(v,(start,end))|v[1]>0. && v[2]==(end-start) as f64);
        let model=r.owner.snapshot();
        r.charge(model.interface().widgets.len())?;
        let own_hidden=model.interface().widgets.iter().filter(|w|w.hidden).count();
        let resources_ok=r.resources.last().is_some_and(|v|v["requested_page_complete"]==true&&v["bitmap"]==true&&v["schema_generation"]==model.generation())
            && r.resources.iter().all(|v|v["probe_dropped"]==0);
        let faults=r.owner.fault_counts();
        let owner_healthy=faults.init.values().all(|v|*v==0)&&faults.runtime.values().all(|v|*v==0)&&faults.setter_type_mismatches.is_empty();
        let core_healthy=r.nonfinite==0 && final_problems.capacity_drops==initial_problems.capacity_drops
            && final_problems.underruns==initial_problems.underruns && final_problems.stream_errors==initial_problems.stream_errors
            && final_problems.nonfinite==initial_problems.nonfinite && final_problems.ignored_input==initial_problems.ignored_input
            && final_problems.script_overruns==initial_problems.script_overruns && final_problems.lua_faults==initial_problems.lua_faults;
        let required_action=match profile {
            Profile::Ao=>r.rows.iter().any(|v|v["phase"]=="retained_layer_next_after_note_release"&&v["action"]["submitted_exact_target"]==true&&v["action"]["owner"]["ordered_drained"]==true),
            Profile::Clarinet=>r.rows.iter().any(|v|v["phase"]=="retained_visible_menu"&&v["action"]["leaf"]["value_observed"]==true&&v["action"]["leaf"]["submitted_exact_target"]==true&&v["action"]["leaf"]["owner"]["ordered_drained"]==true),
        };
        let ui_checks=owner_healthy&&core_healthy&&resources_ok&&required_action&&r.owner_checks;
        Ok(json!({"schema":"kontra-real-library-ui09-v1","profile":profile.key(),"execution_status":"bounded_observations_completed",
            "root_bank_sha256_preflight":bank_hash,"bank_identity_scope":"Root-supplied pre/post hash receipt required; helper does not certify or rewrite bank identity",
            "part_epoch":r.epoch,"source":"FalconLua","required_product_ui_checks_pass":ui_checks,
            "checks":{"required_real_action":required_action,"requested_page_resources":resources_ok,"owner_faults_zero":owner_healthy,"no_new_core_health_fault":core_healthy,"all_observed_owner_checks":r.owner_checks},
            "owner_fault_counts":{"init":faults.init,"runtime":faults.runtime,"setter_type_mismatch_count":faults.setter_type_mismatches.len()},
            "initialization_completion":"UNOBSERVABLE_DIRECT_INIT_ORIGIN; constructor and earlier owner attempt observed only",
            "native_equivalence":"unverified","whole_library_acceptance":"NOT_ESTABLISHED",
            "actions":r.rows,"final_owner":final_ack,"source_before_actions":source_before,"source_after_actions":source_after,
            "source_observation_scope":"Same actual compiled target nodes and immutable AssetIds, at most3 targets; no whole-layer/sample census",
            "current_model":{"schema_generation":model.generation(),"widgets":model.interface().widgets.len(),"pages":model.interface().pages.len(),
                "own_hidden_flags":own_hidden,"ui_omissions":model.interface().unsupported.len(),"declared_assets":model.interface().assets.len(),
                "scope":"Current actual owner model, own_hidden is not effective ancestry visibility; pixels/resources carry actual drawn schema separately"},"actual_part_resources":r.resources,"actual_part_pixels":r.pixels,
            "registry":{"prepared_cells":storage.prepared_cells,"initial_controls":storage.initial_controls,"current_controls":storage.current_controls,
                "retired_snapshots":storage.retired_snapshots,"initial_requested_bytes":storage.initial_requested_payload_bytes,
                "initial_retained_bytes":storage.initial_retained_payload_bytes,"incoming_bytes":storage.incoming_payload_bytes,
                "requested_bytes":storage.requested_payload_bytes,"retained_bytes":storage.retained_payload_bytes,
                "schema_work":{"limit":work.limit,"requested":work.requested,"admitted":work.admitted,"exhausted":work.exhausted}},
            "held_audio":{"windows":samples,"all_requested_windows_nonzero":audio_nonzero,"nonfinite_raw_samples":r.nonfinite,"physical_note_key":60,"velocity":96,
                "note_frames":NOTE_FRAMES,"before_navigation_taps":taps(before_navigation),"after_navigation_taps":taps(after_navigation),"actual_graph":voice_graph,
                "tap_scope":"Group/key taps are not stable NoteId/source handles; only actual_graph provides exact occupied identities when available"},
            "core_problems_before":initial_problems,"core_problems_after":final_problems,"engine_outcomes":r.outcomes,
            "engine_outcomes_complete":false,"engine_outcome_scope":"At most64 current drained outcomes; no inferred full transaction history or widget-to-DSP identity from value equality",
            "active_voices_after_tail":r.gate.core.voices().active,
            "requested_page_assets_observed_complete":resources_ok,"host_note_end_sink_acceptances":r.note_ends,
            "host_note_end_scope":"MIDI1 notes need not call host NOTE_END sink; count is not Core retirement proof",
            "budget":{"draws":r.draws,"inspection_work":r.work,"rendered_frames":r.rendered,"actions":r.actions,"owner_receipts":r.receipts,
                "zero_service_polls":r.polls,"pixel_observations":r.paints,"wall_ms":r.started.elapsed().as_millis()},
            "remaining":"Direct-init completion, declared-unrequested resources, unavailable Table/menu paths, native pixels, full DSP topology and accepted target law remain separate dimensions"}))
        })();
        // Neither final closure nor refused-gesture cleanup can grant fuel or
        // advance PCM; close the actual block after the last owner observation.
        if let Some(at)=r.pointer_held {let _=r.release_pointer(at,true);}
        r.host_block.finish(&mut r.gate.core,&mut r.note_ends);
        let pointer_receipt=json!({"downs":r.pointer_downs,"ups":r.pointer_ups,"cleanup_ups":r.pointer_cleanup_ups,
            "held":r.pointer_held.is_some(),"cleanup_failure":r.pointer_cleanup_failure});
        match execution {
            Ok(mut report)=>{let cleanup_ok=r.pointer_held.is_none()&&r.pointer_cleanup_failure.is_none()&&r.pointer_downs==r.pointer_ups;
                report["checks"]["physical_pointer_cleanup"]=json!(cleanup_ok);
                if !cleanup_ok{report["required_product_ui_checks_pass"]=json!(false);}
                report["physical_pointer_cleanup"]=pointer_receipt;report["host_note_end_sink_acceptances"]=json!(r.note_ends);Ok(report)},
            Err(reason)=>Ok(json!({"profile":profile.key(),"execution_status":"refused","reason":reason,"part_epoch":r.epoch,
                "actions_completed":r.rows,"resource_observations":r.resources,"pixel_observations":r.pixels,
                "draws":r.draws,"rendered_frames":r.rendered,"inspections":r.work,"current_core_problems":r.gate.core.problems(0),
                "physical_pointer_cleanup":pointer_receipt,"host_note_end_sink_acceptances":r.note_ends,
                "native_equivalence":"unverified","whole_library_acceptance":"NOT_ESTABLISHED"})),
        }
    }

    fn profile_test(profile: Profile) {
        let out=std::env::var("KONTRA_LIBRARY_UI_OUT").expect("explicit private numeric report path");
        let out=Path::new(&out);
        assert!(out.is_absolute() && out.starts_with("/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts") && !out.exists(),"new private report required");
        let identity=std::env::var("KONTRA_LIBRARY_UI_BANK_SHA256").expect("root preflight bank digest required");
        let hook=std::panic::take_hook();std::panic::set_hook(Box::new(|_|{}));
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||execute(profile,&identity)));
        std::panic::set_hook(hook);
        let (report,complete)=match result {Ok(Ok(v))=>{let complete=v["execution_status"]=="bounded_observations_completed"&&v["required_product_ui_checks_pass"]==true;(v,complete)},Ok(Err(e))=>(json!({"profile":profile.key(),"execution_status":"refused","reason":e}),false),Err(_)=>(json!({"profile":profile.key(),"execution_status":"panic_payload_discarded"}),false)};
        let bytes=serde_json::to_vec_pretty(&report).unwrap();
        let within_report=bytes.len()<=MAX_REPORT;
        let bytes=if within_report{bytes}else{serde_json::to_vec(&json!({"profile":profile.key(),"execution_status":"report_bound"})).unwrap()};
        use std::io::Write;
        let mut options=std::fs::OpenOptions::new();options.create_new(true).write(true);
        #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
        let mut file=options.open(out).unwrap();file.write_all(&bytes).unwrap();
        assert!(complete && within_report,"bounded library UI observations failed; private numeric receipt retained");
    }


    fn authored_directory(tag:&str)->std::path::PathBuf {
        static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
        // A writable scratch directory: the runner's temporary directory when
        // it is set, otherwise the workspace scratch, otherwise the platform
        // temporary directory. A hard-coded workspace path is not writable on
        // a CI runner.
        let base=match std::env::var_os("TMPDIR"){Some(dir)=>std::path::PathBuf::from(dir),None=>{
            let local=std::path::PathBuf::from("/mnt/Windows11/DEV_WORKSPACE/BuildScratch");
            if std::fs::create_dir_all(&local).is_ok(){local}else{std::env::temp_dir()}}};
        let path=base.join(format!("ui09-{tag}-{}-{}",std::process::id(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&path).unwrap();path
    }
    #[test]
    fn requested_page_picture_success_does_not_mask_real_manual_wave_failure() {
        let dir=authored_directory("wave");let path=dir.join("authored.uvip");
        let xml=r#"<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[
            setSize(320,200); local art=Button{'Art',bounds={10,10,100,30}};
            art:setStripImage('picture.png',1); local w=WaveView{'Display',bounds={20,60,240,96}};
            w.sample='actually-missing.wav'
        ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
        std::fs::write(&path,xml).unwrap();
        moose::core::screenshot::save_png(&dir.join("picture.png"),&[20u8,40,60,255].repeat(4),2,2);
        let host=sampler_uvi::script::ScriptHost::new(xml,(),Default::default()).unwrap();
        let face=host.interface();let values=host.control_values().into_iter().collect();
        let mut assets=super::super::ir_view::Assets::default();let deadline=Instant::now()+Duration::from_secs(3);
        let report=loop {
            assets.prepare(&path,&face,ir::PageRef(0),ir::Presentation::Bitmap,1.,&values);
            let (report,complete,_)=requested_waves(&assets,&face,ir::PageRef(0),MAX_WORK).unwrap();
            if report["provider"]["rows"].as_array().unwrap().iter().any(|v|v["status"]=="source_unavailable")&&assets.scan().decode_ok==1 {
                assert!(!complete);break report;
            }
            assert!(Instant::now()<deadline,"actual picture/manual-wave requests did not settle");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(assets.pending(),0);assert_eq!(assets.scan().decode_ok,1);assert_eq!(assets.probe_dropped(),0);
        assert_eq!(report["unresolved_requested"],1);
        // Same successful picture without a requested WaveView remains eligible.
        let mut picture_only=face.clone();for w in &mut picture_only.widgets{w.uvi_wave_view_request=None;}
        let mut clean=super::super::ir_view::Assets::default();let deadline=Instant::now()+Duration::from_secs(3);
        loop {
            clean.prepare(&path,&picture_only,ir::PageRef(0),ir::Presentation::Bitmap,1.,&values);
            if clean.pending()==0&&clean.scan().decode_ok==1{break;}
            assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(1));
        }
        let (_,complete,_)=requested_waves(&clean,&picture_only,ir::PageRef(0),MAX_WORK).unwrap();assert!(complete);
        // An actually hidden, never requested manual source is not a failure.
        let mut hidden=face.clone();for w in &mut hidden.widgets{if w.uvi_wave_view_request.is_some(){w.hidden=true;}}
        let (_,complete,_)=requested_waves(&super::super::ir_view::Assets::default(),&hidden,ir::PageRef(0),MAX_WORK).unwrap();assert!(complete);
        drop(assets);drop(clean);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn wave_observation_preflights_depth_rows_and_work_before_provider_projection() {
        let xml="<UVI4><Program><EventProcessors><ScriptProcessor><script>local w=WaveView('Actual');w.sample='missing.wav'</script></ScriptProcessor></EventProcessors></Program></UVI4>";
        let host=sampler_uvi::script::ScriptHost::new(xml,(),Default::default()).unwrap();let mut face=host.interface();
        let assets=super::super::ir_view::Assets::default();
        assert!(matches!(requested_waves(&assets,&face,ir::PageRef(0),0),Err("wave_inspection_bound")));
        face.widgets[0].parent=Some(ir::WidgetRef(0));
        assert!(matches!(requested_waves(&assets,&face,ir::PageRef(0),MAX_WORK),Err("wave_ancestry_or_inspection_bound")));
    }
    #[test]
    fn post_down_frame_refusal_delivers_one_real_up_and_never_retries_button_or_drag() {
        for (kind,constructor) in [("button","Button{'Gesture',bounds={10,10,100,35}}"),("knob","Knob{'Gesture',0,0,1,bounds={10,10,100,70}}")]{
            let _capture=CaptureGuard;let dir=authored_directory(kind);let path=dir.join("authored.uvip");
            let xml=format!("<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[setSize(240,140);local count=Label{{'Count',bounds={{130,10,80,30}}}};count.text='0';local g={constructor};g.triggeredOnMouseDown=true;g.changed=function() count.text=tostring(tonumber(count.text)+1) end]]></script></ScriptProcessor></EventProcessors></Program></UVI4>");
            std::fs::write(&path,xml).unwrap();let mut selection=Selection::default();selection.parts=vec![Part{path:path.to_str().unwrap().to_owned(),program:0,view:1,..Default::default()}];
            let gate=Gate::load(selection);let epoch=gate.core.epoch(0);let owner=gate.params.shared.part(0).unwrap().widget_gate_uvi_owner(epoch).unwrap();
            gate.core.enable_uvi_diagnostics(0,epoch,&owner).unwrap();
            let mut run=Run::observe_gate(gate,owner,epoch,Vec::new(),Instant::now()).unwrap();
            for _ in 0..2{run.tick(Input::default()).unwrap();}
            let snapshot=run.current().unwrap();let index=snapshot.interface().widgets.iter().position(|w|w.name=="Gesture").unwrap();
            let target=run.target(index,kind).unwrap();let at=run.hit(&target.surface).unwrap();let clock=run.gate.core.clock(0);
            run.rendered=MAX_FRAMES; // Real helper frame admission refuses AFTER editor delivered Down.
            assert_eq!(run.pointer(at,true),Err("audio_frame_bound"));
            assert_eq!(run.gate.core.clock(0),clock);assert!(run.pointer_held.is_none());
            assert_eq!((run.pointer_downs,run.pointer_ups,run.pointer_cleanup_ups),(1,1,1));assert!(run.pointer_cleanup_failure.is_none());
            assert!(!run.gate.editor.ui.get(&target.surface).held,"actual MUI capture must be released even though frame budget refused");
            let deadline=Instant::now()+Duration::from_secs(3);let expected=if kind=="button"{"1"}else{"0"};
            loop {
                let now=run.owner.snapshot();let text=&now.interface().widgets.iter().find(|w|w.name=="Count").unwrap().text;
                if text==expected{break;}assert!(Instant::now()<deadline,"actual owner callback did not settle");std::thread::sleep(Duration::from_millis(1));
            }
            run.host_block.finish(&mut run.gate.core,&mut run.note_ends);drop(run);std::fs::remove_dir_all(dir).unwrap();
        }
    }
    #[test]
    fn diagnostic_zero_service_spends_same_real_host_allowance_without_refilling() {
        let source="on init declare $saved end on on note while ($saved<10000000) inc($saved) end while end on";
        let script=sampler_ksp::compile(source,48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
        let plan=script.bind(sampler_core::Prepared::new(48000,vec![],vec![],0).unwrap()).unwrap();
        let limits=sampler_core::Limits::for_plan(&plan,32,8);let runtime=sampler_core::Runtime::new(plan,limits).unwrap();
        let part=crate::sound::v2::Part::new(runtime,crate::sound::tree::MixTree::instrument("actual exhausted callback")).unwrap();
        let mut core=V2Core::with_parts(1,48000.);core.install(0,Some(Box::new(part)));let epoch=core.epoch(0);
        let mut block=ObservedHostBlock::default();let mut ends=0;
        assert_eq!(block.service(&mut core),Err("zero_service_without_actual_host_block"));
        block.begin(&mut core,32,&mut ends).unwrap();core.play(0,Event::midi1(0x90,60,100));core.render(1);
        let before=core.diagnostic_script_cell(0,epoch,0,0).unwrap();assert!(before>0&&before<10000000);
        let clock=core.clock(0);for _ in 0..16 {block.service(&mut core).unwrap();assert_eq!(core.diagnostic_script_cell(0,epoch,0,0).unwrap(),before);assert_eq!(core.clock(0),clock);}
        block.finish(&mut core,&mut ends);assert_eq!(block.service(&mut core),Err("zero_service_without_actual_host_block"));
        block.begin(&mut core,32,&mut ends).unwrap();core.render(1);assert!(core.diagnostic_script_cell(0,epoch,0,0).unwrap()>before);block.finish(&mut core,&mut ends);
    }

    #[test]
    fn refused_wave_projection_reports_reserved_work_before_any_ancestor_walk_or_retry(){
        let xml="<UVI4><Program><EventProcessors><ScriptProcessor><script>local w=WaveView('Actual');w.sample='missing.wav'</script></ScriptProcessor></EventProcessors></Program></UVI4>";
        let host=sampler_uvi::script::ScriptHost::new(xml,(),Default::default()).unwrap();let mut face=host.interface();let assets=super::super::ir_view::Assets::default();
        face.widgets[0].parent=Some(ir::WidgetRef(0));
        let (_,first)=requested_waves_metered(&assets,&face,ir::PageRef(0),MAX_WORK).unwrap_err();
        assert!(first>=face.widgets.len());let mut total=0;charge_work(&mut total,first).unwrap();
        let (_,second)=requested_waves_metered(&assets,&face,ir::PageRef(0),0).unwrap_err();assert_eq!(second,0);assert_eq!(total,first);
    }

    #[test]
    fn wave_observer_and_returned_rows_are_charged_in_addition_to_preflight(){
        let xml="<UVI4><Program><EventProcessors><ScriptProcessor><script>local w=WaveView('Actual');w.sample='missing.wav'</script></ScriptProcessor></EventProcessors></Program></UVI4>";
        let host=sampler_uvi::script::ScriptHost::new(xml,(),Default::default()).unwrap();let mut face=host.interface();
        let wave=face.widgets.iter().find(|w|w.uvi_wave_view_request.is_some()).unwrap().clone();
        let mut ordinary=wave.clone();ordinary.uvi_wave_view_request=None;ordinary.parent=None;
        face.widgets=vec![ordinary;40_000];face.widgets[0]=wave;face.widgets[0].parent=None;
        let assets=super::super::ir_view::Assets::default();let mut charged=0;
        for _ in 0..3 {let (_,_,work)=requested_waves_metered(&assets,&face,ir::PageRef(0),MAX_WORK-charged).unwrap();assert!(work>=80_002);charge_work(&mut charged,work).unwrap();}
        let (_,work)=requested_waves_metered(&assets,&face,ir::PageRef(0),MAX_WORK-charged).unwrap_err();
        assert_eq!(work,0,"fourth complete roster scan is refused before traversal");assert!(charged<=MAX_WORK);
        let (_,_,exact)=requested_waves_metered(&assets,&face,ir::PageRef(0),MAX_WORK).unwrap();
        assert!(requested_waves_metered(&assets,&face,ir::PageRef(0),exact).is_ok());
        let (_,spent)=requested_waves_metered(&assets,&face,ir::PageRef(0),exact-1).unwrap_err();
        assert_eq!(spent,40_000,"observer is not called after its aggregate reservation refuses");
    }
    #[test]
    fn capture_debits_real_part_hook_on_later_tick_refusal_without_reinspection(){
        let _capture=CaptureGuard;let dir=authored_directory("capture-refusal");let path=dir.join("authored.uvip");
        std::fs::write(&path,"<UVI4><Program><EventProcessors><ScriptProcessor><script>setSize(240,140);local l=Label('Actual');l.text='visible'</script></ScriptProcessor></EventProcessors></Program></UVI4>").unwrap();
        let mut selection=Selection::default();selection.parts=vec![Part{path:path.to_str().unwrap().to_owned(),program:0,view:1,..Default::default()}];
        let gate=Gate::load(selection);let epoch=gate.core.epoch(0);let owner=gate.params.shared.part(0).unwrap().widget_gate_uvi_owner(epoch).unwrap();
        let mut run=Run::observe_gate(gate,owner,epoch,Vec::new(),Instant::now()).unwrap();
        for _ in 0..2{run.tick(Input::default()).unwrap();}
        let widgets=run.owner.snapshot().interface().widgets.len();assert!(widgets>0);
        let clock=run.gate.core.clock(0);run.rendered=MAX_FRAMES;
        assert_eq!(run.capture(),Err("audio_frame_bound"));assert_eq!(run.gate.core.clock(0),clock);
        assert_eq!(run.resources.len(),1);assert_eq!(run.work,2*widgets);
        assert!(FACE.with(|v|v.borrow().as_ref().unwrap().report.is_none()));
        let before=run.work;assert_eq!(run.capture(),Err("audio_frame_bound"));assert_eq!(run.resources.len(),2);
        assert_eq!(run.work,before+2*widgets,"each genuine hook is charged once, never reconstructed or skipped");
        run.host_block.finish(&mut run.gate.core,&mut run.note_ends);drop(run);std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "one exact owned AO normal product profile; explicit root hash and numeric output required"]
    fn actual_ao_pizzicato2_part_ui_acceptance09() { profile_test(Profile::Ao); }
    #[test]
    #[ignore = "one exact owned VWinds normal product profile; explicit root hash and numeric output required"]
    fn actual_vwinds_clarinet_a_part_ui_acceptance09() { profile_test(Profile::Clarinet); }
}
