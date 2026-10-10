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
    "$CONTROL_PAR_TEXTLINE",
    "$CONTROL_PAR_HELP",
    "$CONTROL_PAR_UNIT",
    "$CONTROL_PAR_MIN_VALUE",
    "$CONTROL_PAR_MAX_VALUE",
    "$CONTROL_PAR_DEFAULT_VALUE",
    "$CONTROL_PAR_PICTURE",
    "$CONTROL_PAR_CURSOR_PICTURE",
    "$CONTROL_PAR_AUTOMATION_NAME",
    "$CONTROL_PAR_FONT_TYPE",
    "$CONTROL_PAR_FONT_TYPE_ON",
    "$CONTROL_PAR_FONT_TYPE_OFF_PRESSED",
    "$CONTROL_PAR_FONT_TYPE_ON_PRESSED",
    "$CONTROL_PAR_FONT_TYPE_OFF_HOVER",
    "$CONTROL_PAR_FONT_TYPE_ON_HOVER",
    "$CONTROL_PAR_TEXT_ALIGNMENT",
    "$CONTROL_PAR_Z_LAYER",
    "$CONTROL_PAR_PARENT_PANEL",
    "$CONTROL_PAR_VERTICAL",
    "$CONTROL_PAR_RANGE_MIN",
    "$CONTROL_PAR_RANGE_MAX",
    "$CONTROL_PAR_WT_ZONE",
    "$CONTROL_PAR_TEXTPOS_Y",
    "$CONTROL_PAR_VALUEPOS_Y",
    "$CONTROL_PAR_ALLOW_AUTOMATION",
    "$CONTROL_PAR_AUTOMATION_ID",
    "$CONTROL_PAR_SHORT_NAME",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR",
    "$CONTROL_PAR_LABEL",
    "$CONTROL_PAR_PICTURE_STATE",
    "$CONTROL_PAR_SHOW_ARROWS",
    "$CONTROL_PAR_BASEPATH",
    "$CONTROL_PAR_FILE_TYPE",
    "$CONTROL_PAR_COLUMN_WIDTH",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR_X",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR_Y",
    "$CONTROL_PAR_MOUSE_MODE",
    "$CONTROL_PAR_ACTIVE_INDEX",
    "$CONTROL_PAR_WT_VIS_MODE",
    "$CONTROL_PAR_PARALLAX_X",
    "$CONTROL_PAR_PARALLAX_Y",
    "grid_x",
    "grid_y",
    "table_steps_shown",
];

/// `$CONTROL_PAR_*_COLOR` properties by IR colour slot.
type ColorSlot = fn(&mut ir::Colors) -> &mut Option<ir::Rgba>;
const COLORS: [(&str, ColorSlot); 10] = [
    ("$CONTROL_PAR_BG_COLOR", |c| &mut c.background),
    ("$CONTROL_PAR_ON_COLOR", |c| &mut c.on),
    ("$CONTROL_PAR_OFF_COLOR", |c| &mut c.off),
    ("$CONTROL_PAR_BAR_COLOR", |c| &mut c.bar),
    ("$CONTROL_PAR_PEAK_COLOR", |c| &mut c.peak),
    ("$CONTROL_PAR_OVERLOAD_COLOR", |c| &mut c.overload),
    ("$CONTROL_PAR_ZERO_LINE_COLOR", |c| &mut c.zero_line),
    ("$CONTROL_PAR_WAVE_COLOR", |c| &mut c.wave),
    ("$CONTROL_PAR_WAVE_CURSOR_COLOR", |c| &mut c.wave_cursor),
    ("$CONTROL_PAR_SLICEMARKERS_COLOR", |c| &mut c.slice_markers),
];

/// KSP colours are `0xRRGGBB`, or `0xAARRGGBB` when the top byte is set.
fn color(v: i32) -> ir::Rgba {
    let v = v as u32;
    let a = (v >> 24) as u8;
    ir::Rgba {
        a: if a == 0 { 255 } else { a },
        ..ir::Rgba::rgb(v & 0xFF_FFFF)
    }
}

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
            "Horizontal Resizable" => meta.stretch[0] = yes,
            "Vertical Resizable" => meta.stretch[1] = yes,
            "Fixed Top" => meta.margins.top = n,
            "Fixed Bottom" => meta.margins.bottom = n,
            "Fixed Left" => meta.margins.left = n,
            "Fixed Right" => meta.margins.right = n,
            _ => {}
        }
    }
    meta
}

/// Pixel size of a PNG from its header.
pub fn png_size(png: &[u8]) -> Option<ir::Size> {
    let header = png.get(..24)?;
    if &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
        return None;
    }
    let be = |at: usize| u32::from_be_bytes(header[at..at + 4].try_into().unwrap());
    Some(ir::Size {
        width: be(16),
        height: be(20),
    })
}

/// [`picture_meta`] with `size` set to one frame of `png`: the image divided
/// by its frame count along the animation axis. What a loader's picture
/// callback returns given the `.txt` (if any) and the `.png` bytes.
pub fn picture_meta_png(txt: Option<&str>, png: &[u8]) -> ir::ImageMeta {
    let mut meta = picture_meta(txt.unwrap_or_default());
    meta.size = png_size(png).map(|s| match meta.axis {
        ir::Orientation::Horizontal => ir::Size {
            width: s.width / meta.frames,
            ..s
        },
        ir::Orientation::Vertical => ir::Size {
            height: s.height / meta.frames,
            ..s
        },
    });
    meta
}

struct Builder<'p> {
    ui: ir::Interface,
    assets: HashMap<String, ir::AssetRef>,
    styles: HashMap<(i32, i32), ir::StyleRef>,
    fonts: Vec<ir::AssetRef>,
    picture: &'p dyn Fn(&str) -> Option<ir::ImageMeta>,
}

impl Builder<'_> {
    /// The picture asset for `name`. An empty name has no file: on a
    /// control it clears the picture (stock look); on the instrument's
    /// wallpaper or icon it is a failed lookup in the script and is reported.
    fn asset(&mut self, widget: Option<usize>, feature: &str, name: &str) -> Option<ir::AssetRef> {
        if name.is_empty() {
            if widget.is_none() {
                let feature = format!("{feature} (empty picture name)");
                self.unsupported(None, feature, String::new());
            }
            return None;
        }
        let path = picture_path(name);
        if let Some(&a) = self.assets.get(&path) {
            return Some(a);
        }
        let a = ir::AssetRef(self.ui.assets.len());
        let meta = (self.picture)(&path).unwrap_or_default();
        self.ui.assets.push(ir::Asset {
            path: path.clone(),
            kind: ir::AssetKind::Image(meta),
        });
        self.assets.insert(path, a);
        Some(a)
    }
    fn style(&mut self, font: i32, align: i32) -> ir::StyleRef {
        *self.styles.entry((font, align)).or_insert_with(|| {
            // Kontakt's factory fonts carry their own colour and size, sampled
            // from NI's font chart (KSP manual, control parameters).
            const COLORS: [u32; 26] = [
                0xfefefe, 0xfefefe, 0x373733, 0xcccccc, 0xe3c269, 0xe4d182, 0x7d3012, 0x853519,
                0x48443c, 0x000000, 0xd9d9d9, 0x898c8d, 0x1b1b1a, 0xd2dce1, 0xb9b9b9, 0x5f5f5f,
                0x000000, 0xfefefe, 0xfefefe, 0x000000, 0x7f7f7f, 0x7f7f7f, 0x000000, 0x7f7f7f,
                0xffffff, 0x0c2431,
            ];
            let factory = usize::try_from(font).ok().filter(|&f| f < COLORS.len());
            self.ui.styles.push(ir::TextStyle {
                font: font
                    .checked_sub(26)
                    .and_then(|n| self.fonts.get(n as usize))
                    .copied()
                    .map_or(ir::Font::Stock(font), ir::Font::Bitmap),
                size: factory
                    .filter(|f| matches!(f, 1 | 5 | 7 | 16 | 17 | 20))
                    .map(|_| 13.),
                color: factory.map_or(ir::Rgba::default(), |f| ir::Rgba::rgb(COLORS[f])),
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
            performance: m.performance_view,
            ..Default::default()
        },
        assets: HashMap::new(),
        styles: HashMap::new(),
        fonts: Vec::new(),
        picture,
    };
    for name in &m.fonts {
        let asset = ir::AssetRef(bld.ui.assets.len());
        bld.ui.assets.push(ir::Asset {
            path: picture_path(name),
            kind: ir::AssetKind::BitmapFont,
        });
        bld.fonts.push(asset);
    }
    bld.ui.native_ui = model
        .requests
        .iter()
        .rev()
        .find(|r| r.command == "load_native_ui")
        .and_then(|r| r.args.last())
        .and_then(|v| match v {
            Value::Text(entry) if !entry.is_empty() => Some(ir::NativeUi {
                entry: entry.clone(),
            }),
            _ => None,
        });
    let page = ir::PageRef(0);
    let mut background = ir::Background {
        offset_y: m.skin_offset.unwrap_or(0),
        // Pinned classic Kontakt profile: frame origin + header + skin offset.
        origin_y: 68,
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
                background.image = bld.asset(None, "$INST_WALLPAPER_ID", &text(v));
            } else if id == b::INST_WALLPAPER_ID && name == "$CONTROL_PAR_PICTURE_STATE" {
                background.frame = match v {
                    Value::Int(n) => (*n).max(0) as u32,
                    _ => 0,
                };
            } else if id == b::INST_ICON_ID && name == "$CONTROL_PAR_PICTURE" {
                bld.ui.icon = bld.asset(None, "$INST_ICON_ID", &text(v));
            } else if id == b::INST_ICON_ID && name == "$CONTROL_PAR_HIDE" {
                bld.ui.icon_hidden = matches!(v, Value::Int(h) if h & b::HIDE_WHOLE_CONTROL != 0);
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
    let height_rows = match m.height_px {
        None => m.height_grid.map(|r| r.max(1) as u32),
        Some(_) => None,
    };
    // ponytail: 100 px when the script sets no height at all; Kontakt's own default may differ.
    let height = match (m.height_px, height_rows) {
        (Some(px), _) => px.max(1) as u32,
        (None, Some(_)) => 0,
        (None, None) => 100,
    };
    let width = m.width_px.map_or(DEFAULT_WIDTH, |w| w.max(1) as u32);
    bld.ui.pages.push(ir::Page {
        name: m.title.clone().unwrap_or_default(),
        size: ir::Size { width, height },
        background,
        height_rows,
    });

    let widgets: Vec<_> = m.widgets.iter().filter(|w| !w.unresolved).collect();
    let by_id: HashMap<i32, usize> = widgets
        .iter()
        .enumerate()
        .map(|(i, w)| (w.ui_id, i))
        .collect();
    for (i, w) in widgets.iter().enumerate() {
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
        let kind = match w.kind {
            WidgetKind::Panel => ir::Kind::Panel,
            WidgetKind::Knob => ir::Kind::Knob {
                range: range(1_000_000),
                display: display(),
            },
            WidgetKind::Slider => ir::Kind::Slider {
                range: range(1_000_000),
                orientation: if int("$CONTROL_PAR_MOUSE_BEHAVIOUR").is_some_and(|m| m < 0) {
                    ir::Orientation::Vertical
                } else {
                    ir::Orientation::Horizontal
                },
            },
            WidgetKind::Button => ir::Kind::Button { momentary: false },
            WidgetKind::Switch => ir::Kind::Switch,
            WidgetKind::Menu => ir::Kind::Menu {
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
            WidgetKind::Label => ir::Kind::Label,
            WidgetKind::ValueEdit => ir::Kind::ValueEdit {
                range: range(1_000_000),
                display: display(),
                arrows: int("$CONTROL_PAR_SHOW_ARROWS") != Some(0),
            },
            WidgetKind::Table => {
                // declare ui_table %t[columns](width, height, range); negative is bipolar.
                let r = w.params.get(2).copied().unwrap_or(100);
                let mut cells = match &w.value {
                    WidgetValue::Ints(v) => v.clone(),
                    _ => vec![0; len as usize],
                };
                for (&i, v) in w
                    .indexed_properties
                    .get("$CONTROL_PAR_VALUE")
                    .into_iter()
                    .flatten()
                {
                    if let (Some(c), Value::Int(n)) = (cells.get_mut(i as usize), v) {
                        *c = *n;
                    }
                }
                ir::Kind::Table {
                    columns: len,
                    range: ir::Range {
                        min: if r < 0 { f64::from(r) } else { 0.0 },
                        max: f64::from(r.unsigned_abs()),
                        default: 0.0,
                        step: Some(1.0),
                    },
                    bipolar: r < 0,
                    cells: cells.into_iter().map(f64::from).collect(),
                    steps_shown: int("table_steps_shown").and_then(|n| u32::try_from(n).ok()),
                }
            }
            WidgetKind::Xy => ir::Kind::Xy {
                cursors: len / 2,
                sensitivity: [
                    "$CONTROL_PAR_MOUSE_BEHAVIOUR_X",
                    "$CONTROL_PAR_MOUSE_BEHAVIOUR_Y",
                ]
                .map(|p| int(p).map(i32::unsigned_abs)),
                mouse_mode: int("$CONTROL_PAR_MOUSE_MODE"),
            },
            WidgetKind::Waveform => ir::Kind::Waveform,
            WidgetKind::Wavetable => ir::Kind::Wavetable {
                view_mode: int("$CONTROL_PAR_WT_VIS_MODE"),
                parallax: ["$CONTROL_PAR_PARALLAX_X", "$CONTROL_PAR_PARALLAX_Y"]
                    .map(|p| int(p).unwrap_or(0)),
            },
            WidgetKind::LevelMeter => ir::Kind::LevelMeter {
                orientation: if int("$CONTROL_PAR_VERTICAL") != Some(1) {
                    ir::Orientation::Horizontal
                } else {
                    ir::Orientation::Vertical
                },
            },
            WidgetKind::FileSelector => ir::Kind::FileSelector {
                base_path: w.text("$CONTROL_PAR_BASEPATH").map(Into::into),
                // $NI_FILE_TYPE_MIDI, _AUDIO, _ARRAY.
                files: match int("$CONTROL_PAR_FILE_TYPE") {
                    Some(0) => ir::Files::Midi,
                    Some(1) => ir::Files::Audio,
                    Some(2) => ir::Files::Data,
                    _ => ir::Files::Any,
                },
                column_width: int("$CONTROL_PAR_COLUMN_WIDTH").and_then(|n| u32::try_from(n).ok()),
            },
            WidgetKind::TextEdit => ir::Kind::TextEdit,
            WidgetKind::MouseArea => ir::Kind::MouseArea,
        };
        // Missing sizes stay 0: the renderer applies Kontakt's stock sizes.
        let (width, height) = (int("$CONTROL_PAR_WIDTH"), int("$CONTROL_PAR_HEIGHT"));
        let rect = ir::Rect::new(
            int("$CONTROL_PAR_POS_X").unwrap_or(0),
            int("$CONTROL_PAR_POS_Y").unwrap_or(0),
            width.map_or(0, |v| v.max(0) as u32),
            height.map_or(0, |v| v.max(0) as u32),
        );
        let mut out = ir::Widget::new(w.name.clone(), page, rect, kind);
        out.auto_size = width.is_none() || height.is_none();
        out.default_axes = [width.is_none(), height.is_none()];
        out.source_id = Some(w.ui_id);
        out.active_index = int("$CONTROL_PAR_ACTIVE_INDEX");
        out.value = match &w.value {
            WidgetValue::None => None,
            WidgetValue::Int(v) => Some(ir::Value::Integer(*v)),
            WidgetValue::Text(v) => Some(ir::Value::Text(v.clone())),
            WidgetValue::Ints(v) => Some(ir::Value::Integers(v.clone())),
            WidgetValue::Reals(v) => Some(ir::Value::Reals(v.clone())),
        };
        if let ir::Kind::Table { cells, .. } = &out.kind {
            out.value = Some(ir::Value::Integers(
                cells.iter().map(|n| n.round() as i32).collect(),
            ));
        }
        if matches!(w.kind, WidgetKind::Waveform | WidgetKind::Wavetable) {
            out.waveform = waveform(model, w.ui_id).or_else(|| {
                int("$CONTROL_PAR_WT_ZONE").map(|zone| ir::Waveform {
                    zone,
                    flags: 0,
                    cursor_us: 0,
                    table: vec![],
                    highlighted: None,
                    midi_start_note: 60,
                })
            });
        }
        if w.kind == WidgetKind::LevelMeter {
            out.meter = meter_address(model, w.ui_id);
            out.meter_range = Some([
                int("$CONTROL_PAR_RANGE_MIN").unwrap_or(0),
                int("$CONTROL_PAR_RANGE_MAX").unwrap_or(1_000_000),
            ]);
        }
        out.z = int("$CONTROL_PAR_Z_LAYER").unwrap_or(0);
        let hide = int("$CONTROL_PAR_HIDE").unwrap_or(0);
        out.hidden = hide & b::HIDE_WHOLE_CONTROL != 0;
        // move_control(x, y) places on Kontakt's grid; (0, 0) hides.
        if let (Some(column), Some(row)) = (int("grid_x"), int("grid_y")) {
            if column > 0 && row > 0 {
                out.placement = ir::Placement::Grid {
                    column: column as u32,
                    row: row as u32,
                };
            } else {
                out.hidden = true;
            }
        }
        out.text_y = int("$CONTROL_PAR_TEXTPOS_Y");
        out.value_y = int("$CONTROL_PAR_VALUEPOS_Y");
        out.value_text = w.text("$CONTROL_PAR_LABEL").map(Into::into);
        out.drag = int("$CONTROL_PAR_MOUSE_BEHAVIOUR").map(|m| ir::Drag {
            axis: if m < 0 {
                ir::Orientation::Vertical
            } else {
                ir::Orientation::Horizontal
            },
            sensitivity: m.unsigned_abs(),
        });
        for (name, slot) in COLORS {
            if let Some(c) = int(name) {
                *slot(&mut out.colors) = Some(color(c));
            }
        }
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
        let lines = w
            .indexed_properties
            .get("$CONTROL_PAR_TEXT")
            .or_else(|| w.indexed_properties.get("$CONTROL_PAR_TEXTLINE"))
            .map(|l| l.values().map(text).collect::<Vec<_>>().join("\n"));
        out.text = lines
            .as_deref()
            .or(w.text("$CONTROL_PAR_TEXT"))
            .or(w.text("$CONTROL_PAR_TEXTLINE"))
            .map_or_else(
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
        out.automation = ir::Automation {
            name: w.text("$CONTROL_PAR_AUTOMATION_NAME").map(Into::into),
            short_name: w.text("$CONTROL_PAR_SHORT_NAME").map(Into::into),
            allowed: int("$CONTROL_PAR_ALLOW_AUTOMATION") != Some(0),
            id: int("$CONTROL_PAR_AUTOMATION_ID").and_then(|n| u32::try_from(n).ok()),
        };
        if int("$CONTROL_PAR_FONT_TYPE").is_some() || int("$CONTROL_PAR_TEXT_ALIGNMENT").is_some() {
            let font = int("$CONTROL_PAR_FONT_TYPE").unwrap_or(0);
            let align = int("$CONTROL_PAR_TEXT_ALIGNMENT").unwrap_or(1);
            out.style = Some(bld.style(font, align));
        }
        for (index, property) in [
            "$CONTROL_PAR_FONT_TYPE",
            "$CONTROL_PAR_FONT_TYPE_ON",
            "$CONTROL_PAR_FONT_TYPE_OFF_PRESSED",
            "$CONTROL_PAR_FONT_TYPE_ON_PRESSED",
            "$CONTROL_PAR_FONT_TYPE_OFF_HOVER",
            "$CONTROL_PAR_FONT_TYPE_ON_HOVER",
        ]
        .iter()
        .enumerate()
        {
            out.state_styles[index] = int(property)
                .filter(|&font| font >= 0)
                .map(|font| bld.style(font, int("$CONTROL_PAR_TEXT_ALIGNMENT").unwrap_or(1)));
        }
        if let Some(p) = w.text("$CONTROL_PAR_PICTURE")
            && let Some(asset) = bld.asset(Some(i), "$CONTROL_PAR_PICTURE", p)
        {
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
            let mut image = ir::ImageUse::new(asset, role);
            image.frame = int("$CONTROL_PAR_PICTURE_STATE").and_then(|f| u32::try_from(f).ok());
            out.images.push(image);
        }
        if let Some(p) = w.text("$CONTROL_PAR_CURSOR_PICTURE")
            && let Some(asset) = bld.asset(Some(i), "$CONTROL_PAR_CURSOR_PICTURE", p)
        {
            out.images.push(ir::ImageUse::new(asset, ir::Role::Handle));
        }
        if let Some(parent) = int("$CONTROL_PAR_PARENT_PANEL") {
            match by_id.get(&parent) {
                Some(&p) if widgets[p].kind == WidgetKind::Panel && p != i => {
                    out.parent = Some(ir::WidgetRef(p));
                }
                _ => bld.unsupported(Some(i), "$CONTROL_PAR_PARENT_PANEL", parent.to_string()),
            }
        }
        if hide & 8 != 0 {
            bld.unsupported(Some(i), "$HIDE_PART_MOD_LIGHT", hide.to_string());
        }
        for (name, v) in &w.properties {
            if !MAPPED.contains(&name.as_str()) && !COLORS.iter().any(|(c, _)| c == name) {
                bld.unsupported(Some(i), name.clone(), text(v));
            }
        }
        // One entry per indexed property: tables write thousands of cells.
        for (name, values) in &w.indexed_properties {
            let mapped = match name.as_str() {
                "$CONTROL_PAR_VALUE" => w.kind == WidgetKind::Table,
                "$CONTROL_PAR_TEXT" | "$CONTROL_PAR_TEXTLINE" => true,
                _ => false,
            };
            if mapped {
                continue;
            }
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

/// Keep all four dimensions of `attach_level_meter`, including group/effect taps.
fn meter_address(model: &model::Model, ui_id: i32) -> Option<ir::MeterAddress> {
    let request = model.requests.iter().rev().find(|r| {
        r.command == "attach_level_meter" && r.args.first() == Some(&Value::Int(ui_id))
    })?;
    let int = |at| match request.args.get(at) {
        Some(Value::Int(v)) => *v,
        _ => -1,
    };
    Some(ir::MeterAddress {
        group: int(1),
        slot: int(2),
        channel: u8::try_from(int(3)).ok()?,
        bus: (int(4) >= 0).then(|| int(4)),
    })
}

fn waveform(model: &model::Model, ui_id: i32) -> Option<ir::Waveform> {
    let name = model
        .interface
        .widgets
        .iter()
        .find(|w| w.ui_id == ui_id)?
        .name
        .as_str();
    waveform_requests(model, ui_id, name)
}

/// Init has HIR identities but no assembled widget list yet.
pub(crate) fn waveform_requests(model: &model::Model, ui_id: i32, name: &str) -> Option<ir::Waveform> {
    let mut wave = None;
    for request in &model.requests {
        if !matches!(request.args.first(),Some(Value::Int(id)) if *id == ui_id)
            && !matches!(request.args.first(),Some(Value::Text(v)) if v == name)
        {
            continue;
        }
        match request.command {
            "attach_zone" => {
                let [_, Value::Int(zone), Value::Int(flags)] = request.args.as_slice() else {
                    continue;
                };
                wave = Some(ir::Waveform {
                    zone: *zone,
                    flags: *flags as u32,
                    cursor_us: 0,
                    table: vec![],
                    highlighted: None,
                    midi_start_note: 60,
                })
            }
            "set_ui_wf_property" => {
                // NI: (variable, property, index, value). Do not accept the
                // inconsistent three-operand setter in the getter's example.
                let [_, Value::Text(property), Value::Int(index), Value::Int(value)] =
                    request.args.as_slice()
                else {
                    continue;
                };
                if let Some(w) = wave.as_mut() {
                    // Property names are interned symbols, not guessed numeric ordinals.
                    match property.as_str() {
                        "$UI_WF_PROP_PLAY_CURSOR" => w.cursor_us = i64::from(*value),
                        "$UI_WF_PROP_FLAGS" => w.flags = *value as u32,
                        // ponytail: bounded slice annotation snapshot; larger arrays need paged storage.
                        "$UI_WF_PROP_TABLE_VAL" => {
                            if let Ok(index) = usize::try_from(*index) {
                                if index < 65536 {
                                    w.table.resize(w.table.len().max(index + 1), 0);
                                    w.table[index] = *value;
                                }
                            }
                        }
                        "$UI_WF_PROP_TABLE_IDX_HIGHLIGHT" => {
                            // Retain the indexed slice. The value's toggle law
                            // is not established by the selected NI specification.
                            w.highlighted = u32::try_from(*index).ok()
                        }
                        "$UI_WF_PROP_MIDI_DRAG_START_NOTE" => {
                            w.midi_start_note = (*value).clamp(0, 127) as u8
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    wave
}
