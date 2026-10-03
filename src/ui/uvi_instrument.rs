//! Native controls over a bank's authored UVI layout. Snapshots and decoded
//! artwork are adopted off the audio thread; this module never opens resources.
//! Original glyphs, units, mappers and complex displays are not emulated here.
use super::theme::*;
use crate::artwork::Picture;
use crate::uvi::{
    host::{UiEdit, UiEditValue, UiKind, UiModifiers, UiSnapshot, UiValue, UiWidget},
    ui_assets::{key, strip_key},
    worker::{Stamp, UiInput},
};
use moose::mui::mui::{prelude::*, scene::Fit};
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub struct State {
    activation: Option<(u64, u64, usize)>,
    pending: Option<(u32, UiEditValue, u64)>,
    menu: Option<u32>,
    table_cell: Option<(u32, usize)>,
}

fn same_activation(a: Stamp, b: Stamp) -> bool {
    (a.epoch, a.generation) == (b.epoch, b.generation)
}

impl State {
    fn adopt(&mut self, current: Stamp, captured: Stamp, processor: usize) -> bool {
        let activation = Some((current.epoch, current.generation, processor));
        if self.activation != activation {
            *self = Self {
                activation,
                ..Self::default()
            };
        }
        if !same_activation(current, captured) || captured.frame > current.frame {
            self.pending = None;
            self.menu = None;
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
    widget.display_name.as_deref().unwrap_or(&widget.name)
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
    send: &mut impl FnMut(Stamp, UiInput) -> bool,
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
    if send(
        stamp,
        UiInput {
            frame: stamp.frame,
            edit,
        },
    ) {
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
    mut send: impl FnMut(Stamp, UiInput) -> bool,
) -> El {
    if !state.adopt(current, captured, snapshot.processor) || snapshot.widgets.len() > 4096 {
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
    let scale = (room / width).clamp(0.1, 1.);
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
                .fill(Fill::Image(image.clone(), Fit::Fill))
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
        let usable = enabled(snapshot, widget);
        let bounds = widget.absolute_bounds;
        let (w, h) = (bounds.width * scale, bounds.height * scale);
        let response = ui.get(id.as_str());
        let modifiers = ui
            .keys(id.as_str())
            .last()
            .map_or(response.mods, |key| key.mods);
        let label = name(widget);
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
                    let field = field.el.value_text(number_text(value, widget.integer)).el();
                    let content = if widget.style.show_label != Some(false) {
                        row![
                            caption(label).text_size(SMALL * scale).lines(1),
                            field.flex(1).min_w(0)
                        ]
                        .gap(TIGHT * scale)
                        .align(Align::Center)
                    } else {
                        field
                    };
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
                let face = if let Some(image) =
                    picture(widget, pictures, value, response.held, response.hovered)
                {
                    block(w, h).fill(Fill::Image(image, Fit::Fill))
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
                let mut content = vec![
                    face.size(w, (h - captions as f64 * SMALL * scale * 1.4).max(1.))
                        .shrink(0),
                ];
                if widget.style.show_label != Some(false) {
                    content.push(caption(label).text_size(SMALL * scale).lines(1));
                }
                if widget.style.show_value != Some(false) {
                    content.push(
                        caption(number_text(value, widget.integer))
                            .text_size(SMALL * scale)
                            .lines(1),
                    );
                }
                col(content)
                    .gap(0)
                    .align(Align::Center)
                    .focusable()
                    .a11y(A11y::Slider {
                        value,
                        min: *range.start(),
                        max: *range.end(),
                    })
                    .tip(format!(
                        "{label}: {}–{}",
                        number_text(*range.start(), widget.integer),
                        number_text(*range.end(), widget.integer)
                    ))
                    .named(label.to_owned())
                    .id(id.clone())
            }
            UiKind::Button | UiKind::OnOffButton => {
                let on = match shown_value(state, widget) {
                    Some(UiEditValue::Boolean(on)) => on,
                    _ => matches!(widget.value, Some(UiValue::Boolean(true))),
                };
                let (hit, el) = if widget.kind == UiKind::Button {
                    action(
                        ui,
                        id.as_str(),
                        widget.style.text.as_deref().unwrap_or(label),
                        false,
                    )
                } else {
                    latch(
                        ui,
                        id.as_str(),
                        widget.style.text.as_deref().unwrap_or(label),
                        label,
                        on,
                    )
                };
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
                el
            }
            UiKind::Menu => {
                let selected = (value as usize)
                    .checked_sub(1)
                    .and_then(|i| widget.items.get(i))
                    .map_or("Choose…", String::as_str);
                let (hit, el) = dropdown(ui, id.as_str(), selected, label);
                if usable && hit {
                    state.menu = if state.menu == Some(widget.id) {
                        None
                    } else {
                        Some(widget.id)
                    };
                }
                if widget.style.show_label != Some(false) {
                    row![
                        caption(label).text_size(SMALL * scale).lines(1),
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
            UiKind::Label => {
                let align = match widget.style.align.as_deref() {
                    Some("centre" | "center") => Justify::Center,
                    Some("right") => Justify::End,
                    _ => Justify::Start,
                };
                row![
                    text(widget.style.text.as_deref().unwrap_or(label).to_owned())
                        .text_size(widget.style.font_size.unwrap_or(SMALL).max(1.) * scale)
                        .fill(
                            colour(widget.style.text_colour.as_deref())
                                .map_or(Fill::from(Role::Ink), Fill::from)
                        )
                        .lines((h / (SMALL * scale * 1.4)).floor().max(1.) as usize)
                        .min_w(0)
                ]
                .align(Align::Center)
                .justify(align)
                .named(label.to_owned())
            }
            UiKind::Panel | UiKind::Viewport | UiKind::Image => block(w, h),
            UiKind::XY | UiKind::WaveView | UiKind::AudioMeter => caption(label)
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
            let image = block(w, h).fill(Fill::Image(image, Fit::Fill));
            control = if matches!(
                widget.kind,
                UiKind::Panel | UiKind::Viewport | UiKind::Image
            ) {
                image
            } else if matches!(widget.kind, UiKind::Button | UiKind::OnOffButton) {
                let words = if widget.style.show_label == Some(false) {
                    String::new()
                } else {
                    widget.style.text.as_deref().unwrap_or(label).to_owned()
                };
                stack![
                    image,
                    row![caption(words).lines(1)]
                        .justify(Justify::Center)
                        .align(Align::Center)
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
                stack![image, control]
            };
        } else if let Some(fill) = colour(widget.style.background_colour.as_deref()) {
            control = control.fill(fill);
        }
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
        .clip()
        .fill(
            colour(snapshot.root.background_colour.as_deref())
                .map_or(Fill::from(Role::Surface), Fill::from),
        )
        .named("Instrument controls")
        .id(format!("uvi-stage-{owner}"))
}

/// The open native menu, placed at editor-root level outside stage clipping.
pub fn popup(
    ui: &mut Ui,
    state: &mut State,
    owner: usize,
    current: Stamp,
    captured: Stamp,
    snapshot: &UiSnapshot,
    window: Size,
    mut send: impl FnMut(Stamp, UiInput) -> bool,
) -> Option<El> {
    if !state.adopt(current, captured, snapshot.processor) {
        return None;
    }
    let widget = snapshot.widgets.iter().find(|w| Some(w.id) == state.menu)?;
    if !widget.effective_visible || !enabled(snapshot, widget) {
        state.menu = None;
        return None;
    }
    let anchor = identity(owner, current, snapshot, widget.id);
    let id = format!("{anchor}-menu");
    if ui.dismissed(&[&id, &anchor]) {
        state.menu = None;
        return None;
    }
    let mut rows = Vec::with_capacity(widget.items.len());
    for (index, label) in widget.items.iter().enumerate() {
        let item = format!("{id}-{}", index + 1);
        let (hit, el) = action(
            ui,
            item.as_str(),
            label,
            numeric(state, widget) == (index + 1) as f64,
        );
        if hit {
            send_edit(
                state,
                current,
                snapshot,
                widget,
                UiEditValue::Number((index + 1) as f64),
                ui.keys(item.as_str())
                    .last()
                    .map_or(ui.get(item.as_str()).mods, |key| key.mods),
                &mut send,
            );
            state.menu = None;
            return None;
        }
        rows.push(el.h(CONTROL).w(Len::Pct(100.)).shrink(0));
    }
    let bounds = ui.scene()?.surface(&anchor)?.frame;
    let width = bounds
        .size
        .width
        .max(180.)
        .min((window.width - 2. * TIGHT).max(1.));
    let height = (rows.len() as f64 * CONTROL).min((window.height - 2. * TIGHT).max(CONTROL));
    let x = bounds.x.min(window.width - width - TIGHT).max(TIGHT);
    let y = if bounds.bottom() + height > window.height {
        (bounds.y - height).max(TIGHT)
    } else {
        bounds.bottom()
    };
    Some(stack![
        block(window.width, window.height).id(format!("{id}-dismiss")),
        col(rows)
            .gap(0)
            .w(width)
            .max_size(Size::new(width, height))
            .scroll()
            .fill(Role::Raised)
            .at(x, y)
            .id(id)
    ])
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
        edits: &mut Vec<(Stamp, UiInput)>,
    ) {
        let panel = view(
            ui,
            state,
            0,
            current,
            captured,
            snapshot,
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
            && e.edit.processor == 7
            && e.edit.widget == 6
            && e.edit.value == UiEditValue::Boolean(false)
            && e.edit.modifiers.shift_down));
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
                .any(|(_, e)| e.edit.widget == 5 && e.edit.value == UiEditValue::Number(2.))
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
        assert!(edits.iter().any(|(_,e)|e.edit.widget==9&&matches!(e.edit.value,UiEditValue::TableCell{index:2,value}if(value-0.31).abs()<1e-12)));
    }
}
