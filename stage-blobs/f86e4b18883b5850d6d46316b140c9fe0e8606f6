//! Gate receipts contain only counts, source coordinates, hashes and failure enums.
//! Authored resources, values and serialized host state stay in this process.
use super::{native_ui, tests::Harness};
use crate::{plugin::{Load, Part, SamplerParams, Selection}, sound::{Core, v2::V2Core}};
use moose::{mui::mui::prelude::*, prelude::BackgroundTask};
use sampler_ui_ir as ir;
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc, time::{Duration, Instant}};

type Values = BTreeMap<String, ir::Value>;
struct Gate {
    params: Arc<SamplerParams>,
    core: V2Core,
    editor: Harness,
    observing: Option<Values>,
    observed_changes: BTreeSet<String>,
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
        let mut gate = Self { params, core, editor, observing: None, observed_changes: BTreeSet::new() };
        gate.settle();
        gate
    }
    fn tick(&mut self, input: Input) {
        self.editor.tick(input);
        self.core.render(64);
        self.params.shared.widget_gate_readback(&mut self.core);
        Load.run(&self.params);
        if let Some(before) = self.observing.as_ref() {
            let keys = changed(before, &self.values());
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
        for _ in 0..2 { self.tick(Input { pointer: PointerInput {
            pos: Some(p), buttons: if down { Buttons::PRIMARY } else { Buttons::default() },
            ..Default::default()
        }, ..Default::default() }); }
    }
    fn key(&mut self, key: Key, mods: Mods) {
        self.tick(Input { keys: vec![KeyPress { key, mods }], ..Default::default() });
        self.tick(Input::default());
    }
    fn values(&self) -> Values {
        let atoms = self.params.shared.part(0).unwrap();
        let mut out = Values::new();
        // Read the engine or Lua owner, never the renderer's optimistic preview.
        for (id, _) in atoms.control_values() {
            let value = atoms.widget_gate_uvi_value(id)
                .or_else(|| self.core.control_value(0, id));
            if let Some(value) = value { out.insert(format!("control-{}", id.0), ir::Value::Real(value)); }
        }
        let backend = self.core.widget_gate_values(0);
        let view = self.params.shared.view.lock().unwrap();
        for (source, face) in view.parts[0].interfaces.iter().enumerate() {
            for (widget, definition) in face.widgets.iter().enumerate() {
                if let ir::Binding::Variable { script, name } = &definition.binding {
                    let id = ir::ControlId(sampler_ksp::derived_control_id(*script, name).0);
                    if let Some(value) = backend.get(&id) {
                        out.insert(format!("typed-{source}-{widget}"), value.clone());
                    }
                }
            }
        }
        out
    }
    fn targets(&self) -> Vec<(String, String)> {
        let mut targets: BTreeMap<String, String> = native_ui::gate_targets(0).into_iter()
            .filter(|(id, _)| self.editor.ui.scene().unwrap().surface(id).is_some_and(|s| !s.disabled))
            .map(|(id, kind)| (id, format!("native-{kind}"))).collect();
        let view = self.params.shared.view.lock().unwrap();
        let part = &view.parts[0];
        for (source, face) in part.interfaces.iter().enumerate() {
            for (n, widget) in face.widgets.iter().enumerate() {
                if !widget.enabled || !face.visible(ir::WidgetRef(n)) { continue; }
                let kind = match widget.kind {
                    ir::Kind::Knob {..} => "knob", ir::Kind::Slider {..} => "slider",
                    ir::Kind::Button {..} => "button", ir::Kind::Switch => "switch",
                    ir::Kind::Menu {..} => "menu", ir::Kind::ValueEdit {..} => "value-edit",
                    ir::Kind::TextEdit => "text", ir::Kind::Table {..} => "table",
                    ir::Kind::Xy {..} => "xy", ir::Kind::MouseArea => "mouse-area",
                    ir::Kind::FileSelector {..} => "file-selector",
                    
                    _ => continue,
                };
                let id = format!("part-0-epoch-{}-script-{source}-ir-{n}", part.generation);
                if self.editor.ui.scene().unwrap().surface(&id).is_some_and(|s| !s.disabled) { targets.insert(id, kind.into()); }
            }
        }
        targets.into_iter().collect()
    }
    fn hit(&mut self, id: &str) -> Option<Point> {
        let frame = self.editor.ui.scene()?.surface(id)?.frame;
        if frame.size.width <= 0. || frame.size.height <= 0. { return None; }
        for y in [0.5, 0.2, 0.8] { for x in [0.5, 0.2, 0.8] {
            let at = Point::new(frame.x + frame.size.width*x, frame.y + frame.size.height*y);
            self.pointer(at, false);
            if self.editor.ui.get(id).hovered { return Some(at); }
        }}
        None
    }
    fn faults(&mut self) -> usize {
        let p = self.core.problems(0);
        let lua = self.core.scan_lua(0).map_or(0, |f| f.init_count + f.runtime_count + f.budget_hits);
        self.core.scan_runtime_faults(0).len() + lua + (p.nonfinite + p.script_overruns + p.lua_faults) as usize
    }
    fn gesture(&mut self, id: &str, kind: &str, at: Point, attempt: usize) -> Vec<String> {
        self.observing = Some(self.values());
        self.observed_changes.clear();
        match kind {
            "text" | "native-text" | "value-edit" => {
                self.editor.ui.focus(id);
                if kind == "value-edit" { self.key(Key::Enter, Mods::default()); }
                self.key(Key::Char('a'), Mods { ctrl: true, ..Default::default() });
                self.tick(Input { text: if kind == "value-edit" { "1" } else { "Gate name" }.into(), ..Default::default() });
                self.key(Key::Enter, Mods::default());
            }
            "knob" | "slider" | "native-drag" if attempt >= 4 => {
                self.tick(Input { pointer: PointerInput { pos: Some(at), ..Default::default() }, wheel: Vec2::new(0., if attempt == 4 { -120. } else { 120. }), ..Default::default() });
            }
            "knob" | "slider" | "native-drag" | "table" | "xy" | "mouse-area" => {
                let delta = match attempt { 0 => Vec2::new(0., -80.), 1 => Vec2::new(80., 0.), 2 => Vec2::new(0., 80.), _ => Vec2::new(-80., 0.) };
                self.pointer(at, true);
                self.pointer(Point::new(at.x+delta.x, at.y+delta.y), true);
                self.pointer(Point::new(at.x+delta.x, at.y+delta.y), false);
            }
            "file-selector" => {
                let prefix = format!("{id}-file-");
                let rows: Vec<_> = self.editor.ui.scene().unwrap().surfaces()
                    .filter(|surface| !surface.disabled && surface.key.as_str().starts_with(&prefix))
                    .map(|surface| surface.key.to_string()).collect();
                if let Some(row) = rows.get(attempt % rows.len().max(1)) {
                    if let Some(point) = self.hit(row) { self.pointer(point, true); self.pointer(point, false); }
                }
            }
            _ => {
                self.pointer(at, true); self.pointer(at, false); self.settle();
                if kind == "menu" || kind == "native-click" {
                    // A menu choice is another real pointer target. Do not call callbacks directly.
                    let popup: Vec<_> = if kind == "native-click" {
                        self.targets().into_iter().filter(|(key, action)| key.contains("/popover/") && action == "native-click")
                            .map(|(key, _)| key).collect()
                    } else {
                        self.editor.ui.scene().unwrap().surfaces().filter(|s| !s.disabled && s.key.as_str().contains("-item-"))
                            .map(|s| s.key.to_string()).collect()
                    };
                    if !popup.is_empty() {
                        let child = &popup[attempt % popup.len()];
                        if let Some(p) = self.hit(child) { self.pointer(p, true); self.pointer(p, false); }
                    }
                }
            }
        }
        self.settle();
        self.observing = None;
        std::mem::take(&mut self.observed_changes).into_iter().collect()
    }
}

fn changed(before: &Values, after: &Values) -> Vec<String> {
    after.iter().filter(|(key, value)| before.get(*key).is_some_and(|old| old != *value))
        .map(|(key, _)| key.clone()).collect()
}
fn retained(keys: &[String], saved: &Values, reloaded: &Values) -> bool {
    !keys.is_empty() && keys.iter().all(|key| saved.get(key).is_some() && saved.get(key) == reloaded.get(key))
}

#[test]
fn persistence_receipt_requires_each_changed_parameter() {
    let a = BTreeMap::from([("control-1".into(), ir::Value::Real(1.)), ("typed-0-2".into(), ir::Value::Text("edited".into()))]);
    let b = BTreeMap::from([("control-1".into(), ir::Value::Real(1.)), ("typed-0-2".into(), ir::Value::Text("old".into()))]);
    assert!(retained(&["control-1".into()], &a, &b));
    assert!(!retained(&["control-1".into(), "typed-0-2".into()], &a, &b));
    assert!(!retained(&[], &a, &b));
}

#[test]
#[ignore = "release gate: set KONTRA_WIDGET_GATE_PATH and KONTRA_WIDGET_GATE_PROGRAM; all private values stay in RAM"]
fn original_widget_gestures() {
    let path = std::env::var("KONTRA_WIDGET_GATE_PATH").expect("gate path required");
    let program = std::env::var("KONTRA_WIDGET_GATE_PROGRAM").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut selection = Selection::default();
    selection.parts = vec![Part { path, program, view: 1, ..Default::default() }];
    if std::env::var("KONTRA_WIDGET_GATE_CONDITION").as_deref() == Ok("product-warm") {
        drop(Gate::load(selection.clone()));
    }
    let mut gate = Gate::load(selection);
    let deadline = Instant::now() + Duration::from_secs(180);
    let paint_deadline = Instant::now() + Duration::from_secs(15);
    while gate.targets().is_empty() && Instant::now() < paint_deadline {
        gate.settle();
        if gate.params.shared.view.lock().unwrap().parts[0].tree.is_none() { break; }
    }
    // Visit every authored source/page through the editor's actual selectors.
    let sources = gate.params.shared.view.lock().unwrap().parts[0].interfaces.len().max(1);
    let mut seen = BTreeSet::new();
    let mut results = Vec::new();
    let mut initial_faults = gate.faults();
    let mut exhausted = false;
    for source in 0..sources {
        let selector = format!("face-0-{source}");
        if gate.editor.ui.scene().unwrap().surface(&selector).is_some() { gate.editor.press(&selector); gate.settle(); }
        let pages = gate.params.shared.view.lock().unwrap().parts[0].interfaces.get(source).map_or(1,|face|face.pages.len().max(1));
        for page in 0..pages {
            let selector = format!("face-page-0-{page}");
            if gate.editor.ui.scene().unwrap().surface(&selector).is_some() { gate.editor.press(&selector); gate.settle(); }
            loop {
                let candidates = gate.targets();
                let Some((id, kind)) = candidates.into_iter().find(|(id, _)| !seen.contains(id)) else { break };
                seen.insert(id.clone());
                let mut reason = "parameter-unchanged";
                let mut keys = Vec::new();
                if Instant::now() >= deadline { exhausted = true; reason = "probe-budget"; }
                else if let Some(at) = gate.hit(&id) {
                    for attempt in 0..6 {
                        keys = gate.gesture(&id, &kind, at, attempt);
                        if !keys.is_empty() { reason = "persistence-pending"; break; }
                        if gate.editor.ui.scene().unwrap().surface(&id).is_none() { reason = "navigation-only"; break; }
                    }
                } else { reason = "occluded-or-outside-viewport"; }
                let faults = gate.faults();
                initial_faults += faults;
                if faults > 0 { reason = "script-or-render-fault"; }
                results.push((serde_json::json!({"target_sha256": sha2::Sha256::digest(id.as_bytes()).iter().map(|b|format!("{b:02x}")).collect::<String>(), "source": source, "page": page, "kind": kind, "reason": reason, "parameter_changes": keys.len(), "value_changed": !keys.is_empty(), "parameter_reached": !keys.is_empty(), "persistence": false}), keys));
                if exhausted { break; }
            }
            if exhausted { break; }
        }
        if exhausted { break; }
    }
    gate.settle();
    let saved_values = gate.values();
    let mut saved = gate.params.selection.read().unwrap().clone();
    gate.params.shared.capture_ui_controls(&mut saved);
    // Exercise the same serialized host Part boundary; never write it to disk.
    let state = serde_json::to_vec(&saved.parts).unwrap();
    saved.parts = serde_json::from_slice(&state).unwrap();
    let native_diagnostics = native_ui::gate_diagnostics();
    // Native callback-context persistence remains excluded from this alpha.
    let captured_state_bytes: Option<usize> = None;
    let captured_controls = saved.parts[0].control_values.len();
    drop(gate);
    let mut reloaded = Gate::load(saved);
    let reload_values = reloaded.values();
    let faults = reloaded.faults() + initial_faults + native_diagnostics.len() + native_ui::gate_diagnostics().len();
    let reload_status = reloaded.params.shared.view.lock().unwrap().parts[0].status.clone();
    let reload_failed = reload_status.starts_with("Load failed:");
    let reload_failure = if reload_status.contains("schema changed") { "script-schema-changed" }
        else if reload_status.contains("type changed") { "script-value-type-changed" }
        else if reload_status.contains("Script persistence: InvalidInput") { "script-restore-invalid-input" }
        else if reload_status.contains("Script persistence: Capacity") { "script-restore-capacity" }
        else if reload_failed { "load-failed" } else { "none" };
    for (row, keys) in &mut results {
        if row["reason"] == "persistence-pending" {
            let pass = retained(keys, &saved_values, &reload_values);
            row["persistence"] = pass.into();
            row["reason"] = if faults > 0 { "script-or-render-fault" } else if reload_failed { "save-reload-load-failed" } else if pass { "passed" } else { "save-reload-mismatch" }.into();
        }
    }
    let rows: Vec<_> = results.into_iter().map(|(row, _)| row).collect();
    let passed = rows.iter().filter(|r| r["reason"] == "passed").count();
    let total = rows.len();
    let status = if exhausted || total == 0 { "UNKNOWN" } else if passed == total && faults == 0 { "PASS" } else { "FAIL" };
    println!("\n{}", serde_json::json!({"widget_gate_schema": 1, "program": program, "status": status, "passed": passed, "total": total, "coverage_complete": !exhausted && total > 0 && native_diagnostics.is_empty(), "faults": faults, "native_diagnostics": native_diagnostics, "captured_state_bytes": captured_state_bytes, "captured_controls": captured_controls, "reload_failure": reload_failure, "saved_parameters": saved_values.len(), "reloaded_parameters": reload_values.len(), "targets": rows}));
}

use sha2::Digest;
