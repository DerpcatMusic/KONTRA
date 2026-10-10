//! The authored legacy NativeUI graph, using the shared resource service.
use super::{
    native_runtime::{Edit, Package, Session},
    theme::*,
};
use crate::plugin::ui_activity::{Activity, Phase};
use mlua::{Function, Table, Value};
use moose::mui::mui::{geometry::Path as DrawPath, prelude::*, scene::Fit};
use sampler_ui_ir as ir;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

// The editor builder is Send; its Lua VM is created and kept on the GUI thread.
// Weak lifetime tokens let any subsequent GUI paint retire a dropped Face.
struct Local {
    #[cfg(test)]
    package: Arc<Package>,
    lifetime: std::sync::Weak<AtomicBool>,
    session: Session,
    graph: Option<Table>,
    hovered: HashSet<String>,
    drafts: HashMap<String, String>,
    retention: PrimitiveRetention,
}
thread_local! {static LOCAL:std::cell::RefCell<HashMap<u64,Local>>=std::cell::RefCell::new(HashMap::new());}
#[cfg(all(test, feature = "shots"))]
thread_local! {static GATE_PAINTED:std::cell::RefCell<HashMap<usize,u64>>=std::cell::RefCell::new(HashMap::new());}
#[cfg(all(test, feature = "shots"))]
thread_local! {static GATE_DIAGNOSTICS:std::cell::RefCell<HashMap<usize,String>>=std::cell::RefCell::new(HashMap::new());}
#[cfg(all(test, feature = "shots"))]
pub(super) fn gate_clear() {
    GATE_PAINTED.with(|state| state.borrow_mut().clear());
    GATE_DIAGNOSTICS.with(|state| state.borrow_mut().clear());
}
#[cfg(all(test, feature = "shots"))]
pub(super) fn gate_diagnostics() -> Vec<String> {
    GATE_DIAGNOSTICS.with(|state| state.borrow().values().cloned().collect())
}
#[cfg(all(test, feature = "shots"))]
pub(super) fn gate_targets(slot: usize) -> Vec<(String, &'static str)> {
    fn visit(node: &Table, slot: u64, out: &mut Vec<(String, &'static str)>) {
        let kind = string(node, "kind");
        let modifiers = tables(node, "modifiers").unwrap_or_default();
        let action = if kind == "TextInput" {
            Some("text")
        } else if modifiers
            .iter()
            .any(|m| string(m, "name") == "on_drag_gesture")
        {
            Some("drag")
        } else if modifiers
            .iter()
            .any(|m| string(m, "name") == "on_tap_gesture")
        {
            Some("click")
        } else {
            None
        };
        if let Some(action) = action {
            out.push((
                format!(
                    "nui-{slot}-{}{}",
                    string(node, "path"),
                    if action == "text" { "-text" } else { "" }
                ),
                action,
            ));
        }
        for child in tables(node, "children").unwrap_or_default() {
            visit(&child, slot, out);
        }
        for modifier in modifiers {
            if let Ok(child) = modifier.get::<Table>("value") {
                match string(&modifier, "name").as_str() {
                    "background" | "overlay" => visit(&child, slot, out),
                    "popover" => {
                        if let Ok(content) = child.get::<Table>("content") {
                            visit(&content, slot, out);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    LOCAL.with(|local| {
        let mut out = Vec::new();
        let id = GATE_PAINTED.with(|painted| painted.borrow().get(&slot).copied());
        if let Some(id) = id {
            if let Some(graph) = local
                .borrow()
                .get(&id)
                .and_then(|local| local.graph.clone())
            {
                visit(&graph, id, &mut out);
            }
        }
        out
    })
}
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
#[cfg(test)]
pub(super) fn audit_memory(collect: bool) -> usize {
    LOCAL.with(|local| {
        local
            .borrow()
            .values()
            .map(|local| {
                if collect {
                    local.session.lua().gc_collect().unwrap();
                }
                local.session.lua().used_memory()
            })
            .sum()
    })
}
#[cfg(test)]
pub(super) fn audit_assets() -> (usize, usize, usize) {
    LOCAL.with(|local| {
        local
            .borrow()
            .values()
            .fold((0, 0, 0), |(encoded, decoded, pending), local| {
                (
                    encoded + local.package.audit_bytes(),
                    decoded + local.package.images.bytes(),
                    pending + local.package.images.pending(),
                )
            })
    })
}
pub(super) struct State {
    id: u64,
    activity: Option<Arc<Activity>>,
    loading: std::sync::mpsc::Receiver<anyhow::Result<Arc<Package>>>,
    canceled: Arc<AtomicBool>,
    package: Option<Arc<Package>>,
    failed: bool,
    failure: Option<String>,
    started: bool,
    entry: String,
    seed: Vec<(ir::Source, usize, ir::Widget)>,
    size: Size,
    #[cfg(feature = "shots")]
    graph_depth: Option<usize>,
    #[cfg(feature = "shots")]
    graph_work: Option<(usize, usize)>,
}
fn failure(error: &anyhow::Error, phase: &str) -> String {
    fn category(error: &mlua::Error) -> String {
        match error {
            mlua::Error::CallbackError { cause, .. } | mlua::Error::BadArgument { cause, .. } => {
                category(cause)
            }
            mlua::Error::FromLuaConversionError { from, to, .. } => {
                let allowed = [
                    "nil", "boolean", "integer", "number", "string", "table", "function",
                    "userdata", "String", "Function", "Table", "Value",
                ];
                format!(
                    "NativeUI conversion {} to {}",
                    if allowed.contains(from) {
                        from
                    } else {
                        "value"
                    },
                    if allowed.contains(&to.as_str()) {
                        to
                    } else {
                        "value"
                    }
                )
            }
            mlua::Error::SyntaxError { .. } => "NativeUI Lua syntax".into(),
            mlua::Error::ExternalError(error) => {
                let message = error.to_string();
                if let Some(site) =
                    message.strip_prefix("NativeUI interrupt budget exceeded; site ")
                    && site.bytes().all(|c| c.is_ascii_hexdigit() || c == b':')
                {
                    return message;
                }
                [
                    "NativeUI module absent",
                    "NativeUI meter unavailable",
                    "NativeUI control unavailable",
                    "NativeUI syntax translation",
                    "Invalid NativeUI module path",
                    "NativeUI interrupt budget exceeded",
                    "NativeUI time budget exceeded",
                    "NativeUI value type unsupported",
                    "invalid utf-8 sequence",
                ]
                .into_iter()
                .find(|class| message.contains(class))
                .unwrap_or("NativeUI host operation")
                .into()
            }
            mlua::Error::RuntimeError(_) => "NativeUI Lua runtime".into(),
            _ => "NativeUI Lua operation".into(),
        }
    }
    let category = error
        .downcast_ref::<mlua::Error>()
        .map(category)
        .unwrap_or_else(|| {
            if error.to_string() == "NativeUI supplied font unavailable" {
                "NativeUI font service unavailable".into()
            } else {
                "NativeUI package".into()
            }
        });
    format!("{phase}, {category}: {error}")
}
impl Drop for State {
    fn drop(&mut self) {
        self.canceled.store(true, Ordering::Release);
        LOCAL.with(|local| {
            if let Ok(mut local) = local.try_borrow_mut() {
                local.remove(&self.id);
            }
        });
    }
}
impl State {
    pub fn new(path: &Path, entry: &str, controls: Vec<(ir::Source, usize, ir::Widget)>) -> Self {
        let path = path.to_path_buf();
        let (done, loading) = std::sync::mpsc::sync_channel(1);
        let canceled = Arc::new(AtomicBool::new(false));
        let cancel = canceled.clone();
        let _ = std::thread::Builder::new()
            .name("native-resources".into())
            .spawn(move || {
                let result =
                    Package::load_cancel(&path, || cancel.load(Ordering::Acquire)).map(Arc::new);
                if !cancel.load(Ordering::Acquire) {
                    let _ = done.send(result);
                    super::picture_worker::completed();
                }
            });
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            activity: None,
            loading,
            canceled,
            package: None,
            failed: false,
            failure: None,
            started: false,
            entry: entry.into(),
            seed: controls,
            size: Size::new(970., 600.),
            #[cfg(feature = "shots")]
            graph_depth: None,
            #[cfg(feature = "shots")]
            graph_work: None,
        }
    }
    pub fn bytes(&self) -> usize {
        self.package.as_ref().map_or(0, |p| p.images.bytes())
    }
    pub fn pending(&self) -> usize {
        if !self.started && !self.failed {
            1
        } else {
            self.package.as_ref().map_or(0, |p| p.images.pending())
        }
    }
    pub fn edits(&self) -> Vec<Edit> {
        LOCAL.with(|local| {
            local
                .borrow()
                .get(&self.id)
                .map_or(Vec::new(), |l| l.session.take_edits())
        })
    }
    /// Update an already-published source; no private script model or transport.
    pub fn update_view(
        &mut self,
        face: &ir::Interface,
        values: &super::ir_view::Values,
        typed: &HashMap<ir::WidgetRef, ir::Value>,
        meters: &HashMap<ir::WidgetRef, f64>,
    ) {
        for (source, index, widget) in &mut self.seed {
            if *source == face.source
                && let Some(current) = face.widgets.get(*index)
            {
                super::native_runtime::update_widget(
                    widget,
                    Some(current),
                    typed.get(&ir::WidgetRef(*index)),
                );
            }
        }
        LOCAL.with(|local| {
            if let Some(local) = local.borrow().get(&self.id) {
                local.session.update_view(face, values, typed, meters);
            }
        });
    }
    pub fn authored(&self) -> Size {
        self.size
    }
    #[cfg(feature = "shots")]
    pub fn scan(&self) -> super::pictures::Scan {
        self.package
            .as_ref()
            .map_or(Default::default(), |p| p.images.scan())
    }
    #[cfg(feature = "shots")]
    pub fn font_success(&self) -> Option<usize> {
        self.package.as_ref().map(|p| p.fonts.len())
    }
    #[cfg(feature = "shots")]
    pub fn graph_depth(&self) -> Option<usize> {
        self.graph_depth
    }
    #[cfg(feature = "shots")]
    pub fn graph_work(&self) -> Option<(usize, usize)> {
        self.graph_work
    }
    #[cfg(feature = "shots")]
    pub fn failures(&self) -> Vec<String> {
        self.package
            .as_ref()
            .map_or(Vec::new(), |p| p.images.failures())
    }
    #[cfg(feature = "shots")]
    pub fn diagnostic(&self) -> Option<String> {
        self.failure.as_ref().map(|raw| {
            format!(
                "{}; {}",
                raw.split_once(": ")
                    .map_or("NativeUI failure", |(category, _)| category),
                crate::scan_metrics::message(raw)
            )
        })
    }
    pub fn set_activity(&mut self, activity: &Arc<Activity>) {
        self.activity = activity.enabled.then(|| activity.clone());
    }
    pub fn entry(&self) -> &str {
        &self.entry
    }
    pub fn view(
        &mut self,
        ui: &mut Ui,
        _slot: usize,
        scale: f64,
        face: &ir::Interface,
        values: &super::ir_view::Values,
        input: &super::ir_view::InputState,
    ) -> El {
        let slot = self.id as usize;
        if let Ok(result) = self.loading.try_recv() {
            match result {
                Ok(package) => self.package = Some(package),
                Err(error) => {
                    self.failed = true;
                    self.failure = Some(failure(&error, "NativeUI resource preparation"));
                }
            }
        }
        #[cfg(all(test, feature = "shots"))]
        if let Some(diagnostic) = self.diagnostic() {
            GATE_DIAGNOSTICS.with(|state| {
                state.borrow_mut().insert(_slot, diagnostic);
            });
        }
        if self.failed {
            return caption("The authored native interface could not start.")
                .lines(2)
                .pad(12.);
        }
        let Some(package) = &self.package else {
            return caption("Loading the authored interface…").pad(12.);
        };
        let result = LOCAL.with(|local| -> anyhow::Result<El> {
            let mut local = local.borrow_mut();
            local.retain(|_, l| {
                l.lifetime
                    .upgrade()
                    .is_some_and(|live| !live.load(Ordering::Acquire))
            });
            if !local.contains_key(&self.id) {
                let _init = self.activity.as_ref().map(|a| a.span(Phase::UiNativeScript));
                let session =
                    Session::new(package.clone(), &self.entry, std::mem::take(&mut self.seed))?;
                local.insert(
                    self.id,
                    Local {
                        #[cfg(test)]
                        package: package.clone(),
                        lifetime: Arc::downgrade(&self.canceled),
                        session,
                        graph: None,
                        hovered: HashSet::new(),
                        drafts: HashMap::new(),
                        retention: PrimitiveRetention::default(),
                    },
                );
            }
            self.started = true;
            let local = local.get_mut(&self.id).unwrap();
            local.session.set_activity(self.activity.clone());
            let session = &local.session;
            {
                let _sync = self.activity.as_ref().map(|a| a.span(Phase::UiProjectionSync));
                session.update_view(face, values, &input.values, &input.meters);
            }
            if let Some(graph) = &local.graph {
                events(ui, graph, session, slot, scale, &mut local.hovered)?;
            }
            let graph = session.render();
            #[cfg(feature = "shots")]
            {
                self.graph_work = Some(session.graph_work());
            }
            let graph = graph?;
            #[cfg(feature = "shots")]
            {
                self.graph_depth = Some(self.graph_depth.unwrap_or(0).max(graph_depth(&graph)?));
            }
            let _submission = self.activity.as_ref().map(|a| a.span(Phase::UiAuthoredLayoutSubmission));
            let authored = authored_size(&graph);
            self.size = authored;
            let el = draw_retained(
                ui,
                &graph,
                package,
                session,
                slot,
                scale,
                authored,
                &mut local.drafts,
                &mut local.retention,
            )?;
            local.graph = Some(graph);
            #[cfg(all(test, feature = "shots"))]
            GATE_PAINTED.with(|painted| {
                painted.borrow_mut().insert(_slot, self.id);
            });
            Ok(el)
        });
        match result {
            Ok(el) => el,
            Err(error) => {
                self.failed = true;
                self.failure = Some(failure(
                    &error,
                    if self.started {
                        "NativeUI graph"
                    } else {
                        "NativeUI module initialization"
                    },
                ));
                #[cfg(all(test, feature = "shots"))]
                if let Some(diagnostic) = self.diagnostic() {
                    GATE_DIAGNOSTICS.with(|state| {
                        state.borrow_mut().insert(_slot, diagnostic);
                    });
                }
                caption("The authored native interface could not render.")
                    .lines(2)
                    .pad(12.)
            }
        }
    }
}
fn tables(t: &Table, key: &str) -> mlua::Result<Vec<Table>> {
    t.get::<Table>(key)?.sequence_values::<Table>().collect()
}
#[cfg(feature = "shots")]
fn graph_depth(root: &Table) -> anyhow::Result<usize> {
    let mut work = vec![(root.clone(), 0)];
    let (mut nodes, mut maximum) = (0, 0);
    while let Some((node, depth)) = work.pop() {
        nodes += 1;
        anyhow::ensure!(
            nodes <= 16384 && depth <= 192,
            "NativeUI graph budget exceeded"
        );
        maximum = maximum.max(depth);
        work.extend(
            tables(&node, "children")?
                .into_iter()
                .map(|child| (child, depth + 1)),
        );
        for modifier in tables(&node, "modifiers")? {
            if let Value::Table(value) = val(&modifier, "value") {
                match string(&modifier, "name").as_str() {
                    "background" | "overlay" => work.push((value, depth + 1)),
                    "popover" => work.push((value.get("content")?, depth + 1)),
                    _ => {}
                }
            }
        }
    }
    Ok(maximum)
}
fn number(t: &Table, key: &str) -> Option<f64> {
    match t.get::<Value>(key).ok()? {
        Value::Integer(n) => Some(n as f64),
        Value::Number(n) => Some(n),
        _ => None,
    }
}
fn string(t: &Table, key: &str) -> String {
    t.get::<String>(key).unwrap_or_default()
}
fn boolean(v: &Value) -> bool {
    !matches!(v, Value::Nil | Value::Boolean(false))
}
fn color(v: &Value) -> Option<Color> {
    let Value::Table(t) = v else { return None };
    Some(
        Color::srgb(
            number(t, "r")? as f32,
            number(t, "g")? as f32,
            number(t, "b")? as f32,
        )
        .with_alpha(number(t, "a").unwrap_or(1.) as f32),
    )
}
fn val(t: &Table, key: &str) -> Value {
    t.get(key).unwrap_or(Value::Nil)
}
fn authored_size(graph: &Table) -> Size {
    let mut size = Size::new(970., 600.);
    if let Ok(modifiers) = tables(graph, "modifiers") {
        for m in modifiers {
            if string(&m, "name") == "frame"
                && let Ok(t) = m.get::<Table>("value")
            {
                if let Some(w) = number(&t, "width") {
                    size.width = w;
                }
                if let Some(h) = number(&t, "height") {
                    size.height = h;
                }
            }
        }
    }
    size
}
#[derive(Clone)]
struct Style {
    ink: Color,
    size: f64,
    bold: bool,
    disabled: bool,
    hidden: bool,
    font: Option<Font>,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            ink: Color::srgb(1., 1., 1.),
            size: 12.,
            bold: false,
            disabled: false,
            hidden: false,
            font: None,
        }
    }
}
fn align(value: &str) -> (Align, Align) {
    (
        if value.contains("left") {
            Align::Start
        } else if value.contains("right") {
            Align::End
        } else {
            Align::Center
        },
        if value.contains("top") {
            Align::Start
        } else if value.contains("bottom") {
            Align::End
        } else {
            Align::Center
        },
    )
}
// Flexible primitives consume the proposed frame; text and fixed frames hug
// their intrinsic size. Keep this across modifier wrappers rather than making
// every component expand when an outer frame is applied.
fn flexibility(node: &Table) -> (bool, bool) {
    // Each render makes new resolved nodes; reuse derived flags within that graph.
    if let Ok(flags) = node.get::<u8>("__host_flex") {
        return (flags & 1 != 0, flags & 2 != 0);
    }
    let kind = string(node, "kind");
    let mut flex = intrinsic_flexibility(node, &kind);
    for m in tables(node, "modifiers").unwrap_or_default() {
        if string(&m, "name") == "frame" {
            if let Ok(t) = m.get::<Table>("value") {
                if number(&t, "width").is_some() {
                    flex.0 = false;
                } else if number(&t, "max_width").is_some_and(f64::is_finite) {
                    flex.0 = false;
                } else if number(&t, "max_width").is_some_and(|n| n.is_infinite()) {
                    flex.0 = true;
                }
                if number(&t, "height").is_some() {
                    flex.1 = false;
                } else if number(&t, "max_height").is_some_and(f64::is_finite) {
                    flex.1 = false;
                } else if number(&t, "max_height").is_some_and(|n| n.is_infinite()) {
                    flex.1 = true;
                }
            }
        }
    }
    let _ = node.set("__host_flex", u8::from(flex.0) | (u8::from(flex.1) << 1));
    flex
}
fn intrinsic_flexibility(node: &Table, kind: &str) -> (bool, bool) {
    let children = tables(node, "children").unwrap_or_default();
    match kind {
        "ZStack" | "Group" | "HStack" | "VStack" => {
            let mut f = (false, false);
            for child in children {
                let c = flexibility(&child);
                if string(&child, "kind") == "Spacer" {
                    let frames = tables(&child, "modifiers").unwrap_or_default();
                    let fixed = |axis| {
                        frames.iter().any(|m| {
                            string(m, "name") == "frame"
                                && m.get::<Table>("value")
                                    .ok()
                                    .is_some_and(|t| number(&t, axis).is_some())
                        })
                    };
                    f.0 |= kind == "HStack" && !fixed("width");
                    f.1 |= kind == "VStack" && !fixed("height");
                }
                f.0 |= c.0;
                f.1 |= c.1;
                if kind == "ZStack" {
                    for m in tables(&child, "modifiers").unwrap_or_default() {
                        if string(&m, "name") == "align" {
                            let a = string(&m, "value");
                            f.0 |= a.contains("left") || a.contains("right");
                            f.1 |= a.contains("top") || a.contains("bottom");
                        }
                    }
                }
            }
            f
        }
        _ => (false, false),
    }
}
fn expand(mut el: El, flex: (bool, bool)) -> El {
    if flex.0 {
        el = el.w(Len::Pct(100.));
    }
    if flex.1 {
        el = el.h(Len::Pct(100.));
    }
    el
}
// Retain presentation only: render factories, effects and event callbacks have
// already run. Keys own scalar data, never Lua tables/functions or Els.
#[derive(Clone, PartialEq, Eq)]
enum PrimitiveAtom {
    Nil,
    Bool(bool),
    Integer(mlua::Integer),
    Number(u64),
    String(Vec<u8>),
    Count(usize),
}
fn primitive_atom(value: Value) -> Option<PrimitiveAtom> {
    Some(match value {
        Value::Nil => PrimitiveAtom::Nil,
        Value::Boolean(v) => PrimitiveAtom::Bool(v),
        Value::Integer(v) => PrimitiveAtom::Integer(v),
        Value::Number(v) => PrimitiveAtom::Number(v.to_bits()),
        Value::String(v) if v.as_bytes().len() <= 64 << 10 => PrimitiveAtom::String(v.as_bytes().to_vec()),
        _ => return None,
    })
}
fn primitive_bytes(atoms: &[PrimitiveAtom]) -> usize {
    atoms.iter().map(|atom| match atom {
        PrimitiveAtom::String(s) => s.len() + 32,
        _ => 32,
    }).sum()
}
fn primitive_fields(t: &Table, fields: &[&str], out: &mut Vec<PrimitiveAtom>) -> Option<()> {
    // No __index/length/pairs callbacks during analysis. The built-in Color
    // metatable is harmless only if every consumed field is present raw.
    if t.metatable().is_some()
        && fields.iter().any(|key| matches!(t.raw_get::<Value>(*key), Ok(Value::Nil) | Err(_)))
    {
        return None;
    }
    for pair in t.clone().pairs::<Value, Value>() {
        let (Value::String(key), _) = pair.ok()? else { return None };
        if !fields.iter().any(|field| key.as_bytes().as_ref() == field.as_bytes()) {
            return None;
        }
    }
    for field in fields {
        out.push(primitive_atom(t.raw_get(*field).ok()?)?);
    }
    Some(())
}
fn primitive_lookups(t: &Table, fields: &[&str]) -> bool {
    t.metatable().is_none() || fields.iter().all(|key| {
        t.raw_get::<Value>(*key).is_ok_and(|value| !matches!(value, Value::Nil))
    })
}
fn primitive_sequence(t: &Table) -> Option<Vec<Table>> {
    if t.metatable().is_some() || t.raw_len() > 16384 {
        return None;
    }
    let len = t.raw_len();
    let mut seen = 0;
    for pair in t.clone().pairs::<Value, Value>() {
        let (Value::Integer(key), Value::Table(_)) = pair.ok()? else { return None };
        if key < 1 || key as usize > len { return None; }
        seen += 1;
    }
    if seen != len { return None; }
    (1..=len).map(|i| t.raw_get(i).ok()).collect()
}
fn primitive_props(kind: &str, props: &Table, out: &mut Vec<PrimitiveAtom>) -> Option<()> {
    if props.metatable().is_some() { return None; }
    let fields: &[&str] = match kind {
        "Group" | "ZStack" | "HStack" | "VStack" => &["alignment", "spacing"],
        "Text" => &["text"],
        "Spacer" => &["min_length"],
        "Rectangle" => &["color", "corner_radius"],
        "Image" => &["file", "color", "resizable"],
        "Ring" => &["color", "start_angle", "angle", "thickness"],
        // TextInput, Canvas, menus and arbitrary callbacks never qualify.
        _ => return None,
    };
    for pair in props.clone().pairs::<Value, Value>() {
        let (Value::String(key), _) = pair.ok()? else { return None };
        if !fields.iter().any(|field| key.as_bytes().as_ref() == field.as_bytes()) {
            return None;
        }
    }
    for field in fields {
        let value: Value = props.raw_get(*field).ok()?;
        if *field == "color" && let Value::Table(t) = value {
            out.push(PrimitiveAtom::Count(4));
            primitive_fields(&t, &["r", "g", "b", "a"], out)?;
        } else {
            out.push(primitive_atom(value)?);
        }
    }
    Some(())
}
fn primitive_modifier(m: &Table, out: &mut Vec<PrimitiveAtom>) -> Option<()> {
    if m.metatable().is_some() { return None; }
    let name: String = m.raw_get("name").ok()?;
    if name.len() > 64 << 10 { return None; }
    // Fixed field order is canonical; unknown fields are deliberately eager.
    for pair in m.clone().pairs::<Value, Value>() {
        let (Value::String(key), _) = pair.ok()? else { return None };
        if !["name", "value", "alignment"].iter().any(|field| key.as_bytes().as_ref() == field.as_bytes()) {
            return None;
        }
    }
    out.push(PrimitiveAtom::String(name.as_bytes().to_vec()));
    out.push(primitive_atom(m.raw_get("alignment").ok()?)?);
    let value: Value = m.raw_get("value").ok()?;
    let fields: &[&str] = match name.as_str() {
        "frame" => &["width", "height", "min_width", "min_height", "max_width", "max_height", "alignment"],
        "position" | "offset" => &["x", "y"],
        "padding" if matches!(value, Value::Table(_)) => &["all", "horizontal", "vertical", "top", "right", "bottom", "left"],
        "foreground_color" if matches!(value, Value::Table(_)) => &["r", "g", "b", "a"],
        "background" | "overlay" => {
            if !matches!(value, Value::Table(_)) { return None; }
            out.push(PrimitiveAtom::Nil); // child data is in the preorder range
            return Some(());
        }
        "align" | "padding" | "hidden" | "opacity" | "font_size" | "font" | "font_family"
        | "bold" | "disabled" | "rotation" | "line_limit" | "foreground_color" => {
            out.push(primitive_atom(value)?);
            return Some(());
        }
        _ => return None,
    };
    let Value::Table(t) = value else { return None };
    out.push(PrimitiveAtom::Count(fields.len()));
    primitive_fields(&t, fields, out)
}
struct PrimitiveNode {
    table: Table,
    path: String,
    atoms: Option<Vec<PrimitiveAtom>>,
    end: usize,
    pure: bool,
    images: bool,
}
struct PrimitivePlan {
    nodes: Vec<PrimitiveNode>,
    indices: HashMap<usize, usize>,
}
impl PrimitivePlan {
    #[cfg(test)]
    fn analyze(root: &Table) -> anyhow::Result<Option<Self>> {
        Self::analyze_for(root, |_| true)
    }
    fn analyze_for(root: &Table, active_edit: impl Fn(&str) -> bool) -> anyhow::Result<Option<Self>> {
        enum Work { Enter(Table, usize), Exit(usize) }
        let mut plan = Self { nodes: Vec::new(), indices: HashMap::new() };
        let mut work = vec![Work::Enter(root.clone(), 0)];
        let mut paths = HashSet::new();
        let mut atom_bytes = 0;
        let mut scheduled = 1;
        while let Some(job) = work.pop() {
            match job {
                Work::Exit(index) => {
                    let end = plan.nodes.len();
                    // Only direct children are inspected. Each edge is visited
                    // once, not every ancestor's full descendant range.
                    let mut pure = plan.nodes[index].atoms.is_some();
                    let mut images = plan.nodes[index].images;
                    let mut child = index + 1;
                    while child < end {
                        pure &= plan.nodes[child].pure;
                        images |= plan.nodes[child].images;
                        child = plan.nodes[child].end;
                    }
                    plan.nodes[index].end = end;
                    plan.nodes[index].pure = pure;
                    plan.nodes[index].images = images;
                }
                Work::Enter(node, depth) => {
                    if plan.nodes.len() >= 16384 || depth > 192 {
                        return Ok(None); // old traversal reports the original budget error
                    }
                    // An opaque structure must use the old traversal, including
                    // its validation and any observable metatable lookups.
                    if node.metatable().is_some() { return Ok(None); }
                    let Ok(kind) = node.raw_get::<String>("kind") else { return Ok(None) };
                    let Ok(path) = node.raw_get::<String>("path") else { return Ok(None) };
                    if kind.len() > 64 << 10 || path.len() > 64 << 10 { return Ok(None); }
                    if !paths.insert(path.clone()) || plan.indices.contains_key(&(node.to_pointer() as usize)) {
                        return Ok(None); // no duplicate memo identities or cycles
                    }
                    let Ok(props) = node.raw_get::<Table>("props") else { return Ok(None) };
                    // A focused TextInput callback may mutate a later sibling
                    // during lowering. MUI text_edit cannot change or submit
                    // an unfocused field; that field still lowers eagerly and
                    // keeps its fresh callbacks, while pure siblings can memo.
                    // Draw-time metatable lookups can also mutate later data.
                    if props.metatable().is_some() { return Ok(None); }
                    if kind == "TextInput" && active_edit(&path)
                        && ["on_change", "on_accept"].iter().any(|key| {
                            matches!(props.raw_get::<Value>(*key), Ok(Value::Function(_)))
                        })
                    { return Ok(None); }
                    if let Value::Table(color) = props.raw_get::<Value>("color")?
                        && !primitive_lookups(&color, &["r", "g", "b", "a"])
                    { return Ok(None); }
                    let Ok(children) = node.raw_get::<Table>("children") else { return Ok(None) };
                    let Some(mut children) = primitive_sequence(&children) else { return Ok(None) };
                    let child_count = children.len();
                    let Ok(modifiers) = node.raw_get::<Table>("modifiers") else { return Ok(None) };
                    let Some(modifiers) = primitive_sequence(&modifiers) else { return Ok(None) };
                    let mut atoms = vec![PrimitiveAtom::String(kind.as_bytes().to_vec()), PrimitiveAtom::String(path.as_bytes().to_vec()), PrimitiveAtom::Count(child_count), PrimitiveAtom::Count(modifiers.len())];
                    let mut pure = primitive_props(&kind, &props, &mut atoms).is_some();
                    for pair in node.clone().pairs::<Value, Value>() {
                        let (key, _) = pair?;
                        pure &= matches!(key, Value::String(ref key) if ["kind", "path", "props", "children", "modifiers", "__host_flex"].iter().any(|field| key.as_bytes().as_ref() == field.as_bytes()));
                    }
                    pure &= primitive_atom(node.raw_get("__host_flex")?).is_some();
                    // Include the two derived flex atoms added below. Account
                    // incrementally, so repeated shared strings cannot create
                    // an unbounded temporary key before the limit is checked.
                    atom_bytes += primitive_bytes(&atoms) + 64;
                    if atom_bytes > 8 << 20 { return Ok(None); }
                    for m in modifiers {
                        // Structure reads are raw even for ineligible modifiers.
                        if m.metatable().is_some() { return Ok(None); }
                        let name = m.raw_get::<String>("name").unwrap_or_default();
                        if name.len() > 64 << 10 { return Ok(None); }
                        let from = atoms.len();
                        pure &= primitive_modifier(&m, &mut atoms).is_some();
                        atom_bytes += primitive_bytes(&atoms[from..]);
                        if atom_bytes > 8 << 20 { return Ok(None); }
                        let value: Value = m.raw_get("value")?;
                        if let Value::Table(t) = &value {
                            let fields: &[&str] = match name.as_str() {
                                "frame" => &["width", "height", "max_width", "max_height", "min_width", "min_height", "alignment"],
                                "position" | "offset" => &["x", "y"],
                                "padding" => &["all", "horizontal", "vertical", "top", "right", "bottom", "left"],
                                "foreground_color" => &["r", "g", "b", "a"],
                                _ => &[],
                            };
                            if !primitive_lookups(t, fields) { return Ok(None); }
                        }
                        if let Value::Table(t) = value {
                            match name.as_str() {
                                "background" | "overlay" => children.push(t),
                                "popover" => {
                                    if t.metatable().is_some() { return Ok(None); }
                                    let Ok(content) = t.raw_get::<Table>("content") else { return Ok(None) };
                                    children.push(content);
                                }
                                _ => {}
                            }
                        }
                    }
                    scheduled += children.len();
                    if scheduled > 16384 { return Ok(None); }
                    let index = plan.nodes.len();
                    plan.indices.insert(node.to_pointer() as usize, index);
                    plan.nodes.push(PrimitiveNode { table: node, path, atoms: pure.then_some(atoms), end: 0, pure: false, images: kind == "Image" });
                    work.push(Work::Exit(index));
                    work.extend(children.into_iter().rev().map(|child| Work::Enter(child, depth + 1)));
                }
            }
        }
        // Precompute flex bottom-up, so even a depth-192 graph never starts a
        // recursive descendant walk here. All flex lookup tables are raw/plain.
        for node in plan.nodes.iter_mut().rev() {
            let flex = flexibility(&node.table);
            if let Some(atoms) = &mut node.atoms {
                atoms.extend([PrimitiveAtom::Bool(flex.0), PrimitiveAtom::Bool(flex.1)]);
            }
        }
        Ok(Some(plan))
    }
    fn candidate(&self, node: &Table) -> Option<(String, Vec<PrimitiveAtom>, bool)> {
        let index = *self.indices.get(&(node.to_pointer() as usize))?;
        let candidate = &self.nodes[index];
        if !candidate.pure { return None; }
        let mut atoms = Vec::new();
        for node in &self.nodes[index..candidate.end] {
            let local = node.atoms.as_ref()?;
            atoms.push(PrimitiveAtom::Count(local.len()));
            atoms.extend_from_slice(local);
        }
        Some((candidate.path.clone(), atoms, candidate.images))
    }
}
// Parent layout changes the child *after* primitive lowering. Move that
// decoration inside the memo closure; changing a placeholder cannot restyle
// MUI's retained tree. Layer/popup roots stay eager when their parent is eager.
#[derive(Clone, Default, PartialEq)]
enum PrimitivePlacement {
    #[default]
    None,
    Root(Size),
    Child { kind: String, alignment: String, has_flexible_content: bool, index: usize, spacing: f64 },
}
impl PrimitivePlacement {
    fn apply(&self, child: &Table, mut el: El, s: f64) -> anyhow::Result<El> {
        match self {
            Self::None => {}
            Self::Root(size) => {
                el = el.w(size.width * s).h(size.height * s).clip().named("NativeUI performance view");
            }
            Self::Child { kind, alignment, has_flexible_content, index, spacing } => {
                let flexible = flexibility(child);
                if *has_flexible_content && string(child, "kind") == "Spacer" { el = el.grow(0.); }
                if kind == "HStack" && flexible.0 { el = el.w(Len::Auto).grow(1.); }
                else if kind == "VStack" && flexible.1 { el = el.h(Len::Auto).grow(1.); }
                let explicit = tables(child, "modifiers")?.iter().any(|m| string(m, "name") == "align");
                if kind == "ZStack" && !explicit {
                    let (x, y) = align(alignment);
                    el = el.anchor(x, y);
                }
                if *spacing < 0. {
                    let spacing = *spacing * s;
                    if kind == "HStack" { el = el.offset(*index as f64 * spacing, 0.); }
                    else { el = el.offset(0., *index as f64 * spacing); }
                }
            }
        }
        Ok(el)
    }
}
struct PrimitiveKey {
    atoms: Vec<PrimitiveAtom>,
    style: Style,
    scale: u64,
    assets: u64,
    placement: PrimitivePlacement,
}
impl PartialEq for PrimitiveKey {
    fn eq(&self, other: &Self) -> bool {
        self.atoms == other.atoms && self.scale == other.scale && self.assets == other.assets
            && self.placement == other.placement && self.style.ink == other.style.ink
            && self.style.size.to_bits() == other.style.size.to_bits()
            && self.style.bold == other.style.bold && self.style.disabled == other.style.disabled
            && self.style.hidden == other.style.hidden && self.style.font == other.style.font
    }
}
// ponytail: MUI capture and warm splice scan memo roots quadratically.
// Bound this native build to 32 scopes; excess subtrees lower eagerly. A single
// large pure root still retains. Raise only after indexed framework bookkeeping.
const NATIVE_MEMO_SCOPE_LIMIT: usize = 32;
#[derive(Default)]
struct PrimitiveRetention {
    keys: HashMap<String, (PrimitiveKey, u64)>,
    next: u64,
    live: HashSet<String>,
    #[cfg(test)]
    lowered: usize,
}
impl PrimitiveRetention {
    fn revision(&mut self, path: &str, key: PrimitiveKey) -> u64 {
        self.live.insert(path.into());
        if let Some((previous, revision)) = self.keys.get(path) && *previous == key { return *revision; }
        // Compare exact canonical values before issuing MUI's hashed revision.
        self.next = self.next.checked_add(1).expect("NativeUI presentation revision exhausted");
        self.keys.insert(path.into(), (key, self.next));
        self.next
    }
}
fn draw_retained(
    ui: &mut Ui, node: &Table, package: &Arc<Package>, session: &Session,
    slot: usize, s: f64, authored: Size,
    drafts: &mut HashMap<String, String>, retention: &mut PrimitiveRetention,
) -> anyhow::Result<El> {
    let plan = PrimitivePlan::analyze_for(node, |path| {
        ui.focused(format!("nui-{slot}-{path}-text"))
    })?;
    retention.live.clear();
    // Reuse the caller\'s size: authored frame lookups can execute __index.
    let result = draw_impl(ui, node, package, session, slot, s, Style::default(), drafts,
        plan.as_ref(), Some(retention), PrimitivePlacement::Root(authored), true);
    retention.keys.retain(|path, _| retention.live.contains(path));
    result
}

// Native graphs are bounded by the VM, but each unoptimized primitive frame
// is large. Keep all parent continuations and completed children on the heap.
struct DrawNode {
    node: Table,
    props: Table,
    modifiers: Vec<Table>,
    style: Style,
    child_nodes: Vec<Table>,
}
struct DrawFrame {
    node: Table,
    props: Table,
    style: Style,
    modifiers: std::collections::VecDeque<Table>,
    el: El,
    flex: (bool, bool),
    fixed: (bool, bool),
}
enum DrawProgress {
    Complete(El),
    Child(Box<DrawFrame>, Table),
}
fn draw(
    ui: &mut Ui,
    node: &Table,
    package: &Arc<Package>,
    session: &Session,
    slot: usize,
    s: f64,
    style: Style,
    drafts: &mut HashMap<String, String>,
) -> anyhow::Result<El> {
    draw_impl(ui, node, package, session, slot, s, style, drafts, None, None, PrimitivePlacement::None, false)
}
fn draw_impl(
    ui: &mut Ui,
    node: &Table,
    package: &Arc<Package>,
    session: &Session,
    slot: usize,
    s: f64,
    style: Style,
    drafts: &mut HashMap<String, String>,
    plan: Option<&PrimitivePlan>,
    mut retention: Option<&mut PrimitiveRetention>,
    placement: PrimitivePlacement,
    allow_memo: bool,
) -> anyhow::Result<El> {
    enum Work {
        Enter(Table, Style, usize, PrimitivePlacement, bool),
        Build(DrawNode, usize, PrimitivePlacement),
        Resume(Box<DrawFrame>, usize, PrimitivePlacement),
    }
    let mut work = vec![Work::Enter(node.clone(), style, 0, placement, allow_memo)];
    let mut results = Vec::new();
    let mut nodes = 0;
    while let Some(job) = work.pop() {
        let (progress, depth, placement, placed_node) = match job {
            Work::Enter(node, style, depth, placement, allow_memo) => {
                nodes += 1;
                anyhow::ensure!(
                    nodes <= 16384 && depth <= 192,
                    "NativeUI graph budget exceeded"
                );
                if allow_memo
                    && retention.as_ref().is_some_and(|r| r.live.len() < NATIVE_MEMO_SCOPE_LIMIT)
                    && let Some((path, atoms, images)) = plan.and_then(|p| p.candidate(&node))
                    && let Some(retention) = retention.as_deref_mut()
                {
                    let revision = retention.revision(&path, PrimitiveKey {
                        atoms, style: style.clone(), scale: s.to_bits(),
                        assets: if images { super::pictures::revision() } else { 0 }, placement: placement.clone(),
                    });
                    let mut error = None;
                    let el = ui.memo(format!("nui-{slot}-{path}-primitive"), revision, |ui| {
                        #[cfg(test)] { retention.lowered += 1; }
                        match draw(ui, &node, package, session, slot, s, style, drafts)
                            .and_then(|el| placement.apply(&node, el, s))
                        {
                            Ok(el) => el,
                            Err(e) => { error = Some(e); stack([]) }
                        }
                    });
                    if let Some(error) = error { return Err(error); }
                    results.push((el, placement != PrimitivePlacement::None));
                    continue;
                }
                let props: Table = node.get("props")?;
                let modifiers = tables(&node, "modifiers")?;
                let style = draw_style(package, &modifiers, style)?;
                let child_nodes = tables(&node, "children")?;
                let children = child_nodes.clone();
                // A completed plan proves lowering has no Lua side effects.
                // An opaque callback ancestor remains eager, but its resolved
                // child placement is still safe to move inside child memos.
                let movable = plan.is_some();
                let kind = if movable { string(&node, "kind") } else { String::new() };
                let alignment = if movable { string(&props, "alignment") } else { String::new() };
                let spacing = if movable { number(&props, "spacing").unwrap_or(0.) } else { 0. };
                let has_flexible_content = movable && children.iter().any(|child| {
                    let flex = flexibility(child);
                    string(child, "kind") != "Spacer" && ((kind == "HStack" && flex.0) || (kind == "VStack" && flex.1))
                });
                work.push(Work::Build(
                    DrawNode {
                        node,
                        props,
                        modifiers,
                        style: style.clone(),
                        child_nodes,
                    },
                    depth,
                    placement,
                ));
                work.extend(children.into_iter().enumerate().rev().map(|(index, child)| {
                    let placement = if movable { PrimitivePlacement::Child {
                        kind: kind.clone(), alignment: alignment.clone(), has_flexible_content, index, spacing,
                    } } else { PrimitivePlacement::None };
                    Work::Enter(child, style.clone(), depth + 1, placement, movable)
                }));
                continue;
            }
            Work::Build(input, depth, placement) => {
                let children = results.split_off(results.len() - input.child_nodes.len());
                let placed_node = input.node.clone();
                let frame = draw_base(ui, package, session, slot, s, drafts, input, children)?;
                (draw_modifiers(ui, slot, s, frame, None)?, depth, placement, placed_node)
            }
            Work::Resume(frame, depth, placement) => {
                let placed_node = frame.node.clone();
                (draw_modifiers(ui, slot, s, *frame, results.pop().map(|(el, _)| el))?, depth, placement, placed_node)
            }
        };
        match progress {
            DrawProgress::Complete(el) => results.push((placement.apply(&placed_node, el, s)?, placement != PrimitivePlacement::None)),
            DrawProgress::Child(frame, child) => {
                let style = frame.style.clone();
                work.push(Work::Resume(frame, depth, placement));
                // The eager modifier continuation decorates this root. Do not
                // memoize that root, but its normally-placed descendants may be.
                work.push(Work::Enter(child, style, depth + 1, PrimitivePlacement::None, false));
            }
        }
    }
    Ok(results.pop().expect("the root produces one element").0)
}
fn draw_style(package: &Package, modifiers: &[Table], mut style: Style) -> anyhow::Result<Style> {
    let mut font_name = None;
    for m in modifiers {
        let v = val(m, "value");
        match string(m, "name").as_str() {
            "foreground_color" => {
                if let Some(c) = color(&v) {
                    style.ink = c;
                }
            }
            "font_size" => {
                if let Some(n) = number(m, "value") {
                    style.size = n;
                }
            }
            "font" | "font_family" => {
                font_name = Some(string(m, "value"));
            }
            "bold" => style.bold = boolean(&v),
            "disabled" => style.disabled |= boolean(&v),
            "hidden" => style.hidden |= boolean(&v),
            _ => {}
        }
    }
    if let Some(name) = font_name {
        style.font = Some(
            package
                .font(&name, style.bold)
                .ok_or_else(|| anyhow::anyhow!("NativeUI supplied font unavailable"))?,
        );
    }
    Ok(style)
}
fn draw_base(
    ui: &mut Ui,
    package: &Arc<Package>,
    session: &Session,
    slot: usize,
    s: f64,
    drafts: &mut HashMap<String, String>,
    input: DrawNode,
    rendered: Vec<(El, bool)>,
) -> anyhow::Result<DrawFrame> {
    let DrawNode {
        node,
        props,
        modifiers,
        style,
        child_nodes,
    } = input;
    let kind = string(&node, "kind");
    let node = &node;
    let has_flexible_content = child_nodes.iter().any(|child| {
        let flex = flexibility(child);
        string(child, "kind") != "Spacer"
            && ((kind == "HStack" && flex.0) || (kind == "VStack" && flex.1))
    });
    let children = child_nodes
        .iter()
        .zip(rendered)
        .map(|(child, (mut el, placed))| {
            if placed { return Ok((el, true)); }
            let flexible = flexibility(child);
            if has_flexible_content && string(child, "kind") == "Spacer" {
                // Explicit flexible content consumes the proposal first;
                // spacers keep their minimum instead of stealing half of it.
                el = el.grow(0.);
            }
            // A flexible sibling takes the remaining stack space, after fixed
            // siblings and gaps. 100% of the entire stack would overflow it.
            if kind == "HStack" && flexible.0 {
                el = el.w(Len::Auto).grow(1.);
            } else if kind == "VStack" && flexible.1 {
                el = el.h(Len::Auto).grow(1.);
            }
            // MUI's default stack alignment couples the axes for Start.
            // NativeUI specifies them independently, e.g. top center.
            let explicit = tables(child, "modifiers")?
                .iter()
                .any(|m| string(m, "name") == "align");
            Ok((if kind == "ZStack" && !explicit {
                let (x, y) = align(&string(&props, "alignment"));
                el.anchor(x, y)
            } else {
                el
            }, false))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let spacing = number(&props, "spacing").unwrap_or(0.) * s;
    let children = children.into_iter().enumerate().map(|(i, (child, placed))| {
        if spacing < 0. && !placed {
            if kind == "HStack" { child.offset(i as f64 * spacing, 0.) }
            else { child.offset(0., i as f64 * spacing) }
        } else { child }
    }).collect::<Vec<_>>();
    let mut el = match kind.as_str() {
        "HStack" => row(children)
            .gap(spacing.max(0.))
            .align(align(&string(&props, "alignment")).1),
        "VStack" => col(children)
            .gap(spacing.max(0.))
            .align(align(&string(&props, "alignment")).0),
        "ZStack" | "Group" => {
            let (x, y) = align(&string(&props, "alignment"));
            stack(children).align(x).justify(match y {
                Align::Start => Justify::Start,
                Align::End => Justify::End,
                _ => Justify::Center,
            })
        }
        "Spacer" => block(0, 0)
            .min_w(number(&props, "min_length").unwrap_or(0.) * s)
            .grow(1.),
        "Text" => {
            let el = text(string(&props, "text"))
                .text_size(style.size * s)
                .fill(style.ink)
                .text_weight(if style.bold {
                    mui_text::Weight::BOLD
                } else {
                    mui_text::Weight::REGULAR
                });
            if let Some(font) = &style.font {
                el.font(font.clone())
            } else {
                el
            }
        }
        "TextInput" => {
            let id = format!("nui-{slot}-{}-text", string(node, "path"));
            let value = drafts
                .entry(id.clone())
                .or_insert_with(|| string(&props, "text"));
            if !ui.focused(id.as_str()) {
                *value = string(&props, "text");
            }
            static FALLBACK: std::sync::OnceLock<Font> = std::sync::OnceLock::new();
            let font = style.font.clone().unwrap_or_else(|| {
                FALLBACK
                    .get_or_init(|| Font::new(NOTO_SANS).unwrap())
                    .clone()
            });
            let nominal = style.size * s;
            // Reuse v1's bounded caption fit when the host default face is
            // approximated. Authored fonts and focused editing keep their size.
            let size = if style.font.is_none() && !ui.focused(id.as_str()) {
                ui.scene()
                    .and_then(|scene| scene.surface(&id))
                    .map_or(nominal, |surface| {
                        let advance = mui_text::shape_run(
                            std::slice::from_ref(&font),
                            value,
                            nominal,
                            &[("wght", if style.bold { 700. } else { 400. })],
                        )
                        .map_or(0., |r| r.advance);
                        if advance > 0. && surface.frame.size.width > 0. {
                            nominal * (surface.frame.size.width / advance).clamp(0.75, 1.)
                        } else {
                            nominal
                        }
                    })
            } else {
                nominal
            };
            let field = text_edit(ui, id, value, TextOpts::default());
            if field.changed.changed
                && let Ok(f) = props.get::<Function>("on_change")
            {
                session.call(f, value.clone())?;
            }
            if field.changed.submitted
                && let Ok(f) = props.get::<Function>("on_accept")
            {
                session.call(f, value.clone())?;
            }
            let mut el = field
                .el
                .text_size(size)
                .font(font.clone())
                .text_weight(if style.bold {
                    mui_text::Weight::BOLD
                } else {
                    mui_text::Weight::REGULAR
                })
                .fill(Fill::None)
                .pad(0)
                .radius(0.);
            // The generic editor has separate paint/hit-test insets; clearing
            // layout padding alone still clips authored fields by 16 pixels.
            if let Some(edit) = &mut el.payload_mut().extras_mut().editable_text {
                edit.insets = [0., 0.];
            }
            let height = mui_text::shape_run(
                std::slice::from_ref(&font),
                "M",
                size,
                &[("wght", if style.bold { 700. } else { 400. })],
            )
            .map_or(size * 1.25, |run| run.line_height);
            el.h(height)
        }
        "Rectangle" => block(Len::Auto, Len::Auto)
            .fill(color(&val(&props, "color")).unwrap_or(style.ink))
            .radius(number(&props, "corner_radius").unwrap_or(0.) * s),
        "Image" => {
            let file = string(&props, "file").replace('\\', "/").to_lowercase();
            let image = package.images.get(&file);
            let width = image.as_ref().map_or(1, |i| i.width);
            let height = image.as_ref().map_or(1, |i| i.height);
            let resizable = boolean(&val(&props, "resizable"));
            let el = block(
                if resizable {
                    Len::Auto
                } else {
                    Len::Px(f64::from(width) * s)
                },
                if resizable {
                    Len::Auto
                } else {
                    Len::Px(f64::from(height) * s)
                },
            )
            .fill(image.map_or(Fill::None, |i| Fill::Image(i, Fit::Fill)));
            if let Some(ink) = color(&val(&props, "color")) {
                el.mask(ink)
            } else {
                el
            }
        }
        "Ring" => {
            let from = number(&props, "start_angle").unwrap_or(0.).to_radians();
            let angle = number(&props, "angle").unwrap_or(360.).to_radians();
            let thickness = number(&props, "thickness").unwrap_or(1.) * s;
            let ink = color(&val(&props, "color")).unwrap_or(style.ink);
            canvas(move |size| {
                let radius = (size.width.min(size.height) - thickness).max(0.) / 2.;
                let count = (angle.abs() * radius).ceil().clamp(2., 2048.) as usize;
                let points = (0..=count).map(|i| {
                    let a = from + angle * i as f64 / count as f64;
                    Point::new(
                        size.width / 2. + radius * a.cos(),
                        size.height / 2. + radius * a.sin(),
                    )
                });
                vec![Draw::stroke(
                    DrawPath::polyline(points, false),
                    ink,
                    thickness,
                )]
            })
        }
        "Canvas" => {
            let paint = session.paint_callback(props.get("paint")?)?;
            let vm = session.lua().clone();
            let activity = session.activity();
            canvas(move |size| {
                let _submission = activity.as_ref().map(|a| a.span(Phase::UiNativeCanvasSubmission));
                match canvas_draw(&vm, &paint, Size::new(size.width / s, size.height / s), s) {
                    Ok(draw) => draw,
                    Err(e) => {
                        let _ = vm.globals().set("__canvas_error", e.to_string());
                        Vec::new()
                    }
                }
            })
        }
        other => anyhow::bail!("Unsupported NativeUI primitive {other:?}"),
    }
    .shrink(0);
    let mut flex = intrinsic_flexibility(node, &kind);
    if matches!(kind.as_str(), "Canvas" | "Rectangle")
        || (kind == "Image" && boolean(&val(&props, "resizable")))
    {
        flex = (true, true);
    }
    el = expand(el, flex);
    Ok(DrawFrame {
        node: node.clone(),
        props,
        modifiers: modifiers.into(),
        style,
        el,
        flex,
        fixed: (false, false),
    })
}
fn draw_modifiers(
    ui: &mut Ui,
    slot: usize,
    s: f64,
    frame: DrawFrame,
    mut subtree: Option<El>,
) -> anyhow::Result<DrawProgress> {
    let DrawFrame {
        node,
        props,
        style,
        mut modifiers,
        mut el,
        mut flex,
        mut fixed,
    } = frame;
    let kind = string(&node, "kind");
    let container = matches!(kind.as_str(), "HStack" | "VStack" | "ZStack");
    while let Some(m) = modifiers.pop_front() {
        let name = string(&m, "name");
        let value = val(&m, "value");
        match name.as_str() {
            "frame" => {
                if let Value::Table(t) = value {
                    let (x, y) = align(&string(&t, "alignment"));
                    if container {
                        if !fixed.0 && number(&t, "width").is_some() {
                            el = el.w(Len::Pct(100.));
                        }
                        if !fixed.1 && number(&t, "height").is_some() {
                            el = el.h(Len::Pct(100.));
                        }
                    }
                    fixed.0 |= number(&t, "width").is_some();
                    fixed.1 |= number(&t, "height").is_some();
                    if matches!(kind.as_str(), "Ring" | "Canvas" | "Rectangle" | "Image") {
                        if let Some(w) = number(&t, "width") {
                            el = el.w(w.max(0.) * s);
                        }
                        if let Some(h) = number(&t, "height") {
                            el = el.h(h.max(0.) * s);
                        }
                    }
                    let mut frame = stack([el.anchor(x, y)]).align(x).justify(match y {
                        Align::Start => Justify::Start,
                        Align::End => Justify::End,
                        _ => Justify::Center,
                    });
                    if let Some(w) = number(&t, "width") {
                        frame = frame.w(w.max(0.) * s)
                    } else if number(&t, "max_width").is_some_and(|n| n.is_infinite()) {
                        frame = frame.w(Len::Pct(100.)).grow(1.)
                    }
                    if let Some(h) = number(&t, "height") {
                        frame = frame.h(h.max(0.) * s)
                    } else if number(&t, "max_height").is_some_and(|n| n.is_infinite()) {
                        frame = frame.h(Len::Pct(100.)).grow(1.)
                    }
                    let maximum = |key| {
                        number(&t, key)
                            .filter(|n| n.is_finite())
                            .map_or(1e6, |n| (n.max(0.) * s).min(1e6))
                    };
                    if number(&t, "max_width").is_some_and(f64::is_finite)
                        || number(&t, "max_height").is_some_and(f64::is_finite)
                    {
                        frame =
                            frame.max_size(Size::new(maximum("max_width"), maximum("max_height")));
                    }
                    if let Some(w) = number(&t, "max_width").filter(|n| n.is_finite()) {
                        frame = frame.w(w.max(0.) * s);
                        flex.0 = false;
                    }
                    if let Some(h) = number(&t, "max_height").filter(|n| n.is_finite()) {
                        frame = frame.h(h.max(0.) * s);
                        flex.1 = false;
                    }
                    if let Some(w) = number(&t, "min_width") {
                        frame = frame.min_w(w.max(0.) * s)
                    }
                    if let Some(h) = number(&t, "min_height") {
                        frame = frame.min_h(h.max(0.) * s)
                    }
                    if number(&t, "width").is_some() {
                        flex.0 = false;
                    } else if number(&t, "max_width").is_some_and(|n| n.is_infinite()) {
                        flex.0 = true;
                    }
                    if number(&t, "height").is_some() {
                        flex.1 = false;
                    } else if number(&t, "max_height").is_some_and(|n| n.is_infinite()) {
                        flex.1 = true;
                    }
                    el = expand(frame, flex);
                }
            }
            "position" => {
                if let Value::Table(t) = value {
                    el = el.at(
                        number(&t, "x").unwrap_or(0.) * s,
                        number(&t, "y").unwrap_or(0.) * s,
                    )
                }
            }
            "offset" => {
                if let Value::Table(t) = value {
                    el = el.offset(
                        number(&t, "x").unwrap_or(0.) * s,
                        number(&t, "y").unwrap_or(0.) * s,
                    )
                }
            }
            "align" => {
                if let Value::String(v) = value {
                    let (x, y) = align(&v.to_string_lossy());
                    el = el.anchor(x, y)
                }
            }
            "padding" => {
                let (top, right, bottom, left) = if let Value::Table(t) = value {
                    let a = number(&t, "all").unwrap_or(0.);
                    let h = number(&t, "horizontal").unwrap_or(a);
                    let v = number(&t, "vertical").unwrap_or(a);
                    (
                        number(&t, "top").unwrap_or(v),
                        number(&t, "right").unwrap_or(h),
                        number(&t, "bottom").unwrap_or(v),
                        number(&t, "left").unwrap_or(h),
                    )
                } else {
                    let n = number(&m, "value").unwrap_or(0.);
                    (n, n, n, n)
                };
                el = expand(
                    stack([el.offset(left.min(0.) * s, top.min(0.) * s)]).pad(edges(
                        top.max(0.) * s,
                        right.max(0.) * s,
                        bottom.max(0.) * s,
                        left.max(0.) * s,
                    )),
                    flex,
                );
            }
            "background" | "overlay" => {
                if let Value::Table(t) = value {
                    let (x, y) = align(&string(&m, "alignment"));
                    let Some(mut background) = subtree.take() else {
                        modifiers.push_front(m);
                        return Ok(DrawProgress::Child(
                            Box::new(DrawFrame {
                                node,
                                props,
                                style,
                                modifiers,
                                el,
                                flex,
                                fixed,
                            }),
                            t,
                        ));
                    };
                    if !string(&m, "alignment").is_empty() {
                        background = background.anchor(x, y);
                    }
                    background = if name == "background" {
                        background.underlay()
                    } else {
                        background.overlay()
                    };
                    if string(&t, "kind") == "Rectangle" {
                        background = background.full();
                    }
                    el = expand(
                        if name == "background" {
                            stack([background, el])
                        } else {
                            stack([el, background])
                        },
                        flex,
                    );
                }
            }
            "hidden" => {
                if boolean(&value) {
                    el = el.opacity(0.).disabled()
                }
            }
            "opacity" => el = el.opacity(number(&m, "value").unwrap_or(1.) as f32),
            "line_limit" => el = el.lines(number(&m, "value").unwrap_or(1.).max(1.) as usize),
            "popover" => {
                if let Value::Table(t) = value {
                    let content: Table = t.get("content")?;
                    let popup_id = format!("nui-{slot}-{}-popup", string(&node, "path"));
                    let parent_id = format!("nui-{slot}-{}", string(&node, "path"));
                    let parent = ui
                        .scene()
                        .and_then(|scene| scene.surface(&parent_id))
                        .map_or(Size::ZERO, |surface| surface.frame.size);
                    let size = ui
                        .scene()
                        .and_then(|scene| scene.surface(&popup_id))
                        .map_or(Size::ZERO, |surface| surface.frame.size);
                    let spacing = number(&t, "spacing").unwrap_or(0.) * s;
                    let (xa, ya) = align(&string(&t, "alignment"));
                    let x = match xa {
                        Align::Start => 0.,
                        Align::End => parent.width - size.width,
                        _ => (parent.width - size.width) / 2.,
                    };
                    let y = match ya {
                        Align::Start => 0.,
                        Align::End => parent.height - size.height,
                        _ => (parent.height - size.height) / 2.,
                    };
                    let (x, y) = match string(&t, "direction").as_str() {
                        "up" => (x, -size.height - spacing),
                        "left" => (-size.width - spacing, y),
                        "right" => (parent.width + spacing, y),
                        _ => (x, parent.height + spacing),
                    };
                    let Some(popup) = subtree.take() else {
                        modifiers.push_front(m);
                        return Ok(DrawProgress::Child(
                            Box::new(DrawFrame {
                                node,
                                props,
                                style,
                                modifiers,
                                el,
                                flex,
                                fixed,
                            }),
                            content,
                        ));
                    };
                    ui.capture_popup_wheel(popup_id.clone());
                    let popup = popup.id(popup_id).tracks_pointer().float().at(x, y);
                    el = stack([el, popup]);
                }
            }
            "rotation" => el = stack([el]).rotation(number(&m, "value").unwrap_or(0.).to_radians()),
            _ => {}
        }
    }
    for m in tables(&node, "modifiers")? {
        if string(&m, "name") == "align" {
            let (x, y) = align(&string(&m, "value"));
            el = el.anchor(x, y);
        }
    }
    let path = string(&node, "path");
    let interactive = tables(&node, "modifiers")?
        .iter()
        .any(|m| string(m, "name").starts_with("on_") || string(m, "name") == "popover");
    if interactive {
        el = el
            .id(format!("nui-{slot}-{path}"))
            .tracks_pointer()
            .focusable();
    }
    if style.disabled {
        el = el.disabled();
    }
    if style.hidden {
        el = el.opacity(0.).disabled();
    }
    Ok(DrawProgress::Complete(el))
}
fn events(
    ui: &mut Ui,
    node: &Table,
    session: &Session,
    slot: usize,
    s: f64,
    hovered: &mut HashSet<String>,
) -> anyhow::Result<()> {
    let path = string(node, "path");
    let id = format!("nui-{slot}-{path}");
    let r = ui.get(id.as_str());
    let was = hovered.contains(&path);
    if r.hovered {
        hovered.insert(path.clone());
    } else {
        hovered.remove(&path);
    }
    let mut event = None;
    for m in tables(node, "modifiers")? {
        let name = string(&m, "name");
        let Value::Table(t) = val(&m, "value") else {
            continue;
        };
        let triggers: Vec<&str> = match name.as_str() {
            "on_tap_gesture" => [
                (r.pressed || r.key_activated).then_some("start"),
                r.double_clicked.then_some("double"),
                (r.released || r.key_activated).then_some("complete"),
            ]
            .into_iter()
            .flatten()
            .collect(),
            "on_drag_gesture" => [
                r.pressed.then_some("start"),
                r.dragged.then_some("update"),
                r.released.then_some("complete"),
            ]
            .into_iter()
            .flatten()
            .collect(),
            "on_hover_gesture" => [
                (!was && r.hovered).then_some("enter"),
                (was && !r.hovered).then_some("exit"),
                r.hovered.then_some("update"),
            ]
            .into_iter()
            .flatten()
            .collect(),
            _ => Vec::new(),
        };
        for trigger in triggers {
            if let Ok(f) = t.get::<Function>(trigger) {
                if event.is_none() {
                    let local = ui.local(id.as_str()).unwrap_or(Point::ZERO);
                    let frame = ui
                        .scene()
                        .and_then(|scene| scene.surface(&id))
                        .map_or(Size::ZERO, |surface| surface.frame.size);
                    event = Some(session.event(
                        local.x / s,
                        local.y / s,
                        r.drag_delta.x / s,
                        r.drag_delta.y / s,
                        frame.width / s,
                        frame.height / s,
                        r.mods.shift,
                        r.mods.ctrl,
                        r.mods.alt,
                        r.mods.cmd,
                    )?);
                }
                let e = event.as_ref().expect("prepared for the first triggered handler");
                session.call(f, e.clone())?;
            }
        }
        if name == "background" || name == "overlay" {
            events(ui, &t, session, slot, s, hovered)?;
        }
        if name == "popover" {
            let popup_id = format!("{id}-popup");
            if string(&t, "auto_close_when") != "never"
                && ui.dismissed(&[id.as_str(), popup_id.as_str()])
            {
                if let Ok(close) = t.get::<Function>("on_auto_close") {
                    session.call(close, ())?;
                }
            } else if let Ok(content) = t.get::<Table>("content") {
                events(ui, &content, session, slot, s, hovered)?;
            }
        }
    }
    for child in tables(node, "children")? {
        events(ui, &child, session, slot, s, hovered)?;
    }
    Ok(())
}
fn canvas_draw(lua: &mlua::Lua, paint: &Function, size: Size, s: f64) -> mlua::Result<Vec<Draw>> {
    let frame = lua.create_table()?;
    frame.set("width", size.width)?;
    frame.set("height", size.height)?;
    let (commands, painter): (Table, Table) =
        lua.globals().get::<Function>("__painter")?.call(())?;
    paint.call::<()>((painter, frame))?;
    let mut out = Vec::new();
    for command in commands.sequence_values::<Table>() {
        let c = command?;
        let mut path = DrawPath::default();
        for step in c.get::<Table>("path")?.sequence_values::<Table>() {
            let t = step?;
            let n = |i| t.get::<f64>(i).unwrap_or(0.) * s;
            path = match t.get::<String>(1)?.as_str() {
                "move" => path.move_to(Point::new(n(2), n(3))),
                "line" => path.line_to(Point::new(n(2), n(3))),
                "cubic" => path.cubic_to(
                    Point::new(n(2), n(3)),
                    Point::new(n(4), n(5)),
                    Point::new(n(6), n(7)),
                ),
                "close" => path.close(),
                _ => path,
            };
        }
        if let Some(ink) = color(&val(&c, "fill")) {
            out.push(Draw::fill(path.clone(), ink));
        }
        if let Some(ink) = color(&val(&c, "stroke")) {
            let dash = match c.get::<Table>("dash") {
                Ok(t) => t
                    .sequence_values::<f64>()
                    .collect::<mlua::Result<Vec<_>>>()?,
                Err(_) => Vec::new(),
            };
            let width = number(&c, "width").unwrap_or(1.) * s;
            if dash.is_empty()
                || dash.len() > 64
                || dash.iter().any(|n| !n.is_finite() || *n * s < 0.25)
            {
                out.push(Draw::stroke(path, ink, width));
            } else {
                let mut dashed = DrawPath::default();
                for contour in path.flatten(0.25, 16384).map_err(mlua::Error::external)? {
                    let mut index = 0;
                    let mut left = dash[0] * s;
                    for pair in contour.windows(2) {
                        let (a, b) = (pair[0], pair[1]);
                        let dx = b.x - a.x;
                        let dy = b.y - a.y;
                        let length = dx.hypot(dy);
                        let mut at = 0.;
                        while at < length {
                            let step = left.min(length - at);
                            let point =
                                |n: f64| Point::new(a.x + dx * n / length, a.y + dy * n / length);
                            if index % 2 == 0 {
                                dashed = dashed.move_to(point(at)).line_to(point(at + step));
                            }
                            at += step;
                            left -= step;
                            if left <= f64::EPSILON {
                                index = (index + 1) % dash.len();
                                left = dash[index] * s;
                            }
                        }
                    }
                }
                out.push(Draw::stroke(dashed, ink, width));
            }
        }
    }
    Ok(out)
}

#[cfg(all(target_os = "linux", any(test, feature = "shots")))]
pub(super) fn stack_watermark() -> (usize, usize, usize) {
    let mut attr = std::mem::MaybeUninit::<libc::pthread_attr_t>::uninit();
    let (mut base, mut size) = (std::ptr::null_mut(), 0);
    // Only unused storage on this test thread is painted; leave guard pages
    // and 64 KiB below the active frame untouched.
    unsafe {
        assert_eq!(
            libc::pthread_getattr_np(libc::pthread_self(), attr.as_mut_ptr()),
            0
        );
        assert_eq!(
            libc::pthread_attr_getstack(attr.as_ptr(), &mut base, &mut size),
            0
        );
        assert_eq!(libc::pthread_attr_destroy(attr.as_mut_ptr()), 0);
        let marker = 0_u8;
        let end = (&marker as *const u8 as usize) - (64 << 10);
        let start = (base as usize + 2 * libc::sysconf(libc::_SC_PAGESIZE) as usize)
            .max(end.saturating_sub(2 << 20));
        assert!(start < end && end < base as usize + size);
        std::ptr::write_bytes(start as *mut u8, 0xa5, end - start);
        (start, end, base as usize + size)
    }
}

#[cfg(all(target_os = "linux", any(test, feature = "shots")))]
pub(super) fn stack_peak((start, end, top): (usize, usize, usize)) -> usize {
    // The marked range belongs to this thread and is below its live frames.
    let bytes = unsafe { std::slice::from_raw_parts(start as *const u8, end - start) };
    top - (start + bytes.iter().position(|&b| b != 0xa5).unwrap_or(bytes.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn primitive_fixture(source: &str) -> (Arc<Package>, Session) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Resources/native_ui")).unwrap();
        std::fs::write(dir.path().join("Resources/native_ui/main.nui"), source).unwrap();
        std::fs::write(dir.path().join("Resources/native_ui/fixture.ttf"), NOTO_SANS).unwrap();
        let mut package = Package::load(&dir.path().join("fixture.nki")).unwrap();
        package.images = super::super::native_runtime::Images::presentation_fixture();
        let package = Arc::new(package);
        let session = Session::new(package.clone(), "main", vec![]).unwrap();
        (package, session)
    }
    fn primitive_tick(
        ui: &mut Ui, graph: &Table, package: &Arc<Package>, session: &Session,
        scale: f64, retention: Option<&mut PrimitiveRetention>, shell: usize,
    ) {
        let authored = authored_size(graph);
        let el = if let Some(retention) = retention {
            draw_retained(ui, graph, package, session, 11, scale, authored, &mut HashMap::new(), retention).unwrap()
        } else {
            draw(ui, graph, package, session, 11, scale, Style::default(), &mut HashMap::new()).unwrap()
                .w(authored.width * scale).h(authored.height * scale)
                .clip().named("NativeUI performance view")
        };
        ui.frame(stack([el, text(shell.to_string()).at(48., 48.)]).w(64.).h(64.),
            Some(Size::new(64., 64.)), Input::default(), 1. / 60.).unwrap();
    }
    fn primitive_oracle(retained: &Ui, eager: &Ui) {
        assert_eq!(super::super::tests::pixels(retained, 64, 64), super::super::tests::pixels(eager, 64, 64));
        let frames = |ui: &Ui| ui.scene().unwrap().surfaces().map(|surface| surface.frame).collect::<Vec<_>>();
        assert_eq!(frames(retained), frames(eager));
    }
    // Preloaded package: no resource worker is needed to exercise State::view.
    fn primitive_state(package: Arc<Package>) -> State {
        let (_done, loading) = std::sync::mpsc::sync_channel(1);
        State {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            activity: None,
            loading,
            canceled: Arc::new(AtomicBool::new(false)),
            package: Some(package),
            failed: false,
            failure: None,
            started: false,
            entry: "main".into(),
            seed: Vec::new(),
            size: Size::new(970., 600.),
            #[cfg(feature = "shots")]
            graph_depth: None,
            #[cfg(feature = "shots")]
            graph_work: None,
        }
    }
    fn primitive_uis() -> (Ui, Ui) {
        // Same production theme and same font identity on both oracle sides.
        let font = Font::new(NOTO_SANS).unwrap();
        (super::super::theme::ui().font(font.clone()), super::super::theme::ui().font(font))
    }
    #[test]
    fn native_state_view_preserves_eager_authored_size_lookup_order() {
        let (package, eager_session) = primitive_fixture(r#"
            local ui=require('native_ui')
            return function()
                _G.lookup_count=0; _G.lookup_sequence={}
                local ink={r=0,g=0.2,b=0.4,a=1}
                _G.sibling_ink=ink
                local frame=setmetatable({height=32,min_width=0,min_height=0,
                    max_width=100,max_height=100,alignment='left top'}, {
                    __index=function(_,key)
                        assert(key=='width')
                        _G.lookup_count=_G.lookup_count+1
                        _G.lookup_sequence[#_G.lookup_sequence+1]=key..':'.._G.lookup_count
                        ink.r=_G.lookup_count/20
                        return 24+_G.lookup_count
                    end
                })
                return __node(ui.ZStack,function() return {
                    __node(ui.Rectangle,function() return {color=ink} end):frame({width=10,height=10})
                } end):frame(frame):on_tap_gesture({complete=function() end})
            end
        "#);
        let mut state = primitive_state(package.clone());
        let (mut retained, mut eager) = primitive_uis();
        for scale in [1., 1., 1.5] {
            let graph = eager_session.render().unwrap();
            let authored = authored_size(&graph);
            assert_eq!(authored, Size::new(25., 32.));
            // Original order: one authored read before any primitive lowering.
            let el = draw(&mut eager, &graph, &package, &eager_session, 11, scale,
                Style::default(), &mut HashMap::new()).unwrap();
            let el = PrimitivePlacement::Root(authored).apply(&graph, el, scale).unwrap();
            eager.frame(stack([el, text("0").at(48., 48.)]).size(64., 64.),
                Some(Size::new(64., 64.)), Input::default(), 1. / 60.).unwrap();
            let el = state.view(&mut retained, 0, scale, &ir::Interface::default(),
                &HashMap::new(), &super::super::ir_view::InputState::default());
            assert!(!state.failed, "{:?}", state.failure);
            retained.frame(stack([el, text("0").at(48., 48.)]).size(64., 64.),
                Some(Size::new(64., 64.)), Input::default(), 1. / 60.).unwrap();
            assert_eq!(state.authored(), authored);
            let path = string(&graph, "path");
            assert_eq!(retained.scene().unwrap().surface(&format!("nui-{}-{path}", state.id)).unwrap().frame.size,
                Size::new(authored.width * scale, authored.height * scale));
            primitive_oracle(&retained, &eager);
            LOCAL.with(|locals| {
                let locals = locals.borrow();
                let local = &locals[&state.id];
                let globals = local.session.lua().globals();
                let expected = eager_session.lua().globals();
                assert_eq!(globals.get::<usize>("lookup_count").unwrap(), expected.get::<usize>("lookup_count").unwrap());
                let sequence = |globals: &Table| globals.get::<Table>("lookup_sequence").unwrap()
                    .sequence_values::<String>().collect::<mlua::Result<Vec<_>>>().unwrap();
                assert_eq!(sequence(&globals), sequence(&expected));
                assert_eq!(number(&globals.get::<Table>("sibling_ink").unwrap(), "r"),
                    number(&expected.get::<Table>("sibling_ink").unwrap(), "r"));
                assert!(local.retention.keys.is_empty(), "metatable graph stays eager");
            });
        }
        let id = state.id;
        drop(state);
        LOCAL.with(|locals| assert!(!locals.borrow().contains_key(&id)));
    }
    #[test]
    fn native_state_view_typing_and_submit_keep_fresh_callbacks_and_later_sibling_mutations() {
        let (package, eager_session) = primitive_fixture(r#"
            local ui=require('native_ui')
            local token=0; local ink={r=0.2,g=0.3,b=0.4,a=1}
            return function()
                token=token+1; local current=token
                return @ui.VStack {
                    @ui.TextInput {text='Edit',on_change=function(value)
                        _G.change_count=(_G.change_count or 0)+1;_G.change_capture=current
                        _G.changed_text=value;ink.r=0.8
                    end,on_accept=function(value)
                        _G.accept_count=(_G.accept_count or 0)+1;_G.accept_capture=current
                        _G.accepted_text=value;ink.g=0.9
                    end},
                    @ui.Rectangle {color=ink}.frame(width=12,height=12)
                }.font_size(12).frame(width=48,height=48)
            end
        "#);
        let mut state = primitive_state(package.clone());
        let (mut retained, mut eager) = primitive_uis();
        let mut drafts = HashMap::new();
        let frame = |ui: &mut Ui, el: El, input: Input| {
            ui.frame(stack([el, text("0").at(48., 48.)]).size(64., 64.),
                Some(Size::new(64., 64.)), input, 1. / 60.).unwrap();
        };
        for step in 1..=6 {
            let graph = eager_session.render().unwrap();
            let field = tables(&graph, "children").unwrap()[0].clone();
            let field_id = format!("nui-{}-{}-text", state.id, string(&field, "path"));
            if step == 3 {
                retained.focus(field_id.as_str());
                eager.focus(field_id.as_str());
            }
            let input = match step {
                3 => Input { text: "x".into(), ..Default::default() },
                5 => Input { keys: vec![KeyPress { key: Key::Enter, mods: Mods::default() }], ..Default::default() },
                _ => Input::default(),
            };
            // Independent original eager lowering, with its real persistent draft.
            let authored = authored_size(&graph);
            let el = draw(&mut eager, &graph, &package, &eager_session, state.id as usize, 1.,
                Style::default(), &mut drafts).unwrap();
            let el = PrimitivePlacement::Root(authored).apply(&graph, el, 1.).unwrap();
            frame(&mut eager, el, input.clone());
            let el = state.view(&mut retained, 0, 1., &ir::Interface::default(),
                &HashMap::new(), &super::super::ir_view::InputState::default());
            assert!(!state.failed, "{:?}", state.failure);
            frame(&mut retained, el, input);
            primitive_oracle(&retained, &eager);
            LOCAL.with(|locals| {
                let locals = locals.borrow();
                let local = &locals[&state.id];
                let globals = local.session.lua().globals();
                let expected = eager_session.lua().globals();
                if step <= 2 { assert_eq!(local.retention.lowered, 1, "unfocused sibling warms once"); }
                else { assert!(local.retention.keys.is_empty(), "active edit must invalidate sibling proof"); }
                for key in ["change_count", "accept_count", "change_capture", "accept_capture"] {
                    assert_eq!(globals.get::<usize>(key).unwrap_or(0), expected.get::<usize>(key).unwrap_or(0), "{key}, step={step}");
                }
                if step == 6 {
                    assert_eq!(globals.get::<usize>("change_count").unwrap(), 1);
                    assert_eq!(globals.get::<usize>("accept_count").unwrap(), 1);
                    assert_eq!(globals.get::<usize>("change_capture").unwrap(), 4);
                    assert_eq!(globals.get::<usize>("accept_capture").unwrap(), 6);
                    for key in ["changed_text", "accepted_text"] {
                        let value = globals.get::<String>(key).unwrap();
                        assert!(value.contains('x'));
                        assert_eq!(value, expected.get::<String>(key).unwrap());
                    }
                }
            });
        }
    }

    #[test]
    fn native_state_replacement_and_removed_subtrees_do_not_reuse_stale_memos() {
        let (package, _session) = primitive_fixture(r#"
            local ui=require('native_ui')
            return function() return @ui.Group {
                not absent and @ui.Text {text=caption or 'Hi'}
            }.font_size(18).frame(width=48,height=32)
                .on_tap_gesture({complete=function() end}) end
        "#);
        let mut state = primitive_state(package.clone());
        let mut ui = super::super::theme::ui();
        let tick = |state: &mut State, ui: &mut Ui| {
            let el = state.view(ui, 0, 1., &ir::Interface::default(), &HashMap::new(),
                &super::super::ir_view::InputState::default());
            assert!(!state.failed, "{:?}", state.failure);
            ui.frame(el, Some(Size::new(64., 64.)), Input::default(), 1. / 60.).unwrap();
        };
        tick(&mut state, &mut ui);
        for (caption, absent, scopes, lowered) in [("Old", false, 1, 2), ("Old", true, 0, 2), ("Old", false, 1, 3)] {
            LOCAL.with(|locals| {
                let locals = locals.borrow();
                let globals = locals[&state.id].session.lua().globals();
                globals.set("caption", caption).unwrap();
                globals.set("absent", absent).unwrap();
            });
            tick(&mut state, &mut ui);
            LOCAL.with(|locals| {
                let locals = locals.borrow();
                assert_eq!(locals[&state.id].retention.keys.len(), scopes);
                assert_eq!(locals[&state.id].retention.lowered, lowered);
            });
        }
        let old_pixels = super::super::tests::pixels(&ui, 64, 64);
        let old_id = state.id;
        drop(state);
        let mut replacement = primitive_state(package);
        assert_ne!(replacement.id, old_id);
        tick(&mut replacement, &mut ui);
        assert_ne!(super::super::tests::pixels(&ui, 64, 64), old_pixels, "new session's Hi must replace Old");
        LOCAL.with(|locals| {
            let locals = locals.borrow();
            assert!(!locals.contains_key(&old_id));
            assert_eq!(locals[&replacement.id].retention.lowered, 1, "same Ui must lower a new session namespace");
        });
    }

    #[test]
    fn native_broad_mixed_graph_caps_memo_scopes_but_keeps_one_large_pure_root() {
        let (package, session) = primitive_fixture(r#"
            local ui=require('native_ui')
            return function()
                local children={}
                if live then children[1]=__node(ui.Canvas,function() return {paint=function(p,frame)
                    _G.canvas_calls=(_G.canvas_calls or 0)+1
                    p.stroke_style=ui.Color.red;p:draw_line(0,0,frame.width,frame.height)
                end} end):frame({width=8,height=8}) end
                for i=1,count do children[#children+1]=__node(ui.Rectangle,function()
                    return {color=ui.Color(30,80,120)} end):frame({width=2,height=2}) end
                return __node(ui.ZStack,function() return children end):frame({width=32,height=32})
            end
        "#);
        let globals = session.lua().globals();
        for count in [8, NATIVE_MEMO_SCOPE_LIMIT, NATIVE_MEMO_SCOPE_LIMIT + 1, 256, 2048] {
            for live in [true, false] {
                globals.set("count", count).unwrap();
                globals.set("live", live).unwrap();
                let (mut retained, mut eager) = primitive_uis();
                let mut cache = PrimitiveRetention::default();
                let scopes = if live { count.min(NATIVE_MEMO_SCOPE_LIMIT) } else { 1 };
                for shell in 0..2 {
                    let graph = session.render().unwrap();
                    let calls = || globals.get::<usize>("canvas_calls").unwrap_or(0);
                    let before = calls();
                    primitive_tick(&mut eager, &graph, &package, &session, 1., None, shell);
                    let eager_delta = calls() - before;
                    let before = calls();
                    primitive_tick(&mut retained, &graph, &package, &session, 1., Some(&mut cache), shell);
                    assert_eq!(calls() - before, eager_delta, "count={count}, live={live}, shell={shell}");
                    if live { assert!(eager_delta > 0, "visible Canvas must paint on cold and warm frames"); }
                    primitive_oracle(&retained, &eager);
                    assert_eq!(cache.live.len(), scopes);
                    assert_eq!(cache.keys.len(), scopes);
                    assert_eq!(cache.lowered, scopes, "warm scopes reuse; excess siblings remain eager");
                }
            }
        }
    }
    #[test]
    fn native_text_caption_ink_and_bold_each_change_real_glyph_pixels() {
        let (package, session) = primitive_fixture(r#"
            local ui=require('native_ui')
            return function() return @ui.Text {text=caption or 'Hi'}
                .font_size(18).bold(font_bold or false).foreground_color(ui.Color(ink or 255,100,40))
                .frame(width=48,height=32) end
        "#);
        for change in ["caption='Yo'", "ink=40", "font_bold=true"] {
            let (mut retained, mut eager) = primitive_uis();
            let mut cache = PrimitiveRetention::default();
            let mut baseline = Vec::new();
            for (index, update) in ["caption=nil;ink=nil;font_bold=false", change].into_iter().enumerate() {
                session.lua().load(update).exec().unwrap();
                for shell in 0..2 {
                    let graph = session.render().unwrap();
                    primitive_tick(&mut eager, &graph, &package, &session, 1., None, 0);
                    primitive_tick(&mut retained, &graph, &package, &session, 1., Some(&mut cache), 0);
                    primitive_oracle(&retained, &eager);
                    assert!(retained.scene().unwrap().paint.iter().any(|paint| paint.text.as_ref()
                        .is_some_and(|run| run.size == 18. && !run.glyphs.is_empty())), "authored glyphs must paint");
                    let pixels = super::super::tests::pixels(&retained, 64, 64);
                    if index == 0 { baseline = pixels; }
                    else { assert_ne!(pixels, baseline, "isolated {change}, warm={shell}"); }
                    assert_eq!(cache.lowered, index + 1);
                }
            }
        }
    }
    #[test]
    fn native_authored_font_file_family_and_inherited_identity_match_eager() {
        let source = r#"
            local ui=require('native_ui')
            return function() return @ui.Group { @ui.Text {text='Hi'} }
                .font(font_name or 'fixture.ttf').font_size(18).frame(width=48,height=32)
                .on_tap_gesture({complete=function() end}) end
        "#;
        let (first_package, first_session) = primitive_fixture(source);
        let (second_package, second_session) = primitive_fixture(source);
        assert_ne!(first_package.font("fixture.ttf", false).unwrap().id(),
            second_package.font("fixture.ttf", false).unwrap().id());
        let (mut retained, mut eager) = primitive_uis();
        let mut cache = PrimitiveRetention::default();
        for (index, (package, session, name)) in [
            (&first_package, &first_session, "fixture.ttf"),
            (&first_package, &first_session, "Noto Sans"),
            (&second_package, &second_session, "Noto Sans"),
        ].into_iter().enumerate() {
            session.lua().globals().set("font_name", name).unwrap();
            let font = package.font(name, false).unwrap();
            for shell in 0..2 {
                let graph = session.render().unwrap();
                primitive_tick(&mut eager, &graph, package, session, 1., None, shell);
                primitive_tick(&mut retained, &graph, package, session, 1., Some(&mut cache), shell);
                primitive_oracle(&retained, &eager);
                assert_eq!(cache.keys.len(), 1);
                assert!(cache.keys.values().all(|(key, _)| key.style.font.as_ref().unwrap().id() == font.id()));
                assert!(retained.scene().unwrap().paint.iter().any(|paint| paint.text.as_ref()
                    .is_some_and(|run| run.fonts[0].id() == font.id() && !run.glyphs.is_empty())), "authored font must reach glyph paint");
                // File and family resolve to the same inherited font. Changing
                // only its identity must rebuild the child, even with equal bytes.
                assert_eq!(cache.lowered, if index < 2 { 1 } else { 2 });
            }
        }
    }

    #[test]
    fn pure_native_subtree_is_lowered_once_across_forced_shell_builds() {
        let (package, session) = primitive_fixture(r#"
            local ui=require('native_ui')
            local calls=0
            return function()
                calls=calls+1; _G.render_calls=calls; _G.random_sample=math.random()
                ui.create_effect(function() _G.effect_calls=(_G.effect_calls or 0)+1 end)
                return @ui.ZStack {
                    @ui.Rectangle {color=ui.Color(40,70,110)}.frame(width=16,height=16),
                    @ui.Ring {color=ui.Color.white,thickness=2,angle=270}.frame(width=16,height=16),
                    @ui.Text {text='A'}.font_size(8)
                }.frame(width=32,height=32)
                    .background(__node(ui.Rectangle,function() return {color=ui.Color.black} end))
            end
        "#);
        let (mut retained, mut eager) = primitive_uis();
        let mut cache = PrimitiveRetention::default();
        for shell in 0..4 {
            let graph = session.render().unwrap();
            // Resolve the unmodified eager graph before the retained prepass
            // annotates its derived flex flags: this is an independent oracle.
            primitive_tick(&mut eager, &graph, &package, &session, 1., None, shell);
            primitive_tick(&mut retained, &graph, &package, &session, 1., Some(&mut cache), shell);
            primitive_oracle(&retained, &eager);
        }
        assert_eq!(cache.lowered, 1, "one largest pure subtree, not one memo per leaf");
        assert_eq!(session.lua().globals().get::<usize>("render_calls").unwrap(), 4);
        assert_eq!(session.lua().globals().get::<usize>("effect_calls").unwrap(), 4);
    }
    #[test]
    fn retained_native_matches_eager_on_props_style_placement_scale_and_live_canvas() {
        let (package, session) = primitive_fixture(r#"
            local ui=require('native_ui')
            return function()
                local container=ui[parent_kind or 'HStack']
                return __node(container,function() return {
                    alignment=parent_alignment or 'left top', spacing=spacing or 1,
                    __node(ui.Text,function() return {text=caption or 'A'} end),
                    __node(ui.Text,function() return {text='B'} end):frame({
                        width=not flexible and 8 or nil, max_width=flexible and math.huge or nil, height=8}),
                    __node(ui.Spacer,function() return {} end),
                    __node(ui.Canvas,function() return {paint=function(p,frame)
                        _G.canvas_calls=(_G.canvas_calls or 0)+1
                        p.stroke_style=ui.Color.red;p:draw_line(0,0,frame.width,frame.height)
                    end} end):frame({width=8,height=8}):offset({x=canvas_offset or 0,y=0})
                } end):font_size(font_size or 8):bold(font_bold or false):foreground_color(ui.Color(ink or 255,80,30))
                    :disabled(disabled or false):hidden(hidden or false):frame({width=40,height=30})
            end
        "#);
        let (mut retained, mut eager) = primitive_uis();
        let mut cache = PrimitiveRetention::default();
        let globals = session.lua().globals();
        for (change, scale, visible) in [
            ("", 1., true), ("", 1., true), ("caption='Longer'", 1., false),
            ("font_size=12;font_bold=true;ink=40", 1., false), ("flexible=true", 1., false),
            ("parent_kind='VStack';parent_alignment='right bottom'", 1., false),
            ("parent_kind='ZStack';spacing=-2", 1., true), ("", 1.5, true),
            ("disabled=true;hidden=true", 1.5, false), ("disabled=false;hidden=false", 1.5, true),
            ("canvas_offset=200", 1.5, false), ("canvas_offset=0", 1.5, true),
        ] {
            session.lua().load(change).exec().unwrap();
            let graph = session.render().unwrap();
            let calls = || globals.get::<usize>("canvas_calls").unwrap_or(0);
            let before = calls();
            primitive_tick(&mut eager, &graph, &package, &session, scale, None, 0);
            let eager_delta = calls() - before;
            let plan = PrimitivePlan::analyze(&graph).unwrap().unwrap();
            assert!(!plan.nodes[0].pure, "the deferred callback forbids caching its ancestor");
            let before = calls();
            primitive_tick(&mut retained, &graph, &package, &session, scale, Some(&mut cache), 0);
            let retained_delta = calls() - before;
            assert_eq!(retained_delta, eager_delta, "Canvas callback delta: {change}, scale={scale}");
            if visible { assert!(retained_delta > 0, "visible Canvas did not paint: {change}"); }
            // Hidden/clipped callbacks follow the actual eager paint decision.
            primitive_oracle(&retained, &eager);
        }
        assert!(cache.lowered > 1);
        assert!(cache.keys.values().all(|(key, _)| !key.atoms.iter().any(|atom| {
            matches!(atom, PrimitiveAtom::String(s) if s == b"Canvas")
        })));
    }
    #[test]
    fn native_gesture_callbacks_keep_fresh_captures_while_pure_descendants_are_retained() {
        let (package, session) = primitive_fixture(r#"
            local ui=require('native_ui')
            local token=0
            return function()
                token=token+1;local current=token
                return @ui.ZStack {
                    @ui.HStack { @ui.Text {text='A'}, @ui.Text {text='B'} }
                }.frame(width=32,height=32).on_tap_gesture({complete=function()
                    _G.seen_capture=current
                end})
            end
        "#);
        let mut ui = super::super::theme::ui();
        let mut cache = PrimitiveRetention::default();
        for token in 1..=3 {
            let graph = session.render().unwrap();
            primitive_tick(&mut ui, &graph, &package, &session, 1., Some(&mut cache), token);
            let gesture = tables(&graph, "modifiers").unwrap().into_iter()
                .find(|modifier| string(modifier, "name") == "on_tap_gesture").unwrap();
            let callback: Function = gesture.get::<Table>("value").unwrap().get("complete").unwrap();
            session.call(callback, ()).unwrap();
            assert_eq!(session.lua().globals().get::<usize>("seen_capture").unwrap(), token);
            assert!(!cache.keys.contains_key(&string(&graph, "path")));
        }
        assert_eq!(cache.lowered, 1, "the largest pure child survives changing opaque ancestor captures");
    }
    #[test]
    fn native_image_completion_and_replacement_invalidate_retained_pixels() {
        use moose::mui::mui::scene::Image;
        let (package, session) = primitive_fixture(r#"
            local ui=require('native_ui')
            return function() return @ui.Image {file='photo.png',resizable=true}
                .frame(width=16,height=16) end
        "#);
        let (mut retained, mut eager) = primitive_uis();
        let mut cache = PrimitiveRetention::default();
        let mut prior = Vec::new();
        for (index, rgba) in [None, Some([255,0,0,255]), Some([0,255,0,255])].into_iter().enumerate() {
            if let Some(rgba) = rgba {
                package.images.presentation_complete("photo.png", Arc::new(Image::rgba(1, 1, rgba.to_vec()).unwrap()));
            }
            let graph = session.render().unwrap();
            primitive_tick(&mut eager, &graph, &package, &session, 1., None, 0);
            primitive_tick(&mut retained, &graph, &package, &session, 1., Some(&mut cache), 0);
            primitive_oracle(&retained, &eager);
            let pixels = super::super::tests::pixels(&retained, 64, 64);
            if index > 0 { assert_ne!(prior, pixels, "a placeholder/old photo must not freeze"); }
            prior = pixels;
        }
        assert_eq!(cache.lowered, 3);
    }
    #[test]
    fn native_opaque_fields_and_edit_callbacks_keep_the_eager_path() {
        let (package, session) = primitive_fixture(r#"
            local ui=require('native_ui')
            return function() return @ui.Text {text='A'} end
        "#);
        let graph = session.render().unwrap();
        let props: Table = graph.get("props").unwrap();
        props.set("opaque", session.lua().create_function(|_, ()| Ok(())).unwrap()).unwrap();
        let plan = PrimitivePlan::analyze(&graph).unwrap().unwrap();
        assert!(!plan.nodes[0].pure);
        props.set("opaque", Value::Nil).unwrap();
        props.set("text", session.lua().create_function(|_, ()| Ok("A")).unwrap()).unwrap();
        assert!(!PrimitivePlan::analyze(&graph).unwrap().unwrap().nodes[0].pure);
        struct Opaque;
        impl mlua::UserData for Opaque {}
        props.set("text", session.lua().create_userdata(Opaque).unwrap()).unwrap();
        assert!(!PrimitivePlan::analyze(&graph).unwrap().unwrap().nodes[0].pure);
        let metamethod: Table = session.lua().load(r#"
            return {__index=function(_,key) _G.lookups=(_G.lookups or 0)+1;return 'live' end}
        "#).eval().unwrap();
        props.set("text", Value::Nil).unwrap();
        props.set_metatable(Some(metamethod)).unwrap();
        assert!(PrimitivePlan::analyze(&graph).unwrap().is_none());
        assert_eq!(session.lua().globals().get::<Value>("lookups").unwrap().type_name(), "nil");
        let mut ui = super::super::theme::ui();
        let mut cache = PrimitiveRetention::default();
        for _ in 0..3 { primitive_tick(&mut ui, &graph, &package, &session, 1., Some(&mut cache), 0); }
        assert_eq!(cache.lowered, 0);
        // The eager lowerer reads both spacing and text on every frame.
        assert_eq!(session.lua().globals().get::<usize>("lookups").unwrap(), 6);

        let (editing_package, editing) = primitive_fixture(r#"
            local ui=require('native_ui')
            local token=0
            return function()
                token=token+1;local current=token
                return @ui.HStack {
                    @ui.Text {text='static'}, @ui.TextInput {text='edit',on_change=function()
                        _G.edit_capture=current
                    end}
                }
            end
        "#);
        for token in 1..=3 {
            let graph = editing.render().unwrap();
            assert!(PrimitivePlan::analyze(&graph).unwrap().is_none(),
                "editing callbacks may mutate shared graph data during lowering");
            let field = tables(&graph, "children").unwrap()[1].clone();
            ui.focus(format!("nui-11-{}-text", string(&field, "path")));
            primitive_tick(&mut ui, &graph, &editing_package, &editing, 1., Some(&mut cache), 0);
            let props: Table = field.get("props").unwrap();
            editing.call(props.get::<Function>("on_change").unwrap(), "edited").unwrap();
            assert_eq!(editing.lua().globals().get::<usize>("edit_capture").unwrap(), token);
        }
        assert_eq!(cache.lowered, 0);
    }
    #[test]
    fn unfocused_native_text_inputs_stay_live_without_invalidating_pure_siblings() {
        let (package, session) = primitive_fixture(r#"
            local ui=require('native_ui')
            local token=0
            return function()
                token=token+1;local current=token
                return @ui.HStack {
                    @ui.Text {text='static'}, @ui.TextInput {text='Edit',on_accept=function()
                        _G.accept_capture=current
                    end}
                }.font_size(8).frame(width=48,height=24)
            end
        "#);
        let (mut retained, mut eager) = primitive_uis();
        let mut cache = PrimitiveRetention::default();
        for token in 1..=3 {
            let graph = session.render().unwrap();
            primitive_tick(&mut eager, &graph, &package, &session, 1., None, token);
            primitive_tick(&mut retained, &graph, &package, &session, 1., Some(&mut cache), token);
            primitive_oracle(&retained, &eager);
            let field = tables(&graph, "children").unwrap()[1].clone();
            assert!(!cache.keys.contains_key(&string(&field, "path")));
            let props: Table = field.get("props").unwrap();
            session.call(props.get::<Function>("on_accept").unwrap(), ()).unwrap();
            assert_eq!(session.lua().globals().get::<usize>("accept_capture").unwrap(), token);
        }
        assert_eq!(cache.lowered, 1);
    }
    #[test]
    fn native_retention_prepass_is_bounded_and_keeps_the_original_draw_errors() {
        std::thread::Builder::new().stack_size(2 << 20).spawn(|| {
            let (package, session) = primitive_fixture("return function() return nil end");
            let make: Function = session.lua().load(r#"
                local function node(kind,path,children)
                    return {kind=kind,path=path,props={},children=children or {},modifiers={}}
                end
                return function(depth,width)
                    local root=node('Spacer','leaf')
                    for i=1,depth do root=node('Group','depth-'..i,{root}) end
                    for i=1,width do root.children[i]=node('Spacer','wide-'..i) end
                    return root
                end
            "#).eval().unwrap();
            let maximum: Table = make.call((192, 0)).unwrap();
            assert_eq!(PrimitivePlan::analyze(&maximum).unwrap().unwrap().nodes.len(), 193);
            let wide: Table = make.call((0, 16383)).unwrap();
            assert_eq!(PrimitivePlan::analyze(&wide).unwrap().unwrap().nodes.len(), 16384);
            let shared: Table = session.lua().load(r#"
                local value=string.rep('x',65536)
                local modifier={name='font_size',value=value}
                local modifiers={};for i=1,1000 do modifiers[i]=modifier end
                return {kind='Group',path='shared',props={},children={},modifiers=modifiers}
            "#).eval().unwrap();
            assert!(PrimitivePlan::analyze(&shared).unwrap().is_none(), "aggregate key memory is bounded even for shared data");
            let mut ui = Ui::default();
            let mut cache = PrimitiveRetention::default();
            for (depth, width) in [(193, 0), (0, 16384)] {
                let graph: Table = make.call((depth, width)).unwrap();
                assert!(PrimitivePlan::analyze(&graph).unwrap().is_none());
                let eager = draw(&mut ui, &graph, &package, &session, 11, 1., Style::default(), &mut HashMap::new()).err().unwrap().to_string();
                let retained = draw_retained(&mut ui, &graph, &package, &session, 11, 1., authored_size(&graph), &mut HashMap::new(), &mut cache).err().unwrap().to_string();
                assert_eq!(eager, "NativeUI graph budget exceeded");
                assert_eq!(retained, eager);
            }
            // No cached hit bypasses a newly-invalid graph or keeps stale nodes.
            let valid: Table = make.call((1, 0)).unwrap();
            primitive_tick(&mut ui, &valid, &package, &session, 1., Some(&mut cache), 0);
            valid.set("kind", "Unsupported").unwrap();
            let error = draw_retained(&mut ui, &valid, &package, &session, 11, 1., authored_size(&valid), &mut HashMap::new(), &mut cache).err().unwrap();
            assert!(error.to_string().contains("Unsupported NativeUI primitive"));
        }).unwrap().join().unwrap();
    }


    #[test]
    fn native_seed_reuses_unchanged_readback_and_keeps_authored_scalar_policy() {
        let (_done, loading) = std::sync::mpsc::sync_channel(1);
        let mut widget = ir::Widget::new(
            "$readback",
            ir::PageRef(0),
            Default::default(),
            ir::Kind::Label,
        );
        widget.binding = ir::Binding::Control(ir::ControlId(42));
        widget.value = Some(ir::Value::Integer(20));
        widget.text = "Authored caption".into();
        let mut face = ir::Interface {
            source: ir::Source::Ksp { slot: 2 },
            widgets: vec![widget.clone()],
            ..Default::default()
        };
        // Exercise publication before a local VM exists, without a resource worker.
        let mut state = State {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            activity: None,
            loading,
            canceled: Arc::new(AtomicBool::new(false)),
            package: None,
            failed: false,
            failure: None,
            started: false,
            entry: "main".into(),
            seed: vec![
                (face.source, 0, widget.clone()),
                (ir::Source::Ksp { slot: 4 }, 0, widget),
            ],
            size: Size::new(970., 600.),
            #[cfg(feature = "shots")]
            graph_depth: None,
            #[cfg(feature = "shots")]
            graph_work: None,
        };
        let values = HashMap::from([(ir::ControlId(42), 75.25)]);
        let meters = HashMap::new();
        for value in [
            ir::Value::Text("Callback readback".into()),
            ir::Value::Integers(vec![1, 2, 3]),
            ir::Value::Reals(vec![0.25, 0.5]),
            ir::Value::Integer(75),
            ir::Value::Real(75.25),
        ] {
            let typed = HashMap::from([(ir::WidgetRef(0), value.clone())]);
            state.update_view(&face, &values, &typed, &meters);
            let heap_calls = crate::plugin::tests::allocations(|| {
                for _ in 0..64 {
                    state.update_view(&face, &values, &typed, &meters);
                }
            });
            assert_eq!(heap_calls, 0, "unchanged Native seed recopied {value:?}");
            assert_eq!(state.seed[0].2.value.as_ref(), Some(&value));
            assert_eq!(state.seed[1].2.value, Some(ir::Value::Integer(20)));
        }
        face.widgets[0].text = "Changed caption".into();
        face.widgets[0].hidden = true;
        state.update_view(&face, &values, &Default::default(), &meters);
        assert_eq!(state.seed[0].2.text, "Changed caption");
        assert!(state.seed[0].2.hidden);
        assert_eq!(
            state.seed[0].2.value,
            Some(ir::Value::Integer(20)),
            "the seed retains authored scalars; only the local Session overlays telemetry"
        );
    }
    #[test]
    fn intrinsic_flexibility_retains_child_frames_without_allocating_a_wrapper() {
        let lua = mlua::Lua::new();
        let make: Function = lua.load(r#"
            return function(kind, axis)
                local child_frame = {[axis]=5}
                local child = {kind='Spacer',props={},children={},modifiers={{name='frame',value=child_frame}}}
                return {kind=kind,props={},children={child},modifiers={{name='frame',value={width=30,height=10}}}}
            end
        "#).eval().unwrap();
        for (kind, axis, expected) in [
            ("HStack", "height", (true, false)),
            ("VStack", "width", (false, true)),
        ] {
            let node: Table = make.call((kind, axis)).unwrap();
            assert_eq!(intrinsic_flexibility(&node, kind), expected);
            assert_eq!(flexibility(&node), (false, false));
            assert_eq!(intrinsic_flexibility(&node, kind), expected,
                "cached outer-frame flags must not alter the primitive's intrinsic layout");
            assert_eq!(tables(&node, "modifiers").unwrap().len(), 1);
        }
    }

    #[test]
    fn editor_memory_repeated_layout_queries_do_not_rewalk_the_subtree() {
        let lua = mlua::Lua::new();
        lua.set_memory_limit(128 << 20).unwrap();
        let make: Function = lua.load(r#"
            return function(fixed)
                local frame=fixed and {width=20,height=20} or {max_width=math.huge,max_height=math.huge}
                local root={kind='Spacer',props={},children={},modifiers={{name='frame',value=frame}}}
                for _=1,64 do root={kind='VStack',props={},children={root},modifiers={}} end
                return root
            end
        "#).eval().unwrap();
        let graph: Table = make.call(false).unwrap();
        assert_eq!(flexibility(&graph), (true, true));
        let calls = crate::plugin::tests::allocations(|| {
            for _ in 0..32 {
                assert_eq!(flexibility(&graph), (true, true));
            }
        });
        assert!(
            calls < 512,
            "repeated layout queries made {calls} allocator calls"
        );
        let fresh: Table = make.call(true).unwrap();
        assert_eq!(flexibility(&fresh), (false, false));
    }

    #[test]
    fn inactive_native_graph_does_not_allocate_lua_event_payloads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Resources/native_ui")).unwrap();
        std::fs::write(
            dir.path().join("Resources/native_ui/main.nui"),
            b"return function() return nil end",
        )
        .unwrap();
        let package = Arc::new(Package::load(&dir.path().join("fixture.nki")).unwrap());
        let session = Session::new(package, "main", vec![]).unwrap();
        let graph: Table = session.lua().load(r#"
            local root={kind='Group',path='root',props={},children={},modifiers={}}
            for i=1,512 do
                root.children[i]={kind='Text',path='root/'..i,props={text='Static'},children={},modifiers={}}
            end
            return root
        "#).eval().unwrap();
        let mut ui = super::super::theme::ui();
        let mut hovered = HashSet::new();
        events(&mut ui, &graph, &session, 0, 1., &mut hovered).unwrap();
        let before = session.event_payload_count();
        for _ in 0..32 {
            events(&mut ui, &graph, &session, 0, 1., &mut hovered).unwrap();
        }
        assert_eq!(session.event_payload_count(), before);
    }

    #[test]
    fn triggered_native_handlers_share_one_payload_and_keep_callback_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Resources/native_ui")).unwrap();
        std::fs::write(
            dir.path().join("Resources/native_ui/main.nui"),
            b"return function() return nil end",
        )
        .unwrap();
        let package = Arc::new(Package::load(&dir.path().join("fixture.nki")).unwrap());
        let session = Session::new(package, "main", vec![]).unwrap();
        let graph: Table = session
            .lua()
            .load(
                r#"
            calls=0
            return {kind='Rectangle',path='root',props={},children={},modifiers={
                {name='on_hover_gesture',value={
                    enter=function(e) e.marker=42; calls=calls+1 end,
                    update=function(e) assert(e.marker==42); calls=calls+1 end
                }},
                {name='on_hover_gesture',value={
                    update=function(e) assert(e.marker==42); calls=calls+1 end
                }}
            }}
        "#,
            )
            .eval()
            .unwrap();
        let mut ui = super::super::theme::ui();
        ui.frame(
            block(80., 24.).id("nui-0-root").tracks_pointer(),
            None,
            Input::default(),
            1. / 60.,
        )
        .unwrap();
        ui.frame(
            block(80., 24.).id("nui-0-root").tracks_pointer(),
            None,
            Input {
                pointer: PointerInput {
                    pos: Some(Point::new(5., 5.)),
                    ..Default::default()
                },
                ..Default::default()
            },
            1. / 60.,
        )
        .unwrap();
        assert!(ui.get("nui-0-root").hovered);
        events(&mut ui, &graph, &session, 0, 1., &mut HashSet::new()).unwrap();
        assert_eq!(session.event_payload_count(), 1);
        assert_eq!(session.lua().globals().get::<i32>("calls").unwrap(), 3);
    }

    #[test]
    fn native_popover_keeps_its_geometry_and_pointer_target_while_capturing_wheel() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Resources/native_ui")).unwrap();
        std::fs::write(
            dir.path().join("Resources/native_ui/main.nui"),
            b"return function() return nil end",
        )
        .unwrap();
        let package = Arc::new(Package::load(&dir.path().join("fixture.nki")).unwrap());
        let session = Session::new(package.clone(), "main", vec![]).unwrap();
        let graph: Table = session
            .lua()
            .load(
                r#"return {
            kind="Rectangle",path="root",props={},children={},modifiers={
                {name="frame",value={width=80,height=24}},
                {name="popover",value={direction="down",spacing=4,content={
                    kind="Rectangle",path="root/popover",props={},children={},modifiers={
                        {name="frame",value={width=137,height=83}}
                    }
                }}}
            }
        }"#,
            )
            .eval()
            .unwrap();
        let mut ui = super::super::theme::ui();
        let tick = |ui: &mut Ui, input: Input| {
            let el = draw(
                ui,
                &graph,
                &package,
                &session,
                0,
                1.,
                Style::default(),
                &mut HashMap::new(),
            )
            .unwrap();
            ui.frame(
                stack![
                    col![block(300., 900.)]
                        .size(300., 200.)
                        .scroll()
                        .id("background"),
                    el.at(100., 30.)
                ]
                .size(300., 200.),
                None,
                input,
                1. / 60.,
            )
            .unwrap();
        };
        for _ in 0..4 {
            tick(&mut ui, Input::default());
        }
        let popup = ui
            .scene()
            .unwrap()
            .surface("nui-0-root-popup")
            .unwrap()
            .frame;
        assert_eq!(popup.size, Size::new(137., 83.));
        let at = Point::new(popup.x + 10., popup.y + 10.);
        tick(
            &mut ui,
            Input {
                pointer: PointerInput {
                    pos: Some(at),
                    ..Default::default()
                },
                wheel: Vec2::new(0., 60.),
                ..Default::default()
            },
        );
        assert!(
            ui.get("nui-0-root-popup").hovered,
            "the authored popup keeps its pointer target"
        );
        assert_eq!(ui.wheel("background"), None);
        assert_eq!(ui.scroll("background"), [0., 0.]);
    }

    #[cfg(feature = "shots")]
    #[test]
    fn widget_gate_uses_the_painted_session_namespace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Resources/native_ui")).unwrap();
        std::fs::write(
            dir.path().join("Resources/native_ui/main.nui"),
            b"return function() return nil end",
        )
        .unwrap();
        let package = Arc::new(Package::load(&dir.path().join("fixture.nki")).unwrap());
        let session = Session::new(package.clone(), "main", vec![]).unwrap();
        let graph = session.lua().create_table().unwrap();
        graph.set("kind", "TextInput").unwrap();
        graph.set("path", "root").unwrap();
        for field in ["children", "modifiers"] {
            graph
                .set(field, session.lua().create_table().unwrap())
                .unwrap();
        }
        let lifetime = Arc::new(AtomicBool::new(false));
        LOCAL.with(|local| {
            local.borrow_mut().insert(
                97,
                Local {
                    package: package.clone(),
                    lifetime: Arc::downgrade(&lifetime),
                    session,
                    graph: Some(graph),
                    hovered: Default::default(),
                    drafts: Default::default(),
                    retention: PrimitiveRetention::default(),
                },
            );
        });
        GATE_PAINTED.with(|painted| {
            painted.borrow_mut().insert(0, 97);
        });
        assert_eq!(gate_targets(0), vec![("nui-97-root-text".into(), "text")]);
        assert!(gate_targets(1).is_empty());
        gate_clear();
        assert!(gate_targets(0).is_empty());
        LOCAL.with(|local| {
            local.borrow_mut().remove(&97);
        });
    }

    #[test]
    fn scheduler_delay_does_not_reject_bounded_native_graph_work() {
        let dir =
            std::env::temp_dir().join(format!("kontra-native-work-budget-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Resources/native_ui")).unwrap();
        std::fs::write(
            dir.join("Resources/native_ui/main.nui"),
            br#"
            local ui=require('native_ui')
            return function()
                __audit_pause()
                local sum=0;for i=1,2000 do sum=sum+i end
                return @ui.Text {text=tostring(sum)}
            end
        "#,
        )
        .unwrap();
        let package = Arc::new(Package::load(&dir.join("fixture.nki")).unwrap());
        let session = Session::new(package, "main", vec![]).unwrap();
        let pause = Arc::new(AtomicBool::new(false));
        let callback_pause = pause.clone();
        session
            .lua()
            .globals()
            .set(
                "__audit_pause",
                session
                    .lua()
                    .create_function(move |_, ()| {
                        if callback_pause.load(Ordering::Relaxed) {
                            std::thread::sleep(std::time::Duration::from_millis(300));
                        }
                        Ok(())
                    })
                    .unwrap(),
            )
            .unwrap();
        let graph = session.render().unwrap();
        let work = session.graph_work();
        pause.store(true, Ordering::Relaxed);
        let delayed = session
            .render()
            .expect("scheduler delay must not invalidate bounded graph work");
        assert_eq!(
            session.graph_work(),
            work,
            "work counts must be independent of scheduler delay"
        );
        assert_eq!(
            string(&graph.get::<Table>("props").unwrap(), "text"),
            string(&delayed.get::<Table>("props").unwrap(), "text")
        );
        assert!(work.0 > 0 && work.0 <= 16384 && work.1 > 0 && work.1 < 1_000_000);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deferred_canvas_gets_a_fresh_callback_budget() {
        let dir = std::env::temp_dir().join(format!(
            "kontra-native-canvas-budget-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join("Resources/native_ui")).unwrap();
        std::fs::write(
            dir.join("Resources/native_ui/main.nui"),
            br#"
            local ui=require('native_ui')
            return function() return @ui.Canvas {paint=function(painter,frame)
                for i=1,600 do painter:move_to(0,0);painter:line_to(16,16) end
                painter.stroke_style=ui.Color.white;painter:draw_path()
            end}.frame(width=16,height=16) end
        "#,
        )
        .unwrap();
        let package = Arc::new(Package::load(&dir.join("fixture.nki")).unwrap());
        let session = Session::new(package.clone(), "main", vec![]).unwrap();
        let graph = session.render().unwrap();
        let mut ui = super::super::theme::ui();
        let el = draw(
            &mut ui,
            &graph,
            &package,
            &session,
            0,
            1.,
            Style::default(),
            &mut HashMap::new(),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        ui.frame(el, Some(Size::new(16., 16.)), Input::default(), 1. / 60.)
            .unwrap();
        let _ = super::super::tests::pixels(&ui, 16, 16);
        assert!(
            session
                .lua()
                .globals()
                .get::<Value>("__canvas_error")
                .unwrap()
                .is_nil(),
            "deferred Canvas reused an expired graph budget"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn native_graph_lowering_fits_a_plain_two_mib_thread() {
        std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(|| {
                let dir = std::env::temp_dir()
                    .join(format!("kontra-native-stack-{}", std::process::id()));
                std::fs::create_dir_all(dir.join("Resources/native_ui")).unwrap();
                std::fs::write(
                    dir.join("Resources/native_ui/main.nui"),
                    b"return function() return nil end",
                )
                .unwrap();
                let package = Arc::new(Package::load(&dir.join("fixture.nki")).unwrap());
                let session = Session::new(package.clone(), "main", vec![]).unwrap();
                let lua = session.lua();
                let node = |kind: &str, child: Option<Table>| {
                    let node = lua.create_table().unwrap();
                    node.set("kind", kind).unwrap();
                    node.set("props", lua.create_table().unwrap()).unwrap();
                    node.set("modifiers", lua.create_table().unwrap()).unwrap();
                    let children = lua.create_table().unwrap();
                    if let Some(child) = child {
                        children.push(child).unwrap();
                    }
                    node.set("children", children).unwrap();
                    node
                };
                let mut graph = node("Rectangle", None);
                for _ in 0..64 {
                    graph = node("Group", Some(graph));
                }
                let mut ui = super::super::theme::ui();
                let el = draw(
                    &mut ui,
                    &graph,
                    &package,
                    &session,
                    0,
                    1.,
                    Style::default(),
                    &mut HashMap::new(),
                )
                .unwrap();
                ui.frame(el, Some(Size::new(16., 16.)), Input::default(), 1. / 60.)
                    .unwrap();
                assert!(ui.scene().is_some());
                std::fs::remove_dir_all(dir).unwrap();
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    #[ignore = "requires locally owned NativeUI library; graph and bindings stay in RAM"]
    fn native_edit_selector_bindings_use_the_published_source() {
        let path = std::path::PathBuf::from(std::env::var_os("KONTRA_AUDIT_WIDGET_PATCH").unwrap());
        let mut source = sampler_kontakt::read(&path).unwrap().instrument;
        source.zones.clear();
        source.source_indices.zones.clear();
        source.assets.clear();
        let loaded = sampler_kontakt::prepare(
            source,
            vec![],
            &sampler_kontakt::Options {
                library: Some(path.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let entry = loaded
            .interfaces
            .iter()
            .find_map(|f| f.native_ui.as_ref())
            .unwrap()
            .entry
            .clone();
        let controls: Vec<_> = loaded
            .interfaces
            .iter()
            .flat_map(|f| {
                f.widgets
                    .iter()
                    .enumerate()
                    .map(move |(n, w)| (f.source, n, w.clone()))
            })
            .collect();
        let package = Arc::new(Package::load(&path).unwrap());
        let session = Session::new(package, &entry, controls.clone()).unwrap();
        let reads = Arc::new(std::sync::Mutex::new(
            Vec::<(String, usize, String, String)>::new(),
        ));
        let trace = reads.clone();
        session
            .lua()
            .globals()
            .set(
                "__audit_parameter",
                session
                    .lua()
                    .create_function(
                        move |_, (name, binding, path, access): (String, i64, String, String)| {
                            if binding >= 0
                                && (name.starts_with("Edit__Synth__Src__")
                                    || name.starts_with("Edit__Synth__Shp__"))
                            {
                                let mut trace = trace.lock().unwrap();
                                if trace.len() < 16384 {
                                    trace.push((name, binding as usize, path, access));
                                }
                            }
                            Ok(())
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        fn tap(node: &Table, target: &str) -> Option<Function> {
            let mut contains = string(node, "kind") == "Text"
                && node
                    .get::<Table>("props")
                    .ok()
                    .is_some_and(|p| string(&p, "text").eq_ignore_ascii_case(target));
            for child in tables(node, "children").unwrap() {
                if let Some(callback) = tap(&child, target) {
                    return Some(callback);
                }
                contains |= has_text(&child, target);
            }
            if contains {
                for modifier in tables(node, "modifiers").unwrap() {
                    if string(&modifier, "name") == "on_tap_gesture"
                        && let Ok(value) = modifier.get::<Table>("value")
                        && let Ok(callback) = value
                            .get::<Function>("complete")
                            .or_else(|_| value.get("start"))
                    {
                        return Some(callback);
                    }
                    if matches!(string(&modifier, "name").as_str(), "background" | "overlay")
                        && let Ok(child) = modifier.get::<Table>("value")
                        && let Some(callback) = tap(&child, target)
                    {
                        return Some(callback);
                    }
                }
            }
            None
        }
        fn has_text(node: &Table, target: &str) -> bool {
            (string(node, "kind") == "Text"
                && node
                    .get::<Table>("props")
                    .ok()
                    .is_some_and(|p| string(&p, "text").eq_ignore_ascii_case(target)))
                || tables(node, "children")
                    .unwrap()
                    .iter()
                    .any(|c| has_text(c, target))
        }
        let graph = session
            .render()
            .unwrap_or_else(|e| panic!("{}", failure(&e, "graph").split_once(": ").unwrap().0));
        let callback = tap(&graph, "EDIT").expect("authored Edit tab has a tap callback");
        session
            .call(
                callback,
                session
                    .event(0., 0., 0., 0., 0., 0., false, false, false, false)
                    .unwrap(),
            )
            .unwrap();
        reads.lock().unwrap().clear();
        let graph = session
            .render()
            .unwrap_or_else(|e| panic!("{}", failure(&e, "graph").split_once(": ").unwrap().0));
        fn node_kinds(node: &Table, out: &mut HashMap<String, String>) {
            out.insert(string(node, "path"), string(node, "kind"));
            for child in tables(node, "children").unwrap() {
                node_kinds(&child, out);
            }
            for modifier in tables(node, "modifiers").unwrap() {
                if matches!(string(&modifier, "name").as_str(), "background" | "overlay")
                    && let Ok(child) = modifier.get::<Table>("value")
                {
                    node_kinds(&child, out);
                }
            }
        }
        let mut kinds = HashMap::new();
        node_kinds(&graph, &mut kinds);
        let mut seen = std::collections::BTreeSet::new();
        for (name, binding, path, access) in reads.lock().unwrap().iter() {
            if !(name.starts_with("Edit__Synth__Src__") || name.starts_with("Edit__Synth__Shp__")) {
                continue;
            }
            let kind = kinds.get(path).map(String::as_str).unwrap_or("Component");
            if !seen.insert((name.clone(), kind.to_owned(), *binding, access.clone())) {
                continue;
            }
            let (source, _, widget) = controls
                .get(*binding)
                .expect("binding addresses a published control");
            println!(
                "NATIVE_BINDING node={kind} parameter={name} access={access} source={source:?} ui_id={:?} binding={:?}",
                widget.source_id, widget.binding
            );
        }
        assert!(
            !seen.is_empty(),
            "Edit graph reads published selector parameters"
        );
    }

    #[cfg(feature = "shots")]
    #[test]
    #[ignore = "requires locally owned NativeUI program; numeric work only"]
    fn native_program_work_counts() {
        let path = std::path::PathBuf::from(std::env::var_os("KONTRA_AUDIT_WIDGET_PATCH").unwrap());
        let program = std::env::var("KONTRA_AUDIT_PROGRAM")
            .ok()
            .map_or(0, |n| n.parse().unwrap());
        let mut source = if path.extension().is_some_and(|e| e == "nkm") {
            sampler_kontakt::read_program(&path, program)
                .unwrap()
                .instrument
        } else {
            sampler_kontakt::read(&path).unwrap().instrument
        };
        source.retain_zones(|_| false);
        source.assets.clear();
        let loaded = sampler_kontakt::prepare(
            source,
            vec![],
            &sampler_kontakt::Options {
                library: Some(path.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let entry = loaded
            .interfaces
            .iter()
            .find_map(|f| f.native_ui.as_ref())
            .unwrap()
            .entry
            .clone();
        let controls = loaded
            .interfaces
            .iter()
            .flat_map(|f| {
                f.widgets
                    .iter()
                    .enumerate()
                    .map(move |(n, w)| (f.source, n, w.clone()))
            })
            .collect();
        let package = Arc::new(Package::load(&path).unwrap());
        let session = Session::new(package, &entry, controls).unwrap();
        println!(
            "NATIVE_WORK_INIT program={program} checkpoints={} checkpoint_budget=500000",
            500_000 - session.work_remaining()
        );
        for frame in 0..8 {
            let _ = session.render().unwrap();
            let (nodes, checkpoints) = session.graph_work();
            assert!(nodes > 0 && nodes <= 16384 && checkpoints < 1_000_000);
            println!(
                "NATIVE_WORK program={program} frame={frame} nodes={nodes} checkpoints={checkpoints} node_budget=16384 checkpoint_budget=1000000"
            );
        }
    }

    #[cfg(all(target_os = "linux", feature = "shots"))]
    #[test]
    #[ignore = "requires locally owned NativeUI program; graph stays in RAM"]
    fn native_program_stack_watermark() {
        let path = std::path::PathBuf::from(std::env::var_os("KONTRA_AUDIT_WIDGET_PATCH").unwrap());
        let program = std::env::var("KONTRA_AUDIT_PROGRAM")
            .ok()
            .map_or(0, |n| n.parse().unwrap());
        let mut source = if path.extension().is_some_and(|e| e == "nkm") {
            sampler_kontakt::read_program(&path, program)
                .unwrap()
                .instrument
        } else {
            sampler_kontakt::read(&path).unwrap().instrument
        };
        source.retain_zones(|_| false);
        source.assets.clear();
        let loaded = sampler_kontakt::prepare(
            source,
            vec![],
            &sampler_kontakt::Options {
                library: Some(path.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let entry = loaded
            .interfaces
            .iter()
            .find_map(|f| f.native_ui.as_ref())
            .unwrap()
            .entry
            .clone();
        let controls = loaded
            .interfaces
            .iter()
            .flat_map(|f| {
                f.widgets
                    .iter()
                    .enumerate()
                    .map(move |(n, w)| (f.source, n, w.clone()))
            })
            .collect();
        let package = Arc::new(Package::load(&path).unwrap());
        let watermark = stack_watermark();
        let session = Session::new(package.clone(), &entry, controls).unwrap();
        let graph = session.render().unwrap();
        let mut ui = super::super::theme::ui();
        let mut drafts = HashMap::new();
        for _ in 0..4 {
            let el = draw(
                &mut ui,
                &graph,
                &package,
                &session,
                0,
                1.,
                Style::default(),
                &mut drafts,
            )
            .unwrap();
            ui.frame(el, Some(authored_size(&graph)), Input::default(), 1. / 60.)
                .unwrap();
        }
        let peak = stack_peak(watermark);
        assert!(
            peak < watermark.2 - watermark.0 - 4096,
            "stack watermark saturated"
        );
        println!(
            "NATIVE_STACK program={program} depth={} native_init_graph_draw_layout_peak_bytes={peak} unmarked_top_bytes={}",
            graph_depth(&graph).unwrap(),
            watermark.2 - watermark.1
        );
    }

    #[test]
    #[ignore = "requires locally owned NativeUI library; text stays in RAM"]
    fn native_saved_text_reaches_authored_field_without_host_insets() {
        let path = std::path::PathBuf::from(std::env::var_os("KONTRA_AUDIT_WIDGET_PATCH").unwrap());
        let mut source = sampler_kontakt::read(&path).unwrap().instrument;
        source.retain_zones(|_| false);
        source.assets.clear();
        let loaded = sampler_kontakt::prepare(
            source,
            vec![],
            &sampler_kontakt::Options {
                library: Some(path.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let entry = loaded
            .interfaces
            .iter()
            .find_map(|f| f.native_ui.as_ref())
            .unwrap()
            .entry
            .clone();
        let controls: Vec<_> = loaded
            .interfaces
            .iter()
            .flat_map(|f| {
                f.widgets
                    .iter()
                    .enumerate()
                    .map(move |(n, w)| (f.source, n, w.clone()))
            })
            .collect();
        let saved: Vec<_> = controls
            .iter()
            .filter(|(_, _, w)| w.name.starts_with("@Footer__Macro__Name__"))
            .map(|(_, _, w)| match &w.value {
                Some(ir::Value::Text(s)) => s.clone(),
                _ => panic!("published text missing"),
            })
            .collect();
        assert_eq!(saved.len(), 6);
        let package = Arc::new(Package::load(&path).unwrap());
        let session = Session::new(package.clone(), &entry, controls).unwrap();
        let graph = session.render().unwrap();
        fn font_usage(node: &Table, package: &Package, counts: &mut [usize; 3]) {
            for modifier in tables(node, "modifiers").unwrap() {
                let kind = string(&modifier, "name");
                if matches!(kind.as_str(), "font" | "font_family") {
                    counts[0] += 1;
                    let name = string(&modifier, "value");
                    counts[1] += usize::from(!name.is_empty());
                    counts[2] += usize::from(package.font(&name, false).is_some());
                }
                if matches!(kind.as_str(), "background" | "overlay") {
                    if let Ok(child) = modifier.get::<Table>("value") {
                        font_usage(&child, package, counts);
                    }
                }
            }
            for child in tables(node, "children").unwrap() {
                font_usage(&child, package, counts);
            }
        }
        let mut fonts = [0; 3];
        font_usage(&graph, &package, &mut fonts);
        println!(
            "NATIVE_FONT declarations={} string_names={} supplied_matches={}",
            fonts[0], fonts[1], fonts[2]
        );
        fn fields(node: &Table, saved: &[String], out: &mut Vec<(String, usize, usize)>) {
            let kind = string(node, "kind");
            if matches!(kind.as_str(), "Text" | "TextInput") {
                let props = node.get::<Table>("props").unwrap();
                let text = string(&props, "text");
                if let Some(n) = saved.iter().position(|s| s == &text) {
                    println!(
                        "NATIVE_TEXT_PROPS slot={n} font_size={:?} size={:?} family_declared={} font_table={} style_table={}",
                        number(&props, "font_size"),
                        number(&props, "size"),
                        !string(&props, "font_family").is_empty(),
                        matches!(val(&props, "font"), Value::Table(_)),
                        matches!(val(&props, "style"), Value::Table(_))
                    );
                    out.push((string(node, "path"), n, text.chars().count()));
                }
            }
            for child in tables(node, "children").unwrap() {
                fields(&child, saved, out);
            }
            for modifier in tables(node, "modifiers").unwrap() {
                if matches!(string(&modifier, "name").as_str(), "background" | "overlay") {
                    if let Ok(child) = modifier.get::<Table>("value") {
                        fields(&child, saved, out);
                    }
                }
            }
        }
        let mut fields_found = Vec::new();
        fields(&graph, &saved, &mut fields_found);
        assert!(
            fields_found.len() >= 6,
            "six complete saved strings reach native primitives"
        );
        let mut ui = super::super::theme::ui();
        let mut drafts = HashMap::new();
        #[cfg(target_os = "linux")]
        let watermark = stack_watermark();
        for _ in 0..4 {
            let el = draw(
                &mut ui,
                &graph,
                &package,
                &session,
                0,
                1.,
                Style::default(),
                &mut drafts,
            )
            .unwrap();
            ui.frame(el, Some(authored_size(&graph)), Input::default(), 1. / 60.)
                .unwrap();
        }
        #[cfg(target_os = "linux")]
        println!(
            "NATIVE_STACK draw_and_layout_peak_bytes={}",
            stack_peak(watermark)
        );
        let scene = ui.scene().unwrap();
        let mut matched = 0;
        for (path, n, len) in fields_found {
            if let Some(surface) = scene.surface(&format!("nui-0-{path}-text")) {
                let geometry = surface.text_geometry.as_ref().unwrap();
                let advance = geometry.lines[0].carets.x(geometry.text.len());
                println!(
                    "NATIVE_TEXT slot={n} chars={len} equals_saved={} frame_width={} viewport_width={} advance={} insets={:?}",
                    geometry.text.as_ref() == saved[n],
                    surface.frame.size.width,
                    geometry.viewport.width,
                    advance,
                    geometry.state.insets
                );
                assert!(
                    geometry.text.as_ref() == saved[n],
                    "full saved text reaches final field geometry"
                );
                assert_eq!(
                    geometry.state.insets,
                    [0., 0.],
                    "authored text fields must not retain generic editor padding"
                );
                assert!(
                    advance <= geometry.viewport.width + 0.01,
                    "fallback metrics fit the complete saved caption"
                );
                matched += 1;
            }
        }
        assert_eq!(matched, 6);
        let modifiers = graph.get::<Table>("modifiers").unwrap();
        let missing = session.lua().create_table().unwrap();
        missing.set("name", "font_family").unwrap();
        missing.set("value", "unavailable-test-font").unwrap();
        modifiers.push(missing).unwrap();
        let error = draw(
            &mut ui,
            &graph,
            &package,
            &session,
            0,
            1.,
            Style::default(),
            &mut drafts,
        )
        .err()
        .unwrap();
        assert!(failure(&error, "graph").starts_with("graph, NativeUI font service unavailable:"));
    }
}

#[cfg(all(test, feature = "shots"))]
#[test]
#[ignore = "Conflux alias ownership: resources and identifiers stay in RAM"]
fn native_alias_menus_are_legacy_skin_proxies() {
    let path = std::path::PathBuf::from(std::env::var_os("KONTRA_AUDIT_WIDGET_PATCH").unwrap());
    let mut source = sampler_kontakt::read(&path).unwrap().instrument;
    source.retain_zones(|_| false);
    source.assets.clear();
    let loaded = sampler_kontakt::prepare(
        source,
        vec![],
        &sampler_kontakt::Options {
            library: Some(path.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let package = Package::load(&path).unwrap();
    let aliases: Vec<_> = loaded
        .interfaces
        .iter()
        .flat_map(|f| &f.widgets)
        .filter(|w| matches!(w.kind, ir::Kind::Menu { .. }) && w.name.ends_with("Alias"))
        .collect();
    assert_eq!(aliases.len(), 4);
    // A NativeUI module cannot bind these names indirectly without even the
    // Alias suffix occurring in its immutable module sources.
    assert_eq!(
        package.audit_token_count("Alias"),
        0,
        "NativeUI mentions aliases; classify its actual binding before excluding them"
    );
    for (n, alias) in aliases.into_iter().enumerate() {
        let underlying = alias.name.trim_start_matches('$').trim_end_matches("Alias");
        assert!(
            package.audit_token_count(underlying) > 0,
            "native skin must bind the corresponding original selector"
        );
        println!(
            "NATIVE_ALIAS_BY_DESIGN index={n} native_alias_references=0 underlying_selector_present=true"
        );
    }
}
