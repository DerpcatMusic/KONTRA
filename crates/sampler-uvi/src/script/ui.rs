//! Stateful UVI widgets lowered to the shared UI IR. Lua runs only on its owner.
use super::ScriptHost;
use mlua::{Function, Table, Value};
use sampler_ui_ir as ui;
use std::collections::BTreeMap;

pub fn control_id(widget: usize, component: usize) -> ui::ControlId {
    ui::ControlId(0x55564900000000000000000000000000 | ((widget as u128) << 32) | component as u128)
}
fn identity(id: ui::ControlId) -> Option<(usize, usize)> {
    ((id.0 >> 64) == 0x5556490000000000).then_some((
        ((id.0 >> 32) & 0xffff_ffff) as usize,
        (id.0 & 0xffff_ffff) as usize,
    ))
}
fn text(t: &Table, key: &str) -> Option<String> {
    match t.get::<Value>(key).ok()? {
        Value::String(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}
fn num(t: &Table, key: &str) -> Option<f64> {
    match t.get::<Value>(key).ok()? {
        Value::Number(n) => Some(n),
        Value::Integer(n) => Some(n as f64),
        Value::Boolean(b) => Some(f64::from(u8::from(b))),
        _ => None,
    }
}
fn flag(t: &Table, key: &str, default: bool) -> bool {
    match t.get::<Value>(key) {
        Ok(Value::Boolean(b)) => b,
        _ => default,
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
        let face = self.interface();
        let mut out = Vec::new();
        for (i, w) in widgets(self).iter().enumerate() {
            match &face.widgets[i].kind {
                ui::Kind::Table { cells, .. } => out.extend(
                    cells
                        .iter()
                        .enumerate()
                        .map(|(c, v)| (control_id(i + 1, c + 1), *v)),
                ),
                ui::Kind::Xy { .. } => {} // Axes share their original knobs' controls.
                _ if matches!(face.widgets[i].binding, ui::Binding::Control(_)) => {
                    out.push((control_id(i + 1, 0), num(w, "value").unwrap_or(0.0)));
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
        self.shared.arm(self.shared.config.callback);
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
        let mut out = ui::Interface {
            source: ui::Source::FalconLua,
            ..Default::default()
        };
        let Ok(root) = self.lua.globals().raw_get::<Table>("__ui") else {
            return out;
        };
        let widgets = widgets(self);
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
        for (index, w) in widgets.iter().enumerate() {
            let kind = text(w, "kind").unwrap_or_default();
            let name = text(w, "name").unwrap_or_default();
            let (x, y, wd, ht) = (
                num(w, "x").unwrap_or(0.),
                num(w, "y").unwrap_or(0.),
                num(w, "width").unwrap_or(0.).max(0.),
                num(w, "height").unwrap_or(0.).max(0.),
            );
            let value = num(w, "value").unwrap_or(0.);
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
                w.get::<Table>("items")
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
                        .get::<Table>("values")
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
                ui::Rect::new(x as i32, y as i32, wd as u32, ht as u32),
                kind_ir,
            );
            wd.source_id = Some(index as i32 + 1);
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
            wd.hide.title = !flag(w, "showLabel", true);
            wd.hide.value = !flag(w, "showValue", true);
            wd.parent = num(w, "parent_id")
                .filter(|id| *id > 0.)
                .map(|id| ui::WidgetRef(id as usize - 1));
            wd.automation.allowed = flag(w, "exported", false);
            wd.automation.id = num(w, "paramId").map(|n| n as u32);
            wd.mapper = text(w, "mapper");
            wd.menu_cycle = kind == "MultiStateButton";
            wd.colors.background = text(w, "backgroundColour").and_then(|c| color(&c));
            wd.colors.on = text(w, "backgroundColourOn").and_then(|c| color(&c));
            wd.colors.off = text(w, "backgroundColourOff").and_then(|c| color(&c));
            wd.colors.bar = text(w, "sliderColour")
                .or_else(|| text(w, "fillColour"))
                .and_then(|c| color(&c));
            if kind == "Viewport" {
                wd.viewport = Some(
                    w.get::<Table>("viewPosition")
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
                wd.binding = ui::Binding::Meter {
                    bus: None,
                    channel: num(w, "channel").unwrap_or(0.) as u8,
                };
                if let Ok(element) = w.get::<Table>("meter_element")
                    && text(&element, "type").as_deref() != Some("Program")
                {
                    out.unsupported.push(ui::Unsupported {
                        widget: Some(ui::WidgetRef(index)),
                        feature: "AudioMeter element".into(),
                        value: "non-program bus metering unavailable".into(),
                    });
                }
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
                        ..Default::default()
                    };
                    wd.images.push(ui::ImageUse::new(
                        asset(&path, ui::AssetKind::Image(meta), &mut out),
                        role,
                    ));
                }
            }
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
                "stereo",
                "channel",
            ];
            if let Ok(data) = w.raw_get::<Table>("__data") {
                for (key, _) in data.pairs::<String, Value>().flatten() {
                    // Lua scripts freely attach private data/functions to widgets.
                    // Only documented UI properties count as rendering gaps.
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
                    if !modeled.contains(&key.as_str()) && documented.contains(&key.as_str()) {
                        out.unsupported.push(ui::Unsupported {
                            widget: Some(ui::WidgetRef(index)),
                            feature: format!("{kind}.{key}"),
                            value: "property not represented by renderer".into(),
                        });
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
        let width = out
            .widgets
            .iter()
            .enumerate()
            .map(|(i, _)| out.page_rect(ui::WidgetRef(i)))
            .map(|r| r.x.saturating_add(r.width as i32).max(0) as u32)
            .max()
            .unwrap_or(720);
        let height = out
            .widgets
            .iter()
            .enumerate()
            .map(|(i, _)| out.page_rect(ui::WidgetRef(i)))
            .map(|r| r.y.saturating_add(r.height as i32).max(0) as u32)
            .max()
            .unwrap_or(100);
        let background = text(&root, "background")
            .filter(|s| !s.is_empty())
            .map(|s| asset(&s, ui::AssetKind::Image(Default::default()), &mut out));
        out.pages.push(ui::Page {
            name: "main".into(),
            size: ui::Size {
                width: num(&root, "width").unwrap_or(width as f64).max(1.) as u32,
                height: num(&root, "height").unwrap_or(height as f64).max(1.) as u32,
            },
            background: ui::Background {
                image: background,
                color: text(&root, "backgroundColour").and_then(|c| color(&c)),
                ..Default::default()
            },
            ..Default::default()
        });
        out
    }
}

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
    pub fn save_ui_state(&self) -> Result<UiState, String> {
        self.shared.arm(self.shared.config.load);
        let mut out = UiState::default();
        let mut remaining = 65536;
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
        if let Ok(Value::Function(f)) = self.lua.globals().raw_get::<Value>("onSave") {
            let value = f.call::<Value>(()).map_err(super::lua_error)?;
            out.custom = Some(save_value(value, &mut Vec::new(), &mut remaining)?);
        }
        if serde_json::to_vec(&out).map_err(|e| e.to_string())?.len() > 8 << 20 {
            return Err("UVI state exceeds 8 MiB".into());
        }
        Ok(out)
    }
    pub(super) fn restore_ui_values(&self, state: &UiState) -> Result<(), String> {
        self.shared.arm(self.shared.config.load);
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
            self.shared.arm(self.shared.config.load);
            f.call::<()>(load_value(&self.lua, value, 0, &mut 65536)?)
                .map_err(super::lua_error)?;
            self.cycle();
        }
        Ok(())
    }
}
