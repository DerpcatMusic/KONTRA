//! Emit a script's `on init` interface into the format-neutral UI IR.
use crate::builtins as b;
use crate::model::{self, Value, WidgetKind, WidgetValue};
use sampler_ui_ir as ir;
use std::collections::HashMap;

/// Properties the IR expresses; every other one becomes `Unsupported`.
const MAPPED: &[&str] = &[
    "$CONTROL_PAR_POS_X",
    "$CONTROL_PAR_POS_Y",
    "$CONTROL_PAR_WIDTH",
    "$CONTROL_PAR_HEIGHT",
    "$CONTROL_PAR_HIDE",
    "$CONTROL_PAR_TEXT",
    "$CONTROL_PAR_HELP",
    "$CONTROL_PAR_UNIT",
    "$CONTROL_PAR_MIN_VALUE",
    "$CONTROL_PAR_MAX_VALUE",
    "$CONTROL_PAR_DEFAULT_VALUE",
    "$CONTROL_PAR_PICTURE",
    "$CONTROL_PAR_CURSOR_PICTURE",
    "$CONTROL_PAR_AUTOMATION_NAME",
    "$CONTROL_PAR_FONT_TYPE",
    "$CONTROL_PAR_TEXT_ALIGNMENT",
    "$CONTROL_PAR_Z_LAYER",
    "$CONTROL_PAR_PARENT_PANEL",
    "$CONTROL_PAR_VERTICAL",
];

/// Kontakt's default instrument width in pixels.
const DEFAULT_WIDTH: u32 = 633;

/// Library-relative path of a KSP picture name.
pub fn picture_path(name: &str) -> String {
    format!("Resources/pictures/{name}.png")
}

/// Image layout from a Kontakt picture's `.txt` description. Missing or
/// malformed lines keep the defaults.
pub fn picture_meta(txt: &str) -> ir::ImageMeta {
    let mut meta = ir::ImageMeta::default();
    let (mut resizable, mut margins) = (false, ir::Margins::default());
    for line in txt.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let yes = value.eq_ignore_ascii_case("yes");
        let n = value.parse::<u32>().unwrap_or(0);
        match key.trim() {
            "Has Alpha Channel" => meta.alpha = yes,
            "Number of Animations" => meta.frames = n.max(1),
            "Horizontal Animation" if yes => meta.axis = ir::Orientation::Horizontal,
            "Vertical Resizable" | "Horizontal Resizable" => resizable |= yes,
            "Fixed Top" => margins.top = n,
            "Fixed Bottom" => margins.bottom = n,
            "Fixed Left" => margins.left = n,
            "Fixed Right" => margins.right = n,
            _ => {}
        }
    }
    meta.stretch = resizable.then_some(margins);
    meta
}

struct Builder<'p> {
    ui: ir::Interface,
    assets: HashMap<String, ir::AssetRef>,
    styles: HashMap<(i32, i32), ir::StyleRef>,
    picture: &'p dyn Fn(&str) -> Option<ir::ImageMeta>,
}

impl Builder<'_> {
    fn asset(&mut self, name: &str) -> ir::AssetRef {
        let path = picture_path(name);
        if let Some(&a) = self.assets.get(&path) {
            return a;
        }
        let a = ir::AssetRef(self.ui.assets.len());
        let meta = (self.picture)(&path).unwrap_or_default();
        self.ui.assets.push(ir::Asset {
            path: path.clone(),
            kind: ir::AssetKind::Image(meta),
        });
        self.assets.insert(path, a);
        a
    }
    fn style(&mut self, font: i32, align: i32) -> ir::StyleRef {
        *self.styles.entry((font, align)).or_insert_with(|| {
            self.ui.styles.push(ir::TextStyle {
                font: ir::Font::Stock(font),
                size: None,
                // Kontakt's stock fonts carry their own colour.
                color: ir::Rgba::default(),
                align: match align {
                    0 => ir::Align::Left,
                    2 => ir::Align::Right,
                    _ => ir::Align::Center,
                },
            });
            ir::StyleRef(self.ui.styles.len() - 1)
        })
    }
    fn unsupported(&mut self, widget: Option<usize>, feature: impl Into<String>, value: String) {
        self.ui.unsupported.push(ir::Unsupported {
            widget: widget.map(ir::WidgetRef),
            feature: feature.into(),
            value,
        });
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Real(r) => crate::eval::real_text(*r),
        Value::Text(s) => s.clone(),
    }
}

fn unit(n: i32) -> &'static str {
    match n {
        1 => "dB",
        2 => "Hz",
        3 => "%",
        4 => "ms",
        5 => "oct",
        6 => "st",
        _ => "",
    }
}

/// Build the IR for a compiled script's interface. `picture` returns the
/// metadata of a library-relative image path (see [`picture_meta`]); `None`
/// assumes a single opaque-free frame.
pub fn interface(
    model: &model::Model,
    slot: u8,
    picture: &dyn Fn(&str) -> Option<ir::ImageMeta>,
) -> Result<ir::Interface, ir::Error> {
    let m = &model.interface;
    let mut bld = Builder {
        ui: ir::Interface {
            source: ir::Source::Ksp { slot },
            ..Default::default()
        },
        assets: HashMap::new(),
        styles: HashMap::new(),
        picture,
    };
    let page = ir::PageRef(0);
    let mut background = ir::Background {
        offset_y: m.skin_offset.unwrap_or(0),
        ..Default::default()
    };
    if let Some(r) = model
        .requests
        .iter()
        .rev()
        .find(|r| r.command == "set_ui_color")
        && let Some(Value::Int(c)) = r.args.first()
    {
        background.color = Some(ir::Rgba::rgb(*c as u32));
    }
    for (&id, props) in &m.instrument {
        for (name, v) in props {
            if id == b::INST_WALLPAPER_ID && name == "$CONTROL_PAR_PICTURE" {
                background.image = Some(bld.asset(&text(v)));
            } else {
                let target = if id == b::INST_WALLPAPER_ID {
                    "$INST_WALLPAPER_ID"
                } else {
                    "$INST_ICON_ID"
                };
                bld.unsupported(None, format!("{target} {name}"), text(v));
            }
        }
    }
    if m.height_px.is_none() && m.height_grid.is_some() {
        bld.unsupported(None, "set_ui_height", format!("{:?}", m.height_grid));
    }
    // ponytail: 100 px when the script never sets a height; Kontakt's own default may differ.
    let height = m.height_px.unwrap_or(100).max(1) as u32;
    let width = m.width_px.map_or(DEFAULT_WIDTH, |w| w.max(1) as u32);
    bld.ui.pages.push(ir::Page {
        name: m.title.clone().unwrap_or_default(),
        size: ir::Size { width, height },
        background,
        ..Default::default()
    });

    let by_id: HashMap<i32, usize> = m
        .widgets
        .iter()
        .enumerate()
        .map(|(i, w)| (w.ui_id, i))
        .collect();
    for (i, w) in m.widgets.iter().enumerate() {
        let int = |p: &str| w.int(p);
        let range = |default_max: i32| {
            let (lo, hi) = w.range.unwrap_or((0, default_max));
            let lo = int("$CONTROL_PAR_MIN_VALUE").unwrap_or(lo);
            let hi = int("$CONTROL_PAR_MAX_VALUE").unwrap_or(hi);
            let default = int("$CONTROL_PAR_DEFAULT_VALUE").unwrap_or(lo);
            ir::Range {
                min: f64::from(lo),
                max: f64::from(hi),
                default: f64::from(default),
                step: Some(1.0),
            }
        };
        let display = || ir::Display {
            ratio: f64::from(w.params.get(2).copied().filter(|&r| r != 0).unwrap_or(1)),
            unit: unit(int("$CONTROL_PAR_UNIT").unwrap_or(0)).into(),
        };
        let len = match &w.value {
            WidgetValue::Ints(v) => v.len(),
            WidgetValue::Reals(v) => v.len(),
            _ => 0,
        } as u32;
        let (kind, size) = match w.kind {
            WidgetKind::Panel => (ir::Kind::Panel, (0, 0)),
            WidgetKind::Knob => (
                ir::Kind::Knob {
                    range: range(1_000_000),
                    display: display(),
                },
                (92, 52),
            ),
            WidgetKind::Slider => (
                ir::Kind::Slider {
                    range: range(1_000_000),
                    orientation: ir::Orientation::Horizontal,
                },
                (92, 16),
            ),
            WidgetKind::Button => (ir::Kind::Button { momentary: false }, (92, 20)),
            WidgetKind::Switch => (ir::Kind::Switch, (92, 20)),
            WidgetKind::Menu => (
                ir::Kind::Menu {
                    items: w
                        .menu
                        .iter()
                        .map(|m| ir::MenuItem {
                            text: m.text.clone(),
                            value: m.value,
                            visible: m.visible,
                        })
                        .collect(),
                },
                (92, 20),
            ),
            WidgetKind::Label => (ir::Kind::Label, (92, 20)),
            WidgetKind::ValueEdit => (
                ir::Kind::ValueEdit {
                    range: range(1_000_000),
                    display: display(),
                    arrows: false,
                },
                (92, 20),
            ),
            WidgetKind::Table => {
                // declare ui_table %t[columns](width, height, range); negative is bipolar.
                let r = w.params.get(2).copied().unwrap_or(100);
                let kind = ir::Kind::Table {
                    columns: len,
                    range: ir::Range {
                        min: if r < 0 { f64::from(r) } else { 0.0 },
                        max: f64::from(r.unsigned_abs()),
                        default: 0.0,
                        step: Some(1.0),
                    },
                    bipolar: r < 0,
                    cells: Vec::new(),
                };
                (kind, (92, 92))
            }
            WidgetKind::Xy => (ir::Kind::Xy { cursors: len / 2 }, (92, 92)),
            WidgetKind::Waveform => (ir::Kind::Waveform, (184, 92)),
            WidgetKind::Wavetable => (ir::Kind::Wavetable, (184, 92)),
            WidgetKind::LevelMeter => (
                ir::Kind::LevelMeter {
                    orientation: if int("$CONTROL_PAR_VERTICAL") == Some(0) {
                        ir::Orientation::Horizontal
                    } else {
                        ir::Orientation::Vertical
                    },
                },
                (8, 92),
            ),
            WidgetKind::FileSelector => (ir::Kind::FileSelector, (184, 184)),
            WidgetKind::TextEdit => (ir::Kind::TextEdit, (92, 20)),
            WidgetKind::MouseArea => (ir::Kind::MouseArea, (92, 92)),
        };
        // ponytail: per-kind default sizes approximate Kontakt's; scripts that
        // place controls by pixel nearly always set WIDTH/HEIGHT.
        let rect = ir::Rect::new(
            int("$CONTROL_PAR_POS_X").unwrap_or(0),
            int("$CONTROL_PAR_POS_Y").unwrap_or(0),
            int("$CONTROL_PAR_WIDTH").map_or(size.0, |v| v.max(0) as u32),
            int("$CONTROL_PAR_HEIGHT").map_or(size.1, |v| v.max(0) as u32),
        );
        let mut out = ir::Widget::new(w.name.clone(), page, rect, kind);
        out.source_id = Some(w.ui_id);
        out.z = int("$CONTROL_PAR_Z_LAYER").unwrap_or(0);
        let hide = int("$CONTROL_PAR_HIDE").unwrap_or(0);
        out.hidden = hide & b::HIDE_WHOLE_CONTROL != 0;
        out.hide = ir::Parts {
            background: hide & 1 != 0,
            value: hide & 2 != 0,
            title: hide & 4 != 0,
            unit: false,
        };
        out.binding = match (w.control, w.kind) {
            (Some(c), _) => ir::Binding::Control(ir::ControlId(c.0)),
            (None, WidgetKind::LevelMeter) => meter(model, w.ui_id),
            (None, _) if w.value != WidgetValue::None => ir::Binding::Variable {
                script: slot,
                name: w.name.clone(),
            },
            _ => ir::Binding::None,
        };
        // Kontakt captions controls with their variable name until TEXT is set.
        out.text = w.text("$CONTROL_PAR_TEXT").map_or_else(
            || match w.kind {
                WidgetKind::Knob
                | WidgetKind::Slider
                | WidgetKind::Button
                | WidgetKind::Switch
                | WidgetKind::ValueEdit => w.name[1..].to_string(),
                _ => String::new(),
            },
            str::to_string,
        );
        out.tooltip = w.text("$CONTROL_PAR_HELP").unwrap_or_default().into();
        out.automation.name = w.text("$CONTROL_PAR_AUTOMATION_NAME").map(Into::into);
        if let Some(font) = int("$CONTROL_PAR_FONT_TYPE") {
            let align = int("$CONTROL_PAR_TEXT_ALIGNMENT").unwrap_or(1);
            out.style = Some(bld.style(font, align));
        }
        if let Some(p) = w.text("$CONTROL_PAR_PICTURE").filter(|p| !p.is_empty()) {
            let role = match w.kind {
                WidgetKind::Knob
                | WidgetKind::Slider
                | WidgetKind::Button
                | WidgetKind::Switch
                | WidgetKind::Menu
                | WidgetKind::ValueEdit
                | WidgetKind::LevelMeter => ir::Role::Strip,
                _ => ir::Role::Background,
            };
            out.images.push(ir::ImageUse::new(bld.asset(p), role));
        }
        if let Some(p) = w
            .text("$CONTROL_PAR_CURSOR_PICTURE")
            .filter(|p| !p.is_empty())
        {
            out.images.push(ir::ImageUse::new(bld.asset(p), ir::Role::Handle));
        }
        if let Some(parent) = int("$CONTROL_PAR_PARENT_PANEL") {
            match by_id.get(&parent) {
                Some(&p) if m.widgets[p].kind == WidgetKind::Panel && p != i => {
                    out.parent = Some(ir::WidgetRef(p));
                }
                _ => bld.unsupported(Some(i), "$CONTROL_PAR_PARENT_PANEL", parent.to_string()),
            }
        }
        if hide & 8 != 0 {
            bld.unsupported(Some(i), "$HIDE_PART_MOD_LIGHT", hide.to_string());
        }
        for (name, v) in &w.properties {
            if !MAPPED.contains(&name.as_str()) {
                bld.unsupported(Some(i), name.clone(), text(v));
            }
        }
        // One entry per indexed property: tables write thousands of cells.
        for (name, values) in &w.indexed_properties {
            let shown: Vec<_> = values
                .iter()
                .take(8)
                .map(|(i, v)| format!("[{i}]={}", text(v)))
                .collect();
            let more = values.len().saturating_sub(shown.len());
            let mut value = shown.join(" ");
            if more > 0 {
                value.push_str(&format!(" (+{more} more)"));
            }
            bld.unsupported(Some(i), format!("{name}[]"), value);
        }
        bld.ui.widgets.push(out);
    }
    // A panel parent may itself be nested later in declaration order; reject
    // cycles the script built rather than handing over a malformed tree.
    if let Err(ir::Error::ParentCycle(at)) = bld.ui.validate() {
        let parent = bld.ui.widgets[at.0].parent.take();
        bld.unsupported(
            Some(at.0),
            "$CONTROL_PAR_PARENT_PANEL",
            format!("cycle via {:?}", parent.map(|p| p.0)),
        );
    }
    bld.ui.validate()?;
    Ok(bld.ui)
}

/// `attach_level_meter(ui id, group, slot, channel, bus)`; bus -1 is the instrument.
fn meter(model: &model::Model, ui_id: i32) -> ir::Binding {
    let attached =
        model.requests.iter().rev().find(|r| {
            r.command == "attach_level_meter" && r.args.first() == Some(&Value::Int(ui_id))
        });
    let int = |i: usize| match attached.and_then(|r| r.args.get(i)) {
        Some(Value::Int(n)) => *n,
        _ => -1,
    };
    ir::Binding::Meter {
        bus: u32::try_from(int(4)).ok(),
        channel: int(3).clamp(0, 255) as u8,
    }
}
