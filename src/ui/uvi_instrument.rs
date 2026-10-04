//! Native controls over a bank's authored UVI layout. Snapshots and decoded
//! artwork are adopted off the audio thread; this module never opens resources.
//! Unit readouts follow documented conversions; mappers and complex displays
//! are not emulated here. Typed numeric edits still use the raw value domain.
use super::theme::*;
use crate::artwork::Picture;
use crate::uvi::{
    host::{UiEdit, UiEditValue, UiKind, UiModifiers, UiSnapshot, UiValue, UiWidget},
    ui_assets::{key, strip_key},
    worker::Stamp,
};
use moose::mui::mui::{prelude::*, scene::Fit};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Default)]
pub struct State {
    activation: Option<(u64, u64, usize)>,
    pending: Option<(u32, UiEditValue, u64)>,
    menu: Option<u32>,
    menu_path: Vec<String>,
    table_cell: Option<(u32, usize)>,
    read_only: bool,
}

fn same_activation(a: Stamp, b: Stamp) -> bool {
    (a.epoch, a.generation) == (b.epoch, b.generation)
}

impl State {
    pub fn set_interactive(&mut self, interactive: bool) {
        self.read_only = !interactive;
        if self.read_only {
            self.pending = None;
            self.menu = None;
            self.menu_path.clear();
            self.table_cell = None;
        }
    }

    fn adopt(&mut self, current: Stamp, captured: Stamp, processor: usize) -> bool {
        let activation = Some((current.epoch, current.generation, processor));
        if self.activation != activation {
            *self = Self {
                activation,
                read_only: self.read_only,
                ..Self::default()
            };
        }
        if !same_activation(current, captured) || captured.frame > current.frame {
            self.pending = None;
            self.menu = None;
            self.menu_path.clear();
            return false;
        }
        if self
            .pending
            .is_some_and(|(_, _, frame)| captured.frame > frame)
        {
            self.pending = None;
        }
        true
    }
}

fn identity(owner: usize, stamp: Stamp, snapshot: &UiSnapshot, widget: u32) -> String {
    format!(
        "uvi-{owner}-{}-{}-{}-{widget}",
        stamp.epoch, stamp.generation, snapshot.processor
    )
}

fn enabled(snapshot: &UiSnapshot, widget: &UiWidget) -> bool {
    let mut at = Some(widget);
    for _ in 0..=snapshot.widgets.len() {
        let Some(w) = at else { return true };
        if !w.enabled {
            return false;
        }
        at = match w.parent {
            None => None,
            Some(id) => match id
                .checked_sub(1)
                .and_then(|i| snapshot.widgets.get(i as usize))
            {
                Some(parent) => Some(parent),
                None => return false,
            },
        };
    }
    false
}

fn name(widget: &UiWidget) -> &str {
    widget
        .display_name
        .as_deref()
        .filter(|label| !label.is_empty())
        .unwrap_or(&widget.name)
}

// Native Button/OnOffButton labels are off until the script enables them;
// supplying custom artwork does not change that default.
fn button_text(widget: &UiWidget) -> &str {
    if widget.style.show_label == Some(true) {
        widget
            .style
            .text
            .as_deref()
            .or(widget.display_name.as_deref())
            .unwrap_or("")
    } else {
        ""
    }
}

// Theme controls have intrinsic text/padding too. Scale those with the
// authored canvas instead of leaving fixed-size text inside resized fields.
fn scale_control(el: &mut El, ui: &Ui, scale: f64) {
    let font = el.payload().text_px(ui.theme());
    el.payload_mut().text_size = Some(font * scale);
    let pad = el.padding(ui.theme().spacing);
    *el = el.clone().pad(edges(
        pad.top * scale,
        pad.right * scale,
        pad.bottom * scale,
        pad.left * scale,
    ));
    let gap = el.gap_mut();
    *gap = (gap.resolve(ui.theme().spacing) * scale).into();
    for child in el.children_mut() {
        scale_control(child, ui, scale);
    }
}

fn widget_font(el: &mut El, widget: &UiWidget, fonts: &HashMap<String, Font>) {
    let Some(font) = widget.style.font.as_ref().and_then(|path| fonts.get(path)) else { return };
    fn apply(el: &mut El, font: &Font) {
        el.payload_mut().font = Some(font.clone());
        for child in el.children_mut() { apply(child, font); }
    }
    apply(el, font);
}

fn range(widget: &UiWidget) -> Option<std::ops::RangeInclusive<f64>> {
    let (lo, hi) = widget.min.zip(widget.max)?;
    (lo.is_finite() && hi.is_finite() && lo <= hi).then_some(lo..=hi)
}

fn number_text(value: f64, integer: bool) -> String {
    if integer {
        format!("{value:.0}")
    } else {
        format!("{value:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}

// Native enum IDs are installed by the host shim. Unit changes only the
// readout, never the engine value, range, sprite position, or edit payload.
fn unit_text(value: f64, integer: bool, unit: Option<f64>) -> String {
    let (value, suffix) = match unit {
        Some(1.) => (value, "%"),
        Some(2.) => (value * 100., "%"),
        Some(3.) if value.abs() < 1. => (value * 1000., "ms"),
        Some(3.) => (value, "s"),
        Some(5.) if value.abs() > 1000. => (value / 1000., "s"),
        Some(5.) => (value, "ms"),
        Some(7.) if value.abs() > 1000. => (value / 1000., "kHz"),
        Some(7.) => (value, "Hz"),
        Some(9.) => (value, "dB"),
        Some(11.) if value <= 0. => return "-inf dB".into(),
        Some(11.) => (20. * value.log10(), "dB"),
        Some(14.) => (value, "st"),
        // Pan's native text law and UVI filter formatting need calibration.
        _ => return number_text(value, integer),
    };
    format!("{} {suffix}", number_text(value + 0., false))
}

fn value_text(widget: &UiWidget, value: f64) -> String {
    widget.style.display_text.as_ref().filter(|s| !s.is_empty()).cloned()
        .unwrap_or_else(|| unit_text(value, widget.integer, widget.style.unit))
}


fn colour(text: Option<&str>) -> Option<Color> {
    let text = text?.trim();
    match text.to_ascii_lowercase().as_str() {
        "white" => return Some(Color::srgb(1., 1., 1.)),
        "black" => return Some(Color::srgb(0., 0., 0.)),
        "transparent" => return Some(Color::srgba(0., 0., 0., 0.)),
        _ => {}
    }
    let text = text.strip_prefix('#').unwrap_or(text);
    let value = u32::from_str_radix(text, 16).ok()?;
    let alpha = match text.len() {
        6 => 1.,
        8 => (value >> 24) as f32 / 255.,
        _ => return None,
    };
    Some(Color::srgba(
        ((value >> 16) & 255) as f32 / 255.,
        ((value >> 8) & 255) as f32 / 255.,
        (value & 255) as f32 / 255.,
        alpha,
    ))
}

// UVI text boxes use nine authored anchor positions. Keep the inherited
// face when a bank font is unavailable, but honor the supplied size and ink.
fn styled_text(widget: &UiWidget, said: &str, scale: f64, fallback: Justify) -> El {
    let (justify, align) = match widget.style.align.as_deref() {
        Some("centred" | "centre" | "center") => (Justify::Center, Align::Center),
        Some("left") => (Justify::Start, Align::Center),
        Some("right") => (Justify::End, Align::Center),
        Some("top") => (Justify::Center, Align::Start),
        Some("bottom") => (Justify::Center, Align::End),
        Some("topLeft") => (Justify::Start, Align::Start),
        Some("topRight") => (Justify::End, Align::Start),
        Some("bottomLeft") => (Justify::Start, Align::End),
        Some("bottomRight") => (Justify::End, Align::End),
        _ => (fallback, Align::Center),
    };
    row![text(said.to_owned())
        .text_size(widget.style.font_size.unwrap_or(SMALL).max(1.) * scale)
        .fill(colour(widget.style.text_colour.as_deref())
            .map_or(Fill::from(Role::Ink), Fill::from))
        .lines(if widget.kind == UiKind::Label {
            (widget.absolute_bounds.height / (widget.style.font_size.unwrap_or(SMALL).max(1.) * 1.4))
                .floor().max(1.) as usize
        } else { 1 })
        .min_w(0)]
        .justify(justify)
        .align(align)
        .min_w(0)
}


// Keep MUI's text editor and caret intact; the resting numeric readout uses
// the same authored typography as labels and menus, over its bank artwork.
fn numbox_readout(
    field: &mut El,
    widget: &UiWidget,
    value: f64,
    pictures: &HashMap<String, Arc<Picture>>,
    scale: f64,
) {
    let style = &widget.style;
    if style.font_size.is_none() && style.align.is_none() && style.text_colour.is_none()
        && style.background_image.is_none() && style.background_colour.is_none() {
        return;
    }
    if field.payload().semantics.as_ref()
        .is_some_and(|s| matches!(s.role, A11y::TextInput { .. })) {
        return;
    }
    if let Some(child) = field.children_mut().first_mut() {
        let mut readout = styled_text(widget, &value_text(widget, value), scale, Justify::Start)
            .flex(1).w(Len::Pct(100.)).h(Len::Pct(100.));
        if matches!(widget.style.align.as_deref(),
            None | Some("centred" | "centre" | "center" | "left" | "right")) {
            // Font leading can exceed the authored box; center the ink in
            // that box rather than allowing the text's minimum height to grow it.
            readout.children_mut()[0] = readout.children_mut()[0].clone()
                .min_h(0).h(widget.absolute_bounds.height * scale);
        }
        *child = readout;
    }
    *field = field.clone().pad(0).radius(0.).align(Align::Stretch);
    let image = widget.style.background_image.as_ref()
        .and_then(|art| pictures.get(&key(art)))
        .and_then(|picture| picture.frames.first()).cloned();
    if let Some(image) = image {
        *field = field.clone().fill(Fill::Image(image, Fit::Contain));
    } else if let Some(colour) = colour(widget.style.background_colour.as_deref()) {
        *field = field.clone().fill(colour);
    }
}


// Decorate existing input surfaces only: help must not turn background images
// or labels into new hit targets over the bank's controls.
fn widget_tooltip(el: &mut El, widget: &UiWidget) {
    let Some(help) = widget
        .style
        .tooltip
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    else {
        return;
    };
    fn apply(el: &mut El, widget: &UiWidget, help: &str) {
        if el.payload().has(moose::mui::mui::scene::Element::FOCUSABLE) {
            let extra = el.payload().extras().tip.as_deref();
            let said = match extra {
                Some(extra)
                    if help == extra
                        || help == widget.name
                            && extra.starts_with(&format!("{}:", name(widget))) =>
                {
                    extra.to_owned()
                }
                Some(extra) => format!("{help} — {extra}"),
                None => help.to_owned(),
            };
            *el = el.clone().tip(said);
        }
        for child in el.children_mut() {
            apply(child, widget, help);
        }
    }
    apply(el, widget, help);
}

fn shown_value(state: &State, widget: &UiWidget) -> Option<UiEditValue> {
    state
        .pending
        .filter(|(id, ..)| *id == widget.id)
        .map(|(_, value, _)| value)
}

fn numeric(state: &State, widget: &UiWidget) -> f64 {
    if let Some(UiEditValue::Number(value)) = shown_value(state, widget) {
        return value;
    }
    match widget.value {
        Some(UiValue::Number(value)) => value,
        _ => 0.,
    }
}

fn send_edit(
    state: &mut State,
    stamp: Stamp,
    snapshot: &UiSnapshot,
    widget: &UiWidget,
    value: UiEditValue,
    mods: Mods,
    send: &mut impl FnMut(Stamp, UiEdit) -> bool,
) {
    let edit = UiEdit {
        processor: snapshot.processor,
        widget: widget.id,
        value,
        modifiers: UiModifiers {
            alt_down: mods.alt,
            command_down: mods.cmd,
            shift_down: mods.shift,
        },
    };
    if send(stamp, edit) {
        state.pending = Some((widget.id, value, stamp.frame));
    }
}

fn picture(
    widget: &UiWidget,
    pictures: &HashMap<String, Arc<Picture>>,
    value: f64,
    pressed: bool,
    hovered: bool,
) -> Option<Arc<moose::mui::mui::scene::Image>> {
    if let Some(strip) = &widget.style.strip_image {
        let image = pictures.get(&strip_key(strip))?;
        let t = range(widget).map_or(0., |r| {
            if r.start() == r.end() {
                0.
            } else {
                ((value - r.start()) / (r.end() - r.start())).clamp(0., 1.)
            }
        });
        let at = (t * image.frames.len().saturating_sub(1) as f64).round() as usize;
        return image.frames.get(at).cloned();
    }
    let style = &widget.style;
    let artwork = if pressed {
        style.pressed_image.as_ref().or(style.normal_image.as_ref())
    } else if hovered {
        style.over_image.as_ref().or(style.normal_image.as_ref())
    } else {
        style
            .image
            .as_ref()
            .or(style.normal_image.as_ref())
            .or(style.background_image.as_ref())
    }?;
    pictures.get(&key(artwork))?.frames.first().cloned()
}

/// Draw one validated processor snapshot at its authored coordinates. Captured
/// and current activation identities must agree before any gesture is admitted.
/// `send` queues the stamped edit; false leaves the published value intact.
pub fn view(
    ui: &mut Ui,
    state: &mut State,
    owner: usize,
    current: Stamp,
    captured: Stamp,
    snapshot: &UiSnapshot,
    pictures: &HashMap<String, Arc<Picture>>,
    fonts: &HashMap<String, Font>,
    mut send: impl FnMut(Stamp, UiEdit) -> bool,
) -> El {
    let admitted = state.adopt(current, captured, snapshot.processor);
    let initializing = current.epoch == captured.epoch && captured.generation > current.generation;
    if (!admitted && !initializing) || snapshot.widgets.len() > 4096 {
        return caption("Loading instrument controls…")
            .fill(secondary())
            .pad(INSET);
    }
    let width = snapshot.root.width;
    let height = snapshot.root.height;
    if width <= 0.
        || height <= 0.
        || !width.is_finite()
        || !height.is_finite()
        || width > 16_384.
        || height > 16_384.
    {
        return caption("This instrument has no performance controls.")
            .fill(secondary())
            .pad(INSET);
    }
    let room = ui
        .scene()
        .and_then(|s| s.surface(&format!("part-{owner}")))
        .map_or(width, |s| s.frame.size.width);
    let scale = if room.is_finite() && room > 0. {
        room / width
    } else {
        1.
    };
    let mut layers = Vec::with_capacity(snapshot.widgets.len() + 1);
    if let Some(image) = snapshot
        .root
        .background
        .as_ref()
        .and_then(|a| pictures.get(&key(a)))
        .and_then(|p| p.frames.first())
    {
        layers.push(
            block(width * scale, height * scale)
                .fill(Fill::Image(image.clone(), Fit::Contain))
                .at(0., 0.),
        );
    }
    for widget in &snapshot.widgets {
        if !widget.effective_visible
            || widget.effective_alpha == 0.
            || widget.absolute_bounds.width <= 0.
            || widget.absolute_bounds.height <= 0.
        {
            continue;
        }
        let id = identity(owner, current, snapshot, widget.id);
        let usable = admitted && !state.read_only && enabled(snapshot, widget);
        let bounds = widget.absolute_bounds;
        let (w, h) = (bounds.width * scale, bounds.height * scale);
        let response = ui.get(id.as_str());
        let modifiers = ui
            .keys(id.as_str())
            .last()
            .map_or(response.mods, |key| key.mods);
        let label = name(widget);
        let display_name = widget.display_name.as_deref().unwrap_or(label);
        let mut value = numeric(state, widget);
        let mut control = match widget.kind {
            UiKind::Knob | UiKind::Slider | UiKind::NumBox => {
                let Some(range) = range(widget) else { continue };
                let before = value;
                let vertical = widget.kind != UiKind::Slider || h > w;
                if widget.kind == UiKind::NumBox {
                    let field =
                        drag_value(ui, id.as_str(), label, &mut value, range.clone()).size(S);
                    if !usable {
                        value = before;
                    }
                    if widget.integer {
                        value = value.round();
                    }
                    if usable && before != value {
                        send_edit(
                            state,
                            current,
                            snapshot,
                            widget,
                            UiEditValue::Number(value),
                            modifiers,
                            &mut send,
                        );
                    }
                    let mut field = field.el.value_text(value_text(widget, value)).el()
                        .tip(format!("Enter a number from {} to {}.",
                            number_text(*range.start(), widget.integer),
                            number_text(*range.end(), widget.integer)));
                    scale_control(&mut field, ui, scale);
                    numbox_readout(&mut field, widget, value, pictures, scale);
                    widget_tooltip(&mut field, widget);
                    let mut content = if widget.style.show_label != Some(false) {
                        row![
                            caption(display_name).text_size(SMALL * scale).lines(1),
                            field.flex(1).min_w(0)
                        ]
                        .gap(TIGHT * scale)
                        .align(Align::Center)
                    } else {
                        field
                    };
                    widget_font(&mut content, widget, fonts);
                    layers.push(
                        content
                            .size(w, h)
                            .opacity(widget.effective_alpha as f32)
                            .when(!usable, |e| e.disabled())
                            .at(bounds.x * scale, bounds.y * scale),
                    );
                    continue;
                }
                if usable && range.start() != range.end() {
                    drive(
                        ui,
                        &id,
                        &mut value,
                        &range,
                        if widget.kind == UiKind::Knob {
                            TRAVEL
                        } else if vertical {
                            h
                        } else {
                            w
                        },
                        vertical,
                        before,
                    );
                    value = value.clamp(*range.start(), *range.end());
                    if widget.integer {
                        value = value.round().clamp(*range.start(), *range.end());
                    }
                    if before != value {
                        send_edit(
                            state,
                            current,
                            snapshot,
                            widget,
                            UiEditValue::Number(value),
                            modifiers,
                            &mut send,
                        );
                    }
                }
                let unit = if range.start() == range.end() {
                    0.
                } else {
                    (value - range.start()) / (range.end() - range.start())
                };
                let image = picture(widget, pictures, value, response.held, response.hovered);
                let skinned = image.is_some();
                let face = if let Some(image) = image {
                    block(w, h).fill(Fill::Image(image, Fit::Contain))
                } else if widget.kind == UiKind::Knob {
                    dial_face(
                        unit,
                        0.,
                        ui.state(id.as_str()).hover as f32,
                        ui.focus_visible(id.as_str()),
                    )
                } else {
                    fader_face(
                        unit,
                        0.,
                        None,
                        vertical,
                        ui.state(id.as_str()).hover as f32,
                        ui.focus_visible(id.as_str()),
                    )
                };
                let captions = usize::from(widget.style.show_label != Some(false))
                    + usize::from(widget.style.show_value != Some(false));
                let mut content = if skinned {
                    vec![spacer()]
                } else {
                    vec![
                        face.clone()
                            .size(w, (h - captions as f64 * SMALL * scale * 1.4).max(1.))
                            .shrink(0),
                    ]
                };
                if widget.style.show_label != Some(false) {
                    content.push(caption(display_name).text_size(SMALL * scale).lines(1));
                }
                if widget.style.show_value != Some(false) {
                    content.push(
                        caption(value_text(widget, value))
                            .text_size(SMALL * scale)
                            .lines(1),
                    );
                }
                let captions = col(content).gap(0).align(Align::Center);
                // Skin coordinates describe the complete source frame. Captions
                // overlay that frame; reserving their height squeezes its art.
                let content = if skinned {
                    stack![face.size(w, h), captions.size(w, h)]
                } else {
                    captions
                };
                content
                    .focusable()
                    .a11y(A11y::Slider {
                        value,
                        min: *range.start(),
                        max: *range.end(),
                    })
                    .tip(format!(
                        "{label}: {}–{}",
                        unit_text(*range.start(), widget.integer, widget.style.unit),
                        unit_text(*range.end(), widget.integer, widget.style.unit)
                    ))
                    .named(label.to_owned())
                    .id(id.clone())
            }
            UiKind::Button | UiKind::OnOffButton => {
                let on = match shown_value(state, widget) {
                    Some(UiEditValue::Boolean(on)) => on,
                    _ => matches!(widget.value, Some(UiValue::Boolean(true))),
                };
                let (hit, mut el) = if widget.kind == UiKind::Button {
                    action(ui, id.as_str(), button_text(widget), false)
                } else {
                    latch(ui, id.as_str(), button_text(widget), label, on)
                };
                scale_control(&mut el, ui, scale);
                if usable && hit {
                    send_edit(
                        state,
                        current,
                        snapshot,
                        widget,
                        if widget.kind == UiKind::Button {
                            UiEditValue::Push
                        } else {
                            UiEditValue::Boolean(!on)
                        },
                        modifiers,
                        &mut send,
                    );
                }
                value = f64::from(on);
                el.named(label.to_owned())
            }
            UiKind::Menu => {
                let selected = (value as usize)
                    .checked_sub(1)
                    .and_then(|i| widget.items.get(i))
                    .map_or("Choose…", String::as_str);
                let (hit, mut el) = dropdown(ui, id.as_str(), selected, label);
                scale_control(&mut el, ui, scale);
                if let Some(selected_text) = el.children_mut().first_mut() {
                    *selected_text = styled_text(widget, selected, scale, Justify::Start).flex(1);
                }
                if let Some(caret) = el.children_mut().get_mut(1) {
                    *caret = glyph(Icon::Down, TEXT * scale, secondary());
                }
                // Authored bounds already include the menu's text area. Generic
                // field padding/minimums squeeze narrow CC selections at bank size.
                let el = el.min_w(0).pad(0);
                if usable && hit {
                    state.menu_path.clear();
                    state.menu = if state.menu == Some(widget.id) {
                        None
                    } else {
                        Some(widget.id)
                    };
                }
                if widget.style.show_label != Some(false) {
                    row![
                        caption(display_name).text_size(SMALL * scale).lines(1),
                        el.flex(1).min_w(0)
                    ]
                    .gap(TIGHT * scale)
                    .align(Align::Center)
                } else {
                    el
                }
            }
            UiKind::Table => {
                let Some(UiValue::Table(values)) = &widget.value else {
                    continue;
                };
                if values.len() > 65_536 {
                    continue;
                }
                let Some(range) = range(widget) else { continue };
                if usable
                    && (response.pressed || response.held)
                    && !values.is_empty()
                    && let Some(at) = ui.local(id.as_str())
                {
                    let index = ((at.x / w * values.len() as f64) as usize).min(values.len() - 1);
                    let mut value = (range.start()
                        + (1. - at.y / h).clamp(0., 1.) * (range.end() - range.start()))
                    .clamp(*range.start(), *range.end());
                    if widget.integer {
                        value = value.round().clamp(*range.start(), *range.end());
                    }
                    if values[index] != value {
                        send_edit(
                            state,
                            current,
                            snapshot,
                            widget,
                            UiEditValue::TableCell {
                                index: index as u32 + 1,
                                value,
                            },
                            modifiers,
                            &mut send,
                        );
                    }
                }
                let mut selected = state
                    .table_cell
                    .filter(|(id, _)| *id == widget.id)
                    .map_or(0, |(_, index)| index)
                    .min(values.len().saturating_sub(1));
                if usable && !values.is_empty() {
                    if response.pressed
                        && let Some(at) = ui.local(id.as_str())
                    {
                        selected =
                            ((at.x / w * values.len() as f64) as usize).min(values.len() - 1);
                    }
                    for press in ui.keys(id.as_str()) {
                        let step = if widget.integer {
                            1.
                        } else {
                            (range.end() - range.start()) / 100.
                        };
                        match press.key {
                            Key::Left => selected = selected.saturating_sub(1),
                            Key::Right => selected = (selected + 1).min(values.len() - 1),
                            Key::Up | Key::Down => {
                                let direction = if press.key == Key::Up { 1. } else { -1. };
                                let value = (values[selected] + direction * step)
                                    .clamp(*range.start(), *range.end());
                                send_edit(
                                    state,
                                    current,
                                    snapshot,
                                    widget,
                                    UiEditValue::TableCell {
                                        index: selected as u32 + 1,
                                        value,
                                    },
                                    press.mods,
                                    &mut send,
                                );
                            }
                            _ => {}
                        }
                    }
                    if response.held || ui.focused(id.as_str()) {
                        state.table_cell = Some((widget.id, selected));
                    }
                }
                let mut values = values.clone();
                if let Some(UiEditValue::TableCell { index, value }) = shown_value(state, widget)
                    && let Some(cell) = values.get_mut(index.saturating_sub(1) as usize)
                {
                    *cell = value;
                }
                let (lo, span) = (
                    *range.start(),
                    (range.end() - range.start()).max(f64::EPSILON),
                );
                canvas(move |size| {
                    let columns = (size.width.ceil() as usize)
                        .clamp(1, 2048)
                        .min(values.len());
                    (0..columns)
                        .map(|column| {
                            let start = column * values.len() / columns;
                            let end = ((column + 1) * values.len() / columns).max(start + 1);
                            let v = values[start..end].iter().copied().fold(lo, f64::max);
                            let high = ((v - lo) / span).clamp(0., 1.) * size.height;
                            Draw::fill(
                                rect(
                                    column as f64 * size.width / columns as f64,
                                    size.height - high,
                                    size.width / columns as f64,
                                    high,
                                ),
                                value_ink(0.),
                            )
                        })
                        .collect()
                })
                .fill(Role::Field)
                .focusable()
                .named(format!(
                    "{label}: Left/Right selects a cell; Up/Down edits its value"
                ))
                .id(id.clone())
            }
            UiKind::Label => styled_text(
                widget,
                widget.style.text.as_deref().unwrap_or(label),
                scale,
                Justify::Start,
            ).named(label.to_owned()),
            UiKind::Panel | UiKind::Viewport | UiKind::Image => block(w, h),
            UiKind::XY | UiKind::WaveView | UiKind::AudioMeter => caption(label)
                .text_size(SMALL * scale)
                .fill(secondary())
                .tip("This display is unavailable.")
                .lines(1),
        };
        if !matches!(widget.kind, UiKind::Knob | UiKind::Slider)
            && let Some(image) = picture(
                widget,
                pictures,
                value,
                response.held || widget.kind == UiKind::OnOffButton && value != 0.,
                response.hovered,
            )
        {
            let image = block(w, h).fill(Fill::Image(image, Fit::Contain));
            control = if matches!(
                widget.kind,
                UiKind::Panel | UiKind::Viewport | UiKind::Image
            ) {
                image
            } else if matches!(widget.kind, UiKind::Button | UiKind::OnOffButton) {
                let words = button_text(widget).to_owned();
                stack![
                    image,
                    styled_text(widget, &words, scale, Justify::Center).size(w, h)
                ]
                .focusable()
                .a11y(if widget.kind == UiKind::Button {
                    A11y::Button
                } else {
                    A11y::Toggle { on: value != 0. }
                })
                .named(label.to_owned())
                .id(id.clone())
                .on(moose::mui::mui::prelude::State::FocusVisible, |e| {
                    e.stroke(accent()).stroke_width(1)
                })
            } else {
                stack![image, control.size(w, h)]
            };
        } else if let Some(fill) = colour(widget.style.background_colour.as_deref()) {
            control = control.fill(fill);
        }
        widget_tooltip(&mut control, widget);
        widget_font(&mut control, widget, fonts);
        layers.push(
            control
                .size(w, h)
                .opacity(widget.effective_alpha as f32)
                .when(!usable, |e| e.disabled())
                .at(bounds.x * scale, bounds.y * scale),
        );
    }
    stack(layers)
        .size(width * scale, height * scale)
        .shrink(0)
        .clip()
        .fill(
            colour(snapshot.root.background_colour.as_deref())
                .map_or(Fill::from(Role::Surface), Fill::from),
        )
        .named("Instrument controls")
        .id(format!("uvi-stage-{owner}"))
}

// Paths refer only to already-owned item strings. Keep the original leaf index
// so choosing a category cannot change the authored callback's value domain.
struct MenuEntry<'a> {
    index: usize,
    label: &'a str,
    branch: bool,
}

fn menu_entries<'a>(
    widget: &'a UiWidget,
    path: &[String],
    hierarchical: bool,
) -> Vec<MenuEntry<'a>> {
    let mut entries: Vec<MenuEntry<'a>> = Vec::new();
    let mut groups = HashSet::new();
    for (index, label) in widget.items.iter().enumerate() {
        if !hierarchical {
            entries.push(MenuEntry {
                index,
                label,
                branch: false,
            });
            continue;
        }
        let mut parts = label.split('/');
        if !path.iter().all(|part| parts.next() == Some(part.as_str())) {
            continue;
        }
        let Some(label) = parts.next() else { continue };
        let branch = parts.next().is_some();
        if !branch || groups.insert(label) {
            entries.push(MenuEntry {
                index,
                label,
                branch,
            });
        }
    }
    entries
}

fn menu_hierarchical(widget: &UiWidget) -> bool {
    widget.style.hierarchical == Some(true)
        && widget.items.iter().all(|item| {
            let parts = item.split('/');
            parts.clone().count() <= 32 && parts.clone().all(|part| !part.is_empty())
        })
}

fn menu_item_id(id: &str, depth: usize, entry: &MenuEntry<'_>) -> String {
    if entry.branch {
        format!("{id}-group-{depth}-{}", entry.index + 1)
    } else {
        format!("{id}-{}", entry.index + 1)
    }
}

/// Open menus float at editor-root level outside the authored stage's clipping.
pub fn popup(
    ui: &mut Ui,
    state: &mut State,
    owner: usize,
    current: Stamp,
    captured: Stamp,
    snapshot: &UiSnapshot,
    window: Size,
    mut send: impl FnMut(Stamp, UiEdit) -> bool,
) -> Option<El> {
    if state.read_only || !state.adopt(current, captured, snapshot.processor) {
        return None;
    }
    let widget = snapshot.widgets.iter().find(|w| Some(w.id) == state.menu)?;
    if !widget.effective_visible || !enabled(snapshot, widget) {
        state.menu = None;
        state.menu_path.clear();
        return None;
    }
    let anchor = identity(owner, current, snapshot, widget.id);
    let id = format!("{anchor}-menu");
    // Unusual paths stay selectable as flat entries instead of losing content.
    // The depth cap also bounds the number of simultaneously open panels.
    let hierarchical = menu_hierarchical(widget);
    if !hierarchical {
        state.menu_path.clear();
    }
    while !state.menu_path.is_empty()
        && menu_entries(widget, &state.menu_path, hierarchical).is_empty()
    {
        state.menu_path.pop();
    }
    let panels: Vec<_> = (0..=state.menu_path.len())
        .map(|depth| {
            if depth == 0 {
                id.clone()
            } else {
                format!("{id}-submenu-{depth}")
            }
        })
        .collect();
    let mut dismiss: Vec<_> = panels.iter().map(String::as_str).collect();
    dismiss.push(&anchor);
    if ui.dismissed(&dismiss) {
        state.menu = None;
        state.menu_path.clear();
        return None;
    }
    let bounds = ui.scene()?.surface(&anchor)?.frame;
    let width = bounds
        .size
        .width
        .max(180.)
        .min((window.width - 2. * TIGHT).max(1.));
    let mut layers = vec![block(window.width, window.height).id(format!("{id}-dismiss"))];
    let mut at = Point::new(bounds.x, bounds.bottom());
    let mut depth = 0;
    let mut focus_next = None;
    loop {
        let entries = menu_entries(widget, &state.menu_path[..depth], hierarchical);
        if entries.is_empty() {
            break;
        }
        let panel_id = if depth == 0 {
            id.clone()
        } else {
            format!("{id}-submenu-{depth}")
        };
        if depth == 0 && ui.focus_key() == Some(anchor.as_str()) {
            focus_next = Some(menu_item_id(&id, depth, &entries[0]));
        }
        let mut rows = Vec::with_capacity(entries.len());
        let mut opened = None;
        for entry in &entries {
            let item = menu_item_id(&id, depth, entry);
            let keys = ui.keys(item.as_str());
            if depth > 0 && keys.iter().any(|press| press.key == Key::Left) {
                let parent = menu_entries(widget, &state.menu_path[..depth - 1], hierarchical)
                    .into_iter()
                    .find(|entry| entry.branch && entry.label == state.menu_path[depth - 1]);
                let parent = parent.map(|entry| menu_item_id(&id, depth - 1, &entry));
                state.menu_path.truncate(depth - 1);
                let panel = popup(ui, state, owner, current, captured, snapshot, window, send);
                if let Some(parent) = parent {
                    ui.focus(parent);
                }
                return panel;
            }
            let right = keys.iter().any(|press| press.key == Key::Right);
            let selected = if entry.branch {
                state
                    .menu_path
                    .get(depth)
                    .is_some_and(|part| part == entry.label)
            } else {
                numeric(state, widget) == (entry.index + 1) as f64
            };
            let (hit, mut el) = action(ui, item.as_str(), entry.label, selected);
            if entry.branch {
                el = el.justify(Justify::Start);
                if let Some(label) = el.children_mut().first_mut() {
                    *label = label.clone().flex(1);
                }
                el = el.push(glyph(Icon::Right, TEXT, secondary()));
                if hit || right {
                    opened = Some(entry.label.to_owned());
                }
            } else if hit {
                send_edit(
                    state,
                    current,
                    snapshot,
                    widget,
                    UiEditValue::Number((entry.index + 1) as f64),
                    ui.keys(item.as_str())
                        .last()
                        .map_or(ui.get(item.as_str()).mods, |key| key.mods),
                    &mut send,
                );
                state.menu = None;
                state.menu_path.clear();
                return None;
            }
            rows.push(el.h(CONTROL).w(Len::Pct(100.)).shrink(0));
        }
        if let Some(label) = opened {
            state.menu_path.truncate(depth);
            state.menu_path.push(label);
            if let Some(first) = menu_entries(widget, &state.menu_path, hierarchical).first() {
                focus_next = Some(menu_item_id(&id, depth + 1, first));
            }
        }
        let height = (rows.len() as f64 * CONTROL).min((window.height - 2. * TIGHT).max(CONTROL));
        let x = at.x.min(window.width - width - TIGHT).max(TIGHT);
        let y = if depth == 0 && at.y + height > window.height {
            (bounds.y - height).max(TIGHT)
        } else {
            at.y.min(window.height - height - TIGHT).max(TIGHT)
        };
        layers.push(
            col(rows)
                .gap(0)
                .w(width)
                .max_size(Size::new(width, height))
                .scroll()
                .fill(Role::Raised)
                .at(x, y)
                .id(panel_id.clone()),
        );
        if depth >= state.menu_path.len() {
            break;
        }
        let Some(branch) = entries
            .iter()
            .find(|entry| entry.branch && entry.label == state.menu_path[depth])
        else {
            state.menu_path.truncate(depth);
            break;
        };
        // Anchor to this frame's row grid. Reusing the prior row's absolute
        // position introduces a one-frame jump when the popup is resized.
        let scroll = ui.scene().and_then(|scene| {
            let panel = scene.surface(&panel_id)?;
            let first = scene.surface(&menu_item_id(&id, depth, &entries[0]))?;
            Some((first.frame.y - panel.frame.y).min(0.))
        }).unwrap_or(0.);
        let row = entries.iter().position(|entry| entry.index == branch.index && entry.branch).unwrap();
        at = Point::new(
            if x + 2. * width + TIGHT <= window.width {
                x + width
            } else {
                (x - width).max(TIGHT)
            },
            y + row as f64 * CONTROL + scroll,
        );
        depth += 1;
    }
    if let Some(id) = focus_next {
        ui.focus(id);
    }
    Some(stack(layers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uvi::host::{UiBounds, UiRoot, UiStyle};

    pub(super) fn authored() -> UiSnapshot {
        let spec = [
            (
                UiKind::Label,
                "Authored instrument controls",
                16.,
                10.,
                600.,
                28.,
            ),
            (UiKind::Knob, "Tone", 20., 52., 88., 100.),
            (UiKind::Slider, "Level", 132., 58., 230., 70.),
            (UiKind::NumBox, "Voices", 388., 58., 100., 28.),
            (UiKind::Menu, "Mode", 388., 108., 210., 28.),
            (UiKind::OnOffButton, "Enabled", 20., 170., 100., 28.),
            (UiKind::Button, "Reset", 132., 170., 100., 28.),
            (UiKind::Label, "Steps", 20., 217., 90., 20.),
            (UiKind::Table, "Steps", 132., 211., 330., 56.),
            (UiKind::Panel, "Hidden panel", 0., 0., 20., 20.),
            (UiKind::Label, "Hidden child", 0., 0., 20., 20.),
            (UiKind::Panel, "Disabled panel", 505., 155., 95., 60.),
            (UiKind::NumBox, "Locked", 505., 174., 95., 28.),
        ];
        let widgets = spec
            .into_iter()
            .enumerate()
            .map(|(index, (kind, name, x, y, width, height))| {
                let id = index as u32 + 1;
                let value = match kind {
                    UiKind::OnOffButton => Some(UiValue::Boolean(true)),
                    UiKind::Table => {
                        Some(UiValue::Table(vec![0.1, 0.3, 0.5, 0.8, 0.6, 0.4, 0.2, 0.7]))
                    }
                    UiKind::Menu => Some(UiValue::Number(1.)),
                    UiKind::NumBox => Some(UiValue::Number(4.)),
                    UiKind::Knob | UiKind::Slider => Some(UiValue::Number(0.375)),
                    _ => None,
                };
                UiWidget {
                    id,
                    parent: match id {
                        11 => Some(10),
                        13 => Some(12),
                        _ => None,
                    },
                    kind,
                    name: name.into(),
                    display_name: None,
                    bounds: UiBounds {
                        x,
                        y,
                        width,
                        height,
                    },
                    absolute_bounds: UiBounds {
                        x,
                        y,
                        width,
                        height,
                    },
                    visible: id != 10,
                    effective_visible: !matches!(id, 10 | 11),
                    enabled: id != 12,
                    alpha: 1.,
                    effective_alpha: 1.,
                    value,
                    min: Some(if kind == UiKind::NumBox || kind == UiKind::Menu {
                        1.
                    } else {
                        0.
                    }),
                    max: Some(if kind == UiKind::NumBox {
                        16.
                    } else if kind == UiKind::Menu {
                        3.
                    } else {
                        1.
                    }),
                    integer: matches!(kind, UiKind::NumBox | UiKind::Menu),
                    items: if kind == UiKind::Menu {
                        vec!["Warm".into(), "Bright".into(), "Soft".into()]
                    } else {
                        Vec::new()
                    },
                    param_x: None,
                    param_y: None,
                    has_changed_callback: true,
                    style: UiStyle {
                        font_size: Some(13.),
                        ..UiStyle::default()
                    },
                }
            })
            .collect();
        UiSnapshot {
            processor: 7,
            root: UiRoot {
                width: 640.,
                height: 288.,
                performance_view: true,
                background: None,
                background_colour: Some("#252525".into()),
            },
            widgets,
        }
    }

    fn stamp(generation: u64) -> Stamp {
        Stamp {
            epoch: 3,
            generation,
            frame: 0,
        }
    }
    fn tick(
        ui: &mut Ui,
        state: &mut State,
        snapshot: &UiSnapshot,
        current: Stamp,
        captured: Stamp,
        input: Input,
        edits: &mut Vec<(Stamp, UiEdit)>,
    ) {
        let panel = view(
            ui,
            state,
            0,
            current,
            captured,
            snapshot,
            &HashMap::new(),
            &HashMap::new(),
            |s, e| {
                edits.push((s, e));
                true
            },
        );
        let overlay = popup(
            ui,
            state,
            0,
            current,
            captured,
            snapshot,
            Size::new(680., 340.),
            |s, e| {
                edits.push((s, e));
                true
            },
        );
        let mut layers = vec![panel.at(20., 20.)];
        layers.extend(overlay);
        ui.frame(
            stack(layers).size(680., 340.).fill(Role::Surface),
            Some(Size::new(680., 340.)),
            input,
            1. / 60.,
        )
        .unwrap();
    }

    #[test]
    fn initialized_panel_is_visible_but_cannot_edit_until_audio_adoption() {
        let snapshot = authored();
        let mut ui = super::super::theme::ui();
        let mut state = State::default();
        let mut edits = Vec::new();
        let current = stamp(3);
        let captured = stamp(4);
        for _ in 0..3 {
            tick(&mut ui, &mut state, &snapshot, current, captured, Input::default(), &mut edits);
        }
        let id = identity(0, current, &snapshot, 6);
        assert!(ui.scene().unwrap().surface("uvi-stage-0").is_some());
        assert!(ui.scene().unwrap().surface(&id).is_some());
        ui.focus(id);
        tick(&mut ui, &mut state, &snapshot, current, captured,
            Input { keys: vec![KeyPress { key: Key::Space, mods: Mods::default() }], ..Input::default() },
            &mut edits);
        assert!(edits.is_empty());
        assert!(state.pending.is_none() && state.menu.is_none());
        let mut stale = captured;
        stale.epoch -= 1;
        tick(&mut ui, &mut state, &snapshot, current, stale, Input::default(), &mut edits);
        assert!(ui.scene().unwrap().surface("uvi-stage-0").is_none());
        tick(&mut ui, &mut state, &snapshot, captured, captured, Input::default(), &mut edits);
        assert!(ui.scene().unwrap().surface("uvi-stage-0").is_some());
    }

    #[test]
    fn retained_failed_panel_is_visible_and_all_gestures_are_inert() {
        let snapshot = authored();
        let mut ui = super::super::theme::ui();
        let mut state = State::default();
        let mut edits = Vec::new();
        let current = stamp(4);
        for _ in 0..3 {
            tick(&mut ui,&mut state,&snapshot,current,current,Input::default(),&mut edits);
        }
        state.menu = Some(5);
        state.pending = Some((2,UiEditValue::Number(0.8),0));
        state.table_cell = Some((9,2));
        state.set_interactive(false);
        for (widget,key) in [(2,Key::Up),(3,Key::Right),(4,Key::Up),(5,Key::Enter),
                             (6,Key::Space),(7,Key::Enter),(9,Key::Up)] {
            let id=identity(0,current,&snapshot,widget);
            ui.focus(id.clone());
            tick(&mut ui,&mut state,&snapshot,current,current,
                Input{keys:vec![KeyPress{key,mods:Mods::default()}],..Input::default()},&mut edits);
            tick(&mut ui,&mut state,&snapshot,current,current,Input::default(),&mut edits);
            assert!(ui.scene().unwrap().surface(&id).is_some());
        }
        assert!(ui.scene().unwrap().surface("uvi-stage-0").is_some());
        assert!(state.pending.is_none() && state.menu.is_none() && state.table_cell.is_none());
        assert!(edits.is_empty(),"owned retained controls cannot submit even when send accepts");
        let mut reset=current; reset.epoch+=1;
        tick(&mut ui,&mut state,&snapshot,reset,current,Input::default(),&mut edits);
        assert!(ui.scene().unwrap().surface("uvi-stage-0").is_none());
        state.set_interactive(true);
        tick(&mut ui,&mut state,&snapshot,reset,reset,Input::default(),&mut edits);
        assert!(ui.scene().unwrap().surface("uvi-stage-0").is_some());
    }

    #[test]
    fn authored_tooltips_hover_without_changing_control_hits() {
        let mut snapshot = authored();
        for id in [2, 4, 5, 6, 7, 9] {
            snapshot.widgets[id - 1].style.tooltip = Some(format!("Authored help {id}"));
        }
        snapshot.widgets[0].style.tooltip = Some("Decorative help".into());
        snapshot.widgets[2].style.tooltip = Some(snapshot.widgets[2].name.clone());
        let current = stamp(4);
        let mut ui = super::super::theme::ui();
        let mut state = State::default();
        let mut edits = Vec::new();
        for _ in 0..3 {
            tick(&mut ui, &mut state, &snapshot, current, current, Input::default(), &mut edits);
        }
        for widget in [2, 4, 5, 6, 7, 9] {
            let tip = ui.scene().unwrap().surface(&identity(0, current, &snapshot, widget))
                .unwrap().tip.as_deref().unwrap();
            assert!(tip.starts_with(&format!("Authored help {widget}")));
        }
        let numeric = ui.scene().unwrap().surface(&identity(0, current, &snapshot, 4)).unwrap();
        assert_eq!(numeric.tip.as_deref(), Some("Authored help 4 — Enter a number from 1 to 16."));
        let slider = ui.scene().unwrap().surface(&identity(0, current, &snapshot, 3)).unwrap();
        assert_eq!(slider.tip.as_deref(), Some("Level: 0–1"), "the default name does not duplicate range help");
        assert!(!ui.scene().unwrap().surfaces().any(|surface|
            surface.tip.as_deref() == Some("Decorative help")), "decorations gain no help hit targets");
        let toggle = identity(0, current, &snapshot, 6);
        let frame = ui.scene().unwrap().surface(&toggle).unwrap().frame;
        let pos = Point::new(frame.x + frame.size.width * 0.5, frame.y + frame.size.height * 0.5);
        for _ in 0..60 {
            tick(&mut ui, &mut state, &snapshot, current, current,
                PointerInput { pos: Some(pos), ..PointerInput::default() }.into(), &mut edits);
        }
        assert!(ui.scene().unwrap().surface("/tip").is_some(), "the genuine hover shows help");
        for buttons in [Buttons::PRIMARY, Buttons::default()] {
            tick(&mut ui, &mut state, &snapshot, current, current,
                PointerInput { pos: Some(pos), buttons, ..PointerInput::default() }.into(), &mut edits);
        }
        tick(&mut ui, &mut state, &snapshot, current, current, Input::default(), &mut edits);
        assert!(edits.iter().any(|(stamp, input)| same_activation(*stamp, current)
            && input.widget == 6 && input.value == UiEditValue::Boolean(false)),
            "authored help does not intercept the existing input surface");
    }

    #[test]
    fn authored_panel_scales_up_and_down_without_changing_coordinates() {
        let mut snapshot = authored();
        let menu = &mut snapshot.widgets[4];
        menu.style.show_label = Some(false);
        menu.style.font = Some("bank-face.ttf".into());
        menu.items = vec!["CC11".into()];
        menu.style.background_image = Some(crate::uvi::host::UiArtwork {
            path: "menu.png".into(),
            bank_root: false,
        });
        menu.absolute_bounds.width = 60.;
        menu.absolute_bounds.height = 18.;
        menu.bounds = menu.absolute_bounds;
        let mut pictures = HashMap::new();
        pictures.insert(
            "menu.png".into(),
            Arc::new(Picture {
                frames: vec![Arc::new(
                    moose::mui::mui::scene::Image::rgba(20, 10, vec![255; 800]).unwrap(),
                )],
                stretch: [false; 2],
                atlas: None,
            }),
        );
        let font = Font::new(NOTO_SANS).unwrap();
        let fonts = HashMap::from([("bank-face.ttf".into(), font.clone())]);
        let current = stamp(4);
        let mut ui = super::super::theme::ui();
        let mut state = State::default();
        for room in [320., 960., 480.] {
            let scale = room / snapshot.root.width;
            for _ in 0..3 {
                let panel = view(
                    &mut ui,
                    &mut state,
                    0,
                    current,
                    current,
                    &snapshot,
                    &pictures,
                    &fonts,
                    |_, _| false,
                );
                ui.frame(
                    col![panel]
                        .align(Align::Start)
                        .size(room, snapshot.root.height * scale)
                        .id("part-0"),
                    Some(Size::new(room, snapshot.root.height * scale)),
                    Input::default(),
                    1. / 60.,
                )
                .unwrap();
            }
            let scene = ui.scene().unwrap();
            let stage = scene.surface("uvi-stage-0").unwrap().frame;
            assert!((stage.size.width - room).abs() < 0.01);
            assert!((stage.size.height - snapshot.root.height * scale).abs() < 0.01);
            let menu = scene
                .surface(&identity(0, current, &snapshot, 5))
                .unwrap()
                .frame;
            assert!((menu.x - 388. * scale).abs() < 0.01);
            assert!((menu.y - 108. * scale).abs() < 0.01);
            assert!((menu.size.width - 60. * scale).abs() < 0.01);
            assert!((menu.size.height - 18. * scale).abs() < 0.01);
            let selected = scene.paint.iter().filter_map(|p| p.text.as_ref())
                .find(|t| t.fonts[0].id() == font.id()).unwrap();
            assert_eq!(selected.glyphs.len(), 4, "short CC selection remains legible");
            assert!((f64::from(selected.size) - 13. * scale).abs() < 0.01);
            let knob = scene
                .surface(&identity(0, current, &snapshot, 2))
                .unwrap()
                .frame;
            let intrinsic = snapshot.widgets[1].absolute_bounds;
            assert!((knob.x - intrinsic.x * scale).abs() < 0.01);
            assert!((knob.y - intrinsic.y * scale).abs() < 0.01);
            assert!((knob.size.width - intrinsic.width * scale).abs() < 0.01);
            assert!((knob.size.height - intrinsic.height * scale).abs() < 0.01);
        }
    }

    #[test]
    fn documented_used_units_format_without_rescaling_values_or_edits() {
        for (unit, value, expected) in [
            (1., 25., "25 %"), (2., 0.25, "25 %"),
            (3., 0.25, "250 ms"), (3., 1., "1 s"),
            (5., 250., "250 ms"), (5., 1000., "1000 ms"), (5., 1250., "1.25 s"),
            (7., 1000., "1000 Hz"), (7., 1250., "1.25 kHz"),
            (9., -60., "-60 dB"), (11., 0., "-inf dB"),
            (11., 1., "0 dB"), (11., 0.5, "-6.021 dB"), (14., -3., "-3 st"),
        ] {
            assert_eq!(unit_text(value, false, Some(unit)), expected);
        }
        let mut snapshot = authored();
        let knob = &mut snapshot.widgets[1];
        knob.style.unit = Some(2.);
        assert_eq!(value_text(knob, 0.375), "37.5 %");
        knob.style.display_text = Some("Authored override".into());
        assert_eq!(value_text(knob, 0.375), "Authored override");
        knob.style.display_text = Some(String::new());
        assert_eq!(value_text(knob, 0.375), "37.5 %");
        let current = stamp(4);
        let mut ui = super::super::theme::ui();
        let mut state = State::default();
        let mut edits = Vec::new();
        tick(&mut ui, &mut state, &snapshot, current, current, Input::default(), &mut edits);
        ui.focus(identity(0, current, &snapshot, 2));
        tick(&mut ui, &mut state, &snapshot, current, current, Input {
            keys: vec![KeyPress { key: Key::Up, mods: Mods::default() }],
            ..Input::default()
        }, &mut edits);
        tick(&mut ui, &mut state, &snapshot, current, current, Input::default(), &mut edits);
        assert!(edits.iter().any(|(_, input)| matches!(input.value,
            UiEditValue::Number(value) if (value - 0.385).abs() < 1e-12)));
        assert!(matches!(snapshot.widgets[1].value, Some(UiValue::Number(0.375))));
    }

    #[test]
    fn numbox_double_click_types_raw_values_and_commits_or_cancels() {
        for display in [None, Some("Authored display override")] {
            for finish in ["unchanged", "enter", "blur", "escape", "unit_text"] {
                let mut snapshot = authored();
                let widget = &mut snapshot.widgets[3];
                widget.value = Some(UiValue::Number(0.375));
                widget.min = Some(0.);
                widget.max = Some(1.);
                widget.integer = false;
                widget.style.show_label = Some(false);
                widget.style.unit = Some(2.);
                widget.style.display_text = display.map(str::to_owned);
                widget.style.tooltip = Some("Authored numeric help".into());
                let current = stamp(4);
                let id = identity(0, current, &snapshot, 4);
                let mut ui = super::super::theme::ui();
                let mut state = State::default();
                let mut edits = Vec::new();
                for _ in 0..3 {
                    tick(&mut ui,&mut state,&snapshot,current,current,Input::default(),&mut edits);
                }
                let bounds = ui.scene().unwrap().surface(&id).unwrap().frame;
                assert_eq!(ui.scene().unwrap().surface(&id).unwrap().tip.as_deref(),
                    Some("Authored numeric help — Enter a number from 0 to 1."));
                let pos = Point::new(bounds.x+0.5*bounds.size.width,
                    bounds.y+0.5*bounds.size.height);
                let mut double_clicked = false;
                for down in [true,false,true,false] {
                    let pointer = PointerInput {pos:Some(pos),
                        buttons:if down {Buttons::PRIMARY} else {Buttons::default()},
                        ..PointerInput::default()};
                    tick(&mut ui,&mut state,&snapshot,current,current,pointer.into(),&mut edits);
                    double_clicked |= ui.get(&id).double_clicked;
                }
                tick(&mut ui,&mut state,&snapshot,current,current,Input::default(),&mut edits);
                assert!(double_clicked, "a genuine double click opened the field");
                assert!(ui.focus_is_text());
                let field = ui.focus_key().unwrap().to_owned();
                let field_value = |ui:&Ui| match &ui.scene().unwrap().surface(&field)
                    .unwrap().semantics.as_ref().unwrap().role {
                    A11y::TextInput {value,..} => value.to_string(),
                    _ => panic!("editing the actual numeric text field"),
                };
                assert_eq!(field_value(&ui), "0.375", "display override cannot seed typed input");
                assert!(edits.is_empty());
                if finish != "unchanged" {
                    let input = Input {text:if finish == "unit_text" {"37.5 %"} else {"0.625"}.into(),
                        ..Input::default()};
                    tick(&mut ui,&mut state,&snapshot,current,current,input,&mut edits);
                    tick(&mut ui,&mut state,&snapshot,current,current,Input::default(),&mut edits);
                    assert_eq!(field_value(&ui), if finish == "unit_text" {"37.5 %"} else {"0.625"});
                }
                let input = if finish == "blur" {
                    PointerInput {pos:Some(Point::new(10.,10.)),buttons:Buttons::PRIMARY,
                        ..PointerInput::default()}.into()
                } else {
                    Input {keys:vec![KeyPress {key:if finish == "escape" {Key::Escape} else {Key::Enter},
                        mods:Mods::default()}],..Input::default()}
                };
                tick(&mut ui,&mut state,&snapshot,current,current,input,&mut edits);
                for _ in 0..2 {
                    tick(&mut ui,&mut state,&snapshot,current,current,Input::default(),&mut edits);
                }
                assert!(!ui.focus_is_text());
                assert!(ui.scene().unwrap().surface(&field).is_none());
                if matches!(finish, "enter" | "blur") {
                    assert_eq!(edits.len(),1);
                    assert!(matches!(edits[0].1.value,UiEditValue::Number(v) if v==0.625));
                } else {
                    assert!(edits.is_empty(), "unchanged, cancelled, or unit-suffixed text cannot edit");
                }
                assert!(matches!(snapshot.widgets[3].value,Some(UiValue::Number(0.375))));
            }
        }
    }

    #[test]
    fn authored_numbox_readout_honors_skin_ink_size_and_alignment_at_each_scale() {
        let mut snapshot = authored();
        snapshot.root.width = 720.;
        snapshot.root.height = 480.;
        let widget = &mut snapshot.widgets[3];
        widget.absolute_bounds = UiBounds {x:325.,y:315.,width:38.,height:12.};
        widget.bounds = widget.absolute_bounds;
        widget.value = Some(UiValue::Number(200.));
        widget.min = Some(0.);
        widget.max = Some(10000.);
        widget.integer = false;
        widget.style = UiStyle {
            font:Some("authored-face.ttf".into()),font_size:Some(10.),
            align:Some("centred".into()),text_colour:Some("#c8c9ca".into()),
            background_colour:Some("#00000000".into()),show_label:Some(false),unit:Some(7.),
            background_image:Some(crate::uvi::host::UiArtwork {
                path:"authored-box.png".into(),bank_root:false,
            }),..UiStyle::default()
        };
        let image = Arc::new(moose::mui::mui::scene::Image::rgba(38,12,vec![255;38*12*4]).unwrap());
        let font = Font::new(NOTO_SANS).unwrap();
        let fonts = HashMap::from([("authored-face.ttf".into(),font.clone())]);
        let pictures = HashMap::from([("authored-box.png".into(),Arc::new(Picture {
            frames:vec![image.clone()],stretch:[false;2],atlas:None,
        }))]);
        let current = stamp(4);
        for scale in [0.5,1.,1.5] {
            let mut ui = super::super::theme::ui();
            let mut state = State::default();
            for _ in 0..3 {
                let panel = view(&mut ui,&mut state,0,current,current,&snapshot,&pictures,&fonts,
                    |_,_|panic!("readout cannot submit edits"));
                ui.frame(col![panel].size(720.*scale,480.*scale).id("part-0"),
                    Some(Size::new(720.*scale,480.*scale)),Input::default(),1./60.).unwrap();
            }
            let scene = ui.scene().unwrap();
            let field = scene.surface(&identity(0,current,&snapshot,4)).unwrap().frame;
            assert!((field.x-325.*scale).abs()<0.01 && (field.y-315.*scale).abs()<0.01);
            assert!((field.size.width-38.*scale).abs()<0.01 && (field.size.height-12.*scale).abs()<0.01);
            let painted = scene.paint.iter().find(|p|p.text.as_ref()
                .is_some_and(|t|t.fonts[0].id()==font.id())).unwrap();
            let text = painted.text.as_ref().unwrap();
            assert_eq!(text.glyphs.len(),6,"200 Hz stays on one line with all glyphs");
            assert!((f64::from(text.size)-10.*scale).abs()<0.01);
            assert_eq!(painted.paint,moose::mui::mui::scene::Paint::Solid(colour(Some("#c8c9ca")).unwrap()));
            let ink = scene.surface(painted.key.as_str()).unwrap().frame;
            assert!((ink.x+0.5*ink.size.width-field.x-0.5*field.size.width).abs()<0.01);
            assert!((ink.y+0.5*ink.size.height-field.y-0.5*field.size.height).abs()<0.01);
            assert!((ink.size.height-field.size.height).abs()<0.01);
            assert!(scene.paint.iter().any(|p|matches!(&p.paint,
                moose::mui::mui::scene::Paint::Image {image:owned,..} if owned==&image)));
        }
    }

    #[test]
    fn authored_font_is_owned_and_missing_face_uses_scene_fallback() {
        let mut widget = authored().widgets[0].clone();
        widget.style.font = Some("bank-face.ttf".into());
        let font = Font::new(NOTO_SANS).unwrap();
        for available in [false, true] {
            let mut fonts = HashMap::new();
            if available { fonts.insert("bank-face.ttf".into(), font.clone()); }
            let mut el = styled_text(&widget, "Authored face", 1., Justify::Start).size(240., 28.);
            widget_font(&mut el, &widget, &fonts);
            // The element owns cloned handles; the cache owner can go away.
            drop(fonts);
            let mut ui = super::super::theme::ui();
            ui.frame(el, Some(Size::new(240., 28.)), Input::default(), 1. / 60.).unwrap();
            let drawn = ui.scene().unwrap().paint.iter().find_map(|p| p.text.as_ref()).unwrap();
            assert_eq!(drawn.fonts[0].id() == font.id(), available);
        }
    }

    #[test]
    fn authored_text_size_ink_and_nine_anchors_scale_together() {
        let mut widget = authored().widgets[0].clone();
        widget.style.font_size = Some(13.);
        widget.style.text_colour = Some("#c8c9ca".into());
        for scale in [0.5, 1., 1.5] {
            let mut centers = Vec::new();
            for anchor in ["centred", "left", "right", "top", "bottom",
                "topLeft", "topRight", "bottomLeft", "bottomRight"] {
                widget.style.align = Some(anchor.into());
                let mut ui = super::super::theme::ui();
                let mut box_el = styled_text(&widget, "Authored", scale, Justify::Start)
                    .size(200. * scale, 80. * scale).id("authored-box");
                box_el.children_mut()[0] = box_el.children_mut()[0].clone().id("authored-ink");
                ui.frame(box_el, Some(Size::new(200. * scale, 80. * scale)),
                    Input::default(), 1. / 60.).unwrap();
                let scene = ui.scene().unwrap();
                let frame = scene.surface("authored-ink").unwrap().frame;
                centers.push((anchor, frame.x + frame.size.width / 2.,
                    frame.y + frame.size.height / 2.));
                let ink = scene.paint.iter().find(|p| p.key.as_str() == "authored-ink"
                    && p.text.is_some()).unwrap();
                assert!((f64::from(ink.text.as_ref().unwrap().size) - 13. * scale).abs() < 0.01);
                assert_eq!(ink.paint, moose::mui::mui::scene::Paint::Solid(colour(Some("#c8c9ca")).unwrap()));
            }
            let (_, cx, cy) = centers[0];
            assert!((cx - 100. * scale).abs() < 0.01);
            assert!((cy - 40. * scale).abs() < 0.01);
            for (anchor, x, y) in centers.into_iter().skip(1) {
                if anchor.ends_with("Left") || anchor == "left" { assert!(x < cx); }
                if anchor.ends_with("Right") || anchor == "right" { assert!(x > cx); }
                if anchor.starts_with("top") { assert!(y < cy); }
                if anchor.starts_with("bottom") { assert!(y > cy); }
            }
        }
    }

    #[test]
    fn native_button_labels_require_explicit_show_label() {
        let mut widget = authored().widgets[5].clone();
        for kind in [UiKind::Button, UiKind::OnOffButton] {
            widget.kind = kind;
            widget.display_name = Some("Authored label".into());
            widget.style.text = Some("Authored text".into());
            widget.style.show_label = None;
            assert_eq!(button_text(&widget), "");
            widget.style.show_label = Some(false);
            assert_eq!(button_text(&widget), "");
            widget.style.show_label = Some(true);
            assert_eq!(button_text(&widget), "Authored text");
            widget.style.text = None;
            assert_eq!(button_text(&widget), "Authored label");
            assert_eq!(name(&widget), "Authored label");
        }
    }

    #[test]
    fn authored_widget_edits_are_scoped_and_generation_checked() {
        let snapshot = authored();
        let mut ui = super::super::theme::ui();
        let mut state = State::default();
        let mut edits = Vec::new();
        let current = stamp(4);
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            current,
            current,
            Input::default(),
            &mut edits,
        );
        let id = |widget| identity(0, current, &snapshot, widget);
        assert!(ui.scene().unwrap().surface(&id(11)).is_none());
        assert!(!enabled(&snapshot, &snapshot.widgets[12]));
        ui.focus(id(6));
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            current,
            current,
            Input {
                keys: vec![KeyPress {
                    key: Key::Enter,
                    mods: Mods {
                        shift: true,
                        ..Mods::default()
                    },
                }],
                ..Input::default()
            },
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            current,
            current,
            Input::default(),
            &mut edits,
        );
        assert!(edits.iter().any(|(s, e)| same_activation(*s, current)
            && e.processor == 7
            && e.widget == 6
            && e.value == UiEditValue::Boolean(false)
            && e.modifiers.shift_down));
        let before = edits.len();
        ui.focus(id(13));
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            current,
            current,
            Input {
                keys: vec![KeyPress {
                    key: Key::Up,
                    mods: Mods::default(),
                }],
                ..Input::default()
            },
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            current,
            current,
            Input::default(),
            &mut edits,
        );
        assert_eq!(edits.len(), before);
        let next = stamp(5);
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            next,
            current,
            Input::default(),
            &mut edits,
        );
        assert!(state.pending.is_none() && state.menu.is_none());
        assert!(ui.scene().unwrap().surface(&id(6)).is_none());
        assert_eq!(edits.len(), before);
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            next,
            next,
            Input::default(),
            &mut edits,
        );
        let fresh = identity(0, next, &snapshot, 6);
        assert!(ui.scene().unwrap().surface(&fresh).is_some());
    }

    #[test]
    fn hierarchical_menu_pointer_and_keyboard_keep_original_leaf_indices() {
        fn click(
            ui: &mut Ui,
            state: &mut State,
            snapshot: &UiSnapshot,
            stamp: Stamp,
            key: &str,
            edits: &mut Vec<(Stamp, UiEdit)>,
        ) {
            let frame = ui.scene().unwrap().surface(key).unwrap().frame;
            let pos = Point::new(
                frame.x + frame.size.width * 0.5,
                frame.y + frame.size.height * 0.5,
            );
            for buttons in [Buttons::PRIMARY, Buttons::default()] {
                tick(
                    ui,
                    state,
                    snapshot,
                    stamp,
                    stamp,
                    PointerInput {
                        pos: Some(pos),
                        buttons,
                        ..PointerInput::default()
                    }
                    .into(),
                    edits,
                );
            }
            for _ in 0..2 {
                tick(ui, state, snapshot, stamp, stamp, Input::default(), edits);
            }
        }
        let mut snapshot = authored();
        let widget = &mut snapshot.widgets[4];
        widget.items = [
            "Root",
            "Woodwinds/Soft",
            "Woodwinds/Bright",
            "Brass/Muted",
            "Woodwinds/Low/Bass",
        ]
        .map(str::to_owned)
        .into();
        widget.max = Some(5.);
        widget.style.hierarchical = Some(true);
        let stamp = stamp(4);
        let anchor = identity(0, stamp, &snapshot, 5);
        let menu = format!("{anchor}-menu");
        let group = format!("{menu}-group-0-2");
        let mut ui = super::super::theme::ui();
        let mut state = State::default();
        let mut edits = Vec::new();
        for _ in 0..3 {
            tick(
                &mut ui,
                &mut state,
                &snapshot,
                stamp,
                stamp,
                Input::default(),
                &mut edits,
            );
        }
        click(&mut ui, &mut state, &snapshot, stamp, &anchor, &mut edits);
        assert!(ui.scene().unwrap().surface(&group).is_some());
        assert!(ui.scene().unwrap().surface(&format!("{menu}-3")).is_none());
        click(&mut ui, &mut state, &snapshot, stamp, &group, &mut edits);
        assert!(
            edits.is_empty(),
            "opening a category cannot invoke the preset callback"
        );
        assert!(
            ui.scene()
                .unwrap()
                .surface(&format!("{menu}-submenu-1"))
                .is_some()
        );
        click(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            &format!("{menu}-3"),
            &mut edits,
        );
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].1.widget, 5);
        assert!(edits[0].1.value == UiEditValue::Number(3.));
        assert!(state.menu.is_none() && state.menu_path.is_empty());
        edits.clear();
        click(&mut ui, &mut state, &snapshot, stamp, &anchor, &mut edits);
        click(&mut ui, &mut state, &snapshot, stamp, &group, &mut edits);
        let nested = format!("{menu}-group-1-5");
        click(&mut ui, &mut state, &snapshot, stamp, &nested, &mut edits);
        assert!(
            ui.scene()
                .unwrap()
                .surface(&format!("{menu}-submenu-2"))
                .is_some()
        );
        let key = |key| Input {
            keys: vec![KeyPress {
                key,
                mods: Mods::default(),
            }],
            ..Input::default()
        };
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            key(Key::Left),
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            Input::default(),
            &mut edits,
        );
        assert_eq!(
            state.menu_path,
            ["Woodwinds"],
            "Left closes exactly one nested category"
        );
        assert_eq!(ui.focus_key(), Some(nested.as_str()));
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            key(Key::Right),
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            Input::default(),
            &mut edits,
        );
        click(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            &format!("{menu}-5"),
            &mut edits,
        );
        assert_eq!(edits.len(), 1);
        assert!(edits[0].1.value == UiEditValue::Number(5.));
        edits.clear();
        click(&mut ui, &mut state, &snapshot, stamp, &anchor, &mut edits);
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            key(Key::Tab),
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            Input::default(),
            &mut edits,
        );
        assert_eq!(ui.focus_key(), Some(group.as_str()));
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            key(Key::Right),
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            Input::default(),
            &mut edits,
        );
        assert_eq!(state.menu_path, ["Woodwinds"]);
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            key(Key::Left),
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            Input::default(),
            &mut edits,
        );
        assert!(state.menu_path.is_empty());
        assert_eq!(ui.focus_key(), Some(group.as_str()));
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            key(Key::Enter),
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            Input::default(),
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            key(Key::Enter),
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            stamp,
            stamp,
            Input::default(),
            &mut edits,
        );
        assert_eq!(edits.len(), 1);
        assert!(edits[0].1.value == UiEditValue::Number(2.));
        assert!(same_activation(edits[0].0, stamp));
        assert!(matches!(
            snapshot.widgets[4].value,
            Some(UiValue::Number(1.))
        ));
        edits.clear();
        click(&mut ui, &mut state, &snapshot, stamp, &anchor, &mut edits);
        click(&mut ui, &mut state, &snapshot, stamp, &group, &mut edits);
        let mut reload = stamp;
        reload.epoch += 1;
        tick(&mut ui, &mut state, &snapshot, reload, stamp, key(Key::Enter), &mut edits);
        tick(&mut ui, &mut state, &snapshot, reload, stamp, Input::default(), &mut edits);
        assert!(edits.is_empty(), "a stale popup cannot send its prior library's index");
        assert!(state.menu.is_none() && state.menu_path.is_empty());
        assert!(ui.scene().unwrap().surface(&menu).is_none());
        tick(&mut ui, &mut state, &snapshot, reload, reload, Input::default(), &mut edits);
        assert!(ui.scene().unwrap().surface(&identity(0,reload,&snapshot,5)).is_some());
        assert!(ui.scene().unwrap().surface(&group).is_none());
        assert!(edits.is_empty());

    }

    #[test]
    fn menu_path_grouping_is_opt_in_and_unusual_paths_remain_selectable() {
        let mut snapshot = authored();
        let widget = &mut snapshot.widgets[4];
        widget.items = ["Plain", "A/First", "A/Second", "A/B/Third"]
            .map(str::to_owned)
            .into();
        let flat = menu_entries(widget, &[], false);
        assert_eq!(flat.len(), 4);
        assert!(!flat.iter().any(|entry| entry.branch));
        let root = menu_entries(widget, &[], true);
        assert_eq!(root.len(), 2);
        assert_eq!(root[1].index, 1);
        let children = menu_entries(widget, &["A".into()], true);
        assert_eq!(
            children.iter().map(|entry| entry.index).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(children[2].branch);
        let nested = menu_entries(widget, &["A".into(), "B".into()], true);
        assert_eq!(nested[0].index, 3);
        assert_eq!(nested[0].label, "Third");
        widget.style.hierarchical = Some(true);
        assert!(menu_hierarchical(widget));
        widget.items.push("Malformed//Path".into());
        assert!(!menu_hierarchical(widget));
        assert_eq!(menu_entries(widget, &[], false).len(), 5);
        widget.items.pop();
        widget.items.push(vec!["deep"; 33].join("/"));
        assert!(!menu_hierarchical(widget));
        assert_eq!(menu_entries(widget, &[], false).len(), 5);
    }
    #[test]
    fn authored_menu_and_table_use_native_one_based_indices() {
        let snapshot = authored();
        let mut ui = super::super::theme::ui();
        let mut state = State::default();
        let mut edits = Vec::new();
        let current = stamp(4);
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            current,
            current,
            Input::default(),
            &mut edits,
        );
        let menu = identity(0, current, &snapshot, 5);
        ui.focus(menu.clone());
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            current,
            current,
            Input {
                keys: vec![KeyPress {
                    key: Key::Enter,
                    mods: Mods::default(),
                }],
                ..Input::default()
            },
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            current,
            current,
            Input::default(),
            &mut edits,
        );
        assert!(
            ui.scene()
                .unwrap()
                .surface(&format!("{menu}-menu"))
                .is_some()
        );
        ui.focus(format!("{menu}-menu-2"));
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            current,
            current,
            Input {
                keys: vec![KeyPress {
                    key: Key::Enter,
                    mods: Mods::default(),
                }],
                ..Input::default()
            },
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            current,
            current,
            Input::default(),
            &mut edits,
        );
        assert!(
            edits
                .iter()
                .any(|(_, e)| e.widget == 5 && e.value == UiEditValue::Number(2.))
        );
        let table = identity(0, current, &snapshot, 9);
        ui.focus(table);
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            current,
            current,
            Input {
                keys: vec![
                    KeyPress {
                        key: Key::Right,
                        mods: Mods::default(),
                    },
                    KeyPress {
                        key: Key::Up,
                        mods: Mods::default(),
                    },
                ],
                ..Input::default()
            },
            &mut edits,
        );
        tick(
            &mut ui,
            &mut state,
            &snapshot,
            current,
            current,
            Input::default(),
            &mut edits,
        );
        assert!(edits.iter().any(|(_,e)|e.widget==9&&matches!(e.value,UiEditValue::TableCell{index:2,value}if(value-0.31).abs()<1e-12)));
    }
}
