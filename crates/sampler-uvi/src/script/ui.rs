//! The widgets a Lua script built, as a `sampler_ui_ir::Interface`.
//!
//! The prelude's widget constructors record every widget; after the script has
//! initialised, this reads them back. Values are the ones the script left (the
//! preset's saved state included). Properties the IR has no place for are
//! listed in `Interface::unsupported`.
use super::ScriptHost;
use mlua::{Table, Value};
use sampler_ui_ir as ui;
use std::collections::BTreeMap;

fn text(t: &Table, key: &str) -> Option<String> {
    match t.raw_get::<Value>(key).ok()? {
        Value::String(s) => s.to_str().ok().map(|s| s.to_string()),
        _ => None,
    }
}

fn num(t: &Table, key: &str) -> Option<f64> {
    match t.raw_get::<Value>(key).ok()? {
        Value::Number(n) => Some(n),
        Value::Integer(n) => Some(n as f64),
        _ => None,
    }
}

fn flag(t: &Table, key: &str, default: bool) -> bool {
    match t.raw_get::<Value>(key) {
        Ok(Value::Boolean(b)) => b,
        _ => default,
    }
}

impl ScriptHost {
    /// The interface the script declared; empty (no widgets) without UI code.
    pub fn interface(&self) -> ui::Interface {
        let mut out = ui::Interface {
            source: ui::Source::FalconLua,
            ..Default::default()
        };
        let Ok(root) = self.lua.globals().get::<Table>("__ui") else {
            return out;
        };
        let Ok(list) = root.raw_get::<Table>("widgets") else {
            return out;
        };
        let widgets: Vec<Table> = list.sequence_values::<Table>().flatten().collect();
        let mut assets: BTreeMap<String, usize> = BTreeMap::new();
        let mut asset = |path: &str| -> ui::AssetRef {
            let next = assets.len();
            let index = *assets.entry(path.to_owned()).or_insert(next);
            ui::AssetRef(index)
        };
        let mut width = 0.0f64;
        let mut height = 0.0f64;
        let mut unsupported: Vec<(usize, String, String)> = Vec::new();
        for (index, w) in widgets.iter().enumerate() {
            let kind = text(w, "kind").unwrap_or_default();
            let name = text(w, "name").unwrap_or_default();
            let (x, y) = (num(w, "x").unwrap_or(0.0), num(w, "y").unwrap_or(0.0));
            let (wd, ht) = (
                num(w, "width").unwrap_or(0.0).max(0.0),
                num(w, "height").unwrap_or(0.0).max(0.0),
            );
            let range = ui::Range {
                min: num(w, "min").unwrap_or(0.0),
                max: num(w, "max").unwrap_or(1.0),
                default: num(w, "value").unwrap_or(0.0),
                step: flag(w, "integer", false).then_some(1.0),
            };
            let items = || -> Vec<ui::MenuItem> {
                let Ok(items) = w.raw_get::<Table>("items") else {
                    return Vec::new();
                };
                items
                    .sequence_values::<Value>()
                    .flatten()
                    .enumerate()
                    .filter_map(|(i, v)| match v {
                        Value::String(s) => Some(ui::MenuItem {
                            text: s.to_string_lossy().to_string(),
                            value: i as i32 + 1,
                            visible: true,
                        }),
                        _ => None,
                    })
                    .collect()
            };
            let mut binding = ui::Binding::Variable {
                script: 0,
                name: name.clone(),
            };
            let kind_ir = match kind.as_str() {
                "Panel" | "Frame" => {
                    binding = ui::Binding::None;
                    ui::Kind::Panel
                }
                "Knob" => ui::Kind::Knob {
                    range,
                    display: Default::default(),
                },
                "Slider" => ui::Kind::Slider {
                    range,
                    orientation: if wd > ht {
                        ui::Orientation::Horizontal
                    } else {
                        ui::Orientation::Vertical
                    },
                },
                "OnOffButton" => ui::Kind::Button { momentary: false },
                "Button" => ui::Kind::Button { momentary: true },
                "Menu" => ui::Kind::Menu { items: items() },
                "NumBox" => ui::Kind::ValueEdit {
                    range,
                    display: Default::default(),
                    arrows: false,
                },
                "Label" | "Text" => {
                    binding = ui::Binding::None;
                    ui::Kind::Label
                }
                "Image" => {
                    binding = ui::Binding::None;
                    ui::Kind::Image
                }
                "Table" => ui::Kind::Table {
                    columns: num(w, "length").unwrap_or(0.0) as u32,
                    range: ui::Range {
                        default: 0.0,
                        ..range
                    },
                    bipolar: range.min < 0.0,
                    cells: w
                        .raw_get::<Table>("values")
                        .map(|v| {
                            v.sequence_values::<f64>()
                                .flatten()
                                .map(|n| n.round() as i32)
                                .collect()
                        })
                        .unwrap_or_default(),
                    steps_shown: None,
                },
                "AudioMeter" => {
                    binding = ui::Binding::Meter {
                        bus: None,
                        channel: 0,
                    };
                    ui::Kind::LevelMeter {
                        orientation: ui::Orientation::Vertical,
                    }
                }
                "XY" => ui::Kind::Xy {
                    cursors: 1,
                    sensitivity: [None; 2],
                    mouse_mode: None,
                },
                "WaveForm" => ui::Kind::Waveform,
                other => {
                    unsupported.push((index, other.to_owned(), "drawn as an empty panel".into()));
                    binding = ui::Binding::None;
                    ui::Kind::Panel
                }
            };
            let mut widget = ui::Widget::new(
                name.clone(),
                ui::PageRef(0),
                ui::Rect::new(x as i32, y as i32, wd as u32, ht as u32),
                kind_ir,
            );
            widget.source_id = Some(index as i32 + 1);
            widget.binding = binding;
            widget.hidden = !flag(w, "visible", true);
            widget.enabled = flag(w, "enabled", true);
            widget.tooltip = text(w, "tooltip").unwrap_or_default();
            if matches!(kind.as_str(), "Label" | "Button" | "OnOffButton" | "Menu") {
                widget.text = text(w, "text")
                    .filter(|t| !t.is_empty())
                    .or_else(|| text(w, "displayName"))
                    .unwrap_or_default();
            }
            widget.parent = num(w, "parent_id").map(|id| ui::WidgetRef(id as usize - 1));
            // Pictures: a still image, a background, or a strip of frames.
            let picture = |key: &str| text(w, key).filter(|p| !p.is_empty());
            if kind == "Image" {
                if let Some(path) = picture("image").or_else(|| text(w, "name")) {
                    widget
                        .images
                        .push(ui::ImageUse::new(asset(&path), ui::Role::Background));
                }
            }
            if let Some(path) = picture("backgroundImage") {
                widget
                    .images
                    .push(ui::ImageUse::new(asset(&path), ui::Role::Background));
            }
            for key in ["normalImage", "stripImage"] {
                if let Some(path) = picture(key) {
                    widget
                        .images
                        .push(ui::ImageUse::new(asset(&path), ui::Role::Strip));
                    break;
                }
            }
            width = width.max(x + wd);
            height = height.max(y + ht);
            out.widgets.push(widget);
        }
        let (w, h) = (
            num(&root, "width").unwrap_or(width),
            num(&root, "height").unwrap_or(height),
        );
        let background = text(&root, "background").map(|p| asset(&p));
        out.pages.push(ui::Page {
            name: "main".into(),
            size: ui::Size {
                width: w.max(0.0) as u32,
                height: h.max(0.0) as u32,
            },
            height_rows: None,
            background: ui::Background {
                image: background,
                ..Default::default()
            },
        });
        let mut paths: Vec<(String, usize)> = assets.into_iter().collect();
        paths.sort_by_key(|(_, i)| *i);
        out.assets = paths
            .into_iter()
            .map(|(path, _)| ui::Asset {
                path,
                kind: ui::AssetKind::Image(Default::default()),
            })
            .collect();
        out.unsupported = unsupported
            .into_iter()
            .map(|(i, feature, value)| ui::Unsupported {
                widget: Some(ui::WidgetRef(i)),
                feature,
                value,
            })
            .collect();
        out
    }
}
