//! Stateful UVI widgets lowered to the shared UI IR. Lua runs only on its owner.
use super::ScriptHost;
use mlua::{Function, Table, Value};
use sampler_ui_ir as ui;
use std::collections::{BTreeMap, HashMap, HashSet};

pub fn control_id(widget: usize, component: usize) -> ui::ControlId {
    ui::ControlId(0x55564900000000000000000000000000 | ((widget as u128) << 32) | component as u128)
}
fn identity(id: ui::ControlId) -> Option<(usize, usize)> {
    ((id.0 >> 64) == 0x5556490000000000).then_some((
        ((id.0 >> 32) & 0xffff_ffff) as usize,
        (id.0 & 0xffff_ffff) as usize,
    ))
}
// Canonical reads retain Lua's __index semantics. The private projection
// specialization below may supply the same scalar values from a fresh raw pass.
trait UiRead {
    fn read(&self, key: &str) -> Option<Value>;
}
impl UiRead for Table {
    fn read(&self, key: &str) -> Option<Value> {
        self.get::<Value>(key).ok()
    }
}
fn text(t: &impl UiRead, key: &str) -> Option<String> {
    match t.read(key)? {
        Value::String(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}
fn mapper(t: &impl UiRead) -> Option<String> {
    text(t, "mapper").or_else(|| {
        let id = num(t, "mapper")?;
        if id.fract() != 0. || !(0. ..=9.).contains(&id) {
            return None;
        }
        [
            "Linear",
            "Exponential",
            "QuinticRoot",
            "QuarticRoot",
            "CubeRoot",
            "SquareRoot",
            "Quadratic",
            "Cubic",
            "Quartic",
            "Quintic",
        ]
        .get(id as usize)
        .map(|name| (*name).to_owned())
    })
}
fn unit(t: &impl UiRead) -> Option<f64> {
    num(t, "unit").or_else(|| {
        Some(match text(t, "unit")?.as_str() {
            "Generic" => 0.,
            "Percent" => 1.,
            "PercentNormalized" => 2.,
            "Seconds" => 3.,
            "MilliSeconds" => 5.,
            "Hertz" => 7.,
            "Decibels" => 9.,
            "UviFilter" => 10.,
            "LinearGain" => 11.,
            "Pan" => 12.,
            "Megabyte" => 13.,
            "SemiTones" => 14.,
            "Cents" => 15.,
            "MidiKey" => 16.,
            _ => return None,
        })
    })
}

// Numeric host IDs select the same readout formatter used by the renderer.
fn unit_text(value: f64, integer: bool, unit: Option<f64>) -> String {
    let name = match unit {
        Some(1.) => "Percent",
        Some(2.) => "PercentNormalized",
        Some(3.) => "Seconds",
        Some(5.) => "MilliSeconds",
        Some(7.) => "Hertz",
        Some(9.) => "Decibels",
        Some(10.) => "UviFilter",
        Some(11.) => "LinearGain",
        Some(12.) => "Pan",
        Some(13.) => "Megabyte",
        Some(14.) => "SemiTones",
        Some(15.) => "Cents",
        Some(16.) => "MidiKey",
        _ => "",
    };
    ui::Display {
        unit: name.into(),
        ..Default::default()
    }
    .format_value(value, integer, ui::Source::FalconLua)
}

fn num(t: &impl UiRead, key: &str) -> Option<f64> {
    match t.read(key)? {
        Value::Number(n) => Some(n),
        Value::Integer(n) => Some(n as f64),
        Value::Boolean(b) => Some(f64::from(u8::from(b))),
        _ => None,
    }
}
fn flag(t: &impl UiRead, key: &str, default: bool) -> bool {
    match t.read(key) {
        Some(Value::Boolean(b)) => b,
        _ => default,
    }
}

fn documented_property(key: &str) -> bool {
    matches!(
        key,
        "textColourOn"
            | "textColourOff"
            | "outlineColour"
            | "arrowColour"
            | "0dBColour"
            | "fillStyle"
            | "displayAsMono"
            | "showPopupDisplay"
            | "displayArrow"
            | "hierarchical"
            | "popupBackgroundColour"
            | "popupTextColour"
            | "popupArrowColour"
            | "popupOutlineColour"
            | "popupTextColourHighlight"
            | "popupBackgroundColourHighlight"
            | "showScrollBar"
            | "scrollBarColour"
            | "waveColour"
            | "cursorColour"
            | "gridColour"
    )
}

// This is a per-export experiment, never a retained widget/value cache.
// A plain scalar widget executes no Lua or file service while it is lowered:
// all property reads use this exact no-metatable table, element is nil, and
// the excluded kinds cover nested sequences, cross-widget lookup and services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputRoute {
    Plain,
    Metatable,
    Bound,
    IndirectKind,
}
impl InputRoute {
    fn for_table(data: &Table) -> Self {
        if data.metatable().is_some() {
            return Self::Metatable;
        }
        if !matches!(data.raw_get::<Value>("element"), Ok(Value::Nil)) {
            return Self::Bound;
        }
        let Ok(Value::String(kind)) = data.raw_get::<Value>("kind") else {
            return Self::IndirectKind;
        };
        let Ok(kind) = kind.to_str() else {
            return Self::IndirectKind;
        };
        if matches!(
            kind.as_ref(),
            "Panel"
                | "Frame"
                | "ParameterValue"
                | "Knob"
                | "ParamKnob"
                | "Slider"
                | "ParamSlider"
                | "Button"
                | "OnOffButton"
                | "ParamOnOffButton"
                | "NumBox"
                | "ParamNumBox"
                | "Label"
                | "Text"
        ) {
            Self::Plain
        } else {
            Self::IndirectKind
        }
    }
}

macro_rules! plain_properties {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy)]
        #[repr(usize)]
        enum PlainProperty { $($variant,)+ Count }
        fn plain_property(key: &str) -> Option<PlainProperty> {
            match key { $($name => Some(PlainProperty::$variant),)+ _ => None }
        }
    };
}
plain_properties! {
    Kind => "kind", Name => "name", X => "x", Y => "y", Width => "width",
    Height => "height", Value => "value", Min => "min", Max => "max",
    Default => "default", Integer => "integer", Unit => "unit", Vertical => "vertical",
    Alpha => "alpha", InterceptsMouse => "interceptsMouseClicks", Visible => "visible",
    Enabled => "enabled", Tooltip => "tooltip", Text => "text", DisplayName => "displayName",
    DisplayText => "displayText", Exported => "exported", ParamId => "paramId", Mapper => "mapper",
    DisplayArrow => "displayArrow", Hierarchical => "hierarchical", ShowPopup => "showPopupDisplay",
    Outline => "outlineColour", Arrow => "arrowColour", Background => "backgroundColour",
    BackgroundOn => "backgroundColourOn", BackgroundOff => "backgroundColourOff",
    Slider => "sliderColour", Fill => "fillColour", Image => "image",
    BackgroundImage => "backgroundImage", NormalImage => "normalImage", StripImage => "stripImage",
    PressedImage => "pressedImage", OverImage => "overImage", OverPressedImage => "overPressedImage",
    HandleImage => "handleImage", Frames => "frames", StripHorizontal => "stripHorizontal",
    ShowLabel => "showLabel", ShowValue => "showValue", Font => "font", FontSize => "fontSize",
    TextColour => "textColour", Align => "align", TextOff => "textColourOff", TextOn => "textColourOn",
}

struct PlainWidgetInputs {
    values: [Value; PlainProperty::Count as usize],
    documented: Vec<String>,
}
impl PlainWidgetInputs {
    fn project(data: &Table) -> Self {
        let mut out = Self {
            values: std::array::from_fn(|_| Value::Nil),
            documented: Vec::new(),
        };
        let mut inventory_open = true;
        // Continue collecting scalar fields after an inventory key conversion
        // fault. Canonical property reads are independent of that raw cutoff.
        let _ = data.for_each::<Value, Value>(|key, value| {
            match key {
                Value::String(key) => match key.to_str() {
                    Ok(key) => {
                        if inventory_open && documented_property(&key) {
                            out.documented.push(key.to_owned());
                        }
                        if let Some(property) = plain_property(&key) {
                            out.values[property as usize] = value;
                        }
                    }
                    Err(_) => inventory_open = false,
                },
                // Canonical String conversion accepts numeric keys. They cannot
                // name a documented or modeled property.
                Value::Integer(_) | Value::Number(_) => {}
                _ => inventory_open = false,
            }
            Ok(())
        });
        out
    }
}
enum WidgetInputs<'a> {
    Canonical(&'a Table),
    Plain(PlainWidgetInputs),
}
impl UiRead for WidgetInputs<'_> {
    fn read(&self, key: &str) -> Option<Value> {
        match self {
            Self::Canonical(data) => data.read(key),
            Self::Plain(inputs) => Some(
                plain_property(key)
                    .map(|property| inputs.values[property as usize].clone())
                    .unwrap_or(Value::Nil),
            ),
        }
    }
}
impl WidgetInputs<'_> {
    fn raw_read(&self, key: &str) -> Option<Value> {
        match self {
            Self::Canonical(data) => data.raw_get::<Value>(key).ok(),
            Self::Plain(_) => self.read(key),
        }
    }
    fn table(&self, key: &str) -> Option<Table> {
        match self.read(key)? {
            Value::Table(table) => Some(table),
            _ => None,
        }
    }
}

fn unsigned(t: &Table, key: &str) -> Option<u32> {
    match t.raw_get::<Value>(key).ok()? {
        Value::Integer(n) => u32::try_from(n).ok(),
        Value::Number(n)
            if n.is_finite() && n.fract() == 0. && (0. ..=u32::MAX as f64).contains(&n) =>
        {
            Some(n as u32)
        }
        _ => None,
    }
}
fn widgets(host: &ScriptHost) -> Vec<Table> {
    host.lua
        .globals()
        .raw_get::<Table>("__ui")
        .and_then(|r| r.raw_get::<Table>("widgets"))
        .map(|t| t.sequence_values::<Table>().flatten().collect())
        .unwrap_or_default()
}
// Port v1 host.rs snapshot traversal: object identity stays in declaration order.
fn widget_tree(
    widgets: &[Table],
) -> Result<(Vec<Option<ui::WidgetRef>>, Vec<ui::WidgetRef>), &'static str> {
    let count = widgets.len();
    let mut identities = HashMap::with_capacity(count);
    for (index, widget) in widgets.iter().enumerate() {
        if identities
            .insert(widget.to_pointer() as usize, index)
            .is_some()
        {
            return Err("Duplicate UVI UI snapshot widget identity");
        }
    }
    let mut parents = Vec::with_capacity(count);
    let mut child_pointers = Vec::with_capacity(count);
    let mut items = 0usize;
    for widget in widgets {
        let state = widget
            .raw_get::<Table>("__data")
            .map_err(|_| "Invalid UVI UI snapshot widget state")?;
        let parent = match state
            .raw_get::<Value>("parent")
            .map_err(|_| "Invalid UVI UI snapshot parent")?
        {
            Value::Nil => None,
            Value::Table(t) => Some(ui::WidgetRef(
                *identities
                    .get(&(t.to_pointer() as usize))
                    .ok_or("UVI UI snapshot parent is outside processor scope")?,
            )),
            _ => return Err("Invalid UVI UI snapshot parent"),
        };
        let mut pointers = Vec::new();
        match state
            .raw_get::<Value>("children")
            .map_err(|_| "Invalid UVI UI snapshot child list")?
        {
            Value::Nil => {}
            Value::Table(source) => {
                // A valid sibling list cannot contain more identities than the model.
                if source.raw_len() > count {
                    return Err("UVI UI snapshot child list exceeds widget count");
                }
                items = items.saturating_add(source.raw_len());
                if items > 131_072 {
                    return Err("UVI UI snapshot item limit exceeded");
                }
                let mut entries = 0;
                for pair in source.pairs::<Value, Value>() {
                    let (key, _) = pair.map_err(|_| "Invalid UVI UI snapshot sequence")?;
                    let index = match key {
                        Value::Integer(i) => i as f64,
                        Value::Number(n) => n,
                        _ => 0.,
                    };
                    if !index.is_finite()
                        || index.fract() != 0.
                        || index < 1.
                        || index > source.raw_len() as f64
                    {
                        return Err("Invalid UVI UI snapshot sequence");
                    }
                    entries += 1;
                }
                if entries != source.raw_len() {
                    return Err("Sparse UVI UI snapshot sequence");
                }
                for i in 1..=source.raw_len() {
                    let Value::Table(child) = source
                        .raw_get::<Value>(i)
                        .map_err(|_| "Invalid UVI UI snapshot child reference")?
                    else {
                        return Err("Invalid UVI UI snapshot child reference");
                    };
                    pointers.push(child.to_pointer() as usize);
                }
            }
            _ => return Err("Invalid UVI UI snapshot child list"),
        }
        parents.push(parent);
        child_pointers.push(pointers);
    }
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
                return Err("Cyclic UVI UI snapshot parent tree");
            }
            visited[i] = 1;
            chain.push(i);
            next = parents[i].map(|p| p.0);
        }
        for i in chain {
            visited[i] = 2;
        }
    }
    let mut derived = vec![Vec::new(); count];
    for (index, parent) in parents.iter().enumerate() {
        if let Some(parent) = parent {
            derived[parent.0].push(ui::WidgetRef(index));
        }
    }
    let mut children = Vec::with_capacity(count);
    for (index, pointers) in child_pointers.into_iter().enumerate() {
        let mut seen = HashSet::with_capacity(pointers.len());
        let mut ordered = Vec::new();
        for pointer in pointers {
            let child = ui::WidgetRef(
                *identities
                    .get(&pointer)
                    .ok_or("UVI UI snapshot child is outside processor scope")?,
            );
            if child.0 == index || !seen.insert(child) {
                return Err("Duplicate or self UVI UI snapshot child reference");
            }
            // Current parent wins over a stale constructor list after reparenting.
            if parents[child.0] == Some(ui::WidgetRef(index)) {
                ordered.push(child);
            }
        }
        ordered.extend(
            derived[index]
                .iter()
                .copied()
                .filter(|id| !seen.contains(id)),
        );
        children.push(ordered);
    }
    let mut pending: Vec<_> = parents
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.is_none().then_some(i))
        .rev()
        .collect();
    let mut order = Vec::with_capacity(count);
    while let Some(index) = pending.pop() {
        order.push(ui::WidgetRef(index));
        pending.extend(children[index].iter().rev().map(|id| id.0));
    }
    debug_assert_eq!(order.len(), count);
    Ok((parents, order))
}

fn color(value: &str) -> Option<ui::Rgba> {
    let value = value.strip_prefix('#').unwrap_or(value);
    let packed = u32::from_str_radix(value, 16).ok();
    match (value.len(), packed) {
        (6, Some(n)) => Some(ui::Rgba::rgb(n)),
        (8, Some(n)) => Some(ui::Rgba {
            a: (n >> 24) as u8,
            ..ui::Rgba::rgb(n)
        }),
        _ => Some(ui::Rgba::rgb(match value.to_ascii_lowercase().as_str() {
            "black" => 0,
            "white" => 0xffffff,
            "red" => 0xff0000,
            "green" => 0x008000,
            "blue" => 0x0000ff,
            "grey" | "gray" => 0x808080,
            "darkgrey" | "darkgray" => 0xa9a9a9,
            "yellow" => 0xffff00,
            "transparent" => return Some(ui::Rgba::default()),
            _ => return None,
        })),
    }
}

impl ScriptHost {
    fn meter_request(&self, widget: &Table) -> Result<ui::UviMeterRequest, &'static str> {
        use super::Scope;
        let element = widget
            .raw_get::<Table>("meter_element")
            .map_err(|_| "meter element is unavailable")?;
        let class = self
            .lua
            .globals()
            .raw_get::<Table>("__element_mt")
            .map_err(|_| "element class identity is unavailable")?;
        if element
            .metatable()
            .is_none_or(|mt| mt.to_pointer() != class.to_pointer())
        {
            return Err("meter target is not a promoted element");
        }
        let element_id = unsigned(&element, "__id").ok_or("invalid promoted element identity")?;
        let id = element_id as usize;
        let element_node = self
            .shared
            .nodes
            .borrow()
            .get(id)
            .and_then(|&(node, _)| node)
            .and_then(|node| u32::try_from(node).ok())
            .ok_or("exact XML element identity is unavailable")?;
        let scope = match self.shared.scopes.borrow().get(id).copied().flatten() {
            Some(Scope::Program) => ui::UviMeterScope::Program,
            Some(Scope::Layer(ordinal)) => ui::UviMeterScope::Layer { ordinal },
            Some(Scope::Keygroup(node)) if u32::try_from(node).ok() == Some(element_node) => {
                ui::UviMeterScope::Keygroup
            }
            Some(Scope::Oscillator(node)) if u32::try_from(node).ok() == Some(element_node) => {
                ui::UviMeterScope::Oscillator
            }
            _ => return Err("exact meter element scope is unavailable"),
        };
        let bus_id = unsigned(widget, "busId").ok_or("invalid meter bus identity")?;
        let use_output = match widget.raw_get::<Value>("useOutput") {
            Ok(Value::Boolean(value)) => value,
            _ => return Err("invalid meter input/output selection"),
        };
        let display_as_mono = match widget.raw_get::<Value>("displayAsMono") {
            Ok(Value::Boolean(value)) => value,
            Ok(Value::Nil) => false,
            _ => return Err("invalid mono display policy"),
        };
        Ok(ui::UviMeterRequest {
            element_id,
            element_node,
            scope,
            bus_id,
            use_output,
            display_as_mono,
        })
    }

    /// Property names/classes only; no proprietary script values.
    pub fn ui_properties(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for w in widgets(self) {
            let kind = text(&w, "kind").unwrap_or_default();
            let data = w.raw_get::<Table>("__data").unwrap_or(w);
            for (key, _) in data.pairs::<String, Value>().flatten() {
                if !key.starts_with('_') {
                    *out.entry(format!("{kind}.{key}")).or_default() += 1;
                }
            }
        }
        out
    }
    pub fn ui_revision(&self) -> u64 {
        self.lua
            .globals()
            .raw_get::<Table>("__ui")
            .and_then(|r| r.raw_get("revision"))
            .unwrap_or(0)
    }
    /// Scalar, table-cell and XY-axis state, without integer conversion.
    pub fn control_values(&self) -> Vec<(ui::ControlId, f64)> {
        Self::control_values_from(&self.interface())
    }
    pub(crate) fn control_values_from(face: &ui::Interface) -> Vec<(ui::ControlId, f64)> {
        let mut out = Vec::new();
        for (i, w) in face.widgets.iter().enumerate() {
            match &w.kind {
                ui::Kind::Table { cells, .. } => out.extend(
                    cells
                        .iter()
                        .enumerate()
                        .map(|(c, v)| (control_id(i + 1, c + 1), *v)),
                ),
                ui::Kind::Xy { .. } => {} // Axes share their original knobs' controls.
                _ if matches!(w.binding, ui::Binding::Control(_)) => {
                    out.push((control_id(i + 1, 0), w.initial_value));
                }
                _ => {}
            }
        }
        out
    }
    /// A renderer edit. Executes the real `changed` handler synchronously on Lua's
    /// owner; yielding is an error, while spawn remains available.
    pub fn set_control(&mut self, id: ui::ControlId, value: f64) -> Result<(), String> {
        if !value.is_finite() {
            return Err("non-finite UI value".into());
        }
        let (widget, component) = identity(id).ok_or("not a UVI control")?;
        self.shared.arm(
            self.shared.config.callback,
            self.shared.config.callback_work,
        );
        let f = self
            .lua
            .globals()
            .raw_get::<Function>("__ui_edit")
            .map_err(super::lua_error)?;
        let result = f
            .call::<()>((widget, component, value))
            .map_err(super::lua_error);
        if let Err(e) = &result {
            self.shared.find("lua UI callback", e);
        }
        self.cycle();
        result
    }
    /// Current declaration, with script-driven geometry, visibility and text.
    pub fn interface(&self) -> ui::Interface {
        self.interface_classified::<3>()
    }

    // The canonical iterator and rejected classifier are test-only oracles.
    // Ordinary exports use the same membership rules with mlua's raw for_each.
    fn interface_classified<const INVENTORY_MODE: u8>(&self) -> ui::Interface {
        let _profile = self.shared.profile.span("ui_ir_export");
        let _budget = self.shared.inspection_budget();
        let _span = sampler_kontakt::audit::Span::new("uvi_lua_ui_snapshot");
        let mut out = ui::Interface {
            source: ui::Source::FalconLua,
            ..Default::default()
        };
        let Ok(root) = self.lua.globals().raw_get::<Table>("__ui") else {
            return out;
        };
        out.performance = flag(&root, "performance", false);
        let widgets = {
            #[cfg(feature = "scan")]
            let _owner_span = self.owner_span(super::diagnostics::OwnerStage::ExportRegistry);
            widgets(self)
        };
        let topology = {
            #[cfg(feature = "scan")]
            let _owner_span = self.owner_span(super::diagnostics::OwnerStage::ExportTopology);
            widget_tree(&widgets)
        };
        let (parents, order) = match topology {
            Ok(tree) => tree,
            Err(error) => {
                self.shared.find("lua UI snapshot", error);
                out.unsupported.push(ui::Unsupported {
                    widget: None,
                    feature: "lua UI snapshot".into(),
                    value: error.into(),
                });
                out.paint_order = Some(Vec::new());
                return out;
            }
        };
        out.paint_order = Some(order);
        let mut assets = BTreeMap::<String, usize>::new();
        let mut asset = |path: &str, kind: ui::AssetKind, out: &mut ui::Interface| {
            if let Some(&index) = assets.get(path) {
                return ui::AssetRef(index);
            }
            let index = out.assets.len();
            assets.insert(path.into(), index);
            out.assets.push(ui::Asset {
                path: path.into(),
                kind,
            });
            ui::AssetRef(index)
        };
        #[cfg(feature = "scan")]
        let widget_span = self.owner_span(super::diagnostics::OwnerStage::ExportWidgets);
        #[cfg(feature = "scan")]
        let export_audit = self.owner_audit();
        #[cfg(feature = "scan")]
        let sample_offset = export_audit
            .as_ref()
            .map(|audit| audit.export_sample_offset());
        #[cfg(feature = "scan")]
        let (mut bound_values, mut sampled_bound_values) = (0usize, 0usize);
        for (index, proxy) in widgets.iter().enumerate() {
            #[cfg(feature = "scan")]
            let sampled_audit = export_audit
                .as_ref()
                .filter(|_| sample_offset == Some(index & 127));
            #[cfg(feature = "scan")]
            let _widget_sample = sampled_audit
                .map(|audit| audit.span(super::diagnostics::OwnerStage::ExportWidgetSample));
            // Plain reads of the widget's data table: the proxy's __index is a Lua
            // call per property, and a snapshot reads ~70 per widget. Only a value
            // bound to an element is computed (by the proxy).
            let data = proxy
                .raw_get::<Table>("__data")
                .unwrap_or_else(|_| proxy.clone());
            let route = if INVENTORY_MODE == 3 {
                Some(InputRoute::for_table(&data))
            } else {
                #[cfg(feature = "scan")]
                {
                    export_audit.as_ref().map(|_| InputRoute::for_table(&data))
                }
                #[cfg(not(feature = "scan"))]
                {
                    None
                }
            };
            #[cfg(feature = "scan")]
            if let (Some(audit), Some(route)) = (&export_audit, route) {
                use super::diagnostics::OwnerCounter as Counter;
                audit.add(
                    match route {
                        InputRoute::Plain => Counter::ExportPlainWidgets,
                        InputRoute::Metatable => Counter::ExportMetatableWidgets,
                        InputRoute::Bound => Counter::ExportBoundWidgets,
                        InputRoute::IndirectKind => Counter::ExportIndirectWidgets,
                    },
                    1,
                );
            }
            let inputs = if INVENTORY_MODE == 3 && route == Some(InputRoute::Plain) {
                WidgetInputs::Plain(PlainWidgetInputs::project(&data))
            } else {
                WidgetInputs::Canonical(&data)
            };
            let w = &inputs;
            let kind = text(w, "kind").unwrap_or_default();
            let name = text(w, "name").unwrap_or_default();
            let (x, y, wd, ht) = (
                num(w, "x").unwrap_or(0.),
                num(w, "y").unwrap_or(0.),
                num(w, "width").unwrap_or(0.),
                num(w, "height").unwrap_or(0.),
            );
            let value = {
                #[cfg(feature = "scan")]
                let _value_sample = sampled_audit
                    .map(|audit| audit.span(super::diagnostics::OwnerStage::ExportValueSample));
                if matches!(w.raw_read("element"), Some(Value::Nil) | None) {
                    num(w, "value")
                } else {
                    #[cfg(feature = "scan")]
                    if export_audit.is_some() {
                        bound_values += 1;
                        sampled_bound_values += usize::from(sampled_audit.is_some());
                    }
                    num(proxy, "value")
                }
            }
            .unwrap_or(0.);
            let range = ui::Range {
                min: num(w, "min").unwrap_or(0.),
                max: num(w, "max").unwrap_or(1.),
                default: num(w, "default").unwrap_or(value),
                step: flag(w, "integer", false).then_some(1.),
            };
            let display = ui::Display {
                unit: text(w, "unit")
                    .filter(|u| u != "Generic")
                    .unwrap_or_default(),
                ..Default::default()
            };
            let items = || {
                w.table("items")
                    .map(|t| {
                        t.sequence_values::<String>()
                            .flatten()
                            .enumerate()
                            .map(|(i, text)| ui::MenuItem {
                                text,
                                value: i as i32 + 1,
                                visible: true,
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let kind_ir = match kind.as_str() {
                "Panel" | "Frame" | "Viewport" | "ParameterValue" => ui::Kind::Panel,
                "Knob" | "ParamKnob" => ui::Kind::Knob { range, display },
                "Slider" | "ParamSlider" => ui::Kind::Slider {
                    range,
                    orientation: if flag(w, "vertical", false) {
                        ui::Orientation::Vertical
                    } else {
                        ui::Orientation::Horizontal
                    },
                },
                "Button" => ui::Kind::Button { momentary: true },
                "OnOffButton" | "ParamOnOffButton" => ui::Kind::Button { momentary: false },
                "Menu" | "ParamMenu" | "MultiStateButton" => ui::Kind::Menu { items: items() },
                "NumBox" | "ParamNumBox" => ui::Kind::ValueEdit {
                    range,
                    display,
                    arrows: false,
                },
                "Label" | "Text" => ui::Kind::Label,
                "Image" | "SVG" => ui::Kind::Image,
                "Table" => ui::Kind::Table {
                    columns: num(w, "length").unwrap_or(0.) as u32,
                    range,
                    bipolar: range.min < 0.,
                    cells: w
                        .table("values")
                        .map(|t| t.sequence_values::<f64>().flatten().collect())
                        .unwrap_or_default(),
                    steps_shown: None,
                },
                "AudioMeter" => ui::Kind::LevelMeter {
                    orientation: if flag(w, "vertical", true) {
                        ui::Orientation::Vertical
                    } else {
                        ui::Orientation::Horizontal
                    },
                },
                "XY" => ui::Kind::Xy {
                    cursors: 1,
                    sensitivity: [None; 2],
                    mouse_mode: None,
                },
                "WaveView" | "WaveForm" => ui::Kind::Waveform,
                "FileSelector" => ui::Kind::FileSelector {
                    base_path: None,
                    files: ui::Files::Any,
                    column_width: None,
                },
                "DnDArea" => ui::Kind::MouseArea,
                other => {
                    out.unsupported.push(ui::Unsupported {
                        widget: Some(ui::WidgetRef(index)),
                        feature: other.into(),
                        value: "unsupported widget".into(),
                    });
                    ui::Kind::Panel
                }
            };
            let mut wd = ui::Widget::new(
                name.clone(),
                ui::PageRef(0),
                ui::Rect {
                    x,
                    y,
                    width: wd,
                    height: ht,
                },
                kind_ir,
            );
            wd.source_id = Some(index as i32 + 1);
            // Image's omitted axes use its resource size; explicit script bounds win.
            // UVI Image geometry contract (class_image.html); preserve this distinction
            // before the shared renderer sees a default 100x100 rectangle.
            if kind == "Image"
                && (!flag(w, "width_set", false) || !flag(w, "height_set", false))
                && let Some(path) = text(w, "image")
                && let Some(size) = self.shared.files.image_size(&path)
            {
                if !flag(w, "width_set", false) {
                    wd.rect.width = size.width;
                }
                if !flag(w, "height_set", false) {
                    wd.rect.height = size.height;
                }
            }
            wd.initial_value = value;
            wd.opacity = num(w, "alpha").unwrap_or(1.).clamp(0., 1.) as f32;
            wd.intercepts_mouse = flag(w, "interceptsMouseClicks", true);
            wd.hidden = !flag(w, "visible", true);
            wd.enabled = flag(w, "enabled", true);
            wd.tooltip = text(w, "tooltip").unwrap_or_default();
            wd.text = text(w, "text")
                .filter(|t| !t.is_empty())
                .or_else(|| text(w, "displayName"))
                .unwrap_or_default();
            wd.value_text = text(w, "displayText").filter(|t| !t.is_empty());
            wd.parent = parents[index];
            wd.automation.allowed = flag(w, "exported", false);
            wd.automation.id = num(w, "paramId").map(|n| n as u32);
            wd.mapper = mapper(w);
            if wd.value_text.is_none()
                && matches!(
                    wd.kind,
                    ui::Kind::Knob { .. } | ui::Kind::Slider { .. } | ui::Kind::ValueEdit { .. }
                )
            {
                wd.value_text =
                    unit(w).map(|u| unit_text(value, flag(w, "integer", false), Some(u)));
            }
            wd.menu_cycle = kind == "MultiStateButton";
            wd.menu_arrow = flag(w, "displayArrow", true);
            if !wd.menu_cycle
                && matches!(w.raw_read("hierarchical"), Some(Value::Boolean(true)))
                && let ui::Kind::Menu { items } = &wd.kind
            {
                match ui::menu_hierarchy(items) {
                    Ok(tree) => wd.menu_hierarchy = Some(tree),
                    Err(error) => out.unsupported.push(ui::Unsupported {
                        widget: Some(ui::WidgetRef(index)),
                        feature: format!("{kind}.hierarchical"),
                        value: format!("documented path tree cannot be represented: {error:?}"),
                    }),
                }
            }
            if matches!(wd.kind, ui::Kind::Knob { .. } | ui::Kind::Slider { .. }) {
                wd.popup_display = match w.raw_read("showPopupDisplay") {
                    Some(Value::Boolean(value)) => Some(value),
                    _ => None,
                };
            }
            if matches!(wd.kind, ui::Kind::Menu { .. }) {
                wd.colors.outline = text(w, "outlineColour").and_then(|c| color(&c));
                wd.colors.arrow = text(w, "arrowColour").and_then(|c| color(&c));
            }
            wd.colors.background = text(w, "backgroundColour").and_then(|c| color(&c));
            wd.colors.on = text(w, "backgroundColourOn").and_then(|c| color(&c));
            wd.colors.off = text(w, "backgroundColourOff").and_then(|c| color(&c));
            wd.colors.bar = text(w, "sliderColour")
                .or_else(|| text(w, "fillColour"))
                .and_then(|c| color(&c));
            if kind == "Viewport" {
                wd.viewport = Some(
                    w.table("viewPosition")
                        .map(|t| [t.raw_get(1).unwrap_or(0), t.raw_get(2).unwrap_or(0)])
                        .unwrap_or([0, 0]),
                );
            }
            if matches!(
                wd.kind,
                ui::Kind::Knob { .. }
                    | ui::Kind::Slider { .. }
                    | ui::Kind::ValueEdit { .. }
                    | ui::Kind::Button { .. }
                    | ui::Kind::Menu { .. }
            ) {
                wd.binding = ui::Binding::Control(control_id(index + 1, 0));
            } else if let ui::Kind::Table { columns, .. } = wd.kind {
                wd.components = (1..=columns as usize)
                    .map(|c| control_id(index + 1, c))
                    .collect();
                wd.binding = wd
                    .components
                    .first()
                    .copied()
                    .map(ui::Binding::Control)
                    .unwrap_or_default();
            } else if kind == "XY" {
                for key in ["paramX", "paramY"] {
                    let target = text(w, key);
                    if let Some(i) = widgets.iter().position(|w| text(w, "name") == target) {
                        wd.components.push(control_id(i + 1, 0));
                    } else {
                        out.unsupported.push(ui::Unsupported {
                            widget: Some(ui::WidgetRef(index)),
                            feature: format!("XY {key}"),
                            value: "unbound axis".into(),
                        });
                    }
                }
                wd.binding = wd
                    .components
                    .first()
                    .copied()
                    .map(ui::Binding::Control)
                    .unwrap_or_default();
            } else if kind == "AudioMeter" {
                // Falcon's element bus/input selection is not a Kontakt output
                // channel. Keep its exact request for the dedicated tap bridge.
                let detail = match self.meter_request(&data) {
                    Ok(request) => {
                        wd.uvi_meter_request = Some(request);
                        "exact element input/output meter tap unavailable"
                    }
                    Err(reason) => reason,
                };
                out.unsupported.push(ui::Unsupported {
                    widget: Some(ui::WidgetRef(index)),
                    feature: "AudioMeter element".into(),
                    value: detail.into(),
                });
            }
            for (key, role) in [
                ("image", ui::Role::Background),
                ("backgroundImage", ui::Role::Background),
                ("normalImage", ui::Role::Strip),
                ("stripImage", ui::Role::Strip),
                ("pressedImage", ui::Role::Pressed),
                ("overImage", ui::Role::Hover),
                ("overPressedImage", ui::Role::HoverPressed),
                ("handleImage", ui::Role::Handle),
            ] {
                if let Some(path) = text(w, key).filter(|s| !s.is_empty()) {
                    let meta = ui::ImageMeta {
                        frames: if key == "stripImage" {
                            num(w, "frames").unwrap_or(1.) as u32
                        } else {
                            1
                        },
                        axis: if key == "stripImage" && flag(w, "stripHorizontal", false) {
                            ui::Orientation::Horizontal
                        } else {
                            ui::Orientation::Vertical
                        },
                        ..Default::default()
                    };
                    wd.images.push(ui::ImageUse::new(
                        asset(&path, ui::AssetKind::Image(meta), &mut out),
                        role,
                    ));
                }
            }
            // Authored skins carry their own legends and sit under neighbouring
            // controls: a stripped widget's name and value captions are drawn only
            // when the script asks for them (showLabel/showValue, or displayText).
            let skinned = wd.images.iter().any(|i| i.role == ui::Role::Strip);
            wd.hide.title = !flag(w, "showLabel", !skinned);
            wd.hide.value = !flag(
                w,
                "showValue",
                !skinned || text(w, "displayText").is_some_and(|t| !t.is_empty()),
            );
            if let Some(font) = text(w, "font").filter(|s| !s.is_empty()) {
                let a = asset(&font, ui::AssetKind::TrueTypeFont, &mut out);
                wd.style = Some(ui::StyleRef(out.styles.len()));
                out.styles.push(ui::TextStyle {
                    font: ui::Font::File(a),
                    size: num(w, "fontSize").map(|n| n as f32),
                    color: text(w, "textColour")
                        .and_then(|c| color(&c))
                        .unwrap_or(ui::Rgba::rgb(0xffffff)),
                    align: match text(w, "align").as_deref() {
                        Some("left") => ui::Align::Left,
                        Some("right") => ui::Align::Right,
                        _ => ui::Align::Center,
                    },
                });
            } else if num(w, "fontSize").is_some() || text(w, "textColour").is_some() {
                wd.style = Some(ui::StyleRef(out.styles.len()));
                out.styles.push(ui::TextStyle {
                    font: ui::Font::Default,
                    size: num(w, "fontSize").map(|n| n as f32),
                    color: text(w, "textColour")
                        .and_then(|c| color(&c))
                        .unwrap_or(ui::Rgba::rgb(0xffffff)),
                    align: ui::Align::Center,
                });
            }
            if matches!(wd.kind, ui::Kind::Button { .. }) {
                // State ink inherits the authored face, size and alignment.
                // Missing on/off properties retain the general textColour style.
                for (on, key) in [(false, "textColourOff"), (true, "textColourOn")] {
                    if let Some(color) = text(w, key).and_then(|c| color(&c)) {
                        let mut style = wd
                            .style
                            .and_then(|s| out.styles.get(s.0))
                            .cloned()
                            .unwrap_or(ui::TextStyle {
                                font: ui::Font::Default,
                                size: None,
                                color: ui::Rgba::rgb(0xffffff),
                                align: ui::Align::Center,
                            });
                        style.color = color;
                        let reference = ui::StyleRef(out.styles.len());
                        out.styles.push(style);
                        for state in [usize::from(on), 2 + usize::from(on), 4 + usize::from(on)] {
                            wd.state_styles[state] = Some(reference);
                        }
                    }
                }
            }
            let modeled = [
                "kind",
                "name",
                "value",
                "default",
                "min",
                "max",
                "integer",
                "x",
                "y",
                "width",
                "height",
                "alpha",
                "visible",
                "enabled",
                "text",
                "tooltip",
                "displayName",
                "parent_id",
                "id",
                "length",
                "values",
                "items",
                "changed",
                "persistent",
                "exported",
                "paramId",
                "paramX",
                "paramY",
                "vertical",
                "mapper",
                "unit",
                "font",
                "fontSize",
                "align",
                "image",
                "frames",
                "stripHorizontal",
                "stripImage",
                "normalImage",
                "pressedImage",
                "overImage",
                "overPressedImage",
                "backgroundImage",
                "handleImage",
                "showLabel",
                "showValue",
                "displayText",
                "interceptsMouseClicks",
                "viewPosition",
                "backgroundColour",
                "backgroundColourOn",
                "backgroundColourOff",
                "sliderColour",
                "fillColour",
                "textColour",
                "element",
                "parameter",
                "bound",
                "meter_element",
                "busId",
                "useOutput",
                "stereo",
                "channel",
            ];
            {
                #[cfg(feature = "scan")]
                let _inventory_sample = sampled_audit
                    .map(|audit| audit.span(super::diagnostics::OwnerStage::ExportInventorySample));
                let mut inventory = |key: String| {
                    // Lua scripts freely attach private data/functions to widgets.
                    // Only documented UI properties count as rendering gaps.
                    if INVENTORY_MODE == 1 && !documented_property(&key) {
                        return;
                    }
                    let documented = [
                        "textColourOn",
                        "textColourOff",
                        "outlineColour",
                        "arrowColour",
                        "0dBColour",
                        "fillStyle",
                        "displayAsMono",
                        "showPopupDisplay",
                        "displayArrow",
                        "hierarchical",
                        "popupBackgroundColour",
                        "popupTextColour",
                        "popupArrowColour",
                        "popupOutlineColour",
                        "popupTextColourHighlight",
                        "popupBackgroundColourHighlight",
                        "showScrollBar",
                        "scrollBarColour",
                        "waveColour",
                        "cursorColour",
                        "gridColour",
                    ];
                    let consumed = match key.as_str() {
                        "textColourOn" | "textColourOff" => {
                            matches!(wd.kind, ui::Kind::Button { .. })
                                && text(w, &key).and_then(|c| color(&c)).is_some()
                        }
                        "outlineColour" | "arrowColour" => {
                            matches!(wd.kind, ui::Kind::Menu { .. })
                                && text(w, &key).and_then(|c| color(&c)).is_some()
                        }
                        "displayArrow" => {
                            matches!(wd.kind, ui::Kind::Menu { .. })
                                && matches!(w.raw_read("displayArrow"), Some(Value::Boolean(_)))
                        }
                        "hierarchical" => {
                            !wd.menu_cycle
                                && matches!(wd.kind, ui::Kind::Menu { .. })
                                && matches!(w.raw_read("hierarchical"), Some(Value::Boolean(_)))
                        }
                        "showPopupDisplay" => {
                            matches!(wd.kind, ui::Kind::Knob { .. } | ui::Kind::Slider { .. })
                                && wd.popup_display.is_some()
                        }
                        _ => false,
                    };
                    if !consumed
                        && !modeled.contains(&key.as_str())
                        && (INVENTORY_MODE == 1 || documented.contains(&key.as_str()))
                    {
                        out.unsupported.push(ui::Unsupported {
                            widget: Some(ui::WidgetRef(index)),
                            feature: format!("{kind}.{key}"),
                            value: "property not represented by renderer".into(),
                        });
                    }
                };
                if let WidgetInputs::Plain(inputs) = w {
                    for key in &inputs.documented {
                        inventory(key.clone());
                    }
                } else if INVENTORY_MODE == 2 || INVENTORY_MODE == 3 {
                    // Pinned mlua/Luau uses the same raw iterator and stops on
                    // the first conversion error; ignore that terminal error
                    // exactly as the canonical pairs().flatten() path does.
                    let _ = data.for_each::<String, Value>(|key, _| {
                        inventory(key);
                        Ok(())
                    });
                } else {
                    for (key, _) in data.pairs::<String, Value>().flatten() {
                        inventory(key);
                    }
                }
            }
            if matches!(
                kind.as_str(),
                "WaveView" | "WaveForm" | "FileSelector" | "DnDArea" | "SVG"
            ) {
                out.unsupported.push(ui::Unsupported {
                    widget: Some(ui::WidgetRef(index)),
                    feature: format!("{kind} service"),
                    value: "renderer/engine service unavailable".into(),
                });
            }
            out.widgets.push(wd);
        }
        #[cfg(feature = "scan")]
        if let Some(audit) = &export_audit {
            audit.add(
                super::diagnostics::OwnerCounter::ExportBoundValues,
                bound_values,
            );
            audit.add(
                super::diagnostics::OwnerCounter::ExportSampledBoundValues,
                sampled_bound_values,
            );
        }
        #[cfg(feature = "scan")]
        drop(widget_span);
        #[cfg(feature = "scan")]
        let page_span = self.owner_span(super::diagnostics::OwnerStage::ExportPage);
        let width = out
            .widgets
            .iter()
            .enumerate()
            .map(|(i, _)| out.page_rect(ui::WidgetRef(i)))
            .map(|r| (r.x + r.width).max(0.))
            .reduce(f64::max)
            .unwrap_or(720.);
        let height = out
            .widgets
            .iter()
            .enumerate()
            .map(|(i, _)| out.page_rect(ui::WidgetRef(i)))
            .map(|r| (r.y + r.height).max(0.))
            .reduce(f64::max)
            .unwrap_or(100.);
        let background = text(&root, "background")
            .filter(|s| !s.is_empty())
            .map(|s| asset(&s, ui::AssetKind::Image(Default::default()), &mut out));
        out.pages.push(ui::Page {
            name: "main".into(),
            size: ui::Size {
                width: num(&root, "width").unwrap_or(width),
                height: num(&root, "height").unwrap_or(height),
            },
            background: ui::Background {
                image: background,
                color: text(&root, "backgroundColour").and_then(|c| color(&c)),
                ..Default::default()
            },
            ..Default::default()
        });
        #[cfg(feature = "scan")]
        drop(page_span);
        let validation = {
            #[cfg(feature = "scan")]
            let _owner_span = self.owner_span(super::diagnostics::OwnerStage::ExportValidate);
            out.validate()
        };
        if let Err(error) = validation {
            let error = format!("Invalid UVI UI snapshot: {error}");
            self.shared.find("lua UI snapshot", &error);
            return ui::Interface {
                source: ui::Source::FalconLua,
                paint_order: Some(Vec::new()),
                unsupported: vec![ui::Unsupported {
                    widget: None,
                    feature: "lua UI snapshot".into(),
                    value: error,
                }],
                ..Default::default()
            };
        }
        out
    }
}

#[cfg(all(test, feature = "scan"))]
mod export_profile_tests;

#[cfg(test)]
mod inventory_tests;

/// UVI script state is independent of KSP integer memory. Table keys preserve
/// Lua's numeric/string distinction; cycles and userdata fail rather than truncate.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SavedValue {
    Nil,
    Boolean(bool),
    Number(f64),
    String(String),
    Bytes(Vec<u8>),
    Table(Vec<(SavedValue, SavedValue)>),
}
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UiState {
    pub widgets: Vec<(usize, String, SavedValue)>,
    pub custom: Option<SavedValue>,
}
pub(super) fn json_state(source: &str) -> Result<SavedValue, String> {
    if source.len() > 2 << 20 {
        return Err("UVI state exceeds 2 MiB".into());
    }
    fn convert(
        v: serde_json::Value,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<SavedValue, String> {
        *remaining = remaining
            .checked_sub(1)
            .ok_or("UVI state exceeds 65536 values")?;
        if depth > 64 {
            return Err("UVI state exceeds depth limit".into());
        }
        Ok(match v {
            serde_json::Value::Null => SavedValue::Nil,
            serde_json::Value::Bool(b) => SavedValue::Boolean(b),
            serde_json::Value::Number(n) => SavedValue::Number(
                n.as_f64()
                    .filter(|n| n.is_finite())
                    .ok_or("invalid state number")?,
            ),
            serde_json::Value::String(s) => SavedValue::String(s),
            serde_json::Value::Array(v) => SavedValue::Table(
                v.into_iter()
                    .enumerate()
                    .map(|(i, v)| {
                        Ok((
                            SavedValue::Number((i + 1) as f64),
                            convert(v, depth + 1, remaining)?,
                        ))
                    })
                    .collect::<Result<_, String>>()?,
            ),
            serde_json::Value::Object(v) => SavedValue::Table(
                v.into_iter()
                    .map(|(k, v)| Ok((SavedValue::String(k), convert(v, depth + 1, remaining)?)))
                    .collect::<Result<_, String>>()?,
            ),
        })
    }
    let value: serde_json::Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
    if !value.is_object() && !value.is_array() && !value.is_null() {
        return Err("UVI custom state must be a table or nil".into());
    }
    convert(value, 0, &mut 65536)
}
fn save_value(
    v: Value,
    stack: &mut Vec<usize>,
    remaining: &mut usize,
) -> Result<SavedValue, String> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or("UVI state exceeds 65536 values")?;
    Ok(match v {
        Value::Nil => SavedValue::Nil,
        Value::Boolean(b) => SavedValue::Boolean(b),
        Value::Integer(n) => SavedValue::Number(n as f64),
        Value::Number(n) if n.is_finite() => SavedValue::Number(n),
        Value::String(s) if s.as_bytes().len() <= 1 << 20 => match s.to_str() {
            Ok(s) => SavedValue::String(s.to_owned()),
            Err(_) => SavedValue::Bytes(s.as_bytes().to_vec()),
        },
        Value::Table(t) => {
            let ptr = t.to_pointer() as usize;
            if stack.len() >= 64 || stack.contains(&ptr) {
                return Err("UVI state is cyclic or deeper than 64 tables".into());
            }
            // Engine/widget proxies are not serializable Lua data.
            if t.metatable().is_some() {
                return Err("UVI state contains a metatable/userdata".into());
            }
            stack.push(ptr);
            let mut values = Vec::new();
            for pair in t.pairs::<Value, Value>() {
                let (k, v) = pair.map_err(super::lua_error)?;
                values.push((
                    save_value(k, stack, remaining)?,
                    save_value(v, stack, remaining)?,
                ));
            }
            stack.pop();
            SavedValue::Table(values)
        }
        _ => return Err("UVI state contains an unsupported value".into()),
    })
}
fn load_value(
    lua: &mlua::Lua,
    v: &SavedValue,
    depth: usize,
    remaining: &mut usize,
) -> Result<Value, String> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or("UVI state exceeds 65536 values")?;
    if depth >= 64 {
        return Err("UVI state exceeds depth limit".into());
    }
    Ok(match v {
        SavedValue::Nil => Value::Nil,
        SavedValue::Boolean(b) => Value::Boolean(*b),
        SavedValue::Number(n) if n.is_finite() => Value::Number(*n),
        SavedValue::String(s) if s.len() <= 1 << 20 => {
            Value::String(lua.create_string(s).map_err(super::lua_error)?)
        }
        SavedValue::Bytes(s) if s.len() <= 1 << 20 => {
            Value::String(lua.create_string(s).map_err(super::lua_error)?)
        }
        SavedValue::Table(pairs) => {
            let t = lua.create_table().map_err(super::lua_error)?;
            for (k, v) in pairs {
                t.raw_set(
                    load_value(lua, k, depth + 1, remaining)?,
                    load_value(lua, v, depth + 1, remaining)?,
                )
                .map_err(super::lua_error)?;
            }
            Value::Table(t)
        }
        _ => return Err("UVI state contains an invalid value".into()),
    })
}
impl ScriptHost {
    /// Explicit state restoration: widget callbacks, custom onLoad, no onInit.
    pub fn restore_ui_state(&self, state: &UiState) -> Result<(), String> {
        self.restore_ui_values(state)?;
        self.restore_ui_custom(state)
    }
    pub fn save_ui_state(&self) -> Result<UiState, String> {
        let _span = sampler_kontakt::audit::Span::new("uvi_lua_ui_save");
        self.shared
            .arm(self.shared.config.load, self.shared.config.load_work);
        let mut out = UiState::default();
        let mut remaining = 65536;
        if let Ok(Value::Function(f)) = self.lua.globals().raw_get::<Value>("onSave") {
            let value = f.call::<Value>(()).map_err(super::lua_error)?;
            out.custom = Some(save_value(value, &mut Vec::new(), &mut remaining)?);
        }
        for (i, w) in widgets(self).iter().enumerate() {
            if !flag(w, "persistent", true) || text(w, "kind").as_deref() == Some("Button") {
                continue;
            }
            let v = if text(w, "kind").as_deref() == Some("Table") {
                w.get::<Value>("values")
            } else {
                w.get::<Value>("value")
            }
            .map_err(super::lua_error)?;
            if !matches!(v, Value::Nil) {
                out.widgets.push((
                    i + 1,
                    text(w, "name").unwrap_or_default(),
                    save_value(v, &mut Vec::new(), &mut remaining)?,
                ));
            }
        }
        if serde_json::to_vec(&out).map_err(|e| e.to_string())?.len() > 8 << 20 {
            return Err("UVI state exceeds 8 MiB".into());
        }
        Ok(out)
    }
    pub(super) fn restore_ui_values(&self, state: &UiState) -> Result<(), String> {
        self.shared
            .arm(self.shared.config.load, self.shared.config.load_work);
        let list = widgets(self);
        let mut remaining = 65536;
        for (i, name, v) in &state.widgets {
            let Some(w) = i.checked_sub(1).and_then(|i| list.get(i)) else {
                continue;
            };
            if text(w, "name").as_ref() != Some(name) || !flag(w, "persistent", true) {
                continue;
            }
            let setter = w.get::<Function>("setValue").map_err(super::lua_error)?;
            let value = load_value(&self.lua, v, 0, &mut remaining)?;
            if text(w, "kind").as_deref() == Some("Table") {
                let Value::Table(values) = value else {
                    return Err("invalid saved Table value".into());
                };
                let length = num(w, "length").unwrap_or(0.) as usize;
                for (i, v) in values.sequence_values::<Value>().take(length).enumerate() {
                    setter
                        .call::<()>((w.clone(), i + 1, v.map_err(super::lua_error)?))
                        .map_err(super::lua_error)?;
                }
            } else {
                setter
                    .call::<()>((w.clone(), value))
                    .map_err(super::lua_error)?;
            }
        }
        self.cycle();
        Ok(())
    }
    pub(super) fn restore_ui_custom(&self, state: &UiState) -> Result<(), String> {
        if let Some(value) = &state.custom
            && let Ok(Value::Function(f)) = self.lua.globals().raw_get::<Value>("onLoad")
        {
            self.shared
                .arm(self.shared.config.load, self.shared.config.load_work);
            f.call::<()>(load_value(&self.lua, value, 0, &mut 65536)?)
                .map_err(super::lua_error)?;
            self.cycle();
        }
        Ok(())
    }
}
