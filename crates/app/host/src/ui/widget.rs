//! One widget from one declared control: the KSP control parameters it
//! carries, by name (`Props`), onto the tree's widget.

use super::*;

/// One declared control and its state after init.
pub(super) struct Decl<'a> {
    pub(super) kind: ControlKind,
    /// Declaration parameters (the view's, for a view control).
    pub(super) params: &'a [i32],
    pub(super) view: Option<&'a ViewControl>,
    pub(super) name: &'a str,
    pub(super) cells: Cells,
    pub(super) id: i32,
    pub(super) look: ControlLook<'a>,
}

/// Points a knob's drag travels up to sweep its range (Kontakt 8, measured;
/// scratch/laneJ/drag/FACTS.md).
const KNOB_SWEEP: f64 = 350.;
/// Points a slider without `MOUSE_BEHAVIOUR` travels to sweep its range (same).
const SLIDER_SWEEP: f64 = 172.;
/// The `MOUSE_BEHAVIOUR` that sweeps a slider in its picture's length (manual).
const BEHAVIOUR_PER_LENGTH: f64 = 1000.;
/// Points a value edit's drag travels per unit (same measurement).
const VALUE_EDIT_PER_UNIT: f64 = 4.;

pub(super) fn widget(
    bld: &mut Builder<'_>,
    slot: u8,
    d: &Decl<'_>,
    p: &Props<'_>,
    engine: &dyn Looks,
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
            // Ratio 0 ($VALUE_EDIT_MODE_NOTE_NAMES) shows note names.
            display: ui::Display {
                ratio: f64::from(param(2).unwrap_or(1)),
                ..display()
            },
            // Off until the script sets it (Kontakt 8.13, measured).
            arrows: p.int("$CONTROL_PAR_SHOW_ARROWS").is_some_and(|on| on != 0),
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
            channel: p.int("level_meter").and_then(|c| u8::try_from(c).ok()),
            // attach_level_meter(id, group, slot, channel, generic): with no
            // group, a generic of 0..15 is that instrument bus.
            bus: (p.int("level_meter_group") == Some(-1))
                .then(|| p.int("level_meter_generic"))
                .flatten()
                .and_then(|b| u8::try_from(b).ok())
                .filter(|&b| usize::from(b) < kontra_protocol::BUSES),
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
    // A switch fires on mouse down, a button on mouse up (KSP manual, ui_switch).
    w.activate_on_press = matches!(kind, ControlKind::Switch);
    w.value_y = p.int("$CONTROL_PAR_VALUEPOS_Y");
    w.value_text = p.text("$CONTROL_PAR_LABEL").map(Into::into);
    // Kontakt 8's drags, measured (scratch/laneJ/drag/FACTS.md): a knob
    // sweeps in 350 points up, ignoring MOUSE_BEHAVIOUR; a slider in 172,
    // or 1000 / |MOUSE_BEHAVIOUR| of its picture along the sign's axis; a
    // value edit 4 points a unit, up. Each runs its callback again on release.
    // ponytail: 172 measured on the stock skin only; a picture may scale it.
    let behaviour = p.int("$CONTROL_PAR_MOUSE_BEHAVIOUR").filter(|&m| m != 0);
    let sweep = match kind {
        ControlKind::Knob => Some(ui::Sweep::Points(KNOB_SWEEP)),
        ControlKind::Slider => Some(behaviour.map_or(ui::Sweep::Points(SLIDER_SWEEP), |m| {
            ui::Sweep::Lengths(BEHAVIOUR_PER_LENGTH / f64::from(m.unsigned_abs()))
        })),
        ControlKind::ValueEdit => Some(ui::Sweep::PerUnit(VALUE_EDIT_PER_UNIT)),
        _ => None,
    };
    w.drag = sweep.map(|sweep| ui::Drag {
        axis: if vertical || kind != ControlKind::Slider {
            ui::Orientation::Vertical
        } else {
            ui::Orientation::Horizontal
        },
        sweep,
    });
    w.reports_release = true;
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
    // Kontakt sets a label's words from the left and centres the rest
    // (KSP manual: "Set and center text in labels").
    let align = p.int("$CONTROL_PAR_TEXT_ALIGNMENT");
    let set = align.is_some();
    let align = align.unwrap_or(i32::from(kind != ControlKind::Label));
    if font.is_some() || set {
        let font = font.unwrap_or(0);
        w.style = Some(bld.style(font, align, engine.font(slot, font)));
    }
    for (style, par) in w.state_styles.iter_mut().zip(STATE_FONTS) {
        *style = p
            .int(par)
            .filter(|&font| font >= 0)
            .map(|font| bld.style(font, align, engine.font(slot, font)));
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
        // KSP: PICTURE_STATE picks the frame of a label, table, value edit
        // or XY pad only. Any other control's value picks its frame; read for
        // all, an unset 0 pinned every knob and switch to its first frame.
        let fixed = matches!(
            kind,
            ControlKind::Label | ControlKind::Table | ControlKind::ValueEdit | ControlKind::Xy
        );
        image.frame = p
            .int("$CONTROL_PAR_PICTURE_STATE")
            .filter(|_| fixed)
            .and_then(|f| u32::try_from(f).ok());
        w.images.push(image);
    }
    w
}
