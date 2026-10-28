//! A KSP script's performance view as a `kontra_ui_model` tree, built once
//! per load from the declarations and the control store as `on init` left it
//! (07-APP.md §5.1). The UI draws from the tree and needs nothing else from
//! the engine until a control moves.

use std::collections::HashMap;

use kontra_engine::{ControlLook, Engine, Page};
use kontra_ksp::{Cells, Control, ControlKind, FIRST_UI_ID, SYMBOL_BASE, Script};

use crate::perf_view::{View, ViewControl};
use kontra_ui_model as ui;

use crate::script::LOOK;

/// Kontakt's instrument width in pixels when the script sets none.
const DEFAULT_WIDTH: u32 = 633;
/// Classic Kontakt frame: the wallpaper starts under the 68 px header.
const HEADER: u32 = 68;

/// What the tree needs from a compiled script once its program is in the
/// plan: the declared controls and the names behind its symbols.
#[derive(Clone, Debug)]
pub struct Face {
    controls: Vec<(Control, Box<str>, Cells)>,
    symbols: Vec<Box<str>>,
    /// The performance view the script loads, if any.
    view: Option<View>,
}

impl Face {
    #[must_use]
    /// `data` holds the script's library files; a `<name>.nckp` among
    /// them is its performance view.
    pub fn of(script: &Script, data: &[(String, Vec<u8>)]) -> Self {
        let controls = script
            .controls
            .iter()
            .filter_map(|c| {
                let v = script.variables.get(c.variable as usize)?;
                Some((c.clone(), v.name.clone(), v.cells))
            })
            .collect();
        Self {
            controls,
            symbols: script.symbols.clone(),
            view: data
                .iter()
                .find(|(name, _)| name.ends_with(".nckp"))
                .and_then(|(_, bytes)| crate::perf_view::parse(bytes)),
        }
    }

    /// The name of parameter id `par`: a symbol, or a look parameter the
    /// script never named (`script::look_par`).
    fn par_name(&self, par: i32) -> Option<&str> {
        let n = usize::try_from(par.checked_sub(SYMBOL_BASE)?).ok()?;
        match self.symbols.get(n) {
            Some(s) => Some(s),
            None => LOOK.get(n - self.symbols.len()).copied(),
        }
    }
}

/// `$CONTROL_PAR_*_COLOR` properties by colour slot.
type ColorSlot = fn(&mut ui::Colors) -> &mut Option<ui::Rgba>;
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

/// The parameters the tree carries; any other a script set is reported.
const MAPPED: &[&str] = &[
    "$CONTROL_PAR_VALUE",
    "$CONTROL_PAR_TEXT",
    "$CONTROL_PAR_LABEL",
    "$CONTROL_PAR_HELP",
    "$CONTROL_PAR_UNIT",
    "$CONTROL_PAR_DEFAULT_VALUE",
    "$CONTROL_PAR_MIN_VALUE",
    "$CONTROL_PAR_MAX_VALUE",
    "$CONTROL_PAR_POS_X",
    "$CONTROL_PAR_POS_Y",
    "$CONTROL_PAR_WIDTH",
    "$CONTROL_PAR_HEIGHT",
    "$CONTROL_PAR_HIDE",
    "$CONTROL_PAR_Z_LAYER",
    "$CONTROL_PAR_PICTURE",
    "$CONTROL_PAR_PICTURE_STATE",
    "$CONTROL_PAR_PARENT_PANEL",
    "$CONTROL_PAR_TEXTPOS_Y",
    "$CONTROL_PAR_VALUEPOS_Y",
    "$CONTROL_PAR_FONT_TYPE",
    "$CONTROL_PAR_TEXT_ALIGNMENT",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR",
    "$CONTROL_PAR_AUTOMATION_NAME",
    "$CONTROL_PAR_SHORT_NAME",
    "$CONTROL_PAR_ALLOW_AUTOMATION",
    "$CONTROL_PAR_AUTOMATION_ID",
    "$CONTROL_PAR_SHOW_ARROWS",
    "$CONTROL_PAR_VERTICAL",
    "$CONTROL_PAR_DISABLE_TEXT_SHIFTING",
    "$CONTROL_PAR_ACTIVE_INDEX",
    "$CONTROL_PAR_BASEPATH",
    "$CONTROL_PAR_FILE_TYPE",
    "$CONTROL_PAR_COLUMN_WIDTH",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR_X",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR_Y",
    "$CONTROL_PAR_MOUSE_MODE",
    "$CONTROL_PAR_WT_VIS_MODE",
    "$CONTROL_PAR_WT_ZONE",
    "$CONTROL_PAR_PARALLAX_X",
    "$CONTROL_PAR_PARALLAX_Y",
    "grid_x",
    "grid_y",
    "table_steps_shown",
];

/// The text style of each picture state, in [`ui::Widget::state_styles`] order.
const STATE_FONTS: [&str; 6] = [
    "$CONTROL_PAR_FONT_TYPE",
    "$CONTROL_PAR_FONT_TYPE_ON",
    "$CONTROL_PAR_FONT_TYPE_OFF_PRESSED",
    "$CONTROL_PAR_FONT_TYPE_ON_PRESSED",
    "$CONTROL_PAR_FONT_TYPE_OFF_HOVER",
    "$CONTROL_PAR_FONT_TYPE_ON_HOVER",
];

/// Carried, or accepted without effect: NKS properties map a control for
/// Komplete Kontrol hardware and have no view or sound of their own.
fn mapped(par: &str) -> bool {
    MAPPED.contains(&par)
        || STATE_FONTS.contains(&par)
        || COLORS.iter().any(|(c, _)| *c == par)
        || par.starts_with("$CONTROL_PAR_NKS_")
}

/// A control's parameters by name, as the script left them.
struct Props<'a> {
    ints: HashMap<&'a str, i32>,
    texts: HashMap<&'a str, &'a str>,
}

impl Props<'_> {
    fn int(&self, name: &str) -> Option<i32> {
        self.ints.get(name).copied()
    }
    fn text(&self, name: &str) -> Option<&str> {
        self.texts.get(name).copied()
    }
}

struct Builder<'p> {
    ui: ui::Interface,
    assets: HashMap<String, ui::AssetRef>,
    styles: HashMap<(i32, i32), ui::StyleRef>,
    picture: &'p dyn Fn(&str) -> Option<ui::ImageMeta>,
}

impl Builder<'_> {
    /// The picture asset for `name`; an empty name means the stock look.
    fn asset(&mut self, name: &str) -> Option<ui::AssetRef> {
        if name.is_empty() {
            return None;
        }
        let path = picture_path(name);
        if let Some(&a) = self.assets.get(&path) {
            return Some(a);
        }
        let a = ui::AssetRef(self.ui.assets.len());
        let meta = (self.picture)(&path).unwrap_or_default();
        self.ui.assets.push(ui::Asset {
            path: path.clone(),
            kind: ui::AssetKind::Image(meta),
        });
        self.assets.insert(path, a);
        Some(a)
    }

    fn style(&mut self, font: i32, align: i32) -> ui::StyleRef {
        let styles = &mut self.ui.styles;
        *self.styles.entry((font, align)).or_insert_with(|| {
            // Kontakt's factory fonts carry their own colour and size, from
            // NI's font chart (KSP manual, control parameters).
            const COLORS: [u32; 26] = [
                0xfefefe, 0xfefefe, 0x373733, 0xcccccc, 0xe3c269, 0xe4d182, 0x7d3012, 0x853519,
                0x48443c, 0x000000, 0xd9d9d9, 0x898c8d, 0x1b1b1a, 0xd2dce1, 0xb9b9b9, 0x5f5f5f,
                0x000000, 0xfefefe, 0xfefefe, 0x000000, 0x7f7f7f, 0x7f7f7f, 0x000000, 0x7f7f7f,
                0xffffff, 0x0c2431,
            ];
            let factory = usize::try_from(font)
                .ok()
                .and_then(|f| COLORS.get(f).map(|&c| (f, c)));
            styles.push(ui::TextStyle {
                font: ui::Font::Stock(font),
                size: factory
                    .filter(|(f, _)| matches!(f, 1 | 5 | 7 | 16 | 17 | 20))
                    .map(|_| 13.),
                color: factory.map_or(ui::Rgba::default(), |(_, c)| ui::Rgba::rgb(c)),
                align: match align {
                    0 => ui::Align::Left,
                    2 => ui::Align::Right,
                    _ => ui::Align::Center,
                },
            });
            ui::StyleRef(styles.len() - 1)
        })
    }

    fn unsupported(&mut self, widget: Option<usize>, feature: impl Into<String>, value: String) {
        self.ui.unsupported.push(ui::Unsupported {
            widget: widget.map(ui::WidgetRef),
            feature: feature.into(),
            value,
        });
    }
}

/// Library-relative path of a KSP picture name.
#[must_use]
pub fn picture_path(name: &str) -> String {
    format!("Resources/pictures/{name}.png")
}

/// KSP colours are `0xRRGGBB`, or `0xAARRGGBB` when the top byte is set.
fn color(v: i32) -> ui::Rgba {
    let v = v as u32;
    let a = (v >> 24) as u8;
    ui::Rgba {
        a: if a == 0 { 255 } else { a },
        ..ui::Rgba::rgb(v & 0xFF_FFFF)
    }
}

fn unit(n: i32) -> &'static str {
    // $KNOB_UNIT_* (constants.tsv).
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

/// Script `slot`'s performance view as `on init` left it. `picture` gives the
/// layout of a library-relative image path; `None` means one plain frame.
///
/// # Errors
/// When the script built a tree the model rejects (bad nesting).
pub fn interface<'a>(
    face: &'a Face,
    slot: u8,
    engine: &'a Engine,
    picture: &dyn Fn(&str) -> Option<ui::ImageMeta>,
) -> Result<ui::Interface, ui::Error> {
    let mut bld = Builder {
        ui: ui::Interface {
            source: ui::Source::Ksp { slot },
            ..Default::default()
        },
        assets: HashMap::new(),
        styles: HashMap::new(),
        picture,
    };
    if let Some(page) = engine.page(slot) {
        page_of(&mut bld, page, face.view.as_ref());
    }
    let declared =
        |control: &'a Control, name: &'a str, cells: Cells, view: Option<&'a ViewControl>| {
            let id = FIRST_UI_ID + i32::try_from(control.variable).ok()?;
            Some(Decl {
                kind: view.map_or(control.kind, |v| v.kind),
                params: view.map_or(&control.params, |v| &v.params),
                view,
                name,
                cells,
                id,
                look: engine.control_look(slot, id)?,
            })
        };
    let script: HashMap<&str, _> = face
        .controls
        .iter()
        .map(|(control, name, cells)| (&**name, (control, *cells)))
        .collect();
    // The view's controls in its order (the script compiles only those it
    // names), then the script's own.
    let viewed = face.view.iter().flat_map(|v| &v.controls);
    let mut decls: Vec<Decl<'_>> = viewed
        .clone()
        .enumerate()
        .filter_map(|(n, v)| match script.get(v.name.as_str()) {
            Some(&(control, cells)) => declared(control, &v.name, cells, Some(v)),
            None => Some(Decl {
                kind: v.kind,
                params: &v.params,
                view: Some(v),
                name: &v.name,
                cells: Cells::Const,
                // Not addressable by the script: an id no control has.
                id: -1 - i32::try_from(n).ok()?,
                look: ControlLook {
                    value: None,
                    pars: &[],
                    texts: &[],
                    menu: &[],
                },
            }),
        })
        .collect();
    let in_view: std::collections::HashSet<&str> = viewed.map(|v| v.name.as_str()).collect();
    decls.extend(
        face.controls
            .iter()
            .filter(|(_, name, _)| !in_view.contains(&**name))
            .filter_map(|(control, name, cells)| declared(control, name, *cells, None)),
    );
    let by_id: HashMap<i32, usize> = decls.iter().enumerate().map(|(i, d)| (d.id, i)).collect();
    let by_name: HashMap<&str, i32> = decls.iter().map(|d| (d.name, d.id)).collect();
    for (i, d) in decls.iter().enumerate() {
        let props = props(face, d);
        let mut w = widget(&mut bld, slot, d, &props, engine);
        let parent = props.int("$CONTROL_PAR_PARENT_PANEL").or_else(|| {
            let name = d.view?.parent.as_deref()?;
            by_name.get(name).copied()
        });
        if let Some(parent) = parent {
            match by_id.get(&parent) {
                Some(&p)
                    if p != i && decls.get(p).is_some_and(|d| d.kind == ControlKind::Panel) =>
                {
                    w.parent = Some(ui::WidgetRef(p));
                }
                _ => bld.unsupported(Some(i), "$CONTROL_PAR_PARENT_PANEL", parent.to_string()),
            }
        }
        for (&par, &value) in &props.ints {
            if !mapped(par) {
                bld.unsupported(Some(i), par, value.to_string());
            }
        }
        for (&par, &value) in &props.texts {
            if !mapped(par) {
                bld.unsupported(Some(i), par, value.into());
            }
        }
        bld.ui.widgets.push(w);
    }
    for skipped in face.view.iter().flat_map(|v| &v.skipped) {
        bld.unsupported(None, "performance view control", skipped.clone());
    }
    // Panels can be nested in any order; break a cycle rather than hand over
    // a malformed tree.
    if let Err(ui::Error::ParentCycle(at)) = bld.ui.validate()
        && let Some(w) = bld.ui.widgets.get_mut(at.0)
    {
        let parent = w.parent.take();
        bld.unsupported(
            Some(at.0),
            "$CONTROL_PAR_PARENT_PANEL",
            format!("cycle via {:?}", parent.map(|p| p.0)),
        );
    }
    bld.ui.validate()?;
    Ok(bld.ui)
}

/// The page as init left it, over the performance view's when it loads one.
fn page_of(bld: &mut Builder<'_>, page: &Page, view: Option<&View>) {
    bld.ui.performance = page.performance || view.is_some();
    let or_view = |set: &str, view: Option<&String>| {
        let set = (!set.is_empty()).then(|| set.to_owned());
        set.or_else(|| view.cloned()).unwrap_or_default()
    };
    let mut background = ui::Background {
        offset_y: page.skin_offset,
        origin_y: HEADER,
        color: page
            .color
            .or(view.and_then(|v| v.color))
            .map(|c| ui::Rgba::rgb(c as u32)),
        frame: u32::try_from(page.wallpaper_frame).unwrap_or(0),
        ..Default::default()
    };
    let wallpaper = or_view(
        page.wallpaper.as_str(),
        view.and_then(|v| v.wallpaper.as_ref()),
    );
    background.image = bld.asset(&wallpaper);
    let icon = or_view(page.icon.as_str(), view.and_then(|v| v.icon.as_ref()));
    bld.ui.icon = bld.asset(&icon);
    bld.ui.icon_hidden = page.icon_hide & HIDE_WHOLE_CONTROL != 0;
    let height_px = page.height_px.or(view
        .and_then(|v| v.height)
        .filter(|_| page.height_rows.is_none()));
    let height_rows = height_px
        .is_none()
        .then(|| page.height_rows.map(|r| r.max(1) as u32))
        .flatten();
    // ponytail: 100 px when the script sets no height; Kontakt's own default may differ.
    let height = match (height_px, height_rows) {
        (Some(px), _) => px.max(1) as u32,
        (None, Some(_)) => 0,
        (None, None) => 100,
    };
    let width = page
        .width_px
        .or(view.and_then(|v| v.width))
        .map_or(DEFAULT_WIDTH, |w| w.max(1) as u32);
    bld.ui.pages.push(ui::Page {
        name: page.title.as_str().into(),
        size: ui::Size {
            width: f64::from(width),
            height: f64::from(height),
        },
        background,
        height_rows,
    });
}

/// `$HIDE_*` (constants.tsv, measured in Kontakt).
const HIDE_PART_BG: i32 = 1;
const HIDE_PART_TITLE: i32 = 2;
const HIDE_PART_VALUE: i32 = 4;
const HIDE_WHOLE_CONTROL: i32 = 16;

/// The view's properties, then what the script set over them.
fn props<'a>(face: &'a Face, d: &'a Decl<'a>) -> Props<'a> {
    let view = d.view.into_iter();
    let ints = view
        .clone()
        .flat_map(|v| &v.ints)
        .map(|(p, v)| (p.as_str(), *v));
    let texts = view
        .flat_map(|v| &v.texts)
        .map(|(p, t)| (p.as_str(), t.as_str()));
    let look = &d.look;
    Props {
        ints: ints
            .chain(
                look.pars
                    .iter()
                    .filter_map(|&(par, v)| Some((face.par_name(par)?, v))),
            )
            .collect(),
        texts: texts
            .chain(
                look.texts
                    .iter()
                    .filter_map(|(par, t)| Some((face.par_name(*par)?, t.as_str()))),
            )
            .collect(),
    }
}

/// One declared control and its state after init.
struct Decl<'a> {
    kind: ControlKind,
    /// Declaration parameters (the view's, for a view control).
    params: &'a [i32],
    view: Option<&'a ViewControl>,
    name: &'a str,
    cells: Cells,
    id: i32,
    look: ControlLook<'a>,
}

fn widget(
    bld: &mut Builder<'_>,
    slot: u8,
    d: &Decl<'_>,
    p: &Props<'_>,
    engine: &Engine,
) -> ui::Widget {
    let Decl {
        kind: control_kind,
        params,
        view,
        name,
        cells,
        id,
        look,
    } = d;
    let (kind, cells, id, name) = (*control_kind, *cells, *id, *name);
    let param = |n: usize| params.get(n).copied();
    let range = |lo: i32, hi: i32| {
        let lo = p.int("$CONTROL_PAR_MIN_VALUE").unwrap_or(lo);
        let hi = p.int("$CONTROL_PAR_MAX_VALUE").unwrap_or(hi);
        ui::Range {
            min: f64::from(lo),
            max: f64::from(hi),
            default: f64::from(p.int("$CONTROL_PAR_DEFAULT_VALUE").unwrap_or(lo)),
            step: Some(1.0),
        }
    };
    let declared = || range(param(0).unwrap_or(0), param(1).unwrap_or(1_000_000));
    let display = || ui::Display {
        ratio: f64::from(param(2).filter(|&r| r != 0).unwrap_or(1)),
        unit: unit(p.int("$CONTROL_PAR_UNIT").unwrap_or(0)).into(),
    };
    let array = match cells {
        Cells::Int { base, len } | Cells::Real { base, len } if len > 1 => Some((base, len)),
        _ => None,
    };
    let vertical = p.int("$CONTROL_PAR_MOUSE_BEHAVIOUR").is_some_and(|m| m < 0);
    let ui_kind = match kind {
        ControlKind::Panel => ui::Kind::Panel,
        ControlKind::Knob => ui::Kind::Knob {
            range: declared(),
            display: display(),
        },
        ControlKind::Slider => ui::Kind::Slider {
            range: declared(),
            orientation: if vertical {
                ui::Orientation::Vertical
            } else {
                ui::Orientation::Horizontal
            },
        },
        ControlKind::Button => ui::Kind::Button { momentary: false },
        ControlKind::Switch => ui::Kind::Switch,
        ControlKind::Menu => ui::Kind::Menu {
            items: match (look.menu, view) {
                ([], Some(v)) => v
                    .menu
                    .iter()
                    .map(|(text, value, visible)| ui::MenuItem {
                        text: text.clone(),
                        value: *value,
                        visible: *visible,
                    })
                    .collect(),
                (menu, _) => menu
                    .iter()
                    .map(|m| ui::MenuItem {
                        text: m.text.as_str().into(),
                        value: m.value,
                        visible: m.visible,
                    })
                    .collect(),
            },
        },
        ControlKind::Label => ui::Kind::Label,
        ControlKind::ValueEdit => ui::Kind::ValueEdit {
            range: declared(),
            display: display(),
            arrows: p.int("$CONTROL_PAR_SHOW_ARROWS") != Some(0),
        },
        ControlKind::Table => {
            // declare ui_table %t[columns](width, height, range); negative is bipolar.
            let r = param(2).unwrap_or(100);
            let (base, len) = array.unwrap_or((0, 0));
            let cells = (base..base + len)
                .map(|c| engine.script_cell_in(slot, c as usize).unwrap_or(0) as f64)
                .collect();
            ui::Kind::Table {
                columns: len,
                range: ui::Range {
                    min: if r < 0 { f64::from(r) } else { 0.0 },
                    max: f64::from(r.unsigned_abs()),
                    default: 0.0,
                    step: Some(1.0),
                },
                bipolar: r < 0,
                cells,
                steps_shown: p
                    .int("table_steps_shown")
                    .and_then(|n| u32::try_from(n).ok()),
            }
        }
        ControlKind::Xy => ui::Kind::Xy {
            cursors: array.map_or(0, |(_, len)| len / 2),
            sensitivity: [
                "$CONTROL_PAR_MOUSE_BEHAVIOUR_X",
                "$CONTROL_PAR_MOUSE_BEHAVIOUR_Y",
            ]
            .map(|n| p.int(n).map(i32::unsigned_abs)),
            mouse_mode: p.int("$CONTROL_PAR_MOUSE_MODE"),
        },
        ControlKind::Waveform => ui::Kind::Waveform,
        ControlKind::Wavetable => ui::Kind::Wavetable {
            view_mode: p.int("$CONTROL_PAR_WT_VIS_MODE"),
            parallax: ["$CONTROL_PAR_PARALLAX_X", "$CONTROL_PAR_PARALLAX_Y"]
                .map(|n| p.int(n).unwrap_or(0)),
        },
        ControlKind::LevelMeter => ui::Kind::LevelMeter {
            orientation: if p.int("$CONTROL_PAR_VERTICAL") == Some(1) {
                ui::Orientation::Vertical
            } else {
                ui::Orientation::Horizontal
            },
        },
        ControlKind::FileSelector => ui::Kind::FileSelector {
            base_path: p.text("$CONTROL_PAR_BASEPATH").map(Into::into),
            // $NI_FILE_TYPE_MIDI, _AUDIO, _ARRAY.
            files: match p.int("$CONTROL_PAR_FILE_TYPE") {
                Some(0) => ui::Files::Midi,
                Some(1) => ui::Files::Audio,
                Some(2) => ui::Files::Data,
                _ => ui::Files::Any,
            },
            column_width: p
                .int("$CONTROL_PAR_COLUMN_WIDTH")
                .and_then(|n| u32::try_from(n).ok()),
        },
        ControlKind::TextEdit => ui::Kind::TextEdit,
        ControlKind::MouseArea => ui::Kind::MouseArea,
    };
    // Missing sizes stay 0: the renderer applies Kontakt's stock sizes.
    let (width, height) = (p.int("$CONTROL_PAR_WIDTH"), p.int("$CONTROL_PAR_HEIGHT"));
    let rect = ui::Rect::new(
        p.int("$CONTROL_PAR_POS_X").unwrap_or(0),
        p.int("$CONTROL_PAR_POS_Y").unwrap_or(0),
        width.map_or(0, |v| v.max(0) as u32),
        height.map_or(0, |v| v.max(0) as u32),
    );
    let mut w = ui::Widget::new(name, ui::PageRef(0), rect, ui_kind);
    w.auto_size = width.is_none() || height.is_none();
    w.default_axes = [width.is_none(), height.is_none()];
    w.source_id = (id >= 0).then_some(id);
    w.active_index = p.int("$CONTROL_PAR_ACTIVE_INDEX");
    w.value = match &w.kind {
        ui::Kind::Table { cells, .. } => Some(ui::Value::Integers(
            cells.iter().map(|n| n.round() as i32).collect(),
        )),
        _ => look.value.map(|v| ui::Value::Integer(v as i32)),
    };
    if matches!(kind, ControlKind::Waveform | ControlKind::Wavetable) {
        w.waveform = p.int("$CONTROL_PAR_WT_ZONE").map(|zone| ui::Waveform {
            zone,
            flags: 0,
            cursor_us: 0,
            table: vec![],
            highlighted: None,
            midi_start_note: 60,
        });
    }
    w.z = p.int("$CONTROL_PAR_Z_LAYER").unwrap_or(0);
    let hide = p.int("$CONTROL_PAR_HIDE").unwrap_or(0);
    w.hidden = hide & HIDE_WHOLE_CONTROL != 0;
    // move_control(x, y) places on Kontakt's grid; (0, 0) hides.
    if let (Some(column), Some(row)) = (p.int("grid_x"), p.int("grid_y")) {
        match (u32::try_from(column), u32::try_from(row)) {
            (Ok(column @ 1..), Ok(row @ 1..)) => {
                w.placement = ui::Placement::Grid { column, row };
            }
            _ => w.hidden = true,
        }
    }
    w.hide = ui::Parts {
        background: hide & HIDE_PART_BG != 0,
        value: hide & HIDE_PART_VALUE != 0,
        title: hide & HIDE_PART_TITLE != 0,
        unit: false,
    };
    w.text_y = p.int("$CONTROL_PAR_TEXTPOS_Y");
    w.text_shift_on_press = p.int("$CONTROL_PAR_DISABLE_TEXT_SHIFTING") != Some(1);
    w.value_y = p.int("$CONTROL_PAR_VALUEPOS_Y");
    w.value_text = p.text("$CONTROL_PAR_LABEL").map(Into::into);
    w.drag = p.int("$CONTROL_PAR_MOUSE_BEHAVIOUR").map(|m| ui::Drag {
        axis: if m < 0 {
            ui::Orientation::Vertical
        } else {
            ui::Orientation::Horizontal
        },
        sensitivity: m.unsigned_abs(),
    });
    for (par, slot) in COLORS {
        if let Some(c) = p.int(par) {
            *slot(&mut w.colors) = Some(color(c));
        }
    }
    w.binding = match kind {
        ControlKind::Label | ControlKind::Panel | ControlKind::LevelMeter => ui::Binding::None,
        _ => ui::Binding::Variable {
            script: slot,
            name: name.into(),
        },
    };
    // Kontakt captions a control with its variable name until TEXT is set.
    w.text = p.text("$CONTROL_PAR_TEXT").map_or_else(
        || match kind {
            ControlKind::Knob
            | ControlKind::Slider
            | ControlKind::Button
            | ControlKind::Switch
            | ControlKind::ValueEdit => name.get(1..).unwrap_or_default().into(),
            _ => String::new(),
        },
        Into::into,
    );
    w.tooltip = p.text("$CONTROL_PAR_HELP").unwrap_or_default().into();
    w.automation = ui::Automation {
        name: p.text("$CONTROL_PAR_AUTOMATION_NAME").map(Into::into),
        short_name: p.text("$CONTROL_PAR_SHORT_NAME").map(Into::into),
        allowed: p.int("$CONTROL_PAR_ALLOW_AUTOMATION") != Some(0),
        id: p
            .int("$CONTROL_PAR_AUTOMATION_ID")
            .and_then(|n| u32::try_from(n).ok()),
    };
    let font = p.int("$CONTROL_PAR_FONT_TYPE");
    let align = p.int("$CONTROL_PAR_TEXT_ALIGNMENT");
    if font.is_some() || align.is_some() {
        w.style = Some(bld.style(font.unwrap_or(0), align.unwrap_or(1)));
    }
    for (style, par) in w.state_styles.iter_mut().zip(STATE_FONTS) {
        *style = p
            .int(par)
            .filter(|&font| font >= 0)
            .map(|font| bld.style(font, align.unwrap_or(1)));
    }
    if let Some(asset) = p.text("$CONTROL_PAR_PICTURE").and_then(|n| bld.asset(n)) {
        let role = match kind {
            ControlKind::Knob
            | ControlKind::Slider
            | ControlKind::Button
            | ControlKind::Switch
            | ControlKind::Menu
            | ControlKind::ValueEdit
            | ControlKind::LevelMeter => ui::Role::Strip,
            _ => ui::Role::Background,
        };
        let mut image = ui::ImageUse::new(asset, role);
        image.frame = p
            .int("$CONTROL_PAR_PICTURE_STATE")
            .and_then(|f| u32::try_from(f).ok());
        w.images.push(image);
    }
    w
}
