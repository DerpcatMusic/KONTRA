//! Offline Falcon object/UI state. No VM or GUI work belongs on the audio thread.
//!
//! Parameter and widget signatures: https://lua.uvi.net/class_element.html,
//! https://lua.uvi.net/class_unit.html, https://lua.uvi.net/class_table.html,
//! https://lua.uvi.net/group___voice.html and https://lua.uvi.net/group___async.html.
//! XML field names were observed in privately held presets; no preset source is
//! included here. Retaining a command does not establish DSP support for it.
//! UI edit callbacks use original native probes and documented ModifierKeys:
//! https://lua.uvi.net/class_modifier_keys.html and class_button.html. Numeric/menu
//! callbacks receive the widget; Table also receives its index; OnOffButton also
//! receives immutable modifiers. Programmatic Button.push(bool) is stateless and
//! supplies only the widget; UI clicks supply documented modifiers.
//! Table boundary behavior was measured against official UVI Workstation 4.0.9 using
//! original synthetic probes (ignored writes/default reads outside 1..N).
//! Original native probes also establish Unit enum values and that parameter
//! writes with a mismatched scalar type are ignored without conversion.
//! Numeric widgets retain float32 values, truncate integers, allow programmatic
//! values beyond their display range, and notify only when a value changes.
//! The exposed Lua API revision (23) was measured with an original UVI Workstation 4.0.9
//! probe; unknown operations still fail explicitly instead of claiming support.

use super::program::{NodeId, Program};
use mlua::{
    AnyUserData, Function, Lua, MetaMethod, MultiValue, Table, UserData, UserDataMethods, Value,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, HashSet},
    rc::Rc,
};

const LIMIT: usize = 65_536;
const SOURCE_LIMIT: usize = 2 << 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParameterValue {
    Number(f64),
    Boolean(bool),
    Text(String),
}

impl ParameterValue {
    fn from_lua(value: Value) -> mlua::Result<Self> {
        match value {
            Value::Number(n) if n.is_finite() => Ok(Self::Number(n)),
            Value::Integer(n) => Ok(Self::Number(n as f64)),
            Value::Boolean(b) => Ok(Self::Boolean(b)),
            Value::String(s) => Ok(Self::Text(s.to_str()?.to_owned())),
            _ => Err(mlua::Error::runtime(
                "UVI parameters require finite numbers, booleans or strings",
            )),
        }
    }

    fn to_lua(&self, lua: &Lua) -> mlua::Result<Value> {
        Ok(match self {
            Self::Number(n) => Value::Number(*n),
            Self::Boolean(b) => Value::Boolean(*b),
            Self::Text(s) => Value::String(lua.create_string(s)?),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ResourceKind {
    Sample,
    Impulse,
}

#[derive(Debug, Clone)]
pub struct ResourceInfo {
    pub name: String,
    pub rate: u32,
    pub channels: usize,
    pub frames: usize,
}

/// Requests go only to an explicitly approved Library/private-state facility.
#[derive(Debug, Clone)]
pub enum ResourceRequest {
    ReadAudio {
        kind: ResourceKind,
        path: String,
    },
    ReadData {
        path: String,
    },
    ReadState {
        path: String,
    },
    WriteState {
        path: String,
        bytes: Vec<u8>,
    },
    Browse {
        mode: String,
        title: String,
        initial: String,
        patterns: String,
    },
}

#[derive(Debug, Clone)]
pub enum ResourceResponse {
    /// The caller must decode and retain the actual audio before returning this.
    Audio(ResourceInfo),
    Bytes(Vec<u8>),
    Saved,
    Selected(Option<String>),
}

pub type Resources = Rc<dyn Fn(&ResourceRequest) -> mlua::Result<ResourceResponse>>;

/// Context objects belong to a host owner, not the authored Program graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExternalContextTarget {
    Part,
    Synth,
}

#[derive(Debug, Clone, Serialize)]
pub enum Action {
    Parameter {
        node: NodeId,
        parameter: String,
        value: ParameterValue,
    },
    ScriptModulation {
        id: u8,
        start: Option<f64>,
        target: f64,
        ramp_ms: f64,
        voice: Option<u32>,
        /// Issuing Layer, or Program-wide when absent.
        layer: Option<NodeId>,
    },
    LoadResource {
        node: NodeId,
        kind: ResourceKind,
        path: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Command {
    pub frame: u64,
    pub action: Action,
}

pub struct HostConfig<'a> {
    pub program: Option<&'a Program>,
    /// Only sources resolved from approved resources by the caller. No disk search.
    pub modules: BTreeMap<String, Vec<u8>>,
    pub now: Rc<dyn Fn() -> u64>,
    pub resources: Option<Resources>,
    /// Runtime-issued IDs, including voices posted for a future frame.
    pub valid_voice: Option<Rc<dyn Fn(u32) -> bool>>,
    /// Read at emission so shared engine functions retain the calling scope.
    pub layer_scope: Option<Rc<dyn Fn() -> Option<NodeId>>>,
}

#[derive(Clone)]
pub struct Host {
    pub commands: Rc<RefCell<Vec<Command>>>,
    pub parameters: Rc<RefCell<Vec<BTreeMap<String, ParameterValue>>>>,
    pub(crate) baseline: Vec<BTreeMap<String, ParameterValue>>,
    pub(crate) loaded_resources: Rc<RefCell<BTreeMap<NodeId, (ResourceKind, String)>>>,
    pub(crate) objects: Table,
    identities: Rc<RefCell<HashMap<usize, NodeId>>>,
    pub(crate) types: Rc<Vec<String>>,
    pub(crate) modules: Rc<BTreeMap<String, Vec<u8>>>,
    resources: Option<Resources>,
    now: Rc<dyn Fn() -> u64>,
    task_ids: Rc<Cell<u32>>,
}

impl Host {
    /// Validate actual Lua table identity rather than a mutable script field.
    pub fn object_id(&self, object: &Table) -> mlua::Result<NodeId> {
        node_id(object, &self.identities)
    }

    /// Run every processor in its own environment while sharing one engine graph.
    /// The runtime's base globals must contain only common APIs, not user chunks.
    pub fn script_environment(
        &self,
        lua: &Lua,
        program: &Program,
        processor: NodeId,
    ) -> mlua::Result<Table> {
        if self.types.get(processor).map(String::as_str) != Some("ScriptProcessor")
            || program.nodes.get(processor).map(|n| n.kind.as_str()) != Some("ScriptProcessor")
        {
            return Err(mlua::Error::runtime("Invalid UVI ScriptProcessor scope"));
        }
        let environment = lua.create_table()?;
        let metatable = lua.create_table()?;
        metatable.set("__index", lua.globals())?;
        environment.set_metatable(Some(metatable))?;
        environment.set("_G", environment.clone())?;
        // Standard-library tables are mutable globals too; keep helper additions
        // local to their processor while reusing the native library functions.
        for name in ["table", "string", "math", "Event"] {
            if let Some(source) = lua.globals().get::<Option<Table>>(name)? {
                let local = lua.create_table()?;
                for pair in source.pairs::<Value, Value>() {
                    let (key, value) = pair?;
                    local.raw_set(key, value)?;
                }
                environment.set(name, local)?;
            }
        }
        environment.set("this", self.objects.raw_get::<Table>(processor + 1)?)?;
        environment.set("Program", self.objects.raw_get::<Table>(program.root + 1)?)?;
        install_class(lua, &environment)?;
        install_modules(lua, self.modules.clone(), &environment)?;
        install_resources(
            lua,
            self,
            self.now.clone(),
            self.resources.clone(),
            &environment,
        )?;
        install_ui(lua, &environment)?;
        Ok(environment)
    }
}

/// Owned renderer data; these types deliberately do not implement Debug because
/// labels and artwork references can belong to a private commercial instrument.
#[derive(Clone, PartialEq, Serialize)]
pub struct UiSnapshot {
    pub processor: NodeId,
    pub root: UiRoot,
    /// Constructor order is also the stable, processor-local widget identity.
    pub widgets: Vec<UiWidget>,
    /// Parent subtrees and authored child order, without changing edit identities.
    pub paint_order: Vec<u32>,
}

#[derive(Clone, PartialEq, Serialize)]
pub struct UiRoot {
    pub width: f64,
    pub height: f64,
    pub performance_view: bool,
    pub background: Option<UiArtwork>,
    pub background_colour: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Serialize)]
pub enum UiKind {
    Panel,
    Viewport,
    Label,
    Image,
    WaveView,
    AudioMeter,
    XY,
    Menu,
    Table,
    Slider,
    Knob,
    NumBox,
    Button,
    OnOffButton,
}

#[derive(Clone, Copy, PartialEq, Serialize)]
pub struct UiBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, PartialEq, Serialize)]
pub enum UiValue {
    Number(f64),
    Boolean(bool),
    Table(Vec<f64>),
}

/// A resource reference, not a filesystem path. Resolve through the owning
/// bank's Library using the processor's Program path; never open it directly.
#[derive(Clone, PartialEq, Serialize)]
pub struct UiArtwork {
    pub path: String,
    /// Native leading slash denotes the bank root, not the OS filesystem root.
    pub bank_root: bool,
}

#[derive(Clone, PartialEq, Serialize)]
pub struct UiStrip {
    pub artwork: UiArtwork,
    pub frames: u32,
    pub horizontal: bool,
}

#[derive(Clone, PartialEq, Serialize, Default)]
pub struct UiStyle {
    pub text: Option<String>,
    pub display_text: Option<String>,
    pub tooltip: Option<String>,
    pub align: Option<String>,
    pub font: Option<String>,
    pub font_size: Option<f64>,
    pub text_colour: Option<String>,
    pub background_colour: Option<String>,
    pub slider_colour: Option<String>,
    pub draw_inner_edge: Option<bool>,
    pub inner_edge_colour: Option<String>,
    pub show_label: Option<bool>,
    pub show_value: Option<bool>,
    pub show_popup_display: Option<bool>,
    pub unit: Option<f64>,
    pub mapper: Option<f64>,
    pub hierarchical: Option<bool>,
    pub background_image: Option<UiArtwork>,
    pub image: Option<UiArtwork>,
    pub normal_image: Option<UiArtwork>,
    pub pressed_image: Option<UiArtwork>,
    pub over_image: Option<UiArtwork>,
    pub strip_image: Option<UiStrip>,
}

#[derive(Clone, PartialEq, Serialize)]
pub struct UiWidget {
    pub id: u32,
    pub parent: Option<u32>,
    pub kind: UiKind,
    pub name: String,
    pub display_name: Option<String>,
    pub bounds: UiBounds,
    pub absolute_bounds: UiBounds,
    pub visible: bool,
    pub effective_visible: bool,
    pub enabled: bool,
    pub alpha: f64,
    pub effective_alpha: f64,
    pub value: Option<UiValue>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub integer: bool,
    pub items: Vec<String>,
    pub param_x: Option<String>,
    pub param_y: Option<String>,
    pub has_changed_callback: bool,
    pub style: UiStyle,
}

const UI_WIDGET_LIMIT: usize = 4096;
const UI_ITEM_LIMIT: usize = 131_072;
const UI_STRING_LIMIT: usize = 4096;
const UI_TOTAL_STRING_LIMIT: usize = 1 << 20;
const UI_EXTENT_LIMIT: f64 = 16_384.0;
const UI_POSITION_LIMIT: f64 = 1_048_576.0;

fn ui_error(message: &'static str) -> mlua::Error {
    mlua::Error::runtime(message)
}

#[derive(Default)]
struct UiBudget {
    strings: usize,
    items: usize,
}
impl UiBudget {
    fn string(&mut self, table: &Table, key: impl mlua::IntoLua) -> mlua::Result<Option<String>> {
        match table.raw_get::<Value>(key)? {
            Value::Nil => Ok(None),
            Value::String(value) => {
                let value = value.to_str()?;
                self.strings += value.len();
                if value.len() > UI_STRING_LIMIT
                    || self.strings > UI_TOTAL_STRING_LIMIT
                    || value.contains('\0')
                {
                    return Err(ui_error("UVI UI snapshot text limit exceeded"));
                }
                Ok(Some(value.to_owned()))
            }
            _ => Err(ui_error("Invalid UVI UI snapshot text field")),
        }
    }
    fn artwork(
        &mut self,
        table: &Table,
        key: impl mlua::IntoLua,
    ) -> mlua::Result<Option<UiArtwork>> {
        let Some(path) = self.string(table, key)? else {
            return Ok(None);
        };
        if path.is_empty() {
            return Ok(None);
        }
        let path = path.replace('\\', "/");
        if path.contains(':') || path.starts_with("//") || path.chars().any(char::is_control) {
            return Err(ui_error("Invalid UVI UI bank artwork reference"));
        }
        Ok(Some(UiArtwork {
            bank_root: path.starts_with('/'),
            path,
        }))
    }
    fn sequence(&mut self, table: &Table, limit: usize) -> mlua::Result<usize> {
        let count = table.raw_len();
        if count > limit || self.items.saturating_add(count) > UI_ITEM_LIMIT {
            return Err(ui_error("UVI UI snapshot item limit exceeded"));
        }
        let mut entries = 0;
        for pair in table.pairs::<Value, Value>() {
            let (key, _) = pair?;
            let index = match key {
                Value::Integer(i) => i as f64,
                Value::Number(n) => n,
                _ => 0.0,
            };
            if !index.is_finite() || index.fract() != 0.0 || index < 1.0 || index > count as f64 {
                return Err(ui_error("Invalid UVI UI snapshot sequence"));
            }
            entries += 1;
        }
        if entries != count {
            return Err(ui_error("Sparse UVI UI snapshot sequence"));
        }
        self.items += count;
        Ok(count)
    }
}

fn ui_number(table: &Table, key: impl mlua::IntoLua) -> mlua::Result<Option<f64>> {
    match table.raw_get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::Number(n) if n.is_finite() => Ok(Some(n)),
        Value::Integer(n) => Ok(Some(n as f64)),
        _ => Err(ui_error("Invalid UVI UI snapshot numeric field")),
    }
}
fn ui_bool(table: &Table, key: &str, default: bool) -> mlua::Result<bool> {
    match table.raw_get::<Value>(key)? {
        Value::Nil => Ok(default),
        Value::Boolean(b) => Ok(b),
        _ => Err(ui_error("Invalid UVI UI snapshot Boolean field")),
    }
}
fn ui_optional_bool(table: &Table, key: &str) -> mlua::Result<Option<bool>> {
    if table.raw_get::<Value>(key)?.is_nil() {
        Ok(None)
    } else {
        ui_bool(table, key, false).map(Some)
    }
}
fn ui_extent(table: &Table, key: &str) -> mlua::Result<f64> {
    let n = ui_number(table, key)?.unwrap_or(0.0);
    if !(0.0..=UI_EXTENT_LIMIT).contains(&n) {
        return Err(ui_error("UVI UI snapshot extent exceeds render bounds"));
    }
    Ok(n)
}
fn ui_position(table: &Table, key: &str) -> mlua::Result<f64> {
    let n = ui_number(table, key)?.unwrap_or(0.0);
    if n.abs() > UI_POSITION_LIMIT {
        return Err(ui_error("UVI UI snapshot position exceeds render bounds"));
    }
    Ok(n)
}

/// Read raw host state only: no widget getter, metamethod, callback, or script is
/// invoked. Errors contain fixed diagnostics rather than instrument strings.
/// Snapshot limits bound owned allocations independently of the Lua heap cap.
pub fn snapshot_ui(processor: NodeId, environment: &Table) -> mlua::Result<UiSnapshot> {
    let ui = environment.raw_get::<Table>("UVI_UI_STATE")?;
    let root = ui.raw_get::<Table>("root")?;
    let order = ui.raw_get::<Table>("order")?;
    let mut budget = UiBudget::default();
    let root = UiRoot {
        width: ui_extent(&root, "width")?,
        height: ui_extent(&root, "height")?,
        performance_view: ui_bool(&root, "performanceView", false)?,
        background: budget.artwork(&root, "background")?,
        background_colour: budget.string(&root, "backgroundColour")?,
    };
    let count = budget.sequence(&order, UI_WIDGET_LIMIT)?;
    let mut identities = HashMap::with_capacity(count);
    let mut parents = Vec::with_capacity(count);
    let mut child_pointers = Vec::with_capacity(count);
    let mut widgets = Vec::with_capacity(count);
    for index in 1..=count {
        let widget = order.raw_get::<Table>(index)?;
        if identities
            .insert(widget.to_pointer() as usize, index as u32)
            .is_some()
        {
            return Err(ui_error("Duplicate UVI UI snapshot widget identity"));
        }
        let state = widget.raw_get::<Table>("_state")?;
        parents.push(match state.raw_get::<Value>("parent")? {
            Value::Nil => None,
            Value::Table(t) => Some(t.to_pointer() as usize),
            _ => return Err(ui_error("Invalid UVI UI snapshot parent")),
        });
        let mut children = Vec::new();
        match state.raw_get::<Value>("children")? {
            Value::Nil => {}
            Value::Table(source) => {
                for i in 1..=budget.sequence(&source, UI_WIDGET_LIMIT)? {
                    let Value::Table(child) = source.raw_get::<Value>(i)? else {
                        return Err(ui_error("Invalid UVI UI snapshot child reference"));
                    };
                    children.push(child.to_pointer() as usize);
                }
            }
            _ => return Err(ui_error("Invalid UVI UI snapshot child list")),
        }
        child_pointers.push(children);
        let kind = match state.raw_get::<Value>("kind")? {
            Value::String(s) => match s.to_str()?.as_ref() {
                "Panel" => UiKind::Panel,
                "Viewport" => UiKind::Viewport,
                "Label" => UiKind::Label,
                "Image" => UiKind::Image,
                "WaveView" => UiKind::WaveView,
                "AudioMeter" => UiKind::AudioMeter,
                "XY" => UiKind::XY,
                "Menu" => UiKind::Menu,
                "Table" => UiKind::Table,
                "Slider" => UiKind::Slider,
                "Knob" => UiKind::Knob,
                "NumBox" => UiKind::NumBox,
                "Button" => UiKind::Button,
                "OnOffButton" => UiKind::OnOffButton,
                _ => return Err(ui_error("Unsupported UVI UI snapshot widget kind")),
            },
            _ => return Err(ui_error("Invalid UVI UI snapshot widget kind")),
        };
        let mut items = Vec::new();
        if kind == UiKind::Menu {
            let source = state.raw_get::<Table>("items")?;
            for i in 1..=budget.sequence(&source, UI_WIDGET_LIMIT)? {
                // Reuse the bounded string reader without accessing metamethods.
                items.push(
                    budget
                        .string(&source, i)?
                        .ok_or_else(|| ui_error("Invalid UVI UI menu item"))?,
                );
            }
        }
        let value = if kind == UiKind::Table {
            let source = state.raw_get::<Table>("values")?;
            let mut values = Vec::new();
            for i in 1..=budget.sequence(&source, 65_536)? {
                values.push(
                    ui_number(&source, i)?.ok_or_else(|| ui_error("Invalid UVI UI Table value"))?,
                );
            }
            Some(UiValue::Table(values))
        } else {
            match state.raw_get::<Value>("value")? {
                Value::Nil => None,
                Value::Boolean(b) => Some(UiValue::Boolean(b)),
                Value::Number(n) if n.is_finite() => Some(UiValue::Number(n)),
                Value::Integer(n) => Some(UiValue::Number(n as f64)),
                _ => return Err(ui_error("Invalid UVI UI snapshot control value")),
            }
        };
        let strip_image = match state.raw_get::<Value>("stripImage")? {
            Value::Nil => None,
            Value::Table(strip) => {
                budget.sequence(&strip, 3)?;
                let artwork = budget
                    .artwork(&strip, 1)?
                    .ok_or_else(|| ui_error("Missing UVI UI sprite artwork"))?;
                let frames =
                    ui_number(&strip, 2)?.ok_or_else(|| ui_error("Missing UVI UI sprite count"))?;
                if frames.fract() != 0.0 || !(1.0..=4096.0).contains(&frames) {
                    return Err(ui_error("Invalid UVI UI sprite count"));
                }
                let horizontal = match strip.raw_get::<Value>(3)? {
                    Value::Nil => false,
                    Value::Boolean(b) => b,
                    _ => return Err(ui_error("Invalid UVI UI sprite orientation")),
                };
                Some(UiStrip {
                    artwork,
                    frames: frames as u32,
                    horizontal,
                })
            }
            _ => return Err(ui_error("Invalid UVI UI sprite strip")),
        };
        let font_size = ui_number(&state, "fontSize")?;
        if font_size.is_some_and(|size| !(0.0..=512.0).contains(&size)) {
            return Err(ui_error("UVI UI snapshot font size exceeds render bounds"));
        }
        let style = UiStyle {
            text: budget.string(&state, "text")?,
            display_text: budget.string(&state, "displayText")?,
            tooltip: budget.string(&state, "tooltip")?,
            align: budget.string(&state, "align")?,
            font: budget.string(&state, "font")?,
            font_size,
            text_colour: budget.string(&state, "textColour")?,
            background_colour: budget.string(&state, "backgroundColour")?,
            slider_colour: budget.string(&state, "sliderColour")?,
            draw_inner_edge: ui_optional_bool(&state, "drawInnerEdge")?,
            inner_edge_colour: budget.string(&state, "innerEdgeColour")?,
            show_label: ui_optional_bool(&state, "showLabel")?,
            show_value: ui_optional_bool(&state, "showValue")?,
            show_popup_display: ui_optional_bool(&state, "showPopupDisplay")?,
            unit: ui_number(&state, "unit")?,
            mapper: ui_number(&state, "mapper")?,
            hierarchical: ui_optional_bool(&state, "hierarchical")?,
            background_image: budget.artwork(&state, "backgroundImage")?,
            image: budget.artwork(&state, "image")?,
            normal_image: budget.artwork(&state, "normalImage")?,
            pressed_image: budget.artwork(&state, "pressedImage")?,
            over_image: budget.artwork(&state, "overImage")?,
            strip_image,
        };
        let alpha = ui_number(&state, "alpha")?.unwrap_or(1.0);
        if !(0.0..=1.0).contains(&alpha) {
            return Err(ui_error("Invalid UVI UI snapshot opacity"));
        }
        widgets.push(UiWidget {
            id: index as u32,
            parent: None,
            kind,
            name: budget
                .string(&state, "name")?
                .ok_or_else(|| ui_error("Missing UVI UI widget name"))?,
            display_name: budget.string(&state, "displayName")?,
            bounds: UiBounds {
                x: ui_position(&state, "x")?,
                y: ui_position(&state, "y")?,
                width: ui_extent(&state, "width")?,
                height: ui_extent(&state, "height")?,
            },
            absolute_bounds: UiBounds {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            visible: ui_bool(&state, "visible", true)?,
            effective_visible: false,
            enabled: ui_bool(&state, "enabled", true)?,
            alpha,
            effective_alpha: 0.0,
            value,
            min: ui_number(&state, "min")?,
            max: ui_number(&state, "max")?,
            integer: ui_bool(&state, "integer", false)?,
            items,
            param_x: budget.string(&state, "paramX")?,
            param_y: budget.string(&state, "paramY")?,
            has_changed_callback: matches!(state.raw_get::<Value>("changed")?, Value::Function(_)),
            style,
        });
    }
    for (widget, parent) in widgets.iter_mut().zip(parents) {
        widget.parent = parent
            .map(|pointer| {
                identities
                    .get(&pointer)
                    .copied()
                    .ok_or_else(|| ui_error("UVI UI snapshot parent is outside processor scope"))
            })
            .transpose()?;
    }
    // Iterative traversal also handles a parent constructed after its child.
    let mut visited = vec![0u8; count];
    for index in 0..count {
        if visited[index] == 2 {
            continue;
        }
        let mut chain = Vec::new();
        let mut next = Some(index);
        while let Some(i) = next {
            if visited[i] == 2 {
                break;
            }
            if visited[i] == 1 {
                return Err(ui_error("Cyclic UVI UI snapshot parent tree"));
            }
            visited[i] = 1;
            chain.push(i);
            next = widgets[i].parent.map(|p| p as usize - 1);
        }
        for i in chain.into_iter().rev() {
            let (visible, alpha) = widgets[i]
                .parent
                .map(|p| {
                    let w = &widgets[p as usize - 1];
                    (w.effective_visible, w.effective_alpha)
                })
                .unwrap_or((true, 1.0));
            let (px, py) = widgets[i]
                .parent
                .map(|p| {
                    let bounds = &widgets[p as usize - 1].absolute_bounds;
                    (bounds.x, bounds.y)
                })
                .unwrap_or((0.0, 0.0));
            let local = widgets[i].bounds;
            let (x, y) = (px + local.x, py + local.y);
            if x.abs() > UI_POSITION_LIMIT || y.abs() > UI_POSITION_LIMIT {
                return Err(ui_error(
                    "UVI UI snapshot ancestor position exceeds render bounds",
                ));
            }
            widgets[i].absolute_bounds = UiBounds {
                x,
                y,
                width: local.width,
                height: local.height,
            };
            widgets[i].effective_visible = visible && widgets[i].visible;
            widgets[i].effective_alpha = alpha * widgets[i].alpha;
            visited[i] = 2;
        }
    }
    let mut derived = vec![Vec::new(); count];
    for widget in &widgets {
        if let Some(parent) = widget.parent {
            derived[parent as usize - 1].push(widget.id);
        }
    }
    let mut children = Vec::with_capacity(count);
    for (index, pointers) in child_pointers.into_iter().enumerate() {
        let mut seen = HashSet::with_capacity(pointers.len());
        let mut ordered = Vec::new();
        for pointer in pointers {
            let child = identities.get(&pointer).copied().ok_or_else(||
                ui_error("UVI UI snapshot child is outside processor scope"))?;
            if child as usize == index + 1 || !seen.insert(child) {
                return Err(ui_error("Duplicate or self UVI UI snapshot child reference"));
            }
            // Manual reparenting can leave stale constructor child lists.
            // The validated current parent is authoritative; retain the raw
            // sibling order for references still belonging to this container.
            if widgets[child as usize - 1].parent == Some(index as u32 + 1) {
                ordered.push(child);
            }
        }
        ordered.extend(derived[index].iter().copied().filter(|id| !seen.contains(id)));
        children.push(ordered);
    }
    let mut pending: Vec<_> = widgets.iter().enumerate()
        .filter_map(|(index, w)| w.parent.is_none().then_some(index)).rev().collect();
    let mut paint_order = Vec::with_capacity(count);
    while let Some(index) = pending.pop() {
        paint_order.push(widgets[index].id);
        pending.extend(children[index].iter().rev().map(|id| *id as usize - 1));
    }
    debug_assert_eq!(paint_order.len(), count);
    Ok(UiSnapshot {
        processor,
        root,
        widgets,
        paint_order,
    })
}

/// A control-thread request, never a Lua handle. Visibility/enabled/range checks
/// are GUI admission rules; they do not restrict native programmatic setters.
#[derive(Clone, Copy, PartialEq, Serialize)]
pub struct UiEdit {
    pub processor: NodeId,
    pub widget: u32,
    pub value: UiEditValue,
    pub modifiers: UiModifiers,
}

#[derive(Clone, Copy, PartialEq, Serialize)]
pub enum UiEditValue {
    Number(f64),
    Boolean(bool),
    TableCell { index: u32, value: f64 },
    Push,
}

/// Documented ModifierKeys fields, also observed in original native probes.
#[derive(Clone, Copy, Default, PartialEq, Serialize)]
pub struct UiModifiers {
    pub alt_down: bool,
    pub command_down: bool,
    pub shift_down: bool,
}

impl UserData for UiModifiers {
    fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("altDown", |_, this| Ok(this.alt_down));
        fields.add_field_method_get("commandDown", |_, this| Ok(this.command_down));
        fields.add_field_method_get("shiftDown", |_, this| Ok(this.shift_down));
    }
}

const UI_SETTERS: &str = "kontra.uvi.private-ui-setters";

/// Validate without changing values, time, or outputs. The returned trusted
/// setter must run on the VM's scheduler: changed callbacks can yield.
/// Revalidate after advancing pending work, since callbacks can alter the UI.
pub fn prepare_ui_edit(
    lua: &Lua,
    environment: &Table,
    edit: &UiEdit,
) -> mlua::Result<(Function, MultiValue)> {
    let snapshot = snapshot_ui(edit.processor, environment)?;
    let index = edit
        .widget
        .checked_sub(1)
        .ok_or_else(|| ui_error("Invalid UVI UI widget identity"))? as usize;
    let widget = snapshot
        .widgets
        .get(index)
        .ok_or_else(|| ui_error("Invalid UVI UI widget identity"))?;
    if !widget.effective_visible {
        return Err(ui_error("UVI UI widget is hidden"));
    }
    let mut ancestor = Some(widget);
    while let Some(w) = ancestor {
        if !w.enabled {
            return Err(ui_error("UVI UI widget is disabled"));
        }
        ancestor = w.parent.map(|id| &snapshot.widgets[id as usize - 1]);
    }
    let number = |value: f64| -> mlua::Result<()> {
        if !value.is_finite()
            || !(value as f32).is_finite()
            || widget.integer && value.fract() != 0.0
        {
            return Err(ui_error("Invalid UVI UI numeric edit"));
        }
        let (min, max) = widget
            .min
            .zip(widget.max)
            .ok_or_else(|| ui_error("Missing UVI UI editable range"))?;
        if min > max || value < min || value > max {
            return Err(ui_error("UVI UI edit is outside control range"));
        }
        Ok(())
    };
    let (a, b) = match (&edit.value, widget.kind) {
        (UiEditValue::Number(n), UiKind::Knob | UiKind::Slider | UiKind::NumBox | UiKind::Menu) => {
            number(*n)?;
            if widget.kind == UiKind::Menu
                && (n.fract() != 0.0 || *n < 1.0 || *n > widget.items.len() as f64)
            {
                return Err(ui_error("Invalid UVI UI menu selection"));
            }
            (Value::Number(*n), Value::Nil)
        }
        (UiEditValue::Boolean(value), UiKind::OnOffButton) => (Value::Boolean(*value), Value::Nil),
        (UiEditValue::TableCell { index, value }, UiKind::Table) => {
            let count = match &widget.value {
                Some(UiValue::Table(values)) => values.len(),
                _ => 0,
            };
            if *index == 0 || *index as usize > count {
                return Err(ui_error("Invalid UVI UI Table cell"));
            }
            number(*value)?;
            (Value::Integer(*index as i64), Value::Number(*value))
        }
        (UiEditValue::Push, UiKind::Button) => (Value::Nil, Value::Nil),
        _ => return Err(ui_error("UVI UI edit type does not match widget kind")),
    };
    let setters = lua.named_registry_value::<Table>(UI_SETTERS)?;
    let setter = setters.raw_get::<Function>(environment.clone())?;
    let ui = environment.raw_get::<Table>("UVI_UI_STATE")?;
    let order = ui.raw_get::<Table>("order")?;
    let object = order.raw_get::<Table>(edit.widget)?;
    Ok((
        setter,
        MultiValue::from_vec(vec![
            Value::Table(object),
            a,
            b,
            Value::UserData(lua.create_userdata(edit.modifiers)?),
        ]),
    ))
}

fn emit(
    commands: &RefCell<Vec<Command>>,
    now: &dyn Fn() -> u64,
    action: Action,
) -> mlua::Result<()> {
    let mut commands = commands.borrow_mut();
    if commands.len() >= LIMIT {
        return Err(mlua::Error::runtime("UVI host command limit exceeded"));
    }
    commands.push(Command {
        frame: now(),
        action,
    });
    Ok(())
}

fn attribute(name: &str, value: &str) -> ParameterValue {
    if matches!(
        name,
        "Name" | "DisplayName" | "SamplePath" | "OutputName" | "Source" | "Destination" | "Mapper"
    ) || name.ends_with("Path")
    {
        return ParameterValue::Text(value.to_owned());
    }
    // XML encodes these documented Boolean parameters as 0/1.
    let boolean = matches!(
        name,
        "Bypass"
            | "BypassInsertFX"
            | "Bipolar"
            | "Inverted"
            | "SyncToHost"
            | "NormalizePower"
            | "Enabled"
            | "Reverse"
            | "SamplePurged"
            | "AllowStreaming"
            | "Streaming"
            | "Mute"
            | "Solo"
            | "MidiMute"
            | "PreFader"
            | "PreInsert"
    ) || name
        .strip_prefix("Enabled")
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
    if boolean && matches!(value, "0" | "1") {
        return ParameterValue::Boolean(value == "1");
    }
    value
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .map_or_else(
            || ParameterValue::Text(value.to_owned()),
            ParameterValue::Number,
        )
}

fn collections() -> [(&'static str, &'static str); 10] {
    [
        ("layers", "Layers"),
        ("keygroups", "Keygroups"),
        ("oscillators", "Oscillators"),
        ("inserts", "Inserts"),
        ("auxs", "Auxs"),
        ("modulations", "ControlSignalSources"),
        ("eventProcessors", "EventProcessors"),
        ("sends", "BusRouters"),
        ("mappers", "Mappers"),
        ("chains", "Chains"),
    ]
}

// Only this measured absent EffectRack property family has compatibility
// setter behavior. Descriptor absence does not create a getter or state slot.
fn rack_gain_coefficient(name: &str) -> bool {
    name.strip_prefix("Gain_")
        .and_then(|indices| indices.split_once('_'))
        .is_some_and(|(input, output)| {
            [input, output].into_iter().all(|index| {
                matches!(index.as_bytes(), [b'1'..=b'9'] | [b'1', b'0'..=b'2'])
            })
        })
}

/// Must be installed before running the instrument's source.
pub(crate) fn source_parameters(program: &Program) -> Vec<BTreeMap<String, ParameterValue>> {
    let wrappers = collections().map(|(_, xml)| xml);
    program
        .nodes
        .iter()
        .map(|node| {
            let mut values: BTreeMap<_, _> = node
                .attributes
                .iter()
                .map(|(name, value)| (name.clone(), attribute(name, value)))
                .collect();
            // Original Workstation 4.0.9 authored missing-attribute probes:
            // these getters are linear Gain=1 and Pan=0. SamplePlayer has
            // Gain, but native hasParameter('Pan') is false (getter is nil).
            if matches!(
                node.kind.as_str(),
                "Program" | "Layer" | "Keygroup" | "SamplePlayer"
            ) {
                values
                    .entry("Gain".into())
                    .or_insert(ParameterValue::Number(1.));
            }
            if matches!(node.kind.as_str(), "Program" | "Layer" | "Keygroup") {
                values
                    .entry("Pan".into())
                    .or_insert(ParameterValue::Number(0.));
            }
            if !wrappers.contains(&node.kind.as_str()) && node.kind != "Connections" {
                values
                    .entry("Bypass".into())
                    .or_insert(ParameterValue::Boolean(false));
            }
            values
        })
        .collect()
}

pub fn install(lua: &Lua, config: HostConfig<'_>) -> mlua::Result<Host> {
    let HostConfig {
        program,
        modules,
        now,
        resources,
        valid_voice,
        layer_scope,
    } = config;
    lua.globals().set("__API_VERSION__", 23)?;
    let baseline = program.map_or_else(Vec::new, source_parameters);
    let state = Rc::new(RefCell::new(baseline.clone()));
    let host = Host {
        parameters: state.clone(),
        baseline,
        loaded_resources: Rc::new(RefCell::new(BTreeMap::new())),
        commands: Rc::new(RefCell::new(Vec::new())),
        objects: lua.create_table()?,
        identities: Rc::new(RefCell::new(HashMap::new())),
        types: Rc::new(program.map_or_else(Vec::new, |p| {
            p.nodes
                .iter()
                .map(|n| n.kind.clone())
                .chain(["Part".to_owned(), "Synth".to_owned()])
                .collect()
        })),
        modules: Rc::new(modules),
        resources: resources.clone(),
        now: now.clone(),
        task_ids: Rc::new(Cell::new(0)),
    };
    if let Some(program) = program {
        // One Lua-managed inventory retains every node, including XML wrappers.
        // Keeping one Rust Table per node exhausts MLua's auxiliary reference stack.
        let objects = host.objects.clone();
        for id in 0..program.nodes.len() {
            let object = lua.create_table()?;
            host.identities
                .borrow_mut()
                .insert(object.to_pointer() as usize, id);
            objects.raw_set(id + 1, object)?;
        }
        // Parent/children refer to semantic nodes, skipping XML collection wrappers.
        let wrappers = collections().map(|(_, xml)| xml);
        let mut children_by_node = vec![Vec::new(); program.nodes.len()];
        for (id, node) in program.nodes.iter().enumerate() {
            if let Some(parent) = node.parent {
                children_by_node[parent].push(id);
            }
        }
        let mut connections_by_owner = vec![Vec::new(); program.nodes.len()];
        for connection in &program.connections {
            connections_by_owner[connection.owner]
                .push((connection.destination.clone(), connection.node));
        }
        let methods = lua.create_table()?;
        let params = state.clone();
        let ids = host.identities.clone();
        methods.set(
            "getParameter",
            lua.create_function(move |lua, (object, name): (Table, String)| {
                let id = node_id(&object, &ids)?;
                params.borrow()[id]
                    .get(&name)
                    .ok_or_else(|| {
                        mlua::Error::runtime(format!(
                            "Unknown or unretained UVI parameter {name} on node {id}"
                        ))
                    })?
                    .to_lua(lua)
            })?,
        )?;
        let params = state.clone();
        let ids = host.identities.clone();
        methods.set(
            "hasParameter",
            lua.create_function(move |_, (object, name): (Table, String)| {
                let id = node_id(&object, &ids)?;
                Ok(params.borrow()[id].contains_key(&name))
            })?,
        )?;
        let params = state.clone();
        let commands = host.commands.clone();
        let clock = now.clone();
        let ids = host.identities.clone();
        let types = host.types.clone();
        let graph_nodes = host.baseline.len();
        methods.set(
            "setParameter",
            lua.create_function(move |_, (object, name, value): (Table, String, Value)| {
                let id = node_id(&object, &ids)?;
                let value = ParameterValue::from_lua(value)?;
                let mut params = params.borrow_mut();
                if !params[id].contains_key(&name)
                    && types.get(id).map(String::as_str) == Some("EffectRack")
                    && matches!(value, ParameterValue::Number(_))
                    && rack_gain_coefficient(&name)
                {
                    // Original Rack registration has no Gain_i_j descriptor;
                    // original numeric setter returns without changing state.
                    return Ok(());
                }
                let old = params[id].get(&name).ok_or_else(|| {
                    mlua::Error::runtime(format!(
                        "Unknown or unretained UVI parameter {name} on node {id}"
                    ))
                })?;
                if std::mem::discriminant(old) != std::mem::discriminant(&value) {
                    return Ok(());
                }
                // install_context owns the two identities after the graph.
                let context = match id.checked_sub(graph_nodes) {
                    Some(0) => Some(ExternalContextTarget::Part),
                    Some(1) => Some(ExternalContextTarget::Synth),
                    _ => None,
                };
                if let Some(target) = context {
                    return Err(mlua::Error::runtime(format!(
                        "Unowned UVI {target:?} context parameter {name}: no host routing/audio owner is installed"
                    )));
                }
                emit(
                    &commands,
                    &*clock,
                    Action::Parameter {
                        node: id,
                        parameter: name.clone(),
                        value: value.clone(),
                    },
                )?;
                params[id].insert(name, value);
                Ok(())
            })?,
        )?;
        let targets = connections_by_owner;
        let connection_objects = objects.clone();
        let ids = host.identities.clone();
        methods.set(
            "getParameterConnections",
            lua.create_function(move |lua, (object, name): (Table, String)| {
                let id = node_id(&object, &ids)?;
                let list = lua.create_table()?;
                for (i, (_, target)) in targets[id]
                    .iter()
                    .filter(|(destination, _)| destination == &name)
                    .enumerate()
                {
                    list.set(i + 1, connection_objects.raw_get::<Table>(*target + 1)?)?;
                }
                Ok(list)
            })?,
        )?;
        let object_metatable = lua.create_table()?;
        object_metatable.set("__index", methods)?;
        for (id, node) in program.nodes.iter().enumerate() {
            let object = objects.raw_get::<Table>(id + 1)?;
            object.set("_nodeId", id)?;
            object.set("type", node.kind.as_str())?;
            object.set("name", node.name.as_deref().unwrap_or(node.kind.as_str()))?;
            object.set(
                "displayName",
                node.attributes
                    .get("DisplayName")
                    .or(node.name.as_ref())
                    .map(String::as_str)
                    .unwrap_or(node.kind.as_str()),
            )?;
            let mut parent = node.parent;
            while parent.is_some_and(|p| {
                wrappers.contains(&program.nodes[p].kind.as_str())
                    || program.nodes[p].kind == "Connections"
            }) {
                parent = parent.and_then(|p| program.nodes[p].parent);
            }
            if let Some(parent) = parent {
                object.set("parent", objects.raw_get::<Table>(parent + 1)?)?;
            }
            let children = lua.create_table()?;
            let synth_children = lua.create_table()?;
            let mut child_index = 0;
            for (field, wrapper) in collections() {
                if !matches!(
                    node.kind.as_str(),
                    "Program" | "Layer" | "Keygroup" | "AuxEffect" | "EffectRack"
                ) && !children_by_node[id]
                    .iter()
                    .any(|&child| program.nodes[child].kind == wrapper)
                {
                    continue;
                }
                let list = lua.create_table()?;
                // Only modulations expose a separate named collection. Other
                // fields expose their ordered list, so no unused table is needed.
                let named = (field == "modulations").then(|| lua.create_table()).transpose()?;
                let mut index = 0;
                for &wrapper_id in children_by_node[id].iter().filter(|&&child| {
                    program.nodes[child].kind == wrapper
                        || (field == "auxs"
                            && node.kind == "EffectRack"
                            && program.nodes[child].kind == "Chains")
                }) {
                    for &child_id in &children_by_node[wrapper_id] {
                        let child = &program.nodes[child_id];
                        index += 1;
                        list.set(index, objects.raw_get::<Table>(child_id + 1)?)?;
                        if let Some(name) = &child.name {
                            children.set(name.as_str(), objects.raw_get::<Table>(child_id + 1)?)?;
                            if let Some(named) = &named {
                                named
                                    .set(name.as_str(), objects.raw_get::<Table>(child_id + 1)?)?;
                            }
                        }
                        if matches!(field, "layers" | "keygroups") {
                            child_index += 1;
                            synth_children
                                .set(child_index, objects.raw_get::<Table>(child_id + 1)?)?;
                        }
                    }
                }
                if let Some(named) = named {
                    object.set("mods", list.clone())?;
                    object.set(field, named)?;
                } else {
                    object.set(field, list)?;
                }
            }
            object.set("children", children)?;
            if matches!(node.kind.as_str(), "Program" | "Layer" | "Keygroup") {
                object.set("synthChildren", synth_children)?;
            }
            // Attributes are authoritative. The only omitted default supplied here
            // is the documented common processor Bypass=false.
            if !wrappers.contains(&node.kind.as_str()) && node.kind != "Connections" {
                state.borrow_mut()[id]
                    .entry("Bypass".into())
                    .or_insert(ParameterValue::Boolean(false));
            }
            object.set_metatable(Some(object_metatable.clone()))?;
        }
        install_context(lua, &objects.raw_get::<Table>(program.root + 1)?, &host)?;
        if let Some((id, _)) = program
            .nodes
            .iter()
            .enumerate()
            .find(|(_, n)| n.kind == "ScriptProcessor")
        {
            lua.globals()
                .set("this", objects.raw_get::<Table>(id + 1)?)?;
        }
        let layer_names = program
            .layers
            .iter()
            .map(|&id| {
                program.nodes[id]
                    .attributes
                    .get("DisplayName")
                    .or(program.nodes[id].name.as_ref())
                    .cloned()
            })
            .collect::<Vec<_>>();
        lua.globals().set(
            "findLayer",
            lua.create_function(move |_, name: String| {
                Ok(layer_names
                    .iter()
                    .position(|n| n.as_ref() == Some(&name))
                    .map(|index| index + 1))
            })?,
        )?;
        lua.globals()
            .set("Program", objects.raw_get::<Table>(program.root + 1)?)?;
    }
    install_class(lua, &lua.globals())?;
    install_modules(lua, host.modules.clone(), &lua.globals())?;
    install_modulation(lua, &host, now.clone(), valid_voice, layer_scope)?;
    install_resources(lua, &host, now, resources, &lua.globals())?;
    install_ui(lua, &lua.globals())?;
    Ok(host)
}

fn install_ui(lua: &Lua, environment: &Table) -> mlua::Result<()> {
    environment.set(
        "_uvi_float",
        lua.create_function(|_, value: f64| {
            let value = value as f32;
            if !value.is_finite() {
                return Err(mlua::Error::runtime(
                    "UVI widget value exceeds float32 range",
                ));
            }
            Ok(f64::from(value))
        })?,
    )?;
    environment.raw_set(
        "_uvi_modifiers",
        lua.create_function(|lua, ()| lua.create_userdata(UiModifiers::default()))?,
    )?;
    let setter = lua
        .load(UI)
        .set_name("UVI offline UI state")
        .set_environment(environment.clone())
        .eval::<Function>()?;
    let setters = match lua.named_registry_value::<Option<Table>>(UI_SETTERS)? {
        Some(table) => table,
        None => {
            let table = lua.create_table()?;
            lua.set_named_registry_value(UI_SETTERS, table.clone())?;
            table
        }
    };
    setters.raw_set(environment.clone(), setter)
}

fn node_id(object: &Table, identities: &RefCell<HashMap<usize, NodeId>>) -> mlua::Result<NodeId> {
    identities
        .borrow()
        .get(&(object.to_pointer() as usize))
        .copied()
        .ok_or_else(|| mlua::Error::runtime("Table is not a UVI Program object"))
}

// Only native parameter widgets have persistent values. Stateless Button and
// containers/labels/images may appear beside them, including stale scalar XML.
fn persistent_parameter_widget(kind: &str) -> bool {
    matches!(
        kind,
        "Table" | "Knob" | "Slider" | "NumBox" | "Menu" | "OnOffButton"
    )
}

/// Restore observed ScriptProcessor scalars and ScriptData Table cells after
/// constructors execute. Native restoration visits Tables first, then scalar
/// controls, in constructor order, notifying immediately after each changed cell.
pub fn restore_widgets(lua: &Lua, program: &Program) -> mlua::Result<usize> {
    restore_widgets_in(lua, program, None, &lua.globals())
}

pub fn restore_widgets_scoped(
    lua: &Lua,
    program: &Program,
    processor: NodeId,
    environment: &Table,
) -> mlua::Result<usize> {
    restore_widgets_in(lua, program, Some(processor), environment)
}

pub(crate) fn restore_saved_widgets(
    lua: &Lua,
    program: &Program,
    environment: &Table,
) -> mlua::Result<usize> {
    // Unlike embedded preset state, this bundle promises a captured widget set.
    // Native widgets belong to the main script chunk. Reject absent controls
    // rather than silently accepting a restore that leaves defaults intact.
    let mut scalars = HashSet::new();
    let mut tables = HashSet::new();
    for widget in environment
        .get::<Table>("UVI_UI_STATE")?
        .get::<Table>("order")?
        .sequence_values::<Table>()
    {
        let widget = widget?;
        let kind = widget.get::<String>("kind")?;
        if persistent_parameter_widget(&kind) && widget.get::<bool>("persistent")? {
            let names = if kind == "Table" {
                &mut tables
            } else {
                &mut scalars
            };
            names.insert(widget.get::<String>("name")?);
        }
    }
    for node in &program.nodes {
        let missing = match node.kind.as_str() {
            "ScriptProcessor" => node
                .attributes
                .keys()
                .any(|name| name != "API_version" && !scalars.contains(name)),
            "ScriptData" => node.attributes.keys().any(|name| !tables.contains(name)),
            _ => false,
        };
        if missing {
            return Err(mlua::Error::runtime(
                "UVI saved widget is unavailable at the native restoration boundary",
            ));
        }
    }
    restore_widgets_in(lua, program, None, environment)
}

fn restore_widgets_in(
    lua: &Lua,
    program: &Program,
    processor: Option<NodeId>,
    environment: &Table,
) -> mlua::Result<usize> {
    let processors = program
        .nodes
        .iter()
        .enumerate()
        .filter(|(id, n)| n.kind == "ScriptProcessor" && processor.is_none_or(|p| p == *id))
        .collect::<Vec<_>>();
    if processors.is_empty() {
        if processor.is_some() {
            return Err(mlua::Error::runtime("Invalid UVI widget scope"));
        }
        return Ok(0);
    }
    if processors.len() != 1 {
        return Err(mlua::Error::runtime(
            "Multiple UVI ScriptProcessor widget namespaces are not supported",
        ));
    }
    let (processor_id, processor) = processors[0];
    let data = program
        .nodes
        .iter()
        .filter(|n| n.parent == Some(processor_id) && n.kind == "ScriptData")
        .collect::<Vec<_>>();
    if data.len() > 1 {
        return Err(mlua::Error::runtime(
            "Multiple UVI ScriptData widget stores are not supported",
        ));
    }
    let ui = environment.get::<Table>("UVI_UI_STATE")?;
    let order = ui.get::<Table>("order")?;
    let mut restored = 0;
    for tables in [true, false] {
        for widget in order.sequence_values::<Table>() {
            let widget = widget?;
            let kind = widget.get::<String>("kind")?;
            if !persistent_parameter_widget(&kind) || (kind == "Table") != tables {
                continue;
            }
            if !widget.get::<bool>("persistent")? {
                continue;
            }
            let name = widget.get::<String>("name")?;
            let set = widget.get::<Function>("setValue")?;
            if kind == "Table" {
                let Some(text) = data.first().and_then(|n| n.attributes.get(&name)) else {
                    continue;
                };
                // Observed Falcon persistence uses a decimal comma inside each
                // whitespace-separated value, without indices or pair delimiters.
                let values = text
                    .split_whitespace()
                    .map(|token| {
                        token
                            .replace(',', ".")
                            .parse::<f64>()
                            .ok()
                            .filter(|n| n.is_finite())
                            .ok_or_else(|| {
                                mlua::Error::runtime(format!(
                                    "Malformed UVI Table persistence for {name}"
                                ))
                            })
                    })
                    .collect::<mlua::Result<Vec<_>>>()?;
                if values.len() != widget.get::<usize>("length")? {
                    return Err(mlua::Error::runtime(format!(
                        "UVI Table persistence length differs for {name}"
                    )));
                }
                for (index, value) in values.into_iter().enumerate() {
                    set.call::<()>((widget.clone(), index + 1, value))?;
                }
                restored += 1;
            } else if let Some(text) = processor.attributes.get(&name) {
                let value = match kind.as_str() {
                    "OnOffButton" => match text.as_str() {
                        "0" => Value::Boolean(false),
                        "1" => Value::Boolean(true),
                        _ => {
                            return Err(mlua::Error::runtime(format!(
                                "Malformed UVI Boolean persistence for {name}"
                            )));
                        }
                    },
                    "Knob" | "Slider" | "NumBox" | "Menu" => attribute(&name, text).to_lua(lua)?,
                    _ => continue,
                };
                set.call::<()>((widget.clone(), value))?;
                restored += 1;
            }
        }
    }
    Ok(restored)
}

fn install_context(lua: &Lua, program: &Table, host: &Host) -> mlua::Result<()> {
    // A standalone Program load creates an Omni Part in a fresh Synth. These
    // defaults are playback context, not parent data recovered from the preset.
    let part = lua.create_table()?;
    let synth = lua.create_table()?;
    let base = host.parameters.borrow().len();
    for (offset, object, kind) in [(0, &part, "Part"), (1, &synth, "Synth")] {
        let id = base + offset;
        host.objects.raw_set(id + 1, object.clone())?;
        let mut defaults = BTreeMap::from([
            ("Gain".into(), ParameterValue::Number(1.)),
            ("Pan".into(), ParameterValue::Number(0.)),
            ("Bypass".into(), ParameterValue::Boolean(false)),
        ]);
        if kind == "Part" {
            defaults.insert("MidiChannel".into(), ParameterValue::Number(-1.));
            defaults.insert("MidiInput".into(), ParameterValue::Number(-1.));
        }
        host.parameters.borrow_mut().push(defaults);
        host.identities
            .borrow_mut()
            .insert(object.to_pointer() as usize, id);
        object.set("_nodeId", id)?;
        object.set("type", kind)?;
        object.set("name", kind)?;
        for (field, _) in collections() {
            object.set(field, lua.create_table()?)?;
        }
        object.set("children", lua.create_table()?)?;
        object.set("mods", lua.create_table()?)?;
        object.set("synthChildren", lua.create_table()?)?;
        object.set_metatable(program.metatable())?;
        object.set(
            "getParameterConnections",
            lua.create_function(|lua, (_self, _name): (Table, String)| lua.create_table())?,
        )?;
    }
    program.set("parent", part.clone())?;
    part.set("parent", synth.clone())?;
    part.set("program", program.clone())?;
    part.get::<Table>("synthChildren")?
        .set(1, program.clone())?;
    synth.get::<Table>("synthChildren")?.set(1, part.clone())?;
    lua.globals().set("Part", part)?;
    Ok(())
}

struct ScriptClass;
struct ScriptInstance;

impl UserData for ScriptClass {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        for operator in [MetaMethod::Eq, MetaMethod::ToString] {
            methods.add_meta_function(operator, |_, _: MultiValue| -> mlua::Result<Value> {
                Err(mlua::Error::runtime("Unsupported UVI class operator"))
            });
        }
        methods.add_meta_function(
            MetaMethod::Index,
            |_, (class, key): (AnyUserData, String)| class.user_value::<Table>()?.get::<Value>(key),
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |_, (class, key, value): (AnyUserData, String, Value)| {
                class.user_value::<Table>()?.set(key, value)
            },
        );
        methods.add_meta_function(MetaMethod::Call, |lua, mut args: MultiValue| {
            let Some(Value::UserData(class)) = args.pop_front() else {
                return Err(mlua::Error::runtime("Invalid UVI class constructor"));
            };
            let members = class.user_value::<Table>()?;
            let init = members.get::<Function>("__init")?;
            let instance = lua.create_userdata(ScriptInstance)?;
            let state = lua.create_table()?;
            state.set("members", members)?;
            state.set("fields", lua.create_table()?)?;
            instance.set_user_value(state)?;
            args.push_front(Value::UserData(instance.clone()));
            init.call::<()>(args)?;
            Ok(instance)
        });
    }
}

impl UserData for ScriptInstance {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        for operator in [MetaMethod::Eq, MetaMethod::ToString] {
            methods.add_meta_function(operator, |_, _: MultiValue| -> mlua::Result<Value> {
                Err(mlua::Error::runtime("Unsupported UVI class operator"))
            });
        }
        methods.add_meta_function(
            MetaMethod::Index,
            |_, (instance, key): (AnyUserData, String)| {
                let state = instance.user_value::<Table>()?;
                let value = state.get::<Table>("fields")?.get::<Value>(key.as_str())?;
                if matches!(value, Value::Nil) {
                    state.get::<Table>("members")?.get::<Value>(key)
                } else {
                    Ok(value)
                }
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |_, (instance, key, value): (AnyUserData, String, Value)| {
                instance
                    .user_value::<Table>()?
                    .get::<Table>("fields")?
                    .set(key, value)
            },
        );
    }
}

// Native class() publishes userdata immediately and returns an optional-base
// builder. Inheritance copies existing members but requires its own __init.
fn install_class(lua: &Lua, environment: &Table) -> mlua::Result<()> {
    let scope = environment.clone();
    environment.set(
        "class",
        lua.create_function(move |lua, name: String| {
            if name.is_empty() || name.len() > 256 || name.contains('\0') {
                return Err(mlua::Error::runtime("Invalid UVI class name"));
            }
            let class = lua.create_userdata(ScriptClass)?;
            class.set_user_value(lua.create_table()?)?;
            scope.set(name, class.clone())?;
            lua.create_function(move |_, base: AnyUserData| {
                if !base.is::<ScriptClass>() {
                    return Err(mlua::Error::runtime("Invalid UVI base class"));
                }
                let members = class.user_value::<Table>()?;
                for pair in base.user_value::<Table>()?.pairs::<String, Value>() {
                    let (key, value) = pair?;
                    if key != "__init" {
                        members.set(key, value)?;
                    }
                }
                Ok(())
            })
        })?,
    )
}

struct AsyncUpdaterFactory;
struct AsyncUpdater;

// Native trigger waits in its caller and coalesces while busy, including callback
// reentry. The Lua closure can yield without crossing a Rust method call boundary.
// User values retain callbacks in Lua rather than permanent Rust reference slots.
impl UserData for AsyncUpdaterFactory {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(
            MetaMethod::Call,
            |lua, (factory, callback): (AnyUserData, Function)| {
                let environment = factory.user_value::<Table>()?;
                let trigger = lua
                    .load(
                        "local pending=false;return function(self,ms)\
                         if pending then return end;pending=true;wait(ms);\
                         self.callback();pending=false end",
                    )
                    .set_name("UVI offline AsyncUpdater")
                    .set_environment(environment)
                    .eval::<Function>()?;
                let state = lua.create_table()?;
                state.set("callback", callback)?;
                state.set("trigger", trigger)?;
                let updater = lua.create_userdata(AsyncUpdater)?;
                updater.set_user_value(state)?;
                Ok(updater)
            },
        );
    }
}

impl UserData for AsyncUpdater {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(
            MetaMethod::Index,
            |_, (updater, key): (AnyUserData, String)| {
                updater.user_value::<Table>()?.get::<Value>(key)
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |_, (updater, key, callback): (AnyUserData, String, Function)| {
                if key != "callback" {
                    return Err(mlua::Error::runtime("Unknown UVI AsyncUpdater property"));
                }
                updater.user_value::<Table>()?.set(key, callback)
            },
        );
    }
}

pub(crate) fn resolve_module<'a>(
    modules: &'a BTreeMap<String, Vec<u8>>,
    name: &str,
) -> mlua::Result<&'a [u8]> {
    Ok(if let Some(source) = modules.get(name) {
        source.as_slice()
    } else if name == "uvi.ChordRec" {
        CHORD_REC.as_bytes()
    } else {
        // Embedded processors may omit their resource folder. Resolve only
        // approved bank names, accepting duplicate aliases of the same bytes.
        let relative = name.replace(['/', '\\'], ".");
        let suffix = format!(".{relative}");
        let mut matches = modules
            .iter()
            .filter(|(key, _)| key.as_str() == relative || key.ends_with(&suffix));
        let (_, source) = matches.next().ok_or_else(|| {
            mlua::Error::runtime(format!("UVI embedded module {name:?} is not approved"))
        })?;
        if matches.any(|(_, candidate)| candidate != source) {
            return Err(mlua::Error::runtime(format!(
                "UVI embedded module {name:?} is ambiguous"
            )));
        }
        source.as_slice()
    })
}

fn install_modules(
    lua: &Lua,
    modules: Rc<BTreeMap<String, Vec<u8>>>,
    environment: &Table,
) -> mlua::Result<()> {
    let mut total = 0usize;
    for (name, source) in modules.iter() {
        total = total.saturating_add(source.len());
        if name.is_empty()
            || name.len() > 256
            || source.len() > SOURCE_LIMIT
            || total > 16 << 20
            || source.starts_with(b"\x1bLua")
            || std::str::from_utf8(source).is_err()
        {
            return Err(mlua::Error::runtime(
                "Invalid or oversized UVI embedded Lua module",
            ));
        }
    }
    let cache = lua.create_table()?;
    let loading = Rc::new(RefCell::new(HashSet::<String>::new()));
    let module_environment = environment.clone();
    environment.set(
        "require",
        lua.create_function(move |lua, name: String| {
            if name.is_empty() || name.len() > 256 || name.contains('\0') {
                return Err(mlua::Error::runtime("Invalid UVI embedded module name"));
            }
            let cached = cache.get::<Value>(name.as_str())?;
            if !matches!(cached, Value::Nil | Value::Boolean(false)) {
                return Ok(cached);
            }
            if name == "uvi.AsyncUpdater" && !modules.contains_key(&name) {
                let factory = lua.create_userdata(AsyncUpdaterFactory)?;
                factory.set_user_value(module_environment.clone())?;
                module_environment.set("AsyncUpdater", factory)?;
                cache.set(name, true)?;
                return Ok(Value::Boolean(true));
            }
            let source = resolve_module(&modules, &name)?;
            if !loading.borrow_mut().insert(name.clone()) {
                return Err(mlua::Error::runtime(format!(
                    "UVI module load cycle at {name:?}"
                )));
            }
            let result = lua
                .load(source)
                .set_name(format!("embedded module {name}"))
                .set_environment(module_environment.clone())
                .call::<Value>(name.clone());
            loading.borrow_mut().remove(&name);
            let result = result?;
            let result = if matches!(result, Value::Nil) {
                Value::Boolean(true)
            } else {
                result
            };
            cache.set(name, result.clone())?;
            Ok(result)
        })?,
    )?;
    Ok(())
}

fn install_modulation(
    lua: &Lua,
    host: &Host,
    now: Rc<dyn Fn() -> u64>,
    valid_voice: Option<Rc<dyn Fn(u32) -> bool>>,
    layer_scope: Option<Rc<dyn Fn() -> Option<NodeId>>>,
) -> mlua::Result<()> {
    for (name, explicit_start) in [
        ("sendScriptModulation", false),
        ("sendScriptModulation2", true),
    ] {
        let commands = host.commands.clone();
        let clock = now.clone();
        let valid_voice = valid_voice.clone();
        let layer_scope = layer_scope.clone();
        lua.globals().set(
            name,
            lua.create_function(move |_, mut args: MultiValue| {
                let id = match args.pop_front() {
                    Some(Value::Integer(n)) if (0..128).contains(&n) => n as u8,
                    Some(Value::Number(n)) if n.fract() == 0. && (0. ..128.).contains(&n) => {
                        n as u8
                    }
                    _ => {
                        return Err(mlua::Error::runtime(
                            "UVI script modulation id must be 0..127",
                        ));
                    }
                };
                let number = |value: Option<Value>, default: Option<f64>| -> mlua::Result<f64> {
                    match value {
                        Some(Value::Number(n)) if n.is_finite() => Ok(n),
                        Some(Value::Integer(n)) => Ok(n as f64),
                        None | Some(Value::Nil) if default.is_some() => Ok(default.unwrap()),
                        _ => Err(mlua::Error::runtime(
                            "UVI modulation requires finite numbers",
                        )),
                    }
                };
                let start = if explicit_start {
                    Some(number(args.pop_front(), None)?)
                } else {
                    None
                };
                let target = number(args.pop_front(), None)?;
                let ramp_ms = number(args.pop_front(), Some(20.))?;
                let voice = match args.pop_front() {
                    None | Some(Value::Nil) => None,
                    Some(value) => Some(super::script::voice_id(value)?),
                };
                if !(-1. ..=1.).contains(&target)
                    || start.is_some_and(|s| !(-1. ..=1.).contains(&s))
                    || !(0. ..=60000.).contains(&ramp_ms)
                    || !args.is_empty()
                {
                    return Err(mlua::Error::runtime(
                        "UVI modulation value/ramp outside supported range",
                    ));
                }
                if voice.is_some_and(|id| !valid_voice.as_ref().is_some_and(|valid| valid(id))) {
                    return Err(mlua::Error::runtime("Unknown UVI modulation voice id"));
                }
                emit(
                    &commands,
                    &*clock,
                    Action::ScriptModulation {
                        id,
                        start,
                        target,
                        ramp_ms,
                        voice,
                        layer: layer_scope.as_ref().and_then(|scope| scope()),
                    },
                )
            })?,
        )?;
    }
    Ok(())
}

pub(crate) fn resource_path(path: &str) -> mlua::Result<()> {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err(mlua::Error::runtime("Invalid UVI resource path"));
    }
    Ok(())
}

fn resource_read(
    resources: &Option<Resources>,
    request: &ResourceRequest,
) -> mlua::Result<ResourceResponse> {
    resources
        .as_ref()
        .ok_or_else(|| mlua::Error::runtime("UVI resource capability is not configured"))?(
        request
    )
}

fn task(lua: &Lua, ids: &Cell<u32>, error: Option<&mlua::Error>) -> mlua::Result<Table> {
    let id = ids
        .get()
        .checked_add(1)
        .ok_or_else(|| mlua::Error::runtime("UVI resource task ID space exhausted"))?;
    ids.set(id);
    let task = lua.create_table()?;
    task.set("id", id)?;
    task.set("finished", true)?;
    task.set("progress", 1.)?;
    task.set("state", "finished")?;
    task.set("success", error.is_none())?;
    if let Some(error) = error {
        task.set("error", error.to_string())?;
    }
    Ok(task)
}

fn json_data(lua: &Lua, bytes: &[u8]) -> mlua::Result<Value> {
    if bytes.len() > SOURCE_LIMIT {
        return Err(mlua::Error::runtime("UVI resource data exceeds 2 MiB"));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(mlua::Error::external)?;
    super::script::saved_value(lua, &value, 0, &mut 0).map_err(mlua::Error::external)
}

/// Native loadState restores controls/callbacks, then onLoad; it does not rerun onInit.
fn read_state(lua: &Lua, bytes: &[u8]) -> mlua::Result<(Program, Option<Value>)> {
    let (program, saved) = parse_state(bytes)?;
    let saved = saved
        .map(|value| {
            super::script::saved_value(lua, &value, 0, &mut 0).map_err(mlua::Error::external)
        })
        .transpose()?;
    Ok((program, saved))
}

pub(crate) fn parse_state(bytes: &[u8]) -> mlua::Result<(Program, Option<serde_json::Value>)> {
    if bytes.len() > SOURCE_LIMIT {
        return Err(mlua::Error::runtime("UVI state exceeds 2 MiB"));
    }
    let source = std::str::from_utf8(bytes).map_err(mlua::Error::external)?;
    let document = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: LIMIT as u32,
            ..Default::default()
        },
    )
    .map_err(mlua::Error::external)?;
    let root = document.root_element();
    let processors = root
        .children()
        .filter(|n| n.is_element())
        .collect::<Vec<_>>();
    if root.tag_name().name() != "UVI4"
        || processors.len() != 1
        || processors[0].tag_name().name() != "ScriptProcessor"
    {
        return Err(mlua::Error::runtime(
            "Unsupported UVI script state document",
        ));
    }
    let processor = processors[0];
    let children = processor
        .children()
        .filter(|n| n.is_element())
        .collect::<Vec<_>>();
    if children.iter().any(|n| {
        !matches!(n.tag_name().name(), "ScriptData" | "state")
            || n.children().any(|child| child.is_element())
    }) || children
        .iter()
        .filter(|n| n.has_tag_name("ScriptData"))
        .count()
        > 1
    {
        return Err(mlua::Error::runtime(
            "UVI script state contains unsupported elements",
        ));
    }
    let states = processor
        .children()
        .filter(|n| n.has_tag_name("state"))
        .collect::<Vec<_>>();
    if states.len() > 1 {
        return Err(mlua::Error::runtime("Multiple UVI script states"));
    }
    let saved = states
        .first()
        .map(|n| {
            serde_json::from_str::<serde_json::Value>(n.text().unwrap_or(""))
                .map_err(mlua::Error::external)
        })
        .transpose()?;
    if saved
        .as_ref()
        .is_some_and(|s| !s.is_object() && !s.is_array() && !s.is_null())
    {
        return Err(mlua::Error::runtime(
            "UVI script state must decode to a table or nil",
        ));
    }
    fn bounded(value: &serde_json::Value, depth: usize, count: &mut usize) -> mlua::Result<()> {
        *count += 1;
        if depth > 64 || *count > LIMIT {
            return Err(mlua::Error::runtime("UVI state exceeds structure limit"));
        }
        match value {
            serde_json::Value::Array(values) => {
                for value in values {
                    bounded(value, depth + 1, count)?;
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values() {
                    bounded(value, depth + 1, count)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    if let Some(saved) = &saved {
        bounded(saved, 0, &mut 0)?;
    }
    let wrapped = format!("<Program>{}</Program>", &source[processor.range()]);
    let program = super::program::parse_program(&wrapped).map_err(mlua::Error::external)?;
    Ok((program, saved))
}

fn restore_state(
    lua: &Lua,
    program: &Program,
    saved: Option<Value>,
    environment: &Table,
) -> mlua::Result<()> {
    restore_widgets_in(lua, program, None, environment)?;
    if let (Some(saved), Some(callback)) = (saved, environment.get::<Option<Function>>("onLoad")?) {
        callback.call::<()>(saved)?;
    }
    Ok(())
}

fn json_value(
    value: Value,
    depth: usize,
    count: &mut usize,
    parents: &mut HashSet<usize>,
) -> mlua::Result<serde_json::Value> {
    *count += 1;
    if depth > 64 || *count > LIMIT {
        return Err(mlua::Error::runtime("UVI state exceeds structure limit"));
    }
    Ok(match value {
        Value::Nil => serde_json::Value::Null,
        Value::Boolean(b) => b.into(),
        Value::Integer(n) => n.into(),
        Value::Number(n) => serde_json::Number::from_f64(n)
            .map(serde_json::Value::Number)
            .ok_or_else(|| mlua::Error::runtime("Nonfinite UVI state number"))?,
        Value::String(s) => s.to_str()?.as_ref().into(),
        Value::Table(t) => {
            let id = t.to_pointer() as usize;
            if !parents.insert(id) {
                return Err(mlua::Error::runtime("Circular UVI state table"));
            }
            let mut indices = BTreeMap::new();
            let mut names = serde_json::Map::new();
            for pair in t.pairs::<Value, Value>() {
                let (key, value) = pair?;
                let value = json_value(value, depth + 1, count, parents)?;
                match key {
                    Value::String(s) => {
                        names.insert(s.to_str()?.to_owned(), value);
                    }
                    Value::Integer(n) if n > 0 => {
                        indices.insert(n as usize, value);
                    }
                    Value::Number(n) if n.fract() == 0. && n > 0. && n <= LIMIT as f64 => {
                        indices.insert(n as usize, value);
                    }
                    _ => return Err(mlua::Error::runtime("Unsupported UVI state table key")),
                }
            }
            parents.remove(&id);
            if indices.is_empty() {
                serde_json::Value::Object(names)
            } else if names.is_empty() && indices.keys().copied().eq(1..=indices.len()) {
                serde_json::Value::Array(indices.into_values().collect())
            } else {
                return Err(mlua::Error::runtime(
                    "UVI state table must be a contiguous array or dictionary",
                ));
            }
        }
        _ => return Err(mlua::Error::runtime("Unsupported UVI saved state value")),
    })
}

fn xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// Original synthetic native saveState probes establish this document shape.
pub(crate) fn save_state(environment: &Table) -> mlua::Result<Vec<u8>> {
    let mut scalars = String::new();
    let mut tables = String::new();
    let order = environment
        .get::<Table>("UVI_UI_STATE")?
        .get::<Table>("order")?;
    for widget in order.sequence_values::<Table>() {
        let widget = widget?;
        let kind = widget.get::<String>("kind")?;
        if !persistent_parameter_widget(&kind) || !widget.get::<bool>("persistent")? {
            continue;
        }
        let name = widget.get::<String>("name")?;
        let (destination, value) = if kind == "Table" {
            let get = widget.get::<Function>("getValue")?;
            let values = (1..=widget.get::<usize>("length")?)
                .map(|i| {
                    get.call::<f64>((widget.clone(), i))
                        .map(|v| format!("{v:.6}"))
                })
                .collect::<mlua::Result<Vec<_>>>()?;
            (&mut tables, values.join(" "))
        } else {
            let value = match ParameterValue::from_lua(widget.get::<Value>("value")?)? {
                ParameterValue::Number(n) => n.to_string(),
                ParameterValue::Boolean(b) => if b { "1" } else { "0" }.to_owned(),
                ParameterValue::Text(_) => {
                    return Err(mlua::Error::runtime(
                        "Unsupported UVI persistent widget value",
                    ));
                }
            };
            (&mut scalars, value)
        };
        destination.push_str(&format!(" {}=\"{}\"", name, xml_text(&value)));
        if scalars.len() + tables.len() > SOURCE_LIMIT {
            return Err(mlua::Error::runtime("UVI state exceeds 2 MiB"));
        }
    }
    let saved = if let Some(callback) = environment.get::<Option<Function>>("onSave")? {
        let value = callback.call::<Value>(())?;
        if !matches!(value, Value::Table(_) | Value::Nil) {
            return Err(mlua::Error::runtime(
                "UVI script state saving requires an onSave table or nil",
            ));
        }
        let value = json_value(value, 0, &mut 0, &mut HashSet::new())?;
        format!(
            "<state>{}</state>",
            xml_text(&serde_json::to_string(&value).map_err(mlua::Error::external)?)
        )
    } else {
        String::new()
    };
    let source = format!(
        "<UVI4><ScriptProcessor API_version=\"13\"{scalars}><ScriptData{tables}/>{saved}</ScriptProcessor></UVI4>"
    );
    if source.len() > SOURCE_LIMIT {
        return Err(mlua::Error::runtime("UVI state exceeds 2 MiB"));
    }
    roxmltree::Document::parse(&source).map_err(mlua::Error::external)?;
    Ok(source.into_bytes())
}

fn install_resources(
    lua: &Lua,
    host: &Host,
    now: Rc<dyn Fn() -> u64>,
    resources: Option<Resources>,
    environment: &Table,
) -> mlua::Result<()> {
    let task_ids = host.task_ids.clone();
    let target_types = host.types.clone();
    // Capture the runtime scheduler before authored scripts can replace globals.
    // Standalone object hosts have no cooperative runtime and complete inline.
    let callback_scheduler = environment.get::<Option<Function>>("spawn")?;
    for (name, kind) in [
        ("loadSample", ResourceKind::Sample),
        ("loadImpulse", ResourceKind::Impulse),
    ] {
        let commands = host.commands.clone();
        let clock = now.clone();
        let parameters = host.parameters.clone();
        let loaded_resources = host.loaded_resources.clone();
        let ids = host.identities.clone();
        let target_types = target_types.clone();
        let resources = resources.clone();
        let task_ids = task_ids.clone();
        let callback_scheduler = callback_scheduler.clone();
        environment.set(
            name,
            lua.create_function(
                move |lua, (object, path, callback): (Table, String, Option<Function>)| {
                    let node = node_id(&object, &ids)?;
                    resource_path(&path)?;
                    let target_type = target_types
                        .get(node)
                        .ok_or_else(|| mlua::Error::runtime("Unknown UVI resource target"))?;
                    if (kind == ResourceKind::Sample && target_type != "SamplePlayer")
                        || (kind == ResourceKind::Impulse
                            && !matches!(target_type.as_str(), "Convolver" | "SampledReverb"))
                    {
                        return Err(mlua::Error::runtime(
                            "UVI resource target has incompatible processor type",
                        ));
                    }
                    let result = (|| {
                        let ResourceResponse::Audio(info) = resource_read(
                            &resources,
                            &ResourceRequest::ReadAudio {
                                kind,
                                path: path.clone(),
                            },
                        )?
                        else {
                            return Err(mlua::Error::runtime(
                                "UVI audio capability returned an incompatible response",
                            ));
                        };
                        if info.rate == 0
                            || info.channels == 0
                            || info.channels > 64
                            || info.name.len() > 4096
                            || info.frames == 0
                        {
                            return Err(mlua::Error::runtime("Invalid UVI decoded audio metadata"));
                        }
                        let sample = lua.create_table_from([
                            ("name", Value::String(lua.create_string(&info.name)?)),
                            ("samplerate", Value::Number(info.rate as f64)),
                            ("channels", Value::Number(info.channels as f64)),
                            (
                                "duration",
                                Value::Number(info.frames as f64 * 1000. / info.rate as f64),
                            ),
                        ])?;
                        Ok(sample)
                    })();
                    if let Ok(sample) = &result {
                        // A failed task leaves the current asset intact. Output-capacity
                        // failure still aborts admission before host metadata can change.
                        emit(
                            &commands,
                            &*clock,
                            Action::LoadResource {
                                node,
                                kind,
                                path: path.clone(),
                            },
                        )?;
                        if kind == ResourceKind::Sample {
                            object.set("sampleInfo", sample.clone())?;
                        }
                        parameters.borrow_mut()[node]
                            .insert("SamplePath".into(), ParameterValue::Text(path.clone()));
                        loaded_resources
                            .borrow_mut()
                            .insert(node, (kind, path.clone()));
                    }
                    let task = task(lua, &task_ids, result.as_ref().err())?;
                    if let Err(error) = &result {
                        let exact = error.to_string();
                        let reason = exact.chars().take(512).collect::<String>();
                        crate::diagnostics::event(
                            crate::diagnostics::LogLevel::Warning,
                            "uvi.host",
                            "resource_task_failed",
                            serde_json::json!({
                                "node":node, "kind":kind, "frame":clock(),
                                "reason":reason,
                                "reason_truncated":exact.chars().nth(512).is_some(),
                            }),
                        );
                    }
                    if let Some(callback) = callback {
                        if let Some(scheduler) = &callback_scheduler {
                            // Completion includes failures. The existing scoped scheduler
                            // supplies ordering, yielding, budgets and host-root ownership.
                            scheduler.call::<()>((callback, task.clone()))?;
                        } else {
                            callback.call::<()>(task.clone())?;
                        }
                    }
                    Ok(task)
                },
            )?,
        )?;
    }
    for (name, text) in [("loadData", false), ("loadTextData", true)] {
        let resources = resources.clone();
        let task_ids = task_ids.clone();
        environment.set(
            name,
            lua.create_function(move |lua, (path, callback): (String, Option<Function>)| {
                resource_path(&path)?;
                let mut raw = None;
                let result = (|| {
                    let ResourceResponse::Bytes(bytes) =
                        resource_read(&resources, &ResourceRequest::ReadData { path })?
                    else {
                        return Err(mlua::Error::runtime(
                            "UVI data capability returned an incompatible response",
                        ));
                    };
                    if bytes.len() > SOURCE_LIMIT {
                        return Err(mlua::Error::runtime("UVI resource data exceeds 2 MiB"));
                    }
                    let data = lua.create_string(&bytes)?;
                    raw = Some(data.clone());
                    if text {
                        Ok(Value::String(data))
                    } else {
                        json_data(lua, &bytes)
                    }
                })();
                let task = task(lua, &task_ids, result.as_ref().err())?;
                if let Some(raw) = raw {
                    task.set("data", raw)?;
                    // Native success tracks reading, even when JSON decoding fails.
                    task.set("success", true)?;
                }
                if let (Ok(data), Some(callback)) = (result, callback) {
                    callback.call::<()>(data)?;
                }
                Ok(task)
            })?,
        )?;
    }
    {
        let resources = resources.clone();
        let task_ids = task_ids.clone();
        let state_environment = environment.clone();
        environment.set(
            "loadState",
            lua.create_function(move |lua, (path, callback): (String, Option<Function>)| {
                resource_path(&path)?;
                // Read failures are task failures; Lua restoration callback errors propagate.
                let result = resource_read(&resources, &ResourceRequest::ReadState { path })
                    .and_then(|response| match response {
                        ResourceResponse::Bytes(bytes) => read_state(lua, &bytes),
                        _ => Err(mlua::Error::runtime(
                            "UVI state capability returned an incompatible response",
                        )),
                    });
                let task = task(lua, &task_ids, result.as_ref().err())?;
                if let Ok((program, saved)) = result {
                    restore_state(lua, &program, saved, &state_environment)?;
                    if let Some(callback) = callback {
                        callback.call::<()>(task.clone())?;
                    }
                }
                Ok(task)
            })?,
        )?;
    }
    {
        let resources = resources.clone();
        let task_ids = task_ids.clone();
        let state_environment = environment.clone();
        environment.set(
            "saveState",
            lua.create_function(move |lua, (path, callback): (String, Option<Function>)| {
                resource_path(&path)?;
                let result = (|| {
                    let bytes = save_state(&state_environment)?;
                    match resource_read(&resources, &ResourceRequest::WriteState { path, bytes })? {
                        ResourceResponse::Saved => Ok(()),
                        _ => Err(mlua::Error::runtime(
                            "UVI state capability returned an incompatible response",
                        )),
                    }
                })();
                let task = task(lua, &task_ids, result.as_ref().err())?;
                if let (Ok(()), Some(callback)) = (result, callback) {
                    callback.call::<()>(task.clone())?;
                }
                Ok(task)
            })?,
        )?;
    }
    environment.set(
        "browseForFile",
        lua.create_function(
            move |lua,
                  (mode, title, initial, patterns, callback): (
                String,
                String,
                String,
                String,
                Option<Function>,
            )| {
                if !matches!(mode.as_str(), "open" | "save")
                    || [&title, &initial, &patterns]
                        .iter()
                        .any(|s| s.len() > 4096 || s.contains('\0'))
                {
                    return Err(mlua::Error::runtime("Invalid UVI file browsing request"));
                }
                let result = resource_read(
                    &resources,
                    &ResourceRequest::Browse {
                        mode,
                        title,
                        initial,
                        patterns,
                    },
                );
                let error = match &result {
                    Ok(ResourceResponse::Selected(_)) => None,
                    Ok(_) => Some(mlua::Error::runtime(
                        "UVI browse capability returned an incompatible response",
                    )),
                    Err(e) => Some(e.clone()),
                };
                let task = task(lua, &task_ids, error.as_ref())?;
                if let Ok(ResourceResponse::Selected(selected)) = result {
                    task.set("result", selected.clone().unwrap_or_default())?;
                    if selected.is_none() {
                        task.set("success", false)?;
                        task.set("state", "cancelled")?;
                    }
                }
                if let Some(callback) = callback {
                    callback.call::<()>(task.clone())?;
                }
                Ok(task)
            },
        )?,
    )?;
    Ok(())
}

// Original implementation calibrated with authored pitch-set probes against the
// official player. Numeric masks encode standard intervals above a candidate root.
// The native module installs a global and returns nil (require caches true).
const CHORD_REC: &str = r#"
local kinds={
  [5]="sus2",[9]="m",[11]="maddb9",[13]="madd9",[17]="M",[19]="Maddb9",
  [21]="Madd9",[25]="Madd#9",[33]="sus4",[37]="sus2sus4",[41]="mbb5",[65]="5-",
  [73]="dim",[81]="Mb5",[129]="5",[133]="sus2",[137]="m",[139]="maddb9",
  [141]="madd9",[145]="M",[147]="Maddb9",[149]="Madd9",[153]="Madd#9",[161]="sus4",
  [165]="sus2sus4",[261]="sus2#5",[273]="aug",[525]="m6/9",[533]="6/9",[585]="dim7",
  [649]="m6",[653]="m6/9",[657]="6",[661]="6/9",[1029]="7sus2no5",[1033]="m7",
  [1035]="m7b9",[1037]="m9",[1041]="7",[1043]="7b9",[1045]="9",[1049]="7#9",
  [1057]="7sus4no5",[1061]="7sus2sus4no5",[1069]="m9/11",[1077]="11",[1097]="m7b5",[1101]="m9b5",
  [1105]="7b5",[1109]="9b5",[1157]="7sus2",[1161]="m7",[1163]="m7b9",[1165]="m9",
  [1169]="7",[1171]="7b9",[1173]="9",[1177]="7#9",[1185]="7sus4",[1189]="7sus2sus4",
  [1193]="m7/11",[1197]="m11",[1205]="11",[1225]="m7/#11",[1289]="m7#5",[1297]="7#5",
  [1301]="9#5",[1581]="m13",[1589]="13",[1709]="m13",[1717]="13",[2053]="M7sus2",
  [2057]="mM7",[2061]="mM9",[2065]="M7",[2067]="M7b9",[2069]="M9",[2073]="M7#9",
  [2081]="M7sus4no5",[2085]="M7sus2sus4no5",[2089]="mM7bb5",[2093]="mM11",[2101]="M11",[2121]="mM7b5",
  [2129]="M7b5",[2133]="M#11",[2181]="M7sus2",[2185]="mM7",[2189]="mM9",[2193]="M7",
  [2195]="M7b9",[2197]="M9",[2201]="M7#9",[2209]="M7sus4",[2213]="M7sus2sus4",[2221]="mM11",
  [2229]="M11",[2261]="M#11",[2313]="mM7#5",[2321]="M7#5",[2325]="M9#5",[2605]="mM13",
  [2613]="M13",[2733]="mM13",[2741]="M13",
}
ChordRec={}
function ChordRec.getChroma(root,notes)
  local present={}
  for _,pitch in ipairs(notes)do present[(pitch-root)%12]=true end
  local chroma={}
  for interval=0,11 do chroma[interval+1]=present[interval]and 1 or 0 end
  return chroma
end
function ChordRec.getChromaString(chroma)return table.concat(chroma)end
function ChordRec.chordKind(notes)
  local bass=notes[1]%12
  for _,note in ipairs(notes)do
    local root=note%12
    local chroma=ChordRec.getChroma(root,notes)
    local mask=0
    for interval=0,11 do mask=mask+chroma[interval+1]*2^interval end
    local kind=kinds[mask]
    if kind then return root,kind,bass end
  end
end
"#;

// Original, state-only widget implementation using native Lua tables. Unit is
// formatting metadata: it never rescales the engine parameter or stored value.
const UI: &str = r#"
Unit={Generic=0,Percent=1,PercentNormalized=2,Seconds=3,MilliSeconds=5,Hertz=7,Decibels=9,UviFilter=10,LinearGain=11,Pan=12,Megabyte=13,SemiTones=14,Cents=15,MidiKey=16}
-- Native enum IDs; mappers describe visual position, never stored-value scaling.
Mapper={Linear=0,Exponential=1,QuinticRoot=2,QuarticRoot=3,CubeRoot=4,SquareRoot=5,Quadratic=6,Cubic=7,Quartic=8,Quintic=9}
local float32=_uvi_float;_uvi_float=nil
local modifiers=_uvi_modifiers;_uvi_modifiers=nil
local widgets, order, root = {}, {}, {width=0,height=0}
local methods={}
local values={Table=true,Menu=true,Knob=true,Slider=true,NumBox=true,OnOffButton=true}
local function integer(v)if v<0 then return math.ceil(v)end;return math.floor(v)end
local function geometry(p,k,v)
  p[k]=v
  if k=='bounds' then p.x=v[1];p.y=v[2];p.width=v[3];p.height=v[4]
  elseif k=='size' then p.width=v[1];p.height=v[2]
  elseif k=='position' or k=='pos' then p.x=v[1];p.y=v[2] end
end
function methods:setValue(a,b,c,d)
  local p=self._state
  if not values[p.kind] then error('This UVI widget has no value control')end
  local call=true
  local index=nil
  if p.kind=='Table' then
    index=a; if type(index)~='number' or index~=index or index==math.huge or index==-math.huge then error('Invalid UVI Table index') end;index=integer(index)
    if index<1 or index>p.length then return end
    if type(b)~='number' or b~=b or b==math.huge or b==-math.huge then error('UVI Table value must be finite') end
    local v=b; if p.integer then v=integer(v) end
    v=float32(v);local old=p.values[index];p.values[index]=v;call=c~=false and old~=v
  else
    if p.kind=='OnOffButton' or p.kind=='Button' then if type(a)~='boolean' then error('UVI button value must be boolean') end
    elseif type(a)~='number' or a~=a or a==math.huge or a==-math.huge then error('UVI control value must be finite') end
    local v=a
    if type(v)=='number' then if p.integer then v=integer(v) end;v=float32(v) end
    local old=p.value;p.value=v;call=b~=false and old~=v
  end
  if call and type(p.changed)=='function' then
    if p.kind=='Table' then p.changed(self,index)
    elseif p.kind=='OnOffButton' then p.changed(self,d or modifiers())
    else p.changed(self)end
  end
end
function methods:push(callChangedCallback,mods)
  local p=self._state
  if p.kind~='Button' or type(callChangedCallback)~='boolean' then error('UVI Button push requires a boolean callback flag')end
  if callChangedCallback and type(p.changed)=='function' then if mods then p.changed(self,mods)else p.changed(self)end end
end
function methods:getValue(index)
  local p=self._state
  if not values[p.kind] then error('This UVI widget has no value control')end
  if p.kind=='Table' then if type(index)~='number' or index~=index or index==math.huge or index==-math.huge then error('Invalid UVI Table index') end return p.values[integer(index)] or p.default end
  return p.value
end
function methods:getText(index) local p=self._state;if p.kind~='Menu' then error('Only UVI Menu has getText')end;if type(index)~='number' then error('UVI Menu getText requires an index')end;return p.items[integer(index)] or '' end
function methods:clear()local p=self._state;if p.kind~='Menu' then error('Only UVI Menu can clear items')end;p.items={};p.max=0 end
function methods:addItem(text)local p=self._state;if p.kind~='Menu' or type(text)~='string' then error('UVI Menu item must be text')end;table.insert(p.items,text);p.max=#p.items;return #p.items end
function methods:setItem(index,text)
  local p=self._state; if p.kind~='Menu' or type(index)~='number' or type(text)~='string' then error('Invalid UVI Menu item') end
  index=integer(index);if index<1 or index>#p.items then error('Invalid UVI Menu item')end;p.items[index]=text
end
function methods:setRange(min,max) if type(min)~='number' or type(max)~='number' or min>max then error('Invalid UVI widget range') end self._state.min=min; self._state.max=max end
function methods:setStripImage(image,numImages,orientation) self._state.stripImage={image,numImages,orientation} end
local function construct(kind,...)
  local args={...}; local p={kind=kind,enabled=true,visible=true,alpha=1,persistent=true,children={},x=0,y=0,width=0,height=0}
  if type(args[1])=='table' then for k,v in pairs(args[1]) do p[k]=v end; args=args[1] end
  if kind=='XY' then
    p.paramX=p.paramX or args[1];p.paramY=p.paramY or args[2]
    if type(p.paramX)~='string' or type(p.paramY)~='string' then error('UVI XY requires two parameter names')end
    p.name=p.name or ('XY_'..p.paramX..'_'..p.paramY)
  end
  p.name=p.name or args[1];if p.name==nil and (kind=='Panel' or kind=='Viewport')then p.name=''end
  if type(p.name)~='string' then error('UVI widget requires a name') end
  if kind=='Button' or kind=='OnOffButton' or kind=='Knob' then
    p.displayName=p.displayName or ''
    if p.showLabel==nil then p.showLabel=kind=='Knob' end
    if kind=='Knob' and p.showValue==nil then p.showValue=true end
  else p.displayName=p.displayName or p.name end
  p.tooltip=p.tooltip or p.name
  p.integer=p.integer or false
  if kind=='WaveView' then p.sample=p.sample or '' end
  if kind=='Table' then p.length=p.length or args[2] or 16; p.default=p.default or args[3] or 0; p.min=p.min or args[4] or 0; p.max=p.max or args[5] or 1; p.integer=p.integer or args[6] or false; p.values={}; if p.length<1 or p.length>65536 or p.length%1~=0 then error('Invalid UVI Table length') end; for i=1,p.length do p.values[i]=p.default end
  elseif kind=='Menu' then p.items=p.items or args[2] or {}; p.min=1; p.max=#p.items; p.value=p.value or p.selected or args[3] or 1; p.integer=true
  elseif kind=='OnOffButton' then if p.value==nil then p.value=args[2] or false end
  elseif kind=='Knob' or kind=='Slider' or kind=='NumBox' then p.min=p.min or args[3] or 0; p.max=p.max or args[4] or 1; p.value=p.value or args[2] or 0; p.integer=p.integer or args[5] or false
  end
  if p.min then p.min=float32(p.min);p.max=float32(p.max)end
  if kind=='Table' then p.default=float32(p.default);for i=1,p.length do p.values[i]=p.default end
  elseif kind=='Knob' or kind=='Slider' or kind=='NumBox' then p.default=float32(p.default or args[2] or 0);p.value=float32(p.value)end
  if p.size then geometry(p,'size',p.size)end;if p.position then geometry(p,'position',p.position)end;if p.pos then geometry(p,'pos',p.pos)end;if p.bounds then geometry(p,'bounds',p.bounds)end
  local widget=setmetatable({_state=p},{__index=function(t,k)
    if p.kind=='Button' and (k=='setValue' or k=='getValue' or k=='setRange')then return nil end
    if p.kind=='Menu' then if k=='selected' then return p.value elseif k=='text' or k=='selectedText' then return p.items[p.value] or '' elseif k=='length' then return #p.items end end
    if k=='size' then return {p.width,p.height} elseif k=='position' or k=='pos' then return {p.x,p.y} elseif k=='bounds' then return {p.x,p.y,p.width,p.height}end
    return methods[k] or p[k]
  end,__newindex=function(t,k,v) if k=='value' or (p.kind=='Menu' and k=='selected') then methods.setValue(t,v) else geometry(p,k,v) end end})
  if p.parent then table.insert(p.parent._state.children,widget) end
  widgets[p.name]=widget
  table.insert(order,widget)
  return widget
end
for _,kind in ipairs{'Panel','Viewport','Label','Image','WaveView','AudioMeter','XY','Menu','Table','Slider','Knob','NumBox','Button','OnOffButton'} do
  _G[kind]=function(...) return construct(kind,...) end
  methods[kind]=function(parent,...) if parent.kind~='Panel' and parent.kind~='Viewport' then error('Only UVI containers can create child widgets')end;local widget=construct(kind,...);if not widget.parent then widget.parent=parent;table.insert(parent._state.children,widget)end;return widget end
end
function setSize(w,h) root.width=w;root.height=h end
function setHeight(h) root.height=h end
function setBackground(path) root.background=path end
function setBackgroundColour(colour) root.backgroundColour=colour end
function makePerformanceView() root.performanceView=true end
function setKeyColour(note,colour) root.keyColours=root.keyColours or {};root.keyColours[note]=colour end
function resetKeyColour(note) if root.keyColours then root.keyColours[note]=nil end end
function setKeySwitches(notes) root.keySwitches=notes end
UVI_UI_STATE={widgets=widgets,order=order,root=root}
return function(widget,a,b,mods)
  if widget._state.kind=='Button' then methods.push(widget,true,mods)
  elseif widget._state.kind=='OnOffButton' then methods.setValue(widget,a,true,nil,mods)
  else methods.setValue(widget,a,b)end
end
"#;

#[cfg(test)]
mod tests {
    #[test]
    fn completed_resource_tasks_do_not_exhaust_after_65536() {
        let lua = mlua::Lua::new();
        let ids = std::cell::Cell::new(65536);
        let result = super::task(&lua, &ids, None).unwrap();
        assert_eq!(result.get::<u32>("id").unwrap(), 65537);
        ids.set(u32::MAX);
        assert!(super::task(&lua, &ids, None).is_err());
        assert_eq!(ids.get(), u32::MAX);
    }

    use super::super::program::parse_program;
    use super::*;
    use mlua::{LuaOptions, StdLib};
    fn vm() -> Lua {
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH,
            LuaOptions::default(),
        )
        .unwrap();
        lua.set_memory_limit(32 << 20).unwrap();
        lua
    }
    fn ui_environment(lua: &Lua) -> Table {
        let environment = lua.create_table().unwrap();
        let metatable = lua.create_table().unwrap();
        metatable.raw_set("__index", lua.globals()).unwrap();
        environment.set_metatable(Some(metatable)).unwrap();
        environment.raw_set("_G", environment.clone()).unwrap();
        install_ui(lua, &environment).unwrap();
        environment
    }

    #[test]
    fn native_absent_rack_gain_writes_preserve_commands_state_and_saved_baseline() {
        let program = parse_program(r#"<Program><AuxEffects><AuxEffect><Inserts><EffectRack Name="rack"/><GainMatrix Gain_1_1="1" Gain_2_2="1"/><EffectRack Gain_1_1="0.5"/><DigitalEq/></Inserts></AuxEffect></AuxEffects></Program>"#).unwrap();
        let lua = vm();
        let host = install(&lua, HostConfig {program:Some(&program),modules:BTreeMap::new(),now:Rc::new(||17),resources:None,valid_voice:None,layer_scope:None}).unwrap();
        let baseline = host.parameters.borrow().clone();
        let fingerprint = super::super::state::fingerprint(&program).unwrap();
        let saved = super::super::state::SavedState::new(fingerprint,BTreeMap::new(),&host).unwrap().encode().unwrap();
        lua.load(r#"
          rack=Program.auxs[1].inserts[1]
          for input=1,12 do for output=1,12 do
            local name='Gain_'..input..'_'..output
            assert(not rack:hasParameter(name))
            rack:setParameter(name,0.98)
            assert(not rack:hasParameter(name))
            assert(not pcall(function()return rack:getParameter(name)end))
          end end
        "#).exec().unwrap();
        assert!(host.commands.borrow().is_empty());
        assert_eq!(*host.parameters.borrow(),baseline);
        assert_eq!(super::super::state::SavedState::new(fingerprint,BTreeMap::new(),&host).unwrap().encode().unwrap(),saved);
        let decoded = super::super::state::SavedState::decode(&saved).unwrap();
        assert_eq!(decoded.encode().unwrap(),saved);
        lua.load(r#"
          Program.auxs[1].inserts[2]:setParameter('Gain_1_1',0.98)
          Program.auxs[1].inserts[3]:setParameter('Gain_1_1',0.7)
          rack:setParameter('Bypass',true)
        "#).exec().unwrap();
        assert_eq!(host.commands.borrow().len(),3);
        assert!(matches!(&host.commands.borrow()[0].action,Action::Parameter{parameter,value:ParameterValue::Number(n),..} if parameter=="Gain_1_1" && *n==0.98));
        assert!(matches!(&host.commands.borrow()[1].action,Action::Parameter{parameter,value:ParameterValue::Number(n),..} if parameter=="Gain_1_1" && *n==0.7));
        assert!(matches!(&host.commands.borrow()[2].action,Action::Parameter{parameter,value:ParameterValue::Boolean(true),..} if parameter=="Bypass"));
    }

    #[test]
    fn native_absent_rack_gain_compatibility_keeps_unknown_and_value_guards() {
        let program = parse_program(r#"<Program><AuxEffects><AuxEffect><Inserts><EffectRack/><GainMatrix/><DigitalEq/></Inserts></AuxEffect></AuxEffects></Program>"#).unwrap();
        let lua = vm();
        let host = install(&lua, HostConfig {program:Some(&program),modules:BTreeMap::new(),now:Rc::new(||17),resources:None,valid_voice:None,layer_scope:None}).unwrap();
        let baseline = host.parameters.borrow().clone();
        lua.load(r#"
          local rack=Program.auxs[1].inserts[1]
          for _,name in ipairs{'Gain_0_1','Gain_13_1','Gain_1_0','Gain_1_13','Gain_01_1','Gain_1_01','Gain_1_1_1','Gain_+1_1','Gain_1.0_1','Gain__1','gain_1_1','Invented'} do
            assert(not pcall(function()rack:setParameter(name,0.98)end))
          end
          for _,value in ipairs{true,'0.98',{},0/0,math.huge,-math.huge} do
            assert(not pcall(function()rack:setParameter('Gain_1_1',value)end))
          end
          assert(not pcall(function()rack:setParameter('Gain_1_1',nil)end))
          assert(not pcall(function()rack:setParameter('DisplayType','authored')end))
          for _,context in ipairs{Program.parent,Program.parent.parent} do
            for _,name in ipairs{'Gain_1_1','Invented'} do
              local ok,err=pcall(function()context:setParameter(name,0.98)end)
              assert(not ok and string.find(tostring(err),'Unknown or unretained UVI parameter',1,true))
            end
          end
          assert(not pcall(function()Program.auxs[1].inserts[2]:setParameter('Gain_1_1',0.98)end))
          assert(not pcall(function()Program.auxs[1].inserts[3]:setParameter('Gain_1_1',0.98)end))
        "#).exec().unwrap();
        assert!(host.commands.borrow().is_empty());
        assert_eq!(*host.parameters.borrow(),baseline);
    }

    #[test]
    fn native_button_and_knob_caption_defaults_preserve_empty_strings() {
        let lua = vm();
        let environment = ui_environment(&lua);
        lua.load(
            r#"
          button=Button('button');toggle=OnOffButton('toggle',false)
          knob=Knob{'knob',0.3,0,1};knob:setStripImage('strip.png',2)
          explicit=Button{name='explicit',displayName='Visible',showLabel=true,text=''}
        "#,
        )
        .set_environment(environment.clone())
        .exec()
        .unwrap();
        let snapshot = snapshot_ui(0, &environment).unwrap();
        for widget in &snapshot.widgets[..2] {
            assert_eq!(widget.display_name.as_deref(), Some(""));
            assert_eq!(widget.style.show_label, Some(false));
        }
        assert_eq!(snapshot.widgets[2].display_name.as_deref(), Some(""));
        assert_eq!(snapshot.widgets[2].style.show_label, Some(true));
        assert_eq!(snapshot.widgets[2].style.show_value, Some(true));
        assert_eq!(snapshot.widgets[3].display_name.as_deref(), Some("Visible"));
        assert_eq!(snapshot.widgets[3].style.show_label, Some(true));
        assert_eq!(snapshot.widgets[3].style.text.as_deref(), Some(""));
    }

    #[test]
    fn ui_snapshot_is_owned_scoped_and_reads_without_callbacks_or_metamethods() {
        fn send<T: Send>() {}
        send::<UiSnapshot>();
        let lua = vm();
        let a = ui_environment(&lua);
        let b = ui_environment(&lua);
        lua.load(r#"
          setSize(720,480);setBackground('../Textures/authored.png');makePerformanceView()
          calls=0
          p=Panel{name='same',bounds={10,20,100,100},visible=false,alpha=0.5}
          n=p:Knob{name='gain',value=0.25,min=0,max=1,bounds={3,4,20,30},alpha=0.5}
          n.changed=function()calls=calls+1 end
          n.displayText='Authored display override'
          n.tooltip='Authored help'
          n:setStripImage('/Textures/authored-strip.png',16,false)
          menu=Menu{name='choices',items={'First','Second'},selected=2};menu.hierarchical=true
          curve=Table{'curve',3,0,0,1};curve:setValue(2,0.5,false)
          curve.sliderColour='#804080C0';curve.drawInnerEdge=false;curve.innerEdgeColour='#101010'
          setmetatable(n._state,{__index=function()calls=calls+1;error('getter executed')end})
          setmetatable(UVI_UI_STATE.root,{__index=function()calls=calls+1;error('root getter executed')end})
          setmetatable(UVI_UI_STATE.order,{__index=function()calls=calls+1;error('order getter executed')end})
        "#).set_environment(a.clone()).exec().unwrap();
        lua.load("p=Panel{name='same',bounds={1,2,40,50}}")
            .set_environment(b.clone())
            .exec()
            .unwrap();
        let first = snapshot_ui(7, &a).unwrap();
        let other = snapshot_ui(11, &b).unwrap();
        assert!(first.processor == 7 && other.processor == 11);
        assert!(first.widgets.len() == 4 && other.widgets.len() == 1);
        assert!(first.widgets[0].id == other.widgets[0].id);
        assert!(first.widgets[0].name == other.widgets[0].name);
        let knob = &first.widgets[1];
        assert!(knob.parent == Some(1) && knob.visible && !knob.effective_visible);
        assert!(
            knob.bounds.x == 3.0
                && knob.absolute_bounds.x == 13.0
                && knob.absolute_bounds.y == 24.0
        );
        assert!(knob.effective_alpha == 0.25 && knob.has_changed_callback);
        assert!(matches!(knob.value,Some(UiValue::Number(n)) if n==0.25));
        assert!(first.widgets[2].items == ["First", "Second"]);
        assert_eq!(first.widgets[2].style.hierarchical, Some(true));
        assert!(matches!(&first.widgets[3].value,Some(UiValue::Table(v)) if v==&[0.0,0.5,0.0]));
        assert_eq!(first.widgets[3].style.slider_colour.as_deref(), Some("#804080C0"));
        assert_eq!(first.widgets[3].style.draw_inner_edge, Some(false));
        assert_eq!(first.widgets[3].style.inner_edge_colour.as_deref(), Some("#101010"));
        assert_eq!(knob.style.display_text.as_deref(), Some("Authored display override"));
        assert_eq!(knob.style.tooltip.as_deref(), Some("Authored help"));
        assert!(knob.style.strip_image.as_ref().unwrap().frames == 16);
        assert!(knob.style.strip_image.as_ref().unwrap().artwork.bank_root);
        assert!(!first.root.background.as_ref().unwrap().bank_root);
        assert!(first == snapshot_ui(7, &a).unwrap());
        assert!(a.raw_get::<u32>("calls").unwrap() == 0);
        assert!(other.widgets[0].effective_visible);
        assert_eq!(other.widgets[0].style.tooltip.as_deref(), Some("same"));
        lua.load("p.visible=true;n:setValue(0.75);n.tooltip='Updated help';menu.hierarchical=false;curve.sliderColour='#00FF00';curve.drawInnerEdge=true")
            .set_environment(a.clone())
            .exec()
            .unwrap();
        let updated = snapshot_ui(7, &a).unwrap();
        assert!(updated.widgets[1].effective_visible);
        assert_eq!(updated.widgets[3].style.slider_colour.as_deref(), Some("#00FF00"));
        assert_eq!(first.widgets[3].style.slider_colour.as_deref(), Some("#804080C0"));
        assert_eq!(updated.widgets[3].style.draw_inner_edge, Some(true));
        assert_eq!(first.widgets[3].style.draw_inner_edge, Some(false));
        assert_eq!(updated.widgets[2].style.hierarchical, Some(false));
        assert_eq!(first.widgets[2].style.hierarchical, Some(true));
        assert_eq!(updated.widgets[1].style.tooltip.as_deref(), Some("Updated help"));
        assert_eq!(first.widgets[1].style.tooltip.as_deref(), Some("Authored help"));
        assert!(matches!(updated.widgets[1].value,Some(UiValue::Number(n)) if n==0.75));
        assert!(a.raw_get::<u32>("calls").unwrap() == 1);
    }

    #[test]
    fn ui_edits_validate_before_mutation_and_use_native_callback_arguments() {
        fn send<T: Send>() {}
        send::<UiEdit>();
        let lua = vm();
        let environment = ui_environment(&lua);
        lua.load(r#"
          calls=0
          n=Knob{'gain',0.25,0,1};n.changed=function(...)assert(select('#',...)==1);calls=calls+1 end
          m=Menu{'menu',{'One','Two'}};m.changed=function(...)assert(select('#',...)==1);calls=calls+1 end
          t=Table{'cells',3,0,0,1};t.changed=function(...)local self,index=...;assert(select('#',...)==2 and index==2);calls=calls+1 end
          o=OnOffButton{'toggle',false};o.changed=function(self,mods)
            assert(type(mods)=='userdata' and mods.altDown==false and mods.commandDown==false)
            assert(not pcall(function()mods.altDown=true end));lastShift=mods.shiftDown;calls=calls+1
          end
          p=Button('push');p.changed=function(...)local self,mods=...;assert(self==p)
            if mods then assert(select('#',...)==2 and mods.shiftDown)else assert(select('#',...)==1)end
            calls=calls+1
          end
          assert(p.value==nil and p.setValue==nil);p:push(false);p:push(true);assert(calls==1)
        "#).set_environment(environment.clone()).exec().unwrap();
        let edit = |widget, value| UiEdit {
            processor: 3,
            widget,
            value,
            modifiers: UiModifiers {
                shift_down: true,
                ..UiModifiers::default()
            },
        };
        let before = snapshot_ui(3, &environment).unwrap();
        for request in [
            edit(0, UiEditValue::Number(0.5)),
            edit(99, UiEditValue::Number(0.5)),
            edit(1, UiEditValue::Boolean(true)),
            edit(1, UiEditValue::Number(f64::NAN)),
            edit(1, UiEditValue::Number(1.01)),
            edit(2, UiEditValue::Number(1.5)),
            edit(2, UiEditValue::Number(3.0)),
            edit(
                3,
                UiEditValue::TableCell {
                    index: 0,
                    value: 0.5,
                },
            ),
            edit(
                3,
                UiEditValue::TableCell {
                    index: 4,
                    value: 0.5,
                },
            ),
            edit(5, UiEditValue::Boolean(true)),
        ] {
            assert!(prepare_ui_edit(&lua, &environment, &request).is_err());
        }
        assert!(before == snapshot_ui(3, &environment).unwrap());
        assert!(environment.raw_get::<u32>("calls").unwrap() == 1);
        for request in [
            edit(1, UiEditValue::Number(0.25000000001)),
            edit(1, UiEditValue::Number(0.75)),
            edit(2, UiEditValue::Number(2.0)),
            edit(
                3,
                UiEditValue::TableCell {
                    index: 2,
                    value: 0.5,
                },
            ),
            edit(4, UiEditValue::Boolean(true)),
            edit(5, UiEditValue::Push),
        ] {
            let (setter, args) = prepare_ui_edit(&lua, &environment, &request).unwrap();
            setter.call::<()>(args).unwrap();
        }
        assert!(environment.raw_get::<u32>("calls").unwrap() == 6);
        assert!(environment.raw_get::<bool>("lastShift").unwrap());
        lua.load("n.visible=false")
            .set_environment(environment.clone())
            .exec()
            .unwrap();
        assert!(prepare_ui_edit(&lua, &environment, &edit(1, UiEditValue::Number(0.5))).is_err());
        lua.load("n.visible=true;n.enabled=false")
            .set_environment(environment.clone())
            .exec()
            .unwrap();
        assert!(prepare_ui_edit(&lua, &environment, &edit(1, UiEditValue::Number(0.5))).is_err());
        lua.load(
            "container=Panel{name='disabled',enabled=false};child=container:Knob{'child',0.25,0,1}",
        )
        .set_environment(environment.clone())
        .exec()
        .unwrap();
        assert!(prepare_ui_edit(&lua, &environment, &edit(7, UiEditValue::Number(0.5))).is_err());
    }

    #[test]
    fn ui_snapshot_paints_owned_child_order_and_reconciles_manual_reparenting() {
        let lua = vm();
        let environment = ui_environment(&lua);
        lua.load(r#"
          calls=0
          a=Panel{name='a',x=5};b=Panel{name='b',x=100}
          a1=a:Knob{name='a1',x=1,value=0.25,min=0,max=1}
          b1=b:Table{'b1',4,0,0,1};a2=a:Knob{'a2',0,0,1}
          a1.changed=function()calls=calls+1 end
          setmetatable(a._state.children,{__index=function()error('child getter executed')end,
            __pairs=function()error('child pairs callback executed')end})
        "#).set_environment(environment.clone()).exec().unwrap();
        let first = snapshot_ui(7, &environment).unwrap();
        assert_eq!(first.paint_order, [1, 3, 5, 2, 4]);
        assert_eq!(first.widgets.iter().map(|w|w.id).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
        assert_eq!(environment.raw_get::<u32>("calls").unwrap(), 0);
        lua.load("a.children={a2,a1}").set_environment(environment.clone()).exec().unwrap();
        assert_eq!(snapshot_ui(7, &environment).unwrap().paint_order, [1, 5, 3, 2, 4]);
        assert_eq!(first.paint_order, [1, 3, 5, 2, 4]);
        lua.load("a1.parent=b").set_environment(environment.clone()).exec().unwrap();
        let moved = snapshot_ui(7, &environment).unwrap();
        assert_eq!(moved.paint_order, [1, 5, 2, 4, 3]);
        assert_eq!(moved.widgets[2].parent, Some(2));
        assert_eq!(moved.widgets[2].absolute_bounds.x, 101.);
        let (setter, args) = prepare_ui_edit(&lua, &environment, &UiEdit {
            processor: 7, widget: 3, value: UiEditValue::Number(0.75),
            modifiers: UiModifiers::default(),
        }).unwrap();
        setter.call::<()>(args).unwrap();
        assert_eq!(environment.raw_get::<u32>("calls").unwrap(), 1);
        assert!(matches!(snapshot_ui(7, &environment).unwrap().widgets[2].value,
            Some(UiValue::Number(v)) if v == 0.75));
    }

    #[test]
    fn ui_snapshot_traverses_a_deep_valid_parent_tree_iteratively() {
        let lua = vm();
        let environment = ui_environment(&lua);
        lua.load(
            "p=Panel{name='root',x=1,visible=false};for i=2,4096 do p=p:Panel{name='child',x=1}end",
        )
        .set_environment(environment.clone())
        .exec()
        .unwrap();
        let snapshot = snapshot_ui(0, &environment).unwrap();
        assert!(snapshot.widgets.len() == 4096);
        assert_eq!(snapshot.paint_order.len(), 4096);
        assert_eq!(snapshot.paint_order.last(), Some(&4096));
        assert!(snapshot.widgets[4095].absolute_bounds.x == 4096.0);
        assert!(!snapshot.widgets[4095].effective_visible);
    }

    #[test]
    fn ui_snapshot_rejects_parent_cycles_bad_bounds_and_oversized_state() {
        let lua = vm();
        for (source, diagnostic) in [
            ("p=Panel('p');q=p:Panel('q');p.parent=q", "Cyclic"),
            ("p=Panel('p');p.parent={}", "outside processor scope"),
            ("p=Panel('p');p.width=-1", "extent"),
            ("p=Panel('p');p.x=0/0", "numeric"),
            ("p=Panel('p');p.width=16385", "extent"),
            ("p=Label('p');p.fontSize=1e100", "font size"),
            (
                "p=Panel{name='p',x=1048576};q=p:Panel{name='q',x=1}",
                "ancestor position",
            ),
            (
                "p=Panel('p');for i=2,4097 do UVI_UI_STATE.order[i]=p end",
                "item limit",
            ),
            ("p=Panel(string.rep('x',4097))", "text limit"),
            ("p=Knob('p');p.displayText=string.rep('x',4097)", "text limit"),
            ("p=Knob('p');p.tooltip=string.rep('x',4097)", "text limit"),
            ("p=Menu('p');p.hierarchical='yes'", "Boolean"),
            ("p=Table('p');p.drawInnerEdge='yes'", "Boolean"),
            ("p=Table('p');p.sliderColour=4", "text field"),
            ("p=Table('p');p.innerEdgeColour=4", "text field"),
            ("p=Panel('p');c=p:Knob('c');p.children={c,c}", "child reference"),
            ("p=Panel('p');p.children={p}", "child reference"),
            ("p=Panel('p');p.children={{}}", "outside processor scope"),
            ("p=Panel('p');p.children={false}", "child reference"),
            ("p=Panel('p');p.children=4", "child list"),
            ("p=Panel('p');p.children={};for i=1,4097 do p.children[i]=p end", "item limit"),
            (
                "p=Menu('p',{});for i=1,300 do p._state.items[i]=string.rep('x',4096)end",
                "text limit",
            ),
            (
                "p=Panel('p');p.image='https://invalid.example/image.png'",
                "artwork reference",
            ),
            (
                "p=Panel('p');p.image='//outside/image.png'",
                "artwork reference",
            ),
            (
                "p=Knob('p');p:setStripImage('authored.png',0,false)",
                "sprite count",
            ),
            ("p=Panel('p');UVI_UI_STATE.order[3]=p", "sequence"),
        ] {
            let environment = ui_environment(&lua);
            lua.load(source)
                .set_environment(environment.clone())
                .exec()
                .unwrap();
            let error = snapshot_ui(0, &environment)
                .err()
                .expect("malformed UI must fail");
            assert!(
                error.to_string().contains(diagnostic),
                "expected fixed diagnostic {diagnostic}"
            );
        }
    }

    #[test]
    fn host_resolves_only_unambiguous_approved_relative_modules() {
        let lua = vm();
        let counter =
            b"local name=...;counter=(counter or 0)+1;return {name=name,count=counter}".to_vec();
        let modules = BTreeMap::from([
            (
                "Scripts.MIDI Scripts._Folder.Counter".into(),
                counter.clone(),
            ),
            ("ApprovedAlias._Folder.Counter".into(), counter),
            ("First._Conflict.Main".into(), b"return 'first'".to_vec()),
            ("Second._Conflict.Main".into(), b"return 'second'".to_vec()),
            ("_Conflict/Main".into(), b"return 'exact'".to_vec()),
            (
                "Scripts._Loop.Main".into(),
                b"return require('_Loop/Main')".to_vec(),
            ),
        ]);
        install(
            &lua,
            HostConfig {
                program: None,
                modules,
                now: Rc::new(|| 0),
                resources: None,
                valid_voice: None,
                layer_scope: None,
            },
        )
        .unwrap();
        lua.load(r#"
          local m=require('_Folder/Counter')
          assert(m==require('_Folder/Counter') and m.count==1 and m.name=='_Folder/Counter')
          assert(require('_Conflict/Main')=='exact')
          local ok,err=pcall(require,'_Conflict.Main');assert(not ok and string.find(tostring(err),'ambiguous'))
          assert(not pcall(require,'_Loop/Main'))
          assert(not pcall(require,'outside/Counter'))
          assert(not pcall(require,''))
          assert(not pcall(require,string.rep('x',257)))
          assert(require('uvi.ChordRec')==true)
          local extend=class'AuthoredBase';assert(type(extend)=='function' and type(AuthoredBase)=='userdata')
          AuthoredBase.static=7
          function AuthoredBase:__init(x)self.x=x end
          function AuthoredBase:sum()return self.x+AuthoredBase.static end
          local a=AuthoredBase(4);a.y=9
          assert(type(a)=='userdata' and a.x==4 and a.y==9 and a:sum()==11 and a.static==7 and a.missing==nil and a==a)
          assert(not pcall(function()return a==AuthoredBase(4)end))
          assert(not pcall(function()return tostring(a)end))
          assert(class'AuthoredChild'(AuthoredBase)==nil)
          assert(not pcall(function()AuthoredChild(3)end))
          function AuthoredChild:__init(x)self.x=x*2 end
          local child=AuthoredChild(5);assert(child.x==10 and child:sum()==17 and child.static==7)
          AuthoredBase.static=10;function AuthoredBase:sum()return 1000 end
          assert(child:sum()==20 and child.static==7)
          local rec=ChordRec;assert(require('uvi.ChordRec')==true and rec==ChordRec)
          local root,kind,bass=rec.chordKind{60,64,67};assert(root==0 and kind=='M' and bass==0)
          root,kind,bass=rec.chordKind{64,67,72};assert(root==0 and kind=='M' and bass==4)
          root,kind,bass=rec.chordKind{69,60,64,67};assert(root==9 and kind=='m7' and bass==9)
          root,kind,bass=rec.chordKind{64,60,67,69};assert(root==0 and kind=='6' and bass==4)
          root,kind,bass=rec.chordKind{64,69,60,67};assert(root==9 and kind=='m7' and bass==4)
          root,kind,bass=rec.chordKind{60.5,64.5,67.5};assert(root==0.5 and kind=='M' and bass==0.5)
          assert(rec.chordKind{60,61}==nil and rec.chordKind{60}==nil)
          assert(rec.getChromaString(rec.getChroma(0,{60,64,67}))=='100010010000')
          assert(not pcall(rec.chordKind,{}))
          assert(require('uvi.AsyncUpdater')==true and type(AsyncUpdater)=='userdata')
          local factory=AsyncUpdater;require('uvi.AsyncUpdater');assert(AsyncUpdater==factory)
          local delays={};local count=0;local u
          wait=function(ms)table.insert(delays,ms);u:trigger(99)end
          u=AsyncUpdater(function(...)assert(select('#',...)==0);count=count+1;u:trigger(0)end)
          assert(type(u)=='userdata' and type(u.trigger)=='function' and type(u.callback)=='function' and u.pending==nil and u.cancel==nil)
          u:trigger(20);assert(count==1 and #delays==1 and delays[1]==20)
          u:trigger(30);assert(count==2 and #delays==2 and delays[2]==30)
          u.callback=function()count=count+10 end;u:trigger(0);assert(count==12)
        "#).exec().unwrap();
    }
    #[test]
    fn unowned_context_writes_fail_before_parameter_or_command_mutation() {
        let program = parse_program(r#"<Program Name="P" Gain="0.8"/>"#).unwrap();
        let lua = vm();
        let host = install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 17),
                resources: None,
                valid_voice: None,
                layer_scope: None,
            },
        )
        .unwrap();
        let before = host.parameters.borrow().clone();
        let program_object: Table = lua.globals().get("Program").unwrap();
        let part: Table = program_object.get("parent").unwrap();
        let synth: Table = part.get("parent").unwrap();
        assert_eq!(host.object_id(&part).unwrap(), program.nodes.len());
        assert_eq!(host.object_id(&synth).unwrap(), program.nodes.len() + 1);
        lua.load(r#"
          local part=Program.parent
          local synth=part.parent
          -- Public fields do not grant or alter object ownership.
          part._nodeId=0;part.type='Program'
          local function rejected(object,owner,name,value)
            local ok,err=pcall(function()object:setParameter(name,value)end)
            assert(not ok)
            assert(string.find(tostring(err),'Unowned UVI '..owner..' context parameter '..name,1,true))
          end
          -- Even unchanged defaults require a real owner to consume the write.
          rejected(part,'Part','MidiChannel',-1)
          rejected(part,'Part','MidiInput',-1)
          rejected(part,'Part','Gain',0.5)
          rejected(part,'Part','Pan',0.25)
          rejected(part,'Part','Bypass',true)
          rejected(synth,'Synth','Gain',0.5)
          rejected(synth,'Synth','Pan',0.25)
          rejected(synth,'Synth','Bypass',true)
          -- Preserve measured mismatched-type handling.
          part:setParameter('MidiChannel',false)
          assert(part:getParameter('MidiChannel')==-1)
        "#).exec().unwrap();
        assert_eq!(*host.parameters.borrow(), before);
        assert!(host.commands.borrow().is_empty());
        lua.load(r#"
          Program:setParameter('Gain',0.4)
          assert(Program:getParameter('Gain')==0.4)
          assert(not pcall(function()Program.setParameter({_nodeId=0},'Gain',0.2)end))
          assert(not pcall(function()Program:setParameter('Unretained',1)end))
        "#).exec().unwrap();
        let commands = host.commands.borrow();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0].action,
            Action::Parameter { node, parameter, value }
                if *node == program.root && parameter == "Gain"
                    && *value == ParameterValue::Number(0.4)
        ));
        assert_eq!(commands[0].frame, 17);
    }

    #[test]
    fn host_graph_modules_units_controls_and_timed_mutations() {
        let program=parse_program(r#"<Program Name="P" Gain="0.8"><Layers><Layer Name="L" Gain="0.6"><Keygroups><Keygroup Name="K" Gain="0.4"><Oscillators><SamplePlayer Name="S" BaseNote="60" SamplePath="synthetic.wav"/></Oscillators><Connections><SignalConnection Name="C" Destination="Gain" Source="X" Ratio="0.2"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let lua = vm();
        let clock = Rc::new(std::cell::Cell::new(17));
        let now = clock.clone();
        let mut modules = BTreeMap::new();
        modules.insert(
            "counter".into(),
            b"counter=(counter or 0)+1;return {value=counter}".to_vec(),
        );
        modules.insert("loop".into(), b"return require('loop')".to_vec());
        let host = install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules,
                now: Rc::new(move || now.get()),
                resources: None,
                valid_voice: Some(Rc::new(|id| id == 1)),
                layer_scope: None,
            },
        )
        .unwrap();
        lua.globals()
            .set(
                "authoredVoice",
                super::super::script::voice_handle(&lua, 1).unwrap(),
            )
            .unwrap();
        lua.load(r#"
          assert(__API_VERSION__==23)
          assert(Mapper.Linear==0 and Mapper.Exponential==1 and Mapper.Quintic==9)
          local unnamed=Panel{};local view=unnamed:Viewport{}
          assert(unnamed.name=='' and view.name=='' and view.parent==unnamed and unnamed.children[1]==view)
          assert(not pcall(function()Knob{}end))
          local x=Knob{'axisX',0.25,0,1};local y=Knob{'axisY',0.75,-1,1}
          local xy=view:XY{'axisX','axisY',bounds={2,3,128,64}}
          assert(xy.name=='XY_axisX_axisY' and xy.paramX=='axisX' and xy.paramY=='axisY' and xy.parent==view)
          assert(xy.x==2 and xy.y==3 and xy.width==128 and xy.height==64 and xy.value==nil)
          local axisCalls=0;local xyCalls=0;x.changed=function()axisCalls=axisCalls+1 end;xy.changed=function()xyCalls=xyCalls+1 end
          x:setValue(0.5);assert(axisCalls==1 and xyCalls==0 and UVI_UI_STATE.widgets[xy.paramX]==x)
          xy.paramX='axisY';assert(xy.paramX=='axisY' and xy.name=='XY_axisX_axisY')
          assert(not pcall(function()xy:setValue(0.2,0.3)end))
          assert(not pcall(function()XY{'axisX'}end))
          local wave=WaveView{'authored-wave',size={128,64},hiWaveColour='#FF0000'};wave.visible=false
          assert(wave.kind=='WaveView' and wave.sample=='' and wave.visible==false and wave.width==128 and wave.height==64)
          assert(UVI_UI_STATE.widgets['authored-wave']==wave and wave.hiWaveColour=='#FF0000')
          assert(not pcall(function()wave:setValue(0.5)end))
          local l=Program.layers[1];local k=l.keygroups[1]
          assert(l==Program.children.L and k.parent==l)
          assert(k:getParameter('Gain')==0.4 and k.oscillators[1]:getParameter('BaseNote')==60)
          k._nodeId=0 -- object identity stays bound to the original graph node
          k:setParameter('Gain',0.7);assert(k:getParameter('Gain')==0.7)
          assert(not pcall(function()k.setParameter({_nodeId=0},'Gain',0.2)end))
          local c=k:getParameterConnections('Gain')[1];c:setParameter('Ratio',0.3)
          assert(c:getParameter('Ratio')==0.3)
          assert(not pcall(function() return k:getParameter('Invented') end))
          k:setParameter('Gain',true);assert(k:getParameter('Gain')==0.7)
          assert(not pcall(function() k:setParameter('Gain',{}) end))
          assert(require('counter')==require('counter') and counter==1)
          assert(not pcall(function()require('loop')end))
          assert(not pcall(function()require('disk/module')end))
          local knob=Knob{'n',0.25,0,1,unit=Unit.PercentNormalized}
          local called=0;knob.changed=function(self)called=called+1;k:setParameter('Gain',self.value)end
          knob:setValue(0.6,false);assert(called==0 and math.abs(knob.value-0.6)<1e-6)
          knob:setValue(0.6);assert(called==0)
          knob.value=0.9;assert(called==1 and math.abs(k:getParameter('Gain')-0.9)<1e-6)
          assert(Unit.MilliSeconds==5 and Unit.SemiTones==14 and Unit.PercentNormalized==2)
          local ms=Knob{'ms',1500,0,2000,unit=Unit.MilliSeconds};assert(ms.value==1500)
          local pct=Knob{'pct',75,0,100,unit=Unit.Percent};assert(pct.value==75)
          local t=Table{'t',3,0,0,1};t.changed=function(self,index)called=called+index end
          t:setValue(2,0.4);assert(math.abs(t:getValue(2)-0.4)<1e-6 and called==3)
          t:setValue(0,1);t:setValue(4,1);assert(t:getValue(0)==0 and t:getValue(4)==0 and called==3)
          local ints=Knob{'ints',0,-5,5,true};ints:setValue(-1.8,false);assert(ints.value==-1);ints:setValue(6,false);assert(ints.value==6)
          local defaults=Table{'defaults',2,0.25,0,1};defaults:setValue(1.8,2,false);assert(defaults:getValue(1)==2 and defaults:getValue(0.8)==0.25)
          assert(type(authoredVoice)=='userdata')
          sendScriptModulation(3,0.4,100,authoredVoice)
          assert(not pcall(function()sendScriptModulation(3,0.4,100,1)end))
          assert(not pcall(function()sendScriptModulation(3,0.4,100,99999)end))
          assert(not pcall(function()sendScriptModulation2(3,0.1,0.4,100,99999)end))
          sendScriptModulation2(4,0.1,0.2,0,authoredVoice)
        "#).exec().unwrap();
        let commands = host.commands.borrow();
        assert_eq!(commands.len(), 5);
        assert!(commands.iter().all(|c| c.frame == 17));
        assert!(
            matches!(&commands[1].action,Action::Parameter{parameter,value:ParameterValue::Number(n),..} if parameter=="Ratio" && *n==0.3)
        );
        assert!(matches!(
            &commands[3].action,
            Action::ScriptModulation {
                ramp_ms: 100.,
                voice: Some(1),
                ..
            }
        ));
        assert!(matches!(
            commands[4].action,
            Action::ScriptModulation {
                start: Some(0.1),
                target: 0.2,
                voice: Some(1),
                ..
            }
        ));
    }

    #[test]
    fn audio_resource_validation_preserves_previous_asset_and_checks_arguments_before_reads() {
        let program=parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="old.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let lua = vm();
        let reads = Rc::new(Cell::new(0));
        let observed = reads.clone();
        let host = install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 17),
                valid_voice: None,
                layer_scope: None,
                resources: Some(Rc::new(move |request| {
                    observed.set(observed.get() + 1);
                    match request {
                        ResourceRequest::ReadAudio { path, .. } if path == "wrong-response.wav" => {
                            Ok(ResourceResponse::Saved)
                        }
                        ResourceRequest::ReadAudio { path, .. } => {
                            Ok(ResourceResponse::Audio(ResourceInfo {
                                name: path.clone(),
                                rate: if path == "invalid-metadata.wav" {
                                    0
                                } else {
                                    48_000
                                },
                                channels: 1,
                                frames: 256,
                            }))
                        }
                        _ => Err(mlua::Error::runtime("Unexpected authored resource request")),
                    }
                })),
            },
        )
        .unwrap();
        lua.load(
            r#"
            local oscillator=Program.layers[1].keygroups[1].oscillators[1]
            assert(loadSample(oscillator,'owned.wav').success)
            local previous=oscillator.sampleInfo
            local completed=0
            for _,path in ipairs{'invalid-metadata.wav','wrong-response.wav'}do
                local task=loadSample(oscillator,path,function(t)
                    completed=completed+1;assert(t.finished and not t.success and t.error)
                end)
                assert(not task.success and oscillator.sampleInfo==previous)
                assert(oscillator:getParameter('SamplePath')=='owned.wav')
            end
            assert(completed==2)
            assert(not pcall(function()loadSample(Program.layers[1],'owned.wav')end))
            assert(not pcall(function()loadImpulse(oscillator,'owned.wav')end))
            assert(not pcall(function()loadSample(oscillator,'')end))
            assert(not pcall(function()loadSample(oscillator,'nul\0path')end))
            assert(not pcall(function()loadSample(oscillator,'owned.wav',true)end))
            assert(not pcall(function()loadSample({},'owned.wav')end))
        "#,
        )
        .exec()
        .unwrap();
        assert_eq!(reads.get(), 3);
        assert_eq!(host.commands.borrow().len(), 1);
        let node = program
            .nodes
            .iter()
            .position(|node| node.kind == "SamplePlayer")
            .unwrap();
        assert_eq!(
            host.loaded_resources.borrow().get(&node),
            Some(&(ResourceKind::Sample, "owned.wav".into()))
        );
    }

    #[test]
    fn host_restores_original_values_and_completes_failed_resource_tasks() {
        let program=parse_program(r#"<Program Name="P"><EventProcessors><ScriptProcessor Name="Script" n="0.8" enabled="1"><ScriptData t="0,100000 0,750000"/></ScriptProcessor></EventProcessors><Layers><Layer Name="L"><Keygroups><Keygroup Name="K"><Oscillators><SamplePlayer Name="S" SamplePath="synthetic.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let lua = vm();
        let host = install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 31),
                resources: None,
                valid_voice: None,
                layer_scope: None,
            },
        )
        .unwrap();
        lua.load(r#"
          called=0
          n=Knob{'n',0,0,1};n.changed=function(self)called=called+1;assert(math.abs(self.value-0.8)<1e-6 and t:getValue(2)==0.75)end
          t=Table{'t',2,0,0,1};t.changed=function(self,index)called=called+1 end
          enabled=OnOffButton{'enabled',false}
          completed=0
          loadSample(Program.layers[1].keygroups[1].oscillators[1],'approved-resource',function(task)completed=completed+1;assert(task.finished and not task.success and task.error)end)
          assert(completed==1)
        "#).exec().unwrap();
        assert_eq!(restore_widgets(&lua, &program).unwrap(), 3);
        lua.load("assert(called==3 and enabled.value==true and math.abs(t:getValue(1)-0.1)<1e-6)")
            .exec()
            .unwrap();
        let commands = host.commands.borrow();
        assert!(commands.is_empty());
    }
    #[test]
    fn host_parameter_widget_with_malformed_setter_still_fails_restoration() {
        let program=parse_program(r#"<Program><EventProcessors><ScriptProcessor gain="0.5"/></EventProcessors></Program>"#).unwrap();
        let lua = vm();
        install_ui(&lua, &lua.globals()).unwrap();
        lua.load(
            r#"
          gain=Knob{'gain',0,0,1}
          local original=getmetatable(gain).__index
          setmetatable(gain,{__index=function(self,key)
            if key=='setValue' then return nil end
            return original(self,key)
          end})
        "#,
        )
        .exec()
        .unwrap();
        let failure = restore_widgets(&lua, &program).unwrap_err();
        assert!(
            failure
                .to_string()
                .contains("error converting Lua nil to function")
        );
    }

    #[test]
    fn host_unchanged_persisted_widget_does_not_invoke_callback() {
        let program=parse_program(r#"<Program><EventProcessors><ScriptProcessor n="0.6"><ScriptData t="0,250000 0,750000"/></ScriptProcessor></EventProcessors></Program>"#).unwrap();
        let lua = vm();
        install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 0),
                resources: None,
                valid_voice: None,
                layer_scope: None,
            },
        )
        .unwrap();
        lua.load(
            r#"
          n=Knob{'n',0.6,0,1};n.changed=function()error('unchanged callback')end
          t=Table{'t',2,0.25,0,1};t:setValue(2,0.75,false)
          t.changed=function()error('unchanged table callback')end
        "#,
        )
        .exec()
        .unwrap();
        assert_eq!(restore_widgets(&lua, &program).unwrap(), 2);
    }

    #[test]
    fn host_approved_capabilities_preserve_data_state_and_audio_results() {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="old.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let lua = vm();
        let requests = Rc::new(RefCell::new(Vec::new()));
        let recorded = requests.clone();
        let saved = Rc::new(RefCell::new(Vec::new()));
        let storage = saved.clone();
        let host = install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 42),
                valid_voice: None,
                layer_scope: None,
                resources: Some(Rc::new(move |request| {
                    recorded.borrow_mut().push(request.clone());
                    Ok(match request {
                        ResourceRequest::ReadAudio { path, .. } if path == "owned.wav" => {
                            ResourceResponse::Audio(ResourceInfo {
                                name: "owned.wav".into(),
                                rate: 48_000,
                                channels: 2,
                                frames: 24_000,
                            })
                        }
                        ResourceRequest::ReadData { path } if path == "owned.json" => {
                            ResourceResponse::Bytes(
                                br#"{"name":"authored","values":[2,3]}"#.to_vec(),
                            )
                        }
                        ResourceRequest::ReadData { path } if path == "invalid.json" => {
                            ResourceResponse::Bytes(b"invalid".to_vec())
                        }
                        ResourceRequest::ReadData { path } if path == "null.json" => {
                            ResourceResponse::Bytes(
                                br#"{"missing":null,"values":[1,null,3]}"#.to_vec(),
                            )
                        }
                        ResourceRequest::ReadState { path } if path == "invalid.state" => {
                            ResourceResponse::Bytes(b"<bad/>".to_vec())
                        }
                        ResourceRequest::WriteState { path, bytes } if path == "owned.state" => {
                            *storage.borrow_mut() = bytes.clone();
                            ResourceResponse::Saved
                        }
                        ResourceRequest::ReadState { path } if path == "owned.state" => {
                            ResourceResponse::Bytes(storage.borrow().clone())
                        }
                        ResourceRequest::Browse { .. } => ResourceResponse::Selected(None),
                        _ => return Err(mlua::Error::runtime("Resource is not approved")),
                    })
                })),
            },
        )
        .unwrap();
        lua.load(r#"
            local oscillator=Program.layers[1].keygroups[1].oscillators[1]
            Program.layers[1].type='SamplePlayer'
            assert(not pcall(function()loadSample(Program.layers[1],'owned.wav')end))
            oscillator.type='Layer' -- target type, like node identity, stays bound to the graph
            local task=loadSample(oscillator,'owned.wav',function(t)assert(t.success and t.finished and t.state=='finished')end)
            assert(task.id==1 and oscillator.sampleInfo.duration==500 and oscillator.sampleInfo.samplerate==48000)
            assert(oscillator:getParameter('SamplePath')=='owned.wav')
            local completed=false
            local bad=loadSample(oscillator,'unapproved.wav',function(t)completed=true;assert(t.finished and not t.success and t.error)end);assert(completed and not bad.success and oscillator:getParameter('SamplePath')=='owned.wav')
            local json=loadData('owned.json',function(data)assert(data.name=='authored' and data.values[2]==3)end)
            assert(json.success and string.find(json.data,'authored'))
            local badJSON=loadData('invalid.json',function()error('failed JSON decoding must not call completion')end);assert(badJSON.success and badJSON.data=='invalid' and badJSON.error)
            local nullCalled=false
            loadData('null.json',function(data)nullCalled=true;assert(data.missing==nil and data.values[2]==nil and data.values[3]==3)end)
            assert(nullCalled)
            local badState=loadState('invalid.state',function()error('invalid state must not call completion')end);assert(not badState.success)
            k=Knob{'k',0.8,0,1};t=Table{'t',2,0.3,0,1};events={}
            function onSave()return{foo=7,values={1,2},text='<&'}end
            saveState('owned.state',function(task)assert(task.success)end)
            k:setValue(0.2,false);t:setValue(1,0.1,false);t:setValue(2,0.1,false)
            k.changed=function()table.insert(events,'changed')end
            t.changed=function(_,i)table.insert(events,'table'..i)end
            function onLoad(data)assert(data.foo==7 and data.values[2]==2 and data.text=='<&');assert(math.abs(k.value-0.8)<1e-6);table.insert(events,'load')end
            function onInit()error('loadState must not run onInit')end
            loadState('owned.state',function(task)assert(task.success);table.insert(events,'done')end)
            assert(table.concat(events,',')=='table1,table2,changed,load,done')
            local cancelled=browseForFile('open','Choose','','*.wav');assert(not cancelled.success and cancelled.state=='cancelled' and cancelled.result=='')
            k.changed=function()error('callback error propagates')end;k:setValue(0.1,false)
            assert(not pcall(function()loadState('owned.state')end))
        "#).exec().unwrap();
        assert_eq!(host.commands.borrow().len(), 1);
        assert!(
            host.commands
                .borrow()
                .iter()
                .all(|command| command.frame == 42)
        );
        assert_eq!(requests.borrow().len(), 10);
        let text = String::from_utf8(saved.borrow().clone()).unwrap();
        assert!(text.contains("<UVI4><ScriptProcessor") && text.contains("<ScriptData t="));
    }

    #[test]
    fn host_menu_mutations_and_default_capability_denial_are_explicit() {
        let lua = vm();
        install(
            &lua,
            HostConfig {
                program: None,
                modules: BTreeMap::new(),
                now: Rc::new(|| 0),
                resources: None,
                valid_voice: None,
                layer_scope: None,
            },
        )
        .unwrap();
        lua.load(r#"
            local menu=Menu{'menu',{'first','second'}};local changed=0;menu.changed=function()changed=changed+1 end
            assert(menu:getText(1.8)=='first' and menu:getText(-1)=='')
            assert(not pcall(function()menu:getText()end))
            menu:clear();assert(menu.length==0 and menu.value==1 and menu.text=='')
            assert(menu:addItem('new')==1 and menu:addItem('other')==2)
            menu:setItem(1.8,'renamed');assert(menu:getText(1)=='renamed' and changed==0)
            assert(not pcall(function()menu:setItem(0,'bad')end))
            local data=loadData('unapproved',function()error('denied data must not call completion')end);assert(not data.success and data.error)
            local state=loadState('unapproved');assert(not state.success)
            local save=saveState('unapproved');assert(not save.success)
            local browse=browseForFile('open','Choose','','');assert(not browse.success and browse.error)
            sendScriptModulation(0,0.1)
            assert(not pcall(function()sendScriptModulation(0,0.1,20,1)end))
        "#).exec().unwrap();
    }

    #[test]
    fn host_persistence_notifies_tables_first_and_stops_at_callback_errors() {
        let program = parse_program(r#"<Program><EventProcessors><ScriptProcessor z="0.8" a="0.9"><ScriptData ta="0.1 0.75" tz="0.2 0.65"/></ScriptProcessor></EventProcessors></Program>"#).unwrap();
        let lua = vm();
        install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 0),
                resources: None,
                valid_voice: None,
                layer_scope: None,
            },
        )
        .unwrap();
        lua.load(r#"
          z=Knob{'z',0,0,1};a=Knob{'a',0,0,1};tz=Table{'tz',2,0,0,1};ta=Table{'ta',2,0,0,1};events={}
          tz.changed=function(_,i)assert(z.value==0 and a.value==0);if i==1 then assert(tz:getValue(2)==0)end;table.insert(events,'tz'..i)end
          ta.changed=function(_,i)assert(z.value==0 and a.value==0);table.insert(events,'ta'..i)end
          z.changed=function()assert(a.value==0);table.insert(events,'z')end
          a.changed=function()table.insert(events,'a')end
        "#).exec().unwrap();
        assert_eq!(restore_widgets(&lua, &program).unwrap(), 4);
        lua.load(
            r#"
          assert(table.concat(events,',')=='tz1,tz2,ta1,ta2,z,a')
          z:setValue(0,false);a:setValue(0,false);tz:setValue(1,0,false);tz:setValue(2,0,false)
          tz.changed=function()error('stop here')end
        "#,
        )
        .exec()
        .unwrap();
        assert!(restore_widgets(&lua, &program).is_err());
        lua.load("assert(z.value==0 and a.value==0 and tz:getValue(2)==0)")
            .exec()
            .unwrap();
    }

    #[test]
    fn host_large_graph_keeps_objects_in_lua_without_exhausting_reference_stack() {
        let mut xml = String::from(
            r#"<Program><Layers><Layer Name="L"><Keygroups><Keygroup Name="K" Gain="1"><Connections>"#,
        );
        for i in 0..9000 {
            xml.push_str(&format!(
                r#"<SignalConnection Name="C{i}" Source="X" Destination="Gain" Ratio="0.1"/>"#
            ));
        }
        xml.push_str("</Connections></Keygroup></Keygroups></Layer></Layers></Program>");
        let program = parse_program(&xml).unwrap();
        assert!(program.nodes.len() > 7996);
        let lua = vm();
        let host = install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 23),
                resources: None,
                valid_voice: None,
                layer_scope: None,
            },
        )
        .unwrap();
        lua.gc_collect().unwrap();
        lua.load(
            r#"
          local k=Program.layers[1].keygroups[1]
          local connections=k:getParameterConnections('Gain')
          assert(#connections==9000 and connections[9000].name=='C8999')
          assert(connections[9000].parent==k and k.parent==Program.layers[1])
          connections[9000]:setParameter('Ratio',0.2)
          assert(k:getParameterConnections('Gain')[9000]==connections[9000])
        "#,
        )
        .exec()
        .unwrap();
        assert!(
            matches!(host.commands.borrow()[0], Command {frame: 23,action: Action::Parameter {node, ..}} if node > 7996)
        );
        // Exhaustion of the configured Lua heap remains a recoverable Lua error.
        let small = vm();
        small.set_memory_limit(128 << 10).unwrap();
        assert!(
            install(
                &small,
                HostConfig {
                    program: Some(&program),
                    modules: BTreeMap::new(),
                    now: Rc::new(|| 0),
                    resources: None,
                    valid_voice: None,
                    layer_scope: None,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn host_scopes_isolate_modules_widgets_and_state_while_sharing_engine_objects() {
        let program = parse_program(r#"<Program><EventProcessors><ScriptProcessor Name="A" n="0.75"/></EventProcessors><Layers><Layer Name="L" Gain="1"><EventProcessors><ScriptProcessor Name="B" n="0.5"/></EventProcessors></Layer></Layers></Program>"#).unwrap();
        let processors = program
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.kind == "ScriptProcessor")
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let lua = vm();
        let storage = Rc::new(RefCell::new(BTreeMap::new()));
        let files = storage.clone();
        let host = install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::from([(
                    "scope".into(),
                    b"moduleCount=(moduleCount or 0)+1;return {owner=this.name,count=moduleCount}"
                        .to_vec(),
                )]),
                now: Rc::new(|| 11),
                valid_voice: None,
                layer_scope: None,
                resources: Some(Rc::new(move |request| {
                    Ok(match request {
                        ResourceRequest::WriteState { path, bytes } => {
                            files.borrow_mut().insert(path.clone(), bytes.clone());
                            ResourceResponse::Saved
                        }
                        ResourceRequest::ReadState { path } => ResourceResponse::Bytes(
                            files
                                .borrow()
                                .get(path)
                                .cloned()
                                .ok_or_else(|| mlua::Error::runtime("Unapproved state"))?,
                        ),
                        _ => return Err(mlua::Error::runtime("Unapproved resource")),
                    })
                })),
            },
        )
        .unwrap();
        let a = host
            .script_environment(&lua, &program, processors[0])
            .unwrap();
        let b = host
            .script_environment(&lua, &program, processors[1])
            .unwrap();
        lua.load(r#"
          assert(sentinel==nil and _G.sentinel==nil);sentinel='A';table.scopeSentinel='A'
          local first=require('scope');assert(first==require('scope') and first.owner=='A' and first.count==1)
          require('uvi.ChordRec');ChordRec.scopeSentinel='A'
          class'ScopeClass';function ScopeClass:__init()self.owner='A'end
          n=Knob{'n',0.1,0,1};n.changed=function()Program.layers[1]:setParameter('Gain',n.value)end
          function onSave()return{owner='A'}end
          function onLoad(data)assert(data.owner=='A');loaded='A'end
        "#).set_environment(a.clone()).exec().unwrap();
        lua.load(r#"
          assert(sentinel==nil and _G.sentinel==nil and table.scopeSentinel==nil);sentinel='B'
          local first=require('scope');assert(first==require('scope') and first.owner=='B' and first.count==1)
          require('uvi.ChordRec');assert(ChordRec.scopeSentinel==nil)
          assert(ScopeClass==nil);class'ScopeClass';function ScopeClass:__init()self.owner='B'end
          assert(this.parent==Program.layers[1])
          n=Knob{'n',0.2,0,1};n.changed=function()Program.layers[1]:setParameter('Gain',n.value)end
          function onSave()return{owner='B'}end
          function onLoad(data)assert(data.owner=='B');loaded='B'end
        "#).set_environment(b.clone()).exec().unwrap();
        assert_eq!(
            a.get::<Table>("Program").unwrap(),
            b.get::<Table>("Program").unwrap()
        );
        assert_eq!(
            host.object_id(&a.get::<Table>("this").unwrap()).unwrap(),
            processors[0]
        );
        assert_eq!(
            host.object_id(&b.get::<Table>("this").unwrap()).unwrap(),
            processors[1]
        );
        restore_widgets_scoped(&lua, &program, processors[0], &a).unwrap();
        lua.load(
            "assert(Program.layers[1]:getParameter('Gain')==0.75 and math.abs(n.value-0.2)<1e-6)",
        )
        .set_environment(b.clone())
        .exec()
        .unwrap();
        restore_widgets_scoped(&lua, &program, processors[1], &b).unwrap();
        lua.load("assert(Program.layers[1]:getParameter('Gain')==0.5 and n.value==0.75)")
            .set_environment(a.clone())
            .exec()
            .unwrap();
        lua.load("local task=saveState('A.state');assert(task.success);n:setValue(0.3,false);assert(loadState('A.state').success and loaded=='A' and n.value==0.75)").set_environment(a.clone()).exec().unwrap();
        lua.load("assert(loaded==nil and n.value==0.5);assert(saveState('B.state').success);n:setValue(0.4,false);assert(loadState('B.state').success and loaded=='B' and n.value==0.5)").set_environment(b.clone()).exec().unwrap();
        assert!(lua.globals().get::<Value>("sentinel").unwrap().is_nil());
        assert!(lua.globals().get::<Value>("onSave").unwrap().is_nil());
        assert_eq!(storage.borrow().len(), 2);
        assert!(restore_widgets(&lua, &program).is_err());
        assert!(
            host.script_environment(&lua, &program, program.root)
                .is_err()
        );
        assert!(
            host.commands
                .borrow()
                .iter()
                .all(|command| command.frame == 11)
        );
    }
}
